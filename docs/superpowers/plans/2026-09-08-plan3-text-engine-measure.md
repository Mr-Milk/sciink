# Plan 3 — Text engine part 1: fonts, metrics, parsing, layout, `text_bbox` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure SVG text the way Inkscape renders it — resolve fonts, measure glyph advances/kerning/ink boxes, parse `<text>` into lines/chunks/characters and compute per-character positions — exposing `text_bbox`, `char_extents` and two debug tools, so the Flattener's text pipeline (part 2) and every bbox consumer (Text Ghoster, Scaler, Homogenizer) can build on it.

**Architecture:** A `text` module tree: `fonts.rs` (fontdb enumeration + own CSS/fontconfig-style matching + fallback ladder), `metrics.rs` (rustybuzz shaping + ttf-parser outlines → `CProp` in em units), `style.rs` (font-size / line-height / letter-spacing / baseline-shift semantics), `tree.rs` (lxml-style text/tail run iteration), `whitespace.rs` (depathologize), `table.rs` (character table), `parse.rs` (`ParsedText` model: lines, chunks, chars), `layout.rs` (chunk geometry, extents, `text_bbox`). Stateless pure functions over the arena DOM; the model is built once per element. Two tools (`font-probe`, `text-highlight`) expose the pipeline for experiments against upstream's `--debugparser` reference. Two prerequisites first: the parked `parse_d` arc-guard bypass, and the DOM accessors the spec assumes (`transform`, `composed_transform`, `xml_space_preserve`, `resolve_href`, `new_id`).

**Tech Stack:** Rust 2024 (rustc ≥ 1.85), kurbo 0.13, quick-xml 0.42 (existing); new: fontdb 0.24.0, rustybuzz 0.20.1, ttf-parser 0.25.1 (resolved together with no duplicate versions — verified in a scratch crate). Vendored test fonts: DejaVu Sans (Regular, Bold) and Roboto (Regular, Bold).

**Spec:** `docs/spec/01-text-engine.md` (Appendix A: §A.1 Stage 0–1 and "Geometry", §A.2 model, §A.3 metrics, §A.4 API, §A.5 experiments); `docs/spec/03-infrastructure.md` §C.3 (About tool font lines, env vars) and §C.5 (fonts for determinism). Part 2 (stages 2–12: editing, kerning removal, writer) is a later plan.

## Global Constraints

- Every number written into the document goes through `sciink::num::fmt` (8 significant digits, `-0` → `0`).
- All tree walks are iterative (no recursion over document structure). Depth of text elements is small, but never recurse over `Doc` descendants.
- No usvg text internals, no Inkscape subprocess, no Python. Metrics come from fontdb + rustybuzz + ttf-parser only (spec §A.0 decision 2).
- Font determinism in tests: tests build `FontSystem::from_dirs(&[tests/fonts])` (no system fonts). Tests that need system fonts are `#[ignore]` and run only with `SCIINK_SYSTEM_FONTS=1`. The binary honours `SCIINK_FONT_DIRS` (extra dirs, `std::env::split_paths` separator) and `SCIINK_NO_SYSTEM_FONTS=1` (spec §C.3).
- Style values are `Rc<Style>` (single-threaded binary; the spec's `Arc` is not needed — ruling recorded in the ledger).
- Constants from spec §A.1: `XY_TOL = 1e-6`; `font-size` keywords small/medium/large = 10/12/14 px, other keywords → 12 px; `line-height: normal` = 1.25; `baseline-shift: super` = +40 %, `sub` = −20 % of the **parent's** untransformed font size.
- Positions are in the element's untransformed frame; `pts_t = composed_transform ∘ pts_ut`. `composed_transform` excludes the root `<svg>`'s own `transform`.
- Unrendered characters (no font on the system has the glyph) get a zero-width, zero-ink property and a warning is collected; they must never panic or abort a tool.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` must pass on every commit (CI runs them).
- Nothing in `tests/fonts/` may be packaged into release zips (`dist/package.sh` copies only `inx/`, the binary, README and LICENSE — unchanged).

---

## File structure

| File | Responsibility |
|---|---|
| `src/geom/path.rs` (modify) | arc guard: bound endpoints and post-conversion radii |
| `src/geom/mod.rs` (modify) | `impl Doc { transform, composed_transform }` |
| `src/dom.rs` (modify) | `xml_space_preserve`, `resolve_href`, `new_id` |
| `src/text/mod.rs` (new) | module tree + `Warnings` collector |
| `src/text/fonts.rs` (new) | `FontSystem`, `FaceKey`, `FaceInfo`, `FontSpec`, resolution + fallback |
| `src/text/metrics.rs` (new) | `CProp`, shaping, ink boxes, pair advances, caches |
| `src/text/style.rs` (new) | `composed_font_size`, `composed_line_height`, `letter_spacing`, `baseline_shift`, `Anchor` |
| `src/text/tree.rs` (new) | `Run`, `runs()`, `run_text()`, `set_run_text()` (lxml text/tail model) |
| `src/text/whitespace.rs` (new) | `depathologize` (position overflows, whitespace, comment tails) |
| `src/text/table.rs` (new) | `CharTable` (Stage 0) |
| `src/text/parse.rs` (new) | `ParsedText`, `TLine`, `TChunk`, `TChar`, `ParsedText::parse` |
| `src/text/layout.rs` (new) | `chunk_geom`, `char_pts_ut/_t`, extents, `text_bbox` |
| `src/tools/font_probe.rs`, `src/tools/text_highlight.rs` (new); `src/tools/about.rs`, `src/cli.rs`, `src/lib.rs` (modify) | debug tools, About font lines |
| `inx/font_probe.inx`, `inx/text_highlight.inx` (new) | menu entries under Scientific ▸ Debug |
| `tests/fonts/*` (new) | DejaVu Sans / Roboto TTFs + licences |
| `tests/text_fonts.rs`, `tests/text_metrics.rs`, `tests/text_style.rs`, `tests/text_tree.rs`, `tests/text_parse.rs`, `tests/text_layout.rs`, `tests/text_tools.rs` (new); `tests/geom.rs`, `tests/dom.rs`, `tests/cli.rs` (modify) | tests |

Test helper conventions (existing): `mod support;` at the top of each integration test; `Doc::parse(s.as_bytes()).unwrap()`; `support::upstream_data_dir()` returns `None` (with a SKIP note) when the upstream fixtures are absent — fixture tests must `return` in that case.

---

### Task 1: Close the parked `parse_d` arc-guard bypass

**Files:**
- Modify: `src/geom/path.rs:104-137` (the `EllipticalArc` arm)
- Test: `tests/geom.rs`

**Interfaces:**
- Consumes: `sciink::geom::path::parse_d(&str) -> Option<ParsedPath>` (existing).
- Produces: unchanged signature; hostile arcs degrade to a `LineTo`.

Background: `kurbo::Arc::from_svg_arc` scales the radii up when the endpoints are too far apart for the requested radii, and `append_iter(1e-4)` then emits a number of cubics that grows with `(radius / tolerance)^(1/6)`. `M 0 0 A 5 5 0 0 1 1e60 1` yields a radius near 5e59 and ~4e10 cubics — the process hangs. The existing guard only looks at the radii attributes.

- [ ] **Step 1: Write the failing test**

Append to `tests/geom.rs`:

```rust
#[test]
fn hostile_arc_endpoints_degrade_to_a_line_quickly() {
    use sciink::geom::path::parse_d;
    // Endpoint 1e60 away: kurbo would scale the 5-unit radii to ~5e59.
    let p = parse_d("M 0 0 A 5 5 0 0 1 1e60 1").expect("parses");
    assert!(p.path.elements().len() <= 3, "expected MoveTo+LineTo, got {} elements", p.path.elements().len());
    // Huge but finite coordinates on both endpoints.
    let p = parse_d("M 1e300 0 A 1 1 0 0 1 -1e300 0").expect("parses");
    assert!(p.path.elements().len() <= 3);
    // A sane arc still becomes cubics.
    let p = parse_d("M 0 0 A 5 5 0 0 1 10 0").expect("parses");
    assert!(p.path.elements().len() > 3, "a normal arc must be subdivided into cubics");
}
```

- [ ] **Step 2: Run it to verify it fails (or hangs)**

Run: `timeout 20 cargo test --test geom hostile_arc -- --nocapture`
Expected: the test does not finish within the timeout (exit 124) — that is the bug.

- [ ] **Step 3: Bound the endpoints and the converted radii**

In `src/geom/path.rs`, replace the `hostile` computation and the `from_svg_arc` match with:

```rust
                const LIMIT: f64 = 1e15;
                let hostile = !rx.is_finite()
                    || !ry.is_finite()
                    || !p.x.is_finite()
                    || !p.y.is_finite()
                    || !cur.x.is_finite()
                    || !cur.y.is_finite()
                    || rx.abs() > LIMIT
                    || ry.abs() > LIMIT
                    || p.x.abs() > LIMIT
                    || p.y.abs() > LIMIT
                    || cur.x.abs() > LIMIT
                    || cur.y.abs() > LIMIT;
                if hostile {
                    path.line_to(p);
                } else {
                    let svg_arc = SvgArc {
                        from: cur,
                        to: p,
                        radii: Vec2::new(rx.abs(), ry.abs()),
                        x_rotation: x_axis_rotation.to_radians(),
                        large_arc,
                        sweep,
                    };
                    match Arc::from_svg_arc(&svg_arc) {
                        // kurbo may have scaled the radii up to reach the endpoint;
                        // re-check them before subdividing.
                        Some(arc)
                            if arc.radii.x.is_finite()
                                && arc.radii.y.is_finite()
                                && arc.radii.x.abs() <= LIMIT
                                && arc.radii.y.abs() <= LIMIT =>
                        {
                            for el in arc.append_iter(1e-4) {
                                path.push(el);
                            }
                        }
                        _ => path.line_to(p),
                    }
                }
```

(`x`/`y` in the old guard were the raw, possibly relative, segment coordinates; `p` is the absolute endpoint — check the absolute values.)

- [ ] **Step 4: Run the test and the whole geom suite**

Run: `timeout 60 cargo test --test geom`
Expected: all pass, including `hostile_arc_endpoints_degrade_to_a_line_quickly`, in well under a second.

- [ ] **Step 5: Commit**

```bash
git add src/geom/path.rs tests/geom.rs
git commit -m "fix(geom): bound arc endpoints and converted radii so hostile arcs cannot hang parse_d"
```

---

### Task 2: DOM accessors the text engine assumes

**Files:**
- Modify: `src/geom/mod.rs` (add `impl Doc` block at the end)
- Modify: `src/dom.rs` (add three methods to `impl Doc`, after `ensure_id`)
- Test: `tests/geom.rs`, `tests/dom.rs`

**Interfaces:**
- Produces:
  - `impl Doc { pub fn transform(&self, n: NodeId) -> Affine }` — the element's own `transform` attribute parsed with `geom::parse_transform`; identity when absent or unparsable.
  - `impl Doc { pub fn composed_transform(&self, n: NodeId) -> Affine }` — product of `transform(a)` for every element ancestor `a` strictly below the root `<svg>`, outermost first, times `transform(n)`; the root `<svg>` is excluded. For `n == svg()` returns identity. (`kurbo` composition: `outer * inner` applies `inner` first.)
  - `impl Doc { pub fn xml_space_preserve(&self, n: NodeId) -> bool }` — nearest `xml:space` on `n` or an ancestor equals `"preserve"`.
  - `impl Doc { pub fn resolve_href(&self, n: NodeId) -> Option<NodeId> }` — `href`/`xlink:href` of the form `#id` looked up in the id index.
  - `impl Doc { pub fn new_id(&mut self, prefix: &str) -> String }` — `prefix` + smallest positive integer such that the id is unused (upstream `cache.py:934-955`: `FMArrowstart1`, …). Does not set it on any node.

- [ ] **Step 1: Write the failing tests**

Append to `tests/geom.rs`:

```rust
#[test]
fn composed_transform_excludes_root_svg_and_composes_outer_first() {
    use kurbo::{Affine, Point};
    use sciink::dom::Doc;
    let d = Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg" transform="scale(100)">
  <g id="a" transform="translate(10,20)"><g id="b" transform="scale(2)"><path id="p" d="M0,0" transform="translate(1,1)"/></g></g>
  <path id="q" d="M0,0"/></svg>"#).unwrap();
    let p = d.by_id("p").unwrap();
    let t = d.composed_transform(p);
    // outer-first: translate(10,20) * scale(2) * translate(1,1) maps (0,0) -> (12, 22)
    let got = t * Point::new(0.0, 0.0);
    assert!((got.x - 12.0).abs() < 1e-9 && (got.y - 22.0).abs() < 1e-9, "{got:?}");
    assert_eq!(d.composed_transform(d.by_id("q").unwrap()), Affine::IDENTITY);
    assert_eq!(d.composed_transform(d.svg()), Affine::IDENTITY);
    assert_eq!(d.transform(d.by_id("a").unwrap()), Affine::translate((10.0, 20.0)));
    assert_eq!(d.transform(d.by_id("q").unwrap()), Affine::IDENTITY);
}
```

Append to `tests/dom.rs`:

```rust
#[test]
fn xml_space_href_and_new_id_accessors() {
    let mut d = Doc::parse(br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <defs><path id="m1" d="M0,0"/></defs>
  <text id="t" xml:space="preserve"><tspan id="s"> a </tspan></text>
  <text id="u" xml:space="default"><tspan id="v">b</tspan></text>
  <use id="c" xlink:href="#m1"/><use id="e" href="#nope"/><use id="f" href="m1"/>
  <path id="FMArrowstart1" d="M0,0"/></svg>"##).unwrap();
    let id = |d: &Doc, i: &str| d.by_id(i).unwrap();
    assert!(d.xml_space_preserve(id(&d, "s")));
    assert!(d.xml_space_preserve(id(&d, "t")));
    assert!(!d.xml_space_preserve(id(&d, "v")));
    assert!(!d.xml_space_preserve(id(&d, "m1")));
    assert_eq!(d.resolve_href(id(&d, "c")), Some(id(&d, "m1")));
    assert_eq!(d.resolve_href(id(&d, "e")), None);
    assert_eq!(d.resolve_href(id(&d, "f")), None, "only fragment hrefs resolve");
    assert_eq!(d.new_id("FMArrowstart"), "FMArrowstart2");
    assert_eq!(d.new_id("sciink-x"), "sciink-x1");
    // new_id does not reserve: the same answer twice until something takes it
    assert_eq!(d.new_id("FMArrowstart"), "FMArrowstart2");
    let n = d.new_element("path");
    d.set_attr(n, "id", "FMArrowstart2");
    d.append_child(d.svg(), n);
    assert_eq!(d.new_id("FMArrowstart"), "FMArrowstart3");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test geom composed_transform 2>&1 | tail -5; cargo test --test dom xml_space_href 2>&1 | tail -5`
Expected: compile errors `no method named composed_transform` / `xml_space_preserve`.

- [ ] **Step 3: Implement**

Append to `src/geom/mod.rs`:

```rust
use crate::dom::{Doc, NodeId};

impl Doc {
    /// The element's own `transform` attribute (identity when absent or invalid).
    pub fn transform(&self, n: NodeId) -> Affine {
        self.attr(n, "transform")
            .and_then(parse_transform)
            .unwrap_or(Affine::IDENTITY)
    }

    /// Product of every ancestor's transform below the root `<svg>` (outermost first)
    /// and the element's own transform; the root's transform is excluded (inkex semantics).
    pub fn composed_transform(&self, n: NodeId) -> Affine {
        let svg = self.svg();
        if n == svg {
            return Affine::IDENTITY;
        }
        let mut chain: Vec<NodeId> = vec![n];
        for a in self.ancestors(n) {
            if a == svg || !self.is_element(a) {
                break;
            }
            chain.push(a);
        }
        let mut t = Affine::IDENTITY;
        for &node in chain.iter().rev() {
            t *= self.transform(node);
        }
        t
    }
}
```

(Check that `ancestors(n)` yields parents nearest-first and excludes `n` itself; if it includes `n`, skip the first item.)

Add to `impl Doc` in `src/dom.rs`, right after `ensure_id`:

```rust
    /// `true` when the nearest `xml:space` on the element or an ancestor is `preserve`.
    pub fn xml_space_preserve(&self, n: NodeId) -> bool {
        std::iter::once(n)
            .chain(self.ancestors(n))
            .filter(|&a| self.is_element(a))
            .find_map(|a| self.attr(a, "xml:space"))
            .is_some_and(|v| v.trim() == "preserve")
    }

    /// Target of an `href`/`xlink:href` of the form `#id`.
    pub fn resolve_href(&self, n: NodeId) -> Option<NodeId> {
        let h = self.href(n)?.trim();
        let id = h.strip_prefix('#')?;
        self.by_id(id)
    }

    /// `prefix` + the smallest positive integer giving an unused id (not reserved).
    pub fn new_id(&mut self, prefix: &str) -> String {
        let mut i = 1u32;
        loop {
            let cand = format!("{prefix}{i}");
            if !self.ids.contains_key(&cand) {
                return cand;
            }
            i += 1;
        }
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test geom && cargo test --test dom`
Expected: all pass.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/geom/mod.rs src/dom.rs tests/geom.rs tests/dom.rs
git commit -m "feat(dom): transform/composed_transform, xml_space_preserve, resolve_href, new_id"
```

---

### Task 3: Font system — vendored test fonts, face enumeration, face metrics, family matching

**Files:**
- Modify: `Cargo.toml` (`[dependencies]`)
- Create: `tests/fonts/DejaVuSans.ttf`, `tests/fonts/DejaVuSans-Bold.ttf`, `tests/fonts/Roboto-Regular.ttf`, `tests/fonts/Roboto-Bold.ttf`, `tests/fonts/LICENSE-DejaVu.txt`, `tests/fonts/LICENSE-Roboto.txt`, `tests/fonts/README.md`
- Create: `src/text/mod.rs`, `src/text/fonts.rs`
- Modify: `src/lib.rs` (add `pub mod text;`)
- Test: `tests/text_fonts.rs`

**Interfaces:**
- Produces (`sciink::text::fonts`):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
  pub struct FaceKey(pub u32);
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub enum FontStyle { Normal, Italic, Oblique }
  #[derive(Debug, Clone)]
  pub struct FaceInfo {
      pub family: String,      // first family name reported by the font
      pub path: Option<std::path::PathBuf>,
      pub index: u32,
      pub weight: u16,         // 100..=900
      pub style: FontStyle,
      pub width: u16,          // usWidthClass 1..=9, 5 = normal
      pub upem: f64,
      pub ascent: f64, pub descent: f64,          // normalised so ascent + descent = 1
      pub ascent_max: f64, pub descent_max: f64,  // hhea, em units
      pub x_height: f64, pub cap_height: f64,     // em units
  }
  pub struct FontSystem { /* private */ }
  impl FontSystem {
      pub fn load() -> FontSystem;                       // system fonts unless SCIINK_NO_SYSTEM_FONTS=1, plus SCIINK_FONT_DIRS
      pub fn from_dirs(dirs: &[std::path::PathBuf]) -> FontSystem; // no system fonts (tests)
      pub fn face_count(&self) -> usize;
      pub fn load_ms(&self) -> f64;
      pub fn face_info(&self, k: FaceKey) -> &FaceInfo;
      pub fn faces(&self) -> impl Iterator<Item = FaceKey> + '_;   // stable order: (family, weight, style, width, path)
      pub fn family_faces(&self, family: &str) -> &[FaceKey];      // case-insensitive family lookup, &[] when unknown
      pub fn pick(&self, cands: &[FaceKey], weight: u16, style: FontStyle, width: u16) -> Option<FaceKey>;
      pub fn with_face<T>(&self, k: FaceKey, f: impl FnOnce(&ttf_parser::Face<'_>) -> T) -> Option<T>;
      pub fn face_data(&self, k: FaceKey) -> Option<(std::rc::Rc<Vec<u8>>, u32)>; // bytes + face index, cached
      pub fn has_glyph(&self, k: FaceKey, c: char) -> bool;
  }
  ```
- `src/text/mod.rs` declares `pub mod fonts; pub mod metrics; pub mod style; pub mod tree; pub mod whitespace; pub mod table; pub mod parse; pub mod layout;` — add each `pub mod` line in the task that creates the file (only `fonts` in this task) — and:
  ```rust
  /// User-facing warnings collected while measuring/parsing (shown once, deduplicated).
  #[derive(Debug, Default)]
  pub struct Warnings(pub Vec<String>);
  impl Warnings {
      pub fn push(&mut self, s: impl Into<String>) { let s = s.into(); if !self.0.contains(&s) { self.0.push(s); } }
  }
  ```

- [ ] **Step 1: Add the dependencies**

In `Cargo.toml` `[dependencies]` add (keep alphabetical):

```toml
fontdb = "0.24.0"
rustybuzz = "0.20.1"
ttf-parser = "0.25.1"
```

Run: `cargo build 2>&1 | tail -3 && cargo tree -e normal --prefix depth | grep -E 'ttf-parser' | sort -u`
Expected: builds; exactly one `ttf-parser v0.25.1` line (no duplicate versions).

- [ ] **Step 2: Vendor the test fonts**

```bash
mkdir -p tests/fonts
curl -fsSL -o /tmp/dejavu.zip https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.zip
unzip -j -o /tmp/dejavu.zip '*/ttf/DejaVuSans.ttf' '*/ttf/DejaVuSans-Bold.ttf' -d tests/fonts/
unzip -j -o /tmp/dejavu.zip '*/LICENSE' -d /tmp/dejavu-license && mv /tmp/dejavu-license/LICENSE tests/fonts/LICENSE-DejaVu.txt
curl -fsSL -o /tmp/roboto.zip https://github.com/googlefonts/roboto/releases/download/v2.138/roboto-android.zip
unzip -l /tmp/roboto.zip | grep -E 'Roboto-(Regular|Bold)\.ttf'
unzip -j -o /tmp/roboto.zip '*Roboto-Regular.ttf' '*Roboto-Bold.ttf' -d tests/fonts/
curl -fsSL -o tests/fonts/LICENSE-Roboto.txt https://raw.githubusercontent.com/googlefonts/roboto-2/main/LICENSE
ls -la tests/fonts
```

Expected: four `.ttf` files (DejaVu ≈ 700–760 KB each, Roboto ≈ 130–180 KB each) and two licence files. Write `tests/fonts/README.md`:

```markdown
# Test fonts

Vendored so `cargo test` measures text deterministically on every OS (spec §C.5).

| Files | Source | Licence |
|---|---|---|
| `DejaVuSans.ttf`, `DejaVuSans-Bold.ttf` | dejavu-fonts 2.37 | Bitstream Vera + Arev licence (`LICENSE-DejaVu.txt`) |
| `Roboto-Regular.ttf`, `Roboto-Bold.ttf` | googlefonts/roboto v2.138 (`roboto-android.zip`) | Apache-2.0 (`LICENSE-Roboto.txt`) |

Not shipped in release zips. Tests load them with `FontSystem::from_dirs`.
```

- [ ] **Step 3: Write the failing tests**

Create `tests/text_fonts.rs`:

```rust
mod support;

use std::path::PathBuf;

use sciink::text::fonts::{FontStyle, FontSystem};

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}

#[test]
fn enumerates_vendored_faces_with_metrics() {
    let fs = fonts();
    assert_eq!(fs.face_count(), 4, "DejaVu Sans ×2 + Roboto ×2");
    let dv = fs.family_faces("dejavu sans"); // case-insensitive
    assert_eq!(dv.len(), 2);
    let reg = fs.pick(dv, 400, FontStyle::Normal, 5).unwrap();
    let bold = fs.pick(dv, 700, FontStyle::Normal, 5).unwrap();
    assert_ne!(reg, bold);
    let info = fs.face_info(reg);
    assert_eq!(info.family, "DejaVu Sans");
    assert_eq!(info.weight, 400);
    assert_eq!(info.style, FontStyle::Normal);
    assert_eq!(info.upem, 2048.0);
    assert!((info.ascent + info.descent - 1.0).abs() < 1e-12, "normalised ascent+descent");
    assert!(info.ascent > 0.7 && info.ascent < 0.85, "{}", info.ascent);
    // cap height from the 'I' glyph: DejaVu Sans caps are 0.729 em
    assert!((info.cap_height - 0.729).abs() < 0.005, "{}", info.cap_height);
    assert!(info.x_height > 0.5 && info.x_height < 0.6, "{}", info.x_height);
    assert_eq!(fs.face_info(bold).weight, 700);
    assert!(fs.family_faces("No Such Family").is_empty());
    assert!(fs.has_glyph(reg, 'A'));
    assert!(fs.has_glyph(reg, '\u{23A3}'), "DejaVu Sans covers ⎣");
    let rob = fs.pick(fs.family_faces("Roboto"), 400, FontStyle::Normal, 5).unwrap();
    assert!(!fs.has_glyph(rob, '\u{23A3}'), "Roboto lacks ⎣");
    assert!(fs.face_data(reg).is_some());
    assert!(fs.with_face(reg, |f| f.units_per_em()).unwrap() == 2048);
}

#[test]
fn pick_follows_css_weight_and_style_rules() {
    let fs = fonts();
    let dv = fs.family_faces("DejaVu Sans");
    let reg = fs.pick(dv, 400, FontStyle::Normal, 5).unwrap();
    let bold = fs.pick(dv, 700, FontStyle::Normal, 5).unwrap();
    // below 400: look lighter first, then heavier → only 400/700 exist → 400
    assert_eq!(fs.pick(dv, 300, FontStyle::Normal, 5), Some(reg));
    // 400..=500 tries up to 500 first, then lighter, then heavier → 400
    assert_eq!(fs.pick(dv, 500, FontStyle::Normal, 5), Some(reg));
    // above 500: heavier first → 700
    assert_eq!(fs.pick(dv, 600, FontStyle::Normal, 5), Some(bold));
    assert_eq!(fs.pick(dv, 900, FontStyle::Normal, 5), Some(bold));
    // no italic faces vendored: italic request falls back to normal of the same weight
    assert_eq!(fs.pick(dv, 700, FontStyle::Italic, 5), Some(bold));
    assert_eq!(fs.pick(&[], 400, FontStyle::Normal, 5), None);
}

#[test]
fn faces_iterate_in_stable_order_and_env_is_respected() {
    let fs = fonts();
    let fams: Vec<String> = fs.faces().map(|k| fs.face_info(k).family.clone()).collect();
    assert_eq!(fams, ["DejaVu Sans", "DejaVu Sans", "Roboto", "Roboto"]);
    // from_dirs never loads system fonts, so counts are exact regardless of the host.
    let empty = FontSystem::from_dirs(&[]);
    assert_eq!(empty.face_count(), 0);
    assert!(fs.load_ms() >= 0.0);
}
```

- [ ] **Step 4: Run to verify they fail**

Run: `cargo test --test text_fonts 2>&1 | head -20`
Expected: `unresolved import sciink::text`.

- [ ] **Step 5: Implement `src/text/mod.rs` and `src/text/fonts.rs`**

`src/lib.rs`: add `pub mod text;` (alphabetical, after `pub mod style;`).

`src/text/mod.rs`:

```rust
//! Text engine (spec docs/spec/01-text-engine.md): fonts → metrics → parse → layout.

pub mod fonts;

/// User-facing warnings collected while measuring/parsing (deduplicated).
#[derive(Debug, Default)]
pub struct Warnings(pub Vec<String>);

impl Warnings {
    pub fn push(&mut self, s: impl Into<String>) {
        let s = s.into();
        if !self.0.contains(&s) {
            self.0.push(s);
        }
    }
}
```

`src/text/fonts.rs`:

```rust
//! Font discovery and selection (spec §A.3). fontdb enumerates faces; matching is
//! ours so it is case-insensitive and follows CSS weight rules like fontconfig does.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceKey(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone)]
pub struct FaceInfo {
    pub family: String,
    pub path: Option<PathBuf>,
    pub index: u32,
    pub weight: u16,
    pub style: FontStyle,
    pub width: u16,
    pub upem: f64,
    pub ascent: f64,
    pub descent: f64,
    pub ascent_max: f64,
    pub descent_max: f64,
    pub x_height: f64,
    pub cap_height: f64,
}

pub struct FontSystem {
    db: fontdb::Database,
    ids: Vec<fontdb::ID>,
    infos: Vec<FaceInfo>,
    by_family: HashMap<String, Vec<FaceKey>>,
    data: RefCell<HashMap<FaceKey, Rc<Vec<u8>>>>,
    load_ms: f64,
}

impl FontSystem {
    /// System fonts (unless `SCIINK_NO_SYSTEM_FONTS=1`) plus every dir in `SCIINK_FONT_DIRS`.
    pub fn load() -> FontSystem {
        let mut db = fontdb::Database::new();
        if std::env::var_os("SCIINK_NO_SYSTEM_FONTS").is_none_or(|v| v != "1") {
            db.load_system_fonts();
        }
        if let Some(dirs) = std::env::var_os("SCIINK_FONT_DIRS") {
            for d in std::env::split_paths(&dirs) {
                db.load_fonts_dir(d);
            }
        }
        Self::from_db(db)
    }

    /// Only the given directories (tests).
    pub fn from_dirs(dirs: &[PathBuf]) -> FontSystem {
        let mut db = fontdb::Database::new();
        for d in dirs {
            db.load_fonts_dir(d);
        }
        Self::from_db(db)
    }

    fn from_db(db: fontdb::Database) -> FontSystem {
        let t0 = Instant::now();
        let mut entries: Vec<(fontdb::ID, FaceInfo)> = Vec::new();
        for f in db.faces() {
            let family = f
                .families
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| f.post_script_name.clone());
            let (path, index) = match &f.source {
                fontdb::Source::File(p) => (Some(p.clone()), f.index),
                fontdb::Source::SharedFile(p, _) => (Some(p.clone()), f.index),
                fontdb::Source::Binary(_) => (None, f.index),
            };
            let style = match f.style {
                fontdb::Style::Normal => FontStyle::Normal,
                fontdb::Style::Italic => FontStyle::Italic,
                fontdb::Style::Oblique => FontStyle::Oblique,
            };
            let metrics = db.with_face_data(f.id, |data, idx| {
                ttf_parser::Face::parse(data, idx).ok().map(|face| face_metrics(&face))
            });
            let Some(Some(m)) = metrics else { continue }; // unparsable face: skip
            entries.push((
                f.id,
                FaceInfo {
                    family,
                    path,
                    index,
                    weight: f.weight.0,
                    style,
                    width: f.stretch.to_number(),
                    upem: m.0,
                    ascent: m.1,
                    descent: m.2,
                    ascent_max: m.3,
                    descent_max: m.4,
                    x_height: m.5,
                    cap_height: m.6,
                },
            ));
        }
        entries.sort_by(|a, b| {
            let ka = (&a.1.family, a.1.weight, a.1.style as u8, a.1.width, &a.1.path, a.1.index);
            let kb = (&b.1.family, b.1.weight, b.1.style as u8, b.1.width, &b.1.path, b.1.index);
            ka.cmp(&kb)
        });
        let mut by_family: HashMap<String, Vec<FaceKey>> = HashMap::new();
        for (i, (id, _)) in entries.iter().enumerate() {
            if let Some(f) = db.face(*id) {
                for (name, _) in &f.families {
                    by_family
                        .entry(name.trim().to_lowercase())
                        .or_default()
                        .push(FaceKey(i as u32));
                }
            }
        }
        for v in by_family.values_mut() {
            v.sort();
            v.dedup();
        }
        let (ids, infos): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        FontSystem {
            db,
            ids,
            infos,
            by_family,
            data: RefCell::new(HashMap::new()),
            load_ms: t0.elapsed().as_secs_f64() * 1000.0,
        }
    }

    pub fn face_count(&self) -> usize {
        self.infos.len()
    }

    pub fn load_ms(&self) -> f64 {
        self.load_ms
    }

    pub fn face_info(&self, k: FaceKey) -> &FaceInfo {
        &self.infos[k.0 as usize]
    }

    pub fn faces(&self) -> impl Iterator<Item = FaceKey> + '_ {
        (0..self.infos.len() as u32).map(FaceKey)
    }

    /// Faces whose font reports `family` (case-insensitive); empty when unknown.
    pub fn family_faces(&self, family: &str) -> &[FaceKey] {
        self.by_family
            .get(&family.trim().to_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// CSS Fonts §5.2 matching: width, then style (italic↔oblique, then normal), then weight.
    pub fn pick(&self, cands: &[FaceKey], weight: u16, style: FontStyle, width: u16) -> Option<FaceKey> {
        if cands.is_empty() {
            return None;
        }
        // width: nearest; ties prefer narrower when width <= 5, wider otherwise
        let best_w = cands
            .iter()
            .map(|&k| {
                let w = self.face_info(k).width;
                let d = (w as i32 - width as i32).abs();
                let side = if width <= 5 { (w > width) as i32 } else { (w < width) as i32 };
                (d, side)
            })
            .min()?;
        let cands: Vec<FaceKey> = cands
            .iter()
            .copied()
            .filter(|&k| {
                let w = self.face_info(k).width;
                let d = (w as i32 - width as i32).abs();
                let side = if width <= 5 { (w > width) as i32 } else { (w < width) as i32 };
                (d, side) == best_w
            })
            .collect();
        let style_pref: &[FontStyle] = match style {
            FontStyle::Normal => &[FontStyle::Normal, FontStyle::Oblique, FontStyle::Italic],
            FontStyle::Italic => &[FontStyle::Italic, FontStyle::Oblique, FontStyle::Normal],
            FontStyle::Oblique => &[FontStyle::Oblique, FontStyle::Italic, FontStyle::Normal],
        };
        let styled: Vec<FaceKey> = style_pref
            .iter()
            .find_map(|s| {
                let v: Vec<FaceKey> = cands.iter().copied().filter(|&k| self.face_info(k).style == *s).collect();
                (!v.is_empty()).then_some(v)
            })
            .unwrap_or(cands);
        let ws: Vec<(u16, FaceKey)> = styled.iter().map(|&k| (self.face_info(k).weight, k)).collect();
        let exact = ws.iter().find(|(w, _)| *w == weight).map(|(_, k)| *k);
        if exact.is_some() {
            return exact;
        }
        let lighter = || ws.iter().filter(|(w, _)| *w < weight).max_by_key(|(w, _)| *w).map(|(_, k)| *k);
        let heavier = || ws.iter().filter(|(w, _)| *w > weight).min_by_key(|(w, _)| *w).map(|(_, k)| *k);
        if (400..=500).contains(&weight) {
            let up_to_500 = ws.iter().filter(|(w, _)| *w > weight && *w <= 500).min_by_key(|(w, _)| *w).map(|(_, k)| *k);
            up_to_500.or_else(lighter).or_else(heavier)
        } else if weight < 400 {
            lighter().or_else(heavier)
        } else {
            heavier().or_else(lighter)
        }
    }

    /// Font bytes and face index, read once and cached.
    pub fn face_data(&self, k: FaceKey) -> Option<(Rc<Vec<u8>>, u32)> {
        let info = self.face_info(k);
        if let Some(d) = self.data.borrow().get(&k) {
            return Some((d.clone(), info.index));
        }
        let bytes = self.db.with_face_data(self.ids[k.0 as usize], |data, _| data.to_vec())?;
        let rc = Rc::new(bytes);
        self.data.borrow_mut().insert(k, rc.clone());
        Some((rc, info.index))
    }

    pub fn with_face<T>(&self, k: FaceKey, f: impl FnOnce(&ttf_parser::Face<'_>) -> T) -> Option<T> {
        let (data, index) = self.face_data(k)?;
        let face = ttf_parser::Face::parse(&data, index).ok()?;
        Some(f(&face))
    }

    pub fn has_glyph(&self, k: FaceKey, c: char) -> bool {
        self.with_face(k, |f| f.glyph_index(c).is_some()).unwrap_or(false)
    }
}

/// Port of Inkscape's `find_font_metrics` (spec §A.3): returns
/// (upem, ascent, descent, ascent_max, descent_max, x_height, cap_height).
fn face_metrics(face: &ttf_parser::Face<'_>) -> (f64, f64, f64, f64, f64, f64, f64) {
    let upem = face.units_per_em() as f64;
    let (mut asc, mut desc) = match (face.typographic_ascender(), face.typographic_descender()) {
        (Some(a), Some(d)) => ((a as f64 / upem).abs(), (d as f64 / upem).abs()),
        _ => ((face.ascender() as f64 / upem).abs(), (face.descender() as f64 / upem).abs()),
    };
    let asc_max = (face.ascender() as f64 / upem).abs();
    let desc_max = (face.descender() as f64 / upem).abs();
    let em = asc + desc;
    if em > 0.0 {
        asc /= em;
        desc /= em;
    }
    let x_height = match face.x_height() {
        Some(x) if x != 0 => (x as f64 / upem).abs(),
        _ => face
            .glyph_index('x')
            .and_then(|g| face.glyph_bounding_box(g))
            .map(|b| (b.y_max as f64 / upem).abs())
            .unwrap_or(0.5),
    };
    let cap_height = face
        .glyph_index('I')
        .and_then(|g| face.glyph_bounding_box(g))
        .map(|b| b.y_max as f64 / upem)
        .filter(|v| *v > 0.0)
        .or_else(|| face.capital_height().filter(|v| *v != 0).map(|v| v as f64 / upem))
        .unwrap_or(0.7);
    (upem, asc, desc, asc_max, desc_max, x_height, cap_height)
}
```

Notes for the implementer: `fontdb::Stretch` is `ttf_parser::Width`, whose `to_number()` gives the usWidthClass 1–9. `fontdb::Source` has the three variants above in 0.24 (`SharedFile` is behind the `memmap` feature; if the compiler reports it as unknown, drop that arm). `Option::is_none_or` is stable since Rust 1.82. `glyph_bounding_box` is `None` for CFF fonts; `cap_height` then falls back to `capital_height()`.

- [ ] **Step 6: Run the tests**

Run: `cargo test --test text_fonts`
Expected: 3 passed. If `cap_height` is off, print `fs.face_info(reg)` and compare with `fc-query tests/fonts/DejaVuSans.ttf` — DejaVu Sans 'I' yMax is 1493/2048 = 0.72900390625.

- [ ] **Step 7: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add Cargo.toml Cargo.lock src/lib.rs src/text tests/fonts tests/text_fonts.rs
git commit -m "feat(text): font system — fontdb enumeration, face metrics, CSS-style family matching; vendored DejaVu Sans and Roboto for tests"
```

---

### Task 4: `FontSpec` from a style and the resolution ladder (aliases, generics, last resort, per-char fallback)

**Files:**
- Modify: `src/text/fonts.rs` (append)
- Test: `tests/text_fonts.rs` (append)

**Interfaces:**
- Consumes: `FontSystem::{family_faces, pick, has_glyph, faces, face_info}` (Task 3); `sciink::style::Style` (`get(&str) -> Option<&str>`).
- Produces (`sciink::text::fonts`):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Hash)]
  pub struct FontSpec { pub families: Vec<String>, pub weight: u16, pub style: FontStyle, pub width: u16 }
  impl FontSpec {
      pub fn from_style(st: &Style) -> FontSpec;   // FP:357–370 + weight map FP:1143–1168
      pub fn key(&self) -> String;                 // upstream fsty string: "'A','B'|700|italic|5"
  }
  impl FontSystem {
      pub fn resolve(&mut self, spec: &FontSpec) -> Option<FaceKey>;                 // None only when the system has no faces at all
      pub fn resolve_for_char(&mut self, spec: &FontSpec, c: char) -> Option<FaceKey>; // FP:220–246; None = unrendered
      pub fn candidates(&self, spec: &FontSpec) -> Vec<FaceKey>;                       // the whole ordered ladder, deduplicated
  }
  pub const GENERIC_SANS: &[&str]; pub const GENERIC_SERIF: &[&str]; pub const GENERIC_MONO: &[&str];
  pub const METRIC_ALIASES: &[&[&str]];
  pub const WIDE_COVERAGE: &[&str];
  ```
  Ladder for `candidates(spec)` (spec §A.3 steps 1–4 and per-char fallback): for each family in order — the family itself, then every family in its metric-alias group, then, if the family is a generic (`sans-serif`/`serif`/`monospace`/`cursive`/`fantasy`/`system-ui`) its generic list; after all families: `GENERIC_SANS`; then `WIDE_COVERAGE`; then every remaining face sorted by (same style first, |weight − requested|, family). Each family contributes `pick(family_faces(f), weight, style, width)`; the ladder is deduplicated preserving order. `resolve` = first candidate. `resolve_for_char` = first candidate that `has_glyph(c)`. Both memoised in `FontSystem` (`HashMap<FontSpec, Vec<FaceKey>>`, `HashMap<(FontSpec, char), Option<FaceKey>>`).

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_fonts.rs`:

