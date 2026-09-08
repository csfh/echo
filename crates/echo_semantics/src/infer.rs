//! Kind inference over a file (v1: widths, casts, structs, result/option).

use std::collections::HashMap;

use echo_ast::{
    BinaryOp, BindLeader, Expr, File, Ident, LoopKind, MatchArmKind, Stmt, StructStmt, UnaryOp,
    Width,
};
use echo_diagnostics::{Diagnostic, Diagnostics};
use echo_source::Span;

use crate::effect::{ReturnShape, effects_in_stmts};
use crate::types::{Subst, Type, VarId};
use crate::unify::unify;
use crate::{BindingKind, ImportedModule};

/// Data field vs method slot on a `%` / `@` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldSlot {
    /// Data field; `has_default` when the shape provides `= expr`.
    Data {
        has_default: bool,
    },
    Method,
}

struct Env {
    scopes: Vec<HashMap<String, Type>>,
    /// Struct name → field name → type (vars until constrained).
    structs: HashMap<String, HashMap<String, Type>>,
    /// Struct name → member name → data vs method (for lit checking).
    struct_slots: HashMap<String, HashMap<String, FieldSlot>>,
    /// Function name → type (for top-level / local fns).
    funs: HashMap<String, Type>,
    /// module → export → type
    modules: HashMap<String, HashMap<String, Type>>,
    subst: Subst,
    next_var: u32,
    diags: Diagnostics,
    fn_depth: u32,
    /// Nested `& { … }` effect blocks — result/option use payloads.
    effect_depth: u32,
    /// Expected return type of current function (if any).
    expected_ret: Option<Type>,
    /// `% Shape` name while inferring a method body (for `.` receiver).
    method_receiver: Option<String>,
}

impl Env {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            structs: HashMap::new(),
            struct_slots: HashMap::new(),
            funs: HashMap::new(),
            modules: HashMap::new(),
            subst: Subst::new(),
            next_var: 0,
            diags: Diagnostics::new(),
            fn_depth: 0,
            effect_depth: 0,
            expected_ret: None,
            method_receiver: None,
        }
    }

    fn fresh(&mut self) -> Type {
        let id = VarId(self.next_var);
        self.next_var += 1;
        Type::Var(id)
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
        if self.scopes.is_empty() {
            self.scopes.push(HashMap::new());
        }
    }

    fn bind(&mut self, name: &str, ty: Type) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), ty);
        }
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(self.subst.apply(t));
            }
        }
        None
    }

    fn apply(&self, t: &Type) -> Type {
        self.subst.apply(t)
    }

    fn unify(&mut self, a: &Type, b: &Type, span: Span) -> Type {
        unify(&mut self.subst, a, b, span, &mut self.diags)
    }
}

/// Applied return kinds of top-level `$ name = ( ) { }` binds.
///
/// Walks binds in order so sibling calls see earlier functions (`is_empty` →
/// `len`). Skips expression statements (`test.it`, …) so suite bodies cannot
/// monomorphize export returns. Only leaf kinds are exported (see
/// [`exportable_return_kind`]).
#[must_use]
pub fn infer_export_return_types(file: &File, modules: &[ImportedModule]) -> HashMap<String, Type> {
    let mut env = setup_infer_env(file, modules);
    let mut out = HashMap::new();
    for stmt in &file.stmts {
        let echo_ast::Stmt::Bind(b) = stmt else {
            continue;
        };
        if let Some(echo_ast::Expr::Fn { params, body, .. }) = &b.init {
            let ft = infer_fn_type(&mut env, params, body, None);
            let applied = env.apply(&ft);
            env.bind(&b.name.name, applied.clone());
            env.funs.insert(b.name.name.clone(), applied.clone());
            if let Type::Fn { ret, .. } = applied {
                if let Some(leaf) = exportable_return_kind(*ret) {
                    out.insert(b.name.name.clone(), leaf);
                }
            }
        } else {
            infer_stmt(&mut env, stmt);
        }
    }
    out
}

/// Leaf kind safe to attach as `ModuleExport.return_ty`.
///
/// Result/option wrappers are peeled (the export's `return_shape` re-wraps).
/// Integers and named structs are omitted: width-tagged uses and handle
/// types must stay open at the importer.
#[must_use]
pub fn exportable_return_kind(t: Type) -> Option<Type> {
    let t = peel_return_shape(t);
    match t {
        Type::String | Type::Bool | Type::Float | Type::Float32 | Type::Bytes | Type::Duration => {
            Some(t)
        }
        _ => None,
    }
}

fn peel_return_shape(t: Type) -> Type {
    match t {
        Type::Result { ok, .. } => peel_return_shape(*ok),
        Type::Option(inner) => peel_return_shape(*inner),
        other => other,
    }
}

/// Infer kinds for `file`; returns only inference diagnostics.
#[must_use]
pub fn infer_file(file: &File, modules: &[ImportedModule]) -> Diagnostics {
    let mut env = setup_infer_env(file, modules);
    for stmt in &file.stmts {
        infer_stmt(&mut env, stmt);
    }
    env.diags
}

