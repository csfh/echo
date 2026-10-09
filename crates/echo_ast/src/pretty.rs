//! Canonical Echo pretty-printer (shared pipeline host: `xo fmt`).
//!
//! Rules: `docs/syntax.md`, `docs/pipeline.md` § formatter — leaders, same-line
//! `{`, indentation, no trailing commas. Does not change program meaning.

use echo_source::Span;

use crate::{
    AssignTarget, BinaryOp, BindLeader, BindStmt, Expr, File, Ident, ImportPathSeg, LoopKind,
    MatchArm, MatchArmKind, MultiBindStmt, Stmt, StringKind, TaskBody, TaskJoinKind, UnaryOp,
    Width,
};

const INDENT: &str = "    ";

/// Why a file could not be formatted without losing a comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError {
    /// Stable diagnostic code (`fmt-comment-in-expression`).
    pub code: &'static str,
    pub message: String,
    /// Source span of the comment that could not be placed.
    pub span: Span,
}

/// Printer state: output text plus the source and comment spans that let the
/// formatter keep `;` comments and blank lines (`docs/pipeline.md` § formatter).
struct Out<'a> {
    text: String,
    src: &'a str,
    comments: &'a [Span],
    next: usize,
    error: Option<FormatError>,
}

impl<'a> Out<'a> {
    fn new(src: &'a str, comments: &'a [Span]) -> Self {
        Self {
            text: String::new(),
            src,
            comments,
            next: 0,
            error: None,
        }
    }

    fn push(&mut self, c: char) {
        self.text.push(c);
    }

    fn push_str(&mut self, s: &str) {
        self.text.push_str(s);
    }

    fn comment_text(&self, c: Span) -> &'a str {
        let bytes = self.src.as_bytes();
        let (a, b) = (c.start.0 as usize, (c.end.0 as usize).min(bytes.len()));
        self.src.get(a..b).unwrap_or("").trim_end()
    }

    /// True when the source has at least one blank line between byte offsets.
    fn blank_between(&self, from: u32, to: u32) -> bool {
        let bytes = self.src.as_bytes();
        let (a, b) = (from as usize, (to as usize).min(bytes.len()));
        a < b && bytes[a..b].iter().filter(|c| **c == b'\n').count() >= 2
    }

    fn comment_before(&self, upto: u32) -> bool {
        self.comments
            .get(self.next)
            .is_some_and(|c| c.end.0 <= upto)
    }

    /// Consume a comment that sits on the same line right after `stmt_end`.
    fn take_trailing(&mut self, stmt_end: u32, limit: u32) -> Option<Span> {
        let c = *self.comments.get(self.next)?;
        if c.start.0 < stmt_end || c.start.0 >= limit {
            return None;
        }
        let gap = self
            .src
            .as_bytes()
            .get(stmt_end as usize..c.start.0 as usize)?;
        if gap.contains(&b'\n') {
            return None;
        }
        self.next += 1;
        Some(c)
    }

    /// Consume a comment on the first line of a braced node, before its body.
    fn take_header_comment(&mut self, owner_start: u32, limit: u32) -> Option<Span> {
        let c = *self.comments.get(self.next)?;
        if c.start.0 < owner_start || c.start.0 >= limit {
            return None;
        }
        let gap = self
            .src
            .as_bytes()
            .get(owner_start as usize..c.start.0 as usize)?;
        if gap.contains(&b'\n') {
            return None;
        }
        self.next += 1;
        Some(c)
    }

    /// A comment that starts inside a statement but was not placed by a nested
    /// statement list sits in the middle of an expression. Never drop it.
    fn reject_inner_comment(&mut self, stmt: Span) {
        if self.error.is_some() {
            return;
        }
        if let Some(c) = self.comments.get(self.next)
            && c.start.0 < stmt.end.0
        {
            self.error = Some(FormatError {
                code: "fmt-comment-in-expression",
                message: "comment sits inside an expression; move it to its own line".into(),
                span: *c,
            });
        }
    }
}