```rust
use sciink::style::Style;
use sciink::text::fonts::FontSpec;

#[test]
fn font_spec_from_style_follows_upstream_font_style() {
    let st = Style::parse("font-family: 'DejaVu Sans' , Arial ;font-weight:bold;font-style:italic;font-stretch:condensed");
    let spec = FontSpec::from_style(&st);
    assert_eq!(spec.families, ["DejaVu Sans", "Arial"]);
    assert_eq!(spec.weight, 700);
    assert_eq!(spec.style, FontStyle::Italic);
    assert_eq!(spec.width, 3);
    assert_eq!(spec.key(), "'DejaVu Sans','Arial'|700|italic|3");
    let dflt = FontSpec::from_style(&Style::parse("fill:red"));
    assert_eq!(dflt.families, ["sans-serif"]);
    assert_eq!((dflt.weight, dflt.style, dflt.width), (400, FontStyle::Normal, 5));
    // numeric weights pass through; unknown keywords (bolder/lighter/semibold) → 400 like Inkscape
    assert_eq!(FontSpec::from_style(&Style::parse("font-weight:300")).weight, 300);
    assert_eq!(FontSpec::from_style(&Style::parse("font-weight:bolder")).weight, 400);
    assert_eq!(FontSpec::from_style(&Style::parse("font-weight:semibold")).weight, 400);
    assert_eq!(FontSpec::from_style(&Style::parse("font-style:oblique")).style, FontStyle::Oblique);
    assert_eq!(FontSpec::from_style(&Style::parse("font-stretch:ultra-expanded")).width, 9);
}

#[test]
fn resolution_ladder_with_vendored_fonts_only() {
    let mut fs = fonts();
    let spec = |css: &str| FontSpec::from_style(&Style::parse(css));
    let dv = fs.pick(fs.family_faces("DejaVu Sans"), 400, FontStyle::Normal, 5).unwrap();
    let dvb = fs.pick(fs.family_faces("DejaVu Sans"), 700, FontStyle::Normal, 5).unwrap();
    let rob = fs.pick(fs.family_faces("Roboto"), 400, FontStyle::Normal, 5).unwrap();
    assert_eq!(fs.resolve(&spec("font-family:DejaVu Sans")), Some(dv));
    assert_eq!(fs.resolve(&spec("font-family:'dejavu sans';font-weight:bold")), Some(dvb));
    assert_eq!(fs.resolve(&spec("font-family:Roboto")), Some(rob));
    // generic sans-serif → first present family of the fontconfig list (DejaVu Sans)
    assert_eq!(fs.resolve(&spec("font-family:sans-serif")), Some(dv));
    // metric alias: Helvetica ↔ Arial ↔ Liberation Sans ↔ Nimbus Sans — none present → generic sans → DejaVu
    assert_eq!(fs.resolve(&spec("font-family:Helvetica")), Some(dv));
    // family list order wins before any fallback
    assert_eq!(fs.resolve(&spec("font-family:Nope, Roboto, 'DejaVu Sans'")), Some(rob));
    // unknown family, serif and monospace all fall through to sans-serif → DejaVu Sans
    assert_eq!(fs.resolve(&spec("font-family:Zapf Chancery")), Some(dv));
    assert_eq!(fs.resolve(&spec("font-family:serif")), Some(dv));
    assert_eq!(fs.resolve(&spec("font-family:monospace")), Some(dv));
    // per-char fallback: Roboto lacks ⎣, DejaVu has it; nothing has U+10348
    assert_eq!(fs.resolve_for_char(&spec("font-family:Roboto"), 'a'), Some(rob));
    assert_eq!(fs.resolve_for_char(&spec("font-family:Roboto"), '\u{23A3}'), Some(dv));
    assert_eq!(fs.resolve_for_char(&spec("font-family:Roboto"), '\u{10348}'), None);
    // the ladder lists every face exactly once
    let c = fs.candidates(&spec("font-family:Roboto;font-weight:bold"));
    assert_eq!(c.len(), 4, "{c:?}");
    assert_eq!(c[0], fs.pick(fs.family_faces("Roboto"), 700, FontStyle::Normal, 5).unwrap());
    let empty = FontSystem::from_dirs(&[]);
    let mut empty = empty;
    assert_eq!(empty.resolve(&spec("font-family:Arial")), None);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_fonts 2>&1 | grep -E '^error' | head -5`
Expected: `cannot find struct FontSpec`.

- [ ] **Step 3: Implement**

Append to `src/text/fonts.rs` (and add `use crate::style::Style;` at the top; add the two memo maps to the `FontSystem` struct and initialise them empty in `from_db`):

```rust
/// fontconfig 60-latin.conf preference order (spec §A.3 step 3).
pub const GENERIC_SANS: &[&str] = &[
    "DejaVu Sans", "Bitstream Vera Sans", "Verdana", "Arial", "Albany AMT", "Luxi Sans",
    "Nimbus Sans L", "Nimbus Sans", "Helvetica", "Lucida Sans Unicode", "Tahoma", "Noto Sans",
];
pub const GENERIC_SERIF: &[&str] = &[
    "DejaVu Serif", "Bitstream Vera Serif", "Times New Roman", "Thorndale AMT", "Luxi Serif",
    "Nimbus Roman No9 L", "Nimbus Roman", "Times", "Noto Serif",
];
pub const GENERIC_MONO: &[&str] = &[
    "DejaVu Sans Mono", "Bitstream Vera Sans Mono", "Inconsolata", "Andale Mono", "Courier New",
    "Cumberland AMT", "Luxi Mono", "Nimbus Mono L", "Nimbus Mono PS", "Courier", "Noto Sans Mono",
];
/// fontconfig 30-metric-aliases.conf groups (spec §A.3 step 2).
pub const METRIC_ALIASES: &[&[&str]] = &[
    &["Helvetica", "Arial", "Liberation Sans", "Nimbus Sans", "Nimbus Sans L", "Arimo", "Albany", "Albany AMT"],
    &["Times", "Times New Roman", "Liberation Serif", "Nimbus Roman", "Nimbus Roman No9 L", "Tinos", "Thorndale", "Thorndale AMT"],
    &["Courier", "Courier New", "Liberation Mono", "Nimbus Mono", "Nimbus Mono L", "Nimbus Mono PS", "Cousine", "Cumberland", "Cumberland AMT"],
    &["Calibri", "Carlito"],
    &["Cambria", "Caladea"],
    &["Georgia", "Gelasio"],
];
/// Curated wide-coverage fallbacks tried before "any face".
pub const WIDE_COVERAGE: &[&str] = &[
    "Noto Sans", "Noto Sans Symbols", "Noto Sans Symbols 2", "Noto Sans Math", "DejaVu Sans",
    "Arial Unicode MS", "Segoe UI Symbol", "Cambria Math", "Apple Symbols", "STIX Two Math", "Symbola",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontSpec {
    pub families: Vec<String>,
    pub weight: u16,
    pub style: FontStyle,
    pub width: u16,
}

impl FontSpec {
    /// The four properties that select a font (upstream `font_style`, FP:357–370).
    pub fn from_style(st: &Style) -> FontSpec {
        let fam = st.get("font-family").unwrap_or("sans-serif");
        let families: Vec<String> = fam
            .split(',')
            .map(|f| f.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
        let families = if families.is_empty() { vec!["sans-serif".to_string()] } else { families };
        let weight = match st.get("font-weight").map(str::trim).unwrap_or("normal") {
            "bold" => 700,
            w => w
                .parse::<u16>()
                .ok()
                .filter(|v| (100..=1000).contains(v) && v % 50 == 0)
                .unwrap_or(400), // normal, bolder, lighter, semibold, … → Inkscape uses normal
        };
        let style = match st.get("font-style").map(str::trim).unwrap_or("normal") {
            "italic" => FontStyle::Italic,
            "oblique" => FontStyle::Oblique,
            _ => FontStyle::Normal,
        };
        let width = match st.get("font-stretch").map(str::trim).unwrap_or("normal") {
            "ultra-condensed" => 1,
            "extra-condensed" => 2,
            "condensed" => 3,
            "semi-condensed" => 4,
            "semi-expanded" => 6,
            "expanded" => 7,
            "extra-expanded" => 8,
            "ultra-expanded" => 9,
            _ => 5,
        };
        FontSpec { families, weight, style, width }
    }

    /// Upstream's `fsty` key: quoted, comma-joined families plus weight/style/width.
    pub fn key(&self) -> String {
        let fams: Vec<String> = self.families.iter().map(|f| format!("'{f}'")).collect();
        let sty = match self.style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        };
        format!("{}|{}|{}|{}", fams.join(","), self.weight, sty, self.width)
    }
}

fn generic_list(family: &str) -> Option<&'static [&'static str]> {
    match family.to_ascii_lowercase().as_str() {
        "sans-serif" | "sans" | "system-ui" | "ui-sans-serif" => Some(GENERIC_SANS),
        "serif" | "ui-serif" => Some(GENERIC_SERIF),
        "monospace" | "mono" | "ui-monospace" => Some(GENERIC_MONO),
        "cursive" | "fantasy" => Some(GENERIC_SANS),
        _ => None,
    }
}

impl FontSystem {
    fn push_family(&self, out: &mut Vec<FaceKey>, family: &str, spec: &FontSpec) {
        if let Some(k) = self.pick(self.family_faces(family), spec.weight, spec.style, spec.width) {
            if !out.contains(&k) {
                out.push(k);
            }
        }
    }

    /// The whole ordered fallback ladder for `spec` (spec §A.3), each face once.
    pub fn candidates(&self, spec: &FontSpec) -> Vec<FaceKey> {
        let mut out = Vec::new();
        for fam in &spec.families {
            self.push_family(&mut out, fam, spec);
            for group in METRIC_ALIASES {
                if group.iter().any(|g| g.eq_ignore_ascii_case(fam)) {
                    for g in *group {
                        self.push_family(&mut out, g, spec);
                    }
                }
            }
            if let Some(list) = generic_list(fam) {
                for g in list {
                    self.push_family(&mut out, g, spec);
                }
            }
        }
        for g in GENERIC_SANS.iter().chain(WIDE_COVERAGE) {
            self.push_family(&mut out, g, spec);
        }
        // last resort: every remaining face, same style first, nearest weight, then family
        let mut rest: Vec<FaceKey> = self.faces().filter(|k| !out.contains(k)).collect();
        rest.sort_by_key(|&k| {
            let i = self.face_info(k);
            ((i.style != spec.style) as u8, (i.weight as i32 - spec.weight as i32).abs(), i.family.clone(), k)
        });
        out.extend(rest);
        out
    }

    /// The face Inkscape would pick for the whole run (`true_style`).
    pub fn resolve(&mut self, spec: &FontSpec) -> Option<FaceKey> {
        self.ladder(spec).first().copied()
    }

    /// The face that actually renders `c` under `spec`; `None` = no installed font has the glyph.
    pub fn resolve_for_char(&mut self, spec: &FontSpec, c: char) -> Option<FaceKey> {
        if let Some(r) = self.char_memo.get(&(spec.clone(), c)) {
            return *r;
        }
        let r = self.ladder(spec).iter().copied().find(|&k| self.has_glyph(k, c));
        self.char_memo.insert((spec.clone(), c), r);
        r
    }

    fn ladder(&mut self, spec: &FontSpec) -> Vec<FaceKey> {
        if let Some(v) = self.ladder_memo.get(spec) {
            return v.clone();
        }
        let v = self.candidates(spec);
        self.ladder_memo.insert(spec.clone(), v.clone());
        v
    }
}
```

