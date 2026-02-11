#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/release_package.sh <target_triple> <asset_name> [bin_name] [outdir]

Examples:
  scripts/release_package.sh x86_64-unknown-linux-gnu fgm-linux.zip
  scripts/release_package.sh x86_64-apple-darwin fgm-macos.zip fgm dist
EOF
}

if [[ ${1:-} == "-h" || ${1:-} == "--help" ]]; then
  usage
  exit 0
fi

target="${1:-}"
asset="${2:-}"
bin="${3:-fgm}"
outdir="${4:-dist}"

if [[ -z "${target}" || -z "${asset}" ]]; then
  usage 1>&2
  exit 2
fi

bin_path="target/${target}/release/${bin}"
if [[ ! -f "${bin_path}" ]]; then
  echo "binary not found: ${bin_path}" 1>&2
  exit 1
fi

mkdir -p "${outdir}"
tmp="$(mktemp -d)"
cleanup() {
  rm -rf "${tmp}" || true
}
trap cleanup EXIT

cp "${bin_path}" "${tmp}/${bin}"
chmod +x "${tmp}/${bin}"

# Use python zipfile for portability (macOS runner has no 'zip' by default in some images).
OUTDIR="${outdir}" ARCHIVE="${asset}" BIN="${bin}" TMP="${tmp}" python3 - <<'PY'
import os
import zipfile

outdir = os.environ["OUTDIR"]
archive = os.environ["ARCHIVE"]
bin_name = os.environ["BIN"]
tmp = os.environ["TMP"]

zip_path = os.path.join(outdir, archive)
bin_path = os.path.join(tmp, bin_name)

with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
  zf.write(bin_path, arcname=bin_name)
PY

echo "packaged: ${outdir}/${asset}"

