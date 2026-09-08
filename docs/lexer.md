# Lexer

| | |
|--|--|
| **Status** | **Implemented** for statement leaders (§2.1) + supporting tokens |
| **Owners** | `echo_lexer`, leader facts in `echo_syntax::leaders` |
| **Related** | `docs/syntax.md`, `docs/roadmap.md` |
| **CLI** | `xo lex <file>` |

## Where to follow along (source)

| Path | What |
|------|------|
| `crates/echo_syntax/src/leaders/mod.rs` | `LeaderKind`, full table, families |
| `crates/echo_syntax/src/leaders/bind.rs` | `~` `$` `#` |
| `crates/echo_syntax/src/leaders/shape.rs` | `%` `@` |
| `crates/echo_syntax/src/leaders/control.rs` | control-flow leaders |
| `crates/echo_syntax/src/leaders/module.rs` | `/` `\` |
| `crates/echo_lexer/src/lib.rs` | Scanner (statement start, dual-use) |
| `crates/xo/src/main.rs` | `xo lex` dump |
| `echo26/leaders/` | Lex fixtures (bind/shape/control/module) |
| `crates/e26/` | Suite runner binary `e26 --binary …` |

## Source

- UTF-8
- Identifiers: `[A-Za-z_][A-Za-z0-9_]*`, case-sensitive, ASCII
- Style: **snake_case**; **struct names** lowercase
- `#` constant **names** are validated later (semantics); lexer emits `ident`

## Comments

- `;` → EOL (full-line or trailing); not emitted as tokens
- Not `//`

## Statement start

A position is **statement start** after:

- beginning of file
- a newline (`\n` or `\r\n`)
- only spaces/tabs (indent) since that newline

Line comments do not clear the next line’s statement start.

At statement start, leader glyphs become `TokenKind::Leader(LeaderKind)`.
Elsewhere, dual-use glyphs are expression tokens (`star`, `slash`, `bang`,
`lt`, …). Glyphs that are **only** leaders (`~ $ # @ ? ^ \`) produce
`lex-unexpected-leader-glyph` outside leader position.

## Leaders (roadmap §2.1)

Table owned by `echo_syntax::LeaderKind` / `LEADERS`.

Whitespace **required** after the leader glyph except bare `<` and `>`.
Missing whitespace → diagnostic `lex-leader-ws` (token still emitted).

| Glyph | Token name | Role |
|-------|------------|------|
| `~` | `leader_tilde` | mutable bind |
| `$` | `leader_dollar` | immutable bind |
| `#` | `leader_hash` | compile-time constant |
| `%` | `leader_percent` | struct shape **or** match type arm (parser dual-use) |
| `@` | `leader_at` | more struct members |
| `?` | `leader_question` | if |
| `:` | `leader_colon` | else-if / else / match default |
| `!` | `leader_bang` | error return (Result err) |
| `^` | `leader_caret` | return |
| `*` | `leader_star` | loop |
| `<` | `leader_lt` | break |
| `>` | `leader_gt` | continue |
| `\|` | `leader_pipe` | match |
| `+` | `leader_plus` | task spawn |
| `-` | `leader_minus` | task join / immediate block |
| `&` | `leader_ampersand` | effect block (auto-unwrap result/option) |
| `/` | `leader_slash` | import |
| `\` | `leader_backslash` | export (not continuation) |

`+` and `-` are leaders **only** at statement start (with required whitespace).
In expression position they remain `plus` / `minus` operators.

`&` is dual-use: **effect block** at statement start; bitwise **and** in expression
position (with `\|` `^` `<<` `>>` and unary `~`).

`~` and `^` are dual-use: leaders (bind / return) at statement start; expression
tokens `tilde` (bit-not) and `caret` (bit-xor) elsewhere.

**No** line-continuation token.

## Other tokens (supporting)

| Kind | Notes |
|------|--------|
| `ident` | ASCII idents |
| `number` | decimal, `0x`/`0b`, `_`, floats, exponent |
| `string_pure` / `string_rich` | `'…'` / `"…"` (rich escapes validated; `{ident}` left for later stages) |
| `underscore` | lone `_` (false) |
| `pipe` | expr `\|` (true atom **or** bitwise OR between ints) — not match leader |
| ops / punct | `+ - * / % == != === !== < > <= >= << >> && \|\| & ^ ~ ! . , : = ( ) [ ] { }` |

## Diagnostics

| Code | Meaning |
|------|---------|
| `lex-leader-ws` | Leader missing required trailing whitespace |
| `lex-unexpected-leader-glyph` | Leader-only glyph in expression position |
| `lex-unexpected` | Unknown character |
| `lex-string-pure` / `lex-string-rich` | Unterminated string |
| `lex-escape` | Unknown / incomplete / invalid hex escape in rich `"…"`, `b"…"`, `p"…"` |

## Numbers / strings (detail)

- Decimal, `0x`/`0X`, `0b`/`0B`, `_` between digits
- Floats: `3.14`, `1e-3`
- Pure `'...'`: no escapes; interior `'` ends the string
- Rich `"..."` / `b"..."` / `p"..."`: locked escapes `\n` `\t` `\r` `\\` `\"` `\{` `\}` `\xHH`.
  Anything else is `lex-escape` (not rewritten). `{ident}` left for later stages
