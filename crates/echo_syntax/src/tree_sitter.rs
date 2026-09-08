//! Emit a tree-sitter grammar package from shared `LeaderKind` / surface facts.
//!
//! Authority: [`crate::leaders`] (and lexer docs for literals/comments). The
//! generated package is a derived artifact — regenerate via
//! `xo tools grammar tree-sitter -o <dir>`; do not hand-maintain a second
//! leader table in the package.

use std::fs;
use std::io;
use std::path::Path;

use crate::leaders::{LeaderKind, LEADERS};

/// One file in a generated tree-sitter package (relative path + UTF-8 body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarFile {
    pub relative_path: String,
    pub content: String,
}

/// Build the full package as in-memory files (stable, pure).
#[must_use]
pub fn tree_sitter_package_files() -> Vec<GrammarFile> {
    vec![
        GrammarFile {
            relative_path: "grammar.js".into(),
            content: emit_grammar_js(),
        },
        GrammarFile {
            relative_path: "package.json".into(),
            content: emit_package_json(),
        },
        GrammarFile {
            relative_path: "tree-sitter.json".into(),
            content: emit_tree_sitter_json(),
        },
        GrammarFile {
            relative_path: "queries/highlights.scm".into(),
            content: emit_highlights_scm(),
        },
        GrammarFile {
            relative_path: "README.md".into(),
            content: emit_readme(),
        },
    ]
}

/// Write the package under `output_dir` (creates parents as needed).
pub fn write_tree_sitter_grammar(output_dir: &Path) -> io::Result<()> {
    for file in tree_sitter_package_files() {
        let path = output_dir.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, file.content.as_bytes())?;
    }
    Ok(())
}

fn js_string_char(c: char) -> String {
    match c {
        '\\' => r"'\\'".into(),
        '\'' => r"'\''".into(),
        _ => format!("'{c}'"),
    }
}

fn emit_leader_token_rules() -> String {
    let mut out = String::new();
    out.push_str(
        "    // Leader-only glyphs: dedicated tokens (invalid as free expr atoms in echo_lexer).\n",
    );
    for kind in LEADERS.iter().filter(|k| !k.is_dual_use()) {
        let name = kind.token_name();
        let glyph = js_string_char(kind.glyph());
        out.push_str(&format!(
            "    // {name}: {}\n    {name}: _ => {glyph},\n",
            kind.role()
        ));
    }
    out.push_str(
        "    // Dual-use glyphs (* / ! < > | + - % :) are anonymous terminals shared by\n\
         // statement introducers and expression operators — one token per char\n\
         // (docs/lexer.md). Statement rules reference them as string literals;\n\
         // token names still appear in comments + highlights for the full set.\n",
    );
    for kind in LEADERS.iter().filter(|k| k.is_dual_use()) {
        out.push_str(&format!(
            "    // {} ({}) dual-use — glyph {} in statements and expressions\n",
            kind.token_name(),
            kind.role(),
            kind.glyph()
        ));
    }
    out
}

/// JS fragment for a leader introducer in a statement rule.
fn leader_ref(kind: LeaderKind) -> String {
    if kind.is_dual_use() {
        js_string_char(kind.glyph())
    } else {
        format!("$.{}", kind.token_name())
    }
}

fn emit_dual_use_footer() -> String {
    let mut out = String::from("\n/* Dual-use leaders (glyph also expression token):\n");
    for kind in LEADERS.iter().filter(|k| k.is_dual_use()) {
        out.push_str(&format!(" *   {} ({})\n", kind.token_name(), kind.glyph()));
    }
    out.push_str(" * Leader-only (error outside statement start in echo_lexer):\n");
    for kind in LEADERS.iter().filter(|k| !k.is_dual_use()) {
        out.push_str(&format!(" *   {} ({})\n", kind.token_name(), kind.glyph()));
    }
    out.push_str(" */\n");
    out
}

