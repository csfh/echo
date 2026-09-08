# Diagnostics

Structured compiler diagnostics shared across hosts.

| | |
|--|--|
| **Status** | Scaffold only (crate exists; codes not defined) |
| **Owners** | `echo_diagnostics` (model); producing layers emit diagnostics |
| **Related** | `docs/adr/0005-structured-diagnostics-contract.md` |

## Scope

Diagnostic shape, severity, stable codes, related spans, and presentation
boundaries. CLI and LSP format; they do not invent analysis categories.

## Facts

- Shared model crate: `echo_diagnostics`.
- Presentation may use pretty printers later; the model is the contract.

## Code catalog

<!-- Table of stable codes as they land. -->

| Code | Severity | Summary | Emitting layer |
|------|----------|---------|----------------|
| — | — | — | — |

## Open questions

- Code numbering scheme (`E0001` vs domain prefixes)
