//! Slimmer (Plan 10): makes a whole document smaller and faster for Inkscape. No upstream
//! counterpart except the unused-definition step, which follows `dhelpers.py:990
//! clean_up_document`. Whole-document scope; every step that is on by default is rendering-exact
//! — the argument is in each step's doc comment, the guard tests in `tests/slimmer.rs` pin what
//! must stay. No `Ctx`: nothing here creates clips, every deletion is of something unreferenced by
//! construction, and the merge step repoints its own references.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fmt::Write as _;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::geom::path::parse_d;
use crate::geom::{PathEl, ipx};
use crate::ops::bbox::{SHAPES, has_bbox};
use crate::ops::cleanup::{css_idents, detach_tidy, is_layer, referenced_ids};

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
    /// Also remove shapes with neither fill nor stroke (they still carry bounding boxes).
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub removeinvisible: bool,
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
            let first = doc.children(svg).find(|&c| doc.is_element(c));
            match first {
                Some(first) if first != only => doc.insert_before(only, first),
                _ => doc.append_child(svg, only),
            }
            moved = true;
        }
    }
    (removed, moved)
}

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
/// shapes, empty `<text>`, empty non-layer `<g>` — and, when `invisible`, shapes with neither fill
/// nor stroke — in reverse document order, so a group emptied by this pass is caught in the same
/// pass. Exact by the predicates above; the guards are in `removable_context` and `no_markers`.
pub fn remove_empty(doc: &mut Doc, refs: &HashSet<String>, invisible: bool) -> usize {
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
                no_markers(doc, n)
                    && (empty_shape(doc, n) || (invisible && invisible_shape(doc, n)))
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
        .filter(|&n| {
            doc.is_element(n) && doc.tag(n) == "g" && wrapper_child(doc, n, refs).is_some()
        })
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
    "style",
    "glyph",
    "script",
    "metadata",
    "title",
    "desc",
    "font",
    "font-face",
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
/// `<defs>` stays; Inkscape expects one); emptied non-layer `<g>` go too, but only when
/// `remove_groups` (`o.removeempty`). Exact: nothing rendered pointed at any of it. Returns
/// (definitions removed, rounds run, emptied containers removed).
pub fn prune_unused(doc: &mut Doc, remove_groups: bool) -> (usize, usize, usize) {
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
        let emptied = empty_defs.len()
            + if remove_groups {
                prune_empty_groups(doc, &refs)
            } else {
                0
            };
        pruned += removed_now;
        containers += emptied;
        if removed_now + emptied == 0 {
            break;
        }
    }
    (pruned, rounds, containers)
}

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
    let s = if r == 0.0 {
        "0".to_string()
    } else {
        format!("{r}")
    };
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

/// Runs the enabled steps in order, one `Timer` phase each.
pub fn slim(doc: &mut Doc, o: &SlimmerCli, t: &mut crate::log::Timer) -> Report {
    let mut r = Report::default();
    if o.dedupstyles {
        let (n, moved) = dedup_stylesheets(doc);
        r.sheets_removed = n;
        r.sheet_moved = moved;
        t.phase("styles", || format!("removed={n} moved={moved}"));
    }
    if o.removeempty {
        let refs = referenced_ids(doc);
        let n = remove_empty(doc, &refs, o.removeinvisible);
        r.empty_removed = n;
        t.phase("empty", || {
            format!("removed={n} invisible={}", o.removeinvisible)
        });
    }
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
    if o.pruneunused {
        let (n, rounds, containers) = prune_unused(doc, o.removeempty);
        r.defs_pruned = n;
        r.prune_rounds = rounds;
        r.containers_removed = containers;
        t.phase("prune", || {
            format!("removed={n} rounds={rounds} containers={containers}")
        });
    }
    if o.mergedefs {
        let (n, m) = merge_identical_defs(doc);
        r.defs_merged = n;
        r.attrs_repointed = m;
        t.phase("merge", || format!("merged={n} repointed={m}"));
    }
    if o.precision > 0 {
        let n = round_coordinates(doc, o.precision);
        r.numbers_rounded = n;
        t.phase("precision", || format!("sig={} changed={n}", o.precision));
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
        if r.sheet_moved {
            " (1 kept, moved to the document root)"
        } else {
            ""
        }
    );
    let _ = writeln!(
        s,
        "  empty or invisible elements removed: {}",
        r.empty_removed
    );
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
