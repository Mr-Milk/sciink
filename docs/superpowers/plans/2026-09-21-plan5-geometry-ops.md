# Plan 5 — Geometry ops, Combine by Color, Text Ghoster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the non-text document operations of spec §B.2 — style composition, clip/mask merging, ungroup/group/unlink/deswitch, bounding boxes without Inkscape, stroke/fill analysis, the rectangle test, transform fusing, `global_transform`, `combine_paths`, housekeeping — as `sciink::ops`, and ship the first two real tools on top of them: **Combine by Color** and **Text Ghoster** (spec §B.3), verified against upstream's reference outputs.

**Architecture:** Five modules under `src/ops/` (`style`, `clip`, `bbox`, `xform`, `cleanup`) plus a small `ops::Ctx` that a tool carries through one run: the clips/masks the ops duplicated (garbage-collected at the end), the ids `delete_up` removed (their dangling `clip-path`/`mask` references are dropped at the end), collected warnings, and the text engine's character table built lazily the first time a text bounding box is needed (fonts load only when a tool measures text). Every op takes `(&mut Doc, &mut Ctx, …)`. Bounding boxes are memoized only within one call (`bbox`/`bb2` own a `HashMap`), never across mutations, so there is no invalidation protocol to get wrong. Structural recursion (nested groups, clip chains, `<use>` targets) is bounded by `ops::MAX_NEST = 64` levels; document-wide walks use the iterative `Doc::descendants`.

**Tech Stack:** Rust 2024 (rustc ≥ 1.85), kurbo 0.13 (`Affine`, `BezPath`, `Rect`, `Shape`), svgtypes 0.16 (`Paint`, `Color`), quick-xml 0.42, clap 4.6 — all existing; **no new dependencies**. Fonts only through `text::fonts`/`text::table` (Plan 3). Vendored test fonts: `tests/fonts/` (DejaVu Sans, Roboto).

**Spec:** `docs/spec/02-geometry-tools.md` — §B.1 (primitives, already in `geom`), §B.2 (every algorithm ported here), §B.3 "Combine by color" and "Text Ghoster", §B.4 (compatibility attribute `inkscape-scientific-combined-by-color`), §B.5 (layout, API sketch), §B.6 (risks). Upstream references, all under `~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/scientific_inkscape/` (read-only; never copy Python into the crate): `DH` = `dhelpers.py`, `AT` = `applytransform_mod.py`, `CBC` = `combine_by_color.py`, `TG` = `text_ghoster.py`, `U` = `inkex1_3_0/inkex/text/utils.py`, `C` = `inkex1_3_0/inkex/text/cache.py`. Upstream test data: `tests/upstream/data/{svg,refs}` (symlink; absent in CI → fixture tests `return` early with a SKIP note, see `tests/support/mod.rs::upstream_data_dir`).

## Global Constraints

- Every number written into the document goes through `sciink::num::fmt`; every transform through `geom::fmt_transform` via the new `Doc::set_transform` (identity → the attribute is removed). Lengths written by this plan (`stroke-width`, `stroke-dasharray`, rect/ellipse/line/image attributes) are unitless user units, as Inkscape itself writes them.
- Document-wide walks are iterative (`Doc::descendants`, `Doc::children`). Recursion is allowed only where the depth is the nesting depth of groups, clip chains or `<use>` targets (`bbox`, `merge_clipmask`, `is_rectangle`), always carries a `depth` parameter, and stops with the safe default plus a warning past `ops::MAX_NEST = 64` (a Rust stack overflow cannot be caught; a hostile self-referencing clip or clone must not abort the process).
- `clip-path`, `mask` and `transform` are **attributes**, never style properties, in every op (upstream reads them with `llget=True`): `ops::clip_ref` resolves the attribute, `Doc::set_attr` writes it, `ops::style::remove_inline` clears an inline-style copy without touching the attribute. `Doc::remove_style` must not be used for these two properties (it also removes a same-named presentation attribute).
- Only elements are composed onto / moved / deleted; Text/CData/Comment children are left where they are except where upstream deletes comments (`ungroup`).
- Ids: duplicated clips/masks/gradients get an id from `Doc::ensure_id` right after `Doc::deep_clone` (which drops ids) and go to the root `<defs>` (`Doc::defs`); `Ctx.created` records every duplicated clip/mask for `cleanup::gc_created_clips`; `cleanup::delete_up` records removed ids in `Ctx.deleted` for `cleanup::drop_dangling_refs`. A tool calls `ctx.finish(&mut doc)` exactly once, after all edits.
- Tolerances and constants (spec §B.1–B.3, exact): transform equality `geom::TOL = 1e-5`; singular when `|det| < 1e-12` (`geom::inverse` → `None`, callers skip with a warning); rectangle test `tol = 1e-3 · max(x-range, y-range)` and `uniquetol == 2` on both axes; Combine by Color match tolerances `0.001` for stroke width, alpha and dash entries, lightness threshold `lightnessth / 100` (default `15` → `0.15`), effective lightness `alpha · L/255 + (1 − alpha)` with `L = floor((max + min) / 2)` over the 0–255 channels (inkex's integer HSL lightness); Text Ghoster `EXTENT = 0.5`, `OPACITY = 0.75`, `STDDEV = 0.5`, fallback font size `ipx("8pt") = 10.666…`; `global_transform` restores `stroke-width := visual_before / sf_after` (and dashes alike) only when that differs from the current specified width by more than `1e-9` relative.
- Font determinism in tests: `support::with_vendored_fonts(|| …)` around anything that reaches `FontSystem::load()` (the `Ctx` does, lazily); oracles that need installed fonts are `#[ignore]` and start with the `SCIINK_SYSTEM_FONTS` guard used in `tests/text_fixtures.rs`.
- Tools print nothing on success: `Output.messages` carries only warnings (each prefixed `warning: `) and the empty-selection notice — Inkscape shows a dialog for any stderr output, and upstream's Combine/Ghoster are silent.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` must pass on every commit.
- Deviations from upstream are allowed only where this plan marks them **Deviation** (with the reason); Task 8 mirrors every one into `docs/spec/02-geometry-tools.md` under a new "Deliberate deviations (Plan 5)" section.

---

## File structure

| File | Responsibility |
|---|---|
| `src/dom.rs` (modify) | `Doc::set_tag` (rename keeping the prefix) |
| `src/style.rs` (modify) | `Doc::sheet_value` (what the `<style>` sheets alone say) |
| `src/geom/mod.rs` (modify) | `Doc::set_transform`, `Doc::viewbox` |
| `src/ops/mod.rs` (new) | `Ctx`, `ClipKind`, `clip_ref`, `MAX_NEST`, `label` |
| `src/ops/cleanup.rs` (new) | `delete_up`, `drop_dangling_refs`, `gc_created_clips`, `url_id` |
| `src/ops/style.rs` (new) | `compose_style`, `remove_inline`, `fix_css_clipmask`, `composed_list`, `Rgba`, `StrokeFill`, `strokefill` |
| `src/ops/bbox.rs` (new) | `BboxOpts`, `bbox`, `bb2`, `has_bbox`, `is_drawn`, `is_rectangle`, tag sets |
| `src/ops/clip.rs` (new) | `compose_all`, `merge_clipmask`, `unlink`, `group`, `ungroup`, `deswitch`, `ui_language` |
| `src/ops/xform.rs` (new) | `fuse`, `object_to_path`, `global_transform`, `combine_paths` |
| `src/tools/combine_by_color.rs`, `inx/combine_by_color.inx` (new) | Scientific ▸ Combine by Color |
| `src/tools/text_ghoster.rs`, `inx/text_ghoster.inx` (new) | Scientific ▸ Text Ghoster |
| `src/lib.rs`, `src/cli.rs`, `src/tools/mod.rs` (modify) | module list, dispatch, shared `first_line` |
| `tests/ops_cleanup.rs`, `tests/ops_style.rs`, `tests/ops_bbox.rs`, `tests/ops_clip.rs`, `tests/ops_xform.rs`, `tests/combine_by_color.rs`, `tests/text_ghoster.rs` (new); `tests/dom.rs`, `tests/style.rs`, `tests/geom.rs` (append) | tests |
| `README.md`, `docs/spec/02-geometry-tools.md` (modify) | tool listing, deviations |

Test conventions (existing): `mod support;` first; `Doc::parse(s.as_bytes()).unwrap()`; `roxmltree` (dev-dependency) to inspect written output; `support::upstream_data_dir()` returns `None` (with a SKIP note) when the upstream fixtures are absent — fixture tests must `return` in that case. Every new `tests/ops_*.rs` file starts with this preamble (copy it verbatim; remove `use` items a file does not need — clippy's `-D warnings` fails on unused imports):

```rust
mod support;

use kurbo::{Affine, Rect};
use sciink::dom::{Doc, NodeId};
use sciink::ops::Ctx;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:svg=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}
fn rect_close(r: Rect, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    close(r.x0, x0) && close(r.y0, y0) && close(r.x1, x1) && close(r.y1, y1)
}
/// The serialized document (for `contains` checks and roxmltree parsing).
fn out(d: &Doc) -> String {
    let mut v = Vec::new();
    d.write(&mut v);
    String::from_utf8(v).unwrap()
}
/// Element children of `n`, in order.
fn kids(d: &Doc, n: NodeId) -> Vec<NodeId> {
    d.children(n).filter(|&c| d.is_element(c)).collect()
}
```

---

### Task 1: Foundations — `Doc::set_tag`, `Doc::sheet_value`, `Doc::set_transform`/`viewbox`, `ops::Ctx`, `ops::cleanup`

**Files:**
- Modify: `src/dom.rs` (append inside `impl Doc`, next to `ensure_id`), `src/style.rs` (append inside the `impl Doc` block at the end), `src/geom/mod.rs` (append inside `impl Doc`), `src/lib.rs` (add `pub mod ops;` between `pub mod num;` and `pub mod paths;`)
- Create: `src/ops/mod.rs`, `src/ops/cleanup.rs`
- Test: `tests/dom.rs`, `tests/style.rs`, `tests/geom.rs` (append), `tests/ops_cleanup.rs` (new)

**Interfaces:**
- Consumes (Plan 1/3): `Doc::{attr, set_attr, remove_attr, by_id, children, descendants, parent, detach, is_element, is_comment, tag, qname, svg, defs, ensure_id, text, set_text}`, `geom::{ipx, fmt_transform, Affine, Rect}`, `style::Stylesheet` internals (`rules`, `Selector::matches`, `specificity`, `order`, `decls`), `text::Warnings`, `text::fonts::FontSystem::load`, `text::table::CharTable::build`.
- Produces:
  - `Doc::set_tag(&mut self, n: NodeId, local: &str)`
  - `Doc::sheet_value(&self, n: NodeId, prop: &str) -> Option<String>`
  - `Doc::set_transform(&mut self, n: NodeId, t: Affine)`, `Doc::viewbox(&self) -> Option<Rect>`
  - `ops::MAX_NEST: usize = 64`; `ops::ClipKind { Clip, Mask }` with `attr(self) -> &'static str`; `ops::clip_ref(doc, n, kind) -> Option<NodeId>`; `ops::label(doc, n) -> String`
  - `ops::Ctx { pub created: Vec<NodeId>, pub deleted: HashSet<String>, pub warn: Warnings, text: Option<CharTable> }` with `new()`, `ensure_char_table(&mut self, doc: &Doc)`, `char_table(&mut self, doc: &Doc) -> &mut CharTable`, `finish(&mut self, doc: &mut Doc)`
  - `ops::cleanup::{delete_up(doc, ctx, n), drop_dangling_refs(doc, deleted: &HashSet<String>), gc_created_clips(doc, created: &mut Vec<NodeId>), url_id(v: &str) -> Option<&str>}`

- [ ] **Step 1: Write the failing tests**

Append to `tests/dom.rs` (the file has `mod support;`, `roundtrip`, and uses `sciink::dom::Doc`; add `use sciink::dom::NodeId;` only if it is not imported yet):

```rust
#[test]
fn set_tag_keeps_the_prefix_and_invalidates_styles() {
    let mut d = Doc::parse(
        br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:svg="http://www.w3.org/2000/svg"><style>path{fill:red}</style><svg:line id="l" x1="0"/><rect id="r"/></svg>"#,
    )
    .unwrap();
    let l = d.by_id("l").unwrap();
    let r = d.by_id("r").unwrap();
    assert_eq!(d.specified(l, "fill"), None);
    d.set_tag(l, "path");
    assert_eq!(d.qname(l), "svg:path");
    assert_eq!(d.tag(l), "path");
    assert_eq!(d.specified(l, "fill").as_deref(), Some("red"), "tag selectors re-match");
    d.set_tag(r, "path");
    assert_eq!(d.qname(r), "path");
    assert_eq!(d.by_id("l"), Some(l), "ids survive a rename");
    let mut v = Vec::new();
    d.write(&mut v);
    let s = String::from_utf8(v).unwrap();
    assert!(s.contains(r#"<svg:path id="l" x1="0"/>"#) && s.contains(r#"<path id="r"/>"#), "{s}");
}
```

Append to `tests/style.rs` (helpers `doc`, `id`, `NS` exist there):

```rust
#[test]
fn sheet_value_reports_only_stylesheet_declarations() {
    let d = doc(&format!(
        "<svg {NS}><style>#r{{clip-path:url(#a)}} rect{{clip-path:url(#b);fill:red}}</style>\
         <rect id=\"r\" clip-path=\"url(#c)\" style=\"clip-path:url(#d);fill:blue\"/><rect id=\"q\"/></svg>"
    ));
    let r = id(&d, "r");
    // the id rule beats the tag rule; neither the attribute nor the inline style count
    assert_eq!(d.sheet_value(r, "clip-path").as_deref(), Some("url(#a)"));
    assert_eq!(d.sheet_value(r, "fill").as_deref(), Some("red"));
    assert_eq!(d.sheet_value(r, "stroke"), None);
    assert_eq!(d.sheet_value(id(&d, "q"), "clip-path").as_deref(), Some("url(#b)"));
    // later rules of equal weight win, `!important` beats everything
    let d = doc(&format!(
        "<svg {NS}><style>rect{{fill:red !important}} rect{{fill:green}} #r{{fill:blue}}</style><rect id=\"r\"/></svg>"
    ));
    assert_eq!(d.sheet_value(id(&d, "r"), "fill").as_deref(), Some("red"));
}
```

Append to `tests/geom.rs` (it imports `sciink::dom::Doc`, `sciink::geom::*`, has `close`):

```rust
#[test]
fn set_transform_writes_or_removes_the_attribute() {
    let mut d = Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg"><g id="g" transform="scale(2)"/></svg>"#).unwrap();
    let g = d.by_id("g").unwrap();
    d.set_transform(g, Affine::translate((1.0, 2.0)));
    assert_eq!(d.attr(g, "transform"), Some("translate(1,2)"));
    d.set_transform(g, Affine::IDENTITY);
    assert_eq!(d.attr(g, "transform"), None);
    d.set_transform(g, Affine::new([1.0, 0.0, 0.0, 1.0, 1e-7, 0.0]));
    assert_eq!(d.attr(g, "transform"), None, "within TOL of identity");
}

#[test]
fn viewbox_parses_or_falls_back_to_width_height() {
    let d = Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"/>"#).unwrap();
    let r = d.viewbox().unwrap();
    assert!(close(r.x0, 0.0) && close(r.width(), 200.0) && close(r.height(), 100.0));
    let d = Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="1,2,3,4"/>"#).unwrap();
    let r = d.viewbox().unwrap();
    assert!(close(r.x0, 1.0) && close(r.y0, 2.0) && close(r.width(), 3.0) && close(r.height(), 4.0));
    let d = Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="20mm"/>"#).unwrap();
    let r = d.viewbox().unwrap();
    assert!(close(r.width(), 37.795275591) && close(r.height(), 75.590551181), "{r:?}");
    let d = Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#).unwrap();
    assert_eq!(d.viewbox(), None);
}
```

Create `tests/ops_cleanup.rs` with the preamble (keep `Doc`, `NodeId`, `Ctx`, `NS`, `doc`, `id`, `out`; drop the rest) and:

```rust
use std::collections::HashSet;

use sciink::ops::cleanup::{delete_up, drop_dangling_refs, gc_created_clips, url_id};

#[test]
fn url_id_parses_url_references() {
    assert_eq!(url_id("url(#abc)"), Some("abc"));
    assert_eq!(url_id(" url(# abc ) "), Some("abc"));
    assert_eq!(url_id("none"), None);
    assert_eq!(url_id("#abc"), None);
}

#[test]
fn delete_up_removes_emptied_ancestors_below_the_root_and_records_ids() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="layer"><g id="inner"><path id="p"/></g><rect id="keep"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_p = id(&d, "p");
    delete_up(&mut d, &mut ctx, n_p);
    assert_eq!(d.by_id("p"), None);
    assert_eq!(d.by_id("inner"), None, "left without element children → deleted too");
    assert!(d.by_id("layer").is_some(), "still holds the rect");
    assert_eq!(ctx.deleted, HashSet::from(["p".to_string(), "inner".to_string()]));
    let n_keep = id(&d, "keep");
    delete_up(&mut d, &mut ctx, n_keep);
    assert_eq!(d.by_id("layer"), None);
    assert!(ctx.deleted.contains("layer") && ctx.deleted.contains("keep"));
    // `Doc::write` keeps each element's parsed open/close form (lossless round-trip), so the
    // emptied root is written `<svg …></svg>`, not `<svg …/>`
    assert_eq!(out(&d), format!(r#"<svg {NS}></svg>"#), "the root is never deleted");
}

#[test]
fn delete_up_counts_comments_as_children_and_records_subtree_ids() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="g"><!-- note --><g id="s"><path id="a"/><path id="b"/></g></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_s = id(&d, "s");
    delete_up(&mut d, &mut ctx, n_s);
    assert!(d.by_id("g").is_some(), "a comment keeps the group alive (lxml len counts it)");
    assert_eq!(ctx.deleted, HashSet::from(["s".to_string(), "a".to_string(), "b".to_string()]));
}

#[test]
fn drop_dangling_refs_touches_only_deleted_ids() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="a" clip-path="url(#gone)"/><rect id="b" mask="url(#gone)" clip-path="url(#other)"/><rect id="c" style="clip-path:url(#gone);fill:red"/></svg>"#
    ));
    let deleted: HashSet<String> = HashSet::from(["gone".to_string()]);
    drop_dangling_refs(&mut d, &deleted);
    assert_eq!(d.attr(id(&d, "a"), "clip-path"), None);
    assert_eq!(d.attr(id(&d, "b"), "mask"), None);
    assert_eq!(d.attr(id(&d, "b"), "clip-path"), Some("url(#other)"), "pre-existing dangling refs are not ours to fix");
    assert_eq!(d.attr(id(&d, "c"), "style"), Some("fill:red"), "inline copies are dropped too");
    drop_dangling_refs(&mut d, &HashSet::new());
    assert_eq!(d.attr(id(&d, "b"), "clip-path"), Some("url(#other)"));
}

#[test]
fn gc_created_clips_removes_unreferenced_clips_and_chains() {
    let mut d = doc(&format!(
        r#"<svg {NS}><style>#r{{clip-path:url(#c4)}}</style><defs><clipPath id="c1"><path d="M0 0h1v1z"/></clipPath><clipPath id="c2"><path clip-path="url(#c1)" d="M0 0h1v1z"/></clipPath><clipPath id="c3"><path d="M0 0h1v1z"/></clipPath><clipPath id="c4"><path d="M0 0h1v1z"/></clipPath><clipPath id="c5"><path d="M0 0h1v1z"/></clipPath></defs><rect id="r" clip-path="url(#c3)"/><rect id="s" style="mask:url(#c5)"/></svg>"#
    ));
    let mut created = vec![id(&d, "c1"), id(&d, "c2"), id(&d, "c3"), id(&d, "c4"), id(&d, "c5")];
    gc_created_clips(&mut d, &mut created);
    assert_eq!(d.by_id("c2"), None, "nothing references c2");
    assert_eq!(d.by_id("c1"), None, "only the dead c2 referenced c1 → second pass");
    assert!(d.by_id("c3").is_some(), "attribute reference");
    assert!(d.by_id("c4").is_some(), "stylesheet reference");
    assert!(d.by_id("c5").is_some(), "inline style reference");
    assert_eq!(created, vec![id(&d, "c3"), id(&d, "c4"), id(&d, "c5")]);
}

#[test]
fn ctx_finish_runs_both_sweeps() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><path d="M0 0h1v1z"/></clipPath></defs><g id="g"><path id="p" clip-path="url(#q)"/></g><rect id="q"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    ctx.created.push(id(&d, "c"));
    let n_q = id(&d, "q");
    delete_up(&mut d, &mut ctx, n_q);
    ctx.finish(&mut d);
    assert_eq!(d.by_id("c"), None);
    assert_eq!(d.attr(id(&d, "p"), "clip-path"), None);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test dom --test style --test geom --test ops_cleanup 2>&1 | grep -E "^error|^test result" | head` → compile errors (`set_tag`, `sheet_value`, `set_transform`, `viewbox`, `sciink::ops` missing).

- [ ] **Step 3: Implement**

`src/dom.rs`, inside `impl Doc` right after `ensure_id`:

```rust
    /// Renames an element, keeping its namespace prefix (`svg:line` → `svg:path`). Style caches
    /// are invalidated because tag selectors may now match differently.
    pub fn set_tag(&mut self, n: NodeId, local: &str) {
        let Kind::Element { name, .. } = &mut self.nodes[n as usize].kind else {
            return;
        };
        *name = match name.rsplit_once(':') {
            Some((prefix, _)) => format!("{prefix}:{local}"),
            None => local.to_string(),
        };
        self.bump_style();
        self.bump();
    }
```

`src/style.rs`, inside the `impl Doc` block (after `remove_style`):

```rust
    /// The value the `<style>` sheets alone give `prop` on `n` — no presentation attribute, no
    /// inline `style` — i.e. what Inkscape would apply over an attribute we write (upstream
    /// `svg.cssdict[id][prop]`, cache.py:1073–1164). Highest key `(!important, specificity,
    /// source order)` wins; later declarations win ties.
    pub fn sheet_value(&self, n: NodeId, prop: &str) -> Option<String> {
        let sheet = self.stylesheet();
        // (clippy `type_complexity`: alias the key like `DeclKey` above — `type SheetKey = (bool, (u32, u32, u32), usize);`)
        let mut best: Option<(SheetKey, String)> = None;
        for rule in &sheet.rules {
            if !rule.selector.matches(self, n) {
                continue;
            }
            for (k, v, imp) in &rule.decls {
                if k != prop {
                    continue;
                }
                let key = (*imp, rule.selector.specificity, rule.order);
                if best.as_ref().is_none_or(|(bk, _)| key >= *bk) {
                    best = Some((key, v.clone()));
                }
            }
        }
        best.map(|(_, v)| v)
    }
```

`src/geom/mod.rs`, inside `impl Doc` (after `composed_transform`):

```rust
    /// Writes `transform` (`translate`/`scale`/`matrix` form); an identity removes the attribute.
    pub fn set_transform(&mut self, n: NodeId, t: Affine) {
        match fmt_transform(t) {
            Some(s) => self.set_attr(n, "transform", s),
            None => {
                self.remove_attr(n, "transform");
            }
        }
    }

    /// The root `viewBox` as a rectangle; without one, `[0, 0, width, height]` of the root
    /// (`cache.py:1185–1192`); `None` when neither is usable.
    pub fn viewbox(&self) -> Option<Rect> {
        let svg = self.svg();
        if let Some(vb) = self.attr(svg, "viewBox") {
            let v: Vec<f64> = vb
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f64>().ok())
                .collect::<Option<Vec<_>>>()?;
            if v.len() == 4 && v[2] > 0.0 && v[3] > 0.0 {
                return Some(Rect::new(v[0], v[1], v[0] + v[2], v[1] + v[3]));
            }
            return None;
        }
        let w = ipx(self.attr(svg, "width")?)?;
        let h = ipx(self.attr(svg, "height")?)?;
        (w > 0.0 && h > 0.0).then(|| Rect::new(0.0, 0.0, w, h))
    }
```

Create `src/ops/mod.rs`:

```rust
//! Document operations shared by the tools (spec docs/spec/02-geometry-tools.md §B.2): style
//! composition, clip/mask merging, ungrouping, unlinking, bounding boxes, transform fusing.

pub mod bbox;
pub mod cleanup;
pub mod clip;
pub mod style;
pub mod xform;

use std::collections::HashSet;

use crate::dom::{Doc, NodeId};
use crate::text::Warnings;
use crate::text::fonts::FontSystem;
use crate::text::table::CharTable;

/// Nesting depth past which the recursive helpers (`bbox`, `clip::merge_clipmask`,
/// `bbox::is_rectangle`) stop following groups, clips and clones: deeper is a cycle or a hostile
/// document, and a Rust stack overflow cannot be caught (`main` can only catch panics).
pub const MAX_NEST: usize = 64;

/// The two url-referencing attributes the ops manage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipKind {
    Clip,
    Mask,
}

impl ClipKind {
    pub fn attr(self) -> &'static str {
        match self {
            ClipKind::Clip => "clip-path",
            ClipKind::Mask => "mask",
        }
    }
}

