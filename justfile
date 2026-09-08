set shell := ["bash", "-cu"]

# Host binary for benches/tests (default: release). Override: XO=target/debug/xo
xo := env("XO", "target/release/xo")

check:
    cargo check --workspace

# Workspace check with warnings as errors (matches pre-commit).
check-deny:
    #!/usr/bin/env bash
    set -euo pipefail
    extra=(-Dwarnings)
    if [[ "$(uname -s)" == "Linux" ]] && command -v mold >/dev/null 2>&1; then
      extra+=(-C "link-arg=-fuse-ld=mold")
    fi
    export RUSTFLAGS="${RUSTFLAGS:+${RUSTFLAGS} }${extra[*]}"
    cargo check --workspace --locked

# Point this clone at versioned .githooks/ (pre-commit rustc clean).
hooks:
    scripts/install-hooks.sh

test CRATE:
    cargo test -p {{CRATE}}

test-fast:
    scripts/gate changed

test-full:
    scripts/gate workspace

# echo26 suite against the workspace xo binary
e26:
    cargo build -q -p xo -p e26
    cargo run -q -p e26 -- --binary target/debug/xo

# --- Bench host (Rust / cargo profile) vs Echo LLVM -O (separate!) ---
#
# 1) just bench-host     → cargo --release xo + stage libecho_runtime.a  (ONCE)
# 2) just std-bench      → Echo -O2 compile (cached) + run benches
# 3) just std-bench      → warm: mostly re-exec AOT children + measure
#
# "cargo build -p xo" optimizes the HOST (compiler + runtime staticlib).
# "xo test --bench -O2" optimizes the ECHO program (sip, str, …) via LLVM.

# Build optimized host once. Stages libecho_runtime next to the profile xo.
bench-host:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "bench-host: cargo build --release -p xo (host + runtime)" >&2
    cargo build --release -q -p xo
    scripts/ci/stage-runtime-lib.sh release >/dev/null
    # Prefer newest hashed staticlib under deps/ if stage left a stale unhashed name.
    if ls -1t target/release/deps/libecho_runtime-*.a >/dev/null 2>&1; then
      src="$(ls -1t target/release/deps/libecho_runtime-*.a | head -1)"
      # stage-runtime-lib may already have linked the same path — skip no-op cp.
      if [[ "$(realpath "$src")" != "$(realpath target/release/libecho_runtime.a 2>/dev/null || true)" ]]; then
        cp -f "$src" target/release/libecho_runtime.a
      fi
    fi
    test -x target/release/xo
    test -f target/release/libecho_runtime.a
    echo "bench-host: ok  target/release/xo + libecho_runtime.a" >&2

# Ensure {{xo}} exists. Does not rebuild if present (use bench-host / xo-rebuild).
xo-ensure:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -x "{{xo}}" ]]; then
      exit 0
    fi
    if [[ "{{xo}}" == *"/release/"* ]] || [[ "{{xo}}" == target/release/xo ]]; then
      echo "xo-ensure: missing {{xo}} — run: just bench-host" >&2
    else
      echo "xo-ensure: missing {{xo}} — run: cargo build -p xo" >&2
    fi
    exit 1

# Force-rebuild debug host (dev loop, not for published bench numbers).
xo-rebuild:
    cargo build -p xo
    scripts/ci/stage-runtime-lib.sh debug >/dev/null || true

# Co-located std unit suites (uses prebuilt xo)
std-test: xo-ensure
    {{xo}} test std

# Finite example entries (check + run). Uses scripts/gate so server.echo is not started.
examples:
    scripts/gate examples

# Std benches: prebuilt host + Echo -O{{O}} + IR/AOT cache.
# Does NOT cargo-build. First run compiles Echo; later runs should aot: hit.
std-bench O="2": xo-ensure
    mkdir -p .xo/bench
    {{xo}} test --bench -O{{O}} std \
      --bench-out .xo/bench/last.jsonl \
      --cache-status

# Explicit: host release build, then one warmable bench pass
std-bench-full O="2": bench-host
    just std-bench O={{O}} XO=target/release/xo

# Cold pipeline (no IR/AOT cache) — compile cost, not microbench ns/op
std-bench-cold O="2": xo-ensure
    mkdir -p .xo/bench
    {{xo}} test --bench -O{{O}} std \
      --bench-out .xo/bench/last.jsonl \
      --no-cache \
      --cache-status

# Re-run and compare to baseline (same host binary + same -O)
std-bench-compare O="2": xo-ensure
    mkdir -p .xo/bench
    {{xo}} test --bench -O{{O}} std \
      --bench-out .xo/bench/last.jsonl \
      --bench-baseline .xo/bench/baseline.jsonl \
      --bench-threshold 20 \
      --cache-status

# Promote last run to local baseline
std-bench-save:
    mkdir -p .xo/bench
    cp -f .xo/bench/last.jsonl .xo/bench/baseline.jsonl
    @echo "saved .xo/bench/baseline.jsonl"

# Algorithm demos + std hot paths (real functions, normal test.bench).
algo-bench O="2": xo-ensure
    mkdir -p .xo/bench
    {{xo}} test --bench -O{{O}} \
      examples/algos/call.echo \
      examples/algos/collatz.echo \
      examples/algos/fibonacci.echo \
      examples/algos/gcd.echo \
      examples/algos/list.echo \
      examples/algos/primes.echo \
      examples/algos/sort.echo \
      std/list.echo \
      std/bytes.echo \
      std/collections/map.echo \
      std/crypto/hash/sip.echo \
      std/crypto/hash/sha256.echo \
      --bench-out .xo/bench/algo-last.jsonl \
      --cache-status

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt-check

profile:
    time cargo check --workspace
    time cargo test --workspace --no-run
    time cargo nextest run --workspace

sccache:
    sccache --show-stats

tools:
    scripts/gate tools

# User XDG install (release xo + std → ~/.local/bin/xo)
install:
    scripts/install.sh install

upgrade:
    scripts/install.sh upgrade

uninstall *ARGS:
    scripts/install.sh uninstall {{ARGS}}

install-doctor:
    scripts/install.sh doctor

gate *ARGS:
    scripts/gate {{ARGS}}

web-install:
    npm --prefix www install

web-dev:
    npm --prefix www run dev

web-lint:
    npm --prefix www run lint

web-format:
    npm --prefix www run format

web-test:
    npm --prefix www run test

web-build:
    npm --prefix www run build

# Browser check host (compiler frontend only). Writes www/public/echo-wasm/.
wasm:
    scripts/build-wasm.sh

# Rebuild the wasm checker, then serve the site (open /try).
try: wasm
    npm --prefix www run dev
