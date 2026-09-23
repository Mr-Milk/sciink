# Plan 8 — Favorite Markers and Release Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the sixth and last core tool — **Extensions ▸ Scientific ▸ Favorite Markers** (spec §B.3 "Favorite markers", upstream `favorite_markers.py`): apply a stored start/mid/end marker template to the selected shapes at a chosen size, store the selection's own markers as a new template, remove a template, list them — with a persistent store that needs no restart and never rewrites the `.inx`; then make the repository release-ready for `v0.1.0` (README, CHANGELOG, oracle instructions, packaging check).

**Architecture:** One tool module `src/tools/favorite_markers.rs` in two halves: the `Store` (a small SVG document holding `<marker sciink:template="…" sciink:position="…">` elements, read and written with the crate's own lossless DOM — no JSON library) and the tool (`apply`, `add`, `remove`, `list`, `run`). The three built-in templates live in the binary as one SVG string and seed the store on first use. Marker reuse follows upstream: a marker whose id contains `FM<Template><position>` and whose first child `<g>` carries the requested scale is reused; otherwise a new one is appended to the root `<defs>`.

**Tech Stack:** Rust 2024 (rustc ≥ 1.85), quick-xml 0.42 (through `dom`), kurbo 0.13, clap 4.6. No new dependencies.

**Spec:** `docs/spec/02-geometry-tools.md` — §B.3 "Favorite markers", §B.4 (marker ids `FM<Template><start|mid|end><n>`); `docs/spec/03-infrastructure.md` §C.3 (`.inx`/CLI contract), §C.4 (packaging), §C.5 (b) (the `favorite_markers` reference row). Upstream reference (read-only): `FM` = `favorite_markers.py` under `~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/scientific_inkscape/`. Upstream test data behind `tests/upstream/data/{svg,refs}` (absent in CI → fixture tests `return` early).

## Global Constraints

- Template = `[start, mid, end]`, each absent or `MarkerData { attrs (never `id`), paths: [path attrs (never `id`)] }` (`FM:290–313`). Paths come from the marker's first element child when it is a `<g>`, else from the marker itself.
- Apply (`FM:315–355`): `name = "FM" + template + position` with all whitespace removed; `s = size / 100`; reuse a `<marker>` under the root `<defs>` whose id contains `name` and whose first element child is a `<g>` whose transform has `|a − s| < 0.01 && |d − s| < 0.01` (an absent transform is the identity, `a = d = 1`); otherwise append `<marker {attrs}><g transform="scale(s)">{paths}</g></marker>` with id `Doc::new_id(name)` (`scale(1)` is the identity and is written as no attribute — upstream's inkex drops identity transforms too); then set inline `marker-<position>: url(#id)`. An unchecked position removes the inline `marker-<position>`.
- Shapes are `path | line | polyline | rect | circle | ellipse` among the selection and its descendants (`FM:446–457`).
- **Storage (Deviation from the spec's JSON):** an SVG document written with the crate's DOM — `<svg><marker sciink:template="Arrow" sciink:position="start" …attrs…><path …/>…</marker>…</svg>` — at `$INKSCAPE_PROFILE_DIR/sciink/favorite_markers.svg` (Inkscape exports the variable), else `<inx dir>/favorite_markers.svg` (`paths::inx_dir()`); the hidden `--store <path>` parameter overrides both (tests). A missing file means the built-ins; an unreadable one is an error. No self-modifying `.inx`, no pickle, no restart.
- Built-ins Arrow, Triangle, Distance exactly as `FM:24–214` (attribute values and order, ids dropped).
- Tools are silent on success except: `list` prints the stored names; add/remove print `Templates successfully updated!`; a removal of an unknown name is a `warning:`; every upstream crash path (template index out of range, empty template name, no shape selected for `add`, template not stored) is an `Err(String)` shown by Inkscape with the document echoed unchanged.
- `needs-live-preview="false"` (upstream's value): a live preview on the Add/remove page would rewrite the store on every parameter change.
- Every number written goes through `num::fmt` (`Doc::set_transform`).
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` pass on every commit.
- Deviations from upstream only where this plan marks them **Deviation**; Task 3 mirrors each into `docs/spec/02-geometry-tools.md` under "Deliberate deviations (Plan 8)".

---

## File structure

| File | Responsibility |
|---|---|
| `src/tools/favorite_markers.rs` (new) | `MarkerData`, `Template`, `BUILTINS`, `Store`, `store_path`, `marker_props`, `ensure_marker`, `apply`, `FavoriteMarkersCli`, `run` |
| `src/tools/mod.rs`, `src/lib.rs` (modify) | module, dispatch arm `"favorite-markers"` |
| `inx/favorite_markers.inx` (new) | Scientific ▸ Favorite Markers (two pages) |
| `tests/favorite_markers.rs` (new), `tests/favorite_markers_fixtures.rs` (new) | tests, upstream oracle |
| `README.md`, `CHANGELOG.md` (new), `docs/spec/02-geometry-tools.md` (modify) | menu entry, release notes, deviations |

Test conventions: `mod support;`, `use std::ffi::OsString;`, the `NS`/`args`/`by_id` helpers exactly as in `tests/flattener.rs:1-40`; tool runs through `sciink::run(&args(&[…]), svg.as_bytes())`; every test that touches a store passes its own `--store <path>` under `std::env::temp_dir()` (a unique name per test: `sciink-fm-<pid>-<test>.svg`) and removes it at the end.

---

### Task 1: The store — data model, built-ins, load/save

**Files:**
- Create: `src/tools/favorite_markers.rs` (store half)
- Modify: `src/tools/mod.rs` (`pub mod favorite_markers;`, alphabetical: after `combine_by_color`)
- Test: `tests/favorite_markers.rs` (new)

**Interfaces:**
- Consumes: `Doc::{parse, write, svg, children, tag, attr, attrs, set_attr, new_element, append_child, detach, is_element}`, `Attr { name, value, .. }`, `paths::inx_dir()`.
- Produces (all `pub`): `TEMPLATE_ATTR = "sciink:template"`, `POSITION_ATTR = "sciink:position"`, `POSITIONS: [&str; 3] = ["start", "mid", "end"]`, `SHAPE_TAGS`, `BUILTINS: &str`, `struct MarkerData { pub attrs: Vec<(String, String)>, pub paths: Vec<Vec<(String, String)>> }` (`Debug, Clone, PartialEq, Default`), `type Template = [Option<MarkerData>; 3]`, `struct Store` with `builtins()`, `load(&Path) -> Result<Store, String>`, `save(&self, &Path) -> Result<(), String>`, `templates(&self) -> Vec<String>`, `get(&self, &str) -> Option<Template>`, `set(&mut self, &str, &Template)`, `remove(&mut self, &str) -> bool`; `fn store_path(override_: Option<&Path>) -> PathBuf`.

- [ ] **Step 1: Write the failing tests**

Create `tests/favorite_markers.rs`:

```rust
mod support;

use std::ffi::OsString;
use std::path::PathBuf;

use sciink::tools::favorite_markers::{MarkerData, Store, Template, store_path};

#[allow(dead_code)] // used by Task 2's tests
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

#[allow(dead_code)]
fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink").chain(v.iter().copied()).map(OsString::from).collect()
}
#[allow(dead_code)]
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants().find(|n| n.attribute("id") == Some(id)).unwrap_or_else(|| panic!("no element {id}"))
}
fn tmp_store(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sciink-fm-{}-{name}.svg", std::process::id()))
}
fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn builtins_are_upstreams_three_templates() {
    let s = Store::builtins();
    assert_eq!(s.templates(), vec!["Arrow".to_string(), "Triangle".to_string(), "Distance".to_string()]);
    let arrow = s.get("Arrow").unwrap();
    let start = arrow[0].as_ref().unwrap();
    assert_eq!(
        start.attrs,
        kv(&[("style", "overflow:visible"), ("refX", "0.0"), ("refY", "0.0"), ("orient", "auto"), ("inkscape:stockid", "Arrow2Mstart"), ("inkscape:isstock", "true")])
    );
    assert_eq!(start.paths.len(), 1);
    assert_eq!(start.paths[0][0], ("transform".to_string(), "scale(0.6, 0.6)".to_string()));
    assert!(start.paths[0].iter().all(|(k, _)| k != "id"), "ids are never stored");
    let tri = s.get("Triangle").unwrap();
    assert_eq!(tri[1].as_ref().unwrap().paths[0][0], ("transform".to_string(), "scale(0.4, 0.4)".to_string()));
    assert_eq!(tri[0].as_ref().unwrap().attrs.iter().find(|(k, _)| k == "inkscape:stockid").unwrap().1, "TriangleInM");
    let dist = s.get("Distance").unwrap();
    assert_eq!(dist[0].as_ref().unwrap().paths.len(), 3, "the distance start marker has three paths");
    assert_eq!(dist[1].as_ref().unwrap().attrs.iter().find(|(k, _)| k == "inkscape:stockid").unwrap().1, "StopL");
    assert_eq!(dist[2].as_ref().unwrap().paths[1][0].1, "M 0,0 L -13,4 L -9,0 -13,-4 L 0,0 z ");
    assert!(s.get("Nope").is_none());
}

#[test]
fn store_round_trips_set_replace_remove_and_seeds_from_builtins() {
    let path = tmp_store("roundtrip");
    let _ = std::fs::remove_file(&path);
    let mut s = Store::load(&path).unwrap();
    assert_eq!(s.templates().len(), 3, "a missing file yields the built-ins");
    let md = MarkerData { attrs: kv(&[("orient", "auto"), ("refX", "1"), ("id", "dropped")]), paths: vec![kv(&[("d", "M0,0 L1,1"), ("style", "fill:#f00"), ("id", "dropped")]), kv(&[("d", "M0,0 h2")])] };
    let t: Template = [Some(md.clone()), None, Some(MarkerData { attrs: vec![], paths: vec![kv(&[("d", "M0,0 v2")])] })];
    s.set("Mine", &t);
    s.save(&path).unwrap();
    let s2 = Store::load(&path).unwrap();
    assert_eq!(s2.templates(), vec!["Arrow".to_string(), "Triangle".to_string(), "Distance".to_string(), "Mine".to_string()]);
    let got = s2.get("Mine").unwrap();
    assert_eq!(got[0].as_ref().unwrap().attrs, kv(&[("orient", "auto"), ("refX", "1")]), "id dropped, order kept");
    assert_eq!(got[0].as_ref().unwrap().paths, vec![kv(&[("d", "M0,0 L1,1"), ("style", "fill:#f00")]), kv(&[("d", "M0,0 h2")])]);
    assert!(got[1].is_none());
    assert_eq!(got[2].as_ref().unwrap().paths, vec![kv(&[("d", "M0,0 v2")])]);
    // replacing keeps one copy and the original position in the list
    let mut s3 = s2;
    s3.set("Mine", &[None, Some(md), None]);
    assert_eq!(s3.templates().len(), 4);
    let got = s3.get("Mine").unwrap();
    assert!(got[0].is_none() && got[1].is_some() && got[2].is_none());
    assert!(s3.remove("Mine"));
    assert!(!s3.remove("Mine"), "already gone");
    assert!(s3.get("Mine").is_none());
    assert_eq!(s3.templates().len(), 3);
    s3.save(&path).unwrap();
    assert_eq!(Store::load(&path).unwrap().templates().len(), 3);
    std::fs::remove_file(&path).unwrap();
    // an unreadable store is an error, not silently the built-ins
    let dir = tmp_store("dir-not-file");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(Store::load(&dir).is_err());
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn save_creates_the_parent_directory_and_the_store_is_svg() {
    let path = tmp_store("nested").with_extension("").join("deeper").join("favorite_markers.svg");
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    Store::builtins().save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let d = roxmltree::Document::parse(&text).unwrap();
    assert_eq!(d.root_element().tag_name().name(), "svg");
    assert_eq!(d.descendants().filter(|n| n.has_tag_name("marker")).count(), 9, "3 templates × 3 positions");
    std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
}

#[test]
fn store_path_prefers_the_override() {
    let p = PathBuf::from("/tmp/x/fm.svg");
    assert_eq!(store_path(Some(&p)), p);
    assert!(store_path(None).ends_with("favorite_markers.svg"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test favorite_markers 2>&1 | tail -15`
Expected: compile error — `sciink::tools::favorite_markers` does not exist.

- [ ] **Step 3: Implement the store**

Create `src/tools/favorite_markers.rs`:

```rust
//! Favorite Markers (spec §B.3; upstream favorite_markers.py): apply stored start/mid/end marker
//! templates to the selected shapes, store the selection's markers as a template, remove one.
//! The store is a small SVG document (see `BUILTINS`) — no JSON, no self-modifying `.inx`.

use std::path::{Path, PathBuf};

use crate::dom::{Doc, NodeId};

pub const TEMPLATE_ATTR: &str = "sciink:template";
pub const POSITION_ATTR: &str = "sciink:position";
pub const POSITIONS: [&str; 3] = ["start", "mid", "end"];
/// Elements that take markers (`FM:446–457`).
pub const SHAPE_TAGS: &[&str] = &["path", "line", "polyline", "rect", "circle", "ellipse"];

/// One marker of a template: its attributes and its paths' attributes, never an `id`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarkerData {
    pub attrs: Vec<(String, String)>,
    pub paths: Vec<Vec<(String, String)>>,
}

/// Start, mid, end.
pub type Template = [Option<MarkerData>; 3];

/// The three built-in templates (`FM:24–214`, ids dropped, upstream's attribute order kept).
pub const BUILTINS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" xmlns:sciink="https://github.com/Mr-Milk/sciink">
<marker sciink:template="Arrow" sciink:position="start" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="Arrow2Mstart" inkscape:isstock="true"><path transform="scale(0.6, 0.6)" d="M 8.7185878,4.0337352 L -2.2072895,0.016013256 L 8.7185884,-4.0017078 C 6.9730900,-1.6296469 6.9831476,1.6157441 8.7185878,4.0337352 z " style="stroke:context-stroke;fill-rule:evenodd;fill:context-stroke;stroke-width:0.62500000;stroke-linejoin:round"/></marker>
<marker sciink:template="Arrow" sciink:position="mid" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="Arrow2Mend" inkscape:isstock="true"><path transform="scale(-0.6, -0.6)" d="M 8.7185878,4.0337352 L -2.2072895,0.016013256 L 8.7185884,-4.0017078 C 6.9730900,-1.6296469 6.9831476,1.6157441 8.7185878,4.0337352 z " style="stroke:context-stroke;fill-rule:evenodd;fill:context-stroke;stroke-width:0.62500000;stroke-linejoin:round"/></marker>
<marker sciink:template="Arrow" sciink:position="end" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="Arrow2Mend" inkscape:isstock="true"><path transform="scale(-0.6, -0.6)" d="M 8.7185878,4.0337352 L -2.2072895,0.016013256 L 8.7185884,-4.0017078 C 6.9730900,-1.6296469 6.9831476,1.6157441 8.7185878,4.0337352 z " style="stroke:context-stroke;fill-rule:evenodd;fill:context-stroke;stroke-width:0.62500000;stroke-linejoin:round"/></marker>
<marker sciink:template="Triangle" sciink:position="start" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="TriangleInM" inkscape:isstock="true"><path transform="scale(-0.4, -0.4)" style="fill-rule:evenodd;fill:context-stroke;stroke:context-stroke;stroke-width:1.0pt" d="M 5.77,0.0 L -2.88,5.0 L -2.88,-5.0 L 5.77,0.0 z "/></marker>
<marker sciink:template="Triangle" sciink:position="mid" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="TriangleOutM" inkscape:isstock="true"><path transform="scale(0.4, 0.4)" style="fill-rule:evenodd;fill:context-stroke;stroke:context-stroke;stroke-width:1.0pt" d="M 5.77,0.0 L -2.88,5.0 L -2.88,-5.0 L 5.77,0.0 z "/></marker>
<marker sciink:template="Triangle" sciink:position="end" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="TriangleOutM" inkscape:isstock="true"><path transform="scale(0.4, 0.4)" style="fill-rule:evenodd;fill:context-stroke;stroke:context-stroke;stroke-width:1.0pt" d="M 5.77,0.0 L -2.88,5.0 L -2.88,-5.0 L 5.77,0.0 z "/></marker>
<marker sciink:template="Distance" sciink:position="start" inkscape:stockid="DistanceStart" orient="auto" refY="0.0" refX="0.0" style="overflow:visible" inkscape:isstock="true"><path d="M 0,0 L 2,0" style="fill:none;stroke:context-fill;stroke-width:1.15;stroke-linecap:square"/><path d="M 0,0 L 13,4 L 9,0 13,-4 L 0,0 z " style="fill:context-stroke;fill-rule:evenodd;stroke:none"/><path d="M 0,-4 L 0,40" style="fill:none;stroke:context-stroke;stroke-width:1;stroke-linecap:square"/></marker>
<marker sciink:template="Distance" sciink:position="mid" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="StopL" inkscape:isstock="true"><path transform="scale(0.8, 0.8)" style="fill:none;fill-opacity:0.75000000;fill-rule:evenodd;stroke:context-stroke;stroke-width:1.0pt" d="M 0.0,5.65 L 0.0,-5.65"/></marker>
<marker sciink:template="Distance" sciink:position="end" inkscape:stockid="DistanceEnd" orient="auto" refY="0.0" refX="0.0" style="overflow:visible" inkscape:isstock="true"><path d="M 0,0 L -2,0" style="fill:none;stroke:context-fill;stroke-width:1.15;stroke-linecap:square"/><path d="M 0,0 L -13,4 L -9,0 -13,-4 L 0,0 z " style="fill:context-stroke;fill-rule:evenodd;stroke:none"/><path d="M 0,-4 L 0,40" style="fill:none;stroke:context-stroke;stroke-width:1;stroke-linecap:square"/></marker>
</svg>
"#;

/// The template store: `<marker sciink:template sciink:position …>` children of one `<svg>`.
pub struct Store {
    doc: Doc,
}

fn attrs_of(doc: &Doc, n: NodeId, skip: &[&str]) -> Vec<(String, String)> {
    doc.attrs(n)
        .iter()
        .filter(|a| !skip.contains(&a.name.as_str()))
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect()
}

impl Store {
    pub fn builtins() -> Store {
        Store { doc: Doc::parse(BUILTINS.as_bytes()).expect("BUILTINS is a valid document") }
    }

    /// The store at `path`; a missing file means the built-ins, anything else unreadable is an error.
    pub fn load(path: &Path) -> Result<Store, String> {
        match std::fs::read(path) {
            Ok(bytes) => Doc::parse(&bytes).map(|doc| Store { doc }).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::builtins()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let mut out = Vec::new();
        self.doc.write(&mut out);
        std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn markers(&self) -> Vec<NodeId> {
        let svg = self.doc.svg();
        self.doc.children(svg).filter(|&n| self.doc.is_element(n) && self.doc.tag(n) == "marker").collect()
    }

    /// Template names in order of first appearance.
    pub fn templates(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in self.markers() {
            if let Some(t) = self.doc.attr(m, TEMPLATE_ATTR) {
                if !out.iter().any(|x| x == t) {
                    out.push(t.to_string());
                }
            }
        }
        out
    }

    pub fn get(&self, name: &str) -> Option<Template> {
        let mut t: Template = [None, None, None];
        let mut found = false;
        for m in self.markers() {
            if self.doc.attr(m, TEMPLATE_ATTR) != Some(name) {
                continue;
            }
            found = true;
            let Some(i) = POSITIONS.iter().position(|p| Some(*p) == self.doc.attr(m, POSITION_ATTR)) else { continue };
            let paths = self
                .doc
                .children(m)
                .filter(|&k| self.doc.is_element(k) && self.doc.tag(k) == "path")
                .map(|k| attrs_of(&self.doc, k, &["id"]))
                .collect();
            t[i] = Some(MarkerData { attrs: attrs_of(&self.doc, m, &["id", TEMPLATE_ATTR, POSITION_ATTR]), paths });
        }
        found.then_some(t)
    }

    /// Stores `t` under `name`, replacing an existing template of that name in place (its
    /// markers are replaced where the first of them stood, so the list order is stable).
    pub fn set(&mut self, name: &str, t: &Template) {
        let svg = self.doc.svg();
        let old: Vec<NodeId> = self.markers().into_iter().filter(|&m| self.doc.attr(m, TEMPLATE_ATTR) == Some(name)).collect();
        let anchor = old.first().and_then(|&m| {
            // the element sibling before the first old marker, so the new ones go back there
            let kids: Vec<NodeId> = self.doc.children(svg).collect();
            let idx = kids.iter().position(|&k| k == m)?;
            kids[..idx].iter().rev().copied().find(|&k| self.doc.is_element(k))
        });
        for m in &old {
            self.doc.detach(*m);
        }
        let mut prev = anchor;
        for (i, m) in t.iter().enumerate() {
            let Some(m) = m else { continue };
            let mk = self.doc.new_element("marker");
            self.doc.set_attr(mk, TEMPLATE_ATTR, name);
            self.doc.set_attr(mk, POSITION_ATTR, POSITIONS[i]);
            for (k, v) in &m.attrs {
                if k != "id" && k != TEMPLATE_ATTR && k != POSITION_ATTR {
                    self.doc.set_attr(mk, k, v.clone());
                }
            }
            for p in &m.paths {
                let pe = self.doc.new_element("path");
                for (k, v) in p {
                    if k != "id" {
                        self.doc.set_attr(pe, k, v.clone());
                    }
                }
                self.doc.append_child(mk, pe);
            }
            match prev {
                Some(p) => self.doc.insert_after(mk, p), // (node, anchor)
                None if !old.is_empty() => self.doc.prepend_child(svg, mk),
                None => self.doc.append_child(svg, mk),
            }
            prev = Some(mk);
        }
    }

    /// Removes a template; `false` when no marker carried that name.
    pub fn remove(&mut self, name: &str) -> bool {
        let old: Vec<NodeId> = self.markers().into_iter().filter(|&m| self.doc.attr(m, TEMPLATE_ATTR) == Some(name)).collect();
        for m in &old {
            self.doc.detach(*m);
        }
        !old.is_empty()
    }
}

/// Where the store lives: the override (`--store`), else `$INKSCAPE_PROFILE_DIR/sciink/`
/// (Inkscape exports the variable for extensions), else next to the `.inx` files.
pub fn store_path(override_: Option<&Path>) -> PathBuf {
    if let Some(p) = override_ {
        return p.to_path_buf();
    }
    if let Some(dir) = std::env::var_os("INKSCAPE_PROFILE_DIR") {
        return PathBuf::from(dir).join("sciink").join("favorite_markers.svg");
    }
    crate::paths::inx_dir().join("favorite_markers.svg")
}
```

`Doc::insert_after(n, anchor)` (node first, then the anchor; `src/dom.rs:814`) and `Doc::prepend_child(parent, n)` (`:785`) exist. Register `pub mod favorite_markers;` in `src/tools/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test favorite_markers 2>&1 | tail -15`
Expected: 4 passed.

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/tools/favorite_markers.rs src/tools/mod.rs tests/favorite_markers.rs
git commit -m "feat(favorite-markers): SVG template store with the three built-in templates"
```

---

### Task 2: The tool — apply, add, remove, list, CLI, `.inx`, dispatch

`FM:258–472`.

**Files:**
- Modify: `src/tools/favorite_markers.rs` (tool half), `src/lib.rs` (dispatch arm), `tests/cli.rs` (retire the unimplemented-tool test), `tests/favorite_markers.rs` (append)
- Create: `inx/favorite_markers.inx`

**Interfaces:**
- Consumes: Task 1's `Store`/`MarkerData`/`Template`/`store_path`/`SHAPE_TAGS`; `Doc::{selection, descendants, defs, new_id, set_style, specified_style, by_id, transform, set_transform}`, `ops::style::remove_inline`, `ops::cleanup::url_id`, `geom::Affine`, `cli::{Common, inx_bool}`, `super::first_line`, `crate::Output`.
- Produces: `pub struct FavoriteMarkersCli`, `pub fn marker_props(doc, murl: Option<&str>) -> Option<MarkerData>`, `pub fn ensure_marker(doc, name, m: &MarkerData, s: f64) -> String`, `pub fn apply(doc, store, shapes: &[NodeId], tname, flags: [bool; 3], size) -> Result<(), String>`, `pub fn template_name(cli) -> Result<String, String>`, `pub fn run`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/favorite_markers.rs` (drop the two `#[allow(dead_code)]` lines now that the helpers are used):

```rust
fn fm(svg: &str, store: &std::path::Path, extra: &[&str]) -> Result<(String, Vec<String>), String> {
    let store = format!("--store={}", store.display());
    let mut a = vec!["--tool=favorite-markers", store.as_str()];
    a.extend(extra);
    let out = sciink::run(&args(&a), svg.as_bytes())?;
    Ok((String::from_utf8(out.svg).unwrap(), out.messages))
}
fn ok(svg: &str, store: &std::path::Path, extra: &[&str]) -> (String, Vec<String>) {
    fm(svg, store, extra).unwrap_or_else(|e| panic!("favorite-markers failed: {e}"))
}
fn style_of(n: roxmltree::Node) -> sciink::style::Style {
    n.attribute("style").map(sciink::style::Style::parse).unwrap_or_default()
}
fn markers(d: &roxmltree::Document) -> Vec<roxmltree::Node<'_, '_>> {
    d.descendants().filter(|n| n.has_tag_name("marker")).collect()
}
const SHAPES: &str = r##"<g id="g"><path id="p" d="M0,0 L10,0" style="fill:none;stroke:#000"/><rect id="r" x="0" y="5" width="4" height="4" style="stroke:#000;marker-mid:url(#old)"/><text id="t" style="font-size:4px">no markers</text></g><line id="l" x1="0" y1="20" x2="10" y2="20" style="stroke:#00f"/>"##;

#[test]
fn apply_creates_one_marker_per_position_and_size_and_reuses_it() {
    let store = tmp_store("apply");
    let _ = std::fs::remove_file(&store);
    let svg = format!(r#"<svg {NS}>{SHAPES}</svg>"#);
    // Triangle (index 1), start + end, size 100
    let (s, msgs) = ok(&svg, &store, &["--tab=markers", "--template=1", "--smarker=true", "--emarker=true", "--id=g", "--id=l"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let mk = markers(&d);
    assert_eq!(mk.len(), 2, "one start and one end marker, shared by the three shapes");
    let start = mk.iter().find(|m| m.attribute("id").unwrap().contains("FMTrianglestart")).unwrap();
    let end = mk.iter().find(|m| m.attribute("id").unwrap().contains("FMTriangleend")).unwrap();
    assert_eq!(start.attribute(("http://www.inkscape.org/namespaces/inkscape", "stockid")), Some("TriangleInM"));
    assert_eq!(start.attribute("orient"), Some("auto"));
    assert_eq!(start.parent().unwrap().tag_name().name(), "defs", "markers live in the root defs");
    let g = start.first_element_child().unwrap();
    assert_eq!(g.tag_name().name(), "g");
    assert_eq!(g.attribute("transform"), None, "size 100 % is the identity: no attribute");
    let p = g.first_element_child().unwrap();
    assert_eq!(p.attribute("transform"), Some("scale(-0.4, -0.4)"));
    assert_eq!(p.attribute("d"), Some("M 5.77,0.0 L -2.88,5.0 L -2.88,-5.0 L 5.77,0.0 z "));
    for id in ["p", "r", "l"] {
        let st = style_of(by_id(&d, id));
        assert_eq!(st.get("marker-start"), Some(format!("url(#{})", start.attribute("id").unwrap()).as_str()), "{id}");
        assert_eq!(st.get("marker-end"), Some(format!("url(#{})", end.attribute("id").unwrap()).as_str()), "{id}");
        assert_eq!(st.get("marker-mid"), None, "{id}: an unchecked position is removed");
    }
    assert_eq!(style_of(by_id(&d, "t")).get("marker-start"), None, "text takes no markers");
    // a second run at 50 % adds new markers with the scale; a third run at 50 % reuses them
    let (s2, _) = ok(&s, &store, &["--tab=markers", "--template=1", "--smarker=true", "--size=50", "--id=p"]);
    let d2 = roxmltree::Document::parse(&s2).unwrap();
    assert_eq!(markers(&d2).len(), 3);
    let half = markers(&d2).into_iter().find(|m| m.first_element_child().unwrap().attribute("transform").is_some()).unwrap();
    assert_eq!(half.first_element_child().unwrap().attribute("transform"), Some("scale(0.5,0.5)"));
    assert_eq!(style_of(by_id(&d2, "p")).get("marker-start"), Some(format!("url(#{})", half.attribute("id").unwrap()).as_str()));
    assert_eq!(style_of(by_id(&d2, "p")).get("marker-end"), None, "end unchecked this time: removed");
    let (s3, _) = ok(&s2, &store, &["--tab=markers", "--template=1", "--smarker=true", "--size=50", "--id=r"]);
    assert_eq!(markers(&roxmltree::Document::parse(&s3).unwrap()).len(), 3, "reused");
    // an empty selection is a message, not an error
    let (_, msgs) = ok(&svg, &store, &["--tab=markers", "--template=0"]);
    assert_eq!(msgs, vec!["favorite-markers: nothing selected".to_string()]);
}

#[test]
fn template_selection_and_errors() {
    let store = tmp_store("errors");
    let _ = std::fs::remove_file(&store);
    let svg = format!(r#"<svg {NS}>{SHAPES}</svg>"#);
    let e = fm(&svg, &store, &["--tab=markers", "--template=3", "--smarker=true", "--id=p"]).unwrap_err();
    assert!(e.contains("template name"), "custom without a name: {e}");
    let e = fm(&svg, &store, &["--tab=markers", "--template=3", "--custom_name=Nope", "--smarker=true", "--id=p"]).unwrap_err();
    assert!(e.contains("'Nope'") && e.contains("Arrow, Triangle, Distance"), "{e}");
    let e = fm(&svg, &store, &["--tab=markers", "--template=7", "--smarker=true", "--id=p"]).unwrap_err();
    assert!(e.contains("template"), "{e}");
    // Arrow (0) and Distance (2) resolve to the built-ins; Distance start has three paths
    let (s, _) = ok(&svg, &store, &["--tab=markers", "--template=2", "--smarker=true", "--id=p"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let mk = &markers(&d)[0];
    assert!(mk.attribute("id").unwrap().starts_with("FMDistancestart"));
    assert_eq!(mk.first_element_child().unwrap().children().filter(|c| c.has_tag_name("path")).count(), 3);
    let e = fm(&svg, &store, &["--tab=addremove", "--addt=true", "--template_name=X", "--id=t"]).unwrap_err();
    assert!(e.contains("Select a path"), "{e}");
    let e = fm(&svg, &store, &["--tab=addremove", "--addt=true", "--id=p"]).unwrap_err();
    assert!(e.contains("name"), "{e}");
}

#[test]
fn add_remove_and_list_round_trip_through_the_store() {
    let store = tmp_store("addremove");
    let _ = std::fs::remove_file(&store);
    let svg = format!(
        r##"<svg {NS}><defs><marker id="m1" orient="auto" refX="1"><g transform="scale(2)"><path id="mp" d="M0,0 L1,1" style="fill:#f00"/></g></marker><marker id="m2"><path d="M0,0 h2"/><rect width="1" height="1"/></marker></defs><path id="p" d="M0,0 L10,0" style="stroke:#000;marker-start:url(#m1);marker-end:url(#m2)"/></svg>"##
    );
    let (s, msgs) = ok(&svg, &store, &["--tab=addremove", "--addt=true", "--template_name= My Arrows ", "--id=p"]);
    assert_eq!(msgs, vec!["Templates successfully updated!".to_string()]);
    assert_eq!(s, svg, "the add/remove page never edits the document");
    let st = Store::load(&store).unwrap();
    assert_eq!(st.templates().last().map(String::as_str), Some("My Arrows"), "trimmed");
    let t = st.get("My Arrows").unwrap();
    assert_eq!(t[0].as_ref().unwrap().attrs, kv(&[("orient", "auto"), ("refX", "1")]));
    assert_eq!(t[0].as_ref().unwrap().paths, vec![kv(&[("d", "M0,0 L1,1"), ("style", "fill:#f00")])], "paths from the first-child group, ids dropped");
    assert!(t[1].is_none(), "no mid marker on the source");
    assert_eq!(t[2].as_ref().unwrap().paths, vec![kv(&[("d", "M0,0 h2")])], "paths directly under the marker; the rect is not a path");
    // apply the new template elsewhere: custom name, whitespace removed from the marker id
    let target = format!(r#"<svg {NS}><path id="q" d="M0,0 L5,5" style="stroke:#000"/></svg>"#);
    let (s2, _) = ok(&target, &store, &["--tab=markers", "--template=3", "--custom_name=My Arrows", "--smarker=true", "--size=200", "--id=q"]);
    let d = roxmltree::Document::parse(&s2).unwrap();
    let mk = &markers(&d)[0];
    assert!(mk.attribute("id").unwrap().starts_with("FMMyArrowsstart"), "{}", mk.attribute("id").unwrap());
    assert_eq!(mk.attribute("refX"), Some("1"));
    assert_eq!(mk.first_element_child().unwrap().attribute("transform"), Some("scale(2,2)"));
    // list, remove, remove again
    let (_, msgs) = ok(&svg, &store, &["--tab=addremove", "--list=true"]);
    assert_eq!(msgs, vec!["favorite-markers: stored templates: Arrow, Triangle, Distance, My Arrows".to_string()]);
    let (_, msgs) = ok(&svg, &store, &["--tab=addremove", "--remt=true", "--template_rem=My Arrows"]);
    assert_eq!(msgs, vec!["Templates successfully updated!".to_string()]);
    assert!(Store::load(&store).unwrap().get("My Arrows").is_none());
    let (_, msgs) = ok(&svg, &store, &["--tab=addremove", "--remt=true", "--template_rem=My Arrows"]);
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(msgs[0].starts_with("warning: ") && msgs[0].contains("My Arrows"), "{}", msgs[0]);
    std::fs::remove_file(&store).unwrap();
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test favorite_markers 2>&1 | tail -15`
Expected: compile errors / `Err("the favorite-markers tool is not implemented yet")`.

- [ ] **Step 3: Implement the tool**

Append to `src/tools/favorite_markers.rs` (imports at the top: `use std::ffi::OsString; use clap::Parser; use crate::Output; use crate::cli::{Common, inx_bool}; use crate::geom::Affine; use crate::ops::cleanup::url_id; use crate::ops::style::remove_inline; use super::first_line;`):

```rust
#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct FavoriteMarkersCli {
    #[command(flatten)]
    pub common: Common,
    /// `markers` | `addremove`
    #[arg(long, default_value = "markers")]
    pub tab: String,
    /// 0 Arrow, 1 Triangle, 2 Distance, 3 the custom name below (upstream's argparse default is 1)
    #[arg(long, default_value_t = 1)]
    pub template: u8,
    #[arg(long, default_value = "")]
    pub custom_name: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub smarker: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub mmarker: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub emarker: bool,
    /// Percent
    #[arg(long, default_value_t = 100.0)]
    pub size: f64,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub addt: bool,
    #[arg(long, default_value = "")]
    pub template_name: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub remt: bool,
    /// The name to remove (upstream: an index into a self-rewritten dropdown)
    #[arg(long, default_value = "")]
    pub template_rem: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub list: bool,
    /// Store file (tests; default `store_path(None)`).
    #[arg(long, hide = true)]
    pub store: Option<PathBuf>,
}

/// `FM:290–313`: the marker a `marker-*` value points at, as template data; `None` for `none`,
/// an empty value, a missing marker or a marker without children.
pub fn marker_props(doc: &Doc, murl: Option<&str>) -> Option<MarkerData> {
    let id = url_id(murl?.trim())?;
    let mk = doc.by_id(id)?;
    let first = doc.children(mk).find(|&c| doc.is_element(c))?;
    let parent = if doc.tag(first) == "g" { first } else { mk };
    let paths = doc
        .children(parent)
        .filter(|&k| doc.is_element(k) && doc.tag(k) == "path")
        .map(|k| attrs_of(doc, k, &["id"]))
        .collect();
    Some(MarkerData { attrs: attrs_of(doc, mk, &["id"]), paths })
}

/// `FM:317–350`: the id of a marker named `name` (whitespace removed) at scale `s` under the root
/// `<defs>` — an existing one whose first child `<g>` has that scale, else a new one.
pub fn ensure_marker(doc: &mut Doc, name: &str, m: &MarkerData, s: f64) -> String {
    let name: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let defs = doc.defs();
    let candidates: Vec<NodeId> = doc
        .descendants(defs)
        .filter(|&n| doc.is_element(n) && doc.tag(n) == "marker" && doc.attr(n, "id").is_some_and(|i| i.contains(&name)))
        .collect();
    for mk in candidates {
        let Some(g) = doc.children(mk).find(|&c| doc.is_element(c)) else { continue };
        if doc.tag(g) != "g" {
            continue;
        }
        let [a, _, _, d, _, _] = doc.transform(g).as_coeffs();
        if (a - s).abs() < 0.01 && (d - s).abs() < 0.01 {
            return doc.attr(mk, "id").unwrap_or_default().to_string();
        }
    }
    let mk = doc.new_element("marker");
    for (k, v) in &m.attrs {
        if k != "id" {
            doc.set_attr(mk, k, v.clone());
        }
    }
    let g = doc.new_element("g");
    doc.append_child(mk, g);
    doc.set_transform(g, Affine::scale(s));
    for p in &m.paths {
        let pe = doc.new_element("path");
        for (k, v) in p {
            if k != "id" {
                doc.set_attr(pe, k, v.clone());
            }
        }
        doc.append_child(g, pe);
    }
    doc.append_child(defs, mk);
    let id = doc.new_id(&name);
    doc.set_attr(mk, "id", id.clone());
    id
}

/// `FM:446–472` over `shapes`: checked positions get the template's marker, unchecked ones lose
/// their inline `marker-*`.
pub fn apply(doc: &mut Doc, store: &Store, shapes: &[NodeId], tname: &str, flags: [bool; 3], size: f64) -> Result<(), String> {
    let t = store.get(tname).ok_or_else(|| {
        format!("Template '{tname}' is not stored. Stored templates: {}", store.templates().join(", "))
    })?;
    let s = size / 100.0;
    for &el in shapes {
        for (i, pos) in POSITIONS.iter().enumerate() {
            let prop = format!("marker-{pos}");
            match (flags[i], &t[i]) {
                (true, Some(m)) => {
                    let id = ensure_marker(doc, &format!("FM{tname}{pos}"), m, s);
                    doc.set_style(el, &prop, &format!("url(#{id})"));
                }
                _ => remove_inline(doc, el, &prop),
            }
        }
    }
    Ok(())
}

/// The template the Markers page selects: three fixed built-in names or the custom name.
pub fn template_name(cli: &FavoriteMarkersCli) -> Result<String, String> {
    match cli.template {
        0 => Ok("Arrow".to_string()),
        1 => Ok("Triangle".to_string()),
        2 => Ok("Distance".to_string()),
        3 => {
            let n = cli.custom_name.trim();
            if n.is_empty() {
                Err("Custom template selected: type its template name in the box below the template list.".to_string())
            } else {
                Ok(n.to_string())
            }
        }
        other => Err(format!("unknown template option {other} (0–3)")),
    }
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = FavoriteMarkersCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut messages: Vec<String> = Vec::new();
    let path = store_path(cli.store.as_deref());
    let mut store = Store::load(&path)?;
    // FM:368–370: the selection and its descendants, shapes only, each once
    let mut shapes: Vec<NodeId> = Vec::new();
    for r in doc.selection(&cli.common.ids) {
        for n in doc.descendants(r).filter(|&n| doc.is_element(n) && SHAPE_TAGS.contains(&doc.tag(n))) {
            if !shapes.contains(&n) {
                shapes.push(n);
            }
        }
    }
    if cli.tab == "addremove" {
        if cli.addt {
            let name = cli.template_name.trim();
            if name.is_empty() {
                return Err("Give the new template a name.".to_string());
            }
            let Some(&first) = shapes.first() else {
                return Err("Select a path whose markers should become the template.".to_string());
            };
            let sty = doc.specified_style(first);
            let t: Template = [
                marker_props(&doc, sty.get("marker-start")),
                marker_props(&doc, sty.get("marker-mid")),
                marker_props(&doc, sty.get("marker-end")),
            ];
            store.set(name, &t);
        }
        if cli.remt {
            let name = cli.template_rem.trim();
            if !store.remove(name) {
                messages.push(format!("warning: template '{name}' is not stored; nothing removed"));
            }
        }
        if cli.addt || cli.remt {
            store.save(&path)?;
            messages.push("Templates successfully updated!".to_string());
        }
        if cli.list {
            messages.push(format!("favorite-markers: stored templates: {}", store.templates().join(", ")));
        }
    } else if shapes.is_empty() {
        messages.push("favorite-markers: nothing selected".to_string());
    } else {
        let tname = template_name(&cli)?;
        apply(&mut doc, &store, &shapes, &tname, [cli.smarker, cli.mmarker, cli.emarker], cli.size)?;
    }
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
```

`src/lib.rs`: replace the remaining placeholder arm with `"favorite-markers" => tools::favorite_markers::run(argv, input),` and drop the "not implemented yet" arm entirely — every tool is implemented now. `tests/cli.rs`: delete the test `unimplemented_tool_echoes_input` (it needs a tool that is not implemented; none is left) and, if that leaves an unused helper or import, remove it too. `Doc::specified_style` returns `Rc<Style>`; `sty.get` works through it. `doc.attr(mk, "id").unwrap_or_default()` needs `Option<&str>::unwrap_or_default` (stable since 1.83; the crate requires ≥ 1.85).

Create `inx/favorite_markers.inx`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Favorite Markers (sciink)</name>
    <id>org.sciink.favorite-markers</id>
    <param name="tool" type="string" gui-hidden="true">favorite-markers</param>
    <param name="tab" type="notebook">
        <page name="markers" gui-text="Markers">
            <label>Stores your favorite markers for convenient access.</label>
            <param name="template" type="optiongroup" appearance="combo" gui-text="Template">
                <option value="0">Arrow</option>
                <option value="1">Triangle</option>
                <option value="2">Distance</option>
                <option value="3">Custom (name below)</option>
            </param>
            <param name="custom_name" type="string" gui-text="Custom template name" gui-description="A template stored on the Add/remove page. Use 'List stored templates' there to see the names."></param>
            <param name="smarker" type="bool" gui-text="Start marker?">false</param>
            <param name="mmarker" type="bool" gui-text="Mid marker?">false</param>
            <param name="emarker" type="bool" gui-text="End marker?">true</param>
            <param name="size" type="float" precision="1" min="0" max="10000" gui-text="Size (%)">100</param>
        </page>
        <page name="addremove" gui-text="Add/remove templates">
            <param name="addt" type="bool" gui-text="Add selected path's markers as a new template?">false</param>
            <param name="template_name" type="string" gui-text="New template name"></param>
            <param name="remt" type="bool" gui-text="Remove a template?">false</param>
            <param name="template_rem" type="string" gui-text="Template to remove (name)"></param>
            <param name="list" type="bool" gui-text="List stored templates?" gui-description="Shows the stored template names when you click Apply.">false</param>
            <label>Templates are stored in sciink/favorite_markers.svg in Inkscape's profile folder; changes take effect at once.</label>
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

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test favorite_markers 2>&1 | tail -15`
Expected: 7 passed.

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add src/tools/favorite_markers.rs src/lib.rs inx/favorite_markers.inx tests/favorite_markers.rs tests/cli.rs
git commit -m "feat(favorite-markers): apply, add, remove and list marker templates; .inx and dispatch"
```

---

### Task 3: Upstream oracle, README, CHANGELOG, spec deviations, packaging check

**Files:**
- Create: `tests/favorite_markers_fixtures.rs`, `CHANGELOG.md`
- Modify: `README.md`, `docs/spec/02-geometry-tools.md`

- [ ] **Step 1: Write the oracle**

Create `tests/favorite_markers_fixtures.rs`:

```rust
//! Favorite Markers against upstream's reference (`--id=path6928 --id=path6952 --smarker=True
//! --tab=markers` on Other_tests.svg, template index 1 = Triangle): one shared start marker,
//! same attributes, same path, both paths pointing at it.
mod support;

use std::ffi::OsString;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink").chain(v.iter().copied()).map(OsString::from).collect()
}
fn attrs_sans_id(n: roxmltree::Node) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = n
        .attributes()
        .filter(|a| a.name() != "id")
        .map(|a| (format!("{}:{}", a.namespace().unwrap_or(""), a.name()), a.value().to_string()))
        .collect();
    v.sort();
    v
}

#[test]
fn start_marker_matches_the_reference() {
    let Some(dir) = support::upstream_data_dir() else { return };
    let input = std::fs::read(dir.join("svg/Other_tests.svg")).unwrap();
    let reference = std::fs::read_to_string(dir.join("refs/favorite_markers__--id__path6928__--id__path6952__--smarker__True__--tab__markers__Other_tests__svg.out")).unwrap();
    let store = std::env::temp_dir().join(format!("sciink-fm-{}-oracle.svg", std::process::id()));
    let _ = std::fs::remove_file(&store);
    let store_arg = format!("--store={}", store.display());
    let out = sciink::run(&args(&["--tool=favorite-markers", "--tab=markers", "--template=1", "--smarker=true", "--id=path6928", "--id=path6952", &store_arg]), &input).unwrap();
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    let ours = String::from_utf8(out.svg).unwrap();
    let (d_in, d_ours, d_ref) = (
        roxmltree::Document::parse(std::str::from_utf8(&input).unwrap()).unwrap(),
        roxmltree::Document::parse(&ours).unwrap(),
        roxmltree::Document::parse(&reference).unwrap(),
    );
    let count = |d: &roxmltree::Document| d.descendants().filter(|n| n.has_tag_name("marker")).count();
    assert_eq!(count(&d_ours), count(&d_in) + 1, "exactly one marker added");
    assert_eq!(count(&d_ref), count(&d_in) + 1);
    let new_marker = |d: &roxmltree::Document<'_>| {
        let ids: std::collections::HashSet<&str> = d_in.descendants().filter_map(|n| n.attribute("id")).collect();
        d.descendants().find(|n| n.has_tag_name("marker") && !ids.contains(n.attribute("id").unwrap_or(""))).unwrap()
    };
    let (mo, mr) = (new_marker(&d_ours), new_marker(&d_ref));
    assert!(mo.attribute("id").unwrap().starts_with("FMTrianglestart"));
    assert_eq!(attrs_sans_id(mo), attrs_sans_id(mr), "marker attributes");
    let (go, gr) = (mo.first_element_child().unwrap(), mr.first_element_child().unwrap());
    assert_eq!(go.tag_name().name(), "g");
    assert_eq!(go.attribute("transform"), gr.attribute("transform"), "size 100 %: no transform on either");
    assert_eq!(attrs_sans_id(go.first_element_child().unwrap()), attrs_sans_id(gr.first_element_child().unwrap()), "path attributes");
    for id in ["path6928", "path6952"] {
        let n = d_ours.descendants().find(|n| n.attribute("id") == Some(id)).unwrap();
        let st = sciink::style::Style::parse(n.attribute("style").unwrap());
        assert_eq!(st.get("marker-start"), Some(format!("url(#{})", mo.attribute("id").unwrap()).as_str()), "{id}");
        assert_eq!(st.get("marker-mid"), None);
        assert_eq!(st.get("marker-end"), None);
        let r = d_ref.descendants().find(|n| n.attribute("id") == Some(id)).unwrap();
        let rs = sciink::style::Style::parse(r.attribute("style").unwrap());
        assert!(rs.get("marker-start").is_some_and(|v| v.starts_with("url(#FMTrianglestart")), "{id}: the reference points at its marker too");
    }
    std::fs::remove_file(&store).unwrap();
}
```

- [ ] **Step 2: Run the oracle**

Run: `cargo test --test favorite_markers_fixtures 2>&1 | tail -8` — passes (or returns early without the upstream data).

- [ ] **Step 3: README, CHANGELOG, spec**

README: "nine menu entries — five tools" → "ten menu entries — six tools"; after the Text Ghoster bullet add:

```markdown
- **Extensions ▸ Scientific ▸ Favorite Markers** — puts a stored marker template (start, mid, end)
  on the selected paths at the size you choose; store the markers of a selected path as a new
  template and remove templates on the second page, no restart needed. Arrow, Triangle and
  Distance come built in.
```

README "Developing": list every oracle in the one command:

```
SCIINK_SYSTEM_FONTS=1 cargo test --test text_fixtures --test text_tools --test text_ghoster --test flattener_fixtures --test scaler_fixtures --test homogenizer_fixtures -- --ignored --test-threads=1
```

and add one line after it: "The non-ignored fixture tests (`cargo test --test '*_fixtures'`) run whenever `tests/upstream/data` is present and are skipped otherwise."

Create `CHANGELOG.md`:

```markdown
# Changelog

## 0.1.0 (unreleased)

First release with all six tools of Scientific-Inkscape's core, as one native binary per platform
(macOS universal, Windows x64, Linux x64) with no Python dependency:

- **Flattener** — deep ungroup, clone unlinking, matplotlib minus signs and thin rectangles restored,
  the text pipeline (kerning removal, merges, splits, justification, font replacement), duplicate
  and white-background-rectangle removal.
- **Scaler** — Correction and Matching modes with tick, text and group preservation; Advanced-tab
  markings.
- **Homogenizer** — font size, font family (Inkscape font specifications), text distortion, stroke
  width, transform fusing, clip/mask removal; plot-aware text placement.
- **Text Ghoster**, **Combine by Color**, **Favorite Markers**.
- Diagnostics (About) and three debug tools (Font Probe, Text Highlight, Text Fix).
- One-line installers for macOS/Linux (`install.sh`) and Windows (`install.ps1`).

Known differences from the Python original are listed in `docs/spec/02-geometry-tools.md`
("Deliberate deviations") and `docs/spec/01-text-engine.md`.

## 0.1.0-alpha.1 — 2026-09-08

Release pipeline and the About tool only.
```

`docs/spec/02-geometry-tools.md`: append

```markdown
## Deliberate deviations (Plan 8)
- Favorite Markers stores templates as an SVG document (`favorite_markers.svg`, markers tagged
  `sciink:template`/`sciink:position`) written with the crate's own DOM, not JSON, and seeds it
  with the built-ins on first use; a hidden `--store <path>` parameter overrides the location.
- The Markers page selects Arrow, Triangle, Distance or "Custom (name below)"; the Add/remove page
  removes by typed name and can list the stored names — upstream indexes a dropdown it rewrites
  in its own `.inx` (which needs a restart).
- Upstream's crash paths are errors with a message: a template index out of range, an empty
  template name, no shape selected when adding, a custom name that is not stored; removing an
  unknown name is a warning.
- A marker written at 100 % carries no `transform` on its `<g>` (identity), as inkex writes it.
- Path ids are dropped when a template is captured (upstream keeps them in the pickle and skips
  them when applying).
- `needs-live-preview` stays off (upstream's value): a preview on the Add/remove page would rewrite
  the store on every parameter change.
```

Also update §B.3 "Favorite markers" **Storage** paragraph's first sentence to name the SVG store and `--store`.

- [ ] **Step 4: Packaging check**

Run the local packaging check from Plan 2 (a release build, the zip, then the zip's contract test — `dist/out/` is git-ignored):

```bash
cargo build --release 2>&1 | tail -2 && dist/package.sh macos-universal target/release/sciink && dist/test-package.sh dist/out/sciink-macos-universal.zip 2>&1 | tail -5
```

Expected: the last line is `PACKAGE-OK dist/out/sciink-macos-universal.zip` (the asset name is only a label here; the binary is this machine's). Paste the output into the report; a failure here is a finding for the controller, not something to patch silently. Do not commit anything under `dist/out/`.

- [ ] **Step 5: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`

```bash
git add tests/favorite_markers_fixtures.rs README.md CHANGELOG.md docs/spec/02-geometry-tools.md
git commit -m "test(favorite-markers): upstream reference oracle; README, CHANGELOG 0.1.0, Plan 8 deviations"
```

---

## After the plan (controller, not tasks)

Merge the branch (authorised), then `gh workflow run release.yml --ref main` as the release dry run (the publish job is tag-gated, so only the three builds and the artifact uploads execute); wait for it, record the run URL and artifact sizes, and stop: the manual Inkscape pass (`dist/dev-install.sh`, every tool from the menu on a matplotlib SVG and a PDF import) and the `v0.1.0` tag are the owner's steps.

## Out of scope (deferred)

Notarisation; the benchmark table (blocked on the Inkscape bundle repair); Autoexporter and Gallery Viewer; migrating an upstream `favorite_markers.settings` pickle.

## Self-review notes (controller)

- Spec coverage (§B.3 Favorite markers): `get_marker_props` → `marker_props`; `set_marker_props` → `ensure_marker` + `apply`; apply over shapes → Task 2 `run`; add/remove → Task 2; storage → Task 1 (SVG instead of JSON, deviation); built-ins → `BUILTINS`; UX (dropdown + typed name, `list`) → Task 2 `.inx`; §C.5 (b) reference row → Task 3.
- Type consistency: `Template = [Option<MarkerData>; 3]` everywhere; `Store::{get, set, remove, templates, load, save}` used identically in Tasks 1–3; `attrs_of(doc, n, skip)` shared by `Store::get` and `marker_props`; `POSITIONS` indexes `flags`/`Template` in the same order.
- Placeholder scan: every step carries code or an exact command; the oracle reads the reference file.
