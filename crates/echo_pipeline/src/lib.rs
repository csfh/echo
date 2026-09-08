//! Shared analyze / lower / compile entry surface.
//!
//! **SOTA rule:** language meaning is decided in [`analyze`]; executable
//! lowering runs only when [`AnalysisProduct::is_ok`]. Hosts (`xo`, LSP, …)
//! must call this crate rather than assembling check → raw-AST → HIR themselves.
//!
//! See `docs/sota-gaps.md` and ADR 0012.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use echo_ast::{BindLeader, File};
use echo_build::project_root_for;
use echo_cache::{ArtifactStore, CacheLayout};
use echo_codegen::{decode_ir_artifact, emit_llvm_with, encode_ir_artifact};

/// Re-export for hosts (`xo`, tests).
pub use echo_codegen::OptLevel;
use echo_diagnostics::Diagnostics;
use echo_hir::{HirExprKind, HirModule, HirStmt, lower_file};
use echo_mir::{LoweredProgram, MirProgram, ModuleLowerInput, lower_program};
use echo_resolver::{
    CheckCacheOutcome, ProjectChecked, ResolveParseCacheStats, check_entry_with_overlays,
    module_bind_name,
};
use echo_semantics::{BindingKind, SemanticModel, ValueKind};
use echo_source::Span;
use echo_std::is_runtime_module_path;

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Options for front-end analysis.
#[derive(Debug, Clone, Default)]
pub struct AnalyzeOptions {
    /// Use `.xo` parse/check caches.
    pub use_cache: bool,
    /// Editor dirty buffers (canonical path → text).
    pub overlays: HashMap<PathBuf, String>,
}

/// One module after successful packaging of analysis facts.
#[derive(Debug, Clone)]
pub struct AnalyzedModule {
    pub path: PathBuf,
    /// Source AST with spans (provenance).
    pub file: Option<File>,
    /// HIR built with import classification and method tables.
    pub hir: HirModule,
    /// Import bind name → resolved module path.
    pub imports: HashMap<String, PathBuf>,
    /// Export names from this module.
    pub exports: Vec<String>,
    /// Bind/kind/struct typing facts for MIR (consume-only).
    pub semantic: SemanticModel,
}

/// Consumable analysis product: the only legal input to executable lower.
#[derive(Debug)]
pub struct AnalysisProduct {
    pub entry: PathBuf,
    pub diagnostics: Diagnostics,
    pub modules: Vec<AnalyzedModule>,
    pub check_cache: CheckCacheOutcome,
    pub parse_cache: ResolveParseCacheStats,
}

impl AnalysisProduct {
    /// True when the program may be lowered / executed.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.diagnostics.error_count() == 0
    }
}

/// Analyze entry path into an [`AnalysisProduct`].
#[must_use]
pub fn analyze(entry: &Path, opts: &AnalyzeOptions) -> AnalysisProduct {
    let store = if opts.use_cache {
        let layout = CacheLayout::for_project(project_root_for(entry));
        Some(ArtifactStore::new(layout))
    } else {
        None
    };

    let checked: ProjectChecked = check_entry_with_overlays(entry, store.as_ref(), &opts.overlays);

    let modules = package_modules(&checked);

    AnalysisProduct {
        entry: checked.graph.entry.clone(),
        diagnostics: checked.diagnostics,
        modules,
        check_cache: checked.cache,
        parse_cache: checked.parse_cache,
    }
}

fn package_modules(checked: &ProjectChecked) -> Vec<AnalyzedModule> {
    let mut out = Vec::new();
    for unit in &checked.graph.modules {
        if is_runtime_module_path(&unit.path) {
            continue;
        }
        let mut imports = HashMap::new();
        let mut import_names = HashSet::new();
        for (imp, target) in &unit.import_targets {
            if let Some(name) = module_bind_name(&imp.segments) {
                import_names.insert(name.clone());
                imports.insert(name, target.clone());
            }
        }
        let exports: Vec<String> = unit.facts.exports.iter().map(|e| e.name.clone()).collect();

        let (file, hir) = match unit.parsed.file.as_ref() {
            Some(f) => (Some(f.clone()), lower_file(f, &import_names)),
            None => (
                None,
                HirModule {
                    import_modules: import_names.clone(),
                    ..HirModule::default()
                },
            ),
        };

        let semantic = build_semantic_model(&hir, &import_names);

        out.push(AnalyzedModule {
            path: unit.path.clone(),
            file,
            hir,
            imports,
            exports,
            semantic,
        });
    }
    out
}