/// The `<clipPath>`/`<mask>` the element's **attribute** points at (`url(#id)`), if it resolves.
/// Attributes only (upstream `get_link(…, llget=True)`); see `text::write::clip_of` for the
/// style-aware variant the text writer uses.
pub fn clip_ref(doc: &Doc, n: NodeId, kind: ClipKind) -> Option<NodeId> {
    doc.attr(n, kind.attr())
        .and_then(cleanup::url_id)
        .and_then(|id| doc.by_id(id))
}

/// `<tag id="…">` for messages.
pub fn label(doc: &Doc, n: NodeId) -> String {
    match doc.attr(n, "id") {
        Some(id) => format!("<{} id=\"{id}\">", doc.tag(n)),
        None => format!("<{}>", doc.tag(n)),
    }
}

/// Per-run state a tool threads through the ops.
#[derive(Default)]
pub struct Ctx {
    /// `<clipPath>`/`<mask>` elements duplicated by the ops; the ones nothing references any
    /// more are removed by `finish` (`cleanup::gc_created_clips`).
    pub created: Vec<NodeId>,
    /// ids of elements removed by `cleanup::delete_up`; `finish` strips `clip-path`/`mask`
    /// references that still point at them (`cleanup::drop_dangling_refs`).
    pub deleted: HashSet<String>,
    pub warn: Warnings,
    /// Character table over every `<text>`/`<flowRoot>` of the document, built on first use so
    /// fonts load only when a tool measures text.
    text: Option<CharTable>,
}

impl Ctx {
    pub fn new() -> Ctx {
        Ctx::default()
    }

    /// Builds the character table if it does not exist yet.
    pub fn ensure_char_table(&mut self, doc: &Doc) {
        if self.text.is_none() {
            let els: Vec<NodeId> = doc
                .descendants(doc.svg())
                .filter(|&n| doc.is_element(n) && matches!(doc.tag(n), "text" | "flowRoot"))
                .collect();
            let ct = CharTable::build(doc, &els, FontSystem::load(), &mut self.warn);
            self.text = Some(ct);
        }
    }

    pub fn char_table(&mut self, doc: &Doc) -> &mut CharTable {
        self.ensure_char_table(doc);
        self.text.as_mut().expect("built by ensure_char_table")
    }

    /// End-of-run housekeeping; call once after all edits.
    pub fn finish(&mut self, doc: &mut Doc) {
        cleanup::drop_dangling_refs(doc, &self.deleted);
        cleanup::gc_created_clips(doc, &mut self.created);
    }
}
```

Until Tasks 2–6 create them, make `bbox.rs`, `clip.rs`, `style.rs`, `xform.rs` one-line files containing only their module doc comment (`//! Bounding boxes (Task 3).` etc.) so the crate compiles; each later task replaces its file.

Create `src/ops/cleanup.rs`:

```rust
//! Housekeeping (spec §B.2 "Housekeeping"; upstream cache.py:685–717 `delete(deleteup=True)`,
//! flatten_plots.py:513–527 created-clip garbage collection).

use std::collections::HashSet;

use crate::dom::{Doc, NodeId};
use crate::style::Style;

use super::Ctx;

/// `url(#id)` → `id`.
pub fn url_id(v: &str) -> Option<&str> {
    v.trim()
        .strip_prefix("url(#")?
        .strip_suffix(')')
        .map(str::trim)
}

/// Deletes `n`, then every ancestor left without element or comment children (lxml's `len`
/// counts both), stopping below the root `<svg>`. Every removed id lands in `ctx.deleted`.
pub fn delete_up(doc: &mut Doc, ctx: &mut Ctx, n: NodeId) {
    let mut target = n;
    loop {
        let parent = doc.parent(target);
        let ids: Vec<String> = doc
            .descendants(target)
            .filter_map(|d| doc.attr(d, "id").map(str::to_string))
            .collect();
        ctx.deleted.extend(ids);
        doc.detach(target);
        match parent {
            Some(p)
                if p != doc.svg()
                    && doc.is_element(p)
                    && !doc.children(p).any(|c| doc.is_element(c) || doc.is_comment(c)) =>
            {
                target = p;
            }
            _ => break,
        }
    }
}

/// Removes `clip-path`/`mask` attributes and inline-style entries that reference an id in
/// `deleted`, so no orphan `url(#…)` survives a deletion. Done once per run instead of per
/// deletion (upstream cache.py:697–704); references that were dangling before are left alone.
pub fn drop_dangling_refs(doc: &mut Doc, deleted: &HashSet<String>) {
    if deleted.is_empty() {
        return;
    }
    let nodes: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|&n| doc.is_element(n))
        .collect();
    for n in nodes {
        for att in ["clip-path", "mask"] {
            if doc
                .attr(n, att)
                .and_then(url_id)
                .is_some_and(|id| deleted.contains(id))
            {
                doc.remove_attr(n, att);
            }
        }
        if let Some(inline) = doc.attr(n, "style") {
            let mut st = Style::parse(inline);
            let mut changed = false;
            for att in ["clip-path", "mask"] {
                if st
                    .get(att)
                    .and_then(url_id)
                    .is_some_and(|id| deleted.contains(id))
                {
                    st.remove(att);
                    changed = true;
                }
            }
            if changed {
                doc.set_style_map(n, &st);
            }
        }
    }
}

/// Every id referenced as `url(#id)` by a `clip-path`/`mask` attribute, an inline style, or a
/// `<style>` sheet anywhere in the document.
fn referenced_clip_ids(doc: &Doc) -> HashSet<String> {
    let mut out = HashSet::new();
    for n in doc.descendants(doc.svg()) {
        if !doc.is_element(n) {
            continue;
        }
        for att in ["clip-path", "mask"] {
            if let Some(id) = doc.attr(n, att).and_then(url_id) {
                out.insert(id.to_string());
            }
        }
        if let Some(inline) = doc.attr(n, "style") {
            let st = Style::parse(inline);
            for att in ["clip-path", "mask"] {
                if let Some(id) = st.get(att).and_then(url_id) {
                    out.insert(id.to_string());
                }
            }
        }
        if doc.tag(n) == "style" {
            let css = doc.text_content(n);
            for piece in css.split("url(#").skip(1) {
                if let Some(id) = piece.split(')').next() {
                    out.insert(id.trim().to_string());
                }
            }
        }
    }
    out
}

/// Deletes every created clip/mask nothing references any more, repeating until stable (a clip
/// may be referenced only from another dead clip). Survivors stay in `created`.
pub fn gc_created_clips(doc: &mut Doc, created: &mut Vec<NodeId>) {
    loop {
        let referenced = referenced_clip_ids(doc);
        let before = created.len();
        let mut keep = Vec::with_capacity(before);
        for &c in created.iter() {
            let alive = doc.parent(c).is_some();
            let used = doc.attr(c, "id").is_some_and(|id| referenced.contains(id));
            if alive && !used {
                doc.detach(c);
            } else if alive {
                keep.push(c);
            }
        }
        *created = keep;
        if created.len() == before {
            break;
        }
    }
}
```

`src/lib.rs`: add `pub mod ops;` (alphabetical, after `pub mod num;`).

- [ ] **Step 4: Run to verify success**

Run: `cargo test --test dom --test style --test geom --test ops_cleanup 2>&1 | grep -E "^test result|FAILED|panicked"` → all pass. Then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/dom.rs src/style.rs src/geom/mod.rs src/lib.rs src/ops tests/dom.rs tests/style.rs tests/geom.rs tests/ops_cleanup.rs
git commit -m "feat(ops): Ctx, cleanup (delete_up, dangling refs, clip gc), Doc::set_tag/sheet_value/set_transform/viewbox"
```

---

### Task 2: `ops/style.rs` — style composition, CSS clip pinning, `composed_list`, `strokefill`

**Files:**
- Create: `src/ops/style.rs` (replace the placeholder)
- Test: `tests/ops_style.rs` (new)

**Interfaces:**
- Consumes: Task 1 (`ClipKind`, `cleanup::url_id`, `Doc::sheet_value`), `Doc::{cascaded_style, specified_style, specified, set_style_map, ensure_id, children, prepend_child, new_element, new_text, append_child, text, set_text}`, `text::style::composed_width(doc, n, prop) -> FontSize { tfs, scf, utfs }` (generic over `prop`; `tfs` is the visual size, `scf` the composed scale factor, `utfs` the untransformed size), `geom::{ipx, scale_factor}`, `svgtypes::{Paint, Color}`.
- Produces:
  - `compose_style(doc: &mut Doc, child: NodeId, group_style: &Style)`
  - `remove_inline(doc: &mut Doc, n: NodeId, prop: &str)`
  - `fix_css_clipmask(doc: &mut Doc, n: NodeId, kind: ClipKind)`
  - `composed_list(doc: &Doc, n: NodeId, prop: &str) -> Option<Vec<f64>>` (visual units)
  - `pub struct Rgba { pub r: u8, pub g: u8, pub b: u8, pub alpha: f64, pub efflightness: f64 }`
  - `pub struct StrokeFill { pub stroke: Option<Rgba>, pub fill: Option<Rgba>, pub stroke_is_url: bool, pub fill_is_url: bool, pub stroke_width: Option<f64>, pub dasharray: Option<Vec<f64>>, pub marker_start: Option<String>, pub marker_mid: Option<String>, pub marker_end: Option<String> }` (`Default`, `PartialEq`)
  - `strokefill(doc: &Doc, n: NodeId) -> StrokeFill`

**Deviations (documented in Task 8):** (a) `compose_style` writes `opacity` only when either side specified one (upstream writes `opacity:1.0` on every ungrouped child); (b) `currentColor` resolves through the specified `color` (upstream: no paint); (c) a `url(#…)` paint reports `stroke_is_url`/`fill_is_url` with `stroke`/`fill = None` **and** `stroke_width = None` (upstream keeps the width next to a gradient element that its own consumers then trip over); (d) an unparsable dash entry makes the whole list `None` (upstream raises).

- [ ] **Step 1: Write the failing tests**

Create `tests/ops_style.rs` with the preamble (keep `Doc`, `NodeId`, `NS`, `doc`, `id`, `close`, `out`; drop `Affine`, `Rect`, `Ctx`, `rect_close`, `kids`) and:

```rust
use sciink::ops::ClipKind;
use sciink::ops::style::{compose_style, composed_list, fix_css_clipmask, remove_inline, strokefill};
use sciink::style::Style;

#[test]
fn compose_style_pushes_group_declarations_under_the_childs_own() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="g" style="fill:red;opacity:0.5" stroke="blue"><path id="p" style="fill:green;opacity:0.5"/><path id="q" fill="yellow"/></g></svg>"#
    ));
    let gst = d.cascaded_style(id(&d, "g"));
    let p = id(&d, "p");
    compose_style(&mut d, p, &gst);
    let st = Style::parse(d.attr(p, "style").unwrap());
    assert_eq!(st.get("fill"), Some("green"), "the child's own value wins");
    assert_eq!(st.get("stroke"), Some("blue"), "the group's presentation attribute is pushed down");
    assert_eq!(st.get("opacity"), Some("0.25"), "opacities multiply");
    let q = id(&d, "q");
    compose_style(&mut d, q, &gst);
    let st = Style::parse(d.attr(q, "style").unwrap());
    assert_eq!(st.get("fill"), Some("yellow"), "a presentation attribute is the child's own declaration");
    assert_eq!(st.get("opacity"), Some("0.5"));
}

#[test]
fn compose_style_writes_no_opacity_when_neither_side_has_one() {
    let mut d = doc(&format!(r#"<svg {NS}><g id="g" style="fill:red"><path id="p"/></g></svg>"#));
    let gst = d.cascaded_style(id(&d, "g"));
    let p = id(&d, "p");
    compose_style(&mut d, p, &gst);
    assert_eq!(d.attr(p, "style"), Some("fill:red"));
}

#[test]
fn remove_inline_leaves_the_attribute_alone() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="r" clip-path="url(#a)" style="clip-path:url(#b);fill:red"/></svg>"#
    ));
    let r = id(&d, "r");
    remove_inline(&mut d, r, "clip-path");
    assert_eq!(d.attr(r, "style"), Some("fill:red"));
    assert_eq!(d.attr(r, "clip-path"), Some("url(#a)"));
    remove_inline(&mut d, r, "clip-path");
    assert_eq!(d.attr(r, "style"), Some("fill:red"));
}

#[test]
fn fix_css_clipmask_pins_the_attribute_with_an_id_rule() {
    let mut d = doc(&format!(
        r#"<svg {NS}><style>#r{{clip-path:url(#a)}}</style><defs><clipPath id="a"/><clipPath id="b"/></defs><rect id="r" clip-path="url(#b)" style="clip-path:url(#a);fill:red"/></svg>"#
    ));
    let r = id(&d, "r");
    fix_css_clipmask(&mut d, r, ClipKind::Clip);
    let s = out(&d);
    assert!(s.contains("#r{clip-path:url(#a)}\n#r{clip-path:url(#b)}</style>"), "{s}");
    assert_eq!(d.attr(r, "style"), Some("fill:red"));
    assert_eq!(d.attr(r, "clip-path"), Some("url(#b)"));
    assert_eq!(d.sheet_value(r, "clip-path").as_deref(), Some("url(#b)"), "the appended rule now wins");
    // an agreeing sheet appends nothing; a removed attribute is pinned as `none`
    fix_css_clipmask(&mut d, r, ClipKind::Clip);
    assert_eq!(out(&d).matches("#r{").count(), 2);
    d.remove_attr(r, "clip-path");
    fix_css_clipmask(&mut d, r, ClipKind::Clip);
    assert!(out(&d).contains("\n#r{clip-path:none}</style>"), "{}", out(&d));
    // the mask flavour on a document without a sheet: only the inline copy goes
    let mut d = doc(&format!(r#"<svg {NS}><rect mask="url(#m)" style="mask:url(#n)"/></svg>"#));
    let r = d.children(d.svg()).find(|&c| d.is_element(c)).unwrap();
    fix_css_clipmask(&mut d, r, ClipKind::Mask);
    assert_eq!(d.attr(r, "style"), None);
    assert!(!out(&d).contains("<style"), "no disagreement → no sheet created");
    // a disagreeing sheet elsewhere, an id-less element: id assigned, root <style> created first
    let mut d = doc(&format!(
        r#"<svg {NS}><g><style>rect{{mask:url(#x)}}</style></g><defs/><rect mask="url(#m)"/></svg>"#
    ));
    let r = d.descendants(d.svg()).find(|&c| d.is_element(c) && d.tag(c) == "rect").unwrap();
    fix_css_clipmask(&mut d, r, ClipKind::Mask);
    let s = out(&d);
    let rid = d.attr(r, "id").expect("an id was assigned").to_string();
    assert!(s.starts_with(&format!("<svg {NS}><style>")), "root style is the first child: {s}");
    assert!(s.contains(&format!("\n#{rid}{{mask:url(#m)}}</style>")), "{s}");
    assert_eq!(d.sheet_value(r, "mask").as_deref(), Some("url(#m)"), "the new sheet is seen");
}

#[test]
fn composed_list_scales_dashes_and_handles_none() {
    let d = doc(&format!(
        r#"<svg {NS}><g transform="scale(2)"><path id="a" style="stroke-dasharray:1, 2 3"/><path id="b" style="stroke-dasharray:none"/><path id="c"/><path id="d" style="stroke-dasharray:1,abc"/></g></svg>"#
    ));
    assert_eq!(composed_list(&d, id(&d, "a"), "stroke-dasharray"), Some(vec![2.0, 4.0, 6.0]));
    assert_eq!(composed_list(&d, id(&d, "b"), "stroke-dasharray"), None);
    assert_eq!(composed_list(&d, id(&d, "c"), "stroke-dasharray"), None);
    assert_eq!(composed_list(&d, id(&d, "d"), "stroke-dasharray"), None);
}

#[test]
fn strokefill_resolves_paints_widths_dashes_and_markers() {
    let d = doc(&format!(
        r#"<svg {NS}><g transform="scale(2)" style="color:#ffffff"><path id="p" style="stroke:#204060;stroke-opacity:0.5;opacity:0.5;stroke-width:2;fill:none;stroke-dasharray:1,2;marker-end:url(#m)"/><path id="q"/><path id="u" style="fill:url(#g);stroke:url(#h);stroke-width:1"/><path id="c" style="fill:currentColor;stroke:white;stroke-width:0"/><path id="w" style="stroke:red;stroke-width:1;stroke-dasharray:1"/></g></svg>"#
    ));
    let p = strokefill(&d, id(&d, "p"));
    let s = p.stroke.unwrap();
    assert_eq!((s.r, s.g, s.b), (0x20, 0x40, 0x60));
    assert!(close(s.alpha, 0.25));
    // L = floor((0x60 + 0x20) / 2) = 64
    assert!(close(s.efflightness, 0.25 * 64.0 / 255.0 + 0.75), "{}", s.efflightness);
    assert_eq!(p.fill, None);
    assert!(!p.fill_is_url && !p.stroke_is_url);
    assert!(close(p.stroke_width.unwrap(), 4.0), "2 × scale 2");
    assert_eq!(p.dasharray, Some(vec![2.0, 4.0]));
    assert_eq!(p.marker_end.as_deref(), Some("url(#m)"));
    assert_eq!(p.marker_start, None);
    // defaults: fill black (opaque, lightness 0); no stroke → no width, no dashes
    let q = strokefill(&d, id(&d, "q"));
    let f = q.fill.unwrap();
    assert_eq!((f.r, f.g, f.b), (0, 0, 0));
    assert!(close(f.alpha, 1.0) && close(f.efflightness, 0.0));
    assert_eq!((q.stroke, q.stroke_width, q.dasharray), (None, None, None));
    // url paints
    let u = strokefill(&d, id(&d, "u"));
    assert!(u.fill_is_url && u.stroke_is_url);
    assert_eq!((u.fill, u.stroke, u.stroke_width), (None, None, None));
    // currentColor resolves through `color`; a zero-width stroke is no stroke
    let c = strokefill(&d, id(&d, "c"));
    let f = c.fill.unwrap();
    assert_eq!((f.r, f.g, f.b), (255, 255, 255));
    assert!(close(f.efflightness, 1.0));
    assert_eq!((c.stroke, c.stroke_width), (None, None));
    // a real stroke keeps its dashes; opaque red has L = floor(255 / 2) = 127
    let w = strokefill(&d, id(&d, "w"));
    assert!(close(w.stroke.unwrap().efflightness, 127.0 / 255.0));
    assert_eq!(w.dasharray, Some(vec![2.0]));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test ops_style 2>&1 | grep -E "^error" | head -3` → unresolved imports.

- [ ] **Step 3: Implement**

Replace `src/ops/style.rs`:

```rust
//! Style-level operations (spec §B.2 "Style composition", "fix_css_clipmask", "Stroke/fill";
//! upstream DH:359–366, 397–413, 182–194, 1171–1225).

use crate::dom::{Doc, NodeId};
use crate::geom::{ipx, scale_factor};
use crate::num;
use crate::style::Style;
use crate::text::style::composed_width;

use super::ClipKind;
use super::cleanup::url_id;

/// Pushes a group's own declarations under a child's (DH:359–366): the child's inline style
/// becomes `group_style` overridden by `cascaded(child)`, and opacities multiply.
///
/// **Deviation:** `opacity` is written only when either side specified one (upstream writes
/// `opacity:1.0` on every ungrouped child).
pub fn compose_style(doc: &mut Doc, child: NodeId, group_style: &Style) {
    let own = doc.cascaded_style(child);
    let opacity = |st: &Style| st.get("opacity").and_then(|v| v.trim().parse::<f64>().ok());
    let mut merged = group_style.clone();
    merged.merge_over(&own);
    match (opacity(&own), opacity(group_style)) {
        (None, None) => {
            merged.remove("opacity");
        }
        (a, b) => merged.set("opacity", &num::fmt(a.unwrap_or(1.0) * b.unwrap_or(1.0))),
    }
    doc.set_style_map(child, &merged);
}

/// Removes `prop` from the inline `style` attribute only — a same-named presentation attribute
/// (`clip-path="url(#…)"`) is left alone, unlike `Doc::remove_style`.
pub fn remove_inline(doc: &mut Doc, n: NodeId, prop: &str) {
    if let Some(inline) = doc.attr(n, "style") {
        let mut st = Style::parse(inline);
        if st.remove(prop).is_some() {
            doc.set_style_map(n, &st);
        }
    }
}

/// The root `<style>` (first `<style>` child of `<svg>`), created as the first child of `<svg>`
/// when absent (C:915–927).
fn root_style(doc: &mut Doc) -> NodeId {
    let svg = doc.svg();
    if let Some(s) = doc
        .children(svg)
        .find(|&c| doc.is_element(c) && doc.tag(c) == "style")
    {
        return s;
    }
    let s = doc.new_element("style");
    doc.prepend_child(svg, s);
    s
}

/// Appends `add` to the sheet text of `sty` (its last text/CDATA child, or a new text node).
/// Goes through `Doc::set_text` in both cases so the stylesheet cache is invalidated.
fn append_sheet_text(doc: &mut Doc, sty: NodeId, add: &str) {
    let last = doc.children(sty).filter(|&c| doc.text(c).is_some()).last();
    let t = match last {
        Some(t) => t,
        None => {
            let t = doc.new_text("");
            doc.append_child(sty, t);
            t
        }
    };
    let s = format!("{}{add}", doc.text(t).unwrap_or(""));
    doc.set_text(t, &s);
}

/// DH:397–413: Inkscape lets a stylesheet `clip-path`/`mask` override the attribute, so when the
/// sheet disagrees with the attribute we just wrote, pin the attribute's value with an id rule
/// appended to the root `<style>` (`\n#id{clip-path:url(#x)}`; `none` when the attribute is
/// absent — upstream writes Python's `None` there) and clear any inline-style copy.
pub fn fix_css_clipmask(doc: &mut Doc, n: NodeId, kind: ClipKind) {
    let att = kind.attr();
    if let Some(css) = doc.sheet_value(n, att) {
        let value = doc.attr(n, att).map(str::trim).unwrap_or("none").to_string();
        if css.trim() != value {
            let id = doc.ensure_id(n);
            let sty = root_style(doc);
            append_sheet_text(doc, sty, &format!("\n#{id}{{{att}:{value}}}"));
        }
    }
    remove_inline(doc, n, att);
}

/// DH:182–194 `composed_list`: a list-valued length property (`stroke-dasharray`) in visual
/// (transformed) user units; `None` for `none`, absent, or an unparsable entry.
pub fn composed_list(doc: &Doc, n: NodeId, prop: &str) -> Option<Vec<f64>> {
    let v = doc.specified(n, prop)?;
    let v = v.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("none") {
        return None;
    }
    let sf = scale_factor(doc.composed_transform(n));
    v.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| ipx(s).map(|x| x * sf))
        .collect()
}

