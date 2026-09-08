#!/usr/bin/env bash
# Exercise install.sh without network: install a local zip into a temp dir, check, upgrade, uninstall.
# Usage: dist/test-install.sh dist/out/sciink-<asset>.zip
set -euo pipefail
zip="${1:?usage: test-install.sh <zip>}"
here="$(cd "$(dirname "$0")/.." && pwd)"
ext="$(mktemp -d)/with space/extensions"
trap 'rm -rf "$(dirname "$(dirname "$ext")")"' EXIT

SCIINK_ZIP="$zip" SCIINK_EXT_DIR="$ext" sh "$here/install.sh"
test -x "$ext/sciink/bin/sciink" || { echo "FAIL: binary not installed"; exit 1; }
test -f "$ext/sciink/about.inx" || { echo "FAIL: inx not installed"; exit 1; }

# a stale file from an older install must disappear on re-install
touch "$ext/sciink/stale.inx"
SCIINK_ZIP="$zip" SCIINK_EXT_DIR="$ext" sh "$here/install.sh"
test ! -e "$ext/sciink/stale.inx" || { echo "FAIL: stale file survived re-install"; exit 1; }

# other extensions in the directory are untouched
mkdir -p "$ext/other_extension" && touch "$ext/other_extension/keep.inx"
SCIINK_EXT_DIR="$ext" sh "$here/install.sh" --uninstall
test ! -e "$ext/sciink" || { echo "FAIL: uninstall left sciink/"; exit 1; }
test -f "$ext/other_extension/keep.inx" || { echo "FAIL: uninstall touched another extension"; exit 1; }
echo "INSTALL-OK"
