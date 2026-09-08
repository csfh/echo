# Echo Architecture

Echo is a compiled language with one LLVM backend and one Rust-owned runtime.
This document is the **ownership map** for the workspace.

- **Current vs SOTA gaps:** [`sota-gaps.md`](sota-gaps.md) (authority for spine quality)
- Full pipeline diagram: [`pipeline.md`](pipeline.md)
- ADR 0012: analysis product + shared library entry
- Agent workflow: [`../AGENTS.md`](../AGENTS.md)

## Compilation pipeline (actual)

```text
.echo sources
    │
    ▼
echo_pipeline::analyze / compile_to_llvm   ← hosts call this (xo, tests)
    │
    ├─ echo_source          SourceMap, Span, SourceId
    ├─ echo_lexer           tokens (via parse)
    ├─ echo_parser          Parsed { File }  (echo_ast types)
    ├─ echo_index           ModuleFacts
    ├─ echo_resolver        closed graph, imports, %/@ merge
    ├─ echo_semantics       validate → Diagnostics
    ├─ AnalysisProduct      File + HIR + SemanticModel + import/method facts + spans
    ├─ MIR SSA CFG          structured → CFG → φ/rename; codegen consumes cfg
    │         │ only if is_ok()
    ├─ echo_mir             MirProgram (structured executable IR)
    ├─ echo_codegen         LLVM IR (+ AOT/JIT)
    └─ echo_runtime         echo_runtime_* heap/services
```

Short form:

```text
Source → Lex → Parse/AST → Index → Graph resolve → Semantics
       → AnalysisProduct → MIR → LLVM → Runtime
```

**Rule:** Language meaning is decided in analysis. Lowering and codegen implement
that product; they must not re-decide module vs field, method binding, or similar.

There is no bytecode VM. AOT and JIT share `echo_runtime` symbols
(`echo_codegen_abi`).

## Crate ownership

| Crate | Owns |
|-------|------|
| `echo_source` | Source identity, text, byte spans |
| `echo_diagnostics` | Shared diagnostics (+ encode for cache) |
| `echo_syntax` | Leader/metadata facts (not a transform stage) |
| `echo_lexer` | Tokenization |
| `echo_ast` | Source-shaped AST types |
| `echo_parser` | Parse → AST |
| `echo_index` | Module facts for the graph |
| `echo_resolver` | Multi-file graph, import paths, struct merge |
| `echo_semantics` | Local scopes, binds, receivers, kinds/infer |
| `echo_pipeline` | **Shared analyze/compile entry**; `AnalysisProduct` |
| `echo_hir` | Source-shaped IR + import classification + methods + spans |
| `echo_mir` | Backend-neutral structured executable IR |
| `echo_codegen` | MIR → LLVM; link; JIT |
| `echo_codegen_abi` | Stable `echo_runtime_*` names |
| `echo_runtime` | Runtime values and services |
| `echo_std` | Std/runtime package tables |
| `echo_fingerprint` / `cache` / `build` | Incremental (orthogonal) |
| `echo_lsp` | Editor presentation over shared analyze path |
| `echo_wasm` | Browser check + playground-run host (`just wasm` → www `/try`) |
| `echo_oci` | Scratch OCI layout + registry push (ADR 0018); not a compiler backend |
| `xo` | CLI client of `echo_pipeline` |

Hosts must not redefine language semantics.

## Vertical slices

Language features land as full verticals on this spine. Incomplete analysis→lower
contracts are incomplete work (see `sota-gaps.md`).