/// A resolved paint with its effective alpha and lightness against a white background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// `(stroke|fill)-opacity × opacity` (× the colour's own alpha for `rgba()` values).
    pub alpha: f64,
    /// `alpha·L/255 + (1 − alpha)` with `L = floor((max + min)/2)` over the channels — inkex's
    /// integer HSL lightness — so 0 is opaque black and 1 is white or fully transparent.
    pub efflightness: f64,
}

/// What upstream `get_strokefill` (DH:1171–1225) reports about an element's specified style.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrokeFill {
    pub stroke: Option<Rgba>,
    pub fill: Option<Rgba>,
    /// The paint was a `url(#…)` (gradient/pattern): the colour is `None` and this is set.
    pub stroke_is_url: bool,
    pub fill_is_url: bool,
    /// Visual (transformed) stroke width; `None` when there is no stroke colour or it is zero wide.
    pub stroke_width: Option<f64>,
    /// Visual dash lengths; `None` when absent, `none`, or there is no stroke.
    pub dasharray: Option<Vec<f64>>,
    /// Raw specified values, compared verbatim by Combine by Color.
    pub marker_start: Option<String>,
    pub marker_mid: Option<String>,
    pub marker_end: Option<String>,
}

fn rgba(c: svgtypes::Color, alpha: f64) -> Rgba {
    let alpha = alpha * f64::from(c.alpha) / 255.0;
    let max = f64::from(c.red.max(c.green).max(c.blue));
    let min = f64::from(c.red.min(c.green).min(c.blue));
    let l = ((max + min) / 2.0).floor();
    Rgba {
        r: c.red,
        g: c.green,
        b: c.blue,
        alpha,
        efflightness: alpha * l / 255.0 + (1.0 - alpha),
    }
}

/// Parses a paint value into `(colour, is_url)`. `currentColor` resolves through the specified
/// `color` (**Deviation**: upstream treats it as no paint); `none`, `inherit`, `context-*` and
/// unparsable values are no paint.
fn paint(doc: &Doc, n: NodeId, v: &str, alpha: f64) -> (Option<Rgba>, bool) {
    use svgtypes::Paint;
    match Paint::from_str(v.trim()) {
        Ok(Paint::Color(c)) => (Some(rgba(c, alpha)), false),
        Ok(Paint::CurrentColor) => {
            let c = doc
                .specified(n, "color")
                .and_then(|s| s.trim().parse::<svgtypes::Color>().ok());
            (c.map(|c| rgba(c, alpha)), false)
        }
        Ok(Paint::FuncIRI(..)) => (None, true),
        _ => (None, false),
    }
}

fn opacity_of(st: &Style, prop: &str) -> f64 {
    st.get(prop)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(1.0)
}

/// DH:1171–1225 `get_strokefill`. Defaults: stroke `none`, fill `black`, every opacity 1. A
/// zero-width or colourless stroke clears `stroke`, `stroke_width` and `dasharray` together.
pub fn strokefill(doc: &Doc, n: NodeId) -> StrokeFill {
    let sty = doc.specified_style(n);
    let op = opacity_of(&sty, "opacity");
    let (mut stroke, stroke_is_url) = paint(
        doc,
        n,
        sty.get("stroke").unwrap_or("none"),
        opacity_of(&sty, "stroke-opacity") * op,
    );
    let (fill, fill_is_url) = paint(
        doc,
        n,
        sty.get("fill").unwrap_or("black"),
        opacity_of(&sty, "fill-opacity") * op,
    );
    let mut stroke_width = Some(composed_width(doc, n, "stroke-width").tfs);
    let mut dasharray = composed_list(doc, n, "stroke-dasharray");
    if stroke.is_none() || stroke_width.is_none_or(|w| w == 0.0) {
        stroke = None;
        stroke_width = None;
        dasharray = None;
    }
    let raw = |k: &str| sty.get(k).map(|v| v.trim().to_string());
    StrokeFill {
        stroke,
        fill,
        stroke_is_url,
        fill_is_url,
        stroke_width,
        dasharray,
        marker_start: raw("marker-start"),
        marker_mid: raw("marker-mid"),
        marker_end: raw("marker-end"),
    }
}
```

If `svgtypes::Paint::from_str` is not found as an inherent method, it is the `FromStr`-like inherent constructor `Paint::from_str(text: &'a str) -> Result<Paint<'a>, svgtypes::Error>` declared in `svgtypes-0.16.1/src/paint.rs:102`; `svgtypes::Color` implements `std::str::FromStr` (`"red".parse::<svgtypes::Color>()`).

- [ ] **Step 4: Run to verify success**

Run: `cargo test --test ops_style 2>&1 | grep -E "^test result|FAILED|panicked"` → 6 passed; then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/ops/style.rs tests/ops_style.rs
git commit -m "feat(ops): compose_style, fix_css_clipmask, composed_list, strokefill"
```

---

### Task 3: `ops/bbox.rs` — bounding boxes, `bb2`, `has_bbox`/`is_drawn`, `is_rectangle`

**Files:**
- Create: `src/ops/bbox.rs` (replace the placeholder)
- Test: `tests/ops_bbox.rs` (new)

**Interfaces:**
- Consumes: Task 1 (`Ctx::ensure_char_table`, the private `Ctx.text` field — visible to child modules of `ops` — `clip_ref`, `label`, `MAX_NEST`, `cleanup::url_id`, `Doc::viewbox`), `geom::path::{shape_path, bbox_exact, bbox_rough, end_points}` (`ParsedPath { path, cmd_start }`, `cmd_start.len() == ncmds + 1`), `geom::{intersection, union, transform_rect, uniquetol, ipx}`, `text::parse::ParsedText::parse(doc: &mut Doc, el, ct: &mut CharTable, warn: &mut Warnings) -> Option<ParsedText>`, `text::layout::full_extent(&ParsedText) -> Option<Rect>` (untransformed frame, upstream `get_full_extent`), `Doc::{resolve_href, transform, composed_transform, specified}`.
- Produces:
  - `pub struct BboxOpts { pub transform: bool, pub stroke: bool, pub rough: bool, pub clip: bool }` (`Copy`, `Eq`, `Hash`), consts `VISUAL` (all true but `rough`) and `LOCAL` (`VISUAL` with `transform: false`)
  - `bbox(doc: &mut Doc, ctx: &mut Ctx, n: NodeId, o: BboxOpts) -> Option<Rect>`
  - `bb2(doc: &mut Doc, ctx: &mut Ctx, els: &[NodeId], rough: bool) -> HashMap<NodeId, Rect>`
  - `has_bbox(doc: &Doc, n) -> bool`, `is_drawn(doc: &Doc, n) -> bool`, `is_rectangle(doc: &Doc, n, including_transform: bool) -> bool`
  - `pub const UNRENDERED`, `GROUPLIKE`, `SHAPES`, `BB2_SUPPORT: &[&str]`

**Rulings (documented in Task 8):** the memo lives inside one `bbox`/`bb2` call (no cross-mutation invalidation to get wrong; `bb2` still computes every nested group once); the spec's `parsed` flag is dropped — a box computed from a fresh parse is the parsed box; `%` stroke widths and the unspecified `stroke-width` count as `0` (upstream's `"0px"` default and its silent `except`); recursion carries `depth` and stops at `MAX_NEST`.

- [ ] **Step 1: Write the failing tests**

Create `tests/ops_bbox.rs` with the preamble (keep everything except `Affine`; add the two imports below) and:

```rust
use sciink::ops::bbox::{BboxOpts, LOCAL, VISUAL, bb2, bbox, has_bbox, is_drawn, is_rectangle};
use support::with_vendored_fonts;

fn rect_near(r: Rect, x0: f64, y0: f64, x1: f64, y1: f64, tol: f64) -> bool {
    (r.x0 - x0).abs() < tol && (r.y0 - y0).abs() < tol && (r.x1 - x1).abs() < tol && (r.y1 - y1).abs() < tol
}

#[test]
fn has_bbox_and_is_drawn_follow_the_unrendered_and_container_sets() {
    let d = doc(&format!(
        r#"<svg {NS}><defs><path id="in_defs"/></defs><g id="g"><path id="p"/><path id="hidden" style="display:none"/><text id="t"><tspan id="ts">x</tspan></text></g><sodipodi:namedview id="nv"/></svg>"#
    ));
    assert!(has_bbox(&d, id(&d, "p")) && has_bbox(&d, id(&d, "g")) && has_bbox(&d, d.svg()));
    assert!(!has_bbox(&d, id(&d, "in_defs")) && !has_bbox(&d, id(&d, "ts")) && !has_bbox(&d, id(&d, "nv")));
    assert!(is_drawn(&d, id(&d, "p")) && is_drawn(&d, id(&d, "t")));
    assert!(!is_drawn(&d, id(&d, "g")), "containers are not drawn themselves");
    assert!(!is_drawn(&d, id(&d, "hidden")) && !is_drawn(&d, id(&d, "in_defs")));
}

#[test]
fn shape_boxes_with_stroke_transform_and_rough_mode() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g transform="translate(10,20)"><path id="p" d="M0 0 L10 0 L10 5 Z" style="stroke:#000;stroke-width:2"/><path id="c" d="M0 0 C 0 10 10 10 10 0"/><path id="pct" d="M0 0 L10 0 L10 5 Z" style="stroke:#000;stroke-width:10%"/><path id="defaultw" d="M0 0 L10 0 L10 5 Z" style="stroke:#000"/><path id="nostroke" d="M0 0 L10 0 L10 5 Z" style="stroke:none;stroke-width:2"/><path id="empty" d=""/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    assert!(rect_close(bbox(&mut d, &mut ctx, p, LOCAL).unwrap(), -1.0, -1.0, 11.0, 6.0));
    assert!(rect_close(bbox(&mut d, &mut ctx, p, VISUAL).unwrap(), 9.0, 19.0, 21.0, 26.0));
    let no_stroke = BboxOpts { stroke: false, ..LOCAL };
    assert!(rect_close(bbox(&mut d, &mut ctx, p, no_stroke).unwrap(), 0.0, 0.0, 10.0, 5.0));
    let c = id(&d, "c");
    let exact = bbox(&mut d, &mut ctx, c, LOCAL).unwrap();
    assert!(rect_close(exact, 0.0, 0.0, 10.0, 7.5), "tight Bézier box: {exact:?}");
    let rough = bbox(&mut d, &mut ctx, c, BboxOpts { rough: true, ..LOCAL }).unwrap();
    assert!(rect_close(rough, 0.0, 0.0, 10.0, 10.0), "control-point box: {rough:?}");
    let n_pct = id(&d, "pct");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_pct, LOCAL).unwrap(), 0.0, 0.0, 10.0, 5.0), "a % width counts as 0");
    let n_defaultw = id(&d, "defaultw");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_defaultw, LOCAL).unwrap(), 0.0, 0.0, 10.0, 5.0), "unspecified width defaults to 0px here, not 1");
    let n_nostroke = id(&d, "nostroke");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_nostroke, LOCAL).unwrap(), 0.0, 0.0, 10.0, 5.0));
    let n_empty = id(&d, "empty");
    assert_eq!(bbox(&mut d, &mut ctx, n_empty, LOCAL), None);
}

#[test]
fn line_and_image_boxes() {
    let mut d = doc(&format!(
        r#"<svg {NS} viewBox="0 0 200 100"><line id="l" x1="0" y1="0" x2="4" y2="3" style="stroke:red;stroke-width:1"/><line id="l2" x2="4" y2="-3"/><image id="i" x="10%" width="50%" height="100%"/><image id="j" x="1" y="2" width="3mm" height="4"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_l = id(&d, "l");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_l, LOCAL).unwrap(), -0.5, -0.5, 4.5, 3.5));
    let n_l2 = id(&d, "l2");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_l2, LOCAL).unwrap(), 0.0, -3.0, 4.0, 0.0));
    let n_i = id(&d, "i");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_i, LOCAL).unwrap(), 20.0, 0.0, 120.0, 100.0));
    let n_j = id(&d, "j");
    let j = bbox(&mut d, &mut ctx, n_j, LOCAL).unwrap();
    assert!(rect_near(j, 1.0, 2.0, 1.0 + 3.0 * 96.0 / 25.4, 6.0, 1e-9), "{j:?}");
}

#[test]
fn group_use_and_root_boxes() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><rect id="r" width="2" height="3" transform="scale(2)"/></defs><g id="g" transform="translate(100,0)"><rect id="a" width="2" height="2"/><rect id="b" width="1" height="1" transform="translate(5,5)"/><!-- comment --></g><use id="u" xlink:href="#r" x="1" y="1" transform="translate(10,0)"/><use id="dangling" xlink:href="#nope"/><g id="empty"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_g = id(&d, "g");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_g, LOCAL).unwrap(), 0.0, 0.0, 6.0, 6.0));
    let n_g = id(&d, "g");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_g, VISUAL).unwrap(), 100.0, 0.0, 106.0, 6.0));
    // translate(x,y) · target.transform on the target's own box, then the use's own transform
    let n_u = id(&d, "u");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_u, LOCAL).unwrap(), 1.0, 1.0, 5.0, 7.0));
    let n_u = id(&d, "u");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_u, VISUAL).unwrap(), 11.0, 1.0, 15.0, 7.0));
    let n_dangling = id(&d, "dangling");
    assert_eq!(bbox(&mut d, &mut ctx, n_dangling, LOCAL), None);
    let n_empty = id(&d, "empty");
    assert_eq!(bbox(&mut d, &mut ctx, n_empty, LOCAL), None);
    // the root is a container too; <defs> contributes nothing
    let root = d.svg();
    let whole = bbox(&mut d, &mut ctx, root, VISUAL).unwrap();
    assert!(rect_close(whole, 11.0, 0.0, 106.0, 7.0), "{whole:?}");
}

#[test]
fn clip_and_mask_clamp_the_box() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect x="5" y="5" width="15" height="15" style="stroke:#000;stroke-width:4"/></clipPath><clipPath id="emptyc"><g/></clipPath><clipPath id="far"><rect x="50" y="50" width="1" height="1"/></clipPath><mask id="m"><rect x="0" y="0" width="7" height="100"/></mask><clipPath id="selfc"><rect id="inner" clip-path="url(#selfc)" width="3" height="3"/></clipPath></defs><rect id="r" width="10" height="10" clip-path="url(#c)"/><rect id="gone" width="10" height="10" clip-path="url(#emptyc)"/><rect id="away" width="10" height="10" clip-path="url(#far)"/><rect id="both" width="10" height="10" clip-path="url(#c)" mask="url(#m)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_r = id(&d, "r");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_r, LOCAL).unwrap(), 5.0, 5.0, 10.0, 10.0), "the clip's own stroke does not count");
    let n_gone = id(&d, "gone");
    assert_eq!(bbox(&mut d, &mut ctx, n_gone, LOCAL), None, "an empty clip clips everything away");
    let n_away = id(&d, "away");
    assert_eq!(bbox(&mut d, &mut ctx, n_away, LOCAL), None, "a disjoint clip too");
    let n_both = id(&d, "both");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_both, LOCAL).unwrap(), 5.0, 5.0, 7.0, 10.0));
    let unclipped = BboxOpts { clip: false, ..LOCAL };
    let n_r = id(&d, "r");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_r, unclipped).unwrap(), 0.0, 0.0, 10.0, 10.0));
    // a clipPath child clipped by its own parent: the self-reference is ignored
    let n_inner = id(&d, "inner");
    assert!(rect_close(bbox(&mut d, &mut ctx, n_inner, LOCAL).unwrap(), 0.0, 0.0, 3.0, 3.0));
}

#[test]
fn text_boxes_come_from_the_char_table() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g transform="translate(5,0)"><text id="t" style="font-family:'DejaVu Sans';font-size:10px" x="0" y="0">Hi</text><text id="tp" style="font-family:'DejaVu Sans';font-size:10px"><textPath xlink:href="#p">on a path</textPath></text><path id="p" d="M0 0 L100 0"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let t = id(&d, "t");
    let local = with_vendored_fonts(|| bbox(&mut d, &mut ctx, t, LOCAL)).unwrap();
    assert!(close(local.x0, 0.0) && local.x1 > 8.0 && local.x1 < 20.0, "{local:?}");
    assert!((local.y0 + 7.29).abs() < 0.05 && close(local.y1, 0.0), "cap height 0.729 × 10: {local:?}");
    let visual = bbox(&mut d, &mut ctx, t, VISUAL).unwrap();
    assert!(close(visual.x0, 5.0) && close(visual.x1, local.x1 + 5.0));
    let n_tp = id(&d, "tp");
    assert_eq!(bbox(&mut d, &mut ctx, n_tp, LOCAL), None, "text on a path has no box (Plan 4 residual)");
}

#[test]
fn bb2_reports_supported_rendered_elements_only() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><rect id="in_defs" width="1" height="1"/></defs><g id="g"><path id="p" d="M0 0 L2 0 L2 2 Z"/><rect id="r" x="10" width="1" height="1"/><g id="hidden" style="display:none"><rect id="h" width="1" height="1"/></g><polyline id="pl" points="0,0 1,1"/><circle id="c" cx="5" cy="5" r="1"/><ellipse id="e" cx="5" cy="5" rx="1" ry="2"/><polygon id="pg" points="0,0 1,0 1,1"/><a id="a"><rect id="inlink" width="1" height="1"/></a></g><sodipodi:namedview id="nv"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let els: Vec<NodeId> = d.descendants(d.svg()).filter(|&n| d.is_element(n)).collect();
    let bbs = bb2(&mut d, &mut ctx, &els, false);
    for i in ["g", "p", "r", "h", "pl", "c", "e", "pg", "inlink"] {
        assert!(bbs.contains_key(&id(&d, i)), "{i} should be reported");
    }
    for i in ["in_defs", "nv", "a"] {
        assert!(!bbs.contains_key(&id(&d, i)), "{i} should not be reported");
    }
    assert!(bbs.contains_key(&d.svg()));
    // arcs become cubics (tolerance 1e-4), so round shapes are only near their exact box
    assert!(rect_near(bbs[&id(&d, "g")], 0.0, 0.0, 11.0, 7.0, 1e-3), "{:?}", bbs[&id(&d, "g")]);
    assert!(rect_near(bbs[&id(&d, "c")], 4.0, 4.0, 6.0, 6.0, 1e-3));
    assert!(rect_near(bbs[&id(&d, "e")], 4.0, 3.0, 6.0, 7.0, 1e-3));
    assert!(rect_close(bbs[&id(&d, "h")], 0.0, 0.0, 1.0, 1.0), "display:none is not bb2's concern");
}

#[test]
fn is_rectangle_cases() {
    let d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="rc"><rect width="1" height="1"/></clipPath><clipPath id="sc"><path d="M0 0 L1 0 L2 1 Z"/></clipPath><mask id="m"><rect width="1" height="1"/></mask><filter id="f"/><rect id="target" width="1" height="1"/></defs><path id="p1" d="M0 0 L10 0 L10 5 L0 5 Z"/><path id="p2" d="M0 0 h10 v5 h-10 z"/><path id="tri" d="M0 0 L10 0 L10 5 Z"/><path id="skew" d="M0 0 L10 0 L12 5 L2 5 Z"/><path id="seven" d="M0 0 L10 0 L10 5 L0 5 L0 0 L0 0 Z"/><path id="rot" d="M0 0 L10 0 L10 5 L0 5 Z" transform="rotate(45)"/><path id="scaled" d="M0 0 L10 0 L10 5 L0 5 Z" transform="scale(2,3)"/><path id="three" d="M0 0 L10 0 Z"/><rect id="r" width="2" height="1" transform="rotate(30)"/><rect id="rr" width="2" height="1" rx="0.2"/><line id="l" x2="1" y2="1"/><polyline id="pl" points="0,0 4,0 4,3 0,3 0,0"/><polygon id="pg" points="0,0 4,0 4,3 0,3"/><use id="u" xlink:href="#target"/><use id="ud" xlink:href="#nope"/><path id="masked" d="M0 0 L10 0 L10 5 L0 5 Z" mask="url(#m)"/><path id="filtered" d="M0 0 L10 0 L10 5 L0 5 Z" style="filter:url(#f)"/><path id="filtered_dangling" d="M0 0 L10 0 L10 5 L0 5 Z" style="filter:url(#nofilter)"/><path id="rectclip" d="M0 0 L10 0 L10 5 L0 5 Z" clip-path="url(#rc)"/><path id="skewclip" d="M0 0 L10 0 L10 5 L0 5 Z" clip-path="url(#sc)"/><path id="near" d="M0 0 L10 0 L10.005 5 L0 5 Z"/><path id="far" d="M0 0 L10 0 L10.02 5 L0 5 Z"/></svg>"#
    ));
    // upstream's test is "two distinct x's and two distinct y's among the end points": a right
    // triangle passes it (tri), a parallelogram does not (skew); polygons are not rect-like tags
    let yes = ["p1", "p2", "tri", "pl", "u", "ud", "filtered_dangling", "rectclip", "near"];
    let no = ["skew", "seven", "three", "l", "pg", "masked", "filtered", "skewclip", "far", "rr"];
    for i in yes {
        assert!(is_rectangle(&d, id(&d, i), true), "{i} should be a rectangle");
    }
    for i in no {
        assert!(!is_rectangle(&d, id(&d, i), true), "{i} should not be a rectangle");
    }
    assert!(!is_rectangle(&d, id(&d, "rot"), true) && is_rectangle(&d, id(&d, "rot"), false));
    assert!(is_rectangle(&d, id(&d, "scaled"), true), "axis-aligned scaling keeps it rectangular");
    assert!(!is_rectangle(&d, id(&d, "r"), true), "a rotated <rect> is not one with its transform");
    assert!(is_rectangle(&d, id(&d, "r"), false) && is_rectangle(&d, id(&d, "rr"), false), "…but a <rect> is one by definition without it");
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test ops_bbox 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 3: Implement**

Replace `src/ops/bbox.rs`:

```rust
//! Bounding boxes without Inkscape, and the rectangle test (spec §B.2 "Bounding boxes",
//! "is_rectangle"; upstream DH:1426–1552 `bounding_box2`, DH:644–659 `hasbbox`/`isdrawn`,
//! DH:663–696 `BB2`, U:180–242 `isrectangle`).

use std::collections::HashMap;

use kurbo::{Affine, Rect};

use crate::dom::{Doc, NodeId};
use crate::geom::path::{bbox_exact, bbox_rough, end_points, shape_path};
use crate::geom::{intersection, ipx, transform_rect, union, uniquetol};
use crate::text::layout::full_extent;
use crate::text::parse::ParsedText;

use super::cleanup::url_id;
use super::{ClipKind, Ctx, MAX_NEST, clip_ref, label};

/// Which box: `transform` = in root coordinates (else in the element's own frame, before its own
/// `transform`); `stroke` = grow shapes by half the stroke width; `rough` = control-point box
/// instead of the tight Bézier box; `clip` = clamp by `clip-path`/`mask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BboxOpts {
    pub transform: bool,
    pub stroke: bool,
    pub rough: bool,
    pub clip: bool,
}

/// Visual box in root coordinates (upstream's defaults).
pub const VISUAL: BboxOpts = BboxOpts {
    transform: true,
    stroke: true,
    rough: false,
    clip: true,
};
/// Visual box in the element's own frame (`dotransform=False`).
pub const LOCAL: BboxOpts = BboxOpts {
    transform: false,
    ..VISUAL
};

