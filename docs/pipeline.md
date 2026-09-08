# Full implementation pipeline

End-to-end map of the Echo toolchain: **shared spine**, **hosts**, **conformance**,
and **product build-out order**.

Related:

- Crate ownership diagram: [`architecture.md`](architecture.md)
- Per-feature checklist: [`implementation.md`](implementation.md)
- Design status: [`roadmap.md`](roadmap.md)
- Fixtures: [`fixtures.md`](fixtures.md)
- Agent rules: [`../AGENTS.md`](../AGENTS.md)

**Rule:** Language meaning lives in the **earliest shared spine crate**. Hosts
(`xo`, LSP, fmt, REPL, www, `e26`) only orchestrate and present.

**Edition:** **Echo 2026** is the current language edition and canonical public
Language Spec (ADR 0015). The **executable contract** is `echo26/` via `e26`.

---

## 1. Shared compiler spine

**Authority for gaps vs SOTA:** [`sota-gaps.md`](sota-gaps.md).  
**Library entry:** `echo_pipeline::{analyze, compile_to_llvm, lower_to_mir}`.

```text
.echo source files
        │
        ▼
┌───────────────────┐
│  echo_source      │  paths, SourceId, spans, file text
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_diagnostics │  structured diagnostics (lexer onward)
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_syntax      │  leaders, grammar facts (tables)
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_lexer       │  tokens, dual-use glyphs
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_parser      │  chumsky → AST
│  echo_ast         │  source-shaped tree (ADR 0003)
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_index       │  ModuleFacts
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_resolver    │  closed graph, imports, %/@ merge
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_semantics   │  scopes, binds, kinds → Diagnostics
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_pipeline    │  AnalysisProduct (AST+HIR+SemanticModel+facts+spans)
│                   │  lower only if is_ok()
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  echo_mir         │  structured executable IR
└─────────┬─────────┘
          │
          ▼
            ┌───────────────────┐
            │  echo_codegen_abi │
            │  echo_codegen     │  MIR → LLVM IR
            └─────────┬─────────┘
                      │
           ┌──────────┴──────────┐
           ▼                     ▼
     AOT (native)          JIT (in-process)
           │                     │
           └──────────┬──────────┘
                      ▼
            ┌───────────────────┐
            │  echo_runtime     │  shared AOT + JIT (`echo_runtime_*`)
            │  echo_std         │  std package facts / runtime export table
            │  echo_reflection  │  callable metadata (from graph, not nm)
            └───────────────────┘
```

**Short form:**

```text
Source → Lex → Parse/AST → Index/Resolve → Semantics (infer)
       → HIR → MIR → LLVM → AOT | JIT → Runtime
```

There is **no** bytecode VM and **no** second language engine (ADR 0001–0004).

### Std and `/ runtime` in the spine (locked)

Full rules: [`stdlib.md`](stdlib.md). Every host uses this path:

```text
User:  / std/io  →  io.print
Std:   / runtime →  runtime.print(value)     ; only legal in std root files
Codegen: runtime.*  →  echo_runtime_*
LSP/check/reflection: real Echo AST + exports (no free userland print)
```

| Layer | Must |
|-------|------|
| `echo_resolver` | Gate `/ runtime`; resolve `/ std/…` to toolchain std |
| `echo_semantics` | No free `print`; `runtime.*` only if import legal |
| `echo_codegen` | Lower `runtime.*` via ABI map; not bare-name magic in userland |
| `echo_runtime` | Implement matching `echo_runtime_*` |
| `echo_reflection` / `echo_lsp` | Same facts as check — std surface always visible |

---

## 2. Incremental build (orthogonal)

Does not redefine language meaning; wraps the spine for reuse:

```text
echo_fingerprint  →  phase invalidation + component versions
echo_cache        →  `.xo/` layout + ArtifactStore
echo_build        →  BuildMode / plan / cascade helpers
```

CLI: `xo cache status|doctor|clean` (v0). Pipeline phases do **not** yet
read/write the store on every check/run — see **[`incremental.md`](incremental.md)**
for phases, versioning policy, and infra milestones.

