# Echo compiler: current pipeline vs state-of-the-art

| | |
|--|--|
| **Status** | Living document — **target spine is the authority for new work** |
| **Related** | [`architecture.md`](architecture.md), [`pipeline.md`](pipeline.md), ADR 0012 |

This document records (1) what the implementation **actually** does, (2) every
**material gap** vs a state-of-the-art (SOTA) compiler spine, and (3) which gaps
are **closed** vs **deferred**.

---

## 1. Current pipeline (as implemented)

### 1.1 Meaning spine (hosts must use `echo_pipeline`)

```text
SourceMap / SourceFile          echo_source
        │
        ▼
lex (Token + Span)              echo_lexer          ← usually called inside parse
        │
        ▼
Parsed { file: Option<File> }   echo_parser → echo_ast types
        │
        ▼
ModuleFacts                     echo_index::extract
        │
        ▼
ResolvedGraph (multi-file)      echo_resolver
  import paths, %/@ merge,
  ModuleUnit { parsed, facts, import_targets }
        │
        ▼
Diagnostics (check)             echo_semantics::check_file_with_modules
  + infer                       (scopes, binds, receivers, modules list)
        │
        ▼
AnalysisProduct                 echo_pipeline::analyze
  AnalyzedModule { file, hir, imports, exports, semantic, … }
  is_ok() ⇔ no error diagnostics
        │
        │  only if is_ok()
        ▼
MirProgram                      echo_mir::lower_program (SemanticModel + HIR facts)
  MirFn { body, cfg: SSA MirCfg, … }
        │
        ▼
LLVM IR string                  echo_codegen::emit_llvm (SSA CFG: blocks + φ)
        │
        ├─ AOT: clang + libecho_runtime
        └─ JIT: same echo_runtime_* symbols
```

### 1.2 Orthogonal layers (not second semantics)

- `echo_fingerprint` / `echo_cache` / `echo_build` — phase keys and `.xo` store  
- `echo_diagnostics` — shared diagnostic model  
- `echo_syntax` — leader/metadata tables (not a transform stage)  
- `echo_codegen_abi` — stable `echo_runtime_*` names  
- `echo_std` — privileged std/runtime package tables  
- Hosts: `xo`, `echo_lsp`, `e26` — clients of `echo_pipeline` for meaning  

### 1.3 Important types

| Stage | Key types / functions |
|-------|------------------------|
| Source | `SourceMap`, `SourceFile`, `Span`, `SourceId` |
| Lex | `lex` → `Lexed { tokens, diagnostics }` |
| Parse | `parse` / `parse_with_cache` → `Parsed` |
| Index | `extract(&File) → ModuleFacts` |
| Graph | `resolve_entry*`, `ResolvedGraph`, `ModuleUnit` |
| Check | `check_file_with_modules` → `Diagnostics` |
| Analysis product | `echo_pipeline::analyze` → `AnalysisProduct` |
| HIR | `HirModule`, `lower_file(file, import_names)` |
| MIR | `ModuleLowerInput` from `AnalyzedModule`, `lower_program` |
| Codegen | `emit_llvm`, `link_aot`, `run_jit_ir` |

---

## 2. Gap inventory (SOTA vs current)

Each item: **gap** → **SOTA requirement** → **status**.

