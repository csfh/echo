# REPL

Interactive host over the **shared pipeline** + **LLVM JIT**.

| | |
|--|--|
| **Status** | **v0** — `xo repl` (always in `xo`) |
| **Owners** | `crates/xo/src/repl.rs` |
| **Related** | [`pipeline.md`](pipeline.md) §7, [`development-speed.md`](development-speed.md) |

## Design

Same rules as `xo run --jit` — no private interpreter:

1. Buffer input (multi-line while `{` depth > 0; `rustyline` validator).
2. Build a temporary program: **session chunks** + current input.
3. `echo_pipeline::compile_to_llvm` → `echo_codegen::run_jit_ir`.
4. On success for **statements**, append the chunk to the session (including
   a `+` spawn that exits non-zero for unjoined tasks, so a later `-` can join).

Each evaluation re-JITs the full cumulative session source. After every JIT
run the runtime drains task workers and resets process-global unjoined state
so the next in-process eval is safe.

## Supported forms

These program forms work under interactive and **piped** (non-TTY) `xo repl`:

| Form | Notes |
|------|--------|
| Binds `$` / `~` / const `#` | Persist in the session |
| Bare expressions | Auto-display for int/float/bool/string/struct/list/bytes/duration/range |
| Multi-line functions | Brace-buffered; call as bare expr to display |
| Control `?` / `:` / `*` / `<` / `>` | Multi-line blocks |
| Match `\|` | Value, range, and result/option arms |
| Lists and ranges | Literals, index, for-in |
| Pure / rich strings | `'…'` and `"…{name}"` |
| Imports `/ std/…` | Then `module.export` calls (e.g. `io.print`) |
| Structs `%` / `@` | Field access; methods; mutation via `~` statements |
| Result / option | `!` / bare `^` produce; required match |
| Tasks `+` / `-` | Spawn then join in the session; later lines re-run both |

### Intentional limits

- Bare-expression auto-display does not cover every type; use `io.print` /
  `str.from_*` for other shapes.
- Side-effect bare calls such as `io.print(...)` run but are **not** stored in
  the session (they would re-fire on every later eval).
- Mutations performed only as bare expressions (e.g. `p.bump()` as a display
  line) are not added to the session; use `~` statements so later lines see
  the change when the cumulative program re-runs.
- A session that stores a `+` without a matching `-` will re-report unjoined
  tasks on later evaluations until a join is entered (or `:clear`).
- Multi-file packages and long-running accept loops are not first-class REPL
  products.

## Run

```bash
cargo build -p xo
./target/debug/xo repl
```

| Meta | Action |
|------|--------|
| `:help` / `:?` | help |
| `:session` | print accumulated source |
| `:clear` | clear session |
| `:quit` / `:exit` / Ctrl-D | leave |

**Inline hints** (rustyline `Hinter`): a dim suffix when the cursor is at
end-of-line.

| Source | Example | Right arrow |
|--------|---------|-------------|
| **Eager eval** (int bare expr) | `5 + 3` → dim `  → i64 8`; `$ a = <i32> 5` then `a` → `  → i32 5` | does **not** insert |
| Meta commands | `:hel` → dim `p` | accepts / inserts |
| History | retype prefix of an earlier line | accepts / inserts |

Eager eval uses the **shared pipeline + JIT** (same as Enter on a bare expr),
with `runtime.print` captured via `echo_runtime::with_print_capture` so the
preview does not spam the terminal. Kind label comes from
`echo_semantics::infer_last_expr_type` (`i32` / `i64` / …). Incomplete /
failing exprs stay silent. Session binds are included
(`$ x = 40` then `x + 2` → `i64 42`).

History file (XDG): `$XDG_STATE_HOME/xo/history`  
(default `~/.local/state/xo/history`).

## Behavior notes

- **Bare expressions** (single `Stmt::Expr`) are displayed by inferred kind:
  - `i32` / `i64` → `io.print(str.from_int((…)))`
  - `f32` / `f64` → `io.print(str.from_float((…)))`
  - `string` → `io.print((…))`
  - `bool` → `|` / `_` glyphs via `?` / `:`
  - named/anon struct, list, bytes, duration, range → `io.print(str.from_debug((…)))`
  - other kinds: execute as a statement if possible, or use `io.print` /
    `str.from_*` explicitly.
- **Statements** (`$ x = …`, imports, structs, tasks, …) persist in the session
  and re-JIT on every subsequent input.
- Diagnostics match the shared check pipeline (`sem-*`, `res-*`, …).
- Temp sources live under `<workspace>/.xo/repl/` so `/ std/…` resolves.

## Piped / tests

Non-TTY stdin runs without an interactive editor (good for scripts and tests).

```bash
printf '%s\n' '$ x = 1' 'x + 2' ':quit' | ./target/debug/xo repl
```

```bash
# Unit + integration (real binary):
cargo test -p xo
```