/// Derive MIR-facing facts from HIR + import set (analysis ownership).
fn build_semantic_model(hir: &HirModule, import_names: &HashSet<String>) -> SemanticModel {
    let mut model = SemanticModel::new();
    let dummy = Span::new(
        echo_source::SourceId::from_u32(0),
        echo_source::BytePos(0),
        echo_source::BytePos(0),
    );
    for name in import_names {
        model.introduce_in_scope(
            name.clone(),
            BindingKind::Module,
            ValueKind::Module,
            dummy,
            0,
        );
    }
    for f in &hir.bodies {
        if let (Some(st), Some(mname)) = (&f.receiver_struct, &f.method_name) {
            model.set_method_returns_receiver(st.clone(), mname.clone(), f.returns_receiver);
        }
        // Each function body walks scopes from root 0 for ownership facts.
        for p in &f.params {
            if p == echo_hir::RECV_PARAM {
                if let Some(st) = &f.receiver_struct {
                    model.introduce_in_scope(
                        p.clone(),
                        BindingKind::Immutable,
                        ValueKind::Struct { name: st.clone() },
                        f.span,
                        0,
                    );
                }
            } else {
                model.introduce_in_scope(
                    p.clone(),
                    BindingKind::Immutable,
                    ValueKind::Unknown,
                    f.span,
                    0,
                );
            }
        }
        collect_stmt_facts(&f.body, &mut model, 0);
    }
    collect_stmt_facts(&hir.entry, &mut model, 0);
    model
}

fn binding_from_leader(leader: BindLeader) -> BindingKind {
    match leader {
        BindLeader::Tilde => BindingKind::Mutable,
        BindLeader::Dollar => BindingKind::Immutable,
        BindLeader::Hash => BindingKind::Const,
    }
}

fn value_kind_of_expr(e: &echo_hir::HirExpr, model: &SemanticModel) -> ValueKind {
    match &e.kind {
        HirExprKind::Int { .. } => ValueKind::Int,
        HirExprKind::Bool(_) => ValueKind::Bool,
        HirExprKind::StringLit { .. } => ValueKind::String,
        HirExprKind::List(_) => ValueKind::List,
        HirExprKind::StructLit { name, .. } if !name.is_empty() => {
            ValueKind::Struct { name: name.clone() }
        }
        // Anonymous product `{ k: v }` — runtime object, not a `%` type for methods.
        HirExprKind::StructLit { name, .. } if name.is_empty() => ValueKind::Unknown,
        HirExprKind::Name(n) => {
            if let Some(st) = model.struct_of(n) {
                ValueKind::Struct {
                    name: st.to_string(),
                }
            } else if model.is_module_import(n) {
                ValueKind::Module
            } else {
                model
                    .binds
                    .get(n)
                    .map(|b| b.value_kind.clone())
                    .unwrap_or(ValueKind::Unknown)
            }
        }
        HirExprKind::MethodCall {
            receiver, method, ..
        } => {
            // Receiver's struct type (name, chained self-return, or nested).
            let recv_st = value_kind_of_expr(receiver, model)
                .struct_name()
                .map(str::to_string)
                .or_else(|| {
                    if let HirExprKind::Name(n) = &receiver.kind {
                        model.struct_of(n).map(str::to_string)
                    } else {
                        None
                    }
                });
            if let Some(st) = recv_st {
                if model
                    .returns_receiver
                    .get(&(st.clone(), method.clone()))
                    .copied()
                    .unwrap_or(false)
                {
                    return ValueKind::Struct { name: st };
                }
            }
            ValueKind::Unknown
        }
        HirExprKind::Group(inner) => value_kind_of_expr(inner, model),
        _ => ValueKind::Unknown,
    }
}

