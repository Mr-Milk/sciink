#!/usr/bin/env bash
# Assemble a release zip: dist/package.sh <asset> <binary>
#   asset  : macos-universal | windows-x64 | linux-x64
#   binary : path to the built sciink[.exe]
# Writes dist/out/sciink-<asset>.zip with layout sciink/{*.inx,bin/sciink[.exe],README.txt,LICENSE}
set -euo pipefail
asset="${1:?usage: package.sh <asset> <binary>}"
binary="${2:?usage: package.sh <asset> <binary>}"
here="$(cd "$(dirname "$0")/.." && pwd)"
version="$(cd "$here" && cargo metadata --no-deps --format-version 1 | sed -n 's/.*"version":"\([^"]*\)".*/\1/p' | head -1)"
test -n "$version" || { echo "could not read the Cargo version" >&2; exit 1; }
test -f "$binary" || { echo "binary not found: $binary" >&2; exit 1; }

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/sciink/bin"

case "$asset" in
  windows-*) binname="sciink.exe" ;;
  *)         binname="sciink" ;;
esac
cp "$binary" "$stage/sciink/bin/$binname"
chmod 755 "$stage/sciink/bin/$binname"

for f in "$here"/inx/*.inx; do
  out="$stage/sciink/$(basename "$f")"
  sed -e "s/@VERSION@/$version/g" "$f" > "$out"
  if [ "$binname" = "sciink.exe" ]; then
    sed -i.bak 's#<command location="inx">bin/sciink</command>#<command location="inx">bin/sciink.exe</command>#' "$out"
    rm -f "$out.bak"
  fi
done
sed -e "s/@VERSION@/$version/g" "$here/dist/README-dist.txt" > "$stage/sciink/README.txt"
cp "$here/LICENSE" "$stage/sciink/LICENSE"

mkdir -p "$here/dist/out"
zip_path="$here/dist/out/sciink-$asset.zip"
rm -f "$zip_path"
if command -v zip >/dev/null 2>&1; then
  (cd "$stage" && zip -qr "$zip_path" sciink)
elif command -v 7z >/dev/null 2>&1; then
  (cd "$stage" && 7z a -tzip -bso0 -bsp0 "$zip_path" sciink >/dev/null)
else
  echo "need zip or 7z to build the archive" >&2
  exit 1
fi
echo "$zip_path"
