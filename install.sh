#!/bin/sh
# sciink installer for macOS and Linux.
#   curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh
# Options (environment):
#   SCIINK_VERSION=v0.1.0   install a specific release tag (default: latest)
#   SCIINK_EXT_DIR=<dir>    Inkscape user extensions directory (default: detected;
#                           set this for Snap/Flatpak or other non-default layouts)
#   SCIINK_ZIP=<file>       install from a local zip instead of downloading
# Flags: --uninstall        remove the sciink folder from the extensions directory
#   curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh -s -- --uninstall
set -eu

main() {
  REPO="Mr-Milk/sciink"
  VERSION="${SCIINK_VERSION:-latest}"
  if [ "$VERSION" != "latest" ]; then
    VERSION="v${VERSION#v}"
  fi

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
    rm -rf "$EXT"/.sciink-stage.* 2>/dev/null || true
    echo "sciink: removed $EXT/sciink"
    exit 0
  fi

  # Start of the install path: clear any staging leftovers from a crashed
  # previous run before doing anything else. Inkscape scans subdirectories
  # for .inx files, so a stray staging dir would create duplicate menu
  # entries. mkdir here (rather than down by the final swap) because the
  # staging dir below is created inside $EXT and needs it to already exist.
  mkdir -p "$EXT"
  rm -rf "$EXT"/.sciink-stage.* 2>/dev/null || true

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

  fetch() {
    # fetch <url> <dest> - download with curl or wget, whichever is available.
    if command -v curl >/dev/null 2>&1; then
      curl -fsSL -o "$2" "$1"
    else
      wget -nv -O "$2" "$1"
    fi
  }

  tmp="$(mktemp -d)"
  stage=""
  cleanup() {
    # Each removal tolerates failure: under set -e a failing first rm would
    # otherwise abort the trap and leave the staging dir inside $EXT.
    rm -rf "$tmp" 2>/dev/null || true
    [ -z "$stage" ] || rm -rf "$stage" 2>/dev/null || true
  }
  trap cleanup EXIT
  zip="$tmp/$asset"

  if [ -n "${SCIINK_ZIP:-}" ]; then
    cp "$SCIINK_ZIP" "$zip"
  elif [ "$VERSION" = "latest" ]; then
    url="https://github.com/$REPO/releases/latest/download/$asset"
    echo "sciink: downloading $url"
    if ! fetch "$url" "$zip"; then
      # GitHub's "latest" release excludes pre-releases, so this 404s until
      # the first stable release exists. Fall back to the newest release of
      # any kind.
      echo "sciink: no stable release yet, checking for a pre-release"
      list="$tmp/releases.json"
      fetch "https://api.github.com/repos/$REPO/releases?per_page=1" "$list" || {
        echo "sciink: failed to query releases for $REPO" >&2
        exit 1
      }
      tag="$(sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "$list" | head -1)"
      [ -n "$tag" ] || { echo "sciink: no releases found for $REPO" >&2; exit 1; }
      url="https://github.com/$REPO/releases/download/$tag/$asset"
      echo "sciink: downloading $url"
      fetch "$url" "$zip" || { echo "sciink: download failed: $url" >&2; exit 1; }
    fi
  else
    url="https://github.com/$REPO/releases/download/$VERSION/$asset"
    echo "sciink: downloading $url"
    fetch "$url" "$zip" || { echo "sciink: download failed: $url" >&2; exit 1; }
  fi

  # Stage the extraction inside the destination directory (not the system
  # temp dir) so the final swap below is a same-volume rename, and validate
  # it fully before touching the real extensions directory, so a bad
  # download or a missing tool never destroys an existing install.
  stage="$(mktemp -d "$EXT/.sciink-stage.XXXXXX")"
  if command -v unzip >/dev/null 2>&1; then
    unzip -oq "$zip" -d "$stage"
  else
    bsdtar -xf "$zip" -C "$stage"
  fi
  test -f "$stage/sciink/bin/sciink" || { echo "sciink: archive did not contain sciink/bin/sciink" >&2; exit 1; }
  chmod +x "$stage/sciink/bin/sciink"
  if [ "$os" = "Darwin" ]; then
    # Clear quarantine on the staged tree before we ever execute it: a zip
    # passed via SCIINK_ZIP that was downloaded by a browser propagates the
    # quarantine flag to the extracted binary, and Gatekeeper would kill the
    # --version check below with a misleading "does not run" message.
    xattr -dr com.apple.quarantine "$stage/sciink" 2>/dev/null || true
  fi
  "$stage/sciink/bin/sciink" --version >/dev/null 2>&1 || { echo "sciink: the downloaded binary does not run on this system ($os/$arch)" >&2; exit 1; }

  rm -rf "$EXT/sciink"
  mv "$stage/sciink" "$EXT/sciink"

  ver="$("$EXT/sciink/bin/sciink" --version)"
  echo "sciink: installed $ver into $EXT/sciink"
  echo "sciink: restart Inkscape; the tools are under Extensions > Scientific"
}

main "$@"