/// Infer the kind of the last top-level **expression statement** in `file`
/// (after prior statements), or `None` if the file does not end in a bare expr.
///
/// Used by the REPL eager-eval hint so previews show `i32` / `i64` correctly.
#[must_use]
pub fn infer_last_expr_type(file: &File, modules: &[ImportedModule]) -> Option<Type> {
    let mut env = setup_infer_env(file, modules);
    let last = file.stmts.last()?;
    let Stmt::Expr(expr) = last else {
        return None;
    };
    for stmt in &file.stmts[..file.stmts.len() - 1] {
        infer_stmt(&mut env, stmt);
    }
    let ty = infer_expr(&mut env, expr);
    Some(env.apply(&ty))
}

fn setup_infer_env(file: &File, modules: &[ImportedModule]) -> Env {
    let mut env = Env::new();

    for m in modules {
        let mut exports = HashMap::new();
        for e in &m.exports {
            let ty = match e.kind {
                BindingKind::Struct => Type::Named(e.name.clone()),
                BindingKind::Module => Type::Module,
                _ => {
                    // Arity or return shape means this export is a function.
                    // Arity alone still builds `fn(value, …)` so a missing shape
                    // cannot monomorphize the name on the first call.
                    if e.return_shape.is_some() || e.arity.is_some() || e.return_ty.is_some() {
                        shape_to_fn_type(
                            &mut env,
                            e.return_shape.unwrap_or(ReturnShape::Plain),
                            e.arity,
                            e.return_ty.clone(),
                        )
                    } else {
                        env.fresh()
                    }
                }
            };
            exports.insert(e.name.clone(), ty);
        }
        env.modules.insert(m.name.clone(), exports);
        env.bind(&m.name, Type::Module);
    }

    // Collect struct shapes (fields start as fresh vars).
    for stmt in &file.stmts {
        if let Stmt::Struct(s) | Stmt::StructExt(s) = stmt {
            register_struct(&mut env, s);
        }
    }

    env
}

fn register_struct(env: &mut Env, s: &StructStmt) {
    let sname = s.name.name.clone();
    for m in &s.members {
        if let Stmt::Bind(b) = m {
            let fname = b.name.name.clone();
            if let Some(Expr::Fn { params, body, .. }) = &b.init {
                let ft = infer_fn_type(env, params, body, Some(&sname));
                env.structs
                    .entry(sname.clone())
                    .or_default()
                    .insert(fname.clone(), ft);
                env.struct_slots
                    .entry(sname.clone())
                    .or_default()
                    .entry(fname)
                    .or_insert(FieldSlot::Method);
            } else {
                // Field width/kind from default expression when present
                // (`~ v0 = <ui64> 0` → field is ui64 for loads/stores).
                let has_default = b.init.is_some();
                let ty = if let Some(init) = &b.init {
                    let t = infer_expr(env, init);
                    env.apply(&t)
                } else {
                    env.fresh()
                };
                env.structs
                    .entry(sname.clone())
                    .or_default()
                    .entry(fname.clone())
                    .or_insert(ty);
                env.struct_slots
                    .entry(sname.clone())
                    .or_default()
                    .entry(fname)
                    .or_insert(FieldSlot::Data { has_default });
            }
        }
    }
    // Do **not** pin free data fields to `value` here. Bare fields like
    // `map.table` stay open until method bodies constrain them (often to a
    // named struct). Key/value slots become `value` when method params that
    // unify with those fields are pinned after `infer_fn_type`.
    // Refresh stored field kinds after methods (subst may have pinned vars).
    if let Some(fields) = env.structs.get(&sname).cloned() {
        for (fname, ty) in fields {
            let is_data = env
                .struct_slots
                .get(&sname)
                .and_then(|m| m.get(&fname))
                .is_some_and(|s| matches!(s, FieldSlot::Data { .. }));
            if is_data {
                let applied = env.apply(&ty);
                env.structs
                    .entry(sname.clone())
                    .or_default()
                    .insert(fname, applied);
            }
        }
    }
    env.bind(&s.name.name, Type::Named(sname));
}

fn shape_to_fn_type(
    env: &mut Env,
    shape: ReturnShape,
    arity: Option<usize>,
    known_ret: Option<Type>,
) -> Type {
    let ret = match (shape, known_ret) {
        (ReturnShape::Plain, Some(t)) => t,
        (ReturnShape::Option, Some(t)) => Type::option(t),
        (ReturnShape::Result, Some(t)) => Type::result(t, env.fresh()),
        (ReturnShape::ResultOption, Some(t)) => Type::result(Type::option(t), env.fresh()),
        (ReturnShape::Plain, None) => env.fresh(),
        (ReturnShape::Option, None) => Type::option(env.fresh()),
        (ReturnShape::Result, None) => Type::result(env.fresh(), env.fresh()),
        (ReturnShape::ResultOption, None) => Type::result(Type::option(env.fresh()), env.fresh()),
    };
    Type::func_imported(arity, ret)
}

