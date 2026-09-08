# Runtime ABI

Contracts between codegen and the Rust runtime for AOT and JIT.

| | |
|--|--|
| **Status** | Active (minimal vertical) |
| **Owners** | `echo_runtime`, `echo_codegen_abi`, `echo_codegen` |
| **Related** | `docs/adr/0004-rust-runtime-owns-executable-semantics.md`, `docs/llvm.md` |

## Scope

Runtime symbols, value representation at the ABI boundary, calling conventions
for builtins, and guarantees that AOT and JIT share one contract.

## Facts

- Runtime crate types: `rlib` and **`staticlib`** (`libecho_runtime.a`).
- Language semantics stay in Rust; host linkers are build plumbing only.
- Symbol names live in `echo_codegen_abi`.
- **No user type names** at the ABI: int/result/option are *shapes* from surface
  syntax (`^` / `!`, lits, …). The only explicit kind surface remains width tags
  like `<i32>` on numeric lits (see `docs/semantics.md`).

### Std → runtime bridge (locked)

- Userland never names `echo_runtime_*` and never imports `/ runtime`.
- **Std sources only** may `/ runtime` and call `runtime.export(…)`.
- Codegen lowers those calls via
  `(runtime export name) → echo_runtime_*` (`echo_codegen_abi`).
- User-facing API remains `std` (`io.print`, …). Tools see real Echo in
  `std/**/*.echo` including the `runtime.*` call in the body.
- Full policy: [`stdlib.md`](stdlib.md), [`modules.md`](modules.md).

### Build order (locked)

**Runtime primitives first, then std.**

1. `echo_runtime_*` implementation (+ crate test when logic is non-trivial)
2. `echo_codegen_abi` `RT_*` constant
3. `echo_std::RUNTIME_EXPORTS` entry (`runtime.name` → native)
4. `echo_codegen`: declare symbol, emit arity (and void-return if needed), JIT map
5. `std/**.echo` wrapper with product policy
6. Proofs + version bumps (`RUNTIME_ABI_VERSION`, `STDLIB_VERSION`)

Do **not** invent OS/UTF-8/socket/clock behavior only in Echo, or export a std
API that has no native backing when native is required. Pure Echo std (e.g.
`collections`, `is_empty`) needs no new symbol.

### Language runtime (not the `/ runtime` package)

List lits, for-in, index, result/option packing emit `echo_runtime_*` from
**syntax**, not from a `runtime` import.
## Symbols (v1)

