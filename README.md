# sciink

Fast, dependency-free Inkscape extensions for scientific figures — a Rust rewrite of
[Scientific-Inkscape](https://github.com/burghoff/Scientific-Inkscape) (Flattener, Scaler,
Homogenizer, Text Ghoster, Combine by Color, Favorite Markers). One compiled binary, no Python,
works with Inkscape 1.2 and later.

Status: early development. The tools land one by one; design specs live in `docs/spec/`. The current
release ships seven menu entries — three tools, three diagnostics and one debug editor:

- **Extensions ▸ Scientific ▸ Flattener** — makes an imported plot editable: deep ungroup (composing
  transforms, clips and styles onto the leaves and unlinking clones), matplotlib minus-sign glyphs back
  to text, thin dark rectangles back to strokes, the text pipeline (manual-kerning removal, merges,
  splits, justification, optional font replacement, text clips removed), then overlapping duplicates
  and white background rectangles removed. Elements marked on the Exclusions page are not ungrouped
  themselves (as in the original, what is inside them is still processed). Same options and defaults
  as the original.
- **Extensions ▸ Scientific ▸ Combine by Color** — merges the selected paths that share stroke, fill,
  width, dashes and markers into one path each (lines darker than the lightness threshold, such as
  axes and ticks, are left alone), releasing their clips and masks. Fewer elements, smaller files, a
  more responsive Inkscape. Same options and defaults as the original.
- **Extensions ▸ Scientific ▸ Text Ghoster** — puts a blurred, semi-transparent white rectangle behind
  each selected object, sized from its text, so labels stay readable on top of data. Group several
  texts first to treat them as one.
- **Extensions ▸ Scientific ▸ Diagnostics** — version, platform, the document's size and element counts,
  and how many font faces were found (and how long that took). Proves the installation works.
- **Extensions ▸ Scientific ▸ Debug ▸ Font Probe** — for every `font-family` the document asks for (and
  for the generic families), the face the text engine actually measures with and the file it came from.
  Use it when text is laid out as if a different font were installed.
- **Extensions ▸ Scientific ▸ Debug ▸ Text Highlight** — draws the text engine's measurements as
  rectangles over the document: per character (advance box or ink box), per chunk, per line, or one box
  for the whole element. Use it to see exactly what the parser thinks your text is.
- **Extensions ▸ Scientific ▸ Debug ▸ Text Fix** — **rewrites the selected text**: it runs the
  Flattener's text pipeline — manual-kerning removal, merges, splits, justification — and replaces every
  selected `<text>` element with a regenerated one. Preview of what the Flattener will do to text once it
  ships; undo (Ctrl+Z) puts the document back. It runs that pipeline *alone*, without the Flattener's
  `setreplacement` pre-pass (which strips `-inkscape-font-specification` first), so on Inkscape-authored
  multi-line text the two can disagree: Text Fix leaves such text joined where the Flattener will split
  it into lines.

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

The upstream oracles need the installed fonts and the `tests/upstream` fixture symlink; run them with

    SCIINK_SYSTEM_FONTS=1 cargo test --test text_fixtures --test text_tools --test text_ghoster -- --ignored --test-threads=1

Set `SCIINK_LOG=/tmp/sciink.log` in Inkscape's environment to get timing lines.
Releases: push a tag `vX.Y.Z` matching `Cargo.toml`'s version; the `release` workflow builds
macOS (universal), Windows (x64) and Linux (x64, static) zips and publishes them.
