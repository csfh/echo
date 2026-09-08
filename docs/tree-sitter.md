# Tree-sitter grammar (generated)

| | |
|--|--|
| **Status** | **Active** — emit from `echo_syntax` via `xo tools grammar tree-sitter` |
| **Owners** | `echo_syntax::tree_sitter`, CLI `xo tools grammar tree-sitter` |
| **Related** | [`lexer.md`](lexer.md), [`syntax.md`](syntax.md), [`pipeline.md`](pipeline.md) § editor grammar |

## Authority

The package is **derived**, not a second language source of truth.

- Statement leaders: `echo_syntax::LEADERS` / `LeaderKind` (glyph, `token_name`, `is_dual_use`)
- Literals, comments, idents: surface aligned with [`lexer.md`](lexer.md)
- Dual-use: leader-only glyphs are named tokens; dual-use glyphs are a **single**
  terminal shared by statement introducers and expression operators. Newlines
  are significant so operators do not cross into the next statement (matches
  lexer statement-start).
- Statement-start `!` / `-` / `+` parse as **error_return** / **task_join** /
  **task_spawn**, not top-level unary. Unary remains available nested in
  expressions (e.g. `$ x = -1`) via `_expression` vs `_expr_non_unary` for
  expression statements.

Do **not** hand-edit a checked-out grammar as language authority — regenerate.

## Generate

```bash
cargo build -p xo
./target/debug/xo tools grammar tree-sitter -o path/to/tree-sitter-echo
```

Writes a recognizable tree-sitter package:

| Path | Role |
|------|------|
| `grammar.js` | DSL entry (`name: 'echo'`) |
| `package.json` | `tree-sitter-echo` metadata |
| `tree-sitter.json` | modern CLI metadata |
| `queries/highlights.scm` | leaders / idents / strings / comments / numbers |
| `src/` | **Tracked** — C sources from `tree-sitter generate` (Zed and other hosts clone this path) |
| `README.md` | regenerate notes + leader table |

In-repo path for hosts: **`grammars/tree-sitter-echo`**. The Zed extension
(`zed-echo`) pins this directory via public `repository` + `rev` + `path` — no
machine-local `file://` URLs.

## Optional: build and parse

Requires the [tree-sitter CLI](https://tree-sitter.github.io/tree-sitter/):

```bash
cd grammars/tree-sitter-echo
tree-sitter generate   # refreshes tracked src/
tree-sitter parse path/to/file.echo
```

## Highlighting vs LSP

- **tree-sitter**: structural basemap (folds, cheap offline highlight).
- **`xo lsp` semantic tokens**: higher-fidelity kinds on the shared pipeline.

Both are intentional; neither replaces the other.

## Website (`www/`)

Docs and the homepage use **web-tree-sitter** + this package’s WASM and
`queries/highlights.scm` (not Shiki/TextMate):

```bash
# regenerate grammar + wasm (requires tree-sitter CLI)
cargo build -p xo
./target/debug/xo tools grammar tree-sitter -o grammars/tree-sitter-echo
(cd grammars/tree-sitter-echo && tree-sitter generate && tree-sitter build --wasm -o tree-sitter-echo.wasm .)
# copy into www/public/tree-sitter/
(cd www && npm run sync:tree-sitter)
```

Highlight entrypoints: `www/src/lib/echo-highlight.ts`, `www/src/components/echo-code.tsx`.

## Tests

```bash
cargo test -p echo_syntax
# exercises write_tree_sitter_grammar / package content vs LEADERS
```
