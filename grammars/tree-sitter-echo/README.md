# tree-sitter-echo

Tree-sitter grammar for the **Echo** language.

## Source of truth

This package is **generated** from shared Echo syntax facts
(`echo_syntax::LEADERS` / `LeaderKind`, aligned with `docs/lexer.md`).

```bash
# from the Echo repo
cargo build -p xo
./target/debug/xo tools grammar tree-sitter -o path/to/tree-sitter-echo
```

Do not treat hand edits to this tree as language authority — re-run the
generator after leader or lexer surface changes.

## Leaders (18)

| Token | Glyph | Dual-use |
|-------|-------|----------|
| `leader_tilde` | `~` | yes |
| `leader_dollar` | `$` | no |
| `leader_hash` | `#` | no |
| `leader_percent` | `%` | yes |
| `leader_at` | `@` | no |
| `leader_question` | `?` | no |
| `leader_colon` | `:` | yes |
| `leader_bang` | `!` | yes |
| `leader_caret` | `^` | yes |
| `leader_star` | `*` | yes |
| `leader_lt` | `<` | yes |
| `leader_gt` | `>` | yes |
| `leader_pipe` | `|` | yes |
| `leader_plus` | `+` | yes |
| `leader_minus` | `-` | yes |
| `leader_ampersand` | `&` | yes |
| `leader_slash` | `/` | yes |
| `leader_backslash` | `\` | no |

- **Dual-use glyphs** (leader at statement start; operator/token in expressions): `~` (leader_tilde), `%` (leader_percent), `:` (leader_colon), `!` (leader_bang), `^` (leader_caret), `*` (leader_star), `<` (leader_lt), `>` (leader_gt), `|` (leader_pipe), `+` (leader_plus), `-` (leader_minus), `&` (leader_ampersand), `/` (leader_slash)
- **Leader-only** (statement introducers; invalid as free expression glyphs in the real lexer): `$` (leader_dollar), `#` (leader_hash), `@` (leader_at), `?` (leader_question), `\` (leader_backslash)

Dual-use is modeled by **grammar context**: `leader_*` tokens only appear as
statement introducers; the same characters appear again inside expression rules.

## Build (optional)

Requires the [tree-sitter CLI](https://tree-sitter.github.io/tree-sitter/):

```bash
tree-sitter generate
tree-sitter parse path/to/file.echo
```

## Highlighting

`queries/highlights.scm` marks:

- leaders → `@keyword`
- idents → `@variable` / `@type` / `@property`
- strings / numbers → `@string` / `@number`
- comments → `@comment`

For IDE-quality kinds (bind vs use, etc.) prefer the Echo language server
semantic tokens (`xo lsp`); this grammar is the structural basemap.
