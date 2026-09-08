# 0011. Chumsky for parsing

## Status

Accepted.

## Context

Echo already has a custom lexer that owns dual-use leaders and statement-start
rules. The parser must consume that token stream and build a source-shaped AST
(ADR 0003). Hand-written recursive descent and several crates were considered;
pest re-lexes poorly for our model.

## Decision

Use **[chumsky](https://github.com/zesterer/chumsky)** (0.9.x line) in
`echo_parser` to parse **lexer tokens** into `echo_ast` nodes.

- Lexer remains the only tokenizer (`echo_lexer`).
- AST types stay hand-defined in `echo_ast` (not chumsky’s default tree).
- Parse errors map into `echo_diagnostics` with source spans.

## Consequences

- Grammar work lives as chumsky combinators under `echo_parser`.
- Expression precedence is implemented with layered chumsky parsers (or Pratt
  later), not a second lexer.
- Switching parser libraries later requires rewriting combinators, not the AST
  contract or lexer.
