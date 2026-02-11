#!/bin/bash

set -e

RELEASE="latest"
OS="$(uname -s)"

case "${OS}" in
   MINGW* | Win*) OS="Windows" ;;
esac

if [ -d "$HOME/.fgm" ]; then
  INSTALL_DIR="$HOME/.fgm"
elif [ -n "$XDG_DATA_HOME" ]; then
  INSTALL_DIR="$XDG_DATA_HOME/fgm"
elif [ "$OS" = "Darwin" ]; then
  INSTALL_DIR="$HOME/Library/Application Support/fgm"
else
  INSTALL_DIR="$HOME/.local/share/fgm"
fi

# Parse Flags
parse_args() {
  while [[ $# -gt 0 ]]; do
    key="$1"

    case $key in
    -d | --install-dir)
      INSTALL_DIR="$2"
      shift # past argument
      shift # past value
      ;;
    -s | --skip-shell)
      SKIP_SHELL="true"
      shift # past argument
      ;;
    -r | --release)
      RELEASE="$2"
      shift # past release argument
      shift # past release value
      ;;
    *)
      echo "Unrecognized argument $key"
      exit 1
      ;;
    esac
  done
}

set_filename() {
  if [ "$OS" = "Linux" ]; then
    # Based on https://stackoverflow.com/a/45125525
    case "$(uname -m)" in
      arm | armv7*)
        FILENAME="fgm-arm32"
        ;;
      aarch* | armv8*)
        FILENAME="fgm-arm64"
        ;;
      *)
        FILENAME="fgm-linux"
    esac
  elif [ "$OS" = "Darwin" ]; then
    FILENAME="fgm-macos"
    echo "Downloading the latest fgm binary from GitHub..."
  elif [ "$OS" = "Windows" ] ; then
    FILENAME="fgm-windows"
    echo "Downloading the latest fgm binary from GitHub..."
  else
    echo "OS $OS is not supported."
    echo "If you think that's a bug - please file an issue to https://github.com/coderbaozi/fgm/issues"
    exit 1
  fi
}

download_fgm() {
  if [ "$RELEASE" = "latest" ]; then
    URL="https://github.com/coderbaozi/fgm/releases/latest/download/$FILENAME.zip"
  else
    URL="https://github.com/coderbaozi/fgm/releases/download/$RELEASE/$FILENAME.zip"
  fi

  DOWNLOAD_DIR=$(mktemp -d)

  echo "Downloading $URL..."

  mkdir -p "$INSTALL_DIR" &>/dev/null

  if ! curl --progress-bar --fail -L "$URL" -o "$DOWNLOAD_DIR/$FILENAME.zip"; then
    echo "Download failed.  Check that the release/filename are correct."
    exit 1
  fi

  unzip -q "$DOWNLOAD_DIR/$FILENAME.zip" -d "$DOWNLOAD_DIR"

  if [ -f "$DOWNLOAD_DIR/fgm" ]; then
    mv "$DOWNLOAD_DIR/fgm" "$INSTALL_DIR/fgm"
  else
    mv "$DOWNLOAD_DIR/$FILENAME/fgm" "$INSTALL_DIR/fgm"
  fi

  chmod u+x "$INSTALL_DIR/fgm"
}

check_dependencies() {
  echo "Checking dependencies for the installation script..."

  echo -n "Checking availability of curl... "
  if hash curl 2>/dev/null; then
    echo "OK!"
  else
    echo "Missing!"
    SHOULD_EXIT="true"
  fi

  echo -n "Checking availability of unzip... "
  if hash unzip 2>/dev/null; then
    echo "OK!"
  else
    echo "Missing!"
    SHOULD_EXIT="true"
  fi

  if [ "$SHOULD_EXIT" = "true" ]; then
    echo "Not installing fgm due to missing dependencies."
    exit 1
  fi
}

ensure_containing_dir_exists() {
  local CONTAINING_DIR
  CONTAINING_DIR="$(dirname "$1")"
  if [ ! -d "$CONTAINING_DIR" ]; then
    echo " >> Creating directory $CONTAINING_DIR"
    mkdir -p "$CONTAINING_DIR"
  fi
}

setup_shell() {
  CURRENT_SHELL="$(basename "$SHELL")"

  if [ "$CURRENT_SHELL" = "zsh" ]; then
    CONF_FILE=${ZDOTDIR:-$HOME}/.zshrc
    ensure_containing_dir_exists "$CONF_FILE"
    echo "Installing for Zsh. Appending the following to $CONF_FILE:"
    {
      echo ''
      echo '# fgm'
      echo 'fgm_PATH="'"$INSTALL_DIR"'"'
      echo 'if [ -d "$fgm_PATH" ]; then'
      echo '  export PATH="$fgm_PATH:$PATH"'
      echo '  eval "`fgm env`"'
      echo 'fi'
    } | tee -a "$CONF_FILE"

  elif [ "$CURRENT_SHELL" = "fish" ]; then
    CONF_FILE=$HOME/.config/fish/conf.d/fgm.fish
    ensure_containing_dir_exists "$CONF_FILE"
    echo "Installing for Fish. Appending the following to $CONF_FILE:"
    {
      echo ''
      echo '# fgm'
      echo 'set fgm_PATH "'"$INSTALL_DIR"'"'
      echo 'if [ -d "$fgm_PATH" ]'
      echo '  set PATH "$fgm_PATH" $PATH'
      echo '  fgm env | source'
      echo 'end'
    } | tee -a "$CONF_FILE"

  elif [ "$CURRENT_SHELL" = "bash" ]; then
    if [ "$OS" = "Darwin" ]; then
      CONF_FILE=$HOME/.profile
    else
      CONF_FILE=$HOME/.bashrc
    fi
    ensure_containing_dir_exists "$CONF_FILE"
    echo "Installing for Bash. Appending the following to $CONF_FILE:"
    {
      echo ''
      echo '# fgm'
      echo 'fgm_PATH="'"$INSTALL_DIR"'"'
      echo 'if [ -d "$fgm_PATH" ]; then'
      echo '  export PATH="$fgm_PATH:$PATH"'
      echo '  eval "`fgm env`"'
      echo 'fi'
    } | tee -a "$CONF_FILE"

  else
    echo "Could not infer shell type. Please set up manually."
    exit 1
  fi

  echo ""
  echo "In order to apply the changes, open a new terminal or run the following command:"
  echo ""
  echo "  source $CONF_FILE"
}

parse_args "$@"
set_filename
check_dependencies
download_fgm
if [ "$SKIP_SHELL" != "true" ]; then
  setup_shell
fi
