//! Build a closed compilation graph from an entry file (ADR 0006).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use echo_cache::ArtifactStore;
use echo_diagnostics::{Diagnostic, Diagnostics};
use echo_index::{ExportFact, ExportKind, ModuleFacts, PathSeg, extract, format_import_path};
use echo_lexer::Lexed;
use echo_parser::{ParseCacheOutcome, Parsed, parse_with_cache};
use echo_source::{SourceId, SourceMap};
use echo_std::{
    RUNTIME_EXPORTS, is_runtime_module_path, is_under_privileged_std, runtime_module_path,
};

use crate::merge::{MergedStruct, merge_structs};
use crate::virtual_fs::{VirtualSources, normalize_path};

/// How to find packages (`std/…`) and relative imports.
#[derive(Debug, Clone)]
pub struct SearchPaths {
    /// Directories that contain `std/` (or are the `std` parent).
    pub package_roots: Vec<PathBuf>,
    /// From entry project `xo.toml` `[dependencies]`: package id → version pin.
    /// Missing host packages in this map may be auto-installed on resolve.
    pub declared_deps: HashMap<String, String>,
}

impl SearchPaths {
    /// Discover roots near the entry file (walk parents for `std/`).
    ///
    /// Also considers the **toolchain install root** (parent of `bin/` when
    /// `xo` lives at `<root>/bin/xo` with `<root>/std/`) and optional
    /// `$XO_INSTALL_ROOT` so a user install works outside the git checkout.
    #[must_use]
    pub fn default_for(entry: &Path) -> Self {
        let mut package_roots = Vec::new();
        let mut push_root = |p: PathBuf| {
            if p.join("std").is_dir() && !package_roots.contains(&p) {
                package_roots.push(p);
            }
        };

        let mut dir = entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        for _ in 0..16 {
            push_root(dir.clone());
            if !dir.pop() {
                break;
            }
        }
        // CWD (project / workspace checkouts).
        if let Ok(cwd) = std::env::current_dir() {
            push_root(cwd);
        }
        // Explicit install root (install script / wrappers).
        if let Ok(root) = std::env::var("XO_INSTALL_ROOT") {
            let p = PathBuf::from(root);
            if !p.as_os_str().is_empty() {
                push_root(p);
            }
        }
        // `<prefix>/bin/xo` → `<prefix>` when `std/` is co-installed.
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(exe) = exe.canonicalize() {
                if let Some(bin_dir) = exe.parent() {
                    if bin_dir.file_name().is_some_and(|n| n == "bin") {
                        if let Some(prefix) = bin_dir.parent() {
                            push_root(prefix.to_path_buf());
                        }
                    }
                }
            }
        }

        let declared_deps = load_declared_deps(entry);
        Self {
            package_roots,
            declared_deps,
        }
    }
}

/// Load `[dependencies]` from **`cwd/xo.toml`** only.
///
/// Invoke `xo` from the directory that holds the project `xo.toml` (locked).
fn load_declared_deps(_entry: &Path) -> HashMap<String, String> {
    let Ok(cwd) = std::env::current_dir() else {
        return HashMap::new();
    };
    let p = cwd.join("xo.toml");
    if !p.is_file() {
        return HashMap::new();
    }
    match crate::package_cache::XoToml::load(&p) {
        Ok(t) => t.dependencies.into_iter().collect(),
        Err(_) => HashMap::new(),
    }
}

#[derive(Debug, Clone)]
pub struct ModuleUnit {
    /// Path of this source file (always a `.echo` file, except virtual runtime).
    pub path: PathBuf,
    /// Module identity for imports: the file itself, or the directory for multi-file modules.
    pub module_root: PathBuf,
    pub source_id: SourceId,
    pub parsed: Parsed,
    pub facts: ModuleFacts,
    /// Import → resolved **module root** (file or directory).
    pub import_targets: Vec<(echo_index::ImportFact, PathBuf)>,
}

#[derive(Debug, Clone)]
pub struct ResolvedGraph {
    pub entry: PathBuf,
    pub modules: Vec<ModuleUnit>,
    pub merged_structs: HashMap<String, MergedStruct>,
    pub diagnostics: Diagnostics,
}

