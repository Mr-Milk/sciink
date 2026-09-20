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
    doc.attr(n, "id")
        .map(str::to_string)
        .unwrap_or_else(|| doc.tag(n).to_string())
}

/// Position in `runs` of the run `{ ddi, is_tail: true }`, indexed by `ddi`; `runs.len()` for a
/// `ddi` that has no tail run (only the root, `ddi == 0`, has none — see `TextTree::runs`).
fn tail_positions(tree: &TextTree, runs: &[Run]) -> Vec<usize> {
    let mut tail_pos = vec![runs.len(); tree.dds.len()];
    for (pos, r) in runs.iter().enumerate() {
        if r.is_tail {
            tail_pos[r.ddi] = pos;
        }
    }
    tail_pos
}

/// `texts_before[pos]` = how many runs among `runs[..pos]` have non-empty text; length is
/// `runs.len() + 1`. `texts_before[b] > texts_before[a]` then answers "does `runs[a..b]` contain
/// a non-empty-text run?" in O(1).
fn text_prefix_counts(doc: &Doc, runs: &[Run]) -> Vec<usize> {
    let mut texts_before = Vec::with_capacity(runs.len() + 1);
    texts_before.push(0usize);
    for r in runs {
        let has_text = run_text(doc, r).is_some_and(|t| !t.is_empty());
        let prev = *texts_before.last().expect("just pushed");
        texts_before.push(prev + has_text as usize);
    }
    texts_before
}

fn remove_position_overflows(doc: &mut Doc, tree: &TextTree, runs: &[Run], warn: &mut Warnings) {
    // `runs` walks `ddi` in pre-order, and a subtree's descendants occupy a contiguous `ddi`
    // range (standard pre-order property — see `TextTree::runs`), so the runs strictly inside
    // element `i`'s subtree, after its own text run, form the contiguous window
    // `runs[ri+1..tail_pos[i]]`. Precomputing `tail_pos` and a text-run prefix count turns the
    // old per-pair O(depth) ancestor walk (formerly `is_strict_descendant`, now removed) into
    // two O(total) passes plus an O(1) lookup per element.
    let tail_pos = tail_positions(tree, runs);
    let texts_before = text_prefix_counts(doc, runs);

    for (ri, r) in runs.iter().enumerate().filter(|(_, r)| !r.is_tail) {
        let n = r.node;
        if !doc.is_element(n) {
            continue;
        }
        let len = run_text(doc, r).map(|t| t.chars().count()).unwrap_or(0);

        // Read all four attributes first so the (still O(depth)-free, but non-trivial) `lossy`
        // check only ever runs for an element that actually has a surplus to report or drop.
        let mut attrs: Vec<(&str, bool, Vec<Option<f64>>)> = Vec::with_capacity(4);
        for attr in ["x", "y", "dx", "dy"] {
            attrs.push((attr, doc.attr(n, attr).is_some(), get_xy(doc, n, attr)));
        }
        let has_overflow = attrs
            .iter()
            .any(|(_, present, vals)| *present && vals.len() > 1 && vals.len() > len);
        if !has_overflow {
            continue;
        }
        // Upstream redistributes the surplus values onto the characters that FOLLOW the
        // element's own text inside its subtree (P:4833–4906), so truncating only loses
        // information when such characters exist. A leaf with one surplus trailing value —
        // the PDF-import shape, 22 of them in Acid_tests.svg — loses nothing, and 22
        // identical lines in Inkscape's modal dialog are pure noise.
        let lossy = texts_before[tail_pos[r.ddi]] > texts_before[ri + 1];
        for (attr, present, vals) in &attrs {
            let present = *present;
            if !present || vals.len() <= 1 || vals.len() <= len {
                continue;
            }
            if lossy {
                warn.push(format!(
                    "{}: {attr} has more values than characters; extra values dropped",
                    label(doc, n)
                ));
            }
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
    doc.children(n)
        .any(|c| doc.is_element(c) || doc.is_comment(c))
}

fn is_last_child(doc: &Doc, n: NodeId) -> bool {
    match doc.parent(n) {
        Some(p) => {
            doc.children(p)
                .filter(|&c| doc.is_element(c) || doc.is_comment(c))
                .last()
                == Some(n)
        }
        None => false,
    }
}

fn cleanup_whitespace(doc: &mut Doc, el: NodeId, runs: &[Run], is_flow: bool) {
    if !doc.xml_space_preserve(el) {
        for r in runs {
            let Some(txt) = run_text(doc, r) else {
                continue;
            };
            if txt.chars().count() <= 1 {
                continue;
            }
            let collapsed = collapse(&txt);
            let new = if !r.is_tail {
                let mut s = collapsed.trim_matches(WS).to_string();
                if has_node_children(doc, r.node)
                    && txt.chars().last().is_some_and(|c| WS.contains(&c))
                {
                    s.push(' ');
                }
                s
            } else {
                let core = collapsed.trim_matches(WS).to_string();
                if txt.chars().next().is_some_and(|c| WS.contains(&c)) {
                    format!(" {core}")
                } else {
                    core
                }
            };
            set_run_text(doc, r, Some(&new));
        }
    }
    if !is_flow {
        for r in runs {
            let Some(txt) = run_text(doc, r) else {
                continue;
            };
            let last_span = if r.is_tail {
                is_last_child(doc, r.node)
            } else {
                !has_node_children(doc, r.node)
            };
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
        let tail = Run {
            ddi: i,
            node: n,
            is_tail: true,
            style_node: n,
        };
        let Some(t) = run_text(doc, &tail) else {
            continue;
        };
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
            Some(p) => Run {
                ddi: 0,
                node: p,
                is_tail: true,
                style_node: p,
            },
            None => {
                let parent = doc.parent(n).expect("comment inside the text element");
                Run {
                    ddi: 0,
                    node: parent,
                    is_tail: false,
                    style_node: parent,
                }
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
