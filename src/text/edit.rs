//! Model editing primitives (spec §A.1 stages 2, 5, 10 and the pieces `Perform_Merges`/splits are
//! built from). Everything here edits the `ParsedText` model only; the DOM is written once, later,
//! by `text::write`. Upstream refs: parser.py (P:…) unless noted.

use std::collections::HashSet;
use std::rc::Rc;

use kurbo::Affine;

use crate::dom::{Doc, NodeId};
use crate::num;
use crate::style::Style;

use super::layout::{chunk_geom, dadv, full_extent, unrendered_space};
use super::parse::{CharLoc, ParsedText, TextLengthAdj, XY_TOL};

/// The node whose style a character carries: the node itself for a text run, its parent for a tail
/// (upstream `CLoc.sel`).
pub fn sel(doc: &Doc, loc: &CharLoc) -> NodeId {
    if loc.tail {
        doc.parent(loc.node).unwrap_or(loc.node)
    } else {
        loc.node
    }
}

/// Order-insensitive value equality (upstream compares `Style` dicts).
pub fn style_eq(a: &Style, b: &Style) -> bool {
    a.0.len() == b.0.len() && a.0.iter().all(|(k, v)| b.get(k) == Some(v.as_str()))
}

/// Rebuild `chars`, the snapshot vectors and every index from the line → chunk → character
/// structure (which is the source of truth after an edit). Returns `new index → old index`.
pub fn reindex(pt: &mut ParsedText) -> Vec<usize> {
    let ParsedText {
        chars,
        lines,
        parsed_ut,
        parsed_t,
        any_dx,
        any_dy,
        ..
    } = pt;
    let old = std::mem::take(chars);
    let old_ut = std::mem::take(parsed_ut);
    let old_t = std::mem::take(parsed_t);
    let snap = !old_ut.is_empty();
    let mut map = Vec::with_capacity(old.len());
    for (li, ln) in lines.iter_mut().enumerate() {
        ln.chars.clear();
        for (ci, ch) in ln.chunks.iter_mut().enumerate() {
            for (wi, oi) in ch.chars.iter_mut().enumerate() {
                let ni = chars.len();
                let mut c = old[*oi].clone();
                c.line = li;
                c.chunk = ci;
                c.windex = wi;
                chars.push(c);
                if snap {
                    parsed_ut.push(old_ut.get(*oi).copied().flatten());
                    parsed_t.push(old_t.get(*oi).copied().flatten());
                }
                map.push(*oi);
                *oi = ni;
                ln.chars.push(ni);
            }
        }
    }
    *any_dx = chars.iter().any(|c| c.dx.abs() > XY_TOL);
    *any_dy = chars.iter().any(|c| c.dy.abs() > XY_TOL);
    map
}

/// Remove characters (by current index) from their chunks; empty chunks and lines are pruned;
/// surviving chunks keep their `x`/`y`. Returns `reindex`'s `new → old` map.
pub fn remove_chars(pt: &mut ParsedText, idxs: &[usize]) -> Vec<usize> {
    let gone: HashSet<usize> = idxs.iter().copied().collect();
    for ln in pt.lines.iter_mut() {
        for ch in ln.chunks.iter_mut() {
            ch.chars.retain(|c| !gone.contains(c));
        }
        ln.chunks.retain(|ch| !ch.chars.is_empty());
    }
    pt.lines.retain(|l| !l.chunks.is_empty());
    reindex(pt)
}