---

## 3. Hosts (thin)

| Host | Entry | Role |
|------|--------|------|
| **`xo`** | `crates/xo` | CLI: lex, ast, check, run, build, fmt, lsp, repl, … |
| **`e26`** | `crates/e26` | Echo 2026 suite runner: drives a **candidate binary** over `echo26/` |
| **Formatter** | `xo fmt` (planned) | Pretty-print from shared AST/syntax |
| **LSP** | `echo_lsp` / `xo lsp` | Editor protocol over pipeline + index (std surface is real Echo; see `stdlib.md` surface vs bridge) |
| **REPL** | `xo repl` (planned) | Interactive; eval via JIT/runtime |
| **www** | `www/` | User-facing docs, search, **Echo 2026** public Spec, `/try` |
| **Browser check** | `echo_wasm` | wasm frontend for `/try` (`just wasm`); check/fmt + playground run |
| **Editor grammar** | from `echo_syntax` | `xo tools grammar tree-sitter -o …` ([`tree-sitter.md`](tree-sitter.md)) |

Hosts **must not** reimplement binding, typing, or execution rules.

### Host plug-in diagram

```text
                    ┌──────── echo_lsp ────────┐
                    │  diags, hover, complete  │
                    │  goto, rename, format    │
                    └────────────┬─────────────┘
                                 │
┌──────── xo ────────┐           │  same spine
│ lex ast check      │◄──────────┤
│ run build          │           │
│ fmt                │◄── fmt ───┤
│ repl → JIT/run     │           │
└────────┬───────────┘           │
         │                       │
         ▼                       ▼
    shared pipeline (source … semantics … [hir/mir/codegen/runtime])
         ▲
         │
    e26 ─┴─ black-box over candidate binary (usually xo)
```

---

## 4. `xo` command surface

| Command | Pipeline depth | Notes |
|---------|----------------|-------|
| `xo lex` | lexer | `--kinds` / `--diag-codes` for e26 |
| `xo ast` | parser | `--kinds` / `--diag-codes` for e26 |
| `xo check` | resolve + semantics | `--diag-codes`, `--graph`, `--no-cache`, `--cache-status` |
| `xo ir` | codegen IR dump | LLVM IR; `-O0`…`-O3`/`-Oz` (default O0); `--no-cache` / `--cache-status` |
| `xo run` / `run --jit` | execute | AOT/JIT; same `-O` as ir/build; IR cache v3; `--no-cache` / `--cache-status` |
| `xo build -o` | AOT native binary | same opt + IR + **AOT binary** cache as `xo run`; `--static` / `--image oci:<dir>` (ADR 0018) |
| `xo image push` | packaging | upload a layout from `xo build --image` (`echo_oci`) |
| `xo test` | language tests / benches | **v0** — Model A (`std/test` + `XO_TEST`); `--bench` → `XO_BENCH` only |
| `xo fmt` | formatter | **done** — shared parse + AST pretty-print (`-w` write in place) |
| `xo lsp` | language server | `echo_lsp` |
| `xo repl` | interactive | **v0** — rustyline + session + JIT |
| `xo index scan` | index | planned |
| `xo cache status/doctor/clean` | incremental v0 | `.xo/` layout; gc later |
| `xo tools …` | grammar generators, etc. | planned |

---

## 5. Conformance: Echo 2026 (`echo26` + `e26`)

Executable contract of the **Echo 2026** edition (ADR 0015). Public Spec lives
on the site (`/e26`); this suite is the machine-checked proof.

```text
e26 --binary <candidate>

  $bin lex  --kinds --diag-codes file.echo  →  .lex  / .diag
  $bin ast  --kinds --diag-codes file.echo  →  .ast
  $bin check --diag-codes file.echo         →  .check   (sem-* / res-*)

  (later) run / fmt / …
```

- Only numbered roots `NNN_*.echo` are suite entries; unnumbered files are
  multi-file support modules.
