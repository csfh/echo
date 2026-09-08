//! Chumsky grammar over lexer tokens.

use std::ops::Range;

use chumsky::Stream;
use chumsky::prelude::*;
use echo_ast::{
    AssignStmt, AssignTarget, BinaryOp, BindLeader, BindStmt, EffectBlockStmt, ElseIfStmt,
    ElseStmt, ErrorReturnStmt, ExportStmt, Expr, File, Ident, IfStmt, ImportPathSeg, ImportStmt,
    LoopKind, LoopStmt, MatchArm, MatchArmKind, MatchStmt, MultiBindItem, MultiBindStmt,
    ReturnStmt, Stmt, StringKind, StructStmt, TaskBody, TaskJoinKind, TaskJoinStmt, TaskSpawnStmt,
    UnaryOp, Width,
};
use echo_diagnostics::Diagnostics;
use echo_lexer::{Token, TokenKind};
use echo_source::{BytePos, SourceFile, Span};
use echo_syntax::LeaderKind;

use crate::parse_error;

type Tok = TokenKind;
type Sp = Range<usize>;
type Err = Simple<Tok, Sp>;

pub(crate) fn parse_tokens(source: &SourceFile, tokens: &[Token]) -> (Option<File>, Diagnostics) {
    let src_text = source.text();
    let source_id = source.id();

    let stream_toks: Vec<(Tok, Sp)> = tokens
        .iter()
        .filter(|t| t.kind != TokenKind::Eof)
        .map(|t| (t.kind, t.span.start.as_usize()..t.span.end.as_usize()))
        .collect();

    let eoi = src_text.len()..src_text.len();
    let stream = Stream::from_iter(eoi.clone(), stream_toks.into_iter());

    let parser = file_parser(src_text, source_id);
    let (file, errors) = parser.parse_recovery(stream);

    let mut diagnostics = Diagnostics::new();
    for err in errors {
        let span = err.span();
        let msg = format_chumsky_error(&err);
        let echo_span = Span::new(
            source_id,
            BytePos(span.start as u32),
            BytePos(span.end as u32),
        );
        diagnostics.push(parse_error(msg, echo_span));
    }

    (
        file.map(|stmts| {
            let mut file = File {
                source: source_id,
                stmts,
                span: Span::new(source_id, BytePos(0), BytePos(src_text.len() as u32)),
            };
            expand_multi_binds(&mut file);
            file
        }),
        diagnostics,
    )
}

/// Human-readable parse error (not chumsky `Debug` dumps).
fn format_chumsky_error(err: &Simple<Tok, Sp>) -> String {
    // Common footgun: bare `name = expr` looks like assignment but Echo needs a leader.
    if matches!(err.found(), Some(TokenKind::Eq)) {
        return "unexpected `=`; use `~ name = …` to reassign a mutable bind \
                (or `$ name = …` / `# name = …` to introduce a new bind)"
            .into();
    }

    let found = match err.found() {
        Some(t) => format!("`{}`", token_surface(*t)),
        None => "end of input".into(),
    };

    let mut expected: Vec<&'static str> = err
        .expected()
        .filter_map(|opt| opt.map(token_surface))
        .collect();
    expected.sort_unstable();
    expected.dedup();

    // Cap long expectation lists so messages stay readable.
    let expected_txt = match expected.as_slice() {
        [] => "a valid token".into(),
        [one] => format!("`{one}`"),
        many if many.len() <= 6 => {
            let parts: Vec<_> = many.iter().map(|t| format!("`{t}`")).collect();
            parts.join(", ")
        }
        many => {
            let head: Vec<_> = many.iter().take(5).map(|t| format!("`{t}`")).collect();
            format!("{}, …", head.join(", "))
        }
    };

    format!("unexpected {found}; expected {expected_txt}")
}

/// Short surface form for error messages (glyphs / keywords, not Debug).
fn token_surface(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::Leader(k) => match k {
            LeaderKind::Tilde => "~",
            LeaderKind::Dollar => "$",
            LeaderKind::Hash => "#",
            LeaderKind::Percent => "%",
            LeaderKind::At => "@",
            LeaderKind::Question => "?",
            LeaderKind::Colon => ":",
            LeaderKind::Bang => "!",
            LeaderKind::Caret => "^",
            LeaderKind::Star => "*",
            LeaderKind::Lt => "<",
            LeaderKind::Gt => ">",
            LeaderKind::Pipe => "|",
            LeaderKind::Plus => "+",
            LeaderKind::Minus => "-",
            LeaderKind::Ampersand => "&",
            LeaderKind::Slash => "/",
            LeaderKind::Backslash => "\\",
        },
        TokenKind::Ident => "identifier",
        TokenKind::Number => "number",
        TokenKind::StringPure | TokenKind::StringRich => "string",
        TokenKind::BytesPure | TokenKind::BytesRich => "bytes",
        TokenKind::LocatorPure | TokenKind::LocatorRich => "locator",
        TokenKind::Duration => "duration",
        TokenKind::Plus => "+",
        TokenKind::Minus => "-",
        TokenKind::Star => "*",
        TokenKind::Slash => "/",
        TokenKind::Percent => "%",
        TokenKind::EqEq => "==",
        TokenKind::NotEq => "!=",
        TokenKind::EqEqEq => "===",
        TokenKind::NotEqEq => "!==",
        TokenKind::Lt => "<",
        TokenKind::Gt => ">",
        TokenKind::LtEq => "<=",
        TokenKind::GtEq => ">=",
        TokenKind::AndAnd => "&&",
        TokenKind::OrOr => "||",
        TokenKind::Ampersand => "&",
        TokenKind::Caret => "^",
        TokenKind::Tilde => "~",
        TokenKind::LtLt => "<<",
        TokenKind::GtGt => ">>",
        TokenKind::Bang => "!",
        TokenKind::Dot => ".",
        TokenKind::DotDot => "..",
        TokenKind::Comma => ",",
        TokenKind::Colon => ":",
        TokenKind::Eq => "=",
        TokenKind::LParen => "(",
        TokenKind::RParen => ")",
        TokenKind::LBracket => "[",
        TokenKind::RBracket => "]",
        TokenKind::LBrace => "{",
        TokenKind::RBrace => "}",
        TokenKind::Pipe => "|",
        TokenKind::Underscore => "_",
        TokenKind::Eof => "end of file",
    }
}

