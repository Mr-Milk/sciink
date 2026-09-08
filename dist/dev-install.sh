#!/usr/bin/env bash
# Build the release binary and symlink it plus the .inx files into Inkscape's
# user extension directory (macOS/Linux). Restart Inkscape afterwards; the
# binary is re-executed on every run, so later `cargo build --release` calls
# take effect immediately.
set -euo pipefail
cd "$(dirname "$0")/.."
case "$(uname -s)" in
  Darwin) EXT="$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink" ;;
  *)      EXT="${XDG_CONFIG_HOME:-$HOME/.config}/inkscape/extensions/sciink" ;;
esac
cargo build --release
mkdir -p "$EXT/bin"
ln -sfn "$PWD/target/release/sciink" "$EXT/bin/sciink"
for f in inx/*.inx; do ln -sfn "$PWD/$f" "$EXT/$(basename "$f")"; done
echo "installed into: $EXT"
echo "restart Inkscape, then use Extensions > Scientific"
