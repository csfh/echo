# `examples/`

Sample Echo programs that exercise the language and toolchain.

| Path | Role |
|------|------|
| [`misc/`](misc/) | **Tiny programs for `xo run`** (codegen v1 demos) |
| [`app/`](app/) | Kitchen-sink surface + multi-file HTTP demo (`xo check`) |
| [`algos/`](algos/) | Classic algorithms (mostly `xo check` until surface expands) |

```bash
cargo build -p xo

./target/debug/xo run examples/misc/hello.echo
./target/debug/xo run examples/misc/multi/main.echo
./target/debug/xo run examples/misc/sum_list.echo
./target/debug/xo run examples/app/surface.echo
./target/debug/xo run examples/algos/factorial.echo
./target/debug/xo run examples/algos/sort.echo
```

Imports use **module scope**: `/ std/io` → `io.print`, not bare `print`.

See [`misc/README.md`](misc/README.md), [`app/README.md`](app/README.md), [`algos/README.md`](algos/README.md).