fn emit_grammar_js() -> String {
    let leaders = emit_leader_token_rules();
    let footer = emit_dual_use_footer();
    // Bind / shape / control / module leader refs from shared LEADERS.
    let ld = |k: LeaderKind| leader_ref(k);
    let bind_leaders = format!(
        "choice({}, {}, {})",
        ld(LeaderKind::Tilde),
        ld(LeaderKind::Dollar),
        ld(LeaderKind::Hash),
    );
    let percent = ld(LeaderKind::Percent);
    let at = ld(LeaderKind::At);
    let question = ld(LeaderKind::Question);
    let colon = ld(LeaderKind::Colon);
    let caret = ld(LeaderKind::Caret);
    let bang = ld(LeaderKind::Bang);
    let star = ld(LeaderKind::Star);
    let lt = ld(LeaderKind::Lt);
    let gt = ld(LeaderKind::Gt);
    let plus = ld(LeaderKind::Plus);
    let minus = ld(LeaderKind::Minus);
    let ampersand = ld(LeaderKind::Ampersand);
    let slash = ld(LeaderKind::Slash);
    let backslash = ld(LeaderKind::Backslash);

    format!(
        r###"/**
 * Tree-sitter grammar for Echo.
 *
 * Generated from echo_syntax::LEADERS / surface facts by
 * `xo tools grammar tree-sitter`. Not a second language authority —
 * regenerate after leader or lexer surface changes.
 *
 * Dual-use: leader_* tokens appear only as statement introducers;
 * the same glyphs reappear as operators/punctuation inside expressions
 * (tree-sitter resolves by parse context, matching docs/lexer.md).
 */
module.exports = grammar({{
  name: 'echo',

  // Newlines are significant: dual-use operators must not cross line boundaries
  // into the next statement (echo_lexer statement-start leaders).
  extras: $ => [
    /[ \t\f\v\r]/,
    $.comment,
  ],

  word: $ => $.ident,

  conflicts: $ => [
    [$.else_if_statement, $.else_statement],
    [$.expression_statement, $._expression],
    [$.fn_expression, $._expression],
    [$.fn_expression, $._expr_non_unary],
    [$.fn_expression, $.parenthesized_expression],
    [$.call_expression, $.parenthesized_expression],
    // `{{ name : … }}` — field init vs bare ident item then `:` else-leader.
    [$.field_initializer, $._expr_non_unary],
    // `: {{ \\n name` — else body item vs anonymous struct field list.
    [$.else_statement, $._struct_field_list],
    [$.else_if_statement, $._struct_field_list],
    [$.if_statement, $._struct_field_list],
    [$.struct_statement, $._struct_field_list],
    [$.struct_extend_statement, $._struct_field_list],
    [$.fn_expression, $._struct_field_list],
    [$.loop_statement, $._struct_field_list],
    [$.match_arm, $._struct_field_list],
    // `~ .field…` — bind path vs unary bit-not of self_field (dual-use `~`).
    [$.bind_lhs, $.self_field],
  ],

  rules: {{
    source_file: $ => repeat(choice($._item, '\n')),

    // `;` → EOL (docs/lexer.md). Not emitted by echo_lexer; still highlighted.
    comment: _ => token(seq(';', /[^\n]*/)),

    _item: $ => choice(
      $.bind_statement,
      $.struct_statement,
      $.struct_extend_statement,
      $.if_statement,
      $.else_if_statement,
      $.else_statement,
      $.return_statement,
      $.error_return_statement,
      $.loop_statement,
      $.break_statement,
      $.continue_statement,
      $.match_statement,
      $.task_spawn_statement,
      $.task_join_statement,
      $.effect_block_statement,
      $.import_statement,
      $.export_statement,
      // Not full _expression: top-level unary !/-/+ would steal dual-use leaders.
      $.expression_statement,
    ),

    // ── Statement leaders (full set from echo_syntax::LEADERS) ──
{leaders}
    // ── Statements (leaders only at statement start via grammar context) ──

    // Multi bind: `~ a = 1, b = 2`. Targets may be paths: `~ p.x =`, `~ xs[i] =`.
    bind_statement: $ => seq(
      field('leader', {bind_leaders}),
      commaSep1($.bind_clause),
    ),

    bind_clause: $ => seq(
      field('target', $.bind_lhs),
      optional(seq(
        '=',
        field('value', $._expression),
      )),
    ),

    // Bind / assign left-hand side: `name`, `a.b.c`, `xs[i]`, `a.b[i]`,
    // and receiver paths `.field` / `.a.b`.
    // prec.right so `.` / `[` shift into the path instead of reducing bare name
    // (prec.left committed early and left `.x` as a free receiver expression).
    bind_lhs: $ => prec.right(10, choice(
      seq(
        '.',
        field('field', $.ident),
        repeat(seq('.', field('field', $.ident))),
      ),
      seq(
        $.ident,
        repeat(choice(
          seq('.', field('field', $.ident)),
          seq('[', optional(field('index', $._expression)), ']'),
        )),
      ),
    )),

    struct_statement: $ => seq(
      field('leader', {percent}),
      field('name', $.ident),
      '{{',
      repeat(choice($._item, '\n')),
      '}}',
    ),

    struct_extend_statement: $ => seq(
      field('leader', {at}),
      field('name', $.ident),
      '{{',
      repeat(choice($._item, '\n')),
      '}}',
    ),

    if_statement: $ => seq(
      field('leader', {question}),
      field('condition', $._expression),
      '{{',
      repeat(choice($._item, '\n')),
      '}}',
    ),

    // Prefer else-if (colon + expr) over bare else when both match.
    else_if_statement: $ => prec(1, seq(
      field('leader', {colon}),
      field('condition', $._expression),
      '{{',
      repeat(choice($._item, '\n')),
      '}}',
    )),

    else_statement: $ => prec(0, seq(
      field('leader', {colon}),
      '{{',
      repeat(choice($._item, '\n')),
      '}}',
    )),

    // Prefer including an optional value (`^ expr`) over bare `^` before next item.
    return_statement: $ => choice(
      prec.right(2, seq(
        field('leader', {caret}),
        field('value', $._expression),
      )),
      prec(1, field('leader', {caret})),
    ),

    // prec.right inside; item-level prec(15) beats unary (8) / expression_statement (-1).
    error_return_statement: $ => prec.right(seq(
      field('leader', {bang}),
      field('value', $._expression),
    )),

    loop_statement: $ => choice(
      // for-in: `* x : items {{` / `* x : 1..4 {{`
      // iterable must not use full _expression (struct_literal would swallow `{{`).
      prec.right(2, seq(
        field('leader', {star}),
        field('binding', $.ident),
        ':',
        field('iterable', $._loop_header_expr),
        '{{',
        repeat(choice($._item, '\n')),
        '}}',
      )),
      prec.right(1, seq(
        field('leader', {star}),
        field('header', $._loop_header_expr),
        '{{',
        repeat(choice($._item, '\n')),
        '}}',
      )),
      seq(
        field('leader', {star}),
        '{{',
        repeat(choice($._item, '\n')),
        '}}',
      ),
    ),

    // Loop headers / for-in iterables: no struct_literal / fn brace forms.
    // Do not reuse field_expression/call_expression (their objects are full
    // _expr_non_unary and reintroduce struct_literal vs `{{` body conflicts).
    _loop_header_expr: $ => choice(
      $.number,
      $.string_pure,
      $.string_rich,
      $.true_atom,
      $.false_atom,
      $.self_field,
      $.receiver,
      $.range_expression,
      $.parenthesized_expression,
      prec(10, seq(
        $.ident,
        optional(choice(
          seq('(', optional(commaSep($._expression)), ')'),
          seq('.', $.ident, repeat(seq('.', $.ident))),
          seq('[', $._expression, ']'),
        )),
      )),
    ),

    break_statement: $ => field('leader', {lt}),

    continue_statement: $ => field('leader', {gt}),

    // Match leader is `|` + required whitespace (docs/lexer.md), distinct from bare
    // true-atom `|` so `$ x = |` and `| scrutinee {{` do not collide.
    match_statement: $ => seq(
      field('leader', $.match_leader),
      field('scrutinee', $._match_scrutinee),
      '{{',
      repeat(choice($.match_arm, '\n')),
      '}}',
    ),

    // Token includes trailing WS so it cannot equal true_atom's bare `|`.
    match_leader: _ => token(seq('|', /[ \t]+/)),

    // Atoms + limited postfix — must not use full field/call rules that allow
    // `_expr_non_unary` objects (those include struct_literal and swallow `{{`).
    _match_scrutinee: $ => choice(
      $.number,
      $.string_pure,
      $.string_rich,
      $.true_atom,
      $.false_atom,
      $.self_field,
      $.receiver,
      $.parenthesized_expression,
      prec(9, seq(
        $.ident,
        optional(choice(
          seq('(', optional(commaSep($._expression)), ')'),
          seq('.', $.ident, repeat(seq('.', $.ident))),
          seq('[', $._expression, ']'),
        )),
      )),
    ),

    match_arm: $ => seq(
      field('header', choice(
        // type arm: `% User {{`
        seq(field('leader', {percent}), field('type', $.ident)),
        // result ok bind: `$ name {{` (leader_dollar + name)
        seq(field('leader', $.leader_dollar), field('name', $.ident)),
        // result err bind: `! name {{`
        seq(field('leader', {bang}), field('name', $.ident)),
        // default arm: `: {{`
        field('leader', {colon}),
        // literal / name / range patterns (comma-separated)
        seq(
          field('pattern', $._match_pattern),
          repeat(seq(',', field('pattern', $._match_pattern))),
        ),
      )),
      '{{',
      repeat(choice($._item, '\n')),
      '}}',
    ),

    // Keep range as `number .. number` only so pattern FIRST sets stay simple.
    _match_pattern: $ => choice(
      $.number,
      $.string_pure,
      $.string_rich,
      $.ident,
      $.true_atom,
      $.false_atom,
      prec(1, seq($.number, '..', $.number)),
      $.width_cast,
    ),

    task_spawn_statement: $ => prec.right(seq(
      field('leader', {plus}),
      optional(seq(field('name', $.ident), '=')),
      choice(
        seq('{{', repeat(choice($._item, '\n')), '}}'),
        $._expression,
      ),
    )),

    task_join_statement: $ => prec.right(seq(
      field('leader', {minus}),
      optional(seq(field('name', $.ident), '=')),
      choice(
        seq('{{', repeat(choice($._item, '\n')), '}}'),
        $._expression,
      ),
    )),

    // `& {{ … }}` / `& name = {{ … }}` — effect block (auto-unwrap result/option).
    effect_block_statement: $ => prec.right(seq(
      field('leader', {ampersand}),
      optional(seq(field('name', $.ident), '=')),
      '{{',
      repeat(choice($._item, '\n')),
      '}}',
    )),

    import_statement: $ => seq(
      field('leader', {slash}),
      field('path', $.import_path),
    ),

    // Single token path (std/io, ./user) — avoids / dual-use associativity issues.
    import_path: _ => token(choice(
      prec(1, /\.\/[A-Za-z_][A-Za-z0-9_]*(\/[A-Za-z_][A-Za-z0-9_]*)*/),
      prec(1, /[A-Za-z_][A-Za-z0-9_]*(\/[A-Za-z_][A-Za-z0-9_]*)*/),
    )),

    export_statement: $ => seq(
      field('leader', {backslash}),
      field('names', commaSep1($.ident)),
    ),

    // Standalone expression item — no top-level unary dual-use (see _expr_non_unary).
    expression_statement: $ => prec(-1, $._expr_non_unary),

    // ── Expressions (dual-use glyphs as operators / punctuation) ──

    // Full expression (bind inits, call args, …) includes unary !/-/+.
    _expression: $ => choice(
      $.unary_expression,
      $._expr_non_unary,
    ),

    // Non-unary roots: item-level !/-/+ are statement leaders (docs/lexer.md).
    _expr_non_unary: $ => choice(
      $.ident,
      $.number,
      $.string_pure,
      $.string_rich,
      $.bytes_pure,
      $.bytes_rich,
      $.locator_pure,
      $.locator_rich,
      $.duration,
      // `|` true / `_` false — true atom lower prec so match_statement wins at item start.
      $.true_atom,
      $.false_atom,
      // Self: bare `.` or `.field` atom (echo_parser receiver); further `.x` via field_expression.
      $.self_field,
      $.receiver,
      $.binary_expression,
      $.call_expression,
      $.field_expression,
      $.index_expression,
      $.list_expression,
      $.struct_literal,
      $.fn_expression,
      $.parenthesized_expression,
      $.range_expression,
      $.width_cast,
    ),

    // Bare self value (method fall-off `^ .` / return `.`).
    receiver: _ => '.',

    // `.name` as Field(Receiver, name) — one leading dot, not `..name`.
    self_field: $ => prec(10, seq('.', field('field', $.ident))),

    // Boolean atoms. True is bare `|` (match uses match_leader with trailing WS).
    true_atom: _ => '|',
    false_atom: _ => '_',

    // Unary only nested in _expression (e.g. `$ x = -1`), never as bare _item.
    // `~` bit-not dual-uses mutable-bind leader glyph. Prec above <<>> (8).
    unary_expression: $ => prec(11, seq(
      field('operator', choice('!', '-', '+', '~')),
      field('argument', $._expression),
    )),

    binary_expression: $ => {{
      // Mirrors echo_parser: higher prec number binds tighter.
      // */% → +- → <<>> → (range) → cmp → & → ^ → | → && → ||
      const table = [
        ['||', 1],
        ['&&', 2],
        ['|', 3],
        ['^', 4],
        ['&', 5],
        [choice('==', '!=', '===', '!==', '<', '>', '<=', '>='), 6],
        [choice('<<', '>>'), 8],
        [choice('+', '-'), 9],
        [choice('*', '/', '%'), 10],
      ];
      return choice(...table.map(([op, precedence]) =>
        prec.left(precedence, seq(
          field('left', $._expression),
          field('operator', op),
          field('right', $._expression),
        ))
      ));
    }},

    range_expression: $ => prec.left(7, seq(
      field('start', $._expression),
      '..',
      field('end', $._expression),
    )),

    // Width / type cast: `<i32> 10`, `<i32> -32` (docs/syntax.md).
    // Prec above <<>> (8) so `<i32>1 << 2` is `(width) << 2`.
    width_cast: $ => prec(11, seq(
      '<',
      field('type', $.ident),
      '>',
      field('value', $._expression),
    )),

    call_expression: $ => prec(9, seq(
      field('function', $._expr_non_unary),
      '(',
      optional(commaSep($._expression)),
      ')',
    )),

    field_expression: $ => prec(9, seq(
      field('object', $._expr_non_unary),
      '.',
      field('field', $.ident),
    )),

    index_expression: $ => prec(9, seq(
      field('object', $._expr_non_unary),
      '[',
      field('index', $._expression),
      ']',
    )),

    // Allow newlines between elements (significant newlines are not extras).
    list_expression: $ => seq(
      '[',
      optional(seq(
        optional('\n'),
        $._expression,
        repeat(seq(
          choice(
            seq(',', optional('\n')),
            seq(optional(','), '\n'),
          ),
          $._expression,
        )),
        optional(','),
        optional('\n'),
      )),
      ']',
    ),

    // Named struct lit / path-typed lit / anonymous field map (incl. empty `{{ }}`).
    // Fields may span lines; empty `{{ }}` has no bare-newline loop (avoids conflict
    // with `: {{ \\n }}` else bodies that only repeat items/newlines).
    struct_literal: $ => choice(
      prec(9, seq(
        field('type', choice($.field_expression, $.ident)),
        '{{',
        optional($._struct_field_list),
        '}}',
      )),
      prec(12, seq(
        '{{',
        optional($._struct_field_list),
        '}}',
      )),
    ),

    _struct_field_list: $ => seq(
      optional('\n'),
      $.field_initializer,
      repeat(seq(
        choice(
          seq(',', optional('\n')),
          seq(optional(','), '\n'),
        ),
        $.field_initializer,
      )),
      optional(','),
      optional('\n'),
    ),

    field_initializer: $ => prec(11, seq(
      field('name', $.ident),
      ':',
      field('value', $._expression),
    )),

    // Fn / task body: (params) {{ }} · (params) [captures] {{ }} · () [a,b] {{ }}.
    // Prefer when ) then optional captures then block; GLR conflict with paren listed above.
    fn_expression: $ => prec(10, seq(
      '(',
      optional(commaSep($.ident)),
      ')',
      optional(seq(
        '[',
        optional(commaSep($.ident)),
        ']',
      )),
      '{{',
      repeat(choice($._item, '\n')),
      '}}',
    )),

    parenthesized_expression: $ => prec(1, seq('(', $._expression, ')')),

    // ── Literals / idents (docs/lexer.md) ──

    ident: _ => /[A-Za-z_][A-Za-z0-9_]*/,

    number: _ => token(choice(
      /0[xX][0-9A-Fa-f_]+/,
      /0[bB][01_]+/,
      /[0-9][0-9_]*(\.[0-9][0-9_]*)?([eE][+-]?[0-9_]+)?/,
    )),

    duration: _ => token(/[0-9][0-9_]*[a-zA-Z]+/),

    string_pure: _ => token(seq("'", /[^']*/, "'")),
    string_rich: _ => token(seq('"', repeat(choice(/[^"\\]/, /\\./)), '"')),

    bytes_pure: _ => token(seq("b'", /[^']*/, "'")),
    bytes_rich: _ => token(seq('b"', repeat(choice(/[^"\\]/, /\\./)), '"')),

    locator_pure: _ => token(seq("p'", /[^']*/, "'")),
    locator_rich: _ => token(seq('p"', repeat(choice(/[^"\\]/, /\\./)), '"')),
  }},
}});

function commaSep(rule) {{
  return optional(commaSep1(rule));
}}

function commaSep1(rule) {{
  return seq(rule, repeat(seq(',', rule)));
}}
{footer}"###,
        leaders = leaders,
        footer = footer,
        bind_leaders = bind_leaders,
        percent = percent,
        at = at,
        question = question,
        colon = colon,
        caret = caret,
        bang = bang,
        star = star,
        lt = lt,
        gt = gt,
        plus = plus,
        minus = minus,
        slash = slash,
        backslash = backslash,
    )
}

fn emit_package_json() -> String {
    r#"{
  "name": "tree-sitter-echo",
  "version": "0.0.1",
  "description": "Echo grammar for tree-sitter (generated from echo_syntax)",
  "main": "bindings/node",
  "keywords": ["parser", "lexer", "echo", "tree-sitter"],
  "license": "MIT",
  "tree-sitter": [
    {
      "scope": "source.echo",
      "file-types": ["echo"],
      "highlights": ["queries/highlights.scm"],
      "injection-regex": "^echo$"
    }
  ]
}
"#
    .into()
}

fn emit_tree_sitter_json() -> String {
    r#"{
  "grammars": [
    {
      "name": "echo",
      "camelcase": "Echo",
      "scope": "source.echo",
      "path": ".",
      "file-types": ["echo"],
      "highlights": "queries/highlights.scm",
      "injection-regex": "^echo$"
    }
  ],
  "metadata": {
    "version": "0.0.1",
    "license": "MIT",
    "description": "Echo grammar for tree-sitter (generated from echo_syntax)",
    "links": {
      "repository": "https://github.com/csfh/echo"
    }
  },
  "bindings": {
    "c": true,
    "go": false,
    "node": true,
    "python": false,
    "rust": true,
    "swift": false
  }
}
"#
    .into()
}

fn emit_highlights_scm() -> String {
    let mut out = String::new();
    out.push_str(
        "; Echo highlights — generated from echo_syntax leader set.\n\
         ; Captures: leaders vs idents vs literals vs comments.\n\n",
    );
    out.push_str("; Line comments (; → EOL)\n(comment) @comment\n\n");
    out.push_str("; Leader-only tokens (named rules)\n");
    for kind in LEADERS.iter().filter(|k| !k.is_dual_use()) {
        out.push_str(&format!("({}) @keyword\n", kind.token_name()));
    }
    out.push_str("\n; Dual-use leaders as statement introducers (anonymous glyph in context)\n");
    for kind in LEADERS.iter().filter(|k| k.is_dual_use()) {
        // parent.field capture where possible; also list glyph under statement forms
        out.push_str(&format!(
            "; {} ({})\n",
            kind.token_name(),
            kind.glyph()
        ));
    }
    // Statement-scoped dual-use glyphs → keyword
    out.push_str(
        "(struct_statement leader: \"%\" @keyword)\n\
         (match_arm leader: \"%\" @keyword)\n\
         (else_if_statement leader: \":\" @keyword)\n\
         (else_statement leader: \":\" @keyword)\n\
         (match_arm leader: \":\" @keyword)\n\
         (error_return_statement leader: \"!\" @keyword)\n\
         (match_arm leader: \"!\" @keyword)\n\
         (loop_statement leader: \"*\" @keyword)\n\
         (break_statement leader: \"<\" @keyword)\n\
         (continue_statement leader: \">\" @keyword)\n\
         (match_leader) @keyword\n\
         (task_spawn_statement leader: \"+\" @keyword)\n\
         (task_join_statement leader: \"-\" @keyword)\n\
         (import_statement leader: \"/\" @keyword)\n\n",
    );
    out.push_str(
        "; Identifiers\n\
         (ident) @variable\n\
         (bind_clause target: (bind_lhs) @variable)\n\
         (bind_lhs (ident) @variable)\n\
         (bind_lhs field: (ident) @property)\n\
         (struct_statement name: (ident) @type)\n\
         (struct_extend_statement name: (ident) @type)\n\
         (struct_literal type: (ident) @type)\n\
         (struct_literal type: (field_expression) @type)\n\
         (field_initializer name: (ident) @property)\n\
         (field_expression field: (ident) @property)\n\
         (call_expression function: (ident) @function)\n\
         (call_expression function: (field_expression field: (ident) @function.method))\n\
         (export_statement names: (ident) @variable)\n\
         (import_statement path: (import_path) @namespace)\n\n\
         ; Literals\n\
         (number) @number\n\
         (duration) @number\n\
         (string_pure) @string\n\
         (string_rich) @string\n\
         (bytes_pure) @string\n\
         (bytes_rich) @string\n\
         (locator_pure) @string\n\
         (locator_rich) @string\n\
         ; true/false atoms (expr `|` / `_`); match leader `|` highlighted above\n\
         (true_atom) @constant.builtin\n\
         (false_atom) @constant.builtin\n\
         (receiver) @variable.builtin\n\
         (self_field field: (ident) @property)\n\
         (width_cast type: (ident) @type)\n\n\
         ; Operators / punctuation (expression dual-use surface)\n\
         ; Listed before more-specific leader captures win via query order in editors\n\
         ; that last-match-wins; leaders above already mark statement glyphs.\n\
         [\n\
           \"=\" \"==\" \"!=\" \"===\" \"!==\"\n\
           \"<\" \">\" \"<=\" \">=\" \"<<\" \">>\"\n\
           \"+\" \"-\" \"*\" \"/\" \"%\"\n\
           \"&&\" \"||\" \"&\" \"^\" \"|\" \"~\" \"!\" \"..\"\n\
           \".\" \",\" \":\"\n\
           \"(\" \")\" \"[\" \"]\" \"{\" \"}\"\n\
         ] @operator\n",
    );
    out
}

fn emit_readme() -> String {
    let dual: Vec<_> = LEADERS
        .iter()
        .filter(|k| k.is_dual_use())
        .map(|k| format!("`{}` ({})", k.glyph(), k.token_name()))
        .collect();
    let only: Vec<_> = LEADERS
        .iter()
        .filter(|k| !k.is_dual_use())
        .map(|k| format!("`{}` ({})", k.glyph(), k.token_name()))
        .collect();

    format!(
        r#"# tree-sitter-echo

Tree-sitter grammar for the **Echo** language.

## Source of truth

This package is **generated** from shared Echo syntax facts
(`echo_syntax::LEADERS` / `LeaderKind`, aligned with `docs/lexer.md`).

```bash
# from the Echo repo
cargo build -p xo
./target/debug/xo tools grammar tree-sitter -o path/to/tree-sitter-echo
```

Do not treat hand edits to this tree as language authority — re-run the
generator after leader or lexer surface changes.

## Leaders ({count})

| Token | Glyph | Dual-use |
|-------|-------|----------|
{leader_rows}

- **Dual-use glyphs** (leader at statement start; operator/token in expressions): {dual}
- **Leader-only** (statement introducers; invalid as free expression glyphs in the real lexer): {only}

Dual-use is modeled by **grammar context**: `leader_*` tokens only appear as
statement introducers; the same characters appear again inside expression rules.

## Build (optional)

Requires the [tree-sitter CLI](https://tree-sitter.github.io/tree-sitter/):

```bash
tree-sitter generate
tree-sitter parse path/to/file.echo
```

## Highlighting

`queries/highlights.scm` marks:

- leaders → `@keyword`
- idents → `@variable` / `@type` / `@property`
- strings / numbers → `@string` / `@number`
- comments → `@comment`

For IDE-quality kinds (bind vs use, etc.) prefer the Echo language server
semantic tokens (`xo lsp`); this grammar is the structural basemap.
"#,
        count = LEADERS.len(),
        leader_rows = LEADERS
            .iter()
            .map(|k| {
                format!(
                    "| `{}` | `{}` | {} |",
                    k.token_name(),
                    k.glyph(),
                    if k.is_dual_use() { "yes" } else { "no" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        dual = dual.join(", "),
        only = only.join(", "),
    )
}

/// Glyphs of every statement leader (for tests / tooling checks).
#[must_use]
pub fn leader_glyphs() -> Vec<char> {
    LEADERS.iter().map(|k| k.glyph()).collect()
}

/// Token names of every statement leader.
#[must_use]
pub fn leader_token_names() -> Vec<&'static str> {
    LEADERS.iter().map(|k| k.token_name()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LeaderFamily, LeaderKind};

    #[test]
    fn package_contains_required_files() {
        let files = tree_sitter_package_files();
        let paths: Vec<_> = files.iter().map(|f| f.relative_path.as_str()).collect();
        assert!(paths.contains(&"grammar.js"));
        assert!(paths.contains(&"package.json"));
        assert!(paths.contains(&"tree-sitter.json"));
        assert!(paths.contains(&"queries/highlights.scm"));
        assert!(paths.contains(&"README.md"));
    }

    #[test]
    fn grammar_names_echo_and_all_leaders() {
        let g = emit_grammar_js();
        assert!(g.contains("name: 'echo'"), "{g}");
        assert_eq!(LEADERS.len(), 18);
        for kind in LEADERS {
            assert!(
                g.contains(kind.token_name()),
                "missing {} in grammar",
                kind.token_name()
            );
            if kind.glyph() == '\\' {
                assert!(g.contains(r"'\\'") || g.contains("leader_backslash"));
            } else {
                let needle = format!("'{}'", kind.glyph());
                assert!(
                    g.contains(&needle) || g.contains(kind.token_name()),
                    "glyph {} missing",
                    kind.glyph()
                );
            }
        }
        assert!(g.contains("Dual-use leaders"));
        for kind in LEADERS.iter().filter(|k| k.is_dual_use()) {
            assert!(g.contains(kind.token_name()));
        }
    }

    #[test]
    fn highlights_distinct_captures() {
        let h = emit_highlights_scm();
        assert!(h.contains("@keyword"));
        assert!(h.contains("@variable"));
        assert!(h.contains("@string"));
        assert!(h.contains("@comment"));
        assert!(h.contains("@number"));
        for kind in LEADERS.iter().filter(|k| !k.is_dual_use()) {
            assert!(
                h.contains(&format!("({}) @keyword", kind.token_name())),
                "missing highlight for {}",
                kind.token_name()
            );
        }
        // Dual-use leaders still documented and statement-scoped in highlights.
        assert!(h.contains("Dual-use leaders") || h.contains("dual-use") || h.contains("@keyword"));
        assert!(h.contains("loop_statement") || h.contains("\"*\" @keyword"));
        for kind in LEADERS {
            assert!(
                h.contains(kind.token_name()) || h.contains(&format!("\"{}\"", kind.glyph())),
                "highlights missing {}",
                kind.token_name()
            );
        }
        assert!(h.contains("(string_pure) @string") || h.contains("string_pure"));
        assert!(h.contains("(comment) @comment"));
    }

    #[test]
    fn leaders_match_echo_syntax_table() {
        let names = leader_token_names();
        assert_eq!(names.len(), LEADERS.len());
        assert_eq!(names[0], LeaderKind::Tilde.token_name());
        assert!(!LeaderKind::Dollar.is_dual_use());
        assert!(LeaderKind::Star.is_dual_use());
        assert!(LeaderKind::Slash.is_dual_use());
        assert!(LeaderKind::Percent.is_dual_use());
        let mut n = 0;
        for fam in [
            LeaderFamily::Bind,
            LeaderFamily::Shape,
            LeaderFamily::Control,
            LeaderFamily::Module,
        ] {
            n += fam.leaders().len();
        }
        assert_eq!(n, LEADERS.len());
    }

    #[test]
    fn write_package_roundtrip_stable() {
        let mut root = std::env::temp_dir();
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-ts-grammar-{t}"));
        let a = root.join("a");
        let b = root.join("b");
        write_tree_sitter_grammar(&a).unwrap();
        write_tree_sitter_grammar(&b).unwrap();
        for file in tree_sitter_package_files() {
            let ca = fs::read_to_string(a.join(&file.relative_path)).unwrap();
            let cb = fs::read_to_string(b.join(&file.relative_path)).unwrap();
            assert_eq!(ca, cb, "unstable emit for {}", file.relative_path);
            assert_eq!(ca, file.content);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_json_and_tree_sitter_json_identify_echo() {
        let pj = emit_package_json();
        assert!(pj.contains("tree-sitter-echo"));
        assert!(pj.contains("echo"));
        let tj = emit_tree_sitter_json();
        assert!(tj.contains(r#""name": "echo""#));
        assert!(tj.contains("highlights.scm"));
    }

    #[test]
    fn grammar_covers_literals_and_comments() {
        let g = emit_grammar_js();
        assert!(g.contains("comment:"));
        assert!(g.contains("string_pure:"));
        assert!(g.contains("string_rich:"));
        assert!(g.contains("bytes_pure:"));
        assert!(g.contains("ident:"));
        assert!(g.contains("number:"));
        assert!(g.contains("bind_statement:"));
        assert!(g.contains("bind_clause:"));
        assert!(g.contains("bind_lhs:"));
        assert!(g.contains("struct_literal:"));
        assert!(g.contains("field_initializer:"));
        assert!(g.contains("Dual-use"));
    }

    #[test]
    fn dual_use_item_start_not_top_level_unary() {
        // Statement-start !/-/+ must be statement rules; unary only nested in _expression.
        let g = emit_grammar_js();
        assert!(
            g.contains("expression_statement: $ => prec(-1, $._expr_non_unary)")
                || g.contains("expression_statement: $ => prec(-1, $._expr_statement_root)"),
            "expression_statement must not accept top-level unary: {g}"
        );
        assert!(g.contains("_expr_non_unary: $ => choice("));
        assert!(g.contains("_expression: $ => choice(\n      $.unary_expression,\n      $._expr_non_unary,"));
        assert!(g.contains("error_return_statement:"));
        assert!(g.contains("task_join_statement:"));
        assert!(g.contains("task_spawn_statement:"));
        // Unary exists for nested uses ($ x = -1) but is not in expression_statement root.
        assert!(g.contains("unary_expression:"));
        let expr_stmt = g
            .split("expression_statement:")
            .nth(1)
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("");
        assert!(
            expr_stmt.contains("_expr_non_unary") && !expr_stmt.contains("$.unary_expression"),
            "expression_statement must use _expr_non_unary only: {expr_stmt}"
        );
    }

    /// When `tree-sitter` CLI is installed, parse dual-use leaders as statement nodes.
    #[test]
    fn tree_sitter_cli_parses_dual_use_statement_leaders() {
        let ts = match std::process::Command::new("tree-sitter")
            .arg("--version")
            .output()
        {
            Ok(o) if o.status.success() => true,
            _ => false,
        };
        if !ts {
            eprintln!("tree-sitter CLI not available; skipping live parse of dual-use leaders");
            return;
        }

        let mut root = std::env::temp_dir();
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("echo-ts-dual-{t}"));
        write_tree_sitter_grammar(&root).unwrap();

        let generate = std::process::Command::new("tree-sitter")
            .arg("generate")
            .current_dir(&root)
            .output()
            .expect("tree-sitter generate");
        assert!(
            generate.status.success(),
            "tree-sitter generate failed:\n{}",
            String::from_utf8_lossy(&generate.stderr)
        );

        let sample = root.join("dual.echo");
        // Statement-start dual-use leaders + nested unary in a bind init.
        fs::write(
            &sample,
            "$ t = 1\n- t\n! t\n+ t\n$ x = -1\n",
        )
        .unwrap();

        let parse = std::process::Command::new("tree-sitter")
            .args(["parse", sample.to_str().unwrap()])
            .current_dir(&root)
            .output()
            .expect("tree-sitter parse");
        let out = format!(
            "{}{}",
            String::from_utf8_lossy(&parse.stdout),
            String::from_utf8_lossy(&parse.stderr)
        );
        assert!(
            parse.status.success(),
            "tree-sitter parse failed:\n{out}"
        );
        assert!(!out.contains("(ERROR"), "unexpected ERROR nodes:\n{out}");
        assert!(
            out.contains("task_join_statement"),
            "expected task_join for `- t`:\n{out}"
        );
        assert!(
            out.contains("error_return_statement"),
            "expected error_return for `! t`:\n{out}"
        );
        assert!(
            out.contains("task_spawn_statement"),
            "expected task_spawn for `+ t`:\n{out}"
        );
        // Nested unary still works in bind init.
        assert!(
            out.contains("unary_expression"),
            "expected unary in `$ x = -1`:\n{out}"
        );
        // Must not mis-parse dual-use leaders as top-level expression_statement+unary only.
        // (unary may appear once for bind init; statement forms must also appear.)
        let join_at = out.find("task_join_statement").expect("join");
        let unary_at = out.find("unary_expression").expect("unary");
        assert!(
            join_at < unary_at || out.matches("unary_expression").count() == 1,
            "structure:\n{out}"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
