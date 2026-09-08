# Echo

Echo is a compiled language implemented in Rust. It uses a single LLVM backend
and a Rust-owned runtime for both AOT native binaries and in-process JIT
execution.

The command-line entrypoint is `xo`.

## Status

Echo is early-stage software. The workspace is scaffolded for full vertical
slices (frontend → semantics → IR → codegen → runtime → CLI). Language features
are implemented incrementally with focused proofs.

## Language design (in progress)

**Echo 2026** is the current language edition and canonical public Language Spec
(site `/e26`, ADR [0015](docs/adr/0015-echo-2026-canonical-edition.md)). The
executable contract is the [`echo26/`](echo26/) suite (`e26` runner).

Keyword-free, statement-led core — implementer surface:
[`docs/syntax.md`](docs/syntax.md).

| Tree | Role |
|------|------|
| [`examples/misc/`](examples/misc/) | Tiny **`xo run`** demos (print, loops, lists, result) |
| [`examples/app/`](examples/app/) | HTTP demo + kitchen-sink surface |
| [`examples/algos/`](examples/algos/) | Classic algorithms (factorial, sort, primes, …) |
| [`std/`](std/) | Standard library Echo sources (IO, TCP, HTTP stubs) |

```bash
cargo build -p xo
./target/debug/xo run examples/misc/hello.echo
./target/debug/xo run examples/misc/sum_list.echo ; echo exit:$?
./target/debug/xo check examples/app/surface.echo
```

Track discussion in [`docs/roadmap.md`](docs/roadmap.md).

## Documentation

| Doc | Role |
|-----|------|
| [`AGENTS.md`](AGENTS.md) | Workflow and invariants for humans/agents |
| [`docs/README.md`](docs/README.md) | Full docs map |
| [`docs/architecture.md`](docs/architecture.md) | Pipeline and crate ownership |
| [`docs/glossary.md`](docs/glossary.md) | Shared vocabulary |
| [`docs/development-speed.md`](docs/development-speed.md) | Local tools and gate |
| [`docs/adr/`](docs/adr/) | Architecture decisions |

Layer specs (`docs/syntax.md`, `docs/parser.md`, …) accumulate rules as each
layer lands.

## Workspace

See [`docs/architecture.md`](docs/architecture.md) for crate ownership and the
compilation pipeline.

- Frontend: `echo_source`, `echo_diagnostics`, `echo_syntax`, `echo_lexer`,
  `echo_ast`, `echo_parser`, `echo_semantics`
- IR and backend: `echo_hir`, `echo_mir`, `echo_codegen`, `echo_codegen_abi`,
  `echo_runtime`, `echo_std`
- Project tooling: `echo_index`, `echo_resolver`, `echo_fingerprint`,
  `echo_cache`, `echo_build`, `echo_reflection`, `echo_lsp`
- CLI: `xo`
- Browser check host: `echo_wasm` (`just wasm`, site `/try`)

## Requirements

- Rust with edition 2024 support
- LLVM 22 when codegen is active (inkwell)
- `clang` and `mold` for native link speed
- `sccache` for compile caching
- `cargo-nextest` and `just` for the local gate

See [`docs/development-speed.md`](docs/development-speed.md) for setup and the
edit/test loop.

## Install (user toolchain)

Published builds are **prereleases**. The current tag is
[`v0.0.1-alpha.rc.1`](https://github.com/csfh/echo/releases/tag/v0.0.1-alpha.rc.1)
and ships `xo-linux-x86_64.tar.gz`, `xo-macos-arm64.tar.gz`, and
`xo-windows-x86_64.tar.gz`.
`from-release` with no tag installs the newest published prerelease. Pass a tag
to pin. GitHub `/releases/latest` only resolves a non-prerelease and 404s today.

```bash
curl -fsSL https://raw.githubusercontent.com/csfh/echo/main/scripts/install.sh \
  | bash -s -- from-release

# Pin this tag
# … | bash -s -- from-release v0.0.1-alpha.rc.1
```

From a checkout, build + install under XDG and link `~/.local/bin/xo`:

```bash
./scripts/install.sh              # build from this tree
./scripts/install.sh from-release # newest published prerelease
./scripts/install.sh from-release v0.0.1-alpha.rc.1
./scripts/install.sh upgrade      # new version, keep previous
./scripts/uninstall.sh            # remove toolchain ( --purge also clears $XO_HOME )
./scripts/install.sh doctor
```

Layout and env vars: [`docs/install.md`](docs/install.md).

## Build and test

```bash
cargo check --workspace
scripts/gate changed --list
scripts/gate changed
scripts/gate workspace
just tools
```

`cargo build -p xo` always includes **LSP** and **REPL** (no Cargo features).

### CI

GitHub Actions (`.github/workflows/ci.yml`) release-builds `xo` **only when a
GitHub release is published** — not on push, PR, or bare tags.

| Artifact | Runner |
|----------|--------|
| Linux x86_64 | `ubuntu-24.04` |
| Windows x86_64 | `windows-2022` |
| macOS arm64 | `macos-14` |

The current published tag (`v0.0.1-alpha.rc.1`) attaches `xo-linux-x86_64`,
`xo-macos-arm64`, and `xo-windows-x86_64`. Windows AOT `xo run` is not
first-class yet; the Windows archive is `xo.exe` for check/fmt/lsp/repl.

On Linux, smoke (`cargo test -p xo`, `xo run` hello) and **`scripts/gate echo26`**
(Echo 2026 conformance) are hard gates when that workflow runs.

CLI surface (commands land as the language grows):

```bash
cargo run -p xo -- --help
cargo run -p xo -- lex <file>
cargo run -p xo -- ast <file>
cargo run -p xo -- ir <file>
cargo run -p xo -- run [--jit] <file>
cargo run -p xo -- build <file> -o <out>
cargo run -p xo -- lsp
cargo run -p xo -- repl
```

## Website

The public site is **[https://xo.run](https://xo.run)** (Cloudflare Pages).
Sources live in `www/` of this repository (Vite, React, Tailwind). GitHub Pages
on this repo only redirects to that host; it does not publish `www/`.

```bash
npm --prefix www install
npm --prefix www run dev
npm --prefix www run lint
npm --prefix www run format
npm --prefix www run build
# or
just web-dev
just web-build
scripts/gate web
```

## Contributing

Contributions are welcome. **By contributing, you accept the project CLA**,
which assigns copyright and IP in your contribution to Christoffer Hallas.

- How to contribute: [`CONTRIBUTING.md`](CONTRIBUTING.md)
- Contributor License Agreement: [`CLA.md`](CLA.md)
- Security: [`SECURITY.md`](SECURITY.md)
- Help wanted (broad): [`docs/roadmap.md`](docs/roadmap.md#help-wanted-broad-scope)

Pull requests run a **Linux** gate (build, smoke, `echo26`). Multi-OS release
builds run only when a GitHub Release is published.

## Community

Use common sense and decency. There is no formal code of conduct. We reserve the right to moderate this community to the extent of the law and the policy of the host. Write oss@christofferhallas.com if you need us.

## License

Licensed under the [MIT License](LICENSE).

Copyright (c) 2026 Christoffer Hallas.
