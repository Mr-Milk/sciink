#!/usr/bin/env bash
# Exercise install.sh without network: install a local zip into a temp dir, check, upgrade, uninstall.
# Usage: dist/test-install.sh dist/out/sciink-<asset>.zip
set -euo pipefail
zip="${1:?usage: test-install.sh <zip>}"
here="$(cd "$(dirname "$0")/.." && pwd)"
ext="$(mktemp -d)/with space/extensions"
minimal_bin=""
cleanup() {
  rm -rf "$(dirname "$(dirname "$ext")")"
  [ -z "$minimal_bin" ] || rm -rf "$minimal_bin"
}
trap cleanup EXIT

# After a (re)install, no crash-leftover staging dir may remain, and $ext
# must contain nothing but sciink/ and the pre-existing sibling extension.
assert_ext_layout() {
  local stray entries
  stray="$(find "$ext" -maxdepth 1 -name '.sciink-stage.*')"
  [ -z "$stray" ] || { echo "FAIL: leftover staging dir: $stray"; exit 1; }
  entries="$(ls -A "$ext" | sort | tr '\n' ' ')"
  [ "$entries" = "other_extension sciink " ] || { echo "FAIL: unexpected entries in \$ext: $entries"; exit 1; }
}

# a sibling extension must never be touched by any install.sh operation below
mkdir -p "$ext/other_extension" && touch "$ext/other_extension/keep.inx"

SCIINK_ZIP="$zip" SCIINK_EXT_DIR="$ext" sh "$here/install.sh"
test -x "$ext/sciink/bin/sciink" || { echo "FAIL: binary not installed"; exit 1; }
test -f "$ext/sciink/about.inx" || { echo "FAIL: inx not installed"; exit 1; }
test -f "$ext/other_extension/keep.inx" || { echo "FAIL: install touched another extension"; exit 1; }
assert_ext_layout

# a stale file from an older install must disappear on re-install
touch "$ext/sciink/stale.inx"
SCIINK_ZIP="$zip" SCIINK_EXT_DIR="$ext" sh "$here/install.sh"
test ! -e "$ext/sciink/stale.inx" || { echo "FAIL: stale file survived re-install"; exit 1; }
test -f "$ext/other_extension/keep.inx" || { echo "FAIL: re-install touched another extension"; exit 1; }
assert_ext_layout

# a broken PATH (no unzip/bsdtar) must fail closed and never touch the existing install
minimal_bin="$(mktemp -d)"
skip_negative=0
for tool in sh uname mktemp cp mkdir rm chmod mv test dirname basename; do
  tool_path="$(command -v "$tool" 2>/dev/null || true)"
  if [ -z "$tool_path" ]; then
    echo "SKIP: negative PATH check (no '$tool' available to build a minimal PATH)"
    skip_negative=1
    break
  fi
  case "$tool_path" in
    /*) ln -s "$tool_path" "$minimal_bin/$tool" ;;
    *) : ;; # a shell builtin (e.g. "test") needs no PATH entry
  esac
done
if [ "$skip_negative" -eq 0 ]; then
  touch "$ext/sciink/marker.txt"
  if err="$(PATH="$minimal_bin" SCIINK_ZIP="$zip" SCIINK_EXT_DIR="$ext" sh "$here/install.sh" 2>&1)"; then
    echo "FAIL: install.sh succeeded without unzip/bsdtar on PATH"; exit 1
  fi
  echo "expected failure without unzip/bsdtar: $err"
  test -f "$ext/sciink/marker.txt" || { echo "FAIL: missing-extractor run destroyed the existing install"; exit 1; }
  test -f "$ext/other_extension/keep.inx" || { echo "FAIL: missing-extractor run touched another extension"; exit 1; }
fi

# other extensions in the directory are untouched, and a crash-leftover
# staging dir is swept on uninstall too (not only on install)
mkdir -p "$ext/.sciink-stage.leftover"
SCIINK_EXT_DIR="$ext" sh "$here/install.sh" --uninstall
test ! -e "$ext/sciink" || { echo "FAIL: uninstall left sciink/"; exit 1; }
test ! -e "$ext/.sciink-stage.leftover" || { echo "FAIL: uninstall left a staging dir"; exit 1; }
test -f "$ext/other_extension/keep.inx" || { echo "FAIL: uninstall touched another extension"; exit 1; }
echo "INSTALL-OK"
