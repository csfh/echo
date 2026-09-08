#!/usr/bin/env bash
# Provide MSVC import/static libs that official LLVM Windows archives need at
# link time but do not ship (xml2s.lib, and often zlib).
#
# Usage (after scripts/ci/llvm.sh):
#   scripts/ci/windows-llvm-deps.sh "$LLVM_SYS_221_PREFIX"
#
# Downloads ShiftMediaProject MSVC17 x64 packages into "$prefix/lib".

set -euo pipefail

PREFIX="${1:?llvm prefix}"
LIB_DIR="${PREFIX}/lib"
mkdir -p "$LIB_DIR"

# Normalize for Git Bash / curl.
if command -v cygpath >/dev/null 2>&1; then
  LIB_DIR_U="$(cygpath -au "$LIB_DIR")"
else
  LIB_DIR_U="$LIB_DIR"
fi

need() {
  local name="$1"
  if [[ -f "${LIB_DIR_U}/${name}" || -f "${LIB_DIR}/${name}" ]]; then
    echo "windows-llvm-deps: already have ${name}" >&2
    return 0
  fi
  return 1
}

# xml2s.lib — required by llvm-sys static link on MSVC ("cannot open xml2s.lib")
if ! need xml2s.lib; then
  # ShiftMediaProject ships MSVC static libs named xml2s.lib (x64 under SMP).
  ver="v2.14.3"
  url="https://github.com/ShiftMediaProject/libxml2/releases/download/${ver}/libxml2_${ver}_msvc17.zip"
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/xml2-XXXXXX")"
  echo "windows-llvm-deps: downloading ${url}" >&2
  curl -fsSL --retry 3 -o "${tmp}/libxml2.zip" "$url"
  unzip -q -o "${tmp}/libxml2.zip" -d "${tmp}/out"
  # ShiftMedia uses lib/x64/libxml2.lib (static) + xml2.lib (import).
  # llvm-sys asks for xml2s.lib — map static libxml2.lib → xml2s.lib.
  found="$(find "${tmp}/out" -path '*/lib/x64/libxml2.lib' | head -1 || true)"
  if [[ -z "$found" ]]; then
    found="$(find "${tmp}/out" -path '*x64*' -iname 'libxml2.lib' | head -1 || true)"
  fi
  if [[ -z "$found" ]]; then
    found="$(find "${tmp}/out" -iname 'xml2s.lib' -o -iname 'libxml2.lib' | head -1 || true)"
  fi
  if [[ -z "$found" ]]; then
    echo "windows-llvm-deps: libxml2.lib/xml2s.lib not in archive; listing:" >&2
    find "${tmp}/out" -name '*.lib' | head -40 >&2 || true
    exit 1
  fi
  cp -f "$found" "${LIB_DIR_U}/xml2s.lib"
  cp -f "$found" "${LIB_DIR_U}/libxml2.lib"
  d="$(dirname "$found")"
  for extra in xml2.lib zlib.lib zlibwapi.lib zlibstatic.lib iconv.lib libiconv.lib; do
    if [[ -f "${d}/${extra}" ]]; then
      cp -f "${d}/${extra}" "${LIB_DIR_U}/"
    fi
  done
  echo "windows-llvm-deps: installed xml2s.lib from ${found}" >&2
  rm -rf "$tmp"
fi

# zlib if missing (LLVM often needs it alongside xml2)
if ! need zlib.lib && ! need zlibstatic.lib; then
  ver="v1.3.1"
  url="https://github.com/ShiftMediaProject/zlib/releases/download/${ver}/zlib_${ver}_msvc17.zip"
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/zlib-XXXXXX")"
  echo "windows-llvm-deps: downloading ${url}" >&2
  if curl -fsSL --retry 3 -o "${tmp}/zlib.zip" "$url"; then
    unzip -q -o "${tmp}/zlib.zip" -d "${tmp}/out"
    found="$(find "${tmp}/out" -path '*x64*' \( -iname 'zlib.lib' -o -iname 'zlibstatic.lib' \) | head -1 || true)"
    if [[ -z "$found" ]]; then
      found="$(find "${tmp}/out" \( -iname 'zlib.lib' -o -iname 'zlibstatic.lib' \) | head -1 || true)"
    fi
    if [[ -n "$found" ]]; then
      cp -f "$found" "${LIB_DIR_U}/zlib.lib"
      echo "windows-llvm-deps: installed zlib from ${found}" >&2
    fi
  else
    echo "windows-llvm-deps: warning: zlib package download failed (continuing)" >&2
  fi
  rm -rf "$tmp"
fi

ls -la "$LIB_DIR_U"/*.lib 2>/dev/null | head -30 >&2 || true
