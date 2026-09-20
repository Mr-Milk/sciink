//! Model editing primitives (spec §A.1 stages 2, 5, 10 and the pieces `Perform_Merges`/splits are
//! built from). Everything here edits the `ParsedText` model only; the DOM is written once, later,
//! by `text::write`. Upstream refs: parser.py (P:…) unless noted.

use std::collections::HashSet;
use std::rc::Rc;

use kurbo::Affine;

use crate::dom::{Doc, NodeId};
use crate::num;
use crate::style::Style;

use super::layout::{full_extent, unrendered_space};
use super::parse::{CharLoc, ParsedText, TChar, TextLengthAdj, XY_TOL};

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

/// Pair kerning between two adjacent characters, as `layout::chunk_geom` applies it: only within one
/// text node and only for pairs the char table measured.
fn dadv(prev: &TChar, cur: &TChar) -> f64 {
    if prev.loc.node == cur.loc.node && prev.loc.tail == cur.loc.tail {
        cur.prop.dadvs.get(&prev.c).copied().unwrap_or(0.0) * cur.utfs
    } else {
        0.0
    }
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
