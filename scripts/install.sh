#!/usr/bin/env bash
# Echo / xo user install (XDG layout, ADR 0014).
#
# Usage:
#   ./scripts/install.sh              # build from checkout (default when in repo)
#   ./scripts/install.sh install
#   ./scripts/install.sh from-release [tag]   # newest published prerelease (or pin a tag)
#   ./scripts/install.sh upgrade      # rebuild + flip current (checkout)
#   ./scripts/install.sh uninstall [--purge]
#   ./scripts/install.sh doctor
#   ./scripts/install.sh paths
#
# One-liner (no checkout):
#   curl -fsSL https://raw.githubusercontent.com/csfh/echo/main/scripts/install.sh \
#     | bash -s -- from-release
#
# Layout (defaults; override via env):
#   Toolchain:  ${XDG_DATA_HOME:-~/.local/share}/xo/toolchains/<version>/{bin,std}
#   Current:    ${XDG_DATA_HOME:-~/.local/share}/xo/current -> toolchains/<version>
#   PATH link:  ${XO_BIN_DIR:-~/.local/bin}/xo
#   Packages:   ${XO_HOME:-${XDG_CACHE_HOME:-~/.cache}/.xo}/packages
#   State:      ${XDG_STATE_HOME:-~/.local/state}/xo   (REPL history, …)
#   Config:     ${XDG_CONFIG_HOME:-~/.config}/xo
#
# Upgrade is atomic: build into a version dir, then repoint `current`.
# Previous toolchains remain until uninstall --purge or manual prune.
set -euo pipefail

# When run as `./scripts/install.sh`, resolve the checkout. When piped via
# `curl … | bash -s -- from-release`, BASH_SOURCE is empty/unbound under `set -u`
# and there is no repo root — only from-release/upgrade-from-release paths work.
_script_src="${BASH_SOURCE[0]:-}"
if [[ -n "${_script_src}" && "${_script_src}" != "bash" && "${_script_src}" != "-" && -f "${_script_src}" ]]; then
  SCRIPT_DIR="$(cd "$(dirname "${_script_src}")" && pwd)"
else
  SCRIPT_DIR=""
fi
unset _script_src
if [[ -n "${SCRIPT_DIR}" && -f "${SCRIPT_DIR}/../Cargo.toml" ]]; then
  REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
else
  REPO_ROOT=""
fi

# GitHub repo for prebuilt installs (owner/name).
ECHO_REPO="${ECHO_REPO:-csfh/echo}"

die() {
  echo "install.sh: $*" >&2
  exit 1
}

info() {
  echo "install.sh: $*" >&2
}

# --- path resolution (mirrors ADR 0014 + docs/install.md) -------------------

xdg_data_home() {
  if [[ -n "${XDG_DATA_HOME:-}" ]]; then
    printf '%s' "$XDG_DATA_HOME"
  else
    printf '%s' "${HOME}/.local/share"
  fi
}

xdg_cache_home() {
  if [[ -n "${XDG_CACHE_HOME:-}" ]]; then
    printf '%s' "$XDG_CACHE_HOME"
  else
    printf '%s' "${HOME}/.cache"
  fi
}

xdg_state_home() {
  if [[ -n "${XDG_STATE_HOME:-}" ]]; then
    printf '%s' "$XDG_STATE_HOME"
  else
    printf '%s' "${HOME}/.local/state"
  fi
}

