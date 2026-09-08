# 0001. Shared compiler pipeline

## Status

Accepted.

## Context

Language tools (CLI, REPL, LSP, tests) all need consistent language meaning. If
each host reimplements binding, typing, or evaluation, behavior drifts and bugs
multiply.

## Decision

There is **one** compiler pipeline. Layers own meaning as mapped in
`docs/architecture.md`. Hosts (`xo`, `echo_lsp`, future embeds) orchestrate and
present; they do not define alternate language semantics.

## Consequences

- New behavior lands in the earliest shared crate that owns it.
- REPL and tests drive the real pipeline, not host-local evaluators.
- Presentation formatting may live in hosts; diagnostic *categories* and
  analysis facts do not.
