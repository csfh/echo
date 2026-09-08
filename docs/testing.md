# Testing Echo

| | |
|--|--|
| **Status** | **v0** (playbook + Model A `xo test` / benches) |
| **Related** | [`fixtures.md`](fixtures.md), [`implementation.md`](implementation.md), [`pipeline.md`](pipeline.md), [`stdlib.md`](stdlib.md), [`runtime-abi.md`](runtime-abi.md), [`../AGENTS.md`](../AGENTS.md) |

How to add a test, and which proof a change needs. Fixture file layout lives in
[`fixtures.md`](fixtures.md). The vertical layer list lives in
[`implementation.md`](implementation.md).

Proof is **feature-and-vertical**. Each language form is driven through the
deepest stage we claim (lex, parse, check, or run). Crate tests cover the crate
that owns the rule. Hosts (`xo`, LSP, REPL, wasm, www) present that result.

---

## Four proofs

A language, runtime, std, or CLI behavior change updates the applicable proofs
in the **same** change (or stacked PR). Policy: [`../AGENTS.md`](../AGENTS.md).

```text
crate tests          pure logic of one crate
echo26 / e26         black-box language contract (candidate binary)
examples/            human-runnable demos stay accurate
xo test + std/test   Echo-written suites (std + user libs)
```

| Proof | Lives in | Run with | Owns |
|-------|----------|----------|------|
| **Crate tests** | `crates/<name>/` `#[test]` (and `crates/xo/tests/`) | `cargo test -p <crate>` or `scripts/gate <layer>` | Decode, unify, resolve edge, pretty-print, LSP protocol, cache keys |
| **Echo 2026** | `echo26/<area>/<feature>/<NNN>_<slug>.echo` + sibling goldens | `scripts/gate echo26` / `just e26` | User-visible lex / ast / check / run against `xo` (or any candidate) |
| **Examples** | `examples/misc/`, `examples/app/`, `examples/algos/` | `scripts/gate examples` or `xo run` / `xo check` of the touched demo | Copy-paste programs stay current |
| **`xo test`** | `std/**/*.echo` `test.it` / `*_test.echo` / `tests/` | `scripts/gate std-test` / `just std-test` / `xo test std` | Std and userland assertions (Model A registration) |

Benches (`xo test --bench`, `just std-bench`) measure. The language contract is
echo26.

Each proof covers a different surface. Ship every row that applies. Crate tests
stay in the crate. Echo 2026 fixtures stay in `echo26/`. Demos stay under
`examples/`. `xo test` stays for Echo-written suites.

---

## Which test to write

| What changed | Write |
|--------------|--------|
| Pure Rust helper / table / algorithm in one crate | Crate unit test in that crate. Stop if the language surface is unchanged. |
| Token, parse form, diagnostic, check rule, runtime meaning, CLI stage flag | Crate test for the new logic. echo26 fixture (happy path, plus a reject fixture when the rule is a hard error). Example if a human would run or copy it. |
| Multi-file import / `%` `@` merge | `echo26/multi` or `echo26/run/multi` (unnumbered support files). Resolver crate tests for graph edges. |
| `std/**/*.echo` API | Co-located `test.it` (`xo test`). `echo26/run/<area>` smoke when the API is language-visible. Crate test in `echo_runtime` / `echo_codegen` only when a new native symbol landed. |
| Host only (LSP method, REPL buffer, wasm playground, www page) | Crate or npm test for that host. If the host change depends on new language meaning, also do crate + echo26 + examples. |
| Docs only | www scripts when public Spec / Reference text moved. |

Red-green: write the failing crate test or echo26 fixture first. Confirm it fails
for the intended reason. Then implement.

Test the **earliest crate that owns the rule**. Runtime print belongs in
`echo_runtime` and echo26 run fixtures. Prefer table-driven cases for kinds,
codes, and small graphs.
`echo_pipeline` tests the shared `analyze` / lower contract.

`e26`, `echo_codegen_abi`, and `echo_reflection` are thin today. Add tests when
those crates gain logic.

---

## Crate tests

Put `#[cfg(test)]` next to the logic (`echo_lexer` leaders, `echo_semantics`
infer, `echo_resolver` graph, `echo_runtime` scope, `echo_lsp` session).

```rust
#[test]
fn dual_use_star_is_not_a_leader() {
    let lexed = lex_str("$ x = 2 * 3\n");
    assert!(lexed.diagnostics.is_empty());
    // assert token kinds…
}
```

Prove: `cargo test -p <crate>` or `scripts/gate lexer`, `semantics`, and so on.

---

## Echo 2026 fixtures

