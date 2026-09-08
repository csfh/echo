# Documentation map

Contributor and implementer facts for Echo. User-facing product documentation
lives under `www/` and is published at [https://xo.run](https://xo.run).

## Always on

| Doc | Role |
|-----|------|
| [`../AGENTS.md`](../AGENTS.md) | Workflow and invariants for humans/agents |
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | How to contribute; CLA by submission |
| [`../CLA.md`](../CLA.md) | Contributor agreement (IP assignment to Christoffer Hallas) |
| [`../LICENSE`](../LICENSE) | MIT License (copyright Christoffer Hallas) |
| [`../SECURITY.md`](../SECURITY.md) | Vulnerability reporting (`oss@christofferhallas.com`) |
| [`architecture.md`](architecture.md) | Crate ownership and pipeline sketch |
| [`sota-gaps.md`](sota-gaps.md) | Current vs SOTA spine; gap inventory |
| [`pipeline.md`](pipeline.md) | **Full** spine + hosts (fmt, LSP, REPL, e26) + build-out order |
| [`incremental.md`](incremental.md) | Fingerprint / cache / build plan (orthogonal infra) |
| [`implementation.md`](implementation.md) | Per-feature vertical checklist (all layers) |
| [`glossary.md`](glossary.md) | Shared vocabulary |
| [`development-speed.md`](development-speed.md) | Local tools, gate, edit loop |
| [`install.md`](install.md) | XDG install / upgrade / uninstall (`scripts/install.sh`) |
| [`ci.md`](ci.md) | GitHub Actions multi-OS build + Linux echo26 gate |
| [`roadmap.md`](roadmap.md) | Language coverage map + design/impl status |
| [`fixtures.md`](fixtures.md) | Echo 2026 suite (`echo26/` / `e26`) conventions |
| [`testing.md`](testing.md) | How to test: crate / echo26 / examples / `xo test` |

## Durable decisions

| Doc | Role |
|-----|------|
| [`adr/`](adr/) | Architecture decision records (compiler/platform; surface ADRs as needed) |

## Language surface

| Doc | Domain |
|-----|--------|
| [`syntax.md`](syntax.md) | Implementer language surface for **Echo 2026** (**core locked**) |
| [ADR 0015](adr/0015-echo-2026-canonical-edition.md) | Edition name + canonical public Spec ownership |
| [`lexer.md`](lexer.md) | Tokens and lexing rules |
| [`semantics.md`](semantics.md) | Scopes, Result/Option, inference direction |
| [`modules.md`](modules.md) | Imports, packages, `%`/`@` merge |
| [`roadmap.md`](roadmap.md) | Entire language map + phases |

## Layer / domain specs

Expand as the corresponding layer gains real rules. Status may be “not started.”

| Doc | Domain |
|-----|--------|
| [`parser.md`](parser.md) | Parsing and AST construction |
| [`hir.md`](hir.md) | High-level IR (active) |
| [`mir.md`](mir.md) | Mid-level executable IR (active) |
| [`diagnostics.md`](diagnostics.md) | Diagnostic codes and contracts |
| [`runtime-abi.md`](runtime-abi.md) | Runtime symbols and ABI |
| [`memory.md`](memory.md) | Scope-owned reclamation (not tracing GC; ADR 0016) |
| [`stdlib.md`](stdlib.md) | Standard library surface |
| [`stdlib-go-gaps.md`](stdlib-go-gaps.md) | Echo std vs Go stdlib gap inventory |
| [`llvm.md`](llvm.md) | Codegen, optimization, link |
| [`lsp.md`](lsp.md) | Language server boundary |
| [`repl.md`](repl.md) | Interactive REPL (`xo repl`, JIT) |
| [`tree-sitter.md`](tree-sitter.md) | Generated tree-sitter grammar (`xo tools grammar tree-sitter`) |
| [`reflection.md`](reflection.md) | Tools `echo_reflection` vs userland `std/reflect` |
| [`incremental.md`](incremental.md) | Cache / fingerprint / build |

## Where to put a new fact

1. **Sticky decision** → ADR (when it must stick).
2. **Who owns a concept** → `architecture.md` / `glossary.md` / `pipeline.md`.
3. **Language rule** → `syntax.md` / `lexer.md` / `semantics.md` / `modules.md` + `roadmap.md`.
4. **How to implement a feature end-to-end** → `implementation.md` checklist.
5. **How to run something** → `development-speed.md` or `AGENTS.md`.
6. **User-facing explanation** → `www/` once the site page exists.

Proof of behavior always belongs in tests or fixtures; docs state the rule.
