# Reflection

Two different surfaces share the English word “reflection.” Do not merge them.

| Surface | Package / crate | Audience | Role |
|---------|-----------------|----------|------|
| **Tools metadata** | `echo_reflection` | LSP, docs, hosts | Export/signature facts from the **same** pipeline as check — never a second type system |
| **Value kind API** | `/ std/reflect` | Userland (and std) | Runtime kind of an ABI slot: `kind`, `kind_name`, `key_bytes`, `is_*` |

| | |
|--|--|
| **Status** | Tools crate: **stub**. `std/reflect`: **done** (see [`stdlib.md`](stdlib.md), [`runtime-abi.md`](runtime-abi.md)) |
| **Owners** | `echo_reflection` (tools); `echo_runtime` + `std/reflect.echo` (values) |
| **Related** | [`semantics.md`](semantics.md) (`unknown` vs `value`), [`pipeline.md`](pipeline.md), [`lsp.md`](lsp.md) |

## Tools crate (`echo_reflection`)

- Workspace linkage only today (`crate_name()`).
- When implemented: feed from index + resolver + semantics; mirror graph exports
  (std + user + runtime package table), not `nm` alone.

## Userland (`std/reflect`)

```echo
/ std/reflect

reflect.kind(x)         ; i64 code (0=int, 2=string, …)
reflect.kind_name(x)    ; "int" | "string" | …
reflect.key_bytes(x)    ; kind-tagged bytes for SipHash (collections)
reflect.is_string(x)    ; …
```

- Bridge: privileged std `/ runtime` → `echo_runtime_reflect_*`.
- Checker params are often internal **`value`**; runtime still discriminates
  each slot. See [`semantics.md`](semantics.md) § `unknown` vs `value`.

## Non-goals

- PHP-style reflection APIs
- Parallel native-only std tables invisible to check
- Collapsing tools metadata and value-kind APIs into one crate
