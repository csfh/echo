# AGENTS.md

Guidance for humans and coding agents working in this repository.

## Product

Echo is a **standalone compiled language** implemented in Rust. It is **not** a
PHP superset. The toolchain entrypoint is `xo`.

One LLVM backend and one Rust-owned runtime serve both AOT native binaries and
in-process JIT. There is no bytecode VM.

**Std / runtime (locked):** userland imports only `/ std/…` (e.g. `io.print`).
Privileged std sources may `/ runtime` and call `runtime.*`; codegen maps those
to `echo_runtime_*`. No free userland `print`, no userland `/ runtime`. LSP and
reflection use the same pipeline surface as check — see `docs/stdlib.md`.

**Language surface design was reset.** Prefer a small composable core. Do not
treat removed freezes, old Echo PHP-era syntax, or prior session locks as
authority. Track design in `docs/roadmap.md`; write agreed rules into
`docs/syntax.md` / `docs/lexer.md`.

## Where facts live

| Kind of fact | Location |
|--------------|----------|
| Crate ownership, pipeline shape | `docs/architecture.md` |
| SOTA gaps / analysis product | `docs/sota-gaps.md`, ADR 0012 |
| Shared vocabulary | `docs/glossary.md` |
| How to run checks and host tools | `docs/development-speed.md` |
| Which test to write (crate / e26 / examples / `xo test`) | `docs/testing.md` |
| Durable decisions | `docs/adr/` |
| Layer / domain rules | `docs/*.md` (syntax, parser, semantics, …) |
| Design/syntax discussion progress | `docs/roadmap.md` |
| Full pipeline (spine + hosts + order) | `docs/pipeline.md` |
| Full toolchain vertical checklist | `docs/implementation.md` |
| User-facing language book / site | `www/` |
| Machine-checked behavior | tests and fixtures |

Do not invent a second source of truth in chat, scratch notes, or CLI-only
tables. Update the matching doc when a fact becomes durable.

See `docs/README.md` for the full docs map.

## Workspace

- Cargo workspace, resolver `2`, edition **2024**.
- Pipeline (ownership detail in `docs/architecture.md`):

  ```text
  source → lexer → ast/parser → semantics → hir → mir → codegen (LLVM) → AOT | JIT
  ```

- Supporting crates: diagnostics, syntax facts, index, resolver, fingerprint,
  cache, build, reflection, std, lsp. CLI: `xo`.

## Implementation style

- Land language features as **full vertical slices** where applicable: syntax →
  parse → semantics → IR → codegen → runtime → CLI → proof → docs.
- Behavior belongs in the **earliest shared layer** that owns it. `xo` and
  `echo_lsp` present and orchestrate; they must not redefine language semantics.
- Prefer strong types and explicit diagnostics. Fail clearly rather than
  silently approximate.
- Prefer unit tests for pure logic; file fixtures for end-to-end language
  behavior. Fixture conventions live in `docs/fixtures.md`.

### Three proofs — all mandatory (same change)

When you change language, runtime, codegen, std, or CLI behavior, update **all
three** in the **same** change (or stacked PR). Incomplete = incomplete work.

| Proof | Where | Role |
|-------|--------|------|
| **1. Crate tests** | `crates/<name>/` unit/integration tests | Pure logic of **that** crate (decode, resolve edge, unify, …) |
| **2. Echo 2026 (echo26 / e26)** | `echo26/` + `e26` | Executable contract of the **Echo 2026** edition via candidate binary |
| **3. Examples** | `examples/misc/`, `examples/app/`, `examples/algos/` as applicable | Human-runnable demos stay accurate (`xo run` / `xo check`) |

**Echo 2026** is the language edition and **canonical public Language Spec**
(ADR 0015). Tooling IDs stay `e26` / `echo26/`. Public law: site `/e26` +
Reference `/docs`. Implementer surface: `docs/syntax.md` and related layer docs.

Rules:

1. **Every crate** that gains or changes logic must keep its own tests **green and
   current** — add tests when adding behavior; do not leave crates testless after
   non-trivial work.
2. **Echo 2026 suite (e26) is not optional** for user-visible language/runtime
   behavior (see below).