- Every language change updates **echo26** and keeps **`scripts/gate echo26`**
  green (`AGENTS.md`).

Details: [`fixtures.md`](fixtures.md).

---

## 6. Product build-out order

| # | Slice | Done means |
|---|--------|------------|
| 0 | Design / docs | syntax, semantics, modules, ADRs |
| 1 | Frontend | lex, parse, `xo lex` / `ast`, e26 lex+ast |
| 2 | Check + multi-file | resolve, scopes, Result/Option, infer v1, `xo check` |
| 3 | Richer surface | width tags, bytes, duration, `p` lits + infer |
| 4 | HIR | lowering from analyzed AST |
| 5 | Execute (thin) | MIR + runtime and/or early JIT → `xo run` |
| 6 | Execute (full) | LLVM AOT/JIT, ABI, `xo build` |
| 7 | Stdlib depth | real IO/net/time; maps/sets as std |
| 8 | **Fmt** | `xo fmt` v0: leaders, blocks, match, tasks; idempotent on success |
| 9 | **LSP** | diags, tokens, goto, complete, format |
| 10 | **REPL** | `xo repl` via same pipeline + JIT/runtime |
| 11 | Cache / build | **v0 APIs + `xo cache`**; wire reuse next ([`incremental.md`](incremental.md)) |
| 12 | **www** | public book, std API, search |
| 13 | Editor ecosystem | tree-sitter, highlighting packs |

Rough progress: slices **0–11** have landed (frontend through REPL + cache v4).
Remaining language work is honesty slices on already-locked surface (see
[`roadmap.md`](roadmap.md) §5), not a restart of 3–13.

---

## 7. Host requirements (fmt / LSP / REPL)

### Formatter (`xo fmt`)

- Parse with **shared** `echo_parser` / `echo_ast`.
- Pretty-print canonical form from `syntax.md` (leaders, braces, no trailing commas).
- **Idempotent:** `fmt(fmt(x)) == fmt(x)`.
- Must **not** change program meaning.
- Optional: range format; still shared pipeline.

### Language server (`echo_lsp`)

| Capability | Depends on |
|------------|------------|
| Diagnostics | parse + semantics (+ resolve) |
| Semantic tokens / highlight | lexer + AST + syntax |
| Hover | semantics (kinds/summaries) |
| Go to definition | resolver + index |
| Find references | index + resolver |
| Completion | partial parse + index |
| Signature help | semantics / reflection |
| Rename | resolver + index (no-shadowing) |
| Document / workspace symbols | AST + index |
| Formatting | same as `xo fmt` |
| Inlay hints | semantics (later) |

**Incremental:** re-lex/re-parse dirty files; reuse index via fingerprints when ready.

### REPL (`xo repl`)

- Parse/check each input (or buffer) through the **same** pipeline.
- Evaluate with **JIT** (`run_jit_ir`) — not a private interpreter with different rules.
- Diagnostics match `xo check` / LSP.
- Details: [`repl.md`](repl.md).

---

## 8. Crate ownership (quick table)

| Crate | Owns |
|-------|------|
| `echo_source` | Source identity, spans |
| `echo_diagnostics` | Diagnostic model |
| `echo_syntax` | Leader / grammar facts |
| `echo_lexer` | Tokens |
| `echo_ast` | Tree shape |
| `echo_parser` | Parse |
| `echo_index` | Project facts |
| `echo_resolver` | Graph, imports, merge |
| `echo_semantics` | Scopes, effects, inference |
| `echo_hir` | High-level analyzed IR |
| `echo_mir` | Mid-level executable IR |
| `echo_codegen` / `_abi` | LLVM + ABI |
| `echo_runtime` / `echo_std` | Execution + std |
| `echo_reflection` | Callable metadata |
| `echo_fingerprint` / `cache` / `build` | Incremental |
| `echo_lsp` | LSP presentation |
| `xo` | CLI |
| `e26` | Suite runner |

Full ownership narrative: [`architecture.md`](architecture.md).
