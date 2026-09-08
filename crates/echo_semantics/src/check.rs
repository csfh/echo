//! File-local semantic analysis: binds, modules, Option/Result handling.

use std::collections::HashMap;

use echo_ast::{
    AssignTarget, BindLeader, BindStmt, Expr, File, LoopKind, MatchArmKind, Stmt, StructStmt,
    UnaryOp,
};
use echo_diagnostics::{Diagnostic, Diagnostics};
use echo_source::Span;

use crate::const_eval::{ConstValue, ShapeDefaults, eval_const_expr_with_shapes};
use crate::effect::{ReturnShape, effects_in_stmts};
use crate::{BindingKind, ImportedModule, ModuleExport};

#[derive(Debug, Clone)]
struct Binding {
    kind: BindingKind,
    exports: HashMap<String, BindingKind>,
    /// If this name is a function value, its return shape.
    return_shape: Option<ReturnShape>,
    /// Module export return shapes (name → shape).
    export_shapes: HashMap<String, ReturnShape>,
}

#[derive(Debug, Default)]
struct Scope {
    names: Vec<(String, Binding)>,
}

struct Cx {
    scopes: Vec<Scope>,
    diagnostics: Diagnostics,
    in_method: bool,
    /// Nested depth of `& { … }` effect blocks (auto-unwrap result/option).
    effect_depth: u32,
    loop_depth: u32,
    fn_depth: u32,
    /// Index of the function’s parameter scope (`None` at top-level).
    /// Captures only care about bindings **outside** this index, not block/loop
    /// scopes nested inside the function.
    fn_scope_base: Option<usize>,
    /// Evaluated `#` constants in declaration order (file-local).
    const_env: HashMap<String, ConstValue>,
    /// `% Shape` data-member defaults for `#` struct-lit fill.
    struct_defaults: ShapeDefaults,
    /// Set while checking `%` / `@` members so defaults can be recorded.
    current_struct: Option<String>,
}

