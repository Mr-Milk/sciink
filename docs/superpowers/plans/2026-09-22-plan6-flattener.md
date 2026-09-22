# Plan 6 — The Flattener Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the tool people install the suite for — **Extensions ▸ Scientific ▸ Flattener** (spec §B.3 "Flattener", upstream `flatten_plots.py`): deep ungroup with clip/mask composition and clone unlinking, the rectangle passes (white-rectangle detection, matplotlib minus-sign reversion, thin-rectangle → stroke), font replacement, language-switch removal, the Plan 4 text pipeline (`remove_kerning`), text-clip removal, overlapping-duplicate removal, white-background-rectangle removal, and the end-of-run cleanup — plus the `--testmode` switch that every upstream Flattener reference was produced with, verified against those references and by a resvg pixel-diff invariance oracle.

**Architecture:** One tool module `src/tools/flattener.rs` built entirely on `sciink::ops` (Plan 5) and `text::kerning::remove_kerning` (Plan 4), organised as the seven phases of upstream's `effect()` in the same order, each a `pub fn` over the working sets upstream keeps (`seld` = selection plus descendants, `ngs` = its non-container members). Every phase re-derives what it needs from the live document (attached nodes only, document order via `Doc::descendants`), so no phase depends on another's bookkeeping beyond the `Vec<NodeId>` it hands on. Two small `ops::cleanup` additions (`strip_whitespace`, `strip_attr`) and a `Doc::comment` accessor complete the library. A resvg-based render harness (dev-dependency) gives the first appearance-invariance oracle, exercised on the ungroup-only Flattener and on Combine by Color.

**Tech Stack:** Rust 2024 (rustc ≥ 1.85), kurbo 0.13, quick-xml 0.42, clap 4.6, fontdb/rustybuzz/ttf-parser (existing); **new dev-dependency** `resvg = "0.48.1"` (re-exports `usvg` and `tiny_skia`; renders with a `fontdb::Database` we fill from `tests/fonts`). Vendored test fonts: DejaVu Sans, Roboto.