xdg_config_home() {
  if [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
    printf '%s' "$XDG_CONFIG_HOME"
  else
    printf '%s' "${HOME}/.config"
  fi
}

# User package / tool cache root (same order as echo_resolver::xo_home).
xo_home() {
  if [[ -n "${XO_HOME:-}" ]]; then
    printf '%s' "$XO_HOME"
  else
    printf '%s' "$(xdg_cache_home)/.xo"
  fi
}

xo_data_root() {
  printf '%s' "$(xdg_data_home)/xo"
}

xo_bin_dir() {
  printf '%s' "${XO_BIN_DIR:-${HOME}/.local/bin}"
}

toolchains_dir() {
  printf '%s' "$(xo_data_root)/toolchains"
}

current_link() {
  printf '%s' "$(xo_data_root)/current"
}

manifest_path() {
  printf '%s' "$(xo_data_root)/install.manifest"
}

resolve_version() {
  if [[ -n "${ECHO_VERSION:-}" ]]; then
    printf '%s' "$ECHO_VERSION"
    return
  fi
  if [[ -n "${XO_VERSION:-}" ]]; then
    printf '%s' "$XO_VERSION"
    return
  fi
  if [[ -n "${REPO_ROOT}" ]] && command -v git >/dev/null 2>&1 && [[ -d "${REPO_ROOT}/.git" ]]; then
    local desc
    desc="$(git -C "$REPO_ROOT" describe --tags --always --dirty 2>/dev/null || true)"
    if [[ -n "$desc" ]]; then
      # Sanitize for directory names.
      printf '%s' "${desc//\//-}"
      return
    fi
  fi
  # Fallback: crates/xo/Cargo.toml version
  if [[ -n "${REPO_ROOT}" && -f "${REPO_ROOT}/crates/xo/Cargo.toml" ]]; then
    local ver
    ver="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "${REPO_ROOT}/crates/xo/Cargo.toml" | head -1)"
    if [[ -n "$ver" ]]; then
      printf '%s' "v${ver}"
      return
    fi
  fi
  printf '%s' "dev"
}

# Host triple used in CI release asset names: xo-<artifact>.tar.gz
detect_release_artifact() {
  local os arch
  os="$(uname -s 2>/dev/null || echo unknown)"
  arch="$(uname -m 2>/dev/null || echo unknown)"
  case "${os}/${arch}" in
  Linux/x86_64 | Linux/amd64) printf '%s' "linux-x86_64" ;;
  Darwin/arm64 | Darwin/aarch64) printf '%s' "macos-arm64" ;;
  Darwin/x86_64)
    die "macOS Intel (x86_64) is not shipped yet; build from source or use macos-arm64"
    ;;
  Linux/aarch64 | Linux/arm64)
    die "Linux aarch64 is not shipped yet; build from source (./scripts/install.sh install)"
    ;;
  MINGW64_NT*/x86_64 | MSYS_NT*/x86_64 | CYGWIN_NT*/x86_64 | Windows_NT/x86_64)
    printf '%s' "windows-x86_64"
    ;;
  *)
    die "unsupported platform for prebuilt install: ${os}/${arch} (try building from source)"
    ;;
  esac
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

# JSON helpers without requiring jq (keep install self-contained).
github_api() {
  local url="$1"
  local args=(-fsSL -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2022-11-28")
  if [[ -n "${GITHUB_TOKEN:-${GH_TOKEN:-}}" ]]; then
    args+=(-H "Authorization: Bearer ${GITHUB_TOKEN:-${GH_TOKEN}}")
  fi
  curl "${args[@]}" "$url"
}

# Resolve release tag + browser download URL for xo-<artifact>.tar.gz
# Prints:  TAG\tURL
resolve_release_asset() {
  local want_tag="${1:-}"
  local artifact
  artifact="$(detect_release_artifact)"
  local asset_name="xo-${artifact}.tar.gz"
  local api_base="https://api.github.com/repos/${ECHO_REPO}"
  local json tag url

  need_cmd curl
  need_cmd python3
  need_cmd tar

  if [[ -z "$want_tag" || "$want_tag" == "latest" ]]; then
    # Newest published release, including prereleases. GitHub /releases/latest
    # only returns a non-prerelease and 404s while every Echo tag is still an alpha.
    json="$(github_api "${api_base}/releases?per_page=10")"
    tag="$(
      printf '%s' "$json" | python3 -c '
import json, sys
rels = json.load(sys.stdin)
for r in rels:
    if r.get("draft"):
        continue
    print(r["tag_name"])
    break
'
    )"
    [[ -n "$tag" ]] || die "could not resolve a GitHub release for ${ECHO_REPO}"
    json="$(github_api "${api_base}/releases/tags/${tag}")"
  else
    tag="$want_tag"
    json="$(github_api "${api_base}/releases/tags/${tag}")"
  fi

  [[ -n "$tag" ]] || die "could not resolve a GitHub release for ${ECHO_REPO}"

  url="$(
    printf '%s' "$json" | python3 -c '
import json, sys
name = sys.argv[1]
rel = json.load(sys.stdin)
for a in rel.get("assets") or []:
    if a.get("name") == name:
        print(a.get("browser_download_url") or "")
        break
' "$asset_name"
  )"

  if [[ -z "$url" ]]; then
    die "release ${tag} has no asset ${asset_name} (publish CI may still be running, or use a newer tag)"
  fi

  printf '%s\t%s\n' "$tag" "$url"
}