One small numbered file is one behavior. Layout and candidate protocol:
[`fixtures.md`](fixtures.md).

```text
echo26/<area>/<feature>/<NNN>_<slug>.echo     source (required)
echo26/<area>/<feature>/<NNN>_<slug>.lex      token kinds (required)
echo26/<area>/<feature>/<NNN>_<slug>.ast      kind tree (required)
echo26/<area>/<feature>/<NNN>_<slug>.diag     lex codes (omit ⇒ none)
echo26/<area>/<feature>/<NNN>_<slug>.check    sem-* / res-* (omit ⇒ none)
echo26/<area>/<feature>/<NNN>_<slug>.run      xo run stdout (opt-in execute)
echo26/<area>/<feature>/<NNN>_<slug>.runexit  process exit (opt-in)
echo26/<area>/<feature>/<NNN>_<slug>.stdin    optional bytes piped to `xo run` (absent ⇒ /dev/null)
```

Only `NNN_*.echo` files are suite roots. Sibling files such as `user.echo` are
imports.

Happy path (run): `echo26/run/bind/001_multi.echo` plus `.run` with expected
prints. Reject path (check): `echo26/check/bind/001_shadow.echo` plus `.check`
containing `sem-shadow`.

1. Write the tiny `.echo` (cite `docs/syntax.md` / `semantics.md` / public Spec).
2. `cargo build -p xo -p e26`
3. `e26 --binary target/debug/xo --filter <area>/<feature>/<NNN> --update`
4. Review the goldens. Unexpected tokens or extra `sem-*` codes mean the
   implementation or the fixture is wrong.
5. `scripts/gate echo26` (or `just e26`).

`e26` always runs **lex + ast + check**. **run** only if `.run` / `.runexit`
exists. That execute path is `xo run` (AOT + clang + `libecho_runtime`).

Prefer extending an existing area (`lex/`, `lits/`, `leaders/`, `parse/`,
`check/`, `infer/`, `effect/`, `multi/`, `run/…`) over a new top-level folder.

---

## Examples

If a user would `xo run` the program, keep a demo under `examples/misc/` (tiny),
`examples/app/` (kitchen sink / HTTP), or `examples/algos/` (classic algorithms).
Examples are not goldens. They must stay runnable.

`scripts/gate examples` checks and runs the finite entries. Skip
`examples/app/server.echo` in that gate: that process listens until you stop
it. Dirty support modules (`user.echo`, `multi/lib.echo`, …) map to their
package entry.

```bash
./target/debug/xo run --no-cache examples/misc/hello.echo
./target/debug/xo run --no-cache examples/app/surface.echo
scripts/gate examples
```

---

## Vertical map

Use [`implementation.md`](implementation.md) as the layer list. For a new
language form:

| Layer | Proof |
|-------|--------|
| Spec | Rule in `www` Spec / `docs/syntax.md` (or the layer doc). |
| `echo_syntax` / lexer | Crate test for the token / leader / dual-use case, and echo26 `.lex` / `.diag`. |
| parser / ast | Crate test for the tree shape, and the required `.ast`. |
| index / resolver | Crate tests for graph / merge / export, and `echo26/multi/**` or `run/multi`. |
| semantics | Crate tests for the rule, and `.check` (omit the file when no `sem-*`). Add a reject fixture when the feature is a hard error. |
| HIR / MIR / codegen / runtime | Crate tests for lowering / ABI / values, and opt-in `.run` / `.runexit` when we claim Run. |
| CLI | `xo` flags used by `e26` (`lex` / `ast` / `check` / `run --diag-codes`). |
| fmt | `echo_ast` pretty unit tests (idempotence). No echo26 fmt stage yet. |
| LSP | `echo_lsp` crate tests over `echo_pipeline::analyze`. |
| REPL | `crates/xo/tests/repl_forms.rs` (JIT). Language meaning stays in e26 AOT. |
| wasm / www | `cargo test -p echo_wasm`; `scripts/gate web`. Playground run is a host demo. |
| std | `test.it` plus `echo26/run/<pkg>` when the API is user-visible. |
| examples | Touched demo still runs. |

Parse-only work still needs `.ast`. Check-only still needs e26 check. Claiming
Run without a `.run` fixture is incomplete.

A feature is **suite-complete** when
[`implementation.md`](implementation.md) §7 holds: spec matches code, the
shared pipeline implements meaning through the deepest claimed stage,
diagnostics have stable codes (and a reject fixture for hard errors),
`e26 --binary xo` is green, touched crate tests are green, touched examples
and `xo test` suites pass, and hosts only present the rule.

