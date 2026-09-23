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
test -f "$tmp/sciink/LICENSE" || { echo "FAIL: LICENSE missing"; exit 1; }
grep -q '@VERSION@' "$tmp"/sciink/*.inx && { echo "FAIL: @VERSION@ left unsubstituted"; exit 1; }

case "$zip" in
  *windows*)
    bin="$tmp/sciink/bin/sciink.exe"
    for f in "$tmp"/sciink/*.inx; do
      grep -q '<command location="inx">bin/sciink.exe</command>' "$f" || { echo "FAIL: $(basename "$f") must reference bin/sciink.exe"; exit 1; }
    done
    ;;
  *)
    bin="$tmp/sciink/bin/sciink"
    for f in "$tmp"/sciink/*.inx; do
      grep -q '<command location="inx">bin/sciink</command>' "$f" || { echo "FAIL: $(basename "$f") must reference bin/sciink"; exit 1; }
    done
    test -x "$bin" || { echo "FAIL: binary is not executable"; exit 1; }
    ;;
esac
test -f "$bin" || { echo "FAIL: binary missing at $bin"; exit 1; }

"$bin" --version | grep -q '^sciink ' || { echo "FAIL: --version banner"; exit 1; }
if ! "$bin" --tool=about "$here/tests/data/edge/simple.svg" > "$tmp/out.svg" 2> "$tmp/err.txt"; then
  echo "FAIL: about exited non-zero"; cat "$tmp/err.txt"; exit 1
fi
cmp -s "$tmp/out.svg" "$here/tests/data/edge/simple.svg" || { echo "FAIL: about did not echo the document"; exit 1; }
grep -q 'document: 3 elements' "$tmp/err.txt" || { echo "FAIL: about report missing"; cat "$tmp/err.txt"; exit 1; }
for f in DejaVuSans.ttf DejaVuSans-Bold.ttf LICENSE-DejaVu.txt; do
  test -f "$tmp/sciink/fonts/$f" || { echo "FAIL: fonts/$f missing"; exit 1; }
done
grep -q 'bundled fonts: .*(2 faces)' "$tmp/err.txt" || { echo "FAIL: about does not see the bundled fonts"; cat "$tmp/err.txt"; exit 1; }
echo "PACKAGE-OK $zip"