/// Expand same-line multi-binds into sequential single binds for the pipeline.
fn expand_multi_binds(file: &mut File) {
    fn expand_list(stmts: &mut Vec<Stmt>) {
        let mut out = Vec::with_capacity(stmts.len());
        for s in std::mem::take(stmts) {
            match s {
                Stmt::MultiBind(m) => {
                    for b in m.into_binds() {
                        out.push(Stmt::Bind(b));
                    }
                }
                Stmt::Struct(mut st) => {
                    expand_list(&mut st.members);
                    out.push(Stmt::Struct(st));
                }
                Stmt::StructExt(mut st) => {
                    expand_list(&mut st.members);
                    out.push(Stmt::StructExt(st));
                }
                Stmt::If(mut s) => {
                    expand_list(&mut s.body);
                    out.push(Stmt::If(s));
                }
                Stmt::ElseIf(mut s) => {
                    expand_list(&mut s.body);
                    out.push(Stmt::ElseIf(s));
                }
                Stmt::Else(mut s) => {
                    expand_list(&mut s.body);
                    out.push(Stmt::Else(s));
                }
                Stmt::Loop(mut s) => {
                    expand_list(&mut s.body);
                    out.push(Stmt::Loop(s));
                }
                Stmt::Match(mut s) => {
                    for arm in &mut s.arms {
                        expand_list(&mut arm.body);
                    }
                    out.push(Stmt::Match(s));
                }
                other => out.push(other),
            }
        }
        *stmts = out;
    }
    expand_list(&mut file.stmts);
}

fn sp(source_id: echo_source::SourceId, r: Sp) -> Span {
    Span::new(source_id, BytePos(r.start as u32), BytePos(r.end as u32))
}

fn merge_span(source_id: echo_source::SourceId, a: Span, b: Span) -> Span {
    Span::new(
        source_id,
        BytePos(a.start.0.min(b.start.0)),
        BytePos(a.end.0.max(b.end.0)),
    )
}

fn text_at(src: &str, r: &Sp) -> String {
    src.get(r.start..r.end).unwrap_or("").to_string()
}

fn leader(kind: LeaderKind) -> impl Parser<Tok, Tok, Error = Err> + Clone {
    just(TokenKind::Leader(kind))
}

fn file_parser(
    src: &str,
    source_id: echo_source::SourceId,
) -> impl Parser<Tok, Vec<Stmt>, Error = Err> + '_ {
    stmt_parser(src, source_id).repeated().then_ignore(end())
}

