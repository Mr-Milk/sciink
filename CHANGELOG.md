# Changelog

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
