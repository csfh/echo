# MIR

Mid-level, backend-neutral executable intermediate representation.

| | |
|--|--|
| **Status** | **Active** (codegen consumer; expands with execute verticals) |
| **Owners** | `echo_mir` |
| **Related** | `docs/hir.md`, `docs/llvm.md`, `docs/runtime-abi.md`, [`memory.md`](memory.md), ADR 0002, [ADR 0016](adr/0016-scope-owned-memory.md), `docs/incremental.md`, `docs/semantics.md` § Value vs reference |

## Value class (language pass semantics)

Distinct from storage [`MirRepr`](../crates/echo_mir/src/repr.rs). API:
`echo_mir::ValueClass` (`value_class.rs`).

Rich `p"…{name}…"` lowers to `MirExpr::LocatorInterp` (same `{name}` / `{.field}`
parts as `StringInterp`). Const-only names still bake to `LocatorLit`. Codegen
builds the string, then `echo_runtime_locator_from_string`.

Rich `b"…{name}…"` lowers to `MirExpr::BytesInterp`. Const-only names bake to
`BytesLit`. Codegen cats lit chunks (`bytes_from_ptr`) with
`echo_runtime_bytes_from_value` for live names so `\xHH` bytes stay raw.

| Class | Kinds | Pass |
|-------|--------|------|
| **RefValue** | struct, list | copy reference (share) |
| **StaticValue** | int/float/bool/string/bytes/… | copy value |

`ValueClass::from_value_kind` / `from_mir_repr` / `from_hir_expr` classify for
tests and future lower seeds. ABI handle packing for strings is still
StaticValue in the language model.

## Ironclad goal

**MIR exposes language semantics and native representations so codegen can emit
hyper-optimizable LLVM IR.** Residual **generic** machine-independent
optimization is **LLVM’s** job (`default<On>`).

| Echo MIR owns | LLVM owns |
|---------------|-----------|
| CFG + SSA form | Sparse constant propagation |
| Representation analysis (native vs boxed) | GVN / CSE |
| Explicit box/unbox boundaries | LICM |
| Local simplify (redundant box/unbox, trivial copies) | Induction / loop opts |
| **Escape analysis** + **`NoEscape` scalar box elision** (runtime ABI) | General DCE and mid-end |
| **Scope-owned memory** (promote / demote / release on scope-exit edges) — ADR 0016; **graph promote** + immediate free | N/A (language dispose is not LLVM free) |
| Language-shaped lowering (list get → runtime, …) | `default<O1>`…`default<Oz>` |

**Do not** re-add MIR passes that only reimplement LLVM (constprop, GVN, LICM,
IV, BCE, …) unless IR/benchmarks prove an Echo/ABI barrier LLVM cannot see.
Escape / box elision is intentional MIR work: it needs Echo runtime
non-retaining call knowledge LLVM does not have.

## Pipeline (authority)

```text
structured MIR
  → inject_lifetime (ADR 0016: scope ops; ScopePromote = graph promote at runtime)
  → CFG
  → SSA
  → analyze_reprs
  → simplify_local
  → analyze_escapes (NoEscape box elision)
  → simplify_local
  → LLVM (verify ± default<On>)
```

### Intentional residual MIR passes

| Pass | Why it stays in MIR |
|------|---------------------|
| `inject_lifetime` | Explicit scope ownership ops before CFG; `ScopePromote` lowers to **graph** promote (ADR 0016) |
| `analyze_reprs` | Echo native vs boxed form for hyper-optimizable LLVM handoff |
| `simplify_local` | Cancel redundant box/unbox and trivial copies at representation boundaries |
| `analyze_escapes` | Classify escape with Echo ABI (e.g. non-retaining `runtime.print`); elide `NoEscape` scalar boxes |

### Scope ownership ops (MIR)

| Op | Role |
|----|------|
| `ScopeEnter { id }` | Push dynamic ownership frame (`id` is per-function compile-time id; re-entrant across calls) |
| `ScopeExit { id }` | Pop frame; release every still-owned handle |
| `ScopeRegister { value }` | Record fresh allocation as owned by current frame |
| `ScopePromote { value, target }` | Move ownership to an open ancestor frame |
| `ScopeDisown { value }` | Drop ownership without free (return transfer) |
| `ScopeRelease { value }` | Logical release of one value |

**Slice 1:** wrap function root + if/loop/for-in/match arms; register fresh allocs; promote on nested field/list/index/assign escape; exit scopes on return/break/continue edges. **Name-keyed demote is off** under immediate free (UAF on aliases); graph promote is the shipped escape path. Runtime registries: `echo_runtime_scope_*` (ABI v22+).

## Scope

Control flow, values, calls, and other executable structure prior to LLVM.
Targets LLVM only (ADR 0002) but does not embed LLVM types in the IR design.

## Facts (current)

