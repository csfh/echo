//! Function result shapes: plain, Option, Result (docs/semantics.md).

use echo_ast::{Expr, Stmt};

/// What a function returns, from `^` / `!` paths in its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReturnShape {
    /// No `!`; not option-shaped (no bare `^` + value `^` pair).
    Plain,
    /// Bare `^` + valued `^`, no `!`.
    Option,
    /// At least one `!`, no option bare/`^` mix beyond Result ok.
    Result,
    /// `!` plus bare `^` and valued `^` → Result(Option[T], E).
    ResultOption,
}

#[derive(Debug, Default, Clone)]
pub struct PathEffects {
    pub has_error_return: bool,
    pub has_bare_return: bool,
    pub has_value_return: bool,
}

impl PathEffects {
    pub fn observe_return(&mut self, value: Option<&Expr>) {
        match value {
            None => self.has_bare_return = true,
            Some(_) => self.has_value_return = true,
        }
    }

    pub fn observe_error_return(&mut self) {
        self.has_error_return = true;
    }

    #[must_use]
    pub fn shape(&self) -> ReturnShape {
        if self.has_error_return {
            if self.has_bare_return && self.has_value_return {
                ReturnShape::ResultOption
            } else {
                ReturnShape::Result
            }
        } else if self.has_bare_return && self.has_value_return {
            ReturnShape::Option
        } else {
            ReturnShape::Plain
        }
    }
}

/// Walk statements counting `^` / `!` (not nested function bodies).
pub fn effects_in_stmts(stmts: &[Stmt]) -> PathEffects {
    let mut e = PathEffects::default();
    for s in stmts {
        observe_stmt(&mut e, s);
    }
    e
}

fn observe_stmt(e: &mut PathEffects, stmt: &Stmt) {
    match stmt {
        Stmt::ErrorReturn(_) => e.observe_error_return(),
        Stmt::Return(r) => e.observe_return(r.value.as_ref()),
        Stmt::If(s) => {
            for st in &s.body {
                observe_stmt(e, st);
            }
        }
        Stmt::ElseIf(s) => {
            for st in &s.body {
                observe_stmt(e, st);
            }
        }
        Stmt::Else(s) => {
            for st in &s.body {
                observe_stmt(e, st);
            }
        }
        Stmt::Loop(s) => {
            for st in &s.body {
                observe_stmt(e, st);
            }
        }
        Stmt::Match(s) => {
            for arm in &s.arms {
                for st in &arm.body {
                    observe_stmt(e, st);
                }
            }
        }
        Stmt::Bind(b) => {
            // Nested function values: do not count their ^/! for outer shape.
            if let Some(Expr::Fn { .. }) = &b.init {
                return;
            }
            if let Some(init) = &b.init {
                observe_expr_nested_fn_ignored(e, init);
            }
        }
        Stmt::Assign(a) => {
            observe_expr_nested_fn_ignored(e, &a.value);
            match &a.target {
                echo_ast::AssignTarget::Index { index, .. } => {
                    if let Some(index) = index {
                        observe_expr_nested_fn_ignored(e, index);
                    }
                }
                echo_ast::AssignTarget::Field { base, .. } => {
                    observe_expr_nested_fn_ignored(e, base);
                }
                echo_ast::AssignTarget::Name(_) => {}
            }
        }
        Stmt::Expr(ex) => observe_expr_nested_fn_ignored(e, ex),
        _ => {}
    }
}

fn observe_expr_nested_fn_ignored(_e: &mut PathEffects, _expr: &Expr) {
    // Returns inside nested exprs only appear as Stmt::Return in fn bodies.
}
