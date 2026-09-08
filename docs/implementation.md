# Implementation checklist (full toolchain)

This document is the **vertical-slice map** for Echo. When you add or change a
language feature, use it as a checklist of **every place that may need work**
across the compiler, runtime, CLI, editor, formatter, REPL, tests, std, and docs.

Related:

- **Full pipeline map (spine + hosts + order):** [`pipeline.md`](pipeline.md)
- Crate ownership: [`architecture.md`](architecture.md)
- Language surface: [`syntax.md`](syntax.md), [`lexer.md`](lexer.md), [`semantics.md`](semantics.md)
- Edition / public Spec: [ADR 0015](adr/0015-echo-2026-canonical-edition.md) (**Echo 2026**)
- Modules: [`modules.md`](modules.md)
- Design tracker: [`roadmap.md`](roadmap.md)
- Fixtures: [`fixtures.md`](fixtures.md)
- Agent workflow: [`../AGENTS.md`](../AGENTS.md)

**Rule:** Behavior is defined once in the **shared pipeline**. Hosts (`xo`,
LSP, formatter, REPL, website, `e26`) **present** that behavior; they do not
reimplement it.

**Proof rule (mandatory):** every change updates **crate tests** (each touched
crate), **Echo 2026 suite (`echo26` / `e26`)**, and **examples** as applicable
— same change. See [`AGENTS.md`](../AGENTS.md) and [`fixtures.md`](fixtures.md).

---

## 1. Pipeline order (implementation spine)

Implement and extend crates in this order for a full feature. A partial vertical
may stop early (e.g. parse-only) but must not invent meaning in a later host.

```text
echo_source
    → echo_diagnostics          (shared; used from lexer onward)
    → echo_syntax               (leaders, keyword-free grammar facts)
    → echo_lexer
    → echo_ast + echo_parser    (chumsky; source-shaped AST)
    → echo_index                (project facts; multi-file)
    → echo_resolver             (imports, graph, % / @ merge, export shapes)
    → echo_semantics            (scopes, Result/Option, kind inference)
    → echo_hir
    → echo_mir
    → echo_codegen_abi + echo_codegen
    → echo_runtime              (+ echo_std / echo_reflection as needed)
```

**Short form:**

```text
Source → Lex → Parse/AST → Index/Resolve → Semantics
       → HIR → MIR → LLVM → AOT | JIT → Runtime
```

See the full diagram: [`pipeline.md`](pipeline.md).

**Hosts (always last, thin):**

```text
xo            CLI orchestration
e26           black-box suite runner (candidate binary)
formatter     xo fmt (planned)
echo_lsp      editor protocol (xo lsp)
REPL          xo repl (planned)
www           user docs / search index
```

**Incremental build (orthogonal, wrap the spine):**

```text
echo_fingerprint → echo_cache → echo_build
```

---

## 2. Per-feature checklist

Copy this section into a PR or issue when implementing a feature. Strike what
does not apply; do not skip applicable rows silently.

### 2.1 Language specification

| Item | Where | Done? |
|------|--------|-------|
| Syntax / leaders / grammar | `docs/syntax.md` | |
| Lexical tokens / literals | `docs/lexer.md` | |
| Semantics / kinds / Result-Option | `docs/semantics.md` | |
| Modules / imports | `docs/modules.md` | |
| Sticky design decision | `docs/adr/` if hard to reverse | |
| Runtime / ABI notes | `docs/runtime-abi.md` | |
| Std API if public | `docs/stdlib.md` + `std/**/*.echo` | |
| Glossary terms | `docs/glossary.md` | |
| Pipeline / host impact | `docs/pipeline.md` | |
| User-facing docs | `www/` when public | |

### 2.2 Compiler frontend

| Item | Crate | Notes |
|------|--------|-------|
| Source registration / spans | `echo_source` | Paths, `SourceId`, byte spans |
| Token kinds / scanning | `echo_lexer` | Leaders, dual-use glyphs by position |
| Syntax metadata | `echo_syntax` | Leader set, optional tree-sitter facts |
| AST nodes | `echo_ast` | Source-shaped only (ADR 0003) |
| Parse rules / recovery | `echo_parser` | Same-line `{`, leader whitespace, match arms |
| Diagnostic emit | `echo_diagnostics` | Stable codes when user-visible |

### 2.3 Project model (multi-file features)

| Item | Crate | Notes |
|------|--------|-------|
| Fact extraction | `echo_index` | `%` / `@` / exports / signatures |
| Import graph | `echo_resolver` | `/ path`, closed graph (ADR 0006) |
| Module-scoped imports | `echo_resolver` + semantics | last path segment → `module.export` |
| Struct merge | `echo_resolver` + semantics | One `%`, many `@`, no duplicate members |
| Package roots | `echo_resolver` | `std/…`, `./…` resolution |
| Export return shapes | `echo_resolver` | for Result/Option unhandled across files |