/// Format a complete file to canonical source text (trailing newline).
///
/// This entry point has no source text, so it cannot place `;` comments or
/// blank lines. Use [`format_file_with_comments`] for user source.
#[must_use]
pub fn format_file(file: &File) -> String {
    format_file_with_comments(file, "", &[]).unwrap_or_default()
}

/// Format a file and keep its `;` comments and blank lines.
///
/// `src` is the text the AST was parsed from and `comments` are the lexer's
/// comment spans in source order. Rules: one blank line is kept where the
/// source had one or more, none is added, none starts or ends a block, and a
/// comment on the same line as a statement stays on that line. A comment inside
/// an expression has no safe place, so it is an error and nothing is rewritten.
pub fn format_file_with_comments(
    file: &File,
    src: &str,
    comments: &[Span],
) -> Result<String, FormatError> {
    let mut out = Out::new(src, comments);
    write_list(
        &file.stmts,
        file.span.end.0,
        0,
        &mut out,
        Stmt::span,
        |stmt, out| write_stmt(stmt, 0, out),
    );
    if let Some(err) = out.error {
        return Err(err);
    }
    // Last line of defense: a comment no list consumed must never vanish.
    if let Some(c) = out.comments.get(out.next) {
        return Err(FormatError {
            code: "fmt-comment-unplaced",
            message: "comment could not be placed; file left unchanged".into(),
            span: *c,
        });
    }
    if out.text.is_empty() {
        out.text.push('\n');
    }
    Ok(out.text)
}

fn write_indent(level: usize, out: &mut Out<'_>) {
    for _ in 0..level {
        out.push_str(INDENT);
    }
}

/// Offset of the closing `}` of a braced node (spans end after it).
fn brace_end(span: Span) -> u32 {
    span.end.0.saturating_sub(1)
}