/// P:4029–4118. Delete one character so the remaining ones stay where they are: the chunk's anchor
/// moves by the deleted character's effective width, weighted by the anchor fraction (a deleted
/// first character shifts a start-anchored chunk right by its width; a deleted last character
/// shifts an end-anchored chunk left). Empty chunks and lines are pruned.
pub fn delete_char(pt: &mut ParsedText, idx: usize) {
    let c = pt.chars[idx].clone();
    let (li, ci, wi) = (c.line, c.chunk, c.windex);
    let ids = pt.lines[li].chunks[ci].chars.clone();
    let anfr = pt.lines[li].spec.anchor.anfr();
    // Upstream's `dko2`/`dkn` look up (self, next) and (prev, next) in the deleted character's own
    // pair table, which only ever holds pairs ENDING at that character, so they are always 0 and
    // only the kerning from the left neighbour survives (P:4036–4044).
    let tdk = if wi > 0 {
        dadv(&pt.chars[ids[wi - 1]], &c)
    } else {
        0.0
    };
    let mut cwo = c.cwd + tdk + c.dx + if wi != 0 { c.lsp } else { 0.0 };
    let n = ids.len();
    if unrendered_space(pt, li, ci) && wi == n - 1 && n > 1 && pt.chars[ids[n - 2]].c != ' ' {
        cwo = tdk; // an unrendered trailing space costs nothing (its kerning weirdly still counts)
    }
    let deltax = if wi == 0 {
        (anfr - 1.0) * cwo
    } else {
        anfr * cwo
    };
    if deltax.abs() > XY_TOL {
        pt.lines[li].chunks[ci].x -= deltax;
    }
    remove_chars(pt, &[idx]);
}

/// Stage 2 (P:673–699), only when manual kerning is being removed. `spacingAndGlyphs`: restore the
/// natural widths and fold the stretch into the element transform
/// (`translate(cx_with,0)·scale(adj,1)·translate(−cx_without,0)`, written by the writer via
/// `transform_extra`). `spacing`: the adjusted letter-spacing simply becomes each character's
/// specified `letter-spacing`. Either way `textLength`/`lengthAdjust` are dropped by the writer.
pub fn remove_textlength(pt: &mut ParsedText) {
    let Some(tl) = pt.text_length else {
        return;
    };
    match tl {
        TextLengthAdj::SpacingAndGlyphs(adj) => {
            if adj != 0.0 && adj.is_finite() {
                let with = full_extent(pt);
                for c in pt.chars.iter_mut() {
                    c.cwd /= adj;
                }
                let without = full_extent(pt);
                if let (Some(w), Some(wo)) = (with, without) {
                    let tfm = Affine::translate((w.center().x, 0.0))
                        * Affine::scale_non_uniform(adj, 1.0)
                        * Affine::translate((-wo.center().x, 0.0));
                    pt.transform *= tfm;
                    pt.transform_extra *= tfm;
                }
            }
        }
        TextLengthAdj::Spacing(_) => {
            for c in pt.chars.iter_mut() {
                let lsp = num::fmt(c.lsp);
                if c.sty.get("letter-spacing") != Some(lsp.as_str()) {
                    let mut s = (*c.sty).clone();
                    s.set("letter-spacing", &lsp);
                    c.sty = Rc::new(s);
                }
            }
        }
    }
    pt.text_length = None;
    pt.text_length_removed = true;
}

/// utils.py:130–153: sort, keep the first value, keep each further value that is more than `tol`
/// from the LAST KEPT one (not from its neighbour). Returns the kept representatives.
pub fn unique_reps(vals: &[f64], tol: f64) -> Vec<f64> {
    let mut v: Vec<f64> = vals.iter().copied().filter(|x| !x.is_nan()).collect();
    v.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    let mut out: Vec<f64> = Vec::new();
    for x in v {
        if out.last().is_none_or(|&l| x - l > tol) {
            out.push(x);
        }
    }
    out
}