Struct fields to add: `ladder_memo: HashMap<FontSpec, Vec<FaceKey>>`, `char_memo: HashMap<(FontSpec, char), Option<FaceKey>>`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_fonts`
Expected: 5 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/fonts.rs tests/text_fonts.rs
git commit -m "feat(text): FontSpec from CSS and the fontconfig-style resolution ladder with per-character fallback"
```

---

### Task 5: Character metrics — advances, space width, cap height, ink boxes, pair adjustments

**Files:**
- Create: `src/text/metrics.rs`
- Modify: `src/text/mod.rs` (`pub mod metrics;`)
- Test: `tests/text_metrics.rs`

**Interfaces:**
- Consumes: `FontSystem::{face_data, face_info, with_face}` (Task 3).
- Produces (`sciink::text::metrics`):
  ```rust
  /// Per-(face, char) properties in em units (upstream `CProp`, P:4244–4287).
  #[derive(Debug, Clone, PartialEq)]
  pub struct CProp {
      pub c: char,
      pub charw: f64,              // advance of the char shaped alone
      pub spacew: f64,             // advance of ' ' in the same face
      pub caph: f64,               // face cap height (FaceInfo::cap_height)
      pub inkbb: [f64; 4],         // [x_min, -y_max, width, height] relative to the pen, y down; zeros for blank glyphs
      pub dadvs: HashMap<char, f64>, // extra advance when `prev` precedes this char: adv(prev+c) − adv(prev) − adv(c)
  }
  pub struct Metrics { /* caches keyed by FaceKey / (FaceKey, char) / (FaceKey, char, char) */ }
  impl Metrics {
      pub fn new() -> Metrics;
      pub fn advance(&mut self, fs: &FontSystem, k: FaceKey, s: &str) -> f64;           // shaped advance of the string, em
      pub fn prop(&mut self, fs: &FontSystem, k: FaceKey, c: char, prev: &[char]) -> Rc<CProp>; // dadvs filled for `prev`
      pub fn pair_adv(&mut self, fs: &FontSystem, k: FaceKey, prev: char, c: char) -> f64;
      pub fn unrendered(c: char) -> Rc<CProp>;                                          // zero widths/ink, caph 0
  }
  ```
  Shaping: `rustybuzz::Face::from_slice(&data, index)`, `UnicodeBuffer::new()` + `push_str`, `rustybuzz::shape(&face, &[], buf)`, sum of `glyph_positions()[i].x_advance` / `units_per_em`. Default features (kern + liga on, like Pango). Ink box: `ttf_parser::Face::outline_glyph(gid, &mut NoopBuilder)` → `Option<ttf_parser::Rect>`; `[x_min, -y_max, x_max−x_min, y_max−y_min] / upem`; `None` (blank glyph or missing) → `[0.0; 4]`. `caph` = `face_info(k).cap_height`.

- [ ] **Step 1: Write the failing tests**

Create `tests/text_metrics.rs`:

```rust
mod support;

use std::path::PathBuf;

use sciink::text::fonts::{FontStyle, FontSystem};
use sciink::text::metrics::Metrics;

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}

#[test]
fn single_char_advance_matches_the_font_tables() {
    let fs = fonts();
    let dv = fs.pick(fs.family_faces("DejaVu Sans"), 400, FontStyle::Normal, 5).unwrap();
    let mut m = Metrics::new();
    // hmtx advance of 'I' straight from ttf-parser, in em
    let expect = fs
        .with_face(dv, |f| {
            let g = f.glyph_index('I').unwrap();
            f.glyph_hor_advance(g).unwrap() as f64 / f.units_per_em() as f64
        })
        .unwrap();
    let p = m.prop(&fs, dv, 'I', &[]);
    assert!((p.charw - expect).abs() < 1e-12, "{} vs {expect}", p.charw);
    assert!(p.charw > 0.29 && p.charw < 0.30, "DejaVu Sans 'I' is 604/2048 em: {}", p.charw);
    assert!(p.spacew > 0.31 && p.spacew < 0.32, "DejaVu Sans space is 651/2048 em: {}", p.spacew);
    assert!((p.caph - 0.729).abs() < 0.005);
    // ink box of 'I': narrow, from the baseline up to the cap height
    let [x, y, w, h] = p.inkbb;
    assert!(x > 0.0 && w > 0.0 && w < p.charw, "{:?}", p.inkbb);
    assert!((y + 0.729).abs() < 0.005, "y = -y_max: {y}");
    assert!((h - 0.729).abs() < 0.005, "I sits on the baseline: {h}");
    // a space has no ink
    let sp = m.prop(&fs, dv, ' ', &[]);
    assert_eq!(sp.inkbb, [0.0; 4]);
    assert!((sp.charw - sp.spacew).abs() < 1e-12);
    // unrendered placeholder
    let u = Metrics::unrendered('\u{10348}');
    assert_eq!((u.charw, u.spacew, u.caph, u.inkbb), (0.0, 0.0, 0.0, [0.0; 4]));
}

#[test]
fn pair_adjustments_capture_kerning_and_ligatures() {
    let fs = fonts();
    let dv = fs.pick(fs.family_faces("DejaVu Sans"), 400, FontStyle::Normal, 5).unwrap();
    let mut m = Metrics::new();
    let av = m.pair_adv(&fs, dv, 'A', 'V');
    assert!(av < -0.01 && av > -0.2, "DejaVu Sans kerns A–V negative: {av}");
    let ii = m.pair_adv(&fs, dv, 'I', 'I');
    assert!(ii.abs() < 1e-9, "no kerning between I and I: {ii}");
    // dadvs of a prop are filled only for the requested predecessors (+ always finite)
    let p = m.prop(&fs, dv, 'V', &['A', ' ']);
    assert_eq!(p.dadvs.len(), 2);
    assert!((p.dadvs[&'A'] - av).abs() < 1e-12);
    assert!(p.dadvs[&' '].abs() < 1e-9);
    // string advance is additive up to kerning
    let a = m.advance(&fs, dv, "A");
    let v = m.advance(&fs, dv, "V");
    let both = m.advance(&fs, dv, "AV");
    assert!((both - (a + v + av)).abs() < 1e-12);
    // the same request twice hits the cache and stays identical
    assert_eq!(m.prop(&fs, dv, 'V', &['A', ' ']), p);
}

#[test]
fn bold_face_is_wider_than_regular() {
    let fs = fonts();
    let reg = fs.pick(fs.family_faces("DejaVu Sans"), 400, FontStyle::Normal, 5).unwrap();
    let bold = fs.pick(fs.family_faces("DejaVu Sans"), 700, FontStyle::Normal, 5).unwrap();
    let mut m = Metrics::new();
    assert!(m.advance(&fs, bold, "Hamburgefonstiv") > m.advance(&fs, reg, "Hamburgefonstiv"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_metrics 2>&1 | grep -E '^error' | head -3`
Expected: `could not find metrics in text`.

- [ ] **Step 3: Implement `src/text/metrics.rs`**

```rust
//! Character measurement with rustybuzz (shaping = HarfBuzz = what Pango uses) and
//! ttf-parser (outlines). All values in em units; callers scale by the font size (spec §A.3).

use std::collections::HashMap;
use std::rc::Rc;

use super::fonts::{FaceKey, FontSystem};

#[derive(Debug, Clone, PartialEq)]
pub struct CProp {
    pub c: char,
    pub charw: f64,
    pub spacew: f64,
    pub caph: f64,
    pub inkbb: [f64; 4],
    pub dadvs: HashMap<char, f64>,
}

#[derive(Default)]
pub struct Metrics {
    adv: HashMap<(FaceKey, String), f64>,
    ink: HashMap<(FaceKey, char), [f64; 4]>,
    pairs: HashMap<(FaceKey, char, char), f64>,
    props: HashMap<(FaceKey, char), Rc<CProp>>,
}

struct NoopBuilder;
impl ttf_parser::OutlineBuilder for NoopBuilder {
    fn move_to(&mut self, _x: f32, _y: f32) {}
    fn line_to(&mut self, _x: f32, _y: f32) {}
    fn quad_to(&mut self, _x1: f32, _y1: f32, _x: f32, _y: f32) {}
    fn curve_to(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x: f32, _y: f32) {}
    fn close(&mut self) {}
}

impl Metrics {
    pub fn new() -> Metrics {
        Metrics::default()
    }

    /// Shaped advance of `s` in em (default features: kerning and standard ligatures on).
    pub fn advance(&mut self, fs: &FontSystem, k: FaceKey, s: &str) -> f64 {
        if let Some(v) = self.adv.get(&(k, s.to_string())) {
            return *v;
        }
        let v = fs
            .face_data(k)
            .and_then(|(data, index)| {
                let face = rustybuzz::Face::from_slice(&data, index)?;
                let mut buf = rustybuzz::UnicodeBuffer::new();
                buf.push_str(s);
                let out = rustybuzz::shape(&face, &[], buf);
                let total: i64 = out.glyph_positions().iter().map(|p| p.x_advance as i64).sum();
                Some(total as f64 / face.units_per_em() as f64)
            })
            .unwrap_or(0.0);
        self.adv.insert((k, s.to_string()), v);
        v
    }

    fn ink(&mut self, fs: &FontSystem, k: FaceKey, c: char) -> [f64; 4] {
        if let Some(v) = self.ink.get(&(k, c)) {
            return *v;
        }
        let v = fs
            .with_face(k, |f| {
                let upem = f.units_per_em() as f64;
                f.glyph_index(c)
                    .and_then(|g| f.outline_glyph(g, &mut NoopBuilder))
                    .map(|r| {
                        [
                            r.x_min as f64 / upem,
                            -(r.y_max as f64) / upem,
                            (r.x_max - r.x_min) as f64 / upem,
                            (r.y_max - r.y_min) as f64 / upem,
                        ]
                    })
                    .unwrap_or([0.0; 4])
            })
            .unwrap_or([0.0; 4]);
        self.ink.insert((k, c), v);
        v
    }

    /// adv(prev + c) − adv(prev) − adv(c): GPOS kerning and ligature effects (spec §A.3).
    pub fn pair_adv(&mut self, fs: &FontSystem, k: FaceKey, prev: char, c: char) -> f64 {
        if let Some(v) = self.pairs.get(&(k, prev, c)) {
            return *v;
        }
        let mut s = String::new();
        s.push(prev);
        s.push(c);
        let v = self.advance(fs, k, &s) - self.advance(fs, k, &prev.to_string()) - self.advance(fs, k, &c.to_string());
        let v = if v.is_finite() { v } else { 0.0 };
        self.pairs.insert((k, prev, c), v);
        v
    }

    /// Properties of `c` in face `k`, with `dadvs` for every `prev` requested (union across calls).
    pub fn prop(&mut self, fs: &FontSystem, k: FaceKey, c: char, prev: &[char]) -> Rc<CProp> {
        let need_more = match self.props.get(&(k, c)) {
            Some(p) => prev.iter().any(|q| !p.dadvs.contains_key(q)),
            None => true,
        };
        if !need_more {
            return self.props[&(k, c)].clone();
        }
        let mut dadvs = self.props.get(&(k, c)).map(|p| p.dadvs.clone()).unwrap_or_default();
        for &q in prev {
            let v = self.pair_adv(fs, k, q, c);
            dadvs.insert(q, v);
        }
        let p = Rc::new(CProp {
            c,
            charw: self.advance(fs, k, &c.to_string()),
            spacew: self.advance(fs, k, " "),
            caph: fs.face_info(k).cap_height,
            inkbb: self.ink(fs, k, c),
            dadvs,
        });
        self.props.insert((k, c), p.clone());
        p
    }

    /// Placeholder for a character no installed font can draw.
    pub fn unrendered(c: char) -> Rc<CProp> {
        Rc::new(CProp { c, charw: 0.0, spacew: 0.0, caph: 0.0, inkbb: [0.0; 4], dadvs: HashMap::new() })
    }
}
```

Add `pub mod metrics;` to `src/text/mod.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_metrics`
Expected: 3 passed. If `pair_adv('A','V')` is 0, check that rustybuzz applied GPOS (DejaVu Sans has both `kern` and GPOS `kern` features; `shape` with `&[]` enables `kern` by default).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/metrics.rs src/text/mod.rs tests/text_metrics.rs
git commit -m "feat(text): character metrics via rustybuzz shaping and ttf-parser outlines (CProp, pair advances, caches)"
```

---

### Task 6: Text style semantics — font size, line height, letter spacing, baseline shift, anchor

**Files:**
- Create: `src/text/style.rs`
- Modify: `src/text/mod.rs` (`pub mod style;`)
- Test: `tests/text_style.rs`

**Interfaces:**
- Consumes: `Doc::{specified_style, cascaded_style, attr, parent, composed_transform, is_element}`, `geom::{ipx, scale_factor}`, `style::default_value`.
- Produces (`sciink::text::style`):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct FontSize { pub tfs: f64, pub scf: f64, pub utfs: f64 }   // transformed size, sqrt|det| of composed transform, untransformed size
  pub fn composed_width(doc: &Doc, n: NodeId, prop: &str) -> FontSize;  // utils.py:45–87 for any length property
  pub fn composed_font_size(doc: &Doc, n: NodeId) -> FontSize;          // = composed_width(doc, n, "font-size")
  pub fn composed_line_height(doc: &Doc, n: NodeId) -> f64;             // utils.py:90–114, user units (transformed)
  pub fn letter_spacing(doc: &Doc, style_node: NodeId, style: &Style) -> f64; // P:3928–3940, untransformed uu
  pub fn baseline_shift(doc: &Doc, style_node: NodeId, style: &Style) -> f64; // P:3962–4019, untransformed uu
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Anchor { Start, Middle, End }
  impl Anchor { pub fn parse(s: &str) -> Option<Anchor>; pub fn anfr(self) -> f64; /* 0, 0.5, 1 */ pub fn css(self) -> &'static str; }
  ```
  Semantics (spec §A.1 stage 1e and utils.py): `composed_width`: `satt = specified(prop)` or the default (`font-size` → `medium`, `stroke-width` → `1`, others via `default_value`); if `satt` ends in `%` or `em`: walk `cel = n` upward while `cascaded_style(cel).get(prop) != satt && attr(cel, prop) != satt` (`cel` may become the root; if the walk runs out of elements use the root `<svg>`); `par = parent(cel)` (or the root `<svg>` when `cel` has no element parent); factor = `%`/100 or the `em` number; `(tsz, scf, utsz) = composed_width(par)`; return `(tsz·f, scf, utsz·f)`. Otherwise `utsz = ipx(satt)`, falling back to small/medium/large = 10/12/14 px, any other keyword → 12; `scf = scale_factor(composed_transform(n))`; `tfs = utsz·scf`. `composed_line_height`: `satt = specified("line-height")` or `normal`; `normal` → 1.25; `%` → /100; else parse as a bare/`em` number; if that fails `ipx(satt) / utfs`; result × `tfs`. `letter_spacing`: value in `style` (the char's specified style); `em` → number × `utfs(style_node)`; else `ipx` or 0; absent → 0. `baseline_shift`: if `style` has no `baseline-shift` → 0; else collect `cel = style_node` and ancestors while `specified_style(cel)` has `baseline-shift`; from the outermost: element whose `cascaded_style` specifies it → `local_baseline` (super → 40 %, sub → −20 %, `%` → × parent's `utfs`, else `ipx` or 0); element that only inherits → adds the running sum again; return the sum.

- [ ] **Step 1: Write the failing tests**

Create `tests/text_style.rs`:

```rust
mod support;

use sciink::dom::{Doc, NodeId};
use sciink::text::style::{
    Anchor, baseline_shift, composed_font_size, composed_line_height, composed_width, letter_spacing,
};

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

#[test]
fn font_size_defaults_keywords_units_and_transforms() {
    let d = doc(&format!(r#"<svg {NS}>
      <g transform="scale(2)">
        <text id="a">x</text>
        <text id="b" style="font-size:medium">x</text>
        <text id="c" style="font-size:large">x</text>
        <text id="d" style="font-size:12pt">x</text>
        <text id="e" font-size="9" transform="scale(3,3)">x</text>
        <text id="f" style="font-size:xx-large">x</text>
      </g></svg>"#));
    let fs = composed_font_size(&d, id(&d, "a"));
    assert_eq!((fs.utfs, fs.scf, fs.tfs), (12.0, 2.0, 24.0));
    assert_eq!(composed_font_size(&d, id(&d, "b")).utfs, 12.0);
    assert_eq!(composed_font_size(&d, id(&d, "c")).utfs, 14.0);
    assert!((composed_font_size(&d, id(&d, "d")).utfs - 16.0).abs() < 1e-9);
    let e = composed_font_size(&d, id(&d, "e"));
    assert_eq!((e.utfs, e.scf, e.tfs), (9.0, 6.0, 54.0));
    assert_eq!(composed_font_size(&d, id(&d, "f")).utfs, 12.0, "unknown keyword → 12");
}

#[test]
fn relative_font_sizes_resolve_against_the_ancestor_that_set_them() {
    let d = doc(&format!(r#"<svg {NS}>
      <text id="t" style="font-size:10px"><tspan id="s" style="font-size:65%"><tspan id="u">x</tspan><tspan id="v" style="font-size:2em">y</tspan></tspan></text>
      <text id="w" style="font-size:150%">z</text></svg>"#));
    let s = composed_font_size(&d, id(&d, "s"));
    assert!((s.utfs - 6.5).abs() < 1e-9 && s.scf == 1.0);
    // u only inherits the 65% string → resolved where it was set (s) → 6.5
    assert!((composed_font_size(&d, id(&d, "u")).utfs - 6.5).abs() < 1e-9);
    // v: 2em of its parent's (s) size
    assert!((composed_font_size(&d, id(&d, "v")).utfs - 13.0).abs() < 1e-9);
    // % on a root-level text: relative to the root's default 12px
    assert!((composed_font_size(&d, id(&d, "w")).utfs - 18.0).abs() < 1e-9);
    // composed_width works for stroke-width too
    let d2 = doc(&format!(r#"<svg {NS}><g transform="scale(2)"><path id="p" style="stroke-width:3"/><path id="q"/></g></svg>"#));
    let p = composed_width(&d2, id(&d2, "p"), "stroke-width");
    assert_eq!((p.utfs, p.tfs), (3.0, 6.0));
    assert_eq!(composed_width(&d2, id(&d2, "q"), "stroke-width").utfs, 1.0);
}

#[test]
fn line_height_variants() {
    let d = doc(&format!(r#"<svg {NS}><g transform="scale(2)">
      <text id="a" style="font-size:10px">x</text>
      <text id="b" style="font-size:10px;line-height:1.5">x</text>
      <text id="c" style="font-size:10px;line-height:125%">x</text>
      <text id="d" style="font-size:10px;line-height:1.1em">x</text>
      <text id="e" style="font-size:10px;line-height:15px">x</text></g></svg>"#));
    assert!((composed_line_height(&d, id(&d, "a")) - 25.0).abs() < 1e-9, "normal = 1.25 × tfs(20)");
    assert!((composed_line_height(&d, id(&d, "b")) - 30.0).abs() < 1e-9);
    assert!((composed_line_height(&d, id(&d, "c")) - 25.0).abs() < 1e-9);
    assert!((composed_line_height(&d, id(&d, "d")) - 22.0).abs() < 1e-9);
    assert!((composed_line_height(&d, id(&d, "e")) - 30.0).abs() < 1e-9, "15px / utfs 10 × tfs 20");
}

#[test]
fn letter_spacing_and_baseline_shift() {
    let d = doc(&format!(r#"<svg {NS}>
      <text id="t" style="font-size:10px;letter-spacing:0.1em"><tspan id="a">x</tspan><tspan id="b" style="letter-spacing:2px">y</tspan><tspan id="c" style="letter-spacing:normal">z</tspan></text>
      <text id="u" style="font-size:10px">as<tspan id="sup" style="font-size:65%;baseline-shift:super">f<tspan id="inh">g</tspan><tspan id="sub" style="baseline-shift:sub">h</tspan></tspan><tspan id="pct" style="baseline-shift:-30%">i</tspan><tspan id="len" style="baseline-shift:3px">j</tspan></text></svg>"#));
    let st = |i: &str| d.specified_style(id(&d, i));
    assert!((letter_spacing(&d, id(&d, "a"), &st("a")) - 1.0).abs() < 1e-9, "0.1em × 10px");
    assert!((letter_spacing(&d, id(&d, "b"), &st("b")) - 2.0).abs() < 1e-9);
    assert_eq!(letter_spacing(&d, id(&d, "c"), &st("c")), 0.0);
    assert_eq!(baseline_shift(&d, id(&d, "u"), &st("u")), 0.0);
    // super: +40% of the PARENT's untransformed font size (10px) = 4
    assert!((baseline_shift(&d, id(&d, "sup"), &st("sup")) - 4.0).abs() < 1e-9);
    // inherited only: the running sum is added again (Inkscape's compounding) → 8
    assert!((baseline_shift(&d, id(&d, "inh"), &st("inh")) - 8.0).abs() < 1e-9);
    // sub inside super: 4 + (−20% of parent size 6.5 = −1.3) = 2.7
    assert!((baseline_shift(&d, id(&d, "sub"), &st("sub")) - 2.7).abs() < 1e-9);
    assert!((baseline_shift(&d, id(&d, "pct"), &st("pct")) + 3.0).abs() < 1e-9);
    assert!((baseline_shift(&d, id(&d, "len"), &st("len")) - 3.0).abs() < 1e-9);
    assert_eq!(Anchor::parse("middle"), Some(Anchor::Middle));
    assert_eq!(Anchor::parse("weird"), None);
    assert_eq!((Anchor::Start.anfr(), Anchor::Middle.anfr(), Anchor::End.anfr()), (0.0, 0.5, 1.0));
    assert_eq!(Anchor::End.css(), "end");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_style 2>&1 | grep -E '^error' | head -3`
Expected: `could not find style in text`.

- [ ] **Step 3: Implement `src/text/style.rs`**

