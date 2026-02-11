#!/usr/bin/env sh
set -eu

# fgm installer (macOS/Linux)
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/<OWNER>/<REPO>/main/scripts/install.sh | sh
#
# Optional environment variables:
#   FGM_REPO=owner/repo        # Default: coderbaozi/fgm (change to your repo)
#   FGM_VERSION=v0.1.0         # Default: latest
#   FGM_BIN_DIR=$HOME/.local/bin

REPO="${FGM_REPO:-coderbaozi/fgm}"
VERSION="${FGM_VERSION:-}"
BIN_DIR="${FGM_BIN_DIR:-${HOME}/.local/bin}"

GREEN='\033[0;32m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m'

die() {
  # shellcheck disable=SC2059
  printf "%b[fgm] %s%b\n" "$RED" "$*" "$NC" 1>&2
  exit 1
}

info() {
  # shellcheck disable=SC2059
  printf "%b[fgm] %s%b\n" "$GREEN" "$*" "$NC"
}

warn() {
  # shellcheck disable=SC2059
  printf "%b[fgm] %s%b\n" "$YELLOW" "$*" "$NC" 1>&2
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "Missing command: $1"
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

http_get() {
  url="$1"
  if have_cmd curl; then
    curl -fsSL "$url"
    return 0
  fi
  if have_cmd wget; then
    wget -qO- "$url"
    return 0
  fi
  die "Please install curl or wget first"
}

http_download() {
  url="$1"
  out="$2"
  if have_cmd curl; then
    curl -fsSL "$url" -o "$out"
    return 0
  fi
  if have_cmd wget; then
    wget -qO "$out" "$url"
    return 0
  fi
  die "Please install curl or wget first"
}

get_release_json() {
  # If VERSION is empty, use latest; otherwise use tags/<VERSION>
  if [ -z "$VERSION" ]; then
    http_get "https://api.github.com/repos/${REPO}/releases/latest"
  else
    http_get "https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"
  fi
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os="apple-darwin" ;;
    Linux) os="linux" ;;
    *) die "Unsupported OS: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *) die "Unsupported architecture: $arch" ;;
  esac

  echo "${arch}-${os}"
}

verify_sha256() {
  file="$1"
  sumfile="$2"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$sumfile" >/dev/null 2>&1 || die "SHA256 verification failed: $file"
    return 0
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$sumfile" >/dev/null 2>&1 || die "SHA256 verification failed: $file"
    return 0
  fi

  die "Missing sha256sum/shasum; cannot verify downloaded file"
}

extract_download_url() {
  # From all browser_download_url entries, match arch + os and prefer .tar.gz
  # Output: url
  json="$1"
  arch_key="$2"
  os_key="$3"

  # Avoid jq: extract all download URLs first, then filter.
  # shellcheck disable=SC2001
  echo "$json" \
    | grep '"browser_download_url"' \
    | sed -n 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
    | grep "$arch_key" \
    | grep "$os_key" \
    | grep '\.tar\.gz$' \
    | head -n 1
}

basename_url() {
  # POSIX-ish basename (avoid relying on basename in some environments)
  echo "$1" | awk -F/ '{print $NF}'
}

main() {
  need_cmd tar

  target="$(detect_target)"
  arch_key="${target%%-*}"
  os_key="${target#*-}"

  info "Fetching release: ${REPO} ${VERSION:-latest}"
  release_json="$(get_release_json)"
  if echo "$release_json" | grep -q "Not Found"; then
    die "Failed to fetch release info (repo/tag may not exist, or network is restricted): ${REPO} ${VERSION:-latest}"
  fi

  url="$(extract_download_url "$release_json" "$arch_key" "$os_key")"
  [ -n "$url" ] || die "No matching asset found for current platform (${arch_key}-${os_key})"
  archive="$(basename_url "$url")"
  sum_url="${url}.sha256"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  cd "$tmp"

  info "Downloading: ${url}"
  http_download "$url" "$archive"

  # SHA256: best-effort (skip verification if the release doesn't provide a .sha256)
  if http_download "$sum_url" "${archive}.sha256" 2>/dev/null; then
    verify_sha256 "$archive" "${archive}.sha256"
  else
    warn "Checksum file not found: ${sum_url}; skipping SHA256 verification"
  fi

  tar -xzf "$archive"

  # Compatibility: the binary may be inside a subdirectory after extraction
  bin_path=""
  if [ -f "./fgm" ]; then
    bin_path="./fgm"
  else
    bin_path="$(find . -type f -name fgm | head -n 1 || true)"
  fi
  [ -n "$bin_path" ] || die "fgm binary not found after extraction"

  mkdir -p "$BIN_DIR"
  if command -v install >/dev/null 2>&1; then
    install -m 0755 "$bin_path" "${BIN_DIR}/fgm"
  else
    cp "$bin_path" "${BIN_DIR}/fgm"
    chmod 0755 "${BIN_DIR}/fgm"
  fi

  info "Installed: ${BIN_DIR}/fgm"
  if ! echo ":${PATH}:" | grep -q ":${BIN_DIR}:"; then
    warn "Tip: add the following to your shell config to ensure PATH is updated:"
    printf '%s\n' "  export PATH=\"${BIN_DIR}:\$PATH\""
  fi
  info "Try: fgm --help"
}

main "$@"
