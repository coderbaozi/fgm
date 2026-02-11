#!/bin/bash

set -e

OWNER="coderbaozi"
REPO="fgm"
BIN_NAME="fgm"
INSTALL_DIR="/usr/local/bin"

# 颜色设置
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO] $1${NC}"; }
log_warn() { echo -e "${YELLOW}[WARN] $1${NC}"; }
log_error() { echo -e "${RED}[ERROR] $1${NC}"; }

# 1. 检测系统架构
OS="$(uname -s)"
ARCH="$(uname -m)"

case $OS in
    Linux)  OS_KEY="linux" ;;
    Darwin) OS_KEY="darwin" ;; # 稍后我们会同时匹配 darwin, mac, osx
    *) log_error "不支持的操作系统: $OS"; exit 1 ;;
esac

case $ARCH in
    x86_64|amd64) ARCH_KEY="amd64" ;;
    aarch64|arm64) ARCH_KEY="arm64" ;;
    *) log_error "不支持的架构: $ARCH"; exit 1 ;;
esac

log_info "检测到系统: $OS ($ARCH)"

# 2. 获取下载链接 (HTML 抓取模式，绕过 API 限制)
log_info "正在获取最新版本 (通过 HTML 页面)..."

# 获取 Latest Release 的页面 HTML
# 使用 -L 跟随重定向，直接获取最新版本的页面内容
HTML_CONTENT=$(curl -sL "https://github.com/$OWNER/$REPO/releases/latest")

if [ -z "$HTML_CONTENT" ]; then
    log_error "无法连接到 GitHub 网页，请检查网络。"
    exit 1
fi

# 3. 提取下载链接
# 使用 grep 和 sed 从 HTML 中提取所有 href 包含 /download/ 的链接
# 格式通常是: /coderbaozi/fgm/releases/download/v1.0.0/filename.tar.gz
URL_LIST=$(echo "$HTML_CONTENT" | grep -oE 'href="[^"]*releases/download/[^"]*"' | sed 's/href="//;s/"//')

if [ -z "$URL_LIST" ]; then
    # 备用方案：如果 latest 页面抓取失败，尝试抓取 expanded_assets (GitHub 新版页面结构)
    # 先获取 tag 名称
    TAG_NAME=$(echo "$HTML_CONTENT" | grep -oE 'releases/tag/[^"]+' | head -n 1 | awk -F/ '{print $NF}')
    if [ -n "$TAG_NAME" ]; then
        HTML_CONTENT=$(curl -sL "https://github.com/$OWNER/$REPO/releases/expanded_assets/$TAG_NAME")
        URL_LIST=$(echo "$HTML_CONTENT" | grep -oE 'href="[^"]*releases/download/[^"]*"' | sed 's/href="//;s/"//')
    fi
fi

if [ -z "$URL_LIST" ]; then
    log_error "未能从页面解析出下载链接。"
    exit 1
fi

# 4. 匹配最佳文件
# 函数：根据关键字筛选链接
filter_url() {
    local keyword1=$1
    local keyword2=$2
    # 排除 .sha256, .md5, .txt 等非二进制文件
    echo "$URL_LIST" | grep -i "$keyword1" | grep -i "$keyword2" | grep -vE '\.(txt|md|sha256|sha|sig|pem)$' | head -n 1
}

# 尝试匹配: OS + Arch
MATCH_PATH=$(filter_url "$OS_KEY" "$ARCH_KEY")

# 特殊处理：如果 macOS 没找到 "darwin"，尝试找 "mac"
if [ -z "$MATCH_PATH" ] && [ "$OS_KEY" == "darwin" ]; then
    MATCH_PATH=$(filter_url "mac" "$ARCH_KEY")
fi

# 特殊处理：如果 macOS arm64 没找到，尝试找 amd64 (Rosetta)
if [ -z "$MATCH_PATH" ] && [ "$OS_KEY" == "darwin" ] && [ "$ARCH_KEY" == "arm64" ]; then
    log_warn "未找到原生 arm64 包，尝试下载 amd64 包..."
    MATCH_PATH=$(filter_url "$OS_KEY" "amd64")
    # 如果 darwin+amd64 也没找到，试 mac+amd64
    if [ -z "$MATCH_PATH" ]; then
        MATCH_PATH=$(filter_url "mac" "amd64")
    fi
fi

if [ -z "$MATCH_PATH" ]; then
    log_error "未找到匹配当前系统的文件。"
    echo "------------------------------------------------"
    echo "发现的所有文件列表："
    echo "$URL_LIST" | awk -F/ '{print $NF}'
    echo "------------------------------------------------"
    exit 1
fi

# 拼接完整 URL
DOWNLOAD_URL="https://github.com$MATCH_PATH"
FILENAME=$(basename "$DOWNLOAD_URL")

log_info "找到文件: $FILENAME"

# 5. 下载并安装
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

cd "$TMP_DIR"
log_info "开始下载..."
# 增加重试机制
curl -L --retry 3 -o "$FILENAME" "$DOWNLOAD_URL"

log_info "解压中..."
if [[ "$FILENAME" == *.tar.gz ]] || [[ "$FILENAME" == *.tgz ]]; then
    tar -xzf "$FILENAME"
elif [[ "$FILENAME" == *.zip ]]; then
    unzip -q "$FILENAME"
else
    chmod +x "$FILENAME"
    mv "$FILENAME" "$BIN_NAME" 2>/dev/null || true
fi

# 寻找二进制文件
if [ ! -f "$BIN_NAME" ]; then
    # 排除 html, txt, md, hidden files, 自身压缩包
    FOUND_BIN=$(find . -type f -not -name "*.*" -not -name "LICENSE" -not -name "README" | head -n 1)
    if [ -n "$FOUND_BIN" ]; then
        mv "$FOUND_BIN" "$BIN_NAME"
    fi
fi

if [ ! -f "$BIN_NAME" ]; then
    # 再次尝试模糊匹配
    FOUND_BIN=$(find . -type f -name "*$BIN_NAME*" -not -name "*.gz" -not -name "*.zip" | head -n 1)
    if [ -n "$FOUND_BIN" ]; then
        mv "$FOUND_BIN" "$BIN_NAME"
    fi
fi

if [ ! -f "$BIN_NAME" ]; then
    log_error "解压后未找到二进制文件。"
    ls -R
    exit 1
fi

log_info "安装到 $INSTALL_DIR..."
if [ -w "$INSTALL_DIR" ]; then
    mv "$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
else
    sudo mv "$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
fi

sudo chmod +x "$INSTALL_DIR/$BIN_NAME"

log_info "安装成功！"
"$BIN_NAME" --version || echo "完成"
