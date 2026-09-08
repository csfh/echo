# Incremental build (fingerprint · cache · plan)

| | |
|--|--|
| **Status** | **v4** — parse + check + IR + **AOT binary** caches; **LSP document model** |
| **Owners** | `echo_fingerprint`, `echo_cache`, `echo_build`, thin `xo cache` |
| **Related** | [`pipeline.md`](pipeline.md) §2, [`architecture.md`](architecture.md), ADR 0001 |

## Policy (locked)

1. **Orthogonal to language meaning.** Cache never redefines syntax, scopes, or
   runtime behavior. A hit must be bit-identical to a cold recompute of that phase.
2. **Phase-keyed.** Artifacts are addressed by **source inputs** + **compiler
   component versions** for that phase (see `phase_fingerprint`).
3. **Bump versions deliberately.** When a crate changes the **shape or meaning**
   of a cacheable output, bump the matching `*_VERSION` in `echo_fingerprint`.
4. **No PHP / Composer.** Project metadata is Echo-only (`ProjectMetadata`);
   package roots follow [`modules.md`](modules.md) / [`stdlib.md`](stdlib.md).
5. **Hosts stay thin.** `xo` and LSP call these crates; they do not reimplement
   invalidation rules.

## Layout (on disk)

Project cache root: **`{project}/.xo/`**

```text
.xo/
  cache/
    lex/<compiler-stamp>/ …
    parse/<compiler-stamp>/ …
    index/<compiler-stamp>/ …
    resolve/<compiler-stamp>/ …
    check/<compiler-stamp>/ …
    lower/<compiler-stamp>/ …
    codegen/<compiler-stamp>/ …
    diagnostics/<compiler-stamp>/ …
  index/          # reserved for project fact files
  tmp/            # atomic renames
```

`<compiler-stamp>` is `phase_fingerprint(phase, &[])` (format + phase +
component versions). Source extras stay in the blob file name. `xo cache gc`
deletes other stamps, leftover files in a phase root (pre-v2 flat layout), and
`.xo/tmp` leftovers. `xo cache clean` still removes the whole `.xo` tree.

CLI:

```bash
xo cache status [--path DIR]
xo cache doctor [--path DIR]
xo cache clean  [--path DIR]
xo cache gc     [--path DIR]   # DIR is the project root; a file walks to Cargo.toml / .git
```

## Phases

Aligned with the standalone pipeline (not the old PHP toolchain):

| Phase | Rough producer | Typical consumer |
|-------|----------------|------------------|
| `lex` | `echo_lexer` | parse |
| `parse` | `echo_parser` / `echo_ast` | index, check |
| `index` | `echo_index` | resolve |
| `resolve` | `echo_resolver` | check, multi-file lower |
| `check` | `echo_semantics` | hosts, lower |
| `lower` | `echo_hir` + `echo_mir` | codegen |
| `codegen` | `echo_codegen` | run / build |
| `diagnostics` | shared codes | LSP / CLI |

`BuildMode::Check` stops after `check`; `Execute` includes `lower` + `codegen`.

## Components & invalidation

Each phase fingerprint hashes `CACHE_FORMAT_VERSION`, the phase name, and the
**versions** of its `CompilerComponent`s (lexer, parser, schemas, …).

**Important:** `phase_components(phase)` lists the **full upstream stack** that
can change that phase’s output without changing source bytes — not only the
crate that “owns” the phase. Example: `codegen` fingerprints include
`mir_lowerer` / `hir_lowerer` / `semantics` / `parser`, so a MIR/SSA fix
invalidates IR cache keys when `MIR_LOWERER_VERSION` (etc.) is bumped, even if
a caller forgets extra `lower_fp` fields.

| Mode | Behavior |
|------|----------|
| `CacheMode::Safe` | Any component change → all phases dirty |
| `CacheMode::Phase` | Only phases that list the component, plus **downstream** cascade |

Downstream cascade: e.g. dirty `parse` invalidates index → resolve → check →
lower → codegen → diagnostics.

Bump the matching `*_VERSION` in `echo_fingerprint` whenever you change
cacheable compiler meaning (see crate header comments).

## Crate API (v0)

### `echo_fingerprint`

- `ArtifactPhase`, `CompilerComponent`, `CacheMode`
- `Fingerprint`, `phase_fingerprint`, `phase_components`
- `invalidated_phases`, `phase_and_downstream`

### `echo_cache`

- `CacheLayout::for_project` → `.xo`
- `PhaseCacheKey::for_source(phase, bytes, extra)`
- `ArtifactStore::put` / `get` / `contains` / `phase_counts` / `gc`

### `echo_build`

- `BuildMode`, `BuildJob`, `BuildPlan`
- `plan_all_phases`, `filter_plan`, `cascade_from`, `phases_to_invalidate`
- `project_root_for` (Cargo.toml / `.git` walk)

## Milestone roadmap (infra)