**Spec:** `docs/spec/02-geometry-tools.md` — §B.3 "Flattener" (steps 1–9), §B.2 "Housekeeping" (`strip_whitespace`, `gc_created_clips`), §B.4 (`inkscape-scientific-flattenexclude`, `mpl_comment`, `unlinked_clone`), "Deliberate deviations (Plan 5)"; `docs/spec/01-text-engine.md` §A.1 (the Flattener's option gating and justification map); `docs/spec/03-infrastructure.md` §C.3 (`.inx`/CLI contract, `--testmode`), §C.5 (b)–(c) (golden and invariance oracles). Upstream references (read-only, never copied into the crate): `F` = `flatten_plots.py`, `DH` = `dhelpers.py`, `RK` = `remove_kerning.py`, all under `~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/scientific_inkscape/`. Upstream test data behind the symlink `tests/upstream/data/{svg,refs}` (absent in CI → fixture tests `return` early; `support::upstream_data_dir()`).

## Global Constraints

- Phase order is upstream's (F:151–548): exclusions → selection sets → defs/clips to root → "No objects selected!" → unlink clones → deep ungroup → rectangle passes → text (font replacement, deswitch, `remove_kerning`, text clips) → bbox stage (duplicates, white rectangles) → cleanup (gc created clips, dangling refs, whitespace, `unlinked_clone`). Text sub-options are ANDed with `fixtext` (F:178–184); `--testmode` forces every option on, `replacement = "sans-serif"`, `justification = 1`, and operates on the CHILDREN of the duplicated selection (F:132–174).
- Working sets are `Vec<NodeId>` in document order. After any phase that can delete or replace nodes, the next phase filters to attached nodes (`doc.parent(n).is_some()`) before touching them — a detached node passed to `ungroup`/`insert_after` panics.
- Only the ops layer and `text::write` mutate the DOM (`ops::*`, `remove_kerning`); the Flattener itself writes attributes only in the places upstream does: `inkscape-scientific-flattenexclude`, `mpl_comment`, the minus-sign `<text>`, the thin-rectangle stroke style, `font-family`/`-inkscape-font-specification`, text `clip-path`/`mask`, the testmode labels.
- `clip-path`/`mask`/`transform` are attributes (Plan 5 discipline); `Doc::remove_style` is never used for `clip-path`/`mask`.
- Every number written goes through `num::fmt` (`fmt_d`, `fmt_transform`, `Doc::set_transform`); the minus-sign text keeps upstream's literal `x="19.3964" y="626.924"` and `font-size:999.997`.
- Constants (spec §B.3): `RECT_THRESHOLD = 2.49`, dark fill `efflightness < 16/255`, white rect = fill `(255,255,255)` with alpha exactly `1`, duplicate boxes equal within `1e-6·max(size_i, size_j)` with `size = max(w, h)` and neither empty, path equality tolerance `1e-6·size_max`, matplotlib minus sign = first three commands of `M 106,355 H 732 V 272 H 106 Z`.
- Hostile input never panics or hangs: `ops::MAX_NEST`/`MAX_STEPS` bound the recursion; `Option` lookups that can miss return early; a `<use>` whose target is missing stays; a group detached by an earlier ungroup is skipped.
- Fonts load through `FontSystem::load()` (env-driven) exactly where upstream builds character tables: once inside `remove_kerning` and lazily in `Ctx` for the bbox stage. Tests wrap runs in `support::with_vendored_fonts`; oracles needing installed fonts are `#[ignore]` behind the `SCIINK_SYSTEM_FONTS` guard.
- Tools are silent on success: `Output.messages` holds only `warning: …` lines; "No objects selected!" is an `Err` (Inkscape shows it, the document is echoed unchanged).
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` pass on every commit.
- Deviations from upstream only where this plan marks them **Deviation** (with the reason); Task 7 mirrors each into `docs/spec/02-geometry-tools.md` under "Deliberate deviations (Plan 6)".

---

## File structure

| File | Responsibility |
|---|---|
| `src/dom.rs` (modify) | `Doc::comment(n) -> Option<&str>` |
| `src/ops/cleanup.rs` (modify) | `strip_whitespace`, `strip_attr` |
| `src/tools/flattener.rs` (new) | `FlattenerCli`, `Options`, phases, `run` |
| `inx/flattener.inx` (new) | Scientific ▸ Flattener (three notebook pages, upstream's parameter names) |
| `src/tools/mod.rs`, `src/lib.rs` (modify) | module, dispatch |
| `Cargo.toml` (modify) | dev-dependency `resvg = "0.48.1"` |
| `tests/support/mod.rs` (modify) | `render_png`, `pixel_diff_fraction` |
| `tests/ops_cleanup.rs`, `tests/dom.rs` (append); `tests/flattener.rs`, `tests/flattener_fixtures.rs`, `tests/invariance.rs` (new) | tests |
| `README.md`, `docs/spec/02-geometry-tools.md` (modify) | tool listing, deviations |

Test conventions (existing): `mod support;` first; `roxmltree` (dev-dependency) to inspect output; `support::upstream_data_dir()` returns `None` (with a SKIP note) when the upstream fixtures are absent — fixture tests must `return` in that case; `support::with_vendored_fonts(|| …)` around every run that may load fonts. Test markup containing `href="#…"` must sit in an `r##"…"##` raw string (a `"#` closes an `r#"…"#` literal). Every new test file in this plan starts with this preamble (copy it; drop what a file does not use — clippy's `-D warnings` fails on unused imports):

```rust
mod support;

use std::ffi::OsString;

use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
/// Runs the Flattener with upstream's defaults plus `extra` (later flags override earlier ones).
fn flatten(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=flattener", "--tab=Options"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes())).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn has(d: &roxmltree::Document, id: &str) -> bool {
    d.descendants().any(|n| n.attribute("id") == Some(id))
}
fn kids<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> Vec<roxmltree::Node<'a, 'i>> {
    n.children().filter(|c| c.is_element()).collect()
}
fn style_of(n: roxmltree::Node) -> sciink::style::Style {
    n.attribute("style").map(sciink::style::Style::parse).unwrap_or_default()
}
const INKSCAPE_NS: &str = "http://www.inkscape.org/namespaces/inkscape";
const SODIPODI_NS: &str = "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd";
/// roxmltree resolves prefixes: a namespaced attribute is looked up by (namespace, local name),
/// never by its prefixed spelling.
fn nsattr<'a>(n: roxmltree::Node<'a, '_>, ns: &str, local: &str) -> Option<&'a str> {
    n.attribute((ns, local))
}
```

---

### Task 1: Library additions — `Doc::comment`, `cleanup::strip_whitespace`, `cleanup::strip_attr`

**Files:**
- Modify: `src/dom.rs` (append inside `impl Doc`, next to `text`), `src/ops/cleanup.rs` (append)
- Test: `tests/dom.rs`, `tests/ops_cleanup.rs` (append)

**Interfaces:**
- Consumes: `Kind::Comment(String)`, `Doc::{descendants, children, parent, prev_sibling, is_element, is_text, tag, detach, remove_attr, attrs}`.
- Produces: `Doc::comment(&self, n) -> Option<&str>` (a Comment node's text); `cleanup::strip_whitespace(doc: &mut Doc)` (spec §B.2 Housekeeping, F:529–548); `cleanup::strip_attr(doc: &mut Doc, name: &str) -> usize` (removes the attribute from every element, returns how many).

Semantics of `strip_whitespace` in lxml terms (F:542–548): an element's `.tail` is the text node right after it, its `.text` the text node right before its first child. Tails survive only on `tspan textPath flowPara flowRegion flowSpan`; texts survive only on `style text tspan textPath flowRoot flowPara flowRegion flowSpan`. In our DOM a Text node `T` whose previous sibling is an element `S` is `S`'s tail (kept iff `S.tag ∈ TAIL_KEEP`); a Text node whose previous sibling is a comment is that comment's tail (comments are never in `TAIL_KEEP` → removed); a Text node with no previous sibling is its parent's `.text` (kept iff `parent.tag ∈ TEXT_KEEP`). A Text node after another Text node cannot occur (the parser merges runs). CData nodes count as text nodes.

- [ ] **Step 1: Write the failing tests**

Append to `tests/dom.rs`:

```rust
#[test]
fn comment_returns_the_comment_text() {
    let d = Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg"><!-- Text --><g id="g"/></svg>"#).unwrap();
    let c = d.children(d.svg()).find(|&n| d.is_comment(n)).unwrap();
    assert_eq!(d.comment(c), Some(" Text "));
    assert_eq!(d.comment(d.by_id("g").unwrap()), None);
}
```

Append to `tests/ops_cleanup.rs` (add `strip_attr, strip_whitespace` to the `sciink::ops::cleanup` import):

```rust
#[test]
fn strip_whitespace_keeps_text_only_where_inkscape_needs_it() {
    let mut d = doc(&format!(
        "<svg {NS}>\n  <style>rect{{fill:red}}</style>\n  <g id=\"g\">\n    <path id=\"p\"/>\n    <!-- c -->\n    tail of comment\n  </g>\n  <text id=\"t\" xml:space=\"preserve\">a<tspan id=\"s\">b</tspan> c<textPath id=\"tp\">d</textPath> e</text>\n  <flowRoot id=\"f\">x<flowPara id=\"fp\">y</flowPara> z</flowRoot>\n</svg>"
    ));
    strip_whitespace(&mut d);
    let s = out(&d);
    assert!(s.contains("<style>rect{fill:red}</style>"), "a <style>'s text stays: {s}");
    assert!(s.contains("<g id=\"g\"><path id=\"p\"/><!-- c --></g>"), "group whitespace and the comment's tail go: {s}");
    assert!(s.contains("<text id=\"t\" xml:space=\"preserve\">a<tspan id=\"s\">b</tspan> c<textPath id=\"tp\">d</textPath> e</text>"), "text runs and tspan/textPath tails stay: {s}");
    assert!(s.contains("<flowRoot id=\"f\">x<flowPara id=\"fp\">y</flowPara> z</flowRoot>"), "{s}");
    assert!(!s.contains("\n"), "no stray whitespace between elements: {s}");
}

#[test]
fn strip_attr_removes_an_attribute_everywhere() {
    let mut d = doc(&format!(
        r#"<svg {NS} unlinked_clone="True"><g unlinked_clone="True"><path id="p" unlinked_clone="True" d="M0 0"/></g><rect id="r"/></svg>"#
    ));
    assert_eq!(strip_attr(&mut d, "unlinked_clone"), 3);
    assert!(!out(&d).contains("unlinked_clone"), "{}", out(&d));
    assert_eq!(d.attr(id(&d, "p"), "d"), Some("M0 0"));
    assert_eq!(strip_attr(&mut d, "unlinked_clone"), 0);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test dom --test ops_cleanup 2>&1 | grep -E "^error" | head -3`.

- [ ] **Step 3: Implement**

`src/dom.rs`, inside `impl Doc` after `text`:

```rust
    /// Content of a Comment node (without the `<!--`/`-->`).
    pub fn comment(&self, n: NodeId) -> Option<&str> {
        match self.kind(n) {
            Kind::Comment(t) => Some(t),
            _ => None,
        }
    }
```

Append to `src/ops/cleanup.rs`:

```rust
/// Elements whose tail text (the text node right after them) Inkscape needs (F:529).
const TAIL_KEEP: &[&str] = &["tspan", "textPath", "flowPara", "flowRegion", "flowSpan"];
/// Elements whose leading text (the text node before their first child) Inkscape needs (F:530–541).
const TEXT_KEEP: &[&str] = &[
    "style", "text", "tspan", "textPath", "flowRoot", "flowPara", "flowRegion", "flowSpan",
];

/// F:542–548 `strip_whitespace`: removes every text node that is not the leading text of a
/// text-bearing element or the tail of a text-run element — the indentation whitespace a deep
/// ungroup leaves behind — so the written document has no stray whitespace between elements.
pub fn strip_whitespace(doc: &mut Doc) {
    let texts: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|&n| doc.is_text(n))
        .collect();
    for t in texts {
        let keep = match doc.prev_sibling(t) {
            Some(prev) if doc.is_element(prev) => TAIL_KEEP.contains(&doc.tag(prev)),
            Some(_) => false, // a comment's tail
            None => doc
                .parent(t)
                .is_some_and(|p| doc.is_element(p) && TEXT_KEEP.contains(&doc.tag(p))),
        };
        if !keep {
            doc.detach(t);
        }
    }
}

/// Removes `name` from every element; returns how many attributes went (the Flattener drops the
/// `unlinked_clone` markers it used, spec §B.3 step 9 — a deviation from upstream, which keeps them).
pub fn strip_attr(doc: &mut Doc, name: &str) -> usize {
    let nodes: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|&n| doc.is_element(n) && doc.attr(n, name).is_some())
        .collect();
    for &n in &nodes {
        doc.remove_attr(n, name);
    }
    nodes.len()
}
```

`Doc::is_text` and `Doc::prev_sibling` exist (`src/dom.rs`). If `is_text` also matches CData, that is intended.

- [ ] **Step 4: Run to verify success**

`cargo test --test dom --test ops_cleanup 2>&1 | grep -E "^test result|FAILED|panicked"`; then `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/dom.rs src/ops/cleanup.rs tests/dom.rs tests/ops_cleanup.rs
git commit -m "feat(ops): strip_whitespace, strip_attr, Doc::comment"
```

---

### Task 2: Flattener skeleton — CLI, options, exclusions, test mode, selection sets, wiring

**Files:**
- Create: `src/tools/flattener.rs`, `inx/flattener.inx`, `tests/flattener.rs`
- Modify: `src/tools/mod.rs` (`pub mod flattener;`), `src/lib.rs` (dispatch arm; remove `"flattener"` from the not-implemented list)

**Interfaces:**
- Consumes: `cli::{Common, inx_bool}`, `tools::first_line`, `Doc::{selection, descendants, children, deep_clone, insert_before, attr, set_attr, remove_attr, is_element, tag, parent}`, `ops::Ctx`, `cleanup::{strip_whitespace, strip_attr}`.
- Produces (all `pub` in `tools::flattener`):
  - `FlattenerCli` (clap) with upstream's parameters (`tab`, `deepungroup`, `fixtext`, `revertpaths`, `removeduppaths`, `removerectw`, `splitdistant`, `mergenearby`, `removemanualkerning`, `mergesubsuper`, `reversions`, `removetextclips`, `justification: u8`, `setreplacement`, `replacement: String`, `markexc: u8`; hidden `testmode`, `debugparser`, `v: String`)
  - `pub struct Options { pub deepungroup, pub fixtext, pub revertpaths, pub removeduppaths, pub removerectw, pub splitdistant, pub mergenearby, pub removemanualkerning, pub mergesubsuper, pub reversions, pub removetextclips, pub setreplacement: bool, pub replacement: String, pub justification: u8 }` with `Options::from_cli(&FlattenerCli)` (ANDing, F:178–184) and `Options::testmode()` (F:162–174)
  - `pub const EXCLUDE_ATTR: &str = "inkscape-scientific-flattenexclude"`
  - `mark_exclusions(doc, sel: &[NodeId], exclude: bool)`
  - `duplicate_for_testmode(doc, sel: &[NodeId]) -> Vec<NodeId>` (the originals' element children, F:132–149)
  - `pub const CONTAINER_TAGS: &[&str] = &["namedview", "defs", "metadata", "foreignObject", "g"]`
  - `working_set(doc, sel: &[NodeId]) -> Vec<NodeId>` (`seld`: selection + descendants, document order, deduplicated, minus elements carrying a non-empty `EXCLUDE_ATTR`)
  - `attached(doc, els: &[NodeId]) -> Vec<NodeId>`, `non_containers(doc, seld: &[NodeId]) -> Vec<NodeId>` (`ngs`), `groups(doc, seld) -> Vec<NodeId>` (`gs`)
  - `run(argv, input) -> Result<Output, String>` — the full phase order with the phases of Tasks 3–6 added as they land (this task: exclusions, test mode, working set, "No objects selected!", cleanup)

- [ ] **Step 1: Write the failing tests**

Create `tests/flattener.rs` with the preamble and:

```rust
#[test]
fn exclusions_tab_marks_and_unmarks_the_selection_and_nothing_else_runs() {
    let svg = format!(r#"<svg {NS}><g id="g"><path id="p" d="M0 0h1"/></g><rect id="r" width="1" height="1"/></svg>"#);
    let (s, msgs) = flatten(&svg, &["--tab=Exclusions", "--markexc=1", "--id=g", "--id=r"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(by_id(&d, "g").attribute("inkscape-scientific-flattenexclude"), Some("True"));
    assert_eq!(by_id(&d, "r").attribute("inkscape-scientific-flattenexclude"), Some("True"));
    assert!(has(&d, "p") && by_id(&d, "p").parent().unwrap().attribute("id") == Some("g"), "no flattening happened");
    let (s, _) = flatten(&s, &["--tab=Exclusions", "--markexc=2", "--id=g"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(by_id(&d, "g").attribute("inkscape-scientific-flattenexclude"), None);
    assert_eq!(by_id(&d, "r").attribute("inkscape-scientific-flattenexclude"), Some("True"), "only the selection changes");
}

#[test]
fn excluded_elements_are_left_alone_and_an_empty_selection_is_an_error() {
    let svg = format!(
        r#"<svg {NS}><g id="keep" inkscape-scientific-flattenexclude="True"><path id="p" d="M0 0h1"/></g><g id="flat"><path id="q" d="M0 0h1"/></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=keep", "--id=flat"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "keep") && by_id(&d, "p").parent().unwrap().attribute("id") == Some("keep"), "excluded group survives untouched");
    assert!(!has(&d, "flat"), "the other group is ungrouped");
    assert_eq!(by_id(&d, "q").parent().unwrap().tag_name().name(), "svg");
    // only excluded/containers selected → upstream's message, document unchanged
    let a = args(&["--tool=flattener", "--tab=Options", "--id=keep"]);
    let err = with_vendored_fonts(|| sciink::run(&a, svg.as_bytes())).err().expect("an error");
    assert!(err.contains("No objects selected!"), "{err}");
    // a <defs> alone is neither a group nor an object (an empty <g> would count, as upstream)
    let empty = format!(r#"<svg {NS}><g id="e"/><defs id="d"/></svg>"#);
    let a = args(&["--tool=flattener", "--tab=Options", "--id=d"]);
    assert!(with_vendored_fonts(|| sciink::run(&a, empty.as_bytes())).is_err());
    let a = args(&["--tool=flattener", "--tab=Options", "--id=e"]);
    assert!(with_vendored_fonts(|| sciink::run(&a, empty.as_bytes())).is_ok(), "an empty group is 'an object' and simply dissolves");
}

#[test]
fn test_mode_duplicates_the_selected_layer_and_flattens_the_original() {
    let svg = format!(
        r#"<svg {NS}><g id="layer1" inkscape:groupmode="layer" inkscape:label="Layer 1"><g id="inner"><path id="p" d="M0 0h1"/></g><rect id="r" width="1" height="1"/></g></svg>"#
    );
    let (s, msgs) = flatten(&svg, &["--id=layer1", "--testmode=true"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let root = d.root_element();
    let top = kids(root);
    assert_eq!(top.len(), 2, "a duplicate layer sits BEFORE the flattened original: {s}");
    let (dup, orig) = (top[0], top[1]);
    assert_eq!(nsattr(dup, INKSCAPE_NS, "label"), Some("Layer 1 original"));
    assert_eq!(nsattr(dup, SODIPODI_NS, "insensitive"), Some("true"));
    assert_eq!(dup.attribute("opacity"), Some("0.3"));
    assert_eq!(dup.attribute("id"), None, "the copy carries no ids");
    assert_eq!(kids(dup).len(), 2, "the copy is the untouched layer");
    assert_eq!(kids(kids(dup)[0]).len(), 1, "…with its nested group intact");
    assert_eq!((orig.attribute("id"), nsattr(orig, INKSCAPE_NS, "label")), (Some("layer1"), Some("Layer 1 flat")));
    let names: Vec<&str> = kids(orig).iter().map(|n| n.tag_name().name()).collect();
    assert_eq!(names, vec!["path", "rect"], "the original's children were flattened in place: {s}");
    assert!(!has(&d, "inner"));
}

#[test]
fn unknown_ids_are_dropped_and_options_are_anded_with_fixtext() {
    use sciink::tools::flattener::{FlattenerCli, Options};
    use clap::Parser;
    let cli = FlattenerCli::try_parse_from(args(&["--tool=flattener", "--fixtext=false", "--splitdistant=true", "--mergenearby=true", "--setreplacement=true", "--removetextclips=true", "--reversions=true", "--justification=3", "--id=x"])).unwrap();
    let o = Options::from_cli(&cli);
    assert!(!o.fixtext && !o.splitdistant && !o.mergenearby && !o.setreplacement && !o.removetextclips && !o.reversions, "text sub-options follow fixtext");
    assert!(o.deepungroup && o.revertpaths && o.removeduppaths && o.removerectw, "defaults");
    assert_eq!((o.justification, o.replacement.as_str()), (3, "Arial"));
    let t = Options::testmode();
    assert!(t.fixtext && t.splitdistant && t.mergenearby && t.setreplacement && t.reversions && t.removetextclips && t.removemanualkerning && t.mergesubsuper && t.deepungroup && t.revertpaths && t.removerectw && t.removeduppaths);
    assert_eq!((t.justification, t.replacement.as_str()), (1, "sans-serif"));
    // hidden upstream parameters are accepted and ignored
    let cli = FlattenerCli::try_parse_from(args(&["--tool=flattener", "--debugparser=true", "--v=1.2", "--id=x"])).unwrap();
    assert!(cli.debugparser);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test flattener 2>&1 | grep -E "^error|not implemented" | head -3`.

- [ ] **Step 3: Implement**

Create `src/tools/flattener.rs`:

```rust
//! The Flattener (spec §B.3; upstream flatten_plots.py): deep ungroup, rectangle reversions,
//! the text pipeline, duplicate and white-rectangle removal, cleanup.

use std::collections::HashSet;
use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::ops::Ctx;
use crate::ops::cleanup::{strip_attr, strip_whitespace};

use super::first_line;

/// Marks an element (and, when it is a container, what upstream calls its "flattening") as
/// excluded (spec §B.4): any non-empty value counts.
pub const EXCLUDE_ATTR: &str = "inkscape-scientific-flattenexclude";
/// Tags that are neither drawn nor flattened themselves (F:221 `gigtags`).
pub const CONTAINER_TAGS: &[&str] = &["namedview", "defs", "metadata", "foreignObject", "g"];

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct FlattenerCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports (`Options`, `Options2`, `Exclusions`).
    #[arg(long, default_value = "Options")]
    pub tab: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub deepungroup: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub fixtext: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub revertpaths: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removeduppaths: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removerectw: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub splitdistant: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergenearby: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removemanualkerning: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergesubsuper: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub reversions: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removetextclips: bool,
    /// 1 = centre, 2 = left, 3 = right, 4 = unchanged.
    #[arg(long, default_value_t = 1)]
    pub justification: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setreplacement: bool,
    #[arg(long, default_value = "Arial")]
    pub replacement: String,
    /// Exclusions page: 1 = mark the selection as not flattened, 2 = flattened again.
    #[arg(long, default_value_t = 1)]
    pub markexc: u8,
    /// Upstream's test switch: duplicate the selection, flatten the original's children with every
    /// fix on, `sans-serif` as the replacement family and centred justification (F:132–174).
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false", hide = true)]
    pub testmode: bool,
    /// Accepted for compatibility with upstream's test suite; the Text Highlight tool draws the same rectangles.
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false", hide = true)]
    pub debugparser: bool,
    /// Accepted for compatibility; unused.
    #[arg(long, default_value = "1.2", hide = true)]
    pub v: String,
}

/// The effective options: text sub-options are ANDed with `fixtext` (F:178–184).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub deepungroup: bool,
    pub fixtext: bool,
    pub revertpaths: bool,
    pub removeduppaths: bool,
    pub removerectw: bool,
    pub splitdistant: bool,
    pub mergenearby: bool,
    pub removemanualkerning: bool,
    pub mergesubsuper: bool,
    pub reversions: bool,
    pub removetextclips: bool,
    pub setreplacement: bool,
    pub replacement: String,
    pub justification: u8,
}

impl Options {
    pub fn from_cli(c: &FlattenerCli) -> Options {
        let t = c.fixtext;
        Options {
            deepungroup: c.deepungroup,
            fixtext: t,
            revertpaths: c.revertpaths,
            removeduppaths: c.removeduppaths,
            removerectw: c.removerectw,
            splitdistant: c.splitdistant && t,
            mergenearby: c.mergenearby && t,
            removemanualkerning: c.removemanualkerning && t,
            mergesubsuper: c.mergesubsuper && t,
            reversions: c.reversions && t,
            removetextclips: c.removetextclips && t,
            setreplacement: c.setreplacement && t,
            replacement: c.replacement.clone(),
            justification: c.justification,
        }
    }

    /// F:162–174: everything on, `sans-serif`, centred.
    pub fn testmode() -> Options {
        Options {
            deepungroup: true,
            fixtext: true,
            revertpaths: true,
            removeduppaths: true,
            removerectw: true,
            splitdistant: true,
            mergenearby: true,
            removemanualkerning: true,
            mergesubsuper: true,
            reversions: true,
            removetextclips: true,
            setreplacement: true,
            replacement: "sans-serif".to_string(),
            justification: 1,
        }
    }
}

/// F:187–194: the Exclusions page sets (`True`) or removes the marker on the selection.
pub fn mark_exclusions(doc: &mut Doc, sel: &[NodeId], exclude: bool) {
    for &el in sel {
        if exclude {
            doc.set_attr(el, EXCLUDE_ATTR, "True");
        } else {
            doc.remove_attr(el, EXCLUDE_ATTR);
        }
    }
}

/// F:132–149 `duplicate_layer1`: every selected element gets a copy inserted right before it —
/// labelled `<label> original`, locked (`sodipodi:insensitive`), at opacity 0.3 — while the
/// original is labelled `<label> flat` and its element children become the selection to flatten.
/// **Deviation:** the copy carries no ids (`Doc::deep_clone` drops them; upstream assigns random ones).
pub fn duplicate_for_testmode(doc: &mut Doc, sel: &[NodeId]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for &el in sel {
        let d = doc.deep_clone(el);
        doc.insert_before(d, el);
        if let Some(label) = doc.attr(el, "inkscape:label").map(str::to_string) {
            doc.set_attr(el, "inkscape:label", format!("{label} flat"));
            doc.set_attr(d, "inkscape:label", format!("{label} original"));
        }
        doc.set_attr(d, "sodipodi:insensitive", "true");
        doc.set_attr(d, "opacity", "0.3");
        out.extend(doc.children(el).filter(|&k| doc.is_element(k)));
    }
    out
}

fn excluded(doc: &Doc, n: NodeId) -> bool {
    doc.attr(n, EXCLUDE_ATTR).is_some_and(|v| !v.trim().is_empty())
}

/// F:195–201 `seld`: the selection (minus excluded elements) and every element under it, in
/// document order, deduplicated, minus the elements that carry the exclusion marker themselves.
pub fn working_set(doc: &Doc, sel: &[NodeId]) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &root in sel {
        if excluded(doc, root) {
            continue;
        }
        for n in doc.descendants(root) {
            if doc.is_element(n) && !excluded(doc, n) && seen.insert(n) {
                out.push(n);
            }
        }
    }
    out
}

/// The members of `els` still in the document.
pub fn attached(doc: &Doc, els: &[NodeId]) -> Vec<NodeId> {
    els.iter()
        .copied()
        .filter(|&n| doc.parent(n).is_some())
        .collect()
}

/// `ngs` (F:224): the attached members of `seld` that are neither containers nor unrendered.
pub fn non_containers(doc: &Doc, seld: &[NodeId]) -> Vec<NodeId> {
    attached(doc, seld)
        .into_iter()
        .filter(|&n| !CONTAINER_TAGS.contains(&doc.tag(n)))
        .collect()
}

/// `gs` (F:223): the attached groups of `seld`.
pub fn groups(doc: &Doc, seld: &[NodeId]) -> Vec<NodeId> {
    attached(doc, seld)
        .into_iter()
        .filter(|&n| doc.tag(n) == "g")
        .collect()
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = FlattenerCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut ctx = Ctx::new();
    let mut sel = doc.selection(&cli.common.ids);
    if cli.tab == "Exclusions" {
        mark_exclusions(&mut doc, &sel, cli.markexc == 1);
        return finish(doc, ctx, false);
    }
    let opts = if cli.testmode {
        sel = duplicate_for_testmode(&mut doc, &sel);
        Options::testmode()
    } else {
        Options::from_cli(&cli)
    };
    let mut seld = working_set(&doc, &sel);
    // (Task 3 inserts: defs/clips to root when deepungroup)
    if groups(&doc, &seld).is_empty() && non_containers(&doc, &seld).is_empty() {
        return Err("No objects selected!".to_string());
    }
    // (Task 3 inserts: unlink clones, deep ungroup)
    let mut ngs = non_containers(&doc, &seld);
    let _ = (&mut seld, &mut ngs, &opts); // consumed by the phases of Tasks 3–6
    finish(doc, ctx, true)
}

/// End of every run: created-clip gc and dangling-reference sweep (`Ctx::finish`), whitespace and
/// `unlinked_clone` markers when the document was flattened (F:513–548, spec §B.3 step 9).
fn finish(mut doc: Doc, mut ctx: Ctx, flattened: bool) -> Result<Output, String> {
    ctx.finish(&mut doc);
    if flattened {
        strip_whitespace(&mut doc);
        strip_attr(&mut doc, "unlinked_clone");
    }
    let messages = ctx.warn.0.iter().map(|w| format!("warning: {w}")).collect();
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
```

The `let _ = (…)` line exists only to keep the skeleton warning-free until Task 3 replaces it; Task 3 removes it.

Register: `src/tools/mod.rs` → `pub mod flattener;`; `src/lib.rs` → `"flattener" => tools::flattener::run(argv, input),` and drop `"flattener"` from the not-implemented list.

Create `inx/flattener.inx` (upstream's parameter names, defaults and page names; spec §C.3):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Flattener (sciink)</name>
    <id>org.sciink.flattener</id>
    <param name="tool" type="string" gui-hidden="true">flattener</param>
    <param name="tab" type="notebook">
        <page name="Options" gui-text="Main options">
            <label appearance="header">Recommended options</label>
            <param name="deepungroup" type="bool" gui-text="Deep ungroup?" gui-description="Remove all groupings, leaving individual objects on canvas">true</param>
            <param name="fixtext" type="bool" gui-text="Apply text fixes?">true</param>
            <param name="revertpaths" type="bool" gui-text="Revert simple paths to strokes?" gui-description="Reverts certain strokes that have been converted to paths back to strokes">true</param>
            <param name="removeduppaths" type="bool" gui-text="Remove overlapping duplicates?" gui-description="When two identical elements overlap, removes the one on the bottom">true</param>
            <label appearance="header">Other options</label>
            <param name="removerectw" type="bool" gui-text="Remove white background rectangles?" gui-description="Removes white-filled rectangles that are behind other objects">true</param>
            <label appearance="header">Note</label>
            <label>For anyone using Inkscape to prepare figures, it is strongly recommended that you change the transformation preferences from Optimized to Preserved in Edit > Preferences > Behavior > Transforms. The Optimized setting can distort certain paths.</label>
        </page>
        <page name="Options2" gui-text="Text fix options">
            <label appearance="header">Recommended options</label>
            <param name="splitdistant" type="bool" gui-text="Split distant text and lines">true</param>
            <param name="mergenearby" type="bool" gui-text="Merge nearby text">true</param>
            <param name="removemanualkerning" type="bool" gui-text="Remove manual kerning">true</param>
            <param name="mergesubsuper" type="bool" gui-text="Merge superscripts and subscripts">true</param>
            <param name="reversions" type="bool" gui-text="Revert known paths to characters">true</param>
            <param name="removetextclips" type="bool" gui-text="Remove text clips and masks">true</param>
            <param name="justification" type="optiongroup" appearance="combo" gui-text="Final text justification">
                <option value="1">Centered</option>
                <option value="2">Left</option>
                <option value="3">Right</option>
                <option value="4">Unchanged</option>
            </param>
            <label appearance="header">Other options</label>
            <param name="setreplacement" type="bool" gui-text="Replace missing fonts">false</param>
            <param name="replacement" type="string" gui-text="Missing font replacement">Arial</param>
        </page>
        <page name="Exclusions" gui-text="Exclusions">
            <label appearance="header">Exclusions</label>
            <label>To mark objects to be excluded from flattening, select them and run the extension with this tab selected.</label>
            <param name="markexc" type="optiongroup" appearance="combo" gui-text="Selected objects should be">
                <option value="1">Not flattened</option>
                <option value="2">Flattened</option>
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

- [ ] **Step 4: Run to verify success**

`cargo test --test flattener 2>&1 | grep -E "^test result|FAILED|panicked"` → the exclusions and options tests pass; `excluded_elements_are_left_alone…` and `test_mode_…` still FAIL on the ungroup assertions (Task 3 makes them pass — commit them failing? No: **mark those two tests `#[ignore = "Task 3"]` in this commit and remove the attribute in Task 3**). Then the gate: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

- [ ] **Step 5: Commit**

```bash
git add src/tools/flattener.rs src/tools/mod.rs src/lib.rs inx/flattener.inx tests/flattener.rs
git commit -m "feat(tools): Flattener skeleton — options, exclusions, test mode, working sets"
```

---

### Task 3: Deep ungroup phase — defs/clips to root, clone unlinking, glyph groups, ungroup

**Files:**
- Modify: `src/tools/flattener.rs` (append the phase functions; extend `run`)
- Test: `tests/flattener.rs` (append; remove the two `#[ignore = "Task 3"]` attributes)

**Interfaces:**
- Consumes: Task 2; `ops::clip::{unlink, ungroup}`, `Doc::{defs, append_child, resolve_href, comment, detach, children, is_comment}`.
- Produces: `move_defs_and_clips_to_root(doc, seld: &mut Vec<NodeId>)` (F:203–218), `unlink_clones(doc, ctx, seld: &mut Vec<NodeId>)` (F:229–246), `deep_ungroup(doc, ctx, seld: &[NodeId], remove_text_clip: bool)` (F:248–274), `pub const MPL_COMMENT: &str = "mpl_comment"`.

Upstream quirk kept (document it in the tests): groups are ungrouped in ascending order of their child count *as counted before any ungroup* (comments count, text nodes do not; `len(list(group))`), ties in document order (Python's sort is stable). A matplotlib text group `<g><!-- label --><defs>…</defs><g><use/></g></g>` therefore has its inner glyph group dissolved first and is then recognised (comment + defs + `unlinked_clone` children) and kept with `mpl_comment`; a text group whose inner group has more children than the outer one is dissolved before its glyphs arrive and loses its comment — exactly upstream's behaviour (F:250–274).

- [ ] **Step 1: Write the failing tests**

Append to `tests/flattener.rs` and delete the two `#[ignore = "Task 3"]` lines from Task 2's tests:

```rust
#[test]
fn deep_ungroup_composes_transforms_styles_and_clips_onto_leaves() {
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="10" height="10"/></clipPath></defs><g id="layer"><g id="a" transform="translate(1,2)" style="fill:red;opacity:0.5" clip-path="url(#c)"><g id="b" transform="scale(2)"><path id="p" d="M0 0h1" style="fill:blue"/></g><rect id="r" width="1" height="1"/></g></g></svg>"#
    );
    // selecting a layer dissolves the layer itself (upstream too: `seld` contains the selection);
    // select the group instead and check its parent
    let (s, msgs) = flatten(&svg, &["--id=a", "--fixtext=false", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let layer = by_id(&d, "layer");
    let names: Vec<Option<&str>> = kids(layer).iter().map(|n| n.attribute("id")).collect();
    assert_eq!(names, vec![Some("p"), Some("r")], "both groups dissolved, order kept: {s}");
    let p = by_id(&d, "p");
    assert_eq!(p.attribute("transform"), Some("matrix(2,0,0,2,1,2)"));
    let st = style_of(p);
    assert_eq!((st.get("fill"), st.get("opacity")), (Some("blue"), Some("0.5")));
    assert!(p.attribute("clip-path").is_some_and(|c| c.starts_with("url(#")), "the group's clip travels down: {s}");
    assert_eq!(by_id(&d, "r").attribute("clip-path"), Some("url(#c)"), "an untransformed child points at the clip itself");
    assert!(!has(&d, "a") && !has(&d, "b"));
    assert!(!s.contains("\n  "), "no indentation whitespace survives: {s}");
}

#[test]
fn selected_defs_and_loose_clips_move_into_the_root_defs() {
    let svg = format!(
        r#"<svg {NS}><defs id="root"/><g id="layer"><defs id="inner"><path id="glyph" d="M0 0h1"/></defs><clipPath id="loose"><rect width="1" height="1"/></clipPath><path id="p" d="M0 0h1" clip-path="url(#loose)"/></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=inner", "--id=loose", "--id=p", "--fixtext=false", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let root = by_id(&d, "root");
    let moved: Vec<Option<&str>> = kids(root).iter().map(|n| n.attribute("id")).collect();
    assert_eq!(moved, vec![Some("inner"), Some("loose")], "appended to the root <defs> in document order: {s}");
    assert_eq!(by_id(&d, "glyph").parent().unwrap().attribute("id"), Some("inner"), "the nested defs is moved whole");
    let layer_kids: Vec<Option<&str>> = kids(by_id(&d, "layer")).iter().map(|n| n.attribute("id")).collect();
    assert_eq!(layer_kids, vec![Some("p")]);
    assert_eq!(by_id(&d, "p").attribute("clip-path"), Some("url(#loose)"));
}

#[test]
fn clones_of_paths_are_unlinked_but_symbol_clones_stay() {
    let svg = format!(
        r##"<svg {NS}><defs><path id="glyph" d="M0 0h1"/><symbol id="sym"><circle id="c" r="1"/></symbol></defs><g id="layer"><g id="g" transform="translate(5,0)"><use id="u" xlink:href="#glyph" x="1"/><use id="us" xlink:href="#sym"/><use id="dangling" xlink:href="#nope"/></g></g></svg>"##
    );
    let (s, _) = flatten(&svg, &["--id=g", "--fixtext=false", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let layer = by_id(&d, "layer");
    let k = kids(layer);
    assert_eq!(k.len(), 3, "{s}");
    assert_eq!((k[0].tag_name().name(), k[0].attribute("id")), ("path", Some("u")), "the clone became a path with the clone's id");
    assert_eq!(k[0].attribute("transform"), Some("translate(6,0)"), "x offset then the group's transform");
    assert_eq!(k[0].attribute("unlinked_clone"), None, "the marker is stripped at the end");
    assert_eq!((k[1].tag_name().name(), k[1].attribute("id")), ("use", Some("us")), "symbol clones are not unlinked by the Flattener");
    assert_eq!((k[2].tag_name().name(), k[2].attribute("id")), ("use", Some("dangling")), "a clone of nothing is left alone");
    assert!(has(&d, "glyph") && has(&d, "sym"), "definitions stay");
}

#[test]
fn matplotlib_glyph_groups_keep_their_comment_as_mpl_comment() {
    // first occurrence of a glyph: matplotlib puts its <path> in a <defs> INSIDE the text group
    let svg = format!(
        r##"<svg {NS}><g id="layer"><g id="text_1"><!-- 0.5 --><defs><path id="DejaVuSans-30" d="M0 0h1v1z"/></defs><g transform="translate(10,20) scale(0.1,-0.1)"><use xlink:href="#DejaVuSans-30"/></g></g><g id="text_2"><!-- 1 --><g transform="translate(30,20)"><use xlink:href="#DejaVuSans-30"/><use xlink:href="#DejaVuSans-30" x="60"/><use xlink:href="#DejaVuSans-30" x="120"/></g></g><g id="already" mpl_comment="kept"><path id="k" d="M0 0h1"/></g></g></svg>"##
    );
    let (s, _) = flatten(&svg, &["--id=layer", "--fixtext=false", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let t1 = by_id(&d, "text_1");
    assert_eq!(t1.attribute("mpl_comment"), Some("0.5"), "{s}");
    assert!(t1.children().all(|c| !c.is_comment()), "the comment itself is removed");
    let t1k: Vec<&str> = kids(t1).iter().map(|n| n.tag_name().name()).collect();
    assert_eq!(t1k, vec!["path"], "the <defs> moved to the root defs first, the inner group dissolved next (1 child < 2), the glyph stays grouped: {s}");
    let glyph = by_id(&d, "DejaVuSans-30");
    assert_eq!(glyph.parent().unwrap().tag_name().name(), "defs");
    assert_eq!(glyph.parent().unwrap().parent().unwrap().tag_name().name(), "defs", "…inside the root <defs>");
    // upstream quirk: a text group with fewer children than its glyph group is dissolved first
    assert!(!has(&d, "text_2"), "{s}");
    assert_eq!(by_id(&d, "already").attribute("mpl_comment"), Some("kept"), "groups already marked are left grouped");
    assert_eq!(by_id(&d, "k").parent().unwrap().attribute("id"), Some("already"));
}

#[test]
fn ungroup_of_a_clipped_out_child_does_not_panic_on_its_dissolved_group() {
    // `inner` (3 children) is processed AFTER `outer` (2 children); ungrouping `outer` clips it out
    // entirely (disjoint rectangles) and deletes it, so when its turn comes it is detached and
    // must be skipped, not touched
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="a"><rect width="1" height="1"/></clipPath><clipPath id="b"><rect x="50" y="50" width="1" height="1"/></clipPath></defs><g id="layer"><g id="outer" clip-path="url(#a)"><g id="inner" clip-path="url(#b)"><path id="p1" d="M0 0h1"/><path id="p2" d="M0 0h1"/><path id="p3" d="M0 0h1"/></g><rect id="r" width="1" height="1"/></g></g></svg>"#
    );
    let (s, msgs) = flatten(&svg, &["--id=outer", "--fixtext=false", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "inner") && !has(&d, "p1") && !has(&d, "p2") && !has(&d, "p3"), "clipped out: {s}");
    assert!(has(&d, "r") && has(&d, "layer"));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test flattener 2>&1 | grep -E "^test |^error" | head`.

- [ ] **Step 3: Implement**

Append to `src/tools/flattener.rs` (add `use crate::ops::clip::{ungroup, unlink};`):

```rust
/// Attribute holding the joined comments of a matplotlib text group (spec §B.4).
pub const MPL_COMMENT: &str = "mpl_comment";

/// F:203–218: every selected `<defs>`, `<clipPath>` and `<mask>` is appended to the root `<defs>`
/// (a `<defs>` moves whole, nested); it and its descendants leave the working set. The root
/// `<defs>` itself (and anything containing it) is never moved.
pub fn move_defs_and_clips_to_root(doc: &mut Doc, seld: &mut Vec<NodeId>) {
    for pass in [&["defs"][..], &["clipPath", "mask"][..]] {
        let movers: Vec<NodeId> = seld
            .iter()
            .copied()
            .filter(|&n| doc.parent(n).is_some() && pass.contains(&doc.tag(n)))
            .collect();
        if movers.is_empty() {
            continue;
        }
        let root = doc.defs(); // created on demand — only when something has to move
        for m in movers {
            if m == root || doc.ancestors(root).any(|a| a == m) {
                continue;
            }
            doc.append_child(root, m);
            let gone: HashSet<NodeId> = doc.descendants(m).collect();
            seld.retain(|n| !gone.contains(n));
        }
    }
}

/// F:229–246: every `<use>` of the working set whose target exists and is not a `<symbol>` is
/// unlinked; the clone leaves the set and the copy's subtree joins it.
pub fn unlink_clones(doc: &mut Doc, ctx: &mut Ctx, seld: &mut Vec<NodeId>) {
    let uses: Vec<NodeId> = seld
        .iter()
        .copied()
        .filter(|&n| doc.parent(n).is_some() && doc.tag(n) == "use")
        .collect();
    for u in uses {
        let Some(target) = doc.resolve_href(u) else {
            continue; // a clone of nothing stays (upstream skips it too)
        };
        if doc.tag(target) == "symbol" {
            continue;
        }
        if let Some(copy) = unlink(doc, ctx, u) {
            seld.retain(|&n| n != u);
            seld.extend(doc.descendants(copy).filter(|&n| doc.is_element(n)));
        }
    }
}

/// F:248–274: groups in ascending order of their child count (comments count, text does not;
/// counted before any ungroup; ties in document order). A group with a comment child whose
/// children are all comments, `<defs>` or unlinked clones is a matplotlib text group: it keeps
/// its glyphs grouped, gets `mpl_comment` = its comments joined by `;`, and loses the comments.
/// A group already carrying `mpl_comment` is kept. Everything else is dissolved with `ungroup`.
pub fn deep_ungroup(doc: &mut Doc, ctx: &mut Ctx, seld: &[NodeId], remove_text_clip: bool) {
    let mut gs: Vec<(usize, NodeId)> = groups(doc, seld)
        .into_iter()
        .map(|g| {
            let n = doc
                .children(g)
                .filter(|&k| doc.is_element(k) || doc.is_comment(k))
                .count();
            (n, g)
        })
        .collect();
    gs.sort_by_key(|&(n, _)| n); // stable: ties keep document order
    for (_, g) in gs {
        if doc.parent(g).is_none() {
            continue; // dissolved or clipped out by an earlier ungroup
        }
        let kids: Vec<NodeId> = doc
            .children(g)
            .filter(|&k| doc.is_element(k) || doc.is_comment(k))
            .collect();
        let has_comment = kids.iter().any(|&k| doc.is_comment(k));
        let glyphish = kids.iter().all(|&k| {
            doc.is_comment(k)
                || doc.tag(k) == "defs"
                || doc.attr(k, "unlinked_clone") == Some("True")
        });
        if has_comment && glyphish {
            let cmnt: Vec<String> = kids
                .iter()
                .filter(|&&k| doc.is_comment(k))
                .map(|&k| {
                    doc.comment(k)
                        .unwrap_or("")
                        .trim_matches(|c| matches!(c, '<' | '!' | '-' | ' ' | '>'))
                        .to_string()
                })
                .collect();
            doc.set_attr(g, MPL_COMMENT, cmnt.join(";"));
            // collect first: the filter borrows `doc` immutably while `detach` needs it mutably
            let comments: Vec<NodeId> = kids.iter().copied().filter(|&k| doc.is_comment(k)).collect();
            for k in comments {
                doc.detach(k);
            }
        } else if doc.attr(g, MPL_COMMENT).is_some() {
            // leave grouped
        } else {
            ungroup(doc, ctx, g, remove_text_clip);
        }
    }
}
```

In `run`, replace the two `// (Task 3 inserts…)` comments and the `let _ = …` line:

```rust
    let mut seld = working_set(&doc, &sel);
    if opts.deepungroup {
        move_defs_and_clips_to_root(&mut doc, &mut seld);
    }
    if groups(&doc, &seld).is_empty() && non_containers(&doc, &seld).is_empty() {
        return Err("No objects selected!".to_string());
    }
    if opts.deepungroup {
        unlink_clones(&mut doc, &mut ctx, &mut seld);
        deep_ungroup(&mut doc, &mut ctx, &seld, opts.removetextclips);
    }
    let mut ngs = non_containers(&doc, &seld);
    let _ = (&mut ngs, &opts); // consumed by the phases of Tasks 4–6
    finish(doc, ctx, true)
```

(`Doc::ancestors` yields the ancestors of a node; `unlink` is the Plan 5 function that returns the replacement node.)

- [ ] **Step 4: Run to verify success**

`cargo test --test flattener 2>&1 | grep -E "^test result|FAILED|panicked"` → 9 passed; gate.

- [ ] **Step 5: Commit**

```bash
git add src/tools/flattener.rs tests/flattener.rs
git commit -m "feat(flattener): deep ungroup — defs to root, clone unlinking, matplotlib glyph groups"
```

---

### Task 4: Rectangle passes — white rectangles, minus-sign reversion, thin rectangles to strokes

**Files:**
- Modify: `src/tools/flattener.rs` (append; extend `run`)
- Test: `tests/flattener.rs` (append)

**Interfaces:**
- Consumes: `ops::bbox::{bbox, is_rectangle, BboxOpts}`, `ops::style::strokefill`, `ops::xform::object_to_path`, `geom::path::{parse_d, path_eq}`, `geom::inverse`, `Doc::{specified, set_style, set_transform, new_element, new_text, insert_before, detach, composed_transform}`, `kurbo::{Affine, BezPath, PathEl, Point}`.
- Produces: `rect_passes(doc, ctx, ngs: &mut Vec<NodeId>, o: &Options) -> Vec<NodeId>` (the white-rectangle candidates, F:277–368), `pub const RECT_THRESHOLD: f64 = 2.49`, `pub const MINUS_D: &str = "M 106,355 H 732 V 272 H 106 Z"`, `is_minus_glyph(d: &str) -> bool`.

**Deviation (documented in Task 7):** the reverted minus sign carries `fill-opacity` when the path's fill was translucent (upstream writes `fill:<rgb>` only); the thin-rectangle `d` is written absolute through `fmt_d` (`M x,y L x,y`) instead of upstream's relative `m … v/h`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/flattener.rs`:

```rust
#[test]
fn matplotlib_minus_glyphs_become_minus_signs() {
    // a real matplotlib minus: the DejaVu glyph path under the usual flip
    let svg = format!(
        r#"<svg {NS}><g id="layer"><g id="t" transform="translate(50,60) scale(0.1,-0.1)"><path id="m" d="M 106,355 H 732 V 272 H 106 Z" style="fill:#336699;fill-opacity:0.5"/></g><path id="other" d="M 106,355 H 732 V 272 H 106 Z" transform="scale(0.1)" style="fill:#000000"/></g></svg>"#
    );
    // reversions and fixtext stay at their `true` defaults; the four kerning flags off keep
    // `remove_kerning` from touching the new <text>
    let (s, _) = flatten(&svg, &["--id=layer", "--revertpaths=false", "--splitdistant=false", "--mergenearby=false", "--removemanualkerning=false", "--mergesubsuper=false", "--removetextclips=false", "--removeduppaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let m = by_id(&d, "m");
    assert_eq!(m.tag_name().name(), "text", "{s}");
    assert_eq!(m.text(), Some("\u{2212}"));
    assert_eq!((m.attribute("x"), m.attribute("y")), (Some("19.3964"), Some("626.924")));
    let st = style_of(m);
    assert_eq!(st.get("font-size"), Some("999.997"));
    assert_eq!(st.get("font-family"), Some("sans-serif"));
    assert_eq!(st.get("fill"), Some("#336699"));
    assert_eq!(st.get("fill-opacity"), Some("0.5"), "translucent fill is kept (Deviation)");
    // the glyph was drawn flipped (det < 0): the text is re-flipped about the glyph's centre so
    // it reads upright, and lands in the (now dissolved) group's frame
    let t = sciink::geom::parse_transform(m.attribute("transform").unwrap()).unwrap();
    assert!(t.determinant() > 0.0, "upright: {t:?}");
    let [a, b, c, dd, _, _] = t.as_coeffs();
    assert!((a - 0.1).abs() < 1e-9 && b.abs() < 1e-9 && c.abs() < 1e-9 && (dd - 0.1).abs() < 1e-9, "{t:?}");
    // an upright glyph keeps its transform as is
    let o = by_id(&d, "other");
    assert_eq!(o.tag_name().name(), "text");
    let ot = sciink::geom::parse_transform(o.attribute("transform").unwrap()).unwrap();
    assert!(sciink::geom::affine_eq(ot, sciink::geom::Affine::scale(0.1)), "{ot:?}");
    assert_eq!(style_of(o).get("fill-opacity"), None);
}

#[test]
fn thin_dark_rectangles_become_strokes() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><path id="v" d="M10 0 h2 v100 h-2 z" style="fill:#0a0a0a"/><rect id="h" x="0" y="50" width="80" height="1" style="fill:#000000;fill-opacity:0.95"/><rect id="fat" width="10" height="10" style="fill:#000000"/><rect id="light" x="0" y="0" width="1" height="50" style="fill:#c0c0c0"/><path id="stroked" d="M0 0 h2 v100 h-2 z" style="fill:#000000;stroke:#ff0000"/></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=layer", "--revertpaths=true", "--fixtext=false", "--removeduppaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let v = by_id(&d, "v");
    assert_eq!(v.tag_name().name(), "path");
    assert_eq!(v.attribute("d"), Some("M 11,0 L 11,100"), "vertical centre line: {s}");
    let st = style_of(v);
    // #0a0a0a: L = 10 → effective lightness 10/255 < 16/255 (a #202020 fill, L = 32, is not "dark")
    assert_eq!((st.get("stroke"), st.get("fill"), st.get("stroke-width"), st.get("stroke-linecap")), (Some("#0a0a0a"), Some("none"), Some("2"), Some("butt")));
    assert_eq!(st.get("stroke-opacity"), None);
    let h = by_id(&d, "h");
    assert_eq!(h.tag_name().name(), "path", "a <rect> is converted");
    assert_eq!(h.attribute("d"), Some("M 0,50.5 L 80,50.5"));
    let st = style_of(h);
    // black at 95 %: effective lightness 0.05 < 16/255, so it is dark AND translucent
    assert_eq!((st.get("stroke-width"), st.get("stroke-opacity"), st.get("opacity")), (Some("1"), Some("0.95"), Some("1")));
    assert_eq!(h.attribute("width"), None, "shape attributes are gone");
    for i in ["fat", "light", "stroked"] {
        assert!(style_of(by_id(&d, i)).get("stroke-linecap").is_none(), "{i} is not thin, dark and unstroked");
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test flattener 2>&1 | grep -E "^test |^error" | head`.

- [ ] **Step 3: Implement**

Append to `src/tools/flattener.rs` (imports: `use crate::geom::inverse; use crate::geom::path::{parse_d, path_eq}; use crate::num; use crate::ops::bbox::{BboxOpts, bbox, is_rectangle}; use crate::ops::style::strokefill; use crate::ops::xform::object_to_path; use kurbo::{Affine, BezPath};`):

```rust
/// Aspect ratio beyond which a dark filled rectangle is really a line (F:288).
pub const RECT_THRESHOLD: f64 = 2.49;
/// The matplotlib minus-sign glyph (F:285–287); only its first three commands are compared.
pub const MINUS_D: &str = "M 106,355 H 732 V 272 H 106 Z";
/// Tags the rectangle passes look at (F:277).
const RECT_TAGS: &[&str] = &["path", "rect", "line"];
/// Parents that disqualify an element from the rectangle passes (F:281).
const FLOW_TAGS: &[&str] = &["flowPara", "flowRegion", "flowRoot"];

fn first_three(p: &BezPath) -> BezPath {
    BezPath::from_vec(p.elements().iter().take(3).copied().collect())
}

/// `d` starts with the minus glyph's `M 106,355 H 732 V 272` (F:312–315).
pub fn is_minus_glyph(d: &str) -> bool {
    let (Some(p), Some(m)) = (parse_d(d), parse_d(MINUS_D)) else {
        return false;
    };
    p.path.elements().len() >= 3 && path_eq(&first_three(&p.path), &first_three(&m.path), 1e-9)
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// F:326–340: the glyph path becomes a `<text>` holding U+2212 in the group's frame, re-flipped
/// about the glyph's centre when the composed transform mirrors (matplotlib draws glyphs with a
/// negative y scale), with upstream's literal position and size. Returns the new element.
fn revert_minus(doc: &mut Doc, ctx: &mut Ctx, el: NodeId, fill: (u8, u8, u8, f64)) -> Option<NodeId> {
    let parent = doc.parent(el)?;
    let mut t0 = doc.composed_transform(el);
    if t0.determinant() < 0.0 {
        let o = BboxOpts {
            transform: false,
            stroke: false,
            rough: false,
            clip: true,
        };
        if let Some(bb) = bbox(doc, ctx, el, o) {
            let c = bb.center().to_vec2();
            t0 = t0 * Affine::translate(c) * Affine::FLIP_Y * Affine::translate(-c);
        }
    }
    let inv = inverse(doc.composed_transform(parent))?;
    let nt = doc.new_element("text");
    doc.insert_before(nt, el);
    let id = doc.attr(el, "id").map(str::to_string);
    doc.detach(el);
    if let Some(id) = id {
        doc.set_attr(nt, "id", id);
    }
    let txt = doc.new_text("\u{2212}");
    doc.append_child(nt, txt);
    doc.set_transform(nt, inv * t0);
    doc.set_attr(nt, "x", "19.3964");
    doc.set_attr(nt, "y", "626.924");
    let (r, g, b, a) = fill;
    let mut style = format!("font-size:999.997;font-family:sans-serif;fill:{}", hex(r, g, b));
    if a != 1.0 {
        // Deviation: upstream drops a translucent fill's alpha
        style.push_str(&format!(";fill-opacity:{}", num::fmt(a)));
    }
    doc.set_attr(nt, "style", style);
    Some(nt)
}

/// F:342–368: a dark, unstroked rectangle at least `RECT_THRESHOLD` times taller than wide (or
/// wider than tall) becomes a stroked centre line of the same colour and thickness.
fn revert_thin(doc: &mut Doc, el: NodeId, bb: kurbo::Rect, fill: (u8, u8, u8, f64)) {
    let (r, g, b, a) = fill;
    let (w, h) = (bb.width(), bb.height());
    let (d, width) = if w < h / RECT_THRESHOLD {
        let xc = bb.center().x;
        (format!("M {},{} L {},{}", num::fmt(xc), num::fmt(bb.y0), num::fmt(xc), num::fmt(bb.y1)), w)
    } else if h < w / RECT_THRESHOLD {
        let yc = bb.center().y;
        (format!("M {},{} L {},{}", num::fmt(bb.x0), num::fmt(yc), num::fmt(bb.x1), num::fmt(yc)), h)
    } else {
        return;
    };
    object_to_path(doc, el);
    doc.set_attr(el, "d", d);
    doc.set_style(el, "stroke", &hex(r, g, b));
    if a != 1.0 {
        doc.set_style(el, "stroke-opacity", &num::fmt(a));
        doc.set_style(el, "opacity", "1");
    }
    doc.set_style(el, "fill", "none");
    doc.set_style(el, "stroke-width", &num::fmt(width));
    doc.set_style(el, "stroke-linecap", "butt");
}

/// F:277–368: over the non-container working set, every unstroked filled rectangle-like element
/// (not inside flowed text) is a white-rectangle candidate when its fill is opaque white; with
/// `reversions` a matplotlib minus glyph becomes a `<text>`; with `revertpaths` a dark thin
/// rectangle becomes a stroke. Returns the white-rectangle candidates; `ngs` gets the reverted
/// texts in place of their glyph paths.
pub fn rect_passes(doc: &mut Doc, ctx: &mut Ctx, ngs: &mut Vec<NodeId>, o: &Options) -> Vec<NodeId> {
    let mut wrects = Vec::new();
    for el in attached(doc, ngs) {
        if !RECT_TAGS.contains(&doc.tag(el)) {
            continue;
        }
        let Some(parent) = doc.parent(el) else { continue };
        if FLOW_TAGS.contains(&doc.tag(parent)) || !is_rectangle(doc, el, false) {
            continue;
        }
        let stroke = doc.specified(el, "stroke").unwrap_or_else(|| "none".to_string());
        let fill = doc.specified(el, "fill").unwrap_or_else(|| "black".to_string());
        if stroke.trim() != "none" || fill.trim() == "none" {
            continue;
        }
        let sf = strokefill(doc, el);
        let Some(f) = sf.fill else { continue };
        let rgba = (f.r, f.g, f.b, f.alpha);
        if (f.r, f.g, f.b) == (255, 255, 255) && f.alpha == 1.0 {
            wrects.push(el);
        }
        if o.reversions && doc.attr(el, "d").is_some_and(is_minus_glyph) {
            if let Some(nt) = revert_minus(doc, ctx, el, rgba) {
                if let Some(i) = ngs.iter().position(|&n| n == el) {
                    ngs.remove(i);
                }
                ngs.push(nt);
                continue;
            }
        }
        if o.revertpaths && !sf.fill_is_url && f.efflightness < 16.0 / 255.0 {
            let lo = BboxOpts {
                transform: false,
                stroke: false,
                rough: false,
                clip: false,
            };
            if let Some(bb) = bbox(doc, ctx, el, lo) {
                revert_thin(doc, el, bb, rgba);
            }
        }
    }
    wrects
}
```

In `run`, replace `let _ = (&mut ngs, &opts);` with:

```rust
    let wrects = if opts.removerectw || opts.reversions || opts.revertpaths {
        rect_passes(&mut doc, &mut ctx, &mut ngs, &opts)
    } else {
        Vec::new()
    };
    let _ = (&wrects, &mut ngs); // consumed by Tasks 5–6
```

Notes: `strokefill` already reports `fill = None` for `url(#…)` paints, so the white-rect and dark tests never see a gradient; `BezPath::from_vec` exists in kurbo 0.13; `Affine::FLIP_Y` is `scale(1, −1)`.

- [ ] **Step 4: Run to verify success**

`cargo test --test flattener 2>&1 | grep -E "^test result|FAILED|panicked"` → 11 passed; gate.

- [ ] **Step 5: Commit**

```bash
git add src/tools/flattener.rs tests/flattener.rs
git commit -m "feat(flattener): rectangle passes — white rectangles, minus-sign reversion, thin rectangles to strokes"
```

---

### Task 5: Text phase — font replacement, language switches, the kerning pipeline, text clips

**Files:**
- Modify: `src/tools/flattener.rs` (append; extend `run`)
- Test: `tests/flattener.rs` (append)

**Interfaces:**
- Consumes: `text::kerning::{KerningOptions, remove_kerning}` (`remove_kerning(doc, els, &opts, FontSystem, &mut Warnings) -> Vec<NodeId>`: rewritten elements replaced by their new nodes, removed ones dropped, split-offs appended; it filters `els` to `<text>`/`<flowRoot>` itself), `text::fonts::FontSystem::load`, `ops::clip::{deswitch, ui_language}`, `ops::style::remove_inline`, `Doc::{specified, set_style, remove_attr}`.
- Produces: `replace_fonts(doc, ngs: &[NodeId], replacement: &str)` (F:372–388), `text_phase(doc, ctx, ngs: &mut Vec<NodeId>, o: &Options)` (F:370–413).

Font replacement (F:372–388), per attached `<text>`/`<tspan>` of `ngs`: the inline `-inkscape-font-specification` goes (inline style only — upstream's `cstyle`); with `ff` = the specified `font-family`: absent/`none`/empty → `font-family: <repl>`; equal to `repl` → nothing; otherwise split on `,`, strip `'" ` from each entry, append `repl` unless the last entry already equals it case-insensitively, and write the entries joined by `,`. (Upstream writes the joined list unquoted; so do we.)

- [ ] **Step 1: Write the failing tests**

Append to `tests/flattener.rs`:

```rust
#[test]
fn font_replacement_appends_the_family_and_drops_the_inkscape_spec() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><text id="a" style="font-family:'Franklin Gothic Book', serif;-inkscape-font-specification:'Franklin Gothic Book'" x="0" y="0">a<tspan id="s" style="font-family:Arial">b</tspan></text><text id="none" style="font-family:none" x="0" y="20">c</text><text id="bare" x="0" y="40">d</text><text id="same" style="font-family:arial" x="0" y="60">e</text></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=layer", "--setreplacement=true", "--replacement=Arial", "--splitdistant=false", "--mergenearby=false", "--removemanualkerning=false", "--mergesubsuper=false", "--reversions=false", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let st = style_of(by_id(&d, "a"));
    assert_eq!(st.get("font-family"), Some("Franklin Gothic Book,serif,Arial"), "{s}");
    assert_eq!(st.get("-inkscape-font-specification"), None);
    assert_eq!(style_of(by_id(&d, "s")).get("font-family"), Some("Arial"), "a tspan already at the replacement is left alone");
    assert_eq!(style_of(by_id(&d, "none")).get("font-family"), Some("Arial"));
    assert_eq!(style_of(by_id(&d, "bare")).get("font-family"), Some("Arial"), "no family at all → the replacement");
    assert_eq!(style_of(by_id(&d, "same")).get("font-family"), Some("arial"), "case-insensitive match of the last entry: nothing appended");
}

#[test]
fn text_phase_merges_split_words_and_removes_text_clips() {
    // "Hello" + " world" as two elements one space apart (DejaVu Sans 10 px), a clipped text, and a
    // language switch — the Flattener's text phase merges, strips the clip and resolves the switch
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1000" height="1000"/></clipPath></defs><g id="layer"><text id="h" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="w" xml:space="preserve" style="{DV};font-size:10px" x="28.5" y="0">world</text><text id="clipped" style="{DV};font-size:10px" x="0" y="50" clip-path="url(#c)" mask="url(#c)">clipped</text><switch id="sw"><text id="de" systemLanguage="de" style="{DV};font-size:10px" x="0" y="80">Hallo</text><text id="en" style="{DV};font-size:10px" x="0" y="80">Hi</text></switch></g></svg>"#
    );
    let (s, msgs) = flatten(&svg, &["--id=layer", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"]);
    assert!(msgs.iter().all(|m| m.starts_with("warning: ")), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let texts: Vec<String> = d
        .descendants()
        .filter(|n| n.has_tag_name("text"))
        .map(|n| n.descendants().filter(|c| c.is_text()).filter_map(|c| c.text()).collect::<String>())
        .collect();
    assert!(texts.iter().any(|t| t.split_whitespace().collect::<Vec<_>>().join(" ") == "Hello world"), "merged: {texts:?}");
    let clipped = d.descendants().find(|n| n.has_tag_name("text") && n.descendants().any(|c| c.text() == Some("clipped"))).expect("clipped text survives");
    assert_eq!((clipped.attribute("clip-path"), clipped.attribute("mask")), (None, None), "text clips removed: {s}");
    assert!(!has(&d, "sw") && !has(&d, "de"), "the switch is resolved to the matching child: {s}");
    assert!(texts.iter().any(|t| t.trim() == "Hi"));
    assert!(!s.contains("<switch"));
}

#[test]
fn fixtext_off_leaves_text_untouched() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><text id="h" xml:space="preserve" style="{DV};font-size:10px;-inkscape-font-specification:x" x="0" y="0">Hello</text><text id="w" xml:space="preserve" style="{DV};font-size:10px" x="28.5" y="0">world</text></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=layer", "--fixtext=false", "--setreplacement=true", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "h") && has(&d, "w"), "{s}");
    assert!(style_of(by_id(&d, "h")).get("-inkscape-font-specification").is_some(), "setreplacement is ANDed with fixtext");
}
```

`x="28.5"` in the merge test puts `world` about one space after `Hello` in DejaVu Sans at 10 px (advance ≈ 25.4 px, space ≈ 3.2 px); `External_Merges` accepts any gap within 0.6 spaces of one space, so the exact advance does not matter.

- [ ] **Step 2: Run to verify failure** — `cargo test --test flattener 2>&1 | grep -E "^test |^error" | head`.

- [ ] **Step 3: Implement**

Append to `src/tools/flattener.rs` (imports: `use crate::ops::clip::{deswitch, ui_language}; use crate::ops::style::remove_inline; use crate::text::fonts::FontSystem; use crate::text::kerning::{KerningOptions, remove_kerning};`):

```rust
/// F:372–388 `setreplacement`: every `<text>`/`<tspan>` of the working set loses its inline
/// `-inkscape-font-specification` and gets `replacement` appended to its family list (or as its
/// family when it has none), unless the list already ends with it.
pub fn replace_fonts(doc: &mut Doc, ngs: &[NodeId], replacement: &str) {
    for el in attached(doc, ngs) {
        if !matches!(doc.tag(el), "text" | "tspan") {
            continue;
        }
        let ff = doc.specified(el, "font-family");
        remove_inline(doc, el, "-inkscape-font-specification");
        let ff = ff.map(|s| s.trim().to_string()).unwrap_or_default();
        if ff.is_empty() || ff == "none" {
            doc.set_style(el, "font-family", replacement);
        } else if ff == replacement {
            // nothing to do
        } else {
            let mut fams: Vec<String> = ff
                .split(',')
                .map(|f| f.trim_matches(|c| matches!(c, '\'' | '"' | ' ')).to_string())
                .collect();
            if !fams
                .last()
                .is_some_and(|l| l.eq_ignore_ascii_case(replacement))
            {
                fams.push(replacement.to_string());
            }
            doc.set_style(el, "font-family", &fams.join(","));
        }
    }
}

/// F:370–413: font replacement, language switches, the kerning pipeline, text clips.
pub fn text_phase(doc: &mut Doc, ctx: &mut Ctx, ngs: &mut Vec<NodeId>, o: &Options) {
    if o.setreplacement {
        replace_fonts(doc, ngs, &o.replacement);
    }
    if o.removemanualkerning || o.mergesubsuper || o.splitdistant || o.mergenearby {
        let lang = ui_language();
        for sw in attached(doc, ngs) {
            if doc.tag(sw) == "switch" {
                deswitch(doc, ctx, sw, &lang);
            }
        }
        *ngs = attached(doc, ngs);
        let tels: Vec<NodeId> = ngs
            .iter()
            .copied()
            .filter(|&n| matches!(doc.tag(n), "text" | "flowRoot"))
            .collect();
        let kopts = KerningOptions::from_inx(
            o.removemanualkerning,
            o.mergesubsuper,
            o.splitdistant,
            o.mergenearby,
            o.justification,
        );
        let out = remove_kerning(doc, &tels, &kopts, FontSystem::load(), &mut ctx.warn);
        let tset: HashSet<NodeId> = tels.into_iter().collect();
        ngs.retain(|n| !tset.contains(n));
        *ngs = attached(doc, ngs); // tspans of rewritten texts are gone
        ngs.extend(out.into_iter().filter(|&n| doc.parent(n).is_some()));
    }
    if o.removetextclips {
        for el in attached(doc, ngs) {
            if matches!(doc.tag(el), "text" | "flowRoot") {
                doc.remove_attr(el, "clip-path");
                doc.remove_attr(el, "mask");
            }
        }
    }
}
```

In `run`, replace `let _ = (&wrects, &mut ngs);` with:

```rust
    if opts.fixtext {
        text_phase(&mut doc, &mut ctx, &mut ngs, &opts);
    }
    let _ = (&wrects, &ngs); // consumed by Task 6
```

- [ ] **Step 4: Run to verify success**

`cargo test --test flattener 2>&1 | grep -E "^test result|FAILED|panicked"` → 14 passed; gate.

- [ ] **Step 5: Commit**

```bash
git add src/tools/flattener.rs tests/flattener.rs
git commit -m "feat(flattener): text phase — font replacement, language switches, remove_kerning, text clips"
```

---

### Task 6: Bounding-box stage — overlapping duplicates and white background rectangles

**Files:**
- Modify: `src/tools/flattener.rs` (append; extend `run`)
- Test: `tests/flattener.rs` (append)

**Interfaces:**
- Consumes: `ops::bbox::{bb2, is_drawn}`, `ops::style::strokefill`, `ops::cleanup::{delete_up, url_id}`, `text::edit::style_eq`, `geom::path::{shape_path, path_eq, reverse}`, `geom::intersects`, `Doc::{composed_transform, specified_style, specified}`.
- Produces: `bbox_stage(doc, ctx, ngs: &[NodeId], wrects: &[NodeId], o: &Options)` (F:415–510), `remove_duplicates(doc, ctx, ngs2: &mut Vec<NodeId>, bbs: &HashMap<NodeId, Rect>)` (F:422–497), `remove_white_rects(doc, ctx, ngs2: &[NodeId], bbs: &HashMap<NodeId, Rect>, wrects: &[NodeId])` (F:499–509).

Semantics (F:415–510): `ngs2` = the attached, drawn (`is_drawn`) members of `ngs` in document order; `bbs = bb2(ngs2, rough = true)`. **Duplicates:** candidates are the `path`/`rect`/`line` members with a box that are not the `shape-inside` target of any `<text>`; two boxes are equal when neither is empty (zero width or height) and all four edges differ by at most `1e-6·max(size_i, size_j)` (`size = max(w, h)`); pairs `(ii < jj)` are visited with `jj` descending, then `ii` ascending, skipping an `ii` already removed; the later (`jj`, on top) element must have a stroke or a fill, neither painted by `url(#…)`; where it has a stroke, that stroke is opaque (`alpha == 1`) and the earlier element has a stroke of the same rgb; likewise for the fill; the two specified styles are equal (order-insensitive); the two global absolute paths are equal or one is the reverse of the other (tolerance `1e-6·size_max`); then the EARLIER one (`ii`, underneath) is `delete_up`'d. **White rectangles:** among the members of `ngs2` that still exist and have a box, each white-rectangle candidate is deleted when no element EARLIER in document order (an element behind it, not already deleted) has a box that strictly intersects its box (`geom::intersects`); a deleted rectangle no longer counts as being behind later ones.

**Deviation (documented in Task 7; corrected in the post-review fix wave):** the paints are compared as rgba (alpha within 1e-9), as upstream's `inkex.Color` equality does (it carries alpha), and the top element's paint must additionally be opaque. The original claim here that `inkex.Color` ignores alpha was false; because the specified-style equality checked first already implies equal alphas, the correction changes the documentation, not a decision. The `same_rgb` helper in the code below became `same_rgba`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/flattener.rs`:

```rust
#[test]
fn overlapping_identical_paths_lose_the_one_underneath() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><path id="bottom" d="M0 0 L10 0 L10 5" style="stroke:#ff0000;fill:none"/><path id="top" d="M0 0 L10 0 L10 5" style="stroke:#ff0000;fill:none"/><path id="reversed" d="M10 5 L10 0 L0 0" style="stroke:#ff0000;fill:none"/><path id="other_style" d="M0 0 L10 0 L10 5" style="stroke:#ff0000;fill:none;stroke-width:3"/><path id="translucent_top" d="M20 0 L30 0" style="stroke:#0000ff;fill:none;stroke-opacity:0.5"/><path id="translucent_top2" d="M20 0 L30 0" style="stroke:#0000ff;fill:none;stroke-opacity:0.5"/><g id="wrap"><path id="moved" d="M0 0 L10 0 L10 5" transform="translate(40,0)" style="fill:#00ff00"/></g><path id="moved_dup" d="M40 0 L50 0 L50 5" style="fill:#00ff00"/></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=layer", "--fixtext=false", "--revertpaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "bottom") && !has(&d, "top"), "top ≡ reversed: `reversed` is the topmost of the three, it survives; the two below it go: {s}");
    assert!(has(&d, "reversed"));
    assert!(has(&d, "other_style"), "a different stroke width is not a duplicate");
    assert!(has(&d, "translucent_top") && has(&d, "translucent_top2"), "a translucent top element never deletes what is under it");
    assert!(!has(&d, "moved") && has(&d, "moved_dup"), "duplicates are compared in root coordinates, through transforms: {s}");
    assert!(!has(&d, "wrap"), "the emptied group went with it");
}

#[test]
fn white_background_rectangles_go_when_nothing_is_behind_them() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><rect id="bg" width="100" height="100" style="fill:#ffffff"/><path id="axis" d="M10 25 L90 25" style="stroke:#000000"/><rect id="cover" x="20" y="20" width="10" height="10" style="fill:#ffffff"/><rect id="alone" x="200" y="200" width="10" height="10" style="fill:#ffffff"/><rect id="stroked" x="300" y="300" width="10" height="10" style="fill:#ffffff;stroke:#000000"/><rect id="offwhite" x="400" y="400" width="10" height="10" style="fill:#fffffe"/></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=layer", "--fixtext=false", "--revertpaths=false", "--removeduppaths=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "bg"), "nothing is behind the background: {s}");
    assert!(has(&d, "cover"), "the axis line is behind it → kept (it may be hiding something on purpose)");
    assert!(!has(&d, "alone"));
    assert!(has(&d, "stroked") && has(&d, "offwhite"), "only unstroked pure-white fills are candidates");
    assert!(has(&d, "axis"));
}

#[test]
fn the_shape_inside_target_of_a_text_is_never_a_duplicate_candidate() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><rect id="frame" width="10" height="10" style="fill:#000000"/><rect id="frame2" width="10" height="10" style="fill:#000000"/><text id="t" style="shape-inside:url(#frame2);{DV};font-size:3px" x="0" y="0"><tspan x="0" y="3">x</tspan></text></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=layer", "--fixtext=false", "--revertpaths=false", "--removerectw=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "frame") && has(&d, "frame2"), "frame2 is a text's shape-inside, so the pair is never considered: {s}");
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --test flattener 2>&1 | grep -E "^test |^error" | head`.

- [ ] **Step 3: Implement**

Append to `src/tools/flattener.rs` (imports: `use std::collections::HashMap; use kurbo::Rect; use crate::geom::intersects; use crate::geom::path::{reverse, shape_path}; use crate::ops::bbox::{bb2, is_drawn}; use crate::ops::cleanup::{delete_up, url_id}; use crate::text::edit::style_eq;`):

```rust
/// Elements some `<text>`'s specified `shape-inside` points at (F:425–426).
fn shape_inside_targets(doc: &Doc) -> HashSet<NodeId> {
    doc.descendants(doc.svg())
        .filter(|&n| doc.is_element(n) && doc.tag(n) == "text")
        .filter_map(|t| doc.specified(t, "shape-inside"))
        .filter_map(|v| url_id(&v).and_then(|id| doc.by_id(id)))
        .collect()
}

fn same_rgb(a: &crate::ops::style::Rgba, b: &crate::ops::style::Rgba) -> bool {
    (a.r, a.g, a.b) == (b.r, b.g, b.b)
}

/// F:422–497: prune identical overlapping paths — of two rectangle-like elements with the same
/// rough box, style, paints and global geometry, the one underneath goes.
pub fn remove_duplicates(doc: &mut Doc, ctx: &mut Ctx, ngs2: &mut Vec<NodeId>, bbs: &HashMap<NodeId, Rect>) {
    let inside = shape_inside_targets(doc);
    let els: Vec<NodeId> = ngs2
        .iter()
        .copied()
        .filter(|&n| RECT_TAGS.contains(&doc.tag(n)) && bbs.contains_key(&n) && !inside.contains(&n))
        .collect();
    let boxes: Vec<Rect> = els.iter().map(|n| bbs[n]).collect();
    let size = |r: &Rect| r.width().max(r.height());
    let equal = |i: usize, j: usize| -> bool {
        let (a, b) = (&boxes[i], &boxes[j]);
        if a.width() == 0.0 || a.height() == 0.0 || b.width() == 0.0 || b.height() == 0.0 {
            return false;
        }
        let tol = 1e-6 * size(a).max(size(b));
        (a.x0 - b.x0).abs() <= tol
            && (a.y0 - b.y0).abs() <= tol
            && (a.x1 - b.x1).abs() <= tol
            && (a.y1 - b.y1).abs() <= tol
    };
    let mut sfs: Vec<Option<crate::ops::style::StrokeFill>> = vec![None; els.len()];
    let mut removed: HashSet<usize> = HashSet::new();
    for jj in (0..els.len()).rev() {
        for ii in 0..jj {
            if removed.contains(&ii) || !equal(ii, jj) {
                continue;
            }
            if sfs[jj].is_none() {
                sfs[jj] = Some(strokefill(doc, els[jj]));
            }
            if sfs[ii].is_none() {
                sfs[ii] = Some(strokefill(doc, els[ii]));
            }
            let (my, oth) = (sfs[jj].as_ref().unwrap(), sfs[ii].as_ref().unwrap());
            if my.stroke_is_url || my.fill_is_url || (my.stroke.is_none() && my.fill.is_none()) {
                continue;
            }
            if let Some(s) = &my.stroke {
                if s.alpha != 1.0 || !oth.stroke.as_ref().is_some_and(|o| same_rgb(s, o)) {
                    continue;
                }
            }
            if let Some(f) = &my.fill {
                if f.alpha != 1.0 || !oth.fill.as_ref().is_some_and(|o| same_rgb(f, o)) {
                    continue;
                }
            }
            if !style_eq(&doc.specified_style(els[jj]), &doc.specified_style(els[ii])) {
                continue;
            }
            let (Some(pj), Some(pi)) = (shape_path(doc, els[jj]), shape_path(doc, els[ii])) else {
                continue;
            };
            let gj = doc.composed_transform(els[jj]) * pj.path;
            let gi = doc.composed_transform(els[ii]) * pi.path;
            let tol = 1e-6 * size(&boxes[ii]).max(size(&boxes[jj]));
            if !(path_eq(&gj, &gi, tol) || path_eq(&gj, &reverse(&gi), tol)) {
                continue;
            }
            delete_up(doc, ctx, els[ii]);
            removed.insert(ii);
        }
    }
    let gone: HashSet<NodeId> = removed.iter().map(|&i| els[i]).collect();
    ngs2.retain(|n| !gone.contains(n));
}

/// F:499–509: a white-rectangle candidate with nothing behind it (no earlier element whose box
/// strictly intersects its own) is a background and goes; a deleted one no longer counts as
/// being behind the next.
pub fn remove_white_rects(doc: &mut Doc, ctx: &mut Ctx, ngs2: &[NodeId], bbs: &HashMap<NodeId, Rect>, wrects: &[NodeId]) {
    let ngs3: Vec<NodeId> = ngs2
        .iter()
        .copied()
        .filter(|n| doc.parent(*n).is_some() && bbs.contains_key(n))
        .collect();
    let white: HashSet<NodeId> = wrects.iter().copied().collect();
    let mut deleted: HashSet<usize> = HashSet::new();
    for ii in 0..ngs3.len() {
        if !white.contains(&ngs3[ii]) {
            continue;
        }
        let wb = bbs[&ngs3[ii]];
        let behind = (0..ii).any(|k| !deleted.contains(&k) && intersects(bbs[&ngs3[k]], wb));
        if !behind {
            delete_up(doc, ctx, ngs3[ii]);
            deleted.insert(ii);
        }
    }
}

/// F:415–510: rough boxes of the drawn working set, then duplicates, then white rectangles.
pub fn bbox_stage(doc: &mut Doc, ctx: &mut Ctx, ngs: &[NodeId], wrects: &[NodeId], o: &Options) {
    let ngset: HashSet<NodeId> = attached(doc, ngs).into_iter().collect();
    let mut ngs2: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|n| ngset.contains(n) && is_drawn(doc, *n))
        .collect();
    let bbs = bb2(doc, ctx, &ngs2, true);
    if o.removeduppaths {
        remove_duplicates(doc, ctx, &mut ngs2, &bbs);
    }
    if o.removerectw {
        remove_white_rects(doc, ctx, &ngs2, &bbs, wrects);
    }
}
```

In `run`, replace `let _ = (&wrects, &ngs);` with:

```rust
    if opts.removerectw || opts.removeduppaths {
        bbox_stage(&mut doc, &mut ctx, &ngs, &wrects, &opts);
    }
```

`StrokeFill` derives `Clone` and `Default` (Plan 5); `Rgba` is `Copy`. `style_eq` is `pub` in `text::edit`.

- [ ] **Step 4: Run to verify success**

`cargo test --test flattener 2>&1 | grep -E "^test result|FAILED|panicked"` → 17 passed; gate.

- [ ] **Step 5: Commit**

```bash
git add src/tools/flattener.rs tests/flattener.rs
git commit -m "feat(flattener): bbox stage — overlapping duplicates and white background rectangles"
```

---

### Task 7: Upstream reference oracles, README, spec deviations

**Files:**
- Create: `tests/flattener_fixtures.rs`
- Modify: `README.md`, `docs/spec/02-geometry-tools.md`

**Interfaces:**
- Consumes: the Flattener through `sciink::run` with `--testmode=true` (Tasks 2–6), `support::{upstream_data_dir, with_vendored_fonts}`, the upstream references `tests/upstream/data/refs/flatten_plots__--id__layer1__--testmode__True__{Text_tests,Text_tests_dx,Acid_tests}__svg.out`.
- Produces: tests only.

What the references contain (measured on 2026-09-22, layer `layer1` = "Layer 1 flat"): Text_tests `g 36→1, path 41→31, text 194→436, clipPath 10→0`; Text_tests_dx `g 32→1, path 31→31, text 174→415`; Acid_tests `g 5936→8 (7 carry mpl_comment), path 1664→1574, rect 26→8, line 305→298, use 141→35, image 64→63, text 676→1003, 26 minus signs, 95 unlinked_clone markers (which we strip)`. The structural oracle reads the reference each time and asserts, per fixture, on the flattened layer: `g`, `clipPath`, `use`, `image`, `line` and `mpl_comment` counts equal the reference's; `path` within 3 % of it; `rect` within ±3 (white-rectangle decisions depend on text boxes, which depend on the fonts available); the number of U+2212 characters in the layer's text equals the reference's; the duplicate layer precedes it with upstream's label, lock and opacity; no `unlinked_clone` attribute survives. Text merge/split decisions depend on font metrics, so the content-multiset oracle (`≥ 85 %` of the reference strings, Plan 4's threshold) runs only with the installed fonts (`#[ignore]`, `SCIINK_SYSTEM_FONTS=1`).

- [ ] **Step 1: Write the tests**

Create `tests/flattener_fixtures.rs`:

```rust
mod support;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use support::with_vendored_fonts;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}

/// Element counts by local name under the element with `id`.
fn counts(d: &roxmltree::Document, id: &str) -> HashMap<String, usize> {
    let mut c = HashMap::new();
    if let Some(layer) = d.descendants().find(|n| n.attribute("id") == Some(id)) {
        for n in layer.descendants().filter(|n| n.is_element()) {
            *c.entry(n.tag_name().name().to_string()).or_default() += 1;
        }
    }
    c
}
fn attr_count(d: &roxmltree::Document, id: &str, attr: &str) -> usize {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .map(|l| l.descendants().filter(|n| n.attribute(attr).is_some()).count())
        .unwrap_or(0)
}
/// Whitespace-collapsed text of every <text> under `id`.
fn layer_texts(d: &roxmltree::Document, id: &str) -> Vec<String> {
    let Some(layer) = d.descendants().find(|n| n.attribute("id") == Some(id)) else {
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
fn minus_signs(texts: &[String]) -> usize {
    texts.iter().map(|t| t.matches('\u{2212}').count()).sum()
}
const INKSCAPE_NS: &str = "http://www.inkscape.org/namespaces/inkscape";
const SODIPODI_NS: &str = "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd";
/// roxmltree looks namespaced attributes up by (namespace, local name).
fn nsattr<'a>(n: roxmltree::Node<'a, '_>, ns: &str, local: &str) -> Option<&'a str> {
    n.attribute((ns, local))
}

fn flatten_fixture(name: &str) -> Option<(String, String)> {
    let dir = support::upstream_data_dir()?;
    let input = std::fs::read(dir.join(format!("svg/{name}.svg"))).unwrap();
    let reference = std::fs::read_to_string(
        dir.join(format!("refs/flatten_plots__--id__layer1__--testmode__True__{name}__svg.out")),
    )
    .unwrap();
    let t0 = std::time::Instant::now();
    let out = with_vendored_fonts(|| {
        sciink::run(&args(&["--tool=flattener", "--tab=Options", "--id=layer1", "--testmode=true"]), &input)
    })
    .unwrap();
    eprintln!("{name}: flattened in {:?}, {} warnings", t0.elapsed(), out.messages.len());
    assert!(out.messages.iter().all(|m| m.starts_with("warning: ")), "{:?}", out.messages);
    Some((String::from_utf8(out.svg).unwrap(), reference))
}

fn structural_oracle(name: &str) {
    let Some((ours, reference)) = flatten_fixture(name) else {
        return;
    };
    let od = roxmltree::Document::parse(&ours).unwrap();
    let rd = roxmltree::Document::parse(&reference).unwrap();
    let (oc, rc) = (counts(&od, "layer1"), counts(&rd, "layer1"));
    let get = |c: &HashMap<String, usize>, k: &str| c.get(k).copied().unwrap_or(0);
    eprintln!("{name}: ours {oc:?}\n{name}: ref  {rc:?}");
    for k in ["g", "clipPath", "use", "image", "line"] {
        assert_eq!(get(&oc, k), get(&rc, k), "{name}: {k} count");
    }
    assert_eq!(get(&oc, "clipPath"), 0, "{name}: clips moved to the root defs");
    let (op, rp) = (get(&oc, "path") as f64, get(&rc, "path") as f64);
    assert!((op - rp).abs() <= 0.03 * rp, "{name}: paths {op} vs {rp}");
    assert!((get(&oc, "rect") as i64 - get(&rc, "rect") as i64).abs() <= 3, "{name}: rects {} vs {}", get(&oc, "rect"), get(&rc, "rect"));
    assert_eq!(attr_count(&od, "layer1", "mpl_comment"), attr_count(&rd, "layer1", "mpl_comment"), "{name}: matplotlib glyph groups");
    assert_eq!(minus_signs(&layer_texts(&od, "layer1")), minus_signs(&layer_texts(&rd, "layer1")), "{name}: minus-sign reversions");
    assert_eq!(attr_count(&od, "layer1", "unlinked_clone"), 0, "{name}: markers are stripped");
    let flat = od.descendants().find(|n| n.attribute("id") == Some("layer1")).unwrap();
    assert_eq!(nsattr(flat, INKSCAPE_NS, "label"), Some("Layer 1 flat"));
    // roxmltree's `prev_siblings()` starts at the node itself
    let orig = flat.prev_siblings().skip(1).find(|n| n.is_element()).expect("the duplicate precedes the flattened layer");
    assert_eq!(nsattr(orig, INKSCAPE_NS, "label"), Some("Layer 1 original"));
    assert_eq!((nsattr(orig, SODIPODI_NS, "insensitive"), orig.attribute("opacity")), (Some("true"), Some("0.3")));
}

#[test]
fn text_tests_structure_matches_the_upstream_reference() {
    structural_oracle("Text_tests");
}

#[test]
fn text_tests_dx_structure_matches_the_upstream_reference() {
    structural_oracle("Text_tests_dx");
}

#[test]
fn acid_tests_structure_matches_the_upstream_reference() {
    structural_oracle("Acid_tests");
}

fn content_oracle(name: &str) {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        eprintln!("SKIP: set SCIINK_SYSTEM_FONTS=1 to run against the installed fonts");
        return;
    }
    // vendored DejaVu Sans on top of the system fonts; SAFETY: set before any FontSystem::load()
    // in this binary's ignored tests, which run alone (`-- --ignored`)
    unsafe {
        std::env::set_var(
            "SCIINK_FONT_DIRS",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts").display().to_string(),
        );
    }
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join(format!("svg/{name}.svg"))).unwrap();
    let reference = std::fs::read_to_string(
        dir.join(format!("refs/flatten_plots__--id__layer1__--testmode__True__{name}__svg.out")),
    )
    .unwrap();
    let out = sciink::run(&args(&["--tool=flattener", "--tab=Options", "--id=layer1", "--testmode=true"]), &input).unwrap();
    let od = roxmltree::Document::parse(std::str::from_utf8(&out.svg).unwrap()).unwrap();
    let rd = roxmltree::Document::parse(&reference).unwrap();
    let (ours, theirs) = (layer_texts(&od, "layer1"), layer_texts(&rd, "layer1"));
    let mut pool: HashMap<&str, i64> = HashMap::new();
    for t in &theirs {
        *pool.entry(t.as_str()).or_default() += 1;
    }
    let mut matched = 0usize;
    for t in &ours {
        if let Some(c) = pool.get_mut(t.as_str()) {
            if *c > 0 {
                *c -= 1;
                matched += 1;
            }
        }
    }
    eprintln!("{name}: {matched}/{} reference strings reproduced ({} ours)", theirs.len(), ours.len());
    assert!(matched as f64 >= 0.85 * theirs.len() as f64, "{name}: {matched}/{}", theirs.len());
}

/// Run: `SCIINK_SYSTEM_FONTS=1 cargo test --test flattener_fixtures -- --ignored --nocapture`
#[test]
#[ignore]
fn text_tests_content_matches_the_upstream_reference() {
    content_oracle("Text_tests");
}

#[test]
#[ignore]
fn text_tests_dx_content_matches_the_upstream_reference() {
    content_oracle("Text_tests_dx");
}
```

- [ ] **Step 2: Run** — `cargo test --test flattener_fixtures 2>&1 | grep -E "^test |flattened in|ours|ref |panicked|assert" | head -40`, then `SCIINK_SYSTEM_FONTS=1 cargo test --test flattener_fixtures -- --ignored --nocapture 2>&1 | grep -E "reproduced|^test result|panicked"`.

If a structural assertion fails, do **not** loosen it: report the fixture, the attribute or count, ours vs the reference, in the task report (the controller rules — candidates: a font-dependent white-rectangle decision, a glyph group whose child counts tie differently, an upstream crash path we handle). If the content oracle scores below 85 %, likewise report the number and the first ten unmatched strings.

- [ ] **Step 3: Documentation**

`README.md`: the intro sentence becomes "The current release ships seven menu entries — three tools, three diagnostics and one debug editor:" and this bullet goes first:

```markdown
- **Extensions ▸ Scientific ▸ Flattener** — makes an imported plot editable: deep ungroup (composing
  transforms, clips and styles onto the leaves and unlinking clones), matplotlib minus-sign glyphs back
  to text, thin dark rectangles back to strokes, the text pipeline (manual-kerning removal, merges,
  splits, justification, optional font replacement, text clips removed), then overlapping duplicates
  and white background rectangles removed. Objects marked on the Exclusions page are left alone.
  Same options and defaults as the original.
```

`docs/spec/02-geometry-tools.md`: append `## Deliberate deviations (Plan 6)` with one bullet each: `unlinked_clone` markers are stripped at the end of a run (upstream keeps them); the `--testmode` duplicate carries no ids (upstream: random ids); a reverted minus sign keeps a translucent fill's alpha as `fill-opacity`; thin-rectangle strokes are written as absolute `M … L …`; duplicate removal compares the paints as rgba (alpha within 1e-9) like upstream's `inkex.Color`, with the top element's paint opaque (corrected in the post-review fix wave: the original "rgb only" justification was false); `remove_kerning` never edits flowed text or text on a path (Plan 4); an empty selection is an `Err` with upstream's message (the document is echoed unchanged); the character table for the bbox stage covers the whole document (`Ctx::new()`, as upstream's `BB2(svg, ngs2)` covers all text of the selection's descendants). Also correct §B.3 step 7's `deswitch` sentence to name `ui_language()`.

- [ ] **Step 4: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

```bash
git add tests/flattener_fixtures.rs README.md docs/spec/02-geometry-tools.md
git commit -m "test(flattener): upstream reference oracles; document the Flattener and Plan 6 deviations"
```

---

### Task 8: Appearance-invariance oracle with resvg

**Files:**
- Modify: `Cargo.toml` (`[dev-dependencies]` gains `resvg = "0.48.1"`), `tests/support/mod.rs` (append)
- Create: `tests/invariance.rs`

**Interfaces:**
- Consumes: `resvg::usvg::{Options, Tree}`, `resvg::tiny_skia::{Pixmap, Transform}`, `resvg::render`; `support::{fontdir, upstream_data_dir, with_vendored_fonts}`.
- Produces: `support::render_png(svg: &[u8], max_side: u32) -> (u32, u32, Vec<u8>)` (premultiplied RGBA8, the vendored fonts only), `support::pixel_diff_fraction(a, b, threshold: u8) -> f64` (fraction of pixels whose largest channel difference exceeds `threshold`; the two renders must have the same size).

Spec §C.5 (c): render at longest side 1500 px, metric = fraction of pixels with max-channel difference > 32; Combine by Color ≤ 0.3 %; Flattener with text fixes, white rectangles and duplicates off ≤ 0.5 %. resvg 0.48 API: `let mut opt = resvg::usvg::Options::default(); opt.fontdb_mut().load_fonts_dir(dir); opt.fontdb_mut().set_sans_serif_family("DejaVu Sans"); opt.font_family = "DejaVu Sans".into(); let tree = resvg::usvg::Tree::from_data(svg, &opt)?; let size = tree.size(); let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).unwrap(); resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pixmap.as_mut()); pixmap.data()`.

- [ ] **Step 1: Write the tests**

Append to `tests/support/mod.rs`:

```rust
/// Renders an SVG with resvg using ONLY the vendored fonts, longest side `max_side` px.
/// Returns `(width, height, premultiplied RGBA8)`.
pub fn render_png(svg: &[u8], max_side: u32) -> (u32, u32, Vec<u8>) {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_fonts_dir(fontdir());
    opt.fontdb_mut().set_sans_serif_family("DejaVu Sans");
    opt.font_family = "DejaVu Sans".to_string();
    let tree = resvg::usvg::Tree::from_data(svg, &opt).expect("resvg parses the document");
    let size = tree.size();
    let scale = max_side as f32 / size.width().max(size.height());
    let w = ((size.width() * scale).round() as u32).max(1);
    let h = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).expect("pixmap");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    (w, h, pixmap.data().to_vec())
}

/// Fraction of pixels whose largest channel difference exceeds `threshold` (spec §C.5 (c)).
pub fn pixel_diff_fraction(a: &(u32, u32, Vec<u8>), b: &(u32, u32, Vec<u8>), threshold: u8) -> f64 {
    assert_eq!((a.0, a.1), (b.0, b.1), "renders differ in size");
    let n = (a.0 as usize) * (a.1 as usize);
    let differing = a
        .2
        .chunks_exact(4)
        .zip(b.2.chunks_exact(4))
        .filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > threshold))
        .count();
    differing as f64 / n.max(1) as f64
}
```

Create `tests/invariance.rs`:

```rust
mod support;

use std::ffi::OsString;

use support::{pixel_diff_fraction, render_png, with_vendored_fonts};

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn run(tool_args: &[&str], svg: &[u8]) -> Vec<u8> {
    with_vendored_fonts(|| sciink::run(&args(tool_args), svg)).unwrap().svg
}
fn diff(before: &[u8], after: &[u8]) -> f64 {
    let (a, b) = (render_png(before, 1500), render_png(after, 1500));
    pixel_diff_fraction(&a, &b, 32)
}

#[test]
fn the_metric_sees_a_moved_rectangle_and_nothing_in_an_identical_render() {
    let a = format!(r##"<svg {NS} width="100" height="100" viewBox="0 0 100 100"><rect x="10" y="10" width="30" height="30" fill="#000"/></svg>"##);
    let b = a.replace(r#"x="10""#, r#"x="50""#);
    assert_eq!(diff(a.as_bytes(), a.as_bytes()), 0.0);
    let d = diff(a.as_bytes(), b.as_bytes());
    assert!(d > 0.1, "two 30×30 squares out of 100×100 → about 18 %: {d}");
}

/// Ungroup + clip/mask composition + clone unlinking only: the document must look the same.
#[test]
fn ungroup_only_flattener_is_visually_invariant_on_text_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let out = run(
        &["--tool=flattener", "--tab=Options", "--id=layer1", "--fixtext=false", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"],
        &input,
    );
    let d = diff(&input, &out);
    eprintln!("Text_tests ungroup-only pixel diff: {:.4} %", d * 100.0);
    assert!(d <= 0.005, "{d}");
}

#[test]
fn combine_by_color_is_visually_invariant_on_other_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Other_tests.svg")).unwrap();
    let out = run(&["--tool=combine-by-color", "--tab=scaling", "--id=layer1"], &input);
    let d = diff(&input, &out);
    eprintln!("Other_tests Combine by Color pixel diff: {:.4} %", d * 100.0);
    assert!(d <= 0.003, "{d}");
}

/// The big one (5.9 MB, 18 000 elements): slow in a debug build, so opt in.
/// Run: `cargo test --release --test invariance -- --ignored --nocapture`
#[test]
#[ignore]
fn ungroup_only_flattener_is_visually_invariant_on_acid_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Acid_tests.svg")).unwrap();
    let out = run(
        &["--tool=flattener", "--tab=Options", "--id=layer1", "--fixtext=false", "--revertpaths=false", "--removeduppaths=false", "--removerectw=false"],
        &input,
    );
    let d = diff(&input, &out);
    eprintln!("Acid_tests ungroup-only pixel diff: {:.4} %", d * 100.0);
    assert!(d <= 0.005, "{d}");
}
```

- [ ] **Step 2: Add the dev-dependency and run**

`Cargo.toml` `[dev-dependencies]`: `resvg = "0.48.1"` (keep `roxmltree`). Then `cargo test --test invariance 2>&1 | grep -E "^test |pixel diff|panicked" ` (the first build compiles resvg; expect a minute). Then the ignored Acid run in release as the doc comment says — report its number.

If an invariance assertion fails, do **not** loosen it: report the fraction and save both renders as PNG for the controller (`resvg::tiny_skia::Pixmap::save_png` on the pixmaps — add a `SCIINK_DIFF_DIR` env check in `render_png` only if you need it, and say so).

- [ ] **Step 3: Gate and commit**

`cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E "^test result|FAILED"`.

```bash
git add Cargo.toml Cargo.lock tests/support/mod.rs tests/invariance.rs
git commit -m "test: resvg appearance-invariance oracle for the ungroup-only Flattener and Combine by Color"
```

---

## Out of scope (deferred)

Scaler, Homogenizer, Favorite Markers (Plans 7–8); document scale `px_per_uu` (Plan 7); a shared `FontSystem` between `remove_kerning` and the bbox stage (fonts load twice per Flattener run — measure before optimising); `gradient_fixup` copy deduplication (Plan 7, parked in Plan 5); flowed text editing and text-on-a-path boxes (Plan 4 residuals); `remove_position_overflows` redistribution (text engine); the `--debugparser` switch (accepted, ignored — Text Highlight draws the same rectangles); CI does not run the fixture oracles (the upstream data is not in the repository).

## Self-review notes (controller)

- Spec coverage (§B.3 Flattener): step 1 exclusions → Task 2; step 2 defs/clips to root → Task 3; step 3 "No objects selected!" → Task 2; step 4 unlink → Task 3; step 5 deep ungroup + glyph groups → Task 3; step 6 rectangle pass (white rects, minus, thin) → Task 4; step 7 font replacement, deswitch, `remove_kerning`, text clips → Task 5; step 8 bbox stage (duplicates, white rects) → Task 6; step 9 gc, whitespace, `unlinked_clone` → Tasks 1–2 (`finish`); `--testmode` → Task 2; §C.5 (b) golden oracles → Task 7; §C.5 (c) invariance → Task 8.
- Type consistency: `Options` fields named after the `.inx` parameters; every phase takes `(doc: &mut Doc, ctx: &mut Ctx, …)` and the working set as `&[NodeId]`/`&mut Vec<NodeId>`; `rect_passes` returns the white-rectangle candidates consumed by `bbox_stage`; `remove_kerning`'s `Vec<NodeId>` replaces the text members of `ngs`; `attached` is applied before every phase that indexes into the set.
- Placeholder scan: every step carries code or an exact command; the fixture oracles compute their expectations from the reference files, not from hard-coded numbers (the measured numbers above are documentation); the two `let _ = …` lines in the skeleton are removed by the tasks that consume the values.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-22-plan6-flattener.md`. Execute with **subagent-driven development** on a branch `plan6-flattener` off `main` (52a6398), with the same process rules as Plans 3–5: never weaken a test to make it pass; plan defects become controller rulings mirrored into this document; sonnet implementers with foreground `cargo` runs; reviewers use the bounded method (one focused `cargo test` run as the numeric evidence, no hand-tracing).

## Post-review fix wave (2026-09-22)

The whole-branch review (opus, range `52a6398..c7eb001`) rated the branch "with fixes"; one fix wave landed as `ecb1acd`, `2136e11`, `95fa3c7`, `47c24ae`, verified by a scoped re-review (every item addressed, no new breakage) and by CI on `47c24ae` (gate: fmt clean, clippy 0 warnings, 257 passed / 0 failed / 6 ignored; all three fixture oracles pass; invariance 0.0000 % / 0.4037 % / 0.1210 %):

1. **F1 (Important)** `text_phase` loaded the system fonts even when the working set held no `text`/`flowRoot`; the kerning block (font load, `remove_kerning`, re-attachment of its output) now runs only when `tels` is non-empty. A text-free Flattener run no longer scans fonts.
2. **F3 (Important)** the `#[ignore]` content oracles set `SCIINK_FONT_DIRS` from two tests concurrently (a `set_var` race); the variable is now set once through a `std::sync::Once` and the README's oracle command carries `--test-threads=1`.
3. **F4 (Important)** the Task 6/7 "rgb only" deviation rested on a false claim about `inkex.Color` (it carries alpha). `remove_duplicates` compares the paints as rgba (`same_rgba`, alpha within 1e-9) with the top paint opaque, and the spec bullet and the two sentences above were corrected. Because the specified-style equality is checked first, no decision changes.
4. **F5 (Important)** the README's Exclusions sentence promised more than the tool does; reworded to what `mark_exclusions` does (mark, un-mark, skip marked subtrees).
5. Minors taken now: `strip_whitespace` keeps a text node that follows a comment inside `<text>` by its parent's tag (a comment's "tail" was dropped as if it belonged to the comment); `remove_duplicates` caches each element's global path lazily instead of re-parsing per pair; `is_minus_glyph` parses `MINUS_D` once (`LazyLock`); the phases are `pub(crate)`; tests for a root `<defs>` created on demand and a `<mask>` moved to it, a `<switch>` left alone when the kerning flags are off, duplicate removal and white-rectangle removal driven together, exclusion through a marked ancestor, an unknown `--id`, and a dedup-only pixel-invariance case (Text_tests, 0.4037 % ≤ 0.5 %); the corresponding deviation bullets in the spec (F13).
6. Parked with a ruling: **F2** — the Flattener loads the system fonts twice per run (once in `remove_kerning`, once in the bbox stage's `Ctx`), about 0.5 s on this machine; sharing one `FontSystem` touches Plan 4's `remove_kerning` ownership and is the first item of Plan 7. Tolerances of the fixture oracles (paths ±3 %, rects ±3, content ≥ 0.85) kept as planned with the measured numbers documented — fonts differ across machines. Plan 5 follow-up: `unlink` on a mutually referencing `<use>` pair rewrites one href silently (pathological input, no crash).

Execution rulings not visible in the tasks above (all mirrored into the task text where they changed it): selecting a layer dissolves the layer itself, as upstream's `seld` does, so the tests select inner elements (`--testmode` keeps the layer because it flattens the layer's children); `RECT_TAGS` is defined once in Task 4 and reused by Task 6; roxmltree resolves prefixes, so namespaced attributes are read as `attribute((ns, local))` through the `nsattr` test helper; the comment-detach loop in `deep_ungroup` collects before mutating (E0502); roxmltree's `prev_siblings()` includes the node itself (`.skip(1)` in the oracle); raw strings containing `"#` use `r##"…"##`; Task 2's two ungroup-dependent tests were ignored for exactly one commit until Task 3 landed; the parent-before-child font replacement (a redundant inline family on an un-styled tspan) matches upstream's order and was accepted.