fn infer_stmt(env: &mut Env, stmt: &Stmt) {
    match stmt {
        Stmt::Bind(b) => {
            if let Some(Expr::Fn { params, body, span }) = &b.init {
                let ft = infer_fn_type(env, params, body, None);
                env.bind(&b.name.name, ft.clone());
                env.funs.insert(b.name.name.clone(), ft);
                let _ = span;
            } else if let Some(init) = &b.init {
                let ty = infer_expr(env, init);
                if b.leader == BindLeader::Tilde {
                    if let Some(old) = env.lookup(&b.name.name) {
                        env.unify(&old, &ty, b.name.span);
                    } else {
                        env.bind(&b.name.name, ty);
                    }
                } else {
                    env.bind(&b.name.name, ty);
                }
            } else {
                // bare field in struct already registered; top-level bare rare
                let v = env.fresh();
                env.bind(&b.name.name, v);
            }
        }
        Stmt::Assign(a) => {
            let vty = infer_expr(env, &a.value);
            match &a.target {
                echo_ast::AssignTarget::Name(n) => {
                    if let Some(old) = env.lookup(&n.name) {
                        env.unify(&old, &vty, n.span);
                    }
                }
                echo_ast::AssignTarget::Field { base, field } => {
                    let bty = infer_expr(env, base);
                    let fty = field_type(env, &bty, &field.name, field.span);
                    env.unify(&fty, &vty, a.span);
                }
                echo_ast::AssignTarget::Index { base, index } => {
                    let bty = infer_expr(env, base);
                    if let Some(index) = index {
                        let ity = infer_expr(env, index);
                        env.unify(&ity, &Type::Int, index.span());
                    }
                    let elem = env.fresh();
                    env.unify(&bty, &Type::list(elem.clone()), base.span());
                    env.unify(&elem, &vty, a.span);
                }
            }
        }
        Stmt::Struct(s) | Stmt::StructExt(s) => {
            register_struct(env, s);
            env.push();
            for m in &s.members {
                infer_stmt(env, m);
            }
            env.pop();
        }
        Stmt::If(s) => {
            let c = infer_expr(env, &s.cond);
            env.unify(&c, &Type::Bool, s.cond.span());
            infer_block(env, &s.body);
        }
        Stmt::ElseIf(s) => {
            let c = infer_expr(env, &s.cond);
            env.unify(&c, &Type::Bool, s.cond.span());
            infer_block(env, &s.body);
        }
        Stmt::Else(s) => infer_block(env, &s.body),
        Stmt::ErrorReturn(s) => {
            let t = infer_expr(env, &s.value);
            if let Some(ret) = env.expected_ret.clone() {
                // Result err payload
                if let Type::Result { err, .. } = env.apply(&ret) {
                    env.unify(&t, &err, s.span);
                }
            }
        }
        Stmt::Return(s) => {
            let t = match &s.value {
                Some(v) => infer_expr(env, v),
                None => Type::Unknown, // none / unit
            };
            if let Some(ret_ty) = env.expected_ret.clone() {
                let applied = env.apply(&ret_ty);
                match applied {
                    Type::Option(inner) => {
                        if s.value.is_none() {
                            // none
                        } else {
                            env.unify(&t, &inner, s.span);
                        }
                    }
                    Type::Result { ok, .. } => {
                        if s.value.is_none() {
                            // optional nesting
                        } else {
                            match env.apply(&ok) {
                                Type::Option(inner) => env.unify(&t, &inner, s.span),
                                other => env.unify(&t, &other, s.span),
                            };
                        }
                    }
                    other => {
                        if s.value.is_some() {
                            let tt = env.apply(&t);
                            // Named-struct return unions: widen return kind instead of mismatch.
                            let merged = match (other, tt) {
                                (Type::Named(a), Type::Named(b)) if a != b => {
                                    Some(Type::union_of([Type::Named(a), Type::Named(b)]))
                                }
                                (Type::Union(xs), Type::Named(b)) => Some(Type::union_of(
                                    xs.into_iter().chain(std::iter::once(Type::Named(b))),
                                )),
                                (Type::Named(a), Type::Union(ys)) => {
                                    Some(Type::union_of(std::iter::once(Type::Named(a)).chain(ys)))
                                }
                                (Type::Union(xs), Type::Union(ys)) => {
                                    Some(Type::union_of(xs.into_iter().chain(ys)))
                                }
                                (a, b) => {
                                    env.unify(&a, &b, s.span);
                                    None
                                }
                            };
                            if let Some(u) = merged {
                                if let Type::Var(v) = ret_ty {
                                    env.subst.insert(v, u.clone());
                                }
                                env.expected_ret = Some(u);
                            }
                        }
                    }
                }
            }
        }
        Stmt::Loop(s) => {
            match &s.kind {
                LoopKind::Infinite => {}
                LoopKind::While(e) => {
                    let t = infer_expr(env, e);
                    env.unify(&t, &Type::Bool, e.span());
                }
                LoopKind::For { item, iter } => {
                    let it = infer_expr(env, iter);
                    let it = env.apply(&it);
                    let elem = match it {
                        Type::Range => {
                            // Inclusive i64 range yields integers.
                            Type::Int
                        }
                        Type::List(e) => env.apply(&e),
                        other => {
                            let elem = env.fresh();
                            env.unify(&other, &Type::list(elem.clone()), iter.span());
                            elem
                        }
                    };
                    env.push();
                    env.bind(&item.name, elem);
                    for st in &s.body {
                        infer_stmt(env, st);
                    }
                    env.pop();
                    return;
                }
            }
            infer_block(env, &s.body);
        }
        Stmt::Match(s) => {
            let scrut = infer_expr(env, &s.scrutinee);
            for arm in &s.arms {
                env.push();
                match &arm.kind {
                    MatchArmKind::Values(ps) => {
                        for p in ps {
                            // `lo..hi` arm: membership against an int scrutinee.
                            if matches!(p, Expr::Range { .. }) {
                                let _ = infer_expr(env, p);
                                env.unify(&scrut, &Type::Int, p.span());
                            } else {
                                let pt = infer_expr(env, p);
                                env.unify(&scrut, &pt, p.span());
                            }
                        }
                    }
                    MatchArmKind::Type { name } => {
                        // Refine name scrutinee to this struct type for the arm body.
                        if let Expr::Name(Ident {
                            name: scrut_name, ..
                        }) = &s.scrutinee
                        {
                            env.bind(scrut_name, Type::Named(name.name.clone()));
                        }
                    }
                    MatchArmKind::BindOk { name } => {
                        let payload = match env.apply(&scrut) {
                            Type::Option(t) => env.apply(&t),
                            Type::Result { ok, .. } => env.apply(&ok),
                            _ => env.fresh(),
                        };
                        env.bind(&name.name, payload);
                    }
                    MatchArmKind::BindErr { name } => {
                        let payload = match env.apply(&scrut) {
                            Type::Result { err, .. } => env.apply(&err),
                            _ => env.fresh(),
                        };
                        env.bind(&name.name, payload);
                    }
                    MatchArmKind::Default => {}
                }
                for st in &arm.body {
                    infer_stmt(env, st);
                }
                env.pop();
            }
        }
        Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Import(_) | Stmt::Export(_) => {}
        Stmt::TaskSpawn(s) => {
            match &s.body {
                echo_ast::TaskBody::Block(body) => infer_block(env, body),
                echo_ast::TaskBody::Call(e) => {
                    let _ = infer_expr(env, e);
                }
                echo_ast::TaskBody::Closure { captures, body } => {
                    for c in captures {
                        let _ = infer_expr(env, &echo_ast::Expr::Name(c.clone()));
                    }
                    env.push();
                    for c in captures {
                        let t = env.fresh();
                        env.bind(&c.name, t);
                    }
                    for st in body {
                        infer_stmt(env, st);
                    }
                    env.pop();
                }
            }
            if let Some(name) = &s.bind {
                // Task handle is an opaque heap value (i64 bits).
                let t = env.fresh();
                env.bind(&name.name, t);
            }
        }
        Stmt::EffectBlock(s) => {
            env.push();
            env.effect_depth += 1;
            for st in &s.body {
                infer_stmt(env, st);
            }
            env.effect_depth -= 1;
            env.pop();
            if let Some(name) = &s.bind {
                // Outcome is dynamic value (ok payload or err payload).
                env.bind(&name.name, Type::Value);
            }
        }
        Stmt::TaskJoin(s) => match &s.kind {
            echo_ast::TaskJoinKind::Block { bind, body } => {
                infer_block(env, body);
                if let Some(name) = bind {
                    // Plain immediate-block payloads are i64 at the ABI.
                    env.bind(&name.name, Type::Int);
                }
            }
            echo_ast::TaskJoinKind::Handle { bind, handle } => {
                let _ = infer_expr(env, handle);
                if let Some(name) = bind {
                    // Plain task payloads are i64 at the ABI; result/option joins
                    // are typically matched, not used as bare values in the REPL.
                    env.bind(&name.name, Type::Int);
                }
            }
        },
        Stmt::MultiBind(m) => {
            for b in m.clone().into_binds() {
                infer_stmt(env, &Stmt::Bind(b));
            }
        }
        Stmt::Expr(e) => {
            let _ = infer_expr(env, e);
        }
    }
}