| Symbol | Signature (C) | Role |
|--------|---------------|------|
| `echo_entry` | `i64 echo_entry(void)` | Generated program body |
| `main` | `i32 main(void)` | Process entry → truncates `echo_entry` |
| `echo_runtime_abort` | `void echo_runtime_abort(const uint8_t *ptr, size_t len)` | Hard abort (rare; not `!`) |
| `echo_runtime_print_i64` | `void echo_runtime_print_i64(int64_t v)` | Print **string** handle only + newline on **stdout** (non-strings ignored) |
| `echo_runtime_eprint` | `void echo_runtime_eprint(int64_t v)` | Print **string** handle only + newline on **stderr** (`std/io.eprint`) |
| `echo_runtime_stdin_read` | `int64_t (void)` | Whole process stdin as **bytes** until EOF; empty payload on empty stdin; `0` only on I/O error (`std/io.read`) |
| `echo_runtime_str_from_int` | `int64_t (int64_t)` | Format signed int → string handle |
| `echo_runtime_str_from_float` | `int64_t (int64_t)` | Format float handle/bits → string handle |
| `echo_runtime_str_from_bytes` | `int64_t (int64_t)` | Bytes handle → string (UTF-8 lossy) |
| `echo_runtime_str_from_duration` | `int64_t (int64_t)` | Duration nanos → string (`5s`, `10ms`, …) |
| `echo_runtime_locator_from_utf8` | `int64_t (const uint8_t *p, size_t n)` | Locator handle from path/URI text |
| `echo_runtime_locator_from_string` | `int64_t (int64_t)` | String handle → locator (path/URI text copy) |
| `echo_runtime_str_from_locator` | `int64_t (int64_t)` | Locator → string (path/URI text) |
| `echo_runtime_locator_class` | `int64_t (int64_t)` | String or locator → `0` rel / `1` abs / `2` URI |
| `echo_runtime_bytes_from_ptr` | `int64_t (const uint8_t *p, size_t n)` | Bytes handle from payload copy |
| `echo_runtime_bytes_from_value` | `int64_t (int64_t)` | Universal value → bytes (`{name}` in rich `b"…"`) |
| `echo_runtime_float_from_f64` | `int64_t (double)` | Box f64 as heap float handle |
| `echo_runtime_float_to_f64` | `double (int64_t)` | Unbox heap float (or bitcast fallback) |
| `echo_runtime_string_from_utf8` | `int64_t echo_runtime_string_from_utf8(const uint8_t *p, size_t n)` | String handle |
| `echo_runtime_eq` / `_ne` | `int64_t (int64_t, int64_t)` | **Deep** eq (lists/structs recursive; string/bytes content; ints) |
| `echo_runtime_eq_id` / `_ne_id` | `int64_t (int64_t, int64_t)` | **Identity** eq (`===` / `!==` — handle/bit pattern) |
| `echo_runtime_string_builder_*` | new / push_str / push_value / finish | Rich `{name}` interpolation (strings; locators stringify as path/URI text) |
| `echo_runtime_list_new` | `int64_t echo_runtime_list_new(void)` | Empty list handle (pointer bits) |
| `echo_runtime_list_push` | `void echo_runtime_list_push(int64_t list, int64_t v)` | Append element |
| `echo_runtime_list_reserve` | `void (int64_t list, int64_t additional)` | Reserve push capacity |
| `echo_runtime_list_new_empty_lists` | `int64_t (int64_t n)` | Outer list of `n` fresh empty lists (hash-table buckets) |
| `echo_runtime_list_len` | `int64_t echo_runtime_list_len(int64_t list)` | Length |
| `echo_runtime_list_get` | `int64_t echo_runtime_list_get(int64_t list, int64_t i)` | Element or 0 |
| `echo_runtime_str_repeat` | `int64_t (int64_t s, int64_t n)` | String `s` repeated `n` times (O(n·\|s\|)) |
| `echo_runtime_bytes_get` | `int64_t (int64_t bytes, int64_t i)` | Byte 0..255 or -1 OOB (**borrows**; no full-buffer clone) |
| `echo_runtime_list_set` | `void echo_runtime_list_set(int64_t list, int64_t i, int64_t v)` | Store or soft no-op OOB |
| `echo_runtime_struct_new` | `int64_t echo_runtime_struct_new(void)` | Empty **anonymous** struct handle (no type tag) |
| `echo_runtime_struct_new_named` | `int64_t (const uint8_t *name, size_t)` | Empty struct with `% Shape` type tag |
| `echo_runtime_struct_type_is` | `int64_t (int64_t, const uint8_t *name, size_t)` | 1 if handle’s type tag equals `name` (for `|` `% Type` arms) |
| `echo_runtime_struct_set` | `void (int64_t, const uint8_t *name, size_t, int64_t)` | Insert/replace field by name |
| `echo_runtime_struct_get` | `int64_t (int64_t, const uint8_t *name, size_t)` | Field by name, or 0 |
| `echo_runtime_test_register` | `void (int64_t name_str, int64_t fn)` | Suite: register test case (`XO_TEST` only) |
| `echo_runtime_test_bench_register` | `void (int64_t name_str, int64_t fn)` | Suite: register benchmark (`XO_TEST` only; runs with `XO_BENCH`) |
| `echo_runtime_test_fail` | `void (int64_t msg_str)` | Suite: mark current case failed |
| `echo_runtime_test_finish` | `int64_t (void)` | Suite: run tests or benches → fail count; `-1` if suite off |
| `echo_runtime_now_ms` | `int64_t (void)` | Wall clock ms since Unix epoch |
| `echo_runtime_sleep_ms` | `void (int64_t ms)` | Sleep ≥ `ms` (no-op if `ms ≤ 0`) |
| `echo_runtime_process_args` | `int64_t (void)` | argv as list of string handles; in-process JIT hosts override to source path + user args (not the `xo` CLI) |
| `echo_runtime_process_env_has` | `int64_t (name)` | 1 if env var set, else 0 |
| `echo_runtime_process_env_get` | `int64_t (name)` | string handle; empty if unset |
| `echo_runtime_process_env_set` | `void (name, value)` | set env var (strings) |
| `echo_runtime_process_env_unset` | `void (name)` | remove env var |
| `echo_runtime_process_exit` | `void (code)` | terminate process |
| `echo_runtime_process_run` | `int64_t (program, args_list)` | spawn+wait; exit code or `-1` spawn fail |
| `echo_runtime_fs_exists` | `int64_t (path)` | 1/0; path string or locator |
| `echo_runtime_fs_is_file` | `int64_t (path)` | 1/0 |
| `echo_runtime_fs_is_dir` | `int64_t (path)` | 1/0 |
| `echo_runtime_fs_join` | `int64_t (base, rel)` | joined path string |
| `echo_runtime_fs_read` | `int64_t (path)` | bytes handle, or `0` fail |
| `echo_runtime_fs_write` | `int64_t (path, data)` | 0 ok, `-1` fail; data bytes or string |
| `echo_runtime_fs_remove` | `int64_t (path)` | remove file; 0 ok, `-1` fail |
| `echo_runtime_fs_create_dir` | `int64_t (path)` | 0 ok, `-1` fail |
| `echo_runtime_fs_create_dir_all` | `int64_t (path)` | 0 ok, `-1` fail |
| `echo_runtime_fs_read_dir` | `int64_t (path)` | list of name strings, or `0` fail |
| `echo_runtime_fs_remove_dir` | `int64_t (path)` | empty dir; 0 ok, `-1` fail |
| `echo_runtime_fs_copy` | `int64_t (from, to)` | 0 ok, `-1` fail |
| `echo_runtime_fs_rename` | `int64_t (from, to)` | 0 ok, `-1` fail |
| `echo_runtime_fs_metadata` | `int64_t (path)` | list `[len,is_file,is_dir,is_symlink,modified_ms]`, or `0` fail |
| `echo_runtime_fs_open_read` | `int64_t (path)` | file handle, or `0` fail |
| `echo_runtime_fs_open_write` | `int64_t (path)` | create/truncate handle, or `0` |
| `echo_runtime_fs_open_append` | `int64_t (path)` | append handle, or `0` |
| `echo_runtime_fs_file_read` | `int64_t (handle, limit)` | bytes; `0` error; empty = EOF |
| `echo_runtime_fs_file_write` | `int64_t (handle, data)` | 0 ok, `-1` fail |
| `echo_runtime_fs_file_seek` | `int64_t (handle, pos)` | new offset, or `-1` |
| `echo_runtime_fs_file_close` | `void (handle)` | close open file |
| `echo_runtime_str_slice` | `int64_t (s, start, end)` | UTF-8 **payload** byte range `[start,end)` then lossy-decode that slice; empty if invalid |
| `echo_runtime_str_get` | `int64_t (s, index)` | UTF-8 byte 0..255, or -1 if OOB |
| `echo_runtime_str_contains` | `int64_t (hay, needle)` | 1/0 substring |
| `echo_runtime_str_starts_with` | `int64_t (s, prefix)` | 1/0 |
| `echo_runtime_str_ends_with` | `int64_t (s, suffix)` | 1/0 |
| `echo_runtime_bytes_slice` | `int64_t (b, start, end)` | byte range `[start,end)`; empty if invalid |
| `echo_runtime_bytes_cat` | `int64_t (a, b)` | concatenate bytes |
| `echo_runtime_bytes_from_str` | `int64_t (string)` | UTF-8 payload as bytes |
| `echo_runtime_reflect_kind` | `int64_t (int64_t)` | Value kind code (`0`=int; heap tags match header) |
| `echo_runtime_reflect_kind_name` | `int64_t (int64_t)` | Kind name string handle (`"int"`, `"string"`, …) |
| `echo_runtime_reflect_key_bytes` | `int64_t (int64_t)` | Kind-tagged bytes for hashing (see below) |

