# Architecture decision records

Durable decisions for Echo. Each ADR records **context**, **decision**, and
**consequences**. Prefer short ADRs over essays.

Language **surface** decisions are intentionally sparse after the design reset.
Prefer writing evolving rules in `docs/syntax.md` / `docs/lexer.md` until a
choice must stick forever—then add an ADR.

## Index

### Compiler and platform (in force)

| ADR | Title |
|-----|-------|
| [0001](0001-shared-compiler-pipeline.md) | Shared compiler pipeline |
| [0002](0002-llvm-only-execution-backend.md) | LLVM-only execution backend |
| [0003](0003-ast-is-source-shaped.md) | AST is source-shaped |
| [0004](0004-rust-runtime-owns-executable-semantics.md) | Rust runtime owns executable semantics |
| [0005](0005-structured-diagnostics-contract.md) | Structured diagnostics are a shared contract |
| [0006](0006-closed-compilation-graph.md) | Compilation graph is closed |
| [0007](0007-docs-vs-www-ownership.md) | `docs/` vs `www/` ownership |
| [0008](0008-standalone-language-not-php-superset.md) | Standalone language, not PHP superset |
| [0009](0009-full-vertical-slices.md) | Language features land as full vertical slices |
| [0010](0010-platform-baseline.md) | Platform baseline |
| [0011](0011-chumsky-parser.md) | Chumsky for parsing |
| [0012](0012-analysis-product-pipeline.md) | Analysis product and shared pipeline library |
| [0013](0013-tasks-event-loop-leaders.md) | Tasks + event loop via `+` / `-` leaders (not `std/task`) |
| [0014](0014-modules-packages-paths.md) | Modules / paths / optional `xo.toml` / user `.xo` package cache |
| [0015](0015-echo-2026-canonical-edition.md) | Echo 2026 edition + canonical public Language Spec |
| [0016](0016-scope-owned-memory.md) | Scope-owned memory: promote / demote / release on scope exit |
| [0018](0018-static-aot-scratch-oci.md) | Static AOT + scratch OCI packaging |

### Language surface

| ADR | Title |
|-----|-------|
| [0013](0013-tasks-event-loop-leaders.md) | Task spawn/join leaders + runtime event loop |
| [0014](0014-modules-packages-paths.md) | Modules / packages / paths / `$XO_HOME/packages` |
| [0015](0015-echo-2026-canonical-edition.md) | Echo 2026 edition + canonical public Language Spec |
| [0016](0016-scope-owned-memory.md) | Scope-owned memory (deterministic disposal) |
| [0018](0018-static-aot-scratch-oci.md) | Static AOT + scratch OCI packaging |

## When to write an ADR

- The choice is hard to reverse or expensive to re-argue.
- Two reasonable alternatives existed and one was selected.
- A product or architecture boundary must stay stable across many PRs.

Do **not** ADR routine syntax experiments while still exploring. Put those in
`syntax.md` and the roadmap session log first.