fn infer_block(env: &mut Env, body: &[Stmt]) {
    env.push();
    for st in body {
        infer_stmt(env, st);
    }
    env.pop();
}

fn infer_fn_type(
    env: &mut Env,
    params: &[echo_ast::Ident],
    body: &[Stmt],
    receiver_struct: Option<&str>,
) -> Type {
    let effects = effects_in_stmts(body);
    let shape = effects.shape();
    let param_tys: Vec<Type> = params.iter().map(|_| env.fresh()).collect();
    let ret = match shape {
        ReturnShape::Plain => env.fresh(),
        ReturnShape::Option => Type::option(env.fresh()),
        ReturnShape::Result => Type::result(env.fresh(), env.fresh()),
        ReturnShape::ResultOption => Type::result(Type::option(env.fresh()), env.fresh()),
    };

    env.push();
    env.fn_depth += 1;
    let prev = env.expected_ret.replace(ret.clone());
    let prev_recv = env.method_receiver.take();
    if let Some(sname) = receiver_struct {
        env.method_receiver = Some(sname.to_string());
    }
    for (p, t) in params.iter().zip(param_tys.iter()) {
        env.bind(&p.name, t.clone());
    }
    for st in body {
        infer_stmt(env, st);
    }
    env.expected_ret = prev;
    env.method_receiver = prev_recv;
    env.fn_depth -= 1;
    env.pop();

    // Free **params** only: unconstrained after body (eq / store / passthrough)
    // become `value` so call sites stay polymorphic. Do **not** pin return vars
    // blindly — that poisoned `^ .` methods before receiver was Named.
    //
    // **Containers:** unconstrained list/option element free vars pin to
    // `unknown`, not `value`. Pinning them to `value` rewrote the caller's note
    // on the same variable after structural uses like `count(xs)` (for-in only),
    // so later `xs[i] > xs[j]` failed. A call must not change how we treat the
    // argument's values outside the callee.
    let params_applied: Vec<Type> = param_tys.iter().map(|p| env.apply(p)).collect();
    for p in &params_applied {
        pin_free_param_vars(env, p);
    }
    // Fall-off only: a plain method with no `^` / `!` returns `.` so
    // `c.inc().n` types (docs/semantics.md). Explicit `^ expr` always wins —
    // even when the expr is still a free var (imported int returns omit
    // `return_ty`; pinning those methods to the receiver broke
    // `hash_table.hash_key` → `^ hash.sip(...)`).
    if let (Some(sname), ReturnShape::Plain) = (receiver_struct, shape) {
        if !effects.has_value_return
            && !effects.has_bare_return
            && matches!(env.apply(&ret), Type::Var(_))
        {
            let span = params.first().map(|p| p.span).unwrap_or_else(|| {
                Span::new(
                    echo_source::SourceId::from_u32(0),
                    echo_source::BytePos(0),
                    echo_source::BytePos(0),
                )
            });
            env.unify(&ret, &Type::Named(sname.to_string()), span);
        }
    }
    // If ret is the same free var as a param (e.g. `id = (x) { ^ x }`), pinning
    // the param already fixed it. Re-apply params + ret for the function type.
    Type::func(
        params_applied.iter().map(|p| env.apply(p)).collect(),
        env.apply(&ret),
    )
}

