# 0003. AST is source-shaped

## Status

Accepted.

## Context

If the AST mixes source syntax with runtime or backend concepts, every later
layer becomes harder to reason about and diagnostics lose fidelity to source.

## Decision

The AST (`echo_ast`) represents **parsed source syntax and structure**, not
lowered semantic, runtime, or LLVM meaning. Source constructs that share meaning
should still lower through shared later paths; the AST may keep distinct surface
forms when that helps diagnostics and tooling.

## Consequences

- Lowering and analysis own meaning (`echo_semantics`, HIR, MIR, runtime).
- Spans stay tied to source tokens and constructs.
- Do not stuff type-checked or machine-level concepts into the AST.
