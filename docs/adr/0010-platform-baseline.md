# 0010. Platform baseline

## Status

Accepted.

## Context

Unlimited platform surface area delays a working compiler. The team needs an
explicit host baseline for tools and CI assumptions.

## Decision

**Primary development and support baseline: Linux.** macOS is a desired secondary
host. Windows-native is out of scope for the current phase; WSL counts as Linux.

Tooling assumptions (clang, mold, sccache, nextest, just) are documented for
Linux first in `docs/development-speed.md`.

## Consequences

- Platform-specific code paths are isolated and justified.
- CI and docs prioritize Linux; macOS gaps are tracked explicitly when found.
- Windows support requires a later ADR or explicit expansion.