/// Stats for parse-phase cache during resolve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolveParseCacheStats {
    pub hits: u32,
    pub misses: u32,
    pub bypasses: u32,
}

/// Resolve entry and all static imports into a closed graph (no parse cache).
#[must_use]
pub fn resolve_entry(entry: &Path, search: &SearchPaths) -> (ResolvedGraph, Diagnostics) {
    let (graph, diags, _) = resolve_entry_with_cache(entry, search, None);
    (graph, diags)
}

/// Resolve with optional per-file parse cache ([`parse_with_cache`]).
#[must_use]
pub fn resolve_entry_with_cache(
    entry: &Path,
    search: &SearchPaths,
    store: Option<&ArtifactStore>,
) -> (ResolvedGraph, Diagnostics, ResolveParseCacheStats) {
    resolve_entry_with_overlays(entry, search, store, &HashMap::new())
}

/// Like [`resolve_entry_with_cache`], but `overlays` replace on-disk text for those paths
/// (editor dirty buffers). Overlay paths should be canonical when possible.
#[must_use]
pub fn resolve_entry_with_overlays(
    entry: &Path,
    search: &SearchPaths,
    store: Option<&ArtifactStore>,
    overlays: &HashMap<PathBuf, String>,
) -> (ResolvedGraph, Diagnostics, ResolveParseCacheStats) {
    resolve_entry_inner(entry, search, store, SourceBackend::Disk { overlays })
}

/// Resolve using only [`VirtualSources`] (no disk, no canonicalize).
#[must_use]
pub fn resolve_entry_virtual(
    entry: &Path,
    search: &SearchPaths,
    sources: &VirtualSources,
) -> (ResolvedGraph, Diagnostics, ResolveParseCacheStats) {
    resolve_entry_inner(entry, search, None, SourceBackend::Virtual(sources))
}

enum SourceBackend<'a> {
    Disk {
        overlays: &'a HashMap<PathBuf, String>,
    },
    Virtual(&'a VirtualSources),
}

impl SourceBackend<'_> {
    fn prepare_entry(&self, entry: &Path) -> Result<PathBuf, String> {
        match self {
            Self::Disk { .. } => entry
                .canonicalize()
                .map_err(|e| format!("cannot open entry {}: {e}", entry.display())),
            Self::Virtual(sources) => {
                let path = normalize_path(entry);
                if sources.is_file(&path) {
                    Ok(path)
                } else {
                    Err(format!("cannot open entry {}", entry.display()))
                }
            }
        }
    }

    fn read_text(&self, path: &Path) -> Result<String, String> {
        match self {
            Self::Disk { overlays } => {
                if let Some(text) = overlay_text(overlays, path) {
                    return Ok(text.to_string());
                }
                std::fs::read_to_string(path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))
            }
            Self::Virtual(sources) => sources
                .get(path)
                .map(str::to_string)
                .ok_or_else(|| format!("cannot read {}", path.display())),
        }
    }

    fn is_dir(&self, path: &Path) -> bool {
        match self {
            Self::Disk { .. } => path.is_dir(),
            Self::Virtual(sources) => sources.is_dir(path),
        }
    }

    fn list_echo(&self, dir: &Path) -> Vec<PathBuf> {
        match self {
            Self::Disk { .. } => crate::package_cache::list_echo_files(dir),
            Self::Virtual(sources) => sources.list_echo_files(dir),
        }
    }

    fn resolve_module(&self, base: &Path) -> Result<PathBuf, String> {
        match self {
            Self::Disk { .. } => crate::package_cache::resolve_file_or_dir_module(base),
            Self::Virtual(sources) => sources.resolve_file_or_dir_module(base),
        }
    }
}

