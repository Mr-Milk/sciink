# Plan 4 — Text engine part 2: edit, kerning removal, clean writer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the measuring text engine of Plan 3 into the Flattener's text pipeline: undo `textLength`, convert differential `dx`/`dy` kerning to absolute positions, merge manually kerned chunks within an element, merge adjacent elements including sub/superscripts, split distant text, re-justify, strip stray spaces and regenerate every processed `<text>` once as a clean, Inkscape-editable element — exposed as `text::kerning::remove_kerning` (what the Flattener will call in Plan 6) and as a `text-fix` debug tool so the pipeline can be run from Inkscape today.

**Architecture:** Port of upstream `remove_kerning.py` + the editing half of `inkex/text/parser.py`, with the spec's two structural decisions (§A.0): **model-only edits, one writer**. All stages operate on a `Vec<ParsedText>` arena (chunks get stable ids; characters move between elements as cloned `TChar`s; structural edits go through `edit::reindex`), and the DOM is touched only by `text::write` (`write_clean_text`, `apply_clip_unions`) at the very end. Three new modules: `text/edit.rs` (model primitives: reindex, delete, re-chunk, append, split, re-anchor), `text/kerning.rs` (the stage drivers 4–11 and `Perform_Merges`), `text/write.rs` (stage 12). No caching of geometry: `layout::chunk_geom` is recomputed on demand (ponytail: cache when profiling says).

**Tech Stack:** Rust 2024 (rustc ≥ 1.85), kurbo 0.13, quick-xml 0.42, fontdb 0.24.0, rustybuzz 0.20.1, ttf-parser 0.25.1 (all existing; no new dependencies). Vendored test fonts: DejaVu Sans, Roboto (`tests/fonts/`).

**Spec:** `docs/spec/01-text-engine.md` — §A.0 (structural decisions), §A.1 Stages 2–12 + "Constants" + `Perform_Merges` + `append_chks` + "Geometry", §A.2 (model, "Deliberate defensive deviations"), §A.4 (public API: `KerningOptions`, `remove_kerning`, `write_clean_text`, `snapshot_parsed`). Upstream references: `RK` = `remove_kerning.py`, `P` = `inkex1_3_0/inkex/text/parser.py`, `C` = `inkex/text/cache.py`, `F` = `flatten_plots.py`, all under `~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/scientific_inkscape/` (read-only reference; never copy Python into the crate).

## Global Constraints

- Every number written into the document goes through `sciink::num::fmt` (8 significant digits, `-0` → `0`). Upstream rounds to 1e-6 then prints the shortest repr; the formatter is the project-wide choice (spec §C.1) and wins.
- All tree walks are iterative (no recursion over document structure or over the text model).
- No usvg text internals, no Inkscape subprocess, no Python. Fonts/metrics come only from `text::fonts`/`text::metrics` (Plan 3).
- Font determinism in tests: `FontSystem::from_dirs(&[tests/fonts])`; tests needing system fonts are `#[ignore]` and run only with `SCIINK_SYSTEM_FONTS=1`. Tool tests set `SCIINK_NO_SYSTEM_FONTS=1` + `SCIINK_FONT_DIRS` behind a `Once` (see `tests/text_tools.rs`).
- Style values are `Rc<Style>`; **style equality is by value and order-insensitive** (`edit::style_eq`), never `Rc::ptr_eq`.
- Constants (spec §A.1 "Constants", RK:28–51): `NUM_SPACES = 1.0`, `XTOLEXT = 0.6`, `YTOLEXT = 0.1`, `XTOLMKN = 1.5`, `XTOLMKP = 0.99`, `YTOLMK = 0.01`, `XTOLSPLIT = 0.5`, `SUBSUPER_THR = 0.99`, `SUBSUPER_YTHR = 1/3`, `FONTSIZE_THR = 0.01`, `XY_TOL = 1e-6` (existing `parse::XY_TOL`), next-chain y tolerance `0.001`, angle tolerance `0.001°`, numeric-merge gap `0.25` spaces, space-import swap `0.01·spw`.
- Stages 6–7 (manual-kerning removal, external merges) decide on **parsed** positions (the `snapshot_parsed` copies); stages 8–11 (splits, justification, space removal, position fix) decide on **current** positions (RK:96–97).
- The DOM is mutated only by `text::write` (and, upstream-style, by `ParsedText::parse`'s depathologize, unchanged from Plan 3). Every other function in `text::edit`/`text::kerning` takes `&Doc` at most.
- After parsing, `TChunk.x`/`TChunk.y` are the authoritative positions; `LineSpec.x`/`LineSpec.y` are parse-time inputs and are only kept in sync as documentation.
- Unrendered characters (no font has the glyph) keep working: zero-width, zero-ink, never a panic. Any `Option` lookup that can legitimately miss (a chunk id that was merged away, a singular transform) returns early instead of `unwrap`ping.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` must pass on every commit.
- Deviations from upstream are allowed only where this plan says so (each is marked **Deviation** with the reason) and each must be mirrored into `docs/spec/01-text-engine.md` "Deliberate defensive deviations" by Task 11.

---

## File structure

| File | Responsibility |
|---|---|
| `src/text/parse.rs` (modify) | `Origin`, chunk ids, `parsed_ut`/`parsed_t` snapshot storage, `transform_extra`, `text_anchor_override`, `text_length_removed`; `continue_x`/`continue_y` resolution; `find_chunk`, `chunk_text`, `line_text` |
| `src/text/layout.rs` (modify) | `snapshot_parsed`, `chunk_char_pts`, `get_ut_pts`, chunk aggregates (`chunk_spw/mch/utfs/tfs/scf`), `angle_deg`, `element_bbox` |
| `src/text/edit.rs` (new) | model primitives: `sel`, `style_eq`, `reindex`, `remove_chars`, `delete_char`, `remove_textlength`, `rechunk_absolute`, `make_next_chain`, `append_chunks`, `split_off`, `change_alignment`, `fix_merged_position` |
| `src/text/kerning.rs` (new) | helpers (`isnumeric`, `wstrip`, `twospaces`, `trailing_leading`), `MergeType`/`WType`/`ChunkRef`/`Cand`, `perform_merges`, stage drivers `remove_manual_kerning`, `external_merges`, `split_distant_chunks`, `split_distant_intrachunk`, `split_lines`, `change_justification`, `remove_trailing_leading_spaces`, `fix_merge_positions`, `KerningOptions`, `remove_kerning` |
| `src/text/write.rs` (new) | `specified_diff`, `write_clean_text`, `ClipUnion`, `apply_clip_unions` |
| `src/text/mod.rs` (modify) | `pub mod edit; pub mod kerning; pub mod write;` |
| `src/tools/text_fix.rs` (new); `src/cli.rs`, `src/lib.rs`, `src/tools/mod.rs` (modify) | `text-fix` debug tool |
| `inx/text_fix.inx` (new) | Scientific ▸ Debug ▸ Text Fix |
| `tests/support/mod.rs` (modify) | `text_positions`, `assert_same_positions` (appearance-invariance oracle) |
| `tests/text_edit.rs`, `tests/text_kerning.rs`, `tests/text_write.rs`, `tests/text_fix.rs` (new); `tests/text_layout.rs`, `tests/text_parse.rs` (modify) | tests |
| `docs/spec/01-text-engine.md`, `README.md` (modify) | deviations, stage-5 wording, tool listing |

Test helper conventions (existing): `mod support;` at the top of each integration test; `Doc::parse(s.as_bytes()).unwrap()`; vendored fonts via `FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])`; `support::upstream_data_dir()` returns `None` (with a SKIP note) when the upstream fixtures are absent — fixture tests must `return` in that case. Every test file in this plan starts with this preamble (copy it verbatim; do not `use` items you do not need — clippy's `-D warnings` fails on unused imports):

```rust
mod support;

use std::path::PathBuf;

use kurbo::Point;
use sciink::dom::{Doc, NodeId};
use sciink::text::Warnings;
use sciink::text::fonts::FontSystem;
use sciink::text::parse::ParsedText;
use sciink::text::table::CharTable;

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";
const DV: &str = "font-family:'DejaVu Sans'";

/// Parses `el` (and builds the char table over every `<text>` in the document).
fn parsed(d: &mut Doc, el: &str) -> (ParsedText, CharTable) {
    let els: Vec<NodeId> = d
        .descendants(d.svg())
        .filter(|&n| d.is_element(n) && d.tag(n) == "text")
        .collect();
    let mut w = Warnings::default();
    let n = id(d, el);
    let mut ct = CharTable::build(d, &els, fonts(), &mut w);
    let pt = ParsedText::parse(d, n, &mut ct, &mut w).expect("parsed");
    (pt, ct)
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}
```

---

### Task 1: Model additions — chunk ids, snapshots, `continue_x` resolution, frame helpers

**Files:**
- Modify: `src/text/parse.rs` (`TChunk`, `ParsedText`, `ParsedText::parse` chunk loop and its tail)
- Modify: `src/text/layout.rs` (append)
- Test: `tests/text_parse.rs`, `tests/text_layout.rs` (append)

**Interfaces:**
- Consumes (Plan 3): `ParsedText::parse`, `TChar { line, chunk, windex, dx, dy, cwd, spw, caph, utfs, tfs, .. }`, `TLine { spec: LineSpec, style, chars, chunks }`, `layout::{chunk_geom, char_pts_ut, transform_pts, pts_bbox, unrendered_space, text_bbox}`, `geom::{inverse, scale_factor}`.
- Produces:
  - `pub enum Origin { Existing, SplitFrom }` (parse.rs)
  - `pub struct TChunk { pub id: u32, pub x: f64, pub y: f64, pub chars: Vec<usize>, pub next: Option<u32>, pub prev: Option<u32>, pub prev_same_tspan: bool }`
  - `ParsedText` new fields: `pub origin: Origin`, `pub parsed_ut: Vec<Option<[Point; 4]>>`, `pub parsed_t: Vec<Option<[Point; 4]>>`, `pub transform_extra: Affine`, `pub text_anchor_override: Option<Anchor>`, `pub text_length_removed: bool`, `pub next_chunk_id: u32`
  - `ParsedText::{new_chunk_id(&mut self) -> u32, find_chunk(&self, id: u32) -> Option<(usize, usize)>, chunk_text(&self, li, ci) -> String, line_text(&self, li) -> String}`
  - layout.rs: `snapshot_parsed(pt: &mut ParsedText)`, `chunk_char_pts(pt, li, ci) -> Vec<[Point; 4]>`, `get_ut_pts(a: &ParsedText, wa: (usize, usize), b: &ParsedText, wb: (usize, usize), parsed: bool) -> Option<[Point; 4]>` (`[tr1, br1, tl2, bl2]`), `chunk_spw/chunk_mch/chunk_utfs/chunk_tfs/chunk_scf(pt, li, ci) -> f64`, `angle_deg(t: Affine) -> f64`, `element_bbox(doc: &mut Doc, el: NodeId, ct: &mut CharTable, warn: &mut Warnings) -> Option<Rect>`

Background: Plan 3 stored a placeholder for lines whose `x` (or `y`) is inherited from the previous line: `LineSpec.x` copies the previous line's list and `continue_x = true`, and `ParsedText::parse` then used that start-of-previous-line value as the chunk `x`. Upstream (P:2640–2653, P:2661–2675) positions such a line at the **end** of the previous line: `x = (1 + anfr)·prev_last.pts_ut[3].x − anfr·prev_last.pts_ut[0].x` (`prev_last` = the previous line's last chunk, `anfr` = the continuing line's own anchor fraction) and `y = the previous line's last chunk y`. Spec §A.1 stage 1d says the same. This task makes `ParsedText::parse` resolve it; `line_specs` and its test are untouched (the `LineSpec` keeps the placeholder and its flags).

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_parse.rs`. The file already has `fonts()`, `doc(s: &str) -> Doc`, `id(&Doc, &str)` and `NS`; add `const DV: &str = "font-family:'DejaVu Sans'";`, the preamble's `parsed(d: &mut Doc, el: &str) -> (ParsedText, CharTable)` and `close(a, b)` helpers (plus `use sciink::text::parse::ParsedText;`), then:

```rust
#[test]
fn continue_lines_start_where_the_previous_line_ends() {
    // "cd" has y but no x: SVG/Inkscape continue it from the pen after "ab" (P:2640–2653).
    let mut d = doc(&format!(
        r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="5" y="20">ab<tspan id="s" y="40">cd</tspan></text></svg>"#
    ));
    let (pt, _) = parsed(&mut d, "t");
    assert_eq!(pt.lines.len(), 2);
    assert!(pt.lines[1].spec.continue_x && !pt.lines[1].spec.continue_y);
    let g0 = sciink::text::layout::chunk_geom(&pt, 0, 0);
    let end = g0.pts_ut[3].x; // start anchor: (1+0)·right − 0·left
    assert!(close(pt.lines[1].chunks[0].x, end), "{} vs {end}", pt.lines[1].chunks[0].x);
    assert!(close(pt.lines[1].chunks[0].y, 40.0));
    assert!(end > 5.0 + pt.chars[0].cwd, "the second line starts after 'ab'");

    // continue_y: x given, y inherited = previous line's last chunk y
    let mut d = doc(&format!(
        r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="5" y="20">ab<tspan id="s" x="50">cd</tspan></text></svg>"#
    ));
    let (pt, _) = parsed(&mut d, "t");
    assert!(pt.lines[1].spec.continue_y && !pt.lines[1].spec.continue_x);
    assert!(close(pt.lines[1].chunks[0].x, 50.0) && close(pt.lines[1].chunks[0].y, 20.0));

    // middle anchor uses upstream's (1+anfr)·right − anfr·left form verbatim (spec risk 7)
    let mut d = doc(&format!(
        r#"<svg {NS}><text id="t" style="{DV};font-size:10px;text-anchor:middle" x="5" y="20">ab<tspan id="s" y="40">cd</tspan></text></svg>"#
    ));
    let (pt, _) = parsed(&mut d, "t");
    let g0 = sciink::text::layout::chunk_geom(&pt, 0, 0);
    let expect = 1.5 * g0.pts_ut[3].x - 0.5 * g0.pts_ut[0].x;
    assert!(close(pt.lines[1].chunks[0].x, expect));
}

#[test]
fn chunk_ids_are_unique_and_findable() {
    let mut d = doc(&format!(
        r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0 30" y="0">ab<tspan x="0" y="20">c</tspan></text></svg>"#
    ));
    let (mut pt, _) = parsed(&mut d, "t");
    let ids: Vec<u32> = pt.chunks().map(|(li, ci)| pt.chunk(li, ci).id).collect();
    assert_eq!(ids, [0, 1, 2]);
    assert_eq!(pt.find_chunk(1), Some((0, 1)));
    assert_eq!(pt.find_chunk(2), Some((1, 0)));
    assert_eq!(pt.find_chunk(7), None);
    assert_eq!(pt.new_chunk_id(), 3);
    assert_eq!(pt.chunk_text(0, 1), "b");
    assert_eq!(pt.line_text(0), "ab");
    assert_eq!(pt.origin, sciink::text::parse::Origin::Existing);
    assert!(pt.parsed_ut.is_empty(), "no snapshot until snapshot_parsed");
    assert!(sciink::geom::is_identity(pt.transform_extra));
}
```

Append to `tests/text_layout.rs`:

```rust
#[test]
fn snapshot_and_get_ut_pts_follow_upstream_frames() {
    use sciink::text::layout::{chunk_char_pts, get_ut_pts, snapshot_parsed, transform_pts};
    // two chunks in one element, translated by (10, 20)
    let mut d = Doc::parse(
        format!(
            r#"<svg {NS}><g transform="translate(10,20)"><text id="t" style="{DV};font-size:10px" x="0 30" y="0">ab</text></g></svg>"#
        )
        .as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    snapshot_parsed(&mut pt);
    assert_eq!(pt.parsed_ut.len(), 2);
    let cur = chunk_char_pts(&pt, 0, 0)[0];
    assert_eq!(pt.parsed_ut[0], Some(cur));
    assert_eq!(pt.parsed_t[0], Some(transform_pts(pt.transform, cur)));
    assert!(close(pt.parsed_t[0].unwrap()[0].x, cur[0].x + 10.0));
    // get_ut_pts: [tr1, br1, tl2, bl2] — a's rightmost TR/BR, b's leftmost TL/BL in a's frame
    let [tr1, br1, tl2, bl2] = get_ut_pts(&pt, (0, 0), &pt, (0, 1), true).unwrap();
    let pa = chunk_char_pts(&pt, 0, 0)[0];
    let pb = chunk_char_pts(&pt, 0, 1)[0];
    assert_eq!((tr1, br1), (pa[2], pa[3]));
    assert!(close(tl2.x, pb[1].x) && close(tl2.y, pb[1].y));
    assert!(close(bl2.x, 30.0) && close(bl2.y, 0.0));
    // current positions agree with parsed ones before any edit
    assert_eq!(get_ut_pts(&pt, (0, 0), &pt, (0, 1), false).unwrap(), [tr1, br1, tl2, bl2]);
}

#[test]
fn chunk_aggregates_and_angle() {
    use sciink::text::layout::{angle_deg, chunk_mch, chunk_scf, chunk_spw, chunk_tfs, chunk_utfs};
    let mut d = Doc::parse(
        format!(
            r#"<svg {NS}><text id="t" transform="matrix(0,1,-1,0,0,0) scale(2)" style="{DV};font-size:10px" x="0" y="0">a<tspan style="font-size:20px">b</tspan></text></svg>"#
        )
        .as_bytes(),
    )
    .unwrap();
    let (pt, _) = parsed(&mut d, "t");
    assert!(close(chunk_utfs(&pt, 0, 0), 20.0));
    assert!(close(chunk_tfs(&pt, 0, 0), 40.0));
    assert!(close(chunk_scf(&pt, 0, 0), 2.0), "first char's tfs/utfs");
    assert!(close(chunk_spw(&pt, 0, 0), pt.chars[1].spw));
    assert!(close(chunk_mch(&pt, 0, 0), pt.chars[1].caph));
    // matrix(0,1,-1,0,…) rotates by 90°: atan2(c, d) = atan2(-2, 0) = -90°
    assert!(close(angle_deg(pt.transform), -90.0));
}

#[test]
fn element_bbox_wraps_parse_plus_text_bbox() {
    use sciink::text::layout::element_bbox;
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="5" y="20">ab</text></svg>"#)
            .as_bytes(),
    )
    .unwrap();
    let els = vec![id(&d, "t")];
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &els, fonts(), &mut w);
    let bb = element_bbox(&mut d, els[0], &mut ct, &mut w).expect("bbox");
    let (pt, _) = parsed(&mut d, "t");
    assert_eq!(Some(bb), text_bbox(&pt));
    let mut d2 = Doc::parse(format!(r#"<svg {NS}><text id="e" x="0" y="0"></text></svg>"#).as_bytes()).unwrap();
    let e = id(&d2, "e");
    assert_eq!(element_bbox(&mut d2, e, &mut ct, &mut w), None);
}
```

`tests/text_layout.rs`'s existing `parsed` helper takes `(svg: &str, el: &str)`; either add the preamble's `parsed(&mut Doc, &str)` under a different name (`parsed_doc`) or adapt these tests to the file's helper — the assertions are what matter.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test text_parse continue_lines chunk_ids 2>&1 | tail -20` and `cargo test --test text_layout snapshot chunk_aggregates element_bbox 2>&1 | tail -20`
Expected: compile errors (`id`, `origin`, `parsed_ut`, `find_chunk`, `snapshot_parsed`, … do not exist).

- [ ] **Step 3: Extend the model in `src/text/parse.rs`**

Add after `TextLengthAdj`:

```rust
/// Where a `ParsedText` came from: an element that exists in the document (rewritten in place,
/// id reused) or a piece split off another element by `edit::split_off` (a new element, inserted
/// right after the element it came from, which is what `el` then names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Existing,
    SplitFrom,
}
```

Replace `TChunk` with:

```rust
#[derive(Debug, Clone)]
pub struct TChunk {
    /// Stable within one `ParsedText`; survives `edit::reindex`, so merge plans can refer to a
    /// chunk while other chunks are being removed. Look it up with `ParsedText::find_chunk`.
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub chars: Vec<usize>,
    /// Next/previous chunk on the same baseline within this element (stage 4, `edit::make_next_chain`).
    pub next: Option<u32>,
    pub prev: Option<u32>,
    /// `prev`'s last char and this chunk's first char sit in the same style node (P:724–725).
    pub prev_same_tspan: bool,
}
```

Add to `ParsedText` (after `any_dy`):

```rust
    pub origin: Origin,
    /// Per-character corner points frozen by `layout::snapshot_parsed` (stage 3): `[BL, TL, TR, BR]`
    /// in this element's frame and in root coordinates. Index-aligned with `chars`; `None` for
    /// characters created after the snapshot (inserted spaces). Empty until the snapshot is taken.
    pub parsed_ut: Vec<Option<[Point; 4]>>,
    pub parsed_t: Vec<Option<[Point; 4]>>,
    /// Extra transform the writer multiplies onto the element's own `transform` (stage 2).
    pub transform_extra: Affine,
    /// `text-anchor`/`text-align` to write on the `<text>` itself (stage 9, RK:175–178).
    pub text_anchor_override: Option<Anchor>,
    /// `textLength`/`lengthAdjust` were undone (stage 2) and must not be copied by the writer.
    pub text_length_removed: bool,
    pub next_chunk_id: u32,
```

Add `use kurbo::Point;` (kurbo is a dependency; `crate::geom::Affine` stays). In `ParsedText::parse`, extend the struct literal with `origin: Origin::Existing, parsed_ut: Vec::new(), parsed_t: Vec::new(), transform_extra: Affine::IDENTITY, text_anchor_override: None, text_length_removed: false, next_chunk_id: 0`. In the chunk loop replace the two `TChunk { x: px, y: py, chars: vec![ci] }` constructions with

```rust
                    let id = pt.new_chunk_id();
                    ln.chunks.push(TChunk {
                        id,
                        x: px,
                        y: py,
                        chars: vec![ci],
                        next: None,
                        prev: None,
                        prev_same_tspan: false,
                    });
```

(`pt` and `lines` are separate locals there, so borrowing is fine.) Then, right after `pt.lines = lines;`, resolve continue lines:

```rust
        // Lines that inherit a coordinate continue from the END of the previous line
        // (P:2640–2653 for x — upstream's anchor form verbatim, spec risk 7; P:2661–2675 for y:
        // the previous line's last chunk y). Chunks after the first in such a line carry the
        // resolved coordinate forward when they had none of their own.
        for li in 1..pt.lines.len() {
            let (cx, cy) = (pt.lines[li].spec.continue_x, pt.lines[li].spec.continue_y);
            if !(cx || cy) {
                continue;
            }
            let pli = li - 1;
            let pci = pt.lines[pli].chunks.len() - 1;
            let prev_y = pt.lines[pli].chunks[pci].y;
            let g = super::layout::chunk_geom(&pt, pli, pci);
            let anfr = pt.lines[li].spec.anchor.anfr();
            let old_x = pt.lines[li].chunks[0].x;
            let old_y = pt.lines[li].chunks[0].y;
            let new_x = (1.0 + anfr) * g.pts_ut[3].x - anfr * g.pts_ut[0].x;
            let ln = &mut pt.lines[li];
            for ch in ln.chunks.iter_mut() {
                if cx && ch.x == old_x {
                    ch.x = new_x;
                }
                if cy && ch.y == old_y {
                    ch.y = prev_y;
                }
            }
            if cx {
                ln.spec.x = vec![Some(new_x)];
            }
            if cy {
                ln.spec.y = vec![Some(prev_y)];
            }
        }
```

(`ch.x == old_x` identifies the chunks that carried the placeholder: Plan 3's chunk loop copies `xs[0]` into every chunk without an own `x`, so they all hold the same placeholder value.) Add the methods:

```rust
impl ParsedText {
    pub fn new_chunk_id(&mut self) -> u32 {
        let id = self.next_chunk_id;
        self.next_chunk_id += 1;
        id
    }

    /// `(line, chunk)` of the chunk with this id, or `None` when it has been merged away.
    pub fn find_chunk(&self, id: u32) -> Option<(usize, usize)> {
        self.lines.iter().enumerate().find_map(|(li, l)| {
            l.chunks
                .iter()
                .position(|c| c.id == id)
                .map(|ci| (li, ci))
        })
    }

    pub fn chunk_text(&self, li: usize, ci: usize) -> String {
        self.lines[li].chunks[ci]
            .chars
            .iter()
            .map(|&c| self.chars[c].c)
            .collect()
    }

    pub fn line_text(&self, li: usize) -> String {
        self.lines[li].chars.iter().map(|&c| self.chars[c].c).collect()
    }
}
```

- [ ] **Step 4: Frame helpers in `src/text/layout.rs`**

Append:

```rust
/// Stage 3 (`precalcs`): freeze every character's current corners in the element frame and in
/// root coordinates. Stages 6–7 compare these, not the live positions.
pub fn snapshot_parsed(pt: &mut ParsedText) {
    let n = pt.chars.len();
    let mut ut = vec![None; n];
    let mut t = vec![None; n];
    each_char(pt, |c, g, wi| {
        let p = char_pts_ut(pt, g, wi);
        ut[c] = Some(p);
        t[c] = Some(transform_pts(pt.transform, p));
    });
    pt.parsed_ut = ut;
    pt.parsed_t = t;
}

/// Current corners `[BL, TL, TR, BR]` of every character of one chunk, index-aligned with `chunk.chars`.
pub fn chunk_char_pts(pt: &ParsedText, li: usize, ci: usize) -> Vec<[Point; 4]> {
    let g = chunk_geom(pt, li, ci);
    (0..pt.lines[li].chunks[ci].chars.len())
        .map(|wi| char_pts_ut(pt, &g, wi))
        .collect()
}

fn char_corners(pt: &ParsedText, (li, ci): (usize, usize), parsed: bool) -> (Vec<Option<[Point; 4]>>, Vec<Option<[Point; 4]>>) {
    let ids = &pt.lines[li].chunks[ci].chars;
    if parsed {
        (
            ids.iter().map(|&c| pt.parsed_ut.get(c).copied().flatten()).collect(),
            ids.iter().map(|&c| pt.parsed_t.get(c).copied().flatten()).collect(),
        )
    } else {
        let ut = chunk_char_pts(pt, li, ci);
        let t = ut.iter().map(|p| Some(transform_pts(pt.transform, *p))).collect();
        (ut.into_iter().map(Some).collect(), t)
    }
}

/// P:3421–3454. `[tr1, br1, tl2, bl2]`: the TR/BR corners of chunk `a`'s rightmost character (in
/// `a`'s frame) and the TL/BL corners of chunk `b`'s leftmost character, taken in root coordinates
/// and mapped back through the inverse of `a`'s transform. `parsed` selects the stage-3 snapshot
/// (stages 6–7) or the live positions (stage 8). `None` when a chunk has no usable points or `a`'s
/// transform is singular.
pub fn get_ut_pts(
    a: &ParsedText,
    wa: (usize, usize),
    b: &ParsedText,
    wb: (usize, usize),
    parsed: bool,
) -> Option<[Point; 4]> {
    let inv = crate::geom::inverse(a.transform)?;
    let (a_ut, _) = char_corners(a, wa, parsed);
    let (b_ut, b_t) = char_corners(b, wb, parsed);
    let mut ai = None;
    let mut maxv = f64::NEG_INFINITY;
    for (i, p) in a_ut.iter().enumerate() {
        if let Some(p) = p {
            if p[3].x > maxv {
                maxv = p[3].x;
                ai = Some(i);
            }
        }
    }
    let mut bi = None;
    let mut minv = f64::INFINITY;
    for (i, p) in b_ut.iter().enumerate() {
        if let Some(p) = p {
            if p[0].x < minv {
                minv = p[0].x;
                bi = Some(i);
            }
        }
    }
    let ap = a_ut[ai?]?;
    let bt = b_t[bi?]?;
    Some([ap[2], ap[3], inv * bt[1], inv * bt[0]])
}

fn chunk_max(pt: &ParsedText, li: usize, ci: usize, f: impl Fn(&TChar) -> f64) -> f64 {
    pt.lines[li].chunks[ci]
        .chars
        .iter()
        .map(|&c| f(&pt.chars[c]))
        .fold(f64::NEG_INFINITY, f64::max)
}
/// Chunk-level values upstream exposes as properties (P:3531–3553): all maxima over the chunk.
pub fn chunk_spw(pt: &ParsedText, li: usize, ci: usize) -> f64 {
    chunk_max(pt, li, ci, |c| c.spw)
}
pub fn chunk_mch(pt: &ParsedText, li: usize, ci: usize) -> f64 {
    chunk_max(pt, li, ci, |c| c.caph)
}
pub fn chunk_utfs(pt: &ParsedText, li: usize, ci: usize) -> f64 {
    chunk_max(pt, li, ci, |c| c.utfs)
}
pub fn chunk_tfs(pt: &ParsedText, li: usize, ci: usize) -> f64 {
    chunk_max(pt, li, ci, |c| c.tfs)
}
/// Scale of the chunk's first character (P:3043–3046); falls back to √|det| for a zero font size.
pub fn chunk_scf(pt: &ParsedText, li: usize, ci: usize) -> f64 {
    let c = &pt.chars[pt.lines[li].chunks[ci].chars[0]];
    if c.utfs > 0.0 {
        c.tfs / c.utfs
    } else {
        crate::geom::scale_factor(pt.transform)
    }
}

/// Rotation of a transform in degrees, upstream's `atan2(c, d)` (P:2635–2638).
pub fn angle_deg(t: Affine) -> f64 {
    let [_, _, c, d, _, _] = t.as_coeffs();
    c.atan2(d).to_degrees()
}

/// Parse `el` and return its extent bbox in root coordinates (spec §A.4 `text_bbox(doc, el, …)`).
pub fn element_bbox(
    doc: &mut crate::dom::Doc,
    el: crate::dom::NodeId,
    ct: &mut crate::text::table::CharTable,
    warn: &mut crate::text::Warnings,
) -> Option<Rect> {
    let pt = ParsedText::parse(doc, el, ct, warn)?;
    text_bbox(&pt)
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --test text_parse --test text_layout 2>&1 | tail -20`
Expected: all pass (the pre-existing tests too — `line_specs` is unchanged).

Run: `cargo test 2>&1 | grep -E "^test result|FAILED|panicked"` — every suite green; `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.

- [ ] **Step 6: Commit**

```bash
git add src/text/parse.rs src/text/layout.rs tests/text_parse.rs tests/text_layout.rs
git commit -m "feat(text): chunk ids, parsed-position snapshot, continue-line resolution, get_ut_pts and chunk aggregates"
```

---

### Task 2: `edit.rs` — reindex, remove, delete a character, undo `textLength`

**Files:**
- Create: `src/text/edit.rs`
- Modify: `src/text/mod.rs` (`pub mod edit;`)
- Test: `tests/text_edit.rs` (new)

**Interfaces:**
- Consumes: Task 1's model; `layout::{chunk_geom, chunk_char_pts, unrendered_space, full_extent}`; `dom::Doc::parent`; `style::{Style, default_value}`; `num::fmt`.
- Produces (all `pub` in `text::edit`):
  - `fn sel(doc: &Doc, loc: &CharLoc) -> NodeId` — the style node of a character location (the node for text runs, its parent for tails: upstream `loc.sel`).
  - `fn style_eq(a: &Style, b: &Style) -> bool` — order-insensitive value equality.
  - `fn reindex(pt: &mut ParsedText) -> Vec<usize>` — rebuilds `chars`/`parsed_*`/indices from the line→chunk structure; returns `new index → old index`.
  - `fn remove_chars(pt: &mut ParsedText, idxs: &[usize]) -> Vec<usize>` — removes characters (pruning empty chunks and lines), then `reindex`.
  - `fn delete_char(pt: &mut ParsedText, idx: usize)` — P:4029–4118: removes one character and shifts its chunk so the rest stays put.
  - `fn remove_textlength(pt: &mut ParsedText)` — stage 2 (P:673–699).

- [ ] **Step 1: Write the failing tests**

Create `tests/text_edit.rs` with the preamble, then:

```rust
use sciink::text::edit::{delete_char, reindex, remove_chars, remove_textlength, sel, style_eq};
use sciink::text::layout::{chunk_char_pts, chunk_geom};
use sciink::style::Style;

fn positions(pt: &ParsedText) -> Vec<(char, f64, f64)> {
    let mut v = Vec::new();
    for (li, ln) in pt.lines.iter().enumerate() {
        for ci in 0..ln.chunks.len() {
            let p = chunk_char_pts(pt, li, ci);
            for (wi, &c) in ln.chunks[ci].chars.iter().enumerate() {
                v.push((pt.chars[c].c, p[wi][0].x, p[wi][0].y));
            }
        }
    }
    v
}
/// Same characters at the same places (floats compared with `close`, never `==`).
fn assert_pos(a: &[(char, f64, f64)], b: &[(char, f64, f64)]) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!(x.0 == y.0 && close(x.1, y.1) && close(x.2, y.2), "{x:?} vs {y:?}");
    }
}

