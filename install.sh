#!/bin/sh
# sciink installer for macOS and Linux.
#   curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh
# Options (environment):
#   SCIINK_VERSION=v0.1.0   install a specific release tag (default: latest)
#   SCIINK_EXT_DIR=<dir>    Inkscape user extensions directory (default: detected)
#   SCIINK_ZIP=<file>       install from a local zip instead of downloading
# Flags: --uninstall        remove the sciink folder from the extensions directory
set -eu

main() {
  REPO="Mr-Milk/sciink"
  VERSION="${SCIINK_VERSION:-latest}"

  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Darwin)
      asset="sciink-macos-universal.zip"
      default_ext="$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions"
      ;;
    Linux)
      case "$arch" in
        x86_64|amd64) asset="sciink-linux-x64.zip" ;;
        *) echo "sciink: no prebuilt binary for Linux/$arch yet. Build from source: cargo build --release, then run dist/dev-install.sh" >&2; exit 1 ;;
      esac
      if [ -d "$HOME/.var/app/org.inkscape.Inkscape" ]; then
        default_ext="$HOME/.var/app/org.inkscape.Inkscape/config/inkscape/extensions"
      else
        default_ext="${XDG_CONFIG_HOME:-$HOME/.config}/inkscape/extensions"
      fi
      ;;
    *)
      echo "sciink: unsupported OS '$os'. On Windows run: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" >&2
      exit 1
      ;;
  esac
  EXT="${SCIINK_EXT_DIR:-$default_ext}"

  if [ "${1:-}" = "--uninstall" ]; then
    rm -rf "$EXT/sciink"
    echo "sciink: removed $EXT/sciink"
    exit 0
  fi

  # Verify we have the tools we'll need before touching anything on disk.
  if [ -z "${SCIINK_ZIP:-}" ]; then
    if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
      echo "sciink: need curl or wget" >&2
      exit 1
    fi
  fi
  if ! command -v unzip >/dev/null 2>&1 && ! command -v bsdtar >/dev/null 2>&1; then
    echo "sciink: need unzip or bsdtar" >&2
    exit 1
  fi

  if [ "$VERSION" = "latest" ]; then
    url="https://github.com/$REPO/releases/latest/download/$asset"
  else
    url="https://github.com/$REPO/releases/download/$VERSION/$asset"
  fi

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  zip="$tmp/$asset"

  if [ -n "${SCIINK_ZIP:-}" ]; then
    cp "$SCIINK_ZIP" "$zip"
  else
    echo "sciink: downloading $url"
    if command -v curl >/dev/null 2>&1; then
      curl -fsSL -o "$zip" "$url"
    else
      wget -qO "$zip" "$url"
    fi
  fi

  # Stage the extraction in a scratch directory and validate it fully before
  # touching the real extensions directory, so a bad download or a missing
  # tool never destroys an existing install.
  mkdir -p "$tmp/extract"
  if command -v unzip >/dev/null 2>&1; then
    unzip -oq "$zip" -d "$tmp/extract"
  else
    bsdtar -xf "$zip" -C "$tmp/extract"
  fi
  test -f "$tmp/extract/sciink/bin/sciink" || { echo "sciink: archive did not contain sciink/bin/sciink" >&2; exit 1; }
  chmod +x "$tmp/extract/sciink/bin/sciink"
  "$tmp/extract/sciink/bin/sciink" --version >/dev/null 2>&1 || { echo "sciink: the downloaded binary does not run on this system ($os/$arch)" >&2; exit 1; }

  mkdir -p "$EXT" && rm -rf "$EXT/sciink" && mv "$tmp/extract/sciink" "$EXT/sciink"
  if [ "$os" = "Darwin" ]; then
    xattr -dr com.apple.quarantine "$EXT/sciink" 2>/dev/null || true
  fi

  ver="$("$EXT/sciink/bin/sciink" --version)"
  echo "sciink: installed $ver into $EXT/sciink"
  echo "sciink: restart Inkscape; the tools are under Extensions > Scientific"
}

main "$@"