```rust
//! Font-size / line-height / letter-spacing / baseline-shift semantics as Inkscape computes
//! them (upstream utils.py:45–114, parser.py:3928–4019; spec §A.1 stage 1e).

use crate::dom::{Doc, NodeId};
use crate::geom::{ipx, scale_factor};
use crate::style::{Style, default_value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontSize {
    pub tfs: f64,
    pub scf: f64,
    pub utfs: f64,
}

fn keyword_px(prop: &str, v: &str) -> f64 {
    if prop == "font-size" {
        match v {
            "small" => 10.0,
            "large" => 14.0,
            _ => 12.0, // medium and every other keyword
        }
    } else {
        default_value(prop).and_then(ipx).unwrap_or(1.0)
    }
}

/// Transformed size, composed scale factor and untransformed size of a length property.
pub fn composed_width(doc: &Doc, n: NodeId, prop: &str) -> FontSize {
    let dflt = if prop == "font-size" { "medium" } else { default_value(prop).unwrap_or("0") };
    let satt = doc.specified(n, prop).unwrap_or_else(|| dflt.to_string());
    let satt = satt.trim().to_string();
    let rel = satt.strip_suffix('%').map(|s| (s, 0.01)).or_else(|| satt.strip_suffix("em").map(|s| (s, 1.0)));
    if let Some((num, mul)) = rel {
        if let Ok(f) = num.trim().parse::<f64>() {
            // find the element that set this exact string, then resolve against its parent
            let mut cel = Some(n);
            while let Some(c) = cel {
                let own = doc.cascaded_style(c).get(prop).map(|v| v.trim() == satt).unwrap_or(false)
                    || doc.attr(c, prop).map(|v| v.trim() == satt).unwrap_or(false);
                if own {
                    break;
                }
                cel = doc.parent(c).filter(|&p| doc.is_element(p));
            }
            let par = cel
                .and_then(|c| doc.parent(c))
                .filter(|&p| doc.is_element(p))
                .unwrap_or_else(|| doc.svg());
            let base = composed_width(doc, par, prop);
            let f = f * mul;
            return FontSize { tfs: base.tfs * f, scf: base.scf, utfs: base.utfs * f };
        }
    }
    let utfs = ipx(&satt).unwrap_or_else(|| keyword_px(prop, &satt));
    let scf = scale_factor(doc.composed_transform(n));
    FontSize { tfs: utfs * scf, scf, utfs }
}

pub fn composed_font_size(doc: &Doc, n: NodeId) -> FontSize {
    composed_width(doc, n, "font-size")
}

/// Absolute line height in (transformed) user units.
pub fn composed_line_height(doc: &Doc, n: NodeId) -> f64 {
    let satt = doc.specified(n, "line-height").unwrap_or_else(|| "normal".to_string());
    let satt = satt.trim();
    let fs = composed_font_size(doc, n);
    let factor = if satt == "normal" {
        1.25
    } else if let Some(p) = satt.strip_suffix('%') {
        p.trim().parse::<f64>().map(|v| v / 100.0).unwrap_or(1.25)
    } else if let Ok(v) = satt.trim_end_matches("em").trim().parse::<f64>() {
        v
    } else {
        match (ipx(satt), fs.utfs) {
            (Some(px), u) if u > 0.0 => px / u,
            _ => 1.25,
        }
    };
    factor * fs.tfs
}

/// Letter spacing of a character whose specified style is `style`, in untransformed user units.
pub fn letter_spacing(doc: &Doc, style_node: NodeId, style: &Style) -> f64 {
    match style.get("letter-spacing").map(str::trim) {
        None | Some("normal") => 0.0,
        Some(v) => match v.strip_suffix("em") {
            Some(num) => num.trim().parse::<f64>().unwrap_or(0.0) * composed_font_size(doc, style_node).utfs,
            None => ipx(v).unwrap_or(0.0),
        },
    }
}

fn local_baseline(doc: &Doc, el: NodeId) -> f64 {
    let own = doc.cascaded_style(el);
    let v = own.get("baseline-shift").unwrap_or("0").trim();
    let v = match v {
        "super" => "40%",
        "sub" => "-20%",
        other => other,
    };
    if let Some(p) = v.strip_suffix('%') {
        let par = doc.parent(el).filter(|&p| doc.is_element(p)).unwrap_or_else(|| doc.svg());
        let f = composed_font_size(doc, par);
        (f.tfs / f.scf) * p.trim().parse::<f64>().unwrap_or(0.0) / 100.0
    } else {
        ipx(v).unwrap_or(0.0)
    }
}

/// Baseline shift of a character in untransformed user units (Inkscape's compounding kept).
pub fn baseline_shift(doc: &Doc, style_node: NodeId, style: &Style) -> f64 {
    if style.get("baseline-shift").is_none() {
        return 0.0;
    }
    let mut chain: Vec<NodeId> = Vec::new();
    let mut cel = Some(style_node);
    while let Some(c) = cel {
        if !doc.is_element(c) || doc.specified_style(c).get("baseline-shift").is_none() {
            break;
        }
        chain.push(c);
        cel = doc.parent(c);
    }
    let mut rel: Vec<f64> = Vec::new();
    for &el in chain.iter().rev() {
        if doc.cascaded_style(el).get("baseline-shift").is_some() {
            rel.push(local_baseline(doc, el));
        } else {
            let s: f64 = rel.iter().sum();
            rel.push(s);
        }
    }
    rel.iter().sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

impl Anchor {
    pub fn parse(s: &str) -> Option<Anchor> {
        match s.trim() {
            "start" => Some(Anchor::Start),
            "middle" => Some(Anchor::Middle),
            "end" => Some(Anchor::End),
            _ => None,
        }
    }
    pub fn anfr(self) -> f64 {
        match self {
            Anchor::Start => 0.0,
            Anchor::Middle => 0.5,
            Anchor::End => 1.0,
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            Anchor::Start => "start",
            Anchor::Middle => "middle",
            Anchor::End => "end",
        }
    }
}
```

Add `pub mod style;` to `src/text/mod.rs`. Check `crate::style::default_value("line-height")` and `("stroke-width")` exist in `DEFAULTS`; if `default_value("stroke-width")` is `None`, the `dflt` fallback `"0"` would be wrong — add `("stroke-width", "1")` to `DEFAULTS` in `src/style.rs` (spec §C.2 initial values) and cover it with the `q` assertion above.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_style`
Expected: 4 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/style.rs src/text/mod.rs src/style.rs tests/text_style.rs
git commit -m "feat(text): composed font size / line height, letter spacing, baseline shift and anchors with Inkscape semantics"
```

---

### Task 7: Text-run tree — lxml `text`/`tail` model over the arena DOM

**Files:**
- Create: `src/text/tree.rs`
- Modify: `src/text/mod.rs` (`pub mod tree;`)
- Test: `tests/text_tree.rs`

**Interfaces:**
- Consumes: `Doc::{children, first_child, next_sibling, is_element, is_comment, text, set_text, new_text, insert_after, prepend_child, detach, parent}`.
- Produces (`sciink::text::tree`):
  ```rust
  /// One text block: an element's leading text (`Text`) or the text following it (`Tail`).
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Run {
      pub ddi: usize,        // index of `node` in `TextTree::dds` (pre-order descendants incl. the root element)
      pub node: NodeId,      // the element/comment this block belongs to
      pub is_tail: bool,
      pub style_node: NodeId, // element whose style applies: `node` for text, its parent for tails
  }
  pub struct TextTree { pub dds: Vec<NodeId>, pub parent: Vec<Option<usize>> /* index of the parent in dds */ }
  impl TextTree {
      pub fn new(doc: &Doc, el: NodeId) -> TextTree;   // pre-order over element AND comment descendants (comments carry tails)
      pub fn runs(&self, doc: &Doc) -> Vec<Run>;      // P:2453–2516 order: Text(node) on entry, Tail(child) after each child; root's tail excluded; comment *text* excluded
      pub fn is_top_level(&self, ddi: usize) -> bool; // direct child of the root element
  }
  pub fn run_text(doc: &Doc, r: &Run) -> Option<String>;      // lxml .text / .tail: the Text/CData node, None when absent
  pub fn set_run_text(doc: &mut Doc, r: &Run, s: Option<&str>); // create / replace / remove that Text node
  ```
  lxml model: an element's `.text` is its first child when that child is a Text/CData node; a node's `.tail` is the Text/CData node immediately following it. Consecutive text nodes never occur after parsing (the DOM merges them).

- [ ] **Step 1: Write the failing tests**

Create `tests/text_tree.rs`:

```rust
mod support;

use sciink::dom::{Doc, NodeId};
use sciink::text::tree::{Run, TextTree, run_text, set_run_text};

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn describe(d: &Doc, runs: &[Run]) -> Vec<String> {
    runs.iter()
        .map(|r| {
            let who = d.attr(r.node, "id").map(str::to_string).unwrap_or_else(|| if d.is_comment(r.node) { "<!-->".into() } else { d.tag(r.node).to_string() });
            format!("{}{}={:?}", if r.is_tail { "tail:" } else { "text:" }, who, run_text(d, r))
        })
        .collect()
}

#[test]
fn runs_follow_lxml_text_tail_order() {
    let d = doc(&format!(r#"<svg {NS}><text id="t">A<tspan id="s">B<tspan id="u">C</tspan>D</tspan>E<!-- c -->F<tspan id="v"/>G</text><text id="w"><tspan id="x">H</tspan></text></svg>"#));
    let t = TextTree::new(&d, id(&d, "t"));
    assert_eq!(t.dds.len(), 5, "t, s, u, comment, v");
    assert!(t.is_top_level(1) && !t.is_top_level(2) && t.is_top_level(3));
    let runs = t.runs(&d);
    assert_eq!(
        describe(&d, &runs),
        [
            "text:t=Some(\"A\")",
            "text:s=Some(\"B\")",
            "text:u=Some(\"C\")",
            "tail:u=Some(\"D\")",
            "tail:s=Some(\"E\")",
            "tail:<!-->=Some(\"F\")",
            "text:v=None",
            "tail:v=Some(\"G\")",
        ]
    );
    // style sources: text → the node, tail → its parent
    assert_eq!(runs[3].style_node, id(&d, "s"));
    assert_eq!(runs[5].style_node, id(&d, "t"));
    assert_eq!(runs[0].ddi, 0);
    assert_eq!(runs[2].ddi, 2);
    let w = TextTree::new(&d, id(&d, "w"));
    assert_eq!(describe(&d, &w.runs(&d)), ["text:w=None", "text:x=Some(\"H\")", "tail:x=None"]);
}

#[test]
fn set_run_text_creates_replaces_and_removes() {
    let mut d = doc(&format!(r#"<svg {NS}><text id="t"><tspan id="s">B</tspan>E</text></svg>"#));
    let t = TextTree::new(&d, id(&d, "t"));
    let runs = t.runs(&d);
    assert_eq!(describe(&d, &runs), ["text:t=None", "text:s=Some(\"B\")", "tail:s=Some(\"E\")"]);
    set_run_text(&mut d, &runs[0], Some("A"));      // create leading text
    set_run_text(&mut d, &runs[1], Some("bb"));     // replace
    set_run_text(&mut d, &runs[2], None);           // remove the tail
    let runs = t.runs(&d);
    assert_eq!(describe(&d, &runs), ["text:t=Some(\"A\")", "text:s=Some(\"bb\")", "tail:s=None"]);
    let mut out = Vec::new();
    d.write(&mut out);
    assert!(String::from_utf8(out).unwrap().contains(r#"<text id="t">A<tspan id="s">bb</tspan></text>"#));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_tree 2>&1 | grep -E '^error' | head -3`
Expected: `could not find tree in text`.

- [ ] **Step 3: Implement `src/text/tree.rs`**

```rust
//! lxml-style view of a text element: every element/comment descendant has a `.text`
//! (leading Text child) and a `.tail` (following Text sibling). Upstream's TextTree (P:2453–2516).

use crate::dom::{Doc, NodeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub ddi: usize,
    pub node: NodeId,
    pub is_tail: bool,
    pub style_node: NodeId,
}

pub struct TextTree {
    pub dds: Vec<NodeId>,
    pub parent: Vec<Option<usize>>,
}

impl TextTree {
    /// Pre-order descendants (elements and comments) starting with `el` itself; iterative.
    pub fn new(doc: &Doc, el: NodeId) -> TextTree {
        let mut dds = vec![el];
        let mut parent = vec![None];
        let mut stack: Vec<(NodeId, usize)> = doc
            .children(el)
            .filter(|&c| doc.is_element(c) || doc.is_comment(c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|c| (c, 0usize))
            .collect();
        while let Some((n, p)) = stack.pop() {
            dds.push(n);
            parent.push(Some(p));
            let me = dds.len() - 1;
            if doc.is_element(n) {
                let kids: Vec<NodeId> = doc.children(n).filter(|&c| doc.is_element(c) || doc.is_comment(c)).collect();
                for c in kids.into_iter().rev() {
                    stack.push((c, me));
                }
            }
        }
        TextTree { dds, parent }
    }

    pub fn is_top_level(&self, ddi: usize) -> bool {
        self.parent.get(ddi).copied().flatten() == Some(0)
    }

    /// Text blocks in document order: `Text(node)` when entering a node, `Tail(node)` after it.
    /// The root's tail is not part of the element; comment text is skipped (comment tails are kept).
    pub fn runs(&self, doc: &Doc) -> Vec<Run> {
        // ddi order is pre-order, so the tail of dds[i] comes after all of its descendants:
        // emit Text(i) at i, and Tail(i) right after the last descendant of i.
        let n = self.dds.len();
        let mut last_desc = vec![0usize; n];
        for i in 0..n {
            last_desc[i] = i;
            let mut p = self.parent[i];
            while let Some(pi) = p {
                last_desc[pi] = i;
                p = self.parent[pi];
            }
        }
        let mut closing: Vec<Vec<usize>> = vec![Vec::new(); n];
        for i in 1..n {
            closing[last_desc[i]].push(i); // tails to emit after node last_desc[i]'s text
        }
        let mut out = Vec::new();
        for i in 0..n {
            let node = self.dds[i];
            if !doc.is_comment(node) {
                out.push(Run { ddi: i, node, is_tail: false, style_node: node });
            }
            // innermost first: deeper nodes end before their ancestors
            let mut c = closing[i].clone();
            c.sort_by(|a, b| b.cmp(a));
            for j in c {
                let pnode = self.dds[self.parent[j].expect("non-root")];
                out.push(Run { ddi: j, node: self.dds[j], is_tail: true, style_node: pnode });
            }
        }
        out
    }
}

fn text_node(doc: &Doc, r: &Run) -> Option<NodeId> {
    if r.is_tail {
        doc.next_sibling(r.node).filter(|&s| doc.text(s).is_some())
    } else {
        doc.first_child(r.node).filter(|&s| doc.text(s).is_some())
    }
}

/// lxml `.text` / `.tail` of the run's node.
pub fn run_text(doc: &Doc, r: &Run) -> Option<String> {
    text_node(doc, r).and_then(|t| doc.text(t).map(str::to_string))
}

/// Create, replace or remove the run's text node.
pub fn set_run_text(doc: &mut Doc, r: &Run, s: Option<&str>) {
    match (text_node(doc, r), s) {
        (Some(t), Some(s)) => doc.set_text(t, s),
        (Some(t), None) => doc.detach(t),
        (None, Some(s)) => {
            let t = doc.new_text(s);
            if r.is_tail {
                doc.insert_after(t, r.node);
            } else {
                doc.prepend_child(r.node, t);
            }
        }
        (None, None) => {}
    }
}
```

Add `pub mod tree;` to `src/text/mod.rs`. Check `Doc::insert_after(n, anchor)` argument order in `src/dom.rs:760` (the plan assumes `(node, anchor)`); check `prepend_child(parent, n)`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_tree`
Expected: 2 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/tree.rs src/text/mod.rs tests/text_tree.rs
git commit -m "feat(text): TextTree — lxml-style text/tail runs over the arena DOM"
```

---

### Task 8: Depathologize — position overflows, whitespace collapsing, comment tails

**Files:**
- Create: `src/text/whitespace.rs`
- Modify: `src/text/mod.rs` (`pub mod whitespace;`)
- Test: `tests/text_tree.rs` (append)

**Interfaces:**
- Consumes: `TextTree`, `run_text`, `set_run_text` (Task 7); `Doc::{xml_space_preserve, attr, set_attr, remove_attr, is_comment, detach}`; `Warnings` (Task 3).
- Produces (`sciink::text::whitespace`):
  ```rust
  pub fn get_xy(doc: &Doc, n: NodeId, attr: &str) -> Vec<Option<f64>>;  // P:821–827: absent/empty → [None]; "none" → None; unit → px
  pub fn depathologize(doc: &mut Doc, el: NodeId, is_flow: bool, warn: &mut Warnings);
  ```
  `depathologize` runs, in order: (1) `remove_position_overflows` — v1 rule (spec §A.1 stage 1a): for every element in the tree whose `x`/`y`/`dx`/`dy` list is longer than one and longer than its own `.text` (or it has no text), truncate the list to the text length (drop it entirely when the text is empty/absent) and push the warning `"<id or tag>: <attr> has more values than characters; extra values dropped"`; (2) `cleanup_whitespace` (P:4853–4882) when `!xml_space_preserve(el)`: for each run whose text is longer than 1 char: `Text` → collapse `[ \t\r\n\f\v]+` to one space, strip both ends, and re-append one space if the node has element/comment children and the original ended with whitespace; `Tail` → collapse, strip both ends, and prepend one space if the original started with whitespace; then for non-flows, for every run (text and tail, any xml:space): `cleanup_returns(s, last_span)` where `last_span` is `node has no children` for text and `node is its parent's last child (elements/comments)` for tails — `cleanup_returns` = `first_newline_to_space(s + "\n")` minus the final char when `last_span`, else `first_newline_to_space(s)`; `first_newline_to_space` replaces, inside each maximal run of `[ \t\n\r\f\v\u{A0}]`, the first `\n`/`\r` with a space and drops further newlines, keeping other whitespace; (3) `condense_comments` (P:4914–4930): a comment's tail moves onto the previous sibling's tail (concatenated) or, with no previous element/comment sibling, onto the parent's text (concatenated); the comment's tail is removed.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_tree.rs`:

```rust
use sciink::text::Warnings;
use sciink::text::whitespace::{depathologize, get_xy};

fn text_of(d: &Doc, i: &str) -> String {
    d.text_content(id(d, i))
}