#[test]
fn style_eq_ignores_order_and_sel_resolves_tails() {
    let a = Style::parse("fill:red;font-size:10px");
    let b = Style::parse("font-size:10px;fill:red");
    let c = Style::parse("font-size:10px;fill:blue");
    assert!(style_eq(&a, &b) && !style_eq(&a, &c) && !style_eq(&a, &Style::parse("fill:red")));
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0">a<tspan id="s">b</tspan>c</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (pt, _) = parsed(&mut d, "t");
    assert_eq!(sel(&d, &pt.chars[0].loc), id(&d, "t"));
    assert_eq!(sel(&d, &pt.chars[1].loc), id(&d, "s"));
    assert!(pt.chars[2].loc.tail);
    assert_eq!(sel(&d, &pt.chars[2].loc), id(&d, "t"), "a tail belongs to the parent");
}

#[test]
fn remove_chars_prunes_and_reindexes() {
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0 30 60" y="0">abc<tspan x="0" y="20">d</tspan></text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    sciink::text::layout::snapshot_parsed(&mut pt);
    let before = positions(&pt);
    let map = remove_chars(&mut pt, &[1, 3]); // 'b' (own chunk) and 'd' (own line)
    assert_eq!(map, [0, 2], "new → old");
    assert_eq!(pt.text(), "ac");
    assert_eq!(pt.lines.len(), 1);
    assert_eq!(pt.lines[0].chunks.len(), 2, "b's chunk is gone, a's and c's stay");
    assert_eq!(pt.lines[0].chunks[1].id, 2, "ids survive");
    assert_eq!(pt.lines[0].chars, [0, 1]);
    for (i, c) in pt.chars.iter().enumerate() {
        assert_eq!((c.line, c.windex), (0, 0));
        assert_eq!(c.chunk, i);
    }
    assert_eq!(pt.parsed_ut.len(), 2);
    let after = positions(&pt);
    assert_pos(&after, &[before[0], before[2]]);
    // reindex on an untouched model is the identity
    assert_eq!(reindex(&mut pt), [0, 1]);
}

#[test]
fn delete_char_shifts_the_chunk_by_the_anchor_rule() {
    // digits: DejaVu Sans has no kerning pairs between them, so the expectations below are exact
    // start anchor: deleting the LAST char changes nothing else; deleting the FIRST moves x right
    let svg = |anchor: &str| {
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:{anchor}" x="10" y="0">123</text></svg>"#)
    };
    let mut d = Doc::parse(svg("start").as_bytes()).unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    delete_char(&mut pt, 2);
    assert_eq!(pt.text(), "12");
    assert!(close(pt.lines[0].chunks[0].x, 10.0));
    assert_pos(&positions(&pt), &before[..2]);
    delete_char(&mut pt, 0);
    assert_eq!(pt.text(), "2");
    let (_, bx, _) = before[1];
    assert!(close(pt.lines[0].chunks[0].x, bx), "'2' stays where it was: {} vs {bx}", pt.lines[0].chunks[0].x);

    // end anchor: deleting the last char moves x left by its width; positions of the rest are kept
    let mut d = Doc::parse(svg("end").as_bytes()).unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    let cw = pt.chars[2].cwd;
    let kern = chunk_geom(&pt, 0, 0);
    let gap = kern.left[2] - kern.right[1]; // pair kerning 2→3 (0 for digits)
    delete_char(&mut pt, 2);
    assert!(close(pt.lines[0].chunks[0].x, 10.0 - cw - gap), "{}", pt.lines[0].chunks[0].x);
    assert_pos(&positions(&pt), &before[..2]);

    // middle anchor: deleting the last char moves x by half its width
    let mut d = Doc::parse(svg("middle").as_bytes()).unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    delete_char(&mut pt, 2);
    let after = positions(&pt);
    for (b, a) in before.iter().zip(&after) {
        assert!(close(b.1, a.1), "{b:?} vs {a:?}");
    }

    // an unrendered trailing space costs nothing when deleted (P:4047–4051)
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:end" x="10" y="0">12 </text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    delete_char(&mut pt, 2);
    assert!(close(pt.lines[0].chunks[0].x, 10.0));
    assert_pos(&positions(&pt), &before[..2]);

    // deleting the only char of a line removes the line
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0">a<tspan x="0" y="20">b</tspan></text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    delete_char(&mut pt, 1);
    assert_eq!(pt.lines.len(), 1);
    assert_eq!(pt.text(), "a");
}