fn resolve_entry_inner(
    entry: &Path,
    search: &SearchPaths,
    store: Option<&ArtifactStore>,
    backend: SourceBackend<'_>,
) -> (ResolvedGraph, Diagnostics, ResolveParseCacheStats) {
    let mut diagnostics = Diagnostics::new();
    let mut parse_stats = ResolveParseCacheStats::default();
    let entry = match backend.prepare_entry(entry) {
        Ok(p) => p,
        Err(e) => {
            diagnostics.push(Diagnostic::error(e).with_code("res-entry"));
            return (
                ResolvedGraph {
                    entry: entry.to_path_buf(),
                    modules: vec![],
                    merged_structs: HashMap::new(),
                    diagnostics: diagnostics.clone(),
                },
                diagnostics,
                parse_stats,
            );
        }
    };

    let mut map = SourceMap::new();
    let mut modules: Vec<ModuleUnit> = Vec::new();
    let mut by_path: HashMap<PathBuf, usize> = HashMap::new();
    // (source file path, module_root for that file)
    let mut pending: Vec<(PathBuf, PathBuf)> = vec![(entry.clone(), entry.clone())];
    let mut visiting: HashSet<PathBuf> = HashSet::new();

    while let Some((path, module_root)) = pending.pop() {
        if by_path.contains_key(&path) {
            continue;
        }
        if !visiting.insert(path.clone()) {
            continue;
        }

        // Virtual runtime-primitive package (no on-disk source).
        if is_runtime_module_path(&path) {
            let unit = synthetic_runtime_unit(&mut map);
            let idx = modules.len();
            by_path.insert(path.clone(), idx);
            modules.push(unit);
            continue;
        }

        let id = match backend.read_text(&path) {
            Ok(text) => map.add(&path, text),
            Err(e) => {
                diagnostics.push(Diagnostic::error(e).with_code("res-read"));
                continue;
            }
        };
        let source = map.get(id).expect("loaded");
        // Overlays always re-parse (no stale disk AST); still may fill parse cache for that text.
        let (parsed, parse_outcome) = parse_with_cache(source, store);
        match parse_outcome {
            ParseCacheOutcome::Hit => parse_stats.hits += 1,
            ParseCacheOutcome::Miss | ParseCacheOutcome::StoreError => parse_stats.misses += 1,
            ParseCacheOutcome::Bypass => parse_stats.bypasses += 1,
        }
        for d in parsed.diagnostics.items() {
            diagnostics.push(d.clone());
        }
        let facts = parsed.file.as_ref().map(extract).unwrap_or(ModuleFacts {
            source: id,
            imports: vec![],
            exports: vec![],
            structs: vec![],
            top_binds: vec![],
            fn_arities: std::collections::HashMap::new(),
        });

        let idx = modules.len();
        by_path.insert(path.clone(), idx);

        let importer_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let mut import_targets = Vec::new();
        for imp in &facts.imports {
            match resolve_import_path(&path, importer_dir, &imp.segments, search, &backend) {
                Ok(resolved_root) => {
                    enqueue_module_root(&resolved_root, &mut pending, &by_path, &backend);
                    import_targets.push((imp.clone(), resolved_root));
                }
                Err((msg, code)) => {
                    diagnostics.push(Diagnostic::error(msg).with_span(imp.span).with_code(code));
                }
            }
        }

        modules.push(ModuleUnit {
            path,
            module_root,
            source_id: id,
            parsed,
            facts,
            import_targets,
        });
    }

    detect_import_cycles(&modules, &mut diagnostics);

    // Re-export: `\ name` may re-export a name imported from a dependency.
    resolve_reexports(&mut modules);

    for unit in &modules {
        for exp in &unit.facts.exports {
            if exp.kind.is_none() {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "export `{}` does not name a top-level bind, `%` struct, or imported export in this module",
                        exp.name
                    ))
                    .with_span(exp.span)
                    .with_code("res-export-missing"),
                );
            }
        }
    }

    let fact_refs: Vec<(String, &ModuleFacts)> = modules
        .iter()
        .map(|m| (m.path.display().to_string(), &m.facts))
        .collect();
    let merged_structs = merge_structs(&fact_refs, &mut diagnostics);

    (
        ResolvedGraph {
            entry,
            modules,
            merged_structs,
            diagnostics: diagnostics.clone(),
        },
        diagnostics,
        parse_stats,
    )
}

fn overlay_text<'a>(overlays: &'a HashMap<PathBuf, String>, path: &Path) -> Option<&'a str> {
    if let Some(t) = overlays.get(path) {
        return Some(t.as_str());
    }
    // Match by canonicalize when keys were registered that way.
    if let Ok(canon) = path.canonicalize() {
        if let Some(t) = overlays.get(&canon) {
            return Some(t.as_str());
        }
    }
    overlays.iter().find_map(|(k, v)| {
        if k == path {
            Some(v.as_str())
        } else if let (Ok(a), Ok(b)) = (k.canonicalize(), path.canonicalize()) {
            if a == b { Some(v.as_str()) } else { None }
        } else {
            None
        }
    })
}