### Value reflection (`std/reflect`)

Userland **runtime** kind inspection and hash-key material. Privileged std only:
`runtime.reflect_*` → symbols above. **Not** the tools crate `echo_reflection`
(export metadata for LSP).

Every ABI slot is still an `i64`. The runtime **always** knows concrete kind
(header tag or bare int) even when the checker labels a param/field `value`.

| Kind code | Name | Wire |
|-----------|------|------|
| `0` | int | bare non-heap (bool as `0`/`1`) |
| `1` | list | heap |
| `2` | string | heap |
| `4` | struct | heap |
| `5` | float | heap |
| `6` | bytes | heap |
| `7` | locator | heap |
| `8` | range | heap |
| `9` | fn | heap |

**`key_bytes`:** `[kind] || content` for hashing (map/set):

| Kind | Payload after tag byte |
|------|------------------------|
| int | `le8(bits)` |
| string | UTF-8 |
| bytes | raw payload |
| other heap | `le8(handle)` (identity-stable, not deep content) |

## Memory reclamation (law — ADR 0016)

Implementer map (GC stance, layers, status): [`memory.md`](memory.md).

**Product law:** every managed allocation has an owning **lexical or dynamic
scope**. Semantics records ownership facts (`owning_scope`); MIR lowers scope
transitions to
explicit **promotion**, **demotion**, and **release**. Every CFG edge that
leaves a scope **deterministically disposes** of values still owned by that
scope.

