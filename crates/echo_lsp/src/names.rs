//! Name occurrences and position lookup over shared AST (no private parser).

use echo_ast::{
    AssignTarget, Expr, File, Ident, ImportPathSeg, MatchArmKind, Stmt, TaskBody, TaskJoinKind,
};
use echo_source::Span;

/// How a name appears in source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRole {
    /// `$` / `~` / `#` bind name.
    BindDef,
    /// `%` / `@` struct type name.
    StructDef,
    /// Struct member bind name.
    MemberDef,
    /// Export list name.
    Export,
    /// Import path segment.
    ImportSeg,
    /// Function / closure parameter.
    Param,
    /// Expression / assign / field / call use.
    Use,
    /// Field name in struct lit / object / access.
    Field,
}

/// One identifier occurrence with role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameHit {
    pub name: String,
    pub span: Span,
    pub role: NameRole,
}

/// Collect every identifier in the file with a role.
#[must_use]
pub fn collect_names(file: &File) -> Vec<NameHit> {
    let mut out = Vec::new();
    for stmt in &file.stmts {
        walk_stmt(stmt, &mut out, false);
    }
    out
}

/// Smallest name whose span covers `byte_offset` (prefer innermost).
#[must_use]
pub fn name_at_offset(file: &File, byte_offset: u32) -> Option<NameHit> {
    let mut best: Option<NameHit> = None;
    for hit in collect_names(file) {
        if span_contains(hit.span, byte_offset) {
            let take = match &best {
                None => true,
                Some(cur) => hit.span.len() <= cur.span.len(),
            };
            if take {
                best = Some(hit);
            }
        }
    }
    best
}

/// All occurrences of `name` (exact string match) in the file.
#[must_use]
pub fn references_to(file: &File, name: &str) -> Vec<NameHit> {
    collect_names(file)
        .into_iter()
        .filter(|h| h.name == name)
        .collect()
}

#[must_use]
pub fn span_contains(span: Span, byte_offset: u32) -> bool {
    byte_offset >= span.start.0 && byte_offset < span.end.0.max(span.start.0.saturating_add(1))
        || (span.start.0 == span.end.0 && byte_offset == span.start.0)
}

fn push_ident(out: &mut Vec<NameHit>, id: &Ident, role: NameRole) {
    out.push(NameHit {
        name: id.name.clone(),
        span: id.span,
        role,
    });
}

