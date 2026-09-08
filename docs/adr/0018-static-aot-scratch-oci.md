# 0018. Static AOT and scratch OCI packaging

## Status

Accepted.

## Context

A deployed Echo program is one ELF. The thinnest container image is that binary
at `/app`, statically linked, with no libc, shell, or extra layers.

OCI is an envelope, not a second compiler backend (ADR 0002). `xo` writes a
layout directory and can push it. It does not invoke Docker or Podman.

## Decision

1. **`xo build --static`** links a fully static ELF (`-static`). The result must
   have no `PT_INTERP`. Prefer a musl `libecho_runtime.a` when present.
2. **`xo build --image oci:<dir>`** implies `--static` and writes a scratch OCI
   image layout (one layer, `Entrypoint: ["/app"]`).
3. **`xo image push`** uploads that layout via the OCI distribution API. Auth
   is `XO_REGISTRY_USER` / `XO_REGISTRY_PASSWORD`.
4. TLS in scratch images uses **webpki-roots** when the OS trust store is empty.

## Consequences

- Default `xo run` / `xo build` stay host-dynamic.
- AOT cache keys include link mode so static and dynamic binaries do not collide.
- Image platform is `linux` plus the ELF machine (`amd64`, `arm64`, `386`).
  `oci:<dir>` is a local layout; a registry reference is what you push and run.
