# 0017. TLS v0 client/server (rustls) + cleartext HTTP honesty

## Status

Accepted (implemented for v0 client/server surfaces).

## Context

Expansive std needed a real TLS path without claiming a full HTTPS product.
Prior design-only gate deferred implementation; this ADR now records the shipped
v0 stack.

## Decision

1. **Stack:** **rustls** (+ `rustls-pemfile`, ring crypto provider) in
   `echo_runtime` — no hand-rolled TLS.
2. **Runtime free functions only:** `tls_listen`, `tls_accept`, `tls_connect`,
   `tls_read`, `tls_write`, `tls_close`, `tls_close_listener`.
3. **Std product:** `std/net/tls` exposes `% listener` / `% conn` by reference
   (same net ownership law as TCP), plus `connect` / `listen` / `load_pem`.
4. **Proofs:** crate unit tests use **local PEMs** under `echo26/run/tls/certs/`
   (CA-signed leaf with SAN). e26 covers connect-failure + CA load (deterministic,
   no public internet). Full handshake loopback is crate-tested (task + nested
   result-match PHI still fragile for rich Echo fixtures).
5. **`std/net/http_client` remains cleartext only** unless a separate HTTPS
   helper is added later on top of `std/net/tls`.

## Consequences

- Apps can open TLS to servers with a supplied CA PEM (or refuse without one).
- Platform roots / mutual TLS / ALPN product surface are out of v0.
- Cleartext HTTP docs must not claim HTTPS completeness.

## Related

- `docs/stdlib.md` inventory + “Where logic lives” (crates for TLS)
- ADR 0004 runtime ownership of executable semantics
