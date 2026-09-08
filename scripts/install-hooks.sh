#!/usr/bin/env bash
# Point this clone at versioned hooks under .githooks/ (see docs/development-speed.md).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ ! -d .githooks ]]; then
  echo "install-hooks: missing .githooks/" >&2
  exit 1
fi

chmod +x .githooks/* 2>/dev/null || true

# Relative path so the setting survives worktrees / path moves within the clone.
git config core.hooksPath .githooks

echo "install-hooks: core.hooksPath=.githooks"
echo "install-hooks: pre-commit will cargo check --workspace with -Dwarnings on Rust commits."
