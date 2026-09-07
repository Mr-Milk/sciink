# Plan: Rewrite Scientific-Inkscape core tools as a Rust binary extension

## Context

[Scientific-Inkscape](https://github.com/burghoff/Scientific-Inkscape) (950★) is the de-facto toolkit for
cleaning up plots exported from matplotlib / MATLAB / R / Origin inside Inkscape (Flattener, Scaler,
Homogenizer, Text Ghoster, Combine by Color, Favorite Markers, Autoexporter, Gallery Viewer). Two chronic
problems motivate a from-scratch rewrite:

1. **Fragility across Inkscape versions.** It is pure Python on top of `inkex`, monkey-patches inkex
   internals, vendors a private copy of inkex 1.3 + site-packages, and depends on the Python/Pango
   bundled with each Inkscape build. Every Inkscape release (1.2 → 1.3 → 1.4, and the GTK4-based 1.5
   in development) breaks something.
2. **Slowness.** Per-element CSS cascade in Python, pure-Python geometry, and shelling out to the
   `inkscape` binary for bounding boxes make the Flattener take many seconds on a normal figure and
   make live preview unusable.

Decisions already taken with the user (2026-09-07):

| Question | Decision |
|---|---|
| Scope | **Core plot tools first**: Flattener, Scaler, Homogenizer, Text Ghoster, Combine by Color, Favorite Markers. Autoexporter + Gallery Viewer (background watcher, Flask UI, Office/PDF conversion, ~7k LOC) deferred to a separate sub-project. |
| Language | **Rust**, shipped as a single compiled binary that Inkscape executes directly. No Python, no inkex. |
| Platforms | **Cross-platform public release**: macOS arm64 + x86_64, Windows x86_64, Linux x86_64, built by GitHub Actions; installed by unzipping into the user-extensions dir. |
| Plot sources that must work | matplotlib SVG (`svg.fonttype=none`), PDF imported via Inkscape, MATLAB/Origin/Excel SVG, R/ggplot (svglite + cairo). |
| Text metrics | **Own metrics stack**: fontdb (font discovery/matching) + rustybuzz (shaping → advances/kerning) + ttf-parser (ink boxes, OS/2 metrics). No usvg text internals, no Inkscape subprocess. |
| Parity bar | **Behavioural parity + visual invariance**: same operations/heuristics/constants as upstream; verified by structural asserts, resvg pixel-diff for appearance-preserving ops, and upstream ref comparisons where fonts allow (±1 px). Upstream quirks may be fixed. |

Project directory: `/Users/yzheng/Projects/better-inkscape-scientific` (currently empty, not yet a git repo).
Local Inkscape: 1.4.4 at `/Applications/Inkscape.app` (bundles Python 3.10, inkex 1.4.0).
Upstream reference source: the installed copy at
`~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/scientific_inkscape`
(~16k LOC); upstream tests/fixtures at `~/Downloads/Scientific-Inkscape-dev/tests`.

## Confirmed facts about the Inkscape extension protocol (from Inkscape source, master)

- `.inx` `<script><command location="inx">bin/NAME</command></script>` with **no `interpreter`
  attribute** makes Inkscape execute the file directly as `argv[0]`
  (`script.cpp`: `interpreted = in_command.size() == 2`; `extension.cpp`: a command without
  `interpreter` is a `Dependency::TYPE_EXECUTABLE`).
- For executable dependencies on Windows, Inkscape probes `.exe/.cmd/.bat/.com` suffixes, so one
  `.inx` naming `bin/NAME` works on all three OSes (`dependency.cpp`). On Unix the file must have the
  exec bit (`FILE_TEST_IS_EXECUTABLE`) — release zips must be built on a Unix runner so mode bits survive.
- Invocation: `argv = [binary, --param=value..., --id=ID (repeated per selected object),
  --selected-nodes=..., /path/to/temp-input.svg]`. Output SVG goes to **stdout**; anything on **stderr**
  is shown to the user in a dialog (this is how errors/warnings are surfaced). Exit code is ignored.
- For a directly-executed binary the working directory is **not** changed to the .inx dir (only
  interpreted scripts get that), so resources must be located relative to `std::env::current_exe()`.
- `<effect needs-live-preview="true">` re-runs the binary on every parameter change → speed matters.
- Ink/Stitch is an existing precedent for a frozen-binary Inkscape extension distributed per-OS.
- macOS Gatekeeper: a binary extracted from a browser-downloaded zip carries `com.apple.quarantine`
  and will be killed on exec unless notarized. Mitigation is a release-notes step
  (`xattr -dr com.apple.quarantine <dir>`) or a curl-based install one-liner (curl does not set
  quarantine); notarization is optional later.

## Confirmed facts about the Rust SVG/text stack (crate versions as of 2026-09-07)

- `usvg 0.48.1` (with `fontdb 0.24`, `rustybuzz`, `ttf-parser 0.25`) parses a full SVG, resolves CSS/
  `use`/transforms, **preserves element ids**, and exposes per-node `bounding_box()` /
  `abs_bounding_box()` for paths, images, groups and text. `Text::layouted()` returns `Span`s whose
  `positioned_glyphs: Vec<PositionedGlyph>` give per-glyph `text`, `font: fontdb::ID`, `id: GlyphId`,
  `font_size()` and a `transform()` that already maps font units → user space. Glyph outlines via
  `ttf-parser` give exact per-character bboxes. → usvg can replace both `inkscape --query-all` and the
  Python Pango text-measurement code as a **read-only measurement oracle**.
- `kurbo 0.13` for affine/bezier/bbox math; `quick-xml 0.42` / `roxmltree 0.21` for XML;
  `svgtypes 0.16` for path-data/length/transform parsing; `clap 4` for the CLI.
- Toolchain present: rustc 1.93 / cargo 1.93 (aarch64-apple-darwin installed).

## Architecture

One Rust crate `sciink` → one binary. Inkscape executes it directly; each `.inx` carries a hidden
`<param name="tool" type="string" gui-hidden="true">flatten</param>` so the binary dispatches on `--tool=`.

```
Inkscape ──argv: --tool=flatten --deepungroup=true … --id=g12 --id=g13 /tmp/ink_ext_XXXX.svg──▶ sciink
                                                                          stdout ◀── modified SVG
                                                                          stderr ◀── warnings (dialog)
src/
  main.rs            CLI (clap): --tool, params as --name=value (bools "true"/"false"), --id (repeat),
                     --selected-nodes, positional input, --output for local use; on ANY error: write the
                     ORIGINAL input to stdout + message to stderr (Inkscape then shows the dialog, doc untouched)
  dom/               arena DOM over quick-xml (lossless: comments, PIs, CDATA, entities, attr order, prefixes),
                     id index, parent pointers, new_id("sciink-N"), serializer
  style/             cascade: presentation attrs < <style> sheet (tag/.class/#id/comma/descendant) < style="",
                     `font:` shorthand, inheritance + SVG defaults table, per-node memo w/ subtree invalidation
  geom/              Affine/BezPath/Rect helpers (kurbo + svgtypes), ipx units, number formatting  (App. B.1)
  ops/               style compose, clip/mask merge, ungroup/unlink/deswitch, bbox, strokefill,
                     fuse/global_transform/combine_paths, cleanup                                  (App. B.2)
  text/              fonts (fontdb) → metrics (rustybuzz/ttf-parser) → char table → parse/layout → edit →
                     kerning pipeline → clean writer                                                 (App. A)
  tools/             flatten, scale, homogenize, ghost, combine, markers                           (App. B.3)
inx/                 6 .inx files (upstream param names/defaults kept so users can switch)
dist/                zip assembly script, README for installers
tests/               unit + golden + visual-invariance (resvg dev-dep) + corpus generator + bench harness
.github/workflows/   ci.yml (fmt, clippy, test on 3 OS), release.yml (tag → per-OS zips)
```
Estimated size: dom+style ≈ 1.2k, geom+ops ≈ 1.8k, text ≈ 4.5k, tools ≈ 1.5k, main/cli ≈ 0.2k → **≈ 9k LOC
Rust** replacing ≈ 25k LOC Python (+ 300k vendored).

Key design choices (why):
- **Edit the model, write once** (App. A.0): upstream re-parses XML after every text stage; we parse `<text>`
  → model once, run all stages on the model, regenerate the element once. Removes the most fragile third.
- **No usvg text internals; no Inkscape subprocess.** rustybuzz is a HarfBuzz port, i.e. the same shaping
  engine Pango uses → same advances/kerning for the same font file. fontdb finds system fonts; we add
  fontconfig's metric-alias and generic-family tables so `sans-serif`/`Helvetica`/`Calibri` resolve like
  Inkscape's fontconfig does (App. A.3).
- **Bbox math in-house with kurbo** (exact bezier extrema, rough control-box mode, clip clamping, `use`,
  groups) — replaces both `--query-all` and 130 lines of Python bbox code (App. B.2).
- **Compatibility attributes kept verbatim** (`inkscape-scientific-*`, App. B.4) so documents processed by the
  Python version keep working.
- **Deliberate deviations from upstream** (each flagged in the appendices): Scaler's hidden Fixed mode
  dropped; Favorite Markers uses JSON + typed template name instead of pickle + self-modifying `.inx`;
  flowed text is bbox-only in v1 and excluded from merges; `currentColor` resolved; `stroke-width`
  compensation only written when the scale actually changed; rect `rx/ry` scaled on fuse; inherited strokes
  get explicit widths on fuse; Homogenizer stroke stats restricted to stroked elements.

## Milestones (each ends with the verification listed; Flattener is usable from M3)

| # | Deliverable | Verification |
|---|---|---|
| M0 | Repo bootstrap: `cargo init`, git init, deps pinned, `sciink --version`, `--tool=noop` that round-trips an SVG; one `.inx` installed into `~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink/`; CI skeleton | Inkscape 1.4.4 runs the noop tool on a matplotlib SVG without dialog; round-trip diff of all 11 upstream fixtures is whitespace-only |
| M1 | `dom` + `style` + `geom` | Unit tests: cascade cases from App. B.2 contract (class sheets in `Text_tests.svg`, `font:` shorthand, presentation attrs); path parse/serialize round-trip on Acid_tests (8460 paths) |
| M2 | `ops`: compose/clip merge/ungroup/unlink/deswitch/bbox/strokefill/cleanup + **Flattener non-text** (deepungroup, white rects, minus/thin-rect reversion, dup removal, font replacement) | resvg pixel-diff input vs output on Acid/Other fixtures (white-rect removal disabled) below threshold; dup-removal counts vs Python on Acid_tests (R8); timing on Acid_tests |
| M3 | `text` fonts+metrics+table+parse+layout + `text_bbox`; **Text Ghoster**; **Combine by Color** (+fuse/global_transform/combine_paths) | `font-probe` vs `fc-match` for all fixture families (A.5-1); char extents vs the `--debugparser` ref for locally-installed fonts (A.5-2A); Ghoster/Combine outputs vs upstream refs within tolerance |
| M4 | `text` edit + kerning stages 4–12 + writer → **Flattener complete** | Flattener refs (`Text_tests`, `Text_tests_dx`, `Acid_tests` combos from `test_main.py`) within tolerance where fonts exist; PDF-import sample merges correctly; visual invariance on matplotlib/svglite corpus |
| M5 | **Scaler** (correction, matching, ticks, scale_free/aspect_locked, combined ranges) + **Homogenizer** | `Other_tests.svg` `g5224` correction and `rect5248`+`g4982` matching vs refs (R9); Homogenizer refs; manual check in Inkscape |
| M6 | **Favorite Markers** (JSON store), all 6 `.inx` final, live-preview enabled where < 200 ms, README/install docs, release workflow producing per-OS zips, macOS quarantine note | Install from the built zip on this Mac and run every tool from the Inkscape menu; CI green on 3 OS; benchmark table old vs new |

Out of scope (separate sub-project later): Autoexporter, Gallery Viewer, Office/PDF finalizer.

Ordering note: the appendices each carry their own build order (A.6, B.7, C.7); where they differ from the
milestone table above, **the milestone table wins** — it front-loads the Flattener because that is the tool
people install the suite for, and uses Text Ghoster / Combine by Color as the first small consumers of the
bbox and transform machinery. Infra items from C.7 steps 1–6 (skeleton, CLI/About, style, test harnesses,
corpus, packaging) are spread across M0–M1 and M6.

## Cross-design decisions (where the three designs disagreed)

- **Favorite Markers**: JSON store + typed template name, **no self-modifying `.inx`** (App. B.3), not the
  atomic `.inx` rewrite variant from App. C. Only one tool writes files, and no Inkscape restart is needed.
- **Style cache**: global generation counter + per-node `(gen, Rc<Style>)` memo (App. C.2), not subtree
  invalidation; O(depth) recompute is fine.
- **`font:` shorthand IS supported** (matplotlib `svg.fonttype=none` emits `style="font: 10px 'DejaVu Sans'"`;
  App. A.2 is right, App. C.2's "unsupported" note is superseded). ~40 LOC.
- **One number formatter** `num::fmt` (8 significant digits, App. C.1) used by geom, text writer and all
  attribute writes; App. A's "1e-6 rounding" is subsumed.
- **Tool names** on the CLI/`.inx`: `flattener | scaler | homogenizer | text-ghoster | combine-by-color |
  favorite-markers | about` (App. C.3), ids `org.sciink.<tool>`, submenu `Scientific`.
- **`--testmode`** hidden param on the Flattener is ported (duplicate selected layer locked at opacity 0.3,
  force all fixes, replacement `sans-serif`) because every upstream Flattener ref was produced with it.

## Verification (end-to-end)

1. **Unit + golden + invariance + snapshot tests** run with `cargo test` (vendored DejaVu Sans + Roboto,
   `SCIINK_NO_SYSTEM_FONTS=1 SCIINK_FONT_DIRS=tests/fonts` for determinism; proprietary-font cases `#[ignore]`
   unless `SCIINK_SYSTEM_FONTS=1`). Golden cases and their reproducibility on this Mac are enumerated in App. C.5.
2. **Manual Inkscape loop on this Mac** (App. C.6): `cargo build --release`, symlink `inx/*.inx` and the binary
   into `~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink/`, restart
   Inkscape, run Extensions ▸ Scientific ▸ Diagnostics, then each tool on a matplotlib SVG; phase timings in
   `SCIINK_LOG`.
3. **Headless Inkscape-side invocation** (same code path as the GUI):
   ```bash
   /Applications/Inkscape.app/Contents/MacOS/inkscape --actions="select-by-id:layer1;org.sciink.flattener.noprefs;export-type:svg;export-filename:/tmp/out.svg;export-do" in.svg
   ```
4. **Visual check**: `rsvg-convert` (present) input vs output PNG + `compare -metric AE`; resvg pixel-diff in tests.
5. **Benchmark table** old vs new (App. C.5f) — blocked until the user repairs Inkscape's bundle
   (`xattr -dr com.apple.quarantine /Applications/Inkscape.app` or reinstall); user action, not ours.
6. **Release dry run**: tag `v0.0.1` (About tool only) → CI zips for 3 OSes → install from the zip on this Mac
   via the curl one-liner and via a browser download (expect Gatekeeper failure; confirm the documented fix).

## Process notes
- After approval: `git init`, commit the two design appendices as `docs/spec/text-engine.md` and
  `docs/spec/geometry-tools.md` (they are the executable specs), then proceed milestone by milestone with
  TDD; `gh repo create --public` is an outward-facing step and will be confirmed with the user first.
- Autoexporter/Gallery Viewer: separate brainstorm + spec later.

---

