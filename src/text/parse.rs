//! `<text>` → lines / chunks / characters (spec §A.1 stage 1, §A.2; upstream parser.py:280–650, 2696–2716).

use std::rc::Rc;

use kurbo::Point;

use crate::dom::{Doc, NodeId};
use crate::geom::{Affine, ipx};
use crate::style::Style;

use super::Warnings;
use super::fonts::{FaceKey, FontSpec};
use super::metrics::CProp;
use super::style::{
    Anchor, baseline_shift, composed_font_size, composed_line_height, letter_spacing,
};
use super::table::CharTable;
use super::tree::{Run, TextTree, run_text};
use super::whitespace::{depathologize, get_xy};

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

        // A tail is positioned by its parent (the run's style source), never by
        // the empty node it follows: `<text x="5"><tspan x="9"/>Hello</text>`
        // opens its line at the `<text>`'s x=5, not the tspan's x=9 (upstream:
        // `edi = dds.index(sel)` for a tail run).
        let edi = if r.is_tail {
            tree.dds
                .iter()
                .position(|&d| d == r.style_node)
                .unwrap_or(0)
        } else {
            r.ddi
        };
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
                        xv = vec![l.x[0]];
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
                        yv = vec![l.y[0]];
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
            // Upstream reads `nsprl[sel]`, which depathologize has already pruned down to
            // `esprl` (P:396–401), so an *inactive* role — a role=line tspan with a
            // multi-value x, say — does not stop the line inheriting the previous
            // anchor. `sel == dds[edi]` for both text and tail runs, so `esprl[edi]` is
            // that same flag.
            if !pos.esprl[edi] && edi > 0 {
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
    /// Stable within one `ParsedText`; survives `edit::reindex`, so merge plans can refer to a
    /// chunk while other chunks are being removed. Look it up with `ParsedText::find_chunk`.
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub chars: Vec<usize>,
    /// Next/previous chunk on the same baseline within this element (stage 4, `edit::make_next_chain`).
    pub next: Option<u32>,
    pub prev: Option<u32>,
    /// `prev`'s last char and this chunk's first char sit in the same style node (P:724–725).
    pub prev_same_tspan: bool,
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

/// Where a `ParsedText` came from: an element that exists in the document (rewritten in place,
/// id reused) or a piece split off another element by `edit::split_off` (a new element, inserted
/// right after the element it came from, which is what `el` then names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Existing,
    SplitFrom,
}

pub struct ParsedText {
    pub el: NodeId,
    pub transform: Affine,
    pub chars: Vec<TChar>,
    pub lines: Vec<TLine>,
    pub is_flow: bool,
    /// The element has a `<textPath>` descendant: measured, never edited (spec §A.2 "Not handled").
    pub has_text_path: bool,
    pub is_inkscape: bool,
    pub is_ml_inkscape: bool,
    pub text_length: Option<TextLengthAdj>,
    pub any_dx: bool,
    pub any_dy: bool,
    pub origin: Origin,
    /// Arena index of the model this one was split from (`Origin::SplitFrom`); `None` for a parsed element.
    pub split_src: Option<usize>,
    /// Per-character corner points frozen by `layout::snapshot_parsed` (stage 3): `[BL, TL, TR, BR]`
    /// in this element's frame and in root coordinates. Index-aligned with `chars`; `None` for
    /// characters created after the snapshot (inserted spaces). Empty until the snapshot is taken.
    pub parsed_ut: Vec<Option<[Point; 4]>>,
    pub parsed_t: Vec<Option<[Point; 4]>>,
    /// Extra transform the writer multiplies onto the element's own `transform` (stage 2).
    pub transform_extra: Affine,
    /// `text-anchor`/`text-align` to write on the `<text>` itself (stage 9, RK:175–178).
    pub text_anchor_override: Option<Anchor>,
    /// `textLength`/`lengthAdjust` were undone (stage 2) and must not be copied by the writer.
    pub text_length_removed: bool,
    pub next_chunk_id: u32,
}

