# 0015. Echo 2026 is the language edition and canonical public specification

## Status

Accepted.

## Context

The product historically used **e26** / **echo26** for a black-box fixture suite
and runner. That tooling is essential, but calling the suite “e26” in user-facing
places made it sound like a test harness rather than the **language edition**
and its **public specification**.

Implementer prose lives under `docs/` (especially `docs/syntax.md`). User-facing
docs live under `www/`. Contributors need a single answer to: *what is the
canonical language for this year of Echo?*

## Decision

1. **Echo 2026** is the **language edition** name for the current surface.
2. The **canonical public Language Spec** for this edition lives on the site
   under the **Echo 2026** section (`www`, URL path `/e26` for now). Form-by-form
   public rules are the Language Spec prose published there and in the site
   **Reference** (`/docs`); together they are the public law of the edition.
3. The **executable contract** of Echo 2026 is the **`echo26/`** fixture suite,
   driven by the **`e26`** runner against a candidate binary (reference: `xo`).
   A green suite is necessary for claiming Echo 2026 behavior.
4. **Tooling identifiers stay short:** CLI/crate `e26`, suite directory
   `echo26/`, gate `scripts/gate echo26`. “Echo 2026” is the display and
   product name; `e26` / `echo26` are stable implementation IDs.
5. **`docs/`** remains **contributor / implementer** authority for compiler
   ownership, pipeline, and layer rules (`docs/syntax.md`, ADRs, etc.). Those
   documents must **align with** Echo 2026; they are not a second public
   language brand. When public Spec and implementer docs diverge, fix the gap
   explicitly (see `AGENTS.md` design-vs-implementation honesty).

## Consequences

- User-facing copy (nav, titles, summaries) says **Echo 2026**, not bare “e26”,
  except when referring to the CLI binary or paths.
- Language work still updates `echo26/` and keeps `e26` green (three proofs).
- Future URL renames (`/e26` → `/echo-2026`) or path renames of `echo26/` are
  optional follow-ups; not required by this ADR.
- ADR 0007 still holds: `docs/` vs `www/` ownership. This ADR only names the
  **edition** and where **public** language law lives.