/// Stage 4 (P:701–725): link the chunks that share a baseline within this element in ascending x
/// order (`next`/`prev`), remembering whether neighbours come from the same style node. A lone
/// `" "` chunk sitting exactly on the following chunk's left edge (a PDF-import artefact) is
/// ordered after that chunk. All links are reset first, so the chain can be rebuilt after stage 5.
pub fn make_next_chain(doc: &Doc, pt: &mut ParsedText) {
    for ln in pt.lines.iter_mut() {
        for ch in ln.chunks.iter_mut() {
            ch.next = None;
            ch.prev = None;
            ch.prev_same_tspan = false;
        }
    }
    const TOL: f64 = 0.001;
    let yvs: Vec<f64> = pt.lines.iter().map(|l| l.chunks[0].y).collect();
    for rep in unique_reps(&yvs, TOL) {
        // (line, chunk), centre x, left x, space width — for every chunk on this baseline
        let mut sws: Vec<((usize, usize), f64, f64, f64)> = Vec::new();
        for li in (0..pt.lines.len()).filter(|&i| (yvs[i] - rep).abs() < TOL) {
            for ci in 0..pt.lines[li].chunks.len() {
                let g = chunk_geom(pt, li, ci);
                sws.push((
                    (li, ci),
                    0.5 * (g.pts_ut[0].x + g.pts_ut[3].x),
                    g.pts_ut[0].x,
                    super::layout::chunk_spw(pt, li, ci),
                ));
            }
        }
        sws.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for i in 1..sws.len() {
            let prev_is_space = pt.chunk_text(sws[i - 1].0.0, sws[i - 1].0.1) == " ";
            if prev_is_space && (sws[i - 1].2 - sws[i].2).abs() < 0.01 * sws[i - 1].3 {
                sws.swap(i - 1, i);
            }
        }
        for i in 1..sws.len() {
            let (a, b) = (sws[i - 1].0, sws[i].0);
            let aid = pt.lines[a.0].chunks[a.1].id;
            let bid = pt.lines[b.0].chunks[b.1].id;
            let a_last = *pt.lines[a.0].chunks[a.1]
                .chars
                .last()
                .expect("non-empty chunk");
            let b_first = pt.lines[b.0].chunks[b.1].chars[0];
            let same = sel(doc, &pt.chars[a_last].loc) == sel(doc, &pt.chars[b_first].loc);
            pt.lines[a.0].chunks[a.1].next = Some(bid);
            let cb = &mut pt.lines[b.0].chunks[b.1];
            cb.prev = Some(aid);
            cb.prev_same_tspan = same;
        }
    }
}