#[test]
fn remove_textlength_restores_widths_and_records_the_transform() {
    use sciink::text::parse::TextLengthAdj;
    // spacingAndGlyphs: 4 chars stretched to 100 units
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0" textLength="100" lengthAdjust="spacingAndGlyphs">abcd</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let Some(TextLengthAdj::SpacingAndGlyphs(adj)) = pt.text_length else { panic!("parsed adj") };
    let stretched: Vec<f64> = pt.chars.iter().map(|c| c.cwd).collect();
    let with = sciink::text::layout::full_extent(&pt).unwrap();
    remove_textlength(&mut pt);
    assert_eq!(pt.text_length, None);
    assert!(pt.text_length_removed);
    for (c, s) in pt.chars.iter().zip(&stretched) {
        assert!(close(c.cwd * adj, *s), "cwd restored: {} × {adj} vs {s}", c.cwd);
    }
    let without = sciink::text::layout::full_extent(&pt).unwrap();
    // translate(cx_with,0) scale(adj,1) translate(-cx_without,0)
    let expect = kurbo::Affine::translate((with.center().x, 0.0))
        * kurbo::Affine::scale_non_uniform(adj, 1.0)
        * kurbo::Affine::translate((-without.center().x, 0.0));
    assert!(sciink::geom::affine_eq(pt.transform_extra, expect));
    assert!(sciink::geom::affine_eq(pt.transform, expect), "element had no transform of its own");
    // the stretched extent is reproduced by transform ∘ restored widths
    let mapped = sciink::geom::transform_rect(pt.transform_extra, without);
    assert!((mapped.width() - with.width()).abs() < 1e-6 && (mapped.center().x - with.center().x).abs() < 1e-6);

    // spacing: letter-spacing is written into every character's style, nothing else moves
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0" textLength="100">abcd</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let lsp = pt.chars[0].lsp;
    assert!(lsp > 0.0);
    let before = positions(&pt);
    remove_textlength(&mut pt);
    assert_eq!(positions(&pt), before);
    assert_eq!(pt.chars[0].sty.get("letter-spacing"), Some(sciink::num::fmt(lsp).as_str()));
    assert!(sciink::geom::is_identity(pt.transform_extra));

    // nothing to do without textLength
    let mut d = Doc::parse(format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0">ab</text></svg>"#).as_bytes()).unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    remove_textlength(&mut pt);
    assert!(!pt.text_length_removed);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test text_edit 2>&1 | tail -5`
Expected: compile error, `sciink::text::edit` not found.

- [ ] **Step 3: Create `src/text/edit.rs`**

```rust
//! Model editing primitives (spec §A.1 stages 2, 5, 10 and the pieces `Perform_Merges`/splits are
//! built from). Everything here edits the `ParsedText` model only; the DOM is written once, later,
//! by `text::write`. Upstream refs: parser.py (P:…) unless noted.

use std::collections::HashSet;
use std::rc::Rc;

use kurbo::Affine;

use crate::dom::{Doc, NodeId};
use crate::num;
use crate::style::Style;

use super::layout::{chunk_char_pts, chunk_geom, full_extent, unrendered_space};
use super::parse::{CharLoc, ParsedText, TChar, TextLengthAdj, XY_TOL};

/// The node whose style a character carries: the node itself for a text run, its parent for a tail
/// (upstream `CLoc.sel`).
pub fn sel(doc: &Doc, loc: &CharLoc) -> NodeId {
    if loc.tail {
        doc.parent(loc.node).unwrap_or(loc.node)
    } else {
        loc.node
    }
}

/// Order-insensitive value equality (upstream compares `Style` dicts).
pub fn style_eq(a: &Style, b: &Style) -> bool {
    a.0.len() == b.0.len() && a.0.iter().all(|(k, v)| b.get(k) == Some(v.as_str()))
}

/// Pair kerning between two adjacent characters, as `layout::chunk_geom` applies it: only within one
/// text node and only for pairs the char table measured.
fn dadv(prev: &TChar, cur: &TChar) -> f64 {
    if prev.loc.node == cur.loc.node && prev.loc.tail == cur.loc.tail {
        cur.prop.dadvs.get(&prev.c).copied().unwrap_or(0.0) * cur.utfs
    } else {
        0.0
    }
}

/// Rebuild `chars`, the snapshot vectors and every index from the line → chunk → character
/// structure (which is the source of truth after an edit). Returns `new index → old index`.
pub fn reindex(pt: &mut ParsedText) -> Vec<usize> {
    let ParsedText {
        chars,
        lines,
        parsed_ut,
        parsed_t,
        any_dx,
        any_dy,
        ..
    } = pt;
    let old = std::mem::take(chars);
    let old_ut = std::mem::take(parsed_ut);
    let old_t = std::mem::take(parsed_t);
    let snap = !old_ut.is_empty();
    let mut map = Vec::with_capacity(old.len());
    for (li, ln) in lines.iter_mut().enumerate() {
        ln.chars.clear();
        for (ci, ch) in ln.chunks.iter_mut().enumerate() {
            for (wi, oi) in ch.chars.iter_mut().enumerate() {
                let ni = chars.len();
                let mut c = old[*oi].clone();
                c.line = li;
                c.chunk = ci;
                c.windex = wi;
                chars.push(c);
                if snap {
                    parsed_ut.push(old_ut.get(*oi).copied().flatten());
                    parsed_t.push(old_t.get(*oi).copied().flatten());
                }
                map.push(*oi);
                *oi = ni;
                ln.chars.push(ni);
            }
        }
    }
    *any_dx = chars.iter().any(|c| c.dx.abs() > XY_TOL);
    *any_dy = chars.iter().any(|c| c.dy.abs() > XY_TOL);
    map
}

/// Remove characters (by current index) from their chunks; empty chunks and lines are pruned;
/// surviving chunks keep their `x`/`y`. Returns `reindex`'s `new → old` map.
pub fn remove_chars(pt: &mut ParsedText, idxs: &[usize]) -> Vec<usize> {
    let gone: HashSet<usize> = idxs.iter().copied().collect();
    for ln in pt.lines.iter_mut() {
        for ch in ln.chunks.iter_mut() {
            ch.chars.retain(|c| !gone.contains(c));
        }
        ln.chunks.retain(|ch| !ch.chars.is_empty());
    }
    pt.lines.retain(|l| !l.chunks.is_empty());
    reindex(pt)
}

/// P:4029–4118. Delete one character so the remaining ones stay where they are: the chunk's anchor
/// moves by the deleted character's effective width, weighted by the anchor fraction (a deleted
/// first character shifts a start-anchored chunk right by its width; a deleted last character
/// shifts an end-anchored chunk left). Empty chunks and lines are pruned.
pub fn delete_char(pt: &mut ParsedText, idx: usize) {
    let c = pt.chars[idx].clone();
    let (li, ci, wi) = (c.line, c.chunk, c.windex);
    let ids = pt.lines[li].chunks[ci].chars.clone();
    let anfr = pt.lines[li].spec.anchor.anfr();
    // Upstream's `dko2`/`dkn` look up (self, next) and (prev, next) in the deleted character's own
    // pair table, which only ever holds pairs ENDING at that character, so they are always 0 and
    // only the kerning from the left neighbour survives (P:4036–4044).
    let tdk = if wi > 0 { dadv(&pt.chars[ids[wi - 1]], &c) } else { 0.0 };
    let mut cwo = c.cwd + tdk + c.dx + if wi != 0 { c.lsp } else { 0.0 };
    let n = ids.len();
    if unrendered_space(pt, li, ci) && wi == n - 1 && n > 1 && pt.chars[ids[n - 2]].c != ' ' {
        cwo = tdk; // an unrendered trailing space costs nothing (its kerning weirdly still counts)
    }
    let deltax = if wi == 0 { (anfr - 1.0) * cwo } else { anfr * cwo };
    if deltax.abs() > XY_TOL {
        pt.lines[li].chunks[ci].x -= deltax;
    }
    remove_chars(pt, &[idx]);
}

/// Stage 2 (P:673–699), only when manual kerning is being removed. `spacingAndGlyphs`: restore the
/// natural widths and fold the stretch into the element transform
/// (`translate(cx_with,0)·scale(adj,1)·translate(−cx_without,0)`, written by the writer via
/// `transform_extra`). `spacing`: the adjusted letter-spacing simply becomes each character's
/// specified `letter-spacing`. Either way `textLength`/`lengthAdjust` are dropped by the writer.
pub fn remove_textlength(pt: &mut ParsedText) {
    let Some(tl) = pt.text_length else {
        return;
    };
    match tl {
        TextLengthAdj::SpacingAndGlyphs(adj) => {
            if adj != 0.0 && adj.is_finite() {
                let with = full_extent(pt);
                for c in pt.chars.iter_mut() {
                    c.cwd /= adj;
                }
                let without = full_extent(pt);
                if let (Some(w), Some(wo)) = (with, without) {
                    let tfm = Affine::translate((w.center().x, 0.0))
                        * Affine::scale_non_uniform(adj, 1.0)
                        * Affine::translate((-wo.center().x, 0.0));
                    pt.transform *= tfm;
                    pt.transform_extra *= tfm;
                }
            }
        }
        TextLengthAdj::Spacing(_) => {
            for c in pt.chars.iter_mut() {
                let lsp = num::fmt(c.lsp);
                if c.sty.get("letter-spacing") != Some(lsp.as_str()) {
                    let mut s = (*c.sty).clone();
                    s.set("letter-spacing", &lsp);
                    c.sty = Rc::new(s);
                }
            }
        }
    }
    pt.text_length = None;
    pt.text_length_removed = true;
}
```

`Style` must derive `Clone` and `Default` (check `src/style.rs:16`; add the derives if missing — `Debug, Clone, Default, PartialEq, Eq`). Add `pub mod edit;` to `src/text/mod.rs` (keep the module list sorted: `edit`, `fonts`, `layout`, …). Unused imports (`chunk_char_pts`, `chunk_geom`) will be needed by later tasks — remove them for this commit if clippy complains and re-add when used.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_edit 2>&1 | tail -15`
Expected: 4 passed. Then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/text/edit.rs src/text/mod.rs tests/text_edit.rs src/style.rs
git commit -m "feat(text): edit primitives — reindex, remove_chars, delete_char (P:4029–4118), remove_textlength (stage 2)"
```

---

### Task 3: Stage 4 next chain and stage 5 differential → absolute kerning

**Files:**
- Modify: `src/text/edit.rs` (append)
- Test: `tests/text_edit.rs` (append)

**Interfaces:**
- Consumes: Task 1–2 (`TChunk.{id,next,prev,prev_same_tspan}`, `new_chunk_id`, `reindex`, `sel`), `layout::{chunk_geom, chunk_char_pts, chunk_spw}`.
- Produces: `pub fn make_next_chain(doc: &Doc, pt: &mut ParsedText)` (P:701–725), `pub fn rechunk_absolute(pt: &mut ParsedText)` (P:1665–1687 + `write_axay` P:865–957), `pub fn unique_reps(vals: &[f64], tol: f64) -> Vec<f64>` (utils.py:130–153 returning the kept values).

Background for stage 5: after converting, upstream's `write_axay` starts a **new line** (not just a new chunk) wherever an absolute coordinate follows a character without one (P:875–893), then re-chunks each line where a coordinate is present (P:2704–2716). A new line whose `x` is missing continues from the end of the previous line (P:2640–2653) and one whose `y` is missing takes the previous line's last y. Spec §A.1 stage 5 says "re-chunk"; the port follows the code (lines), and Task 11 corrects the spec wording. **Deviation (static continue):** the continued coordinate is computed once here (upstream recomputes it on every access); nothing later moves a previous line without going through positions that are re-derived anyway. **Deviation (first segment):** the first character of every chunk is positioned with the same anchor-weighted formula as a dx'd character (`ax = left·(1−anfr) + right_lc·anfr`); upstream keeps the old chunk `x` (P:1679), which displaces a middle/end-anchored first segment (its chunk just got shorter) until stage 11 moves it back — stage-8 decisions in between would see wrong positions.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_edit.rs`:

```rust
use sciink::text::edit::{make_next_chain, rechunk_absolute, unique_reps};

#[test]
fn unique_reps_keeps_the_first_of_each_cluster() {
    assert_eq!(unique_reps(&[3.0, 1.0, 1.0005, 2.0, 3.0004], 0.001), [1.0, 2.0, 3.0]);
    assert!(unique_reps(&[], 0.1).is_empty());
}

#[test]
fn next_chain_links_chunks_on_one_baseline_in_x_order() {
    // three chunks on y=0 written out of x order plus one on another baseline
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="60 0 30" y="0">a<tspan id="s">b</tspan>c<tspan x="0" y="20">d</tspan></text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    make_next_chain(&d, &mut pt);
    let by_char = |c: char| -> (usize, usize) {
        let tc = pt.chars.iter().find(|t| t.c == c).unwrap();
        (tc.line, tc.chunk)
    };
    let (la, ca) = by_char('a');
    let (lb, cb) = by_char('b');
    let (lc, cc) = by_char('c');
    let (ld, cd) = by_char('d');
    let chunk = |(l, c): (usize, usize)| pt.chunk(l, c).clone();
    // x order is b (0), c (30), a (60)
    assert_eq!(chunk((lb, cb)).next, Some(chunk((lc, cc)).id));
    assert_eq!(chunk((lc, cc)).next, Some(chunk((la, ca)).id));
    assert_eq!(chunk((la, ca)).next, None);
    assert_eq!(chunk((lb, cb)).prev, None);
    assert_eq!(chunk((la, ca)).prev, Some(chunk((lc, cc)).id));
    assert_eq!(chunk((ld, cd)).next, None, "other baseline");
    assert_eq!(chunk((ld, cd)).prev, None);
    // b is in <tspan id="s">, c is the tspan's tail (style node = text) → different nodes; a and c share the text node
    assert!(!chunk((lc, cc)).prev_same_tspan);
    assert!(chunk((la, ca)).prev_same_tspan);
}

#[test]
fn next_chain_swaps_a_space_sitting_on_the_next_chunk() {
    // PDF-import bug (P:717–720): a " " chunk with the same x as the following chunk is ordered after it
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0 20 20" y="0">a b</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    make_next_chain(&d, &mut pt);
    let ids: Vec<u32> = pt.chunks().map(|(l, c)| pt.chunk(l, c).id).collect();
    // chunks: a (id 0), " " (id 1), b (id 2); sorted by centre " " comes before b, then swapped
    assert_eq!(pt.chunk(0, 0).next, Some(ids[2]), "a → b");
    assert_eq!(pt.chunk(0, 2).next, Some(ids[1]), "b → space");
    assert_eq!(pt.chunk(0, 1).next, None);
}

#[test]
fn rechunk_absolute_turns_dx_into_new_lines_without_moving_glyphs() {
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="50" y="0" dx="0 0 3 0 -2">abcde</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    assert!(pt.any_dx);
    let before = positions(&pt);
    rechunk_absolute(&mut pt);
    assert!(!pt.any_dx);
    assert!(pt.chars.iter().all(|c| c.dx == 0.0));
    // 'c' and 'e' carried dx → each opens a new line: "ab" | "cd" | "e"
    let texts: Vec<String> = (0..pt.lines.len()).map(|li| pt.line_text(li)).collect();
    assert_eq!(texts, ["ab", "cd", "e"]);
    assert!(pt.lines.iter().all(|l| l.chunks.len() == 1));
    assert!(!pt.lines[1].spec.sprl && !pt.lines[1].spec.continue_x);
    let after = positions(&pt);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!(close(b.1, a.1) && close(b.2, a.2), "{b:?} vs {a:?}");
    }
    // middle anchor: the new chunk's x is the anchor point of its glyph run (P:1676)
    let g = sciink::text::layout::chunk_geom(&pt, 1, 0);
    assert!(close(pt.lines[1].chunks[0].x, 0.5 * (g.left[0] + g.right[1])));

    // dy only → new line with continue_x resolved to the end of the previous line
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0" dx="0 1" dy="0 0 4">abc</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    rechunk_absolute(&mut pt);
    let texts: Vec<String> = (0..pt.lines.len()).map(|li| pt.line_text(li)).collect();
    assert_eq!(texts, ["a", "b", "c"]);
    assert!(pt.lines[2].spec.continue_x && !pt.lines[2].spec.continue_y);
    assert!(close(pt.lines[2].chunks[0].y, 4.0));
    let after = positions(&pt);
    for (b, a) in before.iter().zip(&after) {
        assert!(close(b.1, a.1) && close(b.2, a.2), "{b:?} vs {a:?}");
    }

    // no dx: untouched (dy alone does not trigger the conversion, P:1667)
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0" dy="0 4">ab</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    rechunk_absolute(&mut pt);
    assert_eq!(pt.lines.len(), 1);
    assert!(close(pt.chars[1].dy, 4.0));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test text_edit next_chain rechunk unique 2>&1 | tail -5` — compile errors (functions missing).

- [ ] **Step 3: Implement**

Append to `src/text/edit.rs`:

```rust
/// utils.py:130–153: sort, keep the first value, keep each further value that is more than `tol`
/// from the LAST KEPT one (not from its neighbour). Returns the kept representatives.
pub fn unique_reps(vals: &[f64], tol: f64) -> Vec<f64> {
    let mut v: Vec<f64> = vals.iter().copied().filter(|x| !x.is_nan()).collect();
    v.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    let mut out: Vec<f64> = Vec::new();
    for x in v {
        if out.last().is_none_or(|&l| x - l > tol) {
            out.push(x);
        }
    }
    out
}

/// Stage 4 (P:701–725): link the chunks that share a baseline within this element in ascending x
/// order (`next`/`prev`), remembering whether neighbours come from the same style node. A lone
/// `" "` chunk sitting exactly on the following chunk's left edge (a PDF-import artefact) is
/// ordered after that chunk. All links are reset first, so the chain can be rebuilt after stage 5.
pub fn make_next_chain(doc: &Doc, pt: &mut ParsedText) {
    for ln in pt.lines.iter_mut() {
        for ch in ln.chunks.iter_mut() {
            ch.next = None;
            ch.prev = None;
            ch.prev_same_tspan = false;
        }
    }
    const TOL: f64 = 0.001;
    let yvs: Vec<f64> = pt.lines.iter().map(|l| l.chunks[0].y).collect();
    for rep in unique_reps(&yvs, TOL) {
        // (line, chunk), centre x, left x, space width — for every chunk on this baseline
        let mut sws: Vec<((usize, usize), f64, f64, f64)> = Vec::new();
        for li in (0..pt.lines.len()).filter(|&i| (yvs[i] - rep).abs() < TOL) {
            for ci in 0..pt.lines[li].chunks.len() {
                let g = chunk_geom(pt, li, ci);
                sws.push((
                    (li, ci),
                    0.5 * (g.pts_ut[0].x + g.pts_ut[3].x),
                    g.pts_ut[0].x,
                    super::layout::chunk_spw(pt, li, ci),
                ));
            }
        }
        sws.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for i in 1..sws.len() {
            let prev_is_space = pt.chunk_text(sws[i - 1].0.0, sws[i - 1].0.1) == " ";
            if prev_is_space && (sws[i - 1].2 - sws[i].2).abs() < 0.01 * sws[i - 1].3 {
                sws.swap(i - 1, i);
            }
        }
        for i in 1..sws.len() {
            let (a, b) = (sws[i - 1].0, sws[i].0);
            let aid = pt.lines[a.0].chunks[a.1].id;
            let bid = pt.lines[b.0].chunks[b.1].id;
            let a_last = *pt.lines[a.0].chunks[a.1].chars.last().expect("non-empty chunk");
            let b_first = pt.lines[b.0].chunks[b.1].chars[0];
            let same = sel(doc, &pt.chars[a_last].loc) == sel(doc, &pt.chars[b_first].loc);
            pt.lines[a.0].chunks[a.1].next = Some(bid);
            let cb = &mut pt.lines[b.0].chunks[b.1];
            cb.prev = Some(aid);
            cb.prev_same_tspan = same;
        }
    }
}

/// Stage 5 (P:1665–1687 + `write_axay` P:865–957): every character with a `dx` becomes the first
/// character of a new chunk positioned where it already is (`ax = left·(1−anfr) + right_lc·anfr`,
/// `lc` = last char before the next dx'd one), every `dy` becomes an absolute `y`, and both are
/// zeroed. Upstream then re-parses: a coordinate that follows a character without one opens a new
/// LINE (P:886–893), and within a line any coordinate opens a new chunk (P:2704–2716). Lines that
/// got no x continue from the end of the previous line; no y → previous line's last y.
pub fn rechunk_absolute(pt: &mut ParsedText) {
    if pt.is_flow || !pt.chars.iter().any(|c| c.dx.abs() > XY_TOL) {
        return;
    }
    let mut new_lines: Vec<super::parse::TLine> = Vec::new();
    for li in 0..pt.lines.len() {
        let anfr = pt.lines[li].spec.anchor.anfr();
        // (char, ax, ay) in line order
        let mut axay: Vec<(usize, Option<f64>, Option<f64>)> = Vec::new();
        for ci in 0..pt.lines[li].chunks.len() {
            let pts = chunk_char_pts(pt, li, ci);
            let ch = pt.lines[li].chunks[ci].clone();
            for (j, &c) in ch.chars.iter().enumerate() {
                let (dx, dy) = (pt.chars[c].dx, pt.chars[c].dy);
                // Deviation: the first character of a chunk gets the same anchor-weighted formula
                // as a dx'd one. Upstream keeps `w.x` (P:1679), which mis-places a middle/end-
                // anchored first segment until stage 11 corrects it; for a start anchor with
                // dx[0] == 0 both agree.
                let ax = if dx.abs() > XY_TOL || j == 0 {
                    let lc = (j + 1..ch.chars.len())
                        .find(|&k| pt.chars[ch.chars[k]].dx != 0.0)
                        .map_or(ch.chars.len() - 1, |k| k - 1);
                    if dx.abs() > XY_TOL {
                        pt.chars[c].dx = 0.0;
                    }
                    Some(pts[j][0].x * (1.0 - anfr) + pts[lc][3].x * anfr)
                } else {
                    None
                };
                let ay = if dy.abs() > XY_TOL {
                    pt.chars[c].dy = 0.0;
                    Some(pts[j][0].y)
                } else if j == 0 {
                    Some(ch.y)
                } else {
                    None
                };
                axay.push((c, ax, ay));
            }
        }
        let mut starts = vec![0usize];
        for i in 1..axay.len() {
            let (_, px, py) = axay[i - 1];
            let (_, x, y) = axay[i];
            if (px.is_none() && x.is_some()) || (py.is_none() && y.is_some()) {
                starts.push(i);
            }
        }
        starts.push(axay.len());
        let old = pt.lines[li].clone();
        for (k, w) in starts.windows(2).enumerate() {
            let seg = &axay[w[0]..w[1]];
            let (xv, yv) = (seg[0].1, seg[0].2);
            let mut spec = old.spec.clone();
            if k > 0 {
                spec.sprl = false;
                spec.continue_x = xv.is_none();
                spec.continue_y = yv.is_none();
            }
            spec.x = vec![xv];
            spec.y = vec![yv];
            let style = if k == 0 {
                old.style.clone()
            } else {
                pt.chars[seg[0].0].sty.clone()
            };
            let mut line = super::parse::TLine {
                spec,
                style,
                chars: seg.iter().map(|s| s.0).collect(),
                chunks: Vec::new(),
            };
            // NaN marks "not known yet": resolved below for continue lines, carried forward otherwise
            let (mut cx, mut cy) = (xv.unwrap_or(f64::NAN), yv.unwrap_or(f64::NAN));
            for (i, (c, ax, ay)) in seg.iter().enumerate() {
                if i == 0 || ax.is_some() || ay.is_some() {
                    if let Some(x) = ax {
                        cx = *x;
                    }
                    if let Some(y) = ay {
                        cy = *y;
                    }
                    let id = pt.new_chunk_id();
                    line.chunks.push(super::parse::TChunk {
                        id,
                        x: cx,
                        y: cy,
                        chars: vec![*c],
                        next: None,
                        prev: None,
                        prev_same_tspan: false,
                    });
                } else {
                    line.chunks.last_mut().expect("opened at i == 0").chars.push(*c);
                }
            }
            new_lines.push(line);
        }
    }
    pt.lines = new_lines;
    reindex(pt);
    for li in 0..pt.lines.len() {
        if li > 0 {
            let (cx, cy) = (pt.lines[li].spec.continue_x, pt.lines[li].spec.continue_y);
            if cx || cy {
                let pli = li - 1;
                let pci = pt.lines[pli].chunks.len() - 1;
                let prev_y = pt.lines[pli].chunks[pci].y;
                let g = chunk_geom(pt, pli, pci);
                let anfr = pt.lines[li].spec.anchor.anfr();
                let ln = &mut pt.lines[li];
                if cx {
                    let x = (1.0 + anfr) * g.pts_ut[3].x - anfr * g.pts_ut[0].x;
                    ln.chunks[0].x = x;
                    ln.spec.x = vec![Some(x)];
                }
                if cy {
                    ln.chunks[0].y = prev_y;
                    ln.spec.y = vec![Some(prev_y)];
                }
            }
        }
        // chunks that opened on a y (or x) alone inherit the other coordinate from the chunk before
        let ln = &mut pt.lines[li];
        for ci in 1..ln.chunks.len() {
            if ln.chunks[ci].x.is_nan() {
                ln.chunks[ci].x = ln.chunks[ci - 1].x;
            }
            if ln.chunks[ci].y.is_nan() {
                ln.chunks[ci].y = ln.chunks[ci - 1].y;
            }
        }
    }
}
```

`Option::is_none_or` needs rustc ≥ 1.82 (the crate requires 1.85). `TLine`/`LineSpec` must derive `Clone` (`LineSpec` already does; add `#[derive(Debug, Clone)]` to `TLine` if missing).

- [ ] **Step 4: Run the tests, fmt, clippy, full suite**

Run: `cargo test --test text_edit 2>&1 | tail -15` → 8 passed. `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/text/edit.rs src/text/parse.rs tests/text_edit.rs
git commit -m "feat(text): stage 4 next chain and stage 5 differential→absolute re-chunking"
```

---

### Task 4: `append_chunks` — merge chunks into a target chunk (P:3120–3326)

**Files:**
- Modify: `src/text/edit.rs` (append)
- Test: `tests/text_edit.rs` (append)

**Interfaces:**
- Consumes: Tasks 1–3; `CharTable::{true_face, prop}`; `text::style::composed_font_size`; `Doc::{parent, specified_style}`.
- Produces:
  - `pub type ChunkRef = (usize, u32);` — `(index into the ParsedText arena, chunk id)`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum WType { Normal, Sub, Super }`
  - `pub struct Incoming { pub chunk: ChunkRef, pub wtype: WType, pub max_spaces: Option<usize> }`
  - `pub fn append_chunks(doc: &Doc, pts: &mut [ParsedText], ct: &mut CharTable, target: ChunkRef, incoming: &[Incoming])` — "equivalent to typing the incoming chunks after the target's last character": inserts `round((bl2x − br1x)/spw)` spaces (capped by `max_spaces`) before each incoming block, moves its characters into the target chunk (re-expressing their parsed points in the target's frame), nativizes sub/superscripts (`baseline-shift: super|sub; font-size: 65%`) or corrects `font-size: N%` when the transformed size differs, fixes the first-of-block `dx` and the anchor, and prunes the emptied source chunks/lines.

**Deviation (documented for Task 11):** upstream leaves a merged super/subscript character's model `bshft` untouched (stale) and relies on Inkscape's `baseline-shift:super` when the file is re-opened; this port sets `bshft = ±0.4/−0.2 · host utfs` and `utfs = 0.65 · host utfs` at merge time so the model's *current* positions are what Inkscape will render (stages 8–11 then reason about real positions). Upstream copies the target's last character (with its parsed points) for inserted spaces; here inserted spaces have `parsed_* = None` — every consumer skips `None`, and a copied point could never change a min/max anyway.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_edit.rs`:

```rust
use sciink::text::edit::{Incoming, WType, append_chunks};
use sciink::text::layout::snapshot_parsed;

/// Two <text> elements, the second placed `gap_spaces` space-widths after the first.
fn two_texts(gap_spaces: f64, second_style: &str) -> (Doc, Vec<ParsedText>, CharTable) {
    let probe = format!(r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text></svg>"#);
    let mut pd = Doc::parse(probe.as_bytes()).unwrap();
    let (ppt, _) = parsed(&mut pd, "a");
    let g = chunk_geom(&ppt, 0, 0);
    let x2 = g.right[4] + gap_spaces * ppt.chars[0].spw;
    let svg = format!(
        r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="b" xml:space="preserve" style="{DV};font-size:10px;{second_style}" x="{x2}" y="0">world</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<NodeId> = ["a", "b"].iter().map(|i| id(&d, i)).collect();
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &els, fonts(), &mut w);
    let mut pts: Vec<ParsedText> = els
        .iter()
        .map(|&e| ParsedText::parse(&mut d, e, &mut ct, &mut w).unwrap())
        .collect();
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
    }
    (d, pts, ct)
}

fn all_positions(pts: &[ParsedText]) -> Vec<(char, f64, f64)> {
    let mut v: Vec<(char, f64, f64)> = pts
        .iter()
        .flat_map(|pt| {
            positions(pt).into_iter().map(|(c, x, y)| {
                let p = pt.transform * Point::new(x, y);
                (c, p.x, p.y)
            })
        })
        .filter(|p| p.0 != ' ')
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

#[test]
fn append_chunks_inserts_the_right_number_of_spaces_and_keeps_glyphs_in_place() {
    let (d, mut pts, mut ct) = two_texts(1.0, "");
    let before = all_positions(&pts);
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming { chunk: (1, pts[1].chunk(0, 0).id), wtype: WType::Normal, max_spaces: None };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Hello world");
    assert!(pts[1].chars.is_empty() && pts[1].lines.is_empty(), "source emptied");
    assert_eq!(pts[0].lines[0].chunks.len(), 1);
    let after = all_positions(&pts);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!((b.1 - a.1).abs() < 1e-6 && (b.2 - a.2).abs() < 1e-6, "{b:?} vs {a:?}");
    }
    // the inserted space is a copy of 'o' with c=' ', dx = −lsp (0 here), no snapshot
    let sp = &pts[0].chars[5];
    assert_eq!(sp.c, ' ');
    assert!(close(sp.dx, 0.0) && close(sp.dy, 0.0));
    assert_eq!(pts[0].parsed_ut[5], None);
    assert!(pts[0].parsed_ut[6].is_some(), "moved chars keep their snapshot");
    // moved chars' parsed points were re-expressed in the target frame (identity here → unchanged)
    assert_eq!(pts[0].parsed_ut[6], pts[0].parsed_t[6]);
    // model indices are consistent
    for (i, c) in pts[0].chars.iter().enumerate() {
        assert_eq!((c.line, c.chunk, c.windex), (0, 0, i));
    }

    // max_spaces = Some(0) drops the gap: text has no space, 'w' now touches 'o'
    let (d, mut pts, mut ct) = two_texts(1.0, "");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming { chunk: (1, pts[1].chunk(0, 0).id), wtype: WType::Normal, max_spaces: Some(0) };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Helloworld");

    // a 2.4-space gap rounds to 2 spaces
    let (d, mut pts, mut ct) = two_texts(2.4, "");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming { chunk: (1, pts[1].chunk(0, 0).id), wtype: WType::Normal, max_spaces: None };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Hello  world");
}

#[test]
fn append_chunks_nativizes_superscripts_and_percent_sizes() {
    // superscript: smaller text merged as Super gets 65 % size and +40 % baseline of the host
    let (d, mut pts, mut ct) = two_texts(0.0, "font-size:6px");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming { chunk: (1, pts[1].chunk(0, 0).id), wtype: WType::Super, max_spaces: Some(0) };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    let w = &pts[0].chars[5];
    assert_eq!(w.c, 'w');
    assert!(close(w.utfs, 6.5) && close(w.bshft, 4.0));
    assert_eq!(w.sty.get("baseline-shift"), Some("super"));
    assert_eq!(w.sty.get("font-size"), Some("65%"));
    assert!(close(w.cwd, w.prop.charw * 6.5));
    assert!(!sciink::text::edit::style_eq(&w.sty, &pts[0].chars[4].sty));

    // a differently sized Normal merge is size-corrected to a whole percent of the host size
    let (d, mut pts, mut ct) = two_texts(1.0, "font-size:8px");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming { chunk: (1, pts[1].chunk(0, 0).id), wtype: WType::Normal, max_spaces: None };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    let w = pts[0].chars.iter().find(|c| c.c == 'w').unwrap();
    assert_eq!(w.sty.get("font-size"), Some("80%"));
    assert!(close(w.utfs, 8.0) && close(w.bshft, 0.0));

    // same style, same size → untouched style (no "100%" needed)
    let (d, mut pts, mut ct) = two_texts(1.0, "");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming { chunk: (1, pts[1].chunk(0, 0).id), wtype: WType::Normal, max_spaces: None };
    let sty_before = pts[1].chars[0].sty.clone();
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    let w = pts[0].chars.iter().find(|c| c.c == 'w').unwrap();
    assert!(sciink::text::edit::style_eq(&w.sty, &sty_before));

    // a merge that no longer exists (chunk id gone) is skipped silently
    let (d, mut pts, mut ct) = two_texts(1.0, "");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming { chunk: (1, 99), wtype: WType::Normal, max_spaces: None };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Hello");
}

#[test]
fn append_chunks_middle_anchor_moves_the_anchor_by_half_the_added_width() {
    let probe = format!(r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="50" y="0">Hello</text></svg>"#);
    let mut pd = Doc::parse(probe.as_bytes()).unwrap();
    let (ppt, _) = parsed(&mut pd, "a");
    let g = chunk_geom(&ppt, 0, 0);
    let x2 = g.right[4] + ppt.chars[0].spw; // start of "12345", one space later (digits: no pair
    // kerning, so Σ(cwd + dx) is the exact appended width and the anchor correction is exact)
    let svg = format!(
        r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="50" y="0">Hello</text><text id="b" xml:space="preserve" style="{DV};font-size:10px" x="{x2}" y="0">12345</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<NodeId> = ["a", "b"].iter().map(|i| id(&d, i)).collect();
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &els, fonts(), &mut w);
    let mut pts: Vec<ParsedText> = els.iter().map(|&e| ParsedText::parse(&mut d, e, &mut ct, &mut w).unwrap()).collect();
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
    }
    let before = all_positions(&pts);
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming { chunk: (1, pts[1].chunk(0, 0).id), wtype: WType::Normal, max_spaces: None };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    let after = all_positions(&pts);
    for (b, a) in before.iter().zip(&after) {
        assert!((b.1 - a.1).abs() < 1e-6, "{b:?} vs {a:?}");
    }
    assert!(pts[0].lines[0].chunks[0].x > 50.0, "anchor moved right by half the appended width");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test text_edit append 2>&1 | tail -5` — compile errors.

- [ ] **Step 3: Implement**

Append to `src/text/edit.rs`:

```rust
/// `(index into the ParsedText arena, chunk id)` — how merge plans name a chunk across elements.
pub type ChunkRef = (usize, u32);

/// How a merged chunk relates to the text it joins (`Perform_Merges`' `wtypes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WType {
    Normal,
    Sub,
    Super,
}

pub struct Incoming {
    pub chunk: ChunkRef,
    pub wtype: WType,
    /// Cap on the spaces inserted before this block (`None` = as many as the gap says).
    pub max_spaces: Option<usize>,
}

/// P:3120–3326 (`append_chks`): type the incoming chunks after the target chunk's last character.
pub fn append_chunks(
    doc: &Doc,
    pts: &mut [ParsedText],
    ct: &mut super::table::CharTable,
    target: ChunkRef,
    incoming: &[Incoming],
) {
    let (tp, tid) = target;
    let Some((tli, tci)) = pts[tp].find_chunk(tid) else {
        return;
    };
    let Some(inv) = crate::geom::inverse(pts[tp].transform) else {
        return;
    };
    let anfr = pts[tp].lines[tli].spec.anchor.anfr();

    // 1. Incoming characters, cloned, with their parsed points re-expressed in the target frame (P:3126–3128).
    struct Block {
        chars: Vec<(TChar, Option<[kurbo::Point; 4]>, Option<[kurbo::Point; 4]>)>,
        wtype: WType,
        max_spaces: Option<usize>,
        src: ChunkRef,
    }
    let mut blocks: Vec<Block> = Vec::new();
    for inc in incoming {
        let (sp, sid) = inc.chunk;
        let Some((li, ci)) = pts[sp].find_chunk(sid) else {
            continue; // already merged away (RK:623–624)
        };
        let src = &pts[sp];
        let chars = src.lines[li].chunks[ci]
            .chars
            .iter()
            .map(|&c| {
                let t = src.parsed_t.get(c).copied().flatten();
                (src.chars[c].clone(), t.map(|p| super::layout::transform_pts(inv, p)), t)
            })
            .collect();
        blocks.push(Block { chars, wtype: inc.wtype, max_spaces: inc.max_spaces, src: (sp, sid) });
    }
    if blocks.is_empty() {
        return;
    }

    // 2. Spaces before each block: round((bl2x − br1x) / spw of the target's last char), capped (P:3131–3164).
    let (lchr, first_idx) = {
        let t = &pts[tp];
        let ch = &t.lines[tli].chunks[tci];
        (t.chars[*ch.chars.last().expect("non-empty chunk")].clone(), ch.chars[0])
    };
    let mut br1x = {
        let t = &pts[tp];
        t.lines[tli].chunks[tci]
            .chars
            .iter()
            .filter_map(|&c| t.parsed_ut.get(c).copied().flatten())
            .map(|p| p[3].x)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let space_prop = ct.prop(ct.true_face(&lchr.spec), ' ');
    // (char, parsed_ut, parsed_t, wtype, first of its block)
    let mut new_chars: Vec<(TChar, Option<[kurbo::Point; 4]>, Option<[kurbo::Point; 4]>, WType, bool)> = Vec::new();
    for b in &blocks {
        let bl2x = b.chars.iter().filter_map(|(_, ut, _)| *ut).map(|p| p[0].x).fold(f64::INFINITY, f64::min);
        let br2x = b.chars.iter().filter_map(|(_, ut, _)| *ut).map(|p| p[3].x).fold(f64::NEG_INFINITY, f64::max);
        let mut numsp = if lchr.spw > 0.0 && bl2x.is_finite() && br1x.is_finite() {
            ((bl2x - br1x) / lchr.spw).round().max(0.0) as usize
        } else {
            0
        };
        if let Some(m) = b.max_spaces {
            numsp = numsp.min(m);
        }
        if br2x.is_finite() {
            br1x = br2x;
        }
        for i in 0..numsp {
            let mut sp = lchr.clone();
            sp.c = ' ';
            sp.prop = space_prop.clone();
            sp.cwd = space_prop.charw * sp.utfs;
            sp.dx = -lchr.lsp;
            sp.dy = 0.0;
            new_chars.push((sp, None, None, b.wtype, i == 0));
        }
        for (j, (c, ut, t)) in b.chars.iter().enumerate() {
            new_chars.push((c.clone(), *ut, *t, b.wtype, numsp == 0 && j == 0));
        }
    }

    // 3. Where the new characters live (P:3166–3176): the target's last node, or — when that node is
    //    not the chunk's first node — the tail of its ancestor just below the first node / the element.
    //    Only the host's font size and specified style matter to the model (and `loc` for kerning).
    let first_sel = sel(doc, &pts[tp].chars[first_idx].loc);
    let lchr_sel = sel(doc, &lchr.loc);
    let (host_loc, host_utfs, host_tfs, host_sty): (CharLoc, f64, f64, Rc<Style>) = if lchr_sel == first_sel {
        (
            CharLoc { node: lchr.loc.node, tail: lchr.loc.tail, idx: u32::MAX },
            lchr.utfs,
            lchr.tfs,
            lchr.sty.clone(),
        )
    } else {
        let el = pts[tp].el;
        let mut cel = lchr_sel;
        while let Some(p) = doc.parent(cel) {
            if p == first_sel || p == el {
                break;
            }
            cel = p;
        }
        let parent = doc.parent(cel).unwrap_or(el);
        let (u, t, s) = if parent == first_sel {
            let f = &pts[tp].chars[first_idx];
            (f.utfs, f.tfs, f.sty.clone())
        } else {
            let fs = super::style::composed_font_size(doc, el);
            (fs.utfs, fs.tfs, doc.specified_style(el))
        };
        (CharLoc { node: cel, tail: true, idx: u32::MAX }, u, t, s)
    };

    // 4. Remove the moved characters from their sources (P:3178–3205); a source may be the target element.
    for b in &blocks {
        let (sp, sid) = b.src;
        if let Some((li, ci)) = pts[sp].find_chunk(sid) {
            let ids = pts[sp].lines[li].chunks[ci].chars.clone();
            remove_chars(&mut pts[sp], &ids);
        }
    }
    let Some((tli, tci)) = pts[tp].find_chunk(tid) else {
        return;
    };

    // 5. Append; restyle moved characters (P:3266–3293); fix dx of block-firsts and the anchor (P:3297–3303).
    let pt = &mut pts[tp];
    let scf = if host_utfs > 0.0 { host_tfs / host_utfs } else { 1.0 };
    let mut sum_wd = 0.0;
    let mut prev_lsp = lchr.lsp;
    for (mut c, ut, t, wtype, first) in new_chars {
        c.loc = host_loc;
        let otype = c.sty.get("baseline-shift").map(str::to_string);
        let ntype = match (otype.as_deref(), wtype) {
            (Some("super"), WType::Normal) => WType::Super,
            (Some("sub"), WType::Normal) => WType::Sub,
            (_, w) => w,
        };
        let sizechanged = (c.tfs - host_tfs).abs() > 1e-4;
        if !style_eq(&c.sty, &host_sty) || matches!(ntype, WType::Super | WType::Sub) || sizechanged {
            let mut s = (*c.sty).clone();
            match ntype {
                WType::Super | WType::Sub => {
                    // Inkscape's native super/subscript convention (P:3277–3282)
                    s.set("baseline-shift", if ntype == WType::Super { "super" } else { "sub" });
                    s.set("font-size", "65%");
                    c.bshft = if ntype == WType::Super { 0.4 } else { -0.2 } * host_utfs;
                    c.utfs = 0.65 * host_utfs;
                }
                WType::Normal if sizechanged => {
                    let pct = (c.tfs / host_tfs * 100.0).round();
                    s.set("font-size", &format!("{}%", num::fmt(pct)));
                    c.utfs = host_utfs * pct / 100.0;
                }
                WType::Normal => {
                    s.set("font-size", "100%");
                    c.utfs = host_utfs;
                }
            }
            c.tfs = c.utfs * scf;
            c.cwd = c.prop.charw * c.utfs;
            c.caph = c.prop.caph * c.utfs;
            c.spw = c.prop.spacew * c.utfs;
            c.sty = Rc::new(s);
        }
        if first {
            c.dx = -prev_lsp;
        }
        prev_lsp = c.lsp;
        sum_wd += c.cwd + c.dx;
        let idx = pt.chars.len();
        pt.chars.push(c);
        if !pt.parsed_ut.is_empty() {
            pt.parsed_ut.push(ut);
            pt.parsed_t.push(t);
        }
        pt.lines[tli].chunks[tci].chars.push(idx);
    }
    if anfr != 0.0 {
        pt.lines[tli].chunks[tci].x += anfr * sum_wd;
    }
    reindex(pt);
}
```

`pt.parsed_ut.is_empty()` is only true when no snapshot was taken (unit tests); the pipeline always snapshots first.

- [ ] **Step 4: Run the tests, fmt, clippy, full suite**

Run: `cargo test --test text_edit 2>&1 | tail -15` → 11 passed; `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/text/edit.rs tests/text_edit.rs
git commit -m "feat(text): append_chunks — merge chunks into a target with spaces, sub/superscript nativization and anchor fix (P:3120–3326)"
```

---

### Task 5: `split_off` — characters leave their element as new `ParsedText`s (P:1258–1440)

**Files:**
- Modify: `src/text/edit.rs` (append)
- Test: `tests/text_edit.rs` (append)

**Interfaces:**
- Consumes: Tasks 1–4 (`Origin::SplitFrom`, `remove_chars` → `new → old` map, `chunk_char_pts`).
- Produces: `pub fn split_off(pts: &mut Vec<ParsedText>, src: usize, chr_lists: &[Vec<usize>]) -> Vec<usize>` — returns the arena indices of the new `ParsedText`s in creation order. Callers pass `chr_lists` in **reverse document order** (as upstream does, RK:197–199, 244–250, 306–312, 371–372); the writer inserts each new element right after its source's replacement, so the final document order is natural.

Background: upstream writes the new `<text>` immediately (`chrs_to_textel`, P:1189–1256) and fixes positions on both the remaining and the new chunks (P:1404–1431). Here the new element is only a model (`Origin::SplitFrom`, `el` = the source element whose attributes it will copy); the geometry fix is the same: for each chunk, `err[i] = old_left[i] − new_left[i]`, `Δ[i] = err[i] − err[i−1]` goes into `dx[i]` (i > 0), and the chunk anchor moves by `round((err[0] − (−anfr·ΣΔ[1:]))/XY_TOL)·XY_TOL`; same for `y` with baselines. **Deviation:** upstream skips the anchor shift for a chunk that has no `x` entry of its own in the line list (P:1427); here every chunk owns an `x`, so the shift is always applied (Plan 3 already carries coordinates forward per chunk).

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_edit.rs`:

```rust
use sciink::text::edit::split_off;
use sciink::text::parse::Origin;

#[test]
fn split_off_makes_positioned_elements_and_leaves_no_glyph_behind() {
    // one chunk "ab cd": split "cd" off → new element at c's left edge, everything stays put
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><g transform="translate(3,4)"><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="10" y="20" class="k">ab cd</text></g></svg>"#).as_bytes(),
    )
    .unwrap();
    let (pt, _) = parsed(&mut d, "t");
    let mut pts = vec![pt];
    snapshot_parsed(&mut pts[0]);
    let before = all_positions(&pts);
    let c_left = chunk_char_pts(&pts[0], 0, 0)[3][0].x;
    let news = split_off(&mut pts, 0, &[vec![3, 4]]);
    assert_eq!(news, [1]);
    assert_eq!(pts[0].text(), "ab ");
    assert_eq!(pts[1].text(), "cd");
    assert_eq!(pts[1].origin, Origin::SplitFrom);
    assert_eq!(pts[1].el, pts[0].el, "remembers the element it came from");
    assert_eq!(pts[1].transform, pts[0].transform);
    assert_eq!(pts[1].lines.len(), 1);
    assert_eq!(pts[1].lines[0].chunks.len(), 1);
    assert!(close(pts[1].lines[0].chunks[0].x, c_left) && close(pts[1].lines[0].chunks[0].y, 20.0));
    assert_eq!(pts[1].lines[0].spec.anchor, pts[0].lines[0].spec.anchor);
    assert_eq!(pts[1].parsed_ut.len(), 2, "snapshots travel with the characters");
    let after = all_positions(&pts);
    for (b, a) in before.iter().zip(&after) {
        assert!((b.1 - a.1).abs() < 1e-6 && (b.2 - a.2).abs() < 1e-6, "{b:?} vs {a:?}");
    }

    // non-contiguous characters become separate runs; the survivors are re-spaced with dx
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:end" x="80" y="0">abcde</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (pt, _) = parsed(&mut d, "t");
    let mut pts = vec![pt];
    let before = all_positions(&pts);
    let news = split_off(&mut pts, 0, &[vec![1, 3]]); // 'b' and 'd'
    assert_eq!(news, [1, 2]);
    assert_eq!(pts[0].text(), "ace");
    assert_eq!((pts[1].text().as_str(), pts[2].text().as_str()), ("b", "d"));
    assert!(pts[0].chars[1].dx.abs() > 1e-6, "c keeps its place through dx");
    assert!(pts[0].any_dx);
    let after = all_positions(&pts);
    for (b, a) in before.iter().zip(&after) {
        assert!((b.1 - a.1).abs() < 1e-6 && (b.2 - a.2).abs() < 1e-6, "{b:?} vs {a:?}");
    }

    // several lists keep their order; a whole chunk leaving a two-chunk line leaves one chunk
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="0 50" y="0 0">ab<tspan x="0" y="30">cd</tspan></text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (pt, _) = parsed(&mut d, "t");
    let mut pts = vec![pt];
    let before = all_positions(&pts);
    let line2: Vec<usize> = pts[0].lines[1].chars.clone();
    let chunk_b: Vec<usize> = pts[0].lines[0].chunks[1].chars.clone();
    let news = split_off(&mut pts, 0, &[line2, chunk_b]);
    assert_eq!(news, [1, 2]);
    assert_eq!(pts[1].text(), "cd");
    assert_eq!(pts[2].text(), "b");
    assert_eq!(pts[0].lines.len(), 1);
    assert_eq!(pts[0].lines[0].chunks.len(), 1);
    assert_eq!(pts[0].text(), "a");
    let after = all_positions(&pts);
    for (b, a) in before.iter().zip(&after) {
        assert!((b.1 - a.1).abs() < 1e-6 && (b.2 - a.2).abs() < 1e-6, "{b:?} vs {a:?}");
    }
    // middle anchor: the new element's x is the centre of its run
    let g = chunk_geom(&pts[1], 0, 0);
    assert!(close(pts[1].lines[0].chunks[0].x, 0.5 * (g.left[0] + g.right[1])));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test text_edit split_off 2>&1 | tail -5`: `split_off` not found.

- [ ] **Step 3: Implement**

Append to `src/text/edit.rs` (add `use std::collections::HashMap;` and `use super::parse::{Origin, TChunk, TLine};` to the imports):

```rust
/// P:1258–1440 (`split_off_characters`) minus the XML half: the requested characters leave this
/// element and become new `ParsedText`s (`Origin::SplitFrom`, one line, one chunk each), one per
/// maximal run of characters that were contiguous within one chunk. Each new element sits at
/// `x = anfr·max_right + (1−anfr)·min_left` of its run and `y` = its first character's baseline,
/// copies the source line's anchor/direction/transform, and both the remaining and the new chunks
/// get `dx`/`dy`/anchor corrections so every glyph stays exactly where it was (P:1404–1431).
/// Returns the arena indices of the new `ParsedText`s, in creation order.
pub fn split_off(pts: &mut Vec<ParsedText>, src: usize, chr_lists: &[Vec<usize>]) -> Vec<usize> {
    // current (left, right, base) of every character, by index (P:1267)
    let mut before: HashMap<usize, (f64, f64, f64)> = HashMap::new();
    for li in 0..pts[src].lines.len() {
        for ci in 0..pts[src].lines[li].chunks.len() {
            let p = chunk_char_pts(&pts[src], li, ci);
            for (wi, &c) in pts[src].lines[li].chunks[ci].chars.iter().enumerate() {
                before.insert(c, (p[wi][0].x, p[wi][3].x, p[wi][0].y));
            }
        }
    }
    // runs (P:1158–1187): bucket by chunk in first-seen order, sort by windex, cut where windex jumps
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for list in chr_lists {
        let mut order: Vec<(usize, usize)> = Vec::new();
        let mut by: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for &c in list {
            let k = (pts[src].chars[c].line, pts[src].chars[c].chunk);
            if !by.contains_key(&k) {
                order.push(k);
            }
            by.entry(k).or_default().push(c);
        }
        for k in order {
            let mut cs = by.remove(&k).unwrap_or_default();
            cs.sort_by_key(|&c| pts[src].chars[c].windex);
            let mut run: Vec<usize> = Vec::new();
            for &c in &cs {
                if let Some(&p) = run.last() {
                    if pts[src].chars[c].windex != pts[src].chars[p].windex + 1 {
                        runs.push(std::mem::take(&mut run));
                    }
                }
                run.push(c);
            }
            if !run.is_empty() {
                runs.push(run);
            }
        }
    }
    // one new ParsedText per run (P:1189–1256, P:1380–1402)
    let mut news: Vec<(usize, Vec<usize>)> = Vec::new();
    for run in &runs {
        let s = &pts[src];
        let f = &s.chars[run[0]];
        let ln = &s.lines[f.line];
        let anfr = ln.spec.anchor.anfr();
        let minx = run.iter().map(|c| before[c].0).fold(f64::INFINITY, f64::min);
        let maxx = run.iter().map(|c| before[c].1).fold(f64::NEG_INFINITY, f64::max);
        let xv = anfr * maxx + (1.0 - anfr) * minx;
        let yv = before[&run[0]].2;
        let mut spec = ln.spec.clone();
        spec.x = vec![Some(xv)];
        spec.y = vec![Some(yv)];
        spec.sprl = false;
        spec.continue_x = false;
        spec.continue_y = false;
        spec.style_node = f.loc.node;
        spec.first_run = 0;
        let snap = !s.parsed_ut.is_empty();
        let mut np = ParsedText {
            el: s.el,
            transform: s.transform,
            chars: Vec::with_capacity(run.len()),
            lines: Vec::new(),
            is_flow: false,
            is_inkscape: s.is_inkscape,
            is_ml_inkscape: s.is_ml_inkscape,
            text_length: None,
            any_dx: false,
            any_dy: false,
            origin: Origin::SplitFrom,
            parsed_ut: Vec::new(),
            parsed_t: Vec::new(),
            transform_extra: s.transform_extra,
            text_anchor_override: None,
            text_length_removed: s.text_length_removed,
            next_chunk_id: 0,
        };
        let mut idx = Vec::with_capacity(run.len());
        for (i, &c) in run.iter().enumerate() {
            let mut tc = s.chars[c].clone();
            tc.line = 0;
            tc.chunk = 0;
            tc.windex = i;
            np.chars.push(tc);
            if snap {
                np.parsed_ut.push(s.parsed_ut.get(c).copied().flatten());
                np.parsed_t.push(s.parsed_t.get(c).copied().flatten());
            }
            idx.push(i);
        }
        let id = np.new_chunk_id();
        np.lines.push(TLine {
            spec,
            style: f.sty.clone(),
            chars: idx.clone(),
            chunks: vec![TChunk {
                id,
                x: xv,
                y: yv,
                chars: idx,
                next: None,
                prev: None,
                prev_same_tspan: false,
            }],
        });
        np.any_dx = np.chars.iter().any(|c| c.dx.abs() > XY_TOL);
        np.any_dy = np.chars.iter().any(|c| c.dy.abs() > XY_TOL);
        pts.push(np);
        news.push((pts.len() - 1, run.clone()));
    }
    // remove from the source, then absorb every position error (P:1404–1431)
    let all: Vec<usize> = chr_lists.iter().flatten().copied().collect();
    let map = remove_chars(&mut pts[src], &all);
    fix_positions(&mut pts[src], |i| map.get(i).and_then(|o| before.get(o)).copied());
    for (npi, olds) in &news {
        fix_positions(&mut pts[*npi], |i| olds.get(i).and_then(|o| before.get(o)).copied());
    }
    news.into_iter().map(|(i, _)| i).collect()
}

/// P:1404–1431: `old(i)` gives a character's previous `(left, right, base)`; the difference to its
/// current position goes into `dx`/`dy` (as differences between consecutive errors) and the
/// chunk anchor (the first error, anchor-weighted), rounded to `XY_TOL`.
fn fix_positions(pt: &mut ParsedText, old: impl Fn(usize) -> Option<(f64, f64, f64)>) {
    for li in 0..pt.lines.len() {
        let anfr = pt.lines[li].spec.anchor.anfr();
        for ci in 0..pt.lines[li].chunks.len() {
            let now = chunk_char_pts(pt, li, ci);
            let ids = pt.lines[li].chunks[ci].chars.clone();
            let err: Vec<(f64, f64)> = ids
                .iter()
                .zip(&now)
                .map(|(&c, p)| match old(c) {
                    Some((l, _, b)) => (l - p[0].x, b - p[0].y),
                    None => (0.0, 0.0),
                })
                .collect();
            let Some(&needed) = err.first() else {
                continue;
            };
            let mut dxs = vec![0.0; err.len()];
            let mut dys = vec![0.0; err.len()];
            for i in 1..err.len() {
                dxs[i] = err[i].0 - err[i - 1].0;
                dys[i] = err[i].1 - err[i - 1].1;
            }
            for (i, &c) in ids.iter().enumerate() {
                if dxs[i].abs() > XY_TOL {
                    pt.chars[c].dx += dxs[i];
                }
                if dys[i].abs() > XY_TOL {
                    pt.chars[c].dy += dys[i];
                }
            }
            let fc_dx = -anfr * dxs[1..].iter().sum::<f64>();
            let shift_x = ((needed.0 - fc_dx) / XY_TOL).round() * XY_TOL;
            let shift_y = if needed.1.is_nan() {
                0.0
            } else {
                (needed.1 / XY_TOL).round() * XY_TOL
            };
            let ch = &mut pt.lines[li].chunks[ci];
            if shift_x != 0.0 {
                ch.x += shift_x;
            }
            if shift_y != 0.0 {
                ch.y += shift_y;
            }
        }
    }
    pt.any_dx = pt.chars.iter().any(|c| c.dx.abs() > XY_TOL);
    pt.any_dy = pt.chars.iter().any(|c| c.dy.abs() > XY_TOL);
}
```

- [ ] **Step 4: Run the tests, fmt, clippy, full suite** — `cargo test --test text_edit 2>&1 | tail -15` → 12 passed.

- [ ] **Step 5: Commit**

```bash
git add src/text/edit.rs tests/text_edit.rs
git commit -m "feat(text): split_off — characters leave an element as positioned SplitFrom models with exact position fix-up (P:1258–1440)"
```

---

### Task 6: `kerning.rs` — helpers, `perform_merges`, stage 6 `remove_manual_kerning`

**Files:**
- Create: `src/text/kerning.rs`
- Create: `src/text/write.rs` (only the `ClipUnion` record for now; the writer comes in Task 10)
- Modify: `src/text/mod.rs` (`pub mod kerning; pub mod write;`)
- Test: `tests/text_kerning.rs` (new)

**Interfaces:**
- Consumes: Tasks 1–5; `layout::{get_ut_pts, chunk_spw, chunk_mch}`.
- Produces:
  - `text::write::ClipUnion { pub target: NodeId, pub others: Vec<NodeId> }` — "these elements' clip-paths must be unioned onto `target`" (executed by Task 10's `apply_clip_unions`).
  - `text::kerning` constants (Global Constraints), `pub fn isnumeric(s: &str, countminus: bool) -> bool`, `pub fn wstrip(s: &str) -> String`, `pub fn twospaces(a: &str, b: &str) -> bool`, `pub fn trailing_leading(a: &str, b: &str) -> (usize, usize)`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum MergeType { Same, Sub, Super, SubReturn, SuperReturn }`
  - `pub struct Cand { pub to: ChunkRef, pub mtype: MergeType, pub br1x: f64, pub bl2x: f64 }`
  - `pub fn merge_wtypes(chain: &[(ChunkRef, MergeType)]) -> Option<Vec<WType>>` (RK:566–611 state machine; `None` = chain dropped)
  - `pub fn perform_merges(doc: &Doc, pts: &mut Vec<ParsedText>, ct: &mut CharTable, cands: &[(ChunkRef, Vec<Cand>)], mk: bool, clips: &mut Vec<ClipUnion>)` (RK:535–656)
  - `pub fn remove_manual_kerning(doc: &Doc, pts: &mut Vec<ParsedText>, ct: &mut CharTable, clips: &mut Vec<ClipUnion>)` (RK:318–376)

Notes: `suborsuperreturn`/`superorsubreturn` only arise when `SUBSUPER_THR == 1` (RK:478, 497); with 0.99 they are dead code and are not ported. Python's `round` is half-to-even and Rust's `f64::round` half-away-from-zero; the space counts here are ratios of measured widths and never land on an exact .5.

- [ ] **Step 1: Write the failing tests**

Create `tests/text_kerning.rs` with the preamble, then:

```rust
use sciink::text::edit::{WType, make_next_chain};
use sciink::text::kerning::{MergeType, isnumeric, merge_wtypes, remove_manual_kerning, trailing_leading, twospaces, wstrip};
use sciink::text::layout::{chunk_char_pts, chunk_geom, snapshot_parsed, transform_pts};
use sciink::text::write::ClipUnion;

fn all_positions(pts: &[ParsedText]) -> Vec<(char, f64, f64)> {
    let mut v = Vec::new();
    for pt in pts {
        for (li, ln) in pt.lines.iter().enumerate() {
            for ci in 0..ln.chunks.len() {
                let p = chunk_char_pts(pt, li, ci);
                for (wi, &c) in ln.chunks[ci].chars.iter().enumerate() {
                    if pt.chars[c].c != ' ' {
                        let q = transform_pts(pt.transform, p[wi])[0];
                        v.push((pt.chars[c].c, q.x, q.y));
                    }
                }
            }
        }
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}
fn assert_same(before: &[(char, f64, f64)], after: &[(char, f64, f64)], tol: f64) {
    assert_eq!(before.len(), after.len(), "\n{before:?}\n{after:?}");
    for (b, a) in before.iter().zip(after) {
        assert!(b.0 == a.0 && (b.1 - a.1).abs() < tol && (b.2 - a.2).abs() < tol, "{b:?} vs {a:?}");
    }
}
/// Parse every <text> of `svg` into an arena (snapshot taken, next chain built).
fn arena(svg: &str) -> (Doc, Vec<ParsedText>, CharTable) {
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<NodeId> = d.descendants(d.svg()).filter(|&n| d.is_element(n) && d.tag(n) == "text").collect();
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &els, fonts(), &mut w);
    let mut pts: Vec<ParsedText> = els.iter().filter_map(|&e| ParsedText::parse(&mut d, e, &mut ct, &mut w)).collect();
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
        make_next_chain(&d, pt);
    }
    (d, pts, ct)
}
/// Left edges of the characters of `text` laid out as one chunk at x=0 (DejaVu Sans 10px).
fn lefts(text: &str) -> Vec<f64> {
    let mut d = Doc::parse(format!(r#"<svg {NS}><text id="p" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">{text}</text></svg>"#).as_bytes()).unwrap();
    let (pt, _) = parsed(&mut d, "p");
    chunk_geom(&pt, 0, 0).left
}
fn fmt_list(v: &[f64]) -> String {
    v.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(" ")
}

#[test]
fn text_helpers_follow_upstream() {
    assert!(isnumeric(" 1,000.5 ", false) && isnumeric("−3e2", false) && isnumeric("-", true));
    assert!(!isnumeric("-", false) && !isnumeric("1a", false) && !isnumeric("", false));
    assert_eq!(wstrip(" a\tb\n"), "ab");
    assert!(twospaces("a  ", "b") && twospaces("a ", " b") && twospaces("a", "  b") && !twospaces("a ", "b"));
    assert_eq!(trailing_leading("ab  ", " cd"), (2, 1));
}

#[test]
fn merge_type_state_machine_matches_rk566_611() {
    let r = |ts: &[MergeType]| merge_wtypes(&ts.iter().map(|&t| ((0usize, 0u32), t)).collect::<Vec<_>>());
    use MergeType::*;
    assert_eq!(r(&[Same, Same]), Some(vec![WType::Normal; 3]));
    assert_eq!(r(&[Super, Same, SuperReturn, Same]), Some(vec![WType::Normal, WType::Super, WType::Super, WType::Normal, WType::Normal]));
    assert_eq!(r(&[Sub, SubReturn]), Some(vec![WType::Normal, WType::Sub, WType::Normal]));
    assert_eq!(r(&[SuperReturn]), None, "return without a super");
    assert_eq!(r(&[Super, Sub]), None, "sub inside a super");
    assert_eq!(r(&[Sub, SuperReturn]), None);
    assert_eq!(r(&[]), Some(vec![WType::Normal]));
}

#[test]
fn manual_kerning_removal_rejoins_pdf_style_x_arrays() {
    // PDF import: every glyph positioned by its own x, laid out with our own metrics
    let xs = lefts("Hello");
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">Hello</text></svg>"#,
        fmt_list(&xs)
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 5);
    let before = all_positions(&pts);
    let mut clips: Vec<ClipUnion> = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts.len(), 1, "everything merged, nothing to split off");
    assert_eq!(pts[0].lines[0].chunks.len(), 1);
    assert_eq!(pts[0].text(), "Hello");
    assert_same(&before, &all_positions(&pts), 1e-6);
    assert!(clips.is_empty(), "one element: no clip union");

    // two words one space apart merge with a real space; a far chunk is split into its own element.
    // Per-character x list: "Hello" glyph-by-glyph, then "world" one space later, then "far" six later.
    let hello = lefts("Hello");
    let spw = {
        let mut d = Doc::parse(format!(r#"<svg {NS}><text id="p" style="{DV};font-size:10px" x="0" y="0">a</text></svg>"#).as_bytes()).unwrap();
        parsed(&mut d, "p").0.chars[0].spw
    };
    let hello_right = lefts("Hello ")[5];
    let world = lefts("world");
    let far = lefts("far");
    let mut xs: Vec<f64> = hello.clone();
    xs.extend(world.iter().map(|x| hello_right + spw + x));
    let world_right = hello_right + spw + lefts("world ")[5];
    xs.extend(far.iter().map(|x| world_right + 6.0 * spw + x));
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">Helloworldfar</text></svg>"#,
        fmt_list(&xs)
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 13);
    let before = all_positions(&pts);
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts.len(), 2, "'far' is too far: its own element");
    assert_eq!(pts[0].text(), "Hello world");
    assert_eq!(pts[1].text(), "far");
    assert_eq!(pts[1].origin, sciink::text::parse::Origin::SplitFrom);
    assert_same(&before, &all_positions(&pts), 1e-6);

    // numbers: "−" right before "0.5" merges with dx = 0 (RK:341–342)
    let minus = lefts("−");
    let minus_right = lefts("−0")[1];
    let mut xs = minus.clone();
    xs.extend(lefts("0.5").iter().map(|x| minus_right + x));
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">−0.5</text></svg>"#,
        fmt_list(&xs)
    ));
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts.len(), 1);
    assert_eq!(pts[0].text(), "−0.5");
}