fn stmt_parser(
    src: &str,
    source_id: echo_source::SourceId,
) -> impl Parser<Tok, Stmt, Error = Err> + Clone + '_ {
    recursive(|stmt| {
        let expr = expr_parser(src, source_id, stmt.clone());

        let block = just(TokenKind::LBrace)
            .ignore_then(stmt.clone().repeated())
            .then_ignore(just(TokenKind::RBrace));

        let ident = ident_parser(src, source_id);

        // --- bind: ~ $ #  (single or same-line multi: `~ a = 1, b = 2`) ---
        // --- assign: ~ .field = | ~ name.field = | ~ name[i] = ---
        let bind_leader = choice((
            leader(LeaderKind::Tilde).to(BindLeader::Tilde),
            leader(LeaderKind::Dollar).to(BindLeader::Dollar),
            leader(LeaderKind::Hash).to(BindLeader::Hash),
        ));
        let bind_item = ident
            .clone()
            .then(just(TokenKind::Eq).ignore_then(expr.clone()).or_not())
            .map(|(name, init)| MultiBindItem { name, init });
        // `name [= expr] (, name [= expr])*` — no trailing comma.
        let bind_items = bind_item
            .clone()
            .then(just(TokenKind::Comma).ignore_then(bind_item).repeated())
            .map(|(first, rest)| {
                let mut items = vec![first];
                items.extend(rest);
                items
            });
        let bind = bind_leader
            .then(bind_items)
            .map_with_span(move |(leader, items), span| {
                let s = sp(source_id, span);
                if items.len() == 1 {
                    let MultiBindItem { name, init } = items.into_iter().next().unwrap();
                    Stmt::Bind(BindStmt {
                        leader,
                        name,
                        init,
                        span: s,
                    })
                } else {
                    Stmt::MultiBind(MultiBindStmt {
                        leader,
                        items,
                        span: s,
                    })
                }
            });

        // ~ .field = expr  and  ~ .a.b.c = expr  (receiver field write, chain ok)
        let assign_recv_field = leader(LeaderKind::Tilde)
            .ignore_then(
                just(TokenKind::Dot)
                    .ignore_then(ident.clone())
                    .repeated()
                    .at_least(1),
            )
            .then_ignore(just(TokenKind::Eq))
            .then(expr.clone())
            .map_with_span(move |(fields, value), span| {
                let s = sp(source_id, span);
                let mut fields = fields;
                let last = fields.pop().expect("at_least(1)");
                let mut base = Expr::Receiver {
                    span: Span::new(source_id, s.start, last.span.start),
                };
                for f in fields {
                    let fspan = f.span;
                    base = Expr::Field {
                        base: Box::new(base),
                        field: f,
                        span: fspan,
                    };
                }
                Stmt::Assign(AssignStmt {
                    target: AssignTarget::Field { base, field: last },
                    value,
                    span: s,
                })
            });

        // ~ name.field = expr  and  ~ name.a.b = expr  (value field write, chain ok)
        let assign_value_field = leader(LeaderKind::Tilde)
            .ignore_then(ident.clone())
            .then(
                just(TokenKind::Dot)
                    .ignore_then(ident.clone())
                    .repeated()
                    .at_least(1),
            )
            .then_ignore(just(TokenKind::Eq))
            .then(expr.clone())
            .map_with_span(move |((base_name, fields), value), span| {
                let s = sp(source_id, span);
                let mut fields = fields;
                let last = fields.pop().expect("at_least(1)");
                let mut base = Expr::Name(base_name);
                for f in fields {
                    let fspan = f.span;
                    base = Expr::Field {
                        base: Box::new(base),
                        field: f,
                        span: fspan,
                    };
                }
                Stmt::Assign(AssignStmt {
                    target: AssignTarget::Field { base, field: last },
                    value,
                    span: s,
                })
            });

        // ~ name[index] = expr, ~ name.field[index] = expr
        // ~ name[] = expr, ~ name.field[] = expr  (list append / push)
        let assign_index = leader(LeaderKind::Tilde)
            .ignore_then(ident.clone())
            .then(just(TokenKind::Dot).ignore_then(ident.clone()).repeated())
            .then(
                just(TokenKind::LBracket)
                    .ignore_then(expr.clone().or_not())
                    .then_ignore(just(TokenKind::RBracket)),
            )
            .then_ignore(just(TokenKind::Eq))
            .then(expr.clone())
            .map_with_span(move |(((base_name, fields), index), value), span| {
                let s = sp(source_id, span);
                let mut base = Expr::Name(base_name);
                for f in fields {
                    let fspan = f.span;
                    base = Expr::Field {
                        base: Box::new(base),
                        field: f,
                        span: fspan,
                    };
                }
                Stmt::Assign(AssignStmt {
                    target: AssignTarget::Index { base, index },
                    value,
                    span: s,
                })
            });

        // --- shape: % @ ---
        let struct_body = block.clone();
        let percent = leader(LeaderKind::Percent)
            .ignore_then(ident.clone())
            .then(struct_body.clone())
            .map_with_span(move |(name, members), span| {
                Stmt::Struct(StructStmt {
                    name,
                    members,
                    span: sp(source_id, span),
                })
            });
        let at = leader(LeaderKind::At)
            .ignore_then(ident.clone())
            .then(struct_body)
            .map_with_span(move |(name, members), span| {
                Stmt::StructExt(StructStmt {
                    name,
                    members,
                    span: sp(source_id, span),
                })
            });

        // --- control ---
        let if_stmt = leader(LeaderKind::Question)
            .ignore_then(expr.clone())
            .then(block.clone())
            .map_with_span(move |(cond, body), span| {
                Stmt::If(IfStmt {
                    cond,
                    body,
                    span: sp(source_id, span),
                })
            });

        let else_if = leader(LeaderKind::Colon)
            .ignore_then(expr.clone())
            .then(block.clone())
            .map_with_span(move |(cond, body), span| {
                Stmt::ElseIf(ElseIfStmt {
                    cond,
                    body,
                    span: sp(source_id, span),
                })
            });

        let else_only = leader(LeaderKind::Colon)
            .ignore_then(block.clone())
            .map_with_span(move |body, span| {
                Stmt::Else(ElseStmt {
                    body,
                    span: sp(source_id, span),
                })
            });

        // Prefer `: expr {` over `: {` by trying else_if first.
        let colon_stmt = else_if.or(else_only);

        let error_return = leader(LeaderKind::Bang)
            .ignore_then(expr.clone())
            .map_with_span(move |value, span| {
                Stmt::ErrorReturn(ErrorReturnStmt {
                    value,
                    span: sp(source_id, span),
                })
            });

        let return_stmt = leader(LeaderKind::Caret)
            .ignore_then(expr.clone().or_not())
            .map_with_span(move |value, span| {
                Stmt::Return(ReturnStmt {
                    value,
                    span: sp(source_id, span),
                })
            });

        // * { } | * expr { } | * item : iter { }
        let for_loop = leader(LeaderKind::Star)
            .ignore_then(ident.clone())
            .then_ignore(just(TokenKind::Colon))
            .then(expr.clone())
            .then(block.clone())
            .map_with_span(move |((item, iter), body), span| {
                Stmt::Loop(LoopStmt {
                    kind: LoopKind::For { item, iter },
                    body,
                    span: sp(source_id, span),
                })
            });

        let while_or_inf = leader(LeaderKind::Star)
            .ignore_then(expr.clone().or_not())
            .then(block.clone())
            .map_with_span(move |(cond, body), span| {
                let kind = match cond {
                    None => LoopKind::Infinite,
                    Some(e) => LoopKind::While(e),
                };
                Stmt::Loop(LoopStmt {
                    kind,
                    body,
                    span: sp(source_id, span),
                })
            });

        let loop_stmt = for_loop.or(while_or_inf);

        let break_stmt = leader(LeaderKind::Lt).map_with_span(move |_, span| Stmt::Break {
            span: sp(source_id, span),
        });
        let continue_stmt = leader(LeaderKind::Gt).map_with_span(move |_, span| Stmt::Continue {
            span: sp(source_id, span),
        });

        // | expr { arms }
        // $ name { } | ! name { } | : { } | % Type { } | value, value, … { }
        let match_arm_ok = leader(LeaderKind::Dollar)
            .ignore_then(ident.clone())
            .then(block.clone())
            .map_with_span(move |(name, body), span| MatchArm {
                kind: MatchArmKind::BindOk { name },
                body,
                span: sp(source_id, span),
            });
        let match_arm_err = leader(LeaderKind::Bang)
            .ignore_then(ident.clone())
            .then(block.clone())
            .map_with_span(move |(name, body), span| MatchArm {
                kind: MatchArmKind::BindErr { name },
                body,
                span: sp(source_id, span),
            });
        // Dual-use `%`: shape decl at statement level; type arm inside match.
        let match_arm_type = leader(LeaderKind::Percent)
            .ignore_then(ident.clone())
            .then(block.clone())
            .map_with_span(move |(name, body), span| MatchArm {
                kind: MatchArmKind::Type { name },
                body,
                span: sp(source_id, span),
            });
        let match_arm_default = leader(LeaderKind::Colon)
            .ignore_then(block.clone())
            .map_with_span(move |body, span| MatchArm {
                kind: MatchArmKind::Default,
                body,
                span: sp(source_id, span),
            });
        // True/false at arm start: leaders/underscore, not expr context.
        // `| { … }` → true; `_ { … }` → false (docs/syntax.md bool lits).
        let match_arm_bool_true = leader(LeaderKind::Pipe)
            .ignore_then(block.clone())
            .map_with_span(move |body, span| {
                let s = sp(source_id, span);
                MatchArm {
                    kind: MatchArmKind::Values(vec![Expr::Bool {
                        value: true,
                        span: s,
                    }]),
                    body,
                    span: s,
                }
            });
        let match_arm_bool_false = just(TokenKind::Underscore)
            .ignore_then(block.clone())
            .map_with_span(move |body, span| {
                let s = sp(source_id, span);
                MatchArm {
                    kind: MatchArmKind::Values(vec![Expr::Bool {
                        value: false,
                        span: s,
                    }]),
                    body,
                    span: s,
                }
            });
        // Value arms: one or more exprs (no trailing comma), then block.
        // Matches if scrutinee == any value (deep ==).
        // No trailing commas (syntax.md). `separated_by` without trailing allow.
        let match_arm_values = expr
            .clone()
            .separated_by(just(TokenKind::Comma))
            .at_least(1)
            .collect::<Vec<_>>()
            .then(block.clone())
            .map_with_span(move |(patterns, body), span| MatchArm {
                kind: MatchArmKind::Values(patterns),
                body,
                span: sp(source_id, span),
            });
        let match_arm = choice((
            match_arm_ok,
            match_arm_err,
            match_arm_type,
            match_arm_default,
            match_arm_bool_true,
            match_arm_bool_false,
            match_arm_values,
        ));

        let match_stmt = leader(LeaderKind::Pipe)
            .ignore_then(expr.clone())
            .then(
                just(TokenKind::LBrace)
                    .ignore_then(match_arm.repeated())
                    .then_ignore(just(TokenKind::RBrace)),
            )
            .map_with_span(move |(scrutinee, arms), span| {
                Stmt::Match(MatchStmt {
                    scrutinee,
                    arms,
                    span: sp(source_id, span),
                })
            });

        // --- module ---
        // path: seg ( / seg )*   e.g. std/io  or  ./config  or  github.com/acme/lib
        // Dotted names (`github.com`) are one Name segment (host paths / ADR 0014).
        let dotted_ident = ident
            .clone()
            .then(
                just(TokenKind::Dot)
                    .ignore_then(ident.clone())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map_with_span(move |(first, rest), span| {
                let mut name = first.name;
                for part in rest {
                    name.push('.');
                    name.push_str(&part.name);
                }
                ImportPathSeg::Name(Ident {
                    name,
                    span: sp(source_id, span),
                })
            });
        let import_seg = choice((
            just(TokenKind::DotDot).to(ImportPathSeg::DotDot),
            just(TokenKind::Dot).to(ImportPathSeg::Dot),
            dotted_ident,
        ));
        let import_path = import_seg
            .clone()
            .then(just(TokenKind::Slash).ignore_then(import_seg).repeated())
            .map(|(first, rest)| {
                let mut path = vec![first];
                path.extend(rest);
                path
            });

        // + () [caps]? { body }  — empty params; optional capture list
        // No trailing commas (Echo syntax).
        let capture_list = just(TokenKind::LBracket)
            .ignore_then(
                ident
                    .clone()
                    .separated_by(just(TokenKind::Comma))
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(TokenKind::RBracket));
        let task_closure_body = just(TokenKind::LParen)
            .ignore_then(just(TokenKind::RParen))
            .ignore_then(capture_list.or_not())
            .then(block.clone())
            .map(|(caps, body)| TaskBody::Closure {
                captures: caps.unwrap_or_default(),
                body,
            });

        // + { body } | + name = … | + () […] { } | + f(args)
        let task_spawn_bind = leader(LeaderKind::Plus)
            .ignore_then(ident.clone())
            .then_ignore(just(TokenKind::Eq))
            .then(choice((
                task_closure_body.clone(),
                block.clone().map(TaskBody::Block),
                expr.clone().map(TaskBody::Call),
            )))
            .map_with_span(move |(name, body), span| {
                Stmt::TaskSpawn(TaskSpawnStmt {
                    bind: Some(name),
                    body,
                    span: sp(source_id, span),
                })
            });
        let task_spawn_closure = leader(LeaderKind::Plus)
            .ignore_then(task_closure_body.clone())
            .map_with_span(move |body, span| {
                Stmt::TaskSpawn(TaskSpawnStmt {
                    bind: None,
                    body,
                    span: sp(source_id, span),
                })
            });
        let task_spawn_block = leader(LeaderKind::Plus)
            .ignore_then(block.clone())
            .map_with_span(move |body, span| {
                Stmt::TaskSpawn(TaskSpawnStmt {
                    bind: None,
                    body: TaskBody::Block(body),
                    span: sp(source_id, span),
                })
            });
        // Only true calls (`f(…)`), not a bare name — so `+ job = …` is not stolen.
        let task_spawn_call =
            leader(LeaderKind::Plus)
                .ignore_then(expr.clone())
                .try_map(move |e, span| match &e {
                    Expr::Call { .. } => Ok(Stmt::TaskSpawn(TaskSpawnStmt {
                        bind: None,
                        body: TaskBody::Call(e),
                        span: sp(source_id, span),
                    })),
                    _ => Err(Simple::custom(span, "task spawn call must be f(args)")),
                });
        // closure before block so `+ ()` is not misread
        let task_spawn = task_spawn_bind
            .or(task_spawn_closure)
            .or(task_spawn_block)
            .or(task_spawn_call);

        // & name = { body } | & { body } — effect block (auto-unwrap)
        let effect_bind = leader(LeaderKind::Ampersand)
            .ignore_then(ident.clone())
            .then_ignore(just(TokenKind::Eq))
            .then(block.clone())
            .map_with_span(move |(name, body), span| {
                Stmt::EffectBlock(EffectBlockStmt {
                    bind: Some(name),
                    body,
                    span: sp(source_id, span),
                })
            });
        let effect_block = leader(LeaderKind::Ampersand)
            .ignore_then(block.clone())
            .map_with_span(move |body, span| {
                Stmt::EffectBlock(EffectBlockStmt {
                    bind: None,
                    body,
                    span: sp(source_id, span),
                })
            });
        let effect = effect_bind.or(effect_block);

        // - name = { body } | - { body } | - name = handle | - handle
        let task_join_bind_block = leader(LeaderKind::Minus)
            .ignore_then(ident.clone())
            .then_ignore(just(TokenKind::Eq))
            .then(block.clone())
            .map_with_span(move |(name, body), span| {
                Stmt::TaskJoin(TaskJoinStmt {
                    kind: TaskJoinKind::Block {
                        bind: Some(name),
                        body,
                    },
                    span: sp(source_id, span),
                })
            });
        let task_join_block = leader(LeaderKind::Minus)
            .ignore_then(block.clone())
            .map_with_span(move |body, span| {
                Stmt::TaskJoin(TaskJoinStmt {
                    kind: TaskJoinKind::Block { bind: None, body },
                    span: sp(source_id, span),
                })
            });
        let task_join_bind_handle = leader(LeaderKind::Minus)
            .ignore_then(ident.clone())
            .then_ignore(just(TokenKind::Eq))
            .then(expr.clone())
            .map_with_span(move |(name, handle), span| {
                Stmt::TaskJoin(TaskJoinStmt {
                    kind: TaskJoinKind::Handle {
                        bind: Some(name),
                        handle,
                    },
                    span: sp(source_id, span),
                })
            });
        let task_join_handle = leader(LeaderKind::Minus)
            .ignore_then(expr.clone())
            .map_with_span(move |handle, span| {
                Stmt::TaskJoin(TaskJoinStmt {
                    kind: TaskJoinKind::Handle { bind: None, handle },
                    span: sp(source_id, span),
                })
            });
        // bind+block before bind+handle so `- n = {` is not read as handle `{`
        let task_join = task_join_bind_block
            .or(task_join_block)
            .or(task_join_bind_handle)
            .or(task_join_handle);

        let import_stmt = leader(LeaderKind::Slash)
            .ignore_then(import_path)
            .map_with_span(move |path, span| {
                Stmt::Import(ImportStmt {
                    path,
                    span: sp(source_id, span),
                })
            });

        let export_stmt = leader(LeaderKind::Backslash)
            .ignore_then(
                ident
                    .clone()
                    .separated_by(just(TokenKind::Comma))
                    .at_least(1),
            )
            .map_with_span(move |names, span| {
                Stmt::Export(ExportStmt {
                    names,
                    span: sp(source_id, span),
                })
            });

        let expr_stmt = expr.map(Stmt::Expr);

        // Assign forms before plain bind so `~ .n` / `~ p.f` / `~ xs[` are not misread as binds.
        // Index before value-field: `~ a.b[i] =` must not be stolen as a field assign.
        choice((
            assign_recv_field,
            assign_index,
            assign_value_field,
            bind,
            percent,
            at,
            if_stmt,
            colon_stmt,
            error_return,
            return_stmt,
            loop_stmt,
            break_stmt,
            continue_stmt,
            match_stmt,
            task_spawn,
            task_join,
            effect,
            import_stmt,
            export_stmt,
            expr_stmt,
        ))
    })
}