/// Inside `&` effect blocks, result/option call results are payloads.
fn effect_unwrap_ret(env: &Env, t: Type) -> Type {
    if env.effect_depth == 0 {
        return t;
    }
    match env.apply(&t) {
        Type::Result { ok, .. } => env.apply(&ok),
        Type::Option(inner) => env.apply(&inner),
        other => other,
    }
}

/// Pin free type variables in a **parameter** type after body inference.
///
/// - Bare free params → [`Type::Value`] (opaque polymorphic payload).
/// - Free vars under list/option → [`Type::Unknown`] (structural “any element”;
///   must not freeze the caller's element kind to `value`).
fn pin_free_param_vars(env: &mut Env, t: &Type) {
    match env.apply(t) {
        Type::Var(v) => {
            env.subst.insert(v, Type::Value);
        }
        Type::List(e) | Type::Option(e) => match env.apply(&e) {
            Type::Var(v) => {
                env.subst.insert(v, Type::Unknown);
            }
            other => pin_free_param_vars(env, &other),
        },
        Type::Result { ok, err } => {
            pin_free_param_vars(env, &ok);
            pin_free_param_vars(env, &err);
        }
        Type::Fn { params, ret, .. } => {
            for p in params {
                pin_free_param_vars(env, &p);
            }
            pin_free_param_vars(env, &ret);
        }
        Type::Anon(fields) => {
            for (_, ty) in fields {
                pin_free_param_vars(env, &ty);
            }
        }
        Type::Union(xs) => {
            for x in xs {
                pin_free_param_vars(env, &x);
            }
        }
        _ => {}
    }
}