impl Cx {
    fn new() -> Self {
        Self {
            scopes: vec![Scope::default()],
            diagnostics: Diagnostics::new(),
            in_method: false,
            effect_depth: 0,
            loop_depth: 0,
            fn_depth: 0,
            fn_scope_base: None,
            const_env: HashMap::new(),
            struct_defaults: HashMap::new(),
            current_struct: None,
        }
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::error(message).with_span(span).with_code(code));
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        if self.scopes.is_empty() {
            self.scopes.push(Scope::default());
        }
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        self.lookup_depth(name).map(|(b, _)| b)
    }

    /// Binding and absolute scope index (0 = outermost module scope).
    fn lookup_scope_index(&self, name: &str) -> Option<(&Binding, usize)> {
        for (idx, scope) in self.scopes.iter().enumerate().rev() {
            if let Some((_, b)) = scope.names.iter().rev().find(|(n, _)| n == name) {
                return Some((b, idx));
            }
        }
        None
    }

    fn lookup_depth(&self, name: &str) -> Option<(&Binding, usize)> {
        // Kept for callers expecting depth-from-innermost.
        let n = self.scopes.len();
        self.lookup_scope_index(name)
            .map(|(b, idx)| (b, n - 1 - idx))
    }

    fn introduce(
        &mut self,
        name: &str,
        kind: BindingKind,
        return_shape: Option<ReturnShape>,
        span: Span,
    ) {
        self.introduce_full(
            name,
            kind,
            HashMap::new(),
            HashMap::new(),
            return_shape,
            span,
        );
    }

    fn introduce_module(&mut self, m: &ImportedModule) {
        let mut exports = HashMap::new();
        let mut export_shapes = HashMap::new();
        for ModuleExport {
            name,
            kind,
            return_shape,
            arity: _,
            return_ty: _,
        } in &m.exports
        {
            exports.insert(name.clone(), *kind);
            if let Some(rs) = return_shape {
                export_shapes.insert(name.clone(), *rs);
            }
        }
        self.introduce_full(
            &m.name,
            BindingKind::Module,
            exports,
            export_shapes,
            None,
            m.span,
        );
    }

    fn introduce_full(
        &mut self,
        name: &str,
        kind: BindingKind,
        exports: HashMap<String, BindingKind>,
        export_shapes: HashMap<String, ReturnShape>,
        return_shape: Option<ReturnShape>,
        span: Span,
    ) {
        if self.lookup(name).is_some() {
            self.error(
                "sem-shadow",
                format!("cannot reintroduce `{name}` (no shadowing)"),
                span,
            );
            return;
        }
        self.push_binding(name, kind, exports, export_shapes, return_shape);
    }

    /// Function / task parameters may reuse outer names (they hide outer bindings
    /// for the body). Only the current scope is checked for duplicates.
    fn introduce_param(&mut self, name: &str, span: Span) {
        if let Some(scope) = self.scopes.last() {
            if scope.names.iter().any(|(n, _)| n == name) {
                self.error(
                    "sem-shadow",
                    format!("cannot reintroduce `{name}` (no shadowing)"),
                    span,
                );
                return;
            }
        }
        self.push_binding(
            name,
            BindingKind::Immutable,
            HashMap::new(),
            HashMap::new(),
            None,
        );
    }

    fn push_binding(
        &mut self,
        name: &str,
        kind: BindingKind,
        exports: HashMap<String, BindingKind>,
        export_shapes: HashMap<String, ReturnShape>,
        return_shape: Option<ReturnShape>,
    ) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.names.push((
                name.to_string(),
                Binding {
                    kind,
                    exports,
                    return_shape,
                    export_shapes,
                },
            ));
        }
    }

    fn bind_stmt(&mut self, b: &BindStmt) {
        if b.leader == BindLeader::Hash && !is_screaming_snake(&b.name.name) {
            self.error(
                "sem-hash-name",
                format!(
                    "`#` constant `{}` must be SCREAMING_SNAKE ([A-Z][A-Z0-9_]*)",
                    b.name.name
                ),
                b.name.span,
            );
        }

        // Function value: introduce the binding before the body so self-calls
        // see a bound name (same “bind before use” rule as any other value).
        if let Some(Expr::Fn { params, body, .. }) = &b.init {
            let ret_shape = Some(effects_in_stmts(body).shape());
            self.finish_bind_introduce(b, ret_shape);
            let _ = self.check_fn(params, body, false);
            return;
        }

        let mut ret_shape = None;
        if let Some(init) = &b.init {
            if let Some(st) = self.current_struct.clone() {
                if !matches!(init, Expr::Fn { .. }) {
                    self.struct_defaults
                        .entry(st)
                        .or_default()
                        .entry(b.name.name.clone())
                        .or_insert_with(|| init.clone());
                }
            }
            if b.leader == BindLeader::Hash {
                // Const `#`: only literals + ops on other `#` (docs/syntax.md).
                match eval_const_expr_with_shapes(init, &self.const_env, &self.struct_defaults) {
                    Ok(v) => {
                        self.const_env.insert(b.name.name.clone(), v);
                    }
                    Err(err) => {
                        self.error("sem-const", err.message, b.span);
                    }
                }
            } else {
                // Init checked before the name is introduced (`$ a = a + 1` unbound).
                self.expr(init, UseContext::Value);
                // Rebind of a function value keeps its return shape (`$ f = load`).
                if let Expr::Name(n) = init {
                    ret_shape = self.lookup(&n.name).and_then(|b| b.return_shape);
                }
            }
        } else if b.leader == BindLeader::Hash {
            self.error(
                "sem-const",
                format!("`#` constant `{}` requires an initializer", b.name.name),
                b.name.span,
            );
        }

        self.finish_bind_introduce(b, ret_shape);
    }

    /// Introduce / reassign after init rules (or before body for function values).
    fn finish_bind_introduce(&mut self, b: &BindStmt, ret_shape: Option<ReturnShape>) {
        match b.leader {
            BindLeader::Tilde => match self.lookup(&b.name.name).map(|b| b.kind) {
                None => self.introduce(&b.name.name, BindingKind::Mutable, ret_shape, b.name.span),
                Some(BindingKind::Mutable) => {}
                Some(
                    BindingKind::Immutable
                    | BindingKind::Const
                    | BindingKind::Struct
                    | BindingKind::Module,
                ) => {
                    self.error(
                        "sem-immutable",
                        format!("cannot assign to immutable `{}`", b.name.name),
                        b.name.span,
                    );
                }
            },
            BindLeader::Dollar => {
                self.introduce(&b.name.name, BindingKind::Immutable, ret_shape, b.name.span);
            }
            BindLeader::Hash => {
                self.introduce(&b.name.name, BindingKind::Const, ret_shape, b.name.span);
            }
        }
    }

    fn check_fn(
        &mut self,
        params: &[echo_ast::Ident],
        body: &[Stmt],
        as_method: bool,
    ) -> ReturnShape {
        let shape = effects_in_stmts(body).shape();
        self.push_scope();
        self.fn_depth += 1;
        let prev_base = self.fn_scope_base;
        self.fn_scope_base = Some(self.scopes.len() - 1);
        let prev = self.in_method;
        self.in_method = as_method;
        for p in params {
            self.introduce_param(&p.name, p.span);
        }
        for st in body {
            stmt_(self, st);
        }
        self.in_method = prev;
        self.fn_scope_base = prev_base;
        self.fn_depth -= 1;
        self.pop_scope();
        shape
    }

    /// Shape of an expression if it is a call to a known function.
    fn expr_return_shape(&self, expr: &Expr) -> Option<ReturnShape> {
        match expr {
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::Name(n) => self.lookup(&n.name).and_then(|b| b.return_shape),
                Expr::Field { base, field, .. } => {
                    if let Expr::Name(mod_name) = base.as_ref() {
                        if let Some(b) = self.lookup(&mod_name.name) {
                            if b.kind == BindingKind::Module {
                                return b.export_shapes.get(&field.name).copied();
                            }
                        }
                    }
                    None
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn expr(&mut self, expr: &Expr, ctx: UseContext) {
        // Unhandled Option/Result: only allowed as match scrutinee, or inside
        // an `&` effect block (auto-unwrap / short-circuit).
        if ctx == UseContext::Value && self.effect_depth == 0 {
            if let Some(shape) = self.expr_return_shape(expr) {
                match shape {
                    ReturnShape::Option | ReturnShape::ResultOption => {
                        self.error(
                            "sem-unhandled-option",
                            "option value must be handled with `|` match (`$ name` / `: `)",
                            expr.span(),
                        );
                    }
                    ReturnShape::Result => {
                        self.error(
                            "sem-unhandled-result",
                            "result value must be handled with `|` match (`$ name` / `! name`)",
                            expr.span(),
                        );
                    }
                    ReturnShape::Plain => {}
                }
            }
        }

        match expr {
            Expr::Name(n) => self.check_name_use(n),
            Expr::Number { .. }
            | Expr::Duration { .. }
            | Expr::String { .. }
            | Expr::Bytes { .. }
            | Expr::Locator { .. }
            | Expr::Bool { .. } => {}
            Expr::Receiver { span } => {
                if !self.in_method {
                    self.error(
                        "sem-receiver",
                        "receiver `.` is only valid in a method body",
                        *span,
                    );
                }
            }
            Expr::Unary { op, expr, span } => {
                // Width tags bind to the literal after `>` (`<i32> -32`). A tag
                // must not appear under a unary (`-<i32> 32` / `!<i32> 1`).
                if matches!(op, UnaryOp::Neg | UnaryOp::Not) {
                    if let Some(w) = width_tag_of_numeric_lit(expr) {
                        self.error(
                            "sem-width-unary",
                            format!(
                                "width tag cannot follow a unary operator; write `<{w}> -N` (sign as part of the literal), not `- <{w}> N`"
                            ),
                            *span,
                        );
                    }
                }
                self.expr(expr, UseContext::Value);
            }
            Expr::WidthCast {
                width,
                tag,
                expr,
                span,
            } => {
                if width.is_none() {
                    self.error(
                        "sem-width-unknown",
                        format!(
                            "unknown width tag `{tag}`; use i8/i16/i32/i64, ui8/ui16/ui32/ui64, byte, f32, or f64"
                        ),
                        *span,
                    );
                }
                self.expr(expr, UseContext::Value);
            }
            Expr::Binary { left, right, .. } => {
                self.expr(left, UseContext::Value);
                self.expr(right, UseContext::Value);
            }
            Expr::Range { start, end, .. } => {
                self.expr(start, UseContext::Value);
                self.expr(end, UseContext::Value);
            }
            Expr::Call { callee, args, .. } => {
                // Callee itself is not a "value use" of return shape; the call is.
                self.expr_callee(callee);
                for a in args {
                    self.expr(a, UseContext::Value);
                }
            }
            Expr::Field { base, field, span } => {
                if let Expr::Name(mod_name) = base.as_ref() {
                    if let Some(b) = self.lookup(&mod_name.name) {
                        if b.kind == BindingKind::Module {
                            if !b.exports.contains_key(&field.name) {
                                self.error(
                                    "sem-module-export",
                                    format!(
                                        "`{}` is not exported by module `{}`",
                                        field.name, mod_name.name
                                    ),
                                    *span,
                                );
                            }
                            return;
                        }
                    }
                }
                self.expr(base, UseContext::Value);
            }
            Expr::Index { base, index, .. } => {
                self.expr(base, UseContext::Value);
                self.expr(index, UseContext::Value);
            }
            Expr::List { items, .. } => {
                for i in items {
                    self.expr(i, UseContext::Value);
                }
            }
            Expr::Object { fields, .. } => {
                for (_, e) in fields {
                    self.expr(e, UseContext::Value);
                }
            }
            Expr::StructLit { path, fields, span } => {
                self.check_struct_path(path, *span);
                for (_, e) in fields {
                    self.expr(e, UseContext::Value);
                }
            }
            Expr::Fn { params, body, .. } => {
                let _ = self.check_fn(params, body, false);
            }
            Expr::Group { expr, .. } => self.expr(expr, ctx),
        }
    }

    /// Name as value **or** callee: unbound + closed-function capture rules.
    ///
    /// Outer **function values** (binds with a known return shape) may be used
    /// from nested closed bodies — they lower to code refs, not env capture.
    /// Outer **data** `$`/`~`/params without a return shape → `sem-capture`.
    /// Call sites must use this path too: historically only value uses checked
    /// capture, so `f(x)` on an outer param SEGV'd instead of diagnosing.
    fn check_name_use(&mut self, n: &echo_ast::Ident) {
        if self.lookup(&n.name).is_none() {
            self.error("sem-unbound", format!("unbound name `{}`", n.name), n.span);
            return;
        }
        let Some(base) = self.fn_scope_base else {
            return;
        };
        // Nested block/loop scopes inside the function are not "outer".
        if let Some((b, idx)) = self.lookup_scope_index(&n.name) {
            if idx < base
                && matches!(b.kind, BindingKind::Mutable | BindingKind::Immutable)
                && b.return_shape.is_none()
            {
                self.error(
                    "sem-capture",
                    format!(
                        "functions are closed: cannot use outer binding `{}` (pass it as a parameter, or put state on a struct and use a method)",
                        n.name
                    ),
                    n.span,
                );
            }
        }
    }

    fn expr_callee(&mut self, expr: &Expr) {
        match expr {
            Expr::Name(n) => self.check_name_use(n),
            Expr::Field { base, field, span } => {
                if let Expr::Name(mod_name) = base.as_ref() {
                    if let Some(b) = self.lookup(&mod_name.name) {
                        if b.kind == BindingKind::Module {
                            if !b.exports.contains_key(&field.name) {
                                self.error(
                                    "sem-module-export",
                                    format!(
                                        "`{}` is not exported by module `{}`",
                                        field.name, mod_name.name
                                    ),
                                    *span,
                                );
                            }
                            return;
                        }
                    }
                }
                self.expr(base, UseContext::Value);
            }
            other => self.expr(other, UseContext::Value),
        }
    }

    fn check_struct_path(&mut self, path: &[echo_ast::Ident], span: Span) {
        match path {
            [] => {}
            [_] => {}
            [module, ty, rest @ ..] => {
                if !rest.is_empty() {
                    self.error(
                        "sem-struct-path",
                        "struct type path must be `Type` or `module.Type`",
                        span,
                    );
                    return;
                }
                if let Some(b) = self.lookup(&module.name) {
                    if b.kind == BindingKind::Module {
                        match b.exports.get(&ty.name) {
                            Some(BindingKind::Struct) => {}
                            Some(_) => {
                                self.error(
                                    "sem-module-export",
                                    format!(
                                        "`{}` on module `{}` is not a struct type",
                                        ty.name, module.name
                                    ),
                                    ty.span,
                                );
                            }
                            None => {
                                self.error(
                                    "sem-module-export",
                                    format!(
                                        "`{}` is not exported by module `{}`",
                                        ty.name, module.name
                                    ),
                                    ty.span,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UseContext {
    /// Value position: Result/Option must already be handled.
    Value,
    /// Match scrutinee: handling is the match itself.
    Scrutinee,
}

pub(crate) fn analyze(file: &File, modules: &[ImportedModule]) -> Diagnostics {
    let mut cx = Cx::new();
    for m in modules {
        cx.introduce_module(m);
    }
    for stmt in &file.stmts {
        stmt_(&mut cx, stmt);
    }
    cx.diagnostics
}

fn stmt_(cx: &mut Cx, stmt: &Stmt) {
    match stmt {
        Stmt::Bind(b) => cx.bind_stmt(b),
        Stmt::Assign(a) => {
            match &a.target {
                AssignTarget::Name(n) => match cx.lookup(&n.name).map(|b| b.kind) {
                    Some(BindingKind::Mutable) => {}
                    Some(
                        BindingKind::Immutable
                        | BindingKind::Const
                        | BindingKind::Struct
                        | BindingKind::Module,
                    ) => {
                        cx.error(
                            "sem-immutable",
                            format!("cannot assign to immutable `{}`", n.name),
                            n.span,
                        );
                    }
                    None => {
                        cx.error("sem-unbound", format!("unbound name `{}`", n.name), n.span);
                    }
                },
                AssignTarget::Field { base, .. } => {
                    cx.expr(base, UseContext::Value);
                }
                AssignTarget::Index { base, index } => {
                    cx.expr(base, UseContext::Value);
                    if let Some(index) = index {
                        cx.expr(index, UseContext::Value);
                    }
                }
            }
            cx.expr(&a.value, UseContext::Value);
        }
        Stmt::Struct(s) => struct_body(cx, s, true),
        Stmt::StructExt(s) => struct_body(cx, s, false),
        Stmt::If(s) => {
            cx.expr(&s.cond, UseContext::Value);
            block(cx, &s.body);
        }
        Stmt::ElseIf(s) => {
            cx.expr(&s.cond, UseContext::Value);
            block(cx, &s.body);
        }
        Stmt::Else(s) => block(cx, &s.body),
        Stmt::ErrorReturn(s) => {
            // `!` is Result-err return from a function (`docs/semantics.md`).
            // Top-level `^` is process status; top-level `!` is not.
            if cx.fn_depth == 0 {
                cx.error(
                    "sem-error-return",
                    "`!` error return is only legal inside a function",
                    s.span,
                );
            }
            cx.expr(&s.value, UseContext::Value);
        }
        Stmt::Return(s) => {
            // Top-level is the program body — `^` returns process status, not "outside function".
            if let Some(v) = &s.value {
                cx.expr(v, UseContext::Value);
            }
        }
        Stmt::Loop(s) => {
            match &s.kind {
                LoopKind::Infinite => {}
                LoopKind::While(e) => cx.expr(e, UseContext::Value),
                LoopKind::For { item, iter } => {
                    cx.expr(iter, UseContext::Value);
                    cx.push_scope();
                    cx.introduce(&item.name, BindingKind::Immutable, None, item.span);
                    cx.loop_depth += 1;
                    for st in &s.body {
                        stmt_(cx, st);
                    }
                    cx.loop_depth -= 1;
                    cx.pop_scope();
                    return;
                }
            }
            cx.loop_depth += 1;
            block(cx, &s.body);
            cx.loop_depth -= 1;
        }
        Stmt::Break { span } => {
            if cx.loop_depth == 0 {
                cx.error("sem-break", "break outside loop", *span);
            }
        }
        Stmt::Continue { span } => {
            if cx.loop_depth == 0 {
                cx.error("sem-continue", "continue outside loop", *span);
            }
        }
        Stmt::Match(s) => {
            let shape = cx.expr_return_shape(&s.scrutinee);
            cx.expr(&s.scrutinee, UseContext::Scrutinee);
            validate_match_arms(cx, shape, &s.arms, s.span);
            for arm in &s.arms {
                cx.push_scope();
                match &arm.kind {
                    MatchArmKind::BindOk { name } | MatchArmKind::BindErr { name } => {
                        cx.introduce(&name.name, BindingKind::Immutable, None, name.span);
                    }
                    MatchArmKind::Type { name } => match cx.lookup(&name.name).map(|b| b.kind) {
                        Some(BindingKind::Struct) => {}
                        Some(_) => {
                            cx.error(
                                "sem-match-type",
                                format!("`% {}` is not a struct type", name.name),
                                name.span,
                            );
                        }
                        None => {
                            cx.error(
                                "sem-match-type",
                                format!("unknown struct type `% {}`", name.name),
                                name.span,
                            );
                        }
                    },
                    MatchArmKind::Values(ps) => {
                        for p in ps {
                            cx.expr(p, UseContext::Value);
                        }
                    }
                    MatchArmKind::Default => {}
                }
                for st in &arm.body {
                    stmt_(cx, st);
                }
                cx.pop_scope();
            }
        }
        Stmt::Import(_) | Stmt::Export(_) => {}
        Stmt::TaskSpawn(s) => {
            match &s.body {
                echo_ast::TaskBody::Block(body) => {
                    // Task `{ }` bodies are closed activations — `!` is legal.
                    cx.fn_depth += 1;
                    block(cx, body);
                    cx.fn_depth -= 1;
                }
                echo_ast::TaskBody::Call(e) => {
                    if let echo_ast::Expr::Call { args, .. } = e {
                        if args.len() > 8 {
                            cx.error(
                                "sem-task-arity",
                                format!(
                                    "task spawn call has {} arguments (max 8 in v0)",
                                    args.len()
                                ),
                                s.span,
                            );
                        }
                    }
                    cx.expr(e, UseContext::Value);
                }
                echo_ast::TaskBody::Closure { captures, body } => {
                    // Captures resolve in the **parent** scope only. Unbound → hard error.
                    // By reference: pass the binding's runtime value (heap handle identity);
                    // no deep clone. Body params share those handles.
                    if captures.len() > 8 {
                        let span = captures.last().map(|c| c.span).unwrap_or(s.span);
                        cx.error(
                            "sem-task-arity",
                            format!(
                                "task capture list has {} names (max 8 in v0)",
                                captures.len()
                            ),
                            span,
                        );
                    }
                    for c in captures {
                        if cx.lookup(&c.name).is_none() {
                            cx.error(
                                "sem-task-capture",
                                format!(
                                    "task capture `{}` is unbound (must name an existing binding)",
                                    c.name
                                ),
                                c.span,
                            );
                        }
                    }
                    cx.push_scope();
                    cx.fn_depth += 1;
                    let prev_base = cx.fn_scope_base;
                    cx.fn_scope_base = Some(cx.scopes.len() - 1);
                    for c in captures.iter().take(8) {
                        cx.introduce_param(&c.name, c.span);
                    }
                    block(cx, body);
                    cx.fn_scope_base = prev_base;
                    cx.fn_depth -= 1;
                    cx.pop_scope();
                }
            }
            if let Some(name) = &s.bind {
                cx.introduce(&name.name, BindingKind::Immutable, None, name.span);
            }
        }
        Stmt::TaskJoin(s) => match &s.kind {
            echo_ast::TaskJoinKind::Block { bind, body } => {
                cx.fn_depth += 1;
                block(cx, body);
                cx.fn_depth -= 1;
                if let Some(name) = bind {
                    cx.introduce(&name.name, BindingKind::Immutable, None, name.span);
                }
            }
            echo_ast::TaskJoinKind::Handle { bind, handle } => {
                cx.expr(handle, UseContext::Value);
                if let Some(name) = bind {
                    cx.introduce(&name.name, BindingKind::Immutable, None, name.span);
                }
            }
        },
        Stmt::EffectBlock(s) => {
            // Body is checked in a nested scope; auto-unwrap suppresses
            // unhandled-result/option. Bind is introduced after the block.
            cx.push_scope();
            cx.effect_depth += 1;
            block(cx, &s.body);
            cx.effect_depth -= 1;
            cx.pop_scope();
            if let Some(name) = &s.bind {
                cx.introduce(&name.name, BindingKind::Immutable, None, name.span);
            }
        }
        Stmt::Expr(e) => cx.expr(e, UseContext::Value),
        // Expanded to Bind before check; kept for exhaustiveness if a host skips expand.
        Stmt::MultiBind(m) => {
            for b in m.clone().into_binds() {
                stmt_(cx, &Stmt::Bind(b));
            }
        }
    }
}

fn validate_match_arms(
    cx: &mut Cx,
    shape: Option<ReturnShape>,
    arms: &[echo_ast::MatchArm],
    span: Span,
) {
    let mut has_ok = false;
    let mut has_err = false;
    let mut has_default = false;
    let mut has_lit = false;
    let mut has_type = false;
    for arm in arms {
        match &arm.kind {
            MatchArmKind::BindOk { .. } => has_ok = true,
            MatchArmKind::BindErr { .. } => has_err = true,
            MatchArmKind::Default => has_default = true,
            MatchArmKind::Values(_) => has_lit = true,
            MatchArmKind::Type { .. } => has_type = true,
        }
    }
    let has_ordinary = has_lit || has_type;

    match shape {
        Some(ReturnShape::Option) | Some(ReturnShape::ResultOption) => {
            // Option: need $ and :
            if has_err {
                cx.error(
                    "sem-match-arm",
                    "Option match cannot use `! name` err arms (use `: ` for none)",
                    span,
                );
            }
            if !has_ok || !has_default {
                cx.error(
                    "sem-match-incomplete",
                    "Option match needs `$ name { … }` (some) and `: { … }` (none)",
                    span,
                );
            }
            if has_ordinary {
                cx.error(
                    "sem-match-arm",
                    "Option match cannot mix value/`% type` arms with `$` / `:`",
                    span,
                );
            }
        }
        Some(ReturnShape::Result) => {
            if has_default && !has_err {
                // : alone is wrong for Result
                cx.error(
                    "sem-match-arm",
                    "Result match uses `! name { … }` for err, not `: `",
                    span,
                );
            }
            if !has_ok || !has_err {
                cx.error(
                    "sem-match-incomplete",
                    "Result match needs `$ name { … }` (ok) and `! name { … }` (err)",
                    span,
                );
            }
            if has_ordinary {
                cx.error(
                    "sem-match-arm",
                    "Result match cannot mix value/`% type` arms with `$` / `!`",
                    span,
                );
            }
        }
        Some(ReturnShape::Plain) | None => {
            // Ordinary or unknown: empty `| expr { }` is incomplete (docs/semantics.md).
            if arms.is_empty() {
                cx.error("sem-match-incomplete", "match needs at least one arm", span);
            }
            // Ordinary or unknown: allow literal/default; reject ok/err if present without Result
            if has_ok || has_err {
                // Might still be valid if we don't know shape — treat as Result dialect
                if !has_ok || (!has_err && !has_default) {
                    // incomplete result-like
                }
                if has_ok && has_err {
                    // assume Result handling for unknown call
                } else if has_ok && has_default && !has_err {
                    // assume Option for unknown
                }
            }
        }
    }
}

fn struct_body(cx: &mut Cx, s: &StructStmt, redeclare_name: bool) {
    if redeclare_name {
        cx.introduce(&s.name.name, BindingKind::Struct, None, s.name.span);
    }

    let prev = cx.current_struct.replace(s.name.name.clone());
    cx.push_scope();
    for m in &s.members {
        match m {
            Stmt::Bind(b) => {
                if let Some(Expr::Fn { params, body, .. }) = &b.init {
                    let shape = cx.check_fn(params, body, true);
                    member_bind_name(cx, b, Some(shape));
                } else {
                    cx.bind_stmt(b);
                }
            }
            other => stmt_(cx, other),
        }
    }
    cx.pop_scope();
    cx.current_struct = prev;
}

fn member_bind_name(cx: &mut Cx, b: &BindStmt, return_shape: Option<ReturnShape>) {
    if b.leader == BindLeader::Hash && !is_screaming_snake(&b.name.name) {
        cx.error(
            "sem-hash-name",
            format!(
                "`#` constant `{}` must be SCREAMING_SNAKE ([A-Z][A-Z0-9_]*)",
                b.name.name
            ),
            b.name.span,
        );
    }
    let kind = match b.leader {
        BindLeader::Tilde => BindingKind::Mutable,
        BindLeader::Dollar => BindingKind::Immutable,
        BindLeader::Hash => BindingKind::Const,
    };
    cx.introduce(&b.name.name, kind, return_shape, b.name.span);
}

fn block(cx: &mut Cx, body: &[Stmt]) {
    cx.push_scope();
    for st in body {
        stmt_(cx, st);
    }
    cx.pop_scope();
}

fn is_screaming_snake(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Width name if `expr` is a (possibly grouped) width-tagged numeric literal.
fn width_tag_of_numeric_lit(expr: &Expr) -> Option<&'static str> {
    let mut e = expr;
    while let Expr::Group { expr: inner, .. } = e {
        e = inner.as_ref();
    }
    match e {
        Expr::Number { width: Some(w), .. } => Some(w.as_str()),
        _ => None,
    }
}
