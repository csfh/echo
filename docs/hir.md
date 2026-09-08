# HIR

High-level intermediate representation: analyzed form still close to source
structure, ready for MIR lowering.

| | |
|--|--|
| **Status** | **Active** — analysis product component (ADR 0012) |
| **Owners** | `echo_hir` (built inside `echo_pipeline::analyze`) |
| **Related** | `docs/sota-gaps.md`, `docs/semantics.md`, `docs/mir.md` |

## Scope

HIR is source-shaped IR with **import classification**, **method tables**, and
**expression/statement spans**. It is not a VM and does not embed LLVM types.

## Facts (current)

- Entry: `lower_file(&File, &HashSet import_module_names) -> HirModule`.
- **Function values:**
  - Design: nameless closed values; binds name them like any other value.
  - `HirModule.bodies` = closed **code objects** (`HirBody.symbol` = linkage).
  - Bind → `FnRef { symbol }` → MIR `FnValue` (runtime: code pointer as `i64`).
  - Call known bind → `Call { symbol }` (direct). Call through value (param/local)
    → `CallValue` → MIR `CallTarget::Indirect`. Nested bodies use `__n_{id}`.
  - Function values carry ret shape (plain/result/option) for indirect calls.
  - Methods remain call-form only (not freestanding values).
- Top-level executable stmts → `entry` (includes function-value binds as `FnRef`).
- **Import names** (analysis fact) classify `ModuleCall` / `ModuleField` vs
  `MethodCall` / `Field` — MIR must not re-decide import vs value.
- **Methods:** `%`/`@` fn members → `__m_{struct}_{method}` body symbol +
  `HirModule.methods` (graph-wide union at MIR lower).
- **Struct lits:** tagged `name { … }` → `StructLit` with type name; structural
  `{ k: v }` → `StructLit` with **empty** name (not a `%` type / methods).
- **Match arms:** value multi-expr, `% Type` (`HirMatchArm::Type`), default,
  Option/Result `$` / `!` binds.
- Every `HirExpr` has `span`; stmts carry `span` for provenance.
- Built only as part of `AnalysisProduct`; hosts use `echo_pipeline`, not raw lower.

## Schema / cache

When HIR node shapes change in a way that would invalidate stored lower
artifacts, bump `HIR_SCHEMA_VERSION` / `HIR_LOWERER_VERSION` in
`echo_fingerprint` ([`incremental.md`](incremental.md)).

## Open questions

- How much desugaring stays in HIR vs MIR as surface grows
