# Memory reclamation

How Echo reclaims managed heap. This is the implementer map for the product law
in [ADR 0016](adr/0016-scope-owned-memory.md).

| | |
|--|--|
| **Status** | **Law locked**; **graph promote landed** (region evacuation + epoch); slice 2 free/exit; public `/docs/memory` |
| **Owners** | Semantics (lifetime facts) · MIR (`inject_lifetime`) · `echo_runtime` (`scope_*` graph promote) · codegen (emit ops only) |
| **Related** | [ADR 0016](adr/0016-scope-owned-memory.md), [`semantics.md`](semantics.md) § Managed heap lifetime, [`mir.md`](mir.md) § Scope ownership ops, [`runtime-abi.md`](runtime-abi.md) § Memory reclamation · www `/docs/memory` |

## Garbage collection

**Echo does not use tracing garbage collection** as the user-facing or primary
reclamation model. There is no Boehm-style or JVM-style concurrent collector,
no mark-and-sweep heap walk, and no cycle detector as the product story.

Classic tracing GC and pure reference counting were considered and rejected for
the primary design (ADR 0016). The preference is **predictable reclamation tied
to language structure**: when control leaves a scope, still-owned managed values
are disposed on that edge—deterministically, not when a collector later runs.

| Approach | Role in Echo |
|----------|----------------|
| **Tracing GC** | **Out** as the product model |
| **Pure refcounting** | **Out** as the user-facing model |
| **Scope-owned dispose** | **In** — every managed allocation has an owning scope; leave-scope edges **release** |
| Runtime RC-like internals | Allowed only where the MIR contract requires shared ownership; never the story users reason about |

Do not treat “add Boehm/JVM GC” as the default plan without reversing ADR 0016.

## Product law

**Every managed allocation is assigned an owning lexical or dynamic scope.**

Semantic lifetime analysis lowers scope transitions into explicit MIR operations.
**Every control-flow edge leaving a scope deterministically disposes of values
still owned by that scope.**

| Term | Meaning |
|------|---------|
| **Owning scope** | Lexical block, function frame, or other locked dynamic scope that **owns** a managed allocation |
| **Promotion** | **Graph evacuation**: root + every reachable alloc still owned by the source frame moves to dest |
| **Demotion** | Ownership moves into a nested/shorter scope when analysis proves a shorter life (optional optimize) |
| **Release** | Deterministic dispose of still-owned values on **every** CFG edge that leaves the scope |

## Graph promotion (region evacuation)

**Law:** when a managed value escapes its owning scope **S** into destination **T**,
every reachable managed allocation whose **current owner is S** is transferred to
**T**. Allocations owned by any other frame (including longer-lived shared roots)
stay put.

```text
promote_graph(root, T):
  S = owner(root)          # source dynamic frame
  if no S: rehome root → T only; return
  epoch++
  queue ← [root]  # unique via header.promotion_epoch == epoch
  while queue:
    h ← pop
    if owner(h) != S: continue
    rehome h → T
    for each managed child of h (list elems / struct fields):
      if not yet marked this epoch: enqueue
```

| Kind | Children walked |
|------|-----------------|
| list | live heap elems |
| struct | live heap field values |
| string / bytes / float / range / fn / locator | none (leaves) |

**Not GC:** no whole-heap mark, no cycle collector — only rehome owner==S.
**Cycles:** epoch mark ⇒ each object processed at most once per promote.
**Cost:** size of the escaping subgraph.

**ABI:** `echo_runtime_scope_promote` / `echo_runtime_scope_promote_graph` (same
semantics). See [`runtime-abi.md`](runtime-abi.md).

### Intuition

```text
{                       ← enter scope S
  $ xs = [1, 2, 3]      ← register allocation → owned by S
  …
}                       ← exit S → release still-owned values (including xs if not promoted)
```

If a value must outlive `S` (return, store into outer struct/list, …), analysis
**promotes** ownership to an enclosing scope before exit. Unpromoted owners are
released when their scope ends—including on `return`, `break`, `continue`, and
error-shaped early exits once those edges are fully wired.

## Layer ownership

| Layer | Responsibility |
|-------|----------------|
| **Semantics** | Lifetime / ownership facts; which allocations bind to which scopes; diagnostics for illegal escapes |
| **MIR** | Explicit promote / demote / release ops on scope transitions; CFG edges must not drop ownership silently |
| **Runtime** | Implement release/promote/demote for heap kinds ([ADR 0004](adr/0004-rust-runtime-owns-executable-semantics.md)) |
| **Codegen** | Emit the MIR ops; **no inventing a free policy** |