fn infer_expr(env: &mut Env, expr: &Expr) -> Type {
    let ty = match expr {
        Expr::Name(n) => env.lookup(&n.name).unwrap_or_else(|| env.fresh()),
        Expr::Number { text, width, .. } => match width {
            Some(Width::I8) => Type::Int8,
            Some(Width::I16) => Type::Int16,
            Some(Width::I32) => Type::Int32,
            Some(Width::I64) => Type::Int,
            Some(Width::Ui8) => Type::UInt8,
            Some(Width::Ui16) => Type::UInt16,
            Some(Width::Ui32) => Type::UInt32,
            Some(Width::Ui64) => Type::UInt64,
            Some(Width::F32) => Type::Float32,
            Some(Width::F64) => Type::Float,
            None => {
                if text.contains('.') || text.contains('e') || text.contains('E') {
                    Type::Float
                } else {
                    Type::Int
                }
            }
        },
        Expr::Duration { .. } => Type::Duration,
        Expr::String { .. } => Type::String,
        Expr::Bytes { .. } => Type::Bytes,
        Expr::Locator { .. } => Type::Named("locator".into()),
        Expr::Bool { .. } => Type::Bool,
        Expr::Receiver { .. } => env
            .method_receiver
            .as_ref()
            .map(|n| Type::Named(n.clone()))
            .unwrap_or_else(|| env.fresh()),
        Expr::WidthCast {
            width,
            tag,
            expr,
            span,
        } => {
            let from = infer_expr(env, expr);
            let Some(w) = width else {
                return Type::Error;
            };
            let from = env.apply(&from);
            if !matches!(
                from,
                Type::Unknown | Type::Value | Type::Var(_) | Type::Error
            ) && !from.is_numeric()
            {
                env.diags.push(
                    Diagnostic::error(format!(
                        "cannot cast `{from}` to `<{tag}>`; width casts apply to integers and floats"
                    ))
                    .with_code("sem-width-cast")
                    .with_span(*span),
                );
                return Type::Error;
            }
            Type::from_width(*w)
        }
        Expr::Unary { op, expr, span } => {
            let t = infer_expr(env, expr);
            match op {
                UnaryOp::Neg => {
                    // signed int or float (preserve width); unsigned has no unary `-`
                    match env.apply(&t) {
                        Type::Float => Type::Float,
                        Type::Float32 => Type::Float32,
                        Type::Int => Type::Int,
                        Type::Int8 => Type::Int8,
                        Type::Int16 => Type::Int16,
                        Type::Int32 => Type::Int32,
                        Type::Unknown | Type::Var(_) => {
                            env.unify(&t, &Type::Int, *span);
                            Type::Int
                        }
                        other => {
                            env.unify(&other, &Type::Int, *span);
                            Type::Int
                        }
                    }
                }
                UnaryOp::Not => {
                    env.unify(&t, &Type::Bool, *span);
                    Type::Bool
                }
                UnaryOp::BitNot => match env.apply(&t) {
                    t if is_int_kind(&t) => t,
                    Type::Unknown | Type::Var(_) => {
                        env.unify(&t, &Type::Int, *span);
                        Type::Int
                    }
                    other => {
                        env.unify(&other, &Type::Int, *span);
                        Type::Int
                    }
                },
            }
        }
        Expr::Range { start, end, .. } => {
            let a = infer_expr(env, start);
            let b = infer_expr(env, end);
            // Inclusive i64 range; ends should be int-like.
            env.unify(&a, &Type::Int, start.span());
            env.unify(&b, &Type::Int, end.span());
            Type::Range
        }
        Expr::Binary {
            op,
            left,
            right,
            span,
        } => {
            let lt = infer_expr(env, left);
            let rt = infer_expr(env, right);
            infer_binary(env, *op, &lt, &rt, *span)
        }
        Expr::Call { callee, args, span } => {
            let cty = infer_callee(env, callee);
            let cty = env.apply(&cty);
            match cty {
                Type::Fn {
                    params,
                    ret,
                    arity_known,
                } => {
                    if arity_known && params.len() != args.len() {
                        env.diags.push(
                            Diagnostic::error(format!(
                                "expected {} argument(s), found {}",
                                params.len(),
                                args.len()
                            ))
                            .with_span(*span)
                            .with_code("sem-arity"),
                        );
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let at = infer_expr(env, arg);
                        if let Some(p) = params.get(i) {
                            env.unify(&at, p, arg.span());
                        }
                    }
                    effect_unwrap_ret(env, env.apply(&ret))
                }
                Type::Var(_) | Type::Unknown => {
                    let params: Vec<Type> = args.iter().map(|a| infer_expr(env, a)).collect();
                    let ret = env.fresh();
                    let fty = Type::func(params, ret.clone());
                    env.unify(&cty, &fty, *span);
                    effect_unwrap_ret(env, ret)
                }
                other => {
                    env.diags.push(
                        Diagnostic::error(format!("not callable: `{other}`"))
                            .with_span(*span)
                            .with_code("sem-not-callable"),
                    );
                    Type::Error
                }
            }
        }
        Expr::Field { base, field, span } => {
            // module.export
            if let Expr::Name(m) = base.as_ref() {
                if let Some(exports) = env.modules.get(&m.name).cloned() {
                    if let Some(t) = exports.get(&field.name) {
                        return env.apply(t);
                    }
                }
            }
            let bty = infer_expr(env, base);
            reject_method_as_value(env, &bty, &field.name, *span);
            field_type(env, &bty, &field.name, *span)
        }
        Expr::Index { base, index, span } => {
            let bty = infer_expr(env, base);
            let ity = infer_expr(env, index);
            env.unify(&ity, &Type::Int, index.span());
            let elem = env.fresh();
            env.unify(&bty, &Type::list(elem.clone()), *span);
            elem
        }
        Expr::List { items, span } => {
            if items.is_empty() {
                Type::list(Type::Unknown)
            } else {
                let mut elem = infer_expr(env, &items[0]);
                for it in items.iter().skip(1) {
                    let t = infer_expr(env, it);
                    elem = env.unify(&elem, &t, *span);
                }
                Type::list(elem)
            }
        }
        Expr::Object { fields, .. } => {
            let mut fs = Vec::new();
            for (n, e) in fields {
                fs.push((n.name.clone(), infer_expr(env, e)));
            }
            Type::Anon(fs)
        }
        Expr::StructLit { path, fields, span } => {
            let name = path
                .last()
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "anon".into());
            // Ensure struct known
            if !env.structs.contains_key(&name) {
                env.structs.insert(name.clone(), HashMap::new());
            }
            let slots = env.struct_slots.get(&name).cloned().unwrap_or_default();
            let mut provided = std::collections::HashSet::new();
            for (fname, e) in fields {
                if !provided.insert(fname.name.clone()) {
                    env.diags.push(
                        Diagnostic::error(format!(
                            "duplicate field `{}` in `{name}` literal",
                            fname.name
                        ))
                        .with_code("sem-struct-dup-field")
                        .with_span(fname.span),
                    );
                }
                match slots.get(&fname.name) {
                    Some(FieldSlot::Method) => {
                        env.diags.push(
                            Diagnostic::error(format!(
                                "cannot set method `{}` in `{name}` literal",
                                fname.name
                            ))
                            .with_code("sem-struct-method-field")
                            .with_span(fname.span),
                        );
                    }
                    Some(FieldSlot::Data { .. }) => {}
                    None if !slots.is_empty() => {
                        env.diags.push(
                            Diagnostic::error(format!(
                                "unknown field `{}` on `{name}`",
                                fname.name
                            ))
                            .with_code("sem-struct-unknown-field")
                            .with_span(fname.span),
                        );
                    }
                    None => {}
                }
                let t = infer_expr(env, e);
                let expected = env
                    .structs
                    .get(&name)
                    .and_then(|m| m.get(&fname.name).cloned())
                    .unwrap_or_else(|| env.fresh());
                let u = env.unify(&t, &expected, fname.span);
                let applied = env.apply(&u);
                env.structs
                    .entry(name.clone())
                    .or_default()
                    .insert(fname.name.clone(), applied);
            }
            // Required data fields (no default) must appear.
            if !slots.is_empty() {
                for (fname, slot) in &slots {
                    if let FieldSlot::Data { has_default: false } = slot {
                        if !provided.contains(fname) {
                            env.diags.push(
                                Diagnostic::error(format!(
                                    "missing required field `{fname}` on `{name}` literal"
                                ))
                                .with_code("sem-struct-missing-field")
                                .with_span(*span),
                            );
                        }
                    }
                }
            }
            Type::Named(name)
        }
        Expr::Fn { params, body, .. } => infer_fn_type(env, params, body, None),
        Expr::Group { expr, .. } => infer_expr(env, expr),
    };
    env.apply(&ty)
}

