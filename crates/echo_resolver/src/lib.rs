//! Closed compilation graph: module-scoped imports, `%` / `@` merge (ADR 0006).

#![forbid(unsafe_code)]

mod merge;
mod package_cache;
mod resolve;
mod virtual_fs;

pub use merge::{MergedMember, MergedStruct};
pub use package_cache::{
    LOCAL_VERSION, PackageSpec, VersionPick, XO_HOME_ENV, XoToml, encode_package_id, install_git,
    install_local_dir, is_host_path, list_echo_files, list_versions, match_installed_version,
    package_version_dir, packages_root, resolve_file_or_dir_module, resolve_host_import,
    select_version, split_host_import, with_xo_home_for_test, xo_home,
};
pub use resolve::{
    ModuleUnit, ResolveParseCacheStats, ResolvedGraph, SearchPaths, resolve_entry,
    resolve_entry_virtual, resolve_entry_with_cache, resolve_entry_with_overlays,
};
pub use virtual_fs::VirtualSources;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use echo_ast::Expr;
use echo_cache::{ArtifactStore, PhaseCacheKey};
use echo_diagnostics::{Diagnostic, Diagnostics, decode_diagnostics, encode_diagnostics};
use echo_fingerprint::{ArtifactPhase, Fingerprint};
use echo_index::{ExportKind, PathSeg};
use echo_semantics::{
    BindingKind, ImportedModule, ModuleExport, ReturnShape, Type, check_file_with_modules,
    effects_in_stmts, infer_export_return_types,
};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Multi-file check from an entry path.
#[derive(Debug)]
pub struct ProjectChecked {
    pub graph: ResolvedGraph,
    pub diagnostics: Diagnostics,
    /// Whether the **semantic** check phase used the artifact cache.
    pub cache: CheckCacheOutcome,
    /// Per-file parse cache stats during resolve.
    pub parse_cache: ResolveParseCacheStats,
}

/// Cache outcome for the semantics phase (resolve always runs; parse may hit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckCacheOutcome {
    /// No store provided (`check_entry` without cache).
    Bypass,
    /// Semantic diagnostics restored from `.xo` cache.
    Hit,
    /// Semantics re-ran; artifact written when store is available.
    Miss,
    /// Store I/O failed; semantics still ran.
    StoreError,
}

/// Load entry, closed graph, merge structs, bind **modules**, check each file.
///
/// Does not use the artifact cache.
#[must_use]
pub fn check_entry(entry: &Path) -> ProjectChecked {
    check_entry_with_cache(entry, None)
}

/// Like [`check_entry`], but may reuse cached **semantic** diagnostics.
///
/// Resolve + parse always run (needed for the graph and for `--graph`). The
/// check-phase cache keys on all module source bytes + component versions.
#[must_use]
pub fn check_entry_with_cache(entry: &Path, store: Option<&ArtifactStore>) -> ProjectChecked {
    check_entry_with_overlays(entry, store, &HashMap::new())
}

/// Check with editor buffer overlays (dirty open files).
#[must_use]
pub fn check_entry_with_overlays(
    entry: &Path,
    store: Option<&ArtifactStore>,
    overlays: &HashMap<PathBuf, String>,
) -> ProjectChecked {
    let (graph, mut diagnostics, parse_cache) =
        resolve_entry_with_overlays(entry, &SearchPaths::default_for(entry), store, overlays);

    // Overlays change effective graph content; check key still uses disk bytes
    // for non-overlay files and would miss overlay text — when any overlay is
    // set, skip semantic cache (always re-check).
    let use_sem_cache = overlays.is_empty();

    let key = check_phase_key(&graph);
    if use_sem_cache {
        if let Some(store) = store {
            if let Ok(Some(bytes)) = store.get(&key) {
                let semantic = decode_diagnostics(&bytes);
                diagnostics.extend(semantic);
                return ProjectChecked {
                    graph,
                    diagnostics,
                    cache: CheckCacheOutcome::Hit,
                    parse_cache,
                };
            }
        }
    }

    let mut semantic = Diagnostics::new();
    run_semantic_checks(&graph, &mut semantic);
    diagnostics.extend(semantic.clone());

    let mut cache = CheckCacheOutcome::Miss;
    if use_sem_cache {
        if let Some(store) = store {
            match store.put(&key, &encode_diagnostics(&semantic)) {
                Ok(_) => {}
                Err(_) => cache = CheckCacheOutcome::StoreError,
            }
        } else {
            cache = CheckCacheOutcome::Bypass;
        }
    } else {
        cache = CheckCacheOutcome::Bypass;
    }

    ProjectChecked {
        graph,
        diagnostics,
        cache,
        parse_cache,
    }
}

