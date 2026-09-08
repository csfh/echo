# Continuous integration

| | |
|--|--|
| **Status** | Active |
| **Workflows** | [`.github/workflows/pr.yml`](../.github/workflows/pr.yml) (PRs), [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) (releases), [`.github/workflows/pages-redirect.yml`](../.github/workflows/pages-redirect.yml) (GitHub Pages → xo.run) |
| **Related** | [`development-speed.md`](development-speed.md), [`llvm.md`](llvm.md), [`fixtures.md`](fixtures.md) |

## Triggers

| Workflow | File | When |
|----------|------|------|
| **PR** | [`.github/workflows/pr.yml`](../.github/workflows/pr.yml) | Every **pull request** — Linux only |
| **CI (release)** | [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) | GitHub **Release published** only |
| **Pages redirect** | [`.github/workflows/pages-redirect.yml`](../.github/workflows/pages-redirect.yml) | Push to `main` when the redirect stub changes, or `workflow_dispatch` |

Multi-platform **release** builds do **not** run on push, PR, bare tags, or
manual dispatch.

Local day-to-day gate remains `scripts/gate echo26` / `cargo test` on the developer machine.

The public site is [https://xo.run](https://xo.run) (Cloudflare Pages, `www/`).
GitHub Pages is enabled with a workflow build so the default project host
redirects there. The workflow publishes only
[`.github/pages-redirect/`](../.github/pages-redirect/); it does not build
`www/`.

## PR gate (`pr.yml`)

On `ubuntu-24.04`, same proof as release `test-linux`:

1. Debug build of `xo` + `e26` + `echo_runtime` (stage staticlib)
2. Smoke: `cargo test -p xo` non-JIT unit filters
3. Smoke: `xo run --no-cache examples/misc/hello.echo` (AOT)
4. **`scripts/gate echo26`** — hard fail if red

Check name (for branch protection): `test-linux (smoke + echo26)`.

## Jobs

### `build` (matrix)

Release-builds `xo` (always includes LSP + REPL — no Cargo features) and uploads:

| Artifact name | Runner | Notes |
|---------------|--------|-------|
| `xo-linux-x86_64` | `ubuntu-24.04` | binary + `xo-linux-x86_64.tar.gz` |
| `xo-windows-x86_64` | `windows-2022` | `xo.exe` + tarball when produced |
| `xo-macos-arm64` | `macos-14` | Apple Silicon only (no Intel mac) |

Each matrix job also **attaches** `xo-<artifact>.tar.gz` to the published
GitHub release (`gh release upload`). Archive layout:

```text
bin/xo                  # or xo.exe on Windows
bin/libecho_runtime.a   # echo_runtime.lib on Windows when produced
std/…                   # co-located std sources
version                 # release tag
```

Users install with `scripts/install.sh from-release` (see
[`docs/install.md`](install.md)). The current published tag is a prerelease
and may not include every matrix artifact; `/install` lists what that tag
attached.

Each job installs **LLVM 22** from **official**
[`llvm/llvm-project` release tarballs](https://github.com/llvm/llvm-project/releases)
via in-repo [`scripts/ci/llvm.sh`](../scripts/ci/llvm.sh)
(SHA256-pinned). Sets `LLVM_SYS_221_PREFIX`. Clears the repo `sccache` rustc
wrapper so runners without sccache still compile.

**Host-specific LLVM env**

| OS | What CI sets | What CI does **not** set |
|----|----------------|---------------------------|
| Linux | LLVM `bin` on `PATH`; `LIBRARY_PATH` + `LD_LIBRARY_PATH` → LLVM `lib/` | — |
| macOS | For **cargo build only**: system `clang` as linker driver + tarball **`ld64.lld`** (`-fuse-ld=…`) so LLVM 22 bitcode links; explicit `-lc++`/`-lc++abi`; min macOS 14. Do **not** set `CC`/`CXX` to LLVM clang. | Job-global `DYLD_LIBRARY_PATH`; `CC`/`CXX`=LLVM clang (missing SDK headers) |
| Windows | `cygpath` for Git Bash `tar`; LLVM `bin` on `PATH`; `scripts/ci/windows-llvm-deps.sh` supplies **`xml2s.lib`** (from ShiftMedia libxml2) into LLVM `lib/` + `LIB=` | raw `D:\…` extract dirs (break `tar`) |

**Windows LLVM rpmalloc vs UCRT:** official LLVM 19+ x64 tarballs build
LLVMSupport with in-tree **rpmalloc** replacing CRT `malloc` / `free` /
`realloc` (static `/MT`). rustc on MSVC links the UCRT (`/MD`). llvm-sys
`prefer-dynamic` still falls back to static on MSVC (dynamic LLVM is
unsupported there). The `xo.exe` link then fails **LNK2005** /
**LNK1169** (`rpmalloc.c.obj` vs `ucrt.lib`). Workspace
[`.cargo/config.toml`](../.cargo/config.toml) `[target.x86_64-pc-windows-msvc]`
and [`crates/xo/build.rs`](../crates/xo/build.rs) pass `/FORCE:MULTIPLE` and
`/NODEFAULTLIB:libcmt` so the first (rpmalloc) heap symbols win and LLVM’s
`libcmt` defaultlib is dropped. Crate proof:
`windows_msvc_llvm_rpmalloc_link_flags`.

**Windows runtime:** net park uses a short yield instead of `mio::unix::SourceFd` (Unix-only).
Task scheduling still uses portable `mio::Poll` + `Waker`. Unix domain socket
natives (`echo_runtime_unix_*`) compile as stubs that return handle 0 / write
`-1`; `std/net/unix` treats that as the same handle-0 failure as TCP.

After `cargo build`, [`scripts/ci/stage-runtime-lib.sh`](../scripts/ci/stage-runtime-lib.sh)
copies the newest `libecho_runtime*.a` into `target/<profile>/libecho_runtime.a`
(stable name for artifacts). `xo` AOT link also scans that profile and `deps/`
and picks the newest matching archive itself.

No third-party `setup-llvm` action — keeps the CI supply chain to GitHub-hosted
actions (`checkout`, `upload-artifact`), `dtolnay/rust-toolchain`, and LLVM
upstream only.

### `test-linux` (hard gate)

On Ubuntu only:

1. Debug build of `xo` + `e26` + `echo_runtime` (stage staticlib)
2. Smoke: `cargo test -p xo` **non-JIT** unit filters only (REPL JIT exec tests
   currently SIGSEGV under in-process `run_jit_ir`; tracked separately)
3. Smoke: `xo run --no-cache examples/misc/hello.echo` (**AOT** — needs clang + runtime)
4. **`scripts/gate echo26`** — workflow fails if red

Language correctness is enforced here; macOS/Windows prove the release binary links.

Local `scripts/gate std-test` and `scripts/gate examples` cover `xo test std`
and finite example programs. The PR workflow does not run those layers yet.

## Host / known gaps

- Windows **AOT** (`xo run` via clang + pthread/dl flags) is not yet first-class;
  shipping `xo.exe` for check/fmt/lsp/repl is still the goal of the Windows build.
- Dynamic LLVM (`prefer-dynamic`) is **not** used on Windows MSVC (llvm-sys
  refuses `--link-shared` there). Static LLVM + rpmalloc is linked with
  `/FORCE:MULTIPLE` (see table above). CI still puts the LLVM bin dir on
  `PATH` for `llvm-config`.