# --- layout -----------------------------------------------------------------

ensure_xdg_layout() {
  local xo home_pkg state cfg data
  xo="$(xo_home)"
  home_pkg="${xo}/packages"
  state="$(xdg_state_home)/xo"
  cfg="$(xdg_config_home)/xo"
  data="$(xo_data_root)"
  mkdir -p \
    "$home_pkg" \
    "$state" \
    "$cfg" \
    "${data}/toolchains" \
    "$(xo_bin_dir)"
  # Lightweight markers so doctor/uninstall know these are ours.
  [[ -f "${xo}/.echo-user-root" ]] || printf 'xo user root (ADR 0014)\n' >"${xo}/.echo-user-root"
  [[ -f "${data}/.echo-install-root" ]] || printf 'xo toolchain install root\n' >"${data}/.echo-install-root"
  [[ -f "${state}/.keep" ]] || : >"${state}/.keep"
  [[ -f "${cfg}/.keep" ]] || : >"${cfg}/.keep"
}

print_paths() {
  cat <<EOF
XO_HOME=$(xo_home)
XO_PACKAGES=$(xo_home)/packages
XO_DATA=$(xo_data_root)
XO_TOOLCHAINS=$(toolchains_dir)
XO_CURRENT=$(current_link)
XO_BIN_DIR=$(xo_bin_dir)
XO_BIN=$(xo_bin_dir)/xo
XDG_STATE_HOME/xo=$(xdg_state_home)/xo
XDG_CONFIG_HOME/xo=$(xdg_config_home)/xo
XO_INSTALL_ROOT (runtime)=$(if [[ -L "$(current_link)" || -d "$(current_link)" ]]; then readlink -f "$(current_link)" 2>/dev/null || printf '%s' "$(current_link)"; else echo "(not installed)"; fi)
VERSION=$(resolve_version)
REPO_ROOT=${REPO_ROOT:-(none)}
ECHO_REPO=${ECHO_REPO}
EOF
}

doctor() {
  print_paths
  echo
  local bin cur
  bin="$(xo_bin_dir)/xo"
  cur="$(current_link)"
  if [[ -e "$bin" ]]; then
    if [[ -L "$bin" ]]; then
      info "PATH link: $bin -> $(readlink "$bin" 2>/dev/null || echo '?')"
    else
      info "PATH link: $bin (not a symlink)"
    fi
    if command -v "$bin" >/dev/null 2>&1 || [[ -x "$bin" ]]; then
      info "xo --help:"
      "$bin" --help 2>&1 | head -3 || true
    fi
  else
    info "PATH link missing: $bin"
  fi
  if [[ -L "$cur" || -d "$cur" ]]; then
    info "current toolchain: $(readlink -f "$cur" 2>/dev/null || echo "$cur")"
    if [[ -d "$(readlink -f "$cur" 2>/dev/null || echo "$cur")/std" ]]; then
      info "std present under current"
    else
      info "warning: std/ missing under current"
    fi
  else
    info "no current toolchain at $cur"
  fi
  if [[ -d "$(toolchains_dir)" ]]; then
    info "installed toolchains:"
    # shellcheck disable=SC2012
    ls -1 "$(toolchains_dir)" 2>/dev/null | sed 's/^/  /' || true
  fi
  case ":${PATH}:" in
  *":$(xo_bin_dir):"*) info "PATH contains $(xo_bin_dir)" ;;
  *) info "note: add $(xo_bin_dir) to PATH if xo is not found" ;;
  esac
}

# --- build / install --------------------------------------------------------

