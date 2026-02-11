# fgm

`fgm` is a Go version manager written in Rust (macOS/Linux first). It supports downloading/installing, switching, listing, uninstalling, printing the current version, and cleaning cache.

## Features

- Install: `install <version>` (supports `go1.22.4` / `1.22.4` / `1.22` / `latest`)
- Switch: `use <version>`
- Current: `current`
- List installed: `list`
- Uninstall: `uninstall <version>`
- Latest stable: `latest`
- Cache: `cache dir|size|clean`
- Diagnose: `doctor`

## Install & build

This project is currently used from source:

```bash
cargo build --release
./target/release/fgm --help
```

## Install via Release

```bash
curl -fsSL https://raw.githubusercontent.com/coderbaozi/fgm/main/scripts/install.sh | sh
```

Optional environment variables:
- `FGM_BIN_DIR`: default `~/.local/bin`

## Quick start

1) Install and switch to a version:

```bash
fgm install 1.22 --use
fgm current
```

2) Ensure `PATH` includes the shim directory (default: `~/.fgm/bin`):

```bash
export PATH="$HOME/.fgm/bin:$PATH"
```

3) Switch versions:

```bash
fgm use 1.22.4
go version
```

## Version resolution rules

- `latest`: fetch the latest stable tag via `https://go.dev/VERSION?m=text` (or an equivalent URL based on `--mirror`).
- `1.22.4` / `go1.22.4`: install the specified patch directly.
- `1.22`: resolve to the latest stable patch under the same minor (e.g. `go1.22.4`).
  - Resolution: request `{mirror}/?mode=json&include=all`, filter `stable=true` entries under the same minor, and pick the highest patch.

## Directory layout (default)

Root: `~/.fgm`

- `~/.fgm/versions/<tag>/go`: extracted Go installation directory
- `~/.fgm/cache`: download cache (`.tar.gz`)
- `~/.fgm/current`: symlink pointing to the current version
- `~/.fgm/bin/go`, `~/.fgm/bin/gofmt`: shims (symlinks to `current/go/bin/*`)

## Global options

- `--root <DIR>`: set `fgm` root directory (default: `~/.fgm`)
- `--mirror <URL>`: set download mirror (default: `https://go.dev/dl`)
- `--quiet`: reduce output
- `--verbose`: print more debug information

Example:

```bash
fgm --mirror https://go.dev/dl install 1.22 --use
```

## Verification & overwrite

- SHA256 verification is enabled by default; use `install --no-verify` to skip.
- If the target version already exists, use `install --force` to overwrite.

## Troubleshooting

```bash
fgm doctor
fgm cache size
fgm cache clean
```

## Known limitations

- Windows is not supported for now (the shim relies on symlink).
- If the mirror specified by `--mirror` does not support `/dl/?mode=json`, `install 1.22` cannot resolve the latest patch automatically (use the default `https://go.dev/dl` or a compatible mirror).
