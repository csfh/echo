# ADR 0012: Analysis product and shared pipeline library

## Status

Accepted

## Context

The sanity check of the Echo spine found that successful `check` only produced
diagnostics, while `xo` rebuilt HIR from raw AST and MIR re-decided module vs
field and related meaning. That is incompatible with a correct,
state-of-the-art compiler: language meaning must be decided once and consumed
by lowering.

## Decision

1. Introduce **`echo_pipeline`** as the shared library entry for analyze/compile.
2. Successful analysis yields an **`AnalysisProduct`** packaging per-module AST,
   HIR (with spans), **`SemanticModel`**, import maps, exports, diagnostics, and
   `is_ok()`.
3. **Executable lowering and codegen run only when `product.is_ok()`.**
4. HIR lowering classifies module vs value operations using the analysis import
   set; method tables live on `HirModule` from the same lower.
5. MIR seeds struct/method typing from `SemanticModel` only (plus flow of
   struct literals and copies of known names); it does not invent types.
6. Each `MirFn` carries a structured `body` and an SSA **`MirCfg`** (basic
   blocks, terminators, φ-nodes, versioned names). **Codegen emits from the
   SSA CFG.**
7. Hosts (`xo`, and any compile driver) call `echo_pipeline`; they must not
   invent a second check→raw-AST→HIR path for program meaning.

## Consequences

- MIR/codegen implement analysis facts; they do not freestyle language rules.
- Cache remains orthogonal (fingerprints of stage outputs).
- After SSA: `analyze_reprs` + `simplify_local` + escape analysis /
  `NoEscape` box elision + final `simplify_local`. Generic mid-end
  (constprop, GVN, LICM, IV, …) is LLVM’s job via `default<On>` (`OptLevel`;
  O0 skips the mid-end).

## Related

[`docs/sota-gaps.md`](../sota-gaps.md), [`docs/pipeline.md`](../pipeline.md)