3. **Examples are not optional** when the change is something users would run or
   copy (`xo run` demos under `examples/misc/`, app/std samples when relevant).
4. A crate unit test is **not** a substitute for e26; e26 is **not** a substitute
   for crate tests; examples are **not** a substitute for either.
5. Prove: `cargo test -p <touched crates>`, `e26 --binary target/debug/xo` (or
   `scripts/gate echo26`), and smoke the touched examples.

### Echo 2026 / echo26 / e26 (mandatory with every language change)

**Every** language-surface or frontend/runtime behavior change must update the
Echo 2026 conformance suite in the same change (or the same PR stack):

1. Add or adjust fixtures under `echo26/` (small numbered cases per feature).
2. Extend the candidate protocol in `e26` / `xo` if a new stage or flag is needed
   (`lex`, `ast`, `check`, `run`, …).
3. Refresh expectations (`e26 --binary target/debug/xo --update` when intentional).
4. Prove green: `scripts/gate echo26` (or `just e26`).
5. Keep public Spec / Reference (`www`) and implementer docs aligned when the
   user-visible rule changes.

See `docs/fixtures.md`, `docs/adr/0015-echo-2026-canonical-edition.md`, and
`docs/implementation.md`.

### Justified edge cases only

Do **not** invent parser/lexer/semantic special cases for convenience, fixture
greenness, or “looks like other languages.”

Before adding a branch, recovery path, extra form, or relaxed rule:

1. **Cite authority** — Echo 2026 public Spec / Reference (`www`),
   `docs/syntax.md`, `docs/lexer.md`, an ADR, or an explicit user decision in
   the session (then write it into the matching doc).
2. **Prefer the general rule** — if the design already covers it, implement that;
   don’t bolt on a one-off.
3. **Name the edge** in code comments only when non-obvious, pointing at the doc.
4. **Cover it in echo26** with a small fixture (happy path + reject path if the
   edge is “must fail”).
5. **Flag unjustified leftovers** — when you notice an existing edge with no
   doc basis, call it out to the user and either remove it, lock it in the spec,
   or open a short roadmap/ADR note. Do not silently keep expanding it.

Examples of **justified** edges: dual-use leaders by statement position (syntax);
`~ .field =` as receiver field write (syntax); no trailing commas (syntax).

Examples of **unjustified** edges: accepting trailing commas because chumsky’s
`allow_trailing` is handy; inventing multi-bind syntax only because
`examples/app/` has it before the grammar is locked; recovery that changes
program meaning.

## Verification

- Smallest useful check first: `scripts/gate changed` / `cargo test -p <crate>`.
- Language work: **also** `scripts/gate echo26`.
- Broad gate: `scripts/gate workspace`.
- Website: `scripts/gate web`.
- Host tools: `scripts/gate tools` or `just tools`.

Details: `docs/development-speed.md`.

## Documentation quality

- Document behavior with at least one concrete snippet when a snippet helps.
- Keep ADRs short and decision-focused. Put operational how-to in
  `development-speed.md` or layer docs, not in ADRs.
- Do not leave empty placeholder docs that claim unfinished behavior is done.
  Layer specs may say **Status: not started** until real rules exist.

## Hard rules

- Do **not** reintroduce PHP compatibility as a product goal unless an explicit
  ADR reverses that.
- Do **not** copy language implementation or docs from other Echo repositories.
  Read reference material if needed; **synthesize and write** for this tree.
- Do **not** land language behavior without updating **Echo 2026** proof
  (**`echo26` + `e26`**, green `scripts/gate echo26`) when that stage exists.
- Do **not** introduce language edge cases without **doc/user justification**
  (see “Justified edge cases only” above).

## Design vs implementation (honesty)

When the **locked language model** and the **current compiler** diverge:

1. Document the **gap explicitly** in the matching layer doc / roadmap checklist
   (see function values in `docs/semantics.md` and `docs/hir.md`).
2. Do **not** mark Run ✓ or “done” for a design that only works via a shortcut
   (e.g. static calls only while “functions are values” is incomplete).
3. Prefer finishing the model over stacking more shortcuts. When implementing
   the real path, **remove** obsolete names/comments/aliases that restate the
   old story so later sessions are not misled.
4. In chat and commits, say **partial / gap** when that is the truth.
