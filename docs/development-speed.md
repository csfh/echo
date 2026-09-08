# Development Speed

The default local loop should answer the smallest useful question first.

Related: [`AGENTS.md`](../AGENTS.md), [`architecture.md`](architecture.md),
[`fixtures.md`](fixtures.md), [`testing.md`](testing.md).

## Tooling profile

This repository expects the Linux development profile in `.cargo/config.toml`:

- `sccache` as the `rustc` wrapper
- `clang` as the linker driver (`xo run` / `build` need `libecho_runtime.a`;
  if AOT reports a missing `echo_runtime_*` symbol, `cargo build -p echo_runtime`)
- `mold` via `-fuse-ld=mold`
- high default `jobs` (override with `CARGO_BUILD_JOBS` on smaller hosts)

Workspace profiles:

- **dev:** incremental on, limited debug info
- **test:** incremental off (avoids unbounded per-test-binary incremental trees);
  sccache still reuses compilation work

Install on Arch Linux:

```bash
sudo pacman -S --needed mold clang sccache just
cargo install cargo-nextest --locked
```

Confirm tools:

```bash
just tools
# or
scripts/gate tools
```

## Git hooks (warnings as errors)

Versioned hooks live in [`.githooks/`](../.githooks/). Install once per clone:

```bash
scripts/install-hooks.sh
# or: just hooks
```

That sets `core.hooksPath=.githooks`. **`pre-commit`** runs
`cargo check --workspace` with `-Dwarnings` when the staged set includes
Rust/Cargo (or the hooks themselves). Pure docs / `std/**/*.echo` /
`www/` commits skip the check.

- Manual equivalent: `just check-deny`
- Emergency skip: `git commit --no-verify`

## Cache / incremental (infra)

```bash
cargo test -p echo_fingerprint -p echo_cache -p echo_build
./target/debug/xo cache doctor
./target/debug/xo cache status
./target/debug/xo cache gc
./target/debug/xo cache clean
```

Design and milestones: [`incremental.md`](incremental.md). **Only `.xo/` trees**
(no `echo/…` dirs). [ADR 0014](adr/0014-modules-packages-paths.md):

| Path | Role |
|------|------|
| `{project}/.xo/cache/` | IR / check / AOT artifacts (fingerprint keys) — **not** package downloads |
| **`$XO_HOME`** | User `.xo` root: `$XO_HOME` → else `$XDG_CACHE_HOME/.xo` → else `~/.cache/.xo` |
| `$XO_HOME/packages/<id>/<version>/` | **Package cache** — always install here (`xo get` / deps) |

```bash
# User package root (override with XO_HOME)
./target/debug/xo home

# Install from local tree into the cache (tests / path packages)
./target/debug/xo get github.com/acme/lib@v1 --path ./my_lib

# Install from git (branch/tag = version)
./target/debug/xo get github.com/csfh/echo-pkg@v0.1.0

# Install package + deps listed in its xo.toml
./target/debug/xo get github.com/acme/lib@v1 --path ./my_lib --deps
```

Cache dirs under `.xo/cache/`:

| Phase dir | Used by |
|-----------|---------|
| `parse/` | resolve / check / run (v2) |
| `check/` | `xo check` + compile front-end (v1) |
| `codegen/` | LLVM IR (v3) + AOT binaries (v4, distinct keys) |

`--no-cache` and `--cache-status` on check, run, ir, and build (`aot cache` on run).

**When to use `--no-cache`:** after changing `std/**/*.echo`, `echo_runtime`
ABI, or task/net surface, stale parse/check/codegen artifacts can mask new
symbols or wrong std. Prefer:

```bash
./target/debug/xo run --no-cache path/to/file.echo
# or
./target/debug/xo cache clean
```

`STDLIB_VERSION` / `RUNTIME_ABI_VERSION` in `echo_fingerprint` should bump when
those surfaces change so healthy cache keys diverge; `--no-cache` is the
escape hatch when in doubt.

