/**
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
module.exports = grammar({
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
    // `{ name : … }` — field init vs bare ident item then `:` else-leader.
    [$.field_initializer, $._expr_non_unary],
    // `: { \\n name` — else body item vs anonymous struct field list.
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

  rules: {
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
      $.import_statement,
      $.export_statement,
      // Not full _expression: top-level unary !/-/+ would steal dual-use leaders.
      $.expression_statement,
    ),

    // ── Statement leaders (full set from echo_syntax::LEADERS) ──
    // Leader-only glyphs: dedicated tokens (invalid as free expr atoms in echo_lexer).
    // leader_tilde: mutable bind
    leader_tilde: _ => '~',
    // leader_dollar: immutable bind
    leader_dollar: _ => '$',
    // leader_hash: compile-time constant
    leader_hash: _ => '#',
    // leader_at: struct members
    leader_at: _ => '@',
    // leader_question: if
    leader_question: _ => '?',
    // leader_caret: return
    leader_caret: _ => '^',
    // leader_backslash: export
    leader_backslash: _ => '\\',
    // Dual-use glyphs (* / ! < > | + - % :) are anonymous terminals shared by
// statement introducers and expression operators — one token per char
// (docs/lexer.md). Statement rules reference them as string literals;
// token names still appear in comments + highlights for the full set.
    // leader_percent (struct shape) dual-use — glyph % in statements and expressions
    // leader_colon (else-if / else / match default) dual-use — glyph : in statements and expressions
    // leader_bang (error return) dual-use — glyph ! in statements and expressions
    // leader_star (loop) dual-use — glyph * in statements and expressions
    // leader_lt (break) dual-use — glyph < in statements and expressions
    // leader_gt (continue) dual-use — glyph > in statements and expressions
    // leader_pipe (match) dual-use — glyph | in statements and expressions
    // leader_plus (task spawn) dual-use — glyph + in statements and expressions
    // leader_minus (task join) dual-use — glyph - in statements and expressions
    // leader_slash (import) dual-use — glyph / in statements and expressions

    // ── Statements (leaders only at statement start via grammar context) ──

    // Multi bind: `~ a = 1, b = 2`. Targets may be paths: `~ p.x =`, `~ xs[i] =`.
    bind_statement: $ => seq(
      field('leader', choice($.leader_tilde, $.leader_dollar, $.leader_hash)),
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
          seq('[', field('index', $._expression), ']'),
        )),
      ),
    )),

    struct_statement: $ => seq(
      field('leader', '%'),
      field('name', $.ident),
      '{',
      repeat(choice($._item, '\n')),
      '}',
    ),

    struct_extend_statement: $ => seq(
      field('leader', $.leader_at),
      field('name', $.ident),
      '{',
      repeat(choice($._item, '\n')),
      '}',
    ),

    if_statement: $ => seq(
      field('leader', $.leader_question),
      field('condition', $._expression),
      '{',
      repeat(choice($._item, '\n')),
      '}',
    ),

    // Prefer else-if (colon + expr) over bare else when both match.
    else_if_statement: $ => prec(1, seq(
      field('leader', ':'),
      field('condition', $._expression),
      '{',
      repeat(choice($._item, '\n')),
      '}',
    )),

    else_statement: $ => prec(0, seq(
      field('leader', ':'),
      '{',
      repeat(choice($._item, '\n')),
      '}',
    )),

    // Prefer including an optional value (`^ expr`) over bare `^` before next item.
    return_statement: $ => choice(
      prec.right(2, seq(
        field('leader', $.leader_caret),
        field('value', $._expression),
      )),
      prec(1, field('leader', $.leader_caret)),
    ),

    // prec.right inside; item-level prec(15) beats unary (8) / expression_statement (-1).
    error_return_statement: $ => prec.right(seq(
      field('leader', '!'),
      field('value', $._expression),
    )),

    loop_statement: $ => choice(
      // for-in: `* x : items {` / `* x : 1..4 {`
      // iterable must not use full _expression (struct_literal would swallow `{`).
      prec.right(2, seq(
        field('leader', '*'),
        field('binding', $.ident),
        ':',
        field('iterable', $._loop_header_expr),
        '{',
        repeat(choice($._item, '\n')),
        '}',
      )),
      prec.right(1, seq(
        field('leader', '*'),
        field('header', $._loop_header_expr),
        '{',
        repeat(choice($._item, '\n')),
        '}',
      )),
      seq(
        field('leader', '*'),
        '{',
        repeat(choice($._item, '\n')),
        '}',
      ),
    ),

    // Loop headers / for-in iterables: no struct_literal / fn brace forms.
    // Do not reuse field_expression/call_expression (their objects are full
    // _expr_non_unary and reintroduce struct_literal vs `{` body conflicts).
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

    break_statement: $ => field('leader', '<'),

    continue_statement: $ => field('leader', '>'),

    // Match leader is `|` + required whitespace (docs/lexer.md), distinct from bare
    // true-atom `|` so `$ x = |` and `| scrutinee {` do not collide.
    match_statement: $ => seq(
      field('leader', $.match_leader),
      field('scrutinee', $._match_scrutinee),
      '{',
      repeat(choice($.match_arm, '\n')),
      '}',
    ),

    // Token includes trailing WS so it cannot equal true_atom's bare `|`.
    match_leader: _ => token(seq('|', /[ \t]+/)),

    // Atoms + limited postfix — must not use full field/call rules that allow
    // `_expr_non_unary` objects (those include struct_literal and swallow `{`).
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
        // type arm: `% User {`
        seq(field('leader', '%'), field('type', $.ident)),
        // result ok bind: `$ name {` (leader_dollar + name)
        seq(field('leader', $.leader_dollar), field('name', $.ident)),
        // result err bind: `! name {`
        seq(field('leader', '!'), field('name', $.ident)),
        // default arm: `: {`
        field('leader', ':'),
        // literal / name / range patterns (comma-separated)
        seq(
          field('pattern', $._match_pattern),
          repeat(seq(',', field('pattern', $._match_pattern))),
        ),
      )),
      '{',
      repeat(choice($._item, '\n')),
      '}',
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
      field('leader', '+'),
      optional(seq(field('name', $.ident), '=')),
      choice(
        seq('{', repeat(choice($._item, '\n')), '}'),
        $._expression,
      ),
    )),

    task_join_statement: $ => prec.right(seq(
      field('leader', '-'),
      optional(seq(field('name', $.ident), '=')),
      choice(
        seq('{', repeat(choice($._item, '\n')), '}'),
        $._expression,
      ),
    )),

    import_statement: $ => seq(
      field('leader', '/'),
      field('path', $.import_path),
    ),

    // Single token path (std/io, ./user) — avoids / dual-use associativity issues.
    import_path: _ => token(choice(
      prec(1, /\.\/[A-Za-z_][A-Za-z0-9_]*(\/[A-Za-z_][A-Za-z0-9_]*)*/),
      prec(1, /[A-Za-z_][A-Za-z0-9_]*(\/[A-Za-z_][A-Za-z0-9_]*)*/),
    )),

    export_statement: $ => seq(
      field('leader', $.leader_backslash),
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

    binary_expression: $ => {
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
    },

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

    // Named struct lit / path-typed lit / anonymous field map (incl. empty `{ }`).
    // Fields may span lines; empty `{ }` has no bare-newline loop (avoids conflict
    // with `: { \\n }` else bodies that only repeat items/newlines).
    struct_literal: $ => choice(
      prec(9, seq(
        field('type', choice($.field_expression, $.ident)),
        '{',
        optional($._struct_field_list),
        '}',
      )),
      prec(12, seq(
        '{',
        optional($._struct_field_list),
        '}',
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

    // Fn / task body: (params) { } · (params) [captures] { } · () [a,b] { }.
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
      '{',
      repeat(choice($._item, '\n')),
      '}',
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
  },
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}

/* Dual-use leaders (glyph also expression token):
 *   leader_percent (%)
 *   leader_colon (:)
 *   leader_bang (!)
 *   leader_star (*)
 *   leader_lt (<)
 *   leader_gt (>)
 *   leader_pipe (|)
 *   leader_plus (+)
 *   leader_minus (-)
 *   leader_slash (/)
 * Leader-only (error outside statement start in echo_lexer):
 *   leader_tilde (~)
 *   leader_dollar ($)
 *   leader_hash (#)
 *   leader_at (@)
 *   leader_question (?)
 *   leader_caret (^)
 *   leader_backslash (\)
 */
