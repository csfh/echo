# LLVM and native link

Codegen to LLVM IR, optimization levels, and native linking.

| | |
|--|--|
| **Status** | Active (inkwell / LLVM 22; AOT via clang) |
| **Owners** | `echo_codegen` (`OptLevel`), build/run path in `xo` / `echo_pipeline` |
| **Related** | `docs/adr/0002-llvm-only-execution-backend.md`, `docs/runtime-abi.md`, [`mir.md`](mir.md), [`incremental.md`](incremental.md) |

## Division of labor (authority)

| Layer | Job |
|-------|-----|
| **MIR** | CFG, SSA, representation analysis, local simplify, escape analysis / `NoEscape` box elision — language + runtime ABI form (see [`mir.md`](mir.md)). |
| **Codegen** | Faithful SSA → LLVM; native scalars; thin `echo_runtime_*` only where required. Shared `OptLevel` type. |
| **LLVM mid-end** | `default<On>` for `n ∈ {1,2,3,z}`: constprop, GVN, LICM, loop opts, DCE, instcombine, size/speed pipelines. **Same policy for JIT and AOT.** |
| **clang / MCJIT** | Object emission and execution — **not** a second mid-end when IR is already optimized. |

Verify after emit and after opt. Do not weaken MIR to invent residual work for tests.

## Optimization levels

| Flag | Token | Pipeline | Notes |
|------|-------|----------|-------|
| `-O0` / `--opt-level 0` | `O0` | **none** — skip `run_passes` | **Default** for `xo ir`, `xo run`, `xo build` |
| `-O1` | `O1` | `default<O1>` | |
| `-O2` | `O2` | `default<O2>` | |
| `-O3` | `O3` | `default<O3>` | |
| `-Oz` | `Oz` | `default<Oz>` | Size-oriented; **not** an alias of `O2`/`O3` |

Shared type: `echo_codegen::OptLevel` (re-exported from `echo_pipeline`). Hosts
must not invent a second opt enum.

### End-to-end flow

```text
xo CLI (-O / --opt-level)
  → echo_pipeline::compile_to_llvm_with(..., opt)
  → MIR handoff (repr + simplify + escape; independent of opt)
  → emit_llvm_with(..., opt)  // verify → run_passes when opt ≠ O0 → verify
  → IR text
       ├─ xo ir     (print)
       ├─ xo run    AOT: link_aot (clang -O0) or binary cache
       ├─ xo run --jit  MCJIT on same IR (engine OptimizationLevel::None)
       └─ xo build  AOT to -o path
```

AOT and JIT use the **same** in-process LLVM pass policy for a given level.
Backend mechanics differ (clang link vs MCJIT map), not requested optimization
intent.

Optimization failures surface as structured diagnostics (`llvm-opt`,
`llvm-verify-emit`, `llvm-verify-opt`) — no panics on user-controlled levels.

## Scope

LLVM IR emission, optimization modes (`-O`), JIT vs AOT, and host tools (`opt`,
`clang`, `llvm-config`) used as plumbing.

## Facts

- Backend library: **inkwell** with **`llvm22-1-prefer-dynamic`** (LLVM **22**)
  and host targets **`target-x86`** + **`target-aarch64`**. On Windows MSVC,
  llvm-sys still **statically** links official LLVM (no `--link-shared`);
  `xo.exe` then needs `/FORCE:MULTIPLE` + `/NODEFAULTLIB:libcmt` because
  LLVMSupport vendors rpmalloc as `malloc`/`free`/`realloc` (see
  [`ci.md`](ci.md)).
- AOT path: emit LLVM IR text (`.ll`) → host **clang** links with
  `libecho_runtime.a` (+ pthread, dl, m). Mid-end opts run **in-process** via
  inkwell; clang is invoked at `-O0` so it does not re-run the mid-end.
- Default `xo run` is AOT (temp dir → link → exec → cleanup).
- `xo run --jit` is **in-process MCJIT**: same optimized IR text, maps
  `echo_runtime_*` to the linked `echo_runtime` crate (ADR 0004), runs
  `echo_entry`, no clang step. MCJIT uses `OptimizationLevel::None` after IR opt.
- `xo ir` prints the LLVM IR for an entry after check → HIR → MIR → codegen (± opt).
- `xo build -o <path>` produces a native binary the same way.
- `xo build --static` links `-static` and refuses a `PT_INTERP` ELF (ADR 0018).
- `xo build --image oci:<dir>` packs that ELF as a scratch OCI layout (`echo_oci`).
- `xo image push` uploads a layout to an OCI registry (no Docker).
- No second non-LLVM language engine (ADR 0002).

## Host requirements

