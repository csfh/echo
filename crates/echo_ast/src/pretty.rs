//! Canonical Echo pretty-printer (shared pipeline host: `xo fmt`).
//!
//! Rules: `docs/syntax.md`, `docs/pipeline.md` § formatter — leaders, same-line
//! `{`, indentation, no trailing commas. Does not change program meaning.

use crate::{
    AssignTarget, BinaryOp, BindLeader, BindStmt, Expr, File, Ident, ImportPathSeg, LoopKind,
    MatchArmKind, MultiBindStmt, Stmt, StringKind, TaskBody, TaskJoinKind, UnaryOp, Width,
};

const INDENT: &str = "    ";

/// Format a complete file to canonical source text (trailing newline).
#[must_use]
pub fn format_file(file: &File) -> String {
    let mut out = String::new();
    for stmt in &file.stmts {
        write_stmt(stmt, 0, &mut out);
        out.push('\n');
    }
    if out.is_empty() {
        out.push('\n');
    }
    out
}

fn write_indent(level: usize, out: &mut String) {
    for _ in 0..level {
        out.push_str(INDENT);
    }
}

fn write_block(body: &[Stmt], level: usize, out: &mut String) {
    out.push_str(" {");
    if body.is_empty() {
        out.push('\n');
        write_indent(level, out);
        out.push('}');
        return;
    }
    out.push('\n');
    for s in body {
        write_indent(level + 1, out);
        write_stmt(s, level + 1, out);
        out.push('\n');
    }
    write_indent(level, out);
    out.push('}');
}

fn write_stmt(stmt: &Stmt, level: usize, out: &mut String) {
    match stmt {
        Stmt::Bind(b) => write_bind(b, level, out),
        Stmt::MultiBind(m) => write_multi_bind(m, level, out),
        Stmt::Assign(a) => {
            out.push_str("~ ");
            write_assign_target(&a.target, level, out);
            out.push_str(" = ");
            write_expr(&a.value, 0, level, out);
        }
        Stmt::Struct(s) => {
            out.push_str("% ");
            out.push_str(&s.name.name);
            write_block(&s.members, level, out);
        }
        Stmt::StructExt(s) => {
            out.push_str("@ ");
            out.push_str(&s.name.name);
            write_block(&s.members, level, out);
        }
        Stmt::If(s) => {
            out.push_str("? ");
            write_expr(&s.cond, 0, level, out);
            write_block(&s.body, level, out);
        }
        Stmt::ElseIf(s) => {
            out.push_str(": ");
            write_expr(&s.cond, 0, level, out);
            write_block(&s.body, level, out);
        }
        Stmt::Else(s) => {
            out.push_str(":");
            write_block(&s.body, level, out);
        }
        Stmt::ErrorReturn(s) => {
            out.push_str("! ");
            write_expr(&s.value, 0, level, out);
        }
        Stmt::Return(s) => match &s.value {
            None => out.push('^'),
            Some(v) => {
                out.push_str("^ ");
                write_expr(v, 0, level, out);
            }
        },
        Stmt::Loop(s) => match &s.kind {
            LoopKind::Infinite => {
                out.push('*');
                write_block(&s.body, level, out);
            }
            LoopKind::While(e) => {
                out.push_str("* ");
                write_expr(e, 0, level, out);
                write_block(&s.body, level, out);
            }
            LoopKind::For { item, iter } => {
                out.push_str("* ");
                out.push_str(&item.name);
                out.push_str(" : ");
                write_expr(iter, 0, level, out);
                write_block(&s.body, level, out);
            }
        },
        Stmt::Break { .. } => out.push('<'),
        Stmt::Continue { .. } => out.push('>'),
        Stmt::Match(m) => {
            out.push_str("| ");
            write_expr(&m.scrutinee, 0, level, out);
            out.push_str(" {");
            out.push('\n');
            for arm in &m.arms {
                write_indent(level + 1, out);
                match &arm.kind {
                    MatchArmKind::Values(ps) => {
                        for (i, p) in ps.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            write_expr(p, 0, level, out);
                        }
                    }
                    MatchArmKind::Type { name } => {
                        out.push_str("% ");
                        out.push_str(&name.name);
                    }
                    MatchArmKind::BindOk { name } => {
                        out.push_str("$ ");
                        out.push_str(&name.name);
                    }
                    MatchArmKind::BindErr { name } => {
                        out.push_str("! ");
                        out.push_str(&name.name);
                    }
                    MatchArmKind::Default => out.push(':'),
                }
                write_block(&arm.body, level + 1, out);
                out.push('\n');
            }
            write_indent(level, out);
            out.push('}');
        }
        Stmt::TaskSpawn(s) => {
            out.push('+');
            if let Some(b) = &s.bind {
                out.push(' ');
                out.push_str(&b.name);
                out.push_str(" =");
            }
            write_task_body(&s.body, level, out);
        }
        Stmt::TaskJoin(s) => match &s.kind {
            TaskJoinKind::Block { bind, body } => {
                out.push('-');
                if let Some(b) = bind {
                    out.push(' ');
                    out.push_str(&b.name);
                    out.push_str(" =");
                }
                write_block(body, level, out);
            }
            TaskJoinKind::Handle { bind, handle } => {
                out.push_str("- ");
                if let Some(b) = bind {
                    out.push_str(&b.name);
                    out.push_str(" = ");
                }
                write_expr(handle, 0, level, out);
            }
        },
        Stmt::EffectBlock(s) => {
            out.push('&');
            if let Some(b) = &s.bind {
                out.push(' ');
                out.push_str(&b.name);
                out.push_str(" =");
            }
            write_block(&s.body, level, out);
        }
        Stmt::Import(s) => {
            out.push_str("/ ");
            write_import_path(&s.path, out);
        }
        Stmt::Export(s) => {
            out.push_str("\\ ");
            for (i, n) in s.names.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&n.name);
            }
        }
        Stmt::Expr(e) => write_expr(e, 0, level, out),
    }
}

