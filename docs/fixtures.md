# Fixtures (echo26) and runner (e26)

**Echo 2026** is the language edition and canonical public Language Spec
(ADR 0015). The **executable contract** of that edition is this suite:
**implementation-agnostic** `e26` drives a **candidate binary** over `echo26/`
(same idea as pointing a JS suite at different runtimes).

| | |
|--|--|
| **Edition / public Spec** | **Echo 2026** (`www` `/e26`, ADR 0015) |
| **Suite** | [`echo26/`](../echo26/) |
| **Runner** | `e26` (`crates/e26`) |
| **Reference candidate** | `xo` |
| **Related** | [`testing.md`](testing.md) (which proof to write), `docs/lexer.md`, `docs/parser.md`, `docs/roadmap.md`, `../AGENTS.md` |

## Mandatory on every language implementation

Whenever you add or change language behavior (syntax, lex, parse, check, run,
diags, CLI stage flags, runtime/std):

### Three proofs (all required)

| Proof | Requirement |
|-------|-------------|
| **Crate tests** | Unit/integration tests in **each touched crate** stay green and cover new logic |
| **echo26 / e26** | Add/adjust fixtures; keep the suite green |
| **Examples** | Update `examples/misc/` (runnable), and `examples/app` / `algos` when relevant |

Each proof covers a different surface; ship every row that applies. Policy:
[`../AGENTS.md`](../AGENTS.md). Which test to write: [`testing.md`](testing.md).

### echo26 steps

1. **Add or extend** `echo26/<area>/<feature>/<NNN>_*.echo` cases (keep them small).  
2. **Update expectations** (`.lex` / `.ast` / `.diag` / `.check` / `.run` / `.runexit`) — usually  
   `e26 --binary target/debug/xo --update` then review the diff.  
3. **Extend `e26` / `xo` protocol** if the suite needs a new stage or flag.  
4. **Run** `scripts/gate echo26` (or `just e26`) and leave it green.

Agents and humans treat a PR without **crate tests + e26 + examples** (as
applicable) as incomplete for language/runtime work.

## Run

```bash
cargo build -p xo -p e26
cargo run -p e26 -- --binary target/debug/xo
# or
scripts/gate echo26
```

```bash
e26 --binary /path/to/other-echo
e26 --binary target/debug/xo --filter leaders/bind
e26 --binary target/debug/xo --update
```

## Layout

```text
echo26/<area>/<feature>/<NNN>_<slug>.echo   # source
echo26/<area>/<feature>/<NNN>_<slug>.lex    # expected token kinds
echo26/<area>/<feature>/<NNN>_<slug>.ast    # expected AST kinds (**required**)
echo26/<area>/<feature>/<NNN>_<slug>.diag     # expected lex diagnostic codes (optional)
echo26/<area>/<feature>/<NNN>_<slug>.check    # expected `sem-*` codes (optional; absent ⇒ none)
echo26/<area>/<feature>/<NNN>_<slug>.run      # expected `xo run` stdout (optional; opt-in)
echo26/<area>/<feature>/<NNN>_<slug>.runexit  # expected process exit code, one integer (optional; opt-in)
echo26/<area>/<feature>/<NNN>_<slug>.stdin    # optional bytes piped to `xo run` stdin (absent ⇒ `/dev/null`)
```

One fixture = one small behavior. Prefer many tiny files.

**Parse stage:** every `.echo` must have a matching `.ast` (empty when
lex/parse emits no tree — e.g. unexpected glyph, unterminated string).  
**Check stage:** every fixture is run through `check`; `.check` lists expected
`sem-*` codes (or omit the file if none).  
**Run stage:** only when `.run` and/or `.runexit` is present. Executes
`xo run` (LLVM IR → clang → `libecho_runtime`). Program body is the entry
file’s top-level statements (see `docs/llvm.md`).

## Candidate protocol

### Lex

```text
$bin lex --kinds --diag-codes <file.echo>
```

| Stream | Content |
|--------|---------|
| stdout | Token kinds → `.lex` |
| stderr | Diagnostic codes → `.diag` (optional) |
| exit 0 / 1 | Compared either way; ≥2 is tool failure |

### Ast

```text
$bin ast --kinds --diag-codes <file.echo>
```

