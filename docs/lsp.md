# Language server

Editor integration presentation boundary.

| | |
|--|--|
| **Status** | **Active** — full planned depth + reliability polish (versioned diags, incremental sync, testable session) |
| **Owners** | `echo_lsp` (`xo lsp`) |
| **Related** | [`incremental.md`](incremental.md), [`pipeline.md`](pipeline.md), ADR 0001, [`diagnostics.md`](diagnostics.md), [`implementation.md`](implementation.md) §2.9 |

## Scope

LSP features consume the **shared pipeline**. The server must not reimplement
semantics, parsing, or resolution.

| Capability | Source of truth |
|------------|-----------------|
| Diagnostics | `echo_pipeline::analyze` / same codes as `xo check` |
| Hover / completion / signature help | semantic model + AST + index facts |
| Go to definition / references / rename | AST name walk + binds + imports |
| Document / workspace symbols | `echo_index::extract` |
| Formatting | `echo_parser::format_source` (same as `xo fmt`) |
| Semantic tokens | shared lexer (+ AST roles when available) |

## Facts

- **`DocumentStore`**: open / change / close; path from `file://` URIs
  (percent-decode on input; percent-encode on `path_to_uri`).
- **`LspSession`**: testable protocol state (no stdio). `server::run_stdio`
  only frames Content-Length JSON-RPC around `session.handle`.
- **Overlays**: dirty buffer text passed to `check_entry_with_overlays` so
  multi-file imports still resolve against disk + open buffers.
- **Diagnostics**:
  - Shared `analyze` per open path with the session overlay map.
  - **Versioned** `publishDiagnostics` (`version` from the open document).
  - Attribution by **SourceId → module path** only (no filename-substring match).
  - On any open/change/save, **every open document** is re-published so
    multi-file overlays stay consistent.
  - On close, empty diagnostics for that URI (no version).
- **Text sync**: **Incremental** (`change: 2`). Full-buffer changes (no
  `range`) are still accepted. Ranges use UTF-16 columns.
- **Positions**: UTF-16 columns ([`position.rs`](../crates/echo_lsp/src/position.rs)).
- **Workspace root**: `workspaceFolders[0]` preferred, then `rootUri` / `rootPath`.
- **Rename**: in-document; refuses new names that would shadow existing binds
  (language no-shadowing).

## Run

```bash
cargo build -p xo
./target/debug/xo lsp
# point the editor at this binary as the language server
```

Example VS Code / compatible client settings:

```json
{
  "command": "xo",
  "args": ["lsp"]
}
```

## Advertised capabilities

`initialize` reports:

| Capability | Method(s) |
|------------|-----------|
| Incremental document sync | `textDocument/didOpen` / `didChange` / `didClose` / `didSave` |
| Diagnostics | `textDocument/publishDiagnostics` (with `version` when known) |
| Hover | `textDocument/hover` |
| Go to definition | `textDocument/definition` |
| Find references | `textDocument/references` |
| Completion | `textDocument/completion` |
| Signature help | `textDocument/signatureHelp` |
| Rename | `textDocument/rename` |
| Document symbols | `textDocument/documentSymbol` |
| Workspace symbols | `workspace/symbol` |
| Formatting | `textDocument/formatting` |
| Semantic tokens (full) | `textDocument/semanticTokens/full` |

**Not in this milestone** (optional later): inlay hints, code actions / quick fixes,
range formatting, pull diagnostics.

## Reliability (host polish)

| Concern | Behavior |
|---------|----------|
| Stale diagnostics | `version` on publish; client can drop outdated notes |
| Multi-file dirty buffers | Shared overlays; republish all open URIs |
| Cross-file diag leak | Path/URI equality only (`paths_equal` / `diagnostic_matches_doc`) |
| Incremental edits | Ordered `contentChanges` with UTF-16 ranges |
| Protocol tests | `LspSession` unit tests (open → change → hover / diags) |
| Wire framing | Content-Length on stdio; invalid JSON body soft-ignored |

## Dependency on infra

LSP must not invent a second cache. Diagnostics use the same phase fingerprints
and store as `xo check` ([`incremental.md`](incremental.md)).
