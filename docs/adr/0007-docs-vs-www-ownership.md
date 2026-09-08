# 0007. `docs/` vs `www/` ownership

## Status

Accepted.

## Context

Mixing implementer ABI notes with public language marketing docs makes both
worse. Agents need stable contributor facts; users need a polished site.

## Decision

- **`docs/`** (and `AGENTS.md`, ADRs): contributor and implementer facts.
- **`www/`**: public user-facing site and language documentation.

Do not store the only copy of architecture ownership solely in `www`, and do not
publish unfinished implementer trackers as the public language book.

## Consequences

- Layer specs and ADRs live under `docs/`.
- When a language surface is ready for users, document it in `www` as well (or
  primarily there) without abandoning the implementer rule in `docs/syntax.md`
  and related specs.