fn collect_stmt_facts(stmts: &[HirStmt], model: &mut SemanticModel, scope: u32) {
    for s in stmts {
        match s {
            HirStmt::Bind {
                leader,
                name,
                init,
                span,
            } => {
                let vk = init
                    .as_ref()
                    .map(|e| value_kind_of_expr(e, model))
                    .unwrap_or(ValueKind::Unknown);
                model.introduce_in_scope(
                    name.clone(),
                    binding_from_leader(*leader),
                    vk,
                    *span,
                    scope,
                );
            }
            HirStmt::Assign { name, value, span } => {
                let vk = value_kind_of_expr(value, model);
                if let HirExprKind::Name(from) = &value.kind {
                    model.copy_struct_type(from, name);
                }
                // Reassign keeps introduction owning_scope (introduce_in_scope).
                model.introduce_in_scope(name.clone(), BindingKind::Mutable, vk, *span, scope);
            }
            HirStmt::If {
                arms, else_body, ..
            } => {
                for (_, body) in arms {
                    let sid = model.alloc_scope();
                    collect_stmt_facts(body, model, sid);
                }
                if let Some(b) = else_body {
                    let sid = model.alloc_scope();
                    collect_stmt_facts(b, model, sid);
                }
            }
            HirStmt::Match { arms, .. } => {
                for arm in arms {
                    let sid = model.alloc_scope();
                    match arm {
                        echo_hir::HirMatchArm::Ok { name, body }
                        | echo_hir::HirMatchArm::Err { name, body } => {
                            model.introduce_in_scope(
                                name.clone(),
                                BindingKind::Immutable,
                                ValueKind::Unknown,
                                Span::new(
                                    echo_source::SourceId::from_u32(0),
                                    echo_source::BytePos(0),
                                    echo_source::BytePos(0),
                                ),
                                sid,
                            );
                            collect_stmt_facts(body, model, sid);
                        }
                        echo_hir::HirMatchArm::Default { body }
                        | echo_hir::HirMatchArm::Values { body, .. }
                        | echo_hir::HirMatchArm::Type { body, .. } => {
                            collect_stmt_facts(body, model, sid);
                        }
                    }
                }
            }
            HirStmt::Loop { body, .. } => {
                let sid = model.alloc_scope();
                collect_stmt_facts(body, model, sid);
            }
            HirStmt::TaskSpawn { bind, span, .. }
            | HirStmt::TaskSpawnFn { bind, span, .. } => {
                if let Some(name) = bind {
                    model.introduce_in_scope(
                        name.clone(),
                        BindingKind::Immutable,
                        ValueKind::Unknown,
                        *span,
                        scope,
                    );
                }
            }
            HirStmt::TaskJoin { bind, span, .. } => {
                if let Some(name) = bind {
                    model.introduce_in_scope(
                        name.clone(),
                        BindingKind::Immutable,
                        ValueKind::Unknown,
                        *span,
                        scope,
                    );
                }
            }
            HirStmt::EffectBlock { bind, body, span } => {
                let sid = model.alloc_scope();
                collect_stmt_facts(body, model, sid);
                if let Some(name) = bind {
                    model.introduce_in_scope(
                        name.clone(),
                        BindingKind::Immutable,
                        ValueKind::Unknown,
                        *span,
                        scope,
                    );
                }
            }
            HirStmt::FieldAssign { .. }
            | HirStmt::IndexAssign { .. }
            | HirStmt::Return { .. }
            | HirStmt::ErrorReturn { .. }
            | HirStmt::Break { .. }
            | HirStmt::Continue { .. }
            | HirStmt::Expr(_)
            | HirStmt::Unsupported { .. } => {}
        }
    }
}

