//! `<text>` → lines / chunks / characters (spec §A.1 stage 1, §A.2; upstream parser.py:280–650, 2696–2716).

use crate::dom::{Doc, NodeId};

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

/// Per-descendant position analysis (P:379–469).
pub struct Positions {
    /// `x`/`y` after bidirectional inheritance.
    pub x: Vec<Vec<Option<f64>>>,
    pub y: Vec<Vec<Option<f64>>>,
    /// `dx`/`dy` as written; never inherited.
    pub dx: Vec<Vec<Option<f64>>>,
    pub dy: Vec<Vec<Option<f64>>>,
    /// `dds` index the (possibly inherited) `x`/`y` value came from.
    pub xsrc: Vec<usize>,
    pub ysrc: Vec<usize>,
    pub esprl: Vec<bool>,
    pub types: Vec<SprlType>,
}

/// Whether `n` carries an (active or inactive) `sodipodi:role="line"`.
fn has_sprl_role(doc: &Doc, n: NodeId) -> bool {
    doc.is_element(n) && doc.attr(n, "sodipodi:role") == Some("line")
}

/// The node's own lxml-style `.text` (leading text run); `None` for comments.
fn own_text(doc: &Doc, tree: &TextTree, i: usize) -> Option<String> {
    let r = Run {
        ddi: i,
        node: tree.dds[i],
        is_tail: false,
        style_node: tree.dds[i],
    };
    if doc.is_comment(tree.dds[i]) {
        None
    } else {
        run_text(doc, &r)
    }
}

/// First `dds` index after `i` that is not a descendant of it.
fn subtree_end(tree: &TextTree, i: usize) -> usize {
    let mut j = i + 1;
    while j < tree.dds.len() && is_descendant(tree, j, i) {
        j += 1;
    }
    j
}

/// Whether `j` is a (possibly indirect) descendant of `ancestor` in `tree`.
fn is_descendant(tree: &TextTree, j: usize, ancestor: usize) -> bool {
    let mut p = tree.parent[j];
    while let Some(pi) = p {
        if pi == ancestor {
            return true;
        }
        p = tree.parent[pi];
    }
    false
}