/// Tags Inkscape never renders, with everything under them (DH:561–583; local names).
pub const UNRENDERED: &[&str] = &[
    "namedview",
    "defs",
    "metadata",
    "foreignObject",
    "guide",
    "clipPath",
    "style",
    "tspan",
    "flowRegion",
    "flowPara",
    "mask",
    "RDF",
    "Work",
    "format",
    "type",
];
/// Containers whose box is the union of their children (DH:1414–1423, plus `a`/`switch`, spec §B.2).
pub const GROUPLIKE: &[&str] = &["svg", "g", "clipPath", "symbol", "mask", "a", "switch"];
/// Shapes `geom::path::shape_path` understands (C:489–497 `cpath_support`).
pub const SHAPES: &[&str] = &[
    "path", "rect", "circle", "ellipse", "line", "polyline", "polygon",
];
/// Tags `bb2` reports (DH:1564–1573).
pub const BB2_SUPPORT: &[&str] = &[
    "text", "flowRoot", "image", "use", "svg", "g", "path", "rect", "circle", "ellipse", "line",
    "polyline", "polygon",
];

/// DH:644–649: `n` and every ancestor is a rendered tag and the chain reaches the root `<svg>`.
pub fn has_bbox(doc: &Doc, n: NodeId) -> bool {
    let svg = doc.svg();
    let mut cur = n;
    loop {
        if cur == svg {
            return true;
        }
        if !doc.is_element(cur) || UNRENDERED.contains(&doc.tag(cur)) {
            return false;
        }
        match doc.parent(cur) {
            Some(p) => cur = p,
            None => return false,
        }
    }
}

/// DH:654–659: has a box, is not a container, and is not `display:none`.
pub fn is_drawn(doc: &Doc, n: NodeId) -> bool {
    doc.is_element(n)
        && !GROUPLIKE.contains(&doc.tag(n))
        && has_bbox(doc, n)
        && doc
            .specified(n, "display")
            .is_none_or(|v| v.trim() != "none")
}

type Memo = HashMap<(NodeId, BboxOpts), Option<Rect>>;

/// Bounding box of `n` (spec §B.2); `None` when it has no geometry or is clipped away.
pub fn bbox(doc: &mut Doc, ctx: &mut Ctx, n: NodeId, o: BboxOpts) -> Option<Rect> {
    let mut memo = Memo::new();
    bbox_rec(doc, ctx, n, o, &mut memo, 0)
}

/// DH:663–696 `BB2`: visual boxes in root coordinates of every supported element of `els` that
/// has a box, sharing one memo. Elements without a box are absent.
pub fn bb2(doc: &mut Doc, ctx: &mut Ctx, els: &[NodeId], rough: bool) -> HashMap<NodeId, Rect> {
    let mut memo = Memo::new();
    let o = BboxOpts { rough, ..VISUAL };
    let mut out = HashMap::new();
    for &n in els {
        if BB2_SUPPORT.contains(&doc.tag(n)) && has_bbox(doc, n) {
            if let Some(r) = bbox_rec(doc, ctx, n, o, &mut memo, 0) {
                out.insert(n, r);
            }
        }
    }
    out
}

fn bbox_rec(
    doc: &mut Doc,
    ctx: &mut Ctx,
    n: NodeId,
    o: BboxOpts,
    memo: &mut Memo,
    depth: usize,
) -> Option<Rect> {
    if depth > MAX_NEST {
        ctx.warn.push(format!(
            "{}: nested deeper than {MAX_NEST} levels, its bounding box is ignored",
            label(doc, n)
        ));
        return None;
    }
    if let Some(r) = memo.get(&(n, o)) {
        return *r;
    }
    let mut ret = local_bbox(doc, ctx, n, o, memo, depth);
    if ret.is_some() && o.clip {
        // DH:1525–1541: clamp by the clip's/mask's own box (no stroke), ignoring a clipPath
        // child that references its own parent
        for kind in [ClipKind::Clip, ClipKind::Mask] {
            let Some(c) = clip_ref(doc, n, kind) else {
                continue;
            };
            if doc.parent(n) == Some(c) {
                continue;
            }
            let copts = BboxOpts {
                transform: false,
                stroke: false,
                ..o
            };
            ret = match bbox_rec(doc, ctx, c, copts, memo, depth + 1) {
                Some(cb) => intersection(ret, Some(cb)),
                None => None,
            };
        }
    }
    if o.transform {
        ret = ret.map(|r| transform_rect(doc.composed_transform(n), r));
    }
    memo.insert((n, o), ret);
    ret
}

/// DH:1454–1458: the specified `stroke-width` (default `0px`, not the CSS initial `1`) when the
/// element has a paint stroke and the caller asked for it; `%` widths count as 0.
fn stroke_pad(doc: &Doc, n: NodeId, include: bool) -> f64 {
    if !include || doc.specified(n, "stroke").is_none_or(|s| s.trim() == "none") {
        return 0.0;
    }
    doc.specified(n, "stroke-width")
        .and_then(|w| ipx(&w))
        .unwrap_or(0.0)
}

/// The element's own box in its own frame, before its `transform` and before clip clamping.
fn local_bbox(
    doc: &mut Doc,
    ctx: &mut Ctx,
    n: NodeId,
    o: BboxOpts,
    memo: &mut Memo,
    depth: usize,
) -> Option<Rect> {
    let tag = doc.tag(n).to_string();
    match tag.as_str() {
        "text" | "flowRoot" => {
            ctx.ensure_char_table(doc);
            let Ctx { text, warn, .. } = ctx;
            let ct = text.as_mut().expect("built by ensure_char_table");
            let pt = ParsedText::parse(doc, n, ct, warn)?;
            full_extent(&pt)
        }
        "line" => {
            let get = |a: &str| ipx(doc.attr(n, a).unwrap_or("0"));
            let (x1, y1, x2, y2) = (get("x1")?, get("y1")?, get("x2")?, get("y2")?);
            let half = stroke_pad(doc, n, o.stroke) / 2.0;
            Some(Rect::new(
                x1.min(x2) - half,
                y1.min(y2) - half,
                x1.max(x2) + half,
                y1.max(y2) + half,
            ))
        }
        t if SHAPES.contains(&t) => {
            let pp = shape_path(doc, n)?;
            let r = if o.rough {
                bbox_rough(&pp.path)
            } else {
                bbox_exact(&pp.path)
            }?;
            let half = stroke_pad(doc, n, o.stroke) / 2.0;
            Some(r.inflate(half, half))
        }
        t if GROUPLIKE.contains(&t) => {
            let kids: Vec<NodeId> = doc.children(n).filter(|&k| doc.is_element(k)).collect();
            let mut acc = None;
            for k in kids {
                let kopts = BboxOpts {
                    transform: false,
                    ..o
                };
                if let Some(b) = bbox_rec(doc, ctx, k, kopts, memo, depth + 1) {
                    acc = union(acc, Some(transform_rect(doc.transform(k), b)));
                }
            }
            acc
        }
        "image" => {
            // C:520–541 `xywh`: `%` is a fraction of the viewBox width (x, width) or height
            let vb = doc.viewbox();
            let len = |a: &str, along_x: bool| -> Option<f64> {
                let v = doc.attr(n, a).unwrap_or("0").trim();
                match v.strip_suffix('%') {
                    Some(p) => {
                        let f = p.trim().parse::<f64>().ok()? / 100.0;
                        let vb = vb?;
                        Some(f * if along_x { vb.width() } else { vb.height() })
                    }
                    None => ipx(v),
                }
            };
            let (x, y) = (len("x", true)?, len("y", false)?);
            let (w, h) = (len("width", true)?, len("height", false)?);
            Some(Rect::new(x, y, x + w, y + h))
        }
        "use" => {
            // DH:1509–1523: the target's box (stroke and clips included) under
            // translate(x, y) · target.transform
            let target = doc.resolve_href(n)?;
            let topts = BboxOpts {
                transform: false,
                stroke: true,
                clip: true,
                rough: o.rough,
            };
            let tb = bbox_rec(doc, ctx, target, topts, memo, depth + 1)?;
            let x = ipx(doc.attr(n, "x").unwrap_or("0"))?;
            let y = ipx(doc.attr(n, "y").unwrap_or("0"))?;
            Some(transform_rect(
                Affine::translate((x, y)) * doc.transform(target),
                tb,
            ))
        }
        _ => None,
    }
}

const PATH_LETTERS: &str = "MmZzLlHhVvCcSsQqTtAa";

/// U:180–242 `isrectangle`: rect-like (`path` with 1–6 command letters, `rect`, `line`,
/// `polyline`; a `<use>` defers to its target) with at least 4 commands whose end points —
/// after the element's own `transform` when `including_transform` — take exactly two distinct
/// x's and two distinct y's (tolerance `1e-3 · max(range)`); a `<rect>` qualifies outright
/// without its transform. Rejected when masked, filtered by an existing filter, or clipped by a
/// non-rectangular clip child.
pub fn is_rectangle(doc: &Doc, n: NodeId, including_transform: bool) -> bool {
    is_rect_rec(doc, n, including_transform, 0)
}

fn is_rect_rec(doc: &Doc, n: NodeId, inc: bool, depth: usize) -> bool {
    if depth > MAX_NEST || !doc.is_element(n) {
        return false;
    }
    let tag = doc.tag(n);
    let shape_ok = if !inc && tag == "rect" {
        true
    } else if matches!(tag, "path" | "rect" | "line" | "polyline") {
        if tag == "path" {
            let letters = doc
                .attr(n, "d")
                .unwrap_or("")
                .chars()
                .filter(|c| PATH_LETTERS.contains(*c))
                .count();
            if !(1..=6).contains(&letters) {
                return false;
            }
        }
        let Some(pp) = shape_path(doc, n) else {
            return false;
        };
        if pp.cmd_start.len() < 5 {
            return false; // fewer than 4 source commands
        }
        let mut pts = end_points(&pp.path);
        if inc {
            let t = doc.transform(n);
            for p in pts.iter_mut() {
                *p = t * *p;
            }
        }
        let xs: Vec<f64> = pts.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
        let range = |v: &[f64]| {
            v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - v.iter().copied().fold(f64::INFINITY, f64::min)
        };
        let tol = 1e-3 * range(&xs).max(range(&ys));
        uniquetol(&xs, tol) == 2 && uniquetol(&ys, tol) == 2
    } else if tag == "use" {
        // upstream quirk kept: a clone of a missing target stays "rectangular"
        match doc.resolve_href(n) {
            Some(t) => is_rect_rec(doc, t, true, depth + 1),
            None => true,
        }
    } else {
        false
    };
    if !shape_ok || clip_ref(doc, n, ClipKind::Mask).is_some() {
        return false;
    }
    if doc
        .specified(n, "filter")
        .as_deref()
        .and_then(url_id)
        .and_then(|id| doc.by_id(id))
        .is_some()
    {
        return false;
    }
    if let Some(c) = clip_ref(doc, n, ClipKind::Clip) {
        let kids: Vec<NodeId> = doc.children(c).filter(|&k| doc.is_element(k)).collect();
        if kids.iter().any(|&k| !is_rect_rec(doc, k, true, depth + 1)) {
            return false;
        }
    }
    true
}
```

- [ ] **Step 4: Run to verify success**

Run: `cargo test --test ops_bbox 2>&1 | grep -E "^test result|FAILED|panicked"` → 8 passed; then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/ops/bbox.rs tests/ops_bbox.rs
git commit -m "feat(ops): bounding boxes without Inkscape (bbox, bb2, has_bbox, is_drawn) and is_rectangle"
```

---

### Task 4: `ops/clip.rs` — `compose_all`, `merge_clipmask`, `unlink`, `group`/`ungroup`, `deswitch`

**Files:**
- Create: `src/ops/clip.rs` (replace the placeholder)
- Test: `tests/ops_clip.rs` (new)

**Interfaces:**
- Consumes: Tasks 1–3 (`Ctx`, `ClipKind`, `clip_ref`, `label`, `MAX_NEST`, `style::{compose_style, fix_css_clipmask}`, `bbox::is_rectangle`), `Doc::{deep_clone, defs, ensure_id, insert_after, append_child, prepend_child, detach, children, descendants, transform, set_transform, cascaded_style, resolve_href, new_element, set_attr, remove_attr, attr, parent}`, `geom::{inverse, is_identity}`, `geom::path::{shape_path, end_points, fmt_d}`, `kurbo::{Rect, Shape}`.
- Produces:
  - `compose_all(doc, ctx, el, clip: Option<NodeId>, mask: Option<NodeId>, t: Affine, style: Option<&Style>, remove_text_clip: bool) -> bool`
  - `merge_clipmask(doc, ctx, node, newclip: NodeId, kind: ClipKind, depth: usize) -> bool`
  - `duplicate_into_defs(doc, ctx, n) -> NodeId`
  - `unlink(doc, ctx, u: NodeId) -> Option<NodeId>`
  - `group(doc, els: &[NodeId]) -> NodeId`, `ungroup(doc, ctx, g, remove_text_clip: bool)`
  - `deswitch(doc, ctx, sw, lang: &str)`, `lang_matches(attr: &str, lang: &str) -> bool`, `preferences_language(xml: &str) -> Option<String>`, `ui_language() -> String`
  - `pub const TEXT_TAGS: &[&str] = &["text", "flowRoot"]`, `pub const UNUNGROUPABLE: &[&str] = &["namedview", "defs", "metadata", "foreignObject"]`

**Deviations (documented in Task 8):** (a) duplicated clips/masks go to the root `<defs>` (upstream: next to the original); (b) `deswitch` matches a `systemLanguage` token when it equals the UI language case-insensitively or shares its primary subtag (`en-US` ~ `en`; upstream: exact string equality, which fails on every regional tag); (c) a singular node transform leaves the new clip un-counter-transformed with a warning (upstream raises); (d) `unlink` follows nested clones iteratively with a 10 000-step guard (a symbol that clones itself).

- [ ] **Step 1: Write the failing tests**

Create `tests/ops_clip.rs` with the preamble (keep everything but `Rect`/`rect_close`) and:

```rust
use sciink::ops::ClipKind;
use sciink::ops::clip::{
    compose_all, deswitch, group, lang_matches, merge_clipmask, preferences_language, ungroup,
    unlink,
};
use sciink::style::Style;

fn clip_target(d: &Doc, n: NodeId) -> NodeId {
    let v = d.attr(n, "clip-path").expect("clip-path attribute");
    id(d, v.trim_start_matches("url(#").trim_end_matches(')'))
}

#[test]
fn compose_all_pushes_style_transform_and_clip_onto_a_child() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><g id="g" transform="translate(1,2)" style="fill:red"><path id="p" d="M0 0h1" transform="scale(2)" style="fill:blue"/><path id="q" d="M0 0h1"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (g, p, q, c) = (id(&d, "g"), id(&d, "p"), id(&d, "q"), id(&d, "c"));
    let gst = d.cascaded_style(g);
    let t = d.transform(g);
    assert!(!compose_all(&mut d, &mut ctx, p, None, None, t, Some(&gst), false), "no clip → never clipped out");
    assert_eq!(d.attr(p, "transform"), Some("matrix(2,0,0,2,1,2)"), "group transform first, then the child's own");
    assert_eq!(d.attr(p, "clip-path"), None);
    assert_eq!(Style::parse(d.attr(p, "style").unwrap()).get("fill"), Some("blue"));
    // an untransformed child simply points at the group's clip
    assert!(!compose_all(&mut d, &mut ctx, q, Some(c), None, t, Some(&gst), false), "a clip that was merely attached never clips out");
    assert_eq!(d.attr(q, "clip-path"), Some("url(#c)"));
    assert_eq!(d.attr(q, "transform"), Some("translate(1,2)"));
    assert_eq!(Style::parse(d.attr(q, "style").unwrap()).get("fill"), Some("red"));
    // identity transform: attribute untouched; text with remove_text_clip loses its clips
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><text id="t" clip-path="url(#c)" mask="url(#c)">x</text></svg>"#
    ));
    let t = id(&d, "t");
    let c = id(&d, "c");
    assert!(!compose_all(&mut d, &mut ctx, t, Some(c), None, Affine::IDENTITY, None, true));
    assert_eq!((d.attr(t, "clip-path"), d.attr(t, "mask"), d.attr(t, "transform")), (None, None, None));
}

#[test]
fn merge_clipmask_attaches_a_new_clip_or_intersects_rectangles() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c1"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="c2"><path d="M5 5 L20 5 L20 20 L5 20 Z"/></clipPath><clipPath id="c3"><rect x="20" y="20" width="5" height="5"/></clipPath></defs><rect id="plain" width="1" height="1"/><rect id="clipped" width="1" height="1" clip-path="url(#c1)"/><rect id="gone" width="1" height="1" clip-path="url(#c1)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (c1, c2, c3) = (id(&d, "c1"), id(&d, "c2"), id(&d, "c3"));
    // no existing clip: point at the new one
    let plain = id(&d, "plain");
    assert!(!merge_clipmask(&mut d, &mut ctx, plain, c2, ClipKind::Clip, 0));
    assert_eq!(d.attr(plain, "clip-path"), Some("url(#c2)"));
    assert!(ctx.created.is_empty(), "nothing duplicated");
    // existing rectangular clip ∩ new rectangular clip → one path with the intersection box
    let clipped = id(&d, "clipped");
    assert!(!merge_clipmask(&mut d, &mut ctx, clipped, c2, ClipKind::Clip, 0));
    let dup = clip_target(&d, clipped);
    assert_ne!(dup, c1, "the original clip is untouched");
    assert_eq!(ctx.created, vec![dup]);
    assert_eq!(d.parent(dup), Some(d.defs()));
    let k = kids(&d, dup);
    assert_eq!(k.len(), 1);
    assert_eq!(d.tag(k[0]), "path");
    assert_eq!(d.attr(k[0], "d"), Some("M 5,5 L 10,5 L 10,10 L 5,10 Z"));
    assert_eq!(kids(&d, c1).len(), 1, "c1 still has its rect");
    assert_eq!(d.tag(kids(&d, c1)[0]), "rect");
    // disjoint rectangles → the child goes and the node is reported clipped out
    let gone = id(&d, "gone");
    assert!(merge_clipmask(&mut d, &mut ctx, gone, c3, ClipKind::Clip, 0));
    let dup2 = clip_target(&d, gone);
    assert!(kids(&d, dup2).is_empty());
    assert_eq!(ctx.created.len(), 2);
}

#[test]
fn merge_clipmask_counter_transforms_the_new_clip_and_nests_non_rectangles() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="tri"><path d="M0 0 L10 0 L12 10 Z"/></clipPath><mask id="m"><rect width="5" height="5"/></mask></defs><rect id="moved" width="1" height="1" transform="translate(10,0)"/><rect id="odd" width="1" height="1" clip-path="url(#tri)"/><rect id="masked" width="1" height="1" mask="url(#m)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (c, tri, m) = (id(&d, "c"), id(&d, "tri"), id(&d, "m"));
    // a transformed node: a copy of the new clip gets the inverse transform on its children
    let moved = id(&d, "moved");
    assert!(!merge_clipmask(&mut d, &mut ctx, moved, c, ClipKind::Clip, 0));
    let dup = clip_target(&d, moved);
    assert_ne!(dup, c);
    assert_eq!(d.attr(kids(&d, dup)[0], "transform"), Some("translate(-10,0)"));
    assert_eq!(d.attr(kids(&d, c)[0], "transform"), None);
    // a non-rectangular existing clip: its children are clipped by the new clip recursively
    let odd = id(&d, "odd");
    assert!(!merge_clipmask(&mut d, &mut ctx, odd, c, ClipKind::Clip, 0));
    let dup = clip_target(&d, odd);
    assert_ne!(dup, tri);
    let inner = kids(&d, dup)[0];
    assert_eq!(d.tag(inner), "path");
    assert_eq!(d.attr(inner, "clip-path"), Some("url(#c)"));
    // masks are never rectangle-intersected
    let masked = id(&d, "masked");
    assert!(!merge_clipmask(&mut d, &mut ctx, masked, c, ClipKind::Mask, 0));
    let v = d.attr(masked, "mask").unwrap().to_string();
    let dup = id(&d, v.trim_start_matches("url(#").trim_end_matches(')'));
    assert_ne!(dup, m);
    assert_eq!(d.attr(kids(&d, dup)[0], "mask"), Some("url(#c)"));
    assert_eq!(ctx.created.len(), 3);
}

#[test]
fn merge_clipmask_unlinks_clones_inside_clips_and_stops_at_max_nest() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><rect id="src" width="10" height="10"/><clipPath id="c"><use xlink:href="#src"/></clipPath><clipPath id="loop"><rect id="lr" width="1" height="1" clip-path="url(#loop)"/></clipPath></defs><rect id="r" width="1" height="1"/><rect id="cyc" width="3" height="3" clip-path="url(#loop)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let c = id(&d, "c");
    let n_r = id(&d, "r");
    assert!(!merge_clipmask(&mut d, &mut ctx, n_r, c, ClipKind::Clip, 0));
    assert_eq!(d.tag(kids(&d, c)[0]), "rect", "the <use> child of the clip was unlinked in place");
    assert!(d.by_id("src").is_some());
    // a self-referencing clip chain cannot overflow the stack
    let loop_ = id(&d, "loop");
    let n_cyc = id(&d, "cyc");
    assert!(!merge_clipmask(&mut d, &mut ctx, n_cyc, loop_, ClipKind::Clip, 0));
    assert!(ctx.warn.0.iter().any(|w| w.contains("64 levels")), "{:?}", ctx.warn.0);
}

#[test]
fn unlink_replaces_a_clone_with_a_composed_copy() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><g id="sym"><rect id="r" width="1" height="1" style="fill:red"/><use id="nested" xlink:href="#r" x="2"/></g><symbol id="s"><circle id="c" r="1"/></symbol><clipPath id="cp"><rect width="1" height="1"/></clipPath></defs><use id="u" xlink:href="#sym" x="3" y="4" transform="scale(2)" style="opacity:0.5" clip-path="url(#cp)"/><use id="dangling" xlink:href="#nope"/><use id="us" xlink:href="#s" transform="translate(1,1)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let u = id(&d, "u");
    let copy = unlink(&mut d, &mut ctx, u).unwrap();
    assert_eq!(d.tag(copy), "g");
    assert_eq!(d.attr(copy, "id"), Some("u"), "the copy takes the clone's id");
    assert_eq!(d.attr(copy, "unlinked_clone"), Some("True"));
    assert_eq!(d.attr(copy, "transform"), Some("matrix(2,0,0,2,6,8)"), "scale(2) · translate(3,4)");
    assert_eq!(d.attr(copy, "style"), Some("opacity:0.5"));
    // the copy carried translate(3,4) when the clip was merged, so the clip is a
    // counter-transformed duplicate, not `cp` itself
    let cpv = d.attr(copy, "clip-path").expect("clip-path").to_string();
    let cdup = id(&d, cpv.trim_start_matches("url(#").trim_end_matches(')'));
    assert_ne!(cdup, id(&d, "cp"));
    assert_eq!(d.attr(kids(&d, cdup)[0], "transform"), Some("translate(-3,-4)"));
    let k = kids(&d, copy);
    assert_eq!(k.len(), 2);
    assert_eq!(d.tag(k[0]), "rect");
    assert_eq!(d.attr(k[0], "id"), None, "descendants of a copy carry no ids");
    assert_eq!(d.tag(k[1]), "rect", "the nested clone inside the copy was unlinked too");
    assert_eq!(d.attr(k[1], "transform"), Some("translate(2,0)"));
    assert!(d.by_id("sym").is_some() && d.by_id("r").is_some() && d.by_id("nested").is_some(), "originals stay");
    assert_eq!(d.parent(copy), Some(d.svg()));
    assert!(!out(&d).contains(r#"<use id="u""#));
    // a clone of nothing is deleted
    let n_dangling = id(&d, "dangling");
    assert_eq!(unlink(&mut d, &mut ctx, n_dangling), None);
    assert_eq!(d.by_id("dangling"), None);
    // a symbol becomes a group (Inkscape's Unlink Clone behaviour)
    let n_us = id(&d, "us");
    let g = unlink(&mut d, &mut ctx, n_us).unwrap();
    assert_eq!(d.tag(g), "g");
    assert_eq!(d.attr(g, "transform"), Some("translate(1,1)"));
    assert_eq!(d.tag(kids(&d, g)[0]), "circle");
    assert_eq!(out(&d).matches("<symbol").count(), 1, "only the original symbol remains: {}", out(&d));
    assert!(d.by_id("s").is_some(), "…in defs, untouched");
}

#[test]
fn group_wraps_elements_in_place() {
    let mut d = doc(&format!(r#"<svg {NS}><path id="a"/><path id="b"/><path id="c"/></svg>"#));
    let (a, c) = (id(&d, "a"), id(&d, "c"));
    let g = group(&mut d, &[a, c]);
    assert_eq!(out(&d), format!(r#"<svg {NS}><g><path id="a"/><path id="c"/></g><path id="b"/></svg>"#));
    assert_eq!(d.parent(a), Some(g));
    let empty = group(&mut d, &[]);
    assert_eq!(d.parent(empty), None, "an empty group is returned detached");
}

#[test]
fn ungroup_composes_onto_children_in_order_and_keeps_unungroupables() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="g" transform="translate(1,1)" style="fill:red;opacity:0.5" xml:space="preserve"><!-- note --><path id="a" d="M0 0h1"/><path id="b" d="M0 0h1" transform="scale(2)" style="fill:blue" xml:space="default"/><defs id="dd"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let g = id(&d, "g");
    ungroup(&mut d, &mut ctx, g, false);
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    let svg_kids = kids(&d, d.svg());
    assert_eq!(svg_kids, vec![g, a, b], "children follow the group in their original order");
    assert_eq!(kids(&d, g), vec![id(&d, "dd")], "<defs> stays inside, so the group survives");
    assert!(!out(&d).contains("<!-- note -->"), "comments are dropped");
    assert_eq!(d.attr(a, "transform"), Some("translate(1,1)"));
    assert_eq!(d.attr(b, "transform"), Some("matrix(2,0,0,2,1,1)"));
    let sa = Style::parse(d.attr(a, "style").unwrap());
    assert_eq!((sa.get("fill"), sa.get("opacity")), (Some("red"), Some("0.5")));
    let sb = Style::parse(d.attr(b, "style").unwrap());
    assert_eq!((sb.get("fill"), sb.get("opacity")), (Some("blue"), Some("0.5")));
    assert_eq!(d.attr(a, "xml:space"), Some("preserve"));
    assert_eq!(d.attr(b, "xml:space"), Some("default"), "an own xml:space wins");
    // an emptied group disappears; a clipped-out child is deleted, not moved
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c1"><rect width="10" height="10"/></clipPath><clipPath id="c2"><rect x="20" y="20" width="1" height="1"/></clipPath></defs><g id="g" clip-path="url(#c1)"><path id="keep" d="M0 0h1"/><path id="drop" d="M0 0h1" clip-path="url(#c2)"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_g = id(&d, "g");
    ungroup(&mut d, &mut ctx, n_g, false);
    assert_eq!(d.by_id("g"), None);
    assert_eq!(d.by_id("drop"), None);
    let keep = id(&d, "keep");
    assert_eq!(d.attr(keep, "clip-path"), Some("url(#c1)"));
    assert_eq!(d.parent(keep), Some(d.svg()));
}

#[test]
fn deswitch_keeps_the_language_match_and_language_helpers_work() {
    assert!(lang_matches("en", "en") && lang_matches("en-US", "en") && lang_matches("de, en-GB", "en"));
    assert!(lang_matches("EN", "en") && lang_matches("en", "en_US"));
    assert!(!lang_matches("de", "en") && !lang_matches("", "en"));
    assert_eq!(
        preferences_language(r#"<inkscape version="1"><group id="options"/><group foo="1" id="ui" language="de" bar="2"/></inkscape>"#),
        Some("de".to_string())
    );
    assert_eq!(preferences_language(r#"<inkscape><group id="ui" language=""/></inkscape>"#), None);
    assert_eq!(preferences_language(r#"<inkscape><group id="ui"/></inkscape>"#), None);
    let src = format!(
        r#"<svg {NS}><switch id="s" transform="translate(1,0)"><text id="de" systemLanguage="de">Hallo</text><text id="en" systemLanguage="en-US">Hello</text><text id="x">Fallback</text></switch></svg>"#
    );
    let mut d = doc(&src);
    let mut ctx = Ctx::new();
    let n_s = id(&d, "s");
    deswitch(&mut d, &mut ctx, n_s, "en");
    assert_eq!(d.by_id("s"), None);
    assert_eq!((d.by_id("de"), d.by_id("x")), (None, None));
    let en = id(&d, "en");
    assert_eq!(d.attr(en, "systemLanguage"), None);
    assert_eq!(d.attr(en, "transform"), Some("translate(1,0)"));
    assert_eq!(d.parent(en), Some(d.svg()));
    // no match: the attribute-less child is the survivor; nothing matches at all: the first
    let mut d = doc(&src);
    let n_s = id(&d, "s");
    deswitch(&mut d, &mut ctx, n_s, "fr");
    assert!(d.by_id("x").is_some() && d.by_id("en").is_none() && d.by_id("de").is_none());
    let mut d = doc(&format!(r#"<svg {NS}><switch id="s"><text id="de" systemLanguage="de">a</text><text id="it" systemLanguage="it">b</text></switch></svg>"#));
    let n_s = id(&d, "s");
    deswitch(&mut d, &mut ctx, n_s, "fr");
    assert!(d.by_id("de").is_some() && d.by_id("it").is_none());
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test ops_clip 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 3: Implement**

Replace `src/ops/clip.rs`:

```rust
//! Composing a container's properties onto its children, clip/mask merging, ungrouping,
//! unlinking clones, language switches (spec §B.2; upstream DH:238–352 `unlink2`/`ungroup`/
//! `group`/`deswitch`, DH:359–391 `compose_all`, DH:416–504 clip merging).

