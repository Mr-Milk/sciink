# sciink

Fast, dependency-free Inkscape extensions for scientific figures — a Rust rewrite of
[Scientific-Inkscape](https://github.com/burghoff/Scientific-Inkscape) (Flattener, Scaler,
Homogenizer, Text Ghoster, Combine by Color, Favorite Markers). One compiled binary, no Python,
works with Inkscape 1.2 and later.

Status: early development. The tools land one by one; design specs live in `docs/spec/`. The current
release ships three menu entries, all diagnostic:

- **Extensions ▸ Scientific ▸ Diagnostics** — version, platform, the document's size and element counts,
  and how many font faces were found (and how long that took). Proves the installation works.
- **Extensions ▸ Scientific ▸ Debug ▸ Font Probe** — for every `font-family` the document asks for (and
  for the generic families), the face the text engine actually measures with and the file it came from.
  Use it when text is laid out as if a different font were installed.
- **Extensions ▸ Scientific ▸ Debug ▸ Text Highlight** — draws the text engine's measurements as
  rectangles over the document: per character (advance box or ink box), per chunk, per line, or one box
  for the whole element. Use it to see exactly what the parser thinks your text is.

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

## Fonts

Text measurement uses the fonts installed on the machine, so a document measures differently where a
family is missing (the tools say which substitute was used). Two environment variables override the
search, mostly for tests and for reproducing a figure built elsewhere:

- `SCIINK_FONT_DIRS` — extra directories to load fonts from, separated by the platform's path-list
  separator (`:` on macOS/Linux, `;` on Windows). Loaded in addition to the system fonts.
- `SCIINK_NO_SYSTEM_FONTS=1` — skip the system font scan entirely, so only `SCIINK_FONT_DIRS` is used.
  With neither set and no system fonts, nothing can be measured.

## Developing

    cargo test
    dist/dev-install.sh      # symlink into Inkscape's user extensions dir, then restart Inkscape

Set `SCIINK_LOG=/tmp/sciink.log` in Inkscape's environment to get timing lines.
Releases: push a tag `vX.Y.Z` matching `Cargo.toml`'s version; the `release` workflow builds
macOS (universal), Windows (x64) and Linux (x64, static) zips and publishes them.