#[test]
fn manual_kerning_removal_records_clip_unions_only_across_elements() {
    // intra-element merging never touches clips; the record is exercised by external merges (Task 7)
    let xs = lefts("ab");
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="9" height="9"/></clipPath></defs><text id="t" clip-path="url(#c)" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">ab</text></svg>"#,
        fmt_list(&xs)
    ));
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts[0].text(), "ab");
    assert!(clips.is_empty());
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test text_kerning 2>&1 | tail -5`: module `kerning`/`write` not found.

- [ ] **Step 3: Create `src/text/write.rs` (record only) and `src/text/kerning.rs`**

`src/text/write.rs`:

```rust
//! Stage 12 — the only place the text pipeline writes the DOM (spec §A.0 decision 1). The writer
//! itself lands in a later task; this file starts with the record the merge stages produce.

use crate::dom::NodeId;

/// Elements whose `clip-path`s must be unioned onto `target` because their text was merged into it
/// (RK:640–656). Executed by `apply_clip_unions` before the elements are rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipUnion {
    pub target: NodeId,
    pub others: Vec<NodeId>,
}
```

`src/text/kerning.rs`:

```rust
//! Port of upstream `remove_kerning.py` (RK): the stage drivers that run over a `Vec<ParsedText>`
//! arena. Geometry comes from `text::layout`, model edits from `text::edit`; nothing here touches
//! the DOM except by returning `ClipUnion` records for `text::write`.