```bash
cargo build -p xo
./target/debug/xo fmt path.echo          # print canonical form
./target/debug/xo fmt -w path.echo       # write in place (--write)
./target/debug/xo fmt -c path.echo       # check only (--check); exit 1 if dirty
./target/debug/xo lsp
# tree-sitter package from echo_syntax facts (see docs/tree-sitter.md):
./target/debug/xo tools grammar tree-sitter -o /tmp/tree-sitter-echo
# interactive REPL (shared pipeline + JIT; see docs/repl.md):
./target/debug/xo repl
```

## App samples

```bash
./target/debug/xo run --no-cache examples/app/main.echo    # finite + live TCP
./target/debug/xo run --no-cache examples/app/server.echo  # long-running
./target/debug/xo run --no-cache examples/app/surface.echo
scripts/gate echo26
```

## Normal loop

```bash
scripts/gate changed --list
cargo test -p crate_you_touched
scripts/gate changed
```

**Language surface / frontend / runtime meaning** — always include the suite
(which proof to write: [`testing.md`](testing.md)):

```bash
cargo build -p xo -p e26
# after intentional expectation changes:
# e26 --binary target/debug/xo --update
scripts/gate echo26
# or
just e26
# when std/ or examples/ moved:
scripts/gate std-test
scripts/gate examples
```

Full quiet gate before broad commits:

```bash
scripts/gate workspace
# or
just test-full
```

Formatting:

```bash
just fmt
just fmt-check
# equivalent: cargo fmt --all / cargo fmt-check
```

## Browser check host (`just wasm`)

The site playground (`/try`) runs the shared frontend (lex → parse → resolve →
semantics) as `wasm32-unknown-unknown`, then a playground run executes checked
MIR and captures `io.print`. It does not ship LLVM. Playground run is a host
demo; native compile and run stay on `xo`. Filesystem, net, process, and tasks
fail with a playground-host error. Bindings stay in `www/public/echo-wasm/`.

```bash
just wasm                 # echo_wasm + wasm-bindgen → www/public/echo-wasm/
just try                  # wasm, then npm --prefix www run dev
cargo test -p echo_wasm   # native tests of check/fmt/playground-run helpers
scripts/gate wasm
```

Needs `wasm32-unknown-unknown` (added on first run) and `wasm-bindgen-cli`
matching the `wasm-bindgen` crate version. Output is generated; rebuild after
frontend or `std/**/*.echo` changes.

## Gate

`scripts/gate` is the focused verification dispatcher:

| Command | Purpose |
|---------|---------|
| `gate changed` | Map dirty files to the smallest useful checks |
| `gate changed --list` | Show derived checks without running them |
| `gate changed --explain` | Show checks with routing reasons |
| `gate workspace` | fmt + check + full nextest/workspace tests |
| `gate <layer>` | Focused crate or pipeline check |
| `gate echo26` | Echo 2026 suite against `target/debug/xo` |
| `gate std-test` | `xo test std` (AOT) |
| `gate examples` | Finite example `xo check` / `xo run` |
| `gate tools` | Assert host toolchain pieces are present |

Useful environment variables:

| Variable | Purpose |
|----------|---------|
| `GATE_ECHO_COMMANDS=1` | Print each gate command |
| `GATE_MAX_OUTPUT_LINES` | Bound failure replay (`0` = full) |
| `CARGO_BUILD_JOBS` | Cap parallel compile jobs |

## Measuring compile vs runtime

```bash
time cargo check --workspace
time cargo test --workspace --no-run
time cargo nextest run --workspace
```

Interpretation:

- slow `--no-run` → compile or link is the bottleneck
- fast `--no-run` but slow tests → test execution is the bottleneck
- nextest much faster than `cargo test` → prefer nextest for full runs

## just recipes

```bash
just check
just test echo_parser
just test-fast
just test-full
just fmt
just fmt-check
just profile
just sccache
just tools
just gate changed --explain
just examples            # finite example xo check / xo run
just std-test            # xo test std (needs prebuilt XO)
```