fn write_bind(b: &BindStmt, level: usize, out: &mut String) {
    out.push_str(bind_leader_glyph(b.leader));
    out.push(' ');
    out.push_str(&b.name.name);
    if let Some(init) = &b.init {
        out.push_str(" = ");
        write_expr(init, 0, level, out);
    }
}

fn write_multi_bind(m: &MultiBindStmt, level: usize, out: &mut String) {
    out.push_str(bind_leader_glyph(m.leader));
    out.push(' ');
    for (i, it) in m.items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&it.name.name);
        if let Some(init) = &it.init {
            out.push_str(" = ");
            write_expr(init, 0, level, out);
        }
    }
}

fn bind_leader_glyph(l: BindLeader) -> &'static str {
    match l {
        BindLeader::Tilde => "~",
        BindLeader::Dollar => "$",
        BindLeader::Hash => "#",
    }
}

fn write_assign_target(t: &AssignTarget, level: usize, out: &mut String) {
    match t {
        AssignTarget::Name(n) => out.push_str(&n.name),
        AssignTarget::Field { base, field } => {
            if matches!(base, Expr::Receiver { .. }) {
                out.push('.');
            } else {
                write_expr(base, 0, level, out);
                out.push('.');
            }
            out.push_str(&field.name);
        }
        AssignTarget::Index { base, index } => {
            write_expr(base, 0, level, out);
            out.push('[');
            if let Some(index) = index {
                write_expr(index, 0, level, out);
            }
            out.push(']');
        }
    }
}

fn write_import_path(path: &[ImportPathSeg], out: &mut String) {
    // `./a/b` → Dot, Name(a), Name(b); `std/io` → Name(std), Name(io);
    // `github.com/x` → Name("github.com"), Name(x) (parser coalesces host dots).
    let mut i = 0;
    if matches!(path.first(), Some(ImportPathSeg::Dot)) {
        out.push_str("./");
        i = 1;
    } else if matches!(path.first(), Some(ImportPathSeg::DotDot)) {
        out.push_str("../");
        i = 1;
    }
    let mut need_slash = false;
    while i < path.len() {
        match &path[i] {
            ImportPathSeg::Dot => {
                // Mid-path dots are unusual; treat as path segment joiner `.`
                out.push('.');
                need_slash = false;
            }
            ImportPathSeg::DotDot => {
                if need_slash {
                    out.push('/');
                }
                out.push_str("..");
                need_slash = true;
            }
            ImportPathSeg::Name(n) => {
                if need_slash {
                    out.push('/');
                }
                out.push_str(&n.name);
                need_slash = true;
            }
        }
        i += 1;
    }
}

fn write_task_body(body: &TaskBody, level: usize, out: &mut String) {
    match body {
        TaskBody::Block(stmts) => write_block(stmts, level, out),
        TaskBody::Call(e) => {
            out.push(' ');
            write_expr(e, 0, level, out);
        }
        TaskBody::Closure { captures, body } => {
            out.push_str(" ()");
            if !captures.is_empty() {
                out.push_str(" [");
                for (i, c) in captures.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&c.name);
                }
                out.push(']');
            }
            write_block(body, level, out);
        }
    }
}

fn bin_prec(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 9,
        BinaryOp::Add | BinaryOp::Sub => 8,
        BinaryOp::Shl | BinaryOp::Shr => 7,
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::EqEqEq
        | BinaryOp::NotEqEq
        | BinaryOp::Lt
        | BinaryOp::Gt
        | BinaryOp::LtEq
        | BinaryOp::GtEq => 6,
        BinaryOp::BitAnd => 5,
        BinaryOp::BitXor => 4,
        BinaryOp::BitOr => 3,
        BinaryOp::And => 2,
        BinaryOp::Or => 1,
    }
}

