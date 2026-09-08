#!/usr/bin/env bash
# Verify a release zip: layout, exec bit, version banner, and the Inkscape pipe contract.
# Usage: dist/test-package.sh dist/out/sciink-<asset>.zip
set -euo pipefail
zip="${1:?usage: test-package.sh <zip>}"
here="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if command -v unzip >/dev/null 2>&1; then unzip -oq "$zip" -d "$tmp"; else bsdtar -xf "$zip" -C "$tmp"; fi

test -d "$tmp/sciink" || { echo "FAIL: zip has no top-level sciink/ folder"; exit 1; }
test -f "$tmp/sciink/README.txt" || { echo "FAIL: README.txt missing"; exit 1; }
test -f "$tmp/sciink/about.inx" || { echo "FAIL: about.inx missing"; exit 1; }
grep -q '@VERSION@' "$tmp"/sciink/*.inx && { echo "FAIL: @VERSION@ left unsubstituted"; exit 1; }

case "$zip" in
  *windows*)
    bin="$tmp/sciink/bin/sciink.exe"
    grep -q '<command location="inx">bin/sciink.exe</command>' "$tmp/sciink/about.inx" || { echo "FAIL: windows inx must reference bin/sciink.exe"; exit 1; }
    ;;
  *)
    bin="$tmp/sciink/bin/sciink"
    grep -q '<command location="inx">bin/sciink</command>' "$tmp/sciink/about.inx" || { echo "FAIL: inx must reference bin/sciink"; exit 1; }
    test -x "$bin" || { echo "FAIL: binary is not executable"; exit 1; }
    ;;
esac
test -f "$bin" || { echo "FAIL: binary missing at $bin"; exit 1; }

"$bin" --version | grep -q '^sciink ' || { echo "FAIL: --version banner"; exit 1; }
"$bin" --tool=about "$here/tests/data/edge/simple.svg" > "$tmp/out.svg" 2> "$tmp/err.txt"
cmp -s "$tmp/out.svg" "$here/tests/data/edge/simple.svg" || { echo "FAIL: about did not echo the document"; exit 1; }
grep -q 'document: 3 elements' "$tmp/err.txt" || { echo "FAIL: about report missing"; cat "$tmp/err.txt"; exit 1; }
echo "PACKAGE-OK $zip"
