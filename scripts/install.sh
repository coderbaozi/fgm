#!/usr/bin/env sh
set -eu

# fgm installer (macOS/Linux)
#
# 用法示例：
#   curl -fsSL https://raw.githubusercontent.com/<OWNER>/<REPO>/main/scripts/install.sh | sh
#
# 可选环境变量：
#   FGM_REPO=owner/repo        # 默认：coderbaozi/fgm（请按实际仓库修改）
#   FGM_VERSION=v0.1.0         # 默认：latest
#   FGM_BIN_DIR=$HOME/.local/bin

REPO="${FGM_REPO:-coderbaozi/fgm}"
VERSION="${FGM_VERSION:-}"
BIN_DIR="${FGM_BIN_DIR:-${HOME}/.local/bin}"

die() {
  echo "[fgm] $*" 1>&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "缺少命令：$1"
}

get_latest_tag() {
  need_cmd curl
  # 不依赖 jq，简单解析 tag_name。
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]\+\)".*/\1/p' \
    | head -n 1
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os="apple-darwin" ;;
    Linux) os="unknown-linux-gnu" ;;
    *) die "不支持的系统：$os" ;;
  esac

  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *) die "不支持的架构：$arch" ;;
  esac

  echo "${arch}-${os}"
}

verify_sha256() {
  file="$1"
  sumfile="$2"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$sumfile" >/dev/null 2>&1 || die "SHA256 校验失败：$file"
    return 0
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$sumfile" >/dev/null 2>&1 || die "SHA256 校验失败：$file"
    return 0
  fi

  die "缺少 sha256sum/shasum，无法校验下载文件"
}

main() {
  need_cmd curl
  need_cmd tar

  target="$(detect_target)"
  if [ -z "$VERSION" ]; then
    VERSION="$(get_latest_tag)"
  fi
  [ -n "$VERSION" ] || die "无法解析最新版本号（请设置 FGM_VERSION，例如 v0.1.0）"

  archive="fgm-${VERSION}-${target}.tar.gz"
  base="https://github.com/${REPO}/releases/download/${VERSION}"
  url="${base}/${archive}"
  sum_url="${url}.sha256"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  cd "$tmp"

  echo "[fgm] 下载：${url}"
  curl -fsSL "$url" -o "$archive"
  curl -fsSL "$sum_url" -o "${archive}.sha256"

  verify_sha256 "$archive" "${archive}.sha256"

  tar -xzf "$archive"
  [ -f fgm ] || die "解压后未找到 fgm 可执行文件"

  mkdir -p "$BIN_DIR"
  if command -v install >/dev/null 2>&1; then
    install -m 0755 "fgm" "${BIN_DIR}/fgm"
  else
    cp "fgm" "${BIN_DIR}/fgm"
    chmod 0755 "${BIN_DIR}/fgm"
  fi

  echo "[fgm] 安装完成：${BIN_DIR}/fgm"
  if ! echo ":${PATH}:" | grep -q ":${BIN_DIR}:"; then
    echo "[fgm] 提示：请将以下内容加入你的 shell 配置以确保 PATH 生效："
    echo "  export PATH=\"${BIN_DIR}:\$PATH\""
  fi
  echo "[fgm] 试试：fgm --help"
}

main "$@"