/// Check an in-memory source set (browser playground, tests without disk).
///
/// `entry` must be a key in `sources`. `search.package_roots` should match the
/// virtual layout those keys use (for example `/echo` with `/echo/std/…`).
#[must_use]
pub fn check_entry_virtual(
    entry: &Path,
    search: &SearchPaths,
    sources: &VirtualSources,
) -> ProjectChecked {
    let (graph, mut diagnostics, parse_cache) = resolve_entry_virtual(entry, search, sources);
    let mut semantic = Diagnostics::new();
    run_semantic_checks(&graph, &mut semantic);
    diagnostics.extend(semantic);
    ProjectChecked {
        graph,
        diagnostics,
        cache: CheckCacheOutcome::Bypass,
        parse_cache,
    }
}

fn run_semantic_checks(graph: &ResolvedGraph, diagnostics: &mut Diagnostics) {
    for unit in &graph.modules {
        let Some(file) = &unit.parsed.file else {
            continue;
        };

        let mut modules = Vec::new();
        let mut seen_names: HashMap<String, PathBuf> = HashMap::new();

        for (imp, target_root) in &unit.import_targets {
            let Some(mod_name) = module_bind_name(&imp.segments) else {
                diagnostics.push(
                    Diagnostic::error("import path has no module name segment")
                        .with_span(imp.span)
                        .with_code("res-import"),
                );
                continue;
            };

            if let Some(prev) = seen_names.get(&mod_name) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "import name conflict: `{mod_name}` from both {} and {}",
                        prev.display(),
                        target_root.display()
                    ))
                    .with_span(imp.span)
                    .with_code("res-import-name-conflict"),
                );
                continue;
            }
            seen_names.insert(mod_name.clone(), target_root.clone());

            // Union exports from all files that share this module root.
            let mut exports = Vec::new();
            let mut seen_export: HashSet<String> = HashSet::new();
            for target in graph
                .modules
                .iter()
                .filter(|m| m.module_root == *target_root)
            {
                for exp in &target.facts.exports {
                    let Some(kind) = exp.kind else {
                        continue;
                    };
                    if !seen_export.insert(exp.name.clone()) {
                        continue;
                    }
                    let return_shape = fn_return_shape(target, &exp.name);
                    exports.push(ModuleExport {
                        name: exp.name.clone(),
                        kind: map_export_kind(kind),
                        return_shape,
                        arity: folder_fn_arity(graph, target_root, &exp.name),
                        return_ty: folder_fn_return_ty(graph, target_root, &exp.name),
                    });
                }
            }

            modules.push(ImportedModule {
                name: mod_name,
                span: imp.span,
                exports,
            });
        }

        for d in check_file_with_modules(file, &modules).into_iter() {
            diagnostics.push(d);
        }
    }
}

/// Fingerprint all module paths + contents (stable order) for phase cache keys.
#[must_use]
pub fn graph_source_fingerprint(graph: &ResolvedGraph) -> Fingerprint {
    Fingerprint::from_bytes(&graph_source_bytes(graph))
}

/// Stable bytes for all modules in the graph (sorted paths + contents).
#[must_use]
pub fn graph_source_bytes(graph: &ResolvedGraph) -> Vec<u8> {
    let mut paths: Vec<&Path> = graph.modules.iter().map(|m| m.path.as_path()).collect();
    paths.sort();
    let mut buf = Vec::new();
    for p in paths {
        let path_s = p.to_string_lossy();
        buf.extend_from_slice(&(path_s.len() as u64).to_le_bytes());
        buf.extend_from_slice(path_s.as_bytes());
        match fs::read(p) {
            Ok(bytes) => {
                buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
                buf.extend_from_slice(&bytes);
            }
            Err(_) => {
                buf.extend_from_slice(&0u64.to_le_bytes());
            }
        }
    }
    buf
}