| Status today | Registries + **graph promote** (region evacuation, header `promotion_epoch`) + **immediate free** on exit; enqueue/drain for batch |
| Target | Same law; demote remains optional optimize (not tracing GC) |

| Symbol | Signature | Role |
|--------|-----------|------|
| `echo_runtime_scope_enter` | `void (i64 scope_id)` | Push ownership frame |
| `echo_runtime_scope_exit` | `void (i64 scope_id)` | Pop frame; release still-owned values |
| `echo_runtime_scope_register` | `void (i64 handle)` | Own handle in current frame |
| `echo_runtime_scope_promote` | `void (i64 handle, i64 target_id)` | **Graph** promote: root + reachable owner==source → target |
| `echo_runtime_scope_promote_graph` | `void (i64 handle, i64 target_id)` | Same as promote (explicit name) |
| `echo_runtime_scope_disown` | `void (i64 handle)` | Drop ownership without free |
| `echo_runtime_scope_release` | `void (i64 handle)` | Logical release one value |
| `echo_runtime_scope_enqueue_release` | `void (i64 handle)` | Logical release + enqueue |
| `echo_runtime_scope_drain_deferred` | `void (void)` | Physical free deferred batch |

Heap headers carry `promotion_epoch` for visit uniqueness during graph promote.
The live-handle set is **process-global** so natives that classify heap vs
bare int (`json_stringify`, …) see allocations from `+` worker threads.

Runtime implements dispose ops for heap kinds; codegen must not invent a
competing free policy. See [ADR 0016](adr/0016-scope-owned-memory.md).

## Value wire (v1)