Roadmap §7 is the feature × layer honesty matrix. Keep it current when a
vertical lands.

---

## Gate

`scripts/gate` is the focused dispatcher. See
[`development-speed.md`](development-speed.md).

| Command | What it proves |
|---------|----------------|
| `gate changed` | Dirty files → the smallest useful checks (crate layer, echo26, std-test, examples, web, docs, tools) |
| `gate echo26` | Build `xo` + `e26`, run the Echo 2026 suite. PR hard gate. |
| `gate std-test` | `xo test std` (AOT). Dirty `std/**/*.echo` routes here. |
| `gate examples` | `xo check` / `xo run` of finite example entries. Dirty `examples/**` routes here. |
| `gate <crate-layer>` | `cargo test -p echo_*` (`lexer`, `semantics`, `std` = `echo_std` crate, …) |
| `gate workspace` | rustfmt + `cargo check` + nextest/workspace **Rust** tests |
| `gate web` | www lint / format / docs+prose+std-ref scripts / build |

PR CI (`docs/ci.md`) runs a subset of `cargo test -p xo` (non-JIT),
`xo run examples/misc/hello.echo`, and `gate echo26`. REPL / JIT tests stay
off in CI while `run_jit_ir` SIGSEGV under those LLVM loads is open.

`e26` execute is AOT (`xo run`). `xo run --jit`, REPL, and wasm playground-run
share `echo_runtime_*` symbols and use different hosts.

---

## Everyday loop

```bash
# 1. crate that owns the new logic
cargo test -p echo_semantics

# 2. fixture + goldens
e26 --binary target/debug/xo --filter check/bind --update   # review the diff
scripts/gate echo26

# 3. std / examples if those surfaces moved
xo test std/math.echo
./target/debug/xo run --no-cache examples/misc/hello.echo

# 4. dirty-file dispatcher
scripts/gate changed --explain
scripts/gate changed
```

Do not `--update` the whole suite unless the change is suite-wide on purpose.

---

## `xo test` + `std/test` (Model A)

User and std suites written in Echo. e26 goldens stay in `echo26/`.

### Model A (locked for v0)

1. Suite files call **`std/test`** helpers at top level (e.g. `test.it`, `test.bench`).
2. Those helpers register cases via **`runtime.test_register`** /
   **`runtime.test_bench_register`** (std only).
3. Registration is active only when the host sets **`XO_TEST`** (done by `xo test`).
4. After the entry top-level body, **`runtime.test_finish`** runs the selected
   kind of cases and becomes the process exit status (failure count). Under
   `xo run`, finish returns “suite off” so normal programs are unchanged.
5. **`xo test`** runs only `test.it` cases. **`xo test --bench`** sets
   **`XO_BENCH`** and runs only `test.bench` cases (harness-looped auto-N).

## `std/test`

```echo
/ std/test

test.it("name", () {
    test.eq(1, 1)
    test.true(|)
})

test.bench("hot_path", () {
    $ n = abs_i(-3)
    test.eq(n, 3)
})
```

Bodies may also be named binds (`$ case = () { … }; test.it("n", case)`).
| Export | Role |
|--------|------|
| `it(name, body)` | Register a zero-arg function value as a test case |
| `bench(name, body)` | Register a zero-arg body; harness calls it N times |
| `eq` / `ne` | Assert deep equality |
| `true` / `false` | Assert bool |
| `fail(msg)` | Mark current case failed |

Custom libraries may wrap these; only privileged std talks to `/ runtime`.

### Benchmarks

- Body is **zero-arg**; the runtime scales **N** until one measured run lasts
  about **1s** (capped), then reports `N` and **ns/op**.
- Assertions (`eq` / `fail`, …) still work inside a bench body; a failed assert
  fails the bench.
- Co-located next to production or tests (same file as `test.it` is fine).
- Registration is a no-op without `XO_TEST`; benches run only with `XO_BENCH`.
- Prefer **no asserts in the hot body** so `ns/op` measures the work, not `test.eq`.

### Std + algorithm benches

Benches call **real** functions (same bodies as demos / std), with args built
in the bench body. No separate “canary wrapper” module.

| Location | Examples |
|----------|----------|
| `std/list` | `sum_ints_1k`, `sort_ints_1k` |
| `std/bytes` | `checksum_1k`, `checksum_64k` |
| `std/crypto/hash/sip` | `sip_empty` … `sip_64k` |
| `std/crypto/hash/sha256` | `sha256_empty`, `sha256_1k` |
| `std/collections/map` | `seed_1k_get` |
| `examples/algos/*` | `fib`, `gcd`, `sum_to`, sorts, primes, call overhead |

