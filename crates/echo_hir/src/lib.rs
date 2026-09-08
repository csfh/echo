//! Analyzed, source-shaped intermediate representation.
//!
//! HIR is built **during analysis** from AST plus import-module names (and
//! method extraction). It carries source spans on every expression. Kinds
//! appear only as shapes from surface syntax — never as user type names.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};

use echo_ast::{
    BinaryOp, BindLeader, Expr, File, Ident, LoopKind, MatchArmKind, Stmt, StringKind, TaskBody,
    TaskJoinKind, UnaryOp, Width,
};
use echo_semantics::{effects_in_stmts, ReturnShape};
use echo_source::Span;

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Implicit first parameter of methods (receiver handle).
pub const RECV_PARAM: &str = "__recv";

pub use echo_ast::parse_duration_nanos;

/// Mangled free-function name for a struct method.
#[must_use]
pub fn method_fn_name(struct_name: &str, method: &str) -> String {
    format!("__m_{struct_name}_{method}")
}

/// Data field on a `%` / `@` shape (not a method).
#[derive(Debug, Clone)]
pub struct HirStructField {
    pub name: String,
    /// Default initializer when the field is omitted from a struct lit.
    pub default: Option<HirExpr>,
}

#[derive(Debug, Clone, Default)]
pub struct HirModule {
    /// Closed function **bodies** (codegen code objects).
    ///
    /// Not a language-level “function table”: source names come only from
    /// ordinary binds (`HirStmt::Bind` + [`HirExprKind::FnRef`]). Each body has
    /// a [`HirBody::symbol`] for linkage/calls.
    pub bodies: Vec<HirBody>,
    /// Top-level statements in source order (including function-value binds).
    pub entry: Vec<HirStmt>,
    /// `struct_name` → (`method_name` → body symbol).
    pub methods: HashMap<String, HashMap<String, String>>,
    /// `struct_name` → data fields (`$`/`~`/`#` members that are not methods).
    pub struct_fields: HashMap<String, Vec<HirStructField>>,
    /// Import bind names (last path segment) known for this module — analysis fact.
    pub import_modules: HashSet<String>,
}

/// One closed function body (code object). Language values are nameless;
/// [`symbol`](Self::symbol) is codegen-only.
#[derive(Debug, Clone)]
pub struct HirBody {
    /// Linkage / call target id — **not** a language name.
    pub symbol: String,
    pub params: Vec<String>,
    pub body: Vec<HirStmt>,
    /// From `^` / `!` paths in the body (syntax-driven; not a user type).
    pub return_shape: ReturnShape,
    /// When set, this is a method of that struct (`params[0]` is [`RECV_PARAM`]).
    pub receiver_struct: Option<String>,
    /// Surface method member name when [`receiver_struct`] is set.
    pub method_name: Option<String>,
    /// True when every value return is the receiver (`.`) — for self-typed chains.
    pub returns_receiver: bool,
    /// Named struct types of plain valued `^` paths (one or more).
    /// Empty = not pure named-struct returns. Single = monomorphic. Multiple = **union**.
    pub returns_structs: Vec<String>,
    /// Span of the bind that introduced this body.
    pub span: Span,
}