/// Write `items` one per line at `level`, with comments and blank lines from
/// the source. `end` bounds the comments that belong to this list.
fn write_list<T>(
    items: &[T],
    end: u32,
    level: usize,
    out: &mut Out<'_>,
    span_of: impl Fn(&T) -> Span,
    mut write: impl FnMut(&T, &mut Out<'_>),
) {
    let mut prev_end: Option<u32> = None;
    for item in items {
        let span = span_of(item);
        flush_comments(span.start.0, level, &mut prev_end, out);
        if let Some(prev) = prev_end
            && out.blank_between(prev, span.start.0)
        {
            out.push('\n');
        }
        write_indent(level, out);
        write(item, out);
        let mut last = span.end.0;
        if let Some(c) = out.take_trailing(span.end.0, end.max(span.end.0)) {
            out.push(' ');
            let text = out.comment_text(c);
            out.push_str(text);
            last = c.end.0;
        }
        out.push('\n');
        out.reject_inner_comment(span);
        prev_end = Some(last);
    }
    flush_comments(end, level, &mut prev_end, out);
}

/// Emit own-line comments that end at or before `upto`.
fn flush_comments(upto: u32, level: usize, prev_end: &mut Option<u32>, out: &mut Out<'_>) {
    while let Some(&c) = out.comments.get(out.next) {
        if c.end.0 > upto {
            break;
        }
        if let Some(prev) = *prev_end
            && out.blank_between(prev, c.start.0)
        {
            out.push('\n');
        }
        write_indent(level, out);
        let text = out.comment_text(c);
        out.push_str(text);
        out.push('\n');
        *prev_end = Some(c.end.0);
        out.next += 1;
    }
}

/// `owner` is the span of the node that owns the braces (statement, arm, or fn).
fn write_block(body: &[Stmt], owner: Span, level: usize, out: &mut Out<'_>) {
    let end = brace_end(owner);
    out.push_str(" {");
    // A comment on the header line (`? x { ; why`) stays on that line.
    let first_stmt = body.first().map_or(end, |s| s.span().start.0);
    if let Some(c) = out.take_header_comment(owner.start.0, first_stmt.min(end)) {
        out.push(' ');
        let text = out.comment_text(c);
        out.push_str(text);
    }
    if body.is_empty() && !out.comment_before(end) {
        out.push('\n');
        write_indent(level, out);
        out.push('}');
        return;
    }
    out.push('\n');
    write_list(body, end, level + 1, out, Stmt::span, |s, out| {
        write_stmt(s, level + 1, out);
    });
    write_indent(level, out);
    out.push('}');
}

fn write_stmt(stmt: &Stmt, level: usize, out: &mut Out<'_>) {
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
            write_block(&s.members, s.span, level, out);
        }
        Stmt::StructExt(s) => {
            out.push_str("@ ");
            out.push_str(&s.name.name);
            write_block(&s.members, s.span, level, out);
        }
        Stmt::If(s) => {
            out.push_str("? ");
            write_expr(&s.cond, 0, level, out);
            write_block(&s.body, s.span, level, out);
        }
        Stmt::ElseIf(s) => {
            out.push_str(": ");
            write_expr(&s.cond, 0, level, out);
            write_block(&s.body, s.span, level, out);
        }
        Stmt::Else(s) => {
            out.push_str(":");
            write_block(&s.body, s.span, level, out);
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
                write_block(&s.body, s.span, level, out);
            }
            LoopKind::While(e) => {
                out.push_str("* ");
                write_expr(e, 0, level, out);
                write_block(&s.body, s.span, level, out);
            }
            LoopKind::For { item, iter } => {
                out.push_str("* ");
                out.push_str(&item.name);
                out.push_str(" : ");
                write_expr(iter, 0, level, out);
                write_block(&s.body, s.span, level, out);
            }
        },
        Stmt::Break { .. } => out.push('<'),
        Stmt::Continue { .. } => out.push('>'),
        Stmt::Match(m) => {
            out.push_str("| ");
            write_expr(&m.scrutinee, 0, level, out);
            out.push_str(" {");
            out.push('\n');
            write_list(
                &m.arms,
                brace_end(m.span),
                level + 1,
                out,
                |arm| arm.span,
                |arm, out| write_arm(arm, level + 1, out),
            );
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
            write_task_body(&s.body, s.span, level, out);
        }
        Stmt::TaskJoin(s) => match &s.kind {
            TaskJoinKind::Block { bind, body } => {
                out.push('-');
                if let Some(b) = bind {
                    out.push(' ');
                    out.push_str(&b.name);
                    out.push_str(" =");
                }
                write_block(body, s.span, level, out);
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
            write_block(&s.body, s.span, level, out);
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

fn write_arm(arm: &MatchArm, level: usize, out: &mut Out<'_>) {
    match &arm.kind {
        MatchArmKind::Values(ps) => {
            for (i, p) in ps.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr(p, 0, level - 1, out);
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
    write_block(&arm.body, arm.span, level, out);
}

fn write_bind(b: &BindStmt, level: usize, out: &mut Out<'_>) {
    out.push_str(bind_leader_glyph(b.leader));
    out.push(' ');
    out.push_str(&b.name.name);
    if let Some(init) = &b.init {
        out.push_str(" = ");
        write_expr(init, 0, level, out);
    }
}

fn write_multi_bind(m: &MultiBindStmt, level: usize, out: &mut Out<'_>) {
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

fn write_assign_target(t: &AssignTarget, level: usize, out: &mut Out<'_>) {
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

fn write_import_path(path: &[ImportPathSeg], out: &mut Out<'_>) {
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

fn write_task_body(body: &TaskBody, owner: Span, level: usize, out: &mut Out<'_>) {
    match body {
        TaskBody::Block(stmts) => write_block(stmts, owner, level, out),
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
            write_block(body, owner, level, out);
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
fn write_expr(e: &Expr, parent_prec: u8, stmt_level: usize, out: &mut Out<'_>) {
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
        Expr::Fn { params, body, span } => {
            out.push('(');
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&p.name);
            }
            out.push(')');
            write_block(body, *span, stmt_level, out);
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

    /// A comment outside every statement list is an error, never silently lost.
    #[test]
    fn unplaced_comment_is_an_error() {
        let file = File {
            source: SourceId::from_u32(0),
            stmts: vec![],
            span: sp(),
        };
        let beyond = Span::new(SourceId::from_u32(0), BytePos(50), BytePos(55));
        let err = format_file_with_comments(&file, "", &[beyond]).unwrap_err();
        assert_eq!(err.code, "fmt-comment-unplaced");
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