```bash
just bench-host
just std-bench          # all std co-located benches
just algo-bench         # algorithms + selected std hot paths
xo test --bench -O2 std --bench-out .xo/bench/last.jsonl
xo test --bench -O2 std --bench-out .xo/bench/last.jsonl \
  --bench-baseline .xo/bench/baseline.jsonl --bench-threshold 20
```

Example output:

```text
bench abs_i  N=25000000  40ns/op  (1000000000ns)
xo test --bench: 1 passed, 0 failed, 1 total (…)
```

### Recording and comparing results

While benches run, each finished case can be **appended as JSONL** (one object
per line) when `--bench-out PATH` is set. The file is truncated at the start of
the run, then grown as cases complete (safe to `tail -f`).

| Flag | Role |
|------|------|
| `-O` / `--opt-level LEVEL` | LLVM opt for suite compile: `0`/`1`/`2`/`3`/`z` (same as `xo run`). Default `0`. Prefer **`-O2`** for benches. |
| `--bench-out PATH` | Stream JSONL results (requires `--bench`) |
| `--bench-baseline PATH` | Prior JSONL to compare after the run (needs `--bench-out`) |
| `--bench-threshold PCT` | Exit 1 if any case is **worse** than baseline by more than `PCT`% `ns/op` |

JSONL fields: `v`, `file`, `name`, `opt` (`O0`…`Oz`), `status` (`ok`/`fail`),
and on success `n`, `ns_per_op`, `total_ns`.

Compare keys are `file::name@opt` so O0 and O2 baselines do not mix.

Compare lines look like:

```text
  REG  std/math.echo::abs_i@O2  1000 → 1300 ns/op  (+30.0%)
  IMP  std/str.echo::cat@O2     2000 → 1800 ns/op  (-10.0%)
  NEW  std/list.echo::sum@O2    500ns/op
summary: 1 regression(s), 1 improvement(s), 1 new, 0 gone
```

Save a local baseline after a good run:

```bash
xo test --bench -O2 std --bench-out .xo/bench/last.jsonl
cp .xo/bench/last.jsonl .xo/bench/baseline.jsonl
# later:
xo test --bench -O2 std --bench-out .xo/bench/last.jsonl \
  --bench-baseline .xo/bench/baseline.jsonl --bench-threshold 20
```

Keep the **opt level fixed** when comparing. Absolute `ns/op` is not portable
across machines; use relative deltas on the same recipe.

### Prebuilt host + warm suite cache

- **Host:** run a prebuilt `xo` (`target/debug/xo` or release). Do not fold
  `cargo build -p xo` into every bench iteration. That measures the Rust
  toolchain, not Echo.
- **Suite files:** each `.echo` is still compiled to an AOT child, but **IR and
  AOT artifact caches** (under `.xo/cache/`) make the second run mostly load +
  exec. Use `--cache-status` to confirm `hit`; use `--no-cache` only for cold
  pipeline experiments.
- **`just std-bench`** uses a prebuilt `XO` binary and passes `--cache-status`.

## `xo test` CLI

```bash
xo test                          # ./ *_test.echo and tests/** (+ std/ co-located suites)
xo test path/to/file.echo
xo test path/to/dir
xo test '**/*_test.echo'
xo test a_test.echo tests/
xo test std                      # co-located std suites (`test.it`)
xo test --bench                  # only files with test.bench (default discovery)
xo test --bench std              # all co-located std benchmarks
xo test --bench std/math.echo
```

- Each matched **file** is compiled and executed as a suite entry with `XO_TEST=1`.
- With **`--bench`**, also `XO_BENCH=1`. Only benchmarks run; `test.it` is skipped.
  Discovery skips files that do not contain `test.bench(`.
- Without **`--bench`**, only `test.it` runs; registered benches are ignored.
- Paths: files, directories (walk), or simple globs (`*`, `**`, `?`).
- Directory convention:
  - `*_test.echo` anywhere
  - all `.echo` under `tests/`
  - under a `std/` path: co-located suites that call `test.it(` / `test.bench(`

## Co-located vs separate files

Either is allowed:

- **Separate** `map_test.echo` next to production (recommended for libs)
- **Same file** with top-level `test.it(...)` / `test.bench(...)`. Cases run
  only under `xo test` / `xo test --bench`, not under `xo run` (registration
  no-op without `XO_TEST`)

Production APIs still use normal `\ ` exports for importers.

## Out of this layer

- e26 goldens (use `echo26/` + `e26`)
- Magic test keywords
- Userland `/ runtime` test hooks
- Fuzzing, allocation counters, or statistical (Criterion-style) benches (later)
