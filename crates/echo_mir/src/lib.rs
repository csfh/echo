//! Backend-neutral executable IR.
//!
//! Return *shapes* (plain / result / option) come from HIR, which inferred them
//! from `^` / `!` surface syntax — not from user type names.

#![forbid(unsafe_code)]

mod cfg;
mod escape;
mod lifetime;
mod repr;
mod simplify;
mod ssa;
mod value_class;

pub use cfg::{
    match_payload_names, structured_to_cfg, structured_to_cfg_with_fallthrough, BlockId, MirBlock,
    MirCfg, MirOp, Terminator,
};
pub use escape::{analyze_escapes, EscapeClass};
pub use lifetime::{expr_is_fresh_alloc, expr_is_managed, inject_lifetime, ROOT_SCOPE};
pub use repr::{analyze_reprs, MirRepr};
pub use simplify::simplify_local;
pub use ssa::construct_ssa;
pub use value_class::ValueClass;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use echo_ast::{BinaryOp, StringKind, UnaryOp, Width};
use echo_diagnostics::{Diagnostic, Diagnostics};
use echo_hir::{
    HirBody, HirExpr, HirExprKind, HirLoopKind, HirMatchArm, HirModule, HirStmt, RECV_PARAM,
};
use echo_semantics::{ConstValue, ReturnShape, SemanticModel};
use echo_std::{is_runtime_module_path, runtime_native_symbol};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// How a function packages its return (syntax-driven shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirRetShape {
    Plain,
    Result,
    Option,
}

impl MirRetShape {
    #[must_use]
    pub fn from_return_shape(s: ReturnShape) -> Self {
        match s {
            ReturnShape::Plain => Self::Plain,
            ReturnShape::Result | ReturnShape::ResultOption => Self::Result,
            ReturnShape::Option => Self::Option,
        }
    }

    #[must_use]
    pub fn is_tagged(self) -> bool {
        matches!(self, Self::Result | Self::Option)
    }
}

pub const TAG_OK: i64 = 0;
pub const TAG_ERR: i64 = 1;
pub const TAG_SOME: i64 = 0;
pub const TAG_NONE: i64 = 1;

/// Prefix for synthetic zero-arg getters of exported/module-level values.
pub const VAL_GETTER_PREFIX: &str = "__val_";

/// Stable LLVM / link name for a free function in a module.
///
/// Module identity is **project-relative** (path under nearest `Cargo.toml` /
/// `.git` ancestor), not the absolute host path — so IR and binaries do not
/// embed `/home/…` layout. Outside a project root, falls back to
/// `parent/file` only (still no full absolute path).
#[must_use]
pub fn mangle_fn(module_path: &Path, name: &str) -> String {
    if is_runtime_module_path(module_path) {
        return format!("__echo_runtime_export_{name}");
    }
    // No special names: `main` is an ordinary identifier, not an entry keyword.
    let key = path_key(module_path);
    format!("m_{key}_{name}")
}

/// Getter name for a module-level value bind (`$ answer = 42` → `__val_answer`).
#[must_use]
pub fn value_getter_name(export: &str) -> String {
    format!("{VAL_GETTER_PREFIX}{export}")
}

/// Sanitize a relative module path into a single LLVM-safe identifier segment.
fn sanitize_path_key(rel: &str) -> String {
    rel.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Module path → mangle key: prefer relative to project root; never bake the
/// full absolute user path into symbols.
fn path_key(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let rel = strip_to_project_relative(&abs)
        .or_else(|| short_path_fallback(&abs))
        .unwrap_or_else(|| {
            abs.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "mod".into())
        });
    sanitize_path_key(&rel)
}