| Stream | Content |
|--------|---------|
| stdout | Kind tree → `.ast` (**required**) |
| stderr | Not compared yet (lex owns `.diag`) |
| exit 0 / 1 | Compared either way; ≥2 is tool failure |

Pipeline for `ast`: `echo_source` → `echo_lexer` → `echo_parser` (chumsky) → `echo_ast`.

### Check

```text
$bin check --diag-codes <file.echo>
```

| Stream | Content |
|--------|---------|
| stdout | unused |
| stderr | `sem-*` codes only → `.check` (optional) |
| exit 0 / 1 | Compared either way; ≥2 is tool failure |

Pipeline: … → `echo_parser` → **`echo_semantics`**.

### Run (optional)

```text
$bin run --diag-codes <file.echo>
```

| Stream | Content |
|--------|---------|
| stdout | Program output → `.run` (if file present) |
| stderr | Compile/tool diags (not fixture-compared yet) |
| stdin | `.stdin` sidecar if present; else `/dev/null` |
| exit | Process status → `.runexit` (if file present) |

Pipeline: check → HIR → MIR → **LLVM IR** → **clang** + **`libecho_runtime`** → exec.

Absent `.run` and `.runexit` ⇒ execute stage skipped for that fixture.

### `.lex` example

```text
leader_dollar
ident
eq
number
eof
```

### `.ast` example

```text
file
  bind_dollar
    name name
    number 42
```

### `.diag` example

```text
lex-leader-ws
```

No `.diag` file means **no** lex diagnostics expected.

### `.check` example

```text
sem-shadow
```

No `.check` file means **no** semantic diagnostics expected.

## Why a separate runner

- **Reference `xo`** is just the first candidate.
- Another Echo runtime/CLI can implement the same flags and pass `e26`.
- Suite data stays out of any one crate’s unit tests.

Later stages (`ast`, `check`, `run`) extend the protocol with more commands and
sibling expectation files; fixture numbers stay stable when possible.

## Current coverage

| Path | Stages | Notes |
|------|--------|-------|
| `echo26/leaders/**` | lex + ast + check | leaders; `sem-break` still illegal at top level |
| `echo26/parse/**` | lex + ast + check | parse depth |
| `echo26/check/bind/` | + check | shadow, tilde update, immutable |
| `echo26/check/receiver/` | + check | free vs method receiver |
| `echo26/check/hash/` | + check | SCREAMING_SNAKE |
| `echo26/check/control/` | + check | break in loop ok |
| `echo26/multi/**` | graph | imports bind, `%`/`@` merge, export/conflict; unnumbered support files |
| `echo26/effect/**` | check | result/option unhandled + match |
| `echo26/run/result/**` | run | result ok/err via `!` + `\|` |
| `echo26/run/option/**` | run | option some/none via bare `^` + `\|` |
| `echo26/run/loop/**` | run | while, break, continue, `* x : xs`, index |
| `echo26/lits/string/**` | lex/ast | pure `'…'` / rich `"…"` |
| `echo26/run/string/**` | run | `io.print` pure/rich strings |
| `echo26/run/locator/**` | run | `p'…'` / `p"…"` including live `{name}` interp |
| `echo26/run/bytes/**` | run | `b'…'` / `b"…"` including live `{name}` interp |
| `echo26/run/cli/**` | run | `cli.parse` on a fixed argv vector |
| `echo26/run/process/**` | run | `process.args` / env / spawn; argv is the program, not host CLI |
| `echo26/run/http/**` | run | parse + `handle_connection`; Content-Length body |
| `echo26/run/multi/**` | run | multi-file `module.fn` / value / chain |
| `echo26/check/hash/**` | check | SCREAMING_SNAKE + `sem-const` |
| `echo26/run/hash/**` | run | `#` const-eval used at runtime |
| `echo26/infer/**` | check | kind mismatch, not-callable, list elem |
| `echo26/lits/**` | lex+ast+check | width tags, duration, bytes, locator |
| `echo26/leaders/module/` | lex + ast | `/` `\`, dual `/` |
| `echo26/parse/expr/` | lex + ast | call, binary, fn, receiver, lit, index, … |
| `echo26/parse/control/` | lex + ast | else-if, while, for, bare `^` |
| `echo26/parse/bind/` | lex + ast | no-init members, hash string |
| `echo26/parse/module/` | lex + ast | relative import, multi export |
| `echo26/parse/stmt/` | lex + ast | method call statement |
