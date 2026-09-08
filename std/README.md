# `std/` — design sources

Public standard library for Echo. **Authority:** [`docs/stdlib.md`](../docs/stdlib.md).

## Rules (locked)

| Rule | |
|------|--|
| Userland | `/ std/…` only — e.g. `io.print`. No free `print`. No `/ runtime`. |
| These files | May `/ runtime` and call `runtime.export(…)` to reach the native runtime. |
| Tools | LSP / check / reflection see **real Echo** (including bodies that call `runtime.*`). |
| Codegen | `runtime.*` → `echo_runtime_*` (only for the privileged runtime package). |
| **Exports** | `\ name` is the **only** public/private control. Default new helpers to **private**. See **Export discipline** in `docs/stdlib.md`. |
| **Build order** | **Runtime primitives first** (`echo_runtime_*` + ABI + exports + codegen), **then** this tree. Pure Echo helpers need no new native. See `docs/stdlib.md` § Build order. |

### Writing `\ ` in std

1. Header comment lists the intended public API.
2. Export **product** free names and `%` types only.
3. Methods on an exported type are public with that type — do not re-list them on `\ `.
4. Do not export implementation types (`entry`), bucket plumbing, or serve-loop helpers.

## Intended `io` shape

```echo
/ runtime

$ print = (value) {
    runtime.print(value)
}

$ log = (value) {
    runtime.print(value)
}

$ eprint = (value) {
    runtime.eprint(value)
}

$ read = () {
    $ d = runtime.stdin_read()
    ? d == 0 {
        ! "stdin read failed"
    }
    ^ d
}

\ print, log, eprint, read
```

`print` / `log` write stdout; `eprint` writes stderr; `read` returns whole stdin as bytes.

## Layout

| Path | Role |
|------|------|
| `io.echo` / `test.echo` | free helpers (`test` for `xo test` / `xo test --bench`) |
| `bytes.echo` | `len` / `get` / `slice` / `cat` / `from_int` / `from_str` |
| `list.echo` | `len` / `is_empty` / result `get` / `contains` |
| `str.echo` | `from_*`, text ops, byte `get`/`slice` |
| `time.echo` | `now_ms` / `sleep_ms` (wall clock) |
| `process.echo` | `args` / `env` / `env_set` / `env_unset` / `exit` / `run` (spawn+wait) |
| `fs.echo` | paths, whole-file, `copy`/`rename`, `% meta`, streaming `% file` |
| `reflect.echo` | runtime kind API (`kind` / `key_bytes` / …); not tools `echo_reflection` |
| `crypto/hash/` | folder module: `sip` (SipHash-2-4) → `/ std/crypto/hash` as `hash.sip` |
| `collections/hash_table.echo` | SipHash table; keys via `reflect.key_bytes` (int/string/…); backs map + set |
| `collections/map.echo` | map over `hash_table` (`put`/`get`/`from_indexed`; mixed keys) |
| `collections/set.echo` | set over `hash_table` (`add`/`has`/`values`/`from_list`; no `keys`) |

## Suites and benchmarks

Co-located `test.it` / `test.bench` live in the same module file as production API
(Model A; see [`docs/testing.md`](../docs/testing.md)).

```bash
xo test std/math.echo          # unit cases
xo test --bench std            # all std modules that define test.bench
xo test --bench std/str.echo   # one module
```

CPU-oriented modules currently ship benches (math, str, bytes, list, json, path,
encoding/hex+base64, crypto/hash/sha256+sip, collections/map). I/O and network
modules keep tests only for now.
| `net/tcp/` | folder module: `conn`, `listener`, free `socket` |
| `net/udp/` | folder module: `% socket` + free reify `socket` |
| `net/request.echo` | `% request` |
| `net/response.echo` | `% response` |
| `net/server.echo` | `% server` |
| `net/http.echo` | `parse_request` + helpers |

No `*_ops` split. Multi-file `@` is still legal language (see `examples/app/`).