#[derive(Debug, Clone)]
pub enum HirStmt {
    Bind {
        leader: BindLeader,
        name: String,
        init: Option<HirExpr>,
        span: Span,
    },
    Assign {
        name: String,
        value: HirExpr,
        span: Span,
    },
    FieldAssign {
        base: HirExpr,
        field: String,
        value: HirExpr,
        span: Span,
    },
    /// `~ base[index] = value`, or `~ base[] = value` when `index` is `None` (list push).
    IndexAssign {
        base: HirExpr,
        index: Option<HirExpr>,
        value: HirExpr,
        span: Span,
    },
    Return {
        value: Option<HirExpr>,
        span: Span,
    },
    ErrorReturn {
        value: HirExpr,
        span: Span,
    },
    If {
        arms: Vec<(HirExpr, Vec<HirStmt>)>,
        else_body: Option<Vec<HirStmt>>,
        span: Span,
    },
    Match {
        scrutinee: HirExpr,
        arms: Vec<HirMatchArm>,
        span: Span,
    },
    Loop {
        kind: HirLoopKind,
        body: Vec<HirStmt>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    /// `+` spawn — body is a closed zero-arg symbol; optional handle bind.
    TaskSpawn {
        body_symbol: String,
        bind: Option<String>,
        span: Span,
    },
    /// `+ f(args)` — schedule free function with args evaluated at spawn site.
    TaskSpawnFn {
        /// Body symbol of the free function (linkage id).
        fn_symbol: String,
        args: Vec<HirExpr>,
        bind: Option<String>,
        span: Span,
    },
    /// Immediate block or join handle (`-`).
    TaskJoin {
        /// When set, join this body (immediate block). Else join `handle`.
        body_symbol: Option<String>,
        handle: Option<HirExpr>,
        bind: Option<String>,
        span: Span,
    },
    /// `& { body }` / `& name = { body }` — effect block (auto-unwrap).
    EffectBlock {
        bind: Option<String>,
        body: Vec<HirStmt>,
        span: Span,
    },
    Expr(HirExpr),
    Unsupported {
        message: String,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum HirLoopKind {
    Infinite,
    While(HirExpr),
    For {
        item: String,
        iter: HirExpr,
    },
}

#[derive(Debug, Clone)]
pub enum HirMatchArm {
    Ok {
        name: String,
        body: Vec<HirStmt>,
    },
    Err {
        name: String,
        body: Vec<HirStmt>,
    },
    Default {
        body: Vec<HirStmt>,
    },
    /// Scrutinee deep-equals any of `pats` (multi-value arm).
    Values {
        pats: Vec<HirExpr>,
        body: Vec<HirStmt>,
    },
    /// Scrutinee is a named struct whose type tag is `name` (`% Type { … }`).
    Type {
        name: String,
        body: Vec<HirStmt>,
    },
}

/// Expression with source provenance.
#[derive(Debug, Clone)]
pub struct HirExpr {
    pub span: Span,
    pub kind: HirExprKind,
}

#[derive(Debug, Clone)]
pub enum HirExprKind {
    Name(String),
    Int {
        value: i64,
        width: Option<Width>,
    },
    Float {
        value: f64,
        width: Option<Width>,
    },
    Bool(bool),
    StringLit {
        kind: StringKind,
        raw: String,
    },
    /// Bytes literal `b'…'` / `b"…"` (payload decoding is MIR).
    BytesLit {
        kind: StringKind,
        raw: String,
    },
    /// Duration literal (`5s`, `10ms`, …) as nanoseconds.
    Duration {
        nanos: i64,
    },
    /// Locator literal `p'…'` / `p"…"` (path/URI payload decoding is MIR).
    LocatorLit {
        kind: StringKind,
        raw: String,
    },
    Unary {
        op: UnaryOp,
        expr: Box<HirExpr>,
    },
    /// Explicit `<width> expr` convert.
    WidthCast {
        width: Width,
        expr: Box<HirExpr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<HirExpr>,
        right: Box<HirExpr>,
    },
    /// Inclusive integer range `lo..hi`.
    Range {
        start: Box<HirExpr>,
        end: Box<HirExpr>,
    },
    /// Direct call to a closed body by codegen [`symbol`](HirBody::symbol).
    Call {
        symbol: String,
        args: Vec<HirExpr>,
    },
    /// Call through a **function value** (param/local holding a closed body).
    CallValue {
        callee: Box<HirExpr>,
        args: Vec<HirExpr>,
    },
    /// Function value: reference to a closed body (bind introduces the name).
    FnRef {
        symbol: String,
    },
    /// `module.export(args)` — module name is an **import bind** (analysis fact).
    ModuleCall {
        module: String,
        name: String,
        args: Vec<HirExpr>,
    },
    /// `value.method(args)` — receiver may be a name, `.`, or a chained call
    /// (e.g. `c.inc().value()`).
    MethodCall {
        receiver: Box<HirExpr>,
        method: String,
        args: Vec<HirExpr>,
    },
    /// `module.export` value (import bind).
    ModuleField {
        module: String,
        name: String,
    },
    /// Value field access (not import).
    Field {
        base: Box<HirExpr>,
        field: String,
    },
    /// Named `type { … }` when `name` is non-empty; structural `{ … }` when `name` is empty.
    StructLit {
        name: String,
        fields: Vec<(String, HirExpr)>,
    },
    List(Vec<HirExpr>),
    Index {
        base: Box<HirExpr>,
        index: Box<HirExpr>,
    },
    Group(Box<HirExpr>),
    Unsupported {
        message: String,
    },
}

impl HirExpr {
    #[must_use]
    pub fn span(&self) -> Span {
        self.span
    }
}

/// Lower AST with analysis-known import module names (last path segments).
///
/// Function values are nameless closed bodies. A `$ name = (params) { … }` bind
/// becomes `Bind { name, init: FnRef { symbol } }` plus a [`HirBody`] under
/// `bodies`. Calls resolve the bind name to that symbol. Nested bodies use
/// synthetic symbols (`__n_{id}`); top-level free binds reuse the bind name as
/// the symbol for stable linkage only. No closure env — `docs/semantics.md`.
#[must_use]
pub fn lower_file(file: &File, import_modules: &HashSet<String>) -> HirModule {
    let mut bodies = Vec::new();
    let mut top = Vec::new();
    let mut methods: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut struct_fields: HashMap<String, Vec<HirStructField>> = HashMap::new();
    let mut next_body_id = 0u32;
    let mut cx = LowerCx {
        import_modules,
        bodies: &mut bodies,
        next_body_id: &mut next_body_id,
        fn_value_binds: vec![HashMap::new()],
    };

    let stmts = group_control(&file.stmts);
    for s in stmts {
        match s {
            Grouped::FnBind {
                leader,
                name,
                params,
                body,
                return_shape,
                span,
            } => {
                // Top-level free bind: symbol coincides with bind name (linkage).
                let symbol = name.clone();
                if let Some(frame) = cx.fn_value_binds.last_mut() {
                    frame.insert(name.clone(), symbol.clone());
                }
                let body_hir = lower_block(&body, &mut cx);
                let returns_structs = body_returns_named_structs(&body);
                cx.bodies.push(HirBody {
                    symbol: symbol.clone(),
                    params,
                    body: body_hir,
                    return_shape,
                    receiver_struct: None,
                    method_name: None,
                    returns_receiver: false,
                    returns_structs,
                    span,
                });
                top.push(HirStmt::Bind {
                    leader,
                    name,
                    init: Some(hexpr(span, HirExprKind::FnRef { symbol })),
                    span,
                });
            }
            Grouped::Stmt(GroupedStmt::Raw(Stmt::Struct(st) | Stmt::StructExt(st))) => {
                extract_struct_methods(&st, &mut methods, &mut cx);
                extract_struct_fields(&st, &mut struct_fields, &mut cx);
            }
            Grouped::Stmt(st) => top.push(lower_stmt(&st, &mut cx)),
        }
    }

    // Propagate `returns_structs` through free-fn call chains (incl. unions).
    refine_free_fn_returns_structs(&mut bodies);

    HirModule {
        bodies,
        entry: top,
        methods,
        struct_fields,
        import_modules: import_modules.clone(),
    }
}

/// Fixpoint: free functions that only return named structs / calls to known
/// free-fn struct returns inherit that type set (same module; order-independent).
fn refine_free_fn_returns_structs(bodies: &mut [HirBody]) {
    loop {
        let known: HashMap<String, Vec<String>> = bodies
            .iter()
            .filter(|b| b.receiver_struct.is_none())
            .filter(|b| !b.returns_structs.is_empty())
            .map(|b| (b.symbol.clone(), b.returns_structs.clone()))
            .collect();
        let mut changed = false;
        for b in bodies.iter_mut() {
            if b.receiver_struct.is_some() || !b.returns_structs.is_empty() {
                continue;
            }
            if let Some(sts) = hir_body_returns_named_structs(&b.body, &known) {
                b.returns_structs = sts;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Like [`body_returns_named_structs`] but on HIR: StructLit, calls to known free
/// fns, and **locals** bound to those (so `~ m = make(); ^ m` infers `map`).
/// Returns sorted unique type names (union when multiple).
fn hir_body_returns_named_structs(
    body: &[HirStmt],
    known: &HashMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    use std::collections::BTreeSet;
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut any = false;
    fn expr_named_structs(
        e: &HirExpr,
        known: &HashMap<String, Vec<String>>,
        locals: &HashMap<String, Vec<String>>,
    ) -> Option<Vec<String>> {
        match &e.kind {
            HirExprKind::StructLit { name, .. } if !name.is_empty() => Some(vec![name.clone()]),
            HirExprKind::Name(n) => locals.get(n).cloned(),
            HirExprKind::Call { symbol, .. } => known.get(symbol).cloned(),
            HirExprKind::ModuleCall { module, name, .. } => known
                .get(&format!("{module}.{name}"))
                .cloned()
                .or_else(|| known.get(name).cloned()),
            HirExprKind::Group(inner) => expr_named_structs(inner, known, locals),
            _ => None,
        }
    }
    fn walk(
        stmts: &[HirStmt],
        known: &HashMap<String, Vec<String>>,
        locals: &mut HashMap<String, Vec<String>>,
        found: &mut BTreeSet<String>,
        any: &mut bool,
    ) -> bool {
        for s in stmts {
            match s {
                HirStmt::Bind { name, init: Some(v), .. }
                | HirStmt::Assign { name, value: v, .. } => {
                    if let Some(tys) = expr_named_structs(v, known, locals) {
                        locals.insert(name.clone(), tys);
                    }
                }
                HirStmt::Return {
                    value: Some(v), ..
                } => {
                    let Some(tys) = expr_named_structs(v, known, locals) else {
                        return false;
                    };
                    *any = true;
                    for ty in tys {
                        found.insert(ty);
                    }
                }
                HirStmt::If { arms, else_body, .. } => {
                    for (_, body) in arms {
                        let mut arm_locals = locals.clone();
                        if !walk(body, known, &mut arm_locals, found, any) {
                            return false;
                        }
                    }
                    if let Some(body) = else_body {
                        let mut arm_locals = locals.clone();
                        if !walk(body, known, &mut arm_locals, found, any) {
                            return false;
                        }
                    }
                }
                HirStmt::Match { arms, .. } => {
                    for arm in arms {
                        let body = match arm {
                            HirMatchArm::Ok { body, .. }
                            | HirMatchArm::Err { body, .. }
                            | HirMatchArm::Default { body }
                            | HirMatchArm::Values { body, .. }
                            | HirMatchArm::Type { body, .. } => body,
                        };
                        let mut arm_locals = locals.clone();
                        if !walk(body, known, &mut arm_locals, found, any) {
                            return false;
                        }
                    }
                }
                HirStmt::Loop { body, .. } => {
                    let mut loop_locals = locals.clone();
                    if !walk(body, known, &mut loop_locals, found, any) {
                        return false;
                    }
                }
                _ => {}
            }
        }
        true
    }
    let mut locals = HashMap::new();
    if walk(body, known, &mut locals, &mut found, &mut any) && any {
        Some(found.into_iter().collect())
    } else {
        None
    }
}

struct LowerCx<'a> {
    import_modules: &'a HashSet<String>,
    bodies: &'a mut Vec<HirBody>,
    next_body_id: &'a mut u32,
    /// Stack of bind-name → body symbol (call resolution only).
    fn_value_binds: Vec<HashMap<String, String>>,
}

impl LowerCx<'_> {
    /// Resolve bind name → body symbol when this name is a known function-value bind.
    fn resolve_fn_symbol(&self, name: &str) -> Option<String> {
        for m in self.fn_value_binds.iter().rev() {
            if let Some(sym) = m.get(name) {
                return Some(sym.clone());
            }
        }
        None
    }

    fn alloc_nested_symbol(&mut self) -> String {
        let id = *self.next_body_id;
        *self.next_body_id += 1;
        format!("__n_{id}")
    }
}

enum Grouped {
    FnBind {
        leader: BindLeader,
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
        return_shape: ReturnShape,
        span: Span,
    },
    Stmt(GroupedStmt),
}

enum GroupedStmt {
    Raw(Stmt),
    If {
        arms: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
}

fn group_control(stmts: &[Stmt]) -> Vec<Grouped> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < stmts.len() {
        match &stmts[i] {
            Stmt::Bind(b) if matches!(&b.init, Some(Expr::Fn { .. })) => {
                if let Some(Expr::Fn { params, body, .. }) = &b.init {
                    let return_shape = effects_in_stmts(body).shape();
                    out.push(Grouped::FnBind {
                        leader: b.leader,
                        name: b.name.name.clone(),
                        params: params.iter().map(|p| p.name.clone()).collect(),
                        body: body.clone(),
                        return_shape,
                        span: b.span,
                    });
                }
                i += 1;
            }
            Stmt::If(if_s) => {
                let mut arms = vec![(if_s.cond.clone(), if_s.body.clone())];
                let mut else_body = None;
                let span = if_s.span;
                i += 1;
                while i < stmts.len() {
                    match &stmts[i] {
                        Stmt::ElseIf(e) => {
                            arms.push((e.cond.clone(), e.body.clone()));
                            i += 1;
                        }
                        Stmt::Else(e) => {
                            else_body = Some(e.body.clone());
                            i += 1;
                            break;
                        }
                        _ => break,
                    }
                }
                out.push(Grouped::Stmt(GroupedStmt::If {
                    arms,
                    else_body,
                    span,
                }));
            }
            Stmt::ElseIf(_) | Stmt::Else(_) => {
                out.push(Grouped::Stmt(GroupedStmt::Raw(stmts[i].clone())));
                i += 1;
            }
            Stmt::MultiBind(m) => {
                for b in m.clone().into_binds() {
                    if matches!(&b.init, Some(Expr::Fn { .. })) {
                        if let Some(Expr::Fn { params, body, .. }) = &b.init {
                            let return_shape = effects_in_stmts(body).shape();
                            out.push(Grouped::FnBind {
                                leader: b.leader,
                                name: b.name.name.clone(),
                                params: params.iter().map(|p| p.name.clone()).collect(),
                                body: body.clone(),
                                return_shape,
                                span: b.span,
                            });
                        }
                    } else {
                        out.push(Grouped::Stmt(GroupedStmt::Raw(Stmt::Bind(b))));
                    }
                }
                i += 1;
            }
            other => {
                out.push(Grouped::Stmt(GroupedStmt::Raw(other.clone())));
                i += 1;
            }
        }
    }
    out
}

fn lower_block(stmts: &[Stmt], cx: &mut LowerCx<'_>) -> Vec<HirStmt> {
    cx.fn_value_binds.push(HashMap::new());
    let out: Vec<HirStmt> = group_control(stmts)
        .into_iter()
        .map(|g| match g {
            Grouped::FnBind {
                leader,
                name,
                params,
                body,
                return_shape,
                span,
            } => {
                // Nested closed body: synthetic symbol; bind holds FnRef.
                let symbol = cx.alloc_nested_symbol();
                if let Some(frame) = cx.fn_value_binds.last_mut() {
                    frame.insert(name.clone(), symbol.clone());
                }
                let body_hir = lower_block(&body, cx);
                let returns_structs = body_returns_named_structs(&body);
                cx.bodies.push(HirBody {
                    symbol: symbol.clone(),
                    params,
                    body: body_hir,
                    return_shape,
                    receiver_struct: None,
                    method_name: None,
                    returns_receiver: false,
                    returns_structs,
                    span,
                });
                HirStmt::Bind {
                    leader,
                    name,
                    init: Some(hexpr(span, HirExprKind::FnRef { symbol })),
                    span,
                }
            }
            Grouped::Stmt(s) => lower_stmt(&s, cx),
        })
        .collect();
    cx.fn_value_binds.pop();
    out
}

fn lower_stmt(st: &GroupedStmt, cx: &mut LowerCx<'_>) -> HirStmt {
    match st {
        GroupedStmt::If {
            arms,
            else_body,
            span,
        } => HirStmt::If {
            arms: arms
                .iter()
                .map(|(c, b)| (lower_expr(c, cx), lower_block(b, cx)))
                .collect(),
            else_body: else_body.as_ref().map(|b| lower_block(b, cx)),
            span: *span,
        },
        GroupedStmt::Raw(Stmt::TaskSpawn(s)) => {
            match &s.body {
                TaskBody::Closure { captures, body } => {
                    // `+ () [a,b]? { body }` → thunk with params a,b; spawn args = outer a,b.
                    let return_shape = effects_in_stmts(body).shape();
                    let symbol = cx.alloc_nested_symbol();
                    let params: Vec<String> = captures.iter().map(|c| c.name.clone()).collect();
                    // Bind capture names as params so lower_block resolves them.
                    cx.fn_value_binds.push(HashMap::new());
                    // Note: params are ordinary locals introduced when lowering the body
                    // via HirBody.params; lower_block still needs the names in scope for
                    // nested analysis — push a frame with param names as identity.
                    let body_hir = lower_block(body, cx);
                    cx.fn_value_binds.pop();
                    cx.bodies.push(HirBody {
                        symbol: symbol.clone(),
                        params: params.clone(),
                        body: body_hir,
                        return_shape,
                        receiver_struct: None,
                        method_name: None,
                        returns_receiver: false,
                        returns_structs: vec![],
                        span: s.span,
                    });
                    // Evaluate capture names in the **parent** scope as spawn args.
                    let arg_hir: Vec<HirExpr> = captures
                        .iter()
                        .map(|c| {
                            hexpr(
                                c.span,
                                HirExprKind::Name(c.name.clone()),
                            )
                        })
                        .collect();
                    HirStmt::TaskSpawnFn {
                        fn_symbol: symbol,
                        args: arg_hir,
                        bind: s.bind.as_ref().map(|i| i.name.clone()),
                        span: s.span,
                    }
                }
                TaskBody::Block(body) => {
                    let return_shape = effects_in_stmts(body).shape();
                    let symbol = cx.alloc_nested_symbol();
                    let body_hir = lower_block(body, cx);
                    cx.bodies.push(HirBody {
                        symbol: symbol.clone(),
                        params: vec![],
                        body: body_hir,
                        return_shape,
                        receiver_struct: None,
                        method_name: None,
                        returns_receiver: false,
                        returns_structs: vec![],
                        span: s.span,
                    });
                    HirStmt::TaskSpawn {
                        body_symbol: symbol,
                        bind: s.bind.as_ref().map(|i| i.name.clone()),
                        span: s.span,
                    }
                }
                TaskBody::Call(e) => {
                    // `+ f(args)` — prefer scheduling f itself with args.
                    if let Expr::Call { callee, args, .. } = e {
                        if let Expr::Name(n) = callee.as_ref() {
                            let fn_symbol = cx
                                .fn_value_binds
                                .iter()
                                .rev()
                                .find_map(|frame| frame.get(&n.name).cloned())
                                .unwrap_or_else(|| n.name.clone());
                            let arg_hir: Vec<HirExpr> =
                                args.iter().map(|a| lower_expr(a, cx)).collect();
                            return HirStmt::TaskSpawnFn {
                                fn_symbol,
                                args: arg_hir,
                                bind: s.bind.as_ref().map(|i| i.name.clone()),
                                span: s.span,
                            };
                        }
                    }
                    // Fallback: zero-arg thunk that evaluates the call expression.
                    let symbol = cx.alloc_nested_symbol();
                    let call = lower_expr(e, cx);
                    let ret_span = e.span();
                    cx.bodies.push(HirBody {
                        symbol: symbol.clone(),
                        params: vec![],
                        body: vec![HirStmt::Return {
                            value: Some(call),
                            span: ret_span,
                        }],
                        return_shape: ReturnShape::Plain,
                        receiver_struct: None,
                        method_name: None,
                        returns_receiver: false,
                        returns_structs: vec![],
                        span: s.span,
                    });
                    HirStmt::TaskSpawn {
                        body_symbol: symbol,
                        bind: s.bind.as_ref().map(|i| i.name.clone()),
                        span: s.span,
                    }
                }
            }
        }
        GroupedStmt::Raw(Stmt::TaskJoin(s)) => match &s.kind {
            TaskJoinKind::Block { bind, body } => {
                let return_shape = effects_in_stmts(body).shape();
                let symbol = cx.alloc_nested_symbol();
                let body_hir = lower_block(body, cx);
                cx.bodies.push(HirBody {
                    symbol: symbol.clone(),
                    params: vec![],
                    body: body_hir,
                    return_shape,
                    receiver_struct: None,
                    method_name: None,
                    returns_receiver: false,
                    returns_structs: vec![],
                    span: s.span,
                });
                HirStmt::TaskJoin {
                    body_symbol: Some(symbol),
                    handle: None,
                    bind: bind.as_ref().map(|i| i.name.clone()),
                    span: s.span,
                }
            }
            TaskJoinKind::Handle { bind, handle } => HirStmt::TaskJoin {
                body_symbol: None,
                handle: Some(lower_expr(handle, cx)),
                bind: bind.as_ref().map(|i| i.name.clone()),
                span: s.span,
            },
        },
        GroupedStmt::Raw(Stmt::EffectBlock(s)) => HirStmt::EffectBlock {
            bind: s.bind.as_ref().map(|i| i.name.clone()),
            body: lower_block(&s.body, cx),
            span: s.span,
        },
        GroupedStmt::Raw(Stmt::Bind(b)) => HirStmt::Bind {
            leader: b.leader,
            name: b.name.name.clone(),
            init: b.init.as_ref().map(|e| lower_expr(e, cx)),
            span: b.span,
        },
        GroupedStmt::Raw(Stmt::Assign(a)) => match &a.target {
            echo_ast::AssignTarget::Name(n) => HirStmt::Assign {
                name: n.name.clone(),
                value: lower_expr(&a.value, cx),
                span: a.span,
            },
            echo_ast::AssignTarget::Field { base, field } => HirStmt::FieldAssign {
                base: lower_expr(base, cx),
                field: field.name.clone(),
                value: lower_expr(&a.value, cx),
                span: a.span,
            },
            echo_ast::AssignTarget::Index { base, index } => HirStmt::IndexAssign {
                base: lower_expr(base, cx),
                index: index.as_ref().map(|i| lower_expr(i, cx)),
                value: lower_expr(&a.value, cx),
                span: a.span,
            },
        },
        GroupedStmt::Raw(Stmt::Struct(s) | Stmt::StructExt(s)) => HirStmt::Unsupported {
            message: String::new(),
            span: s.span,
        },
        GroupedStmt::Raw(Stmt::Return(r)) => HirStmt::Return {
            value: r.value.as_ref().map(|e| lower_expr(e, cx)),
            span: r.span,
        },
        GroupedStmt::Raw(Stmt::ErrorReturn(r)) => HirStmt::ErrorReturn {
            value: lower_expr(&r.value, cx),
            span: r.span,
        },
        GroupedStmt::Raw(Stmt::Match(m)) => HirStmt::Match {
            scrutinee: lower_expr(&m.scrutinee, cx),
            arms: m.arms.iter().map(|a| lower_match_arm(a, cx)).collect(),
            span: m.span,
        },
        GroupedStmt::Raw(Stmt::Loop(l)) => {
            let kind = match &l.kind {
                LoopKind::Infinite => HirLoopKind::Infinite,
                LoopKind::While(e) => HirLoopKind::While(lower_expr(e, cx)),
                LoopKind::For { item, iter } => HirLoopKind::For {
                    item: item.name.clone(),
                    iter: lower_expr(iter, cx),
                },
            };
            HirStmt::Loop {
                kind,
                body: lower_block(&l.body, cx),
                span: l.span,
            }
        }
        GroupedStmt::Raw(Stmt::Break { span }) => HirStmt::Break { span: *span },
        GroupedStmt::Raw(Stmt::Continue { span }) => HirStmt::Continue { span: *span },
        GroupedStmt::Raw(Stmt::Expr(e)) => HirStmt::Expr(lower_expr(e, cx)),
        GroupedStmt::Raw(Stmt::Import(i)) => HirStmt::Unsupported {
            message: String::new(),
            span: i.span,
        },
        GroupedStmt::Raw(Stmt::Export(e)) => HirStmt::Unsupported {
            message: String::new(),
            span: e.span,
        },
        GroupedStmt::Raw(Stmt::If(s)) => HirStmt::Unsupported {
            message: "internal: control not grouped".into(),
            span: s.span,
        },
        GroupedStmt::Raw(Stmt::ElseIf(s)) => HirStmt::Unsupported {
            message: "internal: control not grouped".into(),
            span: s.span,
        },
        GroupedStmt::Raw(Stmt::Else(s)) => HirStmt::Unsupported {
            message: "internal: control not grouped".into(),
            span: s.span,
        },
        GroupedStmt::Raw(Stmt::MultiBind(m)) => HirStmt::Unsupported {
            message: "internal: multi-bind should be expanded before lower".into(),
            span: m.span,
        },
    }
}

fn lower_match_arm(arm: &echo_ast::MatchArm, cx: &mut LowerCx<'_>) -> HirMatchArm {
    let body = lower_block(&arm.body, cx);
    match &arm.kind {
        MatchArmKind::BindOk { name } => HirMatchArm::Ok {
            name: name.name.clone(),
            body,
        },
        MatchArmKind::BindErr { name } => HirMatchArm::Err {
            name: name.name.clone(),
            body,
        },
        MatchArmKind::Default => HirMatchArm::Default { body },
        MatchArmKind::Values(pats) => HirMatchArm::Values {
            pats: pats.iter().map(|p| lower_expr(p, cx)).collect(),
            body,
        },
        MatchArmKind::Type { name } => HirMatchArm::Type {
            name: name.name.clone(),
            body,
        },
    }
}

fn hexpr(span: Span, kind: HirExprKind) -> HirExpr {
    HirExpr { span, kind }
}

fn lower_expr(e: &Expr, cx: &mut LowerCx<'_>) -> HirExpr {
    let span = e.span();
    let kind = match e {
        Expr::Name(Ident { name, .. }) => {
            // Free function bind used as a **value** (not a call): emit FnRef so
            // nested free fns can pass/store outer function values without a local.
            // Params/locals are not in fn_value_binds → stay Name.
            if let Some(symbol) = cx.resolve_fn_symbol(name) {
                HirExprKind::FnRef { symbol }
            } else {
                HirExprKind::Name(name.clone())
            }
        }
        Expr::Number { text, width, .. } => parse_number_kind(text, *width),
        Expr::Bool { value, .. } => HirExprKind::Bool(*value),
        Expr::String { kind, text, .. } => HirExprKind::StringLit {
            kind: *kind,
            raw: text.clone(),
        },
        Expr::Bytes { kind, text, .. } => HirExprKind::BytesLit {
            kind: *kind,
            raw: text.clone(),
        },
        Expr::Duration { text, .. } => match parse_duration_nanos(text) {
            Ok(nanos) => HirExprKind::Duration { nanos },
            Err(msg) => HirExprKind::Unsupported {
                message: msg,
            },
        },
        Expr::Locator { kind, text, .. } => HirExprKind::LocatorLit {
            kind: *kind,
            raw: text.clone(),
        },
        Expr::Unary { op, expr, .. } => HirExprKind::Unary {
            op: *op,
            expr: Box::new(lower_expr(expr, cx)),
        },
        Expr::WidthCast {
            width: Some(w),
            expr,
            ..
        } => HirExprKind::WidthCast {
            width: *w,
            expr: Box::new(lower_expr(expr, cx)),
        },
        Expr::WidthCast { tag, expr, .. } => {
            let _ = lower_expr(expr, cx);
            HirExprKind::Unsupported {
                message: format!("unknown width tag `{tag}`"),
            }
        }
        Expr::Binary {
            op, left, right, ..
        } => HirExprKind::Binary {
            op: *op,
            left: Box::new(lower_expr(left, cx)),
            right: Box::new(lower_expr(right, cx)),
        },
        Expr::Range { start, end, .. } => HirExprKind::Range {
            start: Box::new(lower_expr(start, cx)),
            end: Box::new(lower_expr(end, cx)),
        },
        Expr::Call { callee, args, .. } => match callee.as_ref() {
            Expr::Name(Ident { name, .. }) => {
                let a: Vec<_> = args.iter().map(|a| lower_expr(a, cx)).collect();
                if let Some(symbol) = cx.resolve_fn_symbol(name) {
                    // Known function-value bind → direct body call (same ABI).
                    HirExprKind::Call { symbol, args: a }
                } else {
                    // Param/local holding a function value → call through value.
                    HirExprKind::CallValue {
                        callee: Box::new(hexpr(
                            callee.span(),
                            HirExprKind::Name(name.clone()),
                        )),
                        args: a,
                    }
                }
            }
            Expr::Field {
                base,
                field,
                ..
            } => match base.as_ref() {
                Expr::Name(Ident { name: base_name, .. })
                    if cx.import_modules.contains(base_name) =>
                {
                    HirExprKind::ModuleCall {
                        module: base_name.clone(),
                        name: field.name.clone(),
                        args: args.iter().map(|a| lower_expr(a, cx)).collect(),
                    }
                }
                _ => HirExprKind::MethodCall {
                    receiver: Box::new(lower_expr(base, cx)),
                    method: field.name.clone(),
                    args: args.iter().map(|a| lower_expr(a, cx)).collect(),
                },
            },
            other => HirExprKind::CallValue {
                callee: Box::new(lower_expr(other, cx)),
                args: args.iter().map(|a| lower_expr(a, cx)).collect(),
            },
        },
        Expr::Field { base, field, .. } => match base.as_ref() {
            Expr::Name(Ident { name: base_name, .. }) => {
                if cx.import_modules.contains(base_name) {
                    HirExprKind::ModuleField {
                        module: base_name.clone(),
                        name: field.name.clone(),
                    }
                } else {
                    HirExprKind::Field {
                        base: Box::new(hexpr(
                            base.span(),
                            HirExprKind::Name(base_name.clone()),
                        )),
                        field: field.name.clone(),
                    }
                }
            }
            Expr::Receiver { span: rspan } => HirExprKind::Field {
                base: Box::new(hexpr(*rspan, HirExprKind::Name(RECV_PARAM.into()))),
                field: field.name.clone(),
            },
            other => HirExprKind::Field {
                base: Box::new(lower_expr(other, cx)),
                field: field.name.clone(),
            },
        },
        Expr::Receiver { .. } => HirExprKind::Name(RECV_PARAM.into()),
        Expr::StructLit { path, fields, .. } => {
            let name = path
                .last()
                .map(|id| id.name.clone())
                .unwrap_or_default();
            HirExprKind::StructLit {
                name,
                fields: fields
                    .iter()
                    .map(|(k, v)| (k.name.clone(), lower_expr(v, cx)))
                    .collect(),
            }
        }
        // Structural / anonymous product `{ k: v, ... }` (not a map).
        // Represented as StructLit with empty name (no `%` type / methods).
        Expr::Object { fields, .. } => HirExprKind::StructLit {
            name: String::new(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.name.clone(), lower_expr(v, cx)))
                .collect(),
        },
        Expr::List { items, .. } => {
            HirExprKind::List(items.iter().map(|i| lower_expr(i, cx)).collect())
        }
        Expr::Index { base, index, .. } => HirExprKind::Index {
            base: Box::new(lower_expr(base, cx)),
            index: Box::new(lower_expr(index, cx)),
        },
        Expr::Group { expr, .. } => HirExprKind::Group(Box::new(lower_expr(expr, cx))),
        // Bare `(params) { … }` as a value (e.g. `test.it("n", () { … })`).
        // Same closed-body model as `$ f = (params) { … }`, with a synthetic symbol.
        Expr::Fn { params, body, .. } => {
            let return_shape = effects_in_stmts(body).shape();
            let symbol = cx.alloc_nested_symbol();
            let body_hir = lower_block(body, cx);
            let returns_structs = body_returns_named_structs(body);
            cx.bodies.push(HirBody {
                symbol: symbol.clone(),
                params: params.iter().map(|p| p.name.clone()).collect(),
                body: body_hir,
                return_shape,
                receiver_struct: None,
                method_name: None,
                returns_receiver: false,
                returns_structs,
                span,
            });
            HirExprKind::FnRef { symbol }
        },
    };
    hexpr(span, kind)
}

fn extract_struct_methods(
    s: &echo_ast::StructStmt,
    methods: &mut HashMap<String, HashMap<String, String>>,
    cx: &mut LowerCx<'_>,
) {
    let struct_name = s.name.name.clone();
    for m in &s.members {
        match m {
            Stmt::Bind(b) => {
                if let Some(Expr::Fn { params, body, .. }) = &b.init {
                    let fname = method_fn_name(&struct_name, &b.name.name);
                    let mut p = vec![RECV_PARAM.into()];
                    p.extend(params.iter().map(|x| x.name.clone()));
                    let return_shape = effects_in_stmts(body).shape();
                    // Receiver-typed ok payload when every valued `^` is `.` (incl. result
                    // methods: `! err` / `^ .` so `make().seed(…)` still types as `map`).
                    let returns_receiver = body_returns_only_receiver(body);
                    let returns_structs = if returns_receiver {
                        vec![struct_name.clone()]
                    } else {
                        body_returns_named_structs(body)
                    };
                    let body_hir = lower_block(body, cx);
                    cx.bodies.push(HirBody {
                        symbol: fname.clone(),
                        params: p,
                        body: body_hir,
                        return_shape,
                        receiver_struct: Some(struct_name.clone()),
                        method_name: Some(b.name.name.clone()),
                        returns_receiver,
                        returns_structs,
                        span: b.span,
                    });
                    methods
                        .entry(struct_name.clone())
                        .or_default()
                        .insert(b.name.name.clone(), fname);
                }
            }
            _ => {}
        }
    }
}

/// If every valued plain `^` is a named struct lit, return the sorted unique type
/// names (one = monomorphic, several = **union**). Empty vec if no pure named-struct returns.
fn body_returns_named_structs(body: &[Stmt]) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut any = false;
    fn walk(stmts: &[Stmt], found: &mut BTreeSet<String>, any: &mut bool) -> bool {
        for s in stmts {
            match s {
                Stmt::Return(r) => {
                    if let Some(v) = &r.value {
                        match v {
                            Expr::StructLit { path, .. } if !path.is_empty() => {
                                let ty = path.last().unwrap().name.clone();
                                found.insert(ty);
                                *any = true;
                            }
                            _ => return false,
                        }
                    }
                }
                Stmt::If(i) => {
                    if !walk(&i.body, found, any) {
                        return false;
                    }
                }
                Stmt::ElseIf(i) => {
                    if !walk(&i.body, found, any) {
                        return false;
                    }
                }
                Stmt::Else(e) => {
                    if !walk(&e.body, found, any) {
                        return false;
                    }
                }
                Stmt::Loop(l) => {
                    if !walk(&l.body, found, any) {
                        return false;
                    }
                }
                Stmt::Match(m) => {
                    for arm in &m.arms {
                        if !walk(&arm.body, found, any) {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
        true
    }
    if walk(body, &mut found, &mut any) && any {
        found.into_iter().collect()
    } else {
        Vec::new()
    }
}

/// True if this method should be treated as returning the receiver for typing
/// and fall-off: every valued `^` is bare `.`, or there is no valued `^`
/// (plain fall-off → `.` per locked rule).
///
/// Call only for **methods**. Free functions always pass `false`.
fn body_returns_only_receiver(body: &[Stmt]) -> bool {
    let mut all_recv_or_empty = true;
    fn walk(stmts: &[Stmt], all_recv_or_empty: &mut bool) {
        for s in stmts {
            match s {
                Stmt::Return(r) => {
                    if let Some(v) = &r.value {
                        if !matches!(v, Expr::Receiver { .. }) {
                            *all_recv_or_empty = false;
                        }
                    }
                    // bare `^` is option none — not a self-return; leave as-is
                    // (option-shaped methods are rare; fall-off rule is plain only).
                }
                Stmt::If(i) => walk(&i.body, all_recv_or_empty),
                Stmt::ElseIf(i) => walk(&i.body, all_recv_or_empty),
                Stmt::Else(e) => walk(&e.body, all_recv_or_empty),
                Stmt::Loop(l) => walk(&l.body, all_recv_or_empty),
                Stmt::Match(m) => {
                    for arm in &m.arms {
                        walk(&arm.body, all_recv_or_empty);
                    }
                }
                Stmt::Bind(b) if matches!(&b.init, Some(Expr::Fn { .. })) => {}
                _ => {}
            }
        }
    }
    walk(body, &mut all_recv_or_empty);
    all_recv_or_empty
}

/// Record data fields (and optional defaults) for struct lit checking / lower.
fn extract_struct_fields(
    s: &echo_ast::StructStmt,
    struct_fields: &mut HashMap<String, Vec<HirStructField>>,
    cx: &mut LowerCx<'_>,
) {
    let struct_name = s.name.name.clone();
    let slot = struct_fields.entry(struct_name).or_default();
    for m in &s.members {
        let Stmt::Bind(b) = m else {
            continue;
        };
        // Methods are not data fields.
        if matches!(&b.init, Some(Expr::Fn { .. })) {
            continue;
        }
        // First declaration of a field wins (later `@` redefs are rare / reject earlier).
        if slot.iter().any(|f| f.name == b.name.name) {
            continue;
        }
        let default = b.init.as_ref().map(|e| lower_expr(e, cx));
        slot.push(HirStructField {
            name: b.name.name.clone(),
            default,
        });
    }
}

fn parse_number_kind(text: &str, width: Option<Width>) -> HirExprKind {
    let t = text.replace('_', "");
    // Hex/bin are integers only (no `.` / exponent forms).
    let is_radix = t.starts_with("0x")
        || t.starts_with("0X")
        || t.starts_with("0b")
        || t.starts_with("0B");
    if !is_radix && (t.contains('.') || t.contains('e') || t.contains('E')) {
        match t.parse::<f64>() {
            Ok(value) => HirExprKind::Float { value, width },
            Err(_) => HirExprKind::Unsupported {
                message: format!("invalid float literal `{text}`"),
            },
        }
    } else {
        match echo_ast::parse_int_literal(text) {
            Ok(value) => HirExprKind::Int { value, width },
            Err(msg) => HirExprKind::Unsupported { message: msg },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_parser::parse;
    use echo_source::{BytePos, SourceId, SourceMap};

    #[test]
    fn method_fn_name_stable() {
        assert_eq!(method_fn_name("counter", "inc"), "__m_counter_inc");
        assert_eq!(RECV_PARAM, "__recv");
    }

    #[test]
    fn duration_nanos_units() {
        assert_eq!(parse_duration_nanos("5s").unwrap(), 5_000_000_000);
        assert_eq!(parse_duration_nanos("10ms").unwrap(), 10_000_000);
        assert_eq!(parse_duration_nanos("100us").unwrap(), 100_000);
        assert_eq!(parse_duration_nanos("2m").unwrap(), 120_000_000_000);
        assert_eq!(parse_duration_nanos("1h").unwrap(), 3_600_000_000_000);
    }

    #[test]
    fn expr_spans_preserved_from_ast() {
        let mut map = SourceMap::new();
        let id = map.add("t.echo", "$ x = 1\n");
        let parsed = parse(map.get(id).unwrap());
        let file = parsed.file.expect("ast");
        let hir = lower_file(&file, &HashSet::new());
        match &hir.entry[0] {
            HirStmt::Bind {
                init: Some(e),
                span,
                ..
            } => {
                assert_eq!(span.source, id);
                assert!(e.span.start.0 <= e.span.end.0);
                assert_eq!(e.span.source, id);
            }
            other => panic!("expected bind, got {other:?}"),
        }
    }

    #[test]
    fn object_lit_lowers_to_struct_lit_with_empty_name() {
        let mut map = SourceMap::new();
        let id = map.add("t.echo", "$ p = { x: 1, y: 2 }\n");
        let parsed = parse(map.get(id).unwrap());
        let file = parsed.file.expect("ast");
        let hir = lower_file(&file, &HashSet::new());
        match &hir.entry[0] {
            HirStmt::Bind {
                init: Some(e),
                name,
                ..
            } => {
                assert_eq!(name, "p");
                match &e.kind {
                    HirExprKind::StructLit { name, fields } => {
                        assert!(name.is_empty(), "anon product has empty type name");
                        assert_eq!(fields.len(), 2);
                        assert_eq!(fields[0].0, "x");
                        assert_eq!(fields[1].0, "y");
                    }
                    other => panic!("expected StructLit, got {other:?}"),
                }
            }
            other => panic!("expected bind, got {other:?}"),
        }
    }

    #[test]
    fn import_name_classified_as_module_call() {
        let mut map = SourceMap::new();
        let id = map.add("t.echo", "io.print(1)\n");
        let parsed = parse(map.get(id).unwrap());
        let file = parsed.file.expect("ast");
        let mut imports = HashSet::new();
        imports.insert("io".into());
        let hir = lower_file(&file, &imports);
        match &hir.entry[0] {
            HirStmt::Expr(e) => match &e.kind {
                HirExprKind::ModuleCall { module, name, .. } => {
                    assert_eq!(module, "io");
                    assert_eq!(name, "print");
                }
                other => panic!("expected ModuleCall, got {other:?}"),
            },
            other => panic!("expected expr, got {other:?}"),
        }
    }

    #[test]
    fn non_import_call_is_method_call() {
        let mut map = SourceMap::new();
        let id = map.add("t.echo", "c.inc()\n");
        let parsed = parse(map.get(id).unwrap());
        let file = parsed.file.expect("ast");
        let hir = lower_file(&file, &HashSet::new());
        match &hir.entry[0] {
            HirStmt::Expr(e) => match &e.kind {
                HirExprKind::MethodCall {
                    receiver, method, ..
                } => {
                    assert!(matches!(receiver.kind, HirExprKind::Name(ref n) if n == "c"));
                    assert_eq!(method, "inc");
                }
                other => panic!("expected MethodCall, got {other:?}"),
            },
            other => panic!("expected expr, got {other:?}"),
        }
        let _ = BytePos(0);
        let _ = SourceId::from_u32(0);
    }

    #[test]
    fn nested_fn_value_body_and_fnref_bind() {
        let src = r#"
$ apply = (x) {
    $ double = (n) {
        ^ n + n
    }
    ^ double(x)
}
"#;
        let mut map = SourceMap::new();
        let id = map.add("t.echo", src);
        let parsed = parse(map.get(id).unwrap());
        let file = parsed.file.expect("ast");
        let hir = lower_file(&file, &HashSet::new());
        assert!(
            hir.bodies.iter().any(|f| f.symbol == "apply"),
            "outer body symbol present"
        );
        // Entry records the bind as FnRef, not a bare function table entry only.
        match &hir.entry[0] {
            HirStmt::Bind {
                name,
                init: Some(e),
                ..
            } => {
                assert_eq!(name, "apply");
                assert!(matches!(&e.kind, HirExprKind::FnRef { symbol } if symbol == "apply"));
            }
            other => panic!("expected apply FnRef bind, got {other:?}"),
        }
        let nested: Vec<_> = hir
            .bodies
            .iter()
            .filter(|f| f.symbol.starts_with("__n_"))
            .collect();
        assert_eq!(
            nested.len(),
            1,
            "one nested body, got {:?}",
            nested.iter().map(|f| &f.symbol).collect::<Vec<_>>()
        );
        let apply = hir.bodies.iter().find(|f| f.symbol == "apply").unwrap();
        // Nested bind appears in apply body as FnRef.
        let nested_bind = apply.body.iter().find_map(|s| match s {
            HirStmt::Bind {
                name,
                init: Some(e),
                ..
            } if name == "double" => Some(e),
            _ => None,
        });
        let nested_sym = match nested_bind.map(|e| &e.kind) {
            Some(HirExprKind::FnRef { symbol }) => symbol.as_str(),
            other => panic!("expected double FnRef bind, got {other:?}"),
        };
        assert_eq!(nested_sym, nested[0].symbol.as_str());
        let call_sym = apply.body.iter().find_map(|s| match s {
            HirStmt::Return {
                value: Some(e), ..
            } => match &e.kind {
                HirExprKind::Call { symbol, .. } => Some(symbol.as_str()),
                _ => None,
            },
            _ => None,
        });
        assert_eq!(call_sym, Some(nested[0].symbol.as_str()));
    }
}
