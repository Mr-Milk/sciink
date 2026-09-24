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
/// counts both), stopping below the root `<svg>`, which is never deleted — a call on the root
/// is a no-op. Every removed id lands in `ctx.deleted`.
pub fn delete_up(doc: &mut Doc, ctx: &mut Ctx, n: NodeId) {
    if n == doc.svg() {
        return; // the root is never deleted, and its ids never count as deleted
    }
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
                    && !doc
                        .children(p)
                        .any(|c| doc.is_element(c) || doc.is_comment(c)) =>
            {
                target = p;
            }
            _ => break,
        }
    }
}

/// `haystack` contains `needle` ignoring ASCII case (the inline-style pre-filter must match
/// whatever `Style::parse` would lower-case).
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
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
            if !(contains_ignore_ascii_case(inline, "clip-path")
                || contains_ignore_ascii_case(inline, "mask"))
            {
                continue;
            }
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
            if contains_ignore_ascii_case(inline, "clip-path")
                || contains_ignore_ascii_case(inline, "mask")
            {
                let st = Style::parse(inline);
                for att in ["clip-path", "mask"] {
                    if let Some(id) = st.get(att).and_then(url_id) {
                        out.insert(id.to_string());
                    }
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
    if created.is_empty() {
        return;
    }
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

/// Elements whose tail text (the text node right after them) Inkscape needs (F:529).
const TAIL_KEEP: &[&str] = &["tspan", "textPath", "flowPara", "flowRegion", "flowSpan"];
/// Elements whose leading text (the text node before their first child) Inkscape needs (F:530–541).
const TEXT_KEEP: &[&str] = &[
    "style",
    "text",
    "tspan",
    "textPath",
    "flowRoot",
    "flowPara",
    "flowRegion",
    "flowSpan",
];

/// F:542–548 `strip_whitespace`: removes every text node that is not the leading text of a
/// text-bearing element or the tail of a text-run element — the indentation whitespace a deep
/// ungroup leaves behind — so the written document has no stray whitespace between elements. A
/// text node after a comment is treated like a leading text node (upstream never clears a
/// comment's tail, so a comment inside e.g. `<text>` must not eat the text that follows it).
pub fn strip_whitespace(doc: &mut Doc) {
    let texts: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|&n| doc.is_text(n))
        .collect();
    for t in texts {
        let keep = match doc.prev_sibling(t) {
            Some(prev) if doc.is_element(prev) => TAIL_KEEP.contains(&doc.tag(prev)),
            // after a comment, or first: the parent decides (upstream never clears a comment's tail)
            _ => doc
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
            .take_while(|c| {
                c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':') || !c.is_ascii()
            })
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