fn walk_stmt(stmt: &Stmt, out: &mut Vec<NameHit>, in_struct: bool) {
    match stmt {
        Stmt::Bind(b) => {
            push_ident(
                out,
                &b.name,
                if in_struct {
                    NameRole::MemberDef
                } else {
                    NameRole::BindDef
                },
            );
            if let Some(e) = &b.init {
                walk_expr(e, out);
            }
        }
        Stmt::MultiBind(m) => {
            for item in &m.items {
                push_ident(
                    out,
                    &item.name,
                    if in_struct {
                        NameRole::MemberDef
                    } else {
                        NameRole::BindDef
                    },
                );
                if let Some(e) = &item.init {
                    walk_expr(e, out);
                }
            }
        }
        Stmt::Assign(a) => {
            match &a.target {
                AssignTarget::Name(n) => push_ident(out, n, NameRole::Use),
                AssignTarget::Field { base, field } => {
                    walk_expr(base, out);
                    push_ident(out, field, NameRole::Field);
                }
                AssignTarget::Index { base, index } => {
                    walk_expr(base, out);
                    if let Some(index) = index {
                        walk_expr(index, out);
                    }
                }
            }
            walk_expr(&a.value, out);
        }
        Stmt::Struct(s) | Stmt::StructExt(s) => {
            push_ident(out, &s.name, NameRole::StructDef);
            for m in &s.members {
                walk_stmt(m, out, true);
            }
        }
        Stmt::If(i) => {
            walk_expr(&i.cond, out);
            for s in &i.body {
                walk_stmt(s, out, in_struct);
            }
        }
        Stmt::ElseIf(i) => {
            walk_expr(&i.cond, out);
            for s in &i.body {
                walk_stmt(s, out, in_struct);
            }
        }
        Stmt::Else(e) => {
            for s in &e.body {
                walk_stmt(s, out, in_struct);
            }
        }
        Stmt::ErrorReturn(e) => walk_expr(&e.value, out),
        Stmt::Return(r) => {
            if let Some(e) = &r.value {
                walk_expr(e, out);
            }
        }
        Stmt::Loop(l) => {
            match &l.kind {
                echo_ast::LoopKind::While(cond) => walk_expr(cond, out),
                echo_ast::LoopKind::For { item, iter } => {
                    push_ident(out, item, NameRole::BindDef);
                    walk_expr(iter, out);
                }
                echo_ast::LoopKind::Infinite => {}
            }
            for s in &l.body {
                walk_stmt(s, out, in_struct);
            }
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
        Stmt::Match(m) => {
            walk_expr(&m.scrutinee, out);
            for arm in &m.arms {
                match &arm.kind {
                    MatchArmKind::Values(vs) => {
                        for v in vs {
                            walk_expr(v, out);
                        }
                    }
                    MatchArmKind::Type { name } => push_ident(out, name, NameRole::Use),
                    MatchArmKind::BindOk { name } | MatchArmKind::BindErr { name } => {
                        push_ident(out, name, NameRole::BindDef);
                    }
                    MatchArmKind::Default => {}
                }
                for s in &arm.body {
                    walk_stmt(s, out, in_struct);
                }
            }
        }
        Stmt::TaskSpawn(t) => {
            if let Some(b) = &t.bind {
                push_ident(out, b, NameRole::BindDef);
            }
            walk_task_body(&t.body, out, in_struct);
        }
        Stmt::TaskJoin(t) => match &t.kind {
            TaskJoinKind::Block { bind, body } => {
                if let Some(b) = bind {
                    push_ident(out, b, NameRole::BindDef);
                }
                for s in body {
                    walk_stmt(s, out, in_struct);
                }
            }
            TaskJoinKind::Handle { bind, handle } => {
                if let Some(b) = bind {
                    push_ident(out, b, NameRole::BindDef);
                }
                walk_expr(handle, out);
            }
        },
        Stmt::EffectBlock(e) => {
            if let Some(b) = &e.bind {
                push_ident(out, b, NameRole::BindDef);
            }
            for s in &e.body {
                walk_stmt(s, out, in_struct);
            }
        }
        Stmt::Import(i) => {
            for seg in &i.path {
                if let ImportPathSeg::Name(n) = seg {
                    push_ident(out, n, NameRole::ImportSeg);
                }
            }
        }
        Stmt::Export(e) => {
            for n in &e.names {
                push_ident(out, n, NameRole::Export);
            }
        }
        Stmt::Expr(e) => walk_expr(e, out),
    }
}

fn walk_task_body(body: &TaskBody, out: &mut Vec<NameHit>, in_struct: bool) {
    match body {
        TaskBody::Block(stmts) => {
            for s in stmts {
                walk_stmt(s, out, in_struct);
            }
        }
        TaskBody::Call(e) => walk_expr(e, out),
        TaskBody::Closure { captures, body } => {
            for c in captures {
                push_ident(out, c, NameRole::Param);
            }
            for s in body {
                walk_stmt(s, out, in_struct);
            }
        }
    }
}

fn walk_expr(expr: &Expr, out: &mut Vec<NameHit>) {
    match expr {
        Expr::Name(i) => push_ident(out, i, NameRole::Use),
        Expr::Number { .. }
        | Expr::Duration { .. }
        | Expr::String { .. }
        | Expr::Bytes { .. }
        | Expr::Locator { .. }
        | Expr::Bool { .. }
        | Expr::Receiver { .. } => {}
        Expr::Unary { expr, .. } | Expr::WidthCast { expr, .. } => walk_expr(expr, out),
        Expr::Binary { left, right, .. } | Expr::Range { start: left, end: right, .. } => {
            walk_expr(left, out);
            walk_expr(right, out);
        }
        Expr::Call { callee, args, .. } => {
            walk_expr(callee, out);
            for a in args {
                walk_expr(a, out);
            }
        }
        Expr::Field { base, field, .. } => {
            walk_expr(base, out);
            push_ident(out, field, NameRole::Field);
        }
        Expr::Index { base, index, .. } => {
            walk_expr(base, out);
            walk_expr(index, out);
        }
        Expr::List { items, .. } => {
            for i in items {
                walk_expr(i, out);
            }
        }
        Expr::Object { fields, .. } => {
            for (k, v) in fields {
                push_ident(out, k, NameRole::Field);
                walk_expr(v, out);
            }
        }
        Expr::StructLit { path, fields, .. } => {
            for p in path {
                push_ident(out, p, NameRole::Use);
            }
            for (k, v) in fields {
                push_ident(out, k, NameRole::Field);
                walk_expr(v, out);
            }
        }
        Expr::Fn { params, body, .. } => {
            for p in params {
                push_ident(out, p, NameRole::Param);
            }
            for s in body {
                walk_stmt(s, out, false);
            }
        }
        Expr::Group { expr, .. } => walk_expr(expr, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_parser::parse;
    use echo_source::SourceMap;

    fn file(src: &str) -> File {
        let mut map = SourceMap::new();
        let id = map.add("t.echo", src);
        parse(map.get(id).unwrap()).file.unwrap()
    }

    #[test]
    fn finds_bind_use_at_offset() {
        let src = "$ answer = 42\n$ x = answer\n";
        let f = file(src);
        // offset of 'a' in second-line answer
        let off = src.find("= answer").unwrap() as u32 + 2;
        let hit = name_at_offset(&f, off).expect("hit");
        assert_eq!(hit.name, "answer");
        assert_eq!(hit.role, NameRole::Use);
    }

    #[test]
    fn collects_struct_and_member() {
        let f = file("% counter {\n    ~ n = 0\n}\n");
        let names: Vec<_> = collect_names(&f).into_iter().map(|h| h.name).collect();
        assert!(names.contains(&"counter".to_string()));
        assert!(names.contains(&"n".to_string()));
    }
}