build_xo() {
  local profile="${CARGO_PROFILE:-release}"
  [[ -n "${REPO_ROOT}" ]] || die "not an Echo checkout; use: install.sh from-release"
  info "building xo (${profile}) in ${REPO_ROOT}"
  if [[ ! -f "${REPO_ROOT}/Cargo.toml" ]]; then
    die "not an Echo checkout (missing Cargo.toml at ${REPO_ROOT})"
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    die "cargo not found (install Rust: https://rustup.rs)"
  fi
  (
    cd "$REPO_ROOT"
    if [[ "$profile" == "release" ]]; then
      cargo build -p xo --release
    else
      cargo build -p xo
    fi
  )
  if [[ "$profile" == "release" ]]; then
    printf '%s' "${REPO_ROOT}/target/release/xo"
  else
    printf '%s' "${REPO_ROOT}/target/debug/xo"
  fi
}

find_built_runtime_lib() {
  local profile="${CARGO_PROFILE:-release}"
  local root="${REPO_ROOT}/target/${profile}"
  local cand
  for cand in \
    "${root}/libecho_runtime.a" \
    "${root}/deps/libecho_runtime.a"; do
    if [[ -f "$cand" ]]; then
      printf '%s' "$cand"
      return 0
    fi
  done
  # Hashed cargo staticlib name under deps/.
  local hit
  hit="$(ls -1 "${root}/deps"/libecho_runtime-*.a 2>/dev/null | tail -1 || true)"
  if [[ -n "$hit" && -f "$hit" ]]; then
    printf '%s' "$hit"
    return 0
  fi
  return 1
}

write_manifest() {
  local version="$1"
  local prefix="$2"
  local built_from="$3"
  cat >"$(manifest_path)" <<EOF
# Generated by scripts/install.sh — do not edit by hand unless you know why.
version=${version}
prefix=${prefix}
bin=$(xo_bin_dir)/xo
built_from=${built_from}
installed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF
}

# Finalize a staged toolchain directory (bin/xo, optional runtime .a, std/).
activate_toolchain() {
  local version="$1"
  local staging="$2"
  local source_label="$3"
  local tc_dir
  tc_dir="$(toolchains_dir)/${version}"

  [[ -x "${staging}/bin/xo" ]] || die "staging missing bin/xo: ${staging}"
  [[ -d "${staging}/std" ]] || die "staging missing std/: ${staging}"

  printf '%s\n' "$version" >"${staging}/version"
  printf '%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"${staging}/installed_at"

  rm -rf "$tc_dir"
  mv "$staging" "$tc_dir"

  ln -sfn "toolchains/${version}" "$(current_link)"
  ln -sfn "$(readlink -f "$(current_link)")/bin/xo" "$(xo_bin_dir)/xo"

  write_manifest "$version" "$tc_dir" "$source_label"
  info "installed xo ${version}"
  info "  binary: $(xo_bin_dir)/xo"
  info "  root:   $tc_dir"
  info "  source: $source_label"
  info "  XO_HOME packages: $(xo_home)/packages"
  case ":${PATH}:" in
  *":$(xo_bin_dir):"*) ;;
  *)
    info "add to shell profile if needed:"
    info "  export PATH=\"$(xo_bin_dir):\$PATH\""
    ;;
  esac
}

install_version_from_checkout() {
  local version
  version="$(resolve_version)"
  local staging
  staging="$(toolchains_dir)/.${version}.staging.$$"
  local built
  built="$(build_xo)"
  [[ -x "$built" ]] || die "built binary not executable: $built"

  ensure_xdg_layout
  rm -rf "$staging"
  mkdir -p "${staging}/bin" "${staging}/std"

  info "installing toolchain ${version} → $(toolchains_dir)/${version}"
  install -m 0755 "$built" "${staging}/bin/xo"
  if runtime="$(find_built_runtime_lib 2>/dev/null)"; then
    install -m 0644 "$runtime" "${staging}/bin/libecho_runtime.a"
  else
    info "warning: libecho_runtime.a not found next to build (AOT link may need ECHO_RUNTIME_LIB)"
  fi
  if [[ -d "${REPO_ROOT}/std" ]]; then
    if command -v rsync >/dev/null 2>&1; then
      rsync -a --delete "${REPO_ROOT}/std/" "${staging}/std/"
    else
      rm -rf "${staging}/std"
      cp -a "${REPO_ROOT}/std" "${staging}/std"
    fi
  else
    die "missing std/ at ${REPO_ROOT}/std"
  fi

  activate_toolchain "$version" "$staging" "${REPO_ROOT:-checkout}"
}

