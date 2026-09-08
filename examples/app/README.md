# `examples/app/`

Sample Echo programs that exercise the language surface and a small HTTP app.

| File | Role |
|------|------|
| [`surface.echo`](surface.echo) | Kitchen sink — full surface + `user` / `user_extra` |
| [`main.echo`](main.echo) | Finite HTTP demo: in-process dispatch **+ live TCP smoke** |
| [`server.echo`](server.echo) | **Long-running** HTTP server (`http.serve`) |
| [`config.echo`](config.echo) | `#` constants + export |
| [`user.echo`](user.echo) | `% user` shape + methods |
| [`user_extra.echo`](user_extra.echo) | `@ user` extra members |
| [`users.echo`](users.echo) | free helpers + `user.user { … }` lits |
| [`routes.echo`](routes.echo) | handlers as function values |

```bash
cargo build -p xo
./target/debug/xo run --no-cache examples/app/surface.echo
./target/debug/xo run --no-cache examples/app/main.echo
./target/debug/xo run --no-cache examples/app/server.echo   # blocks; curl /health
./target/debug/xo check examples/app/main.echo
```

Imports are **module-scoped**: `/ std/io` → `io.log`, not bare `log`.
See `docs/modules.md`.

## What `main.echo` does

1. Builds routes and **dispatches sample GETs** in-process (no sockets).
2. Listens on `127.0.0.1:18080`, serves **one** real connection via
   `http.handle_connection` in a task (`+ () [lis, table] { … }`), client
   `GET /health`, then joins and exits (finite; e26/CI friendly).

## What `server.echo` does

Calls `http.serve("0.0.0.0:8080", routes)` — nonblocking accept loop, one
task per connection (`+ handle_connection(conn, routes)`).

```bash
# terminal 1
./target/debug/xo run --no-cache examples/app/server.echo

# terminal 2
curl -s http://127.0.0.1:8080/
curl -s http://127.0.0.1:8080/health
curl -s http://127.0.0.1:8080/users
```

## Surface coverage (`surface.echo`)

| Area | Exercised |
|------|-----------|
| Leaders `~ $ # % @` | binds, shape, `@` extra members |
| Leaders `? :` | if / else-if / else |
| Leaders `* < >` | loops |
| Leaders `\|` | match |
| Leaders `! ^` | result / return |
| Leaders `/ \` | import / export |
| Leaders `+ -` | tasks (also in `main.echo` live smoke) |
| Free fns / methods | counters, `user` |
| Lits / collections / ops | kitchen sink |

When you lock a new language feature, extend **`surface.echo`** (or a sibling)
and keep `xo run` green for `surface.echo` and `main.echo`.
