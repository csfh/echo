# 0016. Scope-owned memory (deterministic disposal)

## Status

Accepted.

## Context

Heap objects today are process-lived handles (`Box::into_raw` with no reclaim).
Classic tracing GC and pure refcounting were considered. Echo prefers **predictable
reclamation tied to language structure**, not a concurrent collector or cycle
detector as the primary story.

## Decision

**Echo assigns every managed allocation to an owning lexical or dynamic scope.**
Semantic lifetime analysis lowers scope transitions into **explicit MIR**
operations for **promotion**, **demotion**, and **release**. **Every control-flow
edge leaving a scope deterministically disposes of the values still owned by that
scope.**

| Term | Meaning |
|------|---------|
| **Owning scope** | Lexical block / function frame / other locked dynamic scope that **owns** a managed allocation |
| **Promotion** | Ownership moves outward on escape — **graph evacuation** (see below) |
| **Demotion** | Ownership moves into a nested/shorter scope when analysis proves a shorter life (optimization) |
| **Release** | Deterministic dispose of still-owned values on **every** CFG edge that leaves the scope |

### Promotion is graph evacuation

When a managed value **escapes** its owning scope **S** into destination scope
**T** (return, store into a longer-lived container, reassign an outer bind, …):

1. The **root** allocation is rehomed from **S** to **T**.
2. Every **reachable** managed allocation whose **current owner is still S** is
   rehomed to **T** (lists’ elements, structs’ managed fields, recursively).
3. Allocations already owned by a scope that is **not** S (including longer-lived
   shared roots) are **left unchanged**.
4. Cycles are safe: each allocation is processed **at most once** per promotion
   (header promotion epoch + work queue).

This is **region ownership with graph evacuation**, not tracing GC: no whole-heap
mark, no concurrent collector, no cycle *collection*. Cost is proportional to the
escaping subgraph.

This is the product **memory-reclamation law**. It is **not** tracing GC and
**not** pure RC as the user-facing model—though the runtime may use RC-like
internals only where the MIR contract requires shared ownership.

Implementer map (status, pipeline, ABI): [`../memory.md`](../memory.md).  
Public reference: site `/docs/memory`.

### Layer ownership

| Layer | Responsibility |
|-------|----------------|
| **Semantics** | Lifetime / ownership facts; which allocations bind to which scopes; diagnostics for illegal escapes |
| **MIR** | Explicit promote / demote / release ops on scope transitions; promote means **graph** promote of the root |
| **Runtime** | Registries; graph promote (child walk + epoch); release for heap kinds (ADR 0004) |
| **Codegen** | Emit the MIR ops; no inventing free policy |

### Out of scope for this ADR

- Exact MIR opcodes and ABI for promote/demote/release (live in layer docs / vertical).
- Whether shared structure fields use secondary RC, unique ownership only, or
  other runtime techniques—**as long as** the scope-exit dispose law holds.
- Incomplete reclaim remains a **gap** only for unfinished edges, not a competing design.

## Consequences

- Long-lived servers do not rely on process exit to reclaim managed heap once
  this vertical is complete.
- Escape analysis for **scalar box elision** (MIR today) stays orthogonal: that
  is ABI packing, not ownership.
- Features that create heap (lists, strings, structs, …) must eventually thread
  ownership through promote/demote/release in the same change style as other
  vertical slices (ADR 0009).
- Design docs and chat must not treat “add Boehm/JVM GC” as the default plan
  without reversing this ADR.
- Compiler name-keyed alias proofs are **not** the soundness path for nested
  graphs; runtime graph promote is.