use std::collections::{HashMap, HashSet};

use crate::dom::Doc;

use super::edit::{ChunkRef, Incoming, WType, append_chunks, split_off};
use super::layout::{chunk_mch, chunk_spw, get_ut_pts};
use super::parse::ParsedText;
use super::table::CharTable;
use super::write::ClipUnion;

pub const NUM_SPACES: f64 = 1.0;
pub const XTOLEXT: f64 = 0.6;
pub const YTOLEXT: f64 = 0.1;
pub const XTOLMKN: f64 = 1.5;
pub const XTOLMKP: f64 = 0.99;
pub const YTOLMK: f64 = 0.01;
pub const XTOLSPLIT: f64 = 0.5;
pub const SUBSUPER_THR: f64 = 0.99;
pub const SUBSUPER_YTHR: f64 = 1.0 / 3.0;
pub const FONTSIZE_THR: f64 = 0.01;

/// RK:659–670: strip, `−` → `-`, drop thousands separators, parse as a float. `countminus` makes a
/// lone `-` count as a number. (Python's `float` also accepts `_` separators; tick labels never carry them.)
pub fn isnumeric(s: &str, countminus: bool) -> bool {
    let t: String = s.trim().replace('−', "-").replace(',', "");
    if countminus && t == "-" {
        return true;
    }
    !t.is_empty() && t.parse::<f64>().is_ok()
}

/// RK:674–676: drop spaces, tabs, CR, LF.
pub fn wstrip(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, ' ' | '\n' | '\t' | '\r')).collect()
}

/// RK:679–685: merging `a` and `b` would put two spaces in a row.
pub fn twospaces(a: &str, b: &str) -> bool {
    a.ends_with("  ") || (a.ends_with(' ') && b.starts_with(' ')) || b.starts_with("  ")
}

/// RK:687–690: `(trailing spaces of a, leading spaces of b)`.
pub fn trailing_leading(a: &str, b: &str) -> (usize, usize) {
    (
        a.chars().rev().take_while(|&c| c == ' ').count(),
        b.chars().take_while(|&c| c == ' ').count(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeType {
    Same,
    Sub,
    Super,
    SubReturn,
    SuperReturn,
}

/// A possible merge of chunk `to` after the chunk that owns the candidate list; `br1x`/`bl2x` are
/// the pen positions the choice is scored on (RK:539–544).
#[derive(Debug, Clone, Copy)]
pub struct Cand {
    pub to: ChunkRef,
    pub mtype: MergeType,
    pub br1x: f64,
    pub bl2x: f64,
}

/// RK:566–611: walk a chain's merge types through the normal/sub/super state machine; `None` when
/// the chain is inconsistent (upstream "bail") and must be dropped. Element `i+1` of the result is
/// the type of the chain's `i`-th chunk; element 0 is the head (`Normal`).
pub fn merge_wtypes(chain: &[(ChunkRef, MergeType)]) -> Option<Vec<WType>> {
    let mut ctype = WType::Normal;
    let mut out = vec![ctype];
    for (_, mt) in chain {
        ctype = match (ctype, mt) {
            (WType::Normal, MergeType::Same) => WType::Normal,
            (WType::Normal, MergeType::Sub) => WType::Sub,
            (WType::Normal, MergeType::Super) => WType::Super,
            (WType::Super, MergeType::Same) => WType::Super,
            (WType::Super, MergeType::SuperReturn) => WType::Normal,
            (WType::Sub, MergeType::Same) => WType::Sub,
            (WType::Sub, MergeType::SubReturn) => WType::Normal,
            _ => return None,
        };
        out.push(ctype);
    }
    Some(out)
}

/// RK:535–656. Each chunk keeps its closest candidate; chains are followed from every unmerged
/// head; chains with an inconsistent sub/super sequence are dropped; the rest are executed with
/// `append_chunks`, capping inserted spaces at 0 when the head already ends in a space, when the
/// merged chunk is a sub/superscript, or (manual-kerning mode) when the combined text has a space
/// and the merged chunk comes from the same style node as its predecessor. Merges that pull text
/// from other elements record a `ClipUnion`.
pub fn perform_merges(
    doc: &Doc,
    pts: &mut Vec<ParsedText>,
    ct: &mut CharTable,
    cands: &[(ChunkRef, Vec<Cand>)],
    mk: bool,
    clips: &mut Vec<ClipUnion>,
) {
    // 1. best candidate per chunk: the one whose start pen is nearest the head's end pen
    let mut link: HashMap<ChunkRef, (ChunkRef, MergeType)> = HashMap::new();
    for (w, cs) in cands {
        let mut best: Option<(f64, &Cand)> = None;
        for c in cs {
            let d = (c.bl2x - c.br1x).abs();
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, c));
            }
        }
        if let Some((_, c)) = best {
            link.insert(*w, (c.to, c.mtype));
        }
    }
    // 2. chains from unmerged heads, following the links transitively (RK:553–564)
    let mut merged: HashSet<ChunkRef> = HashSet::new();
    let mut chains: Vec<(ChunkRef, Vec<(ChunkRef, MergeType)>)> = Vec::new();
    for (w, _) in cands {
        if merged.contains(w) {
            continue;
        }
        let Some(&(mut next, mut mt)) = link.get(w) else {
            continue;
        };
        let mut seen: HashSet<ChunkRef> = HashSet::from([*w]);
        let mut chain = Vec::new();
        loop {
            if !seen.insert(next) {
                break; // a cycle cannot arise geometrically; upstream would spin forever
            }
            merged.insert(next);
            chain.push((next, mt));
            match link.get(&next) {
                Some(&(n2, m2)) => {
                    next = n2;
                    mt = m2;
                }
                None => break,
            }
        }
        chains.push((*w, chain));
    }
    // 3.+4. plan and execute (RK:566–656)
    for (w, chain) in chains {
        if merged.contains(&w) {
            continue; // became part of an earlier head's chain
        }
        let Some(types) = merge_wtypes(&chain) else {
            continue;
        };
        let Some((wli, wci)) = pts[w.0].find_chunk(w.1) else {
            continue;
        };
        let wtxt = pts[w.0].chunk_text(wli, wci);
        let mut alltxt = wtxt.clone();
        for (c, _) in &chain {
            if let Some((l, k)) = pts[c.0].find_chunk(c.1) {
                alltxt.push_str(&pts[c.0].chunk_text(l, k));
            }
        }
        let hasspaces = alltxt.contains(' ');
        let mut incoming = Vec::new();
        let mut mels = Vec::new();
        for (i, (mrg, _)) in chain.iter().enumerate() {
            let Some((li, ci)) = pts[mrg.0].find_chunk(mrg.1) else {
                continue; // already merged elsewhere (RK:623–624)
            };
            let mut max_spaces = None;
            if mk && hasspaces && pts[mrg.0].lines[li].chunks[ci].prev_same_tspan {
                max_spaces = Some(0);
            }
            if wtxt.ends_with(' ') || matches!(types[i + 1], WType::Super | WType::Sub) {
                max_spaces = Some(0);
            }
            if !mels.contains(&pts[mrg.0].el) {
                mels.push(pts[mrg.0].el);
            }
            incoming.push(Incoming { chunk: *mrg, wtype: types[i + 1], max_spaces });
        }
        if incoming.is_empty() {
            continue;
        }
        append_chunks(doc, pts, ct, w, &incoming);
        let target = pts[w.0].el;
        let others: Vec<_> = mels.into_iter().filter(|&e| e != target).collect();
        if !others.is_empty() {
            clips.push(ClipUnion { target, others });
        }
    }
}

/// Stage 6 (RK:318–376): every chunk considers merging its `next` chunk on the same baseline —
/// valid when the next chunk's start pen lies within `[br1 − 1.5·spw, br1 + dx + 0.99·spw]` and
/// within `0.01·mch` vertically, `dx = spw·(1 − trailing − leading spaces)` (0 when both texts are
/// numbers). A lone `" "` chunk that fails re-tests against its own predecessor (a weirdly kerned
/// space). After the merges, every chunk that is not the first of its line is split into its own
/// element (RK:360–376).
pub fn remove_manual_kerning(doc: &Doc, pts: &mut Vec<ParsedText>, ct: &mut CharTable, clips: &mut Vec<ClipUnion>) {
    let n0 = pts.len();
    let mut cands: Vec<(ChunkRef, Vec<Cand>)> = Vec::new();
    for pi in 0..n0 {
        let pt = &pts[pi];
        for (li, ci) in pt.chunks() {
            let w = &pt.lines[li].chunks[ci];
            let mut mw = Vec::new();
            if let Some((l2, c2)) = w.next.and_then(|nid| pt.find_chunk(nid)) {
                let nid = pt.lines[l2].chunks[c2].id;
                let (wtxt, w2txt) = (pt.chunk_text(li, ci), pt.chunk_text(l2, c2));
                if !twospaces(&wtxt, &w2txt) {
                    if let Some([_, br1, _, bl2]) = get_ut_pts(pt, (li, ci), pt, (l2, c2), true) {
                        let (trl, ldg) = trailing_leading(&wtxt, &w2txt);
                        let spw = chunk_spw(pt, li, ci);
                        let mut dx = spw * (NUM_SPACES - trl as f64 - ldg as f64);
                        let xtoln = XTOLMKN * spw;
                        let xtolp = XTOLMKP * spw;
                        let ytol = YTOLMK * chunk_mch(pt, li, ci);
                        if isnumeric(&wtxt, false) && isnumeric(&w2txt, true) {
                            dx = 0.0;
                        }
                        let mut valid = br1.x - xtoln <= bl2.x
                            && bl2.x <= br1.x + dx + xtolp
                            && br1.y - ytol <= bl2.y
                            && bl2.y <= br1.y + ytol;
                        if wtxt == " " && !valid {
                            if let Some((lp, cp)) = w.prev.and_then(|pid| pt.find_chunk(pid)) {
                                if let Some([_, br1p, _, bl2p]) = get_ut_pts(pt, (lp, cp), pt, (l2, c2), true) {
                                    let dx = spw * (NUM_SPACES - trl as f64 - ldg as f64 + 1.0);
                                    valid = br1p.x - xtoln <= bl2p.x && bl2p.x <= br1p.x + dx + xtolp;
                                }
                            }
                        }
                        if valid {
                            mw.push(Cand { to: (pi, nid), mtype: MergeType::Same, br1x: br1.x, bl2x: bl2.x });
                        }
                    }
                }
            }
            cands.push(((pi, w.id), mw));
        }
    }
    perform_merges(doc, pts, ct, &cands, true, clips);
    for pi in 0..n0 {
        let lists: Vec<Vec<usize>> = pts[pi]
            .lines
            .iter()
            .flat_map(|ln| ln.chunks.iter().skip(1).rev().map(|ch| ch.chars.clone()))
            .collect();
        if !lists.is_empty() {
            split_off(pts, pi, &lists);
        }
    }
}
```

Register both modules in `src/text/mod.rs`.

- [ ] **Step 4: Run the tests, fmt, clippy, full suite** — `cargo test --test text_kerning 2>&1 | tail -15` → 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src/text/kerning.rs src/text/write.rs src/text/mod.rs tests/text_kerning.rs
git commit -m "feat(text): kerning helpers, Perform_Merges and stage 6 manual-kerning removal (RK:318–376, 535–656)"
```

---

### Task 7: Stage 7 `external_merges` (RK:382–532)

**Files:**
- Modify: `src/text/kerning.rs` (append)
- Test: `tests/text_kerning.rs` (append)

**Interfaces:**
- Consumes: Task 6; `layout::{chunk_scf, chunk_utfs, chunk_tfs, angle_deg}`; `geom::intersects`; `CharTable::fonts.face_info(k).weight`, `FontSpec.weight`.
- Produces: `pub fn external_merges(doc: &Doc, pts: &mut Vec<ParsedText>, ct: &mut CharTable, merge_nearby: bool, merge_supersub: bool, clips: &mut Vec<ClipUnion>)`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_kerning.rs`:

```rust
use sciink::text::kerning::external_merges;

/// "Hello" at (0,0) and a second element `text2` whose left edge is `gap` space-widths after it,
/// with `dy` baseline offset and extra style/attributes.
fn pair(text2: &str, gap: f64, dy: f64, style2: &str, attrs2: &str) -> (Doc, Vec<ParsedText>, CharTable) {
    let right = lefts("Hello ")[5];
    let spw = lefts(" a")[1];
    let x2 = right + gap * spw;
    arena(&format!(
        r#"<svg {NS}><defs><clipPath id="c1"><rect width="50" height="50"/></clipPath><clipPath id="c2"><rect x="10" width="50" height="50"/></clipPath></defs><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="b" xml:space="preserve" style="{DV};font-size:10px;{style2}" x="{x2}" y="{dy}" {attrs2}>{text2}</text></svg>"#
    ))
}

#[test]
fn external_merges_join_adjacent_elements_with_a_space() {
    let (d, mut pts, mut ct) = pair("world", 1.0, 0.0, "", "");
    let before = all_positions(&pts);
    let mut clips = Vec::new();
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello world");
    assert!(pts[1].chars.is_empty());
    assert_same(&before, &all_positions(&pts), 1e-6);
    assert!(clips.is_empty(), "neither element is clipped");

    // too far (3 spaces > 1 + 0.6): untouched
    let (d, mut pts, mut ct) = pair("world", 3.0, 0.0, "", "");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!((pts[0].text().as_str(), pts[1].text().as_str()), ("Hello", "world"));

    // mergenearby off: same-line merges are disabled (sub/super still allowed)
    let (d, mut pts, mut ct) = pair("world", 1.0, 0.0, "", "");
    external_merges(&d, &mut pts, &mut ct, false, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello");

    // different rotation: never merged
    let (d, mut pts, mut ct) = pair("world", 1.0, 0.0, "", r#"transform="rotate(1)""#);
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello");

    // two numbers one space apart stay apart (tick labels, RK:458–462) …
    let (d, mut pts, mut ct) = {
        let right = lefts("0.5 ")[3];
        let spw = lefts(" a")[1];
        arena(&format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">0.5</text><text id="b" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">1.0</text></svg>"#,
            right + spw
        ))
    };
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "0.5");
    // … but a minus sign touching a number joins it
    let (d, mut pts, mut ct) = {
        let right = lefts("−0")[1];
        arena(&format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">−</text><text id="b" xml:space="preserve" style="{DV};font-size:10px" x="{right}" y="0">0.5</text></svg>"#
        ))
    };
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "−0.5");
}

#[test]
fn external_merges_detect_superscripts_and_union_clips() {
    // "2" at 6px, raised by 4 (= 40 % of 10px), touching the end of "Hello": a superscript
    let (d, mut pts, mut ct) = pair("2", 0.0, -4.0, "font-size:6px", r#"clip-path="url(#c2)""#);
    let mut clips = Vec::new();
    // give the first element a clip too, so the union is recorded
    let a = id(&d, "a");
    let mut d = d;
    d.set_attr(a, "clip-path", "url(#c1)");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello2");
    let two = pts[0].chars.iter().find(|c| c.c == '2').unwrap();
    assert!(close(two.utfs, 6.5) && close(two.bshft, 4.0));
    assert_eq!(two.sty.get("baseline-shift"), Some("super"));
    assert_eq!(clips, vec![ClipUnion { target: id(&d, "a"), others: vec![id(&d, "b")] }]);

    // mergesupersub off: no superscript merge
    let (d, mut pts, mut ct) = pair("2", 0.0, -4.0, "font-size:6px", "");
    external_merges(&d, &mut pts, &mut ct, true, false, &mut clips);
    assert_eq!(pts[0].text(), "Hello");

    // a bold superscript candidate does not merge (weight mismatch, RK:446)
    let (d, mut pts, mut ct) = pair("2", 0.0, -4.0, "font-size:6px;font-weight:bold", "");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello");

    // subscript: "2" lowered so its cap top sits below 1/3 of the line
    let (d, mut pts, mut ct) = pair("2", 0.0, 2.0, "font-size:6px", "");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello2");
    let two = pts[0].chars.iter().find(|c| c.c == '2').unwrap();
    assert!(close(two.bshft, -2.0));
    assert_eq!(two.sty.get("baseline-shift"), Some("sub"));

    // "(a)" never takes a sub/superscript (subfigure labels, RK:449)
    let (d, mut pts, mut ct) = {
        arena(&format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">(a)</text><text id="b" xml:space="preserve" style="{DV};font-size:6px" x="{}" y="-4">2</text></svg>"#,
            lefts("(a) ")[3]
        ))
    };
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "(a)");
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test text_kerning external 2>&1 | tail -5`: `external_merges` not found.

- [ ] **Step 3: Implement**

Append to `src/text/kerning.rs` (extend the imports with `use kurbo::Rect; use crate::geom::intersects; use super::layout::{angle_deg, chunk_scf, chunk_tfs, chunk_utfs}; use super::parse::TChar;`):

```rust
/// The weight of the face that actually renders `c` (upstream `tsty['font-weight']`).
fn char_weight(ct: &CharTable, c: &TChar) -> u16 {
    c.face.map(|f| ct.fonts.face_info(f).weight).unwrap_or(c.spec.weight)
}

/// Stage 7 (RK:382–532): every pair of chunks (any elements) with the same rotation whose
/// bounding boxes come within `spw·scf·1.6` of each other is tested in the first chunk's frame:
/// the second chunk's start pen must lie within `[br1 − 0.6·spw, br1 + dx + 0.6·spw]`, neither
/// text may be blank and merging may not create a double space. Then: same baseline (±0.1·mch)
/// and same transformed size (±1 %) → `Same` (numbers only when the gap is < 0.25 spaces);
/// otherwise a smaller chunk starting above 1/3 of the cap height → `Super`, a bigger one →
/// `SubReturn`; a smaller chunk whose cap top sits below 1/3 → `Sub`, a bigger one →
/// `SuperReturn`. Sub/superscripts need equal font weights and never attach to a "(a)" label.
pub fn external_merges(
    doc: &Doc,
    pts: &mut Vec<ParsedText>,
    ct: &mut CharTable,
    merge_nearby: bool,
    merge_supersub: bool,
    clips: &mut Vec<ClipUnion>,
) {
    struct Info {
        r: ChunkRef,
        li: usize,
        ci: usize,
        bb: Rect,
        bb_big: Rect,
        angle: f64,
    }
    let mut chks: Vec<Info> = Vec::new();
    for (pi, pt) in pts.iter().enumerate() {
        for (li, ci) in pt.chunks() {
            let corners: Vec<kurbo::Point> = pt.lines[li].chunks[ci]
                .chars
                .iter()
                .filter_map(|&c| pt.parsed_t.get(c).copied().flatten())
                .flatten()
                .collect();
            let Some(first) = corners.first() else {
                continue;
            };
            let bb = corners.iter().fold(Rect::from_points(*first, *first), |r, p| r.union_pt(*p));
            let dx = chunk_spw(pt, li, ci) * chunk_scf(pt, li, ci) * (NUM_SPACES + XTOLEXT);
            chks.push(Info {
                r: (pi, pt.lines[li].chunks[ci].id),
                li,
                ci,
                bb,
                bb_big: bb.inflate(dx, dx),
                angle: angle_deg(pt.transform),
            });
        }
    }
    let mut cands: Vec<(ChunkRef, Vec<Cand>)> = Vec::with_capacity(chks.len());
    for (i, w) in chks.iter().enumerate() {
        let pw = &pts[w.r.0];
        let wtxt = pw.chunk_text(w.li, w.ci);
        let spw = chunk_spw(pw, w.li, w.ci);
        let mch = chunk_mch(pw, w.li, w.ci);
        let size = |p: &ParsedText, li: usize, ci: usize| -> (f64, f64) {
            let [a, b, c, d, _, _] = p.transform.as_coeffs();
            let u = chunk_utfs(p, li, ci);
            (u * (a * a + b * b).sqrt(), u * (c * c + d * d).sqrt())
        };
        let w1fs = size(pw, w.li, w.ci);
        let wtfs = chunk_tfs(pw, w.li, w.ci);
        let w_last = &pw.chars[*pw.lines[w.li].chunks[w.ci].chars.last().expect("non-empty")];
        let letterinpar = {
            let cs: Vec<char> = wtxt.chars().collect();
            cs.len() == 3 && cs[0] == '(' && cs[2] == ')' && cs[1].is_ascii_alphabetic()
        };
        let mut mw = Vec::new();
        for (j, w2) in chks.iter().enumerate() {
            if i == j || (w.angle - w2.angle).abs() >= 0.001 || !intersects(w.bb_big, w2.bb) {
                continue;
            }
            let p2 = &pts[w2.r.0];
            let w2txt = p2.chunk_text(w2.li, w2.ci);
            let (trl, ldg) = trailing_leading(&wtxt, &w2txt);
            let dx = spw * (NUM_SPACES - trl as f64 - ldg as f64);
            let xtol = XTOLEXT * spw;
            let ytol = YTOLEXT * mch;
            let Some([tr1, br1, tl2, bl2]) = get_ut_pts(pw, (w.li, w.ci), p2, (w2.li, w2.ci), true) else {
                continue;
            };
            let xpen = br1.x - xtol <= bl2.x && bl2.x <= br1.x + dx + xtol;
            let neither_empty = !wstrip(&wtxt).is_empty() && !wstrip(&w2txt).is_empty();
            if !(xpen && neither_empty && !twospaces(&wtxt, &w2txt)) {
                continue;
            }
            let w2_first = &p2.chars[p2.lines[w2.li].chunks[w2.ci].chars[0]];
            let weight_match = char_weight(ct, w_last) == char_weight(ct, w2_first);
            let w2fs = size(p2, w2.li, w2.ci);
            let w2tfs = chunk_tfs(p2, w2.li, w2.ci);
            let mut mtype = None;
            if (bl2.y - br1.y).abs() < ytol
                && (w1fs.0 - w2fs.0).abs() < FONTSIZE_THR * w1fs.0
                && (w1fs.1 - w2fs.1).abs() < FONTSIZE_THR * w1fs.1
                && merge_nearby
            {
                if isnumeric(&pw.line_text(w.li), false) && isnumeric(&p2.line_text(w2.li), true) {
                    if ((bl2.x - br1.x) / spw).abs() < 0.25 {
                        mtype = Some(MergeType::Same);
                    }
                } else {
                    mtype = Some(MergeType::Same);
                }
            } else if br1.y + ytol >= bl2.y && bl2.y >= tr1.y - ytol && merge_supersub && weight_match && !letterinpar {
                let aboveline = br1.y * (1.0 - SUBSUPER_YTHR) + tr1.y * SUBSUPER_YTHR + ytol >= bl2.y;
                if w2tfs < wtfs * SUBSUPER_THR {
                    if aboveline {
                        mtype = Some(MergeType::Super);
                    }
                } else if wtfs < w2tfs * SUBSUPER_THR {
                    mtype = Some(MergeType::SubReturn);
                }
            } else if br1.y + ytol >= tl2.y && tl2.y >= tr1.y - ytol && merge_supersub && weight_match && !letterinpar {
                let belowline = tl2.y >= br1.y * SUBSUPER_YTHR + tr1.y * (1.0 - SUBSUPER_YTHR) - ytol;
                if w2tfs < wtfs * SUBSUPER_THR {
                    if belowline {
                        mtype = Some(MergeType::Sub);
                    }
                } else if wtfs < w2tfs * SUBSUPER_THR {
                    mtype = Some(MergeType::SuperReturn);
                }
            }
            if let Some(m) = mtype {
                mw.push(Cand { to: w2.r, mtype: m, br1x: br1.x, bl2x: bl2.x });
            }
        }
        cands.push((w.r, mw));
    }
    perform_merges(doc, pts, ct, &cands, false, clips);
}
```

(`Rect::union_pt` and `Rect::inflate` exist in kurbo 0.13; `intersects` is the strict centre-distance test from `geom`.) The pair loop is O(n²) over chunks with two cheap rejections first; spec §A.5 item 10 accepts that.

- [ ] **Step 4: Run, fmt, clippy, full suite** — `cargo test --test text_kerning 2>&1 | tail -15` → 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src/text/kerning.rs tests/text_kerning.rs
git commit -m "feat(text): stage 7 external merges — same-line, sub/superscript detection, clip-union records (RK:382–532)"
```