/// Lower an analysis product to MIR. **Refuses** when `!product.is_ok()`.
pub fn lower_to_mir(product: &AnalysisProduct) -> Result<LoweredProgram, Diagnostics> {
    if !product.is_ok() {
        return Err(product.diagnostics.clone());
    }

    let inputs: Vec<ModuleLowerInput> = product
        .modules
        .iter()
        .map(|m| ModuleLowerInput {
            path: m.path.clone(),
            hir: m.hir.clone(),
            imports: m.imports.clone(),
            exports: m.exports.clone(),
            semantic: m.semantic.clone(),
        })
        .collect();

    let lowered = lower_program(product.entry.clone(), &inputs);
    if lowered.diagnostics.error_count() > 0 {
        return Err(lowered.diagnostics);
    }
    Ok(lowered)
}

/// Result of full compile to LLVM IR text.
#[derive(Debug)]
pub struct CompileResult {
    pub ir: String,
    pub diagnostics: Diagnostics,
    pub analysis: AnalysisProduct,
    /// Whether codegen used the IR artifact cache.
    pub ir_cache_hit: bool,
}

/// Analyze + (if ok) lower + emit LLVM at [`OptLevel::O0`]. Does not link.
pub fn compile_to_llvm(entry: &Path, opts: &AnalyzeOptions) -> CompileResult {
    compile_to_llvm_with(entry, opts, OptLevel::O0)
}

/// Analyze + (if ok) lower + emit LLVM at `opt` (verify ± `default<On>`). Does not link.
pub fn compile_to_llvm_with(entry: &Path, opts: &AnalyzeOptions, opt: OptLevel) -> CompileResult {
    let product = analyze(entry, opts);
    if !product.is_ok() {
        return CompileResult {
            ir: String::new(),
            diagnostics: product.diagnostics.clone(),
            analysis: product,
            ir_cache_hit: false,
        };
    }

    // IR cache is disk-graph keyed (like check semantic cache). Editor overlays
    // must never read or write that cache — otherwise an overlay compile poisons
    // later disk-only compiles (same bug class as semantic check under overlays).
    let use_ir_cache = opts.use_cache && opts.overlays.is_empty();
    let store = if use_ir_cache {
        let layout = CacheLayout::for_project(project_root_for(entry));
        Some(ArtifactStore::new(layout))
    } else {
        None
    };

    let ir_key = if use_ir_cache {
        Some(codegen_ir_cache_key(&product, opt))
    } else {
        None
    };

    if let (Some(s), Some(key)) = (store.as_ref(), ir_key.as_ref()) {
        if let Ok(Some(bytes)) = s.get(key) {
            if let Some(ir) = decode_ir_artifact(&bytes) {
                return CompileResult {
                    ir,
                    diagnostics: product.diagnostics.clone(),
                    analysis: product,
                    ir_cache_hit: true,
                };
            }
        }
    }

    let mir = match lower_to_mir(&product) {
        Ok(m) => m,
        Err(diags) => {
            let mut d = product.diagnostics.clone();
            d.extend(diags);
            return CompileResult {
                ir: String::new(),
                diagnostics: d,
                analysis: product,
                ir_cache_hit: false,
            };
        }
    };

    let emitted = emit_llvm_with(&mir.program, opt);
    let mut diagnostics = product.diagnostics.clone();
    diagnostics.extend(emitted.diagnostics);
    if diagnostics.error_count() > 0 {
        return CompileResult {
            ir: String::new(),
            diagnostics,
            analysis: product,
            ir_cache_hit: false,
        };
    }

    if let (Some(s), Some(key)) = (store.as_ref(), ir_key.as_ref()) {
        let _ = s.put(key, &encode_ir_artifact(&emitted.ir));
    }

    CompileResult {
        ir: emitted.ir,
        diagnostics,
        analysis: product,
        ir_cache_hit: false,
    }
}

