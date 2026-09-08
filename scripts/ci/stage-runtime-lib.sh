#!/usr/bin/env bash
# Ensure libecho_runtime.a (or .lib) is available next to a cargo profile dir so
# `xo run` / AOT link can find it without scanning hashed deps names.
#
# Usage:
#   scripts/ci/stage-runtime-lib.sh [debug|release]
#
# Finds the newest libecho_runtime*.a / echo_runtime*.lib under
# target/<profile>/ and target/<profile>/deps/, copies to
# target/<profile>/libecho_runtime.a (or echo_runtime.lib).

set -euo pipefail

PROFILE="${1:-debug}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIR="${ROOT}/target/${PROFILE}"

if [[ ! -d "$DIR" ]]; then
  echo "stage-runtime-lib: missing ${DIR}" >&2
  exit 1
fi

pick_newest() {
  local pattern="$1"
  # shellcheck disable=SC2086
  ls -1t ${pattern} 2>/dev/null | head -1 || true
}

src=""
for pattern in \
  "${DIR}/libecho_runtime.a" \
  "${DIR}/deps/libecho_runtime-*.a" \
  "${DIR}/deps/libecho_runtime.a" \
  "${DIR}/echo_runtime.lib" \
  "${DIR}/deps/echo_runtime-*.lib" \
  "${DIR}/deps/echo_runtime.lib"
do
  hit="$(pick_newest "$pattern")"
  if [[ -n "$hit" && -f "$hit" ]]; then
    src="$hit"
    break
  fi
done

if [[ -z "$src" ]]; then
  echo "stage-runtime-lib: no libecho_runtime staticlib under ${DIR}" >&2
  ls -la "$DIR" >&2 || true
  ls -la "${DIR}/deps" 2>/dev/null | head -40 >&2 || true
  exit 1
fi

case "$src" in
  *.lib)
    dest="${DIR}/echo_runtime.lib"
    ;;
  *)
    dest="${DIR}/libecho_runtime.a"
    ;;
esac

if [[ "$src" != "$dest" ]]; then
  cp -f "$src" "$dest"
fi
echo "stage-runtime-lib: ${src} -> ${dest}" >&2
printf '%s\n' "$dest"