---

### Task 8: Stage 8 splits (RK:183–315)

**Files:**
- Modify: `src/text/kerning.rs` (append)
- Test: `tests/text_kerning.rs` (append)

**Interfaces:**
- Consumes: Tasks 5–7 (`split_off`, `get_ut_pts(.., false)`, `chunk_char_pts`).
- Produces: `pub fn split_distant_chunks(pts: &mut Vec<ParsedText>)`, `pub fn split_distant_intrachunk(pts: &mut Vec<ParsedText>)`, `pub fn split_lines(pts: &mut Vec<ParsedText>)`. Each iterates only the elements that existed when it started (upstream evaluates `[el.parsed_text for el in els]` once) and appends the split-off models to the arena.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_kerning.rs`:

```rust
use sciink::text::kerning::{split_distant_chunks, split_distant_intrachunk, split_lines};

#[test]
fn distant_chunks_and_lines_become_their_own_elements() {
    let spw = lefts(" a")[1];
    let ab = lefts("ab");
    let ab_r = lefts("ab ")[2];
    let x_cd = ab_r + spw;
    let cd = lefts("cd");
    let cd_r = x_cd + lefts("cd ")[2];
    let x_ef = cd_r + 4.0 * spw;
    let ef = lefts("ef");
    // per-character x: a b (own glyph positions) | c d | e f — only chunk starts differ from the pen
    let xs = vec![ab[0], ab[1], x_cd + cd[0], x_cd + cd[1], x_ef + ef[0], x_ef + ef[1]];
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">abcdef</text></svg>"#,
        fmt_list(&xs)
    ));
    let before = all_positions(&pts);
    split_distant_chunks(&mut pts);
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[0].text(), "abcd");
    assert_eq!(pts[1].text(), "ef");
    assert_same(&before, &all_positions(&pts), 1e-6);

    // lines: every line after the first becomes an element (skipping multi-line Inkscape text)
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">one<tspan x="0" y="12">two</tspan><tspan x="0" y="24">three</tspan></text></svg>"#
    ));
    let before = all_positions(&pts);
    split_lines(&mut pts);
    let texts: Vec<String> = pts.iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["one", "two", "three"]);
    assert_same(&before, &all_positions(&pts), 1e-6);

    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;-inkscape-font-specification:'DejaVu Sans'" x="0" y="0"><tspan sodipodi:role="line" x="0" y="0" style="-inkscape-font-specification:'DejaVu Sans'">one</tspan><tspan sodipodi:role="line" x="0" y="12" style="-inkscape-font-specification:'DejaVu Sans'">two</tspan></text></svg>"#
    ));
    assert!(pts[0].is_ml_inkscape);
    split_lines(&mut pts);
    assert_eq!(pts.len(), 1, "Inkscape-generated multi-line text is left alone");
}

