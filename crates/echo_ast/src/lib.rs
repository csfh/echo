//! Source-shaped syntax tree (ADR 0003).
//!
//! Nodes mirror Echo surface forms. No types, resolution, or runtime meaning.

#![forbid(unsafe_code)]

mod pretty;

pub use pretty::format_file;

use echo_source::{SourceId, Span};
use echo_syntax::LeaderKind;
use serde::{Deserialize, Serialize};

/// Stable crate identity for workspace linkage checks.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub source: SourceId,
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindLeader {
    Tilde,
    Dollar,
    Hash,
}

impl BindLeader {
    #[must_use]
    pub fn from_leader(k: LeaderKind) -> Option<Self> {
        match k {
            LeaderKind::Tilde => Some(Self::Tilde),
            LeaderKind::Dollar => Some(Self::Dollar),
            LeaderKind::Hash => Some(Self::Hash),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tilde => "tilde",
            Self::Dollar => "dollar",
            Self::Hash => "hash",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Bind(BindStmt),
    /// Same-line multi-bind: `~ a = 1, b = 2` or `$ x = 1, y = 2`.
    /// Expanded to sequential [`Bind`] before check/HIR (parser may leave either form).
    MultiBind(MultiBindStmt),
    /// `~ .field = expr` (receiver field write) or other assign forms.
    Assign(AssignStmt),
    Struct(StructStmt),
    StructExt(StructStmt),
    If(IfStmt),
    ElseIf(ElseIfStmt),
    Else(ElseStmt),
    /// `! expr` — error return (Result err), not process abort.
    ErrorReturn(ErrorReturnStmt),
    Return(ReturnStmt),
    Loop(LoopStmt),
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Match(MatchStmt),
    /// `+ { body }` / `+ name = { body }` / `+ call(…)` — spawn task (ADR 0013).
    TaskSpawn(TaskSpawnStmt),
    /// `- { body }` / `- name = { body }` / `- handle` / `- name = handle` — join (ADR 0013).
    TaskJoin(TaskJoinStmt),
    /// `& { body }` / `& name = { body }` — effect block (auto-unwrap result/option).
    EffectBlock(EffectBlockStmt),
    Import(ImportStmt),
    Export(ExportStmt),
    /// Bare expression statement (e.g. call).
    Expr(Expr),
}

/// `& { … }` or `& name = { … }` — auto-unwrap result/option; short-circuit on fail.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct EffectBlockStmt {
    /// When set, bind success payload or err payload / option outcome.
    pub bind: Option<Ident>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// Body of a task spawn or immediate-block join.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum TaskBody {
    /// `{ … }` statement block (zero-arg; no outer locals).
    Block(Vec<Stmt>),
    /// `f(args)` call form.
    Call(Expr),
    /// `() [caps]? { … }` — empty params; optional capture list (spawn args).
    Closure {
        /// Names captured by value at the spawn site (become task params).
        captures: Vec<Ident>,
        body: Vec<Stmt>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TaskSpawnStmt {
    /// When set, bind this name to the **task handle**.
    pub bind: Option<Ident>,
    pub body: TaskBody,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum TaskJoinKind {
    /// `- { … }` or `- name = { … }` — schedule body and join (immediate block).
    Block {
        bind: Option<Ident>,
        body: Vec<Stmt>,
    },
    /// `- handle` or `- name = handle` — join existing handle expression.
    Handle { bind: Option<Ident>, handle: Expr },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TaskJoinStmt {
    pub kind: TaskJoinKind,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum AssignTarget {
    /// `name` (reassign via `~ name =` is still Bind for v0; reserved)
    Name(Ident),
    /// `.field` or `base.field`
    Field { base: Expr, field: Ident },
    /// `base[index]` set, or `base[]` append when `index` is `None`.
    Index { base: Expr, index: Option<Expr> },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AssignStmt {
    pub target: AssignTarget,
    pub value: Expr,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BindStmt {
    pub leader: BindLeader,
    pub name: Ident,
    pub init: Option<Expr>,
    pub span: Span,
}

/// One `name [= expr]` pair in a multi-bind.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MultiBindItem {
    pub name: Ident,
    pub init: Option<Expr>,
}

/// Same-line multi-bind under one leader (no repeated leader).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MultiBindStmt {
    pub leader: BindLeader,
    pub items: Vec<MultiBindItem>,
    pub span: Span,
}

impl MultiBindStmt {
    /// Expand to one [`BindStmt`] per item (same leader).
    #[must_use]
    pub fn into_binds(self) -> Vec<BindStmt> {
        let MultiBindStmt {
            leader,
            items,
            span,
        } = self;
        items
            .into_iter()
            .map(|MultiBindItem { name, init }| BindStmt {
                leader,
                name,
                init,
                span,
            })
            .collect()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StructStmt {
    pub name: Ident,
    pub members: Vec<Stmt>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct IfStmt {
    pub cond: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ElseIfStmt {
    pub cond: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ElseStmt {
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ErrorReturnStmt {
    pub value: Expr,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

/// Match arm shape (value match vs Option/Result arms).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum MatchArmKind {
    /// One or more value exprs; arm matches if scrutinee `==` any
    /// (`a, b, c { body }`). Not Option/Result dialect.
    Values(Vec<Expr>),
    /// `% TypeName { body }` — named struct type tag matches (dual-use `%`).
    Type { name: Ident },
    /// `$ name { body }` — Result ok or Option some
    BindOk { name: Ident },
    /// `! name { body }` — Result err
    BindErr { name: Ident },
    /// `: { body }` — ordinary default or Option none
    Default,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum LoopKind {
    Infinite,
    While(Expr),
    For { item: Ident, iter: Expr },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LoopStmt {
    pub kind: LoopKind,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub kind: MatchArmKind,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MatchStmt {
    pub scrutinee: Expr,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ImportPathSeg {
    Dot,
    DotDot,
    Name(Ident),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ImportStmt {
    pub path: Vec<ImportPathSeg>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ExportStmt {
    pub names: Vec<Ident>,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StringKind {
    Pure,
    Rich,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Neg,
    /// Boolean not (`!`).
    Not,
    /// Bitwise complement (`~`) on integers.
    BitNot,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// Bitwise AND (`&`) — integers only.
    BitAnd,
    /// Bitwise XOR (`^`) — integers only; dual-use with return leader.
    BitXor,
    /// Bitwise OR (`|`) — integers only; dual-use with true atom / match.
    BitOr,
    /// Shift left (`<<`) — integers; count masked to width.
    Shl,
    /// Arithmetic shift right (`>>`) — signed integers; count masked to width.
    Shr,
    Eq,
    NotEq,
    EqEqEq,
    NotEqEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
}

/// Numeric machine width from prefix tag `<i32>…` / cast `<ui64> expr`.
///
/// Signed `i*`, unsigned `ui*`, floats `f*`. Alias: `byte` parses as [`Self::Ui8`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Width {
    I8,
    I16,
    I32,
    I64,
    Ui8,
    Ui16,
    Ui32,
    Ui64,
    F32,
    F64,
}

impl Width {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::Ui8 => "ui8",
            Self::Ui16 => "ui16",
            Self::Ui32 => "ui32",
            Self::Ui64 => "ui64",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    /// Canonical name; `byte` is accepted as an alias of `ui8`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            "ui8" | "byte" => Some(Self::Ui8),
            "ui16" => Some(Self::Ui16),
            "ui32" => Some(Self::Ui32),
            "ui64" => Some(Self::Ui64),
            "f32" => Some(Self::F32),
            "f64" => Some(Self::F64),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_signed_int(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    #[must_use]
    pub fn is_unsigned_int(self) -> bool {
        matches!(self, Self::Ui8 | Self::Ui16 | Self::Ui32 | Self::Ui64)
    }

    #[must_use]
    pub fn is_int(self) -> bool {
        self.is_signed_int() || self.is_unsigned_int()
    }

    #[must_use]
    pub fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// Bit width of integer kinds (8/16/32/64).
    #[must_use]
    pub fn int_bits(self) -> Option<u32> {
        match self {
            Self::I8 | Self::Ui8 => Some(8),
            Self::I16 | Self::Ui16 => Some(16),
            Self::I32 | Self::Ui32 => Some(32),
            Self::I64 | Self::Ui64 => Some(64),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Name(Ident),
    Number {
        text: String,
        /// Set when written as `<width>number`.
        width: Option<Width>,
        span: Span,
    },
    /// Explicit convert `<ui64> expr` (no silent mix).
    ///
    /// `width` is `None` when the tag spelling is not a locked width (`<u8>`).
    WidthCast {
        width: Option<Width>,
        /// Ident as written inside `<>`.
        tag: String,
        expr: Box<Expr>,
        span: Span,
    },
    Duration {
        text: String,
        span: Span,
    },
    String {
        kind: StringKind,
        text: String,
        span: Span,
    },
    Bytes {
        kind: StringKind,
        text: String,
        span: Span,
    },
    Locator {
        kind: StringKind,
        text: String,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    /// Bare receiver `.` in a method body.
    Receiver {
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// Inclusive integer range `lo..hi` (both ends included when lo ≤ hi).
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    Field {
        base: Box<Expr>,
        field: Ident,
        span: Span,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    List {
        items: Vec<Expr>,
        span: Span,
    },
    Object {
        fields: Vec<(Ident, Expr)>,
        span: Span,
    },
    /// Tagged struct lit: `user { … }` or `http.response { … }`.
    StructLit {
        /// One segment (`user`) or module-qualified (`http`, `response`).
        path: Vec<Ident>,
        fields: Vec<(Ident, Expr)>,
        span: Span,
    },
    Fn {
        params: Vec<Ident>,
        body: Vec<Stmt>,
        span: Span,
    },
    Group {
        expr: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Name(i) => i.span,
            Self::Number { span, .. }
            | Self::Duration { span, .. }
            | Self::String { span, .. }
            | Self::Bytes { span, .. }
            | Self::Locator { span, .. }
            | Self::Bool { span, .. }
            | Self::Receiver { span }
            | Self::Unary { span, .. }
            | Self::WidthCast { span, .. }
            | Self::Range { span, .. }
            | Self::Binary { span, .. }
            | Self::Call { span, .. }
            | Self::Field { span, .. }
            | Self::Index { span, .. }
            | Self::List { span, .. }
            | Self::Object { span, .. }
            | Self::StructLit { span, .. }
            | Self::Fn { span, .. }
            | Self::Group { span, .. } => *span,
        }
    }
}

/// Stable kind dump for fixtures (`xo ast --kinds` / `e26`).
#[must_use]
pub fn format_ast_kinds(file: &File) -> String {
    let mut out = String::new();
    out.push_str("file\n");
    for stmt in &file.stmts {
        format_stmt(stmt, 1, &mut out);
    }
    out
}

fn indent(level: usize, out: &mut String) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn format_stmt(stmt: &Stmt, level: usize, out: &mut String) {
    indent(level, out);
    match stmt {
        Stmt::Bind(b) => {
            out.push_str(&format!("bind_{}\n", b.leader.as_str()));
            indent(level + 1, out);
            out.push_str(&format!("name {}\n", b.name.name));
            if let Some(init) = &b.init {
                format_expr(init, level + 1, out);
            }
        }
        Stmt::MultiBind(m) => {
            out.push_str(&format!("multi_bind_{}\n", m.leader.as_str()));
            for item in &m.items {
                indent(level + 1, out);
                out.push_str(&format!("name {}\n", item.name.name));
                if let Some(init) = &item.init {
                    format_expr(init, level + 1, out);
                }
            }
        }
        Stmt::Assign(a) => {
            out.push_str("assign\n");
            match &a.target {
                AssignTarget::Name(n) => {
                    indent(level + 1, out);
                    out.push_str(&format!("target_name {}\n", n.name));
                }
                AssignTarget::Field { base, field } => {
                    indent(level + 1, out);
                    out.push_str(&format!("target_field {}\n", field.name));
                    format_expr(base, level + 2, out);
                }
                AssignTarget::Index { base, index } => {
                    indent(level + 1, out);
                    if index.is_some() {
                        out.push_str("target_index\n");
                    } else {
                        out.push_str("target_push\n");
                    }
                    format_expr(base, level + 2, out);
                    if let Some(index) = index {
                        format_expr(index, level + 2, out);
                    }
                }
            }
            format_expr(&a.value, level + 1, out);
        }
        Stmt::Struct(s) => {
            out.push_str(&format!("struct {}\n", s.name.name));
            for m in &s.members {
                format_stmt(m, level + 1, out);
            }
        }
        Stmt::StructExt(s) => {
            out.push_str(&format!("struct_ext {}\n", s.name.name));
            for m in &s.members {
                format_stmt(m, level + 1, out);
            }
        }
        Stmt::If(s) => {
            out.push_str("if\n");
            format_expr(&s.cond, level + 1, out);
            for st in &s.body {
                format_stmt(st, level + 1, out);
            }
        }
        Stmt::ElseIf(s) => {
            out.push_str("else_if\n");
            format_expr(&s.cond, level + 1, out);
            for st in &s.body {
                format_stmt(st, level + 1, out);
            }
        }
        Stmt::Else(s) => {
            out.push_str("else\n");
            for st in &s.body {
                format_stmt(st, level + 1, out);
            }
        }
        Stmt::ErrorReturn(s) => {
            out.push_str("error_return\n");
            format_expr(&s.value, level + 1, out);
        }
        Stmt::Return(s) => {
            out.push_str("return\n");
            if let Some(v) = &s.value {
                format_expr(v, level + 1, out);
            }
        }
        Stmt::Loop(s) => {
            match &s.kind {
                LoopKind::Infinite => out.push_str("loop\n"),
                LoopKind::While(_) => out.push_str("loop_while\n"),
                LoopKind::For { item, .. } => {
                    out.push_str(&format!("loop_for {}\n", item.name));
                }
            }
            match &s.kind {
                LoopKind::While(e) => format_expr(e, level + 1, out),
                LoopKind::For { iter, .. } => format_expr(iter, level + 1, out),
                LoopKind::Infinite => {}
            }
            for st in &s.body {
                format_stmt(st, level + 1, out);
            }
        }
        Stmt::Break { .. } => out.push_str("break\n"),
        Stmt::Continue { .. } => out.push_str("continue\n"),
        Stmt::TaskSpawn(s) => {
            out.push_str("task_spawn\n");
            if let Some(b) = &s.bind {
                indent(level + 1, out);
                out.push_str(&format!("bind {}\n", b.name));
            }
            match &s.body {
                TaskBody::Block(body) => {
                    indent(level + 1, out);
                    out.push_str("block\n");
                    for st in body {
                        format_stmt(st, level + 2, out);
                    }
                }
                TaskBody::Call(e) => {
                    indent(level + 1, out);
                    out.push_str("call\n");
                    format_expr(e, level + 2, out);
                }
                TaskBody::Closure { captures, body } => {
                    indent(level + 1, out);
                    out.push_str("closure\n");
                    for c in captures {
                        indent(level + 2, out);
                        out.push_str(&format!("capture {}\n", c.name));
                    }
                    for st in body {
                        format_stmt(st, level + 2, out);
                    }
                }
            }
        }
        Stmt::TaskJoin(s) => {
            out.push_str("task_join\n");
            match &s.kind {
                TaskJoinKind::Block { bind, body } => {
                    if let Some(b) = bind {
                        indent(level + 1, out);
                        out.push_str(&format!("bind {}\n", b.name));
                    }
                    indent(level + 1, out);
                    out.push_str("block\n");
                    for st in body {
                        format_stmt(st, level + 2, out);
                    }
                }
                TaskJoinKind::Handle { bind, handle } => {
                    if let Some(b) = bind {
                        indent(level + 1, out);
                        out.push_str(&format!("bind {}\n", b.name));
                    }
                    indent(level + 1, out);
                    out.push_str("handle\n");
                    format_expr(handle, level + 2, out);
                }
            }
        }
        Stmt::EffectBlock(s) => {
            out.push_str("effect_block\n");
            if let Some(b) = &s.bind {
                indent(level + 1, out);
                out.push_str(&format!("bind {}\n", b.name));
            }
            indent(level + 1, out);
            out.push_str("block\n");
            for st in &s.body {
                format_stmt(st, level + 2, out);
            }
        }
        Stmt::Match(s) => {
            out.push_str("match\n");
            format_expr(&s.scrutinee, level + 1, out);
            for arm in &s.arms {
                indent(level + 1, out);
                match &arm.kind {
                    MatchArmKind::Values(ps) => {
                        out.push_str(&format!("arm_values {}\n", ps.len()));
                        for p in ps {
                            format_expr(p, level + 2, out);
                        }
                    }
                    MatchArmKind::Type { name } => {
                        out.push_str(&format!("arm_type {}\n", name.name));
                    }
                    MatchArmKind::BindOk { name } => {
                        out.push_str(&format!("arm_ok {}\n", name.name));
                    }
                    MatchArmKind::BindErr { name } => {
                        out.push_str(&format!("arm_err {}\n", name.name));
                    }
                    MatchArmKind::Default => out.push_str("arm_default\n"),
                }
                for st in &arm.body {
                    format_stmt(st, level + 2, out);
                }
            }
        }
        Stmt::Import(s) => {
            out.push_str("import\n");
            for seg in &s.path {
                indent(level + 1, out);
                match seg {
                    ImportPathSeg::Dot => out.push_str("seg .\n"),
                    ImportPathSeg::DotDot => out.push_str("seg ..\n"),
                    ImportPathSeg::Name(n) => out.push_str(&format!("seg {}\n", n.name)),
                }
            }
        }
        Stmt::Export(s) => {
            out.push_str("export\n");
            for n in &s.names {
                indent(level + 1, out);
                out.push_str(&format!("name {}\n", n.name));
            }
        }
        Stmt::Expr(e) => {
            out.push_str("expr_stmt\n");
            format_expr(e, level + 1, out);
        }
    }
}

fn format_expr(expr: &Expr, level: usize, out: &mut String) {
    indent(level, out);
    match expr {
        Expr::Name(i) => out.push_str(&format!("name {}\n", i.name)),
        Expr::Number { text, width, .. } => match width {
            Some(w) => out.push_str(&format!("number_{} {text}\n", w.as_str())),
            None => out.push_str(&format!("number {text}\n")),
        },
        Expr::WidthCast {
            width, tag, expr, ..
        } => {
            let name = width.map(Width::as_str).unwrap_or(tag.as_str());
            out.push_str(&format!("width_cast {name}\n"));
            format_expr(expr, level + 1, out);
        }
        Expr::Duration { text, .. } => out.push_str(&format!("duration {text}\n")),
        Expr::String { kind, .. } => match kind {
            StringKind::Pure => out.push_str("string_pure\n"),
            StringKind::Rich => out.push_str("string_rich\n"),
        },
        Expr::Bytes { kind, .. } => match kind {
            StringKind::Pure => out.push_str("bytes_pure\n"),
            StringKind::Rich => out.push_str("bytes_rich\n"),
        },
        Expr::Locator { kind, .. } => match kind {
            StringKind::Pure => out.push_str("locator_pure\n"),
            StringKind::Rich => out.push_str("locator_rich\n"),
        },
        Expr::Bool { value, .. } => out.push_str(&format!("bool {value}\n")),
        Expr::Receiver { .. } => out.push_str("receiver\n"),
        Expr::Unary { op, expr, .. } => {
            out.push_str(&format!("unary {op:?}\n"));
            format_expr(expr, level + 1, out);
        }
        Expr::Binary {
            op, left, right, ..
        } => {
            out.push_str(&format!("binary {op:?}\n"));
            format_expr(left, level + 1, out);
            format_expr(right, level + 1, out);
        }
        Expr::Range { start, end, .. } => {
            out.push_str("range\n");
            format_expr(start, level + 1, out);
            format_expr(end, level + 1, out);
        }
        Expr::Call { callee, args, .. } => {
            out.push_str("call\n");
            format_expr(callee, level + 1, out);
            for a in args {
                format_expr(a, level + 1, out);
            }
        }
        Expr::Field { base, field, .. } => {
            out.push_str(&format!("field {}\n", field.name));
            format_expr(base, level + 1, out);
        }
        Expr::Index { base, index, .. } => {
            out.push_str("index\n");
            format_expr(base, level + 1, out);
            format_expr(index, level + 1, out);
        }
        Expr::List { items, .. } => {
            out.push_str("list\n");
            for i in items {
                format_expr(i, level + 1, out);
            }
        }
        Expr::Object { fields, .. } => {
            out.push_str("object\n");
            for (n, e) in fields {
                indent(level + 1, out);
                out.push_str(&format!("field {}\n", n.name));
                format_expr(e, level + 2, out);
            }
        }
        Expr::StructLit { path, fields, .. } => {
            let p = path
                .iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(".");
            out.push_str(&format!("struct_lit {p}\n"));
            for (n, e) in fields {
                indent(level + 1, out);
                out.push_str(&format!("field {}\n", n.name));
                format_expr(e, level + 2, out);
            }
        }
        Expr::Fn { params, body, .. } => {
            out.push_str("fn\n");
            for p in params {
                indent(level + 1, out);
                out.push_str(&format!("param {}\n", p.name));
            }
            for st in body {
                format_stmt(st, level + 1, out);
            }
        }
        Expr::Group { expr, .. } => {
            out.push_str("group\n");
            format_expr(expr, level + 1, out);
        }
    }
}

/// Rewrite all `SourceId`s in a file tree (after loading from the artifact cache).
pub fn remap_source_ids(file: &mut File, new_id: SourceId) {
    file.source = new_id;
    remap_span(&mut file.span, new_id);
    for st in &mut file.stmts {
        remap_stmt(st, new_id);
    }
}

fn remap_span(span: &mut Span, new_id: SourceId) {
    span.source = new_id;
}

fn remap_ident(id: &mut Ident, new_id: SourceId) {
    remap_span(&mut id.span, new_id);
}

fn remap_stmt(st: &mut Stmt, new_id: SourceId) {
    match st {
        Stmt::Bind(b) => {
            remap_ident(&mut b.name, new_id);
            remap_span(&mut b.span, new_id);
            if let Some(e) = &mut b.init {
                remap_expr(e, new_id);
            }
        }
        Stmt::MultiBind(m) => {
            remap_span(&mut m.span, new_id);
            for item in &mut m.items {
                remap_ident(&mut item.name, new_id);
                if let Some(e) = &mut item.init {
                    remap_expr(e, new_id);
                }
            }
        }
        Stmt::Assign(a) => {
            remap_assign_target(&mut a.target, new_id);
            remap_expr(&mut a.value, new_id);
            remap_span(&mut a.span, new_id);
        }
        Stmt::Struct(s) | Stmt::StructExt(s) => {
            remap_ident(&mut s.name, new_id);
            remap_span(&mut s.span, new_id);
            for m in &mut s.members {
                remap_stmt(m, new_id);
            }
        }
        Stmt::If(s) => {
            remap_expr(&mut s.cond, new_id);
            remap_span(&mut s.span, new_id);
            for b in &mut s.body {
                remap_stmt(b, new_id);
            }
        }
        Stmt::ElseIf(s) => {
            remap_expr(&mut s.cond, new_id);
            remap_span(&mut s.span, new_id);
            for b in &mut s.body {
                remap_stmt(b, new_id);
            }
        }
        Stmt::Else(s) => {
            remap_span(&mut s.span, new_id);
            for b in &mut s.body {
                remap_stmt(b, new_id);
            }
        }
        Stmt::ErrorReturn(s) => {
            remap_expr(&mut s.value, new_id);
            remap_span(&mut s.span, new_id);
        }
        Stmt::Return(s) => {
            if let Some(v) = &mut s.value {
                remap_expr(v, new_id);
            }
            remap_span(&mut s.span, new_id);
        }
        Stmt::Loop(s) => {
            match &mut s.kind {
                LoopKind::Infinite => {}
                LoopKind::While(e) => remap_expr(e, new_id),
                LoopKind::For { item, iter } => {
                    remap_ident(item, new_id);
                    remap_expr(iter, new_id);
                }
            }
            remap_span(&mut s.span, new_id);
            for b in &mut s.body {
                remap_stmt(b, new_id);
            }
        }
        Stmt::Break { span } | Stmt::Continue { span } => remap_span(span, new_id),
        Stmt::TaskSpawn(s) => {
            if let Some(b) = &mut s.bind {
                remap_ident(b, new_id);
            }
            match &mut s.body {
                TaskBody::Block(body) => {
                    for st in body {
                        remap_stmt(st, new_id);
                    }
                }
                TaskBody::Call(e) => remap_expr(e, new_id),
                TaskBody::Closure { captures, body } => {
                    for c in captures {
                        remap_ident(c, new_id);
                    }
                    for st in body {
                        remap_stmt(st, new_id);
                    }
                }
            }
            remap_span(&mut s.span, new_id);
        }
        Stmt::TaskJoin(s) => {
            match &mut s.kind {
                TaskJoinKind::Block { bind, body } => {
                    if let Some(b) = bind {
                        remap_ident(b, new_id);
                    }
                    for st in body {
                        remap_stmt(st, new_id);
                    }
                }
                TaskJoinKind::Handle { bind, handle } => {
                    if let Some(b) = bind {
                        remap_ident(b, new_id);
                    }
                    remap_expr(handle, new_id);
                }
            }
            remap_span(&mut s.span, new_id);
        }
        Stmt::EffectBlock(s) => {
            if let Some(b) = &mut s.bind {
                remap_ident(b, new_id);
            }
            for st in &mut s.body {
                remap_stmt(st, new_id);
            }
            remap_span(&mut s.span, new_id);
        }
        Stmt::Match(s) => {
            remap_expr(&mut s.scrutinee, new_id);
            remap_span(&mut s.span, new_id);
            for arm in &mut s.arms {
                match &mut arm.kind {
                    MatchArmKind::BindOk { name }
                    | MatchArmKind::BindErr { name }
                    | MatchArmKind::Type { name } => {
                        remap_ident(name, new_id);
                    }
                    MatchArmKind::Default => {}
                    MatchArmKind::Values(es) => {
                        for e in es {
                            remap_expr(e, new_id);
                        }
                    }
                }
                remap_span(&mut arm.span, new_id);
                for b in &mut arm.body {
                    remap_stmt(b, new_id);
                }
            }
        }
        Stmt::Import(s) => {
            for seg in &mut s.path {
                if let ImportPathSeg::Name(n) = seg {
                    remap_ident(n, new_id);
                }
            }
            remap_span(&mut s.span, new_id);
        }
        Stmt::Export(s) => {
            for n in &mut s.names {
                remap_ident(n, new_id);
            }
            remap_span(&mut s.span, new_id);
        }
        Stmt::Expr(e) => remap_expr(e, new_id),
    }
}

fn remap_assign_target(t: &mut AssignTarget, new_id: SourceId) {
    match t {
        AssignTarget::Name(n) => remap_ident(n, new_id),
        AssignTarget::Field { base, field } => {
            remap_expr(base, new_id);
            remap_ident(field, new_id);
        }
        AssignTarget::Index { base, index } => {
            remap_expr(base, new_id);
            if let Some(index) = index {
                remap_expr(index, new_id);
            }
        }
    }
}

fn remap_expr(e: &mut Expr, new_id: SourceId) {
    match e {
        Expr::Name(n) => remap_ident(n, new_id),
        Expr::Receiver { span } => remap_span(span, new_id),
        Expr::Number { span, .. }
        | Expr::Duration { span, .. }
        | Expr::Bool { span, .. }
        | Expr::String { span, .. }
        | Expr::Bytes { span, .. }
        | Expr::Locator { span, .. } => remap_span(span, new_id),
        Expr::Unary { expr, span, .. } => {
            remap_expr(expr, new_id);
            remap_span(span, new_id);
        }
        Expr::WidthCast { expr, span, .. } => {
            remap_expr(expr, new_id);
            remap_span(span, new_id);
        }
        Expr::Binary {
            left, right, span, ..
        } => {
            remap_expr(left, new_id);
            remap_expr(right, new_id);
            remap_span(span, new_id);
        }
        Expr::Range {
            start, end, span, ..
        } => {
            remap_expr(start, new_id);
            remap_expr(end, new_id);
            remap_span(span, new_id);
        }
        Expr::Call { callee, args, span } => {
            remap_expr(callee, new_id);
            for a in args {
                remap_expr(a, new_id);
            }
            remap_span(span, new_id);
        }
        Expr::Field { base, field, span } => {
            remap_expr(base, new_id);
            remap_ident(field, new_id);
            remap_span(span, new_id);
        }
        Expr::Index { base, index, span } => {
            remap_expr(base, new_id);
            remap_expr(index, new_id);
            remap_span(span, new_id);
        }
        Expr::List { items, span } => {
            for i in items {
                remap_expr(i, new_id);
            }
            remap_span(span, new_id);
        }
        Expr::Object { fields, span } => {
            for (n, v) in fields {
                remap_ident(n, new_id);
                remap_expr(v, new_id);
            }
            remap_span(span, new_id);
        }
        Expr::StructLit { path, fields, span } => {
            for n in path {
                remap_ident(n, new_id);
            }
            for (n, v) in fields {
                remap_ident(n, new_id);
                remap_expr(v, new_id);
            }
            remap_span(span, new_id);
        }
        Expr::Fn {
            params, body, span, ..
        } => {
            for p in params {
                remap_ident(p, new_id);
            }
            for st in body {
                remap_stmt(st, new_id);
            }
            remap_span(span, new_id);
        }
        Expr::Group { expr, span } => {
            remap_expr(expr, new_id);
            remap_span(span, new_id);
        }
    }
}

/// Parse an integer token text into `i64`.
///
/// Accepts decimal, `0x`/`0X` hex, and `0b`/`0B` binary, with optional `_`
/// separators. Empty digit bodies (`0x`, `0b`) and out-of-range values error.
pub fn parse_int_literal(text: &str) -> Result<i64, String> {
    let t = text.replace('_', "");
    if t.is_empty() {
        return Err(format!("invalid int literal `{text}`"));
    }
    // Hex/bin: prefer signed fit; otherwise accept full 64-bit pattern (for `ui64`
    // and two's-complement bit constants above `i64::MAX`).
    if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        if rest.is_empty() {
            return Err(format!("invalid hex literal `{text}`"));
        }
        if let Ok(v) = i64::from_str_radix(rest, 16) {
            return Ok(v);
        }
        return u64::from_str_radix(rest, 16)
            .map(|v| v as i64)
            .map_err(|_| format!("invalid hex literal `{text}`"));
    }
    if let Some(rest) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        if rest.is_empty() {
            return Err(format!("invalid binary literal `{text}`"));
        }
        if let Ok(v) = i64::from_str_radix(rest, 2) {
            return Ok(v);
        }
        return u64::from_str_radix(rest, 2)
            .map(|v| v as i64)
            .map_err(|_| format!("invalid binary literal `{text}`"));
    }
    t.parse::<i64>()
        .map_err(|_| format!("invalid int literal `{text}`"))
}

/// Parse a duration token (`5s`, `10ms`, `100us`, `2m`, `1h`) into nanoseconds.
pub fn parse_duration_nanos(text: &str) -> Result<i64, String> {
    let t = text.replace('_', "");
    let (num_s, mult) = if let Some(rest) = t.strip_suffix("us") {
        (rest, 1_000i64)
    } else if let Some(rest) = t.strip_suffix("ms") {
        (rest, 1_000_000i64)
    } else if let Some(rest) = t.strip_suffix('s') {
        (rest, 1_000_000_000i64)
    } else if let Some(rest) = t.strip_suffix('m') {
        (rest, 60 * 1_000_000_000i64)
    } else if let Some(rest) = t.strip_suffix('h') {
        (rest, 3600 * 1_000_000_000i64)
    } else {
        return Err(format!("invalid duration literal `{text}`"));
    };
    if num_s.is_empty() {
        return Err(format!("invalid duration literal `{text}`"));
    }
    let n: i64 = num_s
        .parse()
        .map_err(|_| format!("invalid duration magnitude in `{text}`"))?;
    n.checked_mul(mult)
        .ok_or_else(|| format!("duration `{text}` overflows i64 nanoseconds"))
}

#[cfg(test)]
mod lit_parse_tests {
    use super::{parse_duration_nanos, parse_int_literal};

    #[test]
    fn decimal_hex_bin() {
        assert_eq!(parse_int_literal("42").unwrap(), 42);
        assert_eq!(parse_int_literal("0xFF").unwrap(), 255);
        assert_eq!(parse_int_literal("0xff").unwrap(), 255);
        assert_eq!(parse_int_literal("0b1010").unwrap(), 10);
        assert_eq!(parse_int_literal("0B10").unwrap(), 2);
        assert_eq!(parse_int_literal("1_000").unwrap(), 1000);
        assert_eq!(parse_int_literal("0xFF_00").unwrap(), 0xFF00);
        assert_eq!(parse_int_literal("0b1010_1010").unwrap(), 0b1010_1010);
    }

    #[test]
    fn rejects_empty_radix_body() {
        assert!(parse_int_literal("0x").is_err());
        assert!(parse_int_literal("0b").is_err());
    }

    #[test]
    fn duration_nanos_units() {
        assert_eq!(parse_duration_nanos("5s").unwrap(), 5_000_000_000);
        assert_eq!(parse_duration_nanos("10ms").unwrap(), 10_000_000);
        assert_eq!(parse_duration_nanos("100us").unwrap(), 100_000);
        assert_eq!(parse_duration_nanos("2m").unwrap(), 120_000_000_000);
        assert_eq!(parse_duration_nanos("1h").unwrap(), 3_600_000_000_000);
        assert_eq!(parse_duration_nanos("1_000ms").unwrap(), 1_000_000_000);
    }
}
