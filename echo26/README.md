# echo26 (Echo 2026 executable contract)

**Echo 2026** is the language edition and canonical public Language Spec
(ADR 0015). This directory is the **machine-checked contract** of that edition.

File-backed language fixtures. The suite runner is **`e26`** (not linked to any
one implementation). Point it at any Echo-compatible candidate binary.

**Policy:** every language implementation change updates this suite and keeps
`e26` green. See [`docs/fixtures.md`](../docs/fixtures.md),
[`docs/testing.md`](../docs/testing.md), and `AGENTS.md`.

```bash
cargo build -p xo -p e26
cargo run -p e26 -- --binary target/debug/xo
```

Third-party toolchain:

```bash
e26 --binary /path/to/my-echo
```

## Layout

```text
echo26/<area>/<feature>/<NNN>_<slug>.echo
echo26/<area>/<feature>/<NNN>_<slug>.lex     # token kinds (required)
echo26/<area>/<feature>/<NNN>_<slug>.ast     # AST kinds (required)
echo26/<area>/<feature>/<NNN>_<slug>.diag    # optional lex diagnostic codes
echo26/<area>/<feature>/<NNN>_<slug>.stdin   # optional bytes piped to `xo run`
```

Areas: `leaders/`, `parse/`, `check/`, `multi/` (multi-file entries + support modules).

Only `NNN_*.echo` files are suite roots; unnumbered `.echo` files are imports only.

## Candidate protocol

```text
$binary lex --kinds --diag-codes path.echo
  stdout → .lex    stderr → .diag

$binary ast --kinds --diag-codes path.echo
  stdout → .ast

$binary check --diag-codes path.echo
  stderr → .check   # `sem-*` only; omit file if none
```

Update expectations from a known-good binary:

```bash
e26 --binary target/debug/xo --update
```

Filter:

```bash
e26 --binary target/debug/xo --filter leaders/bind
```

Details: [`docs/fixtures.md`](../docs/fixtures.md).