fn ident_parser(
    src: &str,
    source_id: echo_source::SourceId,
) -> impl Parser<Tok, Ident, Error = Err> + Clone + '_ {
    filter(|t| matches!(t, TokenKind::Ident))
        .map_with_span(move |_, span| Ident {
            name: text_at(src, &span),
            span: sp(source_id, span),
        })
        .labelled("ident")
}

fn expr_parser<'a>(
    src: &'a str,
    source_id: echo_source::SourceId,
    stmt: impl Parser<Tok, Stmt, Error = Err> + Clone + 'a,
) -> impl Parser<Tok, Expr, Error = Err> + Clone + 'a {
    recursive(move |expr| {
        let ident = ident_parser(src, source_id);

        let number_plain =
            filter(|t| matches!(t, TokenKind::Number)).map_with_span(move |_, span| Expr::Number {
                text: text_at(src, &span),
                width: None,
                span: sp(source_id, span),
            });

        // `<i32>123` / `<i32> -32` — prefix width tag on literals (locked).
        // Sign is part of the literal after `>` (space optional).
        let number_width = just(TokenKind::Lt)
            .ignore_then(ident.clone())
            .then_ignore(just(TokenKind::Gt))
            .then(just(TokenKind::Minus).or_not())
            .then(
                filter(|t| matches!(t, TokenKind::Number))
                    .map_with_span(move |_, span| text_at(src, &span)),
            )
            .map_with_span(move |((wident, neg), text), span| {
                let width = Width::parse(&wident.name);
                let text = if neg.is_some() {
                    format!("-{text}")
                } else {
                    text
                };
                let s = sp(source_id, span);
                match width {
                    Some(w) => Expr::Number {
                        text,
                        width: Some(w),
                        span: s,
                    },
                    None => Expr::WidthCast {
                        width: None,
                        tag: wident.name,
                        expr: Box::new(Expr::Number {
                            text,
                            width: None,
                            span: s,
                        }),
                        span: s,
                    },
                }
            });

        let number = number_width.or(number_plain);

        let duration =
            filter(|t| matches!(t, TokenKind::Duration)).map_with_span(move |_, span| {
                Expr::Duration {
                    text: text_at(src, &span),
                    span: sp(source_id, span),
                }
            });

        let string = choice((
            filter(|t| matches!(t, TokenKind::StringPure)).map_with_span(move |_, span| {
                Expr::String {
                    kind: StringKind::Pure,
                    text: text_at(src, &span),
                    span: sp(source_id, span),
                }
            }),
            filter(|t| matches!(t, TokenKind::StringRich)).map_with_span(move |_, span| {
                Expr::String {
                    kind: StringKind::Rich,
                    text: text_at(src, &span),
                    span: sp(source_id, span),
                }
            }),
        ));

        let bytes = choice((
            filter(|t| matches!(t, TokenKind::BytesPure)).map_with_span(move |_, span| {
                Expr::Bytes {
                    kind: StringKind::Pure,
                    text: text_at(src, &span),
                    span: sp(source_id, span),
                }
            }),
            filter(|t| matches!(t, TokenKind::BytesRich)).map_with_span(move |_, span| {
                Expr::Bytes {
                    kind: StringKind::Rich,
                    text: text_at(src, &span),
                    span: sp(source_id, span),
                }
            }),
        ));

        let locator = choice((
            filter(|t| matches!(t, TokenKind::LocatorPure)).map_with_span(move |_, span| {
                Expr::Locator {
                    kind: StringKind::Pure,
                    text: text_at(src, &span),
                    span: sp(source_id, span),
                }
            }),
            filter(|t| matches!(t, TokenKind::LocatorRich)).map_with_span(move |_, span| {
                Expr::Locator {
                    kind: StringKind::Rich,
                    text: text_at(src, &span),
                    span: sp(source_id, span),
                }
            }),
        ));

        let boolean = choice((
            just(TokenKind::Pipe).map_with_span(move |_, span| Expr::Bool {
                value: true,
                span: sp(source_id, span),
            }),
            just(TokenKind::Underscore).map_with_span(move |_, span| Expr::Bool {
                value: false,
                span: sp(source_id, span),
            }),
        ));

        let block_body = just(TokenKind::LBrace)
            .ignore_then(stmt.clone().repeated())
            .then_ignore(just(TokenKind::RBrace));

        // (params) { body }
        // No trailing commas (syntax.md).
        let params = just(TokenKind::LParen)
            .ignore_then(ident.clone().separated_by(just(TokenKind::Comma)))
            .then_ignore(just(TokenKind::RParen));

        let fn_expr = params
            .then(block_body)
            .map_with_span(move |(params, body), span| Expr::Fn {
                params,
                body,
                span: sp(source_id, span),
            });

        let field_list = ident
            .clone()
            .then_ignore(just(TokenKind::Colon))
            .then(expr.clone())
            .separated_by(just(TokenKind::Comma))
            .delimited_by(just(TokenKind::LBrace), just(TokenKind::RBrace));

        let object = field_list
            .clone()
            .map_with_span(move |fields, span| Expr::Object {
                fields,
                span: sp(source_id, span),
            });

        // `user { … }` or `http.response { … }` (module-scoped type).
        let type_path = ident.clone().separated_by(just(TokenKind::Dot)).at_least(1);
        let struct_lit = type_path
            .then(field_list)
            .map_with_span(move |(path, fields), span| Expr::StructLit {
                path,
                fields,
                span: sp(source_id, span),
            });

        let list = expr
            .clone()
            .separated_by(just(TokenKind::Comma))
            .delimited_by(just(TokenKind::LBracket), just(TokenKind::RBracket))
            .map_with_span(move |items, span| Expr::List {
                items,
                span: sp(source_id, span),
            });

        let paren = expr
            .clone()
            .delimited_by(just(TokenKind::LParen), just(TokenKind::RParen))
            .map_with_span(move |e, span| Expr::Group {
                expr: Box::new(e),
                span: sp(source_id, span),
            });

        // `.` receiver or `.field` as field on implicit receiver
        let receiver = just(TokenKind::Dot)
            .then(ident.clone().or_not())
            .map_with_span(move |(_, field), span| {
                let s = sp(source_id, span);
                match field {
                    None => Expr::Receiver { span: s },
                    Some(f) => Expr::Field {
                        base: Box::new(Expr::Receiver {
                            span: Span::new(source_id, s.start, f.span.start),
                        }),
                        field: f,
                        span: s,
                    },
                }
            });

        let name = ident.clone().map(Expr::Name);

        // Order: fn before paren; struct_lit before name; object before...
        // struct_lit needs Ident LBrace — name alone is fine as atom.
        // Try fn, struct_lit, object, list, paren, literals, receiver, name
        let atom = choice((
            fn_expr, struct_lit, object, list, paren, number, duration, string, bytes, locator,
            boolean, receiver, name,
        ));

        // postfix: call, field, index
        let call_args = expr
            .clone()
            .separated_by(just(TokenKind::Comma))
            .delimited_by(just(TokenKind::LParen), just(TokenKind::RParen));

        let postfix = atom
            .clone()
            .then(
                choice((
                    call_args.map(|args| Postfix::Call(args)),
                    just(TokenKind::Dot)
                        .ignore_then(ident.clone())
                        .map(Postfix::Field),
                    expr.clone()
                        .delimited_by(just(TokenKind::LBracket), just(TokenKind::RBracket))
                        .map(Postfix::Index),
                ))
                .repeated(),
            )
            .map(move |(base, ops)| {
                ops.into_iter().fold(base, |base, op| {
                    let span = merge_span(source_id, base.span(), op.span(source_id));
                    match op {
                        Postfix::Call(args) => Expr::Call {
                            callee: Box::new(base),
                            args,
                            span,
                        },
                        Postfix::Field(field) => Expr::Field {
                            base: Box::new(base),
                            field,
                            span,
                        },
                        Postfix::Index(index) => Expr::Index {
                            base: Box::new(base),
                            index: Box::new(index),
                            span,
                        },
                    }
                })
            });

        // Unary ops bind tighter than binary; recurse only through unary layer.
        // `~` bit-not is dual-use with mutable-bind leader (expr position only).
        // `<ui64> expr` is an explicit width cast (no silent mix).
        let unary = {
            let postfix = postfix.clone();
            let ident_u = ident.clone();
            recursive(move |unary| {
                choice((
                    just(TokenKind::Minus)
                        .ignore_then(unary.clone())
                        .map_with_span(move |e, span| Expr::Unary {
                            op: UnaryOp::Neg,
                            expr: Box::new(e),
                            span: sp(source_id, span),
                        }),
                    just(TokenKind::Bang)
                        .ignore_then(unary.clone())
                        .map_with_span(move |e, span| Expr::Unary {
                            op: UnaryOp::Not,
                            expr: Box::new(e),
                            span: sp(source_id, span),
                        }),
                    just(TokenKind::Tilde)
                        .ignore_then(unary.clone())
                        .map_with_span(move |e, span| Expr::Unary {
                            op: UnaryOp::BitNot,
                            expr: Box::new(e),
                            span: sp(source_id, span),
                        }),
                    // `<width> …` — prefer signed/unsigned tagged **literal**
                    // (`<i32> -32`) over cast-of-unary, then general cast.
                    just(TokenKind::Lt)
                        .ignore_then(ident_u.clone())
                        .then_ignore(just(TokenKind::Gt))
                        .then(choice((
                            just(TokenKind::Minus)
                                .or_not()
                                .then(
                                    filter(|t| matches!(t, TokenKind::Number))
                                        .map_with_span(move |_, span| text_at(src, &span)),
                                )
                                .map(|(neg, text)| {
                                    let text = if neg.is_some() {
                                        format!("-{text}")
                                    } else {
                                        text
                                    };
                                    (true, text, None)
                                }),
                            unary.map(|e| (false, String::new(), Some(e))),
                        )))
                        .map_with_span(move |(wident, (is_lit, text, eopt)), span| {
                            let s = sp(source_id, span);
                            let width = Width::parse(&wident.name);
                            let tag = wident.name;
                            if is_lit {
                                match width {
                                    Some(w) => Expr::Number {
                                        text,
                                        width: Some(w),
                                        span: s,
                                    },
                                    None => Expr::WidthCast {
                                        width: None,
                                        tag,
                                        expr: Box::new(Expr::Number {
                                            text,
                                            width: None,
                                            span: s,
                                        }),
                                        span: s,
                                    },
                                }
                            } else {
                                let e = eopt.expect("cast expr");
                                match (e, width) {
                                    (
                                        Expr::Number {
                                            text, width: None, ..
                                        },
                                        Some(w),
                                    ) => Expr::Number {
                                        text,
                                        width: Some(w),
                                        span: s,
                                    },
                                    (other, w) => Expr::WidthCast {
                                        width: w,
                                        tag,
                                        expr: Box::new(other),
                                        span: s,
                                    },
                                }
                            }
                        }),
                    postfix,
                ))
            })
        };

        let product = unary
            .clone()
            .then(
                choice((
                    just(TokenKind::Star).to(BinaryOp::Mul),
                    just(TokenKind::Slash).to(BinaryOp::Div),
                    just(TokenKind::Percent).to(BinaryOp::Rem),
                ))
                .then(unary)
                .repeated(),
            )
            .foldl(move |left, (op, right)| {
                let span = merge_span(source_id, left.span(), right.span());
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                }
            });

        let sum = product
            .clone()
            .then(
                choice((
                    just(TokenKind::Plus).to(BinaryOp::Add),
                    just(TokenKind::Minus).to(BinaryOp::Sub),
                ))
                .then(product)
                .repeated(),
            )
            .foldl(move |left, (op, right)| {
                let span = merge_span(source_id, left.span(), right.span());
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                }
            });

        // Shifts: lower than +/-, higher than range/cmp (C-like).
        let shift = sum
            .clone()
            .then(
                choice((
                    just(TokenKind::LtLt).to(BinaryOp::Shl),
                    just(TokenKind::GtGt).to(BinaryOp::Shr),
                ))
                .then(sum)
                .repeated(),
            )
            .foldl(move |left, (op, right)| {
                let span = merge_span(source_id, left.span(), right.span());
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                }
            });

        // Inclusive range `lo..hi` — lower precedence than shifts, higher than cmp.
        // `1+2..3+4` → `(1+2)..(3+4)`. Non-associative: only one `..` level.
        let range = shift
            .clone()
            .then(just(TokenKind::DotDot).ignore_then(shift.clone()).or_not())
            .map(move |(left, rest)| match rest {
                None => left,
                Some(right) => {
                    let span = merge_span(source_id, left.span(), right.span());
                    Expr::Range {
                        start: Box::new(left),
                        end: Box::new(right),
                        span,
                    }
                }
            });

        let cmp = range
            .clone()
            .then(
                choice((
                    just(TokenKind::EqEqEq).to(BinaryOp::EqEqEq),
                    just(TokenKind::NotEqEq).to(BinaryOp::NotEqEq),
                    just(TokenKind::EqEq).to(BinaryOp::Eq),
                    just(TokenKind::NotEq).to(BinaryOp::NotEq),
                    just(TokenKind::LtEq).to(BinaryOp::LtEq),
                    just(TokenKind::GtEq).to(BinaryOp::GtEq),
                    just(TokenKind::Lt).to(BinaryOp::Lt),
                    just(TokenKind::Gt).to(BinaryOp::Gt),
                ))
                .then(range)
                .or_not(),
            )
            .map(move |(left, rest)| match rest {
                None => left,
                Some((op, right)) => {
                    let span = merge_span(source_id, left.span(), right.span());
                    Expr::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                        span,
                    }
                }
            });

        // Bitwise AND / XOR / OR (C-like: between cmp and boolean &&/||).
        // Binary `|` is BitOr; bare `|` remains the true atom in primary.
        let bit_and = cmp
            .clone()
            .then(
                just(TokenKind::Ampersand)
                    .to(BinaryOp::BitAnd)
                    .then(cmp)
                    .repeated(),
            )
            .foldl(move |left, (op, right)| {
                let span = merge_span(source_id, left.span(), right.span());
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                }
            });

        let bit_xor = bit_and
            .clone()
            .then(
                just(TokenKind::Caret)
                    .to(BinaryOp::BitXor)
                    .then(bit_and)
                    .repeated(),
            )
            .foldl(move |left, (op, right)| {
                let span = merge_span(source_id, left.span(), right.span());
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                }
            });

        let bit_or = bit_xor
            .clone()
            .then(
                just(TokenKind::Pipe)
                    .to(BinaryOp::BitOr)
                    .then(bit_xor)
                    .repeated(),
            )
            .foldl(move |left, (op, right)| {
                let span = merge_span(source_id, left.span(), right.span());
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                }
            });

        let and = bit_or
            .clone()
            .then(
                just(TokenKind::AndAnd)
                    .to(BinaryOp::And)
                    .then(bit_or)
                    .repeated(),
            )
            .foldl(move |left, (op, right)| {
                let span = merge_span(source_id, left.span(), right.span());
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                }
            });

        and.clone()
            .then(just(TokenKind::OrOr).to(BinaryOp::Or).then(and).repeated())
            .foldl(move |left, (op, right)| {
                let span = merge_span(source_id, left.span(), right.span());
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                    span,
                }
            })
    })
}

enum Postfix {
    Call(Vec<Expr>),
    Field(Ident),
    Index(Expr),
}

impl Postfix {
    fn span(&self, source_id: echo_source::SourceId) -> Span {
        match self {
            Postfix::Call(args) => {
                if let Some(last) = args.last() {
                    last.span()
                } else {
                    Span::new(source_id, BytePos(0), BytePos(0))
                }
            }
            Postfix::Field(i) => i.span,
            Postfix::Index(e) => e.span(),
        }
    }
}
