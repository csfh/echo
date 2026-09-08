# `examples/misc/`

Tiny programs that **`xo run`** today (LLVM AOT + clang + `libecho_runtime`).
Each prints something via `/ std/io` → `io.print`.

```bash
cargo build -p xo
./target/debug/xo run examples/misc/hello.echo
./target/debug/xo run --jit examples/misc/hello.echo   # same runtime, no clang
./target/debug/xo run examples/misc/sum_list.echo
```

| File | Prints | Exit |
|------|--------|------|
| [`hello.echo`](hello.echo) | `42` | 0 |
| [`eprint.echo`](eprint.echo) | stdout `out`; stderr `err` | 0 |
| [`stdin.echo`](stdin.echo) | `printf ping \\| xo run …` → `ping` | 0 |
| [`process.echo`](process.echo) | argv0, env, spawn-fail | 0 |
| [`process_cwd.echo`](process_cwd.echo) | `process.run_cwd` pwd `/tmp` | `1` |
| [`process_capture.echo`](process_capture.echo) | `process.run_capture` pwd | `1` |
| [`fs.echo`](fs.echo) | write/read file + dirs under `/tmp` | 0 |
| [`fs_chmod.echo`](fs_chmod.echo) | `fs.chmod` Unix 0o644 | `chmod` |
| [`fs_symlink.echo`](fs_symlink.echo) | `fs.symlink` | `1` |
| [`fs_create_temp.echo`](fs_create_temp.echo) | `fs.create_temp` | `1` |
| [`fs_append.echo`](fs_append.echo) | `fs.append` + `file.seek` | `cd` |
| [`add.echo`](add.echo) | `42` | 0 |
| [`countdown.echo`](countdown.echo) | `1` … `5` | 0 |
| [`break_loop.echo`](break_loop.echo) | `0` `1` `2` then `3` | 0 |
| [`sum_list.echo`](sum_list.echo) | `10` `20` `12` then `42` | 0 |
| [`list_hof.echo`](list_hof.echo) | `3` / `2` / `4` / `2` / `2` / `6` / `7` | 0 |
| [`list_ints.echo`](list_ints.echo) | `list.sum_ints` / `sort_ints` | `6` / `1` / `3` |
| [`list_contains.echo`](list_contains.echo) | `list.contains` hit/miss/empty | `1` / `0` / `0` / `1` |
| [`bufio_read.echo`](bufio_read.echo) | `bufio.read_lines` | `2` / `b` |
| [`queue.echo`](queue.echo) | FIFO `push`/`pop` | `2` / `10` / `20` |
| [`map.echo`](map.echo) | `std/collections/map` put/get + `from_indexed` | `1` / `3` / `20` |
| [`set.echo`](set.echo) | `std/collections/set` add/has + `from_list` | `1` / `2` / `1` |
| [`hash_table.echo`](hash_table.echo) | `std/collections/hash_table` put/get | `1` / `2` / `20` |
| [`if_branch.echo`](if_branch.echo) | `10` | 0 |
| [`result_ok.echo`](result_ok.echo) | `7` | 0 |
| [`result_err.echo`](result_err.echo) | `99` | 0 |
| [`strings.echo`](strings.echo) | pure + rich + `\{` `\}` `\xHH` | `hello pure` / `hello` `rich` / `{x}` / `A` |
| [`str_text.echo`](str_text.echo) | `std/str` trim/split/replace/case/repeat | `hi` / `abc` / `AB` / `a-b-c` / `bb` / `xxx` |
| [`str_parse.echo`](str_parse.echo) | `str.parse_int` / `parse_float` | `42` / `3.5` / `parse failed` |
| [`str_debug.echo`](str_debug.echo) | `str.from_debug` | `42` / `"hi"` / `[1, 2]` |
| [`str_from_bytes.echo`](str_from_bytes.echo) | `str.from_bytes` UTF-8 lossy | `hi` / `3` / `239` / `0` |
| [`str_search.echo`](str_search.echo) | `str.contains` / starts / ends | `1` / `0` / `1` / `1` / `1` / `1` |
| [`multi/main.echo`](multi/main.echo) | multi-file `./lib` + std | `multi-file` / `42` / `42` |
| [`parent/main.echo`](parent/main.echo) | parent-relative `/ ../multi/lib` | `42` |
| [`http_chunked.echo`](http_chunked.echo) | chunked HTTP head + hex size | `Transfer-Encoding: chunked` / `ff` |
| [`store_memory.echo`](store_memory.echo) | in-memory `std/store` put/get | `200` / `v1` |
| [`const_hash.echo`](const_hash.echo) | `#` const-eval (incl. list/range/struct/field/index/defaults) | `42` / `5010ms` / `raw` / `/tmp` / `2` / `6` / `3` / `0` / `1` / `10` / `const ok` |
| [`interp.echo`](interp.echo) | rich `{name}` + `==` | `n=7!` / `eq ok` |
| [`point.echo`](point.echo) | struct lit + field R/W | `3` `4` `13` / `{x: 13, y: 4}` |
| [`anon_struct.echo`](anon_struct.echo) | structural `{ k: v }` product | `1` `2` `10` / `0` `3` |
| [`floats.echo`](floats.echo) | f64 arith + `str.from_float` | `4.5` / `6` / `5` |
| [`math.echo`](math.echo) | `std/math` abs/min/sqrt/floor/pow | `7` / `3` / `3` / `3` / `8` |
| [`math_trig.echo`](math_trig.echo) | `math.sin` / `tan` at 0 | `0` / `0` |
| [`os.echo`](os.echo) | `os.pid` / `os.platform` | `1` / `ok` |
| [`os_chdir.echo`](os_chdir.echo) | `os.chdir` `/tmp` | `tmp` |
| [`os_hostname.echo`](os_hostname.echo) | `os.hostname` nonempty | `1` |
| [`time_day.echo`](time_day.echo) | `time.format` / `time.parse` day | `2023-11-14` / `2023-11-14` |
| [`time_mono.echo`](time_mono.echo) | `time.mono_ms` nonnegative / nondecreasing | `1` / `1` |
| [`time_now.echo`](time_now.echo) | `time.now_ms` after 2020; `sleep_ms` ≤ 0 | `1` / `ok` |
| [`log_kv.echo`](log_kv.echo) | `log.kv` join | `a=1 b=2` |
| [`reflect.echo`](reflect.echo) | `std/reflect` kind names | `int` / `string` / `list` / `1` |
| [`counter.echo`](counter.echo) | methods + receiver `.` | `1` / `11` / `11` |
| [`at_method/main.echo`](at_method/main.echo) | multi-file `%` + `@` methods | `0` / `1` / `2` / `2` |
| [`match_lit.echo`](match_lit.echo) | ordinary `\|` literal match | `102` |
| [`nested_assign.echo`](nested_assign.echo) | `~ p.nested.y =` chain | `2` / `9` / `10` |
| [`list_assign.echo`](list_assign.echo) | `~ xs[i] =` list mutation | `1` / `9` / `2` / `11` |
| [`width_i32.echo`](width_i32.echo) | `<i32>` / `<i64>` width tags | `30` / `8` / `103` |
| [`width_f32.echo`](width_f32.echo) | `<f32>` / `<f64>` width tags | `3.75` / `1` / `20` |
| [`bytes.echo`](bytes.echo) | `b'…'` / `b"…"` + live `{name}` + `str.from_bytes` | `raw` / `esc\t!` / `x=3` / `1` |
| [`bytes_get.echo`](bytes_get.echo) | `std/bytes` `len` / `get` | `3` / `65` / `66` / `out of bounds` |
| [`bytes_search.echo`](bytes_search.echo) | `std/bytes` contains/starts/ends | `1` / `1` / `1` / `1` / `1` |
| [`bytes_from_int.echo`](bytes_from_int.echo) | `bytes.from_int` little-endian | `8` / `2` / `1` |
| [`bytes_from_str.echo`](bytes_from_str.echo) | `bytes.from_str` UTF-8 payload | `2` / `72` / `105` / `2` / `195` / `0` |
| [`bytes_slice.echo`](bytes_slice.echo) | `bytes.slice` half-open + oob | `3` / `98` / `0` / `out of bounds` |
| [`csv_demo.echo`](csv_demo.echo) | `csv.format_line` / `parse` | `x,y` / `2` |
| [`compress_demo.echo`](compress_demo.echo) | gzip roundtrip + zip first entry | `hello-gzip` / `a.txt` / `hi` |
| [`siphash.echo`](siphash.echo) | `hash.sip` SipHash-2-4 paper vectors | digest ints |
| [`sha512.echo`](sha512.echo) | `hash.sha512` empty digest hex | `cf83e135…da3e` |
| [`hmac.echo`](hmac.echo) | HMAC-SHA256 RFC 4231 Jefe | `5bdcc146…3843` |
| [`aes_gcm.echo`](aes_gcm.echo) | AES-256-GCM encrypt/decrypt roundtrip | `22` / `secret` |
| [`csprng.echo`](csprng.echo) | `crypto/random.fill(16)` length | `16` |
| [`utf8.echo`](utf8.echo) | strict `utf8.valid` / `decode` | `1` / `0` / `hi` / `invalid utf-8` |
| [`core_surface.echo`](core_surface.echo) | integrated core smoke | `3` / `9` / `ok=core` / … / `raw` |
| [`duration.echo`](duration.echo) | `5s` / `10ms` + add + `str.from_duration` | `5s` / `10ms` / `5010ms` / `eq` |
| [`hex_bin.echo`](hex_bin.echo) | `0x` / `0b` integer lits | `255` / `10` / `18` |
| [`bitwise.echo`](bitwise.echo) | `& \| ^ << >> ~` | `8` / `14` / `6` / `16` / `2` / `-1` |
| [`widths.echo`](widths.echo) | `i*` / `ui*` / `byte` / cast | `255` / `5` / `768` / `3` |
| [`locator.echo`](locator.echo) | `p'…'` / `p"…"` + live `{name}` + `str.from_locator` | paths + `eq` |
| [`path_rel.echo`](path_rel.echo) | `path.rel` child | `c` |
| [`path_parent.echo`](path_parent.echo) | `path.parent` root and relative | `/foo` / `/` / `.` / `/` |
| [`path_walk.echo`](path_walk.echo) | shallow `path.walk` | `hit` |
| [`struct_defaults.echo`](struct_defaults.echo) | omit fields with shape defaults | `Ada` / `0` |
| [`eq_deep_id.echo`](eq_deep_id.echo) | deep `==` vs identity `===` | `1` / `0` / `1` / … |
| [`multi_bind.echo`](multi_bind.echo) | same-line `~ a = 1, b = 2` | `3` / `30` |
| [`else_if.echo`](else_if.echo) | `?` / `: cond` / `:` chain | `two` |
| [`return_self.echo`](return_self.echo) | `^ .` keeps struct type | `1` |
| [`method_chain.echo`](method_chain.echo) | `c.inc().value()` chains | `1` / `3` |
| [`nested_fn.echo`](nested_fn.echo) | nested closed fn values | `42` |
| [`match_multi.echo`](match_multi.echo) | multi-value match arms | `hit` |
| [`match_type.echo`](match_type.echo) | `% Type` match arms for named structs | `5` |
| [`union_return.echo`](union_return.echo) | fn returns circle\|rect; match refines | `7` |
| [`first_class_fn.echo`](first_class_fn.echo) | pass fn value + call through | `42` |
| [`return_fn.echo`](return_fn.echo) | return a function value | `42` |
| [`field_fn.echo`](field_fn.echo) | fn value on struct field | `42` |
| [`range.echo`](range.echo) | inclusive `lo..hi` for-in + match | `10` / `big` |

## Limits (codegen v1)

Supported roughly: plain/`result`/`option` i64 path, binds, if, loops, list lit +
for-in + index, named struct `%` + tagged lit + structural `{ k: v }` + field
get/set (incl. `~ a.b.c =`, `~ xs[i] =`), methods with receiver `.` / `~ .field`, pure `'…'` /
rich `"…"` (escapes + `{name}` interp), string `==` / `!=` (no `+` concat),
`/ std/io` → `io.print` (**strings only**; use `str.from_int` / `str.from_float`),
multi-file user packages, `#` const-eval, f64/`<f32>` floats, bytes lits
(`b'…'` / `b"…"` with live `{name}` interp, print via `str.from_bytes`), duration lits (`5s`/`10ms`/… as
nanoseconds; print via `str.from_duration`), locator lits (`p'…'` / `p"…"` with live `{name}` interp,
print via `str.from_locator`). Top-level statements are
the program
(no entry keyword).

Not yet: HTTP/`examples/app` full kitchen-sink run, most
of `examples/algos/`.