| Step | Goal |
|------|------|
| **v0** | Types, versions, store, plan, `xo cache status/clean/doctor` |
| **v1** | `xo check` caches **semantic** diagnostics; `--no-cache` / `--cache-status` |
| **v2** | Per-file **parse AST** cache (bincode) during resolve; index facts re-extracted |
| **v3** | **LLVM IR** cache for `xo run` / `ir` / `build`; skips HIR/MIR/codegen on hit |
| **v4 (this)** | **AOT binary** cache; **LSP** document store + diagnostics over shared check; **`xo cache gc`** |

### Parse cache (v2)

- **`parse_with_cache`** (`echo_parser`): stores `Option<File>` + diagnostics under
  `.xo/cache/parse/`.
- Key: file bytes + path + **Index** phase fingerprint (extract/schema changes
  invalidate) + Parse component versions.
- On hit: deserialize AST, **`remap_source_ids`** to the current `SourceMap` id;
  lexer tokens are **not** stored (empty `Lexed` — use plain `parse` for lex/ast
  tooling that needs tokens).
- **`resolve_entry_with_cache`** uses it for every on-disk module; stats:
  hits/misses/bypasses.
- Index (`extract`) still runs after parse (cheap; no separate blob yet).

### `xo check` cache (v1+v2)

- Resolve **always** runs (closed graph); **parse** may hit per file.
- **Caches** semantic diagnostics under `.xo/cache/check/` (graph content key).
- Flags: `--no-cache`, `--cache-status` (prints check + parse stats).

### Codegen IR cache (v3)

- After a successful check, `compile_to_llvm_with` looks up `.xo/cache/codegen/`
  with graph content + **check_fp** + **lower_fp** + **codegen_fp** +
  **runtime_abi** + **`opt`** (`O0`…`Oz` via `OptLevel::as_str`).
  `PhaseCacheKey::for_source(Codegen, …)` also hashes the **full Codegen phase
  component stack** (frontend → HIR/MIR → codegen → runtime ABI), so version
  bumps alone invalidate hits.
- **Hit:** reuse LLVM IR text; skip HIR lower, MIR lower, and `emit_llvm`.
- **Miss:** lower + emit at the selected opt level, then store via
  `encode_ir_artifact` (`ECHOIR01` header).
- Used by **`xo run`**, **`xo ir`**, **`xo build`** (same `-O` / `--opt-level`
  semantics; flags: `--no-cache`, `--cache-status` prints check/parse/**codegen**).
- Distinct opt levels never share an IR cache entry (including `O2` vs `Oz`).
- Project cache root is `{project_root}/.xo` where `project_root` is found by
  walking up from the entry for `Cargo.toml` / `.git` (else cwd). Files under
  `/tmp` may use a **different** `.xo` than the workspace — `xo cache clean`
  only cleans the root you pass / the entry’s project.

### AOT binary cache (v4)

- After IR is available, `xo run` (AOT path) keys a native binary with
  `aot_binary_cache_key_with_opt(ir, opt)`: post-opt IR bytes + **opt** token +
  runtime ABI + lower/codegen fingerprints under `.xo/cache/codegen/`
  (distinct blob name from IR text). Default run/build is **dynamic** link mode.
- Opt participates via IR content **and** the explicit token (if two levels
  emit identical text they still do not collide).
- **`xo build --static`** / **`--image`** keys with
  `aot_binary_cache_key_with(ir, opt, LinkMode::Static)` so a static ELF never
  reuses a dynamic blob (ADR 0018).
- **Hit:** write cached bytes to a temp path and exec (skip clang).
- **Miss:** `link_aot` (clang at `-O0` after in-process mid-end) then store.
- `--cache-status` prints `aot cache: hit|miss|bypass`.
- **`xo build -o`** uses the **same** AOT binary cache key and store as `xo run`
  (hit → write cached bytes to `-o`; miss → link, store, write `-o`), plus the
  link-mode token when `--static` / `--image` is set.

### LSP document model (v4)

- Crate `echo_lsp`: open-document store, UTF-16 positions, `analyze_path` via
  `check_entry_with_overlays` + the same `.xo` cache.
- Dirty buffers are **overlays** (do not invent a second typechecker).
- `xo lsp`: minimal stdio JSON-RPC — `initialize`, document sync,
  `publishDiagnostics`.

## Proof

```bash
cargo test -p echo_fingerprint -p echo_cache -p echo_build -p echo_codegen -p echo_lsp
cargo build -p xo
./target/debug/xo cache gc
./target/debug/xo cache clean
./target/debug/xo run examples/misc/hello.echo --cache-status
./target/debug/xo run examples/misc/hello.echo --cache-status   # ir + aot hits
```

## Historical note

The prior `echo-php-old` tree had large fingerprint/cache/build crates including
Composer and PHP surface. This design **reuses the phase + version + store
pattern only**, rewritten for keyword-free Echo and the current crate graph.
Do not copy PHP-era code into this repository.
