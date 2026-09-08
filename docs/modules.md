# Modules and packages

Module identity, imports, packages, paths, and project-wide resolution.

| | |
|--|--|
| **Status** | **Locked** (ADR 0014 + v0 decision bundle) |
| **Owners** | `echo_resolver`, `echo_index`, `echo_semantics`, `xo` |
| **Related** | ADR 0006, ADR 0014, `docs/syntax.md`, `docs/stdlib.md` |
| **CLI** | `xo check` · `xo get` · `xo home` · `[--graph]` |

## Vocabulary (locked)

| Term | Meaning |
|------|---------|
| **Module** | A **path-addressable** unit. **Any folder is a module.** Nested folders are not child packages — only longer paths. |
| **Path** | Relative, absolute (v0: not required), host/URL, or privileged `std` / `runtime`. |
| **Package** | **Optional** `xo.toml`: groups modules + deps. Not required for local multi-file work. |
| **Package cache** | User-only: `$XO_HOME/packages/<encoded-id>/<version>/`. |

There is **no** parent/child module hierarchy for tools or identity.

## v0 decision bundle (locked)

| Topic | Lock |
|-------|------|
| **Module unit on disk** | Import path resolves to **`name.echo` file**, else **`name/` directory**. A **directory module** includes **all `*.echo` files** in that directory (sorted); exports are the **union** of their `\ ` exports. Files remain separate parse/check units; graph includes every file. |
| **Package id encoding** | Single path segment under `packages/`: percent-encode `/` and non-unreserved bytes (e.g. `github.com%2Facme%2Flib`). |
| **Version selection (resolve)** | **No `default` alias.** If `xo.toml` pin: use/auto-get that pin. Else sole installed version, or **error** if multiple/none. |
| **`xo get` version (install)** | `@tag`/`@branch`/`@hash` → version dir = that ref; **skip if already cached**. **No `@` on git** → `git ls-remote HEAD` → pin to **full hash**; skip if that hash is already cached. Local `--path` with no `@` → version dir `local`. |
| **Auto-get on check/run** | **Only** for packages listed in **`cwd/xo.toml`** `[dependencies]`. If pin is listed: use that version if installed, else **auto-get the pin** (even when another sole version exists). Host imports **not** listed → `res-import`. |
| **`xo.toml` location** | **`cwd/xo.toml`** — run `xo` from the directory that owns the config. |
| **Folder private scope** | **Export-only** — files in a folder module do not share non-exported binds. |
| **`xo get --deps`** | **Recursive**: install declared deps, then each dep’s `xo.toml` deps (cycle-guard by id@version). |
| **Offline mode** | **None in v0** — auto-get for declared deps always allowed; no `--offline` / `XO_OFFLINE`. |
| **Dep pin values** | **Any ref string**: tag, branch, or commit hash (same as `xo get @ref`). |
| **`xo.toml` schema** | Tool reads only **`[dependencies]`** (host path → ref pin). Other sections ignored. Identity is host/import path, not a name field. |
| **Import alias** | **None** in v0. Last segment is bind name; collision → `res-import-name-conflict`. |
| **Absolute FS imports** | **Out of v0** (relative + host/URL + std/runtime only). |
| **Import cycles** | **Hard error** when detected (`res-import-cycle`). |
| **Project vendoring** | **No.** Packages only under user `$XO_HOME`. Project `.xo/cache/` is IR only. |
| **Cross-file visibility inside a folder module** | **Exports only** for other modules (`module.name`). Same-folder files do **not** share a private scope in v0 (each file’s locals stay file-local). Struct `%`/`@` merge remains **graph-wide** by name. |

## Pipeline

```text
entry (+ cwd/xo.toml deps → auto-get pins if missing)
  → resolve imports (closed graph, ADR 0006)
  → expand directory modules to all *.echo files
  → parse each file; extract facts; re-export fixed-point
  → merge % / @
  → bind one name per import (export union for multi-file modules)
  → file-local semantics
```

## Import model (locked)

### Module-scoped only

```echo
/ std/net/http
/ ./math
/ ../util
/ github.com/csfh/echo-pkg/http

$ s = http.serve(addr, routes)
$ x = math.add(1, 2)
```

| Step | Meaning |
|------|---------|
| `/ path` | Resolve module; bind **one** name (last path segment) |
| Use | `module.export` only |
| `\ name` | What this **file** contributes to the module’s export set |

**Privacy:** names not listed in `\ ` are **not** importable (`sem-module-export`).
That is the only public/private mechanism — see **Export discipline** in
[`stdlib.md`](stdlib.md) for how std applies it (helpers, types, methods).

### Path resolution

| Form | Resolve |
|------|---------|
| Relative `/ ./a/b` | `{importer_dir}/a/b.echo` if file; else `{importer_dir}/a/b/` directory module |
| Relative `/ ../a/b` | `{importer_dir}/../a/b.echo` if file; else `{importer_dir}/../a/b/` directory module |
| Host/URL `/ github.com/acme/lib/util` | Under `$XO_HOME/packages/…/<ver>/`: `util.echo` or `util/` |
| Privileged `/ std/…` | Toolchain std roots |
| Privileged `/ runtime` | Virtual; std sources only |

### Package cache

**`$XO_HOME`:** `$XO_HOME` env → `$XDG_CACHE_HOME/.xo` → `~/.cache/.xo`

```text
$XO_HOME/packages/<encoded-package-id>/<version>/…
```

- **`xo get pkg[@ver]`** — git clone or `--path` local copy into that layout.
- **`xo get … --deps`** — install `[dependencies]` from that package’s `xo.toml`.
- **`xo home`** — print roots.

### `xo.toml` (optional)

Tool reads only **`[dependencies]`**. Extra sections are ignored. Identity is
how the package is hosted / required (`github.com/…` in imports and `xo get`):

```toml
[dependencies]
"github.com/other/lib" = "v1.2.3"
```

Modules are import paths / folders on disk, not listed in TOML.

### `std` / `runtime`

Unchanged — see [`stdlib.md`](stdlib.md).

## Struct merge (`%` / `@`)

Graph-wide; not related to packages. Codes: `res-struct-dup-primary`,
`res-struct-no-primary`, `res-struct-dup-member`.

## Diagnostics

| Code | Meaning |
|------|---------|
| `res-import` | Cannot resolve path / package not installed |
| `res-import-name-conflict` | Two imports same last segment |
| `res-import-cycle` | Import cycle in the graph |
| `res-export-missing` | `\ name` not defined/re-exportable here |
| `res-runtime-forbidden` | `/ runtime` outside privileged std |
| `sem-module-export` | `module.foo` not exported |
| `sem-shadow` | Local bind reuses module name |

## Suite

- `echo26/multi/**` — multi-file graph  
- Folder-module fixtures under `echo26/multi/folder_module/` when present  

## Implementation status

| Piece | Status |
|-------|--------|
| Module-scoped `/` · last segment | **Done** |
| Relative (`./` + `../`) + std + runtime | **Done** |
| Host/URL + `$XO_HOME` + `xo get` | **Done** |
| Folder multi-file module | **Done** |
| `xo.toml` parse + `--deps` | **Done** (minimal schema) |
| Import cycle diagnostic | **Done** (`res-import-cycle`) |
| Imported function return kind | **Done** (`ModuleExport.return_ty` from defining module; params stay `value`) |
| Absolute FS imports | **Out of v0** |
