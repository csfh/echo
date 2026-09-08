# 0009. Language features land as full vertical slices

## Status

Accepted.

## Context

Landing only a lexer or only a runtime helper leaves unusable partial features
and encourages host-local workarounds.

## Decision

When practical, language features ship as **full vertical slices**: relevant
frontend through IR, codegen, runtime, CLI surface, proof (unit or fixture), and
docs. Crate boundaries stay fixed so each slice has a home without inventing
parallel pipelines.

Thin or empty crates may exist as structure; incomplete *features* should not be
declared done after a single layer.

## Consequences

- Planning and PRs prefer end-to-end proofs for user-visible behavior.
- Layer-only refactors remain allowed when they enable verticals.
- Docs for a feature update when the surface is real, not when a stub appears.