### 2.4 Semantics and IR

| Item | Crate | Notes |
|------|--------|-------|
| Name intro / no shadowing | `echo_semantics` | `~` update vs re-intro error |
| `$` / `#` init-once | `echo_semantics` | |
| Result / Option produce | `echo_semantics` | `!` err return; bare `^` / `^ v` option shape |
| Result / Option consume | `echo_semantics` | `\|` arms `$`/`!`/`:` |
| Kind inference | `echo_semantics` | unify + infer v1 |
| `#` const-eval | `echo_semantics` | Literals + ops on `#` only; duration/bytes/locator fold |
| Receiver `.` | `echo_semantics` | Only on method-call activation |
| Call resolution | `echo_semantics` / resolver | Free fn vs `value.member()` vs `module.f` |
| HIR lowering | `echo_hir` | Desugar surface → analyzed form |
| MIR lowering | `echo_mir` | Control flow, calls, error-return, loops |
| Reflection metadata | `echo_reflection` | If feature is reflectable |

### 2.5 Codegen and runtime

| Item | Crate | Notes |
|------|--------|-------|
| ABI / symbols | `echo_codegen_abi` | `echo_runtime_*` name authority |
| LLVM emission | `echo_codegen` | AOT + JIT; lower `runtime.*` → native |
| Runtime behavior | `echo_runtime` | Values, IO, lists, … |
| Std sources | `std/**/*.echo` | Public API; may `/ runtime` only here |
| Runtime package | resolver + `echo_std` | Virtual exports; std-only import |
| Link / JIT | `xo` build/run | clang + `libecho_runtime.a`; same symbols JIT |

### 2.6 Incremental build

| Item | Crate | Notes |
|------|--------|-------|
| Phase fingerprint | `echo_fingerprint` | Phases, component versions, cascade ([`incremental.md`](incremental.md)) |
| Artifact cache | `echo_cache` | `.xo/` + `ArtifactStore` put/get |
| Schedule / plan | `echo_build` | `BuildMode`, `plan_all_phases`, cascade helpers |
| CLI cache UX | `xo cache` + `xo check --cache-status` | v1 semantic check cache; parse/codegen later |

### 2.7 CLI (`xo`)

| Surface | Command / flag | Notes |
|---------|----------------|-------|
| Lex | `xo lex` | `--kinds` / `--diag-codes` for e26 |
| AST | `xo ast` | `--kinds` / `--diag-codes` for e26 |
| Check | `xo check` | `--diag-codes` / `--graph` |
| IR | `xo ir` | LLVM IR dump |
| Run | `xo run` / `xo run --jit` | AOT temp vs JIT |
| Build | `xo build -o` | Native binary; `--static` / `--image oci:<dir>` (ADR 0018) |
| Image | `xo image push` | Registry upload of a scratch layout (`echo_oci`) |
| Test | `xo test` / `xo test --bench` | Language tests + benches (`std/test`) |
| Index | `xo index scan` | Project facts |
| Cache | `xo cache …` | Incremental |
| Tools | `xo tools …` | grammar, etc. |
| Fmt | `xo fmt` | Shared AST; no ad hoc parse |
| LSP | `xo lsp` | `echo_lsp` |
| REPL | `xo repl` | Same pipeline + JIT/runtime |

Every command that understands source must go through the **same pipeline** for
that stage (e.g. `ast` stops after parse; `check` through semantics).

### 2.8 Formatter (`xo fmt`)

| Item | Notes |
|------|--------|
| Parse to AST | Same parser as compiler |
| Pretty-print | Canonical forms from `syntax.md` (leaders, braces, commas) |
| Idempotence | `fmt(fmt(x)) == fmt(x)` |
| Range format | Optional later; still shared pipeline |
| Config | Minimal; avoid style wars (language is already rigid) |
| Tests | Fixture pairs: ugly → pretty (echo26 or fmt fixtures) |
| LSP | `textDocument/formatting` → same engine as `xo fmt` |

Formatter **must not** change program meaning. No “fmt understands more than
parser.”

### 2.9 Language server (`echo_lsp`)

LSP is presentation over the shared pipeline + index. For each feature, consider:

| LSP capability | Depends on | Feature work |
|----------------|------------|--------------|
| Diagnostics | parse + semantics (+ resolve) | Publish codes/spans from shared model |
| Semantic tokens / highlight | lexer + AST (+ syntax) | Token modifiers for leaders, members |
| Hover | semantics + docs | Kind/value summary; not a second typechecker |
| Go to definition | resolver + index | `%` / `@` / binds / imports / module.export |
| Find references | index + resolver | |
| Completion | partial parse + index | Leaders, members, imports, locals |
| Signature help | semantics / reflection | Callables |
| Rename | resolver + index | Respect no-shadowing / export |
| Document symbols | AST + index | Structs, members, top-level binds |
| Workspace symbols | index | |
| Code actions / quick fixes | diagnostics | Optional |
| Formatting | fmt service | Same as `xo fmt` |
| Inlay hints | semantics | Optional later |

**Incremental editing:** re-lex/re-parse dirty files; reuse index/resolver facts
where fingerprints allow (`echo_cache` / LSP session state).

Details: [`lsp.md`](lsp.md), [`pipeline.md`](pipeline.md) §7.

### 2.10 REPL (`xo repl`)

| Item | Notes |
|------|--------|
| Parse / check | Shared pipeline (same as `xo check` for the buffer) |
| Evaluate | JIT and/or runtime — **not** a private interpreter |
| Diagnostics | Same codes/spans as CLI/LSP |
| Multi-line | Buffer until construct complete (leaders / braces) |
| Imports | Respect closed graph / module-scoped imports |
| Tests | REPL scenarios optional; language meaning covered by e26 |

### 2.11 Testing and gates

| Item | Where | Notes |
|------|--------|-------|
| Unit tests | owning crate | Prefer pure layer tests |
| **echo26 fixtures** | `echo26/` | **Required** for language behavior |
| **e26 runner** | `crates/e26` | Candidate binary; extend protocol when stages grow |
| CLI integration | `xo` | flags used by e26 |
| `scripts/gate echo26` | must pass | After language changes |
| `scripts/gate` routes | `echo26/*`, `crates/e26/*`, `std/**/*.echo`, `examples/**` | Keep mapping current |
| `scripts/gate std-test` | `xo test std` | Dirty `std/**/*.echo` |
| `scripts/gate examples` | finite `xo check` / `xo run` | Dirty `examples/**`; skip `app/server.echo` run |
| `just e26` | optional wrapper | |
| Snapshot / golden | `.lex` / `.ast` / `.diag` / `.check` | Via e26 |

See [`fixtures.md`](fixtures.md) and [`testing.md`](testing.md).

**Rule:** Implementing or changing a language feature without an `echo26` update
(and green `e26` against reference `xo`) is incomplete work.

### 2.12 Standard library and samples

| Item | Where |
|------|--------|
| Echo sources | `std/**/*.echo` |
| App / algo samples | `examples/app/**`, `examples/algos/**` |
| Package entry re-exports | e.g. `std/net/http.echo` |
| Runtime bridge | `echo_runtime` / `echo_std` |
| Module-scoped API style | `io.log`, `http.serve` (not bare flood) |

### 2.13 Website (`www/`)

| Item | Notes |
|------|--------|
| Language pages | Leaders, examples, Result/Option, modules |
| Search index | Content records for new pages |
| Snippets | Match **current** `syntax.md` |
| Discovery | `/sitemap.xml` of the public catalog; `/robots.txt` on `https://xo.run` |

### 2.14 Tooling ecosystem (as needed)

| Item | Notes |
|------|--------|
| Tree-sitter / editor grammar | `xo tools grammar tree-sitter` from `echo_syntax` ([`tree-sitter.md`](tree-sitter.md)) |
| Syntax highlighting (non-LSP) | Same token categories as LSP |
| Debug info | Line tables landed (CU + subprogram + loc); types/locals later |
| Package manager | Out of core until modules stabilize |

---

## 3. Feature matrix template

Use when scoping a vertical. Mark each cell: **N/A** · **todo** · **done**.

| Layer | Required? | Status | PR / notes |
|-------|-----------|--------|------------|
| Spec (`syntax` / layer doc) | | | |
| `echo_syntax` | | | |
| `echo_lexer` | | | |
| `echo_ast` | | | |
| `echo_parser` | | | |
| `echo_diagnostics` (codes) | | | |
| `echo_index` | | | |
| `echo_resolver` | | | |
| `echo_semantics` | | | |
| `echo_hir` | | | |
| `echo_mir` | | | |
| `echo_codegen_abi` | | | |
| `echo_codegen` | | | |
| `echo_runtime` | | | |
| `echo_std` / `std/*.echo` | | | |
| `echo_fingerprint` / cache | | | |
| `xo` command surface | | | |
| Formatter (`xo fmt`) | | | |
| `echo_lsp` | | | |
| REPL (`xo repl`) | | | |
| Tests / fixtures / e26 / gate | | | |
| `www` | | | |

---