- `clang` on `PATH` (or `ECHO_CLANG`)
- `libecho_runtime.a` (or a hashed `libecho_runtime-*.a`) in the same cargo
  profile as `xo` (`target/debug/` + `deps/`), or next to an installed `xo`.
  The linker takes the **newest** matching archive so a leftover unhashed
  `.a` cannot hide a current `deps/` build. `ECHO_RUNTIME_LIB` is an explicit
  path. A missing archive, or clang `undefined reference` to `echo_runtime_*`,
  tells you to `cargo build -p echo_runtime`.
- `xo build --static` / `--image`: `musl-clang` (or `ECHO_MUSL_CLANG`) and a musl
  `libecho_runtime.a` (`cargo build -p echo_runtime --target
  x86_64-unknown-linux-musl`, or `ECHO_RUNTIME_LIB`). The link is rejected if
  the ELF has `PT_INTERP`.
- For building `xo` itself: system LLVM 22 headers/libs (`LLVM_SYS_221_PREFIX`
  if needed; Arch typically `/usr`)
- CI installs LLVM 22 from official `llvm/llvm-project` release tarballs
  (`scripts/ci/llvm.sh`, SHA256-pinned) and sets `LLVM_SYS_221_PREFIX`
  (see [`docs/ci.md`](ci.md))

## Program entry (codegen v1)

1. The **entry file’s top-level statements** (binds, calls, control) are the
   program. They lower to a synthetic **`__toplevel`** function — **not** a
   user-facing name and **not** a keyword.
2. Free functions are ordinary values (`$ f = (a) { … }`). The identifier
   **`main` is not special** and is never auto-called.
3. **`echo_entry`** calls `__toplevel` when present (else returns 0). Return
   status (i64) is truncated to i32 process status.
4. C **`@main`** is only the process wrapper that calls **`echo_entry`**.

## Kinds in IR (v1)

Internal shapes only — **no keywords**, no user type names. Width tags
(`<i32>` / `<ui8>` / `<f32>` / …) and explicit `<width> expr` casts are the
only kind-related surface. Native IR uses the matching LLVM integer/float
type; box/unbox widens to the universal `i64` / heap-float ABI.

| Shape (from syntax) | Wire |
|---------------------|------|
| plain | `i64` |
| result (`!` in body) | `i128` tag\|payload; consume with `\|` `$`/`!` arms |
| option (bare `^` + valued `^`) | same wire; consume with `\|` `$`/`:` arms |

`! expr` is **error return** (result err path), never process panic.

## Loops (codegen v1)

| Surface | Lowering |
|---------|----------|
| `* { … }` | infinite CFG; `<` break · `>` continue |
| `* cond { … }` | while; cond truthy continues |
| `* item : iter { … }` | for-in over a **list** or inclusive **range** (`lo..hi`) |
| `[a, b, …]` | `echo_runtime_list_*` handle stored as i64 |
| `xs[i]` | `echo_runtime_list_get` |
| `runtime.*` (std only, via `/ runtime`) | `echo_runtime_*` per ABI map — see `stdlib.md` |
| User multi-file (`/ ./lib`) | All modules lowered; `module.fn` / `module.val` (value → `__val_*` getter) |

## Cache participation

| Artifact | Opt participation |
|----------|-------------------|
| LLVM IR (`.xo/cache/codegen`) | Explicit `opt` token (`O0`…`Oz`) in `PhaseCacheKey` extras + graph/check/lower fingerprints |
| AOT binary | Keyed by **post-opt** IR bytes + runtime ABI + lower fingerprint (opt cannot collide when IR differs) |

See [`incremental.md`](incremental.md).

## Debug info

| | |
|--|--|
| **Status** | Line tables + checker-kind locals (no inlined frames; no Echo DWARF language id) |
| **Producer** | `xo` |
| **DWARF language** | `DW_LANG_C` (no Echo language id); producer identifies the toolchain |

**Emitted**

- One compile unit from the entry file (`DICompileUnit` + `DIFile`).
- One `DISubprogram` per Echo function (`__toplevel`, user fns) plus `echo_entry` and C `main`.
- Per-instruction `DILocation` from MIR source spans (`MirOp::Set` and
  `Terminator::ReturnOk`) via a `LineMap` on the function’s module file. Ops
  without a span keep the last location (or the function’s line 1 / column 1).
- Parameters and locals: `DILocalVariable` + `DIBasicType` named with the
  checker’s kind label (`ValueKind::as_di_label`: `i64`, `string`, `bool`, …).
  Those names are diagnostic labels, not a user type language. `llvm.dbg.declare`
  is omitted: inkwell 0.9 on LLVM 22 treats the declare record as an instruction
  and aborts emit.

AOT: `link_aot` is **one** clang invocation at **`-O0 -g`** so clang keeps
DWARF and does not re-run the mid-end. JIT uses the same IR (host debugger
support for MCJIT varies).

## Open questions

- Inlined-frame DI
- Echo DWARF language id
- Debugger GUI work
- Further IR quality gates beyond verify + metrics smoke