#[test]
fn get_xy_parses_lists_units_and_none() {
    let d = doc(&format!(r#"<svg {NS}><text id="t" x="1 2.5 none 1in" y="" dx=" 3 "/></svg>"#));
    let t = id(&d, "t");
    assert_eq!(get_xy(&d, t, "x"), [Some(1.0), Some(2.5), None, Some(96.0)]);
    assert_eq!(get_xy(&d, t, "y"), [None]);
    assert_eq!(get_xy(&d, t, "dy"), [None]);
    assert_eq!(get_xy(&d, t, "dx"), [Some(3.0)]);
}

#[test]
fn whitespace_is_collapsed_unless_preserved() {
    let mut d = doc(&format!("<svg {NS}><text id=\"t\">  Hello \n  <tspan id=\"s\">big\tworld  </tspan>\n   again </text><text id=\"p\" xml:space=\"preserve\">  a  \n b</text></svg>"));
    let mut w = Warnings::default();
    depathologize(&mut d, id(&d, "t"), false, &mut w);
    depathologize(&mut d, id(&d, "p"), false, &mut w);
    // text with element children keeps one trailing space; tail keeps one leading space
    assert_eq!(text_of(&d, "t"), "Hello big world again");
    assert_eq!(text_of(&d, "s"), "big world");
    // preserved: whitespace kept, but the first newline of a run becomes a space and the rest vanish
    assert_eq!(text_of(&d, "p"), "  a    b");
    assert!(w.0.is_empty());
}

#[test]
fn preserved_newlines_and_last_span_rule() {
    // "a\n\nb" in the parent (has children): first newline → space, second dropped → "a b". A
    // trailing newline in a leaf span's text ("c\n") and in a last-child tail ("d\n") is DROPPED,
    // not converted (upstream cleanup_returns, last_span rule).
    let mut d = doc(&format!("<svg {NS}><text id=\"t\" xml:space=\"preserve\">a\n\nb<tspan id=\"s\">c\n</tspan>d\n</text></svg>"));
    let mut w = Warnings::default();
    depathologize(&mut d, id(&d, "t"), false, &mut w);
    assert_eq!(text_of(&d, "t"), "a bcd");
    assert_eq!(text_of(&d, "s"), "c");
}

#[test]
fn comment_tails_are_condensed_and_overflows_truncated() {
    let mut d = doc(&format!(r#"<svg {NS}><text id="t" x="1 2 3 4 5" dx="1 2 3">ab<!-- note -->cd<tspan id="s" x="7 8"/></text></svg>"#));
    let mut w = Warnings::default();
    depathologize(&mut d, id(&d, "t"), false, &mut w);
    let t = id(&d, "t");
    assert_eq!(d.attr(t, "x"), Some("1 2"), "5 values for 2 chars → truncated");
    assert_eq!(d.attr(t, "dx"), Some("1 2"));
    assert_eq!(d.attr(id(&d, "s"), "x"), None, "positions on an empty tspan are dropped");
    assert_eq!(w.0.len(), 3, "{:?}", w.0);
    assert!(w.0[0].contains("t: x has more values than characters"));
    // the comment's tail "cd" moved onto the parent's text
    assert_eq!(d.text_content(t), "abcd");
    let runs = TextTree::new(&d, t).runs(&d);
    assert_eq!(describe(&d, &runs), ["text:t=Some(\"abcd\")", "tail:<!-->=None", "text:s=None", "tail:s=None"]);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_tree 2>&1 | grep -E '^error' | head -3`
Expected: `could not find whitespace in text`.

- [ ] **Step 3: Implement `src/text/whitespace.rs`**

```rust
//! Depathologize text before parsing (spec §A.1 stage 1a; upstream parser.py:4759–4930).

use crate::dom::{Doc, NodeId};
use crate::geom::ipx;

use super::Warnings;
use super::tree::{Run, TextTree, run_text, set_run_text};

const WS: &[char] = &[' ', '\t', '\r', '\n', '\x0c', '\x0b'];

/// `x`/`y`/`dx`/`dy` lists: `[None]` when absent or empty; `none` entries are `None`.
pub fn get_xy(doc: &Doc, n: NodeId, attr: &str) -> Vec<Option<f64>> {
    match doc.attr(n, attr).map(str::trim) {
        None | Some("") => vec![None],
        Some(v) => v
            .split_whitespace()
            .map(|s| if s == "none" { None } else { ipx(s) })
            .collect(),
    }
}

fn label(doc: &Doc, n: NodeId) -> String {
    doc.attr(n, "id").map(str::to_string).unwrap_or_else(|| doc.tag(n).to_string())
}

fn remove_position_overflows(doc: &mut Doc, tree: &TextTree, runs: &[Run], warn: &mut Warnings) {
    for r in runs.iter().filter(|r| !r.is_tail) {
        let n = r.node;
        if !doc.is_element(n) {
            continue;
        }
        let len = run_text(doc, r).map(|t| t.chars().count()).unwrap_or(0);
        for attr in ["x", "y", "dx", "dy"] {
            let vals = get_xy(doc, n, attr);
            let present = doc.attr(n, attr).is_some();
            if !present || vals.len() <= 1 || vals.len() <= len {
                continue;
            }
            warn.push(format!("{}: {attr} has more values than characters; extra values dropped", label(doc, n)));
            if len == 0 {
                doc.remove_attr(n, attr);
            } else {
                let kept: Vec<String> = doc
                    .attr(n, attr)
                    .unwrap_or("")
                    .split_whitespace()
                    .take(len)
                    .map(str::to_string)
                    .collect();
                doc.set_attr(n, attr, kept.join(" "));
            }
        }
    }
    let _ = tree;
}

fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if WS.contains(&c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// In each maximal whitespace run the first newline becomes a space, later newlines vanish.
fn first_newline_to_space(s: &str) -> String {
    let is_ws = |c: char| WS.contains(&c) || c == '\u{A0}';
    let mut out = String::with_capacity(s.len());
    let mut chunk = String::new();
    let flush = |chunk: &mut String, out: &mut String| {
        if chunk.contains(['\n', '\r']) {
            let mut done = false;
            for ch in chunk.chars() {
                if ch == '\n' || ch == '\r' {
                    if !done {
                        out.push(' ');
                        done = true;
                    }
                } else {
                    out.push(ch);
                }
            }
        } else {
            out.push_str(chunk);
        }
        chunk.clear();
    };
    for c in s.chars() {
        if is_ws(c) {
            chunk.push(c);
        } else {
            flush(&mut chunk, &mut out);
            out.push(c);
        }
    }
    flush(&mut chunk, &mut out);
    out
}

fn cleanup_returns(s: &str, last_span: bool) -> String {
    if last_span {
        let mut t = first_newline_to_space(&format!("{s}\n"));
        t.pop();
        t
    } else {
        first_newline_to_space(s)
    }
}

fn has_node_children(doc: &Doc, n: NodeId) -> bool {
    doc.children(n).any(|c| doc.is_element(c) || doc.is_comment(c))
}

fn is_last_child(doc: &Doc, n: NodeId) -> bool {
    match doc.parent(n) {
        Some(p) => doc.children(p).filter(|&c| doc.is_element(c) || doc.is_comment(c)).last() == Some(n),
        None => false,
    }
}

fn cleanup_whitespace(doc: &mut Doc, el: NodeId, runs: &[Run], is_flow: bool) {
    if !doc.xml_space_preserve(el) {
        for r in runs {
            let Some(txt) = run_text(doc, r) else { continue };
            if txt.chars().count() <= 1 {
                continue;
            }
            let collapsed = collapse(&txt);
            let new = if !r.is_tail {
                let mut s = collapsed.trim_matches(WS).to_string();
                if has_node_children(doc, r.node) && txt.chars().last().is_some_and(|c| WS.contains(&c)) {
                    s.push(' ');
                }
                s
            } else {
                let core = collapsed.trim_matches(WS).to_string();
                if txt.chars().next().is_some_and(|c| WS.contains(&c)) { format!(" {core}") } else { core }
            };
            set_run_text(doc, r, Some(&new));
        }
    }
    if !is_flow {
        for r in runs {
            let Some(txt) = run_text(doc, r) else { continue };
            let last_span = if r.is_tail { is_last_child(doc, r.node) } else { !has_node_children(doc, r.node) };
            let new = cleanup_returns(&txt, last_span);
            if new != txt {
                set_run_text(doc, r, Some(&new));
            }
        }
    }
}

fn condense_comments(doc: &mut Doc, tree: &TextTree) {
    for (i, &n) in tree.dds.iter().enumerate().skip(1) {
        if !doc.is_comment(n) {
            continue;
        }
        let tail = Run { ddi: i, node: n, is_tail: true, style_node: n };
        let Some(t) = run_text(doc, &tail) else { continue };
        let prev = {
            let mut p = doc.prev_sibling(n);
            while let Some(q) = p {
                if doc.is_element(q) || doc.is_comment(q) {
                    break;
                }
                p = doc.prev_sibling(q);
            }
            p
        };
        let target = match prev {
            Some(p) => Run { ddi: 0, node: p, is_tail: true, style_node: p },
            None => {
                let parent = doc.parent(n).expect("comment inside the text element");
                Run { ddi: 0, node: parent, is_tail: false, style_node: parent }
            }
        };
        let existing = run_text(doc, &target).unwrap_or_default();
        set_run_text(doc, &tail, None);
        set_run_text(doc, &target, Some(&format!("{existing}{t}")));
    }
}

/// Normalises a text element in place so the parser can assume sane input.
pub fn depathologize(doc: &mut Doc, el: NodeId, is_flow: bool, warn: &mut Warnings) {
    let tree = TextTree::new(doc, el);
    let runs = tree.runs(doc);
    remove_position_overflows(doc, &tree, &runs, warn);
    cleanup_whitespace(doc, el, &runs, is_flow);
    condense_comments(doc, &tree);
}
```

Add `pub mod whitespace;` to `src/text/mod.rs`. `Run.ddi = 0` in `condense_comments` targets is only a placeholder: `run_text`/`set_run_text` use `node`/`is_tail` alone.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_tree`
Expected: 6 passed. If the "comment_tails" test sees `text:t=Some("abcd")` but the comment run still shows a tail, check that `set_run_text(.., None)` detached the Text node before the parent's text was rewritten.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/whitespace.rs src/text/mod.rs tests/text_tree.rs
git commit -m "feat(text): depathologize — position overflows, xml:space whitespace rules, comment tails"
```

---

### Task 9: Character table (Stage 0)

**Files:**
- Create: `src/text/table.rs`
- Modify: `src/text/mod.rs` (`pub mod table;`)
- Test: `tests/text_parse.rs` (create; more tests are appended in Tasks 10–12)

**Interfaces:**
- Consumes: `TextTree::runs`, `run_text` (Task 7); `FontSpec::from_style`, `FontSystem::{resolve, resolve_for_char, face_info}` (Task 4); `Metrics::{prop, unrendered}` (Task 5); `Doc::specified_style`.
- Produces (`sciink::text::table`):
  ```rust
  pub struct CharTable {
      pub fonts: FontSystem,            // moved in; the table owns the font system for the tool run
      pub metrics: Metrics,
      true_style: HashMap<FontSpec, Option<FaceKey>>,                 // fsty → face of the run (`true_style`)
      char_style: HashMap<(FontSpec, char), Option<FaceKey>>,         // cstys[fsty][c]
      preceders: HashMap<(FaceKey, char), Vec<char>>,                 // pchrset[csty][c] (+ ' ')
  }
  impl CharTable {
      pub fn build(doc: &Doc, els: &[NodeId], fonts: FontSystem, warn: &mut Warnings) -> CharTable;
      pub fn true_face(&self, spec: &FontSpec) -> Option<FaceKey>;
      pub fn char_face(&mut self, spec: &FontSpec, c: char) -> Option<FaceKey>;  // memoised resolve_for_char
      pub fn prop(&mut self, face: Option<FaceKey>, c: char) -> Rc<CProp>;       // None → Metrics::unrendered(c); dadvs include the collected preceders
      pub fn spec_count(&self) -> usize;
  }
  ```
  `build` (P:4352–4383): for every element, every run with text: `spec = FontSpec::from_style(&specified_style(style_node))`; collect `chars(text) + ' '` per spec; for each spec resolve the true face and each char's face; `preceders[(face_of(c), c)] ∪= {text[j−1], ' '}` for `j ≥ 1` when both chars resolve to the same face. Warn once per family not found: `font-family "X" not installed; measured with "Y"` when the true face's family differs case-insensitively from the first requested family (skip when the request is a generic like `sans-serif`), and `no installed font has the character U+XXXX` for unrendered chars.

- [ ] **Step 1: Write the failing tests**

Create `tests/text_parse.rs`:

```rust
mod support;

use std::path::PathBuf;

use sciink::dom::{Doc, NodeId};
use sciink::style::Style;
use sciink::text::Warnings;
use sciink::text::fonts::{FontSpec, FontSystem};
use sciink::text::table::CharTable;

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}
fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

#[test]
fn char_table_collects_faces_preceders_and_warnings() {
    let c1 = '\u{23A3}';
    let c2 = '\u{10348}';
    let d = doc(&format!(r#"<svg {NS}>
      <text id="a" style="font-family:Roboto">AV a<tspan style="font-weight:bold">B</tspan></text>
      <text id="b" style="font-family:Helvetica">x{c1}{c2}</text></svg>"#));
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &[id(&d, "a"), id(&d, "b")], fonts(), &mut w);
    assert_eq!(ct.spec_count(), 3, "Roboto/400, Roboto/700, Helvetica/400");
    let rob = FontSpec::from_style(&Style::parse("font-family:Roboto"));
    let robb = FontSpec::from_style(&Style::parse("font-family:Roboto;font-weight:bold"));
    let helv = FontSpec::from_style(&Style::parse("font-family:Helvetica"));
    let rk = ct.true_face(&rob).unwrap();
    assert_eq!(ct.fonts.face_info(rk).family, "Roboto");
    assert_eq!(ct.fonts.face_info(ct.true_face(&robb).unwrap()).weight, 700);
    let hk = ct.true_face(&helv).unwrap();
    assert_eq!(ct.fonts.face_info(hk).family, "DejaVu Sans", "Helvetica absent → alias/generic → DejaVu Sans");
    // per-char: ⎣ under Helvetica is drawn by DejaVu (same as true face); U+10348 by nobody
    assert_eq!(ct.char_face(&helv, '\u{23A3}'), Some(hk));
    assert_eq!(ct.char_face(&helv, '\u{10348}'), None);
    // props: 'V' preceded by 'A' in Roboto gets that pair's dadv; the unrendered char is zero-width
    let v = ct.prop(Some(rk), 'V');
    assert!(v.dadvs.contains_key(&'A') && v.dadvs.contains_key(&' '), "{:?}", v.dadvs.keys().collect::<Vec<_>>());
    assert!(v.dadvs[&'A'] < 0.0, "Roboto kerns A–V");
    let u = ct.prop(None, '\u{10348}');
    assert_eq!(u.charw, 0.0);
    // 'a' follows a space in "AV a": space is a preceder, 'V' is not
    let a = ct.prop(Some(rk), 'a');
    assert!(a.dadvs.contains_key(&' ') && !a.dadvs.contains_key(&'V'));
    assert_eq!(w.0.len(), 2, "{:?}", w.0);
    assert!(w.0.iter().any(|m| m == "font-family \"Helvetica\" not installed; measured with \"DejaVu Sans\""));
    assert!(w.0.iter().any(|m| m == "no installed font has the character U+10348"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test text_parse 2>&1 | grep -E '^error' | head -3`
Expected: `could not find table in text`.

- [ ] **Step 3: Implement `src/text/table.rs`**

```rust
//! Stage 0: which face draws each character, and which pairs need kerning (P:4328–4400).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::dom::{Doc, NodeId};

use super::Warnings;
use super::fonts::{FaceKey, FontSpec, FontSystem};
use super::metrics::{CProp, Metrics};
use super::tree::{TextTree, run_text};

pub struct CharTable {
    pub fonts: FontSystem,
    pub metrics: Metrics,
    true_style: HashMap<FontSpec, Option<FaceKey>>,
    char_style: HashMap<(FontSpec, char), Option<FaceKey>>,
    preceders: HashMap<(FaceKey, char), Vec<char>>,
}

fn is_generic(f: &str) -> bool {
    matches!(
        f.to_ascii_lowercase().as_str(),
        "sans-serif" | "sans" | "serif" | "monospace" | "mono" | "cursive" | "fantasy" | "system-ui"
    )
}

impl CharTable {
    pub fn build(doc: &Doc, els: &[NodeId], mut fonts: FontSystem, warn: &mut Warnings) -> CharTable {
        // 1. chars per font spec, and the (text, spec) runs for pair collection
        let mut per_spec: HashMap<FontSpec, HashSet<char>> = HashMap::new();
        let mut runs_txt: Vec<(String, FontSpec)> = Vec::new();
        for &el in els {
            let tree = TextTree::new(doc, el);
            for r in tree.runs(doc) {
                let Some(txt) = run_text(doc, &r) else { continue };
                if txt.is_empty() {
                    continue;
                }
                let spec = FontSpec::from_style(&doc.specified_style(r.style_node));
                let set = per_spec.entry(spec.clone()).or_default();
                set.extend(txt.chars());
                set.insert(' ');
                runs_txt.push((txt, spec));
            }
        }
        // 2. true face per spec and face per char
        let mut true_style = HashMap::new();
        let mut char_style: HashMap<(FontSpec, char), Option<FaceKey>> = HashMap::new();
        for (spec, chars) in &per_spec {
            let tf = fonts.resolve(spec);
            true_style.insert(spec.clone(), tf);
            if let (Some(k), Some(first)) = (tf, spec.families.first()) {
                let fam = &fonts.face_info(k).family;
                if !is_generic(first) && !fam.eq_ignore_ascii_case(first) {
                    warn.push(format!("font-family \"{first}\" not installed; measured with \"{fam}\""));
                }
            }
            for &c in chars {
                let f = fonts.resolve_for_char(spec, c);
                if f.is_none() {
                    warn.push(format!("no installed font has the character U+{:04X}", c as u32));
                }
                char_style.insert((spec.clone(), c), f);
            }
        }
        // 3. preceders per (face, char): the previous char when drawn by the same face, plus ' '
        let mut preceders: HashMap<(FaceKey, char), Vec<char>> = HashMap::new();
        for (txt, spec) in &runs_txt {
            let cs: Vec<char> = txt.chars().collect();
            for j in 1..cs.len() {
                let Some(face) = char_style[&(spec.clone(), cs[j])] else { continue };
                if char_style[&(spec.clone(), cs[j - 1])] == Some(face) {
                    let v = preceders.entry((face, cs[j])).or_default();
                    for p in [cs[j - 1], ' '] {
                        if !v.contains(&p) {
                            v.push(p);
                        }
                    }
                }
            }
        }
        CharTable { fonts, metrics: Metrics::new(), true_style, char_style, preceders }
    }

    pub fn spec_count(&self) -> usize {
        self.true_style.len()
    }

    pub fn true_face(&self, spec: &FontSpec) -> Option<FaceKey> {
        self.true_style.get(spec).copied().flatten()
    }

    pub fn char_face(&mut self, spec: &FontSpec, c: char) -> Option<FaceKey> {
        if let Some(f) = self.char_style.get(&(spec.clone(), c)) {
            return *f;
        }
        let f = self.fonts.resolve_for_char(spec, c);
        self.char_style.insert((spec.clone(), c), f);
        f
    }

    pub fn prop(&mut self, face: Option<FaceKey>, c: char) -> Rc<CProp> {
        match face {
            None => Metrics::unrendered(c),
            Some(k) => {
                let prev = self.preceders.get(&(k, c)).cloned().unwrap_or_default();
                self.metrics.prop(&self.fonts, k, c, &prev)
            }
        }
    }
}
```

Add `pub mod table;` to `src/text/mod.rs`.

- [ ] **Step 4: Run the test**

Run: `cargo test --test text_parse`
Expected: 1 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/table.rs src/text/mod.rs tests/text_parse.rs
git commit -m "feat(text): character table — true faces, per-character faces, kerning pair collection, warnings"
```

---

### Task 10: Parsing, part A — positions, effective `sodipodi:role="line"`, inheritance, line starts

**Files:**
- Create: `src/text/parse.rs` (model types + `LineSpec` + `line_specs`)
- Modify: `src/text/mod.rs` (`pub mod parse;`)
- Test: `tests/text_parse.rs` (append)

**Interfaces:**
- Consumes: `TextTree`, `Run`, `run_text` (Task 7); `get_xy` (Task 8); `composed_font_size`, `composed_line_height`, `Anchor` (Task 6); `Doc::{specified_style, attr, composed_transform}`.
- Produces (`sciink::text::parse`):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum SprlType { Normal, PrecededSprl, TlvlSprl }
  /// Per-descendant position analysis (P:379–469).
  pub struct Positions {
      pub x: Vec<Vec<Option<f64>>>, pub y: Vec<Vec<Option<f64>>>,       // after inheritance (ixs/iys)
      pub dx: Vec<Vec<Option<f64>>>, pub dy: Vec<Vec<Option<f64>>>,     // raw
      pub xsrc: Vec<usize>, pub ysrc: Vec<usize>,                       // dds index the x/y came from
      pub esprl: Vec<bool>, pub types: Vec<SprlType>,
  }
  pub fn positions(doc: &Doc, tree: &TextTree) -> Positions;
  /// A line start discovered while walking the runs (the first half of P:477–585).
  #[derive(Debug, Clone, PartialEq)]
  pub struct LineSpec {
      pub x: Vec<Option<f64>>, pub y: Vec<Option<f64>>, pub xsrc: NodeId, pub ysrc: NodeId,
      pub sprl: bool, pub anchor: Anchor, pub rtl: bool, pub tlvlno: Option<usize>,
      pub continue_x: bool, pub continue_y: bool, pub style_node: NodeId,
      pub first_run: usize,          // index into `runs` of the run that opened the line
  }
  pub fn line_specs(doc: &Doc, tree: &TextTree, runs: &[Run], pos: &Positions) -> Vec<LineSpec>;
  ```
  Rules (spec §A.1 stages 1c–1d, P:379–585):
  - `esprl[i]` = node has `sodipodi:role="line"` ∧ `x` list length 1 ∧ `y` list length 1 ∧ node is a direct child of the root element. If additionally the node's own text is empty/absent and some *descendant* (index range `i+1 .. next non-descendant`) has `x[0]` or `y[0]` not None **and** non-empty text → `esprl[i] = false`.
  - `types[i]`: not esprl → `Normal`; else if the preceding tail (the tail of the previous sibling, or of the last descendant of the previous sibling — the last run before this node's text) exists → `PrecededSprl`; else if `i` is the first child and the root has own text → `PrecededSprl`; else `TlvlSprl`. (Compute "preceding tail exists" from the runs list: the run immediately before `Text(i)` is a `Tail` with `Some` text.)
  - Inheritance for `x`/`y` lists whose first entry is None: `inherits_from(i)` = the maximal index window around `i` such that every node in between has empty/absent text, is the parent/child chain (`parent[j+1] == j` going down, `parent[j] == j-1` going up) and no esprl; candidates = nodes in the window with a non-None first entry; prefer candidates at or above `i` (`j ≤ i`), then the nearest; result copies that node's list and records `src = j`. If node 0 still has None: `[Some(0.0)]`, src 0.
  - Line starts: iterate runs; `newsprl = !is_tail && types[ddi] == TlvlSprl`; a run opens a line when it has text or `newsprl`, and either no line exists yet, or it is a `Text` run with (`newsprl` or (`types[ddi] == Normal` and (`x[ddi][0]` or `y[ddi][0]` is Some after inheritance))). `edi` = the run's `ddi` for text; for a tail run, the ddi of the tail's PARENT element (`style_node`; upstream `dds.index(sel)`) — a tail is positioned by its parent, never by the empty node it follows. TlvlSprl line: first line → `x = [x[0][0]]`, `y = [y[0][0]]`, srcs of node 0; later → `x = [prev_sprl.x[0]]`, `y = [prev_sprl.y[0] + lht / scf]` where `prev_sprl` is the first line or the last TlvlSprl line (`sprl_inherits`), `lht = max(composed_line_height(node), composed_line_height(parent(node)))`, `scf = composed_font_size(node).scf`; `continue_x/y = false`. Normal line: `x = pos.x[edi]`, `y = pos.y[edi]`; if `x[0]` is None → copy the previous line's `x`/`xsrc` (or node 0's) and `continue_x = true`; same for `y`. `tlvlno` = index of `dds[ddi]` (the run's own node) among the root's direct children, else `Some(0)` when `edi == 0`, else None (upstream uses `ddi` for the child lookup and `edi` for the root check). Anchor: `text-anchor` of the run's specified style; a non-sprl line after the first inherits the previous line's anchor when `edi > 0` and the node has no `sodipodi:role="line"`; default `start`; `direction: rtl` swaps start/end and sets `rtl`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_parse.rs`:

```rust
use sciink::text::parse::{SprlType, line_specs, positions};
use sciink::text::style::Anchor;
use sciink::text::tree::TextTree;

#[test]
fn positions_effective_sprl_types_and_inheritance() {
    // dds: 0 text, 1 tspan(role line, x,y) , 2 inner tspan (no pos), 3 tspan (role line but 2 x values) , 4 tspan (no role, x only)
    let d = doc(&format!(r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
      <text id="t" style="font-size:10px"><tspan id="a" sodipodi:role="line" x="5" y="10"><tspan id="b">ab</tspan></tspan><tspan id="c" sodipodi:role="line" x="1 2" y="20">cd</tspan><tspan id="e" x="7">e</tspan></text></svg>"#));
    let tree = TextTree::new(&d, id(&d, "t"));
    let p = positions(&d, &tree);
    assert_eq!(p.esprl, [false, true, false, false, false]);
    assert_eq!(p.types, [SprlType::Normal, SprlType::TlvlSprl, SprlType::Normal, SprlType::Normal, SprlType::Normal]);
    // b cannot inherit from a: an effective sprl blocks the window (its chars will join a's line instead)
    assert_eq!(p.x[2], [None]);
    assert_eq!(p.xsrc[2], 2);
    assert_eq!(p.x[3], [Some(1.0), Some(2.0)]);
    assert_eq!(p.x[0], [Some(0.0)], "root without x gets [0]");
    // e has x but no y; nothing in its window supplies one (only node 0 gets the [0] default)
    assert_eq!(p.y[4], [None]);
    assert_eq!(p.ysrc[4], 4);
    assert_eq!(p.x[4], [Some(7.0)]);
    // an sprl whose only text sits in a positioned descendant is disabled
    let d2 = doc(&format!(r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
      <text id="t"><tspan id="a" sodipodi:role="line" x="5" y="10"><tspan id="b" x="9">ab</tspan></tspan></text></svg>"#));
    let tree2 = TextTree::new(&d2, id(&d2, "t"));
    assert_eq!(positions(&d2, &tree2).esprl, [false, false, false]);
}

#[test]
fn line_starts_for_inkscape_multiline_text() {
    // Text_tests-style element: three sodipodi lines; line 2 has no y → sprl inherits y + line height
    let d = doc(&format!(r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
      <text id="t" style="font-size:10px;line-height:1.25;text-anchor:middle" x="3" y="4" transform="scale(2)"><tspan id="a" sodipodi:role="line" x="3" y="4">one</tspan><tspan id="b" sodipodi:role="line" x="3" y="16.5">two</tspan><tspan id="c" sodipodi:role="line" x="3" y="29">three</tspan></text></svg>"#));
    let tree = TextTree::new(&d, id(&d, "t"));
    let runs = tree.runs(&d);
    let pos = positions(&d, &tree);
    let lines = line_specs(&d, &tree, &runs, &pos);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l.sprl && l.anchor == Anchor::Middle && !l.rtl));
    assert_eq!(lines[0].x, [Some(3.0)]);
    assert_eq!(lines[0].y, [Some(4.0)]);
    // sprl lines ignore their own y: y = previous sprl y + line height (1.25 × 10) in untransformed units
    assert_eq!(lines[1].y, [Some(16.5)]);
    assert_eq!(lines[2].y, [Some(29.0)]);
    assert_eq!(lines.iter().map(|l| l.tlvlno).collect::<Vec<_>>(), [Some(0), Some(1), Some(2)]);
    assert_eq!(lines[1].xsrc, id(&d, "t"), "sprl lines inherit x from the first line's source");
    // a normal (non-sprl) positioned tspan after sprl lines opens a line with continue flags
    let d2 = doc(&format!(r#"<svg {NS}><text id="t" x="1" y="2" style="direction:rtl;text-anchor:start">ab<tspan id="s" x="5">cd</tspan><tspan id="u" y="9">ef</tspan></text></svg>"#));
    let tree2 = TextTree::new(&d2, id(&d2, "t"));
    let runs2 = tree2.runs(&d2);
    let pos2 = positions(&d2, &tree2);
    let l2 = line_specs(&d2, &tree2, &runs2, &pos2);
    assert_eq!(l2.len(), 3);
    assert_eq!((l2[0].anchor, l2[0].rtl), (Anchor::End, true), "rtl swaps start/end");
    assert_eq!((l2[1].x, l2[1].continue_x, l2[1].continue_y), (vec![Some(5.0)], false, true));
    assert_eq!(l2[1].y, [Some(2.0)], "y continues from the previous line");
    assert_eq!((l2[2].y, l2[2].continue_x), (vec![Some(9.0)], true));
    assert_eq!(l2[2].x, [Some(5.0)]);
    assert_eq!(l2[0].first_run, 0);
    assert_eq!(l2[0].tlvlno, Some(0));
    assert_eq!(l2[1].tlvlno, Some(0), "s is the first direct child");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_parse 2>&1 | grep -E '^error' | head -3`
Expected: `could not find parse in text`.

- [ ] **Step 3: Implement the first half of `src/text/parse.rs`**

```rust
//! `<text>` → lines / chunks / characters (spec §A.1 stage 1, §A.2; upstream parser.py:280–650, 2696–2716).

use std::rc::Rc;

use kurbo::Affine;

use crate::dom::{Doc, NodeId};
use crate::style::Style;

use super::style::{Anchor, composed_font_size, composed_line_height};
use super::tree::{Run, TextTree, run_text};
use super::whitespace::get_xy;

pub const XY_TOL: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprlType {
    Normal,
    PrecededSprl,
    TlvlSprl,
}

pub struct Positions {
    pub x: Vec<Vec<Option<f64>>>,
    pub y: Vec<Vec<Option<f64>>>,
    pub dx: Vec<Vec<Option<f64>>>,
    pub dy: Vec<Vec<Option<f64>>>,
    pub xsrc: Vec<usize>,
    pub ysrc: Vec<usize>,
    pub esprl: Vec<bool>,
    pub types: Vec<SprlType>,
}

fn own_text(doc: &Doc, tree: &TextTree, i: usize) -> Option<String> {
    let r = Run { ddi: i, node: tree.dds[i], is_tail: false, style_node: tree.dds[i] };
    if doc.is_comment(tree.dds[i]) { None } else { run_text(doc, &r) }
}

fn subtree_end(tree: &TextTree, i: usize) -> usize {
    // first index after i that is not a descendant of i
    let mut j = i + 1;
    while j < tree.dds.len() {
        let mut p = tree.parent[j];
        let mut inside = false;
        while let Some(pi) = p {
            if pi == i {
                inside = true;
                break;
            }
            p = tree.parent[pi];
        }
        if !inside {
            break;
        }
        j += 1;
    }
    j
}

pub fn positions(doc: &Doc, tree: &TextTree) -> Positions {
    let n = tree.dds.len();
    let get = |attr: &str| -> Vec<Vec<Option<f64>>> {
        tree.dds.iter().map(|&d| if doc.is_element(d) { get_xy(doc, d, attr) } else { vec![None] }).collect()
    };
    let (xs, ys, dxs, dys) = (get("x"), get("y"), get("dx"), get("dy"));
    let texts: Vec<Option<String>> = (0..n).map(|i| own_text(doc, tree, i)).collect();
    let empty = |i: usize| texts[i].as_deref().is_none_or(str::is_empty);
    let nsprl: Vec<bool> = tree.dds.iter().map(|&d| doc.is_element(d) && doc.attr(d, "sodipodi:role") == Some("line")).collect();
    let mut esprl = vec![false; n];
    for i in 0..n {
        esprl[i] = nsprl[i] && xs[i].len() == 1 && ys[i].len() == 1 && tree.is_top_level(i);
        if esprl[i] && empty(i) {
            let stop = subtree_end(tree, i);
            for j in i + 1..stop {
                if (xs[j][0].is_some() || ys[j][0].is_some()) && !empty(j) {
                    esprl[i] = false;
                }
            }
        }
    }
    // types need "is there a tail right before this node's text": derive from the run list
    let runs = tree.runs(doc);
    let mut types = vec![SprlType::Normal; n];
    for i in 0..n {
        if !esprl[i] {
            continue;
        }
        let pos = runs.iter().position(|r| !r.is_tail && r.ddi == i).expect("every element has a text run");
        let preceded = pos > 0 && runs[pos - 1].is_tail && run_text(doc, &runs[pos - 1]).is_some();
        let first_kid = tree.parent[i] == Some(0) && tree.dds.iter().enumerate().skip(1).find(|(_, _)| true).map(|(k, _)| k) == Some(i);
        types[i] = if preceded || (first_kid && texts[0].is_some()) { SprlType::PrecededSprl } else { SprlType::TlvlSprl };
    }
    // bidirectional inheritance for x/y whose first entry is None
    let inherits_from = |i: usize| -> (usize, usize) {
        let mut jmax = i;
        while jmax + 1 < n && empty(jmax) && tree.parent[jmax + 1] == Some(jmax) && !esprl[jmax + 1] {
            jmax += 1;
        }
        if jmax + 1 < n && empty(jmax) {
            jmax = i;
        }
        let mut jmin = i;
        while jmin > 0 && empty(jmin - 1) && tree.parent[jmin] == Some(jmin - 1) && !esprl[jmin - 1] {
            jmin -= 1;
        }
        (jmin, jmax)
    };
    let inherit = |vals: &[Vec<Option<f64>>]| -> (Vec<Vec<Option<f64>>>, Vec<usize>) {
        let mut out = vals.to_vec();
        let mut src: Vec<usize> = (0..n).collect();
        for i in 0..n {
            if vals[i][0].is_some() {
                continue;
            }
            let (lo, hi) = inherits_from(i);
            let mut cands: Vec<usize> = (lo..=hi).filter(|&j| vals[j][0].is_some()).collect();
            if cands.is_empty() {
                continue;
            }
            if cands.iter().any(|&j| j <= i) {
                cands.retain(|&j| j <= i);
            }
            let best = *cands.iter().min_by_key(|&&j| (j as i64 - i as i64).abs()).unwrap();
            out[i] = vals[best].clone();
            src[i] = best;
        }
        if out[0][0].is_none() {
            out[0] = vec![Some(0.0)];
        }
        (out, src)
    };
    let (x, xsrc) = inherit(&xs);
    let (y, ysrc) = inherit(&ys);
    Positions { x, y, dx: dxs, dy: dys, xsrc, ysrc, esprl, types }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineSpec {
    pub x: Vec<Option<f64>>,
    pub y: Vec<Option<f64>>,
    pub xsrc: NodeId,
    pub ysrc: NodeId,
    pub sprl: bool,
    pub anchor: Anchor,
    pub rtl: bool,
    pub tlvlno: Option<usize>,
    pub continue_x: bool,
    pub continue_y: bool,
    pub style_node: NodeId,
    pub first_run: usize,
}

pub fn line_specs(doc: &Doc, tree: &TextTree, runs: &[Run], pos: &Positions) -> Vec<LineSpec> {
    let root = tree.dds[0];
    let kids: Vec<NodeId> = doc.children(root).filter(|&c| doc.is_element(c)).collect();
    let nsprl = |d: NodeId| doc.is_element(d) && doc.attr(d, "sodipodi:role") == Some("line");
    let mut lines: Vec<LineSpec> = Vec::new();
    let mut sprl_inherits: Option<usize> = None;
    for (ri, r) in runs.iter().enumerate() {
        let txt = run_text(doc, r);
        let has_txt = txt.as_deref().is_some_and(|t| !t.is_empty());
        let newsprl = !r.is_tail && pos.types[r.ddi] == SprlType::TlvlSprl;
        if !(has_txt || newsprl) {
            continue;
        }
        let makeline = lines.is_empty()
            || (!r.is_tail
                && (newsprl || (pos.types[r.ddi] == SprlType::Normal && (pos.x[r.ddi][0].is_some() || pos.y[r.ddi][0].is_some()))));
        if !makeline {
            continue;
        }
        let edi = if r.is_tail { tree.dds.iter().position(|&d| d == r.style_node).unwrap_or(0) } else { r.ddi };
        let sel = r.style_node;
        let sty = doc.specified_style(sel);
        let (mut xv, mut xsrc, mut yv, mut ysrc) = (pos.x[edi].clone(), pos.xsrc[edi], pos.y[edi].clone(), pos.ysrc[edi]);
        let (mut continue_x, mut continue_y) = (false, false);
        if newsprl {
            match sprl_inherits {
                None => {
                    xv = vec![pos.x[0][0]];
                    xsrc = pos.xsrc[0];
                    yv = vec![pos.y[0][0]];
                    ysrc = pos.ysrc[0];
                }
                Some(li) => {
                    let node = tree.dds[r.ddi];
                    let parent = doc.parent(node).filter(|&p| doc.is_element(p)).unwrap_or(root);
                    let lht = composed_line_height(doc, node).max(composed_line_height(doc, parent));
                    let scf = composed_font_size(doc, node).scf;
                    let prev = &lines[li];
                    xv = vec![prev.x[0]];
                    yv = vec![prev.y[0].map(|y| y + lht / scf)];
                    // sources: keep the node ids; convert back to dds indices for symmetry
                    xsrc = tree.dds.iter().position(|&d| d == prev.xsrc).unwrap_or(0);
                    ysrc = tree.dds.iter().position(|&d| d == prev.ysrc).unwrap_or(0);
                }
            }
        } else {
            if xv[0].is_none() {
                match lines.last() {
                    Some(l) => {
                        xv = l.x.clone();
                        xsrc = tree.dds.iter().position(|&d| d == l.xsrc).unwrap_or(0);
                    }
                    None => {
                        xv = pos.x[0].clone();
                        xsrc = pos.xsrc[0];
                    }
                }
                continue_x = true;
            }
            if yv[0].is_none() {
                match lines.last() {
                    Some(l) => {
                        yv = l.y.clone();
                        ysrc = tree.dds.iter().position(|&d| d == l.ysrc).unwrap_or(0);
                    }
                    None => {
                        yv = pos.y[0].clone();
                        ysrc = pos.ysrc[0];
                    }
                }
                continue_y = true;
            }
        }
        let tlvlno = if r.ddi < tree.dds.len() && kids.contains(&tree.dds[r.ddi]) {
            kids.iter().position(|&k| k == tree.dds[r.ddi])
        } else if edi == 0 {
            Some(0)
        } else {
            None
        };
        let mut anchor = sty.get("text-anchor").and_then(Anchor::parse);
        if let Some(last) = lines.last() {
            if !nsprl(sel) && edi > 0 {
                anchor = Some(last.anchor);
            }
        }
        let mut anchor = anchor.unwrap_or(Anchor::Start);
        let rtl = sty.get("direction").is_some_and(|d| d.trim() == "rtl");
        if rtl {
            anchor = match anchor {
                Anchor::Start => Anchor::End,
                Anchor::End => Anchor::Start,
                a => a,
            };
        }
        lines.push(LineSpec {
            x: xv,
            y: yv,
            xsrc: tree.dds[xsrc],
            ysrc: tree.dds[ysrc],
            sprl: newsprl,
            anchor,
            rtl,
            tlvlno,
            continue_x,
            continue_y,
            style_node: sel,
            first_run: ri,
        });
        if newsprl || lines.len() == 1 {
            sprl_inherits = Some(lines.len() - 1);
        }
    }
    lines
}
```

Add `pub mod parse;` to `src/text/mod.rs`. Simplify the `first_kid` expression to `tree.parent[i] == Some(0) && i == 1` (the first direct child is always dds[1] in pre-order) — the long form above is what it means. Note the rtl swap in the test: the first line's anchor comes from `text-anchor:start` + `direction:rtl` → `End`; upstream applies the swap to inherited anchors too (same code path).

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_parse`
Expected: 3 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/parse.rs src/text/mod.rs tests/text_parse.rs
git commit -m "feat(text): text positions, effective sodipodi:role lines, bidirectional x/y inheritance, line starts"
```

---

### Task 11: Parsing, part B — characters, chunks, flags, `textLength` → `ParsedText`

**Files:**
- Modify: `src/text/parse.rs` (append the model and `ParsedText::parse`)
- Test: `tests/text_parse.rs` (append)

**Interfaces:**
- Consumes: `positions`, `line_specs` (Task 10); `CharTable::{true_face, char_face, prop, fonts}` (Task 9); `composed_font_size`, `letter_spacing`, `baseline_shift` (Task 6); `depathologize` (Task 8); `Doc::{composed_transform, specified_style, attr, resolve_href}`; `geom::ipx`.
- Produces (`sciink::text::parse`):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct CharLoc { pub node: NodeId, pub tail: bool, pub idx: u32 }
  #[derive(Debug, Clone)]
  pub struct TChar {
      pub c: char, pub loc: CharLoc, pub sty: Rc<Style>, pub spec: FontSpec, pub face: Option<FaceKey>, pub prop: Rc<CProp>,
      pub utfs: f64, pub tfs: f64, pub cwd: f64, pub caph: f64, pub spw: f64, pub dx: f64, pub dy: f64, pub lsp: f64, pub bshft: f64,
      pub line: usize, pub chunk: usize, pub windex: usize,
  }
  #[derive(Debug, Clone)]
  pub struct TChunk { pub x: f64, pub y: f64, pub chars: Vec<usize> /* indices into ParsedText.chars, in order */ }
  #[derive(Debug, Clone)]
  pub struct TLine { pub spec: LineSpec, pub style: Rc<Style>, pub chars: Vec<usize>, pub chunks: Vec<TChunk> }
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub enum TextLengthAdj { SpacingAndGlyphs(f64) /* scale */, Spacing(f64) /* extra lsp per gap */ }
  pub struct ParsedText {
      pub el: NodeId, pub transform: Affine, pub chars: Vec<TChar>, pub lines: Vec<TLine>,
      pub is_flow: bool, pub is_inkscape: bool, pub is_ml_inkscape: bool, pub text_length: Option<TextLengthAdj>,
      pub any_dx: bool, pub any_dy: bool,
  }
  impl ParsedText {
      /// Depathologizes `el` in the document, then builds the model. `None` for elements with no lines.
      pub fn parse(doc: &mut Doc, el: NodeId, ct: &mut CharTable, warn: &mut Warnings) -> Option<ParsedText>;
      pub fn chunks(&self) -> impl Iterator<Item = (usize, usize)> + '_;   // (line, chunk) pairs
      pub fn chunk(&self, li: usize, ci: usize) -> &TChunk;
      pub fn text(&self) -> String;                                          // all chars in order
  }
  ```
  Rules (spec §A.1 stages 1e–1g, P:587–650, 2696–2716): `is_flow` = tag is `flowRoot`, or `shape-inside` in the specified style names an existing element via `url(#id)`, or `inline-size` parses with `ipx` to a non-zero value. Flows: return the model with `lines` **empty** and `is_flow = true` (v1: flows are bbox-only and handled later; do not parse them). Characters: walk runs; a run's chars join the current (last) line; if no line exists yet (text before any line start — cannot happen after `line_specs`, but guard) skip. For each run with text: `sty = specified_style(style_node)`; `fs = composed_font_size(style_node)`; `spec = FontSpec::from_style(&sty)`; `tsty = ct.true_face(&spec)` (fall back to `ct.fonts.resolve`); `dx/dy` lists from `pos.dx/dy[ddi]` for text runs only (None → zeros; shorter → padded with 0; tails → zeros). For char `j`: `face = font_picker(txt, j, spec)`: non-space → `ct.char_face(spec, c)`; a space between two non-space chars whose faces agree → that face; at a run edge → the neighbour's face; else `tsty`. `prop = ct.prop(face, c)`; `cwd = prop.charw · utfs`, `caph = prop.caph · utfs`, `spw = prop.spacew · utfs`; `lsp`/`bshft` from `letter_spacing`/`baseline_shift` of the run's first char, shared by all chars of the run. Chunks per line (P:2696–2716): char `i == 0` opens a chunk at `(x[0], y[0])`; `i > 0` opens a new chunk when `x[i]` or `y[i]` exists and is Some, at `(x[min(i, len−1)], y[min(i, len−1)])` (None → the previous chunk's coordinate — use `unwrap_or(prev)`); otherwise the char joins the current chunk. Lines without chars are dropped (indices re-assigned). Flags: `is_inkscape` = every line with `tlvlno > 0` is sprl ∧ at least one such line ∧ every line's style has `-inkscape-font-specification`; `is_ml_inkscape = is_inkscape ∧ lines.len() > 1`. `textLength` (P:648–671) on the element: `spacingAndGlyphs` → `adj = textLength / Σ chunk widths` (chunk width = `Σ(cwd + dx + lsp)`-based width from layout — use `layout::chunk_geom` from Task 12? No: compute here as `Σ cwd` over the element's chars, which equals the spec's `Σwidths` when no dx/lsp; record the ruling in the ledger) → scale every `cwd`; else `adj = (textLength − Σ cwd) / (nchars − nchunks)` (0 when nchars ≤ 1) → add to every `lsp`. `any_dx/any_dy` = any `|dx| > XY_TOL`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_parse.rs`:

```rust
use sciink::text::parse::{ParsedText, TextLengthAdj};

fn parsed(svg: &str, el: &str) -> (Doc, ParsedText, CharTable) {
    let mut d = doc(svg);
    let mut w = Warnings::default();
    let n = id(&d, el);
    let mut ct = CharTable::build(&d, &[n], fonts(), &mut w);
    let pt = ParsedText::parse(&mut d, n, &mut ct, &mut w).expect("parsed");
    (d, pt, ct)
}

#[test]
fn chars_chunks_and_flags_for_a_simple_element() {
    let (_, pt, _) = parsed(
        &format!(r#"<svg {NS}><g transform="scale(2)"><text id="t" style="font-family:'DejaVu Sans';font-size:10px" x="1 2" y="3">AV <tspan id="s" style="font-size:50%;letter-spacing:1px" dx="0.5 0.25">bc</tspan></text></g></svg>"#),
        "t",
    );
    assert_eq!(pt.text(), "AV bc");
    assert_eq!(pt.lines.len(), 1);
    let ln = &pt.lines[0];
    // x="1 2": the second char opens a new chunk; the tspan without x/y joins the current chunk
    assert_eq!(ln.chunks.len(), 2);
    assert_eq!((ln.chunks[0].x, ln.chunks[0].y), (1.0, 3.0));
    assert_eq!((ln.chunks[1].x, ln.chunks[1].y), (2.0, 3.0));
    assert_eq!(ln.chunks[1].chars.len(), 4, "V, space, b, c");
    let a = &pt.chars[0];
    assert_eq!((a.c, a.utfs, a.tfs), ('A', 10.0, 20.0));
    assert!((a.cwd - a.prop.charw * 10.0).abs() < 1e-12);
    assert!((a.caph - 7.29).abs() < 0.05);
    assert_eq!((a.dx, a.dy, a.lsp, a.bshft), (0.0, 0.0, 0.0, 0.0));
    let b = &pt.chars[3];
    assert_eq!((b.c, b.utfs), ('b', 5.0));
    assert_eq!((b.dx, b.lsp), (0.5, 1.0));
    assert_eq!(pt.chars[4].dx, 0.25);
    assert_eq!((b.line, b.chunk, b.windex), (0, 1, 2));
    assert_eq!(b.loc.node, pt.chars[4].loc.node);
    assert_eq!((b.loc.tail, b.loc.idx), (false, 0));
    assert!(pt.any_dx && !pt.any_dy);
    assert!(!pt.is_flow && !pt.is_inkscape && !pt.is_ml_inkscape);
    assert_eq!(pt.transform, kurbo::Affine::scale(2.0));
    assert_eq!(pt.text_length, None);
    // the 'V' after 'A' carries the pair adjustment
    assert!(pt.chars[1].prop.dadvs.contains_key(&'A'));
}

#[test]
fn inkscape_multiline_flags_and_sprl_chunks() {
    let (_, pt, _) = parsed(
        &format!(r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
          <text id="t" style="font-size:10px;line-height:1.25;font-family:'DejaVu Sans';-inkscape-font-specification:'DejaVu Sans'" x="0" y="0"><tspan id="a" sodipodi:role="line" x="0" y="0">ab</tspan><tspan id="b" sodipodi:role="line" x="0" y="12.5">cd</tspan></text></svg>"#),
        "t",
    );
    assert_eq!(pt.lines.len(), 2);
    assert!(pt.is_inkscape && pt.is_ml_inkscape);
    assert_eq!(pt.lines[1].chunks[0].y, 12.5);
    assert_eq!(pt.lines[1].chars, [2, 3]);
    assert_eq!(pt.chars[2].line, 1);
}

#[test]
fn text_length_adjustments_and_flows() {
    let (_, pt, _) = parsed(
        &format!(r#"<svg {NS}><text id="t" style="font-family:'DejaVu Sans';font-size:10px" textLength="100" lengthAdjust="spacingAndGlyphs">ab</text></svg>"#),
        "t",
    );
    let natural: f64 = pt.chars.iter().map(|c| c.prop.charw * 10.0).sum();
    let Some(TextLengthAdj::SpacingAndGlyphs(s)) = pt.text_length else { panic!("{:?}", pt.text_length) };
    assert!((s - 100.0 / natural).abs() < 1e-9);
    assert!((pt.chars.iter().map(|c| c.cwd).sum::<f64>() - 100.0).abs() < 1e-9);
    let (_, pt2, _) = parsed(
        &format!(r#"<svg {NS}><text id="t" style="font-family:'DejaVu Sans';font-size:10px" textLength="100">abc</text></svg>"#),
        "t",
    );
    let natural2: f64 = pt2.chars.iter().map(|c| c.prop.charw * 10.0).sum();
    let Some(TextLengthAdj::Spacing(extra)) = pt2.text_length else { panic!() };
    assert!((extra - (100.0 - natural2) / 2.0).abs() < 1e-9, "spread over nchars − nchunks = 2 gaps");
    assert!(pt2.chars.iter().all(|c| (c.lsp - extra).abs() < 1e-9));
    // flows are recognised but not parsed in v1
    let (_, fl, _) = parsed(
        &format!(r#"<svg {NS}><text id="t" style="font-size:3px;inline-size:24;font-family:'DejaVu Sans'"><tspan x="0" y="1">flowed</tspan></text></svg>"#),
        "t",
    );
    assert!(fl.is_flow && fl.lines.is_empty() && fl.chars.is_empty());
    let mut d = doc(&format!(r#"<svg {NS}><text id="e"/></svg>"#));
    let mut w = Warnings::default();
    let n = id(&d, "e");
    let mut ct = CharTable::build(&d, &[n], fonts(), &mut w);
    assert!(ParsedText::parse(&mut d, n, &mut ct, &mut w).is_none(), "no text → no model");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_parse 2>&1 | grep -E '^error' | head -3`
Expected: `cannot find type ParsedText`.

- [ ] **Step 3: Append the model and `ParsedText::parse` to `src/text/parse.rs`**

```rust
use super::Warnings;
use super::fonts::{FaceKey, FontSpec};
use super::metrics::CProp;
use super::style::{baseline_shift, letter_spacing};
use super::table::CharTable;
use super::whitespace::depathologize;
use crate::geom::ipx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharLoc {
    pub node: NodeId,
    pub tail: bool,
    pub idx: u32,
}

#[derive(Debug, Clone)]
pub struct TChar {
    pub c: char,
    pub loc: CharLoc,
    pub sty: Rc<Style>,
    pub spec: FontSpec,
    pub face: Option<FaceKey>,
    pub prop: Rc<CProp>,
    pub utfs: f64,
    pub tfs: f64,
    pub cwd: f64,
    pub caph: f64,
    pub spw: f64,
    pub dx: f64,
    pub dy: f64,
    pub lsp: f64,
    pub bshft: f64,
    pub line: usize,
    pub chunk: usize,
    pub windex: usize,
}

#[derive(Debug, Clone)]
pub struct TChunk {
    pub x: f64,
    pub y: f64,
    pub chars: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct TLine {
    pub spec: LineSpec,
    pub style: Rc<Style>,
    pub chars: Vec<usize>,
    pub chunks: Vec<TChunk>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextLengthAdj {
    SpacingAndGlyphs(f64),
    Spacing(f64),
}

pub struct ParsedText {
    pub el: NodeId,
    pub transform: Affine,
    pub chars: Vec<TChar>,
    pub lines: Vec<TLine>,
    pub is_flow: bool,
    pub is_inkscape: bool,
    pub is_ml_inkscape: bool,
    pub text_length: Option<TextLengthAdj>,
    pub any_dx: bool,
    pub any_dy: bool,
}

fn is_flow(doc: &Doc, el: NodeId) -> bool {
    if doc.tag(el) == "flowRoot" {
        return true;
    }
    let sty = doc.specified_style(el);
    let shape = sty
        .get("shape-inside")
        .and_then(|v| v.trim().strip_prefix("url(#").and_then(|r| r.strip_suffix(')')))
        .is_some_and(|id| doc.by_id(id.trim()).is_some());
    shape || sty.get("inline-size").and_then(ipx).is_some_and(|v| v != 0.0)
}

impl ParsedText {
    pub fn parse(doc: &mut Doc, el: NodeId, ct: &mut CharTable, warn: &mut Warnings) -> Option<ParsedText> {
        let flow = is_flow(doc, el);
        depathologize(doc, el, flow, warn);
        let transform = doc.composed_transform(el);
        let mut pt = ParsedText {
            el,
            transform,
            chars: Vec::new(),
            lines: Vec::new(),
            is_flow: flow,
            is_inkscape: false,
            is_ml_inkscape: false,
            text_length: None,
            any_dx: false,
            any_dy: false,
        };
        if flow {
            return Some(pt); // v1: flows are detected, never parsed (spec §A.2 "Flowed text v1")
        }
        let tree = TextTree::new(doc, el);
        let runs = tree.runs(doc);
        let pos = positions(doc, &tree);
        let specs = line_specs(doc, &tree, &runs, &pos);
        if specs.is_empty() {
            return None;
        }
        let mut lines: Vec<TLine> = specs
            .iter()
            .map(|s| TLine { spec: s.clone(), style: doc.specified_style(s.style_node), chars: Vec::new(), chunks: Vec::new() })
            .collect();
        let mut next_line = 0usize; // index of the next LineSpec whose first_run we have not reached
        let mut cur: Option<usize> = None;
        for (ri, r) in runs.iter().enumerate() {
            while next_line < specs.len() && specs[next_line].first_run == ri {
                cur = Some(next_line);
                next_line += 1;
            }
            let Some(txt) = run_text(doc, r) else { continue };
            if txt.is_empty() {
                continue;
            }
            let Some(li) = cur else { continue };
            let sty = doc.specified_style(r.style_node);
            let fs = composed_font_size(doc, r.style_node);
            let spec = FontSpec::from_style(&sty);
            let tsty = ct.true_face(&spec).or_else(|| ct.fonts.resolve(&spec));
            let chars: Vec<char> = txt.chars().collect();
            let n = chars.len();
            let list = |v: &Vec<Option<f64>>| -> Vec<f64> {
                if r.is_tail || v[0].is_none() {
                    vec![0.0; n]
                } else {
                    let mut out: Vec<f64> = v.iter().map(|x| x.unwrap_or(0.0)).collect();
                    out.resize(n, 0.0);
                    out
                }
            };
            let dxv = list(&pos.dx[r.ddi]);
            let dyv = list(&pos.dy[r.ddi]);
            let lsp = letter_spacing(doc, r.style_node, &sty);
            let bshft = baseline_shift(doc, r.style_node, &sty);
            for (j, &c) in chars.iter().enumerate() {
                let face = font_picker(ct, &chars, j, &spec, tsty);
                let prop = ct.prop(face, c);
                let idx = pt.chars.len();
                pt.chars.push(TChar {
                    c,
                    loc: CharLoc { node: r.node, tail: r.is_tail, idx: j as u32 },
                    sty: sty.clone(),
                    spec: spec.clone(),
                    face,
                    utfs: fs.utfs,
                    tfs: fs.tfs,
                    cwd: prop.charw * fs.utfs,
                    caph: prop.caph * fs.utfs,
                    spw: prop.spacew * fs.utfs,
                    prop,
                    dx: dxv[j],
                    dy: dyv[j],
                    lsp,
                    bshft,
                    line: li,
                    chunk: 0,
                    windex: 0,
                });
                lines[li].chars.push(idx);
            }
        }
        // chunks (P:2696–2716)
        for ln in lines.iter_mut() {
            let (xs, ys) = (&ln.spec.x, &ln.spec.y);
            let (mut px, mut py) = (xs[0].unwrap_or(0.0), ys[0].unwrap_or(0.0));
            for (i, &ci) in ln.chars.iter().enumerate() {
                let opens = i == 0 || xs.get(i).is_some_and(Option::is_some) || ys.get(i).is_some_and(Option::is_some);
                if opens {
                    px = xs.get(i.min(xs.len() - 1)).copied().flatten().unwrap_or(px);
                    py = ys.get(i.min(ys.len() - 1)).copied().flatten().unwrap_or(py);
                    ln.chunks.push(TChunk { x: px, y: py, chars: vec![ci] });
                } else {
                    ln.chunks.last_mut().expect("opened at i == 0").chars.push(ci);
                }
            }
        }
        lines.retain(|l| !l.chars.is_empty());
        for (li, ln) in lines.iter().enumerate() {
            for (ci, ch) in ln.chunks.iter().enumerate() {
                for (wi, &c) in ch.chars.iter().enumerate() {
                    let tc = &mut pt.chars[c];
                    tc.line = li;
                    tc.chunk = ci;
                    tc.windex = wi;
                }
            }
        }
        if lines.is_empty() {
            return None;
        }
        pt.lines = lines;
        pt.any_dx = pt.chars.iter().any(|c| c.dx.abs() > XY_TOL);
        pt.any_dy = pt.chars.iter().any(|c| c.dy.abs() > XY_TOL);
        let tlvl: Vec<&TLine> = pt.lines.iter().filter(|l| l.spec.tlvlno.is_some_and(|n| n > 0)).collect();
        pt.is_inkscape = !tlvl.is_empty()
            && tlvl.iter().all(|l| l.spec.sprl)
            && pt.lines.iter().all(|l| l.style.get("-inkscape-font-specification").is_some());
        pt.is_ml_inkscape = pt.is_inkscape && pt.lines.len() > 1;
        // textLength (P:648–671)
        if let Some(tl) = doc.attr(el, "textLength").and_then(ipx) {
            let total: f64 = pt.chars.iter().map(|c| c.cwd).sum();
            let nchunks: usize = pt.lines.iter().map(|l| l.chunks.len()).sum();
            if doc.attr(el, "lengthAdjust").map(str::trim) == Some("spacingAndGlyphs") {
                let adj = if total != 0.0 { tl / total } else { 1.0 };
                for c in pt.chars.iter_mut() {
                    c.cwd *= adj;
                }
                pt.text_length = Some(TextLengthAdj::SpacingAndGlyphs(adj));
            } else {
                let gaps = pt.chars.len().saturating_sub(nchunks);
                let adj = if pt.chars.len() > 1 && gaps > 0 { (tl - total) / gaps as f64 } else { 0.0 };
                for c in pt.chars.iter_mut() {
                    c.lsp += adj;
                }
                pt.text_length = Some(TextLengthAdj::Spacing(adj));
            }
        }
        Some(pt)
    }

    pub fn chunks(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.lines.iter().enumerate().flat_map(|(li, l)| (0..l.chunks.len()).map(move |ci| (li, ci)))
    }

    pub fn chunk(&self, li: usize, ci: usize) -> &TChunk {
        &self.lines[li].chunks[ci]
    }

    pub fn text(&self) -> String {
        self.chars.iter().map(|c| c.c).collect()
    }
}

/// Which face Pango uses for a character (P:727–754): spaces borrow their neighbours' fallback face.
fn font_picker(ct: &mut CharTable, txt: &[char], j: usize, spec: &FontSpec, tsty: Option<FaceKey>) -> Option<FaceKey> {
    if txt[j] != ' ' {
        return ct.char_face(spec, txt[j]);
    }
    let before = txt[..j].iter().rev().find(|c| !c.is_whitespace()).copied();
    let after = txt[j + 1..].iter().find(|c| !c.is_whitespace()).copied();
    match (before, after) {
        (Some(b), Some(a)) => {
            let (fb, fa) = (ct.char_face(spec, b), ct.char_face(spec, a));
            if fb == fa { fb } else { tsty }
        }
        (None, Some(a)) => ct.char_face(spec, a),
        (Some(b), None) => ct.char_face(spec, b),
        (None, None) => tsty,
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_parse`
Expected: 6 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/parse.rs tests/text_parse.rs
git commit -m "feat(text): ParsedText — characters with metrics, chunks, Inkscape flags, textLength adjustments"
```

---

### Task 12: Layout — chunk geometry, character/chunk/line extents, ink boxes, `text_bbox`

**Files:**
- Create: `src/text/layout.rs`
- Modify: `src/text/mod.rs` (`pub mod layout;`)
- Test: `tests/text_layout.rs`

**Interfaces:**
- Consumes: `ParsedText`, `TChar`, `TChunk`, `TLine`, `XY_TOL` (Tasks 10–11); `geom::{union, transform_rect}`.
- Produces (`sciink::text::layout`):
  ```rust
  /// Per-character positions of one chunk in the element's untransformed frame (P:3581–3651).
  pub struct ChunkGeom { pub left: Vec<f64>, pub right: Vec<f64>, pub base: Vec<f64>, pub top: Vec<f64>, pub pts_ut: [Point; 4] /* BL, TL, TR, BR */ }
  pub fn unrendered_space(pt: &ParsedText, li: usize, ci: usize) -> bool;                 // P:3560–3579 (non-flow rule)
  pub fn chunk_geom(pt: &ParsedText, li: usize, ci: usize) -> ChunkGeom;
  pub fn char_pts_ut(pt: &ParsedText, g: &ChunkGeom, windex: usize) -> [Point; 4];         // BL, TL, TR, BR of one char
  pub fn char_pts_ink_ut(pt: &ParsedText, g: &ChunkGeom, ci: usize /* char index */, windex: usize) -> [Point; 4];
  pub fn transform_pts(t: Affine, p: [Point; 4]) -> [Point; 4];
  pub fn pts_bbox(p: &[Point; 4]) -> Rect;
  pub fn char_extents(pt: &ParsedText) -> Vec<Rect>;   // untransformed, one per char in `pt.chars` order (NaN-y chars skipped)
  pub fn chunk_extents(pt: &ParsedText) -> Vec<Rect>;
  pub fn line_extents(pt: &ParsedText) -> Vec<Rect>;
  pub fn full_extent(pt: &ParsedText) -> Option<Rect>;         // untransformed union of char extents
  pub fn full_ink_bbox(pt: &ParsedText) -> Option<Rect>;       // untransformed union of ink boxes
  pub fn text_bbox(pt: &ParsedText) -> Option<Rect>;           // union over chars of bbox(transform ∘ char pts) — the consumer API
  pub fn max_tfs(pt: &ParsedText) -> Option<f64>;
  ```
  Formulas (spec §A.1 "Geometry"): with chunk lists `cwd[i]`, `dx` (n+1 entries: `dx[0] = c0.dx`, … , `dx[n] = 0` — note upstream `addc` does `dx[-1] += c.dx; dx.append(0)` so `dx[i]` = char i's dx for `i < n`), `dxlsp = [0, lsp0, lsp1, …, lsp(n−1)]` (n+1 entries; `dxlsp[i]` = letter-spacing added *before* char i), `dadv[0] = 0`, `dadv[i] = c[i].prop.dadvs[c[i−1].c] · utfs` when both chars sit in the same run (`loc.node`, `loc.tail` equal) and the key exists, else 0: `wds[i] = cwd[i] + dx[i] + dxlsp[i] + (dx[i] == 0 ? dadv[i] : 0)`; `cstop = prefix_sum(wds)`; `cstrt[i] = cstop[i] − cwd[i]`; `chkw = cstop[n−1]`; `offx = −anfr · (chkw − (unrendered_space ? cwd[n−1] : 0) − (rtl ? 2·Σdx : 0))`; `left[i] = x + cstrt[i] + offx`, `right[i] = x + cstop[i] + offx`; `base[i] = y + prefix_sum(dy)[i] − bshft[i]`; `top[i] = base[i] − caph[i]`; chunk `pts_ut`: `lx2 = min_i(left[i] − dx[0] − dxlsp[0])` (upstream subtracts the scalars `dx[0]`/`dxlsp[0]` from every left before the min), `rx2 = lx2 + (right[n−1] − left[0])`, `by2 = max base`, `ty2 = min top`. Char `pts_ut = [(left, base), (left, top), (right, top), (right, base)]`. Ink: `x = left + inkbb[0]·utfs`, `y_bottom = base + inkbb[1]·utfs + inkbb[3]·utfs`, size `inkbb[2]·utfs × inkbb[3]·utfs`; `[(x, y_bottom), (x, y_bottom − h), (x + w, y_bottom − h), (x + w, y_bottom)]`. `unrendered_space` = chunk has > 1 chars ∧ its last char is the line's last char ∧ that char is `' '` or U+00A0.

- [ ] **Step 1: Write the failing tests**

Create `tests/text_layout.rs`:

```rust
mod support;

use std::path::PathBuf;

use kurbo::{Affine, Point};
use sciink::dom::{Doc, NodeId};
use sciink::text::Warnings;
use sciink::text::fonts::FontSystem;
use sciink::text::layout::{char_extents, char_pts_ut, chunk_extents, chunk_geom, full_extent, full_ink_bbox, line_extents, max_tfs, text_bbox, unrendered_space};
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

fn parsed(svg: &str, el: &str) -> (ParsedText, CharTable) {
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let mut w = Warnings::default();
    let n = id(&d, el);
    let mut ct = CharTable::build(&d, &[n], fonts(), &mut w);
    let pt = ParsedText::parse(&mut d, n, &mut ct, &mut w).expect("parsed");
    (pt, ct)
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn chunk_geometry_start_anchor_with_dx_letter_spacing_and_kerning() {
    let (pt, _) = parsed(&format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px;letter-spacing:1px" x="5" y="20" dx="0 2">AVx</text></svg>"#), "t");
    let g = chunk_geom(&pt, 0, 0);
    let [a, v, x] = [&pt.chars[0], &pt.chars[1], &pt.chars[2]];
    let kern_av = v.prop.dadvs[&'A'] * 10.0;
    assert!(kern_av < 0.0, "A–V kerns");
    // char 0: left = x + dx0 + dxlsp0(0) = 5 ; right = left + cwd
    assert!(close(g.left[0], 5.0));
    assert!(close(g.right[0], 5.0 + a.cwd));
    // char 1: dx=2 overrides the pair kerning; letter-spacing 1 added before it
    assert!(close(g.left[1], g.right[0] + 2.0 + 1.0));
    assert!(close(g.right[1], g.left[1] + v.cwd));
    // char 2: no dx → kerning applies (V–x, whatever it is) plus letter-spacing
    let kern_vx = x.prop.dadvs.get(&'V').copied().unwrap_or(0.0) * 10.0;
    assert!(close(g.left[2], g.right[1] + 1.0 + kern_vx));
    assert!(g.base.iter().all(|&b| close(b, 20.0)));
    assert!(g.top.iter().all(|&t| close(t, 20.0 - a.caph)));
    // chunk box: lx2 = min(left) − dx[0] − dxlsp[0] = 5
    assert!(close(g.pts_ut[0].x, 5.0) && close(g.pts_ut[0].y, 20.0));
    assert!(close(g.pts_ut[2].x, 5.0 + (g.right[2] - g.left[0])));
    assert!(close(g.pts_ut[1].y, 20.0 - a.caph));
    let p = char_pts_ut(&pt, &g, 1);
    assert_eq!(p[0], Point::new(g.left[1], g.base[1]));
    assert_eq!(p[2], Point::new(g.right[1], g.top[1]));
    assert!(!unrendered_space(&pt, 0, 0));
}

#[test]
fn middle_and_end_anchors_and_trailing_space() {
    let (pt, _) = parsed(&format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="0" y="0">ab </text></svg>"#), "t");
    assert!(unrendered_space(&pt, 0, 0), "trailing space of the line's last chunk is not rendered");
    let g = chunk_geom(&pt, 0, 0);
    // middle anchor centres the *rendered* width: the chunk width minus the unrendered trailing space
    let rendered = (g.right[2] - g.left[0]) - pt.chars[2].cwd;
    assert!(close(g.left[0], -rendered / 2.0), "{} vs {}", g.left[0], -rendered / 2.0);
    assert!(close(g.right[2] - pt.chars[2].cwd, rendered / 2.0));
    let (pt2, _) = parsed(&format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px;text-anchor:end" x="10" y="0">ab</text></svg>"#), "t");
    let g2 = chunk_geom(&pt2, 0, 0);
    assert!(close(g2.right[1], 10.0));
}

#[test]
fn dy_baseline_shift_and_multi_chunk_lines() {
    // x="0 30" on a two-character run → two chunks; the positioned tspan opens a second line
    let (pt, _) = parsed(&format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0 30" y="0" dy="0 5">ab<tspan id="s" x="60" style="font-size:50%;baseline-shift:super">c</tspan></text></svg>"#), "t");
    assert_eq!(pt.lines.len(), 2);
    assert_eq!(pt.lines[0].chunks.len(), 2);
    let g0 = chunk_geom(&pt, 0, 0);
    let g1 = chunk_geom(&pt, 0, 1);
    assert!(close(g0.base[0], 0.0));
    // chunk 1 starts at x=30; its char has dy=5
    assert!(close(g1.pts_ut[0].x, 30.0));
    assert!(close(g1.base[0], 5.0));
    // line 2: x=60, y continues (0); baseline-shift super = +40% of the parent's 10px → base −4
    let g2 = chunk_geom(&pt, 1, 0);
    assert!(pt.lines[1].spec.continue_y);
    assert!(close(g2.pts_ut[0].x, 60.0));
    assert!(close(g2.base[0], -4.0));
    assert!(close(g2.top[0], -4.0 - pt.chars[2].caph));
    assert!(close(pt.chars[2].caph, 0.729 * 5.0) || (pt.chars[2].caph - 3.645).abs() < 0.03);
    assert_eq!(chunk_extents(&pt).len(), 3);
    assert_eq!(line_extents(&pt).len(), 2);
    assert_eq!(char_extents(&pt).len(), 3);
}

#[test]
fn extents_ink_and_transformed_bbox() {
    let (pt, _) = parsed(&format!(r#"<svg {NS}><g transform="translate(100,50) scale(2)"><text id="t" style="{DV};font-size:10px" x="0" y="0">I</text></g></svg>"#), "t");
    let ext = full_extent(&pt).unwrap();
    let c = &pt.chars[0];
    assert!(close(ext.x0, 0.0) && close(ext.x1, c.cwd));
    assert!(close(ext.y1, 0.0) && close(ext.y0, -c.caph));
    let ink = full_ink_bbox(&pt).unwrap();
    assert!(ink.x0 > 0.0 && ink.x1 < c.cwd, "I's ink is narrower than its advance: {ink:?}");
    assert!(close(ink.y1, 0.0) && (ink.y0 + c.caph).abs() < 0.05, "I spans baseline to cap height: {ink:?}");
    // transformed: translate(100,50) scale(2)
    let bb = text_bbox(&pt).unwrap();
    assert!(close(bb.x0, 100.0) && close(bb.x1, 100.0 + 2.0 * c.cwd));
    assert!(close(bb.y1, 50.0) && close(bb.y0, 50.0 - 2.0 * c.caph));
    assert_eq!(max_tfs(&pt), Some(20.0));
    assert_eq!(pt.transform, Affine::translate((100.0, 50.0)) * Affine::scale(2.0));
}

#[test]
fn fixture_text_elements_all_have_finite_bboxes() {
    let Some(dir) = support::upstream_data_dir() else { return };
    let src = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let mut d = Doc::parse(&src).unwrap();
    let texts: Vec<NodeId> = d.descendants(d.svg()).filter(|&n| d.is_element(n) && d.tag(n) == "text").collect();
    assert!(texts.len() > 150, "{}", texts.len());
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &texts, fonts(), &mut w);
    let (mut parsed_n, mut flows) = (0, 0);
    for &t in &texts {
        match ParsedText::parse(&mut d, t, &mut ct, &mut w) {
            Some(pt) if pt.is_flow => flows += 1,
            Some(pt) => {
                parsed_n += 1;
                let bb = text_bbox(&pt).expect("bbox");
                assert!(bb.x0.is_finite() && bb.y0.is_finite() && bb.x1 >= bb.x0 && bb.y1 >= bb.y0, "{bb:?}");
                assert_eq!(char_extents(&pt).len(), pt.chars.len());
            }
            None => {}
        }
    }
    assert!(parsed_n > 120 && flows >= 2, "parsed {parsed_n}, flows {flows}");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_layout 2>&1 | grep -E '^error' | head -3`
Expected: `could not find layout in text`.

- [ ] **Step 3: Implement `src/text/layout.rs`**

```rust
//! Character positions and extents (spec §A.1 "Geometry"; upstream parser.py:3560–3651, 4202–4210, 1701–1793).

use kurbo::{Affine, Point, Rect};

use crate::geom::union;

use super::parse::{ParsedText, TChar};

pub struct ChunkGeom {
    pub left: Vec<f64>,
    pub right: Vec<f64>,
    pub base: Vec<f64>,
    pub top: Vec<f64>,
    pub pts_ut: [Point; 4],
}

/// Last char of a multi-char chunk is the line's last char and a space → not rendered (non-flow rule).
pub fn unrendered_space(pt: &ParsedText, li: usize, ci: usize) -> bool {
    let ln = &pt.lines[li];
    let ch = &ln.chunks[ci];
    let Some(&last) = ch.chars.last() else { return false };
    ch.chars.len() > 1 && ln.chars.last() == Some(&last) && matches!(pt.chars[last].c, ' ' | '\u{A0}')
}

fn dadv(prev: &TChar, cur: &TChar) -> f64 {
    if prev.loc.node == cur.loc.node && prev.loc.tail == cur.loc.tail {
        cur.prop.dadvs.get(&prev.c).copied().unwrap_or(0.0) * cur.utfs
    } else {
        0.0
    }
}

pub fn chunk_geom(pt: &ParsedText, li: usize, ci: usize) -> ChunkGeom {
    let ln = &pt.lines[li];
    let ch = &ln.chunks[ci];
    let cs: Vec<&TChar> = ch.chars.iter().map(|&i| &pt.chars[i]).collect();
    let n = cs.len();
    let anfr = ln.spec.anchor.anfr();
    let mut cstop = Vec::with_capacity(n);
    let mut acc = 0.0;
    for i in 0..n {
        let dx = cs[i].dx;
        let dxlsp = if i == 0 { 0.0 } else { cs[i - 1].lsp };
        let da = if i == 0 { 0.0 } else { dadv(cs[i - 1], cs[i]) };
        acc += cs[i].cwd + dx + dxlsp + if dx == 0.0 { da } else { 0.0 };
        cstop.push(acc);
    }
    let chkw = cstop[n - 1];
    let sum_dx: f64 = cs.iter().map(|c| c.dx).sum();
    let offx = -anfr
        * (chkw - if unrendered_space(pt, li, ci) { cs[n - 1].cwd } else { 0.0 } - if ln.spec.rtl { 2.0 * sum_dx } else { 0.0 });
    let left: Vec<f64> = (0..n).map(|i| ch.x + (cstop[i] - cs[i].cwd) + offx).collect();
    let right: Vec<f64> = (0..n).map(|i| ch.x + cstop[i] + offx).collect();
    let mut ady = 0.0;
    let base: Vec<f64> = cs
        .iter()
        .map(|c| {
            ady += c.dy;
            ch.y + ady - c.bshft
        })
        .collect();
    let top: Vec<f64> = base.iter().zip(&cs).map(|(b, c)| b - c.caph).collect();
    let lx2 = left.iter().map(|l| l - cs[0].dx - 0.0).fold(f64::INFINITY, f64::min);
    let rx2 = lx2 + (right[n - 1] - left[0]);
    let by2 = base.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let ty2 = top.iter().copied().fold(f64::INFINITY, f64::min);
    ChunkGeom {
        left,
        right,
        base,
        top,
        pts_ut: [Point::new(lx2, by2), Point::new(lx2, ty2), Point::new(rx2, ty2), Point::new(rx2, by2)],
    }
}

pub fn char_pts_ut(_pt: &ParsedText, g: &ChunkGeom, windex: usize) -> [Point; 4] {
    let (l, r, b, t) = (g.left[windex], g.right[windex], g.base[windex], g.top[windex]);
    [Point::new(l, b), Point::new(l, t), Point::new(r, t), Point::new(r, b)]
}

pub fn char_pts_ink_ut(pt: &ParsedText, g: &ChunkGeom, ci: usize, windex: usize) -> [Point; 4] {
    let c = &pt.chars[ci];
    let [ix, iy, iw, ih] = c.prop.inkbb;
    let (w, h) = (iw * c.utfs, ih * c.utfs);
    let x = g.left[windex] + ix * c.utfs;
    let y = g.base[windex] + iy * c.utfs + h;
    [Point::new(x, y), Point::new(x, y - h), Point::new(x + w, y - h), Point::new(x + w, y)]
}

pub fn transform_pts(t: Affine, p: [Point; 4]) -> [Point; 4] {
    [t * p[0], t * p[1], t * p[2], t * p[3]]
}

pub fn pts_bbox(p: &[Point; 4]) -> Rect {
    let xs = p.iter().map(|q| q.x);
    let ys = p.iter().map(|q| q.y);
    Rect::new(
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

fn each_char(pt: &ParsedText, mut f: impl FnMut(usize, &ChunkGeom, usize)) {
    for (li, ln) in pt.lines.iter().enumerate() {
        for (ci, ch) in ln.chunks.iter().enumerate() {
            let g = chunk_geom(pt, li, ci);
            for (wi, &c) in ch.chars.iter().enumerate() {
                f(c, &g, wi);
            }
        }
    }
}

/// One rectangle per character in `pt.chars` order (chars with a NaN baseline are skipped).
pub fn char_extents(pt: &ParsedText) -> Vec<Rect> {
    let mut out: Vec<(usize, Rect)> = Vec::new();
    each_char(pt, |c, g, wi| {
        let p = char_pts_ut(pt, g, wi);
        if !p[0].y.is_nan() {
            out.push((c, pts_bbox(&p)));
        }
    });
    out.sort_by_key(|(c, _)| *c);
    out.into_iter().map(|(_, r)| r).collect()
}

pub fn chunk_extents(pt: &ParsedText) -> Vec<Rect> {
    pt.chunks().map(|(li, ci)| pts_bbox(&chunk_geom(pt, li, ci).pts_ut)).filter(|r| !r.y0.is_nan()).collect()
}

pub fn line_extents(pt: &ParsedText) -> Vec<Rect> {
    pt.lines
        .iter()
        .enumerate()
        .filter_map(|(li, ln)| (0..ln.chunks.len()).fold(None, |acc, ci| union(acc, Some(pts_bbox(&chunk_geom(pt, li, ci).pts_ut)))))
        .collect()
}

pub fn full_extent(pt: &ParsedText) -> Option<Rect> {
    char_extents(pt).into_iter().fold(None, |acc, r| union(acc, Some(r)))
}

pub fn full_ink_bbox(pt: &ParsedText) -> Option<Rect> {
    let mut acc = None;
    each_char(pt, |c, g, wi| {
        let p = char_pts_ink_ut(pt, g, c, wi);
        if !p[0].y.is_nan() {
            acc = union(acc, Some(pts_bbox(&p)));
        }
    });
    acc
}

/// Bounding box in root coordinates: union over characters of the box of their transformed corners.
pub fn text_bbox(pt: &ParsedText) -> Option<Rect> {
    let mut acc = None;
    each_char(pt, |_, g, wi| {
        let p = transform_pts(pt.transform, char_pts_ut(pt, g, wi));
        if !p[0].y.is_nan() {
            acc = union(acc, Some(pts_bbox(&p)));
        }
    });
    acc
}

pub fn max_tfs(pt: &ParsedText) -> Option<f64> {
    pt.chars.iter().map(|c| c.tfs).fold(None, |m, v| Some(m.map_or(v, |m: f64| m.max(v))))
}
```

Add `pub mod layout;` to `src/text/mod.rs`. `lx2`: upstream subtracts `dx[0]` (the first char's dx) and `dxlsp[0]` (always 0) from every left before taking the min — implemented as `l - cs[0].dx - 0.0`. `geom::union(Option<Rect>, Option<Rect>)` exists (spec §B.1); `Rect::new(x0, y0, x1, y1)` is kurbo's.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test text_layout`
Expected: 5 passed (the fixture test prints a SKIP note and passes when `tests/upstream` is absent).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/text/layout.rs src/text/mod.rs tests/text_layout.rs
git commit -m "feat(text): chunk geometry, character/chunk/line extents, ink boxes and text_bbox"
```

---

### Task 13: Debug tools `font-probe` and `text-highlight`, About font diagnostics, `.inx` entries, oracle experiment

**Files:**
- Create: `src/tools/font_probe.rs`, `src/tools/text_highlight.rs`, `inx/font_probe.inx`, `inx/text_highlight.inx`
- Modify: `src/tools/mod.rs`, `src/tools/about.rs`, `src/cli.rs` (`ToolName`), `src/lib.rs` (`run` dispatch)
- Test: `tests/text_tools.rs` (new), `tests/cli.rs` (append)

**Interfaces:**
- Consumes: everything above; `crate::Output`, `cli::Common`, `cli::inx_bool`, `num::fmt`, `Doc::{write, new_element, set_attr, append_child, svg, descendants, selection}`.
- Produces:
  - `ToolName::{FontProbe, TextHighlight}` (`--tool=font-probe`, `--tool=text-highlight`); `run()` dispatches them.
  - `font-probe`: echoes the document; stderr report — one line per distinct `FontSpec` used by text in the selection (whole document when no `--id`): `'<families>' weight <w> <style> → <family> (<file name>)` or `→ (no font found)`, then one line per generic: `sans-serif → …`, `serif → …`, `monospace → …`, then `faces: N in M ms`. Warnings from the char table follow.
  - `text-highlight` (upstream `make_highlights`, P:1629–1660): params `--htype=char|charink|chunk|line|full|fullink` (default `char`); for every `<text>` in the selection (+ descendants; whole document when no `--id`) in document order: parse, compute the chosen extents (untransformed) and append to the root `<svg>` one `<rect x y width height transform style>` per extent with `transform = fmt_transform(pt.transform)` (attribute omitted when identity), style `fill:#007575;fill-opacity:0.4675` for even indices, `0.5675` for odd, numbers via `num::fmt`. Flows and elements without lines are skipped. stderr: `highlighted N rectangles (K text elements, F flows skipped)` + warnings.
  - `about`: after the document line, `fonts: N faces in M ms` and three lines `Arial → …`, `DejaVu Sans → …`, `sans-serif → …` (spec §C.3), with the same `→` formatting as font-probe (shared helper `pub fn describe_resolution(fs: &mut FontSystem, css_family: &str) -> String` in `font_probe.rs`).

- [ ] **Step 1: Write the failing tests**

Create `tests/text_tools.rs`:

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
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

// The library reads SCIINK_FONT_DIRS / SCIINK_NO_SYSTEM_FONTS through FontSystem::load();
// set them for the whole test binary so the tools see only the vendored fonts.
fn with_vendored_fonts<T>(f: impl FnOnce() -> T) -> T {
    // SAFETY: tests in this file run single-threaded with respect to these variables (set once, never changed).
    unsafe {
        std::env::set_var("SCIINK_NO_SYSTEM_FONTS", "1");
        std::env::set_var("SCIINK_FONT_DIRS", fontdir());
    }
    f()
}

#[test]
fn text_highlight_appends_one_rect_per_character() {
    let svg = format!(r#"<svg {NS}><g id="layer1" transform="translate(10,20)"><text id="t" style="font-family:'DejaVu Sans';font-size:10px" x="0" y="0">Test</text><text id="f" style="font-size:3px;inline-size:24"><tspan x="0" y="1">flow</tspan></text></g></svg>"#);
    let out = with_vendored_fonts(|| sciink::run(&args(&["--tool=text-highlight", "--htype=char", "--id=layer1"]), svg.as_bytes())).unwrap();
    let s = String::from_utf8(out.svg).unwrap();
    let d = roxmltree::Document::parse(&s).unwrap();
    let rects: Vec<_> = d.descendants().filter(|n| n.has_tag_name("rect")).collect();
    assert_eq!(rects.len(), 4, "{s}");
    for (i, r) in rects.iter().enumerate() {
        assert_eq!(r.attribute("transform"), Some("translate(10,20)"));
        let expect = if i % 2 == 0 { "fill:#007575;fill-opacity:0.4675" } else { "fill:#007575;fill-opacity:0.5675" };
        assert_eq!(r.attribute("style"), Some(expect));
        let w: f64 = r.attribute("width").unwrap().parse().unwrap();
        let h: f64 = r.attribute("height").unwrap().parse().unwrap();
        assert!(w > 0.0 && (h - 7.29).abs() < 0.05, "cap height 0.729 × 10: {h}");
        // rects are appended at the end of the root
        assert_eq!(r.parent().unwrap().tag_name().name(), "svg");
    }
    let x0: f64 = rects[0].attribute("x").unwrap().parse().unwrap();
    let x1: f64 = rects[1].attribute("x").unwrap().parse().unwrap();
    assert!(x0 == 0.0 && x1 > x0);
    assert!(out.messages.iter().any(|m| m.contains("highlighted 4 rectangles (2 text elements, 1 flows skipped)")), "{:?}", out.messages);
    // whole-document mode and the other htypes
    let full = with_vendored_fonts(|| sciink::run(&args(&["--tool=text-highlight", "--htype=full"]), svg.as_bytes())).unwrap();
    let s = String::from_utf8(full.svg).unwrap();
    assert_eq!(s.matches("<rect").count(), 1);
    for ht in ["charink", "chunk", "line", "fullink"] {
        let o = with_vendored_fonts(|| sciink::run(&args(&["--tool=text-highlight", &format!("--htype={ht}")]), svg.as_bytes())).unwrap();
        assert!(String::from_utf8(o.svg).unwrap().contains("<rect"), "{ht}");
    }
    assert!(sciink::run(&args(&["--tool=text-highlight", "--htype=bogus"]), svg.as_bytes()).is_err());
}

#[test]
fn font_probe_reports_resolutions_and_generics() {
    let svg = format!(r#"<svg {NS}><text id="t" style="font-family:Helvetica;font-weight:bold">a</text><text style="font-family:Roboto">b</text></svg>"#);
    let out = with_vendored_fonts(|| sciink::run(&args(&["--tool=font-probe"]), svg.as_bytes())).unwrap();
    assert_eq!(out.svg, svg.as_bytes(), "font-probe echoes the document");
    let report = out.messages.join("\n");
    assert!(report.contains("'Helvetica' weight 700 normal → DejaVu Sans (DejaVuSans-Bold.ttf)"), "{report}");
    assert!(report.contains("'Roboto' weight 400 normal → Roboto (Roboto-Regular.ttf)"), "{report}");
    assert!(report.contains("sans-serif → DejaVu Sans (DejaVuSans.ttf)"), "{report}");
    assert!(report.contains("serif → DejaVu Sans"), "{report}");
    assert!(report.contains("faces: 4 in "), "{report}");
    assert!(report.contains("font-family \"Helvetica\" not installed; measured with \"DejaVu Sans\""), "{report}");
}

#[test]
fn about_reports_fonts() {
    let svg = format!(r#"<svg {NS}><text>x</text></svg>"#);
    let out = with_vendored_fonts(|| sciink::run(&args(&["--tool=about"]), svg.as_bytes())).unwrap();
    let report = out.messages.join("\n");
    assert!(report.contains("fonts: 4 faces in "), "{report}");
    assert!(report.contains("Arial → DejaVu Sans (DejaVuSans.ttf)"), "{report}");
    assert!(report.contains("DejaVu Sans → DejaVu Sans (DejaVuSans.ttf)"), "{report}");
    assert!(report.contains("sans-serif → DejaVu Sans (DejaVuSans.ttf)"), "{report}");
}

/// Experiment A.5-2A: compare our per-character extents with upstream's `--debugparser`
/// reference (rendered by Inkscape/Pango on the author's machine). Needs the same font
/// families installed, so it runs only with SCIINK_SYSTEM_FONTS=1 and prints a per-family table.
#[test]
#[ignore]
fn debugparser_reference_agreement() {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        eprintln!("SKIP: set SCIINK_SYSTEM_FONTS=1 to compare against the upstream reference");
        return;
    }
    let Some(dir) = support::upstream_data_dir() else { return };
    let src = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let reference = std::fs::read_to_string(dir.join("refs/flatten_plots__--id__layer1__--testmode__True__--debugparser__True__Text_tests__svg.out")).unwrap();
    // reference rects in root coordinates
    let rd = roxmltree::Document::parse(&reference).unwrap();
    let ref_boxes: Vec<kurbo::Rect> = rd
        .descendants()
        .filter(|n| n.has_tag_name("rect") && n.attribute("style").is_some_and(|s| s.starts_with("fill:#007575")))
        .map(|n| {
            let g = |a: &str| n.attribute(a).unwrap().parse::<f64>().unwrap();
            let t = n.attribute("transform").and_then(sciink::geom::parse_transform).unwrap_or(kurbo::Affine::IDENTITY);
            sciink::geom::transform_rect(t, kurbo::Rect::new(g("x"), g("y"), g("x") + g("width"), g("y") + g("height")))
        })
        .collect();
    assert!(ref_boxes.len() > 3000, "{}", ref_boxes.len());
    // ours: same tool, whole document, system fonts
    let out = sciink::run(&args(&["--tool=text-highlight", "--htype=char"]), &src).unwrap();
    let od = roxmltree::Document::parse(std::str::from_utf8(&out.svg).unwrap()).unwrap();
    let ours: Vec<(kurbo::Rect, String)> = od
        .descendants()
        .filter(|n| n.has_tag_name("rect") && n.attribute("style").is_some_and(|s| s.starts_with("fill:#007575")))
        .map(|n| {
            let g = |a: &str| n.attribute(a).unwrap().parse::<f64>().unwrap();
            let t = n.attribute("transform").and_then(sciink::geom::parse_transform).unwrap_or(kurbo::Affine::IDENTITY);
            (sciink::geom::transform_rect(t, kurbo::Rect::new(g("x"), g("y"), g("x") + g("width"), g("y") + g("height"))), n.attribute("data-family").unwrap_or("?").to_string())
        })
        .collect();
    // nearest-neighbour deviation per family (px, root coordinates)
    let mut per_family: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    for (r, fam) in &ours {
        let best = ref_boxes
            .iter()
            .map(|q| (q.x0 - r.x0).abs().max((q.y1 - r.y1).abs()).max((q.width() - r.width()).abs()).max((q.height() - r.height()).abs()))
            .fold(f64::INFINITY, f64::min);
        per_family.entry(fam.clone()).or_default().push(best);
    }
    eprintln!("{:<24}{:>6}{:>10}{:>10}", "family", "chars", "median", "p90");
    for (fam, mut v) in per_family {
        v.sort_by(f64::total_cmp);
        let med = v[v.len() / 2];
        let p90 = v[(v.len() * 9 / 10).min(v.len() - 1)];
        eprintln!("{fam:<24}{:>6}{med:>10.3}{p90:>10.3}", v.len());
        if ["Arial", "Tahoma", "Verdana", "Roboto"].contains(&fam.as_str()) {
            assert!(med < 0.5, "{fam}: median deviation {med} px");
        }
    }
}
```

Note: `text-highlight` must also put `data-family="<resolved family of the char's face>"` on each `char`/`charink` rect so the oracle can group by family (harmless extra attribute on a debug output).

Append to `tests/cli.rs` (the binary-level check, mirroring the existing tests' `Command` helper — reuse it):

```rust
#[test]
fn text_highlight_runs_through_the_binary_with_vendored_fonts() {
    let p = tmp(
        "highlight.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg"><text style="font-family:'DejaVu Sans'" x="0" y="0">Hi</text></svg>"#,
    );
    let out = bin()
        .args(["--tool=text-highlight", "--htype=char"])
        .arg(&p)
        .env("SCIINK_NO_SYSTEM_FONTS", "1")
        .env("SCIINK_FONT_DIRS", concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert_eq!(s.matches("<rect").count(), 2, "{s}\n{}", String::from_utf8_lossy(&out.stderr));
}
```

(`bin()` and `tmp()` are the helpers already defined at the top of `tests/cli.rs`.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test text_tools 2>&1 | grep -E 'error|panicked' | head -5`
Expected: `unknown tool 'text-highlight'` / `font-probe` errors (the tests compile but the runs return `Err`).

- [ ] **Step 3: Implement**

`src/cli.rs`: add `FontProbe, TextHighlight,` to `ToolName` (clap's `ValueEnum` derives kebab-case names `font-probe`, `text-highlight`).

`src/lib.rs` `run()`: add arms `"font-probe" => tools::font_probe::run(argv, input), "text-highlight" => tools::text_highlight::run(argv, input),`.

`src/tools/mod.rs`: `pub mod font_probe; pub mod text_highlight;`.

`src/tools/font_probe.rs`:

```rust
//! Debug tool: which face every font specification in the document resolves to (spec §A.5-1).

use std::ffi::OsString;
use std::fmt::Write as _;

use clap::Parser;

use crate::Output;
use crate::cli::Common;
use crate::dom::{Doc, NodeId};
use crate::style::Style;
use crate::text::Warnings;
use crate::text::fonts::{FontSpec, FontStyle, FontSystem};
use crate::text::table::CharTable;
use crate::text::tree::{TextTree, run_text};

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct FontProbeCli {
    #[command(flatten)]
    pub common: Common,
}

fn first_line(e: clap::Error) -> String {
    e.to_string().lines().next().unwrap_or("invalid arguments").to_string()
}

/// `<family> (<file>)` or `(no font found)`.
pub fn describe_face(fs: &FontSystem, k: Option<crate::text::fonts::FaceKey>) -> String {
    match k {
        None => "(no font found)".to_string(),
        Some(k) => {
            let i = fs.face_info(k);
            let file = i.path.as_ref().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "memory".into());
            format!("{} ({file})", i.family)
        }
    }
}

/// `sans-serif → DejaVu Sans (DejaVuSans.ttf)`
pub fn describe_resolution(fs: &mut FontSystem, css_family: &str) -> String {
    let spec = FontSpec::from_style(&Style::parse(&format!("font-family:{css_family}")));
    let k = fs.resolve(&spec);
    format!("{css_family} → {}", describe_face(fs, k))
}

/// Text elements in the selection (with descendants) or the whole document, in document order.
pub fn text_elements(doc: &Doc, ids: &[String]) -> Vec<NodeId> {
    let roots: Vec<NodeId> = if ids.is_empty() { vec![doc.svg()] } else { doc.selection(ids) };
    let mut out = Vec::new();
    for r in roots {
        for n in doc.descendants(r) {
            if doc.is_element(n) && matches!(doc.tag(n), "text" | "flowRoot") && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = FontProbeCli::try_parse_from(argv).map_err(first_line)?;
    let doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let els = text_elements(&doc, &cli.common.ids);
    let mut specs: Vec<FontSpec> = Vec::new();
    for &el in &els {
        let tree = TextTree::new(&doc, el);
        for r in tree.runs(&doc) {
            if run_text(&doc, &r).is_some_and(|t| !t.is_empty()) {
                let s = FontSpec::from_style(&doc.specified_style(r.style_node));
                if !specs.contains(&s) {
                    specs.push(s);
                }
            }
        }
    }
    let mut warn = Warnings::default();
    let mut ct = CharTable::build(&doc, &els, FontSystem::load(), &mut warn);
    let mut rep = String::new();
    for s in &specs {
        let fams: Vec<String> = s.families.iter().map(|f| format!("'{f}'")).collect();
        let sty = match s.style { FontStyle::Normal => "normal", FontStyle::Italic => "italic", FontStyle::Oblique => "oblique" };
        let k = ct.true_face(s);
        let _ = writeln!(rep, "{} weight {} {sty} → {}", fams.join(","), s.weight, describe_face(&ct.fonts, k));
    }
    for g in ["sans-serif", "serif", "monospace"] {
        let _ = writeln!(rep, "{}", describe_resolution(&mut ct.fonts, g));
    }
    let _ = writeln!(rep, "faces: {} in {:.0} ms", ct.fonts.face_count(), ct.fonts.load_ms());
    for w in &warn.0 {
        let _ = writeln!(rep, "warning: {w}");
    }
    Ok(Output { svg: input.to_vec(), messages: vec![rep.trim_end().to_string()] })
}
```

`src/tools/text_highlight.rs`:

```rust
//! Debug tool: draw the parser's character/chunk/line extents as rectangles (upstream make_highlights).

use std::ffi::OsString;

use clap::Parser;
use kurbo::Rect;

use crate::Output;
use crate::cli::Common;
use crate::dom::Doc;
use crate::geom::fmt_transform;
use crate::num;
use crate::text::Warnings;
use crate::text::fonts::FontSystem;
use crate::text::layout::{char_extents, chunk_extents, full_extent, full_ink_bbox, line_extents, char_pts_ink_ut, chunk_geom, pts_bbox};
use crate::text::parse::ParsedText;
use crate::text::table::CharTable;

use super::font_probe::text_elements;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct TextHighlightCli {
    #[command(flatten)]
    pub common: Common,
    #[arg(long, default_value = "char", value_parser = ["char", "charink", "chunk", "line", "full", "fullink"])]
    pub htype: String,
}

const STYLE_EVEN: &str = "fill:#007575;fill-opacity:0.4675";
const STYLE_ODD: &str = "fill:#007575;fill-opacity:0.5675";

/// Untransformed ink box per character, in `pt.chars` order.
fn char_ink_extents(pt: &ParsedText) -> Vec<Rect> {
    let mut out: Vec<(usize, Rect)> = Vec::new();
    for (li, ln) in pt.lines.iter().enumerate() {
        for (ci, ch) in ln.chunks.iter().enumerate() {
            let g = chunk_geom(pt, li, ci);
            for (wi, &c) in ch.chars.iter().enumerate() {
                out.push((c, pts_bbox(&char_pts_ink_ut(pt, &g, c, wi))));
            }
        }
    }
    out.sort_by_key(|(c, _)| *c);
    out.into_iter().map(|(_, r)| r).collect()
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = TextHighlightCli::try_parse_from(argv)
        .map_err(|e| e.to_string().lines().next().unwrap_or("invalid arguments").to_string())?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let els = text_elements(&doc, &cli.common.ids);
    let mut warn = Warnings::default();
    let mut ct = CharTable::build(&doc, &els, FontSystem::load(), &mut warn);
    let (mut nrect, mut ntext, mut nflow) = (0usize, 0usize, 0usize);
    let root = doc.svg();
    for el in els {
        let Some(pt) = ParsedText::parse(&mut doc, el, &mut ct, &mut warn) else { continue };
        if pt.is_flow {
            nflow += 1;
            continue;
        }
        ntext += 1;
        let per_char = matches!(cli.htype.as_str(), "char" | "charink");
        let exts: Vec<Rect> = match cli.htype.as_str() {
            "char" => char_extents(&pt),
            "charink" => char_ink_extents(&pt),
            "chunk" => chunk_extents(&pt),
            "line" => line_extents(&pt),
            "full" => full_extent(&pt).into_iter().collect(),
            _ => full_ink_bbox(&pt).into_iter().collect(),
        };
        let tr = fmt_transform(pt.transform);
        for (i, e) in exts.iter().enumerate() {
            let r = doc.new_element("rect");
            doc.set_attr(r, "x", num::fmt(e.x0));
            doc.set_attr(r, "y", num::fmt(e.y0));
            doc.set_attr(r, "width", num::fmt(e.width()));
            doc.set_attr(r, "height", num::fmt(e.height()));
            if let Some(t) = &tr {
                doc.set_attr(r, "transform", t.clone());
            }
            doc.set_attr(r, "style", if i % 2 == 0 { STYLE_EVEN } else { STYLE_ODD });
            if per_char {
                if let Some(face) = pt.chars.get(i).and_then(|c| c.face) {
                    doc.set_attr(r, "data-family", ct.fonts.face_info(face).family.clone());
                }
            }
            doc.append_child(root, r);
            nrect += 1;
        }
    }
    let mut messages = vec![format!("highlighted {nrect} rectangles ({ntext} text elements, {nflow} flows skipped)")];
    messages.extend(warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
```

`src/tools/about.rs`: after the `document:` line add

```rust
    let mut fs = FontSystem::load();
    let _ = writeln!(r, "fonts: {} faces in {:.0} ms", fs.face_count(), fs.load_ms());
    for fam in ["Arial", "DejaVu Sans", "sans-serif"] {
        let _ = writeln!(r, "{}", crate::tools::font_probe::describe_resolution(&mut fs, fam));
    }
```

with `use crate::text::fonts::FontSystem;`. Keep the existing `selection:` line after these.

`inx/font_probe.inx` and `inx/text_highlight.inx` (copy `inx/about.inx`; change `<name>`, `<id>`, the hidden `tool` value, and put both under `<submenu name="Scientific"><submenu name="Debug"/></submenu>`). `text_highlight.inx` adds:

```xml
    <param name="htype" type="optiongroup" appearance="combo" gui-text="Highlight">
        <option value="char">Character extents</option>
        <option value="charink">Character ink</option>
        <option value="chunk">Chunks</option>
        <option value="line">Lines</option>
        <option value="full">Whole element</option>
        <option value="fullink">Whole element ink</option>
    </param>
```

and `needs-live-preview="true"`. Run `dist/package.sh macos-universal target/release/sciink && dist/test-package.sh dist/out/sciink-macos-universal.zip` after `cargo build --release` to confirm the packager picks up the new `.inx` files and every `<command>` still points at `bin/sciink`.

- [ ] **Step 4: Run all tests**

Run: `cargo test 2>&1 | grep -E '^test result|FAILED|panicked' ; cargo test --test text_tools -- --ignored debugparser 2>&1 | tail -3`
Expected: every suite passes; the ignored test prints its SKIP line (no `SCIINK_SYSTEM_FONTS`). Then the local experiment (this Mac has Arial/Tahoma/Verdana/Roboto; Calibri/Avenir may deviate):

Run: `SCIINK_SYSTEM_FONTS=1 cargo test --test text_tools -- --ignored --nocapture debugparser 2>&1 | tail -15`
Expected: a per-family table; Arial/Tahoma/Verdana/Roboto medians < 0.5 px. Paste the table into the task report. A failure here is a finding for the reviewer, not a reason to loosen the threshold silently.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/cli.rs src/lib.rs src/tools inx tests/text_tools.rs tests/cli.rs
git commit -m "feat(tools): font-probe and text-highlight debug tools; About reports fonts; debugparser oracle experiment"
```

---

## Plan self-review notes

- **Spec coverage** (§A.1 Stage 0 → Task 9; Stage 1a → Task 8; 1b → Task 7; 1c–1d → Task 10; 1e–1g → Task 11; "Geometry" + extents → Task 12; §A.3 → Tasks 3–5; §A.4 `FontSystem::{resolve, resolve_for_char, face_info, prop, pair_adv}` → Tasks 3–5 (as `Metrics`), `font_spec` → `FontSpec::from_style`, `composed_font_size/composed_line_height/baseline_shift/letter_spacing` → Task 6, `build_char_table` → Task 9, `ParsedText::parse`, `full_extent`, `full_ink_bbox`, `char_extents`, `chunk_extents`, `line_extents`, `max_tfs`, `text_bbox` → Tasks 11–12; §A.5 experiments 1 and 2A → Task 13; §C.3 About font lines and env vars → Tasks 3 and 13). Deferred to part 2: `snapshot_parsed`/`Which::Parsed` (needed only once editing exists), `get_ut_pts`, stages 2–12, `KerningOptions`, `write_clean_text`, `word-spacing`. Deferred and ledgered: variable-font axes (`set_variations`, §A.3), flow parsing (`parse_lines_flow`), `TextTree` sub-tree generators.
- **Placeholder scan**: none.
- **Type consistency**: `FaceKey`, `FontSpec`, `FontStyle` (Task 3–4) are used unchanged by Tasks 9, 11, 13; `CProp` fields `charw/spacew/caph/inkbb/dadvs` (Task 5) are read by Tasks 11–12; `Run{ddi,node,is_tail,style_node}` (Task 7) by Tasks 8–11; `LineSpec` fields (Task 10) by Tasks 11–12 via `TLine.spec`; `ParsedText.transform` is the composed transform used by `text_bbox` and `text-highlight`.
- **Known simplifications** (each is a `ponytail:` comment in code): `textLength` natural width uses `Σ cwd` (exact when the element has no dx/letter-spacing); font-size keywords beyond small/medium/large map to 12 px; unrendered characters are zero-width; `caph` is per face, not per glyph (upstream does the same).