fn check_phase_key(graph: &ResolvedGraph) -> PhaseCacheKey {
    let content = graph_source_fingerprint(graph);
    PhaseCacheKey::for_source(
        ArtifactPhase::Check,
        content.as_str().as_bytes(),
        &[("graph", content.as_str())],
    )
}

/// Cache key for LLVM IR.
///
/// [`ArtifactPhase::Codegen`] already fingerprints the full frontend→MIR→codegen
/// stack; extras restate check/lower digests and graph content for explicitness.
#[must_use]
pub fn codegen_phase_key(graph: &ResolvedGraph) -> PhaseCacheKey {
    use echo_fingerprint::phase_fingerprint;
    let content = graph_source_fingerprint(graph);
    let check_fp = phase_fingerprint(ArtifactPhase::Check, &[]);
    let lower_fp = phase_fingerprint(ArtifactPhase::Lower, &[]);
    let content_s = content.as_str().to_string();
    let check_s = check_fp.fingerprint.as_str().to_string();
    let lower_s = lower_fp.fingerprint.as_str().to_string();
    PhaseCacheKey::for_source(
        ArtifactPhase::Codegen,
        content_s.as_bytes(),
        &[
            ("graph", content_s.as_str()),
            ("check_fp", check_s.as_str()),
            ("lower_fp", lower_s.as_str()),
        ],
    )
}

/// Last `Name` segment: `std/net/http` → `http`, `./math` → `math`.
#[must_use]
pub fn module_bind_name(segments: &[PathSeg]) -> Option<String> {
    segments.iter().rev().find_map(|s| match s {
        PathSeg::Name(n) => Some(n.clone()),
        PathSeg::Dot | PathSeg::DotDot => None,
    })
}

fn map_export_kind(k: ExportKind) -> BindingKind {
    match k {
        ExportKind::Mutable => BindingKind::Mutable,
        ExportKind::Immutable => BindingKind::Immutable,
        ExportKind::Const => BindingKind::Const,
        ExportKind::Struct => BindingKind::Struct,
    }
}

/// Param count of a top-level function bind anywhere in the folder module.
fn folder_fn_arity(graph: &ResolvedGraph, root: &Path, name: &str) -> Option<usize> {
    for unit in &graph.modules {
        if unit.module_root == *root {
            if let Some(&n) = unit.facts.fn_arities.get(name) {
                return Some(n);
            }
        }
    }
    None
}

fn type_from_ret_label(label: &str) -> Option<Type> {
    match label {
        "string" => Some(Type::String),
        "i64" => Some(Type::Int),
        "bool" => Some(Type::Bool),
        "bytes" => Some(Type::Bytes),
        "f64" => Some(Type::Float),
        _ => None,
    }
}

fn folder_fn_return_ty(graph: &ResolvedGraph, root: &Path, name: &str) -> Option<Type> {
    for unit in &graph.modules {
        if unit.module_root != *root {
            continue;
        }
        if echo_std::is_runtime_module_path(&unit.path) {
            return echo_std::runtime_ret_kind(name).and_then(type_from_ret_label);
        }
        let Some(file) = unit.parsed.file.as_ref() else {
            continue;
        };
        let mut mods = Vec::new();
        if unit
            .import_targets
            .iter()
            .any(|(_, p)| echo_std::is_runtime_module_path(p))
        {
            mods.push(runtime_as_imported_module());
        }
        let map = infer_export_return_types(file, &mods);
        if let Some(t) = map.get(name) {
            if let Some(leaf) = echo_semantics::exportable_return_kind(t.clone()) {
                return Some(leaf);
            }
        }
    }
    None
}

fn runtime_as_imported_module() -> ImportedModule {
    use echo_source::{BytePos, Span};
    let exports = echo_std::RUNTIME_EXPORTS
        .iter()
        .map(|e| ModuleExport {
            name: e.name.to_string(),
            kind: BindingKind::Immutable,
            return_shape: Some(ReturnShape::Plain),
            arity: None,
            return_ty: echo_std::runtime_ret_kind(e.name).and_then(type_from_ret_label),
        })
        .collect();
    ImportedModule {
        name: "runtime".into(),
        span: Span::new(echo_source::SourceId::from_u32(0), BytePos(0), BytePos(0)),
        exports,
    }
}