Escape analysis for **scalar box elision** (MIR today) is **orthogonal**: that is
ABI packing (native vs boxed), not ownership for dispose.

Value-vs-ref pass rules ([`semantics.md`](semantics.md) § Value vs reference) are
also orthogonal: ref types may share storage, but **ownership for dispose** is
scope-based.

## Pipeline (slice 2)

```text
structured MIR
  → inject_lifetime   # precise promote/demote + leave-scope exits
  → CFG → SSA → …
codegen
  → echo_runtime_scope_*   # registries; immediate free on exit
```

### MIR ops

| Op | Role |
|----|------|
| `ScopeEnter { id }` | Push dynamic ownership frame |
| `ScopeExit { id }` | Pop frame; release every still-owned handle |
| `ScopeRegister { value }` | Record a fresh allocation as owned by the current frame |
| `ScopePromote { value, target }` | **Graph** promote root → target (children with owner==source follow) |
| `ScopeDisown { value }` | Drop ownership without free (e.g. return transfer) |
| `ScopeRelease { value }` | Logical release of one value |

`id` is a per-function compile-time id; the same id may re-enter across nested
calls (each push is a distinct dynamic frame). See [`mir.md`](mir.md).

### Runtime ABI

| Symbol | Role |
|--------|------|
| `echo_runtime_scope_enter` | Push ownership frame |
| `echo_runtime_scope_exit` | Pop frame; release still-owned values |
| `echo_runtime_scope_register` | Own handle in current frame |
| `echo_runtime_scope_promote` | Transfer ownership to target scope |
| `echo_runtime_scope_disown` | Drop ownership without free |
| `echo_runtime_scope_release` | Logical release one value |
| `echo_runtime_scope_enqueue_release` | Logical release + enqueue for deferred free |
| `echo_runtime_scope_drain_deferred` | Physical free of the deferred batch |

Full signatures: [`runtime-abi.md`](runtime-abi.md) § Memory reclamation.

## Status (honest)

| | |
|--|--|
| **Law** | Locked — scope-owned dispose is the product reclamation model |
| **Slice 1** | Registries + MIR inject + leave-scope exits (foundation) |
| **Slice 2 (landed)** | Precise promote targets; demotion helpers; owning_scope facts; **immediate physical free** |
| **Graph promote (landed)** | Runtime region evacuation + header epoch; nest/cycle/shared unit tests; e26 `run/lifetime/010`–`012` |
| **Physical free today** | **Immediate** on `scope_exit` / `scope_release`; `enqueue_release` + `drain_deferred` for short-batch points |
| **Still open** | Richer illegal-escape diagnostics; name-keyed demote (disabled under immediate free); industrial region types (SOTA G9) |
| **Gap framing** | Incomplete reclaim is a **gap** for unfinished edges only — not a competing GC design |

Product model remains **scope-owned dispose** — still **not** tracing GC.

## Slice 2 (landed) — what shipped

| Package | Implementation |
|---------|----------------|
| **2a–2f** | Slice 2: inject promote targets, leave-scope exits, owning_scope facts, demote helpers, immediate free, e26 001–009 |
| **Graph promote** | Runtime queue+epoch; `scope_promote` = graph; crate tests nest/cycle/shared/**deterministic twice**; e26 010–012; docs + www Memory |

### Pipeline (slice 2)

```text
structured MIR
  → inject_lifetime   # precise promote/demote + leave-scope exits
  → CFG → SSA → …
codegen
  → echo_runtime_scope_*   # registries; immediate free on exit
```

## Features that allocate

Anything that creates managed heap (lists, strings, structs, bytes, …) must
eventually thread ownership through promote/demote/release in the same vertical
style as other language features ([ADR 0009](adr/0009-full-vertical-slices.md)).
New heap kinds without scope registration are incomplete work.

## Where facts live

| Kind of fact | Location |
|--------------|----------|
| Sticky decision | [ADR 0016](adr/0016-scope-owned-memory.md) |
| Language-facing lifetime law | [`semantics.md`](semantics.md) § Managed heap lifetime |
| MIR ops and inject pass | [`mir.md`](mir.md) |
| Runtime symbols | [`runtime-abi.md`](runtime-abi.md) |
| Vocabulary | [`glossary.md`](glossary.md) — *scope-owned memory*, *promotion / demotion / release* |
| Design/impl checklist row | [`roadmap.md`](roadmap.md) § Memory reclamation |
| This map | `docs/memory.md` |