- Entry: `lower_program(entry_path, modules) -> LoweredProgram`.
- Code objects: closed bodies from HIR `bodies` (symbol = linkage id) + method
  symbols + optional exported value getters (`__val_*`) + entry `__toplevel`.
- **LLVM link names** (`mangle_fn`): `m_<project_relative_path>_<name>` — path is
  relative to the nearest `Cargo.toml` / `.git` ancestor (e.g.
  `m_std_bytes_echo_len`), not the absolute host path. Outside a project root,
  only `parent/file` is used. Virtual `runtime` keeps
  `__echo_runtime_export_*`.
- Function values: `FnValue` (code pointer); `CallTarget::Indirect` for call
  through a value. Direct `CallTarget::Function` when the bind resolves to a
  known body symbol.
- Each `MirFn` carries SSA `cfg`, `reprs`, and `escapes` (local escape classes).
- **Return shapes:** plain / result / option from syntax (`^` / `!`), not user
  type names (`MirRetShape`).
- **Calls:** free, module export, `runtime.*` → `CallTarget::Runtime`, methods
  via type env (struct lit → local type → method table).
- **Heap values (handles as i64):** list lit/index, string lit/interp, struct
  lit / field get / field set.
- **Control:** if, while/infinite/for-in, break/continue, tagged match
  (result/option), **value match** (if/`==`/`||` chain for multi-expr arms;
  bool arms `|` / `_` at arm start).
- **For-in → index loop:** expands to `list_len` / `list_get` with a continue
  block that increments the index. When the body always returns (or otherwise
  never continues), that cont block is left unwired. SSA dominance / φ
  placement uses only blocks **reachable from entry** so a dead cont cannot
  poison the loop header (would freeze the index at 0 → infinite loop).
- Const `#` folding into `const_env` for getters and foldable exprs (lower-time,
  not a mid-end pass): int/float/bool/string/bytes/locator/duration/list/range/struct
  plus field/index on those values and omitted `%` defaults that `#`-fold.
- **List index:** CFG emits `ListGetChecked` → `echo_runtime_list_get` (soft OOB
  in runtime). No MIR bounds-check elimination (LLVM / runtime own that class).
- **Width tags:** full `MirRepr` int/uint/float grid (`i8`…`i64`, `ui8`…`ui64`,
  `f32`/`f64`) for tagged lits and same-width native arith. `MirExpr::Cast`
  is a real convert (trunc/zext/sext, sitofp/uitofp, fptosi/fptoui,
  fpext/fptrunc). Box edges widen to the heap ABI (`i64` / heap float).
  Call ABI stays boxed `i64`.
- **Bytes lits:** `MirExpr::BytesLit` → `MirRepr::BytesRef` (heap handle, parallel
  to `StringRef` but a distinct runtime kind).
- **Duration lits:** `MirExpr::ConstDuration(nanos)` → `MirRepr::Duration` (i64
  nanoseconds; `+`/`-` with other durations).
- **Locator lits:** `MirExpr::LocatorLit` → `MirRepr::LocatorRef` (heap handle,
  path/URI text; parallel to `BytesRef` / `StringRef`).

## Schema / cache

### Lifetime inject: single-eval returns

Managed `^ expr` paths bind a temp before `ScopeDisown` + scope exits so
`expr` (especially a `Call`) is **not** evaluated twice. Double-eval used to
show up as two `keys()` calls in LLVM for thin wrappers like `set.values`.

Bump `MIR_SCHEMA_VERSION` / `MIR_LOWERER_VERSION` in `echo_fingerprint` when MIR
shape or lowering meaning changes ([`incremental.md`](incremental.md)).

## Multi-file methods (`%` / `@`)

- Each module’s HIR records methods defined in **that file**.
- `lower_program` unions them into a graph-wide table:
  `struct → method → (defining_module, mangled_name)`.
- `MethodCall` uses the **defining** module path in `CallTarget::Function`
  (not the call-site module). Duplicate members are rejected earlier by resolve
  merge (`res-struct-dup-member`).

## Structural products (`{}`)

Anonymous `{ k: v, … }` lowers as HIR/MIR struct lit with empty type name
(`echo_runtime_struct_new`). Named tagged lits use `echo_runtime_struct_new_named`
so the heap carries a type tag for `|` `% Type` arms. Field get/set use string
field keys. Not a map; no methods table.

## Match

- **Value / range / `% type`:** lower to if/else (`==` / range membership /
  `MirExpr::StructTypeIs` → `echo_runtime_struct_type_is`).
- **Option/Result:** `MatchTagged` terminator (tag unpack + bind payload).

## Named-struct unions

- HIR `returns_structs: Vec` — empty / one / many from valued `^` named lits.
- MIR seeds monomorphic `__fnret_*` only when `len == 1`.
- `% Type` match arms **refine** a name scrutinee in `type_env` for the arm body.
