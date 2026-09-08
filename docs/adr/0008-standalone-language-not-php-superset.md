# 0008. Standalone language, not PHP superset

## Status

Accepted.

## Context

An earlier Echo experiment targeted PHP compatibility as a superset. That
product direction is abandoned for this repository.

## Decision

Echo is a **new standalone language**. PHP compatibility, PHP surface catalogs,
PHP golden fixtures, and PHP-prefixed product defaults are **out of scope**
unless a future ADR explicitly reverses this decision.

## Consequences

- Fixtures and docs use Echo-owned expectations only.
- Do not reintroduce framework- or PHP-specific compiler paths.
- Historical repositories may be read as references; nothing is copied forward
  as authority for this tree.