## 4. Minimal verticals (what “done enough” means)

Not every feature needs every row on day one. Typical cut lines:

| Vertical goal | Stop after | Still required |
|---------------|------------|----------------|
| Design only | Spec docs | — |
| Parse | parser + `xo ast` + **echo26 `.ast`** + e26 | diagnostics, syntax, lexer, ast |
| Check | semantics + `xo check` + **echo26** | + index/resolver if multi-file |
| Run subset | mir + runtime **or** JIT + **echo26** run fixtures | shared meaning through that stage |
| Ship formatter | `xo fmt` + fixtures | shared AST only |
| Ship editor | LSP diags + goto + format | no second parser |
| Ship REPL | eval via JIT/runtime | same semantics as `run` |
| Ship binary | AOT build path | runtime + link + cache as needed |

**Never:** LSP-only, REPL-only, or CLI-only semantics.

---

## 5. Echo-specific hotspots

When implementing **this** language, watch these cross-cutting rules:

| Concern | Touches |
|---------|---------|
| Statement **leaders** + dual-use glyphs | lexer, parser, highlight, fmt |
| Leader whitespace + same-line `{` | lexer/parser, fmt |
| `%` / `@` merge multi-file | resolver, index, semantics, LSP symbols |
| Module-scoped imports `module.export` | resolver, semantics, completion |
| Members as `$`/`~`/`#` (data or fn) | AST, semantics, completion |
| Receiver `.` only on method call | semantics, HIR, LSP hover |
| No shadowing; `~` updates | semantics, rename, diagnostics |
| Result: `!` err return; `\|` `$`/`!` arms | parser, semantics, MIR, runtime |
| Option: bare `^` / `^ v`; `\|` `$`/`: ` arms | parser, semantics, MIR |
| Match value + `% Type` arms | parser, semantics, HIR, MIR, runtime type tags |
| Tagged struct lit `user { }` / `mod.T { }` | parser, semantics, HIR, runtime `struct_new_named` |
| Free fns as values `(…){ }` | AST, semantics, codegen |
| Import `/` vs divide; export `\` | lexer position, resolver |
| Pure vs rich strings / bytes | lexer, fmt, highlights |
| Kind inference / width tags | semantics, later lexer |
| Deep `==` vs identity `===` | semantics, runtime |

---

## 6. Suggested build-out order (toolchain product)

Aligned with [`pipeline.md`](pipeline.md) §6:

| # | Slice | Notes |
|---|--------|--------|
| 0 | Design / docs | syntax, semantics, modules, ADRs |
| 1 | Frontend | lex, parse, `xo lex` / `ast`, e26 |
| 2 | Check + multi-file | resolve, Result/Option, infer v1, `xo check` |
| 3 | Richer surface | width tags, bytes, duration, `p` lits |
| 4 | HIR | analyzed IR |
| 5 | Execute (thin) | MIR + runtime and/or early JIT → `xo run` |
| 6 | Execute (full) | LLVM AOT/JIT, `xo build` |
| 7 | Stdlib depth | IO/net; maps/sets as std later |
| 8 | **Fmt** | `xo fmt` + fixtures |
| 9 | **LSP** | diags, tokens, goto, complete, format |
| 10 | **REPL** | `xo repl` via pipeline + JIT/runtime |
| 11 | Cache / build polish | fingerprints, incremental |
| 12 | **www** | public book + search |
| 13 | Editor ecosystem | tree-sitter, highlighting |

Rough progress: slices **0–11** have landed (frontend, check, execute AOT/JIT,
stdlib core, fmt, LSP, REPL, cache v4). Remaining language work is honesty
slices (width/cast, match/effect PHI, `#` fold, reject-path e26, remaining
heap dispose) — not a green-field walk of 3–13. Host www / editor packs and
AOT link polish continue as product work.

---

## 7. Definition of “feature complete” for the suite

A language feature is **suite-complete** when:

1. Spec is the single source of truth and matches code.  
2. Shared pipeline implements meaning through the deepest stage you claim (parse / check / run).  
3. Diagnostics are structured and tested.  
4. CLI exposes the feature at the right command depth (**e26-compatible flags**).  
5. Formatter preserves meaning and matches canonical style (when fmt exists).  
6. LSP surfaces the feature without a fork of semantics (when LSP exists).  
7. REPL uses the same meaning as `run` (when REPL exists).  
8. **`echo26` has small numbered fixtures**; **`e26 --binary xo` is green**.  
9. `std` / `app` samples updated if user-visible.  
10. `scripts/gate` routes still make sense for touched paths.  

If any of 5–7 are deferred, document that explicitly—not as accidental drift.
**Do not defer item 8** for user-visible language behavior.
