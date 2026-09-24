# sciink

[![ci](https://img.shields.io/github/actions/workflow/status/Mr-Milk/sciink/ci.yml?branch=main&label=CI&style=flat-square)](https://github.com/Mr-Milk/sciink/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/Mr-Milk/sciink?style=flat-square&label=Release)](https://github.com/Mr-Milk/sciink/releases/latest)
[![license](https://img.shields.io/badge/license-GPL--2.0--or--later-blue?style=flat-square&label=License)](LICENSE)

Fast Inkscape extensions for scientific figures: a Rust rewrite of
[Scientific-Inkscape](https://github.com/burghoff/Scientific-Inkscape). One native binary, no Python,
and the same tools, options and defaults as the original. Inkscape 1.2 or later on macOS, Windows and
Linux.

## Install

macOS / Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1 | iex
```

Restart Inkscape. The tools appear under **Extensions ▸ Scientific**.

<details>
<summary>Pin a version, custom extensions directory, uninstall</summary>

```sh
curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | SCIINK_VERSION=v0.1.0 sh
curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | SCIINK_EXT_DIR=<dir> sh
curl -fsSL https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.sh | sh -s -- --uninstall
```

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1))) -Version v0.1.0
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1))) -Dest <dir>
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Mr-Milk/sciink/main/install.ps1))) -Uninstall
```

Set the extensions directory explicitly for Snap, Flatpak or other non-default Inkscape layouts.
</details>

<details>
<summary>Manual install</summary>

Download `sciink-<os>.zip` from the [latest release](https://github.com/Mr-Milk/sciink/releases/latest)
and unzip it into Inkscape's user extensions directory (Edit ▸ Preferences ▸ System ▸ User extensions),
so that the directory contains `sciink/`:

| OS | User extensions directory |
|---|---|
| macOS | `~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions` |
| Linux | `~/.config/inkscape/extensions` (Flatpak: `~/.var/app/org.inkscape.Inkscape/config/inkscape/extensions`) |
| Windows | `%APPDATA%\inkscape\extensions` |

macOS quarantines a zip downloaded by a browser and then blocks the binary silently. Clear the flag
once (the `curl` installer never sets it):

```sh
xattr -dr com.apple.quarantine "$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink"
```
</details>

## Tools

| Tool | What it does |
|---|---|
| **Flattener** | Makes an imported plot editable: ungroups everything, unlinks clones, restores matplotlib minus signs and thin rectangles, cleans up text (kerning removal, merges, splits, justification, optional font replacement), removes duplicate paths and white background rectangles. |
| **Scaler** | Resizes grouped plots without distorting text, ticks or markers. *Correction* undoes a manual scale; *Matching* gives every selected plot the plot area or bounding box of the first one. |
| **Homogenizer** | One font size, one font and one stroke width across the selection; distorted text made conformal, transforms fused into paths, clips and masks removed. Nothing moves its centre. |
| **Text Ghoster** | Puts a blurred, semi-transparent white box behind each selected object so labels stay readable over data. |
| **Combine by Color** | Merges paths that share stroke, fill, width, dashes and markers into one path each. Fewer elements, smaller files. |
| **Favorite Markers** | Puts stored start/mid/end marker templates on the selected paths at any size. Arrow, Triangle and Distance are built in; store your own from a selected path. |
| **Slimmer** | Makes the whole document smaller and faster in Inkscape: merges the duplicate stylesheets every matplotlib import adds, removes unused and merges identical definitions, drops empty elements (invisible ones on request) and single-child wrapper groups. Rendering-exact by default; optional coordinate rounding. |
| **Diagnostics**, **Debug** | Version, platform and font report; Font Probe, Text Highlight and Text Fix show what the text engine measures and does. |

Options and defaults match the original. Known differences are listed under "Deliberate deviations" in
[docs/spec/02-geometry-tools.md](docs/spec/02-geometry-tools.md),
[docs/spec/01-text-engine.md](docs/spec/01-text-engine.md) and
[docs/spec/03-infrastructure.md](docs/spec/03-infrastructure.md). Scientific-Inkscape's Autoexporter and
Gallery Viewer are not part of sciink.

## Fonts

Text is measured with the fonts installed on your machine. A missing family is substituted and the tool
reports which one (Diagnostics and Debug ▸ Font Probe show every resolution). `SCIINK_FONT_DIRS` adds
font directories; `SCIINK_NO_SYSTEM_FONTS=1` skips the system fonts. DejaVu Sans (Book and Bold) is
bundled, so matplotlib's default font measures correctly even where it is not installed; an installed
copy takes precedence (`SCIINK_NO_SYSTEM_FONTS=1` also skips the bundled copy; `SCIINK_NO_BUNDLED_FONTS=1`
skips only it).

## Large documents

Inkscape writes the whole document to a temporary file, runs the extension, reads the result back and
re-renders it, so most of the wait on a large file is that round trip, not the tool itself. What makes
the round trip slow is the number of elements and of stylesheet rules, not embedded images: every
imported matplotlib figure brings its own `<style>` rule, and Inkscape matches every rule against every
element on each load and save. Run **Slimmer** once on a document assembled from many imports (it
halved the round trip of a 52 MB manuscript), and run the other tools on one figure's selection rather
than on a whole layer.

## Developing

See [docs/DEVELOPING.md](docs/DEVELOPING.md) for building, tests, the upstream oracles, the Inkscape
dev loop, packaging and releases. Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## License

[GPL-2.0-or-later](LICENSE), like Scientific-Inkscape. DejaVu Sans is redistributed under the
Bitstream Vera and DejaVu licences (`fonts/LICENSE-DejaVu.txt` in the release).