fn bin_glyph(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitXor => "^",
        BinaryOp::BitOr => "|",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::EqEqEq => "===",
        BinaryOp::NotEqEq => "!==",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::LtEq => "<=",
        BinaryOp::GtEq => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

/// `stmt_level` is the indent of the containing statement (for nested fn blocks).
fn write_expr(e: &Expr, parent_prec: u8, stmt_level: usize, out: &mut String) {
    match e {
        Expr::Name(Ident { name, .. }) => out.push_str(name),
        Expr::Number { text, width, .. } => {
            if let Some(w) = width {
                out.push('<');
                out.push_str(w.as_str());
                out.push('>');
            }
            out.push_str(text);
        }
        Expr::Duration { text, .. } => out.push_str(text),
        Expr::String { text, .. } | Expr::Bytes { text, .. } | Expr::Locator { text, .. } => {
            out.push_str(text)
        }
        Expr::Bool { value, .. } => {
            if *value {
                out.push('|');
            } else {
                out.push('_');
            }
        }
        Expr::Receiver { .. } => out.push('.'),
        Expr::Unary { op, expr, .. } => {
            match op {
                UnaryOp::Neg => out.push('-'),
                UnaryOp::Not => out.push('!'),
                UnaryOp::BitNot => out.push('~'),
            }
            write_expr(expr, 10, stmt_level, out);
        }
        Expr::WidthCast {
            width, tag, expr, ..
        } => {
            out.push('<');
            out.push_str(width.map(Width::as_str).unwrap_or(tag.as_str()));
            out.push('>');
            out.push(' ');
            write_expr(expr, 10, stmt_level, out);
        }
        Expr::Binary {
            op, left, right, ..
        } => {
            let p = bin_prec(*op);
            let need = p < parent_prec;
            if need {
                out.push('(');
            }
            write_expr(left, p, stmt_level, out);
            out.push(' ');
            out.push_str(bin_glyph(*op));
            out.push(' ');
            write_expr(right, p + 1, stmt_level, out);
            if need {
                out.push(')');
            }
        }
        Expr::Range { start, end, .. } => {
            write_expr(start, 0, stmt_level, out);
            out.push_str("..");
            write_expr(end, 0, stmt_level, out);
        }
        Expr::Call { callee, args, .. } => {
            write_expr(callee, 7, stmt_level, out);
            out.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr(a, 0, stmt_level, out);
            }
            out.push(')');
        }
        Expr::Field { base, field, .. } => {
            // Receiver field `.n` is Field{base: Receiver, field: n} — one leading dot only.
            if matches!(base.as_ref(), Expr::Receiver { .. }) {
                out.push('.');
            } else {
                write_expr(base, 7, stmt_level, out);
                out.push('.');
            }
            out.push_str(&field.name);
        }
        Expr::Index { base, index, .. } => {
            write_expr(base, 7, stmt_level, out);
            out.push('[');
            write_expr(index, 0, stmt_level, out);
            out.push(']');
        }
        Expr::List { items, .. } => {
            out.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr(it, 0, stmt_level, out);
            }
            out.push(']');
        }
        Expr::Object { fields, .. } => {
            out.push('{');
            if !fields.is_empty() {
                out.push(' ');
                for (i, (n, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&n.name);
                    out.push_str(": ");
                    write_expr(v, 0, stmt_level, out);
                }
                out.push(' ');
            }
            out.push('}');
        }
        Expr::StructLit { path, fields, .. } => {
            for (i, p) in path.iter().enumerate() {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(&p.name);
            }
            out.push_str(" {");
            if !fields.is_empty() {
                out.push(' ');
                for (i, (n, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&n.name);
                    out.push_str(": ");
                    write_expr(v, 0, stmt_level, out);
                }
                out.push(' ');
            }
            out.push('}');
        }
        Expr::Fn { params, body, .. } => {
            out.push('(');
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&p.name);
            }
            out.push(')');
            write_block(body, stmt_level, out);
        }
        Expr::Group { expr, .. } => {
            out.push('(');
            write_expr(expr, 0, stmt_level, out);
            out.push(')');
        }
    }
}

// Silence unused import if StringKind only for match exhaustiveness elsewhere
#[allow(dead_code)]
fn _string_kind_tag(k: StringKind) -> &'static str {
    match k {
        StringKind::Pure => "pure",
        StringKind::Rich => "rich",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BindLeader, BindStmt, File, Ident};
    use echo_source::{BytePos, SourceId, Span};

    fn sp() -> Span {
        Span::new(SourceId::from_u32(0), BytePos(0), BytePos(0))
    }

    fn ident(name: &str) -> Ident {
        Ident {
            name: name.into(),
            span: sp(),
        }
    }

    #[test]
    fn formats_simple_bind() {
        let file = File {
            source: SourceId::from_u32(0),
            stmts: vec![Stmt::Bind(BindStmt {
                leader: BindLeader::Dollar,
                name: ident("x"),
                init: Some(Expr::Number {
                    text: "1".into(),
                    width: None,
                    span: sp(),
                }),
                span: sp(),
            })],
            span: sp(),
        };
        assert_eq!(format_file(&file), "$ x = 1\n");
    }
}
