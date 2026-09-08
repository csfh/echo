# 0004. Rust runtime owns executable semantics

## Status

Accepted.

## Context

AOT and JIT must not disagree about values, IO, tasks, or errors. Putting
semantics in C/C++ libraries or host-specific glue creates dual implementations.

## Decision

**Executable language semantics live in Rust** (`echo_runtime` and related
crates). LLVM IR calls into that runtime. Native linking may use clang (or
equivalent) as **build plumbing** only—not as a place for language semantics or
non-Rust runtime libraries for language behavior.

## Consequences

- AOT and JIT share one runtime contract.
- New built-in behavior is implemented in Rust runtime code and registered for
  both execution modes.
- Transitional link drivers stay thin.