/// Fingerprint on-disk module graph + opt + stage fingerprints for IR cache.
///
/// [`PhaseCacheKey::for_source`] for [`ArtifactPhase::Codegen`] already hashes
/// the full frontend→MIR→codegen component stack (see `phase_components`).
/// Extras still record graph content, nested stage digests, ABI, and [`OptLevel`]
/// so opt levels never collide and keys stay explicit for cache doctor/debug.
fn codegen_ir_cache_key(product: &AnalysisProduct, opt: OptLevel) -> echo_cache::PhaseCacheKey {
    use echo_cache::PhaseCacheKey;
    use echo_fingerprint::{
        ArtifactPhase, Fingerprint, RUNTIME_ABI_VERSION, phase_fingerprint,
    };

    let mut buf = Vec::new();
    let mut paths: Vec<_> = product.modules.iter().map(|m| m.path.clone()).collect();
    paths.sort();
    for p in &paths {
        let s = p.to_string_lossy();
        buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
        if let Ok(bytes) = std::fs::read(p) {
            buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            buf.extend_from_slice(&bytes);
        } else {
            buf.extend_from_slice(&0u64.to_le_bytes());
        }
    }
    let content = Fingerprint::from_bytes(&buf);
    let check_fp = phase_fingerprint(ArtifactPhase::Check, &[]);
    let lower_fp = phase_fingerprint(ArtifactPhase::Lower, &[]);
    let codegen_fp = phase_fingerprint(ArtifactPhase::Codegen, &[]);
    let cs = content.as_str().to_string();
    let cfp = check_fp.fingerprint.as_str().to_string();
    let lfp = lower_fp.fingerprint.as_str().to_string();
    let gfp = codegen_fp.fingerprint.as_str().to_string();
    let opt_s = opt.as_str();
    let abi = RUNTIME_ABI_VERSION.to_string();
    PhaseCacheKey::for_source(
        ArtifactPhase::Codegen,
        cs.as_bytes(),
        &[
            ("graph", cs.as_str()),
            ("check_fp", cfp.as_str()),
            ("lower_fp", lfp.as_str()),
            ("codegen_fp", gfp.as_str()),
            ("runtime_abi", abi.as_str()),
            ("opt", opt_s),
        ],
    )
}

