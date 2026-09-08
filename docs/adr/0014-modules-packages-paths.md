# 0014. Modules, packages, paths, and the `.xo` package cache

## Status

Accepted.

## Context

Echo needed a clear split between:

- **modules** (what `/` imports),
- **packages** (optional grouping + deps),
- **paths** (relative, absolute, URL),
- **on-disk install** for remote packages (`xo get` / install).

Package trees must not invent a second brand directory (`echo/…`). Everything
tool-owned lives under a **`.xo/`** tree. Fetched packages always land in the
**user (XDG) `.xo` cache**, versioned and flat — not copied into the project
tree by default.

## Decision

### 1. Module

- A **module** is a **path-addressable unit**.
- **Any folder is a module.** Nested folders are **not** “child packages” —
  there is **no parent/child module hierarchy**. A longer path is just another
  module path.
- Import bind name remains the **last path segment** (locked elsewhere).

### 2. Paths (import forms)

`/ path` accepts **any of these path shapes**:

| Form | Example | Meaning |
|------|---------|---------|
| Relative | `/ ./math` · `/ ../lib/util` | Relative to the **importer’s directory** |
| Absolute | `/ /home/me/lib/math` | Absolute filesystem path |
| Host / URL path | `/ github.com/csfh/echo-pkg/http` | Resolve via **user package cache** (after install) |
| Privileged std | `/ std/io` | Toolchain std only ([`stdlib.md`](../stdlib.md)) |
| Privileged runtime | `/ runtime` | Std sources only (locked) |

There is no separate “package import” syntax. URLs and host paths are just paths.

### 3. Only `.xo/` trees (no `echo/…` dirs)

Tool-owned directories are always named **`.xo`** (project or user). Do **not**
use `$XDG_*/echo/…` or a parallel `echo/modules` brand.

| Root | Role |
|------|------|
| `{project}/.xo/` | **Project** tool state only (e.g. `.xo/cache/` IR/AOT — [`incremental.md`](../incremental.md)). **Not** the package download target. |
| **User `.xo` root** | Package install cache (see below). |

### 4. Package cache — always user XDG `.xo` (locked)

**All package installs / downloads go to the user `.xo` package cache**, not
into `{project}/.xo/`.

**User `.xo` root** (in order):

1. `$XO_HOME` if set  
2. else `$XDG_CACHE_HOME/.xo`  
3. else `~/.cache/.xo`

**Layout — flat package id + version directories:**

```text
$XO_HOME/packages/<package-id>/<version>/
  … module trees for that package release …
```

| Piece | Rule |
|-------|------|
| `packages/` | Single flat registry under user `.xo` |
| `<package-id>` | Stable id from the package URL / host path (encoding is implementation detail; collision-resistant) |
| `<version>` | Explicit version / rev / tag as installed by the CLI |

- **`xo get` / install** materialises (or updates)  
  `$XO_HOME/packages/<id>/<version>/`.
- Resolver for host/URL imports looks up that cache (plus any version selection
  policy from `xo.toml` / CLI — implementation vertical).
- Reinstalling the same id@version is idempotent overwrite or no-op.
- Project `.xo/` never receives downloaded package trees in v0.

### 5. Package manifest — optional `xo.toml`

- A **package** is optional. Local work needs **no** manifest: modules + `/`
  imports are enough.
- **`xo.toml`**: tool reads **`[dependencies]`** only (host path → ref pin).
  Other sections are ignored (not errors).
- Identity is host path / import id (`github.com/…`), not a TOML name field.
- Modules are import paths and folders on disk, not a TOML inventory.
- Deps are **not** language syntax; `xo` installs them into the **user** package
  cache. The closed graph (ADR 0006) still comes from **entry + imports** once
  deps are present on disk.

```toml
[dependencies]
"github.com/other/lib" = "v1.2.3"
```

### 6. Privileged `std`

Unchanged: `/ std/…` is not “any folder named std.” Install / workspace
toolchain roots only.

## v0 decision bundle (locked with modules.md)

See **`docs/modules.md` § v0 decision bundle** for the full table. Highlights:

| Topic | Lock |
|-------|------|
| Disk unit | `name.echo` else `name/` all `*.echo` (export union; no private shared scope) |
| Cache | `$XO_HOME/packages/<encoded-id>/<version>/` only; no project vendoring |
| No `default` alias | Unspecified git get → **HEAD hash** pin; re-get no-ops if hash cached |
| Multi-version resolve | Sole version, or pin from `cwd/xo.toml`, or error |
| Auto-get | Only declared `cwd/xo.toml` deps (pin wins; fetch pin if missing) |
| `xo get --deps` | Recursive with cycle guard |
| Dep pins | Any ref: tag / branch / hash |
| Alias / abs path / offline flag | Out of v0 |

## Out of scope (this ADR)

- Full registry product / SAT solver (pins in `xo.toml` are enough to start).
- English keywords for packages.
- Requiring `xo.toml` for multi-file programs.
- Per-project vendoring of packages under `{project}/.xo/` (not v0).
- Shared private scope across files in a folder module (exports only).

## Consequences

- Resolver treats **module path** as primary identity; remote paths resolve
  through **`$XO_HOME/packages/…`**.
- Implementation may still map one module path → one `.echo` file until the
  **folder-module** vertical lands.
- `xo get` / install, package cache layout, and optional `xo.toml` land as
  verticals under `echo_resolver` + `xo`.

## Related

- [`modules.md`](../modules.md) — operational rules
- [0006](0006-closed-compilation-graph.md) — closed graph
- [`stdlib.md`](../stdlib.md) — `std` / `runtime`
- [`incremental.md`](../incremental.md) — project `.xo/cache/`
