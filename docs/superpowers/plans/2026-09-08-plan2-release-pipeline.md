# sciink Plan 2 — Release Pipeline and Installers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tagged pushes to `Mr-Milk/sciink` produce per-OS zips on a GitHub Release, and end users install with one command (`curl … | sh` on macOS/Linux, `irm … | iex` on Windows) into Inkscape's user extensions directory.

**Architecture:** `dist/package.sh` turns a built binary plus the `.inx` files into `dist/out/sciink-<asset>.zip` with a top-level `sciink/` folder (unzipping in the extensions dir yields `extensions/sciink/`). `.github/workflows/release.yml` builds a macOS universal binary (lipo of arm64 + x86_64, ad-hoc signed), a static-CRT Windows x64 binary and a static musl Linux x64 binary, packages each, smoke-tests the package, and publishes all three zips with `softprops/action-gh-release`. `install.sh`/`install.ps1` download the right asset from the latest release (or a pinned version), replace any previous `sciink/` folder, clear macOS quarantine, and print the installed version. CI runs the packaging and both installers against a locally built binary on every push so the release path is exercised before any tag exists.

**Tech Stack:** POSIX sh, PowerShell 5+/7, GitHub Actions (`dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `actions/upload-artifact@v4`, `actions/download-artifact@v4`, `softprops/action-gh-release@v2`), `zip`/`7z`/`Compress-Archive`, `lipo`, `codesign`.

**Spec:** `docs/spec/03-infrastructure.md` §C.4 (packaging & release) and §C.3 (`about` tool reports `SCIINK_GIT_SHA`/`SCIINK_TARGET`); `docs/spec/00-overview.md` (macOS quarantine mitigation: the curl route never sets the quarantine xattr).

## Global Constraints

- Repository: `https://github.com/Mr-Milk/sciink`. Asset names are fixed and unversioned so `…/releases/latest/download/<asset>` is a stable URL: `sciink-macos-universal.zip`, `sciink-windows-x64.zip`, `sciink-linux-x64.zip`.
- Zip layout: `sciink/<name>.inx` (all files from `inx/`), `sciink/bin/sciink` (or `sciink/bin/sciink.exe` on Windows), `sciink/README.txt`. Every `@VERSION@` token inside the `.inx` files is replaced by the Cargo package version. The Windows zip's `.inx` files reference `bin/sciink.exe`. Unix zips must preserve the executable bit (zip on the building OS).
- Inkscape user extensions directories: macOS `$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions`; Linux `${XDG_CONFIG_HOME:-$HOME/.config}/inkscape/extensions` (Flatpak: `$HOME/.var/app/org.inkscape.Inkscape/config/inkscape/extensions`); Windows `%APPDATA%\inkscape\extensions`. Installers install into `<that dir>/sciink/`, replacing any existing `sciink/` folder, and touch nothing else in the directory.
- Installers must be testable offline: `SCIINK_ZIP` (path to a local zip, skips download) and `SCIINK_EXT_DIR` (target extensions dir) for `install.sh`; `-Zip` and `-Dest` parameters for `install.ps1`. `SCIINK_VERSION`/`-Version` pin a release tag; default `latest`.
- Builds embed `SCIINK_GIT_SHA` (short SHA) and `SCIINK_TARGET` (target triple, `universal-apple-darwin` for the lipo binary) through the existing `option_env!` reads; no `build.rs`.
- Release workflow refuses a tag whose name (minus the leading `v`) is not the Cargo version or the Cargo version followed by `-<suffix>`; tags containing `-` publish as pre-releases.
- Shell scripts: `#!/bin/sh` + `set -eu` (POSIX, no bashisms) for `install.sh`; `#!/usr/bin/env bash` + `set -euo pipefail` for `dist/*.sh`; quote every path (the macOS extensions dir contains a space).
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` stay green; commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

## File structure

| File | Responsibility |
|---|---|
| `dist/package.sh` | binary + `inx/` → `dist/out/sciink-<asset>.zip` |
| `dist/test-package.sh` | unzip a package and verify layout, exec bit, `--version`, pipe smoke |
| `dist/README-dist.txt` | becomes `sciink/README.txt` inside every zip |
| `install.sh` | macOS/Linux installer (curl/wget, unzip, quarantine) + `--uninstall` |
| `install.ps1` | Windows installer + `-Uninstall` |
| `dist/test-install.sh` | runs `install.sh` offline against a local zip into a temp dir |
| `.github/workflows/release.yml` | tag → build matrix → package → smoke → GitHub Release |
| `.github/workflows/ci.yml` | extended: package + installer smoke on all three OSes |
| `Cargo.toml`, `README.md`, `.gitignore` | repository URL, install docs, ignore `dist/out/` |

---

### Task 1: Packaging script and package smoke test

**Files:**
- Create: `dist/package.sh`, `dist/test-package.sh`, `dist/README-dist.txt`
- Modify: `.gitignore` (add `/dist/out`)

**Interfaces:**
- Produces: `dist/package.sh <asset> <binary-path>` writes `dist/out/sciink-<asset>.zip` and prints its path; `dist/test-package.sh <zip>` exits 0 only if the package is installable and the binary works.
- Consumes: `cargo metadata` for the version; `inx/*.inx`; `tests/data/edge/simple.svg` for the smoke test.

- [ ] **Step 1: Write the smoke test first**

`dist/test-package.sh`:
```bash
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
```

`dist/README-dist.txt`:
```
sciink — fast Inkscape extensions for scientific figures
https://github.com/Mr-Milk/sciink

INSTALL
  Unzip so that this folder (sciink/) sits directly inside Inkscape's user
  extensions directory (Edit > Preferences > System > User extensions):
    macOS    ~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink
    Linux    ~/.config/inkscape/extensions/sciink
    Windows  %APPDATA%\inkscape\extensions\sciink
  Restart Inkscape. The tools appear under Extensions > Scientific.

  Or use the one-line installers from the project README:
    macOS/Linux:  curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh
    Windows:      irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1 | iex

macOS: if the menu entries do nothing after a browser download, clear the
quarantine flag once:
  xattr -dr com.apple.quarantine "$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink"
(The curl installer never sets that flag.)

VERSION  @VERSION@
```

- [ ] **Step 2: Run the test to see it fail**

Run: `chmod +x dist/test-package.sh && dist/test-package.sh dist/out/sciink-macos-universal.zip`
Expected: fails (no such zip yet).

- [ ] **Step 3: Write the packager**

`dist/package.sh`:
```bash
#!/usr/bin/env bash
# Assemble a release zip: dist/package.sh <asset> <binary>
#   asset  : macos-universal | windows-x64 | linux-x64
#   binary : path to the built sciink[.exe]
# Writes dist/out/sciink-<asset>.zip with layout sciink/{*.inx,bin/sciink[.exe],README.txt}
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

mkdir -p "$here/dist/out"
zip_path="$here/dist/out/sciink-$asset.zip"
rm -f "$zip_path"
if command -v zip >/dev/null 2>&1; then
  (cd "$stage" && zip -qr "$zip_path" sciink)
elif command -v 7z >/dev/null 2>&1; then
  (cd "$stage" && 7z a -tzip -bso0 -bsp0 "$zip_path" sciink >/dev/null)
else
  powershell.exe -NoProfile -Command "Compress-Archive -Path '$(cygpath -w "$stage/sciink")' -DestinationPath '$(cygpath -w "$zip_path")' -Force"
fi
echo "$zip_path"
```

Add `/dist/out` to `.gitignore`.

- [ ] **Step 4: Run the test**

Run:
```bash
chmod +x dist/package.sh dist/test-package.sh
cargo build --release
dist/package.sh macos-universal target/release/sciink
dist/test-package.sh dist/out/sciink-macos-universal.zip
dist/package.sh windows-x64 target/release/sciink && unzip -l dist/out/sciink-windows-x64.zip | grep -q 'sciink/bin/sciink.exe' && grep -q 'bin/sciink.exe' <(unzip -p dist/out/sciink-windows-x64.zip sciink/about.inx) && echo WINDOWS-LAYOUT-OK
```
Expected: `PACKAGE-OK …macos-universal.zip` and `WINDOWS-LAYOUT-OK` (the windows zip here just wraps the mac binary under the `.exe` name — layout check only; `test-package.sh` is not run on it locally because the binary would not execute).

- [ ] **Step 5: Commit**

```bash
git add dist/package.sh dist/test-package.sh dist/README-dist.txt .gitignore
git commit -m "build(dist): release zip packager and package smoke test

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: `install.sh` for macOS and Linux

**Files:**
- Create: `install.sh`, `dist/test-install.sh`

**Interfaces:**
- Produces: `sh install.sh` (env: `SCIINK_VERSION`, `SCIINK_ZIP`, `SCIINK_EXT_DIR`; flag `--uninstall`).
- Consumes: the zip layout from Task 1.

- [ ] **Step 1: Write the offline test**

`dist/test-install.sh`:
```bash
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
```

- [ ] **Step 2: Run it to see it fail**

Run: `chmod +x dist/test-install.sh && dist/test-install.sh dist/out/sciink-macos-universal.zip`
Expected: fails (`install.sh` does not exist).

- [ ] **Step 3: Write the installer**

`install.sh`:
```sh
#!/bin/sh
# sciink installer for macOS and Linux.
#   curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh
# Options (environment):
#   SCIINK_VERSION=v0.1.0   install a specific release tag (default: latest)
#   SCIINK_EXT_DIR=<dir>    Inkscape user extensions directory (default: detected)
#   SCIINK_ZIP=<file>       install from a local zip instead of downloading
# Flags: --uninstall        remove the sciink folder from the extensions directory
set -eu

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
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$zip" "$url"
  else
    echo "sciink: need curl or wget" >&2; exit 1
  fi
fi

mkdir -p "$EXT"
rm -rf "$EXT/sciink"
if command -v unzip >/dev/null 2>&1; then
  unzip -oq "$zip" -d "$EXT"
elif command -v bsdtar >/dev/null 2>&1; then
  bsdtar -xf "$zip" -C "$EXT"
else
  echo "sciink: need unzip or bsdtar" >&2; exit 1
fi
chmod +x "$EXT/sciink/bin/sciink"
if [ "$os" = "Darwin" ]; then
  xattr -dr com.apple.quarantine "$EXT/sciink" 2>/dev/null || true
fi

echo "sciink: installed $("$EXT/sciink/bin/sciink" --version) into $EXT/sciink"
echo "sciink: restart Inkscape; the tools are under Extensions > Scientific"
```

- [ ] **Step 4: Run the test**

Run: `chmod +x install.sh && dist/test-install.sh dist/out/sciink-macos-universal.zip`
Expected: `INSTALL-OK` (and two `sciink: installed sciink 0.1.0 …` lines). Also run `sh -n install.sh` and, if available, `shellcheck install.sh dist/*.sh` (no findings other than SC2039-style POSIX notes).

- [ ] **Step 5: Commit**

```bash
git add install.sh dist/test-install.sh
git commit -m "feat(install): one-line installer for macOS and Linux

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: `install.ps1` for Windows

**Files:**
- Create: `install.ps1`

**Interfaces:**
- Produces: `irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1 | iex`; parameters `-Version`, `-Zip`, `-Dest`, `-Uninstall`.

- [ ] **Step 1: Write the installer**

`install.ps1`:
```powershell
# sciink installer for Windows.
#   irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1 | iex
# Or download and run:  .\install.ps1 [-Version v0.1.0] [-Dest <extensions dir>] [-Zip <local zip>] [-Uninstall]
param(
    [string]$Version = "latest",
    [string]$Zip = "",
    [string]$Dest = "",
    [switch]$Uninstall
)
$ErrorActionPreference = "Stop"
$repo = "Mr-Milk/sciink"
$asset = "sciink-windows-x64.zip"
if (-not $Dest) { $Dest = Join-Path $env:APPDATA "inkscape\extensions" }
$target = Join-Path $Dest "sciink"

if ($Uninstall) {
    if (Test-Path $target) { Remove-Item -Recurse -Force $target }
    Write-Host "sciink: removed $target"
    exit 0
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("sciink-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zipPath = Join-Path $tmp $asset
    if ($Zip) {
        Copy-Item -Path $Zip -Destination $zipPath
    } else {
        $url = if ($Version -eq "latest") { "https://github.com/$repo/releases/latest/download/$asset" } else { "https://github.com/$repo/releases/download/$Version/$asset" }
        Write-Host "sciink: downloading $url"
        Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
    }
    if (Test-Path $target) { Remove-Item -Recurse -Force $target }
    New-Item -ItemType Directory -Force -Path $Dest | Out-Null
    Expand-Archive -Path $zipPath -DestinationPath $Dest -Force
    $exe = Join-Path $target "bin\sciink.exe"
    if (-not (Test-Path $exe)) { throw "sciink: archive did not contain sciink\bin\sciink.exe" }
    $banner = & $exe --version
    Write-Host "sciink: installed $banner into $target"
    Write-Host "sciink: restart Inkscape; the tools are under Extensions > Scientific"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
```

- [ ] **Step 2: Verify locally as far as possible**

Run: `command -v pwsh && pwsh -NoProfile -Command "[scriptblock]::Create((Get-Content -Raw install.ps1)) | Out-Null; 'PS-SYNTAX-OK'" || echo "pwsh not installed; CI (windows) validates install.ps1 in Task 4"`
Expected: `PS-SYNTAX-OK`, or the fallback message.

- [ ] **Step 3: Commit**

```bash
git add install.ps1
git commit -m "feat(install): PowerShell installer for Windows

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Release workflow, CI extension, repository metadata, README

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `.github/workflows/ci.yml`, `Cargo.toml`, `README.md`

**Interfaces:**
- Produces: on `git push origin vX.Y.Z[-suffix]`, a GitHub Release with the three zips; on every push/PR, packaging + installers exercised on all three OSes.

- [ ] **Step 1: Release workflow**

`.github/workflows/release.yml`:
```yaml
name: release
on:
  push:
    tags: ["v*"]
  workflow_dispatch:
permissions:
  contents: write
env:
  CARGO_TERM_COLOR: always
jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: macos-latest
            asset: macos-universal
          - os: windows-latest
            asset: windows-x64
          - os: ubuntu-latest
            asset: linux-x64
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Tag must match the Cargo version
        if: startsWith(github.ref, 'refs/tags/')
        shell: bash
        run: |
          v="$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"version":"\([^"]*\)".*/\1/p' | head -1)"
          case "${GITHUB_REF_NAME#v}" in
            "$v"|"$v"-*) echo "tag $GITHUB_REF_NAME matches Cargo version $v" ;;
            *) echo "tag $GITHUB_REF_NAME does not match Cargo version $v"; exit 1 ;;
          esac
      - name: Build (macOS universal)
        if: matrix.os == 'macos-latest'
        shell: bash
        run: |
          export SCIINK_GIT_SHA="${GITHUB_SHA::7}" SCIINK_TARGET="universal-apple-darwin"
          rustup target add aarch64-apple-darwin x86_64-apple-darwin
          cargo build --release --target aarch64-apple-darwin
          cargo build --release --target x86_64-apple-darwin
          mkdir -p target/universal
          lipo -create -output target/universal/sciink target/aarch64-apple-darwin/release/sciink target/x86_64-apple-darwin/release/sciink
          codesign --force --sign - target/universal/sciink
          echo "BIN=target/universal/sciink" >> "$GITHUB_ENV"
      - name: Build (Linux musl, static)
        if: matrix.os == 'ubuntu-latest'
        shell: bash
        run: |
          export SCIINK_GIT_SHA="${GITHUB_SHA::7}" SCIINK_TARGET="x86_64-unknown-linux-musl"
          rustup target add x86_64-unknown-linux-musl
          cargo build --release --target x86_64-unknown-linux-musl
          echo "BIN=target/x86_64-unknown-linux-musl/release/sciink" >> "$GITHUB_ENV"
      - name: Build (Windows x64, static CRT)
        if: matrix.os == 'windows-latest'
        shell: bash
        run: |
          export SCIINK_GIT_SHA="${GITHUB_SHA::7}" SCIINK_TARGET="x86_64-pc-windows-msvc"
          cargo build --release
          echo "BIN=target/release/sciink.exe" >> "$GITHUB_ENV"
      - name: Package
        shell: bash
        run: dist/package.sh ${{ matrix.asset }} "$BIN"
      - name: Smoke-test the package
        shell: bash
        run: dist/test-package.sh dist/out/sciink-${{ matrix.asset }}.zip
      - uses: actions/upload-artifact@v4
        with:
          name: sciink-${{ matrix.asset }}
          path: dist/out/sciink-${{ matrix.asset }}.zip
          if-no-files-found: error
  release:
    needs: build
    if: startsWith(github.ref, 'refs/tags/')
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist/out
          merge-multiple: true
      - name: List assets
        run: ls -l dist/out
      - uses: softprops/action-gh-release@v2
        with:
          files: dist/out/*.zip
          generate_release_notes: true
          prerelease: ${{ contains(github.ref_name, '-') }}
```

- [ ] **Step 2: Extend CI to exercise packaging and the installers**

Replace the `test` job in `.github/workflows/ci.yml` with:
```yaml
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test
      - run: cargo build --release
      - name: smoke (about tool through a pipe)
        shell: bash
        run: |
          ./target/release/sciink --tool=about tests/data/edge/simple.svg > out.svg
          cmp out.svg tests/data/edge/simple.svg
      - name: package + installer (unix)
        if: runner.os != 'Windows'
        shell: bash
        run: |
          asset=linux-x64; [ "$RUNNER_OS" = macOS ] && asset=macos-universal
          dist/package.sh "$asset" target/release/sciink
          dist/test-package.sh "dist/out/sciink-$asset.zip"
          dist/test-install.sh "dist/out/sciink-$asset.zip"
      - name: package + installer (windows)
        if: runner.os == 'Windows'
        shell: bash
        run: |
          dist/package.sh windows-x64 target/release/sciink.exe
          dist/test-package.sh dist/out/sciink-windows-x64.zip
      - name: install.ps1 (windows)
        if: runner.os == 'Windows'
        shell: pwsh
        run: |
          $dest = Join-Path $env:RUNNER_TEMP "ext with space"
          ./install.ps1 -Zip dist/out/sciink-windows-x64.zip -Dest $dest
          if (-not (Test-Path (Join-Path $dest "sciink\bin\sciink.exe"))) { throw "not installed" }
          if (-not (Test-Path (Join-Path $dest "sciink\about.inx"))) { throw "inx missing" }
          ./install.ps1 -Dest $dest -Uninstall
          if (Test-Path (Join-Path $dest "sciink")) { throw "uninstall failed" }
          "PS-INSTALL-OK"
```
(the `check` job is unchanged.)

- [ ] **Step 3: Repository metadata and README**

`Cargo.toml` `[package]`: add `repository = "https://github.com/Mr-Milk/sciink"` and `readme = "README.md"`.

`README.md` (replace):
```markdown
# sciink

Fast, dependency-free Inkscape extensions for scientific figures — a Rust rewrite of
[Scientific-Inkscape](https://github.com/burghoff/Scientific-Inkscape) (Flattener, Scaler,
Homogenizer, Text Ghoster, Combine by Color, Favorite Markers). One compiled binary, no Python,
works with Inkscape 1.2 and later.

Status: early development. The current release contains only the `Diagnostics` menu entry, which
proves the installation works; the tools land one by one. Design specs live in `docs/spec/`.

## Install

macOS / Linux:

    curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh

Windows (PowerShell):

    irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1 | iex

Then restart Inkscape. The tools appear under **Extensions ▸ Scientific**.

Manual install: download `sciink-<os>.zip` from the
[latest release](https://github.com/Mr-Milk/sciink/releases/latest) and unzip it into Inkscape's
user extensions directory (Edit ▸ Preferences ▸ System ▸ User extensions) so that it contains
`sciink/`. Pin a version with `SCIINK_VERSION=v0.1.0` (sh) or `-Version v0.1.0` (PowerShell).
Uninstall with `sh install.sh --uninstall` or `.\install.ps1 -Uninstall`.

macOS note: a zip downloaded by a browser is quarantined and Gatekeeper silently blocks the
binary. The `curl` installer never sets that flag; after a manual download run
`xattr -dr com.apple.quarantine "~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink"`.

## Developing

    cargo test
    dist/dev-install.sh      # symlink into Inkscape's user extensions dir, then restart Inkscape

Set `SCIINK_LOG=/tmp/sciink.log` in Inkscape's environment to get timing lines.
Releases: push a tag `vX.Y.Z` matching `Cargo.toml`'s version; the `release` workflow builds
macOS (universal), Windows (x64) and Linux (x64, static) zips and publishes them.
```

- [ ] **Step 4: Verify locally**

Run:
```bash
python3 -c "import yaml,sys; [yaml.safe_load(open(f)) for f in ['.github/workflows/ci.yml','.github/workflows/release.yml']]; print('YAML-OK')" 2>/dev/null || ruby -ryaml -e "%w[.github/workflows/ci.yml .github/workflows/release.yml].each{|f| YAML.load_file(f)}; puts 'YAML-OK'"
cargo build --release && dist/package.sh macos-universal target/release/sciink && dist/test-package.sh dist/out/sciink-macos-universal.zip && dist/test-install.sh dist/out/sciink-macos-universal.zip
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test
```
Expected: `YAML-OK`, `PACKAGE-OK`, `INSTALL-OK`, suite green.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml .github/workflows/ci.yml Cargo.toml Cargo.lock README.md
git commit -m "ci: release workflow with per-OS zips; installer smoke tests; install docs

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: First pre-release through the pipeline (controller-run, after the PR is merged)

Not a subagent task: the controller pushes the branch, opens the PR, waits for the extended CI (which now exercises packaging and both installers on all three OSes), merges, then tags `v0.1.0-alpha.1` on `main` and watches the `release` workflow publish `sciink-macos-universal.zip`, `sciink-windows-x64.zip`, `sciink-linux-x64.zip` as a pre-release. Final check on this Mac: `curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | SCIINK_VERSION=v0.1.0-alpha.1 sh` installs into the real extensions dir (it replaces the dev symlink folder — re-run `dist/dev-install.sh` afterwards to go back to the dev loop).

## Plan self-review notes

- Spec coverage (§C.4): per-OS zips with the exact asset names and layout ✓ (T1); exec bits via zipping on the building OS ✓; Windows `.exe` rewrite of the `.inx` ✓; `@VERSION@` substitution ✓; universal mac binary + ad-hoc codesign ✓, musl static Linux ✓, static-CRT Windows (already in `.cargo/config.toml`) ✓ (T4); tag/version check + pre-release flag ✓; curl-based install that avoids quarantine ✓ (T2) plus the Windows counterpart ✓ (T3); README troubleshooting ✓; `SCIINK_GIT_SHA`/`SCIINK_TARGET` baked in ✓. Developer-ID notarization remains v2.
- Placeholder scan: none.
- Consistency: asset names identical across `package.sh`, `test-package.sh`, `install.sh`, `install.ps1`, `release.yml`, `ci.yml`, README.