cmd_from_release() {
  local want_tag="${1:-${ECHO_RELEASE:-latest}}"
  ensure_xdg_layout

  local resolved tag url artifact tmp archive staging version
  artifact="$(detect_release_artifact)"
  info "fetching prebuilt xo (${artifact}) from GitHub ${ECHO_REPO}"
  resolved="$(resolve_release_asset "$want_tag")"
  tag="${resolved%%$'\t'*}"
  url="${resolved#*$'\t'}"
  version="${ECHO_VERSION:-${XO_VERSION:-$tag}}"
  # Sanitize version for directory names.
  version="${version//\//-}"

  tmp="$(mktemp -d "${TMPDIR:-/tmp}/xo-install.XXXXXX")"
  # shellcheck disable=SC2064
  trap "rm -rf '$tmp'" RETURN

  archive="${tmp}/xo-${artifact}.tar.gz"
  info "downloading ${tag} → ${archive}"
  local curl_args=(-fL --retry 3 --retry-delay 1 -o "$archive")
  if [[ -n "${GITHUB_TOKEN:-${GH_TOKEN:-}}" ]]; then
    curl_args+=(-H "Authorization: Bearer ${GITHUB_TOKEN:-${GH_TOKEN}}")
  fi
  curl "${curl_args[@]}" "$url" || die "download failed: $url"

  staging="$(toolchains_dir)/.${version}.staging.$$"
  rm -rf "$staging"
  mkdir -p "$staging"
  info "extracting archive"
  tar -xzf "$archive" -C "$staging"

  # Normalize layout: archive may be pkg root (bin/, std/) already.
  # Windows tarballs ship bin/xo.exe; Git Bash runs `xo` from a copy named xo.
  if [[ ! -x "${staging}/bin/xo" ]]; then
    if [[ -f "${staging}/bin/xo.exe" ]]; then
      cp "${staging}/bin/xo.exe" "${staging}/bin/xo"
    elif [[ -x "${staging}/xo" ]]; then
      mkdir -p "${staging}/bin"
      mv "${staging}/xo" "${staging}/bin/xo"
    elif [[ -f "${staging}/xo.exe" ]]; then
      mkdir -p "${staging}/bin"
      mv "${staging}/xo.exe" "${staging}/bin/xo.exe"
      cp "${staging}/bin/xo.exe" "${staging}/bin/xo"
    else
      die "archive missing bin/xo (unexpected layout)"
    fi
  fi
  chmod a+x "${staging}/bin/xo" 2>/dev/null || true
  if [[ ! -d "${staging}/std" ]]; then
    die "archive missing std/ (release package incomplete)"
  fi

  activate_toolchain "$version" "$staging" "github:${ECHO_REPO}@${tag}"
  doctor
}

cmd_install() {
  if [[ -z "${REPO_ROOT}" || ! -f "${REPO_ROOT}/Cargo.toml" ]]; then
    info "no checkout detected — installing from GitHub release"
    cmd_from_release "${1:-${ECHO_RELEASE:-latest}}"
    return
  fi
  ensure_xdg_layout
  install_version_from_checkout
  doctor
}

cmd_upgrade() {
  if [[ -z "${REPO_ROOT}" || ! -f "${REPO_ROOT}/Cargo.toml" ]]; then
    info "no checkout detected — upgrading from GitHub release"
    cmd_from_release "${1:-${ECHO_RELEASE:-latest}}"
    return
  fi
  ensure_xdg_layout
  local prev=""
  if [[ -L "$(current_link)" || -d "$(current_link)" ]]; then
    prev="$(readlink -f "$(current_link)" 2>/dev/null || true)"
  fi
  install_version_from_checkout
  if [[ -n "$prev" ]]; then
    info "previous toolchain kept at: $prev"
    info "remove old toolchains under $(toolchains_dir) when ready"
  fi
}

