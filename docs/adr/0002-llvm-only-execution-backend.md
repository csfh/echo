# 0002. LLVM-only execution backend

## Status

Accepted.

## Context

A second interpreter, bytecode VM, or parallel backend would split lowering and
runtime contracts and double the test surface.

## Decision

Echo has **one** execution backend: **LLVM IR** via `echo_codegen` (inkwell).
From LLVM IR:

- AOT/native binaries (`xo build`, default `xo run`)
- In-process JIT (`xo run --jit`)

There is no language-level bytecode VM and no second execution engine.

## Consequences

- MIR stays backend-neutral but targets LLVM only.
- Runtime symbols and ABI must work for both AOT and JIT.
- Host tools needed for full executable paths include LLVM and a C linker driver
  (currently clang).