/// Path relative to nearest ancestor containing `Cargo.toml` or `.git`.
fn strip_to_project_relative(path: &Path) -> Option<String> {
    let mut dir = path.parent()?.to_path_buf();
    loop {
        if dir.join("Cargo.toml").is_file() || dir.join(".git").exists() {
            let rel = path.strip_prefix(&dir).ok()?;
            return Some(rel.to_string_lossy().replace('\\', "/"));
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// When no project root is found: `parent_name/file_name` only.
fn short_path_fallback(path: &Path) -> Option<String> {
    let file = path.file_name()?.to_string_lossy();
    let parent = path.parent()?.file_name()?.to_string_lossy();
    Some(format!("{parent}/{file}"))
}

#[derive(Debug, Clone)]
pub struct MirProgram {
    pub functions: Vec<MirFn>,
    /// Module path of the program entry file.
    pub entry_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MirFn {
    pub module_path: PathBuf,
    pub name: String,
    pub params: Vec<String>,
    /// Structured body (HIR lower; kept for tests / debug).
    pub body: Vec<MirStmt>,
    /// SSA CFG — authority for codegen (`construct_ssa` + repr analysis).
    pub cfg: MirCfg,
    /// SSA name → proven representation (native vs boxed).
    pub reprs: HashMap<String, MirRepr>,
    /// Local escape classification for SSA names (allocations / boxes).
    pub escapes: HashMap<String, EscapeClass>,
    pub ret: MirRetShape,
    /// Checker kind labels for params/locals (`ValueKind::as_di_label`).
    pub debug_vars: Vec<(String, String)>,
}

impl MirFn {
    #[must_use]
    pub fn mangled_name(&self) -> String {
        mangle_fn(&self.module_path, &self.name)
    }
}

#[derive(Debug, Clone)]
pub enum MirStmt {
    Set {
        name: String,
        value: MirExpr,
        span: Option<echo_source::Span>,
    },
    /// ADR 0016: push ownership scope.
    ScopeEnter {
        id: u32,
    },
    /// ADR 0016: pop scope and release remaining owned values.
    ScopeExit {
        id: u32,
    },
    /// ADR 0016: register managed handle as owned by current scope.
    ScopeRegister {
        value: MirExpr,
    },
    /// ADR 0016: transfer ownership to an open outer (or equal) scope.
    ScopePromote {
        value: MirExpr,
        target: u32,
    },
    /// ADR 0016: drop ownership without free (return / transfer).
    ScopeDisown {
        value: MirExpr,
    },
    /// ADR 0016: logical release of one value.
    ScopeRelease {
        value: MirExpr,
    },
    ReturnOk(MirExpr, Option<echo_source::Span>),
    ReturnErr(MirExpr),
    ReturnNone,
    If {
        arms: Vec<(MirExpr, Vec<MirStmt>)>,
        else_body: Option<Vec<MirStmt>>,
    },
    MatchTagged {
        scrutinee: MirExpr,
        ok_name: Option<String>,
        ok_body: Vec<MirStmt>,
        err_name: Option<String>,
        err_body: Vec<MirStmt>,
    },
    Loop {
        cond: Option<MirExpr>,
        body: Vec<MirStmt>,
    },
    ForIn {
        item: String,
        iter: MirExpr,
        body: Vec<MirStmt>,
    },
    Break,
    Continue,
    Eval(MirExpr),
    /// `~ base.field = value`
    FieldSet {
        base: MirExpr,
        field: String,
        value: MirExpr,
    },
    /// `~ base[index] = value`
    IndexSet {
        base: MirExpr,
        index: MirExpr,
        value: MirExpr,
    },
    /// `~ base[] = value` — list append (runtime list_push).
    ListPush {
        base: MirExpr,
        value: MirExpr,
    },
    /// `+` — schedule closed body on the mio event loop; optional handle bind.
    TaskSpawn {
        module_path: PathBuf,
        body_symbol: String,
        bind: Option<String>,
    },
    /// `+ f(args)` — schedule free function with args.
    TaskSpawnFn {
        module_path: PathBuf,
        fn_symbol: String,
        args: Vec<MirExpr>,
        bind: Option<String>,
    },
    /// `-` immediate block (`body_symbol`) or join `handle`; optional result bind.
    TaskJoin {
        module_path: PathBuf,
        body_symbol: Option<String>,
        handle: Option<MirExpr>,
        bind: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum CallTarget {
    /// Free function in some module (`module_path`, `name` = body symbol).
    Function { module_path: PathBuf, name: String },
    /// Privileged `runtime.export` → native `echo_runtime_*`.
    Runtime { export: String },
    /// Call through a function **value** (`i64` code pointer).
    Indirect { callee: Box<MirExpr> },
}

/// Backend / CFG primitives not present as user-level calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirPrim {
    /// `echo_runtime_iter_len(handle) -> i64` — list or range.
    ListLen,
    /// Bounds-checked element load (`echo_runtime_iter_get`) — list or range.
    ListGetChecked,
}

#[derive(Debug, Clone)]
pub enum MirExpr {
    ConstI64(i64),
    /// Width-tagged `<i32>…` literal (native i32 until boxed).
    ConstI32(i32),
    /// Other integer widths (`i8`/`i16`/`ui*`) as native scalars until boxed.
    ConstInt {
        value: i64,
        width: echo_ast::Width,
    },
    /// Explicit integer/float width convert.
    Cast {
        to: echo_ast::Width,
        expr: Box<MirExpr>,
    },
    ConstBool(bool),
    ConstF64(f64),
    /// Width-tagged `<f32>…` literal (native f32 until boxed).
    ConstF32(f32),
    /// Duration literal / value as **nanoseconds** (native i64 until boxed).
    ConstDuration(i64),
    Name(String),
    Unary {
        op: UnaryOp,
        expr: Box<MirExpr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<MirExpr>,
        right: Box<MirExpr>,
    },
    Call {
        target: CallTarget,
        args: Vec<MirExpr>,
        ret: MirRetShape,
    },
    /// Closed function value: body symbol in this module (runtime = code pointer).
    FnValue {
        module_path: PathBuf,
        symbol: String,
    },
    /// Inclusive integer range `start..end` (ends inclusive when start ≤ end).
    Range {
        start: Box<MirExpr>,
        end: Box<MirExpr>,
    },
    /// Runtime/codegen primitive (CFG expansion of for-in, etc.).
    PrimCall {
        prim: MirPrim,
        args: Vec<MirExpr>,
    },
    ListLit(Vec<MirExpr>),
    /// UTF-8 string value (decoded contents; pure or rich without live names).
    StringLit {
        /// Decoded UTF-8 payload (no surrounding quotes).
        bytes: Vec<u8>,
    },
    /// Bytes value from `b'…'` / `b"…"` (decoded payload; not a string).
    BytesLit {
        bytes: Vec<u8>,
    },
    /// Rich bytes `b"…{name}…"` with live interpolation parts.
    BytesInterp {
        parts: Vec<StrPart>,
    },
    /// Locator value from `p'…'` / `p"…"` (decoded UTF-8 path/URI text).
    LocatorLit {
        text: String,
    },
    /// Rich locator `p"…{name}…"` with live interpolation parts.
    LocatorInterp {
        parts: Vec<StrPart>,
    },
    /// Rich string with `{name}` interpolation parts.
    StringInterp {
        parts: Vec<StrPart>,
    },
    Index {
        base: Box<MirExpr>,
        index: Box<MirExpr>,
    },
    /// Struct literal (fields set by name at runtime).
    /// `type_name` is the `% Shape` tag when non-empty; empty for anonymous.
    StructLit {
        type_name: String,
        fields: Vec<(String, MirExpr)>,
    },
    /// Runtime type-tag test: `echo_runtime_struct_type_is(value, type_name)`.
    StructTypeIs {
        value: Box<MirExpr>,
        type_name: String,
    },
    /// Value field read `base.field`.
    FieldGet {
        base: Box<MirExpr>,
        field: String,
    },
    /// Native / ref → universal runtime Echo value (`i64` ABI bits).
    BoxValue {
        value: Box<MirExpr>,
        from: MirRepr,
    },
    /// Universal runtime value → proven native / ref.
    UnboxValue {
        value: Box<MirExpr>,
        to: MirRepr,
    },
}

/// One piece of a rich interpolated string.
#[derive(Debug, Clone)]
pub enum StrPart {
    Lit(Vec<u8>),
    /// Local / const name evaluated as value then stringified.
    Name(String),
}

#[derive(Debug)]
pub struct LoweredProgram {
    pub program: MirProgram,
    pub diagnostics: Diagnostics,
}

/// One module's HIR plus import map (bind name → resolved path).
pub struct ModuleLowerInput {
    pub path: PathBuf,
    pub hir: HirModule,
    /// Analysis facts: struct typing, binds — MIR must not invent these.
    pub semantic: SemanticModel,
    /// Import bind name → **module root** (file or directory for folder modules).
    pub imports: HashMap<String, PathBuf>,
    /// Names listed in `\\ export` (for cross-module value getters only).
    pub exports: Vec<String>,
}

/// Resolve an import target root + export name to the defining `.echo` file path.
///
/// Folder modules (`std/net/tcp/`) map the import to a directory; free functions
/// live in a sibling file (e.g. `socket.echo`). Codegen mangles with the **file** path.
fn resolve_export_file(
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
    module_root: &Path,
    name: &str,
) -> PathBuf {
    let root = module_root.to_path_buf();
    if fn_shapes.contains_key(&(root.clone(), name.to_string())) {
        return root;
    }
    let getter = value_getter_name(name);
    if fn_shapes.contains_key(&(root.clone(), getter.clone())) {
        return root;
    }
    // Folder module: shapes are keyed by each file path under the directory.
    for (path, n) in fn_shapes.keys() {
        if n != name && *n != getter {
            continue;
        }
        if path == module_root
            || path.parent() == Some(module_root)
            || path.starts_with(module_root)
        {
            return path.clone();
        }
    }
    root
}

/// Method resolution target after `%` / `@` merge.
#[derive(Debug, Clone)]
struct MethodTarget {
    module: PathBuf,
    mangled: String,
    /// Method body only returns the receiver (`.`) — result keeps struct type.
    returns_receiver: bool,
    /// Named struct types of valued returns (monomorphic when len == 1).
    returns_structs: Vec<String>,
}

/// Graph-wide method table after `%` / `@` merge:
/// `struct_name` → `method_name` → target.
///
/// Built from every module’s HIR method extraction so calls in any file resolve
/// methods contributed by `%` or `@` in any file of the closed graph.
type GraphMethods = HashMap<String, HashMap<String, MethodTarget>>;

/// Graph-wide data fields: `struct` → field → optional default HIR expr.
type GraphFields = HashMap<String, HashMap<String, Option<echo_hir::HirExpr>>>;

/// `struct` → field → monomorphic named-struct type (from lit inits / defaults).
type GraphFieldTypes = HashMap<String, HashMap<String, String>>;

fn build_graph_methods(modules: &[ModuleLowerInput]) -> GraphMethods {
    let mut out: GraphMethods = HashMap::new();
    // Index method return facts by mangled name from HIR bodies.
    let mut ret_recv: HashMap<(PathBuf, String), bool> = HashMap::new();
    let mut ret_structs: HashMap<(PathBuf, String), Vec<String>> = HashMap::new();
    for m in modules {
        if is_runtime_module_path(&m.path) {
            continue;
        }
        for f in &m.hir.bodies {
            if f.receiver_struct.is_some() {
                ret_recv.insert((m.path.clone(), f.symbol.clone()), f.returns_receiver);
                ret_structs.insert(
                    (m.path.clone(), f.symbol.clone()),
                    f.returns_structs.clone(),
                );
            }
        }
        for (struct_name, methods) in &m.hir.methods {
            let slot = out.entry(struct_name.clone()).or_default();
            for (method, fname) in methods {
                // Resolver already errors on duplicate members; first definition wins.
                slot.entry(method.clone()).or_insert_with(|| {
                    let returns_receiver = ret_recv
                        .get(&(m.path.clone(), fname.clone()))
                        .copied()
                        .unwrap_or(false);
                    let returns_structs = ret_structs
                        .get(&(m.path.clone(), fname.clone()))
                        .cloned()
                        .unwrap_or_default();
                    MethodTarget {
                        module: m.path.clone(),
                        mangled: fname.clone(),
                        returns_receiver,
                        returns_structs,
                    }
                });
            }
        }
    }
    out
}

fn build_graph_fields(modules: &[ModuleLowerInput]) -> GraphFields {
    let mut out: GraphFields = HashMap::new();
    for m in modules {
        if is_runtime_module_path(&m.path) {
            continue;
        }
        for (struct_name, fields) in &m.hir.struct_fields {
            let slot = out.entry(struct_name.clone()).or_default();
            for f in fields {
                // First declaration wins (same as methods).
                slot.entry(f.name.clone())
                    .or_insert_with(|| f.default.clone());
            }
        }
    }
    out
}

/// Infer field struct types from `StructLit` initializers (and field defaults).
///
/// Enables `.table.is_empty()` when `map { table: hash_table.make() }` appears
/// in the graph — field access alone has no default type on `% map { $ table }`.
fn build_graph_field_types(
    modules: &[ModuleLowerInput],
    methods: &GraphMethods,
    fields: &GraphFields,
) -> GraphFieldTypes {
    let mut out: GraphFieldTypes = HashMap::new();
    let mut base_env: HashMap<String, String> = HashMap::new();
    for m in modules {
        if is_runtime_module_path(&m.path) {
            continue;
        }
        for f in &m.hir.bodies {
            if f.receiver_struct.is_none() && f.returns_structs.len() == 1 {
                base_env.insert(
                    format!("__fnret_{}", f.symbol),
                    f.returns_structs[0].clone(),
                );
            }
        }
    }
    fn note(out: &mut GraphFieldTypes, st: &str, field: &str, ty: String) {
        out.entry(st.to_string())
            .or_default()
            .entry(field.to_string())
            .or_insert(ty);
    }
    fn walk_expr(
        e: &HirExpr,
        methods: &GraphMethods,
        fields: &GraphFields,
        env: &HashMap<String, String>,
        out: &mut GraphFieldTypes,
    ) {
        match &e.kind {
            HirExprKind::StructLit { name, fields: flit } if !name.is_empty() => {
                for (fname, val) in flit {
                    if let Some(ty) = struct_type_of_expr(val, methods, fields, out, env) {
                        note(out, name, fname, ty);
                    }
                    walk_expr(val, methods, fields, env, out);
                }
            }
            HirExprKind::List(xs) => {
                for x in xs {
                    walk_expr(x, methods, fields, env, out);
                }
            }
            HirExprKind::Call { args, .. } | HirExprKind::ModuleCall { args, .. } => {
                for a in args {
                    walk_expr(a, methods, fields, env, out);
                }
            }
            HirExprKind::MethodCall { receiver, args, .. } => {
                walk_expr(receiver, methods, fields, env, out);
                for a in args {
                    walk_expr(a, methods, fields, env, out);
                }
            }
            HirExprKind::Field { base, .. } | HirExprKind::Unary { expr: base, .. } => {
                walk_expr(base, methods, fields, env, out);
            }
            HirExprKind::Binary { left, right, .. } => {
                walk_expr(left, methods, fields, env, out);
                walk_expr(right, methods, fields, env, out);
            }
            HirExprKind::Group(inner) => walk_expr(inner, methods, fields, env, out),
            _ => {}
        }
    }
    fn walk_stmts(
        stmts: &[HirStmt],
        methods: &GraphMethods,
        fields: &GraphFields,
        env: &HashMap<String, String>,
        out: &mut GraphFieldTypes,
    ) {
        for s in stmts {
            match s {
                HirStmt::Bind { init: Some(e), .. }
                | HirStmt::Assign { value: e, .. }
                | HirStmt::Expr(e)
                | HirStmt::Return { value: Some(e), .. }
                | HirStmt::ErrorReturn { value: e, .. } => {
                    walk_expr(e, methods, fields, env, out);
                }
                HirStmt::If {
                    arms, else_body, ..
                } => {
                    for (_, b) in arms {
                        walk_stmts(b, methods, fields, env, out);
                    }
                    if let Some(b) = else_body {
                        walk_stmts(b, methods, fields, env, out);
                    }
                }
                HirStmt::Match { arms, .. } => {
                    for arm in arms {
                        match arm {
                            HirMatchArm::Values { body, .. }
                            | HirMatchArm::Default { body }
                            | HirMatchArm::Type { body, .. }
                            | HirMatchArm::Ok { body, .. }
                            | HirMatchArm::Err { body, .. } => {
                                walk_stmts(body, methods, fields, env, out);
                            }
                        }
                    }
                }
                HirStmt::Loop { body, .. } => walk_stmts(body, methods, fields, env, out),
                _ => {}
            }
        }
    }
    // Defaults on field decls (when present).
    for (st, fmap) in fields {
        for (fname, def) in fmap {
            if let Some(d) = def {
                if let Some(ty) = struct_type_of_expr(d, methods, fields, &out, &base_env) {
                    note(&mut out, st, fname, ty);
                }
            }
        }
    }
    for m in modules {
        if is_runtime_module_path(&m.path) {
            continue;
        }
        let mut env = base_env.clone();
        for (import_name, root) in &m.imports {
            for dep in modules.iter().filter(|d| {
                &d.path == root
                    || d.path.parent() == Some(root.as_path())
                    || d.path.starts_with(root)
            }) {
                for f in &dep.hir.bodies {
                    if f.receiver_struct.is_none()
                        && f.returns_structs.len() == 1
                        && dep.exports.iter().any(|e| e == &f.symbol)
                    {
                        env.insert(
                            format!("__fnret_{import_name}.{}", f.symbol),
                            f.returns_structs[0].clone(),
                        );
                    }
                }
            }
        }
        walk_stmts(&m.hir.entry, methods, fields, &env, &mut out);
        for f in &m.hir.bodies {
            walk_stmts(&f.body, methods, fields, &env, &mut out);
        }
    }
    out
}

/// Lower a whole program (all modules with free functions).
#[must_use]
pub fn lower_program(entry_path: PathBuf, modules: &[ModuleLowerInput]) -> LoweredProgram {
    let mut diagnostics = Diagnostics::new();
    let mut fn_shapes: HashMap<(PathBuf, String), MirRetShape> = HashMap::new();
    let graph_methods = build_graph_methods(modules);
    let graph_fields = build_graph_fields(modules);
    let graph_field_types = build_graph_field_types(modules, &graph_methods, &graph_fields);
    let free_fn_param_structs =
        collect_free_fn_param_structs(modules, &graph_methods, &graph_fields, &graph_field_types);

    for m in modules {
        for f in &m.hir.bodies {
            fn_shapes.insert(
                (m.path.clone(), f.symbol.clone()),
                MirRetShape::from_return_shape(f.return_shape),
            );
        }
        seed_fn_value_shapes(&m.hir.entry, &m.path, &mut fn_shapes);
        for f in &m.hir.bodies {
            seed_fn_value_shapes(&f.body, &m.path, &mut fn_shapes);
        }
    }

    // Register synthetic value getters only for *exported* value binds
    // (cross-module `module.name`). Non-exported top-level binds live only in
    // `__toplevel` and must not get freestanding getters that re-eval them.
    for m in modules {
        if is_runtime_module_path(&m.path) {
            continue;
        }
        for stmt in &m.hir.entry {
            if let HirStmt::Bind { name, .. } = stmt {
                if m.exports.iter().any(|e| e == name) {
                    fn_shapes.insert(
                        (m.path.clone(), value_getter_name(name)),
                        MirRetShape::Plain,
                    );
                }
            }
        }
    }

    let mut functions = Vec::new();
    for m in modules {
        if is_runtime_module_path(&m.path) {
            continue;
        }
        // File-level `#` constants (and other foldable top-level values for getters).
        let mut const_env: HashMap<String, ConstValue> = HashMap::new();
        for stmt in &m.hir.entry {
            if let HirStmt::Bind {
                leader: echo_ast::BindLeader::Hash,
                name,
                init: Some(init),
                ..
            } = stmt
            {
                if let Some(v) = fold_hir_const(init, &const_env) {
                    const_env.insert(
                        name.clone(),
                        fill_const_struct_defaults(v, &graph_fields, &const_env),
                    );
                }
            }
        }

        for f in &m.hir.bodies {
            let ret = MirRetShape::from_return_shape(f.return_shape);
            // Seed only from analysis SemanticModel; flow-sensitive copies
            // inside lower_* never invent struct names without a StructLit or
            // a prior analysis-known name.
            let mut type_env = struct_env_from_semantic(&m.semantic);
            seed_fn_return_struct_types(&mut type_env, m, modules);
            if let Some(st) = &f.receiver_struct {
                type_env.insert(recv_param(), st.clone());
            } else if let Some(param_tys) = free_fn_param_structs.get(&f.symbol) {
                // Monomorphic free-fn params from call sites (named-struct only).
                for (p, ty) in f.params.iter().zip(param_tys.iter()) {
                    if let Some(st) = ty {
                        type_env.insert(p.clone(), st.clone());
                    }
                }
            }
            let body = lower_block(
                &f.body,
                ret,
                &m.path,
                &m.imports,
                &fn_shapes,
                &const_env,
                &graph_methods,
                &graph_fields,
                &graph_field_types,
                &mut type_env,
                &mut diagnostics,
            );
            // ADR 0016: scope enter/exit/register/promote (conservative slice 1).
            let body = inject_lifetime(body);
            // Plain methods: fall-off returns the receiver (locked).
            let fallthrough =
                if f.receiver_struct.is_some() && matches!(f.return_shape, ReturnShape::Plain) {
                    MirExpr::Name(RECV_PARAM.into())
                } else {
                    MirExpr::ConstI64(0)
                };
            let (cfg, reprs, escapes) = finish_cfg(body.clone(), ret, &f.params, fallthrough);
            let debug_vars = debug_vars_from_semantic(&f.params, &body, &m.semantic);
            functions.push(MirFn {
                module_path: m.path.clone(),
                name: f.symbol.clone(),
                params: f.params.clone(),
                body,
                cfg,
                reprs,
                escapes,
                ret,
                debug_vars,
            });
        }
        // Exported value binds → zero-arg getters for `module.name` field access.
        for stmt in &m.hir.entry {
            if let HirStmt::Bind {
                leader, name, init, ..
            } = stmt
            {
                if !m.exports.iter().any(|e| e == name) {
                    continue;
                }
                let getter = value_getter_name(name);
                let mut type_env: HashMap<String, String> = HashMap::new();
                let value = if *leader == echo_ast::BindLeader::Hash {
                    if let Some(v) = const_env.get(name) {
                        const_to_mir(v)
                    } else if let Some(e) = init {
                        match lower_expr(
                            e,
                            &m.path,
                            &m.imports,
                            &fn_shapes,
                            &const_env,
                            &graph_methods,
                            &graph_fields,
                            &graph_field_types,
                            &mut type_env,
                            &mut diagnostics,
                        ) {
                            Some(v) => v,
                            None => continue,
                        }
                    } else {
                        MirExpr::ConstI64(0)
                    }
                } else {
                    match init {
                        Some(e) => match lower_expr(
                            e,
                            &m.path,
                            &m.imports,
                            &fn_shapes,
                            &const_env,
                            &graph_methods,
                            &graph_fields,
                            &graph_field_types,
                            &mut type_env,
                            &mut diagnostics,
                        ) {
                            Some(v) => v,
                            None => continue,
                        },
                        None => MirExpr::ConstI64(0),
                    }
                };
                let body = vec![MirStmt::ReturnOk(value, None)];
                let (cfg, reprs, escapes) =
                    finish_cfg(body.clone(), MirRetShape::Plain, &[], MirExpr::ConstI64(0));
                let debug_vars = debug_vars_from_semantic(&[], &body, &m.semantic);
                functions.push(MirFn {
                    module_path: m.path.clone(),
                    name: getter,
                    params: vec![],
                    body,
                    cfg,
                    reprs,
                    escapes,
                    ret: MirRetShape::Plain,
                    debug_vars,
                });
            }
        }
        // Entry file: top-level sequence in order (binds, calls, control).
        // Closed bodies live in `hir.bodies`; FnRef binds are language names only.
        if m.path == entry_path {
            let top: Vec<HirStmt> = m
                .hir
                .entry
                .iter()
                .filter(|s| match s {
                    HirStmt::Unsupported { message, .. } if message.is_empty() => false,
                    _ => true,
                })
                .cloned()
                .collect();
            if !top.is_empty() {
                let mut type_env = struct_env_from_semantic(&m.semantic);
                seed_fn_return_struct_types(&mut type_env, m, modules);
                let body = lower_block(
                    &top,
                    MirRetShape::Plain,
                    &m.path,
                    &m.imports,
                    &fn_shapes,
                    &const_env,
                    &graph_methods,
                    &graph_fields,
                    &graph_field_types,
                    &mut type_env,
                    &mut diagnostics,
                );
                if !body.is_empty() {
                    let body = inject_lifetime(body);
                    let (cfg, reprs, escapes) =
                        finish_cfg(body.clone(), MirRetShape::Plain, &[], MirExpr::ConstI64(0));
                    let debug_vars = debug_vars_from_semantic(&[], &body, &m.semantic);
                    functions.push(MirFn {
                        module_path: m.path.clone(),
                        name: "__toplevel".into(),
                        params: vec![],
                        body,
                        cfg,
                        reprs,
                        escapes,
                        ret: MirRetShape::Plain,
                        debug_vars,
                    });
                }
            }
        }
    }

    LoweredProgram {
        program: MirProgram {
            functions,
            entry_path,
        },
        diagnostics,
    }
}

fn recv_param() -> String {
    RECV_PARAM.into()
}

/// structured MIR → CFG → SSA → repr → simplify → escape → simplify → LLVM.
///
/// Echo-specific residual work only (representation + NoEscape box elision).
/// Generic mid-end (constprop, GVN, LICM, IV, BCE, …) is LLVM’s job
/// (see `docs/mir.md`).
fn finish_cfg(
    body: Vec<MirStmt>,
    ret: MirRetShape,
    params: &[String],
    fallthrough: MirExpr,
) -> (
    MirCfg,
    HashMap<String, MirRepr>,
    HashMap<String, EscapeClass>,
) {
    let cfg = structured_to_cfg_with_fallthrough(&body, ret, fallthrough);
    let cfg = construct_ssa(cfg, params);
    let (cfg, reprs) = analyze_reprs(cfg, params);
    let (cfg, reprs) = simplify_local(cfg, reprs);
    let (cfg, reprs, escapes) = analyze_escapes(cfg, reprs);
    let (cfg, reprs) = simplify_local(cfg, reprs);
    (cfg, reprs, escapes)
}

fn debug_vars_from_semantic(
    params: &[String],
    body: &[MirStmt],
    semantic: &SemanticModel,
) -> Vec<(String, String)> {
    let mut names: HashSet<String> = params.iter().cloned().collect();
    collect_debug_names(body, &mut names);
    let mut out: Vec<(String, String)> = names
        .into_iter()
        .filter_map(|n| {
            semantic
                .binds
                .get(&n)
                .map(|b| (n, b.value_kind.as_di_label().to_string()))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn collect_debug_names(body: &[MirStmt], names: &mut HashSet<String>) {
    for s in body {
        match s {
            MirStmt::Set { name, .. } => {
                names.insert(name.clone());
            }
            MirStmt::If { arms, else_body } => {
                for (_, b) in arms {
                    collect_debug_names(b, names);
                }
                if let Some(b) = else_body {
                    collect_debug_names(b, names);
                }
            }
            MirStmt::MatchTagged {
                ok_name,
                ok_body,
                err_name,
                err_body,
                ..
            } => {
                if let Some(n) = ok_name {
                    names.insert(n.clone());
                }
                if let Some(n) = err_name {
                    names.insert(n.clone());
                }
                collect_debug_names(ok_body, names);
                collect_debug_names(err_body, names);
            }
            MirStmt::Loop { body, .. } | MirStmt::ForIn { body, .. } => {
                collect_debug_names(body, names);
            }
            _ => {}
        }
    }
}

/// Projection of analysis `value_struct` facts for method/receiver lowering.
/// MIR must not invent struct names outside flow of StructLit / copy from known names.
fn struct_env_from_semantic(semantic: &SemanticModel) -> HashMap<String, String> {
    semantic.value_struct.clone()
}

/// Free-fn body symbol → per-param monomorphic named-struct type (when unique).
///
/// Built from call sites across the graph so `use_box(c)` seeds param `c` as
/// `% box` for method resolve inside the free-fn body (docs/stdlib.md).
type FreeFnParamStructs = HashMap<String, Vec<Option<String>>>;

/// Collect monomorphic free-fn param struct types from call sites (fixpoint).
fn collect_free_fn_param_structs(
    modules: &[ModuleLowerInput],
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
) -> FreeFnParamStructs {
    // Free-fn body symbols and arities (exclude methods).
    let mut free_arity: HashMap<String, usize> = HashMap::new();
    // Bind name → free-fn body symbol (same-module `$ f = (params) { }`).
    let mut bind_to_fn: HashMap<(PathBuf, String), String> = HashMap::new();
    for m in modules {
        if is_runtime_module_path(&m.path) {
            continue;
        }
        for f in &m.hir.bodies {
            if f.receiver_struct.is_none() {
                free_arity.insert(f.symbol.clone(), f.params.len());
            }
        }
        for stmt in &m.hir.entry {
            if let HirStmt::Bind {
                name,
                init: Some(e),
                ..
            } = stmt
            {
                if let HirExprKind::FnRef { symbol } = &e.kind {
                    bind_to_fn.insert((m.path.clone(), name.clone()), symbol.clone());
                }
            }
        }
    }

    // Candidates: fn_symbol → param_i → set of struct names seen at call sites.
    let mut candidates: HashMap<String, Vec<HashSet<String>>> = HashMap::new();
    for (sym, &arity) in &free_arity {
        candidates.insert(sym.clone(), vec![HashSet::new(); arity]);
    }

    // Resolve call target symbol to free-fn body id.
    let resolve_callee =
        |module_path: &Path, symbol: &str, type_env: &HashMap<String, String>| -> Option<String> {
            if free_arity.contains_key(symbol) {
                return Some(symbol.to_string());
            }
            if let Some(s) = bind_to_fn.get(&(module_path.to_path_buf(), symbol.to_string())) {
                return Some(s.clone());
            }
            // Local bind typed as holding a function: use bind_to_fn via name only if unique.
            let _ = type_env;
            None
        };

    // Record arg struct types for a free-fn call.
    let record_call = |candidates: &mut HashMap<String, Vec<HashSet<String>>>,
                       fn_sym: &str,
                       args: &[HirExpr],
                       type_env: &HashMap<String, String>| {
        let Some(slots) = candidates.get_mut(fn_sym) else {
            return;
        };
        for (i, arg) in args.iter().enumerate() {
            if i >= slots.len() {
                break;
            }
            if let Some(st) = struct_type_of_expr(arg, methods, fields, field_types, type_env) {
                slots[i].insert(st);
            }
        }
    };

    // Fixpoint: seed envs from semantic + known monomorphic params; walk all bodies/entry.
    for _ in 0..8 {
        let mut monomorphic: FreeFnParamStructs = HashMap::new();
        for (sym, slots) in &candidates {
            let mapped: Vec<Option<String>> = slots
                .iter()
                .map(|set| {
                    if set.len() == 1 {
                        set.iter().next().cloned()
                    } else {
                        None
                    }
                })
                .collect();
            monomorphic.insert(sym.clone(), mapped);
        }

        let before = format!("{candidates:?}");

        for m in modules {
            if is_runtime_module_path(&m.path) {
                continue;
            }
            // Top-level / entry: semantic env + return seeds + sequential bind flow
            // so `$ s = lis.accept(); drain(s)` sees `s` as `% conn`.
            let mut type_env = struct_env_from_semantic(&m.semantic);
            seed_fn_return_struct_types(&mut type_env, m, modules);
            seed_local_struct_flow(&m.hir.entry, methods, fields, field_types, &mut type_env);
            collect_calls_in_stmts(
                &m.hir.entry,
                &m.path,
                &type_env,
                methods,
                &resolve_callee,
                &mut candidates,
                record_call,
            );

            // Free-fn and method bodies.
            for f in &m.hir.bodies {
                let mut env = struct_env_from_semantic(&m.semantic);
                seed_fn_return_struct_types(&mut env, m, modules);
                if let Some(st) = &f.receiver_struct {
                    env.insert(recv_param(), st.clone());
                } else if let Some(param_tys) = monomorphic.get(&f.symbol) {
                    for (p, ty) in f.params.iter().zip(param_tys.iter()) {
                        if let Some(st) = ty {
                            env.insert(p.clone(), st.clone());
                        }
                    }
                }
                // Flow assigns inside the body for nested calls (light pass).
                seed_local_struct_flow(&f.body, methods, fields, field_types, &mut env);
                collect_calls_in_stmts(
                    &f.body,
                    &m.path,
                    &env,
                    methods,
                    &resolve_callee,
                    &mut candidates,
                    record_call,
                );
            }
        }

        let after = format!("{candidates:?}");
        if before == after {
            break;
        }
    }

    let mut out = FreeFnParamStructs::new();
    for (sym, slots) in candidates {
        let mapped: Vec<Option<String>> = slots
            .iter()
            .map(|set| {
                if set.len() == 1 {
                    set.iter().next().cloned()
                } else {
                    None
                }
            })
            .collect();
        if mapped.iter().any(|t| t.is_some()) {
            out.insert(sym, mapped);
        }
    }
    out
}

/// Light bind/assign flow so nested free-fn calls see local struct types.
fn seed_local_struct_flow(
    stmts: &[HirStmt],
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
) {
    for s in stmts {
        match s {
            HirStmt::Bind {
                name,
                init: Some(e),
                ..
            }
            | HirStmt::Assign { name, value: e, .. } => {
                propagate_struct_type(name, e, methods, fields, field_types, type_env);
            }
            HirStmt::If {
                arms, else_body, ..
            } => {
                for (_, body) in arms {
                    seed_local_struct_flow(body, methods, fields, field_types, type_env);
                }
                if let Some(b) = else_body {
                    seed_local_struct_flow(b, methods, fields, field_types, type_env);
                }
            }
            HirStmt::Match { arms, .. } => {
                for arm in arms {
                    match arm {
                        HirMatchArm::Values { body, .. }
                        | HirMatchArm::Default { body }
                        | HirMatchArm::Type { body, .. }
                        | HirMatchArm::Ok { body, .. }
                        | HirMatchArm::Err { body, .. } => {
                            seed_local_struct_flow(body, methods, fields, field_types, type_env);
                        }
                    }
                }
            }
            HirStmt::Loop { body, .. } => {
                seed_local_struct_flow(body, methods, fields, field_types, type_env)
            }
            _ => {}
        }
    }
}

fn collect_calls_in_stmts(
    stmts: &[HirStmt],
    module_path: &Path,
    type_env: &HashMap<String, String>,
    methods: &GraphMethods,
    resolve_callee: &dyn Fn(&Path, &str, &HashMap<String, String>) -> Option<String>,
    candidates: &mut HashMap<String, Vec<HashSet<String>>>,
    record_call: impl Fn(&mut HashMap<String, Vec<HashSet<String>>>, &str, &[HirExpr], &HashMap<String, String>)
        + Copy,
) {
    for s in stmts {
        match s {
            HirStmt::Bind { init: Some(e), .. }
            | HirStmt::Assign { value: e, .. }
            | HirStmt::Expr(e)
            | HirStmt::Return { value: Some(e), .. }
            | HirStmt::ErrorReturn { value: e, .. } => {
                collect_calls_in_expr(
                    e,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
            HirStmt::FieldAssign { base, value, .. } => {
                collect_calls_in_expr(
                    base,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
                collect_calls_in_expr(
                    value,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
            HirStmt::IndexAssign {
                base, index, value, ..
            } => {
                collect_calls_in_expr(
                    base,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
                if let Some(index) = index {
                    collect_calls_in_expr(
                        index,
                        module_path,
                        type_env,
                        methods,
                        resolve_callee,
                        candidates,
                        record_call,
                    );
                }
                collect_calls_in_expr(
                    value,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
            HirStmt::If {
                arms, else_body, ..
            } => {
                for (cond, body) in arms {
                    collect_calls_in_expr(
                        cond,
                        module_path,
                        type_env,
                        methods,
                        resolve_callee,
                        candidates,
                        record_call,
                    );
                    collect_calls_in_stmts(
                        body,
                        module_path,
                        type_env,
                        methods,
                        resolve_callee,
                        candidates,
                        record_call,
                    );
                }
                if let Some(b) = else_body {
                    collect_calls_in_stmts(
                        b,
                        module_path,
                        type_env,
                        methods,
                        resolve_callee,
                        candidates,
                        record_call,
                    );
                }
            }
            HirStmt::Match {
                scrutinee, arms, ..
            } => {
                collect_calls_in_expr(
                    scrutinee,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
                for arm in arms {
                    match arm {
                        HirMatchArm::Values { body, .. }
                        | HirMatchArm::Default { body }
                        | HirMatchArm::Type { body, .. }
                        | HirMatchArm::Ok { body, .. }
                        | HirMatchArm::Err { body, .. } => {
                            collect_calls_in_stmts(
                                body,
                                module_path,
                                type_env,
                                methods,
                                resolve_callee,
                                candidates,
                                record_call,
                            );
                        }
                    }
                }
            }
            HirStmt::Loop { kind, body, .. } => {
                match kind {
                    HirLoopKind::While(c) => {
                        collect_calls_in_expr(
                            c,
                            module_path,
                            type_env,
                            methods,
                            resolve_callee,
                            candidates,
                            record_call,
                        );
                    }
                    HirLoopKind::For { iter, .. } => {
                        collect_calls_in_expr(
                            iter,
                            module_path,
                            type_env,
                            methods,
                            resolve_callee,
                            candidates,
                            record_call,
                        );
                    }
                    HirLoopKind::Infinite => {}
                }
                collect_calls_in_stmts(
                    body,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
            HirStmt::TaskSpawnFn {
                fn_symbol, args, ..
            } => {
                if let Some(sym) = resolve_callee(module_path, fn_symbol, type_env) {
                    record_call(candidates, &sym, args, type_env);
                }
                for a in args {
                    collect_calls_in_expr(
                        a,
                        module_path,
                        type_env,
                        methods,
                        resolve_callee,
                        candidates,
                        record_call,
                    );
                }
            }
            HirStmt::TaskJoin {
                handle: Some(h), ..
            } => {
                collect_calls_in_expr(
                    h,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
            _ => {}
        }
    }
}

fn collect_calls_in_expr(
    e: &HirExpr,
    module_path: &Path,
    type_env: &HashMap<String, String>,
    methods: &GraphMethods,
    resolve_callee: &dyn Fn(&Path, &str, &HashMap<String, String>) -> Option<String>,
    candidates: &mut HashMap<String, Vec<HashSet<String>>>,
    record_call: impl Fn(&mut HashMap<String, Vec<HashSet<String>>>, &str, &[HirExpr], &HashMap<String, String>)
        + Copy,
) {
    match &e.kind {
        HirExprKind::Call { symbol, args } => {
            if let Some(sym) = resolve_callee(module_path, symbol, type_env) {
                record_call(candidates, &sym, args, type_env);
            }
            for a in args {
                collect_calls_in_expr(
                    a,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
        }
        HirExprKind::ModuleCall { name, args, .. } => {
            // Imported free fn: name is export symbol.
            if let Some(sym) = resolve_callee(module_path, name, type_env) {
                record_call(candidates, &sym, args, type_env);
            } else {
                // Export name may equal body symbol directly.
                record_call(candidates, name, args, type_env);
            }
            for a in args {
                collect_calls_in_expr(
                    a,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
        }
        HirExprKind::MethodCall { receiver, args, .. } => {
            collect_calls_in_expr(
                receiver,
                module_path,
                type_env,
                methods,
                resolve_callee,
                candidates,
                record_call,
            );
            for a in args {
                collect_calls_in_expr(
                    a,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
        }
        HirExprKind::CallValue { callee, args } => {
            collect_calls_in_expr(
                callee,
                module_path,
                type_env,
                methods,
                resolve_callee,
                candidates,
                record_call,
            );
            for a in args {
                collect_calls_in_expr(
                    a,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
        }
        HirExprKind::Binary { left, right, .. }
        | HirExprKind::Range {
            start: left,
            end: right,
            ..
        } => {
            collect_calls_in_expr(
                left,
                module_path,
                type_env,
                methods,
                resolve_callee,
                candidates,
                record_call,
            );
            collect_calls_in_expr(
                right,
                module_path,
                type_env,
                methods,
                resolve_callee,
                candidates,
                record_call,
            );
        }
        HirExprKind::Unary { expr, .. } | HirExprKind::Group(expr) => {
            collect_calls_in_expr(
                expr,
                module_path,
                type_env,
                methods,
                resolve_callee,
                candidates,
                record_call,
            );
        }
        HirExprKind::Field { base, .. } => {
            collect_calls_in_expr(
                base,
                module_path,
                type_env,
                methods,
                resolve_callee,
                candidates,
                record_call,
            );
        }
        HirExprKind::Index { base, index } => {
            collect_calls_in_expr(
                base,
                module_path,
                type_env,
                methods,
                resolve_callee,
                candidates,
                record_call,
            );
            collect_calls_in_expr(
                index,
                module_path,
                type_env,
                methods,
                resolve_callee,
                candidates,
                record_call,
            );
        }
        HirExprKind::List(items) => {
            for it in items {
                collect_calls_in_expr(
                    it,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
        }
        HirExprKind::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_calls_in_expr(
                    v,
                    module_path,
                    type_env,
                    methods,
                    resolve_callee,
                    candidates,
                    record_call,
                );
            }
        }
        _ => {}
    }
}

/// Resolve the `%` struct type of a HIR expression when known (for method dispatch).
fn struct_type_of_expr(
    e: &HirExpr,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &HashMap<String, String>,
) -> Option<String> {
    match &e.kind {
        HirExprKind::Name(n) => type_env.get(n).cloned(),
        // Receiver `.` in methods is bound as `__recv` in type_env.
        HirExprKind::Field { base, field } => {
            let st = struct_type_of_expr(base, methods, fields, field_types, type_env)?;
            if let Some(ty) = field_types.get(&st).and_then(|m| m.get(field)) {
                return Some(ty.clone());
            }
            // Field default initializer (when present and monomorphic struct).
            let def = fields
                .get(&st)
                .and_then(|m| m.get(field))
                .and_then(|d| d.as_ref());
            if let Some(d) = def {
                if let Some(ty) = struct_type_of_expr(d, methods, fields, field_types, type_env) {
                    return Some(ty);
                }
            }
            None
        }
        HirExprKind::MethodCall {
            receiver, method, ..
        } => {
            let st = struct_type_of_expr(receiver, methods, fields, field_types, type_env)?;
            let target = methods.get(&st).and_then(|m| m.get(method))?;
            if target.returns_receiver {
                Some(st)
            } else if target.returns_structs.len() == 1 {
                Some(target.returns_structs[0].clone())
            } else {
                None
            }
        }
        // Free / nested function call (symbol is linkage or bind name).
        HirExprKind::Call { symbol, .. } => type_env.get(&format!("__fnret_{symbol}")).cloned(),
        HirExprKind::ModuleCall { module, name, .. } => type_env
            .get(&format!("__fnret_{module}.{name}"))
            .cloned()
            .or_else(|| type_env.get(&format!("__fnret_{name}")).cloned()),
        HirExprKind::CallValue { callee, .. } => {
            // Call through a function value — type usually unknown.
            let _ = callee;
            None
        }
        HirExprKind::Group(inner) => {
            struct_type_of_expr(inner, methods, fields, field_types, type_env)
        }
        HirExprKind::StructLit { name, .. } if !name.is_empty() => Some(name.clone()),
        _ => None,
    }
}

/// Width of a shape field when its default is a width-tagged scalar.
fn hir_expr_scalar_width(e: &HirExpr) -> Option<Width> {
    match &e.kind {
        HirExprKind::Int { width: Some(w), .. } => Some(*w),
        HirExprKind::WidthCast { width, .. } => Some(*width),
        HirExprKind::Group(inner) => hir_expr_scalar_width(inner),
        // Untagged int default is i64 — still a definite field width.
        HirExprKind::Int { width: None, .. } => Some(Width::I64),
        HirExprKind::Float { width: Some(w), .. } => Some(*w),
        HirExprKind::Float { width: None, .. } => Some(Width::F64),
        HirExprKind::Bool(_) => None,
        _ => None,
    }
}

/// Look up field scalar width from the receiver's monomorphic struct + default.
fn field_width_from_default(
    base: &HirExpr,
    field: &str,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &HashMap<String, String>,
) -> Option<Width> {
    let st = struct_type_of_expr(base, methods, fields, field_types, type_env)?;
    let def = fields.get(&st)?.get(field)?.as_ref()?;
    hir_expr_scalar_width(def)
}

/// Seed free-function return-struct facts for method typing after calls.
fn seed_fn_return_struct_types(
    type_env: &mut HashMap<String, String>,
    module: &ModuleLowerInput,
    all: &[ModuleLowerInput],
) {
    for f in &module.hir.bodies {
        // Monomorphic only: multi-type (union) returns refine via `%` match arms.
        if f.returns_structs.len() == 1 {
            type_env.insert(
                format!("__fnret_{}", f.symbol),
                f.returns_structs[0].clone(),
            );
        }
    }
    // Local bind names that hold FnRef often match the body symbol for top-level frees.
    for stmt in &module.hir.entry {
        if let HirStmt::Bind {
            name,
            init: Some(e),
            ..
        } = stmt
        {
            if let HirExprKind::FnRef { symbol } = &e.kind {
                if let Some(st) = type_env.get(&format!("__fnret_{symbol}")).cloned() {
                    type_env.insert(format!("__fnret_{name}"), st);
                }
            }
        }
    }
    for (import_name, root) in &module.imports {
        for dep in all.iter().filter(|m| {
            &m.path == root || m.path.parent() == Some(root.as_path()) || m.path.starts_with(root)
        }) {
            for f in &dep.hir.bodies {
                if f.returns_structs.len() == 1 {
                    let st = &f.returns_structs[0];
                    // Prefer export names: top-level free fns use symbol == bind name.
                    if dep.exports.iter().any(|e| e == &f.symbol) {
                        // Qualified only. Bare `__fnret_{symbol}` is reserved for
                        // *this* module's free fns — importing `hash_table.make`
                        // must not clobber local `make` → `map` (else
                        // `make().seed` resolves to `hash_table.seed` and
                        // result/option matches fail with cg-match).
                        type_env.insert(format!("__fnret_{import_name}.{}", f.symbol), st.clone());
                    }
                }
            }
        }
    }
}

/// After `Set`/`Assign`, keep method-bearing struct types flowing through locals.
fn propagate_struct_type(
    dest: &str,
    value: &HirExpr,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
) {
    if let Some(st) = struct_type_of_expr(value, methods, fields, field_types, type_env) {
        type_env.insert(dest.to_string(), st);
    }
    // Homogeneous list of named structs → element type for index / for-in.
    if let HirExprKind::List(items) = &value.kind {
        let mut elem: Option<String> = None;
        for it in items {
            match struct_type_of_expr(it, methods, fields, field_types, type_env) {
                Some(st) if elem.as_ref().is_none_or(|e| e == &st) => elem = Some(st),
                _ => {
                    elem = None;
                    break;
                }
            }
        }
        if let Some(st) = elem {
            type_env.insert(format!("__elem_{dest}"), st);
        }
    }
    // `xs[i]` → element type of `xs` when known.
    if let HirExprKind::Index { base, .. } = &value.kind {
        if let HirExprKind::Name(bn) = &base.kind {
            if let Some(st) = type_env.get(&format!("__elem_{bn}")).cloned() {
                type_env.insert(dest.to_string(), st);
            }
        }
    }
    // Copy list element fact when assigning a list name: `$ ys = xs`.
    if let HirExprKind::Name(src) = &value.kind {
        if let Some(st) = type_env.get(&format!("__elem_{src}")).cloned() {
            type_env.insert(format!("__elem_{dest}"), st);
        }
    }
}

fn lower_block(
    stmts: &[HirStmt],
    fn_ret: MirRetShape,
    module_path: &Path,
    imports: &HashMap<String, PathBuf>,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
    const_env: &HashMap<String, ConstValue>,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
    diags: &mut Diagnostics,
) -> Vec<MirStmt> {
    let mut out = Vec::new();
    for s in stmts {
        match s {
            HirStmt::EffectBlock { bind, body, .. } => {
                out.extend(desugar_effect_block(
                    body,
                    bind.as_deref(),
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                ));
            }
            other => {
                if let Some(m) = lower_stmt(
                    other,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                ) {
                    out.push(m);
                }
            }
        }
    }
    out
}

/// Desugar `&` effect block: auto-unwrap result/option on `$ x = fallible()`.
/// Continuation-style nesting: fail short-circuits; success continues rest.
fn desugar_effect_block(
    body: &[HirStmt],
    outer_bind: Option<&str>,
    fn_ret: MirRetShape,
    module_path: &Path,
    imports: &HashMap<String, PathBuf>,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
    const_env: &HashMap<String, ConstValue>,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
    diags: &mut Diagnostics,
) -> Vec<MirStmt> {
    desugar_effect_tail(
        body,
        outer_bind,
        fn_ret,
        module_path,
        imports,
        fn_shapes,
        const_env,
        methods,
        fields,
        field_types,
        type_env,
        diags,
        0,
    )
}

fn desugar_effect_tail(
    body: &[HirStmt],
    outer_bind: Option<&str>,
    fn_ret: MirRetShape,
    module_path: &Path,
    imports: &HashMap<String, PathBuf>,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
    const_env: &HashMap<String, ConstValue>,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
    diags: &mut Diagnostics,
    depth: u32,
) -> Vec<MirStmt> {
    if body.is_empty() {
        return Vec::new();
    }
    let (head, rest) = body.split_first().unwrap();

    // `$ name = call` with result/option shape → MatchTagged unwrap.
    if let HirStmt::Bind {
        name,
        init: Some(init),
        ..
    } = head
    {
        if let Some(shape) = hir_expr_call_shape(init, module_path, imports, fn_shapes) {
            if matches!(shape, MirRetShape::Result | MirRetShape::Option) {
                let scrut = lower_expr(
                    init,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                );
                let Some(scrut) = scrut else {
                    return Vec::new();
                };
                // Force tagged call if needed — lower_expr may use plain call.
                // MatchTagged expects i128 scrutinee for result/option.
                let rest_mir = desugar_effect_tail(
                    rest,
                    outer_bind,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                    depth + 1,
                );
                let err_tmp = format!("__eff_err_{depth}");
                let mut err_body = Vec::new();
                if let Some(ob) = outer_bind {
                    // Bind err payload (low bits) to outer name.
                    err_body.push(MirStmt::Set {
                        name: ob.to_string(),
                        value: MirExpr::Name(err_tmp.clone()),
                        span: None,
                    });
                }
                // rest not run on err
                return vec![MirStmt::MatchTagged {
                    scrutinee: scrut,
                    ok_name: Some(name.clone()),
                    ok_body: rest_mir,
                    err_name: Some(err_tmp),
                    err_body,
                }];
            }
        }
    }

    // `^ expr` inside effect block = success of block (not outer function return).
    if let HirStmt::Return { value: Some(v), .. } = head {
        let val = lower_expr(
            v,
            module_path,
            imports,
            fn_shapes,
            const_env,
            methods,
            fields,
            field_types,
            type_env,
            diags,
        );
        let Some(val) = val else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(ob) = outer_bind {
            out.push(MirStmt::Set {
                name: ob.to_string(),
                value: val,
                span: None,
            });
        } else {
            out.push(MirStmt::Eval(val));
        }
        // Drop rest after success return from block.
        return out;
    }
    if let HirStmt::Return { value: None, .. } = head {
        // bare `^` — success with no payload
        return Vec::new();
    }
    if let HirStmt::ErrorReturn { value, .. } = head {
        // explicit `! e` inside effect → treat as short-circuit err
        let val = lower_expr(
            value,
            module_path,
            imports,
            fn_shapes,
            const_env,
            methods,
            fields,
            field_types,
            type_env,
            diags,
        );
        let Some(val) = val else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(ob) = outer_bind {
            out.push(MirStmt::Set {
                name: ob.to_string(),
                value: val,
                span: None,
            });
        }
        return out;
    }

    // Ordinary statement, then continue.
    let mut out = Vec::new();
    if let Some(m) = lower_stmt(
        head,
        fn_ret,
        module_path,
        imports,
        fn_shapes,
        const_env,
        methods,
        fields,
        field_types,
        type_env,
        diags,
    ) {
        out.push(m);
    }
    out.extend(desugar_effect_tail(
        rest,
        outer_bind,
        fn_ret,
        module_path,
        imports,
        fn_shapes,
        const_env,
        methods,
        fields,
        field_types,
        type_env,
        diags,
        depth,
    ));
    out
}

/// Record bind names that hold a function value (`$ f = load`) so `&` can
/// unwrap `f()` the same way as a named free call.
fn seed_fn_value_shapes(
    stmts: &[HirStmt],
    module_path: &Path,
    fn_shapes: &mut HashMap<(PathBuf, String), MirRetShape>,
) {
    for s in stmts {
        match s {
            HirStmt::Bind {
                name,
                init: Some(init),
                ..
            } => {
                if let Some(shape) = fn_value_shape_of(init, module_path, fn_shapes) {
                    fn_shapes.insert((module_path.to_path_buf(), name.clone()), shape);
                }
            }
            HirStmt::If {
                arms, else_body, ..
            } => {
                for (_, body) in arms {
                    seed_fn_value_shapes(body, module_path, fn_shapes);
                }
                if let Some(b) = else_body {
                    seed_fn_value_shapes(b, module_path, fn_shapes);
                }
            }
            HirStmt::Match { arms, .. } => {
                for arm in arms {
                    match arm {
                        HirMatchArm::Values { body, .. }
                        | HirMatchArm::Type { body, .. }
                        | HirMatchArm::Ok { body, .. }
                        | HirMatchArm::Err { body, .. }
                        | HirMatchArm::Default { body } => {
                            seed_fn_value_shapes(body, module_path, fn_shapes);
                        }
                    }
                }
            }
            HirStmt::Loop { body, .. } | HirStmt::EffectBlock { body, .. } => {
                seed_fn_value_shapes(body, module_path, fn_shapes);
            }
            _ => {}
        }
    }
}

fn fn_value_shape_of(
    e: &HirExpr,
    module_path: &Path,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
) -> Option<MirRetShape> {
    match &e.kind {
        HirExprKind::FnRef { symbol } => fn_shapes
            .get(&(module_path.to_path_buf(), symbol.clone()))
            .copied(),
        HirExprKind::Name(n) => fn_shapes
            .get(&(module_path.to_path_buf(), n.clone()))
            .copied(),
        _ => None,
    }
}

/// If `e` is a call to a known function, return its ret shape.
fn hir_expr_call_shape(
    e: &HirExpr,
    module_path: &Path,
    imports: &HashMap<String, PathBuf>,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
) -> Option<MirRetShape> {
    match &e.kind {
        HirExprKind::Call { symbol, .. } => fn_shapes
            .get(&(module_path.to_path_buf(), symbol.clone()))
            .copied(),
        HirExprKind::ModuleCall { module, name, .. } => {
            let path = imports.get(module)?;
            fn_shapes.get(&(path.clone(), name.clone())).copied()
        }
        HirExprKind::CallValue { callee, .. } => fn_value_shape_of(callee, module_path, fn_shapes),
        _ => None,
    }
}

fn lower_stmt(
    s: &HirStmt,
    fn_ret: MirRetShape,
    module_path: &Path,
    imports: &HashMap<String, PathBuf>,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
    const_env: &HashMap<String, ConstValue>,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
    diags: &mut Diagnostics,
) -> Option<MirStmt> {
    match s {
        HirStmt::Bind {
            leader: _,
            name,
            init,
            span,
        } => {
            let value = match init {
                Some(e) => lower_expr(
                    e,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?,
                None => MirExpr::ConstI64(0),
            };
            // Propagate analysis-known named struct types through locals (handles are by-ref).
            // Empty name is a structural `{}` product — not a method-bearing type.
            if let Some(e) = init {
                propagate_struct_type(name, e, methods, fields, field_types, type_env);
            }
            Some(MirStmt::Set {
                name: name.clone(),
                value,
                span: Some(*span),
            })
        }
        HirStmt::Assign { name, value, span } => {
            let hir_value = value;
            let value = lower_expr(
                hir_value,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?;
            propagate_struct_type(name, hir_value, methods, fields, field_types, type_env);
            Some(MirStmt::Set {
                name: name.clone(),
                value,
                span: Some(*span),
            })
        }
        HirStmt::FieldAssign {
            base, field, value, ..
        } => Some(MirStmt::FieldSet {
            base: lower_expr(
                base,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?,
            field: field.clone(),
            value: lower_expr(
                value,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?,
        }),
        HirStmt::IndexAssign {
            base, index, value, ..
        } => {
            let base = lower_expr(
                base,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?;
            let value = lower_expr(
                value,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?;
            if let Some(index) = index {
                Some(MirStmt::IndexSet {
                    base,
                    index: lower_expr(
                        index,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    )?,
                    value,
                })
            } else {
                Some(MirStmt::ListPush { base, value })
            }
        }
        HirStmt::Return { value: v, span } => match (fn_ret, v) {
            (MirRetShape::Option, None) => Some(MirStmt::ReturnNone),
            (MirRetShape::Result | MirRetShape::Option, Some(e)) => Some(MirStmt::ReturnOk(
                lower_expr(
                    e,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?,
                Some(*span),
            )),
            (MirRetShape::Plain, Some(e)) => Some(MirStmt::ReturnOk(
                lower_expr(
                    e,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?,
                Some(*span),
            )),
            (MirRetShape::Plain, None) | (MirRetShape::Result, None) => {
                Some(MirStmt::ReturnOk(MirExpr::ConstI64(0), Some(*span)))
            }
        },
        HirStmt::ErrorReturn { value: e, .. } => Some(MirStmt::ReturnErr(lower_expr(
            e,
            module_path,
            imports,
            fn_shapes,
            const_env,
            methods,
            fields,
            field_types,
            type_env,
            diags,
        )?)),
        HirStmt::If {
            arms, else_body, ..
        } => {
            let mut mir_arms = Vec::new();
            for (c, body) in arms {
                mir_arms.push((
                    lower_expr(
                        c,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    )?,
                    lower_block(
                        body,
                        fn_ret,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    ),
                ));
            }
            let else_body = else_body.as_ref().map(|b| {
                lower_block(
                    b,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )
            });
            Some(MirStmt::If {
                arms: mir_arms,
                else_body,
            })
        }
        HirStmt::Match {
            scrutinee, arms, ..
        } => lower_match(
            scrutinee,
            arms,
            fn_ret,
            module_path,
            imports,
            fn_shapes,
            const_env,
            methods,
            fields,
            field_types,
            type_env,
            diags,
        ),
        HirStmt::Loop { kind, body, .. } => {
            // For-in: bind item to list element struct type when known.
            if let HirLoopKind::For { item, iter } = kind {
                if let HirExprKind::Name(bn) = &iter.kind {
                    if let Some(st) = type_env.get(&format!("__elem_{bn}")).cloned() {
                        type_env.insert(item.clone(), st);
                    }
                } else if let HirExprKind::List(items) = &iter.kind {
                    // inline list: same as propagate
                    let mut elem: Option<String> = None;
                    for it in items {
                        match struct_type_of_expr(it, methods, fields, field_types, type_env) {
                            Some(st) if elem.as_ref().is_none_or(|e| e == &st) => elem = Some(st),
                            _ => {
                                elem = None;
                                break;
                            }
                        }
                    }
                    if let Some(st) = elem {
                        type_env.insert(item.clone(), st);
                    }
                }
            }
            let body = lower_block(
                body,
                fn_ret,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            );
            match kind {
                HirLoopKind::Infinite => Some(MirStmt::Loop { cond: None, body }),
                HirLoopKind::While(c) => Some(MirStmt::Loop {
                    cond: Some(lower_expr(
                        c,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    )?),
                    body,
                }),
                HirLoopKind::For { item, iter } => Some(MirStmt::ForIn {
                    item: item.clone(),
                    iter: lower_expr(
                        iter,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    )?,
                    body,
                }),
            }
        }
        HirStmt::Break { .. } => Some(MirStmt::Break),
        HirStmt::Continue { .. } => Some(MirStmt::Continue),
        HirStmt::TaskSpawn {
            body_symbol, bind, ..
        } => Some(MirStmt::TaskSpawn {
            module_path: module_path.to_path_buf(),
            body_symbol: body_symbol.clone(),
            bind: bind.clone(),
        }),
        HirStmt::TaskSpawnFn {
            fn_symbol,
            args,
            bind,
            ..
        } => {
            let mir_args: Vec<MirExpr> = args
                .iter()
                .filter_map(|a| {
                    lower_expr(
                        a,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    )
                })
                .collect();
            if mir_args.len() != args.len() {
                return None;
            }
            Some(MirStmt::TaskSpawnFn {
                module_path: module_path.to_path_buf(),
                fn_symbol: fn_symbol.clone(),
                args: mir_args,
                bind: bind.clone(),
            })
        }
        HirStmt::TaskJoin {
            body_symbol,
            handle,
            bind,
            ..
        } => Some(MirStmt::TaskJoin {
            module_path: module_path.to_path_buf(),
            body_symbol: body_symbol.clone(),
            handle: handle.as_ref().and_then(|h| {
                lower_expr(
                    h,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )
            }),
            bind: bind.clone(),
        }),
        HirStmt::Expr(e) => Some(MirStmt::Eval(lower_expr(
            e,
            module_path,
            imports,
            fn_shapes,
            const_env,
            methods,
            fields,
            field_types,
            type_env,
            diags,
        )?)),
        HirStmt::EffectBlock { .. } => {
            // Expanded in `lower_block` via `desugar_effect_block`.
            None
        }
        HirStmt::Unsupported { message, span, .. } => {
            if message.is_empty() {
                return None;
            }
            diags.push(
                Diagnostic::error(message.clone())
                    .with_code("cg-unsupported")
                    .with_span(*span),
            );
            None
        }
    }
}

fn lower_match(
    scrutinee: &HirExpr,
    arms: &[HirMatchArm],
    fn_ret: MirRetShape,
    module_path: &Path,
    imports: &HashMap<String, PathBuf>,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
    const_env: &HashMap<String, ConstValue>,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
    diags: &mut Diagnostics,
) -> Option<MirStmt> {
    let has_values = arms.iter().any(|a| matches!(a, HirMatchArm::Values { .. }));
    let has_type = arms.iter().any(|a| matches!(a, HirMatchArm::Type { .. }));
    let has_result_option = arms
        .iter()
        .any(|a| matches!(a, HirMatchArm::Ok { .. } | HirMatchArm::Err { .. }));

    if (has_values || has_type) && has_result_option {
        diags.push(
            Diagnostic::error(
                "cannot mix value/`% type` match arms with result/option `$` / `!` arms",
            )
            .with_code("cg-match"),
        );
        return None;
    }

    // Ordinary match: lower to if/else chain (`scrutinee == v1 || …` / type_is).
    if has_values
        || has_type
        || (!has_result_option
            && arms
                .iter()
                .any(|a| matches!(a, HirMatchArm::Default { .. })))
    {
        return lower_value_match(
            scrutinee,
            arms,
            fn_ret,
            module_path,
            imports,
            fn_shapes,
            const_env,
            methods,
            fields,
            field_types,
            type_env,
            diags,
        );
    }

    let mut ok_name = None;
    let mut ok_body = Vec::new();
    let mut err_name = None;
    let mut err_body = Vec::new();
    let mut has_tagged = false;

    // Ok-arm payload keeps the scrutinee's monomorphic struct type when known
    // (e.g. `make().seed(…)` → map, so `m.put` resolves as a method).
    let ok_payload_struct = struct_type_of_expr(scrutinee, methods, fields, field_types, type_env);

    for arm in arms {
        match arm {
            HirMatchArm::Ok { name, body } => {
                has_tagged = true;
                ok_name = Some(name.clone());
                if let Some(st) = &ok_payload_struct {
                    type_env.insert(name.clone(), st.clone());
                }
                ok_body = lower_block(
                    body,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                );
                type_env.remove(name);
            }
            HirMatchArm::Err { name, body } => {
                has_tagged = true;
                err_name = Some(name.clone());
                err_body = lower_block(
                    body,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                );
            }
            HirMatchArm::Default { body } => {
                has_tagged = true;
                err_name = None;
                err_body = lower_block(
                    body,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                );
            }
            HirMatchArm::Values { .. } | HirMatchArm::Type { .. } => {
                unreachable!("handled above")
            }
        }
    }

    if !has_tagged {
        diags.push(
            Diagnostic::error("empty match not supported in codegen v1")
                .with_code("cg-unsupported"),
        );
        return None;
    }

    Some(MirStmt::MatchTagged {
        scrutinee: lower_expr(
            scrutinee,
            module_path,
            imports,
            fn_shapes,
            const_env,
            methods,
            fields,
            field_types,
            type_env,
            diags,
        )?,
        ok_name,
        ok_body,
        err_name,
        err_body,
    })
}

/// `| scrut { v1, v2 { … } … : { … } }` → if/else with `==` / `||` tests.
fn lower_value_match(
    scrutinee: &HirExpr,
    arms: &[HirMatchArm],
    fn_ret: MirRetShape,
    module_path: &Path,
    imports: &HashMap<String, PathBuf>,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
    const_env: &HashMap<String, ConstValue>,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
    diags: &mut Diagnostics,
) -> Option<MirStmt> {
    let scrut = lower_expr(
        scrutinee,
        module_path,
        imports,
        fn_shapes,
        const_env,
        methods,
        fields,
        field_types,
        type_env,
        diags,
    )?;
    let mut if_arms: Vec<(MirExpr, Vec<MirStmt>)> = Vec::new();
    let mut else_body: Option<Vec<MirStmt>> = None;

    for arm in arms {
        match arm {
            HirMatchArm::Values { pats, body } => {
                if pats.is_empty() {
                    diags.push(
                        Diagnostic::error("value match arm needs at least one expression")
                            .with_code("cg-match"),
                    );
                    return None;
                }
                let mut eqs = Vec::new();
                for pat in pats {
                    // Syntactic `lo..hi` in an arm: membership (inclusive).
                    if let HirExprKind::Range { start, end } = &pat.kind {
                        let lo = lower_expr(
                            start,
                            module_path,
                            imports,
                            fn_shapes,
                            const_env,
                            methods,
                            fields,
                            field_types,
                            type_env,
                            diags,
                        )?;
                        let hi = lower_expr(
                            end,
                            module_path,
                            imports,
                            fn_shapes,
                            const_env,
                            methods,
                            fields,
                            field_types,
                            type_env,
                            diags,
                        )?;
                        let ge = MirExpr::Binary {
                            op: BinaryOp::GtEq,
                            left: Box::new(scrut.clone()),
                            right: Box::new(lo),
                        };
                        let le = MirExpr::Binary {
                            op: BinaryOp::LtEq,
                            left: Box::new(scrut.clone()),
                            right: Box::new(hi),
                        };
                        eqs.push(MirExpr::Binary {
                            op: BinaryOp::And,
                            left: Box::new(ge),
                            right: Box::new(le),
                        });
                        continue;
                    }
                    let pat_e = lower_expr(
                        pat,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    )?;
                    eqs.push(MirExpr::Binary {
                        op: BinaryOp::Eq,
                        left: Box::new(scrut.clone()),
                        right: Box::new(pat_e),
                    });
                }
                let mut cond = eqs.remove(0);
                for eq in eqs {
                    cond = MirExpr::Binary {
                        op: BinaryOp::Or,
                        left: Box::new(cond),
                        right: Box::new(eq),
                    };
                }
                let body = lower_block(
                    body,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                );
                if_arms.push((cond, body));
            }
            HirMatchArm::Type { name, body } => {
                let cond = MirExpr::StructTypeIs {
                    value: Box::new(scrut.clone()),
                    type_name: name.clone(),
                };
                // Refine scrutinee name to this type for field/method flow in the arm.
                let refine = match &scrutinee.kind {
                    HirExprKind::Name(n) => Some(n.clone()),
                    _ => None,
                };
                let saved = refine.as_ref().and_then(|n| {
                    let prev = type_env.insert(n.clone(), name.clone());
                    Some((n.clone(), prev))
                });
                let body = lower_block(
                    body,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                );
                if let Some((n, prev)) = saved {
                    match prev {
                        Some(p) => {
                            type_env.insert(n, p);
                        }
                        None => {
                            type_env.remove(&n);
                        }
                    }
                }
                if_arms.push((cond, body));
            }
            HirMatchArm::Default { body } => {
                else_body = Some(lower_block(
                    body,
                    fn_ret,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                ));
            }
            HirMatchArm::Ok { .. } | HirMatchArm::Err { .. } => {
                diags.push(
                    Diagnostic::error("internal: tagged arm in value match").with_code("cg-match"),
                );
                return None;
            }
        }
    }

    if if_arms.is_empty() && else_body.is_none() {
        diags.push(
            Diagnostic::error("empty match not supported in codegen v1")
                .with_code("cg-unsupported"),
        );
        return None;
    }

    // Only default: treat as unconditional else body via a true if.
    if if_arms.is_empty() {
        if_arms.push((MirExpr::ConstI64(1), else_body.take().unwrap_or_default()));
        else_body = None;
    }

    Some(MirStmt::If {
        arms: if_arms,
        else_body,
    })
}

fn lower_int_const(
    value: i64,
    width: Option<echo_ast::Width>,
    span: echo_source::Span,
    diags: &mut Diagnostics,
) -> Option<MirExpr> {
    use echo_ast::Width;
    let w = width.unwrap_or(Width::I64);
    match w {
        Width::I64 => Some(MirExpr::ConstI64(value)),
        Width::I32 => {
            if value < i32::MIN as i64 || value > i32::MAX as i64 {
                diags.push(
                    Diagnostic::error(format!(
                        "`<i32>` literal `{value}` does not fit in 32-bit signed range"
                    ))
                    .with_code("cg-width")
                    .with_span(span),
                );
                return None;
            }
            Some(MirExpr::ConstI32(value as i32))
        }
        Width::I8 => {
            if value < i8::MIN as i64 || value > i8::MAX as i64 {
                diags.push(
                    Diagnostic::error(format!(
                        "`<i8>` literal `{value}` does not fit in 8-bit signed range"
                    ))
                    .with_code("cg-width")
                    .with_span(span),
                );
                return None;
            }
            Some(MirExpr::ConstInt {
                value,
                width: Width::I8,
            })
        }
        Width::I16 => {
            if value < i16::MIN as i64 || value > i16::MAX as i64 {
                diags.push(
                    Diagnostic::error(format!(
                        "`<i16>` literal `{value}` does not fit in 16-bit signed range"
                    ))
                    .with_code("cg-width")
                    .with_span(span),
                );
                return None;
            }
            Some(MirExpr::ConstInt {
                value,
                width: Width::I16,
            })
        }
        Width::Ui8 => {
            if value < 0 || value > u8::MAX as i64 {
                diags.push(
                    Diagnostic::error(format!(
                        "`<ui8>`/`byte` literal `{value}` does not fit in 0..255"
                    ))
                    .with_code("cg-width")
                    .with_span(span),
                );
                return None;
            }
            Some(MirExpr::ConstInt {
                value,
                width: Width::Ui8,
            })
        }
        Width::Ui16 => {
            if value < 0 || value > u16::MAX as i64 {
                diags.push(
                    Diagnostic::error(format!(
                        "`<ui16>` literal `{value}` does not fit in 0..65535"
                    ))
                    .with_code("cg-width")
                    .with_span(span),
                );
                return None;
            }
            Some(MirExpr::ConstInt {
                value,
                width: Width::Ui16,
            })
        }
        Width::Ui32 => {
            if value < 0 || value > u32::MAX as i64 {
                diags.push(
                    Diagnostic::error(format!(
                        "`<ui32>` literal `{value}` does not fit in 0..2^32-1"
                    ))
                    .with_code("cg-width")
                    .with_span(span),
                );
                return None;
            }
            Some(MirExpr::ConstInt {
                value,
                width: Width::Ui32,
            })
        }
        Width::Ui64 => {
            // Full 64-bit pattern allowed (hex/bin may set the high bit via u64 parse).
            Some(MirExpr::ConstInt {
                value,
                width: Width::Ui64,
            })
        }
        // Integer spelling with float width tag: `<f32> 1`
        Width::F32 => Some(MirExpr::ConstF32(value as f32)),
        Width::F64 => Some(MirExpr::ConstF64(value as f64)),
    }
}

#[allow(dead_code)]
fn width_to_mir_repr(w: echo_ast::Width) -> MirRepr {
    use echo_ast::Width;
    match w {
        Width::I8 => MirRepr::Int8,
        Width::I16 => MirRepr::Int16,
        Width::I32 => MirRepr::Int32,
        Width::I64 => MirRepr::Int64,
        Width::Ui8 => MirRepr::UInt8,
        Width::Ui16 => MirRepr::UInt16,
        Width::Ui32 => MirRepr::UInt32,
        Width::Ui64 => MirRepr::UInt64,
        Width::F32 => MirRepr::Float32,
        Width::F64 => MirRepr::Float64,
    }
}

fn lower_expr(
    e: &HirExpr,
    module_path: &Path,
    imports: &HashMap<String, PathBuf>,
    fn_shapes: &HashMap<(PathBuf, String), MirRetShape>,
    const_env: &HashMap<String, ConstValue>,
    methods: &GraphMethods,
    fields: &GraphFields,
    field_types: &GraphFieldTypes,
    type_env: &mut HashMap<String, String>,
    diags: &mut Diagnostics,
) -> Option<MirExpr> {
    match &e.kind {
        HirExprKind::Name(n) => {
            if let Some(v) = const_env.get(n) {
                return Some(const_to_mir(v));
            }
            Some(MirExpr::Name(n.clone()))
        }
        HirExprKind::Int { value, width } => lower_int_const(*value, *width, e.span, diags),
        HirExprKind::WidthCast { width, expr } => {
            let inner = lower_expr(
                expr,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?;
            Some(MirExpr::Cast {
                to: *width,
                expr: Box::new(inner),
            })
        }
        HirExprKind::Float { value, width } => match width {
            Some(echo_ast::Width::F32) => Some(MirExpr::ConstF32(*value as f32)),
            // `<f64>` and untagged float default to f64.
            Some(echo_ast::Width::F64) | None => Some(MirExpr::ConstF64(*value)),
            Some(w) if w.is_int() => {
                // Float spelling with int width is unusual; keep f64 value.
                Some(MirExpr::ConstF64(*value))
            }
            _ => Some(MirExpr::ConstF64(*value)),
        },
        HirExprKind::Bool(b) => Some(MirExpr::ConstBool(*b)),
        HirExprKind::StringLit { kind, raw } => match decode_string_to_mir(*kind, raw, const_env) {
            Ok(e) => Some(e),
            Err(msg) => {
                diags.push(
                    Diagnostic::error(msg)
                        .with_code("cg-string")
                        .with_span(e.span),
                );
                None
            }
        },
        HirExprKind::BytesLit { kind, raw } => match decode_bytes_to_mir(*kind, raw, const_env) {
            Ok(e) => Some(e),
            Err(msg) => {
                diags.push(
                    Diagnostic::error(msg)
                        .with_code("cg-bytes")
                        .with_span(e.span),
                );
                None
            }
        },
        HirExprKind::Duration { nanos } => Some(MirExpr::ConstDuration(*nanos)),
        HirExprKind::LocatorLit { kind, raw } => match decode_locator_to_mir(*kind, raw, const_env)
        {
            Ok(e) => Some(e),
            Err(msg) => {
                diags.push(
                    Diagnostic::error(msg)
                        .with_code("cg-locator")
                        .with_span(e.span),
                );
                None
            }
        },
        HirExprKind::Unary { op, expr } => Some(MirExpr::Unary {
            op: *op,
            expr: Box::new(lower_expr(
                expr,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?),
        }),
        HirExprKind::Binary { op, left, right } => Some(MirExpr::Binary {
            op: *op,
            left: Box::new(lower_expr(
                left,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?),
            right: Box::new(lower_expr(
                right,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?),
        }),
        HirExprKind::Call { symbol, args } => {
            let ret = fn_shapes
                .get(&(module_path.to_path_buf(), symbol.clone()))
                .copied()
                .unwrap_or(MirRetShape::Plain);
            let mut a = Vec::new();
            for arg in args {
                a.push(lower_expr(
                    arg,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?);
            }
            Some(MirExpr::Call {
                target: CallTarget::Function {
                    module_path: module_path.to_path_buf(),
                    name: symbol.clone(),
                },
                args: a,
                ret,
            })
        }
        HirExprKind::CallValue { callee, args } => {
            let c = lower_expr(
                callee,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?;
            let mut a = Vec::new();
            for arg in args {
                a.push(lower_expr(
                    arg,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?);
            }
            // First-class values use plain i64 ABI for this slice.
            Some(MirExpr::Call {
                target: CallTarget::Indirect {
                    callee: Box::new(c),
                },
                args: a,
                ret: MirRetShape::Plain,
            })
        }
        HirExprKind::FnRef { symbol } => Some(MirExpr::FnValue {
            module_path: module_path.to_path_buf(),
            symbol: symbol.clone(),
        }),
        HirExprKind::Range { start, end } => Some(MirExpr::Range {
            start: Box::new(lower_expr(
                start,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?),
            end: Box::new(lower_expr(
                end,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?),
        }),
        HirExprKind::ModuleCall { module, name, args } => {
            let Some(target_path) = imports.get(module) else {
                diags.push(
                    Diagnostic::error(format!(
                        "unknown module `{module}` (analysis should have classified imports)"
                    ))
                    .with_code("cg-module")
                    .with_span(e.span),
                );
                return None;
            };
            let mut a = Vec::new();
            for arg in args {
                a.push(lower_expr(
                    arg,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?);
            }
            if is_runtime_module_path(target_path) {
                if runtime_native_symbol(name).is_none() {
                    diags.push(
                        Diagnostic::error(format!("unknown runtime primitive `runtime.{name}`"))
                            .with_code("cg-runtime")
                            .with_span(e.span),
                    );
                    return None;
                }
                return Some(MirExpr::Call {
                    target: CallTarget::Runtime {
                        export: name.clone(),
                    },
                    args: a,
                    ret: MirRetShape::Plain,
                });
            }
            // Folder modules: mangle with defining file, not the directory root.
            let define = resolve_export_file(fn_shapes, target_path, name);
            let ret = fn_shapes
                .get(&(define.clone(), name.clone()))
                .copied()
                .unwrap_or(MirRetShape::Plain);
            Some(MirExpr::Call {
                target: CallTarget::Function {
                    module_path: define,
                    name: name.clone(),
                },
                args: a,
                ret,
            })
        }
        HirExprKind::MethodCall {
            receiver,
            method,
            args,
        } => {
            // Prefer true methods (recv + mangled body). Else treat as field
            // holding a function value: load field, indirect call (no recv inject).
            let struct_name = struct_type_of_expr(receiver, methods, fields, field_types, type_env);
            let method_target = struct_name
                .as_ref()
                .and_then(|st| methods.get(st).and_then(|m| m.get(method)).cloned());
            if let Some(target) = method_target {
                let ret = fn_shapes
                    .get(&(target.module.clone(), target.mangled.clone()))
                    .copied()
                    .unwrap_or(MirRetShape::Plain);
                let recv_v = lower_expr(
                    receiver,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?;
                let mut a = vec![recv_v];
                for arg in args {
                    a.push(lower_expr(
                        arg,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    )?);
                }
                return Some(MirExpr::Call {
                    target: CallTarget::Function {
                        module_path: target.module,
                        name: target.mangled,
                    },
                    args: a,
                    ret,
                });
            }
            // Field function value: `b.f(args)` → call through `b.f`.
            let base = lower_expr(
                receiver,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?;
            let callee = MirExpr::FieldGet {
                base: Box::new(base),
                field: method.clone(),
            };
            let mut a = Vec::new();
            for arg in args {
                a.push(lower_expr(
                    arg,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?);
            }
            Some(MirExpr::Call {
                target: CallTarget::Indirect {
                    callee: Box::new(callee),
                },
                args: a,
                ret: MirRetShape::Plain,
            })
        }
        HirExprKind::ModuleField { module, name } => {
            let Some(target_path) = imports.get(module) else {
                diags.push(
                    Diagnostic::error(format!("unknown module `{module}` for field `{name}`"))
                        .with_code("cg-module")
                        .with_span(e.span),
                );
                return None;
            };
            if is_runtime_module_path(target_path) {
                diags.push(
                    Diagnostic::error("runtime primitives must be called, not read as values")
                        .with_code("cg-runtime")
                        .with_span(e.span),
                );
                return None;
            }
            let define = resolve_export_file(fn_shapes, target_path, name);
            // Exported free function: first-class value (not a zero-arg call).
            if fn_shapes.contains_key(&(define.clone(), name.clone())) {
                return Some(MirExpr::FnValue {
                    module_path: define,
                    symbol: name.clone(),
                });
            }
            // Non-function export: zero-arg getter.
            let g = value_getter_name(name);
            let ret = fn_shapes
                .get(&(define.clone(), g.clone()))
                .copied()
                .unwrap_or(MirRetShape::Plain);
            Some(MirExpr::Call {
                target: CallTarget::Function {
                    module_path: define,
                    name: g,
                },
                args: vec![],
                ret,
            })
        }
        HirExprKind::Field { base, field } => {
            let get = MirExpr::FieldGet {
                base: Box::new(lower_expr(
                    base,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?),
                field: field.clone(),
            };
            // Scalar width from shape default (`~ v = <ui64> 0`) so loads enter
            // native width without user re-tags (docs/semantics.md field width).
            if let Some(w) =
                field_width_from_default(base, field, methods, fields, field_types, type_env)
            {
                Some(MirExpr::Cast {
                    to: w,
                    expr: Box::new(get),
                })
            } else {
                Some(get)
            }
        }
        HirExprKind::StructLit {
            name,
            fields: lit_fields,
        } => {
            let mut out = Vec::new();
            let mut provided = std::collections::HashSet::new();
            for (k, v) in lit_fields {
                provided.insert(k.clone());
                out.push((
                    k.clone(),
                    lower_expr(
                        v,
                        module_path,
                        imports,
                        fn_shapes,
                        const_env,
                        methods,
                        fields,
                        field_types,
                        type_env,
                        diags,
                    )?,
                ));
            }
            if !name.is_empty() {
                if let Some(shape) = fields.get(name) {
                    for (fname, default) in shape {
                        if provided.contains(fname) {
                            continue;
                        }
                        let Some(def) = default else {
                            continue;
                        };
                        out.push((
                            fname.clone(),
                            lower_expr(
                                def,
                                module_path,
                                imports,
                                fn_shapes,
                                const_env,
                                methods,
                                fields,
                                field_types,
                                type_env,
                                diags,
                            )?,
                        ));
                    }
                }
            }
            Some(MirExpr::StructLit {
                type_name: name.clone(),
                fields: out,
            })
        }
        HirExprKind::Group(inner) => lower_expr(
            inner,
            module_path,
            imports,
            fn_shapes,
            const_env,
            methods,
            fields,
            field_types,
            type_env,
            diags,
        ),
        HirExprKind::List(items) => {
            let mut elems = Vec::new();
            for it in items {
                elems.push(lower_expr(
                    it,
                    module_path,
                    imports,
                    fn_shapes,
                    const_env,
                    methods,
                    fields,
                    field_types,
                    type_env,
                    diags,
                )?);
            }
            Some(MirExpr::ListLit(elems))
        }
        HirExprKind::Index { base, index } => Some(MirExpr::Index {
            base: Box::new(lower_expr(
                base,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?),
            index: Box::new(lower_expr(
                index,
                module_path,
                imports,
                fn_shapes,
                const_env,
                methods,
                fields,
                field_types,
                type_env,
                diags,
            )?),
        }),
        HirExprKind::Unsupported { message } => {
            diags.push(
                Diagnostic::error(message.clone())
                    .with_code("cg-unsupported")
                    .with_span(e.span),
            );
            None
        }
    }
}

fn const_to_mir(v: &ConstValue) -> MirExpr {
    match v {
        ConstValue::Int(n) => MirExpr::ConstI64(*n),
        ConstValue::Float(bits) => MirExpr::ConstF64(f64::from_bits(*bits)),
        ConstValue::Bool(b) => MirExpr::ConstI64(i64::from(*b)),
        ConstValue::Str(bytes) => MirExpr::StringLit {
            bytes: bytes.clone(),
        },
        ConstValue::Duration(nanos) => MirExpr::ConstDuration(*nanos),
        ConstValue::Bytes(bytes) => MirExpr::BytesLit {
            bytes: bytes.clone(),
        },
        ConstValue::Locator(text) => MirExpr::LocatorLit { text: text.clone() },
        ConstValue::List(items) => MirExpr::ListLit(items.iter().map(const_to_mir).collect()),
        ConstValue::Range { start, end } => MirExpr::Range {
            start: Box::new(MirExpr::ConstI64(*start)),
            end: Box::new(MirExpr::ConstI64(*end)),
        },
        ConstValue::Struct { name, fields } => MirExpr::StructLit {
            type_name: name.clone(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.clone(), const_to_mir(v)))
                .collect(),
        },
    }
}

fn fold_hir_const(e: &HirExpr, env: &HashMap<String, ConstValue>) -> Option<ConstValue> {
    match &e.kind {
        HirExprKind::Int { value, .. } => Some(ConstValue::Int(*value)),
        HirExprKind::Float { value, .. } => Some(ConstValue::Float(value.to_bits())),
        HirExprKind::Bool(b) => Some(ConstValue::Bool(*b)),
        HirExprKind::Duration { nanos } => Some(ConstValue::Duration(*nanos)),
        HirExprKind::StringLit { kind, raw } => match decode_string_to_mir(*kind, raw, env) {
            Ok(MirExpr::StringLit { bytes }) => Some(ConstValue::Str(bytes)),
            _ => None,
        },
        HirExprKind::BytesLit { kind, raw } => match decode_bytes_to_mir(*kind, raw, env) {
            Ok(MirExpr::BytesLit { bytes }) => Some(ConstValue::Bytes(bytes)),
            _ => None,
        },
        HirExprKind::LocatorLit { kind, raw } => match decode_locator_to_mir(*kind, raw, env) {
            Ok(MirExpr::LocatorLit { text }) => Some(ConstValue::Locator(text)),
            _ => None,
        },
        HirExprKind::Name(n) => env.get(n).cloned(),
        HirExprKind::Group(inner) => fold_hir_const(inner, env),
        HirExprKind::Unary { op, expr } => {
            let v = fold_hir_const(expr, env)?;
            match (op, v) {
                (UnaryOp::Neg, ConstValue::Int(n)) => Some(ConstValue::Int(n.wrapping_neg())),
                (UnaryOp::Neg, ConstValue::Float(b)) => {
                    Some(ConstValue::Float((-f64::from_bits(b)).to_bits()))
                }
                (UnaryOp::Not, ConstValue::Bool(b)) => Some(ConstValue::Bool(!b)),
                (UnaryOp::Not, ConstValue::Int(n)) => Some(ConstValue::Bool(n == 0)),
                (UnaryOp::BitNot, ConstValue::Int(n)) => Some(ConstValue::Int(!n)),
                _ => None,
            }
        }
        HirExprKind::Binary { op, left, right } => {
            let l = fold_hir_const(left, env)?;
            let r = fold_hir_const(right, env)?;
            match (op, l, r) {
                (BinaryOp::Add, ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Int(a.wrapping_add(b)))
                }
                (BinaryOp::Add, ConstValue::Duration(a), ConstValue::Duration(b)) => {
                    Some(ConstValue::Duration(a.wrapping_add(b)))
                }
                (BinaryOp::Sub, ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Int(a.wrapping_sub(b)))
                }
                (BinaryOp::Sub, ConstValue::Duration(a), ConstValue::Duration(b)) => {
                    Some(ConstValue::Duration(a.wrapping_sub(b)))
                }
                (BinaryOp::Mul, ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Int(a.wrapping_mul(b)))
                }
                (BinaryOp::Div, ConstValue::Int(a), ConstValue::Int(b)) if b != 0 => {
                    Some(ConstValue::Int(a / b))
                }
                (BinaryOp::Rem, ConstValue::Int(a), ConstValue::Int(b)) if b != 0 => {
                    Some(ConstValue::Int(a % b))
                }
                (BinaryOp::BitAnd, ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Int(a & b))
                }
                (BinaryOp::BitOr, ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Int(a | b))
                }
                (BinaryOp::BitXor, ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Int(a ^ b))
                }
                (BinaryOp::Shl, ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Int(a.wrapping_shl((b as u32) & 63)))
                }
                (BinaryOp::Shr, ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Int(a.wrapping_shr((b as u32) & 63)))
                }
                (BinaryOp::Add, ConstValue::Float(a), ConstValue::Float(b)) => Some(
                    ConstValue::Float((f64::from_bits(a) + f64::from_bits(b)).to_bits()),
                ),
                (BinaryOp::Sub, ConstValue::Float(a), ConstValue::Float(b)) => Some(
                    ConstValue::Float((f64::from_bits(a) - f64::from_bits(b)).to_bits()),
                ),
                (BinaryOp::Mul, ConstValue::Float(a), ConstValue::Float(b)) => Some(
                    ConstValue::Float((f64::from_bits(a) * f64::from_bits(b)).to_bits()),
                ),
                (BinaryOp::Div, ConstValue::Float(a), ConstValue::Float(b))
                    if f64::from_bits(b) != 0.0 =>
                {
                    Some(ConstValue::Float(
                        (f64::from_bits(a) / f64::from_bits(b)).to_bits(),
                    ))
                }
                (
                    BinaryOp::Eq | BinaryOp::EqEqEq,
                    ConstValue::Duration(a),
                    ConstValue::Duration(b),
                ) => Some(ConstValue::Bool(a == b)),
                (
                    BinaryOp::NotEq | BinaryOp::NotEqEq,
                    ConstValue::Duration(a),
                    ConstValue::Duration(b),
                ) => Some(ConstValue::Bool(a != b)),
                (BinaryOp::Eq | BinaryOp::EqEqEq, ConstValue::Bytes(a), ConstValue::Bytes(b)) => {
                    Some(ConstValue::Bool(a == b))
                }
                (
                    BinaryOp::NotEq | BinaryOp::NotEqEq,
                    ConstValue::Bytes(a),
                    ConstValue::Bytes(b),
                ) => Some(ConstValue::Bool(a != b)),
                (
                    BinaryOp::Eq | BinaryOp::EqEqEq,
                    ConstValue::Locator(a),
                    ConstValue::Locator(b),
                ) => Some(ConstValue::Bool(a == b)),
                (
                    BinaryOp::NotEq | BinaryOp::NotEqEq,
                    ConstValue::Locator(a),
                    ConstValue::Locator(b),
                ) => Some(ConstValue::Bool(a != b)),
                (BinaryOp::Eq, ConstValue::List(a), ConstValue::List(b)) => {
                    Some(ConstValue::Bool(a == b))
                }
                (BinaryOp::NotEq, ConstValue::List(a), ConstValue::List(b)) => {
                    Some(ConstValue::Bool(a != b))
                }
                (
                    BinaryOp::Eq,
                    ConstValue::Struct { fields: a, .. },
                    ConstValue::Struct { fields: b, .. },
                ) => Some(ConstValue::Bool(const_struct_fields_eq(&a, &b))),
                (
                    BinaryOp::NotEq,
                    ConstValue::Struct { fields: a, .. },
                    ConstValue::Struct { fields: b, .. },
                ) => Some(ConstValue::Bool(!const_struct_fields_eq(&a, &b))),
                (
                    BinaryOp::Eq | BinaryOp::EqEqEq,
                    ConstValue::Range {
                        start: a0,
                        end: a1,
                    },
                    ConstValue::Range {
                        start: b0,
                        end: b1,
                    },
                ) => Some(ConstValue::Bool(a0 == b0 && a1 == b1)),
                (
                    BinaryOp::NotEq | BinaryOp::NotEqEq,
                    ConstValue::Range {
                        start: a0,
                        end: a1,
                    },
                    ConstValue::Range {
                        start: b0,
                        end: b1,
                    },
                ) => Some(ConstValue::Bool(a0 != b0 || a1 != b1)),
                _ => None,
            }
        }
        HirExprKind::WidthCast { width, expr } => {
            let v = fold_hir_const(expr, env)?;
            fold_const_width_cast(v, *width)
        }
        HirExprKind::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(fold_hir_const(item, env)?);
            }
            Some(ConstValue::List(out))
        }
        HirExprKind::Range { start, end } => {
            match (fold_hir_const(start, env)?, fold_hir_const(end, env)?) {
                (ConstValue::Int(a), ConstValue::Int(b)) => {
                    Some(ConstValue::Range { start: a, end: b })
                }
                _ => None,
            }
        }
        HirExprKind::StructLit { name, fields } => {
            let mut out = Vec::with_capacity(fields.len());
            for (k, v) in fields {
                out.push((k.clone(), fold_hir_const(v, env)?));
            }
            Some(ConstValue::Struct {
                name: name.clone(),
                fields: out,
            })
        }
        HirExprKind::Field { base, field } => {
            let v = fold_hir_const(base, env)?;
            v.field(field).ok()
        }
        HirExprKind::Index { base, index } => {
            let b = fold_hir_const(base, env)?;
            match fold_hir_const(index, env)? {
                ConstValue::Int(i) => b.index(i).ok(),
                _ => None,
            }
        }
        _ => None,
    }
}

fn const_struct_fields_eq(a: &[(String, ConstValue)], b: &[(String, ConstValue)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|(k, va)| b.iter().any(|(kb, vb)| k == kb && va == vb))
}

/// Apply `%` shape defaults that themselves `#`-fold (lits / other `#`).
fn fill_const_struct_defaults(
    v: ConstValue,
    shapes: &GraphFields,
    env: &HashMap<String, ConstValue>,
) -> ConstValue {
    match v {
        ConstValue::List(xs) => ConstValue::List(
            xs.into_iter()
                .map(|x| fill_const_struct_defaults(x, shapes, env))
                .collect(),
        ),
        ConstValue::Struct { name, fields } => {
            let mut fields: Vec<(String, ConstValue)> = fields
                .into_iter()
                .map(|(k, val)| (k, fill_const_struct_defaults(val, shapes, env)))
                .collect();
            if !name.is_empty() {
                if let Some(shape) = shapes.get(&name) {
                    let provided: HashSet<String> = fields.iter().map(|(k, _)| k.clone()).collect();
                    for (fname, default) in shape {
                        if provided.contains(fname) {
                            continue;
                        }
                        let Some(def) = default else {
                            continue;
                        };
                        if let Some(dv) = fold_hir_const(def, env) {
                            fields.push((
                                fname.clone(),
                                fill_const_struct_defaults(dv, shapes, env),
                            ));
                        }
                    }
                }
            }
            ConstValue::Struct { name, fields }
        }
        other => other,
    }
}

fn fold_const_width_cast(v: ConstValue, w: echo_ast::Width) -> Option<ConstValue> {
    match (v, w) {
        (ConstValue::Int(n), w) if w.is_int() => Some(ConstValue::Int(cast_const_int(n, w))),
        (ConstValue::Int(n), echo_ast::Width::F64) => Some(ConstValue::Float((n as f64).to_bits())),
        (ConstValue::Int(n), echo_ast::Width::F32) => {
            Some(ConstValue::Float((n as f32 as f64).to_bits()))
        }
        (ConstValue::Float(b), w) if w.is_int() => {
            Some(ConstValue::Int(cast_const_int(f64::from_bits(b) as i64, w)))
        }
        (ConstValue::Float(b), echo_ast::Width::F32) => Some(ConstValue::Float(
            (f64::from_bits(b) as f32 as f64).to_bits(),
        )),
        (ConstValue::Float(b), echo_ast::Width::F64) => Some(ConstValue::Float(b)),
        _ => None,
    }
}

fn cast_const_int(n: i64, w: echo_ast::Width) -> i64 {
    match w {
        echo_ast::Width::I8 => n as i8 as i64,
        echo_ast::Width::I16 => n as i16 as i64,
        echo_ast::Width::I32 => n as i32 as i64,
        echo_ast::Width::I64 => n,
        echo_ast::Width::Ui8 => n as u8 as i64,
        echo_ast::Width::Ui16 => n as u16 as i64,
        echo_ast::Width::Ui32 => n as u32 as i64,
        echo_ast::Width::Ui64 => n,
        echo_ast::Width::F32 | echo_ast::Width::F64 => n,
    }
}

/// Decode a source string token into UTF-8 payload bytes (no interpolation).
#[must_use]
pub fn decode_string_lit(kind: StringKind, raw: &str) -> Result<Vec<u8>, String> {
    match kind {
        StringKind::Pure => decode_pure(raw),
        StringKind::Rich => {
            let parts = decode_rich_parts(raw)?;
            if parts.iter().any(|p| matches!(p, StrPart::Name(_))) {
                // Flatten only if no names (const-fold path).
                return Err("rich string has interpolation".into());
            }
            let mut out = Vec::new();
            for p in parts {
                if let StrPart::Lit(b) = p {
                    out.extend(b);
                }
            }
            Ok(out)
        }
    }
}

fn decode_string_to_mir(
    kind: StringKind,
    raw: &str,
    const_env: &HashMap<String, ConstValue>,
) -> Result<MirExpr, String> {
    match kind {
        StringKind::Pure => Ok(MirExpr::StringLit {
            bytes: decode_pure(raw)?,
        }),
        StringKind::Rich => {
            let folded = fold_rich_parts(decode_rich_parts(raw)?, const_env);
            if folded.iter().any(|p| matches!(p, StrPart::Name(_))) {
                Ok(MirExpr::StringInterp { parts: folded })
            } else {
                Ok(MirExpr::StringLit {
                    bytes: parts_to_bytes(&folded),
                })
            }
        }
    }
}

/// Decode `b'…'` / `b"…"` into a bytes lit or live `{name}` interp.
fn decode_bytes_to_mir(
    kind: StringKind,
    raw: &str,
    const_env: &HashMap<String, ConstValue>,
) -> Result<MirExpr, String> {
    let inner = strip_prefix_token(b'b', "bytes", raw)?;
    match kind {
        StringKind::Pure => Ok(MirExpr::BytesLit {
            bytes: decode_pure(inner)?,
        }),
        StringKind::Rich => {
            let folded = fold_rich_parts(decode_rich_parts(inner)?, const_env);
            if folded.iter().any(|p| matches!(p, StrPart::Name(_))) {
                Ok(MirExpr::BytesInterp { parts: folded })
            } else {
                Ok(MirExpr::BytesLit {
                    bytes: parts_to_bytes(&folded),
                })
            }
        }
    }
}

/// Decode `p'…'` / `p"…"` into a locator lit or live `{name}` interp.
fn decode_locator_to_mir(
    kind: StringKind,
    raw: &str,
    const_env: &HashMap<String, ConstValue>,
) -> Result<MirExpr, String> {
    let inner = strip_prefix_token(b'p', "locator", raw)?;
    match kind {
        StringKind::Pure => {
            let payload = decode_pure(inner)?;
            let text = String::from_utf8(payload)
                .map_err(|_| "locator payload is not UTF-8".to_string())?;
            Ok(MirExpr::LocatorLit { text })
        }
        StringKind::Rich => {
            let folded = fold_rich_parts(decode_rich_parts(inner)?, const_env);
            if folded.iter().any(|p| matches!(p, StrPart::Name(_))) {
                Ok(MirExpr::LocatorInterp { parts: folded })
            } else {
                let text = String::from_utf8(parts_to_bytes(&folded))
                    .map_err(|_| "locator payload is not UTF-8".to_string())?;
                Ok(MirExpr::LocatorLit { text })
            }
        }
    }
}

fn strip_prefix_token<'a>(prefix: u8, what: &str, raw: &'a str) -> Result<&'a str, String> {
    let b = raw.as_bytes();
    if b.len() < 3 || b[0] != prefix {
        return Err(format!("invalid {what} token `{raw}`"));
    }
    std::str::from_utf8(&b[1..]).map_err(|_| format!("invalid {what} token UTF-8"))
}

/// Bake `#` consts into lit segments; leave live `{name}` / `{.field}` parts.
fn fold_rich_parts(parts: Vec<StrPart>, const_env: &HashMap<String, ConstValue>) -> Vec<StrPart> {
    let mut folded: Vec<StrPart> = Vec::new();
    let mut lit_buf = Vec::new();
    let flush_lit = |buf: &mut Vec<u8>, out: &mut Vec<StrPart>| {
        if !buf.is_empty() {
            out.push(StrPart::Lit(std::mem::take(buf)));
        }
    };
    for p in parts {
        match p {
            StrPart::Lit(b) => lit_buf.extend(b),
            StrPart::Name(n) => match const_env.get(&n) {
                Some(v) => lit_buf.extend(v.interp_bytes()),
                None => {
                    flush_lit(&mut lit_buf, &mut folded);
                    folded.push(StrPart::Name(n));
                }
            },
        }
    }
    flush_lit(&mut lit_buf, &mut folded);
    folded
}

fn parts_to_bytes(parts: &[StrPart]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        if let StrPart::Lit(b) = p {
            out.extend(b);
        }
    }
    out
}

fn decode_pure(raw: &str) -> Result<Vec<u8>, String> {
    let b = raw.as_bytes();
    if b.len() < 2 || b[0] != b'\'' || b[b.len() - 1] != b'\'' {
        return Err(format!("invalid pure string token `{raw}`"));
    }
    Ok(b[1..b.len() - 1].to_vec())
}

/// Parse rich string into literal / `{name}` parts (escapes applied in lit parts).
pub fn decode_rich_parts(raw: &str) -> Result<Vec<StrPart>, String> {
    let b = raw.as_bytes();
    if b.len() < 2 || b[0] != b'"' || b[b.len() - 1] != b'"' {
        return Err(format!("invalid rich string token `{raw}`"));
    }
    let inner = &b[1..b.len() - 1];
    let mut parts = Vec::new();
    let mut lit = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        if inner[i] == b'\\' {
            i += 1;
            if i >= inner.len() {
                return Err("rich string ends with lone backslash".into());
            }
            match echo_syntax::decode_escape(&inner[i..]) {
                Ok((byte, n)) => {
                    lit.push(byte);
                    i += n;
                }
                Err(e) => return Err(e.to_string()),
            }
            continue;
        }
        if inner[i] == b'{' {
            // Flush lit, parse name until `}`.
            if !lit.is_empty() {
                parts.push(StrPart::Lit(std::mem::take(&mut lit)));
            }
            i += 1;
            let start = i;
            while i < inner.len() && inner[i] != b'}' {
                i += 1;
            }
            if i >= inner.len() {
                return Err("unclosed `{` in rich string".into());
            }
            let name = std::str::from_utf8(&inner[start..i])
                .map_err(|_| "invalid UTF-8 in interpolation name".to_string())?
                .to_string();
            // `{name}` local/const, or `{.field}` receiver field (method body).
            let ok = if let Some(field) = name.strip_prefix('.') {
                !field.is_empty()
                    && field
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            } else {
                !name.is_empty()
                    && name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            };
            if !ok {
                return Err(format!("invalid interpolation name `{{{name}}}`"));
            }
            parts.push(StrPart::Name(name));
            i += 1; // skip `}`
            continue;
        }
        lit.push(inner[i]);
        i += 1;
    }
    if !lit.is_empty() {
        parts.push(StrPart::Lit(lit));
    }
    Ok(parts)
}

#[allow(dead_code)]
fn _hir_body(_: &HirBody) {}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_ast::StringKind;

    #[test]
    fn pure_strips_quotes() {
        let b = decode_string_lit(StringKind::Pure, "'hello'").unwrap();
        assert_eq!(b, b"hello");
    }

    #[test]
    fn bytes_pure_and_rich_decode() {
        let pure = decode_bytes_to_mir(StringKind::Pure, "b'raw'", &HashMap::new()).unwrap();
        assert!(matches!(pure, MirExpr::BytesLit { bytes } if bytes == b"raw"));
        let rich = decode_bytes_to_mir(StringKind::Rich, r#"b"esc\n""#, &HashMap::new()).unwrap();
        assert!(matches!(rich, MirExpr::BytesLit { bytes } if bytes == b"esc\n"));
    }

    #[test]
    fn bytes_live_interp_keeps_name() {
        // docs/syntax.md: rich b"…" is like a rich string (byte payload + {name} interp).
        let e = decode_bytes_to_mir(StringKind::Rich, r#"b"x={n}""#, &HashMap::new());
        let e = e.expect("live bytes {name} interpolation is locked through run");
        match e {
            MirExpr::BytesInterp { parts } => {
                assert!(
                    parts
                        .iter()
                        .any(|p| matches!(p, StrPart::Name(n) if n == "n")),
                    "expected live name n, got {parts:?}"
                );
            }
            other => panic!("expected BytesInterp, got {other:?}"),
        }
    }

    #[test]
    fn bytes_const_fold_mixed_with_live_name() {
        let mut env = HashMap::new();
        env.insert("PRE".into(), ConstValue::Str(b"ab".to_vec()));
        let e = decode_bytes_to_mir(StringKind::Rich, r#"b"{PRE}{n}""#, &env)
            .expect("mixed # const + live name");
        match e {
            MirExpr::BytesInterp { parts } => {
                let lit: Vec<u8> = parts
                    .iter()
                    .filter_map(|p| match p {
                        StrPart::Lit(b) => Some(b.as_slice()),
                        _ => None,
                    })
                    .flatten()
                    .copied()
                    .collect();
                assert_eq!(lit, b"ab", "PRE should bake, got {parts:?}");
                assert!(parts
                    .iter()
                    .any(|p| matches!(p, StrPart::Name(n) if n == "n")));
            }
            other => panic!("expected BytesInterp, got {other:?}"),
        }
    }

    #[test]
    fn bytes_all_const_names_still_lit() {
        let mut env = HashMap::new();
        env.insert("S".into(), ConstValue::Str(b"raw".to_vec()));
        let e = decode_bytes_to_mir(StringKind::Rich, r#"b"{S}""#, &env).unwrap();
        assert!(
            matches!(e, MirExpr::BytesLit { ref bytes } if bytes == b"raw"),
            "all-const bytes interp should bake to BytesLit, got {e:?}"
        );
    }

    #[test]
    fn bytes_hash_const_bakes_payload() {
        let mut env = HashMap::new();
        env.insert("B".into(), ConstValue::Bytes(b"raw".to_vec()));
        let e = decode_bytes_to_mir(StringKind::Rich, r#"b"{B}""#, &env).unwrap();
        assert!(
            matches!(e, MirExpr::BytesLit { ref bytes } if bytes == b"raw"),
            "`#` bytes const should bake, got {e:?}"
        );
    }

    #[test]
    fn locator_hash_const_bakes_text() {
        let mut env = HashMap::new();
        env.insert("P".into(), ConstValue::Locator("/tmp".into()));
        let e = decode_locator_to_mir(StringKind::Rich, r#"p"{P}""#, &env).unwrap();
        assert!(
            matches!(e, MirExpr::LocatorLit { ref text } if text == "/tmp"),
            "`#` locator const should bake, got {e:?}"
        );
    }

    #[test]
    fn duration_hash_const_bakes_nanos_in_string() {
        let mut env = HashMap::new();
        env.insert("D".into(), ConstValue::Duration(5_000_000_000));
        let e = decode_string_to_mir(StringKind::Rich, r#""t={D}""#, &env).unwrap();
        assert!(
            matches!(e, MirExpr::StringLit { ref bytes } if bytes == b"t=5000000000"),
            "`#` duration const should bake nanos, got {e:?}"
        );
    }

    #[test]
    fn const_to_mir_duration_bytes_locator() {
        assert!(matches!(
            const_to_mir(&ConstValue::Duration(5)),
            MirExpr::ConstDuration(5)
        ));
        assert!(matches!(
            const_to_mir(&ConstValue::Bytes(b"raw".to_vec())),
            MirExpr::BytesLit { bytes } if bytes == b"raw"
        ));
        assert!(matches!(
            const_to_mir(&ConstValue::Locator("/tmp".into())),
            MirExpr::LocatorLit { text } if text == "/tmp"
        ));
        assert!(matches!(
            const_to_mir(&ConstValue::List(vec![ConstValue::Int(1), ConstValue::Int(2)])),
            MirExpr::ListLit(items)
                if matches!(
                    items.as_slice(),
                    [MirExpr::ConstI64(1), MirExpr::ConstI64(2)]
                )
        ));
        assert!(matches!(
            const_to_mir(&ConstValue::Range { start: 1, end: 3 }),
            MirExpr::Range { start, end }
                if matches!(&*start, MirExpr::ConstI64(1))
                    && matches!(&*end, MirExpr::ConstI64(3))
        ));
        assert!(matches!(
            const_to_mir(&ConstValue::Struct {
                name: "point".into(),
                fields: vec![("x".into(), ConstValue::Int(3))],
            }),
            MirExpr::StructLit { type_name, fields }
                if type_name == "point"
                    && matches!(
                        fields.as_slice(),
                        [(k, MirExpr::ConstI64(3))] if k == "x"
                    )
        ));
    }

    #[test]
    fn fold_hir_const_list_and_range() {
        use echo_hir::{HirExpr, HirExprKind};
        use echo_source::{BytePos, SourceId, Span};

        let span = Span::new(SourceId::from_u32(0), BytePos(0), BytePos(1));
        let int = |n: i64| HirExpr {
            span,
            kind: HirExprKind::Int {
                value: n,
                width: None,
            },
        };
        let list = HirExpr {
            span,
            kind: HirExprKind::List(vec![int(1), int(2)]),
        };
        assert_eq!(
            fold_hir_const(&list, &HashMap::new()),
            Some(ConstValue::List(vec![
                ConstValue::Int(1),
                ConstValue::Int(2)
            ]))
        );
        let range = HirExpr {
            span,
            kind: HirExprKind::Range {
                start: Box::new(int(1)),
                end: Box::new(int(3)),
            },
        };
        assert_eq!(
            fold_hir_const(&range, &HashMap::new()),
            Some(ConstValue::Range { start: 1, end: 3 })
        );
        let st = HirExpr {
            span,
            kind: HirExprKind::StructLit {
                name: "point".into(),
                fields: vec![("x".into(), int(3)), ("y".into(), int(4))],
            },
        };
        assert_eq!(
            fold_hir_const(&st, &HashMap::new()),
            Some(ConstValue::Struct {
                name: "point".into(),
                fields: vec![
                    ("x".into(), ConstValue::Int(3)),
                    ("y".into(), ConstValue::Int(4))
                ]
            })
        );
        let mut env = HashMap::new();
        env.insert(
            "XS".into(),
            ConstValue::List(vec![ConstValue::Int(10), ConstValue::Int(20)]),
        );
        env.insert(
            "Q".into(),
            ConstValue::Struct {
                name: String::new(),
                fields: vec![("a".into(), ConstValue::Int(1))],
            },
        );
        let idx = HirExpr {
            span,
            kind: HirExprKind::Index {
                base: Box::new(HirExpr {
                    span,
                    kind: HirExprKind::Name("XS".into()),
                }),
                index: Box::new(int(1)),
            },
        };
        assert_eq!(fold_hir_const(&idx, &env), Some(ConstValue::Int(20)));
        let fld = HirExpr {
            span,
            kind: HirExprKind::Field {
                base: Box::new(HirExpr {
                    span,
                    kind: HirExprKind::Name("Q".into()),
                }),
                field: "a".into(),
            },
        };
        assert_eq!(fold_hir_const(&fld, &env), Some(ConstValue::Int(1)));
    }

    #[test]
    fn bytes_rejects_path_interp_name() {
        let err = decode_bytes_to_mir(StringKind::Rich, r#"b"{mod.export}""#, &HashMap::new())
            .unwrap_err();
        assert!(
            err.contains("invalid interpolation name"),
            "expected invalid name, got {err}"
        );
    }

    #[test]
    fn rich_receiver_field_interp_name() {
        let parts = decode_rich_parts(r#""Hello, {.name}""#).unwrap();
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, StrPart::Name(n) if n == ".name")),
            "{parts:?}"
        );
    }

    #[test]
    fn rich_partial_const_fold_leaves_live_name() {
        let mut env = HashMap::new();
        env.insert("TITLE".into(), ConstValue::Str(b"surface".to_vec()));
        let e =
            decode_string_to_mir(StringKind::Rich, r#""title={TITLE} sum={sum}""#, &env).unwrap();
        match e {
            MirExpr::StringInterp { parts } => {
                let lit: Vec<u8> = parts
                    .iter()
                    .filter_map(|p| match p {
                        StrPart::Lit(b) => Some(b.as_slice()),
                        _ => None,
                    })
                    .flatten()
                    .copied()
                    .collect();
                assert!(
                    lit.windows(b"title=surface".len())
                        .any(|w| w == b"title=surface"),
                    "expected TITLE folded into lit, got {parts:?}"
                );
                assert!(parts
                    .iter()
                    .any(|p| matches!(p, StrPart::Name(n) if n == "sum")));
            }
            other => panic!("expected StringInterp, got {other:?}"),
        }
    }

    #[test]
    fn locator_pure_and_rich_decode() {
        let pure =
            decode_locator_to_mir(StringKind::Pure, "p'/home/user'", &HashMap::new()).unwrap();
        assert!(matches!(
            pure,
            MirExpr::LocatorLit { text } if text == "/home/user"
        ));
        let rich = decode_locator_to_mir(StringKind::Rich, r#"p"http://xo.run""#, &HashMap::new())
            .unwrap();
        assert!(matches!(
            rich,
            MirExpr::LocatorLit { text } if text == "http://xo.run"
        ));
    }

    #[test]
    fn locator_live_interp_keeps_name() {
        // docs/semantics.md: rich p"…" interpolates {name} (local / param / # const).
        let e = decode_locator_to_mir(StringKind::Rich, r#"p"http://{host}""#, &HashMap::new());
        let e = e.expect("live locator {name} interpolation is locked through run");
        match e {
            MirExpr::LocatorInterp { parts } => {
                assert!(
                    parts
                        .iter()
                        .any(|p| matches!(p, StrPart::Name(n) if n == "host")),
                    "expected live name host, got {parts:?}"
                );
                let lit: Vec<u8> = parts
                    .iter()
                    .filter_map(|p| match p {
                        StrPart::Lit(b) => Some(b.as_slice()),
                        _ => None,
                    })
                    .flatten()
                    .copied()
                    .collect();
                assert_eq!(&lit, b"http://");
            }
            other => panic!("expected LocatorInterp, got {other:?}"),
        }
    }

    #[test]
    fn locator_const_fold_mixed_with_live_name() {
        let mut env = HashMap::new();
        env.insert("SCHEME".into(), ConstValue::Str(b"http".to_vec()));
        let e = decode_locator_to_mir(StringKind::Rich, r#"p"{SCHEME}://{host}""#, &env)
            .expect("mixed # const + live name");
        match e {
            MirExpr::LocatorInterp { parts } => {
                let lit: Vec<u8> = parts
                    .iter()
                    .filter_map(|p| match p {
                        StrPart::Lit(b) => Some(b.as_slice()),
                        _ => None,
                    })
                    .flatten()
                    .copied()
                    .collect();
                assert!(
                    lit.windows(b"http://".len()).any(|w| w == b"http://"),
                    "SCHEME should bake, got {parts:?}"
                );
                assert!(parts
                    .iter()
                    .any(|p| matches!(p, StrPart::Name(n) if n == "host")));
            }
            other => panic!("expected LocatorInterp, got {other:?}"),
        }
    }

    #[test]
    fn locator_all_const_names_still_lit() {
        let mut env = HashMap::new();
        env.insert("HOST".into(), ConstValue::Str(b"xo.run".to_vec()));
        let e = decode_locator_to_mir(StringKind::Rich, r#"p"http://{HOST}""#, &env).unwrap();
        assert!(
            matches!(e, MirExpr::LocatorLit { ref text } if text == "http://xo.run"),
            "all-const locator interp should bake to LocatorLit, got {e:?}"
        );
    }

    #[test]
    fn locator_rejects_path_interp_name() {
        let err = decode_locator_to_mir(StringKind::Rich, r#"p"{mod.export}""#, &HashMap::new())
            .unwrap_err();
        assert!(
            err.contains("invalid interpolation name"),
            "expected invalid name, got {err}"
        );
    }

    #[test]
    fn pure_keeps_backslash_literal() {
        let b = decode_string_lit(StringKind::Pure, r"'\n'").unwrap();
        assert_eq!(b, b"\\n");
    }

    #[test]
    fn finish_cfg_runs_escape_not_generic_midend() {
        // Production handoff must attach escape classes and never depend on
        // removed generic MIR modules (constprop/GVN/LICM/IV/BCE).
        let body = vec![
            MirStmt::Set {
                name: "x".into(),
                value: MirExpr::ConstI64(1),
                span: None,
            },
            MirStmt::Set {
                name: "b".into(),
                value: MirExpr::BoxValue {
                    value: Box::new(MirExpr::Name("x".into())),
                    from: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::Set {
                name: "y".into(),
                value: MirExpr::UnboxValue {
                    value: Box::new(MirExpr::Name("b".into())),
                    to: MirRepr::Int64,
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("y".into()), None),
        ];
        let (cfg, _reprs, escapes) =
            finish_cfg(body, MirRetShape::Plain, &[], MirExpr::ConstI64(0));
        assert!(!cfg.blocks.is_empty());
        assert!(
            !escapes.is_empty() || cfg.blocks.iter().any(|b| !b.ops.is_empty()),
            "escape analysis must run as part of finish_cfg"
        );
        // Intermediate NoEscape box→unbox should not leave a live b@* BoxValue.
        let intermediate_box = cfg.blocks.iter().any(|b| {
            b.ops.iter().any(|op| {
                matches!(
                    op,
                    MirOp::Set {
                        name,
                        value: MirExpr::BoxValue { .. },
                        ..
                    } if name.starts_with("b@")
                )
            })
        });
        assert!(
            !intermediate_box,
            "NoEscape box elision should remove intermediate boxes; cfg={cfg:?}"
        );
    }

    #[test]
    fn finish_cfg_nested_for_in_return_has_index_phi() {
        let stmts = vec![
            MirStmt::Set {
                name: "buckets".into(),
                value: MirExpr::ListLit(vec![
                    MirExpr::ListLit(vec![]),
                    MirExpr::ListLit(vec![]),
                    MirExpr::ListLit(vec![]),
                ]),
                span: None,
            },
            MirStmt::ForIn {
                item: "chain".into(),
                iter: MirExpr::Name("buckets".into()),
                body: vec![MirStmt::ForIn {
                    item: "e".into(),
                    iter: MirExpr::Name("chain".into()),
                    body: vec![MirStmt::ReturnNone],
                }],
            },
            MirStmt::ReturnOk(MirExpr::ConstBool(true), None),
        ];
        let (cfg, _reprs, _esc) = finish_cfg(stmts, MirRetShape::Option, &[], MirExpr::ConstI64(0));
        // Unreachable for-in cont (always-return body) must not poison loop-header SSA.
        let ok = cfg.blocks.iter().any(|b| {
            matches!(
                &b.term,
                Terminator::Branch {
                    cond: MirExpr::Binary { left, .. },
                    ..
                } if matches!(left.as_ref(), MirExpr::Name(n) if n.contains("__i_") && n.contains('@'))
            )
        });
        assert!(ok, "finish_cfg must keep versioned index; cfg={cfg:?}");
        let outer_i_phi = cfg.blocks.iter().any(|b| {
            b.ops.iter().any(|op| match op {
                MirOp::Phi { name, incomings } => {
                    name.starts_with("__i_") && name.contains('@') && incomings.len() >= 2
                }
                _ => false,
            })
        });
        assert!(
            outer_i_phi,
            "outer for-in needs multi-incoming index φ; cfg={cfg:?}"
        );
    }

    #[test]
    fn rich_expands_escapes() {
        let b = decode_string_lit(StringKind::Rich, r#""a\nb\t""#).unwrap();
        assert_eq!(b, b"a\nb\t");
        let braces = decode_string_lit(StringKind::Rich, r#""\{x\}""#).unwrap();
        assert_eq!(braces, b"{x}");
        let hex = decode_string_lit(StringKind::Rich, r#""\x41""#).unwrap();
        assert_eq!(hex, b"A");
    }

    #[test]
    fn rich_unknown_escape_is_err() {
        assert!(decode_string_lit(StringKind::Rich, r#""\q""#).is_err());
        assert!(decode_string_lit(StringKind::Rich, r#""\xGG""#).is_err());
    }

    #[test]
    fn rich_interp_parts() {
        let parts = decode_rich_parts(r#""n={n}!""#).unwrap();
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], StrPart::Lit(b) if b == b"n="));
        assert!(matches!(&parts[1], StrPart::Name(n) if n == "n"));
        assert!(matches!(&parts[2], StrPart::Lit(b) if b == b"!"));
    }

    #[test]
    fn value_getter_naming() {
        assert_eq!(value_getter_name("answer"), "__val_answer");
        // Outside a project root: parent/file only (no absolute host path).
        let mangled = mangle_fn(Path::new("/proj/lib.echo"), "add");
        assert!(mangled.ends_with("_add"), "{mangled}");
        assert!(
            !mangled.contains("home") && mangled.starts_with("m_"),
            "must not embed host absolute path: {mangled}"
        );
        assert_eq!(mangled, "m_proj_lib_echo_add");
    }

    #[test]
    fn lowers_cfg_on_every_function() {
        use echo_hir::{HirExpr, HirExprKind, HirModule, HirStmt};
        use echo_semantics::{BindingKind, ValueKind};
        use echo_source::{BytePos, SourceId, Span};

        let span = Span::new(SourceId::from_u32(0), BytePos(0), BytePos(1));
        let path = PathBuf::from("/tmp/t.echo");
        let mut semantic = SemanticModel::new();
        semantic.introduce(
            "c",
            BindingKind::Immutable,
            ValueKind::Struct {
                name: "counter".into(),
            },
            span,
        );
        let mut methods = HashMap::new();
        let mut counter_m = HashMap::new();
        counter_m.insert("inc".into(), "counter_inc".into());
        methods.insert("counter".into(), counter_m);

        // Local HIR methods map (string→string); graph table is built in lower_program.
        let hir = HirModule {
            entry: vec![
                HirStmt::Bind {
                    leader: echo_ast::BindLeader::Dollar,
                    name: "c".into(),
                    init: Some(HirExpr {
                        kind: HirExprKind::StructLit {
                            name: "counter".into(),
                            fields: vec![],
                        },
                        span,
                    }),
                    span,
                },
                HirStmt::Return {
                    value: Some(HirExpr {
                        kind: HirExprKind::Int {
                            value: 0,
                            width: None,
                        },
                        span,
                    }),
                    span,
                },
            ],
            bodies: vec![echo_hir::HirBody {
                symbol: "counter_inc".into(),
                params: vec![echo_hir::RECV_PARAM.into()],
                body: vec![HirStmt::Return {
                    value: Some(HirExpr {
                        kind: HirExprKind::Int {
                            value: 1,
                            width: None,
                        },
                        span,
                    }),
                    span,
                }],
                return_shape: ReturnShape::Plain,
                receiver_struct: Some("counter".into()),
                method_name: Some("inc".into()),
                returns_receiver: false, // test
                returns_structs: vec![],
                span,
            }],
            methods,
            struct_fields: Default::default(),
            import_modules: Default::default(),
        };

        let input = ModuleLowerInput {
            path: path.clone(),
            hir,
            semantic,
            imports: HashMap::new(),
            exports: vec![],
        };
        let lowered = lower_program(path, &[input]);
        assert_eq!(lowered.diagnostics.error_count(), 0);
        assert!(!lowered.program.functions.is_empty());
        for f in &lowered.program.functions {
            assert!(!f.cfg.blocks.is_empty(), "function {} missing CFG", f.name);
        }
        let top = lowered
            .program
            .functions
            .iter()
            .find(|f| f.name == "__toplevel")
            .expect("toplevel");
        let entry_ops = &top.cfg.blocks[top.cfg.entry.0 as usize].ops;
        // Lifetime inject (ADR 0016) opens the root scope first.
        assert!(
            matches!(entry_ops.first(), Some(MirOp::ScopeEnter { id: 0 })),
            "expected root ScopeEnter, got {:?}",
            entry_ops.first()
        );
        assert!(
            entry_ops
                .iter()
                .any(|op| matches!(op, MirOp::Set { name, .. } if name.starts_with("c@"))),
            "expected SSA set of c@"
        );
    }

    #[test]
    fn method_resolve_uses_semantic_struct_env() {
        use echo_hir::{HirExpr, HirExprKind, HirModule, HirStmt};
        use echo_semantics::{BindingKind, ValueKind};
        use echo_source::{BytePos, SourceId, Span};

        let span = Span::new(SourceId::from_u32(0), BytePos(0), BytePos(1));
        let path = PathBuf::from("/tmp/m.echo");
        let mut semantic = SemanticModel::new();
        semantic.introduce(
            "c",
            BindingKind::Immutable,
            ValueKind::Struct {
                name: "counter".into(),
            },
            span,
        );
        let mut methods = HashMap::new();
        let mut counter_m = HashMap::new();
        counter_m.insert("inc".into(), "counter_inc".into());
        methods.insert("counter".into(), counter_m);

        // Top-level only uses MethodCall on name `c` — must resolve via semantic
        // value_struct without re-seeing a StructLit in the same block.
        let hir = HirModule {
            entry: vec![HirStmt::Expr(HirExpr {
                kind: HirExprKind::MethodCall {
                    receiver: Box::new(HirExpr {
                        kind: HirExprKind::Name("c".into()),

                        span,
                    }),

                    method: "inc".into(),

                    args: vec![],
                },
                span,
            })],
            bodies: vec![echo_hir::HirBody {
                symbol: "counter_inc".into(),
                params: vec![echo_hir::RECV_PARAM.into()],
                body: vec![HirStmt::Return {
                    value: Some(HirExpr {
                        kind: HirExprKind::Int {
                            value: 0,
                            width: None,
                        },
                        span,
                    }),
                    span,
                }],
                return_shape: ReturnShape::Plain,
                receiver_struct: Some("counter".into()),
                method_name: Some("inc".into()),
                returns_receiver: false, // test
                returns_structs: vec![],
                span,
            }],
            methods,
            struct_fields: Default::default(),
            import_modules: Default::default(),
        };

        let input = ModuleLowerInput {
            path: path.clone(),
            hir,
            semantic,
            imports: HashMap::new(),
            exports: vec![],
        };
        let lowered = lower_program(path, &[input]);
        assert_eq!(
            lowered.diagnostics.error_count(),
            0,
            "{:?}",
            lowered.diagnostics.items()
        );
        let top = lowered
            .program
            .functions
            .iter()
            .find(|f| f.name == "__toplevel")
            .expect("toplevel");
        let has_call = top.body.iter().any(|s| match s {
            MirStmt::Eval(MirExpr::Call {
                target: CallTarget::Function { name, .. },
                ..
            }) => name == "counter_inc",
            _ => false,
        });
        assert!(
            has_call,
            "expected method → counter_inc; body={:?}",
            top.body
        );
    }

    #[test]
    fn graph_methods_resolve_across_modules() {
        use echo_hir::{HirBody, HirExpr, HirExprKind, HirModule, HirStmt};
        use echo_semantics::{BindingKind, ValueKind};
        use echo_source::{BytePos, SourceId, Span};

        let span = Span::new(SourceId::from_u32(0), BytePos(0), BytePos(1));
        let primary = PathBuf::from("/tmp/primary.echo");
        let ops = PathBuf::from("/tmp/ops.echo");
        let entry = PathBuf::from("/tmp/entry.echo");

        let mut primary_methods = HashMap::new();
        let mut pm = HashMap::new();
        pm.insert("get".into(), "__m_counter_get".into());
        primary_methods.insert("counter".into(), pm);

        let primary_hir = HirModule {
            entry: vec![],
            bodies: vec![HirBody {
                symbol: "__m_counter_get".into(),
                params: vec![echo_hir::RECV_PARAM.into()],
                body: vec![HirStmt::Return {
                    value: Some(HirExpr {
                        kind: HirExprKind::Int {
                            value: 7,
                            width: None,
                        },
                        span,
                    }),
                    span,
                }],
                return_shape: ReturnShape::Plain,
                receiver_struct: Some("counter".into()),
                method_name: Some("inc".into()),
                returns_receiver: false, // test
                returns_structs: vec![],
                span,
            }],
            struct_fields: Default::default(),
            methods: primary_methods,
            import_modules: Default::default(),
        };

        let mut ops_methods = HashMap::new();
        let mut om = HashMap::new();
        om.insert("inc".into(), "__m_counter_inc".into());
        ops_methods.insert("counter".into(), om);

        let ops_hir = HirModule {
            entry: vec![],
            bodies: vec![HirBody {
                symbol: "__m_counter_inc".into(),
                params: vec![echo_hir::RECV_PARAM.into()],
                body: vec![HirStmt::Return {
                    value: Some(HirExpr {
                        kind: HirExprKind::Int {
                            value: 8,
                            width: None,
                        },
                        span,
                    }),
                    span,
                }],
                return_shape: ReturnShape::Plain,
                receiver_struct: Some("counter".into()),
                method_name: Some("inc".into()),
                returns_receiver: false, // test
                returns_structs: vec![],
                span,
            }],
            struct_fields: Default::default(),
            methods: ops_methods,
            import_modules: Default::default(),
        };

        let mut semantic = SemanticModel::new();
        semantic.introduce(
            "c",
            BindingKind::Immutable,
            ValueKind::Struct {
                name: "counter".into(),
            },
            span,
        );
        let entry_hir = HirModule {
            entry: vec![
                HirStmt::Expr(HirExpr {
                    kind: HirExprKind::MethodCall {
                        receiver: Box::new(HirExpr {
                            kind: HirExprKind::Name("c".into()),

                            span,
                        }),

                        method: "get".into(),

                        args: vec![],
                    },
                    span,
                }),
                HirStmt::Expr(HirExpr {
                    kind: HirExprKind::MethodCall {
                        receiver: Box::new(HirExpr {
                            kind: HirExprKind::Name("c".into()),

                            span,
                        }),

                        method: "inc".into(),

                        args: vec![],
                    },
                    span,
                }),
            ],
            bodies: vec![],
            struct_fields: Default::default(),
            methods: HashMap::new(),
            import_modules: Default::default(),
        };

        let lowered = lower_program(
            entry.clone(),
            &[
                ModuleLowerInput {
                    path: primary.clone(),
                    hir: primary_hir,
                    semantic: SemanticModel::new(),
                    imports: HashMap::new(),
                    exports: vec!["counter".into()],
                },
                ModuleLowerInput {
                    path: ops.clone(),
                    hir: ops_hir,
                    semantic: SemanticModel::new(),
                    imports: HashMap::new(),
                    exports: vec![],
                },
                ModuleLowerInput {
                    path: entry.clone(),
                    hir: entry_hir,
                    semantic,
                    imports: HashMap::new(),
                    exports: vec![],
                },
            ],
        );
        assert_eq!(
            lowered.diagnostics.error_count(),
            0,
            "{:?}",
            lowered.diagnostics.items()
        );
        let top = lowered
            .program
            .functions
            .iter()
            .find(|f| f.name == "__toplevel" && f.module_path == entry)
            .expect("entry toplevel");
        let targets: Vec<_> = top
            .body
            .iter()
            .filter_map(|s| match s {
                MirStmt::Eval(MirExpr::Call {
                    target: CallTarget::Function { module_path, name },
                    ..
                }) => Some((module_path.clone(), name.clone())),
                _ => None,
            })
            .collect();
        assert!(
            targets
                .iter()
                .any(|(p, n)| p == &primary && n == "__m_counter_get"),
            "get must target primary module; targets={targets:?}"
        );
        assert!(
            targets
                .iter()
                .any(|(p, n)| p == &ops && n == "__m_counter_inc"),
            "inc must target @ module; targets={targets:?}"
        );
    }

    #[test]
    fn free_fn_param_monomorphic_struct_methods() {
        // `$ f = (b) { b.get() }` with call `f(x)` where x is `% box` → method resolves.
        use echo_hir::{HirBody, HirExpr, HirExprKind, HirModule, HirStmt};
        use echo_semantics::SemanticModel;
        use echo_source::{BytePos, SourceId, Span};

        let span = Span::new(SourceId::from_u32(0), BytePos(0), BytePos(1));
        let path = PathBuf::from("/tmp/free_param.echo");

        let mut methods = HashMap::new();
        let mut box_m = HashMap::new();
        box_m.insert("get".into(), "box_get".into());
        methods.insert("box".into(), box_m);

        let hir = HirModule {
            entry: vec![
                HirStmt::Bind {
                    leader: echo_ast::BindLeader::Dollar,
                    name: "f".into(),
                    init: Some(HirExpr {
                        kind: HirExprKind::FnRef {
                            symbol: "f_body".into(),
                        },
                        span,
                    }),
                    span,
                },
                HirStmt::Bind {
                    leader: echo_ast::BindLeader::Dollar,
                    name: "x".into(),
                    init: Some(HirExpr {
                        kind: HirExprKind::StructLit {
                            name: "box".into(),
                            fields: vec![],
                        },
                        span,
                    }),
                    span,
                },
                HirStmt::Bind {
                    leader: echo_ast::BindLeader::Dollar,
                    name: "r".into(),
                    init: Some(HirExpr {
                        kind: HirExprKind::Call {
                            symbol: "f_body".into(),
                            args: vec![HirExpr {
                                kind: HirExprKind::Name("x".into()),
                                span,
                            }],
                        },
                        span,
                    }),
                    span,
                },
            ],
            bodies: vec![
                HirBody {
                    symbol: "f_body".into(),
                    params: vec!["b".into()],
                    body: vec![HirStmt::Return {
                        value: Some(HirExpr {
                            kind: HirExprKind::MethodCall {
                                receiver: Box::new(HirExpr {
                                    kind: HirExprKind::Name("b".into()),
                                    span,
                                }),
                                method: "get".into(),
                                args: vec![],
                            },
                            span,
                        }),
                        span,
                    }],
                    return_shape: ReturnShape::Plain,
                    receiver_struct: None,
                    method_name: None,
                    returns_receiver: false,
                    returns_structs: vec![],
                    span,
                },
                HirBody {
                    symbol: "box_get".into(),
                    params: vec![echo_hir::RECV_PARAM.into()],
                    body: vec![HirStmt::Return {
                        value: Some(HirExpr {
                            kind: HirExprKind::Int {
                                value: 42,
                                width: None,
                            },
                            span,
                        }),
                        span,
                    }],
                    return_shape: ReturnShape::Plain,
                    receiver_struct: Some("box".into()),
                    method_name: Some("get".into()),
                    returns_receiver: false,
                    returns_structs: vec![],
                    span,
                },
            ],
            methods,
            struct_fields: Default::default(),
            import_modules: Default::default(),
        };

        let input = ModuleLowerInput {
            path: path.clone(),
            hir,
            semantic: SemanticModel::new(),
            imports: HashMap::new(),
            exports: vec![],
        };
        let lowered = lower_program(path, &[input]);
        assert_eq!(
            lowered.diagnostics.error_count(),
            0,
            "{:?}",
            lowered.diagnostics.items()
        );
        let f_body = lowered
            .program
            .functions
            .iter()
            .find(|f| f.name == "f_body")
            .expect("f_body");
        let body_dbg = format!("{:?}", f_body.body);
        assert!(
            body_dbg.contains("box_get"),
            "expected param method → box_get; body={body_dbg}"
        );
    }

    #[test]
    fn mangle_fn_project_relative_not_absolute_host() {
        // This workspace has Cargo.toml at the repo root.
        let std_bytes = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../std/bytes.echo")
            .canonicalize()
            .expect("std/bytes.echo");
        let m = mangle_fn(&std_bytes, "__val_len");
        assert_eq!(m, "m_std_bytes_echo___val_len", "got {m}");
        assert!(
            !m.contains("hallas") && !m.contains("home") && !m.contains("Work"),
            "host path leaked: {m}"
        );
    }

    #[test]
    fn seed_fn_value_shapes_copies_rebind() {
        use echo_ast::BindLeader;
        use echo_hir::{HirExpr, HirExprKind, HirStmt};
        use echo_source::{BytePos, SourceId, Span};
        let span = Span::new(SourceId::from_u32(0), BytePos(0), BytePos(1));
        let path = PathBuf::from("t.echo");
        let mut shapes = HashMap::new();
        shapes.insert((path.clone(), "load".into()), MirRetShape::Result);
        let stmts = vec![HirStmt::Bind {
            leader: BindLeader::Dollar,
            name: "f".into(),
            init: Some(HirExpr {
                span,
                kind: HirExprKind::FnRef {
                    symbol: "load".into(),
                },
            }),
            span,
        }];
        seed_fn_value_shapes(&stmts, &path, &mut shapes);
        assert_eq!(
            shapes.get(&(path, "f".into())).copied(),
            Some(MirRetShape::Result)
        );
    }
}