/// If `name` is a top-level `$ name = ( ) { }` in the module, compute return shape.
fn fn_return_shape(unit: &ModuleUnit, name: &str) -> Option<ReturnShape> {
    if echo_std::is_runtime_module_path(&unit.path) {
        // Runtime primitives are plain-valued for check (print, …).
        return Some(ReturnShape::Plain);
    }
    let file = unit.parsed.file.as_ref()?;
    for stmt in &file.stmts {
        if let echo_ast::Stmt::Bind(b) = stmt {
            if b.name.name == name {
                if let Some(Expr::Fn { body, .. }) = &b.init {
                    return Some(effects_in_stmts(body).shape());
                }
            }
        }
    }
    None
}

/// Codes for e26 `.check`: `sem-*` / `res-*`.
#[must_use]
pub fn format_check_diag_codes(diagnostics: &Diagnostics) -> String {
    let mut out = String::new();
    for d in diagnostics.items() {
        if let Some(code) = d.code.as_deref() {
            if code.starts_with("sem-") || code.starts_with("res-") {
                out.push_str(code);
                out.push('\n');
            }
        }
    }
    out
}

#[must_use]
pub fn graph_paths(graph: &ResolvedGraph) -> Vec<PathBuf> {
    graph.modules.iter().map(|m| m.path.clone()).collect()
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use echo_cache::CacheLayout;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let mut p = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("echo-check-cache-{t}"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn second_check_is_cache_hit() {
        let root = temp_root();
        let src = root.join("prog.echo");
        fs::write(&src, "$ x = 1\n").unwrap();
        // std import not required
        let layout = CacheLayout::for_project(&root);
        let store = ArtifactStore::new(layout);

        let first = check_entry_with_cache(&src, Some(&store));
        assert_eq!(first.cache, CheckCacheOutcome::Miss);
        assert!(first.parse_cache.misses >= 1);
        assert_eq!(first.diagnostics.error_count(), 0);

        let second = check_entry_with_cache(&src, Some(&store));
        assert_eq!(second.cache, CheckCacheOutcome::Hit);
        assert!(
            second.parse_cache.hits >= 1,
            "parse stats={:?}",
            second.parse_cache
        );
        assert_eq!(second.diagnostics.error_count(), 0);

        store.layout().clean().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn content_change_misses() {
        let root = temp_root();
        let src = root.join("prog.echo");
        fs::write(&src, "$ x = 1\n").unwrap();
        let layout = CacheLayout::for_project(&root);
        let store = ArtifactStore::new(layout);

        assert_eq!(
            check_entry_with_cache(&src, Some(&store)).cache,
            CheckCacheOutcome::Miss
        );
        fs::write(&src, "$ x = 1\n$ y = 2\n").unwrap();
        assert_eq!(
            check_entry_with_cache(&src, Some(&store)).cache,
            CheckCacheOutcome::Miss
        );

        store.layout().clean().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    fn tempfile_dir() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut p = std::env::temp_dir();
        p.push(format!("echo-import-ret-{t}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn defining_module_string_return_is_used_at_importer() {
        let dir = tempfile_dir();
        fs::write(
            dir.join("lib.echo"),
            "$ greet = () {\n    ^ \"hi\"\n}\n\\ greet\n",
        )
        .unwrap();
        let entry = dir.join("main.echo");
        fs::write(&entry, "/ ./lib\n$ x = lib.greet() + 1\n").unwrap();
        let checked = check_entry(&entry);
        let codes: Vec<_> = checked
            .diagnostics
            .items()
            .iter()
            .filter_map(|d| d.code.clone())
            .collect();
        assert!(
            codes.iter().any(|c| c == "sem-type-mismatch"),
            "string + int at importer: {codes:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn imported_params_stay_value_when_return_kind_known() {
        let dir = tempfile_dir();
        fs::write(
            dir.join("lib.echo"),
            "$ show = (x) {\n    ^ \"ok\"\n}\n\\ show\n",
        )
        .unwrap();
        let entry = dir.join("main.echo");
        fs::write(
            &entry,
            "/ ./lib\n$ a = lib.show(1)\n$ b = lib.show(\"hi\")\n",
        )
        .unwrap();
        let checked = check_entry(&entry);
        assert_eq!(
            checked.diagnostics.error_count(),
            0,
            "known string return must not freeze params: {:?}",
            checked.diagnostics.items()
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
