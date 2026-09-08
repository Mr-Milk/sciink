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

Until the first stable release ships, GitHub's `latest` release excludes pre-releases, so both
installers automatically fall back to the newest release including pre-releases. Pin a specific
version instead with `SCIINK_VERSION=v0.1.0-alpha.1` (sh) or `-Version v0.1.0-alpha.1`
(PowerShell):

    curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | SCIINK_VERSION=v0.1.0-alpha.1 sh
    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1))) -Version v0.1.0-alpha.1

Manual install: download `sciink-<os>.zip` from the
[latest release](https://github.com/Mr-Milk/sciink/releases/latest) and unzip it into Inkscape's
user extensions directory (Edit ▸ Preferences ▸ System ▸ User extensions) so that it contains
`sciink/`. If Inkscape's user extensions directory isn't in the default location (e.g. under a
Snap or Flatpak install), point the installer at it with `SCIINK_EXT_DIR=<dir>`.

Uninstall:

    curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh -s -- --uninstall
    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1))) -Uninstall

or, from a downloaded copy of the script: `sh install.sh --uninstall` / `.\install.ps1 -Uninstall`.
The `& ([scriptblock]::Create(...)))` form is needed for any parameterized PowerShell invocation
(`-Uninstall`, `-Version`, `-Dest`) piped straight from `irm`, since a plain `irm ... | iex`
one-liner cannot take parameters.

macOS note: a zip downloaded by a browser is quarantined and Gatekeeper silently blocks the
binary. The `curl` installer never sets that flag; after a manual download run
`xattr -dr com.apple.quarantine "$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink"`.

## Developing

    cargo test
    dist/dev-install.sh      # symlink into Inkscape's user extensions dir, then restart Inkscape

Set `SCIINK_LOG=/tmp/sciink.log` in Inkscape's environment to get timing lines.
Releases: push a tag `vX.Y.Z` matching `Cargo.toml`'s version; the `release` workflow builds
macOS (universal), Windows (x64) and Linux (x64, static) zips and publishes them.
