# Parser

Parsing tokens into a source-shaped AST.

| | |
|--|--|
| **Status** | Active — §2.1 leaders + expressions (chumsky) |
| **Owners** | `echo_parser`, `echo_ast` |
| **Library** | **chumsky** 0.9 ([ADR 0011](adr/0011-chumsky-parser.md)) |
| **Related** | `docs/syntax.md`, `docs/lexer.md`, ADR 0003 |

## Pipeline

```text
echo_source → echo_lexer → echo_parser (chumsky) → echo_ast
                ↓
         echo_diagnostics
         echo_syntax (leader facts)
```

Hosts call `echo_parser::parse` (or `xo ast`); they do not reimplement grammar.

## Facts

- Input is **tokens** from `echo_lexer` (leaders already classified).
- Output is `echo_ast::File` (source-shaped; no types / resolution).
- CLI: `xo ast [--kinds] [--diag-codes] <file>`
- Suite: `e26` requires `.ast` via `ast --kinds --diag-codes`
- **No trailing commas** (matches `syntax.md`); do not re-enable
  `allow_trailing` without a syntax change
- Field assigns accept **dotted chains**: `~ a.b.c = e`, `~ .a.b = e` (AST
  `AssignTarget::Field` with nested `Expr::Field` base)
- Index assigns: `~ xs[i] = e`, `~ a.b[i] = e` (`AssignTarget::Index` with index);
  list push: `~ xs[] = e`, `~ a.b[] = e` (`AssignTarget::Index` with `index: None`; field
  path before `[` is allowed)
- Edge cases must be justified (see `AGENTS.md` — Justified edge cases only)

## Open questions

- Richer recovery strategies as the grammar grows (must not invent meaning)
- Pratt combinator vs layered expr when expr surface expands
- Multi-bind (`~ a = 1, b = 2` / `$ x = 1, y = 2`) is **locked** and expanded to
  sequential binds after parse (no trailing comma)