pub fn positions(doc: &Doc, tree: &TextTree) -> Positions {
    let n = tree.dds.len();
    let get = |attr: &str| -> Vec<Vec<Option<f64>>> {
        tree.dds
            .iter()
            .map(|&d| {
                if doc.is_element(d) {
                    get_xy(doc, d, attr)
                } else {
                    vec![None]
                }
            })
            .collect()
    };
    let (xs, ys, dxs, dys) = (get("x"), get("y"), get("dx"), get("dy"));
    let texts: Vec<Option<String>> = (0..n).map(|i| own_text(doc, tree, i)).collect();
    let empty = |i: usize| texts[i].as_deref().is_none_or(str::is_empty);
    let nsprl: Vec<bool> = tree.dds.iter().map(|&d| has_sprl_role(doc, d)).collect();

    // Effective sodipodi:role="line": a direct child of the root with exactly one x
    // and one y value; disabled when its own text is empty and some descendant
    // (up to the next non-descendant) has a position and non-empty text of its own.
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

    // Types need "is there a non-empty tail right before this node's own text run",
    // which is only visible from the run sequence, so derive it from `tree.runs`.
    let runs = tree.runs(doc);
    let mut types = vec![SprlType::Normal; n];
    for i in 0..n {
        if !esprl[i] {
            continue;
        }
        let ri = runs
            .iter()
            .position(|r| !r.is_tail && r.ddi == i)
            .expect("every element has a text run");
        let preceded = ri > 0 && runs[ri - 1].is_tail && run_text(doc, &runs[ri - 1]).is_some();
        // The first direct child is always dds[1] in this pre-order arrangement.
        let first_kid = tree.parent[i] == Some(0) && i == 1;
        types[i] = if preceded || (first_kid && texts[0].is_some()) {
            SprlType::PrecededSprl
        } else {
            SprlType::TlvlSprl
        };
    }

    // Bidirectional inheritance for x/y lists whose first entry is None: walk out from
    // `i` through empty-text, non-esprl parent/child links, then prefer an ancestor
    // (j <= i), then the nearest candidate with a value.
    let inherits_from = |i: usize| -> (usize, usize) {
        let mut jmax = i;
        while jmax + 1 < n && empty(jmax) && tree.parent[jmax + 1] == Some(jmax) && !esprl[jmax + 1]
        {
            jmax += 1;
        }
        if jmax + 1 < n && empty(jmax) {
            jmax = i;
        }
        let mut jmin = i;
        while jmin > 0 && empty(jmin - 1) && tree.parent[jmin] == Some(jmin - 1) && !esprl[jmin - 1]
        {
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
            let best = cands.into_iter().min_by_key(|&j| j.abs_diff(i)).unwrap();
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

    Positions {
        x,
        y,
        dx: dxs,
        dy: dys,
        xsrc,
        ysrc,
        esprl,
        types,
    }
}

/// A line start discovered while walking the runs (the first half of P:477–585).
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
    /// Index into `runs` of the run that opened the line.
    pub first_run: usize,
}

pub fn line_specs(doc: &Doc, tree: &TextTree, runs: &[Run], pos: &Positions) -> Vec<LineSpec> {
    let root = tree.dds[0];
    let kids: Vec<NodeId> = doc.children(root).filter(|&c| doc.is_element(c)).collect();
    let mut lines: Vec<LineSpec> = Vec::new();
    // The line a new TLVLSPRL line stacks its line-height off of: the first line,
    // or the most recent TLVLSPRL line.
    let mut sprl_inherits: Option<usize> = None;

    for (ri, r) in runs.iter().enumerate() {
        let has_txt = run_text(doc, r).as_deref().is_some_and(|t| !t.is_empty());
        let newsprl = !r.is_tail && pos.types[r.ddi] == SprlType::TlvlSprl;
        if !(has_txt || newsprl) {
            continue;
        }
        let makeline = lines.is_empty()
            || (!r.is_tail
                && (newsprl
                    || (pos.types[r.ddi] == SprlType::Normal
                        && (pos.x[r.ddi][0].is_some() || pos.y[r.ddi][0].is_some()))));
        if !makeline {
            continue;
        }

        // `ddi` already names the run's own node for both text and tail runs
        // (tree::runs keeps a tail's `ddi` pointing at the node it is the tail
        // of, not its style-lookup parent) so it is always "the node's own ddi".
        let edi = r.ddi;
        let sel = r.style_node;
        let sty = doc.specified_style(sel);
        let (mut xv, mut xsrc, mut yv, mut ysrc) = (
            pos.x[edi].clone(),
            pos.xsrc[edi],
            pos.y[edi].clone(),
            pos.ysrc[edi],
        );
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
                    let parent = doc
                        .parent(node)
                        .filter(|&p| doc.is_element(p))
                        .unwrap_or(root);
                    let lht =
                        composed_line_height(doc, node).max(composed_line_height(doc, parent));
                    let scf = composed_font_size(doc, node).scf;
                    let prev = &lines[li];
                    xv = vec![prev.x[0]];
                    yv = vec![prev.y[0].map(|y| y + lht / scf)];
                    // The sources are kept as node ids on `LineSpec`; convert back to a
                    // `dds` index here so the rest of this function can treat xsrc/ysrc
                    // uniformly, then convert forward again when the line is pushed.
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

        let tlvlno = if kids.contains(&tree.dds[edi]) {
            kids.iter().position(|&k| k == tree.dds[edi])
        } else if edi == 0 {
            Some(0)
        } else {
            None
        };

        let mut anchor = sty.get("text-anchor").and_then(Anchor::parse);
        if let Some(last) = lines.last() {
            if !has_sprl_role(doc, sel) && edi > 0 {
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