| ID | Gap | SOTA requirement | Status |
|----|-----|------------------|--------|
| G1 | Check produced diagnostics only; HIR rebuilt from raw AST in `xo` after a separate check | Analysis yields a **consumable product**; lower consumes only that product | **Closed** — `AnalysisProduct` + `echo_pipeline::compile_*` |
| G2 | MIR re-decided module vs value field and method dispatch using ad hoc envs | Language meaning decided in analysis/HIR facts; MIR implements | **Closed** for module/field (HIR classifies via import set); methods table on `HirModule` from analysis lower |
| G3 | HIR lacked spans / “analyzed” was aspirational | Provenance for AST-spanned constructs survives into analysis product | **Closed** — HIR nodes carry `Span`; product retains `File` |
| G4 | Hosts assembled check → HIR → MIR independently | Shared library entry for check/compile | **Closed** — `echo_pipeline::{analyze, compile_to_llvm, lower_to_mir}` |
| G5 | Failed analysis could still be lowered if a host skipped the error gate | No successful executable lower of rejected programs | **Closed** — `lower_to_mir` / `compile_to_llvm` refuse when `!product.is_ok()` |
| G6 | Docs diagram contradicted live call graph | Docs match reality | **Closed** — this file + architecture/pipeline updates |
| G7 | No line/column map in `echo_source` | Line maps for IDE-quality diags | **Closed** — `LineMap` on `SourceFile`; `xo` emits `path:line:col`; LSP still uses UTF-16 `byte_to_position` |
| G8 | MIR is structured (If/Loop), not BB/SSA | CFG/SSA + repr handoff for **hyper-optimizable LLVM IR** | **Closed** — CFG → SSA → repr → simplify → escape → simplify → LLVM; generic mid-end is LLVM ([`mir.md`](mir.md)) |
| G9 | Full DefId / region type system | Industrial type IR | **Partial** — `SemanticModel` (`BindId`, `ValueKind`, `value_struct`) packaged on `AnalyzedModule`; not full industrial types |
| G10 | Full IDE (hover, complete, goto) | Rich LSP | **Closed** — `echo_lsp` full depth on shared pipeline (see [`lsp.md`](lsp.md)) |
| G11 | No LLVM mid-end / `-O` model | Same `OptLevel` → `default<On>` for JIT+AOT; verify gates | **Closed** — shared `OptLevel`; O0 skips passes; Oz is size pipeline; IR/AOT cache keys safe; `xo ir|run|build` |

---

## 3. Target spine (authority for new work)

```text
Source → Lex → Parse/AST → Index → Graph resolve
       → Semantics (validate + facts)
       → AnalysisProduct { AST + HIR + SemanticModel + imports/exports/methods + diags + spans }
       → MIR structured body → CFG → SSA → repr → simplify → escape → simplify
       → Codegen → LLVM `default<On>` (O0 = no mid-end) → AOT | JIT
```

**Rule:** hosts never call `echo_hir::lower_file` on an AST that was not packaged into an `AnalysisProduct` for the same analysis session, and never lower when `!is_ok()`. MIR seeds struct typing only from `SemanticModel` (+ flow of `StructLit` / copy of known names).

**MIR rule:** language semantics, native representations, and Echo/ABI-aware
escape + `NoEscape` box elision ([`mir.md`](mir.md)). LLVM owns constprop, GVN,
LICM, IV, DCE, and general mid-end opts at `-O1`…`-O3`/`-Oz`.

---

## 4. Non-goals (this SOTA spine pass)

- Completing remaining Echo language surface  
- More MIR textbook passes without an Echo/ABI barrier proof  
- Full DefId / region type system (**G9 remainder**)  
- ~~Full LSP feature set (**G10**)~~ closed  

- PHP compatibility  
- www / book overhaul  
- Cache performance tuning beyond not breaking `.xo` phases  

---

## 5. Gap → fix map

| Gap | Fix |
|-----|-----|
| G1, G4, G5 | `crates/echo_pipeline` — `analyze`, `lower_to_mir`, `compile_to_llvm` |
| Overlay IR cache isolation | `compile_to_llvm` **skips** IR get/put when `overlays` non-empty (same policy as check semantic cache) |
| G2 | `lower_file(file, import_module_names)` classifies ModuleCall/Field; methods on HIR; MIR method resolve from `SemanticModel` |
| G3 | `HirExpr` / `HirStmt` carry `span: Span`; product keeps `File` |
| G6 | `docs/sota-gaps.md`, `architecture.md`, `pipeline.md` |
| G7 | `echo_source::LineMap`; `xo` `emit_diags` line:col; MIR diags `.with_span` |
| G8 | `structured_to_cfg` + `construct_ssa` + CFG codegen; ForIn → prims; MatchTagged + `MatchPayload` |
| G9 | `echo_semantics::SemanticModel` on `AnalyzedModule` |
| G10 | `echo_lsp` features over `analyze` / index / `format_source` |

---

## 6. Proof

- Crate tests: `echo_pipeline`, `echo_mir` (CFG + SSA rename/φ), codegen SSA path  
- `e26 --binary target/debug/xo` green (`echo26/` is the living count)  
- Hosts: `xo` uses `echo_pipeline` + line:col diags; LSP analyze uses shared pipeline
