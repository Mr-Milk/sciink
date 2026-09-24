# Slimmer Implementation Plan (Plan 10, ships in 0.2.0)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new tool, **Slimmer** (`--tool=slimmer`, menu "Slimmer (sciink)"), that makes a whole document
smaller and faster for Inkscape with rendering-exact steps by default: duplicate stylesheets, empty and
invisible elements, single-child wrapper groups, unused definitions, identical definitions; plus an opt-in
coordinate-precision step and a report dialog.

**Architecture:** one new tool module `src/tools/slimmer.rs` holding the six steps as pure functions over
`Doc` (each with its exactness argument in the doc comment), a general reference scanner and two small
helpers in `src/ops/cleanup.rs`, two accessors on `style::Stylesheet`. No `Ctx`, no fonts, no bbox
machinery: every deletion is of something unreferenced by construction and the merge step repoints its own
references. Whole document; the selection is ignored.

**Tech Stack:** Rust (std only, no new dependencies), clap for options, the existing lossless `dom::Doc`,
`style` cascade, `geom::path::parse_d`, `geom::ipx`; tests with roxmltree and the resvg raster oracle in
`tests/support`.

**Spec:** `docs/spec/02-geometry-tools.md` (§B.3 Tools; a "### Slimmer" section and "Deliberate deviations
(Plan 10)" are added by T9). Design and measurements: Appendix A of this plan (copied from the approved
design, 2026-09-24). Upstream reference for the unused-definition step only:
`~/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/scientific_inkscape/dhelpers.py:990`
`clean_up_document`.

## Global Constraints

- Std only; no new crate dependencies.
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` green on every
  commit. **No assertion is ever loosened.**
- Every step that is on by default is rendering-identical (SVG semantics) and carries (1) the argument in
  its doc comment and (2) a guard test naming what must NOT be removed. The raster oracle
  (`support::render_png` + `pixel_diff_fraction`) must report **exactly 0.0** for the default options on
  every upstream fixture and on `BigDoc`.
- Tool contract (Plans 1–9): `pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String>`;
  options parsed with clap in the tool's own struct (bools via `inx_bool` with `action = Set`); `log::Timer`
  phases `parse`, one per step, `write`, `total`; user text only through `Output.messages` (no
  `eprintln!`); `needs-live-preview="false"` in the inx.
- Complexity O(n) or O(n log n) in the element count; the reference scanner runs once per fixpoint round.
- Upstream parity for the unused-definition step's intent; every other behaviour is a documented deviation
  under "Deliberate deviations (Plan 10)" (T9).
- Commit messages end with the trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` exactly
  (do not substitute another model name).
- Shell gotcha for anyone working in this worktree: the session guard refuses commands whose text contains
  `git` or that compute command names; use `/usr/bin/git …` for version control, plain literal commands
  otherwise, and the Edit/Write tools for files.

## File structure

| File | Responsibility |
|---|---|
| `src/tools/slimmer.rs` (new) | `SlimmerCli`, `Report`, `slim` orchestration, the six steps, `round_numbers`, `report_message`, `run` |
| `src/ops/cleanup.rs` | `referenced_ids`, `url_ids`, `css_idents`, `detach_tidy`, `is_layer` (general helpers, reusable) |
| `src/style.rs` | `Doc::stylesheet` made `pub`; `Stylesheet::only_universal_rules`, `Stylesheet::declares_any` |
| `src/cli.rs`, `src/lib.rs`, `src/tools/mod.rs` | registration and HELP |
| `inx/slimmer.inx` (new) | the menu entry |
| `tests/slimmer.rs` (new), `tests/ops_cleanup.rs`, `tests/style.rs`, `tests/cli.rs`, `tests/bench.rs` | tests |
| `README.md`, `docs/spec/02-geometry-tools.md`, `docs/spec/00-overview.md`, `docs/DEVELOPING.md`, `CHANGELOG.md`, `Cargo.toml` | docs and version (T9) |

Step order inside `slim`: parse → (a) `styles` → (b) `empty` → (c) `wrappers` → (d) `prune` → (e) `merge`
→ (f) `precision` → write. Reasons: (b) before (d) because an invisible clipped shape hides an unused
clipPath; (c) after (b) because emptied siblings create new single-child wrappers; (d) before (e) so unused
clips never enter the canonicaliser; (e) orphans nothing.

---

### Task 1: Reference scanner and helpers

**Files:**
- Modify: `src/ops/cleanup.rs` (append after `strip_attr`, end of file)
- Modify: `src/style.rs:395-398` (`impl Stylesheet`), `src/style.rs:680` (`fn stylesheet`)
- Test: `tests/ops_cleanup.rs`, `tests/style.rs`

**Interfaces:**
- Produces: `pub fn referenced_ids(doc: &Doc) -> HashSet<String>`, `pub fn url_ids(v: &str, out: &mut HashSet<String>)`,
  `pub fn css_idents(css: &str, out: &mut HashSet<String>)`, `pub fn detach_tidy(doc: &mut Doc, n: NodeId)`,
  `pub fn is_layer(doc: &Doc, n: NodeId) -> bool` in `sciink::ops::cleanup`;
  `pub fn stylesheet(&self) -> Rc<Stylesheet>` on `Doc`; `Stylesheet::{only_universal_rules, declares_any}`.
- Consumes: `Doc::{descendants, is_element, is_text, text, text_content, attrs, attr, tag, parent, prev_sibling, detach}`,
  the private `TEXT_KEEP` list in `cleanup.rs`, `Rule.universal` and `Stylesheet.props` in `style.rs`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/ops_cleanup.rs` (extend the existing `use sciink::ops::cleanup::{…}` line with
`detach_tidy, is_layer, referenced_ids`):

```rust
#[test]
fn referenced_ids_finds_every_reference_form_and_ignores_data_uris() {
    let d = doc(&format!(
        r##"<svg {NS} xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape">
<style>#s1 {{ fill: red }} .c {{ mask: url( '#s2' ) }} #s4.cls {{ fill: blue }}</style>
<g clip-path="url(#a1)" style="fill:url( #a2 ); stroke:#123456" mask="url(&quot;#a3&quot;)"/>
<use xlink:href="#h1"/><use href="#h2"/>
<path inkscape:path-effect="#pe1;#pe2" inkscape:connection-start="#cs1" d="M0 0 #notanid"/>
<image xlink:href="data:image/png;base64,QUJD#zz"/>
<text id="t1">#hashtag</text>
</svg>"##
    ));
    let r = referenced_ids(&d);
    for id in ["s1", "s2", "s4", "a1", "a2", "a3", "h1", "h2", "pe1", "pe2", "cs1"] {
        assert!(r.contains(id), "{id} is referenced: {r:?}");
    }
    for id in ["notanid", "zz", "hashtag", "t1", "123456"] {
        assert!(!r.contains(id), "{id} is not a reference: {r:?}");
    }
}

#[test]
fn detach_tidy_removes_the_indentation_before_the_node_but_not_text_inside_text_elements() {
    let mut d = doc(&format!(
        "<svg {NS}>\n  <g id=\"g\">\n    <rect id=\"r\"/>\n  </g>\n  <text id=\"t\">a <tspan id=\"s\">b</tspan> c</text>\n</svg>"
    ));
    detach_tidy(&mut d, id(&d, "r"));
    let s = out(&d);
    assert!(
        s.contains("<g id=\"g\">\n  </g>"),
        "the rect and its own indentation are gone, the closing indentation stays: {s}"
    );
    detach_tidy(&mut d, id(&d, "s"));
    let s = out(&d);
    assert!(
        s.contains("<text id=\"t\">a  c</text>"),
        "inside <text> the preceding text is content, not indentation: {s}"
    );
}