fn is_flow(doc: &Doc, el: NodeId) -> bool {
    if doc.tag(el) == "flowRoot" {
        return true;
    }
    let sty = doc.specified_style(el);
    let shape = sty
        .get("shape-inside")
        .and_then(|v| {
            v.trim()
                .strip_prefix("url(#")
                .and_then(|r| r.strip_suffix(')'))
        })
        .is_some_and(|id| doc.by_id(id.trim()).is_some());
    shape
        || sty
            .get("inline-size")
            .and_then(ipx)
            .is_some_and(|v| v != 0.0)
}

/// Whether `el` carries a `<textPath>` descendant. Such an element is measured (it feeds the char
/// table; `text_bbox` reports nothing for it) but never parsed or edited: its glyphs follow a path, which neither the
/// model nor the writer represents, so regenerating it would drop the path and move every glyph
/// to the baseline (spec §A.2 "Not handled … `<textPath>` (skip element)").
fn has_text_path(doc: &Doc, el: NodeId) -> bool {
    doc.descendants(el)
        .any(|n| n != el && doc.is_element(n) && doc.tag(n) == "textPath")
}

impl ParsedText {
    pub fn parse(
        doc: &mut Doc,
        el: NodeId,
        ct: &mut CharTable,
        warn: &mut Warnings,
    ) -> Option<ParsedText> {
        let flow = is_flow(doc, el);
        let on_path = has_text_path(doc, el);
        if !on_path {
            // an element on a path must come back byte-identical, and depathologize writes
            depathologize(doc, el, flow, warn);
        }
        let transform = doc.composed_transform(el);
        let mut pt = ParsedText {
            el,
            transform,
            chars: Vec::new(),
            lines: Vec::new(),
            is_flow: flow,
            has_text_path: on_path,
            is_inkscape: false,
            is_ml_inkscape: false,
            text_length: None,
            any_dx: false,
            any_dy: false,
            origin: Origin::Existing,
            split_src: None,
            parsed_ut: Vec::new(),
            parsed_t: Vec::new(),
            transform_extra: Affine::IDENTITY,
            text_anchor_override: None,
            text_length_removed: false,
            next_chunk_id: 0,
        };
        if flow || on_path {
            // v1: flows and text on a path are detected, never parsed (spec §A.2 "Flowed text v1",
            // "Not handled … `<textPath>` (skip element)")
            return Some(pt);
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
            .map(|s| TLine {
                spec: s.clone(),
                style: doc.specified_style(s.style_node),
                chars: Vec::new(),
                chunks: Vec::new(),
            })
            .collect();
        let mut next_line = 0usize; // index of the next LineSpec whose first_run we have not reached
        let mut cur: Option<usize> = None;
        for (ri, r) in runs.iter().enumerate() {
            while next_line < specs.len() && specs[next_line].first_run == ri {
                cur = Some(next_line);
                next_line += 1;
            }
            let Some(txt) = run_text(doc, r) else {
                continue;
            };
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
                    loc: CharLoc {
                        node: r.node,
                        tail: r.is_tail,
                        idx: j as u32,
                    },
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
                let opens = i == 0
                    || xs.get(i).is_some_and(Option::is_some)
                    || ys.get(i).is_some_and(Option::is_some);
                if opens {
                    px = xs.get(i.min(xs.len() - 1)).copied().flatten().unwrap_or(px);
                    py = ys.get(i.min(ys.len() - 1)).copied().flatten().unwrap_or(py);
                    let id = pt.new_chunk_id();
                    ln.chunks.push(TChunk {
                        id,
                        x: px,
                        y: py,
                        chars: vec![ci],
                        next: None,
                        prev: None,
                        prev_same_tspan: false,
                    });
                } else {
                    ln.chunks
                        .last_mut()
                        .expect("opened at i == 0")
                        .chars
                        .push(ci);
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
        // Lines that inherit a coordinate continue from the END of the previous line
        // (P:2640–2653 for x — upstream's anchor form verbatim, spec risk 7; P:2661–2675 for y:
        // the previous line's last chunk y). Chunks after the first in such a line carry the
        // resolved coordinate forward when they had none of their own.
        for li in 1..pt.lines.len() {
            let (cx, cy) = (pt.lines[li].spec.continue_x, pt.lines[li].spec.continue_y);
            if !(cx || cy) {
                continue;
            }
            let pli = li - 1;
            let pci = pt.lines[pli].chunks.len() - 1;
            let prev_y = pt.lines[pli].chunks[pci].y;
            let g = super::layout::chunk_geom(&pt, pli, pci);
            let anfr = pt.lines[li].spec.anchor.anfr();
            let new_x = (1.0 + anfr) * g.pts_ut[3].x - anfr * g.pts_ut[0].x;
            let ln = &mut pt.lines[li];
            // Every chunk of a continuing line borrowed the coordinate (its LineSpec list is the
            // single-entry placeholder `line_specs` leaves), so all of them take the resolved value.
            for ch in ln.chunks.iter_mut() {
                if cx {
                    ch.x = new_x;
                }
                if cy {
                    ch.y = prev_y;
                }
            }
            if cx {
                ln.spec.x = vec![Some(new_x)];
            }
            if cy {
                ln.spec.y = vec![Some(prev_y)];
            }
        }
        pt.any_dx = pt.chars.iter().any(|c| c.dx.abs() > XY_TOL);
        pt.any_dy = pt.chars.iter().any(|c| c.dy.abs() > XY_TOL);
        let tlvl: Vec<&TLine> = pt
            .lines
            .iter()
            .filter(|l| l.spec.tlvlno.is_some_and(|n| n > 0))
            .collect();
        pt.is_inkscape = !tlvl.is_empty()
            && tlvl.iter().all(|l| l.spec.sprl)
            && pt
                .lines
                .iter()
                .all(|l| l.style.get("-inkscape-font-specification").is_some());
        pt.is_ml_inkscape = pt.is_inkscape && pt.lines.len() > 1;
        // textLength (P:648–671)
        if let Some(tl) = doc.attr(el, "textLength").and_then(ipx) {
            // ponytail: Σ cwd stands in for Σ chunk widths; exact when the element has no dx/letter-spacing
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
                let adj = if pt.chars.len() > 1 && gaps > 0 {
                    (tl - total) / gaps as f64
                } else {
                    0.0
                };
                for c in pt.chars.iter_mut() {
                    c.lsp += adj;
                }
                pt.text_length = Some(TextLengthAdj::Spacing(adj));
            }
        }
        Some(pt)
    }

    pub fn chunks(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.lines
            .iter()
            .enumerate()
            .flat_map(|(li, l)| (0..l.chunks.len()).map(move |ci| (li, ci)))
    }

    pub fn chunk(&self, li: usize, ci: usize) -> &TChunk {
        &self.lines[li].chunks[ci]
    }

    pub fn text(&self) -> String {
        self.chars.iter().map(|c| c.c).collect()
    }
}

impl ParsedText {
    pub fn new_chunk_id(&mut self) -> u32 {
        let id = self.next_chunk_id;
        self.next_chunk_id += 1;
        id
    }

    /// `(line, chunk)` of the chunk with this id, or `None` when it has been merged away.
    pub fn find_chunk(&self, id: u32) -> Option<(usize, usize)> {
        self.lines
            .iter()
            .enumerate()
            .find_map(|(li, l)| l.chunks.iter().position(|c| c.id == id).map(|ci| (li, ci)))
    }

    pub fn chunk_text(&self, li: usize, ci: usize) -> String {
        self.lines[li].chunks[ci]
            .chars
            .iter()
            .map(|&c| self.chars[c].c)
            .collect()
    }

    pub fn line_text(&self, li: usize) -> String {
        self.lines[li]
            .chars
            .iter()
            .map(|&c| self.chars[c].c)
            .collect()
    }
}

/// Which face Pango uses for a character (P:727–754): spaces borrow their neighbours' fallback face.
fn font_picker(
    ct: &mut CharTable,
    txt: &[char],
    j: usize,
    spec: &FontSpec,
    tsty: Option<FaceKey>,
) -> Option<FaceKey> {
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
