# Plan 7 — Scaler and Homogenizer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the two plot-sizing tools — **Extensions ▸ Scientific ▸ Scaler** (spec §B.3 "Scaler", upstream `scale_plots.py`: Correction mode, Matching mode, the Advanced tab's markings, tick correction, scale-free/aspect-locked/normal children, combined-by-colour ranges) and **Extensions ▸ Scientific ▸ Homogenizer** (spec §B.3 "Homogenizer", upstream `homogenizer.py`: font size modes, text-distortion fix, font family from an Inkscape font specification, re-centring incl. plot-aware, stroke width modes, transform fusing, clip/mask clearing) — plus the two library gaps they expose: a shared font scan across one tool run (Plan 6's parked F2) and the document scale `px_per_uu`.

**Architecture:** Two tool modules built on Plan 5's `ops` (`bb2`, `global_transform`, `fuse`, `strokefill`, `delete_up`) and Plan 3/4's text engine (`composed_width`, `baseline_shift`, `ParsedText::parse`). `src/tools/scaler.rs` owns the plot-area geometry (`geometric_bbox`, `find_plot_area`) that the Homogenizer's plot-aware mode reuses, exactly as upstream's `homogenizer.py` imports it from `scale_plots.py`. `FontSystem::load()` keeps its signature but serves the second call of a run from a process-wide scan cache, so `remove_kerning` and `Ctx` no longer pay two filesystem scans. `Doc::px_per_uu()` ports upstream's `document_size` (viewBox × width/height × `preserveAspectRatio`). `Doc::selection_ordered()` preserves the `--id` order the Scaler's Matching mode needs.

**Tech Stack:** Rust 2024 (rustc ≥ 1.85), kurbo 0.13, quick-xml 0.42, clap 4.6, fontdb 0.24 (`Database: Clone`), rustybuzz/ttf-parser; dev: roxmltree 0.21, resvg 0.48.1. No new dependencies.

**Spec:** `docs/spec/02-geometry-tools.md` — §B.3 "Scaler" and "Homogenizer", §B.1 (`uniquetol`, `Option<Rect>` algebra), §B.2 ("Document scale", `global_transform`, `fuse`, `composed_width`), §B.4 (`inkscape-scientific-scaletype`, `inkscape-scientific-combined-by-color`), "Deliberate deviations (Plan 5/6)"; `docs/spec/01-text-engine.md` §A.4 (Homogenizer consumers of the text API); `docs/spec/03-infrastructure.md` §C.3 (`.inx`/CLI contract), §C.5 (b)–(c) (golden and invariance oracles). Upstream references (read-only, never copied into the crate): `SP` = `scale_plots.py`, `HG` = `homogenizer.py`, `DH` = `dhelpers.py`, `FP` = `inkex1_3_0/inkex/text/font_properties.py`, `CA` = `inkex1_3_0/inkex/text/cache.py`, `TU` = `inkex1_3_0/inkex/text/utils.py`, all under `~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/scientific_inkscape/`. Upstream test data behind the symlink `tests/upstream/data/{svg,refs}` (absent in CI → fixture tests `return` early via `support::upstream_data_dir()`).

## Global Constraints

- Every transform the Scaler and the Homogenizer apply goes through `ops::xform::global_transform(doc, ctx, el, t, ranges, preserve_stroke = true)` — upstream's `dh.global_transform` always preserves the visual stroke width (that is the tool's purpose: "Otherwise, the final stroke widths may change"). Composition follows `geom`'s rule: upstream `A @ B` is kurbo `A * B`, `-A` is `inverse(A)`.
- Bounding boxes come from `ops::bbox::bb2(doc, ctx, els, rough = false)` (exact, visual: stroke, clip, root coordinates) — upstream's `BB2(svg, els)` defaults. A geometric box (`geometric_bbox`) is the visual box clamped to the global end points for `path|rect|line|polyline` and the visual box otherwise.
- `--id` order is Inkscape's selection order and is significant for the Scaler (`sel[0]` is the match target). The Scaler resolves its selection with `Doc::selection_ordered` (Task 2), never `Doc::selection` (document order).
- Constants (spec §B.3): tick threshold `tickthr = tickthreshold / 100` (default `0.1`); line detection `xrange < 0.001·h` / `yrange < 0.001·w`; rectangle detection `3 ≤ points ≤ 5` and `uniquetol(xs, 1e-3·max(xrange, yrange)) == 2` (same for `ys`); scale-equality tolerance `1e-5` for the Matching pre-pass; opaque white = `(255, 255, 255)` with `alpha == 1`; one point = `4/3 px`; `default font size when no text measures = 12 pt`.
- Compatibility attributes (spec §B.4): `inkscape-scientific-scaletype ∈ {scale_free, aspect_locked, normal, plot_area}` (Advanced tab writes it, `marksf = 5` removes it); `inkscape-scientific-combined-by-color = "s0 s1 … sk"` read as BezPath-element ranges `[s_i, s_{i+1})` of the element's `d`.
- Every number written goes through `num::fmt` (`Doc::set_transform`, `fmt_d`); the Homogenizer's `font-size` values are rounded as upstream does (2 decimals when `|v| > 1`, else 3 significant digits) BEFORE `num::fmt`.
- Hostile input never panics or hangs: singular transforms skip the element with a warning, empty paths fall back to the visual box, a plot without a box is skipped, statistics over an empty set fall back (12 pt / no stroke change) with a warning.
- Fonts load through `FontSystem::load()` exactly where they do today; Task 1 makes the second load of a process reuse the first scan. Tests wrap runs in `support::with_vendored_fonts`; oracles that need installed fonts are `#[ignore]` behind `SCIINK_SYSTEM_FONTS`.
- Tools are silent on success: `Output.messages` holds only `warning: …` lines; upstream's error dialogs (`IMAGE_ERR`, "Non-Group objects detected…", "Plot-aware scaling requires…", "Font seems to be invalid…") are `Err(String)` (Inkscape shows the text, the document is echoed unchanged).
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` pass on every commit.
- Deviations from upstream only where this plan marks them **Deviation** (with the reason); Task 9 mirrors each into `docs/spec/02-geometry-tools.md` under "Deliberate deviations (Plan 7)".

---

## File structure

| File | Responsibility |
|---|---|
| `src/text/fonts.rs` (modify) | process-wide font scan cache behind `FontSystem::load()`; `scan_count()`; `families()` |
| `src/geom/mod.rs` (modify) | `Doc::px_per_uu()` (upstream `document_size`) |
| `src/dom.rs` (modify) | `Doc::selection_ordered(ids)` |
| `src/ops/mod.rs` (modify) | `Ctx::reset_char_table()` |
| `src/tools/scaler.rs` (new) | `ScalerCli`, `Options`, `geometric_bbox`, `find_plot_area`, `scale_plot`, `run` |
| `src/tools/homogenizer.rs` (new) | `HomogenizerCli`, `inkscape_spec_to_css`, the eight phases, `run` |
| `src/tools/mod.rs`, `src/lib.rs` (modify) | modules, dispatch arms `"scaler"`, `"homogenizer"` |
| `inx/scaler.inx`, `inx/homogenizer.inx` (new) | Scientific ▸ Scaler (three pages), Scientific ▸ Homogenizer (one page) |
| `tests/text_fonts.rs`, `tests/dom.rs`, `tests/geom.rs` (append); `tests/scaler.rs`, `tests/scaler_fixtures.rs`, `tests/homogenizer.rs`, `tests/homogenizer_fixtures.rs` (new); `tests/invariance.rs` (append) | tests |
| `README.md`, `docs/spec/02-geometry-tools.md` (modify) | menu entries; "Deliberate deviations (Plan 7)" |

Test conventions (every new test file): `mod support;`, `use std::ffi::OsString;`, `use support::with_vendored_fonts;`, the `NS`/`args`/`by_id` helpers exactly as in `tests/flattener.rs:1-40`, tool runs through `sciink::run(&args(&[…]), svg.as_bytes())`, output parsed with `roxmltree::Document::parse`.

---

### Task 1: One font scan per process (`FontSystem::load()` cache) and `families()`

Plan 6's final review parked **F2**: the Flattener scans the system fonts twice per run (`remove_kerning` and the bbox stage's `Ctx` each call `FontSystem::load()`), about 0.5 s on this Mac. The Homogenizer measures text twice (before and after restyling) and would pay it again. `load()` today = filesystem scan (`load_system_fonts` + `load_fonts_dir`) + a face pass that re-reads every font file for metrics (`from_db`). Both halves depend only on the environment (`SCIINK_NO_SYSTEM_FONTS`, `SCIINK_FONT_DIRS`), so their result is cached process-wide, keyed by that environment, and later loads clone it (`fontdb::Database: Clone`, `FaceInfo: Clone`; cloning ~500 small records costs microseconds).

**Files:**
- Modify: `src/text/fonts.rs` (`load`, `from_db` split, new statics, `scan_count`, `families`)
- Test: `tests/text_fonts.rs` (append)

**Interfaces:**
- Consumes: `FontSystem::load()`, `FontSystem::from_dirs(&[PathBuf])`, `FaceInfo` (Clone).
- Produces: `pub fn scan_count() -> usize` (module-level, `sciink::text::fonts::scan_count`); `pub fn families(&self) -> Vec<String>` on `FontSystem` (unique family names, original spelling, sorted case-insensitively). `load()`/`from_dirs()` signatures unchanged.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_fonts.rs`:

```rust
#[test]
fn load_scans_the_filesystem_once_per_environment() {
    // `with_vendored_fonts` pins SCIINK_NO_SYSTEM_FONTS / SCIINK_FONT_DIRS for the whole binary
    support::with_vendored_fonts(|| {
        let a = FontSystem::load();
        let scans = sciink::text::fonts::scan_count();
        assert!(scans >= 1, "the first load scanned: {scans}");
        let b = FontSystem::load();
        assert_eq!(
            sciink::text::fonts::scan_count(),
            scans,
            "a second load with the same environment reuses the scan"
        );
        assert_eq!(a.face_count(), b.face_count());
        assert_eq!(a.face_count(), 4, "DejaVu Sans ×2 + Roboto ×2");
        assert!(b.load_ms() < a.load_ms() + 1e-9 || b.load_ms() < 50.0, "cached load is cheap");
    });
}

#[test]
fn families_lists_each_family_once_in_original_spelling() {
    let fs = fonts();
    assert_eq!(fs.families(), vec!["DejaVu Sans".to_string(), "Roboto".to_string()]);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test text_fonts 2>&1 | tail -15`
Expected: compile error — `scan_count` and `families` do not exist.

- [ ] **Step 3: Implement the cache**

In `src/text/fonts.rs` add to the imports:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
```

Add above `impl FontSystem`:

```rust
/// One filesystem scan per environment: the scan (`load_system_fonts`/`load_fonts_dir`) and the
/// face pass (`FaceInfo` per face, which re-reads every font file for its metrics) are the whole
/// cost of `load()`, and a tool run may build two font systems (`remove_kerning` and the bbox
/// stage's `Ctx`); later loads clone the first result (`Database` and `FaceInfo` are `Clone`).
type ScanKey = (bool, Vec<PathBuf>);
type Scan = (fontdb::Database, Vec<(fontdb::ID, FaceInfo)>);
static SCANS: OnceLock<Mutex<HashMap<ScanKey, Arc<Scan>>>> = OnceLock::new();
static SCAN_COUNT: AtomicUsize = AtomicUsize::new(0);

/// How many filesystem font scans this process has run (tests; About prints it).
pub fn scan_count() -> usize {
    SCAN_COUNT.load(Ordering::SeqCst)
}

fn scan_key() -> ScanKey {
    let system = std::env::var_os("SCIINK_NO_SYSTEM_FONTS").is_none_or(|v| v != "1");
    let dirs = std::env::var_os("SCIINK_FONT_DIRS")
        .map(|d| std::env::split_paths(&d).collect())
        .unwrap_or_default();
    (system, dirs)
}

/// The scan for the current environment, from the cache or freshly made (and then cached).
fn scanned() -> Arc<Scan> {
    let key = scan_key();
    let cache = SCANS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = guard.get(&key) {
        return s.clone();
    }
    SCAN_COUNT.fetch_add(1, Ordering::SeqCst);
    let mut db = fontdb::Database::new();
    if key.0 {
        db.load_system_fonts();
    }
    for d in &key.1 {
        db.load_fonts_dir(d);
    }
    let entries = FontSystem::scan_entries(&db);
    let scan = Arc::new((db, entries));
    guard.insert(key, scan.clone());
    scan
}
```

Replace `load` and `from_dirs`, and split `from_db`:

```rust
    /// System fonts (unless `SCIINK_NO_SYSTEM_FONTS=1`) plus every dir in `SCIINK_FONT_DIRS`.
    /// The filesystem is scanned once per process and environment (`scan_count`).
    pub fn load() -> FontSystem {
        let t0 = Instant::now();
        let scan = scanned();
        Self::from_entries(scan.0.clone(), scan.1.clone(), t0)
    }

    /// Only the given directories (tests); never cached.
    pub fn from_dirs(dirs: &[PathBuf]) -> FontSystem {
        let t0 = Instant::now();
        let mut db = fontdb::Database::new();
        for d in dirs {
            db.load_fonts_dir(d);
        }
        let entries = Self::scan_entries(&db);
        Self::from_entries(db, entries, t0)
    }

    /// The face pass: one `FaceInfo` per parsable face, sorted by (family, weight, style, width,
    /// path, index). This is the part of `from_db` up to and including `entries.sort_by(...)`.
    fn scan_entries(db: &fontdb::Database) -> Vec<(fontdb::ID, FaceInfo)> {
        /* move the existing body of `from_db` from `let mut entries` through `entries.sort_by(…)`
           here unchanged (it only reads `db`), then `entries` */
    }

    /// The rest of the old `from_db`: `by_family` index and the struct literal.
    fn from_entries(db: fontdb::Database, entries: Vec<(fontdb::ID, FaceInfo)>, t0: Instant) -> FontSystem {
        /* the existing code from `let mut by_family` to the end of `from_db`, unchanged */
    }
```

(The two comments describe a mechanical move of existing lines — the bodies exist in the file today; do not retype them from memory, cut and paste them.)

Add the accessor next to `face_count`:

```rust
    /// Every family name once, in its original spelling, sorted case-insensitively.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for info in &self.infos {
            if !v.iter().any(|f| f.eq_ignore_ascii_case(&info.family)) {
                v.push(info.family.clone());
            }
        }
        v.sort_by_key(|f| f.to_lowercase());
        v
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test text_fonts 2>&1 | tail -15`
Expected: all pass. Then the whole suite once: `cargo test 2>&1 | grep -E "^test result|FAILED"` — nothing else changes behaviour (`load()` returns the same faces).

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/text/fonts.rs tests/text_fonts.rs
git commit -m "perf(fonts): one filesystem font scan per process; FontSystem::families"
```

---

### Task 2: Library additions — `Doc::px_per_uu`, `Doc::selection_ordered`, `Ctx::{reset_char_table, parse_text}`

Four small pieces the tools need, ported from `CA:1182–1277` (document size) and the Scaler's selection-order requirement.

**Files:**
- Modify: `src/geom/mod.rs` (impl `Doc`, next to `viewbox`), `src/dom.rs` (next to `selection`), `src/ops/mod.rs` (impl `Ctx`)
- Test: `tests/geom.rs` (append), `tests/dom.rs` (append), `tests/ops_bbox.rs` (append)

