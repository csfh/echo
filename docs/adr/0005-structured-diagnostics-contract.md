# 0005. Structured diagnostics are a shared contract

## Status

Accepted.

## Context

CLI, LSP, and tests all need the same errors. Ad hoc strings per host produce
inconsistent UX and untestable messages.

## Decision

Diagnostics are a **shared compiler contract** in `echo_diagnostics`: structured
records with spans, stable codes, severities, and related information as needed.
Hosts format for humans or protocols; they do not invent new diagnostic
categories when analyzing language source.

## Consequences

- New diagnostic kinds are defined in the shared model (or producing layer) with
  stable codes when user-visible.
- Tests can assert on codes and spans, not only rendered text.
- Pretty-printing (for example ariadne-style) is presentation, not the source of
  truth.
