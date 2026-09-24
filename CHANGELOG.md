# Changelog

## 0.2.0 (unreleased)

Large documents no longer freeze Inkscape for our part of the run. On a 52 MB, 62 000-element
manuscript the Flattener went from 1.1 s to 212 ms on one figure and from 6.1 s to 1.0 s on the
whole layer (Homogenizer: 386 ms → 135 ms); Inkscape's own save/reload of such a file is unchanged
and documented in `docs/DEVELOPING.md`. The new Slimmer tool removes what makes Inkscape slow on
such documents.

- Flattener: the character table covers only the measured elements (upstream parity; it measured
  every text in the document); duplicate and white-rectangle removal use index sweeps instead of
  all-pairs tests; the nested-text check in kerning removal is linear.
- Style cascade: universal `*{…}` rules are pre-merged per stylesheet; declarations are no longer
  cloned per node; `clip-path`/`mask` sheet lookups short-circuit.
- DOM: a move re-indexes nothing; identical attribute writes are skipped; the writer copies attribute
  values in bulk and pre-sizes its buffer.
- Fonts: the scan is cached on disk (`fontcache-2-<key hash>.tsv`; ≈ 3 s → tens of ms after boot); DejaVu Sans
  (Book, Bold) is bundled as the matplotlib fallback, an installed copy takes precedence; Diagnostics
  and Font Probe show `[bundled]`.
- Live preview turned off on every tool (`needs-live-preview="false"`, matching upstream); a live
  preview re-ran the tool and reloaded the document on every keystroke, which is what froze Inkscape
  on large files.
- `SCIINK_LOG` records one line per phase with its duration; new switches `SCIINK_NO_FONT_CACHE`,
  `SCIINK_FONT_CACHE`, `SCIINK_NO_BUNDLED_FONTS`.
- CI and release runners pinned to `ubuntu-24.04` instead of the rolling `ubuntu-latest`, so a GitHub
  Actions image change can no longer break a build or silently drop the Linux release asset.
- Corrected the `<use>`-inside-`clipPath` known gap: the missing box was a dangling `href` (upstream
  behaves the same), not a bug — now pinned by tests instead of listed as a limitation.
- Slimmer: a new tool that makes a whole document smaller and faster for Inkscape. Rendering-exact by
  default: keeps one of identical `<style>` elements (matplotlib adds one per imported figure, and
  Inkscape's load and save cost is per rule × element), removes empty elements (invisible shapes on
  request), collapses
  single-child wrapper groups, prunes unused definitions and merges identical ones with references
  repointed. Optional coordinate rounding in significant digits. A report dialog lists what changed. On a
  52 MB manuscript: 176 sheets, 3 489 definitions pruned, 4 246 merged, 2 834 groups collapsed; 62 357 →
  44 073 elements, 52.6 MB → 49.1 MB; Inkscape's round trip 27.2 s → 13.4 s.

## 0.1.0 — 2026-09-23

First release with all six tools of Scientific-Inkscape's core, as one native binary per platform
(macOS universal, Windows x64, Linux x64) with no Python dependency:

- **Flattener** — deep ungroup, clone unlinking, matplotlib minus signs and thin rectangles restored,
  the text pipeline (kerning removal, merges, splits, justification, font replacement), duplicate
  and white-background-rectangle removal.
- **Scaler** — Correction and Matching modes with tick, text and group preservation; Advanced-tab
  markings.
- **Homogenizer** — font size, font family (Inkscape font specifications), text distortion, stroke
  width, transform fusing, clip/mask removal; plot-aware text placement.
- **Text Ghoster**, **Combine by Color**.
- **Favorite Markers** — stored start/mid/end marker templates (Arrow, Triangle, Distance built
  in) applied at any size; add your own from a selected path, no restart needed.
- Diagnostics (About) and three debug tools (Font Probe, Text Highlight, Text Fix).
- One-line installers for macOS/Linux (`install.sh`) and Windows (`install.ps1`).

Known differences from the Python original are listed in `docs/spec/02-geometry-tools.md`
("Deliberate deviations") and `docs/spec/01-text-engine.md`.

## 0.1.0-alpha.1 — 2026-09-08

Release pipeline and the About tool only.