/// Stage 5 (P:1665–1687 + `write_axay` P:865–957): every character with a `dx` becomes the first
/// character of a new chunk positioned where it already is (`ax = left·(1−anfr) + right_lc·anfr`,
/// `lc` = last char before the next dx'd one), every `dy` becomes an absolute `y`, and both are
/// zeroed. Upstream then re-parses: a coordinate that follows a character without one opens a new
/// LINE (P:886–893), and within a line any coordinate opens a new chunk (P:2704–2716). Lines that
/// got no x continue from the end of the previous line; no y → previous line's last y.
pub fn rechunk_absolute(pt: &mut ParsedText) {
    if pt.is_flow || !pt.chars.iter().any(|c| c.dx.abs() > XY_TOL) {
        return;
    }
    let mut new_lines: Vec<super::parse::TLine> = Vec::new();
    for li in 0..pt.lines.len() {
        let anfr = pt.lines[li].spec.anchor.anfr();
        // (char, ax, ay) in line order
        let mut axay: Vec<(usize, Option<f64>, Option<f64>)> = Vec::new();
        for ci in 0..pt.lines[li].chunks.len() {
            let pts = super::layout::chunk_char_pts(pt, li, ci);
            let ch = pt.lines[li].chunks[ci].clone();
            for (j, &c) in ch.chars.iter().enumerate() {
                let (dx, dy) = (pt.chars[c].dx, pt.chars[c].dy);
                // Deviation: the first character of a chunk gets the same anchor-weighted formula
                // as a dx'd one. Upstream keeps `w.x` (P:1679), which mis-places a middle/end-
                // anchored first segment until stage 11 corrects it; for a start anchor with
                // dx[0] == 0 both agree.
                let ax = if dx.abs() > XY_TOL || j == 0 {
                    let lc = (j + 1..ch.chars.len())
                        .find(|&k| pt.chars[ch.chars[k]].dx != 0.0)
                        .map_or(ch.chars.len() - 1, |k| k - 1);
                    if dx.abs() > XY_TOL {
                        pt.chars[c].dx = 0.0;
                    }
                    Some(pts[j][0].x * (1.0 - anfr) + pts[lc][3].x * anfr)
                } else {
                    None
                };
                let ay = if dy.abs() > XY_TOL {
                    pt.chars[c].dy = 0.0;
                    Some(pts[j][0].y)
                } else if j == 0 {
                    Some(ch.y)
                } else {
                    None
                };
                axay.push((c, ax, ay));
            }
        }
        let mut starts = vec![0usize];
        for i in 1..axay.len() {
            let (_, px, py) = axay[i - 1];
            let (_, x, y) = axay[i];
            if (px.is_none() && x.is_some()) || (py.is_none() && y.is_some()) {
                starts.push(i);
            }
        }
        starts.push(axay.len());
        let old = pt.lines[li].clone();
        for (k, w) in starts.windows(2).enumerate() {
            let seg = &axay[w[0]..w[1]];
            let (xv, yv) = (seg[0].1, seg[0].2);
            let mut spec = old.spec.clone();
            if k > 0 {
                spec.sprl = false;
                spec.continue_x = xv.is_none();
                spec.continue_y = yv.is_none();
            }
            spec.x = vec![xv];
            spec.y = vec![yv];
            let style = if k == 0 {
                old.style.clone()
            } else {
                pt.chars[seg[0].0].sty.clone()
            };
            let mut line = super::parse::TLine {
                spec,
                style,
                chars: seg.iter().map(|s| s.0).collect(),
                chunks: Vec::new(),
            };
            // NaN marks "not known yet": resolved below for continue lines, carried forward otherwise
            let (mut cx, mut cy) = (xv.unwrap_or(f64::NAN), yv.unwrap_or(f64::NAN));
            for (i, (c, ax, ay)) in seg.iter().enumerate() {
                if i == 0 || ax.is_some() || ay.is_some() {
                    if let Some(x) = ax {
                        cx = *x;
                    }
                    if let Some(y) = ay {
                        cy = *y;
                    }
                    let id = pt.new_chunk_id();
                    line.chunks.push(super::parse::TChunk {
                        id,
                        x: cx,
                        y: cy,
                        chars: vec![*c],
                        next: None,
                        prev: None,
                        prev_same_tspan: false,
                    });
                } else {
                    line.chunks
                        .last_mut()
                        .expect("opened at i == 0")
                        .chars
                        .push(*c);
                }
            }
            new_lines.push(line);
        }
    }
    pt.lines = new_lines;
    reindex(pt);
    for li in 0..pt.lines.len() {
        if li > 0 {
            let (cx, cy) = (pt.lines[li].spec.continue_x, pt.lines[li].spec.continue_y);
            if cx || cy {
                let pli = li - 1;
                let pci = pt.lines[pli].chunks.len() - 1;
                let prev_y = pt.lines[pli].chunks[pci].y;
                let g = chunk_geom(pt, pli, pci);
                let anfr = pt.lines[li].spec.anchor.anfr();
                let ln = &mut pt.lines[li];
                if cx {
                    let x = (1.0 + anfr) * g.pts_ut[3].x - anfr * g.pts_ut[0].x;
                    ln.chunks[0].x = x;
                    ln.spec.x = vec![Some(x)];
                }
                if cy {
                    ln.chunks[0].y = prev_y;
                    ln.spec.y = vec![Some(prev_y)];
                }
            }
        }
        // chunks that opened on a y (or x) alone inherit the other coordinate from the chunk before
        let ln = &mut pt.lines[li];
        for ci in 1..ln.chunks.len() {
            if ln.chunks[ci].x.is_nan() {
                ln.chunks[ci].x = ln.chunks[ci - 1].x;
            }
            if ln.chunks[ci].y.is_nan() {
                ln.chunks[ci].y = ln.chunks[ci - 1].y;
            }
        }
    }
}