fn infer_binary(env: &mut Env, op: BinaryOp, lt: &Type, rt: &Type, span: Span) -> Type {
    let lt = env.apply(lt);
    let rt = env.apply(rt);
    match op {
        BinaryOp::Add | BinaryOp::Sub
            if matches!(
                (env.apply(&lt), env.apply(&rt)),
                (Type::Duration, Type::Duration)
            ) =>
        {
            env.unify(&lt, &Type::Duration, span);
            env.unify(&rt, &Type::Duration, span);
            Type::Duration
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Rem => {
            numeric_binop(env, &lt, &rt, span, false)
        }
        BinaryOp::Div => numeric_binop(env, &lt, &rt, span, true),
        BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::EqEqEq | BinaryOp::NotEqEq => {
            env.unify(&lt, &rt, span);
            Type::Bool
        }
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => {
            let _ = numeric_binop(env, &lt, &rt, span, false);
            Type::Bool
        }
        BinaryOp::And | BinaryOp::Or => {
            env.unify(&lt, &Type::Bool, span);
            env.unify(&rt, &Type::Bool, span);
            Type::Bool
        }
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
            int_binop(env, &lt, &rt, span)
        }
    }
}

/// Integer-only binary ops (`& | ^ << >>`): same width, or default `i64` yields
/// to a more specific width (untagged literals adopt the other operand's lane).
fn int_binop(env: &mut Env, lt: &Type, rt: &Type, span: Span) -> Type {
    let lt = env.apply(lt);
    let rt = env.apply(rt);
    if matches!(lt, Type::Value) || matches!(rt, Type::Value) {
        env.diags.push(
            Diagnostic::error(format!(
                "cannot use dynamic `value` in integer arithmetic (`{lt}` vs `{rt}`)"
            ))
            .with_span(span)
            .with_code("sem-type-mismatch"),
        );
        return Type::Error;
    }
    match (&lt, &rt) {
        (a, b) if is_int_kind(a) && a == b => a.clone(),
        // Default untagged/`i64` yields to a specific width.
        (Type::Int, b) if is_specific_int(b) => {
            env.unify(&lt, b, span);
            b.clone()
        }
        (a, Type::Int) if is_specific_int(a) => {
            env.unify(&rt, a, span);
            a.clone()
        }
        (a, b) if is_int_kind(a) && is_int_kind(b) && a != b => {
            env.diags.push(
                Diagnostic::error(format!(
                    "cannot mix integer widths `{a}` and `{b}` (no implicit conversion)"
                ))
                .with_span(span)
                .with_code("sem-type-mismatch"),
            );
            Type::Error
        }
        (a, b) if is_int_kind(a) && matches!(b, Type::Unknown | Type::Var(_)) => {
            env.unify(&rt, a, span);
            a.clone()
        }
        (a, b) if is_int_kind(b) && matches!(a, Type::Unknown | Type::Var(_)) => {
            env.unify(&lt, b, span);
            b.clone()
        }
        (Type::Unknown | Type::Var(_), Type::Unknown | Type::Var(_)) => {
            env.unify(&lt, &Type::Int, span);
            env.unify(&rt, &Type::Int, span);
            Type::Int
        }
        _ => {
            env.unify(&lt, &Type::Int, span);
            env.unify(&rt, &Type::Int, span);
            Type::Int
        }
    }
}

fn is_specific_int(t: &Type) -> bool {
    matches!(
        t,
        Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64
    )
}

fn is_int_kind(t: &Type) -> bool {
    matches!(
        t,
        Type::Int
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64
    )
}

#[allow(dead_code)]
fn is_signed_int_kind(t: &Type) -> bool {
    matches!(t, Type::Int | Type::Int8 | Type::Int16 | Type::Int32)
}

#[allow(dead_code)]
fn is_unsigned_int_kind(t: &Type) -> bool {
    matches!(t, Type::UInt8 | Type::UInt16 | Type::UInt32 | Type::UInt64)
}

fn is_float_kind(t: &Type) -> bool {
    matches!(t, Type::Float | Type::Float32)
}