/// Fill export kinds by re-exporting imported exports (fixed-point).
///
/// Justified: package entry files (`std/net/http`) re-export sibling structs.
fn resolve_reexports(modules: &mut [ModuleUnit]) {
    use echo_index::ExportKind;

    let path_index: HashMap<PathBuf, usize> = modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.path.clone(), i))
        .collect();

    let mut tables: Vec<HashMap<String, ExportKind>> = modules
        .iter()
        .map(|m| {
            m.facts
                .exports
                .iter()
                .filter_map(|e| e.kind.map(|k| (e.name.clone(), k)))
                .collect()
        })
        .collect();

    for _ in 0..modules.len().saturating_add(2) {
        let mut changed = false;
        let snapshot = tables.clone();
        for (i, unit) in modules.iter().enumerate() {
            for exp in &unit.facts.exports {
                if tables[i].contains_key(&exp.name) {
                    continue;
                }
                for (_, target_path) in &unit.import_targets {
                    let Some(&ti) = path_index.get(target_path) else {
                        continue;
                    };
                    if let Some(&k) = snapshot[ti].get(&exp.name) {
                        tables[i].insert(exp.name.clone(), k);
                        changed = true;
                        break;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    for (i, unit) in modules.iter_mut().enumerate() {
        for exp in &mut unit.facts.exports {
            if exp.kind.is_none() {
                if let Some(&k) = tables[i].get(&exp.name) {
                    exp.kind = Some(k);
                }
            }
        }
    }
}

fn synthetic_runtime_unit(map: &mut SourceMap) -> ModuleUnit {
    let path = runtime_module_path();
    let id = map.add(&path, "; <echo:runtime> virtual package\n");
    let mut exports = Vec::new();
    for exp in RUNTIME_EXPORTS {
        exports.push(ExportFact {
            name: exp.name.to_string(),
            span: echo_source::Span::new(id, echo_source::BytePos(0), echo_source::BytePos(0)),
            kind: Some(ExportKind::Immutable),
            fn_arity: None,
        });
    }
    let facts = ModuleFacts {
        source: id,
        imports: vec![],
        exports,
        structs: vec![],
        top_binds: vec![],
        fn_arities: std::collections::HashMap::new(),
    };
    ModuleUnit {
        path: path.clone(),
        module_root: path,
        source_id: id,
        parsed: Parsed {
            file: None,
            lexed: Lexed {
                tokens: vec![],
                diagnostics: Diagnostics::new(),
            },
            diagnostics: Diagnostics::new(),
        },
        facts,
        import_targets: vec![],
    }
}

/// Queue all source files for a module root (single file or directory of `*.echo`).
fn enqueue_module_root(
    root: &Path,
    pending: &mut Vec<(PathBuf, PathBuf)>,
    by_path: &HashMap<PathBuf, usize>,
    backend: &SourceBackend<'_>,
) {
    if backend.is_dir(root) {
        for f in backend.list_echo(root) {
            if !by_path.contains_key(&f) {
                pending.push((f, root.to_path_buf()));
            }
        }
    } else if !by_path.contains_key(root) {
        pending.push((root.to_path_buf(), root.to_path_buf()));
    }
}

/// Resolve import to a **module root** (`.echo` file or directory containing `*.echo`).
fn resolve_import_path(
    importer_file: &Path,
    importer_dir: &Path,
    segments: &[PathSeg],
    search: &SearchPaths,
    backend: &SourceBackend<'_>,
) -> Result<PathBuf, (String, &'static str)> {
    let display = format_import_path(segments);

    // `/ runtime` — privileged runtime-primitive package (std sources only).
    if matches!(segments, [PathSeg::Name(n)] if n == "runtime") {
        if is_under_privileged_std(importer_file, &search.package_roots) {
            return Ok(runtime_module_path());
        }
        return Err((
            "`/ runtime` is only allowed in privileged std library sources".into(),
            "res-runtime-forbidden",
        ));
    }

    if matches!(segments.first(), Some(PathSeg::Dot | PathSeg::DotDot)) {
        // ./a/b or ../a/b → relative to the importer’s directory
        let mut base = importer_dir.to_path_buf();
        for seg in segments {
            match seg {
                PathSeg::Name(n) => base.push(n),
                PathSeg::Dot => base.push("."),
                PathSeg::DotDot => base.push(".."),
            }
        }
        return backend.resolve_module(&base).map_err(|e| {
            (
                format!("cannot resolve import `{display}`: {e}"),
                "res-import",
            )
        });
    }

    // bare package path: std/io → {root}/std/io.echo or std/io/
    let mut rel = PathBuf::new();
    for seg in segments {
        if let PathSeg::Name(n) = seg {
            rel.push(n);
        }
    }

    for root in &search.package_roots {
        let base = root.join(&rel);
        if let Ok(p) = backend.resolve_module(&base) {
            return Ok(p);
        }
    }

    // Host / URL paths → user package cache (ADR 0014); may auto-get declared deps.
    if crate::package_cache::is_host_path(segments) {
        return crate::package_cache::resolve_host_import(segments, &search.declared_deps)
            .map_err(|msg| (msg, "res-import"));
    }

    Err((
        format!(
            "cannot find module `{display}` (looked under package roots for {}.echo or {}/)",
            rel.display(),
            rel.display()
        ),
        "res-import",
    ))
}

/// Detect cycles among **module roots** (import edges).
fn detect_import_cycles(modules: &[ModuleUnit], diagnostics: &mut Diagnostics) {
    use std::collections::HashMap as Map;

    let mut edges: Map<PathBuf, Vec<PathBuf>> = Map::new();
    for m in modules {
        let from = m.module_root.clone();
        let entry = edges.entry(from).or_default();
        for (_, to) in &m.import_targets {
            if !entry.contains(to) {
                entry.push(to.clone());
            }
        }
    }

    // 0 = white, 1 = gray, 2 = black
    let mut color: Map<PathBuf, u8> = Map::new();
    let mut stack: Vec<PathBuf> = Vec::new();

    fn dfs(
        node: &Path,
        edges: &Map<PathBuf, Vec<PathBuf>>,
        color: &mut Map<PathBuf, u8>,
        stack: &mut Vec<PathBuf>,
        diagnostics: &mut Diagnostics,
    ) {
        color.insert(node.to_path_buf(), 1);
        stack.push(node.to_path_buf());
        if let Some(succ) = edges.get(node) {
            for next in succ {
                match color.get(next).copied().unwrap_or(0) {
                    1 => {
                        // cycle
                        let mut cycle: Vec<String> = stack
                            .iter()
                            .skip_while(|p| p.as_path() != next.as_path())
                            .map(|p| p.display().to_string())
                            .collect();
                        cycle.push(next.display().to_string());
                        diagnostics.push(
                            Diagnostic::error(format!("import cycle: {}", cycle.join(" → ")))
                                .with_code("res-import-cycle"),
                        );
                    }
                    0 => dfs(next, edges, color, stack, diagnostics),
                    _ => {}
                }
            }
        }
        stack.pop();
        color.insert(node.to_path_buf(), 2);
    }

    let roots: Vec<PathBuf> = edges.keys().cloned().collect();
    for r in roots {
        if color.get(&r).copied().unwrap_or(0) == 0 {
            dfs(&r, &edges, &mut color, &mut stack, diagnostics);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_import_forbidden_outside_std() {
        let dir = tempfile_dir();
        let entry = dir.join("entry.echo");
        std::fs::write(&entry, "/ runtime\n").unwrap();
        let (_graph, diags) = resolve_entry(&entry, &SearchPaths::default_for(&entry));
        assert!(
            diags
                .items()
                .iter()
                .any(|d| d.code.as_deref() == Some("res-runtime-forbidden")),
            "{:?}",
            diags.items()
        );
    }

    #[test]
    fn parent_relative_import() {
        let dir = tempfile_dir();
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let util = dir.join("util.echo");
        let entry = nested.join("entry.echo");
        std::fs::write(&util, "$ n = 7\n\\ n\n").unwrap();
        std::fs::write(&entry, "/ ../util\n").unwrap();

        let (graph, diags) = resolve_entry(&entry, &SearchPaths::default_for(&entry));
        assert_eq!(diags.error_count(), 0, "{:?}", diags.items());
        assert_eq!(graph.modules.len(), 2);
        let names: Vec<_> = graph
            .modules
            .iter()
            .map(|m| m.path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n == "entry"), "{names:?}");
        assert!(names.iter().any(|n| n == "util"), "{names:?}");
    }

    #[test]
    fn relative_import_and_struct_merge() {
        let dir = tempfile_dir();
        let user = dir.join("user.echo");
        let ops = dir.join("user_ops.echo");
        let entry = dir.join("entry.echo");

        std::fs::write(&user, "% user {\n    $ name\n}\n\\ user\n").unwrap();
        std::fs::write(
            &ops,
            "/ ./user\n@ user {\n    $ greet = () {\n        ^ .name\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(&entry, "/ ./user\n/ ./user_ops\n").unwrap();

        let (graph, diags) = resolve_entry(&entry, &SearchPaths::default_for(&entry));
        assert_eq!(diags.error_count(), 0, "{:?}", diags.items());
        assert_eq!(graph.modules.len(), 3);
        let merged = graph.merged_structs.get("user").expect("user");
        let names: Vec<_> = merged.members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"greet"));
    }

    #[test]
    fn duplicate_member_errors() {
        let dir = tempfile_dir();
        let user = dir.join("user.echo");
        let ops = dir.join("ops.echo");
        std::fs::write(&user, "% user {\n    $ name\n}\n").unwrap();
        std::fs::write(&ops, "/ ./user\n@ user {\n    $ name\n}\n").unwrap();
        let entry = dir.join("entry.echo");
        std::fs::write(&entry, "/ ./user\n/ ./ops\n").unwrap();

        let (_graph, diags) = resolve_entry(&entry, &SearchPaths::default_for(&entry));
        assert!(
            diags
                .items()
                .iter()
                .any(|d| d.code.as_deref() == Some("res-struct-dup-member")),
            "{:?}",
            diags.items()
        );
    }

    fn tempfile_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!("echo-res-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn import_cycle_is_diagnosed() {
        let dir = tempfile_dir();
        let a = dir.join("a.echo");
        let b = dir.join("b.echo");
        std::fs::write(&a, "/ ./b\n").unwrap();
        std::fs::write(&b, "/ ./a\n").unwrap();
        let (_graph, diags) = resolve_entry(&a, &SearchPaths::default_for(&a));
        assert!(
            diags
                .items()
                .iter()
                .any(|d| d.code.as_deref() == Some("res-import-cycle")),
            "{:?}",
            diags.items()
        );
    }

    #[test]
    fn folder_module_unions_exports() {
        let dir = tempfile_dir();
        let math = dir.join("math");
        std::fs::create_dir_all(&math).unwrap();
        std::fs::write(math.join("add.echo"), "\\ add\n$ add = (a) {\n    ^ a\n}\n").unwrap();
        std::fs::write(math.join("mul.echo"), "\\ mul\n$ mul = (a) {\n    ^ a\n}\n").unwrap();
        let entry = dir.join("entry.echo");
        std::fs::write(&entry, "/ ./math\n$ x = math.add(1)\n$ y = math.mul(2)\n").unwrap();

        let (graph, diags) = resolve_entry(&entry, &SearchPaths::default_for(&entry));
        assert_eq!(diags.error_count(), 0, "{:?}", diags.items());
        // entry + two math files
        assert_eq!(graph.modules.len(), 3);
        let roots: Vec<_> = graph
            .modules
            .iter()
            .filter(|m| m.path != entry)
            .map(|m| m.module_root.clone())
            .collect();
        assert!(roots.iter().all(|r| r == &math.canonicalize().unwrap()));
    }

    #[test]
    fn folder_export_arity_reaches_check() {
        let dir = tempfile_dir();
        std::fs::write(
            dir.join("io.echo"),
            "\\ print\n$ print = (x) {\n    ^ x\n}\n",
        )
        .unwrap();
        let entry = dir.join("entry.echo");
        std::fs::write(&entry, "/ ./io\nio.print()\nio.print(1, 2)\n").unwrap();
        let checked = crate::check_entry(&entry);
        let n = checked
            .diagnostics
            .items()
            .iter()
            .filter(|d| d.code.as_deref() == Some("sem-arity"))
            .count();
        assert_eq!(n, 2, "{:?}", checked.diagnostics.items());
    }

    #[test]
    fn host_import_resolves_from_package_cache() {
        use crate::package_cache::{install_local_dir, with_xo_home_for_test};

        let xo = tempfile_dir();
        with_xo_home_for_test(xo, || {
            let pkg = tempfile_dir();
            std::fs::write(pkg.join("util.echo"), "\\ n\n$ n = 1\n").unwrap();
            install_local_dir("github.com/acme/lib", "v1", &pkg).unwrap();

            let entry = tempfile_dir().join("entry.echo");
            std::fs::write(&entry, "/ github.com/acme/lib/util\n").unwrap();

            let (graph, diags) = resolve_entry(&entry, &SearchPaths::default_for(&entry));
            assert_eq!(diags.error_count(), 0, "{:?}", diags.items());
            assert!(
                graph
                    .modules
                    .iter()
                    .any(|m| m.path.file_name().is_some_and(|n| n == "util.echo")),
                "modules={:?}",
                graph
                    .modules
                    .iter()
                    .map(|m| m.path.clone())
                    .collect::<Vec<_>>()
            );
        });
    }

    fn virtual_search() -> SearchPaths {
        SearchPaths {
            package_roots: vec![PathBuf::from("/echo")],
            declared_deps: HashMap::new(),
        }
    }

    #[test]
    fn virtual_std_import_allows_runtime_inside_std() {
        let mut sources = VirtualSources::new();
        sources.insert(
            "/echo/std/io.echo",
            "/ runtime\n\\ print\n$ print = (x) {\n    runtime.print(x)\n    ^ x\n}\n",
        );
        sources.insert("/echo/playground.echo", "/ std/io\n$ n = io.print(1)\n");
        let checked = crate::check_entry_virtual(
            Path::new("/echo/playground.echo"),
            &virtual_search(),
            &sources,
        );
        assert_eq!(
            checked.diagnostics.error_count(),
            0,
            "{:?}",
            checked.diagnostics.items()
        );
        assert!(
            checked
                .graph
                .modules
                .iter()
                .any(|m| m.path == PathBuf::from("/echo/std/io.echo"))
        );
    }

    #[test]
    fn virtual_userland_runtime_import_is_forbidden() {
        let mut sources = VirtualSources::new();
        sources.insert("/echo/playground.echo", "/ runtime\n");
        let checked = crate::check_entry_virtual(
            Path::new("/echo/playground.echo"),
            &virtual_search(),
            &sources,
        );
        assert!(
            checked
                .diagnostics
                .items()
                .iter()
                .any(|d| d.code.as_deref() == Some("res-runtime-forbidden")),
            "{:?}",
            checked.diagnostics.items()
        );
    }

    #[test]
    fn virtual_folder_module_unions_exports() {
        let mut sources = VirtualSources::new();
        sources.insert("/echo/math/add.echo", "\\ add\n$ add = (a) {\n    ^ a\n}\n");
        sources.insert("/echo/math/mul.echo", "\\ mul\n$ mul = (a) {\n    ^ a\n}\n");
        sources.insert(
            "/echo/playground.echo",
            "/ ./math\n$ x = math.add(1)\n$ y = math.mul(2)\n",
        );
        let checked = crate::check_entry_virtual(
            Path::new("/echo/playground.echo"),
            &virtual_search(),
            &sources,
        );
        assert_eq!(
            checked.diagnostics.error_count(),
            0,
            "{:?}",
            checked.diagnostics.items()
        );
        assert_eq!(checked.graph.modules.len(), 3);
    }

    #[test]
    fn virtual_missing_entry_is_res_entry() {
        let sources = VirtualSources::new();
        let checked = crate::check_entry_virtual(
            Path::new("/echo/missing.echo"),
            &virtual_search(),
            &sources,
        );
        assert!(
            checked
                .diagnostics
                .items()
                .iter()
                .any(|d| d.code.as_deref() == Some("res-entry")),
            "{:?}",
            checked.diagnostics.items()
        );
    }
}