use kurbo::{Affine, Rect, Shape};

use crate::dom::{Doc, NodeId};
use crate::geom::path::{end_points, fmt_d, shape_path};
use crate::geom::{inverse, is_identity};
use crate::style::Style;

use super::bbox::is_rectangle;
use super::style::{compose_style, fix_css_clipmask};
use super::{ClipKind, Ctx, MAX_NEST, clip_ref, label};

pub const TEXT_TAGS: &[&str] = &["text", "flowRoot"];
/// Children `ungroup` leaves inside the group (DH:285–288).
pub const UNUNGROUPABLE: &[&str] = &["namedview", "defs", "metadata", "foreignObject"];

fn element_children(doc: &Doc, n: NodeId) -> Vec<NodeId> {
    doc.children(n).filter(|&k| doc.is_element(k)).collect()
}

/// DH:359–391. Style first (a CSS clip may depend on it), then clip and mask, then the
/// transform is prepended to the element's own. Returns whether the given `clip` clipped the
/// element out entirely (`false` whenever `clip` is `None`).
#[allow(clippy::too_many_arguments)]
pub fn compose_all(
    doc: &mut Doc,
    ctx: &mut Ctx,
    el: NodeId,
    clip: Option<NodeId>,
    mask: Option<NodeId>,
    t: Affine,
    style: Option<&Style>,
    remove_text_clip: bool,
) -> bool {
    if let Some(st) = style {
        compose_style(doc, el, st);
    }
    let mut cout = false;
    if remove_text_clip && TEXT_TAGS.contains(&doc.tag(el)) {
        doc.remove_attr(el, "clip-path");
        doc.remove_attr(el, "mask");
    } else {
        if let Some(c) = clip {
            cout = merge_clipmask(doc, ctx, el, c, ClipKind::Clip, 0);
        }
        if let Some(m) = mask {
            merge_clipmask(doc, ctx, el, m, ClipKind::Mask, 0);
        }
        if clip.is_some() {
            fix_css_clipmask(doc, el, ClipKind::Clip);
        }
        if mask.is_some() {
            fix_css_clipmask(doc, el, ClipKind::Mask);
        }
    }
    if !is_identity(t) {
        let own = doc.transform(el);
        doc.set_transform(el, t * own);
    }
    clip.is_some() && cout
}

/// A copy of `n` appended to the root `<defs>` with a fresh id, recorded in `ctx.created`.
pub fn duplicate_into_defs(doc: &mut Doc, ctx: &mut Ctx, n: NodeId) -> NodeId {
    let d = doc.deep_clone(n);
    let defs = doc.defs();
    doc.append_child(defs, d);
    doc.ensure_id(d);
    ctx.created.push(d);
    d
}

/// Axis-aligned box of a shape's end points after its own transform.
fn end_point_box(doc: &Doc, n: NodeId) -> Option<Rect> {
    let pp = shape_path(doc, n)?;
    let t = doc.transform(n);
    let mut r: Option<Rect> = None;
    for p in end_points(&pp.path) {
        let q = t * p;
        r = Some(r.map_or(Rect::from_points(q, q), |r| r.union_pt(q)));
    }
    r
}

/// DH:416–443 `intersect_paths` + `compose_clips`: replace the rectangular clip child `k` by the
/// intersection of its box with the new rectangular clip's box (a path appended to `d`), or
/// delete it when the boxes do not overlap. Returns `true` when clipped out.
fn compose_clips(doc: &mut Doc, d: NodeId, k: NodeId, newrect: NodeId) -> bool {
    let inter = match (end_point_box(doc, newrect), end_point_box(doc, k)) {
        (Some(a), Some(b)) => Rect::new(
            a.x0.max(b.x0),
            a.y0.max(b.y0),
            a.x1.min(b.x1),
            a.y1.min(b.y1),
        ),
        _ => Rect::ZERO,
    };
    let keep = inter.width() > 0.0 && inter.height() > 0.0;
    if keep {
        let p = doc.new_element("path");
        doc.set_attr(p, "d", fmt_d(&inter.to_path(0.1)));
        doc.append_child(d, p);
    }
    doc.detach(k);
    !keep
}

/// DH:445–504 (from Deep Ungroup): apply `newclip` to `node` on top of any clip/mask it already
/// carries. Returns `true` when the composition proves the node fully clipped out.
///
/// 1. A transformed node: clips live in the node's user space, so a copy of the new clip gets
///    the inverse transform on its children (recorded for gc).
/// 2. `<use>` children of the new clip are unlinked in place.
/// 3. No existing clip → point at the new one. Otherwise duplicate the existing clip, point the
///    node at the duplicate and compose child by child: rectangle ∩ rectangle becomes one box
///    (clips only, never masks), anything else is clipped recursively.
pub fn merge_clipmask(
    doc: &mut Doc,
    ctx: &mut Ctx,
    node: NodeId,
    newclip: NodeId,
    kind: ClipKind,
    depth: usize,
) -> bool {
    if depth > MAX_NEST {
        ctx.warn.push(format!(
            "{}: clips nested deeper than {MAX_NEST} levels are not merged",
            label(doc, node)
        ));
        return false;
    }
    let mut newclip = newclip;
    let own = doc.transform(node);
    if !is_identity(own) {
        match inverse(own) {
            Some(inv) => {
                let d = duplicate_into_defs(doc, ctx, newclip);
                for k in element_children(doc, d) {
                    compose_all(doc, ctx, k, None, None, inv, None, false);
                }
                newclip = d;
            }
            None => ctx.warn.push(format!(
                "{}: singular transform, its clip is applied untransformed",
                label(doc, node)
            )),
        }
    }
    for u in element_children(doc, newclip) {
        if doc.tag(u) == "use" {
            unlink(doc, ctx, u);
        }
    }
    let Some(old) = clip_ref(doc, node, kind) else {
        let id = doc.ensure_id(newclip);
        doc.set_attr(node, kind.attr(), format!("url(#{id})"));
        return false;
    };
    for u in element_children(doc, old) {
        if doc.tag(u) == "use" {
            unlink(doc, ctx, u);
        }
    }
    let d = duplicate_into_defs(doc, ctx, old);
    let id = doc.ensure_id(d);
    doc.set_attr(node, kind.attr(), format!("url(#{id})"));
    let new_kids = element_children(doc, newclip);
    let newclip_is_rect = new_kids.len() == 1 && is_rectangle(doc, new_kids[0], true);
    let mut all_out = true; // `all([])` is true
    for k in element_children(doc, d).into_iter().rev() {
        let cout = if newclip_is_rect && kind == ClipKind::Clip && is_rectangle(doc, k, true) {
            compose_clips(doc, d, k, new_kids[0])
        } else {
            merge_clipmask(doc, ctx, k, newclip, kind, depth + 1)
        };
        all_out &= cout;
    }
    all_out
}

/// DH:238–282 `unlink2`: replace a `<use>` by a copy of its target that takes the clone's place
/// and id, composed with `translate(x, y)` first, then the clone's clip, mask, transform and
/// cascaded style; nested clones inside the copy are unlinked too. A `<symbol>` copy becomes a
/// `<g>` (Inkscape's Unlink Clone). Returns the replacement, or `None` for a clone of a missing
/// target (the clone is then deleted). Non-`<use>` elements are returned unchanged.
pub fn unlink(doc: &mut Doc, ctx: &mut Ctx, u: NodeId) -> Option<NodeId> {
    if doc.tag(u) != "use" {
        return Some(u);
    }
    let mut result: Option<NodeId> = None;
    let mut work = vec![u];
    let mut steps = 0usize;
    while let Some(u) = work.pop() {
        steps += 1;
        if steps > 10_000 {
            ctx.warn.push("clone chain too long (a symbol cloning itself?), unlinking stopped".to_string());
            break;
        }
        let Some(target) = doc.resolve_href(u) else {
            doc.detach(u);
            continue;
        };
        let d = doc.deep_clone(target);
        doc.insert_after(d, u);
        let x = doc.attr(u, "x").and_then(crate::geom::ipx).unwrap_or(0.0);
        let y = doc.attr(u, "y").and_then(crate::geom::ipx).unwrap_or(0.0);
        compose_all(doc, ctx, d, None, None, Affine::translate((x, y)), None, false);
        let clip = clip_ref(doc, u, ClipKind::Clip);
        let mask = clip_ref(doc, u, ClipKind::Mask);
        let st = doc.cascaded_style(u);
        let t = doc.transform(u);
        compose_all(doc, ctx, d, clip, mask, t, Some(&st), false);
        let id = doc.attr(u, "id").map(str::to_string);
        doc.detach(u);
        if let Some(id) = id {
            doc.set_attr(d, "id", id);
        }
        doc.set_attr(d, "unlinked_clone", "True");
        let mut d = d;
        if doc.tag(d) == "symbol" {
            let g = group(doc, &element_children(doc, d));
            ungroup(doc, ctx, d, false);
            d = g;
        }
        // nested clones inside the copy (the copy itself is never a <use>: its target was not)
        let nested: Vec<NodeId> = doc
            .descendants(d)
            .filter(|&k| k != d && doc.is_element(k) && doc.tag(k) == "use")
            .collect();
        work.extend(nested);
        if result.is_none() {
            result = Some(d);
        }
    }
    result
}

/// DH:320–338: wraps `els` in a new `<g>` placed right after the first element; an empty list
/// returns a detached group.
pub fn group(doc: &mut Doc, els: &[NodeId]) -> NodeId {
    let g = doc.new_element("g");
    if let Some(&first) = els.first() {
        doc.insert_after(g, first);
        for &e in els {
            doc.append_child(g, e);
        }
    }
    g
}

/// DH:292–316: dissolves `g`, composing its transform, clip, mask and cascaded style onto every
/// element child (`<defs>`/metadata/namedview/foreignObject stay inside; comments are removed;
/// text nodes are left where they are). Children the clip proves clipped out are deleted; the
/// rest follow the group in document order. The group is deleted once it has no element or
/// comment children left.
pub fn ungroup(doc: &mut Doc, ctx: &mut Ctx, g: NodeId, remove_text_clip: bool) {
    let gt = doc.transform(g);
    let gclip = clip_ref(doc, g, ClipKind::Clip);
    let gmask = clip_ref(doc, g, ClipKind::Mask);
    let gstyle = doc.cascaded_style(g);
    let gspace = doc.attr(g, "xml:space").map(str::to_string);
    let kids: Vec<NodeId> = doc.children(g).collect();
    for k in kids.into_iter().rev() {
        if doc.is_comment(k) {
            doc.detach(k);
            continue;
        }
        if !doc.is_element(k) || UNUNGROUPABLE.contains(&doc.tag(k)) {
            continue;
        }
        let cout = compose_all(doc, ctx, k, gclip, gmask, gt, Some(&gstyle), remove_text_clip);
        if let Some(sp) = &gspace {
            if doc.attr(k, "xml:space").is_none() {
                doc.set_attr(k, "xml:space", sp.clone());
            }
        }
        if cout {
            doc.detach(k);
        } else {
            doc.insert_after(k, g);
        }
    }
    if !doc.children(g).any(|c| doc.is_element(c) || doc.is_comment(c)) {
        doc.detach(g);
    }
}

/// `systemLanguage` matching. **Deviation:** any comma-separated token equal to `lang`
/// (case-insensitive) or sharing its primary subtag (`en-US` ~ `en`) matches; upstream compares
/// the raw strings.
pub fn lang_matches(attr: &str, lang: &str) -> bool {
    let primary = |s: &str| {
        s.trim()
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let want = lang.trim().to_ascii_lowercase();
    if want.is_empty() {
        return false;
    }
    attr.split(',').any(|tok| {
        let tok = tok.trim();
        !tok.is_empty() && (tok.eq_ignore_ascii_case(&want) || primary(tok) == primary(&want))
    })
}

/// The `language` of `<group id="ui">` in Inkscape's `preferences.xml` (U:396–443), if set.
pub fn preferences_language(xml: &str) -> Option<String> {
    for tag in xml.split('<').skip(1) {
        let tag = tag.split('>').next()?;
        if !tag.trim_start().starts_with("group") || !tag.contains(r#"id="ui""#) {
            continue;
        }
        let rest = tag.split("language=\"").nth(1)?;
        let lang = rest.split('"').next()?.trim();
        return (!lang.is_empty()).then(|| lang.to_string());
    }
    None
}

/// Inkscape's UI language: `preferences.xml` under `INKSCAPE_PROFILE_DIR` (exported by Inkscape
/// to its extensions), else the locale variables' primary subtag, else `en`.
pub fn ui_language() -> String {
    if let Some(dir) = std::env::var_os("INKSCAPE_PROFILE_DIR") {
        let p = std::path::Path::new(&dir).join("preferences.xml");
        if let Some(l) = std::fs::read_to_string(p)
            .ok()
            .and_then(|s| preferences_language(&s))
        {
            return l;
        }
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            let primary = v.split(['_', '.', '-', '@']).next().unwrap_or("").trim();
            if !primary.is_empty() && primary != "C" && primary != "POSIX" {
                return primary.to_string();
            }
        }
    }
    "en".to_string()
}

/// DH:341–352: keep the `<switch>` children whose `systemLanguage` matches `lang` (children
/// without the attribute always match), never deleting the last one; then keep only the first,
/// strip its `systemLanguage`, and ungroup the switch.
pub fn deswitch(doc: &mut Doc, ctx: &mut Ctx, sw: NodeId, lang: &str) {
    let kids = element_children(doc, sw);
    let mut remaining = kids.len();
    for &k in kids.iter().rev() {
        let matches = doc
            .attr(k, "systemLanguage")
            .is_none_or(|v| lang_matches(v, lang));
        if !matches && remaining > 1 {
            doc.detach(k);
            remaining -= 1;
        }
    }
    let kids = element_children(doc, sw);
    for &k in kids.iter().skip(1) {
        doc.detach(k);
    }
    if let Some(&first) = kids.first() {
        doc.remove_attr(first, "systemLanguage");
    }
    ungroup(doc, ctx, sw, false);
}
```

`Rect::ZERO`, `Rect::from_points`, `Rect::union_pt`, `Rect::inflate` and `Shape::to_path` are kurbo 0.13 API; `Affine::translate` accepts `(f64, f64)`.

- [ ] **Step 4: Run to verify success**

Run: `cargo test --test ops_clip 2>&1 | grep -E "^test result|FAILED|panicked"` → 8 passed; then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/ops/clip.rs tests/ops_clip.rs
git commit -m "feat(ops): compose_all, clip/mask merging, unlink, group/ungroup, deswitch"
```

---

### Task 5: `ops/xform.rs` — `object_to_path`, `fuse`, `global_transform`

**Files:**
- Create: `src/ops/xform.rs` (replace the placeholder; Task 6 appends `combine_paths`)
- Test: `tests/ops_xform.rs` (new)

**Interfaces:**
- Consumes: Tasks 1–4 (`Ctx`, `ClipKind`, `clip_ref`, `label`, `clip::duplicate_into_defs`, `style::{fix_css_clipmask, composed_list}`), `Doc::{set_tag, set_transform, transform, composed_transform, attrs, set_style, set_style_map, specified, deep_clone, defs, ensure_id}`, `geom::{inverse, ipx, is_identity, parse_transform, fmt_transform, scale_factor, TOL}`, `geom::path::{shape_path, end_points, fmt_d}`, `text::style::composed_width`, `kurbo::{Affine, BezPath, PathEl, Point}`.
- Produces:
  - `pub const OTP_SUPPORT: &[&str]`
  - `pub type Ranges = Vec<(std::ops::Range<usize>, Affine)>`
  - `object_to_path(doc, el)`
  - `fuse(doc, ctx, el, extra: Affine, ranges: Option<&Ranges>, apply_to_stroke: bool)`
  - `global_transform(doc, ctx, el, t: Affine, ranges: Option<Ranges>, preserve_stroke: bool)`

**Deviations (documented in Task 8):** (a) `object_to_path` removes the shape attributes it converted (upstream leaves `x1`, `points`, … behind on the new `<path>`); (b) rect radii are scaled by `|a|`, `|d|` (spec §B.2); (c) an inherited paint stroke gets an explicit `stroke-width` on fuse (spec §B.2); (d) `global_transform` rewrites `stroke-width`/`stroke-dasharray` only when the restored width differs from the current specified one (spec §B.2: upstream always writes); (e) the shear test uses `geom::TOL` instead of exact zero; (f) gradients are duplicated only when `gradientUnits="userSpaceOnUse"` (spec §B.2; upstream duplicates every gradient).

- [ ] **Step 1: Write the failing tests**

Create `tests/ops_xform.rs` with the preamble (keep everything but `Rect`/`rect_close`) and:

```rust
use sciink::ops::xform::{Ranges, fuse, global_transform, object_to_path};
use sciink::style::Style;

fn style_of(d: &Doc, n: NodeId) -> Style {
    d.attr(n, "style").map(Style::parse).unwrap_or_default()
}

#[test]
fn object_to_path_converts_shapes_and_drops_their_attributes() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="r" x="1" y="2" width="3" height="4" style="fill:red"/><line id="l" x1="0" y1="0" x2="1" y2="1"/><path id="p" d="M0 0h1"/><g id="g"/></svg>"#
    ));
    let r = id(&d, "r");
    object_to_path(&mut d, r);
    assert_eq!(d.tag(r), "path");
    assert_eq!(d.attr(r, "d"), Some("M 1,2 L 4,2 L 4,6 L 1,6 Z"));
    assert_eq!((d.attr(r, "x"), d.attr(r, "width"), d.attr(r, "style")), (None, None, Some("fill:red")));
    let l = id(&d, "l");
    object_to_path(&mut d, l);
    assert_eq!((d.tag(l), d.attr(l, "d"), d.attr(l, "x1")), ("path", Some("M 0,0 L 1,1"), None));
    let p = id(&d, "p");
    object_to_path(&mut d, p);
    assert_eq!(d.attr(p, "d"), Some("M0 0h1"), "an existing path is not re-serialized");
    let g = id(&d, "g");
    object_to_path(&mut d, g);
    assert_eq!(d.tag(g), "g");
}

#[test]
fn fuse_bakes_the_transform_into_a_path_and_its_stroke() {
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="p" d="M0 0 L1 0" transform="scale(2)" style="stroke:#000;stroke-width:1;stroke-dasharray:1,2" sodipodi:nodetypes="cc" inkscape:label="keep me?" inkscape-scientific-combined-by-color="0 2"/><g id="g" transform="scale(2)"><path id="child" d="M0 0 L1 0"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::IDENTITY, None, true);
    assert_eq!(d.attr(p, "d"), Some("M 0,0 L 2,0"));
    assert_eq!(d.attr(p, "transform"), None);
    let st = style_of(&d, p);
    assert_eq!((st.get("stroke-width"), st.get("stroke-dasharray")), (Some("2"), Some("2,4")));
    assert_eq!((d.attr(p, "sodipodi:nodetypes"), d.attr(p, "inkscape:label")), (None, None), "Inkscape's path metadata goes");
    assert_eq!(d.attr(p, "inkscape-scientific-combined-by-color"), Some("0 2"), "…our compatibility attribute stays");
    // groups are untouched, children not visited
    let g = id(&d, "g");
    fuse(&mut d, &mut ctx, g, Affine::IDENTITY, None, true);
    assert_eq!(d.attr(g, "transform"), Some("scale(2)"));
    assert_eq!(d.attr(id(&d, "child"), "d"), Some("M0 0 L1 0"));
    // identity and nothing to do: the d is not even re-serialized
    let mut d = doc(&format!(r#"<svg {NS}><path id="p" d="M0 0h1"/></svg>"#));
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::IDENTITY, None, true);
    assert_eq!(d.attr(p, "d"), Some("M0 0h1"));
}

#[test]
fn fuse_extra_transform_and_inherited_stroke_width() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g style="stroke:red"><path id="p" d="M0 0 L1 0"/><path id="q" d="M0 0 L1 0" style="stroke:none"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::scale(3.0), None, true);
    assert_eq!(d.attr(p, "d"), Some("M 0,0 L 3,0"));
    assert_eq!(d.attr(p, "style"), Some("stroke-width:3"), "inherited stroke: the default width 1 is made explicit and scaled");
    let q = id(&d, "q");
    fuse(&mut d, &mut ctx, q, Affine::scale(3.0), None, true);
    assert_eq!(d.attr(q, "style"), Some("stroke:none"), "no stroke → no width written");
    let mut d = doc(&format!(r#"<svg {NS}><path id="p" d="M0 0 L1 0" style="stroke:red;stroke-width:2"/></svg>"#));
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::scale(3.0), None, false);
    assert_eq!(style_of(&d, p).get("stroke-width"), Some("2"), "apply_to_stroke=false leaves strokes alone");
}

#[test]
fn fuse_handles_rect_circle_ellipse_line_polyline_polygon() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="r" x="1" y="1" width="2" height="3" rx="0.5" transform="matrix(2,0,0,-1,0,10)"/><rect id="rr" width="1" height="1" transform="matrix(0,1,-1,0,0,0)"/><circle id="c" cx="1" cy="1" r="1" transform="scale(2,3)"/><ellipse id="e" cx="0" cy="0" rx="1" ry="2" transform="scale(2,1)"/><line id="l" x1="0" y1="0" x2="1" y2="1" transform="translate(5,6)"/><polyline id="pl" points="0,0 1,1" transform="scale(2)"/><polygon id="pg" points="0,0 1,0 1,1" transform="translate(1,1)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    for i in ["r", "rr", "c", "e", "l", "pl", "pg"] {
        let n = id(&d, i);
        fuse(&mut d, &mut ctx, n, Affine::IDENTITY, None, true);
        assert_eq!(d.attr(n, "transform"), None, "{i}");
    }
    let r = id(&d, "r");
    assert_eq!(d.tag(r), "rect");
    let get = |n: NodeId, a: &str| d.attr(n, a).map(str::to_string);
    assert_eq!((get(r, "x"), get(r, "y"), get(r, "width"), get(r, "height")), (Some("2".into()), Some("6".into()), Some("4".into()), Some("3".into())));
    assert_eq!((get(r, "rx"), get(r, "ry")), (Some("1".into()), Some("0.5".into())), "radii follow the axis scales");
    let rr = id(&d, "rr");
    assert_eq!(d.tag(rr), "path", "a rotated rect becomes a path");
    assert_eq!(get(rr, "d"), Some("M 0,0 L 0,1 L -1,1 L -1,0 Z".into()));
    assert_eq!(get(rr, "width"), None);
    let c = id(&d, "c");
    assert_eq!(d.tag(c), "ellipse", "non-uniform scale turns a circle into an ellipse");
    assert_eq!((get(c, "cx"), get(c, "cy"), get(c, "rx"), get(c, "ry"), get(c, "r")), (Some("2".into()), Some("3".into()), Some("2".into()), Some("3".into()), None));
    let e = id(&d, "e");
    assert_eq!(d.tag(e), "circle", "…and equal edges turn an ellipse into a circle");
    assert_eq!((get(e, "cx"), get(e, "cy"), get(e, "r"), get(e, "rx")), (Some("0".into()), Some("0".into()), Some("2".into()), None));
    let l = id(&d, "l");
    assert_eq!((get(l, "x1"), get(l, "y1"), get(l, "x2"), get(l, "y2")), (Some("5".into()), Some("6".into()), Some("6".into()), Some("7".into())));
    assert_eq!(get(id(&d, "pl"), "points"), Some("0,0 2,2".into()));
    // upstream quirk kept: a polygon's closing command contributes the start point once more
    assert_eq!(get(id(&d, "pg"), "points"), Some("1,1 2,1 2,2 1,1".into()));
}

#[test]
fn fuse_with_ranges_transforms_each_slice_on_its_own() {
    let mut d = doc(&format!(r#"<svg {NS}><path id="p" d="M0 0 L1 0 M0 5 L1 5" transform="scale(2)"/></svg>"#));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    let ranges: Ranges = vec![(0..2, Affine::translate((0.0, 1.0))), (2..4, Affine::translate((0.0, -1.0)))];
    fuse(&mut d, &mut ctx, p, Affine::IDENTITY, Some(&ranges), true);
    assert_eq!(d.attr(p, "d"), Some("M 0,1 L 1,1 M 0,4 L 1,4"), "the element's own transform is NOT applied to ranged geometry — the ranges carry it");
    assert_eq!(d.attr(p, "transform"), None);
}

#[test]
fn fuse_duplicates_transformed_clips_and_user_space_gradients() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1" height="1"/></clipPath><linearGradient id="g" gradientUnits="userSpaceOnUse" gradientTransform="scale(2)"/><linearGradient id="obb"/></defs><path id="p" d="M0 0 L1 0" transform="translate(1,0)" clip-path="url(#c)" style="fill:url(#g);stroke:url(#obb)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::IDENTITY, None, true);
    assert_eq!(d.attr(p, "d"), Some("M 1,0 L 2,0"));
    let cv = d.attr(p, "clip-path").unwrap().to_string();
    let cdup = id(&d, cv.trim_start_matches("url(#").trim_end_matches(')'));
    assert_ne!(cdup, id(&d, "c"));
    assert_eq!(ctx.created, vec![cdup]);
    assert_eq!(d.attr(kids(&d, cdup)[0], "transform"), Some("translate(1,0)"), "the clip keeps following the geometry");
    let st = style_of(&d, p);
    let fill = st.get("fill").unwrap().to_string();
    assert_ne!(fill, "url(#g)");
    let gdup = id(&d, fill.trim_start_matches("url(#").trim_end_matches(')'));
    assert_eq!(d.tag(gdup), "linearGradient");
    assert_eq!(d.attr(gdup, "gradientTransform"), Some("matrix(2,0,0,2,1,0)"), "translate(1,0) · scale(2)");
    assert_eq!(d.attr(id(&d, "g"), "gradientTransform"), Some("scale(2)"), "the original is untouched");
    assert_eq!(st.get("stroke"), Some("url(#obb)"), "an objectBoundingBox gradient follows the box by itself");
    assert_eq!(out(&d).matches("<linearGradient").count(), 3);
}

#[test]
fn global_transform_works_in_the_parent_frame_and_preserves_strokes() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g transform="scale(2)"><g id="k" transform="translate(1,0)" style="stroke-width:1;stroke-dasharray:2,4"><path id="p" d="M0 0 L1 0"/></g></g><g id="k2" style="stroke-width:1"/><path id="s" d="M0 0 L1 0" transform="translate(1,1)" style="stroke:#000;stroke-width:1"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let k = id(&d, "k");
    global_transform(&mut d, &mut ctx, k, Affine::scale(3.0), None, true);
    assert_eq!(d.attr(k, "transform"), Some("matrix(3,0,0,3,3,0)"), "P⁻¹ · scale(3) · P · translate(1,0)");
    let st = style_of(&d, k);
    assert_eq!(st.get("stroke-width"), Some("0.33333333"), "visual width 2 kept under the new scale factor 6");
    assert_eq!(st.get("stroke-dasharray"), Some("0.66666667,1.3333333"));
    assert_eq!(d.attr(id(&d, "p"), "d"), Some("M0 0 L1 0"), "children of a group are not fused");
    // a pure translation keeps the width as it is: nothing is rewritten
    let k2 = id(&d, "k2");
    global_transform(&mut d, &mut ctx, k2, Affine::translate((5.0, 5.0)), None, true);
    assert_eq!(d.attr(k2, "transform"), Some("translate(5,5)"));
    assert_eq!(d.attr(k2, "style"), Some("stroke-width:1"));
    // a shape is fused: the transform disappears into d, the stroke stays visually 1 wide
    let s = id(&d, "s");
    global_transform(&mut d, &mut ctx, s, Affine::scale(2.0), None, true);
    assert_eq!(d.attr(s, "transform"), None);
    assert_eq!(d.attr(s, "d"), Some("M 2,2 L 4,2"));
    assert_eq!(style_of(&d, s).get("stroke-width"), Some("1"), "fused and then restored to the same visual width");
    // without preservation the stroke scales with the geometry
    let mut d = doc(&format!(r#"<svg {NS}><path id="s" d="M0 0 L1 0" style="stroke:#000;stroke-width:1"/></svg>"#));
    let s = id(&d, "s");
    global_transform(&mut d, &mut ctx, s, Affine::scale(2.0), None, false);
    assert_eq!(style_of(&d, s).get("stroke-width"), Some("2"));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test ops_xform 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 3: Implement**

Replace `src/ops/xform.rs`:

```rust
//! Baking transforms into geometry and moving elements in root coordinates (spec §B.2 "fuse",
//! "global_transform", "combine_paths"; upstream AT:20–234 `fuseTransform`, DH:1065–1147).

use std::ops::Range;

use kurbo::{Affine, BezPath, PathEl, Point};

use crate::dom::{Doc, NodeId};
use crate::geom::path::{end_points, fmt_d, shape_path};
use crate::geom::{TOL, fmt_transform, inverse, ipx, is_identity, parse_transform, scale_factor};
use crate::num;
use crate::style::Style;
use crate::text::style::composed_width;

use super::cleanup::url_id;
use super::clip::duplicate_into_defs;
use super::style::{composed_list, fix_css_clipmask};
use super::{ClipKind, Ctx, clip_ref, label};

/// Elements whose geometry a transform can be baked into (C:489–497 `otp_support`).
pub const OTP_SUPPORT: &[&str] = &[
    "rect", "ellipse", "circle", "polygon", "polyline", "line", "path",
];

/// Slices of a path (indices into the `BezPath` elements of the written `d`) with the transform
/// each takes — the Scaler's combined-by-colour pieces.
pub type Ranges = Vec<(Range<usize>, Affine)>;

const SHAPE_ATTRS: &[&str] = &[
    "x", "y", "width", "height", "rx", "ry", "cx", "cy", "r", "points", "x1", "y1", "x2", "y2",
];

/// C:504–516 `object_to_path`: `d` from the shape's geometry, tag `path`, the shape attributes
/// removed (**Deviation**: upstream leaves them behind). No-op for `<path>` and non-shapes.
pub fn object_to_path(doc: &mut Doc, el: NodeId) {
    if doc.tag(el) == "path" || !OTP_SUPPORT.contains(&doc.tag(el)) {
        return;
    }
    let Some(pp) = shape_path(doc, el) else {
        return;
    };
    doc.set_attr(el, "d", fmt_d(&pp.path));
    for a in SHAPE_ATTRS {
        doc.remove_attr(el, a);
    }
    doc.set_tag(el, "path");
}

/// AT:67–86 `transform_clipmask`: a transformed element's clip/mask is duplicated and its
/// children take the element's transform, so the geometry can absorb the transform afterwards.
fn transform_clipmask(doc: &mut Doc, ctx: &mut Ctx, el: NodeId, kind: ClipKind) {
    let own = doc.transform(el);
    if is_identity(own) {
        return;
    }
    let Some(clip) = clip_ref(doc, el, kind) else {
        return;
    };
    let d = duplicate_into_defs(doc, ctx, clip);
    let id = doc.ensure_id(d);
    doc.set_attr(el, kind.attr(), format!("url(#{id})"));
    fix_css_clipmask(doc, el, kind);
    let kids: Vec<NodeId> = doc.children(d).filter(|&k| doc.is_element(k)).collect();
    for k in kids {
        let kt = doc.transform(k);
        doc.set_transform(k, own * kt);
    }
}

/// AT:20–32 `remove_attrs`: a `<path>` loses its `sodipodi:*`/`inkscape:*` attributes except
/// the `inkscape-academic*`/`inkscape-scientific*` compatibility ones.
fn remove_inkscape_attrs(doc: &mut Doc, el: NodeId) {
    let names: Vec<String> = doc
        .attrs(el)
        .iter()
        .map(|a| a.name.clone())
        .filter(|n| {
            (n.contains("sodipodi") || n.contains("inkscape"))
                && !n.contains("inkscape-academic")
                && !n.contains("inkscape-scientific")
        })
        .collect();
    for n in names {
        doc.remove_attr(el, &n);
    }
}

/// AT:36–64 `applyToStrokes` + spec §B.2: inline `stroke-width` and a non-`none` inline
/// `stroke-dasharray` are multiplied by the transform's scale factor; an element that only
/// inherits a paint stroke gets an explicit, scaled `stroke-width` (the specified one, default 1).
fn apply_to_strokes(doc: &mut Doc, el: NodeId, t: Affine) {
    let sf = scale_factor(t);
    let inline = doc.attr(el, "style").map(Style::parse).unwrap_or_default();
    let mut st = inline.clone();
    match inline.get("stroke-width").and_then(ipx) {
        Some(w) => st.set("stroke-width", &num::fmt(w * sf)),
        None => {
            let stroked = doc
                .specified(el, "stroke")
                .is_some_and(|s| s.trim() != "none");
            let w = match doc.specified(el, "stroke-width") {
                None => Some(1.0),
                Some(v) => ipx(&v),
            };
            if let (true, Some(w)) = (stroked, w) {
                st.set("stroke-width", &num::fmt(w * sf));
            }
        }
    }
    if let Some(dash) = inline.get("stroke-dasharray") {
        if !dash.trim().eq_ignore_ascii_case("none") {
            let vals: Option<Vec<f64>> = dash
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(ipx)
                .collect();
            if let Some(v) = vals {
                let s: Vec<String> = v.iter().map(|x| num::fmt(x * sf)).collect();
                st.set("stroke-dasharray", &s.join(","));
            }
        }
    }
    if st != inline {
        doc.set_style_map(el, &st);
    }
}

/// AT:224–231 + spec §B.2: a `userSpaceOnUse` gradient paint travels with the geometry — the
/// element gets a duplicate whose `gradientTransform` is `t · old`. An `objectBoundingBox`
/// gradient (the SVG default) follows the new box by itself and is left alone.
fn gradient_fixup(doc: &mut Doc, el: NodeId, t: Affine) {
    for prop in ["fill", "stroke"] {
        let Some(g) = doc
            .specified(el, prop)
            .as_deref()
            .and_then(url_id)
            .and_then(|id| doc.by_id(id))
        else {
            continue;
        };
        if !doc.tag(g).ends_with("Gradient")
            || doc.attr(g, "gradientUnits").map(str::trim) != Some("userSpaceOnUse")
        {
            continue;
        }
        let d = doc.deep_clone(g);
        let defs = doc.defs();
        doc.append_child(defs, d);
        let id = doc.ensure_id(d);
        let gt = doc
            .attr(d, "gradientTransform")
            .and_then(parse_transform)
            .unwrap_or(Affine::IDENTITY);
        match fmt_transform(t * gt) {
            Some(s) => doc.set_attr(d, "gradientTransform", s),
            None => {
                doc.remove_attr(d, "gradientTransform");
            }
        }
        doc.set_style(el, prop, &format!("url(#{id})"));
    }
}

fn transform_el(e: PathEl, t: Affine) -> PathEl {
    match e {
        PathEl::MoveTo(p) => PathEl::MoveTo(t * p),
        PathEl::LineTo(p) => PathEl::LineTo(t * p),
        PathEl::QuadTo(c, p) => PathEl::QuadTo(t * c, t * p),
        PathEl::CurveTo(c1, c2, p) => PathEl::CurveTo(t * c1, t * c2, t * p),
        PathEl::ClosePath => PathEl::ClosePath,
    }
}

