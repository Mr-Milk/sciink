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
                let kids: Vec<NodeId> = doc
                    .children(n)
                    .filter(|&c| doc.is_element(c) || doc.is_comment(c))
                    .collect();
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
        for (i, _) in closing.iter().enumerate() {
            let node = self.dds[i];
            if !doc.is_comment(node) {
                out.push(Run {
                    ddi: i,
                    node,
                    is_tail: false,
                    style_node: node,
                });
            }
            // innermost first: deeper nodes end before their ancestors
            let mut c = closing[i].clone();
            c.sort_by(|a, b| b.cmp(a));
            for j in c {
                let pnode = self.dds[self.parent[j].expect("non-root")];
                out.push(Run {
                    ddi: j,
                    node: self.dds[j],
                    is_tail: true,
                    style_node: pnode,
                });
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