#[test]
fn distant_characters_inside_a_chunk_split_including_tick_numbers() {
    // "ab" then a 2-space hole then "cd" inside ONE chunk, made with dx on 'c'
    let spw = lefts(" a")[1];
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0" dx="0 0 {}">abcd</text></svg>"#,
        2.0 * spw
    ));
    let before = all_positions(&pts);
    split_distant_intrachunk(&mut pts);
    assert_eq!(pts.len(), 2);
    assert_eq!((pts[0].text().as_str(), pts[1].text().as_str()), ("ab", "cd"));
    assert_same(&before, &all_positions(&pts), 1e-6);

    // numbers separated by a single space always split (tick labels, RK:280–295)
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">0.5 1.0</text></svg>"#
    ));
    split_distant_intrachunk(&mut pts);
    let texts: Vec<String> = pts.iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["0.5", " 1.0"]);

    // words separated by one space do not
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">ab cd</text></svg>"#
    ));
    split_distant_intrachunk(&mut pts);
    assert_eq!(pts.len(), 1);
}
```

- [ ] **Step 2: Run to verify failure** — the three functions do not exist.

- [ ] **Step 3: Implement**

Append to `src/text/kerning.rs` (add `use super::layout::chunk_char_pts;`):

```rust
/// RK:205–253: within each line, sort the chunks by x and split before a chunk whose start pen is
/// more than one space (+0.5 tolerance, minus existing spaces) past the previous chunk's end pen,
/// judged on CURRENT positions. Each split range becomes its own element.
pub fn split_distant_chunks(pts: &mut Vec<ParsedText>) {
    let n0 = pts.len();
    for pi in 0..n0 {
        for li in (0..pts[pi].lines.len()).rev() {
            let n = pts[pi].lines[li].chunks.len();
            if n < 2 {
                continue;
            }
            let mut sws: Vec<usize> = (0..n).collect();
            sws.sort_by(|&a, &b| {
                let (xa, xb) = (pts[pi].lines[li].chunks[a].x, pts[pi].lines[li].chunks[b].x);
                xa.partial_cmp(&xb).unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut splits: Vec<usize> = Vec::new();
            for ii in 1..n {
                let (a, b) = (sws[ii - 1], sws[ii]);
                let pt = &pts[pi];
                let (wtxt, w2txt) = (pt.chunk_text(li, a), pt.chunk_text(li, b));
                let (trl, ldg) = trailing_leading(&wtxt, &w2txt);
                let spw = chunk_spw(pt, li, a);
                let dx = spw * (NUM_SPACES - trl as f64 - ldg as f64);
                let xtol = XTOLSPLIT * spw;
                if let Some([_, br1, _, bl2]) = get_ut_pts(pt, (li, a), pt, (li, b), false) {
                    if bl2.x > br1.x + dx + xtol {
                        splits.push(ii);
                    }
                }
            }
            if splits.is_empty() {
                continue;
            }
            let mut lists: Vec<Vec<usize>> = Vec::new();
            for k in (0..splits.len()).rev() {
                let (sstart, sstop) = (splits[k], splits.get(k + 1).copied().unwrap_or(n));
                lists.push(
                    sws[sstart..sstop]
                        .iter()
                        .flat_map(|&ci| pts[pi].lines[li].chunks[ci].chars.clone())
                        .collect(),
                );
            }
            split_off(pts, pi, &lists);
        }
    }
}

/// RK:257–315 (skipped for multi-line Inkscape text and flows): within each chunk, characters in
/// x order; compare each to the last non-space one and split when the gap exceeds one space
/// (+0.5), or when a space/hyphen separates two numbers in the same text node (tick labels).
/// Upstream slices the chunk text by the SORTED index — kept as is.
pub fn split_distant_intrachunk(pts: &mut Vec<ParsedText>) {
    let n0 = pts.len();
    for pi in 0..n0 {
        if pts[pi].is_ml_inkscape || pts[pi].is_flow {
            continue;
        }
        let ids: Vec<u32> = pts[pi].chunks().map(|(l, c)| pts[pi].chunk(l, c).id).collect();
        for cid in ids {
            let Some((li, ci)) = pts[pi].find_chunk(cid) else {
                continue;
            };
            let lists = {
                let pt = &pts[pi];
                let ch = &pt.lines[li].chunks[ci];
                let now = chunk_char_pts(pt, li, ci);
                let mut order: Vec<usize> = (0..ch.chars.len()).collect();
                order.sort_by(|&a, &b| now[a][0].x.partial_cmp(&now[b][0].x).unwrap_or(std::cmp::Ordering::Equal));
                let txt: Vec<char> = ch.chars.iter().map(|&c| pt.chars[c].c).collect();
                let spw = chunk_spw(pt, li, ci);
                let (dx, xtol) = (spw * NUM_SPACES, XTOLSPLIT * spw);
                let is_space = |c: char| matches!(c, ' ' | '\u{a0}');
                let mut lastnspc: Option<usize> = (!is_space(txt[order[0]])).then_some(order[0]);
                let mut splitiis: Vec<usize> = Vec::new();
                let mut prevsplit = 0usize;
                for ii in 1..order.len() {
                    if let Some(cw) = lastnspc {
                        let c2w = order[ii];
                        let rest: String = txt[ii..].iter().collect();
                        let remaining_numeric = rest
                            .split([' ', '-', '−'])
                            .find(|s| !s.is_empty())
                            .is_some_and(|s| isnumeric(s, false));
                        let seg: String = txt[prevsplit..ii].iter().collect();
                        let (c, c2) = (&pt.chars[ch.chars[cw]], &pt.chars[ch.chars[c2w]]);
                        let numbersplit = isnumeric(&seg, false)
                            && matches!(c2.c, ' ' | '-' | '−')
                            && remaining_numeric
                            && c.loc.node == c2.loc.node;
                        if now[c2w][0].x > now[cw][3].x + dx + xtol || numbersplit {
                            splitiis.push(ii);
                            prevsplit = ii;
                        }
                    }
                    if !is_space(txt[order[ii]]) {
                        lastnspc = Some(order[ii]);
                    }
                }
                let mut lists: Vec<Vec<usize>> = Vec::new();
                for k in (0..splitiis.len()).rev() {
                    let (sstart, sstop) = (splitiis[k], splitiis.get(k + 1).copied().unwrap_or(order.len()));
                    let sel: HashSet<usize> = order[sstart..sstop].iter().copied().collect();
                    lists.push(
                        ch.chars
                            .iter()
                            .enumerate()
                            .filter(|(w, _)| sel.contains(w))
                            .map(|(_, &c)| c)
                            .collect(),
                    );
                }
                lists
            };
            if !lists.is_empty() {
                split_off(pts, pi, &lists);
            }
        }
    }
}

/// RK:183–201: every line after the first becomes its own element (not for multi-line Inkscape
/// text or flows).
pub fn split_lines(pts: &mut Vec<ParsedText>) {
    let n0 = pts.len();
    for pi in 0..n0 {
        let pt = &pts[pi];
        if pt.lines.len() < 2 || pt.is_ml_inkscape || pt.is_flow {
            continue;
        }
        let lists: Vec<Vec<usize>> = (1..pt.lines.len()).rev().map(|li| pt.lines[li].chars.clone()).collect();
        split_off(pts, pi, &lists);
    }
}
```

- [ ] **Step 4: Run, fmt, clippy, full suite** — `cargo test --test text_kerning 2>&1 | tail -15` → 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src/text/kerning.rs tests/text_kerning.rs
git commit -m "feat(text): stage 8 splits — distant chunks, intra-chunk gaps and tick numbers, lines (RK:183–315)"
```

---

### Task 9: Stages 9–11 — justification, stray spaces, merge-position fix

**Files:**
- Modify: `src/text/edit.rs` (append `change_alignment`, `fix_merged_position`)
- Modify: `src/text/kerning.rs` (append the three drivers)
- Test: `tests/text_kerning.rs` (append)

**Interfaces:**
- Consumes: Tasks 1–8; `text::style::Anchor`.
- Produces: `edit::change_alignment(pt: &mut ParsedText, li: usize, newanch: Anchor)` (P:2751–2787), `edit::fix_merged_position(pt: &mut ParsedText, li: usize, ci: usize)` (P:3505–3529), `kerning::change_justification(pts: &mut [ParsedText], j: Option<Anchor>)` (RK:167–179), `kerning::remove_trailing_leading_spaces(pts: &mut [ParsedText]) -> bool` (RK:139–159), `kerning::fix_merge_positions(pts: &mut [ParsedText])` (RK:132–136).

**Deviation (documented for Task 11):** `change_alignment` computes every chunk's box with the OLD anchor before moving any chunk. Upstream assigns `self.anchor = newanch` inside the per-chunk loop (P:2785–2787), so the second and later chunks of a multi-chunk line are measured with the new anchor but their old x whenever their geometry cache is cold — a position error the stated intent ("without affecting character position") rules out.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_kerning.rs`:

```rust
use sciink::text::kerning::{change_justification, fix_merge_positions, remove_trailing_leading_spaces};
use sciink::text::style::Anchor;

#[test]
fn justification_changes_anchor_without_moving_glyphs() {
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0 40" y="0">ab cd </text></svg>"#
    ));
    let before = all_positions(&pts);
    change_justification(&mut pts, Some(Anchor::Middle));
    assert_eq!(pts[0].lines[0].spec.anchor, Anchor::Middle);
    assert_eq!(pts[0].text_anchor_override, Some(Anchor::Middle));
    assert_same(&before, &all_positions(&pts), 1e-6);
    // x="0 40" positions the 1st and 2nd characters, so the chunks are "a" and "b cd ": the second
    // chunk's new anchor is the centre of "b cd" — its trailing unrendered space does not count
    let g = chunk_geom(&pts[0], 0, 1);
    assert_eq!(pts[0].chunk_text(0, 1), "b cd ");
    assert!(close(pts[0].lines[0].chunks[1].x, 0.5 * (g.left[0] + g.right[3])), "{}", pts[0].lines[0].chunks[1].x);
    // None → untouched
    let (_d, mut pts2, _) = arena(&format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0">ab</text></svg>"#));
    change_justification(&mut pts2, None);
    assert_eq!(pts2[0].text_anchor_override, None);
    assert_eq!(pts2[0].lines[0].spec.anchor, Anchor::Start);
}

#[test]
fn stray_spaces_go_and_positions_stay() {
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:end" x="80" y="0">  ab  <tspan x="0" y="20">   </tspan></text></svg>"#
    ));
    let before = all_positions(&pts);
    assert!(remove_trailing_leading_spaces(&mut pts));
    assert_eq!(pts[0].text(), "ab");
    assert_eq!(pts[0].lines.len(), 1, "an all-space line disappears");
    assert_same(&before, &all_positions(&pts), 1e-6);
    assert!(!remove_trailing_leading_spaces(&mut pts), "nothing left to remove");
}

#[test]
fn fix_merge_positions_restores_the_parsed_anchor() {
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="50" y="0">abc</text></svg>"#
    ));
    let before = all_positions(&pts);
    pts[0].lines[0].chunks[0].x += 3.0; // simulate drift left behind by a merge
    fix_merge_positions(&mut pts);
    assert_same(&before, &all_positions(&pts), 1e-9);
    assert!(close(pts[0].lines[0].chunks[0].x, 50.0));
}
```

- [ ] **Step 2: Run to verify failure** — functions missing.

- [ ] **Step 3: Implement**

Append to `src/text/edit.rs` (add `use super::style::Anchor;`):

```rust
/// P:2751–2787 minus the DOM writes: give a line a new anchor while every glyph stays put — each
/// chunk's new `x` is `(1−anfr)·minx + anfr·maxx` of its current box (an unrendered trailing space
/// of the line's last chunk does not count). All boxes are measured with the OLD anchor first.
pub fn change_alignment(pt: &mut ParsedText, li: usize, newanch: Anchor) {
    if pt.lines[li].spec.anchor == newanch {
        return;
    }
    let anfr = newanch.anfr();
    let last = *pt.lines[li].chars.last().expect("non-empty line");
    let mut newx = Vec::with_capacity(pt.lines[li].chunks.len());
    for ci in 0..pt.lines[li].chunks.len() {
        let g = chunk_geom(pt, li, ci);
        let minx = g.pts_ut.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let mut maxx = g.pts_ut.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        if unrendered_space(pt, li, ci) && pt.lines[li].chunks[ci].chars.contains(&last) {
            maxx -= pt.chars[last].cwd;
        }
        newx.push((1.0 - anfr) * minx + anfr * maxx);
    }
    let ln = &mut pt.lines[li];
    for (ci, x) in newx.into_iter().enumerate() {
        ln.chunks[ci].x = x;
    }
    ln.spec.anchor = newanch;
    ln.spec.continue_x = false;
    ln.spec.sprl = false;
    ln.spec.x = ln.chunks.iter().map(|c| Some(c.x)).collect();
}

/// P:3505–3529: after merges (and with the final anchor set), move the chunk so the anchor of its
/// non-space characters is back where the parsed positions had it.
pub fn fix_merged_position(pt: &mut ParsedText, li: usize, ci: usize) {
    let now = chunk_char_pts(pt, li, ci);
    let anfr = pt.lines[li].spec.anchor.anfr();
    let (mut omin, mut omax) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut nmin, mut nmax) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut any = false;
    for (wi, &c) in pt.lines[li].chunks[ci].chars.iter().enumerate() {
        if pt.chars[c].c == ' ' {
            continue;
        }
        let Some(p) = pt.parsed_ut.get(c).copied().flatten() else {
            continue;
        };
        any = true;
        omin = omin.min(p[0].x);
        omax = omax.max(p[3].x);
        nmin = nmin.min(now[wi][0].x);
        nmax = nmax.max(now[wi][3].x);
    }
    if !any {
        return;
    }
    let delta = (nmin * (1.0 - anfr) + nmax * anfr) - (omin * (1.0 - anfr) + omax * anfr);
    if delta.abs() > XY_TOL {
        pt.lines[li].chunks[ci].x -= delta;
    }
}
```

Append to `src/text/kerning.rs` (add `use super::edit::{change_alignment, delete_char, fix_merged_position}; use super::style::Anchor;`):

```rust
/// Stage 9 (RK:167–179): re-anchor every line of every element (not multi-line Inkscape text or
/// flows) and remember to write `text-anchor`/`text-align` on the `<text>` itself.
pub fn change_justification(pts: &mut [ParsedText], j: Option<Anchor>) {
    let Some(a) = j else {
        return;
    };
    for pt in pts.iter_mut() {
        if pt.is_ml_inkscape || pt.is_flow {
            continue;
        }
        for li in 0..pt.lines.len() {
            change_alignment(pt, li, a);
        }
        pt.text_anchor_override = Some(a);
    }
}

/// Stage 10 (RK:139–159): delete trailing, then leading `' '` characters of every line (not
/// multi-line Inkscape text or flows). Returns whether anything was removed.
pub fn remove_trailing_leading_spaces(pts: &mut [ParsedText]) -> bool {
    let mut removed = false;
    for pt in pts.iter_mut() {
        if pt.is_ml_inkscape || pt.is_flow {
            continue;
        }
        let mut li = 0;
        while li < pt.lines.len() {
            let n_before = pt.lines.len();
            while let Some(&last) = pt.lines.get(li).and_then(|l| l.chars.last()) {
                if pt.chars[last].c != ' ' || pt.lines.len() < n_before {
                    break;
                }
                delete_char(pt, last);
                removed = true;
            }
            while let Some(&first) = pt.lines.get(li).and_then(|l| l.chars.first()) {
                if pt.chars[first].c != ' ' || pt.lines.len() < n_before {
                    break;
                }
                delete_char(pt, first);
                removed = true;
            }
            if pt.lines.len() == n_before {
                li += 1; // otherwise the line vanished and `li` already names the next one
            }
        }
    }
    removed
}

/// Stage 11 (RK:132–136).
pub fn fix_merge_positions(pts: &mut [ParsedText]) {
    for pt in pts.iter_mut() {
        for li in 0..pt.lines.len() {
            for ci in 0..pt.lines[li].chunks.len() {
                fix_merged_position(pt, li, ci);
            }
        }
    }
}
```

- [ ] **Step 4: Run, fmt, clippy, full suite** — `cargo test --test text_kerning 2>&1 | tail -15` → 11 passed.

- [ ] **Step 5: Commit**

```bash
git add src/text/edit.rs src/text/kerning.rs tests/text_kerning.rs
git commit -m "feat(text): stages 9–11 — justification without motion, stray-space removal, merged-position fix"
```

---

### Task 10: Stage 12 — the clean writer and clip unions (P:2385–2444, P:3456–3503, RK:640–656)

**Files:**
- Modify: `src/text/write.rs` (extend)
- Test: `tests/text_write.rs` (new)

**Interfaces:**
- Consumes: Tasks 1–9; `Doc::{new_element, new_text, insert_after, prepend_child, append_child, detach, attrs, attr, set_attr, remove_attr, set_style_map, set_style, remove_style, specified_style, transform, composed_transform, deep_clone, ensure_id, by_id, children, prev_sibling, parent}`; `style::{Style, default_value}`; `geom::{fmt_transform, inverse, is_identity}`; `layout::chunk_utfs`; `edit::style_eq`.
- Produces (`text::write`):
  - `pub fn specified_diff(desired: &Style, inherited: &Style) -> Style` (C:206–221)
  - `#[derive(Debug, Clone, Copy)] pub enum Slot { After(NodeId), FirstIn(NodeId) }` — where a split-off element is inserted once its source has been rewritten or removed.
  - `pub fn write_clean_text(doc: &mut Doc, pt: &ParsedText, ct: &CharTable, replaced: &mut HashMap<NodeId, Slot>) -> Option<NodeId>` — regenerates one element; `replaced` maps a source element to its replacement's slot and must be shared across one pipeline run (sources are written before their split-offs because `split_off` appends to the arena).
  - `pub fn apply_clip_unions(doc: &mut Doc, unions: &[ClipUnion])` (RK:640–656).

Element shape (P:2385–2444): a new `<text xml:space="preserve">` right after the old one (old deleted, **id reused**; a split-off gets a fresh `sciink-N` id and is inserted after its source's replacement); attributes copied except `baseline-shift, shape-inside, direction, style, font-family` (and `textLength`/`lengthAdjust` when stage 2 removed them) — **Deviation:** the per-character lists `x, y, dx, dy, rotate` are not copied either (upstream copies them; SVG applies an ancestor's list to every character its own tspan list does not cover, so a stale `dx` on the `<text>` would shift characters whose tspan `dx` list was trimmed — the `sodipodi:role` branch below writes `x`/`y` explicitly when they are meaningful); style = old local `style` minus `{baseline-shift, shape-inside, direction}` plus `font-family:'<true family of the first character>'` (single-quoted, as upstream's `tsty` is) plus the stage-9 `text-anchor`/`text-align`; `transform` = own transform · `transform_extra` when the latter is not identity. One `<tspan>` per chunk (`make_tspan`, P:3456–3503; chunks with a NaN `y` skipped): `x`, `y`, `dx`/`dy` lists (trailing zeros trimmed, omitted when all zero), style = `specified_diff(first char's sty, tspan's inherited specified style)` + `font-size:<chunk max utfs>` + `text-align` + `text-anchor` − `{line-height, direction, baseline-shift, shape-inside}`; when style, `utfs` or `bshft` changes along the chunk or the first character has `|bshft| > XY_TOL`, one nested `<tspan>` per run with `font-size: round3(utfs/chunk_utfs·100)%` (omitted within 0.001) and `baseline-shift` `super` (bs/utfs ≈ 0.4), `sub` (≈ −0.2) or `round3(bs/utfs·100)%` (omitted when |bs| ≤ 0.001), minus declarations equal to the parent tspan's specified style and minus `{text-align, text-anchor, direction, shape-inside}`. Finally `sodipodi:role="line"` on every tspan plus `font-size:<min utfs>`, `line-height:<step>` (1.25 for one chunk) and `x`/`y` on the `<text>` when all chunk x agree (±0.001) and every y step divided by `max(fsz[i+1], min fsz)` equals the first (±0.001).

- [ ] **Step 1: Write the failing tests**

Create `tests/text_write.rs` with the preamble plus:

```rust
use std::collections::HashMap;

use sciink::style::Style;
use sciink::text::edit::{remove_chars, split_off};
use sciink::text::layout::{chunk_char_pts, snapshot_parsed, transform_pts};
use sciink::text::write::{ClipUnion, apply_clip_unions, specified_diff, write_clean_text};

fn all_texts(d: &mut Doc) -> Vec<NodeId> {
    d.descendants(d.svg()).filter(|&n| d.is_element(n) && d.tag(n) == "text").collect()
}
fn arena(d: &mut Doc) -> (Vec<ParsedText>, CharTable) {
    let els = all_texts(d);
    let mut w = Warnings::default();
    let mut ct = CharTable::build(d, &els, fonts(), &mut w);
    let mut pts: Vec<ParsedText> = els.iter().filter_map(|&e| ParsedText::parse(d, e, &mut ct, &mut w)).collect();
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
    }
    (pts, ct)
}
fn positions(pts: &[ParsedText]) -> Vec<(char, f64, f64)> {
    let mut v = Vec::new();
    for pt in pts {
        for (li, ln) in pt.lines.iter().enumerate() {
            for ci in 0..ln.chunks.len() {
                let p = chunk_char_pts(pt, li, ci);
                for (wi, &c) in ln.chunks[ci].chars.iter().enumerate() {
                    if pt.chars[c].c != ' ' {
                        let q = transform_pts(pt.transform, p[wi])[0];
                        v.push((pt.chars[c].c, q.x, q.y));
                    }
                }
            }
        }
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}
fn write_all(d: &mut Doc, pts: &[ParsedText], ct: &CharTable) -> Vec<Option<NodeId>> {
    let mut rep = HashMap::new();
    pts.iter().map(|pt| write_clean_text(d, pt, ct, &mut rep)).collect()
}
fn out(d: &Doc) -> String {
    let mut v = Vec::new();
    d.write(&mut v);
    String::from_utf8(v).unwrap()
}

#[test]
fn specified_diff_follows_cache_py() {
    let desired = Style::parse("fill:red;font-size:10px;font-weight:bold");
    let inherited = Style::parse("fill:red;font-size:12px;stroke:blue");
    let diff = specified_diff(&desired, &inherited);
    assert_eq!(diff.get("fill"), None, "already inherited");
    assert_eq!(diff.get("font-size"), Some("10px"));
    assert_eq!(diff.get("font-weight"), Some("bold"));
    assert_eq!(diff.get("stroke"), Some("none"), "inherited but unwanted → initial value");
}

#[test]
fn writer_regenerates_a_clean_element_that_reparses_to_the_same_positions() {
    let svg = format!(
        r#"<svg {NS}><g transform="translate(5,5)"><text id="t" class="k" data-x="1" style="{DV};font-size:10px;baseline-shift:0;direction:ltr;fill:red" x="0" y="0">ab<tspan style="font-weight:bold">c</tspan> <tspan x="0" y="20" style="font-size:6px;baseline-shift:super">d</tspan>ef</text></g></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (pts, ct) = arena(&mut d);
    let before = positions(&pts);
    let new = write_all(&mut d, &pts, &ct);
    assert_eq!(new.len(), 1);
    let te = new[0].expect("rewritten");
    // structure
    assert_eq!(d.attr(te, "id"), Some("t"));
    assert_eq!(d.attr(te, "xml:space"), Some("preserve"));
    assert_eq!(d.attr(te, "class"), Some("k"));
    assert_eq!(d.attr(te, "data-x"), Some("1"));
    assert_eq!(all_texts(&mut d).len(), 1, "old element gone");
    let st = Style::parse(d.attr(te, "style").unwrap());
    assert_eq!(st.get("font-family"), Some("'DejaVu Sans'"));
    assert_eq!(st.get("fill"), Some("red"));
    assert!(st.get("baseline-shift").is_none() && st.get("direction").is_none());
    let tspans: Vec<NodeId> = d.children(te).filter(|&n| d.is_element(n)).collect();
    assert_eq!(tspans.len(), 2, "one tspan per chunk");
    assert_eq!(d.attr(tspans[0], "x"), Some("0"));
    assert_eq!(d.attr(tspans[1], "y"), Some("20"));
    let s0 = Style::parse(d.attr(tspans[0], "style").unwrap());
    assert_eq!(s0.get("font-size"), Some("10"));
    assert_eq!((s0.get("text-anchor"), s0.get("text-align")), (Some("start"), Some("start")));
    // nested runs: "ab" | "c" (bold) | " "
    let nested: Vec<NodeId> = d.children(tspans[0]).filter(|&n| d.is_element(n)).collect();
    assert_eq!(nested.len(), 3);
    assert_eq!(d.text_content(nested[0]), "ab");
    assert_eq!(d.text_content(nested[1]), "c");
    let sc = Style::parse(d.attr(nested[1], "style").unwrap());
    assert_eq!(sc.get("font-weight"), Some("bold"));
    assert!(sc.get("font-size").is_none() && sc.get("baseline-shift").is_none());
    // second chunk: "d" (60 %, super) | "ef"
    let nested: Vec<NodeId> = d.children(tspans[1]).filter(|&n| d.is_element(n)).collect();
    assert_eq!(nested.len(), 2);
    let sd = Style::parse(d.attr(nested[0], "style").unwrap());
    assert_eq!(sd.get("font-size"), Some("60%"));
    assert_eq!(sd.get("baseline-shift"), Some("super"));
    // both chunks at x=0 with one consistent y step → sodipodi:role="line" everywhere
    assert!(tspans.iter().all(|&t| d.attr(t, "sodipodi:role") == Some("line")));
    assert_eq!((d.attr(te, "x"), d.attr(te, "y")), (Some("0"), Some("0")));
    assert_eq!(st.get("font-size"), Some("10"));
    assert_eq!(st.get("line-height"), Some("2"));
    // appearance: re-parse the written document
    let s = out(&d);
    let mut d2 = Doc::parse(s.as_bytes()).unwrap();
    let (pts2, _) = arena(&mut d2);
    let after = positions(&pts2);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!(b.0 == a.0 && (b.1 - a.1).abs() < 1e-6 && (b.2 - a.2).abs() < 1e-6, "{b:?} vs {a:?}\n{s}");
    }
}

#[test]
fn writer_omits_dx_when_zero_and_skips_role_line_for_ragged_chunks() {
    let svg = format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="0 40" y="0 3" dx="0 2 0">abc</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (pts, ct) = arena(&mut d);
    let te = write_all(&mut d, &pts, &ct)[0].unwrap();
    let tspans: Vec<NodeId> = d.children(te).filter(|&n| d.is_element(n)).collect();
    assert_eq!(tspans.len(), 2);
    assert_eq!(d.attr(tspans[0], "dx"), None, "chunk 'a' has no dx");
    assert_eq!(d.attr(tspans[1], "dx"), Some("2"), "trailing zero trimmed");
    assert_eq!(d.attr(tspans[1], "y"), Some("3"));
    assert!(tspans.iter().all(|&t| d.attr(t, "sodipodi:role").is_none()), "x differs → no role=line");
    assert_eq!(d.attr(te, "x"), None);
    let s1 = Style::parse(d.attr(tspans[1], "style").unwrap());
    assert_eq!((s1.get("text-anchor"), s1.get("text-align")), (Some("middle"), Some("center")));
    // a single chunk always qualifies: line-height 1.25
    let svg = format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="7" y="9">ab</text></svg>"#);
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (pts, ct) = arena(&mut d);
    let te = write_all(&mut d, &pts, &ct)[0].unwrap();
    let st = Style::parse(d.attr(te, "style").unwrap());
    assert_eq!(st.get("line-height"), Some("1.25"));
    assert_eq!((d.attr(te, "x"), d.attr(te, "y")), (Some("7"), Some("9")));
    let root = d.svg();
    assert_eq!(
        d.attr(root, "xmlns:sodipodi"),
        Some("http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"),
        "role=line needs its namespace on a matplotlib-style document"
    );
}

#[test]
fn writer_places_split_offs_reuses_ids_and_removes_emptied_elements() {
    let svg = format!(
        r#"<svg {NS}><g id="g"><rect id="r"/><text id="t" style="{DV};font-size:10px" x="0" y="0">ab</text><text id="u" style="{DV};font-size:10px" x="0" y="30">cd</text></g></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (mut pts, ct) = arena(&mut d);
    let news = split_off(&mut pts, 0, &[vec![1]]); // 'b' leaves t
    assert_eq!(news, [2]);
    let all: Vec<usize> = (0..pts[1].chars.len()).collect();
    remove_chars(&mut pts[1], &all); // u loses everything (as if merged elsewhere)
    let res = write_all(&mut d, &pts, &ct);
    assert!(res[0].is_some() && res[1].is_none() && res[2].is_some());
    let g = id(&d, "g");
    let kids: Vec<(String, Option<String>)> = d
        .children(g)
        .filter(|&n| d.is_element(n))
        .map(|n| (d.tag(n).to_string(), d.attr(n, "id").map(str::to_string)))
        .collect();
    assert_eq!(kids.len(), 3, "{kids:?}");
    assert_eq!(kids[0], ("rect".into(), Some("r".into())));
    assert_eq!(kids[1], ("text".into(), Some("t".into())), "rewritten in place, id reused");
    assert_eq!(kids[2].0, "text");
    assert!(kids[2].1.as_deref().unwrap().starts_with("sciink-"), "split-off gets a fresh id");
    assert!(d.by_id("u").is_none(), "emptied element removed");
    assert_eq!(d.text_content(res[2].unwrap()), "b");

    // a split-off whose source was emptied lands where the source was
    let svg = format!(r#"<svg {NS}><g id="g"><rect id="r"/><text id="t" style="{DV};font-size:10px" x="0" y="0">ab</text><rect id="s"/></g></svg>"#);
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (mut pts, ct) = arena(&mut d);
    split_off(&mut pts, 0, &[vec![1]]);
    let rest: Vec<usize> = (0..pts[0].chars.len()).collect();
    remove_chars(&mut pts[0], &rest);
    let res = write_all(&mut d, &pts, &ct);
    assert!(res[0].is_none() && res[1].is_some());
    let g = id(&d, "g");
    let tags: Vec<String> = d.children(g).filter(|&n| d.is_element(n)).map(|n| d.tag(n).to_string()).collect();
    assert_eq!(tags, ["rect", "text", "rect"]);
}

#[test]
fn clip_unions_duplicate_and_transform_or_drop() {
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="c1"><rect width="50" height="50"/></clipPath><clipPath id="c2"><rect x="10" width="50" height="50"/></clipPath></defs><text id="a" clip-path="url(#c1)" transform="translate(1,2)" style="{DV};font-size:10px" x="0" y="0">a</text><text id="b" clip-path="url(#c2)" transform="translate(3,4)" style="{DV};font-size:10px" x="0" y="0">b</text><text id="c" style="{DV};font-size:10px" x="0" y="0">c</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (a, b, c) = (id(&d, "a"), id(&d, "b"), id(&d, "c"));
    apply_clip_unions(&mut d, &[ClipUnion { target: a, others: vec![b] }]);
    let cp = d.attr(a, "clip-path").unwrap().to_string();
    assert_ne!(cp, "url(#c1)");
    let dc = d.by_id(cp.trim_start_matches("url(#").trim_end_matches(')')).expect("new clipPath");
    assert_eq!(d.tag(dc), "clipPath");
    let kids: Vec<NodeId> = d.children(dc).filter(|&n| d.is_element(n)).collect();
    assert_eq!(kids.len(), 2);
    assert_eq!(d.tag(kids[0]), "rect");
    assert_eq!(d.tag(kids[1]), "g");
    assert_eq!(d.attr(kids[1], "transform"), Some("translate(2,2)"), "(a)⁻¹ · (b)");
    let inner: Vec<NodeId> = d.children(kids[1]).filter(|&n| d.is_element(n)).collect();
    assert_eq!(d.attr(inner[0], "x"), Some("10"));
    assert_eq!(d.children(id(&d, "c1")).filter(|&n| d.is_element(n)).count(), 1, "original untouched");
    // any unclipped participant → the target loses its clip
    apply_clip_unions(&mut d, &[ClipUnion { target: b, others: vec![c] }]);
    assert_eq!(d.attr(b, "clip-path"), None);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test text_write 2>&1 | tail -5`: items missing from `text::write`.

- [ ] **Step 3: Implement**

Replace `src/text/write.rs` with:

```rust
//! Stage 12 — the only place the text pipeline writes the DOM (spec §A.0 decision 1):
//! `write_clean_text` regenerates one element from its model (P:2385–2444 + P:3456–3503, and
//! P:1189–1256 for split-offs); `apply_clip_unions` performs the clip-path unions merges asked for
//! (RK:640–656).

use std::collections::HashMap;

use crate::dom::{Doc, NodeId};
use crate::geom::{fmt_transform, inverse, is_identity};
use crate::num;
use crate::style::{Style, default_value};

use super::layout::chunk_utfs;
use super::parse::{Origin, ParsedText, TChar, XY_TOL};
use super::style::Anchor;
use super::table::CharTable;

/// Elements whose `clip-path`s must be unioned onto `target` because their text was merged into it
/// (RK:640–656). Executed by `apply_clip_unions` before the elements are rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipUnion {
    pub target: NodeId,
    pub others: Vec<NodeId>,
}

/// Where a split-off element goes once its source has been rewritten (after the replacement) or
/// removed (after the node that preceded it, or first in its parent).
#[derive(Debug, Clone, Copy)]
pub enum Slot {
    After(NodeId),
    FirstIn(NodeId),
}

fn align_of(a: Anchor) -> &'static str {
    match a {
        Anchor::Start => "start",
        Anchor::Middle => "center",
        Anchor::End => "end",
    }
}

/// C:206–221 (`set_cspecified_style`): the local declarations that make a fresh child's specified
/// style equal `desired` when it inherits `inherited` — every declaration of `desired` that differs
/// or is missing, plus the initial value of every inherited property `desired` does not mention
/// (skipped when no initial value is known).
pub fn specified_diff(desired: &Style, inherited: &Style) -> Style {
    let mut out = Style::default();
    for (k, v) in &desired.0 {
        if inherited.get(k) != Some(v.as_str()) {
            out.set(k, v);
        }
    }
    for (k, _) in &inherited.0 {
        if desired.get(k).is_none() {
            if let Some(d) = default_value(k) {
                out.set(k, d);
            }
        }
    }
    out
}

/// `dx`/`dy` lists: trailing zeros trimmed, attribute omitted when nothing is left (P:3462–3463).
fn set_list(doc: &mut Doc, n: NodeId, name: &str, vals: impl Iterator<Item = f64>) {
    let mut v: Vec<f64> = vals.collect();
    while v.last() == Some(&0.0) {
        v.pop();
    }
    if v.is_empty() {
        doc.remove_attr(n, name);
    } else {
        let s: Vec<String> = v.iter().map(|x| num::fmt(*x)).collect();
        doc.set_attr(n, name, s.join(" "));
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

const SODIPODI_NS: &str = "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd";

/// `sodipodi:role` needs its namespace declared: matplotlib/svglite documents do not carry it
/// (inkex declares it implicitly through lxml's nsmap).
fn ensure_sodipodi_ns(doc: &mut Doc) {
    let svg = doc.svg();
    if doc.attr(svg, "xmlns:sodipodi").is_none() {
        doc.set_attr(svg, "xmlns:sodipodi", SODIPODI_NS);
    }
}

/// P:3456–3503 (`make_tspan`): one `<tspan>` for a chunk, with nested tspans per style run.
fn make_tspan(doc: &mut Doc, pt: &ParsedText, li: usize, ci: usize, te: NodeId) {
    let ln = &pt.lines[li];
    let ch = &ln.chunks[ci];
    let ts = doc.new_element("tspan");
    doc.append_child(te, ts);
    doc.set_attr(ts, "x", num::fmt(ch.x));
    doc.set_attr(ts, "y", num::fmt(ch.y));
    let cs: Vec<&TChar> = ch.chars.iter().map(|&c| &pt.chars[c]).collect();
    set_list(doc, ts, "dx", cs.iter().map(|c| c.dx));
    set_list(doc, ts, "dy", cs.iter().map(|c| c.dy));
    let utfs = chunk_utfs(pt, li, ci);
    let mut bounds = vec![0usize];
    for i in 1..cs.len() {
        if !super::edit::style_eq(&cs[i].sty, &cs[i - 1].sty)
            || cs[i].utfs != cs[i - 1].utfs
            || cs[i].bshft != cs[i - 1].bshft
        {
            bounds.push(i);
        }
    }
    bounds.push(cs.len());
    let nested = bounds.len() > 2 || cs[0].bshft.abs() > XY_TOL;
    let chars: Vec<char> = cs.iter().map(|c| c.c).collect();
    let inherited = doc.specified_style(ts);
    let mut st = specified_diff(&cs[0].sty, &inherited);
    st.set("font-size", &num::fmt(utfs));
    st.set("text-align", align_of(ln.spec.anchor));
    st.set("text-anchor", ln.spec.anchor.css());
    for k in ["line-height", "direction", "baseline-shift", "shape-inside"] {
        st.remove(k);
    }
    doc.set_style_map(ts, &st);
    if !nested {
        let t = doc.new_text(&chars.iter().collect::<String>());
        doc.append_child(ts, t);
        return;
    }
    let ts_spec = doc.specified_style(ts);
    for w in bounds.windows(2) {
        let tsn = doc.new_element("tspan");
        doc.append_child(ts, tsn);
        let t = doc.new_text(&chars[w[0]..w[1]].iter().collect::<String>());
        doc.append_child(tsn, t);
        let c = cs[w[0]];
        let mut ns = specified_diff(&c.sty, &ts_spec);
        if (c.utfs - utfs).abs() > 0.001 {
            ns.set("font-size", &format!("{}%", num::fmt(round3(c.utfs / utfs * 100.0))));
        } else {
            ns.remove("font-size");
        }
        if c.bshft.abs() > 0.001 {
            let r = c.bshft / utfs;
            let v = if (r - 0.4).abs() < 0.001 {
                "super".to_string()
            } else if (r + 0.2).abs() < 0.001 {
                "sub".to_string()
            } else {
                format!("{}%", num::fmt(round3(r * 100.0)))
            };
            ns.set("baseline-shift", &v);
        } else {
            ns.remove("baseline-shift");
        }
        ns.0.retain(|(k, v)| ts_spec.get(k) != Some(v.as_str()));
        for k in ["text-align", "text-anchor", "direction", "shape-inside"] {
            ns.remove(k);
        }
        doc.set_style_map(tsn, &ns);
    }
}

/// P:2385–2444 (`make_clean_textelement`), P:1189–1256 for split-offs. Returns the new element,
/// or `None` when the model has no characters (an existing element is then removed).
pub fn write_clean_text(
    doc: &mut Doc,
    pt: &ParsedText,
    ct: &CharTable,
    replaced: &mut HashMap<NodeId, Slot>,
) -> Option<NodeId> {
    let old = pt.el;
    if pt.chars.is_empty() {
        if pt.origin == Origin::Existing {
            if let Some(parent) = doc.parent(old) {
                let slot = match doc.prev_sibling(old) {
                    Some(p) => Slot::After(p),
                    None => Slot::FirstIn(parent),
                };
                replaced.insert(old, slot);
                doc.detach(old);
            }
        }
        return None;
    }
    let slot = match pt.origin {
        Origin::Existing => Slot::After(old),
        Origin::SplitFrom => replaced.get(&old).copied().unwrap_or(Slot::After(old)),
    };
    let te = doc.new_element("text");
    // upstream's five exclusions plus the per-character lists (see the task description)
    const SKIP: [&str; 10] = [
        "baseline-shift",
        "shape-inside",
        "direction",
        "style",
        "font-family",
        "x",
        "y",
        "dx",
        "dy",
        "rotate",
    ];
    for a in doc.attrs(old).to_vec() {
        if SKIP.contains(&a.name.as_str()) || a.name == "id" {
            continue;
        }
        if pt.text_length_removed && matches!(a.name.as_str(), "textLength" | "lengthAdjust") {
            continue;
        }
        doc.set_attr(te, &a.name, a.value);
    }
    let mut style = doc.attr(old, "style").map(Style::parse).unwrap_or_default();
    for k in ["baseline-shift", "shape-inside", "direction"] {
        style.remove(k);
    }
    let first = &pt.chars[0];
    let fam = first
        .face
        .map(|f| ct.fonts.face_info(f).family.clone())
        .or_else(|| first.spec.families.first().cloned())
        .unwrap_or_else(|| "sans-serif".to_string());
    style.set("font-family", &format!("'{fam}'"));
    if let Some(a) = pt.text_anchor_override {
        style.set("text-anchor", a.css());
        style.set("text-align", align_of(a));
    }
    if !is_identity(pt.transform_extra) {
        match fmt_transform(doc.transform(old) * pt.transform_extra) {
            Some(t) => doc.set_attr(te, "transform", t),
            None => {
                doc.remove_attr(te, "transform");
            }
        }
    }
    doc.set_attr(te, "xml:space", "preserve");
    match slot {
        Slot::After(a) => doc.insert_after(te, a),
        Slot::FirstIn(p) => doc.prepend_child(p, te),
    }
    match pt.origin {
        Origin::Existing => {
            let id = doc.attr(old, "id").map(str::to_string);
            doc.detach(old);
            match id {
                Some(id) => doc.set_attr(te, "id", id),
                None => {
                    doc.ensure_id(te);
                }
            }
            replaced.insert(old, Slot::After(te));
        }
        Origin::SplitFrom => {
            doc.ensure_id(te);
        }
    }
    doc.set_style_map(te, &style);

    let (mut xs, mut ys, mut fszs) = (Vec::new(), Vec::new(), Vec::new());
    for (li, ci) in pt.chunks() {
        let ch = pt.chunk(li, ci);
        if ch.y.is_nan() {
            continue;
        }
        xs.push(ch.x);
        ys.push(ch.y);
        fszs.push(chunk_utfs(pt, li, ci));
        make_tspan(doc, pt, li, ci, te);
    }
    // sodipodi:role="line" when Inkscape's own re-layout would reproduce these positions (P:2427–2437)
    if !xs.is_empty() {
        let tefsz = fszs.iter().copied().fold(f64::INFINITY, f64::min);
        let step = |i: usize| (ys[i + 1] - ys[i]) / fszs[i + 1].max(tefsz);
        let same_x = xs.iter().all(|x| (x - xs[0]).abs() < 0.001);
        let same_step = (0..ys.len().saturating_sub(1)).all(|i| (step(i) - step(0)).abs() < 0.001);
        if same_x && same_step {
            let lh = if ys.len() > 1 { step(0) } else { 1.25 };
            ensure_sodipodi_ns(doc);
            doc.set_style(te, "font-size", &num::fmt(tefsz));
            doc.set_style(te, "line-height", &num::fmt(lh));
            doc.set_attr(te, "x", num::fmt(xs[0]));
            doc.set_attr(te, "y", num::fmt(ys[0]));
            let kids: Vec<NodeId> = doc.children(te).filter(|&k| doc.is_element(k)).collect();
            for k in kids {
                doc.set_attr(k, "sodipodi:role", "line");
            }
        }
    }
    Some(te)
}

/// The element a `clip-path` (attribute, else style) points at.
fn clip_of(doc: &Doc, el: NodeId) -> Option<NodeId> {
    let v = doc
        .attr(el, "clip-path")
        .map(str::to_string)
        .or_else(|| doc.specified(el, "clip-path"))?;
    let id = v.trim().strip_prefix("url(#")?.strip_suffix(')')?.trim();
    doc.by_id(id)
}

/// RK:640–656: when merged elements carry different clips, the target gets a duplicate of its own
/// clip with every other participant's clip contents appended as a `<g>` transformed by
/// `(target ctm)⁻¹ · (other ctm)`; if any participant is unclipped, the target's clip is dropped.
pub fn apply_clip_unions(doc: &mut Doc, unions: &[ClipUnion]) {
    for u in unions {
        let mut els = vec![u.target];
        for &o in &u.others {
            if !els.contains(&o) {
                els.push(o);
            }
        }
        if els.len() < 2 {
            continue;
        }
        let clips: Vec<Option<NodeId>> = els.iter().map(|&e| clip_of(doc, e)).collect();
        let Some(all): Option<Vec<NodeId>> = clips.into_iter().collect() else {
            doc.remove_attr(u.target, "clip-path");
            doc.remove_style(u.target, "clip-path");
            continue;
        };
        let Some(inv) = inverse(doc.composed_transform(u.target)) else {
            continue;
        };
        let dc = doc.deep_clone(all[0]);
        doc.insert_after(dc, all[0]);
        let dcid = doc.ensure_id(dc);
        for (i, &e) in els.iter().enumerate().skip(1) {
            let ng = doc.new_element("g");
            let kids: Vec<NodeId> = doc.children(all[i]).collect();
            for k in kids {
                let kc = doc.deep_clone(k);
                doc.append_child(ng, kc);
            }
            if let Some(t) = fmt_transform(inv * doc.composed_transform(e)) {
                doc.set_attr(ng, "transform", t);
            }
            doc.append_child(dc, ng);
        }
        doc.set_attr(u.target, "clip-path", format!("url(#{dcid})"));
        doc.remove_style(u.target, "clip-path");
    }
}
```

`Attr` must be `Clone` (it is used by value in `to_vec()`; add `#[derive(Clone)]` in `src/dom.rs` if missing). `Doc::set_attr`'s value parameter is `impl Into<String>`.

- [ ] **Step 4: Run, fmt, clippy, full suite** — `cargo test --test text_write 2>&1 | tail -15` → 5 passed.

- [ ] **Step 5: Commit**

```bash
git add src/text/write.rs src/dom.rs tests/text_write.rs
git commit -m "feat(text): clean writer — regenerate <text> from the model with nested style runs, role=line detection, clip unions (P:2385–2444, 3456–3503)"
```

---

### Task 11: `remove_kerning`, the `text-fix` tool, end-to-end oracles, docs

**Files:**
- Modify: `src/text/kerning.rs` (append `KerningOptions`, `remove_kerning`)
- Create: `src/tools/text_fix.rs`, `inx/text_fix.inx`, `tests/text_fix.rs`, `tests/text_fixtures.rs`
- Modify: `src/tools/mod.rs`, `src/cli.rs`, `src/lib.rs`, `tests/support/mod.rs`, `README.md`, `docs/spec/01-text-engine.md`

**Interfaces:**
- Produces:
  - `pub struct KerningOptions { pub remove_manual: bool, pub merge_supersub: bool, pub split_distant: bool, pub merge_nearby: bool, pub justification: Option<Anchor> }` with `pub fn from_inx(removemanualkerning: bool, mergesubsuper: bool, splitdistant: bool, mergenearby: bool, justification: u8) -> KerningOptions` (F:398: `1 → middle, 2 → start, 3 → end, anything else → None`).
  - `pub fn remove_kerning(doc: &mut Doc, els: &[NodeId], o: &KerningOptions, fonts: FontSystem, warn: &mut Warnings) -> Vec<NodeId>` — RK:64–119; returns `els` with every rewritten element replaced by its new node, removed elements dropped, and the split-off elements appended (upstream returns the stale handles plus the new ones).
  - `sciink --tool=text-fix --removemanualkerning=<bool> --mergesubsuper=<bool> --splitdistant=<bool> --mergenearby=<bool> --justification=<1..4> --id=…` (Scientific ▸ Debug ▸ Text Fix); message `text-fix: N text elements in, M out` plus warnings.
  - `tests/support/mod.rs`: `pub fn text_positions(svg: &str) -> Vec<(char, f64, f64)>` (transformed bottom-left of every non-space character of every `<text>`, vendored fonts, sorted) and `pub fn assert_same_positions(before, after, tol, context)` — the appearance-invariance oracle.

- [ ] **Step 1: Write the failing tests**

Append to `tests/support/mod.rs`:

```rust
/// Transformed bottom-left corner of every non-space character of every `<text>` in `svg`, laid
/// out with the vendored fonts and sorted — the appearance-invariance oracle for the text pipeline:
/// a stage that only re-encodes text must leave this list unchanged.
pub fn text_positions(svg: &str) -> Vec<(char, f64, f64)> {
    use sciink::text::layout::{chunk_char_pts, transform_pts};
    use sciink::text::parse::ParsedText;
    use sciink::text::table::CharTable;
    use sciink::text::{Warnings, fonts::FontSystem};
    let mut d = sciink::dom::Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<_> = d
        .descendants(d.svg())
        .filter(|&n| d.is_element(n) && d.tag(n) == "text")
        .collect();
    let mut w = Warnings::default();
    let fonts = FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")]);
    let mut ct = CharTable::build(&d, &els, fonts, &mut w);
    let mut out = Vec::new();
    for el in els {
        let Some(pt) = ParsedText::parse(&mut d, el, &mut ct, &mut w) else {
            continue;
        };
        for (li, ln) in pt.lines.iter().enumerate() {
            for ci in 0..ln.chunks.len() {
                let ps = chunk_char_pts(&pt, li, ci);
                for (wi, &c) in ln.chunks[ci].chars.iter().enumerate() {
                    let ch = pt.chars[c].c;
                    if ch == ' ' {
                        continue;
                    }
                    let p = transform_pts(pt.transform, ps[wi])[0];
                    out.push((ch, p.x, p.y));
                }
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

pub fn assert_same_positions(before: &[(char, f64, f64)], after: &[(char, f64, f64)], tol: f64, context: &str) {
    assert_eq!(before.len(), after.len(), "{context}: character count changed\n{before:?}\n{after:?}");
    for (b, a) in before.iter().zip(after) {
        assert!(
            b.0 == a.0 && (b.1 - a.1).abs() < tol && (b.2 - a.2).abs() < tol,
            "{context}: {b:?} moved to {a:?}"
        );
    }
}
```

Create `tests/text_fix.rs`:

```rust
mod support;

use std::ffi::OsString;
use std::path::PathBuf;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink").chain(v.iter().copied()).map(OsString::from).collect()
}
fn fontdir() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts").display().to_string()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

static INIT: std::sync::Once = std::sync::Once::new();
fn with_vendored_fonts<T>(f: impl FnOnce() -> T) -> T {
    INIT.call_once(|| {
        // SAFETY: runs once, before any test in this binary reads the environment (every test
        // enters through this function); the values never change afterwards.
        unsafe {
            std::env::set_var("SCIINK_NO_SYSTEM_FONTS", "1");
            std::env::set_var("SCIINK_FONT_DIRS", fontdir());
        }
    });
    f()
}
fn run_fix(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=text-fix"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes())).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}
/// Left edge of each character and the total advance of `text` (DejaVu Sans 10 px, one chunk at 0).
fn layout(text: &str) -> (Vec<f64>, f64) {
    use sciink::text::{Warnings, fonts::FontSystem, layout::chunk_geom, parse::ParsedText, table::CharTable};
    let svg = format!(r#"<svg {NS}><text id="p" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">{text}</text></svg>"#);
    let mut d = sciink::dom::Doc::parse(svg.as_bytes()).unwrap();
    let n = d.by_id("p").unwrap();
    let mut w = Warnings::default();
    let fonts = FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")]);
    let mut ct = CharTable::build(&d, &[n], fonts, &mut w);
    let pt = ParsedText::parse(&mut d, n, &mut ct, &mut w).unwrap();
    let g = chunk_geom(&pt, 0, 0);
    let n = g.left.len();
    (g.left.clone(), g.right[n - 1])
}
fn texts(svg: &str) -> Vec<(Option<String>, String)> {
    let d = roxmltree::Document::parse(svg).unwrap();
    d.descendants()
        .filter(|n| n.has_tag_name("text"))
        .map(|n| {
            let s: String = n.descendants().filter(|c| c.is_text()).filter_map(|c| c.text()).collect();
            (n.attribute("id").map(str::to_string), s)
        })
        .collect()
}

#[test]
fn text_fix_merges_words_splits_ticks_and_keeps_every_glyph_in_place() {
    let (_, hello_w) = layout("Hello");
    let (sp, _) = layout(" a");
    let x2 = hello_w + sp[1];
    let svg = format!(
        r#"<svg {NS}><g id="layer1"><text id="w1" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="w2" xml:space="preserve" style="{DV};font-size:10px" x="{x2}" y="0">world</text><text id="ticks" xml:space="preserve" style="{DV};font-size:10px" x="0" y="30">0 1 2</text><text id="k" xml:space="preserve" style="{DV};font-size:10px" x="0" y="60" dx="0 -1 1 0">kern </text></g></svg>"#
    );
    let before = support::text_positions(&svg);
    let (out, msgs) = run_fix(&svg, &["--justification=2", "--id=layer1"]);
    assert!(msgs[0].starts_with("text-fix: 4 text elements in, 5 out"), "{msgs:?}");
    let after = support::text_positions(&out);
    // words and ticks (y < 50) are only re-encoded: exact invariance
    let keep = |v: &[(char, f64, f64)]| v.iter().copied().filter(|p| p.2 < 50.0).collect::<Vec<_>>();
    support::assert_same_positions(&keep(&before), &keep(&after), 1e-3, "text-fix");
    // "kern " had manual kerning: removing it moves e/r/n to the font's natural advances (that is
    // the feature), so compare against a fresh layout of "kern" instead of the input
    let (natural, _) = layout("kern");
    let mut kern: Vec<(char, f64, f64)> = after.iter().copied().filter(|p| p.2 > 50.0).collect();
    kern.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    assert_eq!(kern.iter().map(|p| p.0).collect::<String>(), "kern");
    for (p, x) in kern.iter().zip(&natural) {
        assert!((p.1 - x).abs() < 1e-3 && (p.2 - 60.0).abs() < 1e-3, "{p:?} vs natural x {x}");
    }
    let mut t: Vec<String> = texts(&out).into_iter().map(|(_, s)| s).collect();
    t.sort();
    assert_eq!(t, ["0", "1", "2", "Hello world", "kern"], "{out}");
    let ids: Vec<Option<String>> = texts(&out).into_iter().map(|(i, _)| i).collect();
    assert!(ids.contains(&Some("w1".into())) && ids.contains(&Some("ticks".into())) && ids.contains(&Some("k".into())));
    assert!(!ids.contains(&Some("w2".into())), "merged away");
    assert_eq!(ids.iter().filter(|i| i.as_deref().is_some_and(|s| s.starts_with("sciink-"))).count(), 2);
    assert!(!out.contains(" dx="), "manual kerning gone:\n{out}");
    assert!(out.matches("sodipodi:role=\"line\"").count() >= 5);
    assert!(out.contains("xml:space=\"preserve\""));
    // no selection → the document comes back unchanged with a message
    let err = with_vendored_fonts(|| sciink::run(&args(&["--tool=text-fix", "--id=nope"]), svg.as_bytes()));
    assert!(err.is_err());
}

#[test]
fn text_fix_on_a_real_inkscape_document_is_appearance_invariant() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/corpus/Simple_text.svg");
    let svg = std::fs::read_to_string(path).unwrap();
    let root_id = roxmltree::Document::parse(&svg).unwrap().root_element().attribute("id").unwrap().to_string();
    let before = support::text_positions(&svg);
    assert!(!before.is_empty());
    // without merges and manual-kerning removal every remaining stage is exact
    let (out, _) = run_fix(
        &svg,
        &["--mergenearby=false", "--mergesubsuper=false", "--removemanualkerning=false", "--justification=4", &format!("--id={root_id}")],
    );
    support::assert_same_positions(&before, &support::text_positions(&out), 1e-3, "no-merge run");
    // with merges glyphs may snap to whole spaces, but none may appear or vanish
    let (out, _) = run_fix(&svg, &[&format!("--id={root_id}")]);
    let mut b: Vec<char> = before.iter().map(|p| p.0).collect();
    let mut a: Vec<char> = support::text_positions(&out).iter().map(|p| p.0).collect();
    b.sort();
    a.sort();
    assert_eq!(a, b);
}
```

Create `tests/text_fixtures.rs` (system fonts; never sets `SCIINK_NO_SYSTEM_FONTS`):

```rust
mod support;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

/// Whitespace-collapsed text of every <text> under the element with `layer_id`.
fn layer_texts(svg: &str, layer_id: &str) -> Vec<String> {
    let d = roxmltree::Document::parse(svg).unwrap();
    let Some(layer) = d.descendants().find(|n| n.attribute("id") == Some(layer_id)) else {
        return Vec::new();
    };
    layer
        .descendants()
        .filter(|n| n.has_tag_name("text"))
        .map(|n| {
            let s: String = n.descendants().filter(|c| c.is_text()).filter_map(|c| c.text()).collect();
            s.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Content parity with upstream's Flattener reference for Text_tests.svg: the multiset of text
/// strings in the processed layer. Positions are not compared (that reference was produced by an
/// older writer and with fonts this machine may lack), so only merge/split DECISIONS are checked.
/// Run: `SCIINK_SYSTEM_FONTS=1 cargo test --test text_fixtures -- --ignored --nocapture`.
#[test]
#[ignore]
fn text_tests_content_matches_the_upstream_reference() {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        eprintln!("SKIP: set SCIINK_SYSTEM_FONTS=1 to run against the installed fonts");
        return;
    }
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    // vendored DejaVu Sans on top of the system fonts (the fixture uses it 96 times)
    // SAFETY: single test in this binary; set before any FontSystem::load().
    unsafe {
        std::env::set_var(
            "SCIINK_FONT_DIRS",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts").display().to_string(),
        );
    }
    let svg = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let reference = std::fs::read_to_string(
        dir.join("refs/flatten_plots__--id__layer1__--testmode__True__Text_tests__svg.out"),
    )
    .unwrap();
    let argv: Vec<OsString> = ["sciink", "--tool=text-fix", "--justification=1", "--id=layer1"]
        .iter()
        .map(OsString::from)
        .collect();
    let out = sciink::run(&argv, &svg).unwrap();
    let ours = layer_texts(std::str::from_utf8(&out.svg).unwrap(), "layer1");
    let theirs = layer_texts(&reference, "layer1"); // "Layer 1 flat" keeps id layer1 in the reference
    assert!(!theirs.is_empty() && !ours.is_empty());
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for t in &theirs {
        *counts.entry(t.as_str()).or_default() += 1;
    }
    let mut matched = 0usize;
    let mut extra: Vec<&str> = Vec::new();
    for t in &ours {
        match counts.get_mut(t.as_str()) {
            Some(c) if *c > 0 => {
                *c -= 1;
                matched += 1;
            }
            _ => extra.push(t),
        }
    }
    let missing: Vec<&str> = counts.iter().filter(|(_, c)| **c > 0).map(|(t, _)| *t).collect();
    eprintln!(
        "Text_tests: {matched}/{} reference strings matched; {} ours unmatched; missing: {missing:?}; extra: {extra:?}",
        theirs.len(),
        extra.len()
    );
    assert!(
        matched as f64 >= 0.8 * theirs.len() as f64,
        "fewer than 80 % of the reference strings reproduced ({matched}/{})",
        theirs.len()
    );
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test text_fix 2>&1 | tail -5`: `text-fix` is an unknown tool / `text_positions` missing.

- [ ] **Step 3: Implement `remove_kerning`**

Append to `src/text/kerning.rs` (add `use std::collections::HashMap; use crate::dom::NodeId; use super::Warnings; use super::edit::{make_next_chain, rechunk_absolute, remove_textlength}; use super::fonts::FontSystem; use super::layout::snapshot_parsed; use super::parse::Origin; use super::write::{Slot, apply_clip_unions, write_clean_text};`):

```rust
/// The Flattener's text options (F:178–184, gated by `fixtext` there; F:398 justification map).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KerningOptions {
    pub remove_manual: bool,
    pub merge_supersub: bool,
    pub split_distant: bool,
    pub merge_nearby: bool,
    pub justification: Option<Anchor>,
}

impl KerningOptions {
    /// `.inx` values: `justification` 1 = middle, 2 = start, 3 = end, 4 = unchanged.
    pub fn from_inx(
        removemanualkerning: bool,
        mergesubsuper: bool,
        splitdistant: bool,
        mergenearby: bool,
        justification: u8,
    ) -> KerningOptions {
        KerningOptions {
            remove_manual: removemanualkerning,
            merge_supersub: mergesubsuper,
            split_distant: splitdistant,
            merge_nearby: mergenearby,
            justification: match justification {
                1 => Some(Anchor::Middle),
                2 => Some(Anchor::Start),
                3 => Some(Anchor::End),
                _ => None,
            },
        }
    }
}

/// RK:64–119 (`remove_kerning`): the whole pipeline over the `<text>` elements of `els`
/// (`<flowRoot>`s only feed the char table). Stages 6–7 decide on parsed positions, 8–11 on
/// current ones (RK:96–97); the DOM is written once at the end. Returns `els` with rewritten
/// elements replaced by their new nodes, removed ones dropped, split-offs appended.
pub fn remove_kerning(
    doc: &mut Doc,
    els: &[NodeId],
    o: &KerningOptions,
    fonts: FontSystem,
    warn: &mut Warnings,
) -> Vec<NodeId> {
    let tels: Vec<NodeId> = els
        .iter()
        .copied()
        .filter(|&e| doc.is_element(e) && matches!(doc.tag(e), "text" | "flowRoot"))
        .collect();
    if tels.is_empty() {
        return els.to_vec();
    }
    let mut ct = CharTable::build(doc, &tels, fonts, warn);
    let mut pts: Vec<ParsedText> = Vec::new();
    for &el in &tels {
        if doc.tag(el) != "text" {
            continue;
        }
        if let Some(pt) = ParsedText::parse(doc, el, &mut ct, warn) {
            if !pt.is_flow {
                pts.push(pt);
            }
        }
    }
    if o.remove_manual {
        for pt in pts.iter_mut() {
            remove_textlength(pt); // before the snapshot: it may change the transform (RK:84–86)
        }
    }
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
        make_next_chain(doc, pt);
    }
    let mut clips: Vec<ClipUnion> = Vec::new();
    if o.remove_manual {
        for pt in pts.iter_mut() {
            rechunk_absolute(pt);
            make_next_chain(doc, pt);
        }
        remove_manual_kerning(doc, &mut pts, &mut ct, &mut clips);
    }
    if o.merge_nearby || o.merge_supersub {
        external_merges(doc, &mut pts, &mut ct, o.merge_nearby, o.merge_supersub, &mut clips);
    }
    if o.split_distant {
        split_distant_chunks(&mut pts);
        split_distant_intrachunk(&mut pts);
        split_lines(&mut pts);
    }
    change_justification(&mut pts, o.justification);
    let removed = remove_trailing_leading_spaces(&mut pts);
    if o.remove_manual || o.merge_nearby || o.merge_supersub || removed {
        fix_merge_positions(&mut pts);
    }
    apply_clip_unions(doc, &clips);
    let mut replaced: HashMap<NodeId, Slot> = HashMap::new();
    let mut new_of: HashMap<NodeId, Option<NodeId>> = HashMap::new();
    let mut extra: Vec<NodeId> = Vec::new();
    for pt in &pts {
        let n = write_clean_text(doc, pt, &ct, &mut replaced);
        match pt.origin {
            Origin::Existing => {
                new_of.insert(pt.el, n);
            }
            Origin::SplitFrom => extra.extend(n),
        }
    }
    let mut out: Vec<NodeId> = Vec::new();
    for &e in els {
        match new_of.get(&e) {
            Some(Some(n)) => out.push(*n),
            Some(None) => {}
            None => out.push(e),
        }
    }
    out.extend(extra);
    out
}
```

- [ ] **Step 4: The `text-fix` tool**

Create `src/tools/text_fix.rs`:

```rust
//! Debug tool: run the Flattener's text pipeline (spec §A.1 stages 2–12) on the selected text, with
//! the Flattener's own option names, so the pipeline can be exercised from Inkscape before the
//! Flattener exists.

use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::Doc;
use crate::text::Warnings;
use crate::text::fonts::FontSystem;
use crate::text::kerning::{KerningOptions, remove_kerning};

use super::font_probe::text_elements;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct TextFixCli {
    #[command(flatten)]
    pub common: Common,
    #[arg(long, value_parser = inx_bool, default_value = "true")]
    pub removemanualkerning: bool,
    #[arg(long, value_parser = inx_bool, default_value = "true")]
    pub mergesubsuper: bool,
    #[arg(long, value_parser = inx_bool, default_value = "true")]
    pub splitdistant: bool,
    #[arg(long, value_parser = inx_bool, default_value = "true")]
    pub mergenearby: bool,
    /// 1 = centre, 2 = left, 3 = right, 4 = unchanged (upstream's optiongroup values).
    #[arg(long, default_value_t = 1)]
    pub justification: u8,
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = TextFixCli::try_parse_from(argv).map_err(|e| {
        e.to_string()
            .lines()
            .next()
            .unwrap_or("invalid arguments")
            .to_string()
    })?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let els = text_elements(&doc, &cli.common.ids);
    if els.is_empty() {
        return Err("select at least one text element (or a group containing text)".to_string());
    }
    let opts = KerningOptions::from_inx(
        cli.removemanualkerning,
        cli.mergesubsuper,
        cli.splitdistant,
        cli.mergenearby,
        cli.justification,
    );
    let mut warn = Warnings::default();
    let out = remove_kerning(&mut doc, &els, &opts, FontSystem::load(), &mut warn);
    let mut messages = vec![format!(
        "text-fix: {} text elements in, {} out",
        els.len(),
        out.len()
    )];
    messages.extend(warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
```

Register: `src/tools/mod.rs` → `pub mod text_fix;`; `src/cli.rs` → `TextFix` variant in `ToolName` and the line `Debug tools (Scientific ▸ Debug): font-probe, text-highlight, text-fix.` appended to `HELP`; `src/lib.rs` → `"text-fix" => tools::text_fix::run(argv, input),`.

Create `inx/text_fix.inx`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Text Fix (sciink)</name>
    <id>org.sciink.text-fix</id>
    <param name="tool" type="string" gui-hidden="true">text-fix</param>
    <param name="removemanualkerning" type="bool" gui-text="Remove manual kerning">true</param>
    <param name="mergenearby" type="bool" gui-text="Merge nearby text">true</param>
    <param name="mergesubsuper" type="bool" gui-text="Merge sub/superscripts">true</param>
    <param name="splitdistant" type="bool" gui-text="Split distant text">true</param>
    <param name="justification" type="optiongroup" appearance="combo" gui-text="Justification">
        <option value="1">Center</option>
        <option value="2">Left</option>
        <option value="3">Right</option>
        <option value="4">Unchanged</option>
    </param>
    <effect needs-live-preview="true">
        <object-type>all</object-type>
        <effects-menu>
            <submenu name="Scientific">
                <submenu name="Debug"/>
            </submenu>
        </effects-menu>
    </effect>
    <script>
        <command location="inx">bin/sciink</command>
    </script>
</inkscape-extension>
```

- [ ] **Step 5: Run everything**

`cargo test --test text_fix --test text_fixtures 2>&1 | tail -15` → 2 passed, 1 ignored. Then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"` — all green. Then the two dev-machine checks (record the numbers in the task report; they are not CI tests):

```bash
SCIINK_SYSTEM_FONTS=1 SCIINK_UPSTREAM_TESTS=$HOME/Downloads/Scientific-Inkscape-dev/tests/data cargo test --test text_fixtures -- --ignored --nocapture 2>&1 | tail -8
```

```bash
cargo build --release && dist/dev-install.sh && /Applications/Inkscape.app/Contents/MacOS/inkscape --actions="select-all;org.sciink.text-fix.noprefs;export-type:svg;export-filename:/tmp/text-fix-out.svg;export-do" tests/data/corpus/Simple_text.svg 2>&1 | tail -3 && grep -c 'sodipodi:role="line"' /tmp/text-fix-out.svg
```

- [ ] **Step 6: Documentation**

`README.md`: add `Text Fix` to the Scientific ▸ Debug list ("runs the Flattener's text pipeline — manual-kerning removal, merges, splits, justification — on the selection; what the Flattener will do to text once it ships").

`docs/spec/01-text-engine.md`:
- Stage 5 paragraph: replace "Rebuild x/y lists, re-chunk (each dx'd char is now its own chunk, same representation as PDF-import x arrays)" with "Rebuild x/y lists; a coordinate that follows a character without one opens a new **line** (P:886–893), any coordinate opens a new chunk (P:2704–2716); a new line without x continues from the end of the previous line, without y takes the previous line's last y".
- "Deliberate defensive deviations" — append these bullets:
  - `continue_x`/`continue_y` and stage-5 continuation coordinates are resolved once when the model is built (upstream recomputes them on every access).
  - `change_alignment` measures every chunk with the old anchor before moving any (upstream mutates the anchor inside its per-chunk loop, P:2785–2787).
  - Merged sub/superscript characters get `bshft = ±0.4/−0.2·host utfs` and `utfs = 0.65·host utfs` in the model at merge time (upstream leaves the model stale until Inkscape re-renders).
  - Inserted spaces carry no parsed points (upstream copies the neighbour's); `split_off` always shifts the chunk anchor (upstream skips chunks without an own `x` entry).
  - Split-off elements are written in natural document order after their source; emptied elements are removed; `remove_kerning` returns live node ids instead of upstream's stale handles.
  - Stage 5 positions the first character of every chunk with the anchor-weighted formula (upstream keeps the old chunk `x`, displacing middle/end-anchored first segments until stage 11).
  - The regenerated `<text>` does not inherit the old element's `x`/`y`/`dx`/`dy`/`rotate` lists (upstream copies every attribute; an ancestor list would re-apply to characters whose tspan list was trimmed); `xmlns:sodipodi` is declared on the root when `sodipodi:role` is written.
- §A.4: update the signatures to `remove_kerning(doc: &mut Doc, els: &[NodeId], o: &KerningOptions, fonts: FontSystem, warn: &mut Warnings) -> Vec<NodeId>` and `write_clean_text(doc: &mut Doc, pt: &ParsedText, ct: &CharTable, replaced: &mut HashMap<NodeId, Slot>) -> Option<NodeId>`; note `snapshot_parsed` and `get_ut_pts` live in `text::layout`, the editing primitives in `text::edit`, the stage drivers in `text::kerning`; drop `debug_highlights` from `KerningOptions` (the `text-highlight` tool covers it).

- [ ] **Step 7: Commit**

```bash
git add src/text/kerning.rs src/tools/text_fix.rs src/tools/mod.rs src/cli.rs src/lib.rs inx/text_fix.inx tests/support/mod.rs tests/text_fix.rs tests/text_fixtures.rs README.md docs/spec/01-text-engine.md
git commit -m "feat(text): remove_kerning pipeline and the text-fix debug tool; appearance-invariance and reference-content oracles; spec deviations"
```

---

## Out of scope (deferred, unchanged from the Plan 3 hand-off)

Variable-font axes, flowed text parsing (`flowRoot`/`shape-inside` are still bbox-only and never edited), `word-spacing`, tab widths, the no-copy glyph probe (Plan 3 I5(c)), the `--debugparser` Flattener switch (the `text-highlight` tool already draws the same rectangles), `Make_All_Editable`/`Final_Cleanup` (dead code in upstream's current pipeline, RK:110–113). The Flattener itself (deep ungroup, rect passes, font replacement, calling `remove_kerning`) is Plan 6, after the geometry ops of Plan 5.

## Self-review notes (controller)

- Spec coverage: stage 2 → Task 2; stage 3 → Task 1; stage 4 → Task 3; stage 5 → Task 3; stage 6 + `Perform_Merges` + `append_chks` → Tasks 4, 6; stage 7 → Task 7; stage 8 + `split_off_characters` → Tasks 5, 8; stages 9–11 → Task 9; stage 12 + clip union → Task 10; RK:64–119 orchestration + Flattener option gating → Task 11; §A.4 `text_bbox(doc, el, …)` → Task 1 `element_bbox`. Not ported on purpose: `suborsuperreturn`/`superorsubreturn` (dead with `SUBSUPER_THR = 0.99`), `debugparser`, `make_editable`, `delete_empty`, `fuse_fonts`, `flow_to_text`.
- Type consistency: `ChunkRef = (usize, u32)` everywhere; `WType` lives in `edit`, `MergeType`/`Cand` in `kerning`; `ClipUnion`/`Slot` in `write`; `get_ut_pts` returns `Option<[Point; 4]>` = `[tr1, br1, tl2, bl2]` in every caller; `remove_chars`/`reindex` return `Vec<usize>` (new → old) used by `split_off`; `Incoming { chunk, wtype, max_spaces }` constructed only by `perform_merges` and tests.
- Placeholder scan: every step carries code or an exact command; tests assert concrete values or exact invariance; the two dev-machine checks in Task 11 are labelled as such.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-20-plan4-text-engine-edit.md`. Execute with **subagent-driven development** (fresh implementer per task, task review, fix rounds with scoped re-review, one final whole-branch fix wave), on a branch `plan4-text-edit` off `main` (6dc6a8a), with the same process rules as Plan 3 (never weaken a test to make it pass; plan defects become controller rulings mirrored into this document).