**Interfaces:**
- Consumes: `Doc::viewbox() -> Option<Rect>`, `geom::ipx`, `Doc::attr`, `Doc::by_id`, `text::parse::ParsedText::parse(doc, el, ct, warn)`.
- Produces: `pub fn px_per_uu(&self) -> f64` (impl `Doc`, in `geom/mod.rs`); `pub fn selection_ordered(&self, ids: &[String]) -> Vec<NodeId>` (impl `Doc`, `dom.rs`); `pub fn reset_char_table(&mut self)` and `pub fn parse_text(&mut self, doc: &mut Doc, el: NodeId) -> Option<ParsedText>` (impl `Ctx`; the latter is the bbox code's private dance made public, since `Ctx.text` is private to `ops`).

Semantics of `px_per_uu` (`CA:1185–1277`): `vb` = `viewbox()` (`[0,0,w,h]` when absent; both unusable → return `1.0`); `xfr = wpx / vb.w` where `wpx = ipx(width)` (width absent → `vb.w` px → `xfr = 1`; `width` in `%` → `xfr = value/100`); `yfr` likewise from `height`; `preserveAspectRatio`: default align `xMidYMid`, `meet`; parse one or two tokens (`none`/an align token, optionally followed by `meet|slice`); align ≠ `none` → `min(xfr, yfr)` for `meet`, `max` for `slice`; align `none` → `xfr` when `|xfr − yfr| < 0.001`, else `sqrt(xfr·yfr)` (**Deviation**: upstream's `uupx` is `None` there and every unit conversion fails; the geometric mean keeps the tool running). Non-finite or non-positive results → `1.0`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/geom.rs`:

```rust
#[test]
fn px_per_uu_follows_upstream_document_size() {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-4;
    let doc = |root_attrs: &str| {
        sciink::dom::Doc::parse(
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {root_attrs}></svg>"#).as_bytes(),
        )
        .unwrap()
    };
    // Other_tests.svg: 5.5290051in × 5.3349633in over a 140.43673 × 135.50807 viewBox → 3.7795 (mm)
    let d = doc(r#"width="5.5290051in" height="5.3349633in" viewBox="0 0 140.43673 135.50807""#);
    assert!(close(d.px_per_uu(), 5.5290051 * 96.0 / 140.43673), "{}", d.px_per_uu());
    // Other_tests_nonuniform.svg: 81.709946mm × 63.921371mm over 40.904464 × 58.764816 → meet = min
    let d = doc(r#"width="81.709946mm" height="63.921371mm" viewBox="23 50 40.904464 58.764816""#);
    let xfr: f64 = 81.709946 * 96.0 / 25.4 / 40.904464; // annotated: `.min()` on an inferred float is E0689
    let yfr: f64 = 63.921371 * 96.0 / 25.4 / 58.764816;
    assert!(close(d.px_per_uu(), xfr.min(yfr)), "{} vs {}", d.px_per_uu(), xfr.min(yfr));
    // slice takes the larger factor; none with distinct factors takes the geometric mean
    let d = doc(r#"width="81.709946mm" height="63.921371mm" viewBox="23 50 40.904464 58.764816" preserveAspectRatio="xMinYMin slice""#);
    assert!(close(d.px_per_uu(), xfr.max(yfr)));
    let d = doc(r#"width="81.709946mm" height="63.921371mm" viewBox="23 50 40.904464 58.764816" preserveAspectRatio="none""#);
    assert!(close(d.px_per_uu(), (xfr * yfr).sqrt()));
    // no viewBox: the width/height ARE the viewBox → 1; no size at all → 1; percent width → value/100
    assert!(close(doc(r#"width="100mm" height="50mm""#).px_per_uu(), 1.0));
    assert!(close(doc("").px_per_uu(), 1.0));
    assert!(close(doc(r#"width="200%" height="200%" viewBox="0 0 10 10""#).px_per_uu(), 2.0));
    // matplotlib: pt sizes over a pt viewBox → 4/3
    let d = doc(r#"width="460.8pt" height="345.6pt" viewBox="0 0 460.8 345.6""#);
    assert!(close(d.px_per_uu(), 4.0 / 3.0));
}
```

Append to `tests/dom.rs`:

```rust
#[test]
fn selection_ordered_keeps_argument_order_and_drops_unknown_and_repeated_ids() {
    let d = sciink::dom::Doc::parse(
        br#"<svg xmlns="http://www.w3.org/2000/svg"><rect id="a"/><rect id="b"/><rect id="c"/></svg>"#,
    )
    .unwrap();
    let ids: Vec<String> = ["c", "nope", "a", "c"].iter().map(|s| s.to_string()).collect();
    let sel = d.selection_ordered(&ids);
    assert_eq!(sel, vec![d.by_id("c").unwrap(), d.by_id("a").unwrap()]);
    // the document-order variant is unchanged
    assert_eq!(d.selection(&ids), vec![d.by_id("a").unwrap(), d.by_id("c").unwrap()]);
}
```

Append to `tests/ops_bbox.rs` (the file declares `mod support;` already; add it if not):

```rust
#[test]
fn ctx_parse_text_reads_the_characters() {
    let mut doc = sciink::dom::Doc::parse(
        br#"<svg xmlns="http://www.w3.org/2000/svg"><text id="t" style="font-family:'DejaVu Sans'">Hi</text><rect id="r"/></svg>"#,
    )
    .unwrap();
    let (t, r) = (doc.by_id("t").unwrap(), doc.by_id("r").unwrap());
    let mut ctx = sciink::ops::Ctx::new();
    let pt = support::with_vendored_fonts(|| ctx.parse_text(&mut doc, t)).unwrap();
    assert_eq!(pt.text(), "Hi");
    assert_eq!(pt.chars.len(), 2);
    assert!(ctx.parse_text(&mut doc, r).is_none(), "not a text element");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test geom --test dom --test ops_bbox 2>&1 | tail -15`
Expected: compile errors — the methods do not exist.

- [ ] **Step 3: Implement**

`src/geom/mod.rs`, in the `impl Doc` block after `viewbox`:

```rust
    /// Pixels per user unit (`cache.py:1182–1277`, Inkscape's Document Properties "Scale"):
    /// `width`/`height` in px over the viewBox, `preserveAspectRatio` picking `min` (meet, the
    /// default) or `max` (slice); `1.0` when the document carries no usable size.
    /// Deviation: align `none` with distinct factors gives the geometric mean (upstream: no scale).
    pub fn px_per_uu(&self) -> f64 {
        let svg = self.svg();
        let Some(vb) = self.viewbox() else { return 1.0 };
        let factor = |attr: &str, vb_len: f64| -> Option<f64> {
            let Some(s) = self.attr(svg, attr) else { return Some(1.0) };
            let s = s.trim();
            if let Some(pct) = s.strip_suffix('%') {
                return num::parse(pct).map(|v| v / 100.0);
            }
            ipx(s).map(|px| px / vb_len)
        };
        let (Some(xfr), Some(yfr)) = (factor("width", vb.width()), factor("height", vb.height()))
        else {
            return 1.0;
        };
        let par = self.attr(svg, "preserveAspectRatio").unwrap_or("");
        let toks: Vec<&str> = par.split_whitespace().collect();
        let (align_none, slice) = match toks.as_slice() {
            ["none"] | ["none", _] => (true, false),
            [_, "slice"] | ["slice"] => (false, true),
            _ => (false, false),
        };
        let v = if align_none {
            if (xfr - yfr).abs() < 0.001 { xfr } else { (xfr * yfr).sqrt() }
        } else if slice {
            xfr.max(yfr)
        } else {
            xfr.min(yfr)
        };
        if v.is_finite() && v > 0.0 { v } else { 1.0 }
    }
```

(`num::parse` is `crate::num::parse`; add `use crate::num;` if the module does not import it yet. When `viewBox` is absent `viewbox()` already returns `[0, 0, ipx(width), ipx(height)]`, so both factors are exactly `1` and the result is `1.0`.)

`src/dom.rs`, after `selection`:

```rust
    /// Nodes for the given ids in the order given (Inkscape's selection order), each once;
    /// unknown ids are dropped. The Scaler's match target is the FIRST selected object.
    pub fn selection_ordered(&self, ids: &[String]) -> Vec<NodeId> {
        let mut out: Vec<NodeId> = Vec::new();
        for id in ids {
            if let Some(n) = self.by_id(id) {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        out
    }
```

`src/ops/mod.rs`, in `impl Ctx` after `char_table`:

```rust
    /// Drops the character table so the next measurement rebuilds it — after a tool restyles
    /// text (new families or sizes need new entries and their own kerning pairs and warnings).
    pub fn reset_char_table(&mut self) {
        self.text = None;
    }

    /// Parses one `<text>`/`<flowRoot>` against the character table (built on first use), the
    /// way the bbox code does; `None` for other elements or unparsable text.
    pub fn parse_text(&mut self, doc: &mut Doc, el: NodeId) -> Option<ParsedText> {
        if !matches!(doc.tag(el), "text" | "flowRoot") {
            return None;
        }
        self.ensure_char_table(doc);
        let Ctx { text, warn, .. } = self;
        let ct = text.as_mut().expect("built by ensure_char_table");
        ParsedText::parse(doc, el, ct, warn)
    }
```

(`use crate::text::parse::ParsedText;` at the top of `src/ops/mod.rs` if it is not imported yet.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test geom --test dom --test ops_bbox 2>&1 | tail -15`
Expected: pass.

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/geom/mod.rs src/dom.rs src/ops/mod.rs tests/geom.rs tests/dom.rs tests/ops_bbox.rs
git commit -m "feat(dom,geom,ops): px_per_uu document scale, ordered selection, Ctx char-table reset and text parse"
```

---

### Task 3: Scaler geometry — `geometric_bbox`, `find_plot_area`, warnings

The Scaler's plot-area detection (`SP:52–145`), as free functions in the new module so Task 8's Homogenizer can reuse them (upstream imports them from `scale_plots`).

**Files:**
- Create: `src/tools/scaler.rs` (geometry half; Task 4 adds the CLI and `run`)
- Modify: `src/tools/mod.rs` (`pub mod scaler;`)
- Test: `tests/scaler.rs` (new)

**Interfaces:**
- Consumes: `geom::{transform_rect, uniquetol, Point, Rect, BezPath}`, `geom::path::{shape_path, end_points}`, `ops::style::strokefill`, `ops::Warnings` (`ctx.warn.push`), `Doc::{tag, attr, composed_transform, children, is_element}`.
- Produces (all `pub`):
  - `pub const PATHLIKE: &[&str] = &["path", "rect", "line", "polyline"];`
  - `pub const SCALEFREE_DEFAULT: &[&str] = &["text", "flowRoot", "g"];`
  - `pub const EXCLUDE_TAGS: &[&str] = &["tspan", "namedview", "defs", "metadata", "foreignObject"];`
  - `pub const SCALETYPE: &str = "inkscape-scientific-scaletype";`
  - `pub const COMBINED: &str = "inkscape-scientific-combined-by-color";`
  - `pub const IMAGE_ERR: &str` (upstream's text, `SP:148–157`).
  - `pub fn global_points(doc: &Doc, el: NodeId, range: Option<Range<usize>>) -> Vec<Point>` — end points of the element's geometry (optionally one BezPath-element range) in root coordinates (`DH.get_points`).
  - `pub fn geometric_bbox(doc: &Doc, el: NodeId, vis: Rect, range: Option<Range<usize>>) -> Rect` (`SP:52–65`).
  - `pub struct PlotArea { pub vl: HashSet<NodeId>, pub hl: HashSet<NodeId>, pub lvel: Option<NodeId>, pub lhel: Option<NodeId> }`
  - `pub fn find_plot_area(doc: &Doc, els: &[NodeId], gbbs: &HashMap<NodeId, Rect>) -> PlotArea` (`SP:69–123`).
  - `pub fn ordinal(n: usize) -> String` (`SP:127–132`), `pub fn warn_non_plot(warn: &mut Warnings, idx: usize, gid: &str)` (`SP:135–145`).

- [ ] **Step 1: Write the failing tests**

Create `tests/scaler.rs`:

```rust
mod support;

use std::collections::HashMap;
use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::Rect;
use sciink::ops::Ctx;
use sciink::ops::bbox::bb2;
use sciink::tools::scaler::{find_plot_area, geometric_bbox, global_points, ordinal};
use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap_or_else(|| panic!("no element {i}"))
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
/// Visual boxes of every element under the plot, then geometric boxes (`SP:271–275`).
fn boxes(doc: &mut Doc, plot: NodeId) -> (HashMap<NodeId, Rect>, HashMap<NodeId, Rect>) {
    let els: Vec<NodeId> = doc.descendants(plot).filter(|&n| doc.is_element(n)).collect();
    let mut ctx = Ctx::new();
    let fbbs = with_vendored_fonts(|| bb2(doc, &mut ctx, &els, false));
    let gbbs = fbbs
        .iter()
        .map(|(&n, &v)| (n, geometric_bbox(doc, n, v, None)))
        .collect();
    (fbbs, gbbs)
}

/// A plot: a stroked box, two vertical ticks at the bottom edge, one horizontal tick at the
/// left edge, a fat data line, a solid white background rectangle, a tick label.
const PLOT: &str = r#"<g id="plot">
  <rect id="bg" x="0" y="0" width="120" height="100" style="fill:#ffffff;stroke:none"/>
  <path id="box" d="M20,10 H110 V80 H20 Z" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="t1" d="M40,80 V84" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="t2" d="M80,80 V84" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="t3" d="M16,45 H20" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="data" d="M25,70 L60,30 L105,50" style="fill:none;stroke:#1f77b4;stroke-width:3"/>
  <text id="lbl" x="40" y="92" style="font-size:6px;text-anchor:middle;FONT">0.5</text>
</g>"#;

fn plot_svg(extra_root_attrs: &str) -> String {
    format!(
        r#"<svg {NS} {extra_root_attrs}>{}</svg>"#,
        PLOT.replace("FONT", DV)
    )
}

#[test]
fn global_points_and_geometric_bbox_clamp_to_the_visual_box() {
    let svg = format!(
        r#"<svg {NS}><g transform="translate(10,20) scale(2)"><path id="p" d="M0,0 L5,0 L5,5 Z" style="stroke:#000;stroke-width:1"/></g></svg>"#
    );
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let p = id(&doc, "p");
    let pts = global_points(&doc, p, None);
    // M, L, L, Z(=start): four points in root coordinates
    assert_eq!(pts.len(), 4);
    assert!(close(pts[1].x, 20.0, 1e-9) && close(pts[1].y, 20.0, 1e-9), "{:?}", pts[1]);
    assert!(close(pts[2].x, 20.0, 1e-9) && close(pts[2].y, 30.0, 1e-9));
    assert!(close(pts[3].x, 10.0, 1e-9) && close(pts[3].y, 20.0, 1e-9), "Z ends at the start");
    // a range covers only some elements: [1, 3) = the two L's
    let pts = global_points(&doc, p, Some(1..3));
    assert_eq!(pts.len(), 2);
    // the visual box (stroke 1 × scale 2 → 1 unit of padding) is clamped away: the geometric box
    // is the end-point box 10..20 × 20..30
    let (fbbs, _) = boxes(&mut doc, p);
    let vis = fbbs[&p];
    assert!(close(vis.x0, 9.0, 1e-6) && close(vis.x1, 21.0, 1e-6), "{vis:?}");
    let g = geometric_bbox(&doc, p, vis, None);
    assert!(close(g.x0, 10.0, 1e-9) && close(g.x1, 20.0, 1e-9) && close(g.y0, 20.0, 1e-9) && close(g.y1, 30.0, 1e-9), "{g:?}");
    // a clipped element: points beyond the visual box are clamped to it
    let g = geometric_bbox(&doc, p, Rect::new(12.0, 22.0, 18.0, 28.0), None);
    assert!(close(g.x0, 12.0, 1e-9) && close(g.x1, 18.0, 1e-9) && close(g.y0, 22.0, 1e-9) && close(g.y1, 28.0, 1e-9));
    // non-path-like elements keep the visual box
    let svg = format!(r#"<svg {NS}><text id="t" x="3" y="4" style="{DV}">Hi</text></svg>"#);
    let doc = Doc::parse(svg.as_bytes()).unwrap();
    let vis = Rect::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(geometric_bbox(&doc, id(&doc, "t"), vis, None), vis);
}

#[test]
fn find_plot_area_picks_the_stroked_box_and_classifies_ticks() {
    let svg = plot_svg("");
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let plot = id(&doc, "plot");
    let (_, gbbs) = boxes(&mut doc, plot);
    let kids: Vec<NodeId> = doc.children(plot).filter(|&n| doc.is_element(n)).collect();
    let pa = find_plot_area(&doc, &kids, &gbbs);
    assert_eq!(pa.lvel, Some(id(&doc, "box")), "the framed rectangle is the largest vertical extent");
    assert_eq!(pa.lhel, Some(id(&doc, "box")));
    assert!(pa.vl.contains(&id(&doc, "t1")) && pa.vl.contains(&id(&doc, "t2")), "vertical ticks");
    assert!(pa.hl.contains(&id(&doc, "t3")), "horizontal tick");
    assert!(!pa.vl.contains(&id(&doc, "data")) && !pa.hl.contains(&id(&doc, "data")));
    assert!(!pa.vl.contains(&id(&doc, "bg")), "the solid white rectangle is neither a line nor a box");
}

#[test]
fn find_plot_area_falls_back_to_lines_and_honours_plot_area_marks() {
    // no box: the longest vertical and horizontal LINES define the plot area
    let svg = format!(
        r#"<svg {NS}><g id="plot">
  <path id="ax" d="M10,90 H110" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="ay" d="M10,90 V10" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="tick" d="M50,90 V93" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="data" d="M20,80 L60,20 L100,60" style="fill:none;stroke:#f00;stroke-width:2"/>
</g></svg>"#
    );
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let plot = id(&doc, "plot");
    let (_, gbbs) = boxes(&mut doc, plot);
    let kids: Vec<NodeId> = doc.children(plot).filter(|&n| doc.is_element(n)).collect();
    let pa = find_plot_area(&doc, &kids, &gbbs);
    assert_eq!(pa.lvel, Some(id(&doc, "ay")));
    assert_eq!(pa.lhel, Some(id(&doc, "ax")));
    assert!(pa.vl.contains(&id(&doc, "tick")));
    // a marked element wins when it is the largest; a stroked box does not count when a
    // taller marked element exists
    let svg = format!(
        r#"<svg {NS}><g id="plot">
  <rect id="box" x="20" y="20" width="50" height="40" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <rect id="marked" x="10" y="10" width="80" height="70" style="fill:#eee;stroke:none" inkscape-scientific-scaletype="plot_area"/>
</g></svg>"#
    );
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let plot = id(&doc, "plot");
    let (_, gbbs) = boxes(&mut doc, plot);
    let kids: Vec<NodeId> = doc.children(plot).filter(|&n| doc.is_element(n)).collect();
    let pa = find_plot_area(&doc, &kids, &gbbs);
    assert_eq!(pa.lvel, Some(id(&doc, "marked")));
    assert_eq!(pa.lhel, Some(id(&doc, "marked")));
    // nothing box-like at all → None
    let svg = format!(r#"<svg {NS}><g id="plot"><text id="t" style="{DV}">x</text></g></svg>"#);
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let plot = id(&doc, "plot");
    let (_, gbbs) = boxes(&mut doc, plot);
    let kids: Vec<NodeId> = doc.children(plot).filter(|&n| doc.is_element(n)).collect();
    let pa = find_plot_area(&doc, &kids, &gbbs);
    assert_eq!((pa.lvel, pa.lhel), (None, None));
}

#[test]
fn ordinals_follow_upstream() {
    assert_eq!(ordinal(1), "1st");
    assert_eq!(ordinal(2), "2nd");
    assert_eq!(ordinal(3), "3rd");
    assert_eq!(ordinal(4), "4th");
    assert_eq!(ordinal(11), "11th");
    assert_eq!(ordinal(12), "12th");
    assert_eq!(ordinal(13), "13th");
    assert_eq!(ordinal(21), "21st");
    assert_eq!(ordinal(112), "112th");
}
```

(`args`, `by_id`, `plot_svg` are used by Task 4's tests in this file; `#[allow(dead_code)]` is not needed once Task 4 lands — until then mark the two unused helpers `#[allow(dead_code)]` so clippy's `-D warnings` gate passes.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test scaler 2>&1 | tail -15`
Expected: compile error — `sciink::tools::scaler` does not exist.

- [ ] **Step 3: Implement the geometry half**

Create `src/tools/scaler.rs`:

```rust
//! Scaler (spec §B.3 "Scaler"; upstream scale_plots.py): corrects manually scaled plots and
//! matches plots to a first selection without distorting text, ticks and groups.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::dom::{Doc, NodeId};
use crate::geom::path::shape_path;
use crate::geom::{BezPath, PathEl, Point, Rect, uniquetol};
use crate::ops::style::strokefill;
use crate::text::Warnings;

/// Elements whose end points, not their visual box, describe their extent (`SP:31–36`).
pub const PATHLIKE: &[&str] = &["path", "rect", "line", "polyline"];
/// Elements that are not scaled unless marked otherwise (`SP:38–40`).
pub const SCALEFREE_DEFAULT: &[&str] = &["text", "flowRoot", "g"];
/// Selection members that are never plots (`SP:41–47`).
pub const EXCLUDE_TAGS: &[&str] = &["tspan", "namedview", "defs", "metadata", "foreignObject"];
pub const SCALETYPE: &str = "inkscape-scientific-scaletype";
pub const COMBINED: &str = "inkscape-scientific-combined-by-color";
/// `SP:148–157`, shown when every selected object is a raster image.
pub const IMAGE_ERR: &str = "Thanks for using Scientific Inkscape!\n\nIt appears that you're attempting to scale a raster Image object. Please note that Inkscape is mainly for working with vector images, not raster images. Vector images preserve all of the information used to generate them, whereas raster images do not. Read about the difference here: \nhttps://en.wikipedia.org/wiki/Vector_graphics\n\nWhile raster images can be embedded in vector images, they cannot be modified directly. If you want to edit a raster image, you will need to use a program like Photoshop or GIMP.";

/// End points of `path`'s elements in `range` (all when `None`), one per element; a `Z` yields
/// the start of its subpath when that start lies inside the range (`inkex/paths.py:1446–1457`).
fn range_end_points(path: &BezPath, range: Option<Range<usize>>) -> Vec<Point> {
    let els = path.elements();
    let range = range.unwrap_or(0..els.len());
    let end = range.end.min(els.len());
    let mut out = Vec::new();
    let mut start: Option<Point> = None;
    for el in &els[range.start.min(end)..end] {
        match *el {
            PathEl::MoveTo(p) => {
                start = Some(p);
                out.push(p);
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => out.push(p),
            PathEl::ClosePath => {
                if let Some(s) = start {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// The element's end points in root coordinates (`DH.get_points`): its own geometry (`shape_path`,
/// absolute), optionally one BezPath-element range, through its composed transform. Empty for
/// elements without geometry.
pub fn global_points(doc: &Doc, el: NodeId, range: Option<Range<usize>>) -> Vec<Point> {
    let Some(pp) = shape_path(doc, el) else { return Vec::new() };
    let ct = doc.composed_transform(el);
    range_end_points(&pp.path, range).into_iter().map(|p| ct * p).collect()
}

/// `SP:52–65`: for path-like elements the box of the global end points, clamped to the visual box
/// (a clipped element's points overshoot its visible extent); otherwise the visual box.
pub fn geometric_bbox(doc: &Doc, el: NodeId, vis: Rect, range: Option<Range<usize>>) -> Rect {
    if !PATHLIKE.contains(&doc.tag(el)) {
        return vis;
    }
    let pts = global_points(doc, el, range);
    if pts.is_empty() {
        return vis;
    }
    let (mut minx, mut maxx, mut miny, mut maxy) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for p in &pts {
        minx = minx.min(p.x);
        maxx = maxx.max(p.x);
        miny = miny.min(p.y);
        maxy = maxy.max(p.y);
    }
    // upstream keeps a negative width when the clamps cross; kurbo's Rect does the same
    Rect::new(minx.max(vis.x0), miny.max(vis.y0), maxx.min(vis.x1), maxy.min(vis.y1))
}

/// Result of `find_plot_area` (`SP:69–123`): the vertical and horizontal lines among `els`, and
/// the elements with the largest vertical (`lvel`) and horizontal (`lhel`) extents among the
/// lines, the framed rectangles and the elements marked `plot_area`.
#[derive(Debug, Default)]
pub struct PlotArea {
    pub vl: HashSet<NodeId>,
    pub hl: HashSet<NodeId>,
    pub lvel: Option<NodeId>,
    pub lhel: Option<NodeId>,
}

fn is_opaque_white(c: &crate::ops::style::Rgba) -> bool {
    (c.r, c.g, c.b) == (255, 255, 255) && c.alpha == 1.0
}

/// `SP:69–123` over `els` (a plot's direct element children) and their geometric boxes; elements
/// without a box are skipped. Ties for the largest extent go to the first candidate in upstream's
/// insertion order: lines (in reversed `els` order), then framed rectangles, then marked elements.
pub fn find_plot_area(doc: &Doc, els: &[NodeId], gbbs: &HashMap<NodeId, Rect>) -> PlotArea {
    let mut pa = PlotArea::default();
    let mut vl: Vec<(NodeId, Rect)> = Vec::new();
    let mut hl: Vec<(NodeId, Rect)> = Vec::new();
    let mut boxes: Vec<(NodeId, Rect)> = Vec::new();
    let mut plotareas: Vec<(NodeId, Rect)> = Vec::new();
    for &el in els.iter().rev() {
        let Some(&gbb) = gbbs.get(&el) else { continue };
        let tag = doc.tag(el);
        let mut isrect = false;
        if PATHLIKE.contains(&tag) {
            let pts = global_points(doc, el, None);
            if !pts.is_empty() {
                let xs: Vec<f64> = pts.iter().map(|p| p.x).collect();
                let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
                let (xmin, xmax) = (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                let (ymin, ymax) = (ys.iter().cloned().fold(f64::INFINITY, f64::min), ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
                if xmax - xmin < 0.001 * gbb.height() {
                    vl.push((el, gbb));
                    pa.vl.insert(el);
                }
                if ymax - ymin < 0.001 * gbb.width() {
                    hl.push((el, gbb));
                    pa.hl.insert(el);
                }
                let tol = 1e-3 * (xmax - xmin).max(ymax - ymin);
                isrect = (3..=5).contains(&pts.len()) && uniquetol(&xs, tol) == 2 && uniquetol(&ys, tol) == 2;
            }
        }
        if isrect || tag == "rect" {
            let sf = strokefill(doc, el);
            let hasfill = sf.fill.as_ref().is_some_and(|f| !is_opaque_white(f));
            let hasstroke = sf.stroke.as_ref().is_some_and(|s| !is_opaque_white(s));
            let same = match (&sf.stroke, &sf.fill) {
                (Some(s), Some(f)) => (s.r, s.g, s.b) == (f.r, f.g, f.b) && (s.alpha - f.alpha).abs() < 1e-9,
                _ => false,
            };
            if hasfill && (!hasstroke || same) {
                // solid rectangle: unused by upstream too
            } else if hasstroke {
                boxes.push((el, gbb));
            }
        }
        if doc.attr(el, SCALETYPE) == Some("plot_area") {
            plotareas.push((el, gbb));
        }
    }
    // largest vertical extent among lines (by height), boxes and marked elements (by height)
    let mut vels: Vec<(NodeId, f64)> = vl.iter().map(|(n, b)| (*n, b.height())).collect();
    let mut hels: Vec<(NodeId, f64)> = hl.iter().map(|(n, b)| (*n, b.width())).collect();
    for (n, b) in boxes.iter().chain(plotareas.iter()) {
        hels.push((*n, b.width()));
        vels.push((*n, b.height()));
    }
    let first_max = |v: &[(NodeId, f64)]| -> Option<NodeId> {
        let mut best: Option<(NodeId, f64)> = None;
        for &(n, x) in v {
            if best.is_none_or(|(_, bx)| x > bx) {
                best = Some((n, x));
            }
        }
        best.map(|(n, _)| n)
    };
    pa.lvel = first_max(&vels);
    pa.lhel = first_max(&hels);
    pa
}

/// `SP:127–132`.
pub fn ordinal(n: usize) -> String {
    let suffix = if (10..=20).contains(&(n % 100)) {
        "th"
    } else {
        match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{n}{suffix}")
}

/// `SP:135–145`: the "no plot area" message for the `idx`-th (0-based) selected plot.
pub fn warn_non_plot(warn: &mut Warnings, idx: usize, gid: &str) {
    warn.push(format!(
        "A box-like plot area could not be automatically detected on the {} selected plot (group ID {gid}).\n\nDraw a box with a stroke to define the plot area or mark objects as plot area-determining in the Advanced tab.\nScaling will still be performed, but the results may not be ideal.",
        ordinal(idx + 1)
    ));
}
```

Notes for the implementer: `uniquetol(&[f64], f64) -> usize` and `Rect` come from `crate::geom`; `PathEl` is re-exported by kurbo — add `pub use kurbo::PathEl;` to `src/geom/mod.rs`'s re-export line if `crate::geom::PathEl` does not resolve (the line today re-exports `Affine, BezPath, Point, Rect, Vec2`). `Warnings` is `crate::text::Warnings`. The `same` test compares rgba because upstream compares `inkex.Color` lists `[r, g, b, a]`. The solid-rectangle branch is deliberately empty (upstream collects `solids` and never reads them); keep the comment so a reviewer sees the intent.

Register the module in `src/tools/mod.rs` (alphabetical: after `pub mod font_probe;`): `pub mod scaler;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test scaler 2>&1 | tail -15`
Expected: 4 passed.

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/tools/scaler.rs src/tools/mod.rs src/geom/mod.rs tests/scaler.rs
git commit -m "feat(scaler): plot-area geometry — global end points, geometric boxes, find_plot_area"
```

---

### Task 4: Scaler CLI, Advanced tab, setup, Correction mode, per-child corrections

`SP:183–408` and `SP:442–613` for Correction mode (both figure modes), tick correction, scale-free / aspect-locked / normal children and combined-by-colour ranges. Matching mode is Task 5 (the code below leaves its branch returning early with a clear marker that Task 5 replaces).

**Files:**
- Modify: `src/tools/scaler.rs` (append the CLI, `Options`, `Boxes`, `Bb2`, `scale_plot`, `run`), `src/lib.rs` (dispatch arm `"scaler"`)
- Test: `tests/scaler.rs` (append)

**Interfaces:**
- Consumes: Task 3's geometry; `Doc::selection_ordered` (Task 2); `ops::xform::global_transform(doc, ctx, el, t, ranges: Option<Ranges>, preserve_stroke)`, `ops::xform::Ranges`; `ops::bbox::bb2`; `geom::{transform_rect, inverse, union, Affine, Point}`; `cli::{Common, inx_bool}`; `super::first_line`; `crate::Output`.
- Produces: `pub struct ScalerCli`, `pub enum Mode`, `pub struct Options { pub mode: Mode, pub figure: bool, pub wholesel: bool, pub tickcorrect: bool, pub tickthr: f64 }` with `pub fn from_cli(&ScalerCli) -> Options`; `pub(crate) struct Boxes { pub f: HashMap<NodeId, Rect>, pub g: HashMap<NodeId, Rect> }` with `compute(doc, ctx, roots) -> Boxes` and `refresh(&mut self, doc, ctx, plot)`; `pub(crate) fn scale_plot(doc, ctx, o: &Options, boxes: &mut Boxes, first: NodeId, plot: NodeId, i: usize, cmode: bool)`; `pub fn run(argv, input) -> Result<Output, String>`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/scaler.rs`:

```rust
fn scale(svg: &str, extra: &[&str]) -> Result<(String, Vec<String>), String> {
    let mut a = vec!["--tool=scaler"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes()))?;
    Ok((String::from_utf8(out.svg).unwrap(), out.messages))
}
fn ok(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    scale(svg, extra).unwrap_or_else(|e| panic!("scaler failed: {e}"))
}
/// x- and y-extent of an element's global end points in an output document.
fn extent(doc: &Doc, id_: &str) -> (f64, f64, Rect) {
    let pts = global_points(doc, id(doc, id_), None);
    assert!(!pts.is_empty(), "{id_} has geometry");
    let r = Rect::from_points(pts[0], pts[0]);
    let r = pts.iter().fold(r, |r, p| r.union_pt(*p));
    (r.width(), r.height(), r)
}
fn composed(doc: &Doc, id_: &str) -> [f64; 6] {
    doc.composed_transform(id(doc, id_)).as_coeffs()
}
fn is_translation(c: [f64; 6]) -> bool {
    close(c[0], 1.0, 1e-9) && close(c[1], 0.0, 1e-9) && close(c[2], 0.0, 1e-9) && close(c[3], 1.0, 1e-9)
}
fn visual_stroke(doc: &Doc, id_: &str) -> f64 {
    let n = id(doc, id_);
    let w: f64 = doc.specified(n, "stroke-width").unwrap().trim_end_matches("px").parse().unwrap();
    w * sciink::geom::scale_factor(doc.composed_transform(n))
}
/// The plot manually scaled by (2, 0.5): sx = 2, sy = 0.5.
fn scaled_plot(extra_group_attrs: &str) -> String {
    plot_svg("").replace(
        r#"<g id="plot">"#,
        &format!(r#"<g id="plot" transform="matrix(2,0,0,0.5,5,7)" {extra_group_attrs}>"#),
    )
}

#[test]
fn advanced_tab_marks_and_clears_the_selection_and_changes_nothing_else() {
    let svg = plot_svg("");
    let (s, msgs) = ok(&svg, &["--tab=options", "--marksf=2", "--id=box", "--id=lbl"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(by_id(&d, "box").attribute("inkscape-scientific-scaletype"), Some("aspect_locked"));
    assert_eq!(by_id(&d, "lbl").attribute("inkscape-scientific-scaletype"), Some("aspect_locked"));
    assert_eq!(by_id(&d, "data").attribute("d"), Some("M25,70 L60,30 L105,50"), "untouched");
    for (m, v) in [("1", "scale_free"), ("3", "normal"), ("4", "plot_area")] {
        let (s, _) = ok(&svg, &["--tab=options", &format!("--marksf={m}"), "--id=box"]);
        let d = roxmltree::Document::parse(&s).unwrap();
        assert_eq!(by_id(&d, "box").attribute("inkscape-scientific-scaletype"), Some(v));
    }
    let (s, _) = ok(&s, &["--tab=options", "--marksf=5", "--id=box", "--id=lbl"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(by_id(&d, "box").attribute("inkscape-scientific-scaletype"), None, "cleared");
    assert_eq!(by_id(&d, "lbl").attribute("inkscape-scientific-scaletype"), None);
}

#[test]
fn errors_follow_upstream() {
    let svg = format!(r#"<svg {NS}><image id="i" width="1" height="1"/><rect id="r" width="1" height="1"/><g id="g"/></svg>"#);
    let e = scale(&svg, &["--tab=correction", "--id=i"]).unwrap_err();
    assert!(e.starts_with("Thanks for using Scientific Inkscape!"), "{e}");
    let e = scale(&svg, &["--tab=correction", "--id=r"]).unwrap_err();
    assert!(e.starts_with("Non-Group objects detected in selection."), "{e}");
    let e = scale(&svg, &["--tab=correction"]).unwrap_err();
    assert_eq!(e, "No objects selected!");
    // matching: the first selection may be anything, the plots must be groups
    let e = scale(&svg, &["--tab=matching", "--id=r", "--id=i"]).unwrap_err();
    assert!(e.starts_with("Non-Group objects detected in selection."), "{e}");
    // the Advanced tab never errors on images
    ok(&svg, &["--tab=options", "--marksf=1", "--id=i"]);
}

#[test]
fn correction_restores_text_and_ticks_and_keeps_the_plot_area_size() {
    let svg = scaled_plot("");
    let (s, msgs) = ok(&svg, &["--tab=correction", "--figuremode=1", "--id=plot"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    // the group's scale is gone (a pure translation is left), the plot area keeps its manual size
    assert!(is_translation(out.transform(id(&out, "plot")).as_coeffs()), "{:?}", out.transform(id(&out, "plot")));
    let (bw, bh, bbox) = extent(&out, "box");
    assert!(close(bw, 180.0, 1e-6) && close(bh, 35.0, 1e-6), "box {bw} × {bh}");
    // data scales with the plot area, its visual stroke width is preserved (3 × sqrt(2 × 0.5) = 3)
    let (dw, dh, _) = extent(&out, "data");
    assert!(close(dw, 160.0, 1e-6) && close(dh, 20.0, 1e-6), "data {dw} × {dh}");
    assert!(close(visual_stroke(&out, "data"), 3.0, 1e-6));
    // the label is unscaled again
    assert!(is_translation(composed(&out, "lbl")), "{:?}", composed(&out, "lbl"));
    // the bottom ticks keep their length and stay attached to the box's bottom edge
    let (_, th, tbox) = extent(&out, "t1");
    assert!(close(th, 4.0, 1e-6), "tick length {th}");
    assert!(close(tbox.y0, bbox.y1, 1e-6), "tick top {} on box bottom {}", tbox.y0, bbox.y1);
    // the left tick keeps its length and touches the box's left edge
    let (tw, _, tbox) = extent(&out, "t3");
    assert!(close(tw, 4.0, 1e-6) && close(tbox.x1, bbox.x0, 1e-6));
    // the solid background rectangle is a "normal" element: it scales with the plot
    let (gw, gh, _) = extent(&out, "bg");
    assert!(close(gw, 240.0, 1e-6) && close(gh, 50.0, 1e-6));
}

#[test]
fn tick_correction_can_be_disabled_and_wholeplot_skips_detection() {
    let svg = scaled_plot("");
    let (s, _) = ok(&svg, &["--tab=correction", "--tickcorrect=false", "--id=plot"]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (_, th, _) = extent(&out, "t1");
    assert!(close(th, 2.0, 1e-6), "ticks scale with the plot: {th}");
    let (s, msgs) = ok(&svg, &["--tab=correction", "--wholeplot3=true", "--id=plot"]);
    assert!(msgs.is_empty(), "no plot-area warning: {msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    assert!(is_translation(composed(&out, "lbl")), "text is still scale-free");
    let (_, th, _) = extent(&out, "t1");
    assert!(close(th, 2.0, 1e-6), "no tick correction either");
}

#[test]
fn figure_mode_keeps_the_figure_bounding_box() {
    // labels outside the plot area supply the margins; no background rectangle
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="matrix(2,0,0,0.5,5,7)">
  <path id="box" d="M20,10 H110 V80 H20 Z" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="data" d="M25,70 L60,30 L105,50" style="fill:none;stroke:#1f77b4;stroke-width:3"/>
  <text id="xl" x="65" y="95" style="font-size:6px;text-anchor:middle;{DV}">time</text>
  <text id="yl" x="8" y="45" style="font-size:6px;text-anchor:middle;{DV}" transform="rotate(-90,8,45)">value</text>
</g></svg>"#
    );
    let before = {
        let mut d = Doc::parse(svg.as_bytes()).unwrap();
        let plot = id(&d, "plot");
        let (f, _) = boxes(&mut d, plot);
        f.values().fold(None, |acc: Option<Rect>, r| Some(acc.map_or(*r, |a| a.union(*r)))).unwrap()
    };
    let (s, msgs) = ok(&svg, &["--tab=correction", "--figuremode=2", "--id=plot"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let mut out = Doc::parse(s.as_bytes()).unwrap();
    let plot = id(&out, "plot");
    let (f, _) = boxes(&mut out, plot);
    let after = f.values().fold(None, |acc: Option<Rect>, r| Some(acc.map_or(*r, |a| a.union(*r)))).unwrap();
    // `after` is re-measured from the serialised output: `num::fmt` writes 8 significant digits,
    // so coordinates near 200 carry ~1e-5 of rounding — 1e-3 is still 5 ppm of the figure
    assert!(close(after.x0, before.x0, 1e-3) && close(after.y0, before.y0, 1e-3), "top-left kept: {after:?} vs {before:?}");
    assert!(close(after.width(), before.width(), 1e-3) && close(after.height(), before.height(), 1e-3), "size kept: {after:?} vs {before:?}");
    assert!(is_translation(composed(&out, "xl")), "labels unscaled");
}

#[test]
fn a_plot_without_a_box_warns_and_is_still_scaled() {
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="scale(2,1)">
  <path id="data" d="M0,0 L50,40 L100,10" style="fill:none;stroke:#f00;stroke-width:1"/>
  <text id="t" x="50" y="60" style="font-size:6px;{DV}">x</text>
</g></svg>"#
    );
    let (s, msgs) = ok(&svg, &["--tab=correction", "--id=plot"]);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].starts_with("warning: A box-like plot area could not be automatically detected on the 1st selected plot (group ID plot)."), "{}", msgs[0]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (dw, _, _) = extent(&out, "data");
    assert!(close(dw, 200.0, 1e-6), "everything is the plot area: data keeps its manual width {dw}");
    assert!(is_translation(composed(&out, "t")));
}

#[test]
fn combined_by_colour_pieces_are_unscaled_one_by_one() {
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="scale(2,1)">
  <path id="box" d="M0,0 H200 V100 H0 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="mk" d="M0,50 L10,50 M50,50 L60,50" style="fill:none;stroke:#00f;stroke-width:1" inkscape-scientific-scaletype="scale_free" inkscape-scientific-combined-by-color="0 2 4"/>
</g></svg>"#
    );
    let (s, msgs) = ok(&svg, &["--tab=correction", "--id=plot"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let mk = id(&out, "mk");
    assert_eq!(out.attr(mk, "inkscape-scientific-combined-by-color"), Some("0 2 4"), "ranges kept");
    let a = global_points(&out, mk, Some(0..2));
    let b = global_points(&out, mk, Some(2..4));
    let len = |p: &[Point]| (p[1].x - p[0].x).abs();
    assert!(close(len(&a), 10.0, 1e-6) && close(len(&b), 10.0, 1e-6), "each piece keeps its length: {a:?} {b:?}");
    let ca = (a[0].x + a[1].x) / 2.0;
    let cb = (b[0].x + b[1].x) / 2.0;
    assert!(close(cb - ca, 100.0, 1e-6), "piece centres follow the plot's scale (gap 50 → 100): {}", cb - ca);
    // the same path WITHOUT the ranges attribute is unscaled as one piece: gap stays 50
    let svg = svg.replace(r#" inkscape-scientific-combined-by-color="0 2 4""#, "");
    let (s, _) = ok(&svg, &["--tab=correction", "--id=plot"]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let mk = id(&out, "mk");
    let (a, b) = (global_points(&out, mk, Some(0..2)), global_points(&out, mk, Some(2..4)));
    assert!(close((b[0].x + b[1].x) / 2.0 - (a[0].x + a[1].x) / 2.0, 50.0, 1e-6));
}

#[test]
fn aspect_locked_children_scale_uniformly() {
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="matrix(4,0,0,1,0,0)">
  <path id="box" d="M0,0 H200 V100 H0 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <rect id="m" x="95" y="45" width="10" height="10" style="fill:#0f0" inkscape-scientific-scaletype="aspect_locked"/>
</g></svg>"#
    );
    let (s, _) = ok(&svg, &["--tab=correction", "--id=plot"]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, _) = extent(&out, "m");
    // sqrt(4 × 1) = 2: the marker is scaled by 2 in both directions
    assert!(close(w, 20.0, 1e-6) && close(h, 20.0, 1e-6), "{w} × {h}");
}
```

Add `use sciink::geom::Point;` to the test file's imports (the `Point` re-export exists).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test scaler 2>&1 | tail -15`
Expected: compile errors — `sciink::run` with `--tool=scaler` returns `Err("the scaler tool is not implemented yet")` once it compiles; the new tests fail.

- [ ] **Step 3: Implement**

Append to `src/tools/scaler.rs` (add the imports `use std::ffi::OsString; use clap::Parser; use crate::Output; use crate::cli::{Common, inx_bool}; use crate::geom::{Affine, inverse, transform_rect, union}; use crate::ops::Ctx; use crate::ops::bbox::bb2; use crate::ops::xform::{Ranges, global_transform}; use super::first_line;` at the top):

```rust
#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct ScalerCli {
    #[command(flatten)]
    pub common: Common,
    /// `correction` | `matching` | `options` (anything else is the Advanced tab, as upstream)
    #[arg(long, default_value = "correction")]
    pub tab: String,
    /// Fixed mode is gone upstream and here; its parameters are accepted and ignored.
    #[arg(long, default_value_t = 100.0)]
    pub hscale: f64,
    #[arg(long, default_value_t = 100.0)]
    pub vscale: f64,
    /// 1 = maintain the plot area, 2 = maintain the bounding box
    #[arg(long, default_value_t = 1)]
    pub figuremode: u8,
    /// 1 = match plot areas, 2 = match bounding boxes
    #[arg(long, default_value_t = 1)]
    pub matchprop: u8,
    /// 1 = do not match, 2 = match, 3 = match and align
    #[arg(long, default_value_t = 1)]
    pub hmatchopts: u8,
    #[arg(long, default_value_t = 1)]
    pub vmatchopts: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub deletematch: bool,
    /// 1 scale_free, 2 aspect_locked, 3 normal, 4 plot_area, 5 clear
    #[arg(long, default_value_t = 1)]
    pub marksf: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub tickcorrect: bool,
    /// Percent of the plot area (the `.inx` says float, upstream parses int; both spellings parse)
    #[arg(long, default_value_t = 10.0)]
    pub tickthreshold: f64,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub wholeplot1: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub wholeplot2: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub wholeplot3: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Correction,
    Matching { hmatch: bool, vmatch: bool, alignx: bool, aligny: bool, bbox: bool, deletematch: bool },
    /// The Advanced tab: mark the selection (`None` clears) and stop.
    Advanced { mark: Option<&'static str> },
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub mode: Mode,
    /// `figuremode == 2`; read by every correction, including Matching's pre-pass (SP:345).
    pub figure: bool,
    pub wholesel: bool,
    pub tickcorrect: bool,
    pub tickthr: f64,
}

impl Options {
    /// SP:239–269. Deviation: option values outside upstream's tables are tolerated
    /// (`figuremode`/`matchprop` ≠ 2 mean the first option, `marksf` outside 1–4 clears).
    pub fn from_cli(c: &ScalerCli) -> Options {
        let (mode, wholesel) = match c.tab.as_str() {
            "matching" => (
                Mode::Matching {
                    hmatch: matches!(c.hmatchopts, 2 | 3),
                    vmatch: matches!(c.vmatchopts, 2 | 3),
                    alignx: c.hmatchopts == 3,
                    aligny: c.vmatchopts == 3,
                    bbox: c.matchprop == 2,
                    deletematch: c.deletematch,
                },
                c.wholeplot2,
            ),
            "correction" => (Mode::Correction, c.wholeplot3),
            _ => (
                Mode::Advanced {
                    mark: match c.marksf {
                        1 => Some("scale_free"),
                        2 => Some("aspect_locked"),
                        3 => Some("normal"),
                        4 => Some("plot_area"),
                        _ => None,
                    },
                },
                false,
            ),
        };
        Options {
            mode,
            figure: c.figuremode == 2,
            wholesel,
            tickcorrect: c.tickcorrect && !wholesel,
            tickthr: c.tickthreshold / 100.0,
        }
    }
}

/// Visual (`f`) and geometric (`g`) boxes in root coordinates, SP:271–275.
pub(crate) struct Boxes {
    pub f: HashMap<NodeId, Rect>,
    pub g: HashMap<NodeId, Rect>,
}

impl Boxes {
    /// Boxes of every element under `roots` (each once).
    pub(crate) fn compute(doc: &mut Doc, ctx: &mut Ctx, roots: &[NodeId]) -> Boxes {
        let mut els: Vec<NodeId> = Vec::new();
        let mut seen: HashSet<NodeId> = HashSet::new();
        for &r in roots {
            for n in doc.descendants(r).filter(|&n| doc.is_element(n)) {
                if seen.insert(n) {
                    els.push(n);
                }
            }
        }
        let f = bb2(doc, ctx, &els, false);
        let g = f.iter().map(|(&n, &v)| (n, geometric_bbox(doc, n, v, None))).collect();
        Boxes { f, g }
    }

    /// Re-measures one plot's subtree (after Matching's correction pre-pass).
    /// Deviation (spec R9): upstream matches against the boxes measured BEFORE the pre-pass.
    pub(crate) fn refresh(&mut self, doc: &mut Doc, ctx: &mut Ctx, plot: NodeId) {
        let fresh = Boxes::compute(doc, ctx, &[plot]);
        self.f.extend(fresh.f);
        self.g.extend(fresh.g);
    }
}

/// Upstream's `bbox2`: the geometric and the visual union of a set of elements.
#[derive(Default, Clone, Copy)]
struct Bb2 {
    g: Option<Rect>,
    f: Option<Rect>,
}

impl Bb2 {
    fn add(&mut self, g: Rect, f: Rect) {
        self.g = union(self.g, Some(g));
        self.f = union(self.f, Some(f));
    }
}

/// `(sx, sy)` of a group's own transform, SP:306–307 / 336–337: `sx = sqrt(a² + b²)`,
/// `sy = det / sx` (negative for a flip).
fn own_scale(t: Affine) -> (f64, f64) {
    let [a, b, c, d, _, _] = t.as_coeffs();
    let sx = (a * a + b * b).sqrt();
    (sx, if sx > 0.0 { (a * d - b * c) / sx } else { 0.0 })
}

/// A scale factor safe to invert: finite and non-zero, else `1` with a warning (hostile input;
/// upstream divides by zero).
fn sane(s: f64, what: &str, warn: &mut Warnings) -> f64 {
    if s.is_finite() && s.abs() > 1e-12 {
        s
    } else {
        warn.push(format!("{what} is degenerate ({s}); using 1"));
        1.0
    }
}

fn t(x: f64, y: f64) -> Affine {
    Affine::translate((x, y))
}
fn s(x: f64, y: f64) -> Affine {
    Affine::scale_non_uniform(x, y)
}

/// SP:297–613 for one grouped plot. `cmode` = Correction; `first` is the Matching target.
#[allow(clippy::too_many_arguments)]
pub(crate) fn scale_plot(
    doc: &mut Doc,
    ctx: &mut Ctx,
    o: &Options,
    boxes: &mut Boxes,
    first: NodeId,
    plot: NodeId,
    i: usize,
    cmode: bool,
) {
    if !cmode {
        // SP:303–309: a plot carrying a scale is corrected first
        let (sx, sy) = own_scale(doc.transform(plot));
        if (sx - 1.0).abs() > 1e-5 || (sy - 1.0).abs() > 1e-5 {
            scale_plot(doc, ctx, o, boxes, first, plot, i, true);
            boxes.refresh(doc, ctx, plot);
        }
    }
    let pid = doc.attr(plot, "id").unwrap_or("").to_string();
    let pels: Vec<NodeId> = doc
        .children(plot)
        .filter(|&k| doc.is_element(k) && boxes.f.contains_key(&k))
        .collect();
    let pa = find_plot_area(doc, &pels, &boxes.g);
    let (noplotarea, lvel, lhel) = if pa.lvel.is_none() || pa.lhel.is_none() || o.wholesel {
        if !o.wholesel {
            warn_non_plot(&mut ctx.warn, i, &pid);
        }
        (true, None, None)
    } else {
        (false, pa.lvel, pa.lhel)
    };
    let in_area = |el: NodeId| noplotarea || Some(el) == lvel || Some(el) == lhel;
    let mut bba = Bb2::default();
    let mut bbp = Bb2::default();
    for &el in &pels {
        bba.add(boxes.g[&el], boxes.f[&el]);
        if in_area(el) {
            bbp.add(boxes.g[&el], boxes.f[&el]);
        }
    }
    // the boxes the per-child step reads (SP:358–367 / 410–411)
    let mut f2: HashMap<NodeId, Rect> = pels.iter().map(|&e| (e, boxes.f[&e])).collect();
    let mut g2: HashMap<NodeId, Rect> = pels.iter().map(|&e| (e, boxes.g[&e])).collect();
    let (Some(_), Some(_)) = (bbp.g, bba.f) else { return }; // nothing with a box: nothing to scale

    let (mut scalex, mut scaley, mut refx, mut refy): (f64, f64, f64, f64);
    let mut bbmatch: Option<Rect> = None;
    if cmode {
        // SP:331–408
        let (sx, sy) = own_scale(doc.transform(plot));
        scalex = sane(sx, "the plot's horizontal scale", &mut ctx.warn);
        scaley = sane(sy, "the plot's vertical scale", &mut ctx.warn);
        let (bbp_g, bba_f) = (bbp.g.unwrap(), bba.f.unwrap());
        (refx, refy) = if !o.figure {
            (bbp_g.center().x, bbp_g.center().y)
        } else {
            (bba_f.x0, bba_f.y0)
        };
        let iextr = t(refx, refy) * s(1.0 / scalex, 1.0 / scaley) * t(-refx, -refy);
        global_transform(doc, ctx, plot, iextr, None, true);
        for v in f2.values_mut() {
            *v = transform_rect(iextr, *v);
        }
        for v in g2.values_mut() {
            *v = transform_rect(iextr, *v);
        }
        let tr_bba = bba;
        bba = Bb2::default();
        bbp = Bb2::default();
        for &el in &pels {
            bba.add(g2[&el], f2[&el]);
            if in_area(el) {
                bbp.add(g2[&el], f2[&el]);
            }
        }
        if o.figure {
            // SP:380–408: keep the figure's visual size and top-left corner
            let (oscalex, oscaley) = (scalex, scaley);
            let (trf, bbaf, bbpg) = (tr_bba.f.unwrap(), bba.f.unwrap(), bbp.g.unwrap());
            scalex = sane((trf.width() - (bbaf.width() - bbpg.width())) / bbpg.width(), "the figure's horizontal scale", &mut ctx.warn);
            scaley = sane((trf.height() - (bbaf.height() - bbpg.height())) / bbpg.height(), "the figure's vertical scale", &mut ctx.warn);
            let tlx = (trf.x0 - refx) / oscalex + refx;
            let dxl = bbpg.x0 - tlx;
            refx = if scalex != 1.0 { (trf.x0 + dxl - bbpg.x0 * scalex) / (1.0 - scalex) } else { trf.x0 + dxl };
            let tly = (trf.y0 - refy) / oscaley + refy;
            let dyl = bbpg.y0 - tly;
            refy = if scaley != 1.0 { (trf.y0 + dyl - bbpg.y0 * scaley) / (1.0 - scaley) } else { trf.y0 + dyl };
        }
    } else {
        // Task 5 replaces this branch (SP:413–440): Matching mode
        let _ = (first, &mut bbmatch);
        return;
    }
    let (bbpg, bbag) = (bbp.g.unwrap(), bba.g.unwrap());
    // SP:442–466
    let (mut finx, mut finy) = (refx, refy);
    if let (false, Mode::Matching { alignx, aligny, bbox, .. }, Some(bm)) = (cmode, o.mode, bbmatch) {
        if alignx {
            finx = bm.center().x;
        }
        if aligny {
            finy = bm.center().y;
        }
        if bbox {
            finx -= 0.5 * ((bbag.x1 - bbpg.x1) - (bbpg.x0 - bbag.x0)) * (1.0 - scalex);
            finy -= 0.5 * ((bbag.y1 - bbpg.y1) - (bbpg.y0 - bbag.y0)) * (1.0 - scaley);
        }
    }
    let gtr = t(finx, finy) * s(scalex, scaley) * t(-refx, -refy);
    let iscl = s(1.0 / scalex, 1.0 / scaley);
    let l = (scalex * scaley).abs().sqrt();
    let liscl = s(l, l) * iscl;
    let trul = gtr * Point::new(bbpg.x0, bbpg.y0);
    let trbr = gtr * Point::new(bbpg.x1, bbpg.y1);
    let thr = o.tickthr;
    for &el in &pels {
        global_transform(doc, ctx, el, gtr, None, true);
        let (fbb, gbb) = (f2[&el], g2[&el]);
        let stype: String = doc.attr(el, SCALETYPE).map(str::to_string).unwrap_or_else(|| {
            if SCALEFREE_DEFAULT.contains(&doc.tag(el)) { "scale_free" } else { "normal" }.to_string()
        });
        // SP:504–530: a tick is a short line at an edge of the plot area
        let (mut vtickt, mut vtickb, mut htickl, mut htickr) = (false, false, false, false);
        if o.tickcorrect && (pa.vl.contains(&el) || pa.hl.contains(&el)) {
            if pa.vl.contains(&el) && gbb.height() < thr * bbpg.height() {
                if gbb.y1 < bbpg.y0 + thr * bbpg.height() {
                    vtickt = true;
                } else if gbb.y0 > bbpg.y1 - thr * bbpg.height() {
                    vtickb = true;
                }
            }
            if pa.hl.contains(&el) && gbb.width() < thr * bbpg.width() {
                if gbb.x1 < bbpg.x0 + thr * bbpg.width() {
                    htickl = true;
                } else if gbb.x0 > bbpg.x1 - thr * bbpg.width() {
                    htickr = true;
                }
            }
        }
        if vtickt || vtickb || htickl || htickr {
            // SP:532–548: unscale about the edge the tick hangs on
            let gbb_tr = transform_rect(gtr, gbb);
            let (cx, cy) = (gbb_tr.center().x, gbb_tr.center().y);
            let p = if vtickt {
                Point::new(cx, if cy > trul.y { gbb_tr.y0 } else { gbb_tr.y1 })
            } else if vtickb {
                Point::new(cx, if cy < trbr.y { gbb_tr.y1 } else { gbb_tr.y0 })
            } else if htickl {
                Point::new(if cx > trul.x { gbb_tr.x0 } else { gbb_tr.x1 }, cy)
            } else {
                Point::new(if cx < trbr.x { gbb_tr.x1 } else { gbb_tr.x0 }, cy)
            };
            global_transform(doc, ctx, el, t(p.x, p.y) * iscl * t(-p.x, -p.y), None, true);
        } else if stype == "scale_free" || stype == "aspect_locked" {
            let inv = if stype == "scale_free" { iscl } else { liscl };
            // SP:562–576: an element outside the plot area keeps its distance to the area
            let offset = |gbb: Rect, cx: f64, cy: f64| -> (f64, f64) {
                let (mut dx, mut dy) = (0.0, 0.0);
                if cx < trul.x {
                    dx = (gbb.center().x - bbpg.x0) - (cx - trul.x);
                }
                if cx > trbr.x {
                    dx = (gbb.center().x - bbpg.x1) - (cx - trbr.x);
                }
                if cy < trul.y {
                    dy = (gbb.center().y - bbpg.y0) - (cy - trul.y);
                }
                if cy > trbr.y {
                    dy = (gbb.center().y - bbpg.y1) - (cy - trbr.y);
                }
                (dx, dy)
            };
            let cbc: Option<Vec<usize>> = doc
                .attr(el, COMBINED)
                .map(|v| v.split_whitespace().filter_map(|x| x.parse().ok()).collect());
            match cbc {
                None => {
                    let gbb_tr = transform_rect(gtr, gbb);
                    let (cx, cy) = (gbb_tr.center().x, gbb_tr.center().y);
                    let tr1 = t(cx, cy) * inv * t(-cx, -cy);
                    let (dx, dy) = offset(gbb, cx, cy);
                    global_transform(doc, ctx, el, t(dx, dy) * tr1, None, true);
                }
                Some(idx) => {
                    // SP:580–613: previously combined paths are unscaled piece by piece; the
                    // element already carries `gtr`, so its global points are the scaled ones
                    let Some(igtr) = inverse(gtr) else { continue };
                    let fbb_tr = transform_rect(gtr, fbb);
                    let mut ranges: Ranges = Vec::new();
                    for w in idx.windows(2) {
                        let range = w[0]..w[1];
                        let gbb_tr = geometric_bbox(doc, el, fbb_tr, Some(range.clone()));
                        let gbb = transform_rect(igtr, gbb_tr);
                        let (cx, cy) = (gbb_tr.center().x, gbb_tr.center().y);
                        let tr1 = t(cx, cy) * inv * t(-cx, -cy);
                        let (dx, dy) = offset(gbb, cx, cy);
                        ranges.push((range, t(dx, dy) * tr1));
                    }
                    global_transform(doc, ctx, el, Affine::IDENTITY, Some(ranges), true);
                }
            }
        }
        // "normal": scaled with the plot, nothing more
    }
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = ScalerCli::try_parse_from(argv).map_err(first_line)?;
    let o = Options::from_cli(&cli);
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut ctx = Ctx::new();
    // SP:231–232: selection order matters (the first selection is the Matching target)
    let sel: Vec<NodeId> = doc
        .selection_ordered(&cli.common.ids)
        .into_iter()
        .filter(|&n| !EXCLUDE_TAGS.contains(&doc.tag(n)))
        .collect();
    if let Mode::Advanced { mark } = o.mode {
        // SP:250–260
        for &el in &sel {
            match mark {
                Some(m) => doc.set_attr(el, SCALETYPE, m),
                None => {
                    doc.remove_attr(el, SCALETYPE);
                }
            }
        }
        return finish(doc, ctx);
    }
    // Deviation: upstream shows IMAGE_ERR for an EMPTY selection too (`all([])` is true)
    if sel.is_empty() {
        return Err("No objects selected!".to_string());
    }
    if sel.iter().all(|&n| doc.tag(n) == "image") {
        return Err(IMAGE_ERR.to_string());
    }
    let cmode = matches!(o.mode, Mode::Correction);
    let first = sel[0];
    let plots: Vec<NodeId> = if cmode { sel.clone() } else { sel[1..].to_vec() };
    if plots.iter().any(|&p| doc.tag(p) != "g") {
        return Err("Non-Group objects detected in selection. Objects in a plot should be grouped prior to scaling.".to_string());
    }
    let mut boxes = Boxes::compute(&mut doc, &mut ctx, &sel);
    for (i, &plot) in plots.iter().enumerate() {
        scale_plot(&mut doc, &mut ctx, &o, &mut boxes, first, plot, i, cmode);
    }
    if let Mode::Matching { deletematch: true, .. } = o.mode {
        // SP:294–295: a plain delete (not delete_up); references to it are dropped by finish
        if let Some(id) = doc.attr(first, "id") {
            ctx.deleted.insert(id.to_string());
        }
        doc.detach(first);
    }
    finish(doc, ctx)
}

fn finish(mut doc: Doc, mut ctx: Ctx) -> Result<Output, String> {
    ctx.finish(&mut doc);
    let messages = ctx.warn.0.iter().map(|w| format!("warning: {w}")).collect();
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
```

In `src/lib.rs` replace the placeholder arm `"scaler" | "homogenizer" | "favorite-markers" => …` with:

```rust
        "scaler" => tools::scaler::run(argv, input),
        "homogenizer" | "favorite-markers" => {
            Err(format!("the {tool} tool is not implemented yet"))
        }
```

Notes for the implementer: `Affine::scale_non_uniform`, `Affine::translate`, `Rect::center`, `Rect::from_points`, `Rect::union_pt` are kurbo 0.13 API; `Doc::specified(n, prop) -> Option<String>`; `Doc::transform(n)` is the element's OWN transform, `composed_transform` the product. `pa` must stay alive for the per-child loop (`pa.vl`/`pa.hl`). `Affine::IDENTITY` is a kurbo const. If `Doc::detach` on the first selection is not enough for a clean document (the element had children referencing defs), `ctx.finish` handles dangling `clip-path`/`mask` refs only — that matches upstream's plain `delete()`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test scaler 2>&1 | tail -20`
Expected: all Task 3 and Task 4 tests pass (13 tests). If `correction_restores_text_and_ticks…` fails on the tick attachment assertion by a small amount, the visual box of the tick includes half its stroke width — the assertion compares end points (`extent` uses `global_points`), so a failure there is a real defect, not a tolerance issue.

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/tools/scaler.rs src/lib.rs tests/scaler.rs
git commit -m "feat(scaler): CLI, Advanced-tab markings, Correction mode with tick and scale-free corrections"
```

---

### Task 5: Scaler Matching mode and `deletematch`

`SP:413–440` (the `else` branch Task 4 left) plus the alignment terms in `fin` (already in place) and the pre-pass refresh (already in place).

**Files:**
- Modify: `src/tools/scaler.rs` (replace the `// Task 5 replaces this branch` block)
- Test: `tests/scaler.rs` (append)

**Interfaces:**
- Consumes: Task 4's `scale_plot` skeleton, `find_plot_area`, `Boxes`.
- Produces: the completed `scale_plot`; no new public items.

- [ ] **Step 1: Write the failing tests**

Append to `tests/scaler.rs`:

```rust
/// Two plots side by side: `a` (the target, 90 × 70 plot area) and `b` (60 × 40 plot area,
/// with a label under it). `b_attrs` goes on plot b's group.
fn two_plots(b_attrs: &str) -> String {
    format!(
        r#"<svg {NS}>
<g id="a">
  <path id="abox" d="M20,10 H110 V80 H20 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="adata" d="M25,70 L60,30 L105,50" style="fill:none;stroke:#1f77b4;stroke-width:2"/>
  <text id="al" x="65" y="92" style="font-size:6px;text-anchor:middle;{DV}">a</text>
</g>
<g id="b" {b_attrs}>
  <path id="bbox" d="M200,30 H260 V70 H200 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="bdata" d="M205,60 L230,35 L255,50" style="fill:none;stroke:#d62728;stroke-width:2"/>
  <path id="bt" d="M230,70 V73" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <text id="bl" x="230" y="82" style="font-size:6px;text-anchor:middle;{DV}">b</text>
</g>
<rect id="r" x="300" y="10" width="45" height="35" style="fill:none;stroke:#000;stroke-width:1"/>
</svg>"#
    )
}

#[test]
fn matching_scales_the_plot_area_to_the_first_selection() {
    let svg = two_plots("");
    // match width only (plot areas): b's box becomes 90 wide, height unchanged, text unscaled
    let (s, msgs) = ok(&svg, &["--tab=matching", "--hmatchopts=2", "--vmatchopts=1", "--matchprop=1", "--id=a", "--id=b"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, bb) = extent(&out, "bbox");
    assert!(close(w, 90.0, 1e-6) && close(h, 40.0, 1e-6), "{w} × {h}");
    assert!(is_translation(composed(&out, "bl")), "label unscaled");
    assert!(close(visual_stroke(&out, "bdata"), 2.0, 1e-6), "visual stroke kept");
    let (_, th, tb) = extent(&out, "bt");
    assert!(close(th, 3.0, 1e-6) && close(tb.y0, bb.y1, 1e-6), "tick length kept, attached to the box");
    // the target plot is untouched
    let (aw, ah, abb) = extent(&out, "abox");
    assert!(close(aw, 90.0, 1e-9) && close(ah, 70.0, 1e-9) && close(abb.x0, 20.0, 1e-9));
    // match height and align vertically: b's box centre y equals a's, heights equal
    let (s, _) = ok(&svg, &["--tab=matching", "--hmatchopts=1", "--vmatchopts=3", "--id=a", "--id=b"]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, bb) = extent(&out, "bbox");
    assert!(close(w, 60.0, 1e-6) && close(h, 70.0, 1e-6), "{w} × {h}");
    assert!(close(bb.center().y, 45.0, 1e-6), "aligned to a's plot-area centre y (10..80): {}", bb.center().y);
    assert!(close(bb.center().x, 230.0, 1e-6), "x untouched");
}

#[test]
fn matching_can_target_a_plain_rectangle_and_delete_it() {
    let svg = two_plots("");
    let (s, msgs) = ok(&svg, &["--tab=matching", "--hmatchopts=3", "--vmatchopts=3", "--deletematch=true", "--id=r", "--id=b"]);
    assert!(msgs.is_empty(), "a stroked rectangle IS a plot area: {msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, bb) = extent(&out, "bbox");
    assert!(close(w, 45.0, 1e-6) && close(h, 35.0, 1e-6), "{w} × {h}");
    assert!(close(bb.center().x, 322.5, 1e-6) && close(bb.center().y, 27.5, 1e-6), "aligned on the rectangle's centre: {:?}", bb.center());
    assert!(out.by_id("r").is_none(), "the first selection was deleted");
    // an unstroked filled rectangle has no plot area: warning, its box is used instead
    let svg = svg.replace(r#"style="fill:none;stroke:#000;stroke-width:1""#, r#"style="fill:#ccc;stroke:none""#);
    let (s, msgs) = ok(&svg, &["--tab=matching", "--hmatchopts=2", "--vmatchopts=2", "--id=r", "--id=b"]);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("on the 1st selected plot (group ID r)"), "{}", msgs[0]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, _) = extent(&out, "bbox");
    assert!(close(w, 45.0, 1e-6) && close(h, 35.0, 1e-6));
}

#[test]
fn matching_bounding_boxes_matches_the_whole_figure() {
    let svg = two_plots("");
    let (s, _) = ok(&svg, &["--tab=matching", "--hmatchopts=2", "--vmatchopts=2", "--matchprop=2", "--id=a", "--id=b"]);
    let mut out = Doc::parse(s.as_bytes()).unwrap();
    let (a, b) = (id(&out, "a"), id(&out, "b"));
    let union_of = |m: &std::collections::HashMap<NodeId, Rect>| m.values().fold(None, |acc: Option<Rect>, r| Some(acc.map_or(*r, |u| u.union(*r)))).unwrap();
    // the match target of a group is its visual box (geometric_bbox of a non-path-like element);
    // after matching, the geometric union of b's CHILDREN has that size: the margins (label
    // below) are kept and the box grew by exactly the difference. The group's own entry is its
    // visual box (stroke-padded), so it is left out; 1e-3 covers num::fmt's 8-digit round trip.
    let (fa, _) = boxes(&mut out, a);
    let (_, mut gb) = boxes(&mut out, b);
    gb.remove(&b);
    let (ua, gb) = (union_of(&fa), union_of(&gb));
    assert!(close(gb.width(), ua.width(), 1e-3) && close(gb.height(), ua.height(), 1e-3), "{gb:?} vs {ua:?}");
}

#[test]
fn a_scaled_plot_is_corrected_before_matching_with_fresh_boxes() {
    let svg = two_plots(r#"transform="matrix(2,0,0,2,-300,-40)""#);
    let (s, msgs) = ok(&svg, &["--tab=matching", "--hmatchopts=2", "--vmatchopts=2", "--id=a", "--id=b"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, _) = extent(&out, "bbox");
    assert!(close(w, 90.0, 1e-6) && close(h, 70.0, 1e-6), "matched after the correction pre-pass: {w} × {h}");
    assert!(is_translation(composed(&out, "bl")), "the pre-pass unscaled the label: {:?}", composed(&out, "bl"));
    let (_, th, _) = extent(&out, "bt");
    assert!(close(th, 3.0, 1e-6), "and the tick: {th}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test scaler matching 2>&1 | tail -15`
Expected: the four new tests fail (Matching returns early: boxes unchanged).

- [ ] **Step 3: Implement the Matching branch**

Replace the `else { // Task 5 replaces this branch … return; }` block in `scale_plot` with:

```rust
    } else {
        // SP:413–440
        let Mode::Matching { hmatch, vmatch, bbox: matchbbox, .. } = o.mode else { return };
        bbmatch = if matchbbox {
            boxes.g.get(&first).copied()
        } else {
            let els: Vec<NodeId> = if doc.tag(first) == "g" {
                doc.children(first).filter(|&k| doc.is_element(k)).collect()
            } else {
                vec![first]
            };
            let pa0 = find_plot_area(doc, &els, &boxes.g);
            match (pa0.lvel, pa0.lhel) {
                (Some(v), Some(h)) => union(boxes.g.get(&v).copied(), boxes.g.get(&h).copied()),
                _ => {
                    if doc.tag(first) != "image" {
                        let fid = doc.attr(first, "id").unwrap_or("").to_string();
                        warn_non_plot(&mut ctx.warn, 0, &fid);
                    }
                    boxes.g.get(&first).copied()
                }
            }
        };
        let (Some(bm), Some(bbpg), Some(bbag)) = (bbmatch, bbp.g, bba.g) else {
            // the first selection has no box: nothing to match against (upstream crashes)
            ctx.warn.push("the first selection has no bounding box; nothing was matched".to_string());
            return;
        };
        scalex = 1.0;
        scaley = 1.0;
        if hmatch {
            scalex = if !matchbbox {
                bm.width() / bbpg.width()
            } else {
                (bm.width() + bbpg.width() - bbag.width()) / bbpg.width()
            };
        }
        if vmatch {
            scaley = if !matchbbox {
                bm.height() / bbpg.height()
            } else {
                (bm.height() + bbpg.height() - bbag.height()) / bbpg.height()
            };
        }
        scalex = sane(scalex, "the horizontal match scale", &mut ctx.warn);
        scaley = sane(scaley, "the vertical match scale", &mut ctx.warn);
        // SP:443–449
        (refx, refy) = if !matchbbox {
            (bbpg.center().x, bbpg.center().y)
        } else {
            (bbag.center().x, bbag.center().y)
        };
    }
```

(The alignment terms after this block — `finx = bm.center().x` etc. — are already in Task 4's code and now receive `bbmatch`.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test scaler 2>&1 | tail -20`
Expected: 17 passed.

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/tools/scaler.rs tests/scaler.rs
git commit -m "feat(scaler): Matching mode (plot areas or bounding boxes, alignment, deletematch) with a fresh-box pre-pass"
```

---

### Task 6: Scaler reference oracles, invariance sanity, `.inx`, README

Three upstream references exist for the Scaler (`C.5 (b)`): correction of `g5224` (Other_tests), matching `rect5248` → `g4982` with `--hmatchopts=2 --vmatchopts=3` (Other_tests), correction of `g109153`+`g109019` (Other_tests_nonuniform). The fourth (`--tab=scaling --hscale=120 --vscale=80`) was produced by the dropped Fixed mode and is not an oracle (today's upstream would treat `scaling` as the Advanced tab). The oracle compares GEOMETRY, not attribute text: upstream always rewrites `stroke-width` and every child's `transform`/`d`; we write the same geometry with fewer attribute changes.

**Files:**
- Create: `tests/scaler_fixtures.rs`, `inx/scaler.inx`
- Modify: `tests/invariance.rs` (append), `README.md`

**Interfaces:**
- Consumes: `sciink::tools::scaler::{global_points, PATHLIKE}`, `Doc::{parse, composed_transform, specified, by_id, descendants, is_element, tag, attr}`, `geom::scale_factor`, `support::{upstream_data_dir, with_vendored_fonts, render_png, pixel_diff_fraction}`.
- Produces: tests and the menu entry; no library changes.

- [ ] **Step 1: Write the oracle**

Create `tests/scaler_fixtures.rs`:

```rust
//! Scaler against upstream's references (`tests/upstream/data/refs/scale_plots__*.out`).
//! Geometry oracle: every path-like descendant of each plot has the same global end points
//! (± 0.02 uu — the reference prints 6 significant digits), the same visual stroke width, and
//! every text has the same global anchor (± TEXT_TOL: its pivot is its font-dependent box).
mod support;

use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::{Point, scale_factor};
use sciink::tools::scaler::global_points;
use support::with_vendored_fonts;

/// Reference coordinates carry 6 significant digits (`{:.6g}`): ± 0.02 uu on ~100 uu values.
const PATH_TOL: f64 = 0.02;
/// Text pivots are text boxes: the reference was produced with other fonts.
const TEXT_TOL: f64 = 0.5;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink").chain(v.iter().copied()).map(OsString::from).collect()
}

fn scale_fixture(svg_name: &str, ref_name: &str, extra: &[&str]) -> Option<(Doc, Doc)> {
    let dir = support::upstream_data_dir()?;
    let input = std::fs::read(dir.join(format!("svg/{svg_name}.svg"))).unwrap();
    let reference = std::fs::read(dir.join(format!("refs/{ref_name}"))).unwrap();
    let mut a = vec!["--tool=scaler"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), &input)).unwrap();
    // font warnings are expected with the vendored fonts; a plot-area warning is not
    assert!(!out.messages.iter().any(|m| m.contains("could not be automatically detected")), "{:?}", out.messages);
    Some((Doc::parse(&out.svg).unwrap(), Doc::parse(&reference).unwrap()))
}

fn ids_under(doc: &Doc, root: &str) -> Vec<String> {
    let r = doc.by_id(root).unwrap_or_else(|| panic!("no {root}"));
    doc.descendants(r)
        .filter(|&n| doc.is_element(n) && n != r)
        .filter_map(|n| doc.attr(n, "id").map(str::to_string))
        .collect()
}

fn anchor(doc: &Doc, n: NodeId) -> Option<Point> {
    let x: f64 = doc.attr(n, "x")?.split([' ', ',']).next()?.trim().parse().ok()?;
    let y: f64 = doc.attr(n, "y")?.split([' ', ',']).next()?.trim().parse().ok()?;
    Some(doc.composed_transform(n) * Point::new(x, y))
}

fn visual_stroke(doc: &Doc, n: NodeId) -> Option<f64> {
    let sw = doc.specified(n, "stroke-width")?;
    let w: f64 = sw.trim().trim_end_matches("px").parse().ok()?;
    Some(w * scale_factor(doc.composed_transform(n)))
}

/// Compares our output with the reference under each plot id; returns the largest text-anchor
/// deviation seen (the path assertions are hard).
fn compare(ours: &Doc, reference: &Doc, plots: &[&str]) -> f64 {
    let mut max_text = 0.0_f64;
    let mut paths = 0usize;
    for plot in plots {
        for id in ids_under(reference, plot) {
            let (Some(a), Some(b)) = (ours.by_id(&id), reference.by_id(&id)) else {
                panic!("{id}: present in the reference but not in our output")
            };
            let tag = reference.tag(b);
            if matches!(tag, "path" | "rect" | "line" | "polyline" | "polygon" | "circle" | "ellipse") {
                let (pa, pb) = (global_points(ours, a, None), global_points(reference, b, None));
                assert_eq!(pa.len(), pb.len(), "{id}: point count");
                for (p, q) in pa.iter().zip(&pb) {
                    assert!((p.x - q.x).abs() <= PATH_TOL && (p.y - q.y).abs() <= PATH_TOL, "{id}: {p:?} vs {q:?}");
                }
                if let (Some(wa), Some(wb)) = (visual_stroke(ours, a), visual_stroke(reference, b)) {
                    assert!((wa - wb).abs() <= 1e-3 * wb.max(1.0), "{id}: visual stroke {wa} vs {wb}");
                }
                paths += 1;
            } else if tag == "text" {
                if let (Some(p), Some(q)) = (anchor(ours, a), anchor(reference, b)) {
                    max_text = max_text.max((p.x - q.x).abs().max((p.y - q.y).abs()));
                }
            }
        }
    }
    assert!(paths > 20, "the oracle compared {paths} shapes — the ids did not line up");
    max_text
}

#[test]
fn correction_matches_the_reference_on_other_tests() {
    let Some((ours, reference)) = scale_fixture(
        "Other_tests",
        "scale_plots__--id__g5224__--tab__correction__Other_tests__svg.out",
        &["--tab=correction", "--id=g5224"],
    ) else {
        return;
    };
    let m = compare(&ours, &reference, &["g5224"]);
    eprintln!("correction g5224: max text-anchor deviation {m:.4} uu");
    assert!(m <= TEXT_TOL, "text anchors: {m}");
}

#[test]
fn matching_matches_the_reference_on_other_tests() {
    let Some((ours, reference)) = scale_fixture(
        "Other_tests",
        "scale_plots__--id__rect5248__--id__g4982__--tab__matching__--hmatchopts__2__--vmatchopts__3__Other_tests__svg.out",
        &["--tab=matching", "--hmatchopts=2", "--vmatchopts=3", "--id=rect5248", "--id=g4982"],
    ) else {
        return;
    };
    let m = compare(&ours, &reference, &["g4982"]);
    eprintln!("matching g4982: max text-anchor deviation {m:.4} uu");
    assert!(m <= TEXT_TOL, "text anchors: {m}");
    // the target rectangle is untouched
    let (a, b) = (ours.by_id("rect5248").unwrap(), reference.by_id("rect5248").unwrap());
    assert_eq!(global_points(&ours, a, None).len(), global_points(&reference, b, None).len());
}

#[test]
fn correction_matches_the_reference_on_the_non_uniform_document() {
    let Some((ours, reference)) = scale_fixture(
        "Other_tests_nonuniform",
        "scale_plots__--id__g109153__--id__g109019__--tab__correction__Other_tests_nonuniform__svg.out",
        &["--tab=correction", "--id=g109153", "--id=g109019"],
    ) else {
        return;
    };
    let m = compare(&ours, &reference, &["g109153", "g109019"]);
    eprintln!("nonuniform correction: max text-anchor deviation {m:.4} uu");
    assert!(m <= TEXT_TOL, "text anchors: {m}");
}
```

Ruling in advance (so the implementer does not stall): if the path assertions pass but a text-anchor maximum exceeds `TEXT_TOL` with the vendored fonts, keep the path assertions in the default tests, move the three `assert!(m <= TEXT_TOL …)` lines into ONE additional `#[test] #[ignore]` function gated on `std::env::var_os("SCIINK_SYSTEM_FONTS").is_some()` (as `tests/flattener_fixtures.rs`'s content oracle does), record the measured maxima (vendored and system fonts) in the report and in a comment above `TEXT_TOL`, and never raise `TEXT_TOL`. If a PATH assertion fails, that is a defect to investigate, not a tolerance to widen.

Append to `tests/invariance.rs` (reuse its existing helpers for reading a fixture and rendering; the file already has `render_png`/`pixel_diff_fraction` imports and an `args` helper — follow the shape of the existing Combine by Color test):

```rust
/// Correcting a plot that carries no scale (g4982: a pure translation) is geometrically a no-op:
/// every child's transform is fused into its path, but nothing moves.
#[test]
fn scaler_correction_of_an_unscaled_plot_is_visually_invariant() {
    let Some(dir) = support::upstream_data_dir() else { return };
    let input = std::fs::read(dir.join("svg/Other_tests.svg")).unwrap();
    let out = with_vendored_fonts(|| sciink::run(&args(&["--tool=scaler", "--tab=correction", "--id=g4982"]), &input)).unwrap();
    eprintln!("scaler identity correction: {} messages (font warnings expected)", out.messages.len());
    let (a, b) = (render_png(&input, 1500), render_png(&out.svg, 1500));
    let d = pixel_diff_fraction(&a, &b, 32);
    eprintln!("scaler identity correction: {:.4} % pixels differ", d * 100.0);
    assert!(d <= 0.001, "{d}");
}
```

- [ ] **Step 2: Run the oracles**

Run: `cargo test --test scaler_fixtures -- --nocapture 2>&1 | tail -25` and `cargo test --test invariance scaler 2>&1 | tail -8`
Expected: 3 oracle tests pass with the printed maxima (or the advance ruling above applies); the invariance test passes. Record every printed number in the report.

- [ ] **Step 3: `.inx` and README**

Create `inx/scaler.inx` (upstream's three pages, our launcher; the `pngs/scale_options.png` image and the version labels are dropped; the hidden Fixed page is not carried):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Scaler (sciink)</name>
    <id>org.sciink.scaler</id>
    <param name="tool" type="string" gui-hidden="true">scaler</param>
    <param name="tab" type="notebook">
        <page name="correction" gui-text="Correction mode">
            <label>Scale the lines and data of a plot without affecting text, ticks, and groups.</label>
            <label>Correction mode corrects plots that have already been manually scaled.</label>
            <label>1. Flatten each plot and group its objects together.</label>
            <label>2. Manually scale grouped plots to their desired size.</label>
            <label>3. Select manually-scaled plots and click Apply.</label>
            <label>Text, ticks, and groups are now restored to their original state. Note that multiple plots can be corrected at once.</label>
            <spacer/>
            <param name="figuremode" type="optiongroup" appearance="combo" gui-text="Maintain size of">
                <option value="1">Plot area (axis)</option>
                <option value="2">Bounding box (figure)</option>
            </param>
            <label appearance="header">Note</label>
            <label>It is important that you scale the plot after grouping it, because the extension infers the scale from the group's properties. Scale with Inkscape's default options (stroke widths scaled with the objects); otherwise the final stroke widths may change.</label>
            <spacer/>
            <param name="wholeplot3" type="bool" gui-text="Selection has no well-defined plot area" gui-description="Scale objects without axis or tick correction. This lets you scale arbitrary objects without affecting text or groups.">false</param>
        </page>
        <page name="matching" gui-text="Matching mode">
            <label>Scale the lines and data of a plot without affecting text, ticks, and groups.</label>
            <label>Matching mode matches plots to the size of the first selected plot, which is useful for subfigure generation.</label>
            <label>1. Flatten each plot and group its objects together.</label>
            <label>2. Select two or more grouped plots and click Apply.</label>
            <label>Every selected plot will inherit the size of the first selected plot.</label>
            <spacer/>
            <param name="hmatchopts" type="optiongroup" appearance="combo" gui-text="Horizontal " indent="0" gui-description="Matching the width makes each selection have the same width, while aligning also ensures they have the same x position.">
                <option value="1">Do not match</option>
                <option value="2">Match width</option>
                <option value="3">Match width and align</option>
            </param>
            <param name="vmatchopts" type="optiongroup" appearance="combo" gui-text="Vertical      " indent="0" gui-description="Matching the height makes each selection have the same height, while aligning also ensures they have the same y position.">
                <option value="1">Do not match</option>
                <option value="2">Match height</option>
                <option value="3">Match height and align</option>
            </param>
            <param name="matchprop" type="optiongroup" appearance="combo" gui-text="Match the " indent="0" gui-description="Property to be matched. For plot area matching, the first selection should be a grouped plot.">
                <option value="1">Plot areas</option>
                <option value="2">Bounding boxes</option>
            </param>
            <spacer/>
            <param name="deletematch" type="bool" gui-text="Delete first selection after completion?" gui-description="Useful if you are replacing the first selection with the second">false</param>
            <param name="wholeplot2" type="bool" gui-text="Selection has no well-defined plot area" gui-description="Scale objects without axis or tick correction. This lets you scale arbitrary objects without affecting text or groups.">false</param>
        </page>
        <page name="options" gui-text="Advanced">
            <label appearance="header">Tick correction</label>
            <label>If tick correction is enabled, any horizontal or vertical lines smaller than the threshold will be corrected for size and position.</label>
            <param name="tickcorrect" type="bool" gui-text="Auto tick correct?">true</param>
            <param name="tickthreshold" type="float" precision="0" min="0" max="100" gui-text="Tick threshold (% plot area)">10</param>
            <label appearance="header">Markings</label>
            <label>Running with this tab selected adds hidden markings to objects.</label>
            <label>Scale markings: Text and groups are unscaled by default, but other objects can be left unscaled or scaled with a fixed aspect ratio (useful for markers).</label>
            <label>Plot area markings: Allows objects to be used in the determination of the plot area.</label>
            <param name="marksf" type="optiongroup" appearance="combo" gui-text="Mark selection as" gui-description="Unscaled: Will not be scaled at all &#13;Scaled with locked aspect ratio: Will be scaled without affecting aspect ratio &#13;Scaled: Will be scaled, even if a text or group &#13;Plot area-determining: Allows objects to determine plot area &#13;(Clear markings): Restore default settings">
                <option value="1">Unscaled</option>
                <option value="2">Scaled with locked aspect ratio</option>
                <option value="3">Scaled</option>
                <option value="4">Plot area-determining</option>
                <option value="5">(Clear markings)</option>
            </param>
        </page>
    </param>
    <effect needs-live-preview="false">
        <object-type>all</object-type>
        <effects-menu>
            <submenu name="Scientific"/>
        </effects-menu>
    </effect>
    <script>
        <command location="inx">bin/sciink</command>
    </script>
</inkscape-extension>
```

README: change "The current release ships seven menu entries — three tools, three diagnostics and one debug editor:" to "eight menu entries — four tools, three diagnostics and one debug editor:" and insert after the Flattener bullet:

```markdown
- **Extensions ▸ Scientific ▸ Scaler** — resizes grouped plots without distorting them: Correction mode
  undoes a manual scale on text, ticks and groups (the data keeps its new size); Matching mode gives
  every selected plot the plot area (or bounding box) of the first selection, optionally aligned;
  the Advanced tab marks objects as unscaled, aspect-locked, scaled or plot-area-determining. Same
  options and defaults as the original (the original's hidden Fixed mode is gone upstream too).
```

- [ ] **Step 4: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add tests/scaler_fixtures.rs tests/invariance.rs inx/scaler.inx README.md
git commit -m "test(scaler): upstream reference oracles and identity invariance; Scaler .inx and README entry"
```

---

### Task 7: Homogenizer part 1 — CLI, selection sets, errors, `inkscape_spec_to_css`, font family, distortion fix, re-centring

`HG:44–147`, `HG:210–263` (distortion fix, font family, plain re-centring) and the font-specification parser `FP:1323–1383`. Font size, plot-aware re-centring, stroke width, fusing and clip clearing are Task 8.

**Files:**
- Create: `src/tools/homogenizer.rs`
- Modify: `src/tools/mod.rs` (`pub mod homogenizer;`), `src/lib.rs` (dispatch arm `"homogenizer"`)
- Test: `tests/homogenizer.rs` (new)

**Interfaces:**
- Consumes: `Ctx::{parse_text, reset_char_table}` (Task 2), `FontSystem::{load, families}` (Task 1), `ops::bbox::bb2`, `ops::xform::global_transform`, `ops::style::remove_inline`, `text::style::{composed_width, baseline_shift}`, `Doc::{selection, descendants, set_style, sheet_value, px_per_uu}`, `tools::scaler::{find_plot_area, geometric_bbox, warn_non_plot}`.
- Produces: `pub struct HomogenizerCli` (fields named after the `.inx` parameters, clap defaults = upstream's argparse defaults: every switch `false`, `fontsize 8`, `setstrokew 1`, `fontmodes 1`, `strokemodes 1`, `fontfamily ""`); `pub const BAD_TAGS`, `TEXTLIKE`, `IMAGE_ERR`, `INVALID_FONT`; `pub fn inkscape_spec_to_css(fstr: &str, families: &[String]) -> Option<Style>`; `pub(crate) fn fix_distortion(doc, ctx, tels: &[NodeId])`, `set_font_family(doc, ctx, sel_text: &[NodeId], spec: &str) -> Result<(), String>`, `recentre(doc, ctx, sel0: &[NodeId], tels: &[NodeId], bbs: &HashMap<NodeId, Rect>, plotaware: bool)` (Task 8 fills the plot-aware branch), `pub fn run`.

- [ ] **Step 1: Write the failing tests**

Create `tests/homogenizer.rs`:

```rust
mod support;

use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::Rect;
use sciink::ops::Ctx;
use sciink::ops::bbox::bb2;
use sciink::tools::homogenizer::inkscape_spec_to_css;
use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink").chain(v.iter().copied()).map(OsString::from).collect()
}
fn homog(svg: &str, extra: &[&str]) -> Result<(String, Vec<String>), String> {
    let mut a = vec!["--tool=homogenizer", "--tab=scaling"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes()))?;
    Ok((String::from_utf8(out.svg).unwrap(), out.messages))
}
fn ok(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    homog(svg, extra).unwrap_or_else(|e| panic!("homogenizer failed: {e}"))
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap_or_else(|| panic!("no element {i}"))
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants().find(|n| n.attribute("id") == Some(id)).unwrap_or_else(|| panic!("no element {id}"))
}
fn style_of(n: roxmltree::Node) -> sciink::style::Style {
    n.attribute("style").map(sciink::style::Style::parse).unwrap_or_default()
}
fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
/// Visual box of one element, measured by the crate itself.
fn vbox(svg: &str, id_: &str) -> Rect {
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let n = id(&doc, id_);
    let mut ctx = Ctx::new();
    let m = with_vendored_fonts(|| bb2(&mut doc, &mut ctx, &[n], false));
    m[&n]
}
fn composed(svg: &str, id_: &str) -> [f64; 6] {
    let doc = Doc::parse(svg.as_bytes()).unwrap();
    doc.composed_transform(id(&doc, id_)).as_coeffs()
}

#[test]
fn inkscape_font_specifications_become_css() {
    let fams: Vec<String> = ["DejaVu Sans", "Roboto", "Avenir Next"].iter().map(|s| s.to_string()).collect();
    let css = |s: &str| inkscape_spec_to_css(s, &fams).map(|st| st.to_css());
    // `Style::to_css` joins declarations with ";" and writes no trailing one
    assert_eq!(css("DejaVu Sans"), Some("font-family:DejaVu Sans".to_string()));
    assert_eq!(css("DejaVu Sans Bold"), Some("font-family:DejaVu Sans;font-weight:bold".to_string()));
    // punctuation and case are ignored (upstream strips punctuation without inserting spaces, so a
    // hyphenated "dejavu-sans" can never match "DejaVu Sans" — upstream rejects it too)
    assert_eq!(css("dejavu sans, Bold Italic"), Some("font-family:DejaVu Sans;font-weight:bold;font-style:italic".to_string()));
    assert_eq!(css("Bold DejaVu Sans"), css("DejaVu Sans Bold"), "the family may come last");
    assert_eq!(css("Avenir Next Semi-Condensed"), Some("font-family:Avenir Next;font-stretch:semi-condensed".to_string()));
    assert_eq!(css("Roboto Weight500"), Some("font-family:Roboto;font-weight:500".to_string()));
    assert_eq!(css("Roboto Semi-Bold"), Some("font-family:Roboto;font-weight:600".to_string()));
    assert_eq!(css("Roboto Normal"), Some("font-family:Roboto;font-weight:normal;font-style:normal;font-stretch:normal".to_string()), "Normal is a weight, a style and a stretch");
    assert_eq!(css("Sans Light"), Some("font-family:Sans;font-weight:300".to_string()), "generic families are always known");
    assert_eq!(css("Nope Sans"), None, "no family and an unknown word");
    assert_eq!(css("Roboto Sparkly"), None, "an unknown style word rejects the whole specification");
    assert_eq!(css(""), Some(String::new()), "an empty specification sets nothing");
}

#[test]
fn errors_follow_upstream() {
    let svg = format!(r#"<svg {NS}><image id="i" width="1" height="1"/><rect id="r" width="1" height="1"/><g id="g"><text id="t" style="{DV}">x</text></g></svg>"#);
    let e = homog(&svg, &["--id=i"]).unwrap_err();
    assert!(e.starts_with("Thanks for using Scientific Inkscape!"), "{e}");
    let e = homog(&svg, &["--plotaware=true", "--id=r"]).unwrap_err();
    assert_eq!(e, "Plot-aware scaling requires that every selected object be a grouped plot.");
    let e = homog(&svg, &["--setfontfamily=true", "--fontfamily=Nope Sans", "--id=g"]).unwrap_err();
    assert_eq!(e, "Font seems to be invalid—check its spelling.");
    // an empty selection is a no-op with a message, not an error
    let (s, msgs) = ok(&svg, &[]);
    assert_eq!(msgs, vec!["homogenizer: nothing selected".to_string()]);
    assert!(s.contains(r#"id="t""#));
}

#[test]
fn set_font_family_rewrites_the_family_drops_the_specification_and_keeps_the_centre() {
    let svg = format!(
        r#"<svg {NS}><g id="g"><text id="t" x="30" y="20" style="font-size:10px;font-family:Roboto;-inkscape-font-specification:'Roboto Bold';text-anchor:start">Hello <tspan id="s" style="font-family:Roboto;font-weight:bold">world</tspan></text></g></svg>"#
    );
    let before = vbox(&svg, "t");
    let (s, msgs) = ok(&svg, &["--setfontfamily=true", "--fontfamily=DejaVu Sans Bold", "--id=g"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    for i in ["t", "s"] {
        let st = style_of(by_id(&d, i));
        assert_eq!(st.get("font-family"), Some("DejaVu Sans"), "{i}");
        assert_eq!(st.get("font-weight"), Some("bold"), "{i}");
        assert_eq!(st.get("font-style"), Some("normal"), "{i}: the other font properties are reset");
        assert_eq!(st.get("font-stretch"), Some("normal"), "{i}");
        assert_eq!(st.get("-inkscape-font-specification"), None, "{i}");
    }
    // the text moved so that its visual box keeps its centre (DejaVu Sans is wider than Roboto)
    let after = vbox(&s, "t");
    assert!(!close(after.width(), before.width(), 1e-3), "the box did change: {before:?} vs {after:?}");
    assert!(close(after.center().x, before.center().x, 1e-6) && close(after.center().y, before.center().y, 1e-6), "{before:?} vs {after:?}");
    // font-size alone does not move a text whose family stays
    let (s2, _) = ok(&svg, &["--setfontfamily=true", "--fontfamily=Roboto", "--id=g"]);
    let again = vbox(&s2, "t");
    assert!(close(again.center().x, before.center().x, 1e-6));
}

#[test]
fn distorted_text_becomes_conformal_and_keeps_its_centre() {
    let svg = format!(
        r#"<svg {NS}><g id="g"><text id="t" x="10" y="20" transform="matrix(2,0,0,1,10,20)" style="font-size:10px;{DV}">Hi</text><text id="f" x="10" y="20" transform="matrix(1,0,0,-2,0,0)" style="font-size:10px;{DV}">flip</text></g></svg>"#
    );
    let (bt, bf) = (vbox(&svg, "t"), vbox(&svg, "f"));
    let (s, msgs) = ok(&svg, &["--fixtextdistortion=true", "--id=g"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    // coefficients are re-read from the serialised output (`num::fmt`, 8 significant digits)
    let c = composed(&s, "t");
    let q = 2.0_f64.sqrt();
    assert!(close(c[0], q, 1e-6) && close(c[1], 0.0, 1e-6) && close(c[2], 0.0, 1e-6) && close(c[3], q, 1e-6), "uniform sqrt(det): {c:?}");
    let c = composed(&s, "f");
    assert!(close(c[0], q, 1e-6) && close(c[3], -q, 1e-6), "a flip stays a flip: {c:?}");
    let (at, af) = (vbox(&s, "t"), vbox(&s, "f"));
    assert!(close(at.center().x, bt.center().x, 1e-6) && close(at.center().y, bt.center().y, 1e-6), "{bt:?} vs {at:?}");
    assert!(close(af.center().x, bf.center().x, 1e-6) && close(af.center().y, bf.center().y, 1e-6));
    // a tspan never gets a transform of its own (Deviation: upstream writes one)
    let svg = format!(r#"<svg {NS}><text id="t" transform="scale(2,1)" style="font-size:10px;{DV}">a<tspan id="s">b</tspan></text></svg>"#);
    let (s, _) = ok(&svg, &["--fixtextdistortion=true", "--id=t"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(by_id(&d, "s").attribute("transform"), None);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test homogenizer 2>&1 | tail -15`
Expected: compile error — `sciink::tools::homogenizer` does not exist.

- [ ] **Step 3: Implement**

Create `src/tools/homogenizer.rs`:

```rust
//! Homogenizer (spec §B.3 "Homogenizer"; upstream homogenizer.py): sets font size, font family,
//! stroke width and transform hygiene on a selection without moving anything's centre.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::geom::{Affine, Rect, inverse};
use crate::ops::Ctx;
use crate::ops::bbox::bb2;
use crate::ops::style::remove_inline;
use crate::ops::xform::global_transform;
use crate::style::Style;
use crate::text::fonts::FontSystem;

use super::first_line;

/// Never restyled (`HG:31–39`).
pub const BAD_TAGS: &[&str] = &["namedview", "defs", "metadata", "foreignObject", "font", "font-face", "missing-glyph"];
/// Text-like elements the text options touch (`HG:139–143`).
pub const TEXTLIKE: &[&str] = &["text", "tspan", "flowRoot", "flowPara", "flowSpan"];
/// `HG:122–130` (whitespace normalised).
pub const IMAGE_ERR: &str = "Thanks for using Scientific Inkscape!\n\nIt appears that you're attempting to homogenize a raster Image object. Please note that Inkscape is mainly for working with vector images, not raster images. Vector images preserve all of the information used to generate them, whereas raster images do not. Read about the difference here:\nhttps://en.wikipedia.org/wiki/Vector_graphics\n\nUnfortunately, this means that there is not much the Homogenizer can do to edit raster images. If you want to edit a raster image, you will need to use a program like Photoshop or GIMP.";
/// `HG:231`.
pub const INVALID_FONT: &str = "Font seems to be invalid—check its spelling.";

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct HomogenizerCli {
    #[command(flatten)]
    pub common: Common,
    #[arg(long, default_value = "scaling")]
    pub tab: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setfontsize: bool,
    #[arg(long, default_value_t = 8.0)]
    pub fontsize: f64,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub fixtextdistortion: bool,
    /// 2 fixed pt, 3 scale %, 4 scale max to pt, 5 mean, 6 median, 7 min, 8 max
    #[arg(long, default_value_t = 1)]
    pub fontmodes: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setfontfamily: bool,
    #[arg(long, default_value = "")]
    pub fontfamily: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setstroke: bool,
    #[arg(long, default_value_t = 1.0)]
    pub setstrokew: f64,
    /// 2 fixed px, 3 scale %, 5 mean, 6 median, 7 min, 8 max
    #[arg(long, default_value_t = 1)]
    pub strokemodes: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub clearclipmasks: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub fusetransforms: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub plotaware: bool,
}

/// `FP:1323–1383`: an Inkscape font specification ("DejaVu Sans Bold Italic") to CSS. The
/// longest run of words at the start or the end that names an installed family (or Serif, Sans,
/// System-ui, Monospace) is the family; every remaining word must be a Pango weight, style or
/// stretch word (or `weightNNN`), else `None`. Punctuation and case are ignored.
pub fn inkscape_spec_to_css(fstr: &str, families: &[String]) -> Option<Style> {
    fn clean(s: &str) -> String {
        let kept: String = s.chars().filter(|c| c.is_alphanumeric() || *c == '_' || c.is_whitespace()).collect();
        kept.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
    }
    const WEIGHTS: &[(&str, &str)] = &[
        ("ultralight", "200"), ("light", "300"), ("semilight", "350"), ("medium", "500"),
        ("semibold", "600"), ("bold", "bold"), ("ultrabold", "800"), ("heavy", "900"),
        ("normal", "normal"), ("book", "380"), ("thin", "100"), ("ultraheavy", "1000"),
    ];
    const STRETCHES: &[(&str, &str)] = &[
        ("ultracondensed", "ultra-condensed"), ("extracondensed", "extra-condensed"),
        ("condensed", "condensed"), ("semicondensed", "semi-condensed"), ("normal", "normal"),
        ("semiexpanded", "semi-expanded"), ("expanded", "expanded"),
        ("extraexpanded", "extra-expanded"), ("ultraexpanded", "ultra-expanded"),
    ];
    const STYLES: &[(&str, &str)] = &[("italic", "italic"), ("oblique", "oblique"), ("normal", "normal")];
    // a closure cannot name the returned lifetime; a nested fn can
    fn look<'a>(t: &[(&str, &'a str)], w: &str) -> Option<&'a str> {
        t.iter().find(|(k, _)| *k == w).map(|(_, v)| *v)
    }

    let cstr = clean(fstr);
    let mut fullfams: Vec<String> = families.to_vec();
    fullfams.extend(["Serif", "Sans", "System-ui", "Monospace"].map(String::from));
    let fmnames: Vec<String> = fullfams.iter().map(|f| clean(f)).collect();
    let words: Vec<&str> = cstr.split(' ').filter(|w| !w.is_empty()).collect();
    let (mut longest, mut match_len, mut prefix) = (String::new(), 0usize, true);
    for i in 1..=words.len() {
        let cur = words[..i].join(" ");
        if fmnames.contains(&cur) && cur.len() > longest.len() {
            (longest, match_len, prefix) = (cur, i, true);
        }
    }
    for i in 1..=words.len() {
        let cur = words[words.len() - i..].join(" ");
        if fmnames.contains(&cur) && cur.len() > longest.len() {
            (longest, match_len, prefix) = (cur, i, false);
        }
    }
    let (fam, stylews): (Option<&str>, Vec<&str>) = if longest.is_empty() {
        (None, words.clone())
    } else {
        let idx = fmnames.iter().position(|n| *n == longest).expect("matched above");
        let rest = if prefix { words[match_len..].to_vec() } else { words[..words.len() - match_len].to_vec() };
        (Some(fullfams[idx].as_str()), rest)
    };
    let (mut weight, mut style, mut stretch): (Option<String>, Option<&str>, Option<&str>) = (None, None, None);
    for w in stylews {
        let mut understood = false;
        if let Some(v) = look(WEIGHTS, w) {
            weight = Some(v.to_string());
            understood = true;
        } else if let Some(d) = w.strip_prefix("weight").filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())) {
            weight = Some(d.to_string());
            understood = true;
        }
        if let Some(v) = look(STYLES, w) {
            style = Some(v);
            understood = true;
        }
        if let Some(v) = look(STRETCHES, w) {
            stretch = Some(v);
            understood = true;
        }
        if !understood {
            return None;
        }
    }
    let mut sty = Style::default();
    if let Some(f) = fam {
        sty.set("font-family", f);
    }
    if let Some(w) = weight {
        sty.set("font-weight", &w);
    }
    if let Some(s) = style {
        sty.set("font-style", s);
    }
    if let Some(s) = stretch {
        sty.set("font-stretch", s);
    }
    Some(sty)
}

/// `HG:210–225`: replace each text's composed transform by the conformal one with the same
/// area (`sqrt|det|`), rotation and flip. Deviation: only `text`/`flowRoot` — upstream also
/// "fixes" tspans, writing `transform` attributes they cannot carry.
pub(crate) fn fix_distortion(doc: &mut Doc, ctx: &mut Ctx, tels: &[NodeId]) {
    for &el in tels {
        let ct = doc.composed_transform(el);
        let [a, b, c, d, e, f] = ct.as_coeffs();
        let det = a * d - b * c;
        let m = (a * a + b * b).sqrt();
        if det == 0.0 || m == 0.0 {
            continue;
        }
        let sgn = if det < 0.0 { -1.0 } else { 1.0 };
        let q = det.abs().sqrt();
        let ctnew = Affine::new([a * q / m, b * q / m, -b * q * sgn / m, a * q * sgn / m, e, f]);
        let Some(ict) = inverse(ct) else { continue };
        global_transform(doc, ctx, el, ctnew * ict, None, true);
    }
}

/// `HG:227–246`: the specification's CSS onto every text-like element (children last, as
/// upstream's `reversed(sel)`), the Inkscape specification dropped. Deviation: upstream's
/// `character_fixer` (Avenir/Whitney non-letters into 'Avenir Next'/'Arial' tspans) is not
/// ported — the text engine falls back per character when a face lacks a glyph.
pub(crate) fn set_font_family(doc: &mut Doc, ctx: &mut Ctx, sel_text: &[NodeId], spec: &str) -> Result<(), String> {
    let fonts = FontSystem::load();
    let Some(mut sty) = inkscape_spec_to_css(spec, &fonts.families()) else {
        return Err(INVALID_FONT.to_string());
    };
    const FACE: [&str; 3] = ["font-weight", "font-style", "font-stretch"];
    if FACE.iter().any(|k| sty.get(k).is_some()) {
        for k in FACE {
            if sty.get(k).is_none() {
                sty.set(k, "normal");
            }
        }
    }
    for &el in sel_text.iter().rev() {
        for (k, v) in &sty.0 {
            doc.set_style(el, k, v);
        }
        remove_inline(doc, el, "-inkscape-font-specification");
    }
    ctx.reset_char_table();
    Ok(())
}

/// `HG:248–319`: after restyling, move every text so its visual box keeps the centre it had
/// (`bbs` = boxes before). Plot-aware (Task 8): texts outside a plot area keep their scaled
/// distance to it instead.
pub(crate) fn recentre(doc: &mut Doc, ctx: &mut Ctx, sel0: &[NodeId], tels: &[NodeId], bbs: &HashMap<NodeId, Rect>, plotaware: bool) {
    ctx.reset_char_table(); // sizes and families changed: measure with a fresh table (BB2(…, True))
    let bbs2 = bb2(doc, ctx, tels, false);
    if !plotaware {
        for &el in tels {
            let (Some(b1), Some(b2)) = (bbs.get(&el), bbs2.get(&el)) else { continue };
            let d = b1.center() - b2.center();
            global_transform(doc, ctx, el, Affine::translate((d.x, d.y)), None, true);
        }
        return;
    }
    // Task 8 fills this branch (HG:265–319)
    let _ = sel0;
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = HomogenizerCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut ctx = Ctx::new();
    let mut messages: Vec<String> = Vec::new();
    let sel0 = doc.selection(&cli.common.ids);
    // HG:118: the selection and every descendant, each once, document order
    let mut sel: Vec<NodeId> = Vec::new();
    let mut seen: HashSet<NodeId> = HashSet::new();
    for &r in &sel0 {
        for n in doc.descendants(r).filter(|&n| doc.is_element(n)) {
            if seen.insert(n) {
                sel.push(n);
            }
        }
    }
    if sel0.is_empty() {
        // Deviation: upstream shows IMAGE_ERR for an empty selection (`all([])` is true)
        messages.push("homogenizer: nothing selected".to_string());
        return finish(doc, ctx, messages);
    }
    if sel.iter().all(|&n| doc.tag(n) == "image") {
        return Err(IMAGE_ERR.to_string());
    }
    if cli.plotaware && sel0.iter().any(|&n| doc.tag(n) != "g") {
        return Err("Plot-aware scaling requires that every selected object be a grouped plot.".to_string());
    }
    let sela: Vec<NodeId> = sel.iter().copied().filter(|&n| !BAD_TAGS.contains(&doc.tag(n))).collect();
    let sel_text: Vec<NodeId> = sel.iter().copied().filter(|&n| TEXTLIKE.contains(&doc.tag(n))).collect();
    let tels: Vec<NodeId> = sel_text.iter().copied().filter(|&n| matches!(doc.tag(n), "text" | "flowRoot")).collect();
    let text_opts = cli.setfontfamily || cli.setfontsize || cli.fixtextdistortion;
    // HG:145–151: the boxes before any change
    let bbs: HashMap<NodeId, Rect> = if !text_opts {
        HashMap::new()
    } else if cli.plotaware {
        bb2(&mut doc, &mut ctx, &sel, false)
    } else {
        bb2(&mut doc, &mut ctx, &tels, false)
    };
    if cli.setfontsize {
        // Task 8: set_font_size(&mut doc, &mut ctx, &tels, cli.fontsize, cli.fontmodes);
    }
    if cli.fixtextdistortion {
        fix_distortion(&mut doc, &mut ctx, &tels);
    }
    if cli.setfontfamily {
        set_font_family(&mut doc, &mut ctx, &sel_text, &cli.fontfamily)?;
    }
    if text_opts {
        recentre(&mut doc, &mut ctx, &sel0, &tels, &bbs, cli.plotaware);
    }
    if cli.setstroke {
        // Task 8: set_stroke(&mut doc, &mut ctx, &sela, cli.setstrokew, cli.strokemodes);
    }
    if cli.fusetransforms {
        // Task 8: fuse_all(&mut doc, &mut ctx, &sela);
    }
    if cli.clearclipmasks {
        // Task 8: clear_clipmasks(&mut doc, &sela);
    }
    let _ = &sela;
    finish(doc, ctx, messages)
}

fn finish(mut doc: Doc, mut ctx: Ctx, mut messages: Vec<String>) -> Result<Output, String> {
    ctx.finish(&mut doc);
    messages.extend(ctx.warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
```

Register `pub mod homogenizer;` in `src/tools/mod.rs` (alphabetical, after `font_probe`) and change the `src/lib.rs` arm to `"homogenizer" => tools::homogenizer::run(argv, input),` leaving `"favorite-markers" => Err(format!("the {tool} tool is not implemented yet")),`. The `let _ = (…)` line in `recentre` and the `let _ = &sela;` line exist only so clippy's `-D warnings` gate passes with Task 8's pieces still missing; Task 8 removes them.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test homogenizer 2>&1 | tail -15`
Expected: 4 passed. The `set_font_family…` test's width assertion needs the two vendored families to differ in advance widths (they do: DejaVu Sans is ~10 % wider than Roboto).

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/tools/homogenizer.rs src/tools/mod.rs src/lib.rs tests/homogenizer.rs
git commit -m "feat(homogenizer): CLI, font specification parser, font family, distortion fix, re-centring"
```

---

### Task 8: Homogenizer part 2 — font size modes, plot-aware re-centring, stroke width modes, fusing, clip clearing

`HG:153–208`, `HG:265–319`, `HG:321–375`.

**Files:**
- Modify: `src/tools/homogenizer.rs`
- Test: `tests/homogenizer.rs` (append)

**Interfaces:**
- Consumes: `Ctx::parse_text` (Task 2), `text::style::{composed_width, baseline_shift}` (`composed_width(doc, n, prop) -> FontSize { tfs, scf, utfs }`; `baseline_shift(doc, style_node, &Style) -> f64`), `Doc::px_per_uu` (Task 2), `ops::xform::{fuse, OTP_SUPPORT}`, `Doc::sheet_value`, `num::fmt`.
- Produces: `pub(crate) fn set_font_size(doc, ctx, tels, fontsize: f64, mode: u8)`, `pub fn fmt_font_size(v: f64) -> String` (upstream's rounding then `num::fmt`, with `px`), `pub(crate) fn set_stroke(doc, ctx, sela, setstrokew: f64, mode: u8)`, `pub(crate) fn fuse_all(doc, ctx, sela)`, `pub(crate) fn clear_clipmasks(doc, sela)`; the completed `recentre`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/homogenizer.rs`:

```rust
/// Three texts of 10, 20 and 40 px (7.5, 15 and 30 pt at 1 px/uu) in a group.
fn three_texts() -> String {
    format!(
        r#"<svg {NS}><g id="g">
<text id="a" x="10" y="20" style="font-size:10px;{DV}">aaa</text>
<text id="b" x="10" y="50" style="font-size:20px;{DV}">bbb</text>
<text id="c" x="10" y="100" style="font-size:40px;{DV}">ccc</text>
</g></svg>"#
    )
}
fn font_size(s: &str, id_: &str) -> String {
    let d = roxmltree::Document::parse(s).unwrap();
    style_of(by_id(&d, id_)).get("font-size").unwrap().to_string()
}

#[test]
fn font_size_modes_follow_upstream() {
    let svg = three_texts();
    // 2: fixed 7 pt → 7 × 4/3 px = 9.33px everywhere
    let (s, _) = ok(&svg, &["--setfontsize=true", "--fontmodes=2", "--fontsize=7", "--id=g"]);
    for i in ["a", "b", "c"] {
        assert_eq!(font_size(&s, i), "9.33px", "{i}");
    }
    // 3: scale 50 %
    let (s, _) = ok(&svg, &["--setfontsize=true", "--fontmodes=3", "--fontsize=50", "--id=g"]);
    assert_eq!((font_size(&s, "a"), font_size(&s, "b"), font_size(&s, "c")), ("5px".into(), "10px".into(), "20px".into()));
    // 4: scale so the largest becomes 15 pt (20 px): everything halves
    let (s, _) = ok(&svg, &["--setfontsize=true", "--fontmodes=4", "--fontsize=15", "--id=g"]);
    assert_eq!((font_size(&s, "a"), font_size(&s, "b"), font_size(&s, "c")), ("5px".into(), "10px".into(), "20px".into()));
    // 5 mean 17.5 pt = 23.33px, 6 median 15 pt = 20px, 7 min 7.5 pt = 10px, 8 max 30 pt = 40px
    for (mode, want) in [("5", "23.33px"), ("6", "20px"), ("7", "10px"), ("8", "40px")] {
        let (s, _) = ok(&svg, &["--setfontsize=true", &format!("--fontmodes={mode}"), "--id=g"]);
        for i in ["a", "b", "c"] {
            assert_eq!(font_size(&s, i), want, "mode {mode}, {i}");
        }
    }
    // small values keep three significant digits: 1 px × 50 % = 0.5px
    let svg = format!(r#"<svg {NS}><text id="t" style="font-size:1px;{DV}">x</text></svg>"#);
    let (s, _) = ok(&svg, &["--setfontsize=true", "--fontmodes=3", "--fontsize=50", "--id=t"]);
    assert_eq!(font_size(&s, "t"), "0.5px");
}

#[test]
fn font_size_respects_transforms_document_scale_and_relative_spans() {
    // 2 px per uu (width 200 px over a 100-unit viewBox): 7 pt = 9.333 px = 4.667 uu; the group
    // scales by 2, so the untransformed size written is 4.667 / 2 = 2.33px
    let svg = format!(
        r#"<svg {NS} width="200" height="100" viewBox="0 0 100 50"><g id="g" transform="scale(2)"><text id="t" x="5" y="10" style="font-size:10px;{DV}">Hi <tspan id="p" style="font-size:50%">half</tspan> <tspan id="s" style="font-size:65%;baseline-shift:super">2</tspan> <tspan id="k" style="font-size:20px">big</tspan></text></g></svg>"#
    );
    let before = vbox(&svg, "t");
    let (s, msgs) = ok(&svg, &["--setfontsize=true", "--fontmodes=2", "--fontsize=7", "--id=g"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    assert_eq!(font_size(&s, "t"), "2.33px");
    // relative spans stay relative: their size as a percentage of the parent's
    assert_eq!(font_size(&s, "p"), "50.00%");
    assert_eq!(font_size(&s, "s"), "65.00%", "a superscript is relative even if its size were absolute");
    // an absolute span becomes the target size too
    assert_eq!(font_size(&s, "k"), "2.33px");
    // and the text keeps its centre
    let after = vbox(&s, "t");
    assert!(close(after.center().x, before.center().x, 1e-6) && close(after.center().y, before.center().y, 1e-6), "{before:?} vs {after:?}");
}

#[test]
fn plot_aware_recentring_keeps_the_scaled_distance_to_the_plot_area() {
    let svg = format!(
        r#"<svg {NS}><g id="plot">
  <path id="box" d="M40,10 H140 V80 H40 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <text id="yl" x="30" y="45" style="font-size:8px;text-anchor:end;{DV}">left</text>
  <text id="in" x="90" y="45" style="font-size:8px;text-anchor:middle;{DV}">inside</text>
  <text id="bl" x="90" y="95" style="font-size:8px;text-anchor:middle;{DV}">below</text>
</g></svg>"#
    );
    let (b_yl, b_in, b_bl) = (vbox(&svg, "yl"), vbox(&svg, "in"), vbox(&svg, "bl"));
    let (s, msgs) = ok(&svg, &["--setfontsize=true", "--fontmodes=3", "--fontsize=200", "--plotaware=true", "--id=plot"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let (a_yl, a_in, a_bl) = (vbox(&s, "yl"), vbox(&s, "in"), vbox(&s, "bl"));
    // left of the plot area (x0 = 40): the gap to the area scales with the box width
    let gap_before = 40.0 - b_yl.x1;
    let gap_after = 40.0 - a_yl.x1;
    assert!(close(gap_after, gap_before * a_yl.width() / b_yl.width(), 1e-6), "{gap_before} → {gap_after}");
    assert!(close(a_yl.center().y, b_yl.center().y, 1e-6), "vertically inside: centred");
    // inside: centred both ways
    assert!(close(a_in.center().x, b_in.center().x, 1e-6) && close(a_in.center().y, b_in.center().y, 1e-6));
    // below the plot area (y1 = 80): the gap scales with the box height
    let gap_before = b_bl.y0 - 80.0;
    let gap_after = a_bl.y0 - 80.0;
    assert!(close(gap_after, gap_before * a_bl.height() / b_bl.height(), 1e-6), "{gap_before} → {gap_after}");
    // a group without a plot area warns and falls back to plain centring
    let svg = format!(r#"<svg {NS}><g id="plot"><text id="t" x="10" y="10" style="font-size:8px;{DV}">alone</text></g></svg>"#);
    let b = vbox(&svg, "t");
    let (s, msgs) = ok(&svg, &["--setfontsize=true", "--fontmodes=3", "--fontsize=200", "--plotaware=true", "--id=plot"]);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("on the 1st selected plot (group ID plot)"), "{}", msgs[0]);
    let a = vbox(&s, "t");
    assert!(close(a.center().x, b.center().x, 1e-6) && close(a.center().y, b.center().y, 1e-6));
}

#[test]
fn stroke_width_modes_follow_upstream_on_stroked_elements_only() {
    // 2 px per uu; the group scales by 4 (sf = 4)
    let svg = format!(
        r#"<svg {NS} width="200" height="100" viewBox="0 0 100 50"><g id="g" transform="scale(4)">
<path id="a" d="M0,0 H10" style="fill:none;stroke:#000;stroke-width:0.25"/>
<path id="b" d="M0,1 H10" style="fill:none;stroke:#000;stroke-width:0.5"/>
<path id="c" d="M0,2 H10" style="fill:none;stroke:#000;stroke-width:1.5"/>
<path id="n" d="M0,3 H10" style="fill:#000;stroke:none"/>
<path id="u" d="M0,4 H10"/>
</g></svg>"#
    );
    let sw = |s: &str, i: &str| -> Option<String> {
        let d = roxmltree::Document::parse(s).unwrap();
        style_of(by_id(&d, i)).get("stroke-width").map(str::to_string)
    };
    // 2: fixed 3 px = 1.5 uu visual → written 1.5 / 4 = 0.375px
    let (s, msgs) = ok(&svg, &["--setstroke=true", "--strokemodes=2", "--setstrokew=3", "--id=g"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    for i in ["a", "b", "c"] {
        assert_eq!(sw(&s, i).as_deref(), Some("0.375px"), "{i}");
    }
    assert_eq!(sw(&s, "n").as_deref(), None, "stroke:none gets no width (Deviation)");
    assert_eq!(sw(&s, "u").as_deref(), None, "no stroke at all gets no width");
    // 3: scale 50 %
    let (s, _) = ok(&svg, &["--setstroke=true", "--strokemodes=3", "--setstrokew=50", "--id=g"]);
    assert_eq!((sw(&s, "a").unwrap(), sw(&s, "b").unwrap(), sw(&s, "c").unwrap()), ("0.125px".to_string(), "0.25px".to_string(), "0.75px".to_string()));
    // visual widths 1, 2, 6: mean 3 → 0.75px, median 2 → 0.5px, min 1 → 0.25px, max 6 → 1.5px
    for (mode, want) in [("5", "0.75px"), ("6", "0.5px"), ("7", "0.25px"), ("8", "1.5px")] {
        let (s, _) = ok(&svg, &["--setstroke=true", &format!("--strokemodes={mode}"), "--id=g"]);
        for i in ["a", "b", "c"] {
            assert_eq!(sw(&s, i).as_deref(), Some(want), "mode {mode}, {i}");
        }
    }
    // no stroked element at all: a warning, nothing written
    let svg = format!(r#"<svg {NS}><path id="n" d="M0,0 H1" style="fill:#000;stroke:none"/></svg>"#);
    let (s, msgs) = ok(&svg, &["--setstroke=true", "--strokemodes=5", "--id=n"]);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("no stroked elements"), "{}", msgs[0]);
    assert_eq!(sw(&s, "n"), None);
}

#[test]
fn fuse_transforms_puts_path_data_in_global_coordinates_and_keeps_appearance() {
    let svg = format!(
        r#"<svg {NS}><g id="g" transform="scale(2)"><path id="p" transform="translate(1,1)" d="M0,0 L1,0" style="fill:none;stroke:#000;stroke-width:1"/><text id="t" transform="translate(3,3)" style="font-size:4px;{DV}">t</text></g></svg>"#
    );
    let (s, msgs) = ok(&svg, &["--fusetransforms=true", "--id=g"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let p = id(&out, "p");
    assert_ne!(out.attr(p, "d"), Some("M0,0 L1,0"), "the path data was rewritten");
    let pts = sciink::tools::scaler::global_points(&out, p, None);
    assert!(close(pts[0].x, 2.0, 1e-9) && close(pts[0].y, 2.0, 1e-9) && close(pts[1].x, 4.0, 1e-9) && close(pts[1].y, 2.0, 1e-9), "same global geometry: {pts:?}");
    let c = out.transform(p).as_coeffs();
    assert!(close(c[0], 0.5, 1e-9) && close(c[3], 0.5, 1e-9) && close(c[4], 0.0, 1e-9) && close(c[5], 0.0, 1e-9), "the inverse of the parent's composed transform: {c:?}");
    let own: Vec<sciink::geom::Point> = sciink::geom::path::end_points(&sciink::geom::path::shape_path(&out, p).unwrap().path);
    assert!(close(own[0].x, 2.0, 1e-9) && close(own[0].y, 2.0, 1e-9) && close(own[1].x, 4.0, 1e-9), "the path data itself is in global coordinates: {own:?}");
    let sw: f64 = out.specified(p, "stroke-width").unwrap().trim_end_matches("px").parse().unwrap();
    assert!(close(sw, 2.0, 1e-9), "stroke scaled with the fused transform: 1 × 2 before, 2 × 1 after — the visual width is unchanged: {sw}");
    assert!(close(out.transform(id(&out, "t")).as_coeffs()[4], 3.0, 1e-9), "text is not fused");
}

#[test]
fn clearing_clips_and_masks_removes_attributes_and_pins_stylesheet_rules() {
    let svg = format!(
        r##"<svg {NS}><style>#q{{clip-path:url(#c)}}</style><defs><clipPath id="c"><rect width="1" height="1"/></clipPath><mask id="m"><rect width="1" height="1"/></mask></defs>
<path id="p" d="M0,0 H1" clip-path="url(#c)" mask="url(#m)" style="clip-path:url(#c);stroke:#000"/>
<path id="q" d="M0,0 H1" clip-path="url(#c)"/></svg>"##
    );
    let (s, msgs) = ok(&svg, &["--clearclipmasks=true", "--id=p", "--id=q"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let p = by_id(&d, "p");
    assert_eq!(p.attribute("clip-path"), None);
    assert_eq!(p.attribute("mask"), None);
    assert_eq!(style_of(p).get("clip-path"), None, "no stylesheet rule: nothing to pin");
    assert_eq!(style_of(p).get("stroke"), Some("#000"), "the rest of the style survives");
    let q = by_id(&d, "q");
    assert_eq!(q.attribute("clip-path"), None);
    assert_eq!(style_of(q).get("clip-path"), Some("none"), "the stylesheet still supplies one: pinned to none");
    assert!(s.contains(r#"<clipPath id="c">"#), "clips we did not create stay in defs");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test homogenizer 2>&1 | tail -20`
Expected: the six new tests fail (the phases are not implemented).

- [ ] **Step 3: Implement**

Add the imports `use crate::geom::union; use crate::num; use crate::ops::xform::{OTP_SUPPORT, fuse}; use crate::text::style::{baseline_shift, composed_width}; use super::scaler::{find_plot_area, geometric_bbox, warn_non_plot};` and these functions to `src/tools/homogenizer.rs`:

```rust
/// Upstream's `font-size` rounding (`HG:207–208`): two decimals when `|v| > 1`, else three
/// significant digits, trailing zeros trimmed, `px` appended.
pub fn fmt_font_size(v: f64) -> String {
    let rounded = if v.abs() > 1.0 {
        (v * 100.0).round() / 100.0
    } else {
        format!("{v:.2e}").parse::<f64>().unwrap_or(v)
    };
    format!("{}px", num::fmt(rounded))
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}
fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 }
}

/// `HG:153–208`: the largest character size (pt) of every text, the target from `mode`
/// (2 fixed pt, 3 scale %, 4 scale so the largest becomes `fontsize` pt, 5–8 mean/median/min/max
/// of the sizes), then every text and every descendant carrying a `font-size` is rewritten:
/// relative spans (`%` or a baseline shift) as a percentage of their parent, the rest absolute.
pub(crate) fn set_font_size(doc: &mut Doc, ctx: &mut Ctx, tels: &[NodeId], fontsize: f64, mode: u8) {
    let onept = (4.0 / 3.0) / doc.px_per_uu(); // 1 pt in user units (`cdocsize.unittouu("1pt")`)
    let mut szs: Vec<(NodeId, f64)> = Vec::new();
    for &el in tels {
        let Some(pt) = ctx.parse_text(doc, el) else { continue };
        let max = pt.chars.iter().map(|c| c.tfs / onept).fold(f64::NEG_INFINITY, f64::max);
        if max.is_finite() {
            szs.push((el, max));
        }
    }
    let values: Vec<f64> = szs.iter().map(|(_, v)| *v).collect();
    let (mut fontsize, mut fixedscale) = (fontsize, false);
    let stat = |f: fn(&[f64]) -> f64| if values.is_empty() { 12.0 } else { f(&values) };
    match mode {
        3 => fixedscale = true,
        4 => {
            fixedscale = true;
            let m = stat(|v| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
            fontsize = fontsize / m * 100.0;
        }
        5 => fontsize = stat(mean),
        6 => fontsize = stat(median),
        7 => fontsize = stat(|v| v.iter().cloned().fold(f64::INFINITY, f64::min)),
        8 => fontsize = stat(|v| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
        _ => {}
    }
    for (el, _) in szs {
        let nodes: Vec<NodeId> = doc.descendants(el).filter(|&n| doc.is_element(n)).collect();
        for &d in nodes.iter().rev() {
            let sty = doc.specified_style(d);
            if d != el && sty.get("font-size").is_none() {
                continue;
            }
            let fs = composed_width(doc, d, "font-size");
            if fs.tfs == 0.0 {
                continue;
            }
            let bshift = baseline_shift(doc, d, &sty);
            let relative = bshift != 0.0 || sty.get("font-size").is_some_and(|v| v.contains('%'));
            if relative {
                // a sub/superscript stays relative to its parent
                let Some(parent) = doc.parent(d).filter(|&p| doc.is_element(p)) else { continue };
                let pfs = composed_width(doc, parent, "font-size").tfs;
                if pfs == 0.0 {
                    continue;
                }
                doc.set_style(d, "font-size", &format!("{:.2}%", fs.tfs / pfs * 100.0));
            } else {
                let scl = if fixedscale { fontsize / 100.0 } else { fontsize * onept / fs.tfs };
                doc.set_style(d, "font-size", &fmt_font_size(fs.utfs * scl));
            }
        }
    }
    ctx.reset_char_table();
}

/// `HG:321–361` with the spec's restriction to elements whose specified stroke is not `none`
/// (upstream writes a width on every element, unstroked ones included, and its statistics count
/// them at the default width 1). Mode 2 = fixed px (converted to user units), 3 = scale %,
/// 5–8 = mean/median/min/max of the visual widths. Written as `visual / sf` + `px`.
pub(crate) fn set_stroke(doc: &mut Doc, ctx: &mut Ctx, sela: &[NodeId], setstrokew: f64, mode: u8) {
    let stroked: Vec<(NodeId, f64, f64)> = sela
        .iter()
        .filter(|&&n| doc.specified(n, "stroke").is_some_and(|s| s.trim() != "none"))
        .map(|&n| {
            let w = composed_width(doc, n, "stroke-width");
            (n, w.tfs, w.scf)
        })
        .collect();
    if stroked.is_empty() {
        ctx.warn.push("stroke width: no stroked elements in the selection; nothing changed".to_string());
        return;
    }
    let widths: Vec<f64> = stroked.iter().map(|(_, w, _)| *w).collect();
    let mut fixedscale = false;
    let target = match mode {
        2 => setstrokew / doc.px_per_uu(),
        3 => {
            fixedscale = true;
            setstrokew
        }
        5 => mean(&widths),
        6 => median(&widths),
        7 => widths.iter().cloned().fold(f64::INFINITY, f64::min),
        8 => widths.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        _ => setstrokew,
    };
    for (n, w, sf) in stroked {
        if sf == 0.0 {
            continue;
        }
        let new = if fixedscale { w * target / 100.0 } else { target };
        doc.set_style(n, "stroke-width", &format!("{}px", num::fmt(new / sf)));
    }
}

/// `HG:363–369`: bake each shape's composed transform into its geometry (and stroke), then give
/// it the inverse of its parent's composed transform — the path data ends up in global
/// coordinates. Elements under a singular transform are left alone with a warning.
pub(crate) fn fuse_all(doc: &mut Doc, ctx: &mut Ctx, sela: &[NodeId]) {
    for &el in sela {
        if !OTP_SUPPORT.contains(&doc.tag(el)) || doc.parent(el).is_none() {
            continue;
        }
        let parent_ct = doc.parent(el).map(|p| doc.composed_transform(p)).unwrap_or(Affine::IDENTITY);
        let Some(inv) = inverse(parent_ct) else {
            ctx.warn.push(format!("{}: singular parent transform; not fused", crate::ops::label(doc, el)));
            continue;
        };
        // upstream (HG:367–369) puts the COMPOSED transform on the element, fuses, then leaves the
        // parent's inverse: `fuse` adjusts clips and masks by the element's own transform only
        // (its `extra` reaches geometry, strokes and gradients but not `transform_clipmask`), so
        // the whole composed transform must sit on the element when it runs
        doc.set_transform(el, parent_ct * doc.transform(el));
        fuse(doc, ctx, el, Affine::IDENTITY, None, true);
        doc.set_transform(el, inv);
    }
}

/// `HG:372–375` as the spec reads it: the `clip-path`/`mask` attributes and inline values go;
/// an inline `none` is written only where a stylesheet rule would otherwise still apply one.
pub(crate) fn clear_clipmasks(doc: &mut Doc, sela: &[NodeId]) {
    for &el in sela {
        for prop in ["clip-path", "mask"] {
            doc.remove_attr(el, prop);
            remove_inline(doc, el, prop);
            if doc.sheet_value(el, prop).is_some() {
                doc.set_style(el, prop, "none");
            }
        }
    }
}
```

Replace the plot-aware placeholder in `recentre` (the `// Task 8 fills this branch` comment and the `let _ = sel0;` line) with `HG:265–319`:

```rust
    let gbbs: HashMap<NodeId, Rect> = bbs.iter().map(|(&n, &v)| (n, geometric_bbox(doc, n, v, None))).collect();
    for (i0, &g) in sel0.iter().enumerate() {
        let pels: Vec<NodeId> = doc.children(g).filter(|&k| doc.is_element(k) && bbs.contains_key(&k)).collect();
        let pa = find_plot_area(doc, &pels, &gbbs);
        let (lvel, lhel) = match (pa.lvel, pa.lhel) {
            (Some(v), Some(h)) => (Some(v), Some(h)),
            _ => {
                let gid = doc.attr(g, "id").unwrap_or("").to_string();
                warn_non_plot(&mut ctx.warn, i0, &gid);
                (None, None)
            }
        };
        let mut bbp: Option<Rect> = None;
        for &el in &pels {
            if Some(el) == lvel || Some(el) == lhel {
                bbp = union(bbp, gbbs.get(&el).copied());
            }
        }
        let texts: Vec<NodeId> = doc.descendants(g).filter(|n| tels.contains(n)).collect();
        for el in texts {
            let (Some(&b1), Some(&b2)) = (bbs.get(&el), bbs2.get(&el)) else { continue };
            let centred = (b1.center().x - b2.center().x, b1.center().y - b2.center().y);
            let (dx, dy) = match bbp {
                Some(p) if b1.width() > 0.0 && b1.height() > 0.0 => {
                    let dx = if b1.center().x < p.x0 {
                        (p.x0 - b2.x1) - (p.x0 - b1.x1) * b2.width() / b1.width()
                    } else if b1.center().x > p.x1 {
                        (b1.x0 - p.x1) * b2.width() / b1.width() - (b2.x0 - p.x1)
                    } else {
                        centred.0
                    };
                    let dy = if b1.center().y < p.y0 {
                        (p.y0 - b2.y1) - (p.y0 - b1.y1) * b2.height() / b1.height()
                    } else if b1.center().y > p.y1 {
                        (b1.y0 - p.y1) * b2.height() / b1.height() - (b2.y0 - p.y1)
                    } else {
                        centred.1
                    };
                    (dx, dy)
                }
                _ => centred,
            };
            global_transform(doc, ctx, el, Affine::translate((dx, dy)), None, true);
        }
    }
```

Wire the phases in `run`: replace the three `// Task 8: …` comments with the calls they name and remove the `let _ = &sela;` line.

Notes: `Ctx::parse_text(doc, el) -> Option<ParsedText>` is Task 2's accessor; `ParsedText.chars: Vec<TChar>` with `tfs` = transformed font size in user units. `composed_width(doc, d, "font-size")` returns `FontSize { tfs, scf, utfs }`. `baseline_shift(doc, d, &sty)` takes the node whose style is `sty`. `doc.specified_style(d)` returns `Rc<Style>`; `sty.get` works through the `Rc`. In `set_font_size` the `12.0` fallback mirrors `HG:182–183` (an empty statistic); it can only matter when no text measured, in which case the loop writes nothing — kept for fidelity.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test homogenizer 2>&1 | tail -20`
Expected: 10 passed. The `2.33px` expectations: 7 pt × 4/3 = 9.3333 px ÷ 2 px/uu = 4.6667 uu visual; the text's `tfs` = 10 × 2 = 20, `utfs` = 10 → `scl = 4.6667 / 20`, `nfs = 10 × 0.23333 = 2.3333` → `2.33px`. Mode 4 in `font_size_modes_follow_upstream`: the largest is 30 pt → `fontsize = 15 / 30 × 100 = 50` → halves. If `fmt_font_size(23.333…)` prints `23.33px` and `mean` of `[7.5, 15, 30]` is `17.5` pt = `23.333…` px ✓.

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/tools/homogenizer.rs tests/homogenizer.rs
git commit -m "feat(homogenizer): font size modes, plot-aware re-centring, stroke width modes, transform fusing, clip clearing"
```

---

### Task 9: Homogenizer reference oracles, invariance, `.inx`, README, spec deviations

Two upstream references (`C.5 (b)`): `homogenizer__5a83c74b7209db59f18b4ea3633bdcc5.out` = Other_tests.svg and `homogenizer__b1a8ca25564328c974db2cf0d2072b84.out` = Other_tests_nonuniform.svg, both with `--id=layer1 --fontsize=7 --setfontsize=True --fixtextdistortion=True --fontmodes=2 --setfontfamily=True --fontfamily=Avenir --setstroke=True --setstrokew=0.75 --strokemodes=2 --fusetransforms=True`. `Avenir` exists only as an installed font (this Mac has it; CI has the vendored DejaVu Sans and Roboto), so the default oracle runs with `--fontfamily=DejaVu Sans` and compares everything that does not depend on the family — fused path geometry, visual stroke widths, `font-size` values (they depend only on transforms and the document scale), the conformal text transforms — and the `#[ignore]` system-fonts oracle runs with `Avenir` and adds the family and the text anchors.

**Files:**
- Create: `tests/homogenizer_fixtures.rs`, `inx/homogenizer.inx`
- Modify: `tests/invariance.rs` (append), `README.md`, `docs/spec/02-geometry-tools.md`

- [ ] **Step 1: Write the oracles**

Create `tests/homogenizer_fixtures.rs`:

```rust
//! Homogenizer against upstream's references. Geometry and numbers are compared, not attribute
//! text: upstream writes `stroke-width` on every element and splits Avenir non-letters into
//! tspans (`character_fixer`, not ported); we write the same geometry with fewer edits.
mod support;

use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::{Point, scale_factor};
use sciink::tools::scaler::global_points;
use support::with_vendored_fonts;

const PATH_TOL: f64 = 0.02;
const TEXT_TOL: f64 = 0.5;
const ARGS: &[&str] = &[
    "--tool=homogenizer", "--tab=scaling", "--id=layer1", "--fontsize=7", "--setfontsize=true",
    "--fixtextdistortion=true", "--fontmodes=2", "--setfontfamily=true", "--setstroke=true",
    "--setstrokew=0.75", "--strokemodes=2", "--fusetransforms=true",
];

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink").chain(v.iter().copied()).map(OsString::from).collect()
}

fn fixture(svg_name: &str, ref_name: &str, family: &str, vendored: bool) -> Option<(Doc, Doc)> {
    let dir = support::upstream_data_dir()?;
    let input = std::fs::read(dir.join(format!("svg/{svg_name}.svg"))).unwrap();
    let reference = std::fs::read(dir.join(format!("refs/{ref_name}"))).unwrap();
    let fam = format!("--fontfamily={family}");
    let mut a = ARGS.to_vec();
    a.push(&fam);
    let out = if vendored {
        with_vendored_fonts(|| sciink::run(&args(&a), &input))
    } else {
        sciink::run(&args(&a), &input)
    }
    .unwrap();
    // font warnings are expected with the vendored fonts; a plot-area warning would not be
    assert!(!out.messages.iter().any(|m| m.contains("could not be automatically detected")), "{:?}", out.messages);
    Some((Doc::parse(&out.svg).unwrap(), Doc::parse(&reference).unwrap()))
}

fn ids_under(doc: &Doc, root: &str) -> Vec<String> {
    let r = doc.by_id(root).unwrap_or_else(|| panic!("no {root}"));
    doc.descendants(r).filter(|&n| doc.is_element(n) && n != r).filter_map(|n| doc.attr(n, "id").map(str::to_string)).collect()
}
fn num_attr(v: &str) -> Option<f64> {
    v.trim().trim_end_matches("px").trim_end_matches('%').parse().ok()
}
fn visual_stroke(doc: &Doc, n: NodeId) -> Option<f64> {
    let s = doc.specified(n, "stroke")?;
    if s.trim() == "none" {
        return None;
    }
    let w = num_attr(&doc.specified(n, "stroke-width")?)?;
    Some(w * scale_factor(doc.composed_transform(n)))
}
fn anchor(doc: &Doc, n: NodeId) -> Option<Point> {
    let x = num_attr(doc.attr(n, "x")?.split([' ', ',']).next()?)?;
    let y = num_attr(doc.attr(n, "y")?.split([' ', ',']).next()?)?;
    Some(doc.composed_transform(n) * Point::new(x, y))
}

/// Font-independent comparisons; returns (shapes compared, texts compared, max anchor deviation).
fn compare(ours: &Doc, reference: &Doc) -> (usize, usize, f64) {
    let (mut shapes, mut texts, mut max_anchor) = (0usize, 0usize, 0.0_f64);
    for id in ids_under(reference, "layer1") {
        let (Some(a), Some(b)) = (ours.by_id(&id), reference.by_id(&id)) else {
            panic!("{id}: present in the reference but not in our output")
        };
        let tag = reference.tag(b);
        if matches!(tag, "path" | "rect" | "line" | "polyline" | "polygon" | "circle" | "ellipse") {
            let (pa, pb) = (global_points(ours, a, None), global_points(reference, b, None));
            assert_eq!(pa.len(), pb.len(), "{id}: point count");
            for (p, q) in pa.iter().zip(&pb) {
                assert!((p.x - q.x).abs() <= PATH_TOL && (p.y - q.y).abs() <= PATH_TOL, "{id}: {p:?} vs {q:?}");
            }
            if let (Some(wa), Some(wb)) = (visual_stroke(ours, a), visual_stroke(reference, b)) {
                assert!((wa - wb).abs() <= 1e-3 * wb.max(1.0), "{id}: visual stroke {wa} vs {wb}");
            }
            shapes += 1;
        } else if tag == "text" {
            // the size written depends only on transforms and the document scale
            if let (Some(fa), Some(fb)) = (ours.specified(a, "font-size"), reference.specified(b, "font-size")) {
                if let (Some(x), Some(y)) = (num_attr(&fa), num_attr(&fb)) {
                    assert!((x - y).abs() <= 0.011, "{id}: font-size {fa} vs {fb}");
                }
            }
            // the distortion fix depends only on transforms
            let ca = ours.composed_transform(a).as_coeffs();
            let cb = reference.composed_transform(b).as_coeffs();
            for k in 0..4 {
                assert!((ca[k] - cb[k]).abs() <= 1e-3, "{id}: transform {ca:?} vs {cb:?}");
            }
            assert_eq!(ours.specified(a, "-inkscape-font-specification"), None, "{id}");
            if let (Some(p), Some(q)) = (anchor(ours, a), anchor(reference, b)) {
                max_anchor = max_anchor.max((p.x - q.x).abs().max((p.y - q.y).abs()));
            }
            texts += 1;
        }
    }
    assert!(shapes > 20 && texts > 5, "compared {shapes} shapes and {texts} texts — the ids did not line up");
    (shapes, texts, max_anchor)
}

#[test]
fn homogenizer_matches_the_reference_geometry_on_other_tests() {
    let Some((ours, reference)) = fixture("Other_tests", "homogenizer__5a83c74b7209db59f18b4ea3633bdcc5.out", "DejaVu Sans", true) else { return };
    let (s, t, m) = compare(&ours, &reference);
    eprintln!("homogenizer Other_tests: {s} shapes, {t} texts, max anchor deviation {m:.4} uu (vendored fonts)");
}

#[test]
fn homogenizer_matches_the_reference_geometry_on_the_non_uniform_document() {
    let Some((ours, reference)) = fixture("Other_tests_nonuniform", "homogenizer__b1a8ca25564328c974db2cf0d2072b84.out", "DejaVu Sans", true) else { return };
    let (s, t, m) = compare(&ours, &reference);
    eprintln!("homogenizer Other_tests_nonuniform: {s} shapes, {t} texts, max anchor deviation {m:.4} uu (vendored fonts)");
}

/// With the installed fonts (Avenir): family and anchors too.
/// Run: `SCIINK_SYSTEM_FONTS=1 cargo test --test homogenizer_fixtures -- --ignored --nocapture --test-threads=1`
#[test]
#[ignore = "needs the installed Avenir; run with SCIINK_SYSTEM_FONTS=1"]
fn homogenizer_matches_the_reference_with_avenir() {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        return;
    }
    for (svg, r) in [
        ("Other_tests", "homogenizer__5a83c74b7209db59f18b4ea3633bdcc5.out"),
        ("Other_tests_nonuniform", "homogenizer__b1a8ca25564328c974db2cf0d2072b84.out"),
    ] {
        let Some((ours, reference)) = fixture(svg, r, "Avenir", false) else { return };
        let (_, _, m) = compare(&ours, &reference);
        eprintln!("homogenizer {svg} with Avenir: max anchor deviation {m:.4} uu");
        assert!(m <= TEXT_TOL, "{svg}: text anchors {m}");
        for id in ids_under(&reference, "layer1") {
            let (Some(a), Some(b)) = (ours.by_id(&id), reference.by_id(&id)) else { continue };
            if reference.tag(b) == "text" && reference.specified(b, "font-family").is_some_and(|f| f.contains("Avenir")) {
                assert!(ours.specified(a, "font-family").is_some_and(|f| f.contains("Avenir")), "{id}");
            }
        }
    }
}
```

Ruling in advance: the vendored-font oracles assert geometry only and PRINT the anchor deviation; the Avenir oracle asserts anchors ≤ `TEXT_TOL`. If the Avenir anchors exceed 0.5 uu, record the measured maximum in the report and in a comment above `TEXT_TOL`, keep the assertion, and report it as a finding for the controller's ruling (the reference was produced with Windows fonts; upstream's own `character_fixer` moves glyphs into other fonts). Never raise `TEXT_TOL` on your own.

Append to `tests/invariance.rs` (spec C.5 (c): "Homogenizer `--fusetransforms` only ≤ 0.1 %"):

```rust
/// Root-coordinate box of every clipped element's clip region, sorted by the element's id: the
/// union over the clip's element children of `composed(el) · child.transform · own-frame box`.
fn clip_boxes(svg: &[u8]) -> Vec<(String, Rect)> {
    use sciink::ops::bbox::{LOCAL, bbox};
    use sciink::ops::{ClipKind, Ctx, clip_ref};
    let mut doc = Doc::parse(svg).unwrap();
    let mut ctx = Ctx::new();
    let els: Vec<NodeId> = doc.descendants(doc.svg()).filter(|&n| doc.is_element(n)).collect();
    let mut out = Vec::new();
    for el in els {
        let Some(clip) = clip_ref(&doc, el, ClipKind::Clip) else { continue };
        let Some(id) = doc.attr(el, "id").map(str::to_string) else { continue };
        let ct = doc.composed_transform(el);
        let kids: Vec<NodeId> = doc.children(clip).filter(|&k| doc.is_element(k)).collect();
        let mut acc: Option<Rect> = None;
        for k in kids {
            if let Some(b) = bbox(&mut doc, &mut ctx, k, LOCAL) {
                acc = union(acc, Some(transform_rect(ct * doc.transform(k), b)));
            }
        }
        if let Some(r) = acc {
            out.push((id, r));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Fusing transforms is a geometric no-op except where a stroke was anisotropic: Other_tests has
/// non-uniformly scaled plots (`g5224` at 0.748 × 0.520, `rect3230`) whose strokes become
/// uniform by design, so pixel identity is impossible there (measured 2026-09-23: 0.2303 %).
/// The precise invariant is that every clip region stays where it was.
#[test]
fn homogenizer_fuse_transforms_keeps_clips_in_place_and_changes_only_anisotropic_strokes() {
    let Some(dir) = support::upstream_data_dir() else { return };
    let input = std::fs::read(dir.join("svg/Other_tests.svg")).unwrap();
    let out = with_vendored_fonts(|| sciink::run(&args(&["--tool=homogenizer", "--tab=scaling", "--fusetransforms=true", "--id=layer1"]), &input)).unwrap();
    assert!(out.messages.is_empty(), "fusing alone loads no fonts: {:?}", out.messages);
    let (a, b) = (render_png(&input, 1500), render_png(&out.svg, 1500));
    let d = pixel_diff_fraction(&a, &b, 32);
    eprintln!("homogenizer fuse: {:.4} % pixels differ", d * 100.0);
    assert!(d <= 0.005, "{d}");
    let (before, after) = (clip_boxes(&input), clip_boxes(&out.svg));
    assert_eq!(before.len(), after.len(), "same clipped elements");
    // 11 clipped elements; `image270`'s clip is a `<use>` whose target `bbox` does not measure,
    // so it drops out of both lists alike
    assert!(before.len() >= 10, "the fixture has clipped elements: {}", before.len());
    for ((ida, ra), (idb, rb)) in before.iter().zip(&after) {
        assert_eq!(ida, idb);
        for (x, y) in [(ra.x0, rb.x0), (ra.y0, rb.y0), (ra.x1, rb.x1), (ra.y1, rb.y1)] {
            assert!((x - y).abs() <= 1e-3, "{ida}: clip region moved: {ra:?} vs {rb:?}");
        }
    }
}
```

- [ ] **Step 2: Run the oracles**

Run: `cargo test --test homogenizer_fixtures -- --nocapture 2>&1 | tail -20`, `SCIINK_SYSTEM_FONTS=1 cargo test --test homogenizer_fixtures -- --ignored --nocapture --test-threads=1 2>&1 | tail -20`, `cargo test --test invariance homogenizer 2>&1 | tail -8`. Record every printed number in the report.

- [ ] **Step 3: `.inx`, README, spec**

Create `inx/homogenizer.inx`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Homogenizer (sciink)</name>
    <id>org.sciink.homogenizer</id>
    <param name="tool" type="string" gui-hidden="true">homogenizer</param>
    <param name="tab" type="notebook">
        <page name="scaling" gui-text="Options">
            <label>Sets the properties of all selected objects without changing objects' center position. For best results, imported PDFs should be Flattened before running.</label>
            <label appearance="header">Text options</label>
            <param name="fixtextdistortion" type="bool" gui-text="Correct distorted text?">true</param>
            <param name="setfontfamily" type="bool" gui-text="Set font?">true</param>
            <param name="fontfamily" type="string" gui-text="New font" gui-description="Can be used to set Font Family and Font Style. If specifying both, separate them with a space."></param>
            <param name="setfontsize" type="bool" gui-text="Set font size?">true</param>
            <param name="fontmodes" type="optiongroup" appearance="combo" gui-text="Font size options:">
                <option value="2">Fixed size (pt)</option>
                <option value="3">Scale (%)</option>
                <option value="5">Mean selected</option>
                <option value="6">Median selected</option>
                <option value="7">Min selected</option>
                <option value="8">Max selected</option>
                <option value="4">Scale max to (pt)</option>
            </param>
            <param name="fontsize" type="float" precision="1" min="0" max="10000" gui-text="Font size value (if applicable)">8</param>
            <param name="plotaware" type="bool" gui-text="Plot-aware text adjustments?" gui-description="When adjusting text, maintain distance to a plot. Requires that each selected item be a grouped plot with a well-defined plot area.">false</param>
            <label appearance="header">Stroke options</label>
            <param name="fusetransforms" type="bool" gui-text="Correct distorted paths?" gui-description="Fuses transforms to paths, ensuring stroke width is constant">true</param>
            <param name="setstroke" type="bool" gui-text="Set stroke width?">true</param>
            <param name="strokemodes" type="optiongroup" appearance="combo" gui-text="Stroke width options:">
                <option value="2">Fixed size (px)</option>
                <option value="3">Scale (%)</option>
                <option value="5">Mean selected</option>
                <option value="6">Median selected</option>
                <option value="7">Min selected</option>
                <option value="8">Max selected</option>
            </param>
            <param name="setstrokew" type="float" precision="2" min="0" max="10000" gui-text="Stroke width value (if applicable)">1</param>
            <label appearance="header">Other options</label>
            <param name="clearclipmasks" type="bool" gui-text="Remove clips and masks?" gui-description="Deletes all clips and masks without releasing them">false</param>
        </page>
    </param>
    <effect needs-live-preview="false">
        <object-type>all</object-type>
        <effects-menu>
            <submenu name="Scientific"/>
        </effects-menu>
    </effect>
    <script>
        <command location="inx">bin/sciink</command>
    </script>
</inkscape-extension>
```

README: "eight menu entries — four tools" → "nine menu entries — five tools", and after the Scaler bullet:

```markdown
- **Extensions ▸ Scientific ▸ Homogenizer** — makes a selection uniform without moving anything's
  centre: one font size (fixed, scaled, or the selection's mean/median/min/max), one font (an
  Inkscape font specification such as `Avenir Next Bold`), distorted text made conformal, one
  stroke width (the same modes), transforms fused into paths, clips and masks removed. Plot-aware
  mode keeps labels at their distance from the plot area. Same options and defaults as the original.
```

`docs/spec/02-geometry-tools.md`: append after "Deliberate deviations (Plan 6)":

```markdown
## Deliberate deviations (Plan 7)
- `FontSystem::load()` scans the filesystem once per process and environment (upstream builds
  a font list per character table); a Flattener or Homogenizer run no longer scans twice.
- `Doc::px_per_uu` (upstream `document_size`) gives the geometric mean of the two factors for
  `preserveAspectRatio="none"` with a non-uniform document (upstream: no scale, every unit
  conversion fails).
- Scaler: an empty selection is "No objects selected!" (upstream shows the raster-image message,
  `all([])` being true); option values outside upstream's tables are tolerated (`figuremode`/
  `matchprop` ≠ 2 mean the first option, `marksf` outside 1–4 clears the mark); Matching's
  correction pre-pass re-measures the plot before matching (upstream matches against the boxes
  measured before the pre-pass, spec R9); a plot or a first selection without any bounding box is
  skipped with a warning and a zero or non-finite scale is treated as 1 with a warning (upstream
  divides by zero); `find_plot_area` skips elements without a box (upstream reads a stale variable
  for a non-path-like element); combined-by-colour ranges index BezPath elements of the written
  `d` (Plan 5's convention); the hidden Fixed mode is absent (upstream too — its parameters are
  accepted and ignored).
- Homogenizer: an empty selection is a no-op with a message; the distortion fix touches only
  `text`/`flowRoot` (upstream also processes tspans, writing `transform` attributes they cannot
  carry); `character_fixer` (Avenir/Whitney non-letters moved into 'Avenir Next'/'Arial' tspans) is
  not ported — the text engine falls back per character; the stroke width is written only on
  elements whose specified stroke is not `none`, and the statistics run over those (upstream writes
  on every element and counts unstroked ones at width 1); a statistic over no stroked element
  leaves widths alone with a warning (upstream raises); clip/mask clearing removes the attributes
  and inline values and pins `none` only where a stylesheet rule remains (upstream writes inline
  `none` on every element); font-size strings are rounded as upstream does and then formatted by
  `num::fmt`; the installed-family list comes from fontdb, not fontconfig.
- The fuse-transforms appearance check is bounded at 0.5 % on Other_tests instead of the spec's
  0.1 %: its non-uniformly scaled plots have anisotropic strokes that fusing makes uniform, by
  design (measured 0.23 %); the precise invariant — every clip region stays in place — is asserted
  exactly.
```

Also fix the module map line in §B.5 that places `doc_scale` in `src/ops/cleanup.rs`: it lives in `src/geom/mod.rs` as `Doc::px_per_uu`.

- [ ] **Step 4: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add tests/homogenizer_fixtures.rs tests/invariance.rs inx/homogenizer.inx README.md docs/spec/02-geometry-tools.md
git commit -m "test(homogenizer): upstream reference oracles and fuse invariance; Homogenizer .inx, README, Plan 7 deviations"
```

---

## Out of scope (deferred)

Favorite Markers and the first full release (Plan 8); `character_fixer`; upstream's `hcall` hook (the Homogenizer never sets it in v1.4.25); the Scaler's diagnostic rectangle (`diagmode`); Fixed mode; flowed-text boxes beyond Plan 4's; CI does not run the fixture oracles (the upstream data is not in the repository).

## Self-review notes (controller)

- Spec coverage (§B.3 Scaler): modes and parameters → Task 4 (`Options::from_cli`); setup/errors → Task 4 `run`; `find_plot_area`/`geometric_bbox` → Task 3; `scale_plot` correction incl. figure mode → Task 4; matching incl. pre-pass, `bbmatch`, alignment, `deletematch` → Task 5; per-child ticks / scale_free / aspect_locked / combined ranges → Task 4; Fixed mode dropped → Task 4 (accepted, ignored). (§B.3 Homogenizer): params/sets/errors → Task 7; step 1 boxes → Task 7 `run`; step 2 font size → Task 8; step 3 distortion → Task 7; step 4 family (`inkscape_spec_to_css`, reset of the others, spec removal; `character_fixer` deviation) → Task 7; step 5 re-centre + plot-aware → Tasks 7/8; step 6 stroke → Task 8; step 7 fuse → Task 8; step 8 clips → Task 8. §B.2 document scale → Task 2. §C.5 (b) refs → Tasks 6/9; (c) invariance → Tasks 6/9. Plan 6 F2 → Task 1.
- Type consistency: `Boxes.f/g: HashMap<NodeId, Rect>`; `scale_plot(doc, ctx, o, boxes, first, plot, i, cmode)` is the same in Tasks 4 and 5; `find_plot_area(doc, els, gbbs) -> PlotArea { vl, hl, lvel, lhel }` is used identically by Tasks 3, 4, 5, 8; `geometric_bbox(doc, el, vis, range)` by Tasks 3, 4, 8; `global_points(doc, el, range)` by Tasks 3, 6, 9; `composed_width(doc, n, prop) -> FontSize { tfs, scf, utfs }` and `baseline_shift(doc, node, &Style)` as the digest of the current crate reports; `Ctx::parse_text(doc, el) -> Option<ParsedText>` and `Ctx::reset_char_table()` from Task 2 are the only new `Ctx` methods; `FontSystem::families() -> Vec<String>` from Task 1 feeds Task 7.
- Placeholder scan: the two "Task N replaces this" markers are code that compiles (an early return and a placeholder tuple) and are removed by the task named; every other step carries code or an exact command; oracle expectations are read from the reference files.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-22-plan7-scaler-homogenizer.md`. Execute with **subagent-driven development** on a branch `plan7-scaler-homogenizer` off `main` (6072fb2), with the same process rules as Plans 3–6: never weaken a test to make it pass; plan defects become controller rulings mirrored into this document; sonnet implementers with foreground `cargo` runs; reviewers use the bounded method (one focused `cargo test --test <file>` run as the numeric evidence, no hand-tracing).

## Post-review fix wave (2026-09-23)

The whole-branch review (opus, range `6072fb2..5fd042f`) rated the branch "with fixes"; one fix wave landed as ``ca6f6c3`, `9f613a2`, `384ad12``, verified by a scoped re-review (every item addressed, no new breakage) and by CI on every pushed head:

1. **F1 (Important)** both tools built their character table over the whole document (`Ctx::new()`) — the Homogenizer twice per run — and warned about fonts of text nobody selected. Now `Ctx::for_roots(selection)`, as upstream's `BB2(svg, els)` → `make_char_table(els=tels)`; every measured element lies under the roots, so the oracle numbers are unchanged.
2. **F2 (Important)** the vendored-font Homogenizer oracle printed the text-anchor deviation without asserting it. Now a regression bound of 1.0 uu (measured 0.5927 uu on Other_tests, 0.0153 uu on the non-uniform document; the reference was produced with other fonts, so `TEXT_TOL` stays with the `#[ignore]` Avenir oracle, which measures 0.0517 / 0.0005 uu).
3. Minors taken now: `scan_count()`'s doc no longer claims About prints it; `set_font_size` warns and returns when no text could be measured (the 12 pt fallback was unobservable); its doc says "specified (inherited) font-size"; `fuse_all` looks the parent up once; the four guarded `unwrap()`s in `scale_plot` are `expect("guarded: pels has a box")`; both oracles count the stroke and font-size pairs they compare and assert floors; the Avenir oracle's doc says `--ignored`, never `--include-ignored`; the README's ignored-oracle command drops `scaler_fixtures`; tests for `sane()` on a degenerate group transform and for `fuse_all` under a singular parent.
4. Accepted with a ruling (the reviewer's triage): `strokemodes` outside {2, 3, 5–8} treats `setstrokew` as user units (unreachable from the `.inx`); the Homogenizer keeps `Doc::selection` (document order) for `sel0`, so `warn_non_plot`'s ordinal counts in document order; the exact `scalex != 1.0` comparison is upstream's; `inx/homogenizer.inx` ships `setfontfamily=true` with an empty family, which sets nothing and drops `-inkscape-font-specification` (upstream-identical); the Flattener's and Combine by Color's `Ctx::new()` stand (Plan 6's deviation bullet).
5. Parked (library follow-up): `ops::bbox::bbox` yields no box for the `<use>` inside `image270`'s clipPath in Other_tests — `has_bbox` probably rejects the target under `<defs>`; the clip-region oracle's floor is 10 of 11 for that reason.

Execution rulings not visible in the tasks above (all mirrored into the task text where they changed it): the `px_per_uu` test's float bindings need `: f64` (E0689); the figure-mode and bounding-box matching tests re-measure `num::fmt`'s 8-significant-digit output, so their tolerance is 1e-3; the bounding-box matching test unions b's CHILDREN — the group's own entry is its stroke-padded visual box, while upstream's match target for a group IS its visual box; `Style::to_css` writes no trailing `;`; a hyphenated family ("dejavu-sans") can never match because upstream's `clean_str` strips punctuation without inserting spaces — the test uses "dejavu sans, Bold Italic"; the distortion coefficient tolerance is 1e-6 after the round trip; `look` is a nested `fn` (a closure cannot name its returned lifetime); Task 4's `deletematch`-before-Matching interim was accepted for exactly one task; `fuse_all` must put the COMPOSED transform on the element before `fuse` because `fuse` adjusts clips and masks by the element's own transform only — caught by the invariance oracle at 0.51 %; that oracle's pixel bound is 0.5 % on Other_tests (anisotropic strokes under `g5224` and `rect3230` become uniform by design, measured 0.2303 %) with the exact clip-region invariant asserted; the oracle's `ids_under` is scoped to the shape/text tags it compares (upstream's unported `character_fixer` injects tspans with ids our output cannot have).