/// Convenience: lower product to MIR program only when ok.
pub fn mir_program(product: &AnalysisProduct) -> Result<MirProgram, Diagnostics> {
    lower_to_mir(product).map(|l| l.program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_echo(src: &str) -> (PathBuf, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let mut root = std::env::temp_dir();
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = N.fetch_add(1, Ordering::Relaxed);
        let tid = std::thread::current().id();
        root.push(format!("echo-pipe-{t}-{n}-{tid:?}"));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("t.echo");
        fs::write(&path, src).unwrap();
        (root, path)
    }

    #[test]
    fn rejects_lower_when_analysis_has_errors() {
        let (root, path) = temp_echo("$ x = 1\n$ x = 2\n");
        let product = analyze(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(!product.is_ok());
        assert!(lower_to_mir(&product).is_err());
        let comp = compile_to_llvm(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(comp.ir.is_empty());
        assert!(comp.diagnostics.error_count() > 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn valid_program_lowers_and_emits() {
        let (root, path) = temp_echo(
            "\
/ std/io
io.print(1)
",
        );
        // std may not resolve from temp — use pure arithmetic return
        let (root2, path2) = temp_echo("^ 42\n");
        let product = analyze(
            &path2,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(product.is_ok(), "{:?}", product.diagnostics.items());
        let mir = lower_to_mir(&product).expect("lower");
        assert!(!mir.program.functions.is_empty());
        let comp = compile_to_llvm(
            &path2,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(
            comp.diagnostics.error_count() == 0,
            "{:?}",
            comp.diagnostics.items()
        );
        assert!(comp.ir.contains("echo_entry") || comp.ir.contains("define"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(root2);
        let _ = path;
    }

    #[test]
    fn analysis_product_retains_ast_and_hir_spans() {
        let (root, path) = temp_echo("$ n = 7\n^ n\n");
        let product = analyze(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(product.is_ok());
        let m = &product.modules[0];
        assert!(m.file.is_some());
        // HIR bind/init carries span
        let has_span = m.hir.entry.iter().any(|s| match s {
            echo_hir::HirStmt::Bind { span, init, .. } => {
                span.start.0 < span.end.0
                    && init
                        .as_ref()
                        .map(|e| e.span.start.0 <= e.span.end.0)
                        .unwrap_or(true)
            }
            _ => false,
        });
        assert!(has_span, "expected HIR spans from AST");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn module_import_fact_on_product() {
        // Without real std, just verify import_modules empty for no imports
        let (root, path) = temp_echo("^ 1\n");
        let product = analyze(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(product.is_ok());
        assert!(product.modules[0].hir.import_modules.is_empty());
        assert!(product.modules[0].imports.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn semantic_model_records_struct_bind() {
        let (root, path) = temp_echo(
            "\
% counter {
    ~ n
}
$ c = counter { n: 0 }
^ c.n
",
        );
        let product = analyze(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(product.is_ok(), "{:?}", product.diagnostics.items());
        let m = &product.modules[0];
        assert_eq!(m.semantic.struct_of("c"), Some("counter"));
        assert!(matches!(
            m.semantic.binds.get("c").map(|b| &b.value_kind),
            Some(ValueKind::Struct { name }) if name == "counter"
        ));
        let mir = lower_to_mir(&product).expect("lower");
        let top = mir
            .program
            .functions
            .iter()
            .find(|f| f.name == "__toplevel")
            .expect("toplevel");
        assert!(!top.cfg.blocks.is_empty(), "CFG must be attached on lower");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ownership_facts_nested_bind_scope() {
        let (root, path) = temp_echo(
            "\
$ outer = [1]
? 1 == 1 {
    $ inner = [2]
    ^ outer[0] + inner[0]
}
^ 0
",
        );
        let product = analyze(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(product.is_ok(), "{:?}", product.diagnostics.items());
        let m = &product.modules[0];
        assert_eq!(m.semantic.owning_scope_of("outer"), Some(0));
        let inner_scope = m.semantic.owning_scope_of("inner");
        assert!(
            matches!(inner_scope, Some(s) if s > 0),
            "inner bind must have nested owning_scope, got {inner_scope:?}"
        );
        // Managed kinds flagged for lifetime.
        assert!(echo_semantics::SemanticModel::is_managed_kind(
            &m.semantic.binds["outer"].value_kind
        ));
        let _ = fs::remove_dir_all(root);
    }

    /// Overlay compile must not poison later disk-only IR cache lookups.
    #[test]
    fn overlay_compile_does_not_poison_disk_ir_cache() {
        let (root, path) = temp_echo("^ 1\n");
        let canon = path.canonicalize().unwrap();

        // 1) Compile with overlay returning 2, cache enabled — must not write disk key.
        let mut overlays = HashMap::new();
        overlays.insert(canon.clone(), "^ 2\n".into());
        let overlay_comp = compile_to_llvm(
            &path,
            &AnalyzeOptions {
                use_cache: true,
                overlays,
            },
        );
        assert_eq!(overlay_comp.diagnostics.error_count(), 0);
        assert!(
            !overlay_comp.ir_cache_hit,
            "overlay session must not hit disk IR cache"
        );
        assert!(
            overlay_comp.ir.contains("ret i64 2") || overlay_comp.ir.contains("i64 2"),
            "overlay should compile ^ 2; ir={}",
            overlay_comp.ir
        );

        // 2) Disk-only compile of ^ 1 — must not reuse overlay IR.
        let disk_comp = compile_to_llvm(
            &path,
            &AnalyzeOptions {
                use_cache: true,
                overlays: HashMap::new(),
            },
        );
        assert_eq!(disk_comp.diagnostics.error_count(), 0);
        assert!(
            !disk_comp.ir_cache_hit,
            "first disk compile after overlay must miss (overlay wrote nothing for disk key)"
        );
        assert!(
            disk_comp.ir.contains("ret i64 1") || disk_comp.ir.contains("i64 1"),
            "disk compile must be ^ 1, not poisoned by overlay; ir={}",
            disk_comp.ir
        );
        assert!(
            !disk_comp.ir.contains("ret i64 2"),
            "poisoned: disk IR still has ret i64 2"
        );

        // 3) Second disk compile may hit cache for ^ 1.
        let disk2 = compile_to_llvm(
            &path,
            &AnalyzeOptions {
                use_cache: true,
                overlays: HashMap::new(),
            },
        );
        assert!(
            disk2.ir_cache_hit,
            "second disk compile should hit IR cache"
        );
        assert!(
            disk2.ir.contains("ret i64 1") || disk2.ir.contains("i64 1"),
            "cached disk IR must still be ^ 1; ir={}",
            disk2.ir
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn default_compile_is_o0() {
        let (root, path) = temp_echo("^ 42\n");
        let opts = AnalyzeOptions {
            use_cache: false,
            ..Default::default()
        };
        let default_ir = compile_to_llvm(&path, &opts);
        let o0 = compile_to_llvm_with(&path, &opts, OptLevel::O0);
        assert_eq!(default_ir.diagnostics.error_count(), 0);
        assert_eq!(o0.diagnostics.error_count(), 0);
        assert_eq!(default_ir.ir, o0.ir, "compile_to_llvm must default to O0");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn opt_levels_produce_distinct_ir_cache_keys() {
        let (root, path) = temp_echo(
            "\
$ add1 = (a) {
    ^ a + 1
}
^ add1(41)
",
        );
        let opts = AnalyzeOptions {
            use_cache: false,
            ..Default::default()
        };
        let product = analyze(&path, &opts);
        assert!(product.is_ok(), "{:?}", product.diagnostics.items());

        let mut names = std::collections::HashSet::new();
        for opt in [
            OptLevel::O0,
            OptLevel::O1,
            OptLevel::O2,
            OptLevel::O3,
            OptLevel::Oz,
        ] {
            let key = codegen_ir_cache_key(&product, opt);
            assert!(
                names.insert(key.blob_name()),
                "opt {opt} collided with another level's IR cache key"
            );
        }
        // Explicit O2 vs Oz non-collision (size vs speed).
        let k2 = codegen_ir_cache_key(&product, OptLevel::O2);
        let kz = codegen_ir_cache_key(&product, OptLevel::Oz);
        assert_ne!(k2.blob_name(), kz.blob_name());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn all_opt_levels_compile_end_to_end() {
        let (root, path) = temp_echo(
            "\
$ add1 = (a) {
    ^ a + 1
}
^ add1(41)
",
        );
        let opts = AnalyzeOptions {
            use_cache: false,
            ..Default::default()
        };
        for opt in [
            OptLevel::O0,
            OptLevel::O1,
            OptLevel::O2,
            OptLevel::O3,
            OptLevel::Oz,
        ] {
            let c = compile_to_llvm_with(&path, &opts, opt);
            assert_eq!(
                c.diagnostics.error_count(),
                0,
                "opt={opt}: {:?}",
                c.diagnostics.items()
            );
            assert!(
                c.ir.contains("echo_entry") || c.ir.contains("define"),
                "opt={opt}: missing IR"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ir_cache_key_includes_codegen_and_abi() {
        let (root, path) = temp_echo("^ 1\n");
        let product = analyze(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(product.is_ok());
        let k = codegen_ir_cache_key(&product, OptLevel::O0);
        // Blob name must be stable for same inputs and distinct across opt.
        let k2 = codegen_ir_cache_key(&product, OptLevel::O2);
        assert_ne!(k.blob_name(), k2.blob_name());
        assert_eq!(
            codegen_ir_cache_key(&product, OptLevel::O0).blob_name(),
            k.blob_name()
        );
        // Codegen phase stack must include MIR (for-in/SSA fixes invalidate IR).
        use echo_fingerprint::{ArtifactPhase, CompilerComponent, phase_components};
        assert!(
            phase_components(ArtifactPhase::Codegen).contains(&CompilerComponent::MirLowerer),
            "IR cache phase must fingerprint mir_lowerer"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ir_cache_hit_then_versioned_key_differs_from_empty_extras() {
        // `PhaseCacheKey::for_source(Codegen, graph, &[])` still embeds the full
        // component stack; opt/graph extras only further specialize.
        use echo_cache::PhaseCacheKey;
        use echo_fingerprint::ArtifactPhase;
        let (root, path) = temp_echo("^ 7\n");
        let product = analyze(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(product.is_ok());
        let full = codegen_ir_cache_key(&product, OptLevel::O0);
        let bare = PhaseCacheKey::for_source(
            ArtifactPhase::Codegen,
            b"same-source-bytes-not-used-as-graph",
            &[],
        );
        // Different source material → different blob names (sanity).
        assert_ne!(full.blob_name(), bare.blob_name());
        // Full key is stable.
        assert_eq!(
            codegen_ir_cache_key(&product, OptLevel::O0).blob_name(),
            full.blob_name()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn o0_and_o2_cache_do_not_collide() {
        let (root, path) = temp_echo(
            "\
$ add1 = (a) {
    ^ a + 1
}
^ add1(1)
",
        );
        let opts = AnalyzeOptions {
            use_cache: true,
            overlays: HashMap::new(),
        };
        let o0a = compile_to_llvm_with(&path, &opts, OptLevel::O0);
        assert!(!o0a.ir_cache_hit);
        let o0b = compile_to_llvm_with(&path, &opts, OptLevel::O0);
        assert!(o0b.ir_cache_hit, "second O0 should hit");
        let o2 = compile_to_llvm_with(&path, &opts, OptLevel::O2);
        assert!(!o2.ir_cache_hit, "O2 must not reuse O0 IR cache entry");
        let o2b = compile_to_llvm_with(&path, &opts, OptLevel::O2);
        assert!(o2b.ir_cache_hit, "second O2 should hit its own key");
        let oz = compile_to_llvm_with(&path, &opts, OptLevel::Oz);
        assert!(!oz.ir_cache_hit, "Oz must not reuse O2 IR cache entry");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn map_get_return_shape_is_option() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../std/collections/map.echo");
        let path = path.canonicalize().unwrap();
        let product = analyze(
            &path,
            &AnalyzeOptions {
                use_cache: false,
                ..Default::default()
            },
        );
        assert!(product.is_ok(), "{:?}", product.diagnostics.items());
        let lowered = lower_to_mir(&product).expect("mir");
        let map_get = lowered
            .program
            .functions
            .iter()
            .find(|f| f.name == "__m_map_get" && f.module_path.ends_with("map.echo"))
            .expect("map get method");
        assert_eq!(
            map_get.ret,
            echo_mir::MirRetShape::Option,
            "map.get method must be option-shaped"
        );
        // make().seed must call map.seed (Result), not hash_table.seed (Plain).
        use echo_mir::{CallTarget, MirExpr, MirStmt};
        let mut saw_map_seed = false;
        let mut saw_ht_seed_as_match = false;
        fn walk(stmts: &[MirStmt], saw_map: &mut bool, saw_ht: &mut bool) {
            for s in stmts {
                if let MirStmt::MatchTagged { scrutinee, .. } = s {
                    if let MirExpr::Call {
                        target: CallTarget::Function { name, module_path },
                        ret,
                        ..
                    } = scrutinee
                    {
                        if name.contains("seed") {
                            if module_path.ends_with("map.echo") {
                                *saw_map = true;
                                assert!(
                                    ret.is_tagged(),
                                    "map.seed match must be tagged, got {ret:?}"
                                );
                            }
                            if module_path.ends_with("hash_table.echo") {
                                *saw_ht = true;
                            }
                        }
                    }
                }
            }
        }
        for f in &lowered.program.functions {
            if f.module_path.ends_with("map.echo") {
                walk(&f.body, &mut saw_map_seed, &mut saw_ht_seed_as_match);
            }
        }
        assert!(
            saw_map_seed,
            "expected match on map.seed in map.echo suite"
        );
        assert!(
            !saw_ht_seed_as_match,
            "make().seed must not resolve to hash_table.seed"
        );
    }
}