fn numeric_binop(env: &mut Env, lt: &Type, rt: &Type, span: Span, _is_div: bool) -> Type {
    let lt = env.apply(lt);
    let rt = env.apply(rt);
    if matches!(lt, Type::Value) || matches!(rt, Type::Value) {
        env.diags.push(
            Diagnostic::error(format!(
                "cannot use dynamic `value` in numeric arithmetic (`{lt}` vs `{rt}`)"
            ))
            .with_span(span)
            .with_code("sem-type-mismatch"),
        );
        return Type::Error;
    }
    match (&lt, &rt) {
        (Type::Int, Type::Int) => Type::Int,
        (Type::Int8, Type::Int8) => Type::Int8,
        (Type::Int16, Type::Int16) => Type::Int16,
        (Type::Int32, Type::Int32) => Type::Int32,
        (Type::UInt8, Type::UInt8) => Type::UInt8,
        (Type::UInt16, Type::UInt16) => Type::UInt16,
        (Type::UInt32, Type::UInt32) => Type::UInt32,
        (Type::UInt64, Type::UInt64) => Type::UInt64,
        (Type::Float, Type::Float) => Type::Float,
        (Type::Float32, Type::Float32) => Type::Float32,
        // Default i64 yields to a specific integer width (literals adopt the lane).
        (Type::Int, b) if is_specific_int(b) => {
            env.unify(&lt, b, span);
            b.clone()
        }
        (a, Type::Int) if is_specific_int(a) => {
            env.unify(&rt, a, span);
            a.clone()
        }
        (a, b) if is_int_kind(a) && is_int_kind(b) && a != b => {
            env.diags.push(
                Diagnostic::error(format!(
                    "cannot mix integer widths `{a}` and `{b}` (no implicit conversion)"
                ))
                .with_span(span)
                .with_code("sem-type-mismatch"),
            );
            Type::Error
        }
        (a, b) if is_float_kind(a) && is_float_kind(b) && a != b => {
            env.diags.push(
                Diagnostic::error(format!(
                    "cannot mix float widths `{a}` and `{b}` (no implicit conversion)"
                ))
                .with_span(span)
                .with_code("sem-type-mismatch"),
            );
            Type::Error
        }
        (a, b) if is_int_kind(a) && is_float_kind(b) || is_float_kind(a) && is_int_kind(b) => {
            env.diags.push(
                Diagnostic::error("cannot mix int and float (no implicit conversion)")
                    .with_span(span)
                    .with_code("sem-type-mismatch"),
            );
            Type::Error
        }
        (Type::Var(_), _) | (_, Type::Var(_)) | (Type::Unknown, _) | (_, Type::Unknown) => {
            if is_float_kind(&lt) || is_float_kind(&rt) {
                env.unify(&lt, &Type::Float, span);
                env.unify(&rt, &Type::Float, span);
                Type::Float
            } else {
                env.unify(&lt, &Type::Int, span);
                env.unify(&rt, &Type::Int, span);
                Type::Int
            }
        }
        _ => {
            env.unify(&lt, &rt, span);
            env.apply(&lt)
        }
    }
}

fn infer_callee(env: &mut Env, expr: &Expr) -> Type {
    if let Expr::Field { base, field, span } = expr {
        if let Expr::Name(m) = base.as_ref() {
            if let Some(exports) = env.modules.get(&m.name).cloned() {
                if let Some(t) = exports.get(&field.name) {
                    return env.apply(t);
                }
            }
        }
        let bty = infer_expr(env, base);
        return field_type(env, &bty, &field.name, *span);
    }
    infer_expr(env, expr)
}

fn reject_method_as_value(env: &mut Env, base: &Type, field: &str, span: Span) {
    let base = env.apply(base);
    let Type::Named(name) = base else {
        return;
    };
    if env
        .struct_slots
        .get(&name)
        .and_then(|m| m.get(field))
        .is_some_and(|s| matches!(s, FieldSlot::Method))
    {
        env.diags.push(
            Diagnostic::error(format!(
                "method `{field}` on `{name}` is not a value; call `{name}.{field}(…)`"
            ))
            .with_span(span)
            .with_code("sem-method-value"),
        );
    }
}

fn field_type(env: &mut Env, base: &Type, field: &str, span: Span) -> Type {
    let base = env.apply(base);
    match base {
        Type::Named(ref name) => {
            if let Some(fields) = env.structs.get(name) {
                if let Some(t) = fields.get(field) {
                    return env.apply(t);
                }
            }
            // Unknown field — fresh and record
            let v = env.fresh();
            env.structs
                .entry(name.clone())
                .or_default()
                .insert(field.to_string(), v.clone());
            v
        }
        Type::Anon(ref fields) => {
            if let Some((_, t)) = fields.iter().find(|(n, _)| n == field) {
                return env.apply(t);
            }
            env.diags.push(
                Diagnostic::error(format!("no field `{field}` on anonymous struct"))
                    .with_span(span)
                    .with_code("sem-no-field"),
            );
            Type::Error
        }
        Type::Module => Type::Unknown,
        Type::Var(_) | Type::Unknown => {
            let v = env.fresh();
            // cannot constrain struct yet
            v
        }
        Type::Value => {
            env.diags.push(
                Diagnostic::error(format!("no field `{field}` on `value`"))
                    .with_span(span)
                    .with_code("sem-no-field"),
            );
            Type::Error
        }
        other => {
            env.diags.push(
                Diagnostic::error(format!("no field `{field}` on `{other}`"))
                    .with_span(span)
                    .with_code("sem-no-field"),
            );
            Type::Error
        }
    }
}