#[test]
fn is_layer_reads_inkscape_groupmode() {
    let d = doc(&format!(
        r#"<svg {NS} xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"><g id="l" inkscape:groupmode="layer"/><g id="g"/><rect id="r" inkscape:groupmode="layer"/></svg>"#
    ));
    assert!(is_layer(&d, id(&d, "l")));
    assert!(!is_layer(&d, id(&d, "g")));
    assert!(!is_layer(&d, id(&d, "r")), "only groups are layers");
}
```

Append to `tests/style.rs`:

```rust
#[test]
fn only_universal_rules_and_declares_any_read_the_parsed_sheet() {
    let d = doc(&format!(
        r#"<svg {NS}><style>*{{stroke-linejoin: round}} * {{ Opacity: .5 }}</style></svg>"#
    ));
    let s = d.stylesheet();
    assert_eq!(s.rule_count(), 2);
    assert!(s.only_universal_rules());
    assert!(s.declares_any(&["opacity", "filter"]), "names are lower-cased");
    assert!(!s.declares_any(&["filter"]));
    let d = doc(&format!(
        r#"<svg {NS}><style>*{{fill:red}} g {{fill:blue}}</style></svg>"#
    ));
    assert!(
        !d.stylesheet().only_universal_rules(),
        "a tag rule can match one element and not another"
    );
    let d = doc(&format!(r#"<svg {NS}><rect/></svg>"#));
    assert_eq!(d.stylesheet().rule_count(), 0);
    assert!(d.stylesheet().only_universal_rules(), "vacuously true");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test ops_cleanup referenced_ids detach_tidy is_layer` and `cargo test --test style only_universal`
Expected: compile errors (`referenced_ids`, `detach_tidy`, `is_layer` not found; `stylesheet` is private).

- [ ] **Step 3: Implement the helpers**

Append to `src/ops/cleanup.rs`:

```rust
/// `inkscape:groupmode="layer"` on a `<g>`: an Inkscape layer, kept even when empty.
pub fn is_layer(doc: &Doc, n: NodeId) -> bool {
    doc.tag(n) == "g" && doc.attr(n, "inkscape:groupmode") == Some("layer")
}

/// `detach(n)` plus the whitespace-only text node right before it — the indentation that would
/// otherwise stay behind as a blank line — unless the parent is text-bearing (`TEXT_KEEP`), where
/// that text is content.
pub fn detach_tidy(doc: &mut Doc, n: NodeId) {
    if let Some(prev) = doc.prev_sibling(n) {
        let blank = doc.is_text(prev) && doc.text(prev).is_some_and(|t| t.trim().is_empty());
        let content = doc
            .parent(n)
            .is_some_and(|p| doc.is_element(p) && TEXT_KEEP.contains(&doc.tag(p)));
        if blank && !content {
            doc.detach(prev);
        }
    }
    doc.detach(n);
}

/// Every id the document points at, over-inclusively — a false positive keeps an element, a
/// false negative would delete one. Counted: `url(#id)` in any attribute value (inline `style`
/// included) and in `<style>` text; `href`/`xlink:href` equal to `#id`; any other attribute whose
/// value is one `#id` token or a `;`/`,`/space list of them (`inkscape:path-effect`,
/// `inkscape:perspectiveID`, `inkscape:connection-start`); every `#ident` in `<style>` text
/// (selectors count, so a styled definition is never pruned or merged). A `fill="#rrggbb"`
/// attribute lands in the set as "rrggbb" — harmless. `id`, `d` and `points` are skipped; a
/// `data:` href costs O(1).
pub fn referenced_ids(doc: &Doc) -> HashSet<String> {
    let mut out = HashSet::new();
    for n in doc.descendants(doc.svg()) {
        if !doc.is_element(n) {
            continue;
        }
        for a in doc.attrs(n) {
            match a.name.as_str() {
                "id" | "d" | "points" => {}
                "href" | "xlink:href" => {
                    if let Some(id) = a.value.trim().strip_prefix('#') {
                        out.insert(id.to_string());
                    }
                }
                _ if a.value.contains("url(") => url_ids(&a.value, &mut out),
                _ if a.value.trim_start().starts_with('#') => {
                    for tok in a.value.split([';', ',', ' ']) {
                        if let Some(id) = tok.trim().strip_prefix('#') {
                            if !id.is_empty() {
                                out.insert(id.to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if doc.tag(n) == "style" {
            css_idents(&doc.text_content(n), &mut out);
        }
    }
    out
}

/// Every `#id` inside `url( … )`, tolerating whitespace and quotes (`url( '#a' )`).
pub fn url_ids(v: &str, out: &mut HashSet<String>) {
    for piece in v.split("url(").skip(1) {
        let Some(end) = piece.find(')') else { break };
        let inner = piece[..end].trim().trim_matches(['\'', '"']).trim();
        if let Some(id) = inner.strip_prefix('#') {
            out.insert(id.to_string());
        }
    }
}

/// Every `#ident` in CSS text — `url(#id)`, `#id {…}` selectors and `#rrggbb` colours alike —
/// plus, for `#id.class` / `#id:hover`, the ident cut at each `.` and `:` (an SVG id may itself
/// contain `.`, so both readings are kept).
pub fn css_idents(css: &str, out: &mut HashSet<String>) {
    for piece in css.split('#').skip(1) {
        let run: String = piece
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':') || !c.is_ascii())
            .collect();
        if run.is_empty() {
            continue;
        }
        for (i, c) in run.char_indices() {
            if i > 0 && (c == '.' || c == ':') {
                out.insert(run[..i].to_string());
            }
        }
        out.insert(run);
    }
}
```

In `src/style.rs`, make the sheet accessor public and add the two accessors:

```rust
    /// The document stylesheet: every `<style>` element's text concatenated in document order
    /// (any depth), cached on `sheet_generation`.
    pub fn stylesheet(&self) -> Rc<Stylesheet> {
```

```rust
impl Stylesheet {
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// True when every rule is a lone `*`: nothing can match one element and not another.
    pub fn only_universal_rules(&self) -> bool {
        self.rules.iter().all(|r| r.universal)
    }

    /// True when the sheet declares any of `props` (lower-case property names).
    pub fn declares_any(&self, props: &[&str]) -> bool {
        props.iter().any(|p| self.props.contains(*p))
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test ops_cleanup` and `cargo test --test style`
Expected: PASS (all, including the pre-existing tests).

- [ ] **Step 5: Gate and commit**

Run: `cargo fmt --all && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: green, 359 + 4 tests.

```bash
/usr/bin/git add src/ops/cleanup.rs src/style.rs tests/ops_cleanup.rs tests/style.rs
/usr/bin/git commit -q -m "feat(cleanup): general reference scanner, detach_tidy, is_layer; stylesheet accessors" -m "Groundwork for the Slimmer tool: referenced_ids counts every url(#id), href, #id-valued attribute and every #ident in <style> text (over-inclusive by design); detach_tidy drops the indentation with the node; Stylesheet::only_universal_rules and declares_any expose the existing Rule.universal flags and props set." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Tool skeleton, registration, inx, step (a) duplicate stylesheets, report

**Files:**
- Create: `src/tools/slimmer.rs`, `inx/slimmer.inx`, `tests/slimmer.rs`
- Modify: `src/cli.rs:12-24` (enum), `src/cli.rs:106` (HELP), `src/lib.rs:38-50` (dispatch), `src/tools/mod.rs` (module), `tests/cli.rs`

**Interfaces:**
- Produces: `sciink::tools::slimmer::{SlimmerCli, Report, run, slim, dedup_stylesheets, report_message}`; the
  `--tool=slimmer` CLI with options `--tab`, `--dedupstyles`, `--removeempty`, `--collapsegroups`,
  `--pruneunused`, `--mergedefs`, `--precision`, `--report`.
- Consumes: T1 helpers; `crate::log::Timer`; `Doc::{descendants, is_element, tag, attrs, text_content, parent, children, insert_before, append_child, element_count, write}`.

- [ ] **Step 1: Write the failing tests**

Create `tests/slimmer.rs`:

```rust
//! Slimmer (Plan 10): every default step is rendering-exact; the guard tests name what must stay.

mod support;

use std::collections::HashSet;
use std::ffi::OsString;

use sciink::dom::Doc;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";
const XLINK: &str = "xmlns:xlink=\"http://www.w3.org/1999/xlink\"";
const INK: &str = "xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}

/// Runs the Slimmer with the default options plus `extra`; returns the document and the messages.
fn slim(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=slimmer", "--tab=Options"];
    a.extend(extra);
    let out = sciink::run(&args(&a), svg.as_bytes()).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}

fn has(d: &roxmltree::Document, id: &str) -> bool {
    d.descendants().any(|n| n.attribute("id") == Some(id))
}

fn by_id<'a>(d: &'a roxmltree::Document<'a>, id: &str) -> roxmltree::Node<'a, 'a> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element with id {id}"))
}

/// Every `url(#x)` and `#x` href in the document has a target.
fn every_reference_resolves(d: &roxmltree::Document) -> bool {
    let ids: HashSet<&str> = d.descendants().filter_map(|n| n.attribute("id")).collect();
    d.descendants().filter(|n| n.is_element()).all(|n| {
        n.attributes().all(|a| {
            let v = a.value();
            let urls = v.split("url(#").skip(1).all(|piece| {
                piece
                    .split(')')
                    .next()
                    .is_some_and(|id| ids.contains(id.trim().trim_matches(['\'', '"'])))
            });
            let href = a.name() != "href" || !v.starts_with('#') || ids.contains(&v[1..]);
            urls && href
        })
    })
}

#[test]
fn duplicate_stylesheets_keep_the_last_copy_and_the_cascade() {
    let svg = format!(
        r#"<svg {NS}><style id="s1">*{{fill:red}}</style><g id="wrap"><style id="s2">*{{fill:blue}}</style></g><style id="s3">*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        !has(&d, "s1") && has(&d, "s2") && has(&d, "s3"),
        "A B A → B A: the first copy goes, the last stays: {s}"
    );
    assert!(
        msgs[0].contains("duplicate stylesheets removed: 1"),
        "{msgs:?}"
    );
    let fill = |svg: &str| {
        let d = Doc::parse(svg.as_bytes()).unwrap();
        d.computed(d.by_id("r").unwrap(), "fill")
    };
    assert_eq!(fill(&svg), "red", "the last rule wins before");
    assert_eq!(fill(&s), "red", "and after");
}

#[test]
fn a_lone_surviving_stylesheet_moves_to_the_root_and_at_rules_or_extra_attributes_block_dedup() {
    let svg = format!(
        r#"<svg {NS}><g id="fig1"><defs><style id="a">*{{fill:red}}</style></defs><rect id="r1" width="1" height="1"/></g><g id="fig2"><defs><style id="b">*{{fill:red}}</style></defs><rect id="r2" width="1" height="1"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "a") && has(&d, "b"), "{s}");
    let first = d
        .root_element()
        .children()
        .find(|c| c.is_element())
        .unwrap();
    assert_eq!(
        first.attribute("id"),
        Some("b"),
        "the lone sheet now leads the root, out of figure 2's <defs>: {s}"
    );
    assert!(
        msgs[0].contains("(1 kept, moved to the document root)"),
        "{msgs:?}"
    );
    let svg = format!(
        r#"<svg {NS}><style id="m">@import url(x.css); *{{fill:red}}</style><style id="n">@import url(x.css); *{{fill:red}}</style><style id="p" media="print">*{{fill:red}}</style><style id="q" media="print">*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["m", "n", "p", "q"] {
        assert!(has(&d, id), "{id}: an @-rule or an extra attribute blocks dedup: {s}");
    }
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
    assert_eq!(s, svg, "nothing changed → byte-identical");
}

#[test]
fn the_report_lists_bytes_elements_and_each_step_and_says_nothing_to_do_on_a_clean_document() {
    let svg = format!(r#"<svg {NS}><rect id="r" width="1" height="1"/></svg>"#);
    let (s, msgs) = slim(&svg, &[]);
    assert_eq!(s, svg);
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
    let (_, msgs) = slim(&svg, &["--report=false"]);
    assert!(msgs.is_empty(), "silent without the report: {msgs:?}");
    let svg = format!(
        r#"<svg {NS}><style>*{{fill:red}}</style><style>*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let lines: Vec<&str> = msgs[0].lines().collect();
    assert!(
        lines[0].starts_with("Slimmer: ") && lines[0].contains(" → ") && lines[0].ends_with("3 → 2 elements"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains(&format!("{} B → {} B", svg.len(), s.len())), "{}", lines[0]);
    assert_eq!(lines[1], "  duplicate stylesheets removed: 1");
    assert_eq!(lines[2], "  empty or invisible elements removed: 0");
    assert_eq!(lines[3], "  wrapper groups collapsed: 0");
    assert_eq!(lines[4], "  unused definitions removed: 0");
    assert_eq!(lines[5], "  identical definitions merged: 0");
    assert_eq!(lines[6], "  coordinate precision: unchanged");
    assert_eq!(lines.len(), 7);
    let (_, msgs) = slim(&svg, &["--dedupstyles=false"]);
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()], "the step can be switched off");
}
```

Append to `tests/cli.rs` (uses the existing `bin`, `tmp` and `SIMPLE`):

```rust
#[test]
fn slimmer_runs_through_the_binary_and_reports_nothing_to_do_on_the_simple_fixture() {
    let p = tmp("slim.svg", SIMPLE);
    let out = bin()
        .args(["--tool=slimmer", "--tab=Options"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        SIMPLE,
        "nothing to slim: the document is echoed byte for byte"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(err.trim(), "Slimmer: nothing to do", "{err}");
    let out = bin()
        .args(["--tool=slimmer", "--precision=3"])
        .arg(&p)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("precision must be 0 or 4–8") && err.ends_with("The document was left unchanged.\n"),
        "{err}"
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test slimmer` and `cargo test --test cli slimmer_runs`
Expected: `unknown tool 'slimmer'` / compile error (module missing).

- [ ] **Step 3: Register the tool**

`src/cli.rs` — add the variant after `FavoriteMarkers,`:

```rust
    FavoriteMarkers,
    Slimmer,
    About,
```

and in `HELP` change the usage line to

```
    sciink --tool=<flattener|scaler|homogenizer|text-ghoster|combine-by-color|favorite-markers|slimmer|about>
```

`src/lib.rs` — add the arm before `other =>`:

```rust
        "slimmer" => tools::slimmer::run(argv, input),
```

`src/tools/mod.rs` — add `pub mod slimmer;` between `scaler` and `text_fix`.

- [ ] **Step 4: Create `inx/slimmer.inx`**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Slimmer (sciink)</name>
    <id>org.sciink.slimmer</id>
    <param name="tool" type="string" gui-hidden="true">slimmer</param>
    <param name="tab" type="notebook">
        <page name="Options" gui-text="Options">
            <label>Makes the whole document smaller and faster to load, save and edit in Inkscape. The selection is ignored. Every option except coordinate precision leaves the rendering exactly unchanged.</label>
            <param name="dedupstyles" type="bool" gui-text="Merge duplicate stylesheets" gui-description="Keeps one copy of identical &lt;style&gt; elements (matplotlib adds one per imported figure). Every rule costs Inkscape time on each load and save.">true</param>
            <param name="removeempty" type="bool" gui-text="Remove empty and invisible elements" gui-description="Empty paths and text, zero-size shapes, shapes with neither fill nor stroke, empty groups. Hidden objects, layers and labelled objects are kept.">true</param>
            <param name="collapsegroups" type="bool" gui-text="Collapse single-child wrapper groups" gui-description="A group holding one object and carrying nothing but an id is replaced by the object, which inherits the id. Layers, labelled, styled and referenced groups are kept.">true</param>
            <param name="pruneunused" type="bool" gui-text="Remove unused definitions" gui-description="Clip paths, masks, gradients, patterns, markers, filters and symbols nothing refers to, including inside nested &lt;defs&gt;.">true</param>
            <param name="mergedefs" type="bool" gui-text="Merge identical definitions" gui-description="Definitions with identical content (for example one clip path per axes) are kept once and every reference is repointed.">true</param>
            <param name="precision" type="optiongroup" appearance="combo" gui-text="Coordinate precision" gui-description="Rounds path and shape coordinates. Not rendering-exact: 6 digits moves a point by at most 0.0005 per 1000 units.">
                <option value="0">Unchanged (exact)</option>
                <option value="8">8 significant digits</option>
                <option value="7">7 significant digits</option>
                <option value="6">6 significant digits</option>
                <option value="5">5 significant digits</option>
                <option value="4">4 significant digits</option>
            </param>
            <param name="report" type="bool" gui-text="Show a report" gui-description="What was removed and the size before and after.">true</param>
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

- [ ] **Step 5: Create `src/tools/slimmer.rs`**

```rust
//! Slimmer (Plan 10): makes a whole document smaller and faster for Inkscape. No upstream
//! counterpart except the unused-definition step, which follows `dhelpers.py:990
//! clean_up_document`. Whole-document scope; every step that is on by default is rendering-exact
//! — the argument is in each step's doc comment, the guard tests in `tests/slimmer.rs` pin what
//! must stay. No `Ctx`: nothing here creates clips, every deletion is of something unreferenced by
//! construction, and the merge step repoints its own references.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Write as _;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::ops::cleanup::detach_tidy;

use super::first_line;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct SlimmerCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports; unused.
    #[arg(long, default_value = "Options")]
    pub tab: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub dedupstyles: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removeempty: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub collapsegroups: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub pruneunused: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergedefs: bool,
    /// Significant digits for path and shape coordinates; 0 keeps them as written.
    #[arg(long, default_value_t = 0)]
    pub precision: u8,
    /// Show the report dialog.
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub report: bool,
}

/// What a run did; every counter feeds one report line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub sheets_removed: usize,
    pub sheet_moved: bool,
    pub empty_removed: usize,
    pub wrappers_collapsed: usize,
    pub defs_pruned: usize,
    pub prune_rounds: usize,
    pub containers_removed: usize,
    pub defs_merged: usize,
    pub attrs_repointed: usize,
    pub numbers_rounded: usize,
    /// Extra lines: a skipped step and why.
    pub notes: Vec<String>,
}

impl Report {
    fn changed(&self) -> bool {
        self.sheet_moved
            || self.sheets_removed
                + self.empty_removed
                + self.wrappers_collapsed
                + self.defs_pruned
                + self.containers_removed
                + self.defs_merged
                + self.numbers_rounded
                > 0
    }
}

fn style_elements(doc: &Doc) -> Vec<NodeId> {
    doc.descendants(doc.svg())
        .filter(|&n| doc.is_element(n) && doc.tag(n) == "style")
        .collect()
}

/// Step (a): `<style>` elements with identical text collapse onto the LAST copy. Same-precedence
/// conflicts are decided by source order, later wins (`style::DeclKey`); a duplicated rule reaches
/// its maximum order at the last copy, so deleting the earlier copies changes no winner — sheets
/// `A B A` render as A, and keeping the first would give B. Sheets with attributes beyond `id` and
/// `type="text/css"`, or containing an `@` rule (`@import`, `@media`: position- and
/// count-sensitive), are left alone. When exactly one sheet remains it moves to the front of the
/// root: position is irrelevant for a lone sheet, and out of a figure's nested `<defs>` deleting
/// that figure can no longer restyle the whole document. Returns (removed, moved).
pub fn dedup_stylesheets(doc: &mut Doc) -> (usize, bool) {
    let mut last: HashMap<String, NodeId> = HashMap::new();
    let mut eligible: Vec<(NodeId, String)> = Vec::new();
    for s in style_elements(doc) {
        let plain = doc.attrs(s).iter().all(|a| {
            a.name == "id" || (a.name == "type" && a.value.trim().eq_ignore_ascii_case("text/css"))
        });
        let key = doc.text_content(s).trim().to_string();
        if !plain || key.contains('@') {
            continue;
        }
        last.insert(key.clone(), s); // later copies overwrite: the last one survives
        eligible.push((s, key));
    }
    let mut removed = 0;
    for (s, key) in eligible {
        if last[&key] != s {
            detach_tidy(doc, s);
            removed += 1;
        }
    }
    let mut moved = false;
    let remaining = style_elements(doc);
    if remaining.len() == 1 {
        let only = remaining[0];
        let svg = doc.svg();
        if doc.parent(only) != Some(svg) {
            match doc.children(svg).find(|&c| doc.is_element(c)) {
                Some(first) if first != only => doc.insert_before(only, first),
                _ => doc.append_child(svg, only),
            }
            moved = true;
        }
    }
    (removed, moved)
}

/// Runs the enabled steps in order, one `Timer` phase each.
pub fn slim(doc: &mut Doc, o: &SlimmerCli, t: &mut crate::log::Timer) -> Report {
    let mut r = Report::default();
    if o.dedupstyles {
        let (n, moved) = dedup_stylesheets(doc);
        r.sheets_removed = n;
        r.sheet_moved = moved;
        t.phase("styles", || format!("removed={n} moved={moved}"));
    }
    r
}

fn human(bytes: usize) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else if bytes >= 10_000 {
        format!("{:.1} kB", bytes as f64 / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// The dialog text: one line of totals, one per step, then the notes.
pub fn report_message(
    in_len: usize,
    out_len: usize,
    els_before: usize,
    els_after: usize,
    precision: u8,
    r: &Report,
) -> String {
    if !r.changed() && r.notes.is_empty() {
        return "Slimmer: nothing to do".to_string();
    }
    let pct = if in_len == 0 {
        0.0
    } else {
        (in_len as f64 - out_len as f64) / in_len as f64 * 100.0
    };
    let mut s = String::new();
    let _ = writeln!(
        s,
        "Slimmer: {} → {} ({}{:.0} %), {els_before} → {els_after} elements",
        human(in_len),
        human(out_len),
        if pct >= 0.0 { "-" } else { "+" },
        pct.abs()
    );
    let _ = writeln!(
        s,
        "  duplicate stylesheets removed: {}{}",
        r.sheets_removed,
        if r.sheet_moved { " (1 kept, moved to the document root)" } else { "" }
    );
    let _ = writeln!(s, "  empty or invisible elements removed: {}", r.empty_removed);
    let _ = writeln!(s, "  wrapper groups collapsed: {}", r.wrappers_collapsed);
    let _ = write!(s, "  unused definitions removed: {}", r.defs_pruned);
    if r.defs_pruned + r.containers_removed > 0 {
        let _ = write!(
            s,
            " in {} round(s) ({} emptied containers)",
            r.prune_rounds, r.containers_removed
        );
    }
    s.push('\n');
    let _ = write!(s, "  identical definitions merged: {}", r.defs_merged);
    if r.defs_merged > 0 {
        let _ = write!(s, " ({} attributes repointed)", r.attrs_repointed);
    }
    s.push('\n');
    if precision == 0 {
        let _ = writeln!(s, "  coordinate precision: unchanged");
    } else {
        let _ = writeln!(
            s,
            "  coordinate precision: {precision} significant digits, {} numbers changed",
            r.numbers_rounded
        );
    }
    for n in &r.notes {
        let _ = writeln!(s, "  {n}");
    }
    s.trim_end().to_string()
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = SlimmerCli::try_parse_from(argv).map_err(first_line)?;
    if cli.precision != 0 && !(4..=8).contains(&cli.precision) {
        return Err(format!(
            "precision must be 0 or 4–8 significant digits, got {}",
            cli.precision
        ));
    }
    let mut t = crate::log::Timer::new("slimmer");
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let els_before = doc.element_count();
    t.phase("parse", || {
        format!("bytes={} elements={els_before}", input.len())
    });
    let report = slim(&mut doc, &cli, &mut t);
    let els_after = doc.element_count();
    let mut svg = Vec::new();
    doc.write(&mut svg);
    t.phase("write", || format!("bytes={}", svg.len()));
    t.total(|| format!("elements_removed={}", els_before.saturating_sub(els_after)));
    let messages = if cli.report {
        vec![report_message(
            input.len(),
            svg.len(),
            els_before,
            els_after,
            cli.precision,
            &report,
        )]
    } else {
        Vec::new()
    };
    Ok(Output { svg, messages })
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test --test slimmer && cargo test --test cli`
Expected: PASS (the three `tests/slimmer.rs` tests, `slimmer_runs_through_the_binary…`, and
`no_inx_file_enables_live_preview` now also covers `inx/slimmer.inx`).

- [ ] **Step 7: Gate and commit**

Run: `cargo fmt --all && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
/usr/bin/git add src/tools/slimmer.rs src/tools/mod.rs src/cli.rs src/lib.rs inx/slimmer.inx tests/slimmer.rs tests/cli.rs
/usr/bin/git commit -q -m "feat(slimmer): new tool with duplicate-stylesheet removal and a report" -m "Slimmer (--tool=slimmer, Scientific menu) works on the whole document. Step (a) keeps the last of identical <style> elements (the cascade decides same-precedence conflicts by source order, so the last copy is the one that matters) and moves a lone survivor to the root. The report lists bytes, elements and one line per step; --report=false keeps the tool silent." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Step (b) — empty, zero-size and invisible elements

**Files:**
- Modify: `src/tools/slimmer.rs`
- Test: `tests/slimmer.rs`

**Interfaces:**
- Produces: `pub fn remove_empty(doc: &mut Doc, refs: &HashSet<String>) -> usize`; private predicates
  `removable_context`, `empty_group`, `empty_text`, `empty_shape`, `invisible_shape`, `no_markers`.
- Consumes: `ops::bbox::{has_bbox, SHAPES}`, `ops::cleanup::{referenced_ids, is_layer, detach_tidy}`,
  `geom::{ipx, PathEl}`, `geom::path::parse_d`, `Doc::{specified, computed, xml_space_preserve, text_content}`.

- [ ] **Step 1: Write the failing tests** (append to `tests/slimmer.rs`)

```rust
#[test]
fn empty_paths_zero_size_shapes_empty_text_and_empty_groups_are_removed() {
    let svg = format!(
        r#"<svg {NS}><g id="layer">
<path id="nod"/><path id="blank" d="  "/><path id="moveonly" d="M 1 2 M 3 4"/><path id="dot" d="M0 0L0 0" style="stroke:#000;stroke-linecap:round"/><path id="closed" d="M0 0Z" style="stroke:#000"/>
<rect id="flat" x="0" y="0" width="0" height="5"/><rect id="nowidth" y="0" height="5"/><circle id="r0" cx="1" cy="1" r="0"/><ellipse id="e0" cx="1" cy="1" rx="0" ry="2"/><line id="zl" x1="0" y1="0" x2="0" y2="0" style="stroke:#000"/>
<polyline id="nopts" points=" "/><rect id="ghost" width="5" height="5" style="fill:none;stroke:none"/><rect id="thin" width="5" height="5" style="fill:none;stroke:#000;stroke-width:0"/><rect id="attrnone" width="5" height="5" fill="none"/>
<text id="et"> </text><text id="wt" xml:space="preserve"> </text><text id="ok">a</text>
<g id="eg"/><g id="eg2"><g id="eg3"/></g><g id="cg"><!-- kept --></g>
<rect id="vis" width="5" height="5"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in [
        "nod", "blank", "moveonly", "flat", "nowidth", "r0", "e0", "nopts", "ghost", "thin",
        "attrnone", "et", "eg", "eg2", "eg3",
    ] {
        assert!(!has(&d, id), "{id} paints nothing and should be gone: {s}");
    }
    for id in ["layer", "dot", "closed", "zl", "wt", "ok", "cg", "vis"] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert!(
        msgs[0].contains("empty or invisible elements removed: 15"),
        "{msgs:?}"
    );
}

#[test]
fn hidden_objects_layers_labelled_spacers_switch_children_markers_filters_and_referenced_shapes_are_kept() {
    let svg = format!(
        r##"<svg {NS} {XLINK} {INK}>
<defs><marker id="m"><path d="M0 0h1"/></marker><filter id="f"><feFlood flood-color="red"/></filter><clipPath id="c"><rect id="clipr" width="1" height="1" style="fill:none;stroke:none"/></clipPath></defs>
<g id="hiddenlayer" inkscape:groupmode="layer" style="display:none"/>
<g id="emptylayer" inkscape:groupmode="layer"/>
<rect id="hidden" width="1" height="1" style="fill:none;stroke:none;display:none"/>
<rect id="spacer" inkscape:label="spacer" width="9" height="9" style="fill:none;stroke:none"/>
<switch><rect id="sw" width="0" height="0"/><text>fallback</text></switch>
<path id="marked" d="M0 0" style="marker-start:url(#m)"/>
<rect id="filtered" width="0" height="0" style="filter:url(#f)"/>
<use xlink:href="#target"/><rect id="target" width="0" height="0"/>
<g id="clipped" clip-path="url(#c)"><rect width="1" height="1"/></g>
</svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in [
        "hiddenlayer", "emptylayer", "hidden", "spacer", "sw", "marked", "filtered", "target",
        "clipr", "m", "f", "c", "clipped",
    ] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
    assert_eq!(s, svg, "byte-identical when nothing is removed");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test slimmer` (cargo accepts a single name filter at most; run the whole binary)
Expected: `empty_paths_…` fails (nothing removed yet, "nothing to do"); `hidden_objects_…` passes already (it is the guard).

- [ ] **Step 3: Implement step (b)**

Add the imports to `src/tools/slimmer.rs` (`HashSet` joins `HashMap`; the cleanup import line grows):

```rust
use std::collections::{HashMap, HashSet};

use crate::geom::path::parse_d;
use crate::geom::{PathEl, ipx};
use crate::ops::bbox::{SHAPES, has_bbox};
use crate::ops::cleanup::{detach_tidy, is_layer, referenced_ids};
```

Add the predicates and the step:

```rust
/// Context checks shared by every removable element: rendered (no `UNRENDERED` ancestor such as
/// `defs`, `clipPath`, `mask`), not referenced by id, not named (`inkscape:label` marks intent —
/// an invisible spacer, say), not a `<switch>` child (removing one changes which sibling is
/// chosen), not hidden (`display:none` is how Inkscape hides objects and layers: hidden is not
/// empty), no filter (a `feFlood` filter paints even on an empty shape).
fn removable_context(doc: &Doc, n: NodeId, refs: &HashSet<String>) -> bool {
    has_bbox(doc, n)
        && doc.attr(n, "id").is_none_or(|id| !refs.contains(id))
        && doc.attr(n, "inkscape:label").is_none()
        && doc.parent(n).is_some_and(|p| doc.tag(p) != "switch")
        && doc
            .specified(n, "display")
            .is_none_or(|v| v.trim() != "none")
        && doc
            .specified(n, "filter")
            .is_none_or(|v| v.trim() == "none")
}

/// A `<g>` with no element or comment children (a comment marks upstream's matplotlib glyph
/// groups), and not a layer.
fn empty_group(doc: &Doc, n: NodeId) -> bool {
    !is_layer(doc, n)
        && !doc
            .children(n)
            .any(|c| doc.is_element(c) || doc.is_comment(c))
}

/// A `<text>` without characters; preserved whitespace stays (it can carry `text-decoration`).
fn empty_text(doc: &Doc, n: NodeId) -> bool {
    let t = doc.text_content(n);
    t.trim().is_empty() && (t.is_empty() || !doc.xml_space_preserve(n))
}

/// Markers paint on a lone `M` and without any stroke, so a marked shape is never "empty".
fn no_markers(doc: &Doc, n: NodeId) -> bool {
    ["marker", "marker-start", "marker-mid", "marker-end"]
        .iter()
        .all(|p| doc.specified(n, p).is_none_or(|v| v.trim() == "none"))
}

/// Shapes the SVG spec does not render at all: an absent or blank `d`; only `moveto`s (`M0 0L0 0`
/// paints a round-cap dot and stays); `points` without a digit; a `rect` whose `width` or `height`
/// is missing or non-positive; `r`, `rx`, `ry` missing or non-positive. Never `line` (zero length
/// still paints caps). An unparsable length (`%`, `auto`) keeps the element.
fn empty_shape(doc: &Doc, n: NodeId) -> bool {
    let non_positive = |a: &str| match doc.attr(n, a) {
        None => true,
        Some(v) => ipx(v).is_some_and(|x| x <= 0.0),
    };
    match doc.tag(n) {
        "path" => match doc.attr(n, "d").map(str::trim) {
            None | Some("") => true,
            Some(d) => parse_d(d).is_some_and(|p| {
                p.path
                    .elements()
                    .iter()
                    .all(|e| matches!(e, PathEl::MoveTo(_)))
            }),
        },
        "polyline" | "polygon" => doc
            .attr(n, "points")
            .is_none_or(|p| !p.bytes().any(|b| b.is_ascii_digit())),
        "rect" => non_positive("width") || non_positive("height"),
        "circle" => non_positive("r"),
        "ellipse" => non_positive("rx") || non_positive("ry"),
        _ => false,
    }
}

/// Paints nothing: `fill:none` and either `stroke:none` or a zero stroke width. Raw values on
/// purpose — `inherit` and `context-*` are paints, not "none"; `opacity:0` is a deliberate hide.
fn invisible_shape(doc: &Doc, n: NodeId) -> bool {
    let none = |p: &str| doc.computed(n, p).trim() == "none";
    let zero_width = doc
        .specified(n, "stroke-width")
        .and_then(|v| ipx(&v))
        .is_some_and(|w| w == 0.0);
    none("fill") && (none("stroke") || zero_width)
}

/// Step (b): removes drawn elements that contribute nothing to the rendering — empty or zero-size
/// shapes, shapes with neither fill nor stroke, empty `<text>`, empty non-layer `<g>` — in reverse
/// document order, so a group emptied by this pass is caught in the same pass. Exact by the
/// predicates above; the guards are in `removable_context` and `no_markers`.
pub fn remove_empty(doc: &mut Doc, refs: &HashSet<String>) -> usize {
    let nodes: Vec<NodeId> = doc
        .descendants(doc.svg())
        .skip(1)
        .filter(|&n| doc.is_element(n))
        .collect();
    let mut removed = 0;
    for &n in nodes.iter().rev() {
        if !removable_context(doc, n, refs) {
            continue;
        }
        let go = match doc.tag(n) {
            "g" => empty_group(doc, n),
            "text" => empty_text(doc, n),
            t if SHAPES.contains(&t) => {
                no_markers(doc, n) && (empty_shape(doc, n) || invisible_shape(doc, n))
            }
            _ => false,
        };
        if go {
            detach_tidy(doc, n);
            removed += 1;
        }
    }
    removed
}
```

In `slim`, after the `dedupstyles` block:

```rust
    if o.removeempty {
        let refs = referenced_ids(doc);
        let n = remove_empty(doc, &refs);
        r.empty_removed = n;
        t.phase("empty", || format!("removed={n}"));
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test slimmer`
Expected: PASS. If `moveonly` survives, `parse_d` returned `None` for consecutive movetos: fix
`parse_d` (it must accept `M 1 2 M 3 4`), never the assertion.

- [ ] **Step 5: Gate and commit**

Run: `cargo fmt --all && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
/usr/bin/git add src/tools/slimmer.rs tests/slimmer.rs
/usr/bin/git commit -q -m "feat(slimmer): remove empty, zero-size and invisible elements" -m "Step (b): shapes the SVG spec does not render (blank d, only movetos, zero width/height/radius, no points), shapes with neither fill nor stroke, empty text and empty non-layer groups, in reverse document order. Guards: hidden objects (display:none), layers, inkscape:label, <switch> children, markers, filters and referenced ids all stay; nothing under defs/clipPath/mask is touched." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Step (c) — collapse single-child wrapper groups

**Files:**
- Modify: `src/tools/slimmer.rs`
- Test: `tests/slimmer.rs`

**Interfaces:**
- Produces: `pub fn collapse_wrappers(doc: &mut Doc, refs: &HashSet<String>) -> Result<usize, String>`
  (`Err` = the whole step was skipped, with the reason for the report); constants `COLLAPSE_CHILD`,
  `COLLAPSE_ANCESTORS`, `GROUP_EFFECT_PROPS`.
- Consumes: T1 `Doc::stylesheet`, `Stylesheet::{rule_count, only_universal_rules, declares_any}`,
  `Doc::{ancestors, root, children, is_comment, text, remove_attr, set_attr, replace}`.

- [ ] **Step 1: Write the failing tests** (append to `tests/slimmer.rs`)

```rust
#[test]
fn wrapper_groups_with_only_an_id_collapse_and_the_child_keeps_its_place_and_gets_the_id() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><rect id="before" width="1" height="1"/><g id="patch_1">
  <path d="M0 0h1"/>
</g><g id="outer"><g id="inner"><rect id="kid" width="1" height="1"/></g></g><g id="two"><rect width="1" height="1"/><rect width="1" height="1"/></g><g id="styled" style="opacity:.5"><rect width="1" height="1"/></g><g id="xf" transform="translate(1)"><rect width="1" height="1"/></g><rect id="after" width="1" height="1"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let kids: Vec<(String, String)> = by_id(&d, "layer")
        .children()
        .filter(|c| c.is_element())
        .map(|c| {
            (
                c.tag_name().name().to_string(),
                c.attribute("id").unwrap_or("").to_string(),
            )
        })
        .collect();
    let want = [
        ("rect", "before"),
        ("path", "patch_1"),
        ("rect", "kid"),
        ("g", "two"),
        ("g", "styled"),
        ("g", "xf"),
        ("rect", "after"),
    ];
    assert_eq!(
        kids,
        want.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<Vec<_>>(),
        "the wrapper's id moves to a child without one; nested wrappers collapse in one pass: {s}"
    );
    assert!(msgs[0].contains("wrapper groups collapsed: 3"), "{msgs:?}");
}

#[test]
fn wrapper_groups_are_kept_under_a_tag_rule_a_combinator_a_universal_opacity_rule_or_an_at_rule() {
    for sheet in [
        "g{fill:red}",
        "g > path{fill:red}",
        "*{opacity:.5}",
        "@media print{*{fill:red}} *{fill:red}",
    ] {
        let svg = format!(
            r#"<svg {NS}><style>{sheet}</style><g id="w"><path id="p" d="M0 0h1"/></g></svg>"#
        );
        let (s, msgs) = slim(&svg, &[]);
        let d = roxmltree::Document::parse(&s).unwrap();
        assert!(has(&d, "w"), "{sheet}: the wrapper stays: {s}");
        assert!(
            msgs.iter().any(|m| m.contains("wrapper groups kept: the stylesheet has")),
            "{sheet}: {msgs:?}"
        );
    }
    let svg = format!(
        r#"<svg {NS}><style>*{{stroke-linejoin:round}}</style><g id="w"><path id="p" d="M0 0h1"/></g></svg>"#
    );
    let (s, _) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        !has(&d, "w") && has(&d, "p"),
        "a lone * rule without group-level properties is safe: {s}"
    );
}

#[test]
fn layers_labelled_groups_use_targets_switch_children_and_title_wrappers_are_never_collapsed() {
    let svg = format!(
        r##"<svg {NS} {XLINK} {INK}>
<g id="layer" inkscape:groupmode="layer"><rect width="1" height="1"/></g>
<g id="named" inkscape:label="Panel A"><rect width="1" height="1"/></g>
<g id="cloned"><rect width="1" height="1"/></g><use xlink:href="#cloned"/>
<switch><g id="insw"><rect width="1" height="1"/></g></switch>
<g id="titled"><title>only a title</title></g>
<g id="commented"><!-- glyph group --><rect width="1" height="1"/></g>
<clipPath id="cp"><g id="inclip"><rect width="1" height="1"/></g></clipPath><rect clip-path="url(#cp)" width="1" height="1"/>
</svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["layer", "named", "cloned", "insw", "titled", "commented", "inclip", "cp"] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test slimmer`
Expected: the two `wrapper_groups_…` tests fail (nothing collapses, no note); `layers_labelled_…` passes already (guard).

- [ ] **Step 3: Implement step (c)**

```rust
/// Rendered children a wrapper may hand up; `title`, `desc`, `metadata`, `defs`, `style` mean
/// something different under a different parent.
const COLLAPSE_CHILD: &[&str] = &[
    "g", "a", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "use", "text",
    "image", "flowRoot", "switch",
];
/// Ancestors under which a group is a plain container. Not `switch` (which picks one child), not
/// `clipPath`/`mask`/`symbol`/`defs` (a `<g>` inside a clipPath is ignored by renderers; collapsing
/// it would activate the child).
const COLLAPSE_ANCESTORS: &[&str] = &["svg", "g", "a"];
/// Properties a lone `*` rule would apply to the wrapper AND its child, where the group level is
/// not redundant: non-inherited effects compound (`opacity` 0.5 × 0.5) or depend on the box.
const GROUP_EFFECT_PROPS: &[&str] = &[
    "opacity",
    "filter",
    "clip-path",
    "mask",
    "mix-blend-mode",
    "isolation",
    "display",
    "transform",
    "enable-background",
];

/// The single element child of `g` when `g` is a pure wrapper: only an `id` attribute (layers,
/// labelled, transformed, styled and `xml:space` groups drop out), one element child of a rendered
/// kind, other children whitespace only (a comment disqualifies), every ancestor a plain container,
/// and no reference to `g` (a `<use>`, an `#id` selector, a connector).
fn wrapper_child(doc: &Doc, g: NodeId, refs: &HashSet<String>) -> Option<NodeId> {
    if !doc.attrs(g).iter().all(|a| a.name == "id") {
        return None;
    }
    if doc.attr(g, "id").is_some_and(|id| refs.contains(id)) {
        return None;
    }
    let root = doc.root();
    if !doc
        .ancestors(g)
        .take_while(|&a| a != root)
        .all(|a| COLLAPSE_ANCESTORS.contains(&doc.tag(a)))
    {
        return None;
    }
    let mut child = None;
    for k in doc.children(g) {
        if doc.is_element(k) {
            if child.is_some() {
                return None;
            }
            child = Some(k);
        } else if doc.is_comment(k) || doc.text(k).is_some_and(|t| !t.trim().is_empty()) {
            return None;
        }
    }
    let c = child?;
    COLLAPSE_CHILD.contains(&doc.tag(c)).then_some(c)
}

/// Step (c): replaces every pure wrapper `<g>` by its only child. Exact when no stylesheet rule can
/// tell the wrapper from its child: a wrapper with only an `id` has no properties of its own, so
/// nothing inherits from it, and `doc.replace` keeps the child's position. The step is skipped as a
/// whole when the stylesheet has a rule that is not a lone `*`, declares a group-level property
/// (`GROUP_EFFECT_PROPS`), or contains an `@` rule (our parser skips `@media` blocks that Inkscape
/// may apply). The wrapper's id moves to a child that has none, so matplotlib's `patch_1`-style
/// names survive on the object. `ops::clip::ungroup` is not reused: it pushes the cascaded style
/// down onto the child. Returns `Err(reason)` when skipped.
pub fn collapse_wrappers(doc: &mut Doc, refs: &HashSet<String>) -> Result<usize, String> {
    let wrappers: Vec<NodeId> = doc
        .descendants(doc.svg())
        .skip(1)
        .filter(|&n| doc.is_element(n) && doc.tag(n) == "g" && wrapper_child(doc, n, refs).is_some())
        .collect();
    if wrappers.is_empty() {
        return Ok(0); // nothing to collapse, so no note about the stylesheet either
    }
    let sheet = doc.stylesheet();
    if sheet.rule_count() > 0 {
        let at_rule = style_elements(doc)
            .into_iter()
            .any(|s| doc.text_content(s).contains('@'));
        if at_rule || !sheet.only_universal_rules() || sheet.declares_any(GROUP_EFFECT_PROPS) {
            return Err(format!(
                "wrapper groups kept: the stylesheet has {} rule(s) that can depend on grouping",
                sheet.rule_count()
            ));
        }
    }
    let mut collapsed = 0;
    for g in wrappers {
        // re-validated: collapsing an outer wrapper changed this one's ancestors
        let Some(c) = wrapper_child(doc, g, refs) else {
            continue;
        };
        if doc.attr(c, "id").is_none() {
            if let Some(id) = doc.remove_attr(g, "id") {
                doc.set_attr(c, "id", id);
            }
        }
        doc.replace(g, c);
        collapsed += 1;
    }
    Ok(collapsed)
}
```

In `slim`, after the `removeempty` block:

```rust
    if o.collapsegroups {
        let refs = referenced_ids(doc);
        match collapse_wrappers(doc, &refs) {
            Ok(n) => {
                r.wrappers_collapsed = n;
                t.phase("wrappers", || format!("collapsed={n}"));
            }
            Err(note) => {
                r.notes.push(note);
                t.phase("wrappers", || "skipped=stylesheet".to_string());
            }
        }
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test slimmer`
Expected: PASS. Pre-order candidates collapse nested wrappers in one pass because each candidate is
re-validated when reached (`outer` hands `inner` up, then `inner` hands the rect up).

- [ ] **Step 5: Gate and commit**

```bash
/usr/bin/git add src/tools/slimmer.rs tests/slimmer.rs
/usr/bin/git commit -q -m "feat(slimmer): collapse single-child wrapper groups" -m "Step (c): a <g> carrying only an id, holding one rendered element and sitting under plain containers is replaced by that element, which inherits the id. Exact because such a group has no properties of its own; the whole step is skipped when the stylesheet has anything but lone * rules without group-level properties, so no rule can tell wrapper and child apart." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Step (d) — unused definitions to a fixpoint

**Files:**
- Modify: `src/tools/slimmer.rs`
- Test: `tests/slimmer.rs`

**Interfaces:**
- Produces: `pub fn prune_unused(doc: &mut Doc) -> (usize, usize, usize)` = (definitions removed,
  rounds run, emptied containers removed); constants `PRUNE_TAGS`, `DEFS_KEEP`; private
  `prune_empty_groups(doc, refs) -> usize`.
- Consumes: `referenced_ids`, `detach_tidy`, `removable_context`, `empty_group` (T3).

- [ ] **Step 1: Write the failing tests** (append to `tests/slimmer.rs`)

```rust
#[test]
fn unused_definitions_are_pruned_to_a_fixpoint_including_nested_defs_and_gradient_chains() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><defs id="root"><clipPath id="used"><rect width="1" height="1"/></clipPath><clipPath id="dead"><rect width="1" height="1"/></clipPath><linearGradient id="base"/><linearGradient id="chain" xlink:href="#base"/><linearGradient id="live" xlink:href="#base"/><linearGradient id="g1" xlink:href="#g2"/><linearGradient id="g2"/><path id="glyph" d="M0 0h1"/><rect id="loose" width="1" height="1"/></defs>
<g id="fig"><defs id="nested"><clipPath id="deadn"><rect width="1" height="1"/></clipPath></defs><rect id="r" clip-path="url(#used)" width="1" height="1" style="fill:url(#live)"/></g>
<mask id="strayfree"><rect width="1" height="1"/></mask>
</svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["root", "used", "base", "live", "r", "fig"] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    for id in ["dead", "chain", "g1", "g2", "glyph", "loose", "deadn", "nested", "strayfree"] {
        assert!(!has(&d, id), "{id} is unused and should be gone: {s}");
    }
    assert!(
        msgs[0].contains("unused definitions removed: 8 in 3 round(s) (1 emptied containers)"),
        "g2 is only freed once g1 is gone, so it takes a second round; the third finds nothing: {msgs:?}"
    );
    assert!(every_reference_resolves(&d));
}

#[test]
fn referenced_definitions_style_glyph_script_children_text_paths_and_the_root_defs_survive_pruning() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><defs id="root"><style id="sheet">.x{{fill:red}}</style><script id="js">//</script><title id="tt">t</title><font id="fnt"><font-face id="ff"/><glyph id="gl"/></font><path id="curve" d="M0 0h1"/><marker id="mk"><path d="M0 0h1"/></marker><pattern id="pat"><rect width="1" height="1"/></pattern></defs>
<text><textPath xlink:href="#curve">on a curve</textPath></text>
<path d="M0 0h1" style="marker-end:url(#mk)"/><rect width="1" height="1" fill="url(#pat)"/>
<g id="emptyroot"><defs id="emptydefs"/></g></svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["root", "sheet", "js", "tt", "fnt", "ff", "gl", "curve", "mk", "pat"] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert!(
        !has(&d, "emptydefs") && !has(&d, "emptyroot"),
        "an empty nested <defs> goes, then the group it emptied: {s}"
    );
    assert!(
        msgs[0].contains("unused definitions removed: 0 in 2 round(s) (2 emptied containers)"),
        "{msgs:?}"
    );
    assert!(has(&d, "root"), "the root <defs> is never removed even when empty");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test slimmer`
Expected: `unused_definitions_…` and `referenced_definitions_…` both fail (nothing pruned; `emptydefs` still there).

- [ ] **Step 3: Implement step (d)**

```rust
/// Definition kinds that render nothing on their own (upstream `clean_up_document`'s list minus
/// `textPath`, `animate*`, `font`, `font-face`, which are content or name-referenced, not
/// id-referenced — upstream deletes rendered content there).
const PRUNE_TAGS: &[&str] = &[
    "clipPath",
    "mask",
    "linearGradient",
    "radialGradient",
    "pattern",
    "symbol",
    "marker",
    "filter",
];
/// Direct `<defs>` children that are used without an id reference.
const DEFS_KEEP: &[&str] = &[
    "style", "glyph", "script", "metadata", "title", "desc", "font", "font-face",
];

/// Non-layer groups left without element or comment children, reverse order (children first).
fn prune_empty_groups(doc: &mut Doc, refs: &HashSet<String>) -> usize {
    let groups: Vec<NodeId> = doc
        .descendants(doc.svg())
        .skip(1)
        .filter(|&n| doc.is_element(n) && doc.tag(n) == "g")
        .collect();
    let mut removed = 0;
    for &g in groups.iter().rev() {
        if removable_context(doc, g, refs) && empty_group(doc, g) {
            detach_tidy(doc, g);
            removed += 1;
        }
    }
    removed
}

/// Step (d), after `dhelpers.py:990 clean_up_document`: a definition (`PRUNE_TAGS` anywhere, or any
/// direct child of any `<defs>` except `DEFS_KEEP`) goes when no id in it — its own or a
/// descendant's — is referenced. Repeats until stable, because a definition can hold the only
/// reference to another (gradient `href` chains). Nested `<defs>` left empty go too (the root
/// `<defs>` stays; Inkscape expects one), as do groups emptied by that. Exact: nothing rendered
/// pointed at any of it. Returns (definitions removed, rounds run, emptied containers removed).
pub fn prune_unused(doc: &mut Doc) -> (usize, usize, usize) {
    let (mut pruned, mut rounds, mut containers) = (0usize, 0usize, 0usize);
    loop {
        rounds += 1;
        let refs = referenced_ids(doc);
        let svg = doc.svg();
        let candidates: Vec<NodeId> = doc
            .descendants(svg)
            .skip(1)
            .filter(|&n| {
                doc.is_element(n)
                    && (PRUNE_TAGS.contains(&doc.tag(n))
                        || (doc
                            .parent(n)
                            .is_some_and(|p| doc.is_element(p) && doc.tag(p) == "defs")
                            && !DEFS_KEEP.contains(&doc.tag(n))))
            })
            .collect();
        let mut removed_now = 0;
        // inner definitions before the containers holding them
        for &n in candidates.iter().rev() {
            let used = doc
                .descendants(n)
                .any(|d| doc.attr(d, "id").is_some_and(|id| refs.contains(id)));
            if !used {
                detach_tidy(doc, n);
                removed_now += 1;
            }
        }
        let empty_defs: Vec<NodeId> = doc
            .descendants(svg)
            .skip(1)
            .filter(|&n| {
                doc.is_element(n)
                    && doc.tag(n) == "defs"
                    && doc.parent(n) != Some(svg)
                    && !doc
                        .children(n)
                        .any(|c| doc.is_element(c) || doc.is_comment(c))
            })
            .collect();
        for &e in &empty_defs {
            detach_tidy(doc, e);
        }
        let emptied = empty_defs.len() + prune_empty_groups(doc, &refs);
        pruned += removed_now;
        containers += emptied;
        if removed_now + emptied == 0 {
            break;
        }
    }
    (pruned, rounds, containers)
}
```

In `slim`, after the `collapsegroups` block:

```rust
    if o.pruneunused {
        let (n, rounds, containers) = prune_unused(doc);
        r.defs_pruned = n;
        r.prune_rounds = rounds;
        r.containers_removed = containers;
        t.phase("prune", || {
            format!("removed={n} rounds={rounds} containers={containers}")
        });
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test slimmer`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
/usr/bin/git add src/tools/slimmer.rs tests/slimmer.rs
/usr/bin/git commit -q -m "feat(slimmer): prune unused definitions to a fixpoint" -m "Step (d) follows upstream's clean_up_document: clipPaths, masks, gradients, patterns, symbols, markers, filters anywhere and every direct <defs> child except style/glyph/script/metadata/title/desc/font are removed when no id inside them is referenced, repeating until nothing changes (gradient href chains). Nested <defs> and groups left empty go too; the root <defs> stays." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Step (e) — merge identical definitions

**Files:**
- Modify: `src/tools/slimmer.rs`
- Test: `tests/slimmer.rs`

**Interfaces:**
- Produces: `pub fn merge_identical_defs(doc: &mut Doc) -> (usize, usize)` = (definitions merged, attributes
  repointed); private `canon`, `canonical_key`, `rewrite_urls`, `repoint`; constant `MERGE_TAGS`.
- Consumes: `referenced_ids`, `css_idents` (T1), `Doc::{cascaded_style, specified_style, attrs, text, set_attr}`.

- [ ] **Step 1: Write the failing tests** (append to `tests/slimmer.rs`)

```rust
#[test]
fn identical_clip_paths_merge_and_every_reference_form_is_repointed() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><defs><clipPath id="c1"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="c2"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="c3"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="c10"><rect x="1" y="0" width="10" height="10"/></clipPath><linearGradient id="g1"><stop offset="0" stop-color="red"/></linearGradient><linearGradient id="g2" xlink:href="#g1"/><linearGradient id="g3" xlink:href="#g1"/></defs>
<rect id="z" clip-path="url(#c1)" width="1" height="1"/><rect id="a" clip-path="url(#c2)" width="1" height="1"/><rect id="b" style="clip-path:url( '#c3' );fill:url(#g3)" width="1" height="1"/><rect id="k" clip-path="url(#c10)" width="1" height="1"/><rect id="f" fill="url(#g2)" width="1" height="1"/></svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["c1", "c10", "g1", "g2"] {
        assert!(has(&d, id), "{id} survives: {s}");
    }
    for id in ["c2", "c3", "g3"] {
        assert!(!has(&d, id), "{id} is a copy of an earlier definition: {s}");
    }
    assert_eq!(by_id(&d, "a").attribute("clip-path"), Some("url(#c1)"));
    let b = by_id(&d, "b").attribute("style").unwrap();
    assert!(
        b.contains("clip-path:url(#c1)") && b.contains("fill:url(#g2)"),
        "quoted and inline references are repointed: {b}"
    );
    assert_eq!(by_id(&d, "k").attribute("clip-path"), Some("url(#c10)"), "c10 is not c1");
    assert!(
        msgs[0].contains("identical definitions merged: 3 (2 attributes repointed)"),
        "{msgs:?}"
    );
    assert!(every_reference_resolves(&d), "{s}");
}

#[test]
fn definitions_with_referenced_inner_ids_style_mentions_or_duplicate_ids_are_not_merged() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><style>#c2 rect{{fill:red}}</style><defs><clipPath id="c1"><rect width="1" height="1"/></clipPath><clipPath id="c2"><rect width="1" height="1"/></clipPath><clipPath id="c3"><rect id="inner" width="1" height="1"/></clipPath><clipPath id="c4"><rect width="1" height="1"/></clipPath><clipPath id="c4"><rect width="1" height="1"/></clipPath></defs>
<rect clip-path="url(#c1)" width="1" height="1"/><rect clip-path="url(#c2)" width="1" height="1"/><rect clip-path="url(#c3)" width="1" height="1"/><rect clip-path="url(#c4)" width="1" height="1"/><use xlink:href="#inner"/></svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["c1", "c2", "c3", "c4"] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert_eq!(
        d.descendants().filter(|n| n.attribute("id") == Some("c4")).count(),
        2,
        "duplicate ids are left alone"
    );
    assert_eq!(
        msgs,
        vec!["Slimmer: nothing to do".to_string()],
        "nothing merged, nothing else to do (no wrapper groups, so no stylesheet note): {msgs:?}"
    );
}

#[test]
fn definitions_in_different_style_contexts_are_not_merged() {
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="a"><rect width="1" height="1"/></clipPath></defs><g style="clip-rule:evenodd"><defs><clipPath id="b"><rect width="1" height="1"/></clipPath></defs></g><defs><clipPath id="c"><rect width="1" height="1" style="clip-rule:evenodd"/></clipPath></defs>
<rect clip-path="url(#a)" width="1" height="1"/><rect clip-path="url(#b)" width="1" height="1"/><rect clip-path="url(#c)" width="1" height="1"/></svg>"#
    );
    let (s, _) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["a", "b", "c"] {
        assert!(has(&d, id), "{id}: inherited or inline clip-rule makes it a different clip: {s}");
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test slimmer`
Expected: `identical_clip_paths_…` fails (nothing merged); the two `definitions_…` tests pass already (guards).

- [ ] **Step 3: Implement step (e)**

Add `css_idents` to the `ops::cleanup` import, then:

```rust
/// Definition kinds whose content alone determines their effect (their coordinates are interpreted
/// in the referencing element's space, so where they sit in the tree is irrelevant).
const MERGE_TAGS: &[&str] = &[
    "clipPath",
    "mask",
    "linearGradient",
    "radialGradient",
    "pattern",
    "marker",
    "filter",
    "symbol",
];

/// Canonical text of a definition: tag, attributes except `id` sorted by name, the element's cascaded
/// style (presentation attributes, inline style and every stylesheet rule that matches it), then the
/// children — elements recursively, non-blank text, comments skipped.
fn canon(doc: &Doc, n: NodeId, out: &mut String) {
    out.push('<');
    out.push_str(doc.tag(n));
    let mut attrs: Vec<(&str, &str)> = doc
        .attrs(n)
        .iter()
        .filter(|a| a.name != "id")
        .map(|a| (a.name.as_str(), a.value.as_str()))
        .collect();
    attrs.sort_unstable();
    for (k, v) in attrs {
        out.push(' ');
        out.push_str(k);
        out.push('=');
        out.push_str(v);
        out.push('\u{2}');
    }
    out.push('|');
    out.push_str(&doc.cascaded_style(n).to_css());
    out.push('>');
    for c in doc.children(n) {
        if doc.is_element(c) {
            canon(doc, c, out);
        } else if let Some(t) = doc.text(c) {
            let t = t.trim();
            if !t.is_empty() {
                out.push('"');
                out.push_str(t);
                out.push('"');
            }
        }
    }
    out.push_str("</>");
}

/// What two definitions must share to be interchangeable: the parent's specified style (what the
/// definition inherits — `clip-rule`, `stop-color`) and the canonical text.
fn canonical_key(doc: &Doc, n: NodeId) -> String {
    let mut s = String::new();
    if let Some(p) = doc.parent(n) {
        if doc.is_element(p) {
            s.push_str(&doc.specified_style(p).to_css());
        }
    }
    s.push('\u{1}');
    canon(doc, n, &mut s);
    s
}

/// `url(#dup)` → `url(#surv)` inside `v`, tolerating whitespace and quotes; `None` when nothing
/// changed. The closing `)` bounds the id, so `clip1` never touches `clip10`.
fn rewrite_urls(v: &str, rename: &HashMap<String, String>) -> Option<String> {
    let mut out = String::with_capacity(v.len());
    let mut changed = false;
    let mut rest = v;
    while let Some(i) = rest.find("url(") {
        out.push_str(&rest[..i + 4]);
        rest = &rest[i + 4..];
        let Some(end) = rest.find(')') else { break };
        let inner = &rest[..end];
        let core = inner.trim().trim_matches(['\'', '"']);
        match core.strip_prefix('#').and_then(|id| rename.get(id)) {
            Some(new) => {
                out.push('#');
                out.push_str(new);
                changed = true;
            }
            None => out.push_str(inner),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    changed.then_some(out)
}

/// Rewrites every reference to a merged definition: `href`/`xlink:href` and any other attribute whose
/// whole value is `#dup`, and every `url(#dup)` in any attribute — inline `style` included, without a
/// parse round trip. Returns the number of attributes rewritten.
fn repoint(doc: &mut Doc, rename: &HashMap<String, String>) -> usize {
    let mut count = 0;
    let nodes: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|&n| doc.is_element(n))
        .collect();
    for n in nodes {
        let attrs: Vec<(String, String)> = doc
            .attrs(n)
            .iter()
            .filter(|a| !matches!(a.name.as_str(), "id" | "d" | "points"))
            .map(|a| (a.name.clone(), a.value.clone()))
            .collect();
        for (name, value) in attrs {
            let new = if value.contains("url(") {
                rewrite_urls(&value, rename)
            } else {
                value
                    .trim()
                    .strip_prefix('#')
                    .and_then(|id| rename.get(id))
                    .map(|s| format!("#{s}"))
            };
            if let Some(v) = new {
                doc.set_attr(n, &name, v);
                count += 1;
            }
        }
    }
    count
}

/// Step (e): definitions with the same canonical key are interchangeable; every copy after the
/// first in document order is removed and its references repointed to the first. Exact because the
/// key covers everything that can make two definitions render differently: every attribute but
/// `id`, each element's cascaded style (an `#id` rule or a combinator matching one copy only yields
/// a different key), the parent's specified style (inheritance into the definition) and the content.
/// Refused for a definition whose descendant ids are referenced (they would vanish), whose ids
/// appear in `<style>` text (CSS is not rewritten), or whose id is missing or not unique (repointing
/// to a duplicated id would resolve to the first-wins index entry). Repeats until stable: two
/// gradients that differ only by `href` to two merged copies become identical in the next round.
/// Returns (definitions merged, attributes repointed).
pub fn merge_identical_defs(doc: &mut Doc) -> (usize, usize) {
    let (mut merged, mut repointed) = (0usize, 0usize);
    loop {
        let refs = referenced_ids(doc);
        let mut sheet_ids = HashSet::new();
        for s in style_elements(doc) {
            css_idents(&doc.text_content(s), &mut sheet_ids);
        }
        let mut id_count: HashMap<String, usize> = HashMap::new();
        for n in doc.descendants(doc.svg()) {
            if let Some(id) = doc.attr(n, "id") {
                *id_count.entry(id.to_string()).or_default() += 1;
            }
        }
        let candidates: Vec<NodeId> = doc
            .descendants(doc.svg())
            .skip(1)
            .filter(|&n| doc.is_element(n) && MERGE_TAGS.contains(&doc.tag(n)))
            .collect();
        let mut survivor: HashMap<String, String> = HashMap::new(); // key → surviving id
        let mut rename: HashMap<String, String> = HashMap::new(); // duplicate id → surviving id
        let mut dups: Vec<NodeId> = Vec::new();
        for n in candidates {
            let Some(id) = doc.attr(n, "id").map(str::to_string) else {
                continue;
            };
            if id_count.get(&id) != Some(&1) || sheet_ids.contains(&id) {
                continue;
            }
            let inner_pinned = doc
                .descendants(n)
                .skip(1)
                .filter_map(|d| doc.attr(d, "id"))
                .any(|i| refs.contains(i) || sheet_ids.contains(i));
            if inner_pinned {
                continue;
            }
            let key = canonical_key(doc, n);
            match survivor.get(&key) {
                Some(first) => {
                    rename.insert(id, first.clone());
                    dups.push(n);
                }
                None => {
                    survivor.insert(key, id);
                }
            }
        }
        if dups.is_empty() {
            break;
        }
        for &d in &dups {
            detach_tidy(doc, d);
        }
        merged += dups.len();
        repointed += repoint(doc, &rename);
    }
    (merged, repointed)
}
```

In `slim`, after the `pruneunused` block:

```rust
    if o.mergedefs {
        let (n, m) = merge_identical_defs(doc);
        r.defs_merged = n;
        r.attrs_repointed = m;
        t.phase("merge", || format!("merged={n} repointed={m}"));
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test slimmer`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
/usr/bin/git add src/tools/slimmer.rs tests/slimmer.rs
/usr/bin/git commit -q -m "feat(slimmer): merge identical definitions and repoint their references" -m "Step (e): clipPaths, masks, gradients, patterns, markers, filters and symbols with the same canonical content (attributes but id, cascaded style of every element, the parent's inherited style, children) are kept once; url(#id), href and #id references move to the survivor. Refused when inner ids are referenced, when ids appear in stylesheet text, or when an id is not unique." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Step (f) — coordinate precision (opt-in)

**Files:**
- Modify: `src/tools/slimmer.rs`
- Test: `tests/slimmer.rs`

**Interfaces:**
- Produces: `pub fn round_numbers(text: &str, sig: u8, path_grammar: bool) -> Option<(String, usize)>`,
  `pub fn round_coordinates(doc: &mut Doc, sig: u8) -> usize`; private `round_token`; constant
  `SHAPE_NUMERIC_ATTRS`.

- [ ] **Step 1: Write the failing tests** (append to `tests/slimmer.rs`)

```rust
#[test]
fn precision_rounds_significant_digits_and_keeps_integers_flags_exponents_and_leading_dots() {
    use sciink::tools::slimmer::round_numbers;
    let (s, k) = round_numbers(
        "M .5 1e-5 -0.1234567 123456.789 L1-2 a1 1 0 01.5.5 z",
        6,
        true,
    )
    .unwrap();
    assert_eq!(s, "M .5 1e-5 -0.123457 123457 L1-2 a1 1 0 01.5.5 z");
    assert_eq!(k, 2, "only the two long numbers changed; .5 and 1e-5 would not get shorter");
    assert_eq!(
        round_numbers("10.000001 20", 4, false).unwrap(),
        ("10 20".to_string(), 1)
    );
    assert_eq!(round_numbers("1em", 6, false), None, "units: leave the value alone");
    assert_eq!(round_numbers("50%", 6, false), None);
    assert_eq!(round_numbers("M0 0", 6, true).unwrap(), ("M0 0".to_string(), 0));
    assert_eq!(round_numbers("-0.0", 6, false).unwrap(), ("0".to_string(), 1));
    assert_eq!(
        round_numbers("a 5 5 0 1 0 10 0", 4, true).unwrap(),
        ("a 5 5 0 1 0 10 0".to_string(), 0),
        "spaced arc flags"
    );
}

#[test]
fn precision_is_off_by_default_and_never_touches_transform_viewbox_style_or_text_positions() {
    let svg = format!(
        r#"<svg {NS} viewBox="0 0 10.123456789 10"><g transform="translate(0.123456789)"><path id="p" d="M0.123456789 0h1" style="stroke-width:0.123456789"/><rect id="r" x="0.123456789" width="1" height="1"/><text id="t" x="0.123456789" y="1">a</text></g></svg>"#
    );
    let (s0, msgs0) = slim(&svg, &[]);
    assert_eq!(s0, svg, "the default keeps every digit: {msgs0:?}");
    let (s, msgs) = slim(&svg, &["--precision=6"]);
    assert!(
        s.contains(r#"d="M0.123457 0h1""#) && s.contains(r#"x="0.123457" width"#),
        "{s}"
    );
    for kept in [
        r#"viewBox="0 0 10.123456789 10""#,
        "translate(0.123456789)",
        "stroke-width:0.123456789",
        r#"<text id="t" x="0.123456789""#,
    ] {
        assert!(s.contains(kept), "{kept} is untouched: {s}");
    }
    assert!(
        msgs[0].contains("coordinate precision: 6 significant digits, 2 numbers changed"),
        "{msgs:?}"
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test slimmer precision`
Expected: compile error (`round_numbers` missing).

- [ ] **Step 3: Implement step (f)**

```rust
/// Integers stay verbatim (already short, and arc flags are integers); a float is re-emitted with
/// `sig` significant digits in the shortest round-trip form, and only when that is shorter.
fn round_token(tok: &str, sig: u8, is_float: bool, changed: &mut usize) -> String {
    if !is_float {
        return tok.to_string();
    }
    let Ok(v) = tok.parse::<f64>() else {
        return tok.to_string();
    };
    if !v.is_finite() {
        return tok.to_string();
    }
    let r: f64 = if v == 0.0 {
        0.0
    } else {
        format!("{:.*e}", usize::from(sig) - 1, v)
            .parse()
            .unwrap_or(v)
    };
    let s = if r == 0.0 { "0".to_string() } else { format!("{r}") };
    if s.len() < tok.len() {
        *changed += 1;
        s
    } else {
        tok.to_string()
    }
}

/// Rounds every non-integer number of an SVG number list or of path data to `sig` significant
/// digits and copies everything else verbatim: separators, path command letters, integers, and any
/// number whose rounded form would not be shorter. In path mode the two arc flags of every `A`/`a`
/// 7-tuple are read as single characters (`01.5` is flag 0, flag 1, number .5). `None` when the
/// text is not a plain number list (units, `%`, anything unexpected): the caller leaves the
/// attribute alone. Returns the new text and how many numbers changed.
pub fn round_numbers(text: &str, sig: u8, path_grammar: bool) -> Option<(String, usize)> {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut changed = 0;
    let (mut i, mut cmd, mut argi) = (0usize, 0u8, 0usize);
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() {
            if !path_grammar {
                return None;
            }
            cmd = c;
            argi = 0;
            out.push(c as char);
            i += 1;
        } else if c.is_ascii_digit() || matches!(c, b'.' | b'-' | b'+') {
            if path_grammar && matches!(cmd, b'a' | b'A') && matches!(argi % 7, 3 | 4) {
                if !matches!(c, b'0' | b'1') {
                    return None;
                }
                out.push(c as char);
                i += 1;
                argi += 1;
                continue;
            }
            let start = i;
            if matches!(b[i], b'-' | b'+') {
                i += 1;
            }
            let (mut digits, mut dot, mut exp) = (0usize, false, false);
            while i < b.len() {
                match b[i] {
                    d if d.is_ascii_digit() => {
                        digits += 1;
                        i += 1;
                    }
                    b'.' if !dot && !exp => {
                        dot = true;
                        i += 1;
                    }
                    b'e' | b'E' if !exp && digits > 0 => {
                        let j = i + 1;
                        let k = if j < b.len() && matches!(b[j], b'-' | b'+') {
                            j + 1
                        } else {
                            j
                        };
                        if k < b.len() && b[k].is_ascii_digit() {
                            exp = true;
                            i = k;
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            if digits == 0 {
                return None;
            }
            out.push_str(&round_token(&text[start..i], sig, dot || exp, &mut changed));
            argi += 1;
        } else if c == b',' || c.is_ascii_whitespace() {
            out.push(c as char);
            i += 1;
        } else {
            return None;
        }
    }
    Some((out, changed))
}

/// Geometry attributes of shapes; `transform`, `viewBox`, styles and text positions are never
/// touched (scale factors and units would amplify the error; kerning lists are semantic input).
const SHAPE_NUMERIC_ATTRS: &[&str] = &[
    "x", "y", "width", "height", "rx", "ry", "cx", "cy", "r", "x1", "y1", "x2", "y2",
];

/// Step (f), opt-in: rounds `d`, `points` and the numeric geometry attributes of shapes to `sig`
/// significant digits. Not rendering-exact — relative error ≤ 5·10⁻ˢⁱᵍ per number, accumulating
/// along relative commands — hence off by default. Returns the number of numbers changed.
pub fn round_coordinates(doc: &mut Doc, sig: u8) -> usize {
    let nodes: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|&n| doc.is_element(n) && SHAPES.contains(&doc.tag(n)))
        .collect();
    let mut total = 0;
    for n in nodes {
        let mut todo: Vec<(&str, bool)> = Vec::new();
        match doc.tag(n) {
            "path" => todo.push(("d", true)),
            "polyline" | "polygon" => todo.push(("points", false)),
            _ => {}
        }
        todo.extend(SHAPE_NUMERIC_ATTRS.iter().map(|a| (*a, false)));
        for (name, path_grammar) in todo {
            let Some(v) = doc.attr(n, name) else {
                continue;
            };
            if let Some((new, k)) = round_numbers(v, sig, path_grammar) {
                if k > 0 {
                    doc.set_attr(n, name, new);
                    total += k;
                }
            }
        }
    }
    total
}
```

In `slim`, after the `mergedefs` block:

```rust
    if o.precision > 0 {
        let n = round_coordinates(doc, o.precision);
        r.numbers_rounded = n;
        t.phase("precision", || format!("sig={} changed={n}", o.precision));
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test slimmer`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
/usr/bin/git add src/tools/slimmer.rs tests/slimmer.rs
/usr/bin/git commit -q -m "feat(slimmer): opt-in coordinate precision in significant digits" -m "Step (f), off by default: a lexical pass over d, points and the shape geometry attributes rounds non-integer numbers to N significant digits when that makes them shorter, keeps integers and arc flags, and leaves transform, viewBox, styles and text positions alone." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Exactness — BigDoc, upstream fixtures, bench

**Files:**
- Test: `tests/slimmer.rs`, `tests/bench.rs`

**Interfaces:**
- Consumes: `support::{BigDoc, render_png, pixel_diff_fraction, upstream_svgs, upstream_data_dir}`.

- [ ] **Step 1: Write the tests** (append to `tests/slimmer.rs`)

```rust
#[test]
fn big_doc_loses_style_rules_minus_one_sheets_merges_its_identical_clips_and_renders_identically() {
    let big = support::BigDoc::default();
    let svg = big.svg();
    let (s, msgs) = slim(&svg, &[]);
    assert!(
        msgs[0].contains(&format!("duplicate stylesheets removed: {}", big.style_rules - 1)),
        "{msgs:?}"
    );
    assert!(
        msgs[0].contains(&format!("identical definitions merged: {}", big.figures - 1)),
        "every figure's clip is the same rectangle: {msgs:?}"
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(d.descendants().filter(|n| n.has_tag_name("style")).count(), 1);
    assert_eq!(d.descendants().filter(|n| n.has_tag_name("clipPath")).count(), 1);
    assert!(every_reference_resolves(&d));
    assert!(s.len() < svg.len(), "{} → {}", svg.len(), s.len());
    let diff = support::pixel_diff_fraction(
        &support::render_png(svg.as_bytes(), 1500),
        &support::render_png(s.as_bytes(), 1500),
        32,
    );
    assert_eq!(diff, 0.0, "rendering-exact by construction");
}

/// Renders `f` before and after a Slimmer run with `extra` options; the pixel-diff fraction must
/// not exceed `max`, and every reference in the output must resolve.
fn check_exact(f: &std::path::Path, extra: &[&str], max: f64) {
    let input = std::fs::read(f).unwrap();
    let mut a = vec!["--tool=slimmer", "--tab=Options"];
    a.extend(extra);
    let out = sciink::run(&args(&a), &input).unwrap().svg;
    let d = support::pixel_diff_fraction(
        &support::render_png(&input, 1500),
        &support::render_png(&out, 1500),
        32,
    );
    eprintln!("{}: pixel diff {:.5} % with {extra:?}", f.display(), d * 100.0);
    assert!(d <= max, "{}: {d} > {max}", f.display());
    let doc = roxmltree::Document::parse(std::str::from_utf8(&out).unwrap()).unwrap();
    assert!(every_reference_resolves(&doc), "{}", f.display());
}

#[test]
fn default_slimmer_renders_every_upstream_fixture_identically() {
    for f in support::upstream_svgs() {
        if f.file_name().is_some_and(|n| n == "Acid_tests.svg") {
            continue; // the 5.9 MB one is opt-in below
        }
        check_exact(&f, &[], 0.0);
    }
}

/// Run: `cargo test --release --test slimmer -- --ignored --nocapture`
#[test]
#[ignore = "5.9 MB fixture; slow in a debug build"]
fn default_slimmer_renders_acid_tests_identically() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    check_exact(&dir.join("svg/Acid_tests.svg"), &[], 0.0);
}

#[test]
fn precision_six_changes_at_most_a_tenth_of_a_percent_of_pixels_on_upstream_fixtures() {
    for f in support::upstream_svgs() {
        if f.file_name().is_some_and(|n| n == "Acid_tests.svg") {
            continue;
        }
        check_exact(&f, &["--precision=6"], 0.001);
    }
}
```

In `tests/bench.rs`, add a fourth argument set to the `for args in [...]` list inside `phase_timings`:

```rust
            vec!["--tool=slimmer".to_string(), "--tab=Options".to_string()],
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --test slimmer` then `cargo test --release --test slimmer -- --ignored --nocapture`
Expected: PASS with `pixel diff 0.00000 %` printed for every fixture with the default options. A non-zero
default diff is a real exactness defect in a step: find which step by re-running with that step's option
off, fix the step, never the assertion. Also `cargo test --release --test bench -- --ignored --nocapture`
prints a `tool=slimmer` phase block for the synthetic document.

- [ ] **Step 3: Gate and commit**

```bash
/usr/bin/git add tests/slimmer.rs tests/bench.rs
/usr/bin/git commit -q -m "test(slimmer): render-identity on BigDoc and every upstream fixture, bench entry" -m "The default options must produce a pixel-identical resvg raster on BigDoc and on all upstream fixtures (Acid_tests opt-in); precision 6 may move at most 0.1 % of the pixels; every reference in the output resolves. The bench prints Slimmer phase timings." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Documentation

**Files:**
- Modify: `README.md` (Tools table, "Large documents"), `docs/spec/02-geometry-tools.md` (§B.3, new
  "Deliberate deviations (Plan 10)"), `docs/spec/00-overview.md:147-148` (tool names),
  `docs/DEVELOPING.md` (tool list :68-69, "Performance on large documents" :104-113), `CHANGELOG.md`.
  No version change: Slimmer ships in 0.2.0 (user decision 2026-09-24), which is still untagged.

- [ ] **Step 1: README**

Add to the Tools table after the Favorite Markers row:

```markdown
| **Slimmer** | Makes the whole document smaller and faster in Inkscape: merges the duplicate stylesheets every matplotlib import adds, removes unused and merges identical definitions, drops empty and invisible elements and single-child wrapper groups. Rendering-exact by default; optional coordinate rounding. |
```

Replace the "Large documents" paragraph with:

```markdown
Inkscape writes the whole document to a temporary file, runs the extension, reads the result back and
re-renders it, so most of the wait on a large file is that round trip, not the tool itself. What makes
the round trip slow is the number of elements and of stylesheet rules, not embedded images: every
imported matplotlib figure brings its own `<style>` rule, and Inkscape matches every rule against every
element on each load and save. Run **Slimmer** once on a document assembled from many imports (it
halved the round trip of a 52 MB manuscript), and run the other tools on one figure's selection rather
than on a whole layer.
```

- [ ] **Step 2: Spec**

In `docs/spec/02-geometry-tools.md`, after the "### Favorite markers" section (before `## B.4`):

```markdown
### Slimmer (sciink-only; the unused-definition step after `dhelpers.py:990 clean_up_document`)
Whole document, selection ignored. Options `dedupstyles`, `removeempty`, `collapsegroups`,
`pruneunused`, `mergedefs` (all true), `precision` (0 = keep; 4–8 significant digits), `report` (true).
Order: styles → empty → wrappers → prune → merge → precision. Every default step is rendering-exact:
- **styles**: `<style>` elements with identical text (attributes ⊆ {id, type=text/css}, no `@`) keep the
  LAST copy — same-precedence conflicts are decided by source order, so only the last copy's position
  matters; a lone survivor moves to the front of the root.
- **empty**: drawn elements (no `UNRENDERED` ancestor, not referenced, no `inkscape:label`, not a `<switch>`
  child, not `display:none`, no filter): shapes with blank `d` or only movetos, `points` without digits,
  non-positive/missing `width`/`height`/`r`/`rx`/`ry`, `fill:none` with `stroke:none` or zero width (no
  markers); `<text>` without characters (not `xml:space="preserve"`); non-layer `<g>` without element or
  comment children. `line` is never empty.
- **wrappers**: a `<g>` with only an `id`, one rendered child (`COLLAPSE_CHILD`), whitespace otherwise,
  ancestors in {svg, g, a}, unreferenced → replaced by the child, which inherits the id. Skipped as a whole
  when the stylesheet has a rule that is not a lone `*`, declares opacity/filter/clip-path/mask/
  mix-blend-mode/isolation/display/transform/enable-background, or contains `@`.
- **prune**: clipPath, mask, gradients, pattern, symbol, marker, filter anywhere and every direct child of
  any `<defs>` except style/glyph/script/metadata/title/desc/font/font-face, removed when no id inside is
  referenced (`referenced_ids`: every `url(#…)`, `href`, `#id`-valued attribute and every `#ident` in
  `<style>` text); to a fixpoint; emptied nested `<defs>` and groups go; the root `<defs>` stays.
- **merge**: clipPath, mask, gradients, pattern, marker, filter, symbol with the same canonical key
  (parent's specified style; tag, attributes but id, cascaded style, children) keep the first; references
  repointed (`url(…)` in any attribute, `href`, whole-value `#id`); refused for referenced inner ids, ids in
  `<style>` text, missing or duplicated ids; to a fixpoint.
- **precision** (opt-in, lossy): `d`, `points`, shape `x y width height rx ry cx cy r x1 y1 x2 y2` rounded to
  N significant digits when shorter; integers, arc flags, `transform`, `viewBox`, styles, text positions
  untouched.
Report: one line of totals (bytes, elements), one per step, notes; `Slimmer: nothing to do` otherwise.
```

Append at the end of the file:

```markdown
## Deliberate deviations (Plan 10)

- Slimmer has no upstream counterpart: duplicate-stylesheet removal, empty/invisible-element removal,
  wrapper-group collapse, identical-definition merging and coordinate precision are sciink-only.
- Unused-definition pruning follows `dhelpers.clean_up_document` in intent but covers nested `<defs>` and
  removes emptied nested `<defs>` (upstream: the root defs only); it does not prune `textPath`,
  `animate*`, `font`, `font-face` (rendered or name-referenced content, which upstream deletes) and keeps
  `script`, `metadata`, `title`, `desc`; the reference scan covers every attribute, inline styles and
  `<style>` text (upstream: a fixed attribute list, hrefs and inline styles).
- Whole-document scope, selection ignored; no `delete_up` (emptied ancestors are handled by the
  empty-group predicate, which keeps layers); no fonts are loaded.
- After merging, a figure may share clip paths with other figures and is no longer self-contained
  (Inkscape copies referenced definitions on copy and paste).
```

- [ ] **Step 3: Overview, DEVELOPING, CHANGELOG**

`docs/spec/00-overview.md:147-148`: add `slimmer` to the tool-name list
(`… combine-by-color | favorite-markers | slimmer | about`).

`docs/DEVELOPING.md:68-69`: add `slimmer` after `favorite-markers`. Replace the "Performance on large
documents" section body with:

```markdown
Inkscape writes the whole document to a temporary file, runs the extension, reads the result back
and re-renders it. On a 50 MB SVG that round trip takes seconds before and after our binary runs,
and we do not control it; time Extensions ▸ Scientific ▸ Diagnostics on the file to see your own
floor (Diagnostics itself does under 0.1 s of work). Measured on a 52 MB, 62 000-element manuscript
with 177 imported matplotlib figures (headless, 2026-09-24): the full round trip was 26.9 s, of
which our binary was 0.08 s. Removing the 27 MB of embedded images changed nothing; keeping one of
the 177 identical `<style>*{…}</style>` sheets (one per import) cut Inkscape's parse to 4.9 s from
8.9 s and the round trip to 12.3 s — Inkscape's cost is per stylesheet rule × element. So: run
Slimmer once on such a document (stylesheets, unused and duplicate definitions, wrapper groups),
then run the tools per figure rather than on a whole layer. Our own share is logged per phase with
`SCIINK_LOG`; the Flattener takes ≈ 0.2 s on one figure and ≈ 1.0 s on the whole layer.
```

`CHANGELOG.md`: Slimmer ships in 0.2.0. Append this bullet at the end of the bullet list of the existing
`## 0.2.0 (unreleased)` section (do not add a new section, do not touch `Cargo.toml` or `tests/cli.rs`):

```markdown
- Slimmer: a new tool that makes a whole document smaller and faster for Inkscape. Rendering-exact by
  default: keeps one of identical `<style>` elements (matplotlib adds one per imported figure, and
  Inkscape's load and save cost is per rule × element), removes empty and invisible elements, collapses
  single-child wrapper groups, prunes unused definitions and merges identical ones with references
  repointed. Optional coordinate rounding in significant digits. A report dialog lists what changed.
```

Also extend the section's intro paragraph with one sentence after "documented in `docs/DEVELOPING.md`.":
"The new Slimmer tool removes what makes Inkscape slow on such documents." (T10 appends the measured
numbers to the bullet.)

- [ ] **Step 4: Gate and commit**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test` and
`dist/package.sh macos-universal target/release/sciink && dist/test-package.sh dist/out/sciink-macos-universal.zip`
after `cargo build --release` (the package must carry `slimmer.inx`; `test-package.sh` checks every inx's
command line).

```bash
/usr/bin/git add README.md docs/spec/02-geometry-tools.md docs/spec/00-overview.md docs/DEVELOPING.md CHANGELOG.md
/usr/bin/git commit -q -m "docs: Slimmer in README, spec, overview, DEVELOPING and the 0.2.0 changelog" -m "Documents the new tool and its deviations and replaces the 'link raster images' advice with the measured facts (Inkscape's round-trip cost is per stylesheet rule × element, images are irrelevant). Slimmer ships in 0.2.0, so no version change." -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: Measure on the manuscript and record

**Files:**
- Modify: `CHANGELOG.md`, this plan (Appendix B).

Dev-machine only (the manuscript is not in the repository). Paths: input
`/Users/yzheng/Documents/Manuscripts/MetPredict/metpredict_figures_clean.svg`; write outputs under the
session scratchpad or `$TMPDIR`, never next to the manuscript.

- [ ] **Step 1: Slimmer on the manuscript**

```bash
cargo build --release
SCIINK_LOG=/tmp/slim.log target/release/sciink --tool=slimmer --tab=Options /Users/yzheng/Documents/Manuscripts/MetPredict/metpredict_figures_clean.svg > /tmp/slim.svg
```

Record: the report (stderr), every `phase=` line with `dt=` from `/tmp/slim.log`, `phase=total`, and
`ls -l` sizes of input and output. Expected on this file: 176 sheets removed, ≥ 2 000 definitions pruned,
≥ 4 000 merged, 2 834 groups collapsed, `phase=total` ≤ 600 ms, output ≥ 4 % smaller.

- [ ] **Step 2: Rendering and Inkscape acceptance**

```bash
cargo test --release --test slimmer -- --ignored --nocapture
/Applications/Inkscape.app/Contents/MacOS/inkscape --export-type=svg --export-plain-svg --export-filename=/tmp/slim-roundtrip.svg /tmp/slim.svg
```

The export must succeed (Inkscape accepts the slimmed file). Then the round-trip floor before and after:

```bash
time /Applications/Inkscape.app/Contents/MacOS/inkscape --actions=org.sciink.about.noprefs /Users/yzheng/Documents/Manuscripts/MetPredict/metpredict_figures_clean.svg
time /Applications/Inkscape.app/Contents/MacOS/inkscape --actions=org.sciink.about.noprefs /tmp/slim.svg
```

(the dev-installed extension must be current: `dist/dev-install.sh` first). Expected: 26.9 s → ≤ 13 s.

- [ ] **Step 3: Record**

Append the measured numbers to the CHANGELOG 0.2.0 Slimmer bullet as one sentence ("On a 52 MB manuscript:
… sheets, … definitions pruned, … merged, … groups; … MB → … MB; Inkscape's round trip … s → … s.") and
write Appendix B of this plan (the table of counts, sizes, phase times and round trips). If a target is
missed, the phase log names the step; record the miss and the reason rather than tuning blindly.

```bash
/usr/bin/git add CHANGELOG.md docs/superpowers/plans/2026-09-24-plan10-slimmer.md
/usr/bin/git commit -q -m "docs: Slimmer numbers on the 52 MB manuscript" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Appendix A — Design summary and measurements (approved 2026-09-24)

**Why.** After Plan 9 our binary needs 0.1–0.3 s on the user's 52.6 MB manuscript
(62 357 elements, 177 imported matplotlib figures) but a Flattener run still takes > 15 s from Apply to
finish: Inkscape's extension round trip (write temp file, run us, re-parse, rebuild, re-render).

| Document variant | Inkscape parse + update (`--query-all`) | Round trip via Diagnostics (`--actions=org.sciink.about.noprefs`) |
|---|---|---|
| As is: 177 `<style>` sheets, 93 embedded images | 8.9 s | 26.9 s (our share 0.08 s) |
| 27.7 MB of embedded images stripped | 8.7 s | — |
| One sheet holding all 177 rule blocks | 8.7 s | — |
| One sheet with the single distinct rule | 4.9 s | 12.3 s |
| No sheets at all | 4.0 s | 9.3 s |
| All clipPaths and `clip-path` attributes stripped | 7.0 s | — |

Inkscape's cost is per **rule × element**; images are irrelevant to its speed; the rest is per-element.

| Content of the file | Count | Slimmer step |
|---|---|---|
| identical `<style>*{stroke-linejoin: round; stroke-linecap: butt}</style>` | 177 (159 in nested `<defs>`) | (a) → 1 |
| clipPath used / unused | 7 636 / ≈ 2 400–3 200 | (d) |
| used clipPaths with identical content | 7 636 → 3 422 distinct | (e) −4 214 |
| unused defs paths / rects | 133 / 5 | (d) |
| `<g>` with one child and only an `id` | 2 834 of 4 917 | (c) |
| empty / invisible / zero-size elements | 0 here | (b) |
| `<defs>` elements | 131 | emptied ones (d) |
| path `d` bytes | 15.3 MB of 52.6 MB | (f) opt-in |
| embedded images | 27.7 MB | out of scope |

**User decisions:** name Slimmer; whole document always; no image extraction; precision option included,
off by default (significant digits); every default step rendering-exact. Ships in **0.2.0** (decided
2026-09-24 while the plan ran: 0.2.0 was still untagged, so Slimmer is folded in; no version bump).

**Targets:** round trip 26.9 s → ≤ 13 s; ≥ 4 % smaller (≥ 15 % with precision 6); 62 357 → ≤ 47 000
elements; Slimmer ≤ 0.6 s; pixel diff exactly 0.0 with defaults, ≤ 0.001 with precision 6.

**Out of scope:** image extraction; removing or shortening ids; inlining stylesheets (would touch 62k
elements, +2.5 MB, for ~1 s more); Inkscape editor metadata; global whitespace stripping; zero-size
`<image>`; running Slimmer steps inside other tools; the Autoexporter.

**Execution rules:** subagent-driven development, one implementer per task in order; every `Agent` call
names its model explicitly and uses only `sonnet` (implementers), `haiku` (transcription, checklist and
scoped re-reviews) or `opus` (whole-branch review, design) — never Fable; review packages copied to plain
names; the controller checks commit trailers.

## Appendix B — Measured results (filled by T10)

_Not yet measured._
