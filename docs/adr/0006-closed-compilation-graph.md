# 0006. Compilation graph is closed

## Status

Accepted.

## Context

Open-ended “compile whatever the runtime finds on disk later” makes caching,
reproducibility, and name resolution unreliable.

## Decision

A program is a **closed compilation graph**: the set of sources and packages
admitted for one build. Graph membership comes from explicit inputs such as
entrypoints, imports, optional `xo.toml` deps (materialised into the **user**
package cache — [ADR 0014](0014-modules-packages-paths.md)), std roots, and
other static includes as those features land.

Dynamic execution must not silently admit arbitrary new sources outside the
graph. Out-of-graph loads fail with clear diagnostics or runtime errors.

## Consequences

- Resolver and build planning reason about a known set of members.
- Caching and fingerprints attach to graph identity.
- Features that need late loading must be designed as explicit graph or package
  operations, not ad hoc filesystem walks after compile.