/// AT:95–234 `fuseTransform`: bakes `extra · el.transform` into a shape's geometry and removes
/// its `transform`. Groups, text, clones and images are untouched (children are never visited).
/// With `ranges`, the path is rebuilt from the listed element slices, each under its own
/// transform (which must already include everything — `global_transform` prepares them), and
/// `extra · el.transform` is used only for strokes and gradients.
pub fn fuse(
    doc: &mut Doc,
    ctx: &mut Ctx,
    el: NodeId,
    extra: Affine,
    ranges: Option<&Ranges>,
    apply_to_stroke: bool,
) {
    if !OTP_SUPPORT.contains(&doc.tag(el)) {
        return;
    }
    transform_clipmask(doc, ctx, el, ClipKind::Clip);
    transform_clipmask(doc, ctx, el, ClipKind::Mask);
    let transf = extra * doc.transform(el);
    doc.remove_attr(el, "transform");
    if doc.tag(el) == "path" {
        remove_inkscape_attrs(doc, el);
    }
    let [a, b, c, d, _, _] = transf.as_coeffs();
    if (b.abs() > TOL || c.abs() > TOL) && matches!(doc.tag(el), "rect" | "ellipse" | "circle") {
        // rotation or shear: only a path can hold the result
        object_to_path(doc, el);
    }
    if is_identity(transf) && ranges.is_none() {
        return;
    }
    let get = |doc: &Doc, name: &str, default: f64| doc.attr(el, name).and_then(ipx).unwrap_or(default);
    let tag = doc.tag(el).to_string();
    match tag.as_str() {
        "polygon" | "polyline" => {
            if let Some(pp) = shape_path(doc, el) {
                let pts: Vec<String> = end_points(&pp.path)
                    .iter()
                    .map(|p| {
                        let q = transf * *p;
                        format!("{},{}", num::fmt(q.x), num::fmt(q.y))
                    })
                    .collect();
                doc.set_attr(el, "points", pts.join(" "));
            }
        }
        "ellipse" | "circle" => {
            let (cx, cy) = (get(doc, "cx", 0.0), get(doc, "cy", 0.0));
            let (rx, ry) = if tag == "circle" {
                let r = get(doc, "r", 0.0);
                (r, r)
            } else {
                (get(doc, "rx", 0.0), get(doc, "ry", 0.0))
            };
            let p1 = transf * Point::new(cx - rx, cy - ry);
            let p2 = transf * Point::new(cx + rx, cy - ry);
            let p3 = transf * Point::new(cx + rx, cy + ry);
            let (edgex, edgey) = (p1.distance(p2), p2.distance(p3));
            doc.set_attr(el, "cx", num::fmt((p1.x + p3.x) / 2.0));
            doc.set_attr(el, "cy", num::fmt((p1.y + p3.y) / 2.0));
            if (edgex - edgey).abs() <= TOL {
                doc.set_tag(el, "circle");
                doc.remove_attr(el, "rx");
                doc.remove_attr(el, "ry");
                doc.set_attr(el, "r", num::fmt(edgex / 2.0));
            } else {
                doc.set_tag(el, "ellipse");
                doc.remove_attr(el, "r");
                doc.set_attr(el, "rx", num::fmt(edgex / 2.0));
                doc.set_attr(el, "ry", num::fmt(edgey / 2.0));
            }
        }
        "line" => {
            let p1 = transf * Point::new(get(doc, "x1", 0.0), get(doc, "y1", 0.0));
            let p2 = transf * Point::new(get(doc, "x2", 0.0), get(doc, "y2", 0.0));
            for (k, v) in [("x1", p1.x), ("y1", p1.y), ("x2", p2.x), ("y2", p2.y)] {
                doc.set_attr(el, k, num::fmt(v));
            }
        }
        "rect" => {
            let (x, y) = (get(doc, "x", 0.0), get(doc, "y", 0.0));
            let (w, h) = (get(doc, "width", 0.0), get(doc, "height", 0.0));
            let corners = [
                Point::new(x, y),
                Point::new(x + w, y),
                Point::new(x + w, y + h),
                Point::new(x, y + h),
            ]
            .map(|p| transf * p);
            let (mut x0, mut y0, mut x1, mut y1) =
                (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for p in corners {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            doc.set_attr(el, "x", num::fmt(x0));
            doc.set_attr(el, "y", num::fmt(y0));
            doc.set_attr(el, "width", num::fmt(x1 - x0));
            doc.set_attr(el, "height", num::fmt(y1 - y0));
            // Deviation (spec §B.2): the radii follow the axis scales; upstream leaves them
            let (rx, ry) = (doc.attr(el, "rx").and_then(ipx), doc.attr(el, "ry").and_then(ipx));
            if rx.is_some() || ry.is_some() {
                let rx0 = rx.or(ry).unwrap_or(0.0);
                let ry0 = ry.or(rx).unwrap_or(0.0);
                doc.set_attr(el, "rx", num::fmt(rx0 * a.abs()));
                doc.set_attr(el, "ry", num::fmt(ry0 * d.abs()));
            }
        }
        _ => {
            if let Some(pp) = shape_path(doc, el) {
                let path = match ranges {
                    None => transf * pp.path,
                    Some(rs) => {
                        let els = pp.path.elements();
                        let mut out = BezPath::new();
                        for (r, t) in rs {
                            let end = r.end.min(els.len());
                            for e in &els[r.start.min(end)..end] {
                                out.push(transform_el(*e, *t));
                            }
                        }
                        out
                    }
                };
                doc.set_attr(el, "d", fmt_d(&path));
            }
        }
    }
    if apply_to_stroke {
        apply_to_strokes(doc, el, transf);
    }
    gradient_fixup(doc, el, transf);
}

/// DH:1065–1107: applies `t` (a root-coordinate transform) to `el` by rewriting `el.transform`
/// as `P⁻¹ · t · P · el.transform` (`P` = the parent's composed transform), then fuses shapes.
/// `ranges` transforms are rewritten the same way and handed to `fuse`. With `preserve_stroke`,
/// the visual stroke width and dashes are kept: `stroke-width := visual_before / sf_after`
/// (**Deviation**: written only when that differs from the current specified width; upstream
/// always writes `stroke-width`, even `1.0` on an untouched group).
pub fn global_transform(
    doc: &mut Doc,
    ctx: &mut Ctx,
    el: NodeId,
    t: Affine,
    ranges: Option<Ranges>,
    preserve_stroke: bool,
) {
    let prt = doc
        .parent(el)
        .filter(|&p| doc.is_element(p))
        .map(|p| doc.composed_transform(p))
        .unwrap_or(Affine::IDENTITY);
    let Some(iprt) = inverse(prt) else {
        ctx.warn.push(format!(
            "{}: singular parent transform, not moved",
            label(doc, el)
        ));
        return;
    };
    let myt = doc.transform(el);
    let newtr = iprt * t * prt * myt;
    let ranges: Option<Ranges> = ranges.map(|rs| {
        rs.into_iter()
            .map(|(r, ti)| (r, iprt * ti * prt * myt))
            .collect()
    });
    let before = composed_width(doc, el, "stroke-width");
    let dashes = composed_list(doc, el, "stroke-dasharray");
    doc.set_transform(el, newtr);
    fuse(doc, ctx, el, Affine::IDENTITY, ranges.as_ref(), true);
    if preserve_stroke {
        let after = composed_width(doc, el, "stroke-width");
        if after.scf > 0.0 {
            let new_w = before.tfs / after.scf;
            if (new_w - after.utfs).abs() > 1e-9 * new_w.abs().max(1.0) {
                doc.set_style(el, "stroke-width", &num::fmt(new_w));
                if let Some(sd) = dashes {
                    let s: Vec<String> = sd.iter().map(|v| num::fmt(v / after.scf)).collect();
                    doc.set_style(el, "stroke-dasharray", &s.join(","));
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run to verify success**

Run: `cargo test --test ops_xform 2>&1 | grep -E "^test result|FAILED|panicked"` → 7 passed; then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/ops/xform.rs tests/ops_xform.rs
git commit -m "feat(ops): fuse transforms into geometry, object_to_path, global_transform"
```

---

### Task 6: `combine_paths`

**Files:**
- Modify: `src/ops/xform.rs` (append)
- Test: `tests/ops_xform.rs` (append)

**Interfaces:**
- Consumes: Task 5 (`object_to_path`), Task 1 (`cleanup::delete_up`), Task 2 (`style::fix_css_clipmask`), `geom::path::{shape_path, fmt_d}`, `geom::inverse`, `Doc::composed_transform`.
- Produces: `combine_paths(doc, ctx, els: &[NodeId], merge_idx: usize) -> bool`

**Deviation (documented in Task 8):** the indices written to `inkscape-scientific-combined-by-color` count the `BezPath` elements of the `d` we write (arcs become several cubics; upstream counts source commands) — spec §B.2 says so. A target with a singular composed transform is left alone with a warning (upstream raises).

- [ ] **Step 1: Write the failing tests**

Append to `tests/ops_xform.rs` (add `combine_paths` to the `sciink::ops::xform` import):

```rust
#[test]
fn combine_paths_concatenates_global_geometry_into_the_target_frame() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><g id="wrap" transform="translate(10,0)"><path id="a" d="M0 0 L1 0"/></g><path id="b" d="M0 0 L0 1 Z" transform="scale(2)" clip-path="url(#c)" style="fill:red"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    assert!(combine_paths(&mut d, &mut ctx, &[b, a], 0));
    // b: 3 elements (M L Z) in root coordinates, then a's 2 → starts 0, 3, total 5
    assert_eq!(d.attr(b, "inkscape-scientific-combined-by-color"), Some("0 3 5"));
    // written back in b's own frame (inverse of scale(2)): a's global M10 0 L11 0 → M5 0 L5.5 0
    assert_eq!(d.attr(b, "d"), Some("M 0,0 L 0,1 Z M 5,0 L 5.5,0"));
    assert_eq!(d.attr(b, "transform"), Some("scale(2)"), "the target keeps its transform attribute as written");
    assert_eq!((d.attr(b, "clip-path"), d.attr(b, "mask")), (Some("none"), Some("none")), "clips and masks are released");
    assert_eq!(d.attr(b, "style"), Some("fill:red"));
    assert_eq!(d.by_id("a"), None);
    assert_eq!(d.by_id("wrap"), None, "the emptied group went with it (delete_up)");
    assert!(ctx.deleted.contains("a") && ctx.deleted.contains("wrap"));
}

#[test]
fn combine_paths_welds_existing_indices_and_converts_a_line_target() {
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="a" d="M0 0 L1 0 M0 1 L1 1" inkscape-scientific-combined-by-color="0 2 4"/><path id="b" d="M5 5 L6 5 Z"/><line id="l" x1="0" y1="0" x2="1" y2="1" style="stroke:#000"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (a, b, l) = (id(&d, "a"), id(&d, "b"), id(&d, "l"));
    assert!(combine_paths(&mut d, &mut ctx, &[a, b], 0));
    assert_eq!(d.attr(a, "inkscape-scientific-combined-by-color"), Some("0 2 4 7"), "a's own pieces stay separate pieces");
    assert_eq!(d.attr(a, "d"), Some("M 0,0 L 1,0 M 0,1 L 1,1 M 5,5 L 6,5 Z"));
    assert_eq!(d.by_id("b"), None);
    // a <line> target becomes a <path>
    assert!(combine_paths(&mut d, &mut ctx, &[a, l], 1));
    assert_eq!(d.tag(l), "path");
    assert_eq!(d.attr(l, "x1"), None);
    assert_eq!(d.attr(l, "d"), Some("M 0,0 L 1,0 M 0,1 L 1,1 M 5,5 L 6,5 Z M 0,0 L 1,1"), "geometry follows the list order, a first");
    assert_eq!(d.attr(l, "inkscape-scientific-combined-by-color"), Some("0 2 4 7 9"));
    assert_eq!(d.attr(l, "style"), Some("stroke:#000"));
    assert_eq!(d.by_id("a"), None);
    // a singular target is refused and nothing changes
    let mut d = doc(&format!(r#"<svg {NS}><path id="a" d="M0 0 L1 0"/><path id="b" d="M0 0 L1 0" transform="scale(0)"/></svg>"#));
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    assert!(!combine_paths(&mut d, &mut ctx, &[a, b], 1));
    assert!(d.by_id("a").is_some() && d.attr(b, "inkscape-scientific-combined-by-color").is_none());
    assert!(ctx.warn.0.iter().any(|w| w.contains("singular")), "{:?}", ctx.warn.0);
}

#[test]
fn combine_paths_pins_released_clips_against_a_stylesheet() {
    let mut d = doc(&format!(
        r#"<svg {NS}><style>#a{{clip-path:url(#c)}}</style><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><path id="a" d="M0 0 L1 0" clip-path="url(#c)"/><path id="b" d="M2 0 L3 0"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    assert!(combine_paths(&mut d, &mut ctx, &[a, b], 0));
    assert!(out(&d).contains("\n#a{clip-path:none}</style>"), "{}", out(&d));
    assert!(!out(&d).contains("#a{mask"), "the sheet says nothing about masks → nothing pinned");
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test ops_xform 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 3: Implement**

Append to `src/ops/xform.rs` (add `use super::cleanup::delete_up;` — merge it into the existing `use super::cleanup::{delete_up, url_id};`):

```rust
/// DH:1111–1147: concatenates the root-coordinate geometry of `els` (in the given order) into
/// `els[merge_idx]`, written back in the target's own frame, releases the target's clip and mask
/// (`none`, pinned against the stylesheet) and deletes the others with `delete_up`. Piece
/// boundaries land in `inkscape-scientific-combined-by-color` (`s0 s1 … total`; an element that
/// already carries the attribute contributes its own pieces). Returns `false`, changing nothing,
/// when the target's composed transform is singular.
pub fn combine_paths(doc: &mut Doc, ctx: &mut Ctx, els: &[NodeId], merge_idx: usize) -> bool {
    let mel = els[merge_idx];
    let Some(inv) = inverse(doc.composed_transform(mel)) else {
        ctx.warn.push(format!(
            "{}: singular transform, paths not combined",
            label(doc, mel)
        ));
        return false;
    };
    let mut pnew = BezPath::new();
    let mut si: Vec<usize> = Vec::new();
    for &el in els {
        let Some(pp) = shape_path(doc, el) else {
            ctx.warn.push(format!("{}: no geometry, skipped", label(doc, el)));
            continue;
        };
        let n0 = pnew.elements().len();
        match doc.attr(el, "inkscape-scientific-combined-by-color") {
            Some(cbc) => {
                let v: Vec<usize> = cbc.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                match v.split_last() {
                    Some((_, pieces)) if !pieces.is_empty() => {
                        si.extend(pieces.iter().map(|x| x + n0));
                    }
                    _ => si.push(n0),
                }
            }
            None => si.push(n0),
        }
        let global = doc.composed_transform(el) * pp.path;
        for e in global.elements() {
            pnew.push(*e);
        }
    }
    si.push(pnew.elements().len());
    object_to_path(doc, mel);
    doc.set_attr(mel, "d", fmt_d(&(inv * pnew)));
    doc.set_attr(mel, "clip-path", "none");
    doc.set_attr(mel, "mask", "none");
    fix_css_clipmask(doc, mel, ClipKind::Clip);
    fix_css_clipmask(doc, mel, ClipKind::Mask);
    let joined: Vec<String> = si.iter().map(|v| v.to_string()).collect();
    doc.set_attr(mel, "inkscape-scientific-combined-by-color", joined.join(" "));
    for (i, &el) in els.iter().enumerate() {
        if i != merge_idx {
            delete_up(doc, ctx, el);
        }
    }
    true
}
```

- [ ] **Step 4: Run to verify success**

Run: `cargo test --test ops_xform 2>&1 | grep -E "^test result|FAILED|panicked"` → 10 passed; then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/ops/xform.rs tests/ops_xform.rs
git commit -m "feat(ops): combine_paths with combined-by-color piece indices"
```

---

### Task 7: Combine by Color tool

**Files:**
- Create: `src/tools/combine_by_color.rs`, `inx/combine_by_color.inx`, `tests/combine_by_color.rs`
- Modify: `src/tools/mod.rs` (`pub mod combine_by_color;` + shared `first_line`), `src/tools/font_probe.rs`, `src/tools/text_fix.rs`, `src/tools/text_highlight.rs`, `src/tools/about.rs` (use the shared `first_line`), `src/lib.rs` (dispatch)

**Interfaces:**
- Consumes: Tasks 2 and 6 (`ops::style::{StrokeFill, Rgba, strokefill}`, `ops::xform::combine_paths`), `ops::Ctx::{new, finish}`, `cli::Common`, `Doc::selection`.
- Produces:
  - `tools::first_line(e: clap::Error) -> String` (`pub(crate)`, in `src/tools/mod.rs`; the private copies in `font_probe.rs`/`text_fix.rs`/`text_highlight.rs`/`about.rs` are deleted and replaced by `use super::first_line;` — `text_fix.rs` and `text_highlight.rs` inline the same closure today: replace it with `.map_err(first_line)`)
  - `tools::combine_by_color::{CombineByColorCli, candidates(doc, ids) -> Vec<NodeId>, mergeable(a: &StrokeFill, b: &StrokeFill) -> bool, combine_by_color(doc, ctx, els, threshold: f64) -> usize, run(argv, input)}`
  - CLI: `--tool=combine-by-color --tab=<page> --lightnessth=<0..100> --id=…`

**Deviations (documented in Task 8):** (a) elements painted by a `url(#…)` (gradient/pattern) are never merged (upstream crashes on them: it compares `efflightness` on the gradient element); (b) an element already welded into a later one is skipped as a merge leader (upstream re-enters it; harmless with exact style equality, wrong under our tolerances); (c) an empty selection reports `combine-by-color: nothing selected` (upstream is silent).

- [ ] **Step 1: Write the failing tests**

Create `tests/combine_by_color.rs`:

```rust
mod support;

use std::collections::BTreeSet;
use std::ffi::OsString;

use sciink::geom::affine_eq;
use sciink::geom::path::{end_points, parse_d};
use sciink::geom::parse_transform;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn run(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=combine-by-color", "--tab=scaling"];
    a.extend(extra);
    let out = sciink::run(&args(&a), svg.as_bytes()).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}
fn attr<'a>(d: &'a roxmltree::Document, id: &str, name: &str) -> Option<&'a str> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .and_then(|n| n.attribute(name))
}
fn has(d: &roxmltree::Document, id: &str) -> bool {
    d.descendants().any(|n| n.attribute("id") == Some(id))
}

#[test]
fn merges_same_style_paths_into_the_topmost_and_skips_dark_ones() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><path id="p1" d="M0 0 L1 0" style="stroke:#ff0000;stroke-width:1;fill:none"/><path id="dark" d="M0 5 L1 5" style="stroke:#000000;stroke-width:1;fill:none"/><path id="p2" d="M0 1 L1 1" style="stroke:#ff0000;stroke-width:1;fill:none"/><rect id="r" width="1" height="1" style="fill:#ff0000"/><path id="f1" d="M0 2 L1 2 Z" style="fill:#ff0000"/><path id="f2" d="M0 3 L1 3 Z" style="fill:#ff0000"/><text id="t">x</text></g></svg>"#
    );
    let (s, msgs) = run(&svg, &["--id=layer", "--lightnessth=15"]);
    assert!(msgs.is_empty(), "silent on success: {msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "p1") && has(&d, "p2"), "p1 is welded into the topmost of its style, p2");
    assert_eq!(attr(&d, "p2", "inkscape-scientific-combined-by-color"), Some("0 2 4"));
    assert_eq!(attr(&d, "p2", "d"), Some("M 0,1 L 1,1 M 0,0 L 1,0"), "the leader's geometry first, then the earlier ones");
    assert_eq!((attr(&d, "p2", "clip-path"), attr(&d, "p2", "mask")), (Some("none"), Some("none")));
    assert!(!has(&d, "f1") && has(&d, "f2"));
    assert_eq!(attr(&d, "f2", "inkscape-scientific-combined-by-color"), Some("0 3 6"));
    assert!(has(&d, "dark") && attr(&d, "dark", "inkscape-scientific-combined-by-color").is_none(), "black is below the lightness threshold");
    assert!(has(&d, "r") && has(&d, "t"), "rects (no d) and text are not candidates");
    // threshold 0: black qualifies, but has no partner
    let (s, _) = run(&svg, &["--id=layer", "--lightnessth=0"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "dark") && attr(&d, "dark", "inkscape-scientific-combined-by-color").is_none());
    // a white stroke with a black fill: the fill is dark → skipped (both paints must be light)
    let svg = format!(
        r#"<svg {NS}><path id="a" d="M0 0 L1 0 Z" style="stroke:#fff;fill:#000"/><path id="b" d="M0 1 L1 1 Z" style="stroke:#fff;fill:#000"/></svg>"#
    );
    let (s, _) = run(&svg, &["--id=a", "--id=b"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "a") && has(&d, "b"));
}

#[test]
fn style_differences_and_url_paints_block_merging() {
    let base = |id: &str, extra: &str| {
        format!(r#"<path id="{id}" d="M0 0 L1 0" style="stroke:#ff0000;stroke-width:1;fill:none{extra}"/>"#)
    };
    let svg = format!(
        "<svg {NS}><g id=\"layer\">{}{}{}{}{}{}{}</g></svg>",
        base("ref", ""),
        base("width", ";stroke-width:1.01"),
        base("alpha", ";stroke-opacity:0.5"),
        base("dash", ";stroke-dasharray:1,2"),
        base("marker", ";marker-end:url(#m)"),
        base("grad", ";stroke:url(#g)"),
        base("same", ";stroke-width:1.0005"),
    );
    let (s, _) = run(&svg, &["--id=layer"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for i in ["width", "alpha", "dash", "marker", "grad"] {
        assert!(has(&d, i) && attr(&d, i, "inkscape-scientific-combined-by-color").is_none(), "{i} must stay separate");
    }
    assert!(!has(&d, "ref") && has(&d, "same"), "a width within 0.001 still matches, and the later element leads");
    assert_eq!(attr(&d, "same", "inkscape-scientific-combined-by-color"), Some("0 2 4"));
}

#[test]
fn empty_selection_is_reported_and_nothing_changes() {
    let svg = format!(r#"<svg {NS}><path id="a" d="M0 0 L1 0"/><path id="b" d="M0 1 L1 1"/></svg>"#);
    let (s, msgs) = run(&svg, &[]);
    assert_eq!(s, svg);
    assert_eq!(msgs, vec!["combine-by-color: nothing selected".to_string()]);
    let (s, msgs) = run(&svg, &["--id=nope"]);
    assert_eq!(s, svg);
    assert_eq!(msgs.len(), 1);
}

/// Upstream's own reference output for Other_tests.svg (paths only, no fonts involved). Skipped
/// when the fixtures are absent (CI).
#[test]
fn matches_the_upstream_reference_for_other_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read_to_string(dir.join("svg/Other_tests.svg")).unwrap();
    let reference =
        std::fs::read_to_string(dir.join("refs/combine_by_color__--id__layer1__Other_tests__svg.out")).unwrap();
    let (ours, msgs) = run(&input, &["--id=layer1", "--lightnessth=15"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let ours = roxmltree::Document::parse(&ours).unwrap();
    let theirs = roxmltree::Document::parse(&reference).unwrap();
    let path_ids = |d: &roxmltree::Document| -> BTreeSet<String> {
        d.descendants()
            .filter(|n| n.has_tag_name("path"))
            .filter_map(|n| n.attribute("id").map(str::to_string))
            .collect()
    };
    assert_eq!(path_ids(&ours), path_ids(&theirs), "the same paths survive");
    assert_eq!(
        ours.descendants().filter(|n| n.has_tag_name("g")).count(),
        theirs.descendants().filter(|n| n.has_tag_name("g")).count(),
        "the same groups survive delete_up"
    );
    let mut combined = 0;
    for t in theirs.descendants().filter(|n| n.attribute("inkscape-scientific-combined-by-color").is_some()) {
        let id = t.attribute("id").unwrap();
        combined += 1;
        assert_eq!(attr(&ours, id, "inkscape-scientific-combined-by-color"), t.attribute("inkscape-scientific-combined-by-color"), "{id}: piece indices");
        assert_eq!(attr(&ours, id, "clip-path"), Some("none"), "{id}");
        assert_eq!(attr(&ours, id, "mask"), Some("none"), "{id}");
        let ta = parse_transform(t.attribute("transform").unwrap_or("")).unwrap();
        let oa = parse_transform(attr(&ours, id, "transform").unwrap_or("")).unwrap();
        assert!(affine_eq(ta, oa), "{id}: transform");
        let tp = end_points(&parse_d(t.attribute("d").unwrap()).unwrap().path);
        let op = end_points(&parse_d(attr(&ours, id, "d").unwrap()).unwrap().path);
        assert_eq!(tp.len(), op.len(), "{id}: same number of commands");
        for (a, b) in tp.iter().zip(&op) {
            // the reference prints 6 significant digits
            assert!((a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01, "{id}: {a:?} vs {b:?}");
        }
    }
    assert_eq!(combined, 15, "the reference welds fifteen groups of paths");
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test combine_by_color 2>&1 | grep -E "^error|not implemented|FAILED" | head -3` → the tool is "not implemented yet".

- [ ] **Step 3: Implement**

`src/tools/mod.rs` — add the shared helper and the module:

```rust
pub mod combine_by_color;

/// The first line of a clap error (its usage dump is noise in an Inkscape dialog).
pub(crate) fn first_line(e: clap::Error) -> String {
    e.to_string()
        .lines()
        .next()
        .unwrap_or("invalid arguments")
        .to_string()
}
```

Delete the private `first_line` in `src/tools/font_probe.rs` and `src/tools/about.rs` (if present) and add `use super::first_line;`; in `src/tools/text_fix.rs` and `src/tools/text_highlight.rs` replace the inline `.map_err(|e| { e.to_string().lines()… })` closure by `.map_err(first_line)` with the same import. Run `cargo build` before moving on.

Create `src/tools/combine_by_color.rs`:

```rust
//! Combine by Color (spec §B.3; upstream combine_by_color.py): merges selected path-like
//! elements that share stroke, fill, width, dashes and markers into one path each, leaving
//! dark ones (axes, ticks) alone.

use std::collections::HashSet;
use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::Common;
use crate::dom::{Doc, NodeId};
use crate::ops::Ctx;
use crate::ops::style::{Rgba, StrokeFill, strokefill};
use crate::ops::xform::combine_paths;

use super::first_line;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct CombineByColorCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports; unused.
    #[arg(long, default_value = "scaling")]
    pub tab: String,
    /// Lightness threshold in percent: strokes or fills darker than this are left alone.
    #[arg(long, default_value_t = 15.0)]
    pub lightnessth: f64,
}

const SKIP_TAGS: &[&str] = &["namedview", "defs", "metadata", "foreignObject", "g", "missing-glyph"];

/// CBC:39–60: the selection and its descendants (document order, deduplicated), keeping the
/// path-like elements — not a skipped tag, and carrying `d`, `points` or `x1`.
pub fn candidates(doc: &Doc, ids: &[String]) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in doc.selection(ids) {
        for n in doc.descendants(root) {
            if !doc.is_element(n) || SKIP_TAGS.contains(&doc.tag(n)) {
                continue;
            }
            let pathlike = ["d", "points", "x1"].iter().any(|a| doc.attr(n, a).is_some());
            if pathlike && seen.insert(n) {
                out.push(n);
            }
        }
    }
    out
}

fn same_width(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() < 0.001,
        _ => false,
    }
}

fn same_paint(a: Option<Rgba>, b: Option<Rgba>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            (x.r, x.g, x.b) == (y.r, y.g, y.b) && (x.alpha - y.alpha).abs() < 0.001
        }
        _ => false,
    }
}

fn same_dashes(a: &Option<Vec<f64>>, b: &Option<Vec<f64>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| (p - q).abs() < 1e-3)
        }
        _ => false,
    }
}

/// CBC:80–112: same stroke width (±0.001), same stroke and fill (rgb exact, alpha ±0.001),
/// same dashes (±0.001 each) and identical raw markers.
pub fn mergeable(a: &StrokeFill, b: &StrokeFill) -> bool {
    same_width(a.stroke_width, b.stroke_width)
        && same_paint(a.stroke, b.stroke)
        && same_paint(a.fill, b.fill)
        && same_dashes(&a.dasharray, &b.dasharray)
        && a.marker_start == b.marker_start
        && a.marker_mid == b.marker_mid
        && a.marker_end == b.marker_end
}

/// CBC:62–125 over `els` (document order): from the last element backwards, every light element
/// (no paint darker than `threshold`) gathers the earlier, still unmerged, mergeable ones and
/// becomes their `combine_paths` target (it is the topmost). Returns how many paths were merged
/// away.
pub fn combine_by_color(doc: &mut Doc, ctx: &mut Ctx, els: &[NodeId], threshold: f64) -> usize {
    let sfs: Vec<StrokeFill> = els.iter().map(|&e| strokefill(doc, e)).collect();
    let is_url = |sf: &StrokeFill| sf.stroke_is_url || sf.fill_is_url;
    let mut merged = vec![false; els.len()];
    let mut removed = 0;
    for ii in (0..els.len()).rev() {
        // Deviation: an element already welded into a later one is gone from the document
        if merged[ii] || is_url(&sfs[ii]) {
            continue;
        }
        let sf1 = &sfs[ii];
        let light = |p: &Option<Rgba>| p.is_none_or(|c| c.efflightness >= threshold);
        if !(light(&sf1.stroke) && light(&sf1.fill)) {
            continue;
        }
        let mut merges = vec![ii];
        merged[ii] = true;
        for jj in 0..ii {
            if !merged[jj] && !is_url(&sfs[jj]) && mergeable(sf1, &sfs[jj]) {
                merges.push(jj);
                merged[jj] = true;
            }
        }
        if merges.len() > 1 {
            let group: Vec<NodeId> = merges.iter().map(|&k| els[k]).collect();
            if combine_paths(doc, ctx, &group, 0) {
                removed += group.len() - 1;
            }
        }
    }
    removed
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = CombineByColorCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    let mut ctx = Ctx::new();
    if doc.selection(&cli.common.ids).is_empty() {
        messages.push("combine-by-color: nothing selected".to_string());
    } else {
        let els = candidates(&doc, &cli.common.ids);
        combine_by_color(&mut doc, &mut ctx, &els, cli.lightnessth / 100.0);
        ctx.finish(&mut doc);
    }
    messages.extend(ctx.warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
```

`src/lib.rs`: in `run`, add the arm `"combine-by-color" => tools::combine_by_color::run(argv, input),` and remove `"combine-by-color"` from the "not implemented yet" list.

Create `inx/combine_by_color.inx` (parameter names, defaults and page name as upstream's, so a user can switch back and forth):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Combine by Color (sciink)</name>
    <id>org.sciink.combine-by-color</id>
    <param name="tool" type="string" gui-hidden="true">combine-by-color</param>
    <param name="tab" type="notebook">
        <page name="scaling" gui-text="Options">
            <label>Combines all selected paths of the same color (and style) into a single path, ignoring lines that are darker than a certain threshold. Clips and masks will be released.</label>
            <label>Useful when your plot rendering program has split your data into many paths, or to shrink a file and make Inkscape more responsive.</label>
            <label appearance="header">Lightness threshold</label>
            <label>If the stroke lightness is less than the lightness threshold, combining will not occur. This can be used to exclude axes and ticks, which are usually black.</label>
            <param name="lightnessth" type="float" precision="0" min="0" max="100" gui-text="Lightness threshold (%)">15</param>
        </page>
    </param>
    <effect needs-live-preview="true">
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

- [ ] **Step 4: Run to verify success**

Run: `cargo test --test combine_by_color 2>&1 | grep -E "^test result|FAILED|panicked"` → 4 passed (the reference test runs here because `tests/upstream` is symlinked; it prints a SKIP and passes vacuously in CI). Then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

If the reference test fails on piece indices or counts, do **not** loosen it: report the first differing id, its expected/actual attribute and `d` in the task report — the controller rules on it (candidates: an arc in the fixture, a `g` deleted or kept by `delete_up`, an element that upstream's Python would have crashed on).

- [ ] **Step 5: Commit**

```bash
git add src/tools/combine_by_color.rs src/tools/mod.rs src/tools/font_probe.rs src/tools/about.rs src/tools/text_fix.rs src/tools/text_highlight.rs src/lib.rs inx/combine_by_color.inx tests/combine_by_color.rs
git commit -m "feat(tools): Combine by Color"
```

---

### Task 8: Text Ghoster tool, README, spec deviations

**Files:**
- Create: `src/tools/text_ghoster.rs`, `inx/text_ghoster.inx`, `tests/text_ghoster.rs`
- Modify: `src/tools/mod.rs`, `src/lib.rs`, `README.md`, `docs/spec/02-geometry-tools.md`

**Interfaces:**
- Consumes: Task 3 (`ops::bbox::{bbox, LOCAL}`), Task 1 (`Ctx`, `label`, `Doc::set_transform`), Task 7 (`tools::first_line`), `text::style::composed_font_size(doc, n) -> FontSize { tfs, scf, utfs }`, `geom::{inverse, ipx, scale_factor}`, `Doc::{defs, new_id, prepend_child, append_child, new_element, selection}`.
- Produces: `tools::text_ghoster::{EXTENT, OPACITY, STDDEV, TextGhosterCli, ghost(doc, ctx, el) -> Option<NodeId>, run(argv, input)}`; CLI `--tool=text-ghoster --tab=<page> --id=…`.

**Deviations (documented in this task):** (a) the rectangle is computed directly in the element's own frame (upstream temporarily removes the group's composed transform with two `global_transform` calls, which also leaves a spurious `stroke-width:1.0` on the group); (b) an element without a bounding box or with a singular transform is wrapped but gets no rectangle, with a warning (upstream: crash / silent skip); (c) an empty selection is reported.

- [ ] **Step 1: Write the failing tests**

Create `tests/text_ghoster.rs`:

```rust
mod support;

use std::ffi::OsString;

use sciink::geom::{affine_eq, parse_transform};
use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn run(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=text-ghoster", "--tab=scaling"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes())).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}
fn num(n: roxmltree::Node, a: &str) -> f64 {
    n.attribute(a).unwrap().parse().unwrap()
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}
/// `(x, y, width, height, rx)` of the ghost rectangle in the group wrapping `id`, the group's
/// transform, the rectangle's style and the blur's stdDeviation.
fn ghost_of(d: &roxmltree::Document, id: &str) -> (Vec<f64>, String, String, f64) {
    let el = by_id(d, id);
    let g = el.parent().unwrap();
    assert_eq!(g.tag_name().name(), "g", "{id} is wrapped in a group");
    let r = g
        .children()
        .find(|n| n.has_tag_name("rect") && n.attribute("id").is_none())
        .expect("ghost rectangle");
    let vals: Vec<f64> = ["x", "y", "width", "height", "rx"].iter().map(|a| num(r, a)).collect();
    let style = r.attribute("style").unwrap().to_string();
    let fid = style.split("filter:url(#").nth(1).unwrap().split(')').next().unwrap();
    let blur = by_id(d, fid).children().find(|n| n.is_element()).unwrap();
    assert_eq!(blur.tag_name().name(), "feGaussianBlur");
    (vals, g.attribute("transform").unwrap_or("").to_string(), style, num(blur, "stdDeviation"))
}

#[test]
fn wraps_the_text_moves_its_transform_and_sizes_the_rectangle_from_the_extent() {
    let svg = format!(
        r#"<svg {NS}><defs><linearGradient id="lg"/></defs><g id="layer" transform="translate(100,0)"><rect id="other" width="1" height="1"/><text id="t" style="font-family:'DejaVu Sans';font-size:10px" x="0" y="0" transform="translate(5,5)">Hi</text><rect id="after" width="1" height="1"/></g></svg>"#
    );
    // the expected extent, from the engine itself, in the text's own frame
    let (x1, cap) = {
        let mut d = sciink::dom::Doc::parse(svg.as_bytes()).unwrap();
        let t = d.by_id("t").unwrap();
        let mut ctx = sciink::ops::Ctx::new();
        let bb = with_vendored_fonts(|| {
            sciink::ops::bbox::bbox(&mut d, &mut ctx, t, sciink::ops::bbox::LOCAL)
        })
        .unwrap();
        assert!((bb.y0 + 7.29).abs() < 0.05 && bb.y1.abs() < 1e-9 && bb.x0.abs() < 1e-9, "{bb:?}");
        (bb.x1, -bb.y0)
    };
    let (s, msgs) = run(&svg, &["--id=t"]);
    assert!(msgs.is_empty(), "silent on success: {msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let layer = by_id(&d, "layer");
    let kids: Vec<_> = layer.children().filter(|n| n.is_element()).collect();
    assert_eq!(kids.len(), 3);
    assert_eq!((kids[0].attribute("id"), kids[1].attribute("id")), (Some("other"), Some("after")));
    let g = kids[2];
    assert_eq!(g.tag_name().name(), "g", "the wrapper sits last in the layer");
    let gk: Vec<_> = g.children().filter(|n| n.is_element()).collect();
    assert_eq!(gk.len(), 2);
    assert_eq!((gk[0].tag_name().name(), gk[1].attribute("id")), ("rect", Some("t")));
    assert_eq!(gk[1].attribute("transform"), None, "the text's transform moved onto the group");
    let (v, gt, style, std) = ghost_of(&d, "t");
    assert_eq!(gt, "translate(5,5)");
    // border = EXTENT · 10px = 5
    assert!((v[0] + 5.0).abs() < 1e-6 && (v[1] + cap + 5.0).abs() < 1e-6, "{v:?}");
    assert!((v[2] - (x1 + 10.0)).abs() < 1e-6 && (v[3] - (cap + 10.0)).abs() < 1e-6, "{v:?}");
    assert!((v[4] - 5.0).abs() < 1e-9);
    let fid = style.split("filter:url(#").nth(1).unwrap().split(')').next().unwrap().to_string();
    assert_eq!(style, format!("fill:#ffffff;stroke:none;filter:url(#{fid});opacity:0.75"));
    assert!((std - 2.5).abs() < 1e-9, "STDDEV · border");
    let defs = d.descendants().find(|n| n.has_tag_name("defs")).unwrap();
    let first = defs.children().find(|n| n.is_element()).unwrap();
    assert_eq!((first.tag_name().name(), first.attribute("id")), ("filter", Some(fid.as_str())), "the filter goes first in <defs>");
    assert!(by_id(&d, "lg").is_element(), "existing defs content stays");
}

#[test]
fn font_size_is_the_largest_in_the_groups_frame_or_falls_back_to_8pt() {
    let svg = format!(
        r#"<svg {NS}><g id="scaled" transform="scale(2)"><g id="grp"><text id="a" style="font-family:'DejaVu Sans';font-size:6px">a</text><text id="b" style="font-family:'DejaVu Sans';font-size:12px" transform="scale(2)">b</text></g><rect id="r" width="4" height="2"/></g></svg>"#
    );
    let (s, msgs) = run(&svg, &["--id=grp", "--id=r"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    // grp: 6 and 12·2 in the group's own frame (the outer scale(2) does not count) → 24 → border 12
    let (v, _, _, std) = ghost_of(&d, "grp");
    assert!((v[4] - 12.0).abs() < 1e-9 && (std - 6.0).abs() < 1e-9, "{v:?}");
    // a plain rect has no font size anywhere: 8pt = 10.6667 → border 5.3333333
    let (v, _, _, _) = ghost_of(&d, "r");
    assert!((v[4] - 16.0 / 3.0).abs() < 1e-6 && (v[0] + 16.0 / 3.0).abs() < 1e-6, "{v:?}");
    assert!((v[2] - (4.0 + 32.0 / 3.0)).abs() < 1e-6 && (v[3] - (2.0 + 32.0 / 3.0)).abs() < 1e-6, "{v:?}");
    assert_eq!(d.descendants().filter(|n| n.has_tag_name("feGaussianBlur")).count(), 2);
    let scaled = by_id(&d, "scaled");
    let sk: Vec<_> = scaled.children().filter(|n| n.is_element()).collect();
    assert_eq!(sk.len(), 2, "both wrappers, in selection (document) order: {s}");
    assert!(sk.iter().all(|n| n.has_tag_name("g")));
}

#[test]
fn singular_or_boxless_elements_are_wrapped_without_a_rectangle() {
    let svg = format!(
        r#"<svg {NS}><text id="t" style="font-family:'DejaVu Sans';font-size:10px" transform="scale(0)">x</text><g id="empty"/></svg>"#
    );
    let (s, msgs) = run(&svg, &["--id=t", "--id=empty"]);
    assert!(!s.contains("<rect") && !s.contains("<filter"), "{s}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let t = by_id(&d, "t");
    assert_eq!(t.parent().unwrap().tag_name().name(), "g");
    assert_eq!(t.parent().unwrap().attribute("transform"), Some("scale(0,0)"));
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(msgs.iter().all(|m| m.starts_with("warning: ")), "{msgs:?}");
    let (s, msgs) = run(&svg, &[]);
    assert_eq!(s, svg);
    assert_eq!(msgs, vec!["text-ghoster: nothing selected".to_string()]);
}

/// Upstream's reference output for `text28136` (Tahoma). Run with the installed fonts only:
/// `SCIINK_SYSTEM_FONTS=1 cargo test --test text_ghoster -- --ignored --nocapture`
/// (not `--include-ignored`: the other tests pin the vendored fonts for the whole binary).
#[test]
#[ignore]
fn matches_the_upstream_reference_for_text28136() {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        eprintln!("SKIP: set SCIINK_SYSTEM_FONTS=1 to compare against the upstream reference (Tahoma)");
        return;
    }
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read_to_string(dir.join("svg/Other_tests.svg")).unwrap();
    let reference =
        std::fs::read_to_string(dir.join("refs/text_ghoster__--id__text28136__Other_tests__svg.out")).unwrap();
    let out = sciink::run(&args(&["--tool=text-ghoster", "--tab=scaling", "--id=text28136"]), input.as_bytes()).unwrap();
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    let ours = String::from_utf8(out.svg).unwrap();
    let ours = roxmltree::Document::parse(&ours).unwrap();
    let theirs = roxmltree::Document::parse(&reference).unwrap();
    let (v, gt, style, std) = ghost_of(&ours, "text28136");
    let (v2, gt2, style2, std2) = ghost_of(&theirs, "text28136");
    assert!(affine_eq(parse_transform(&gt).unwrap(), parse_transform(&gt2).unwrap()), "{gt} vs {gt2}");
    for (a, b) in v.iter().zip(&v2) {
        assert!((a - b).abs() < 0.05, "ours {v:?} vs upstream {v2:?}");
    }
    assert!((std - std2).abs() < 1e-3, "{std} vs {std2}");
    assert!(style.ends_with("opacity:0.75") && style2.ends_with("opacity:0.75"));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test text_ghoster 2>&1 | grep -E "^error|not implemented|FAILED" | head -3`.

- [ ] **Step 3: Implement**

Create `src/tools/text_ghoster.rs`:

```rust
//! Text Ghoster (spec §B.3; upstream text_ghoster.py): a blurred, semi-transparent white
//! rounded rectangle behind each selected element, sized from its bounding box and font size.

use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::Common;
use crate::dom::{Doc, NodeId};
use crate::geom::{inverse, ipx, scale_factor};
use crate::num;
use crate::ops::bbox::{LOCAL, bbox};
use crate::ops::{Ctx, label};
use crate::text::style::composed_font_size;

use super::first_line;

/// How far the rectangle extends beyond the element, in units of the font size (TG:20).
pub const EXTENT: f64 = 0.5;
/// Opacity of the rectangle (TG:22).
pub const OPACITY: f64 = 0.75;
/// Standard deviation of the blur as a fraction of the border (TG:24).
pub const STDDEV: f64 = 0.5;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct TextGhosterCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports; unused.
    #[arg(long, default_value = "scaling")]
    pub tab: String,
}

/// TG:48–125 for one element: wrap `el` in a `<g>` appended at the end of its parent, move
/// `el`'s `transform` onto the group, and put the blurred rectangle first in the group, sized
/// from `el`'s own-frame bounding box grown by `EXTENT × font size`. Returns the group; `None`
/// (element wrapped, no rectangle) when the composed transform is singular or `el` has no box.
pub fn ghost(doc: &mut Doc, ctx: &mut Ctx, el: NodeId) -> Option<NodeId> {
    let parent = doc.parent(el).filter(|&p| doc.is_element(p))?;
    let g = doc.new_element("g");
    doc.append_child(parent, g);
    doc.append_child(g, el);
    let t = doc.transform(el);
    doc.set_transform(g, t);
    doc.remove_attr(el, "transform");
    let ct = doc.composed_transform(g);
    inverse(ct)?;
    let bb = bbox(doc, ctx, el, LOCAL)?;
    // TG:89–98: the largest transformed font size among el and its descendants that specify
    // one, in the group's frame (upstream measures with the group's composed transform removed)
    let scf_g = scale_factor(ct);
    let mut fs: Option<f64> = None;
    let nodes: Vec<NodeId> = doc.descendants(el).filter(|&n| doc.is_element(n)).collect();
    for n in nodes {
        if doc.specified(n, "font-size").is_none() {
            continue;
        }
        let w = composed_font_size(doc, n);
        let local = if scf_g > 0.0 { w.tfs / scf_g } else { w.tfs };
        fs = Some(fs.map_or(local, |m| m.max(local)));
    }
    let fs = fs.unwrap_or_else(|| ipx("8pt").unwrap_or(32.0 / 3.0));
    let border = fs * EXTENT;
    let defs = doc.defs();
    let f = doc.new_element("filter");
    doc.prepend_child(defs, f);
    let fid = doc.new_id("filter");
    doc.set_attr(f, "id", fid.clone());
    let blur = doc.new_element("feGaussianBlur");
    doc.set_attr(blur, "stdDeviation", num::fmt(border * STDDEV));
    doc.append_child(f, blur);
    let r = doc.new_element("rect");
    doc.set_attr(r, "x", num::fmt(bb.x0 - border));
    doc.set_attr(r, "y", num::fmt(bb.y0 - border));
    doc.set_attr(r, "width", num::fmt(bb.width() + 2.0 * border));
    doc.set_attr(r, "height", num::fmt(bb.height() + 2.0 * border));
    doc.set_attr(r, "rx", num::fmt(border));
    doc.set_attr(
        r,
        "style",
        format!("fill:#ffffff;stroke:none;filter:url(#{fid});opacity:{}", num::fmt(OPACITY)),
    );
    doc.prepend_child(g, r);
    Some(g)
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = TextGhosterCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    let mut ctx = Ctx::new();
    let sel = doc.selection(&cli.common.ids);
    if sel.is_empty() {
        messages.push("text-ghoster: nothing selected".to_string());
    }
    for el in sel {
        if ghost(&mut doc, &mut ctx, el).is_none() {
            ctx.warn.push(format!(
                "{}: no bounding box or singular transform, no rectangle added",
                label(&doc, el)
            ));
        }
    }
    ctx.finish(&mut doc);
    messages.extend(ctx.warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
```

`src/tools/mod.rs`: `pub mod text_ghoster;`. `src/lib.rs`: arm `"text-ghoster" => tools::text_ghoster::run(argv, input),`, removed from the "not implemented yet" list.

Create `inx/text_ghoster.inx`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Text Ghoster (sciink)</name>
    <id>org.sciink.text-ghoster</id>
    <param name="tool" type="string" gui-hidden="true">text-ghoster</param>
    <param name="tab" type="notebook">
        <page name="scaling" gui-text="Options">
            <label>Adds a blurred, semi-transparent white rectangle behind each selected object, sized from its text.</label>
            <label>Useful when text has to sit on top of data. To treat several text objects as one, group them first.</label>
        </page>
    </param>
    <effect needs-live-preview="true">
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

- [ ] **Step 4: Run everything**

`cargo test --test text_ghoster 2>&1 | grep -E "^test result|FAILED|panicked"` → 3 passed, 1 ignored; `SCIINK_SYSTEM_FONTS=1 cargo test --test text_ghoster -- --ignored --nocapture 2>&1 | grep -E "^test result|panicked|SKIP"` → passes on this Mac (Tahoma installed); then `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

Dev-machine check (this Mac only; not a test): `cargo build --release && target/release/sciink --tool=combine-by-color --tab=scaling --lightnessth=15 --id=layer1 tests/upstream/data/svg/Other_tests.svg | grep -c inkscape-scientific-combined-by-color` → `15`; `target/release/sciink --tool=text-ghoster --tab=scaling --id=text28136 tests/upstream/data/svg/Other_tests.svg | grep -c feGaussianBlur` → `2`. If `dist/dev-install.sh` has been run before, also: `/Applications/Inkscape.app/Contents/MacOS/inkscape --actions="select-by-id:text28136;org.sciink.text-ghoster.noprefs;export-type:svg;export-filename:/tmp/ghost.svg;export-do" tests/upstream/data/svg/Other_tests.svg` and confirm `/tmp/ghost.svg` contains `filter:url(#filter`.

- [ ] **Step 5: Documentation**

`README.md`: replace the intro sentence and the bullet list with six entries — two tools first, then the four existing ones unchanged:

```markdown
Status: early development. The tools land one by one; design specs live in `docs/spec/`. The current
release ships six menu entries — two tools, three diagnostics and one debug editor:

- **Extensions ▸ Scientific ▸ Combine by Color** — merges the selected paths that share stroke, fill,
  width, dashes and markers into one path each (lines darker than the lightness threshold, such as
  axes and ticks, are left alone), releasing their clips and masks. Fewer elements, smaller files, a
  more responsive Inkscape. Same options and defaults as the original.
- **Extensions ▸ Scientific ▸ Text Ghoster** — puts a blurred, semi-transparent white rectangle behind
  each selected object, sized from its text, so labels stay readable on top of data. Group several
  texts first to treat them as one.
```

(keep the Diagnostics, Font Probe, Text Highlight and Text Fix bullets as they are).

`docs/spec/02-geometry-tools.md`: append a section `## Deliberate deviations (Plan 5)` listing, one bullet each with its reason, the deviations named in Tasks 2–8: `compose_style` opacity only when specified; `currentColor` resolved; url paints report `is_url` with no colour/width; unparsable dash list → `None`; duplicated clips/masks/gradients go to the root `<defs>`; `deswitch` primary-subtag matching; singular node transform leaves the new clip untransformed (warning); `unlink` iterative with a 10 000-step guard; bbox memo per call and no `parsed` flag; `%`/unspecified stroke widths count 0 in boxes; `MAX_NEST = 64` recursion cap; `object_to_path` removes shape attributes; rect radii scaled; inherited stroke made explicit on fuse; `global_transform` writes stroke width/dashes only when they change; `TOL` shear test; gradients duplicated only for `userSpaceOnUse`; combined-by-color indices count `BezPath` elements; singular combine target refused; Combine by Color skips url-painted and already-merged elements and reports an empty selection; Text Ghoster computes in the element's frame, wraps boxless/singular elements without a rectangle, reports an empty selection; tools are silent on success. Also correct §B.3 Text Ghoster's `<g>` sentence to mention the group takes the element's `transform` and that no `stroke-width` is written.

- [ ] **Step 6: Commit**

```bash
git add src/tools/text_ghoster.rs src/tools/mod.rs src/lib.rs inx/text_ghoster.inx tests/text_ghoster.rs README.md docs/spec/02-geometry-tools.md
git commit -m "feat(tools): Text Ghoster; document Plan 5 deviations"
```

---

## Out of scope (deferred)

`strip_whitespace` and `gc` of Flattener-created attributes (Plan 6, with the Flattener that needs them); document scale `px_per_uu` (Plan 7, Homogenizer/Scaler); an approximate bounding box for `flowRoot` and text on a path (Plan 4 residual: they report no box, so a ghosted flow gets no rectangle and a warning); Scaler/Homogenizer/Favorite Markers tools (Plans 7–8); a cross-mutation bbox memo (add when profiling the Flattener on Acid_tests says so); pixel-diff (resvg) invariance tests — geometry ops are asserted numerically here, and the first appearance-changing op set (ungroup + clip merge in the Flattener) is where a renderer oracle pays for its dependency.

## Self-review notes (controller)

- Spec coverage (§B.2): style composition → Task 2 `compose_style`; `fix_css_clipmask` → Task 2; clip/mask merging + `compose_clips`/`intersect_paths` → Task 4; `compose_all`, `ungroup`, `group`, `deswitch`, `unlink` → Task 4; bounding boxes (`bbox`, `has_bbox`, `is_drawn`, `BB2`) → Task 3; stroke/fill (`get_strokefill`, `composed_width` reuse, `composed_list`) → Task 2; `is_rectangle` → Task 3; `fuse` (incl. `transform_clipmask`, `applyToStrokes`, gradient fix-up, `object_to_path`) → Task 5; `global_transform` → Task 5; `combine_paths` → Task 6; housekeeping `delete_up`, `gc_created_clips` → Task 1 (`strip_whitespace`, document scale deferred). §B.3 Combine by color → Task 7; Text Ghoster → Task 8. §B.4 `inkscape-scientific-combined-by-color` → Task 6.
- Type consistency: `Ctx` fields `created: Vec<NodeId>`, `deleted: HashSet<String>`, `warn: Warnings`, private `text: Option<CharTable>`; every op is `(doc: &mut Doc, ctx: &mut Ctx, …)` except the read-only `strokefill`, `composed_list`, `has_bbox`, `is_drawn`, `is_rectangle`, `compose_style`/`remove_inline`/`fix_css_clipmask` (`&mut Doc` only); `ClipKind::attr()` is the attribute name everywhere; `BboxOpts` consts `VISUAL`/`LOCAL`; `Ranges = Vec<(Range<usize>, Affine)>`; `combine_paths(doc, ctx, els, merge_idx) -> bool`; `first_line` shared from `tools/mod.rs` from Task 7 on.
- Placeholder scan: every step carries code or an exact command; every test asserts concrete values (geometry computed by hand in the test comments); the two fixture oracles state their skip conditions; the dev-machine checks in Task 8 are labelled as such.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-21-plan5-geometry-ops.md`. Execute with **subagent-driven development** (fresh implementer per task, task review, fix rounds with scoped re-review, one final whole-branch fix wave), on a branch `plan5-geometry-ops` off `main` (484fdac), with the same process rules as Plans 3–4: never weaken a test to make it pass; plan defects become controller rulings mirrored into this document; sonnet implementers with foreground `cargo` runs.