| Shape (from syntax) | LLVM return | Notes |
|---------------------|-------------|--------|
| plain | `i64` | bool as 0/1; untagged ints default i64 |
| result (`!` path in fn) | `i128` | high 64 = tag (`0` ok, `1` err); low 64 = payload |
| option (bare `^` + valued `^`) | `i128` | high 64 = tag (`0` some, `1` none); low 64 = payload |
| list (`[…]`) | `i64` handle | runtime heap list (magic header + elems) |
| range (`lo..hi`) | `i64` handle | inclusive i64 range; `list_len`/`list_get` iterate |
| function value | `i64` handle | `fn_new(code, shape)`; shape 0 plain / 1 result / 2 option |
| HTTP request parse | `i64` struct handle | `http_parse_request(raw)` via **httparse** (up to **128** headers); `method`/`path`/`body`; `headers` product (names lowercased, `-`→`_`) |
| zip first entry | `i64` struct handle | `zip_unpack_first` always `{ ok, name, data }` (`ok` 0/1; `name` string; `data` bytes) |
| utf8 valid/decode | `i64` | `utf8_valid` 1/0; `utf8_decode` string handle or 0 (strict `from_utf8`) |
| HTTP headers complete | `i64` 0/1 | `http_headers_complete(raw)` — `\r\n\r\n` present (serve read accumulate) |
| TCP listener / stream | `i64` handle | kinds 10/11; **runtime-only** opaque ids — language model: field of `% listener` / `% conn` ([`semantics.md`](semantics.md) § Value vs reference); not a userland `RefValue` leaf |
| UDP socket | `i64` handle | kind 12; same — wrap in a std struct for userland |
| Task handle | `i64` handle | kind 13; `task_spawn_entry(code,shape)` / `task_join` / `task_join_wide` / `task_block[_wide]` via **mio** poller + worker pool |
| TCP/UDP | nonblocking | WouldBlock → `park_fd` on mio registry; multi-worker so other tasks run |
| string (`'…'` / `"…"`) | `i64` handle | runtime heap UTF-8 (magic header); rich expands escapes at lower |
| bytes (`b'…'` / `b"…"`) | `i64` handle | runtime heap byte blob (magic header); distinct from string |
| named / anon struct (`name { k: v }` / `{ k: v }`) | `i64` handle | runtime heap struct (magic header + optional type tag + name→value fields) |
| float (default `f64`) | native `double` / `f32` in SSA (language: **value**, like int) | may box at universal `i64` slots via `float_from_f64` / `float_to_f64` — **ABI packing only**, not a user ref type |
| duration (`5s`, `10ms`, …) | native `i64` nanoseconds | format with `str_from_duration` |
| locator (`p'…'` / `p"…"`) | `i64` handle | heap path/URI text; not a string |

Tagged packing is **internal**. It is not a user struct and not a keyword. Users
produce/consume result/option only via `^` / `!` and `|` match arms. Lists come
from list **literals** and are held as runtime handles. Named structs come from
`%` shapes + tagged lits; fields are read/written by name at the runtime.

Width tags (`<i32>`, …) fix literal storage. Explicit `<width> expr` converts
an integer or float value; they are not general ascriptions.

## Intrinsics / primitives

| Surface | Lowering |
|---------|----------|
| `runtime.print` (std only, via `/ runtime`) | `echo_runtime_print_i64` — **strings only** |
| `runtime.str_from_int` / `str_from_float` / `str_from_bytes` / `str_from_duration` / `str_from_locator` | explicit convert → string for print |
| `runtime.http_headers_complete` | 1 if `\r\n\r\n` present (borrows string/bytes payload) |
| `runtime.http_request_complete` | 1 if headers + body for `Content-Length` (if any) ready (borrows) |
| `runtime.http_parse_request` | method/path/headers/body product (borrows raw; body lossy-decoded once) |
| `runtime.store_*` | memory/S3 object store (`get`/`put`/`head`/CAS/range) → `{ status, body, etag }` |
| `runtime.git_*` | pack valid/count/index (`gix-pack`); receive-pack split; upload-pack wants |
| locator lit | `echo_runtime_locator_from_utf8` |
| bytes lit | `echo_runtime_bytes_from_ptr` |
| Bare userland `print` | **Not** an intrinsic (unbound unless user-defined) |
| List lit / for / index | `echo_runtime_list_*` (language) |
| Struct lit / field get/set | `echo_runtime_struct_*` (language) |

## Open questions

- Strings / heap payloads on err side
- Multi-file / exported shapes in the wire
- JIT registration of the same symbols