cmd_uninstall() {
  local purge=0
  for a in "$@"; do
    case "$a" in
    --purge) purge=1 ;;
    -h | --help)
      cat <<'EOF'
Usage: install.sh uninstall [--purge]

  Removes the PATH link and toolchain install under XDG data.
  --purge  also removes $XO_HOME (packages), state, and config.
EOF
      return 0
      ;;
    *) die "unknown uninstall option: $a" ;;
    esac
  done

  local bin cur data
  bin="$(xo_bin_dir)/xo"
  cur="$(current_link)"
  data="$(xo_data_root)"

  if [[ -L "$bin" ]]; then
    local target
    target="$(readlink -f "$bin" 2>/dev/null || true)"
    if [[ -n "$target" && "$target" == "$(xo_data_root)"/* ]]; then
      rm -f "$bin"
      info "removed $bin"
    else
      info "left $bin (does not point into $(xo_data_root))"
    fi
  elif [[ -e "$bin" ]]; then
    info "left $bin (not a symlink managed by install.sh)"
  fi

  if [[ -d "$data" ]]; then
    rm -rf "$data"
    info "removed toolchain data $data"
  fi

  if [[ "$purge" -eq 1 ]]; then
    local xo state cfg
    xo="$(xo_home)"
    state="$(xdg_state_home)/xo"
    cfg="$(xdg_config_home)/xo"
    if [[ -d "$xo" ]]; then
      rm -rf "$xo"
      info "purged XO_HOME $xo"
    fi
    if [[ -d "$state" ]]; then
      rm -rf "$state"
      info "purged state $state"
    fi
    if [[ -d "$cfg" ]]; then
      rm -rf "$cfg"
      info "purged config $cfg"
    fi
  else
    info "kept package cache $(xo_home)/packages (use --purge to remove)"
    info "kept state $(xdg_state_home)/xo and config $(xdg_config_home)/xo"
  fi
}

usage() {
  cat <<'EOF'
Echo / xo installer (XDG)

Usage:
  scripts/install.sh [install]              From checkout: build + install
                                            Without checkout: same as from-release
  scripts/install.sh from-release [tag]     Newest published prerelease, or pin a tag
  scripts/install.sh upgrade                New version (build or from-release)
  scripts/install.sh uninstall [--purge]
  scripts/install.sh doctor                 Show paths and install status
  scripts/install.sh paths                  Print path assignments only

One-liner (no git clone):
  curl -fsSL https://raw.githubusercontent.com/csfh/echo/main/scripts/install.sh \
    | bash -s -- from-release

  # Pin a tag
  # … | bash -s -- from-release v0.0.1-alpha.rc.1

Current prerelease (v0.0.1-alpha.rc.1) assets: xo-linux-x86_64, xo-macos-arm64, xo-windows-x86_64.

Environment:
  XO_HOME           User .xo root (packages); default $XDG_CACHE_HOME/.xo
  XDG_DATA_HOME     Toolchain root parent (…/xo/toolchains)
  XDG_STATE_HOME    State (REPL history under …/xo)
  XDG_CONFIG_HOME   Config (…/xo)
  XO_BIN_DIR        Where to place the xo PATH link (default ~/.local/bin)
  ECHO_REPO         GitHub owner/name (default csfh/echo)
  ECHO_RELEASE      Release tag, or newest published prerelease when unset
  ECHO_VERSION / XO_VERSION   Force toolchain version directory name
  CARGO_PROFILE     release (default) or debug — checkout builds only
  GITHUB_TOKEN / GH_TOKEN     Optional; higher API rate limits

Upgrade keeps prior toolchains under $XDG_DATA_HOME/xo/toolchains/.
EOF
}

main() {
  local cmd="${1:-install}"
  shift || true
  case "$cmd" in
  install | i) cmd_install "$@" ;;
  from-release | release | prebuilt) cmd_from_release "$@" ;;
  upgrade | u) cmd_upgrade "$@" ;;
  uninstall | remove | rm) cmd_uninstall "$@" ;;
  doctor | status) doctor "$@" ;;
  paths) print_paths ;;
  -h | --help | help) usage ;;
  *)
    usage >&2
    die "unknown command: $cmd"
    ;;
  esac
}

main "$@"
