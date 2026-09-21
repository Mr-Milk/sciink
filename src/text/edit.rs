//! Model editing primitives (spec §A.1 stages 2, 5, 10 and the pieces `Perform_Merges`/splits are
//! built from). Everything here edits the `ParsedText` model only; the DOM is written once, later,
//! by `text::write`. Upstream refs: parser.py (P:…) unless noted.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use kurbo::Affine;

use crate::dom::{Doc, NodeId};
use crate::geom::inverse;
use crate::num;
use crate::style::Style;

use super::layout::{
    chunk_char_pts, chunk_geom, dadv, full_extent, transform_pts, unrendered_space,
};
use super::parse::{CharLoc, Origin, ParsedText, TChar, TChunk, TLine, TextLengthAdj, XY_TOL};
use super::style::Anchor;
use super::style::composed_font_size;
use super::table::CharTable;

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
                // `px`, not upstream's bare number: a unitless CSS length is invalid and browsers
                // drop the declaration (spec §A.1 stage 12, "deliberate defensive deviations")
                let lsp = format!("{}px", num::fmt(c.lsp));
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

/// `(index into the ParsedText arena, chunk id)` — how merge plans name a chunk across elements.
pub type ChunkRef = (usize, u32);

/// How a merged chunk relates to the text it joins (`Perform_Merges`' `wtypes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WType {
    Normal,
    Sub,
    Super,
}

pub struct Incoming {
    pub chunk: ChunkRef,
    pub wtype: WType,
    /// Cap on the spaces inserted before this block (`None` = as many as the gap says).
    pub max_spaces: Option<usize>,
}

/// P:3120–3326 (`append_chks`): type the incoming chunks after the target chunk's last character.
pub fn append_chunks(
    doc: &Doc,
    pts: &mut [ParsedText],
    ct: &mut CharTable,
    target: ChunkRef,
    incoming: &[Incoming],
) {
    let (tp, tid) = target;
    let Some((tli, tci)) = pts[tp].find_chunk(tid) else {
        return;
    };
    let Some(inv) = inverse(pts[tp].transform) else {
        return;
    };
    let anfr = pts[tp].lines[tli].spec.anchor.anfr();

    // 1. Incoming characters, cloned, with their parsed points re-expressed in the target frame (P:3126–3128).
    // A character's optional frozen corner snapshot (`parsed_ut`/`parsed_t`'s element type).
    type Snap = Option<[kurbo::Point; 4]>;
    struct Block {
        chars: Vec<(TChar, Snap, Snap)>,
        wtype: WType,
        max_spaces: Option<usize>,
        src: ChunkRef,
    }
    let mut blocks: Vec<Block> = Vec::new();
    for inc in incoming {
        let (sp, sid) = inc.chunk;
        let Some((li, ci)) = pts[sp].find_chunk(sid) else {
            continue; // already merged away (RK:623–624)
        };
        let src = &pts[sp];
        let chars = src.lines[li].chunks[ci]
            .chars
            .iter()
            .map(|&c| {
                let t = src.parsed_t.get(c).copied().flatten();
                (src.chars[c].clone(), t.map(|p| transform_pts(inv, p)), t)
            })
            .collect();
        blocks.push(Block {
            chars,
            wtype: inc.wtype,
            max_spaces: inc.max_spaces,
            src: (sp, sid),
        });
    }
    if blocks.is_empty() {
        return;
    }

    // 2. Spaces before each block: round((bl2x − br1x) / spw of the target's last char), capped (P:3131–3164).
    let (lchr, first_idx) = {
        let t = &pts[tp];
        let ch = &t.lines[tli].chunks[tci];
        (
            t.chars[*ch.chars.last().expect("non-empty chunk")].clone(),
            ch.chars[0],
        )
    };
    let mut br1x = {
        let t = &pts[tp];
        t.lines[tli].chunks[tci]
            .chars
            .iter()
            .filter_map(|&c| t.parsed_ut.get(c).copied().flatten())
            .map(|p| p[3].x)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let space_prop = ct.prop(ct.true_face(&lchr.spec), ' ');
    // (char, parsed_ut, parsed_t, wtype, first of its block)
    let mut new_chars: Vec<(TChar, Snap, Snap, WType, bool)> = Vec::new();
    for b in &blocks {
        let bl2x = b
            .chars
            .iter()
            .filter_map(|(_, ut, _)| *ut)
            .map(|p| p[0].x)
            .fold(f64::INFINITY, f64::min);
        let br2x = b
            .chars
            .iter()
            .filter_map(|(_, ut, _)| *ut)
            .map(|p| p[3].x)
            .fold(f64::NEG_INFINITY, f64::max);
        let mut numsp = if lchr.spw > 0.0 && bl2x.is_finite() && br1x.is_finite() {
            ((bl2x - br1x) / lchr.spw).round().max(0.0) as usize
        } else {
            0
        };
        if let Some(m) = b.max_spaces {
            numsp = numsp.min(m);
        }
        if br2x.is_finite() {
            br1x = br2x;
        }
        for i in 0..numsp {
            let mut sp = lchr.clone();
            sp.c = ' ';
            sp.prop = space_prop.clone();
            sp.cwd = space_prop.charw * sp.utfs;
            sp.dx = -lchr.lsp;
            sp.dy = 0.0;
            new_chars.push((sp, None, None, b.wtype, i == 0));
        }
        for (j, (c, ut, t)) in b.chars.iter().enumerate() {
            new_chars.push((c.clone(), *ut, *t, b.wtype, numsp == 0 && j == 0));
        }
    }

    // 3. Where the new characters live (P:3166–3176): the target's last node, or — when that node is
    //    not the chunk's first node — the tail of its ancestor just below the first node / the element.
    //    Only the host's font size and specified style matter to the model (and `loc` for kerning).
    let first_sel = sel(doc, &pts[tp].chars[first_idx].loc);
    let lchr_sel = sel(doc, &lchr.loc);
    let same_host = || -> (CharLoc, f64, f64, Rc<Style>) {
        (
            CharLoc {
                node: lchr.loc.node,
                tail: lchr.loc.tail,
                idx: u32::MAX,
            },
            lchr.utfs,
            lchr.tfs,
            lchr.sty.clone(),
        )
    };
    let (host_loc, host_utfs, host_tfs, host_sty) = if lchr_sel == first_sel {
        same_host()
    } else {
        // Climb from the last character's node until the parent is the first character's node or
        // the element (P:3170–3175). Upstream's `totail` is None when the climb runs off the
        // document — the chunk ends in a node LESS nested than its first character
        // (`<text><tspan>Hi</tspan> ya</text>`) — and the characters then join the last
        // character's own node (P:3217–3223); never a node outside the element.
        let el = pts[tp].el;
        let mut cel = lchr_sel;
        let mut stop: Option<NodeId> = None;
        while let Some(p) = doc.parent(cel) {
            if p == first_sel || p == el {
                stop = Some(p);
                break;
            }
            cel = p;
        }
        match stop {
            None => same_host(),
            Some(parent) => {
                let (u, t, s) = if parent == first_sel {
                    let f = &pts[tp].chars[first_idx];
                    (f.utfs, f.tfs, f.sty.clone())
                } else {
                    let fs = composed_font_size(doc, el);
                    (fs.utfs, fs.tfs, doc.specified_style(el))
                };
                (
                    CharLoc {
                        node: cel,
                        tail: true,
                        idx: u32::MAX,
                    },
                    u,
                    t,
                    s,
                )
            }
        }
    };

    // 4. Remove the moved characters from their sources (P:3178–3205); a source may be the target element.
    for b in &blocks {
        let (sp, sid) = b.src;
        if let Some((li, ci)) = pts[sp].find_chunk(sid) {
            let ids = pts[sp].lines[li].chunks[ci].chars.clone();
            remove_chars(&mut pts[sp], &ids);
        }
    }
    let Some((tli, tci)) = pts[tp].find_chunk(tid) else {
        return;
    };

    // 5. Append; restyle moved characters (P:3266–3293); fix dx of block-firsts and the anchor (P:3297–3303).
    let pt = &mut pts[tp];
    let scf = if host_utfs > 0.0 {
        host_tfs / host_utfs
    } else {
        1.0
    };
    let mut sum_wd = 0.0;
    let mut prev_lsp = lchr.lsp;
    for (mut c, ut, t, wtype, first) in new_chars {
        c.loc = host_loc;
        let otype = c.sty.get("baseline-shift").map(str::to_string);
        let ntype = match (otype.as_deref(), wtype) {
            (Some("super"), WType::Normal) => WType::Super,
            (Some("sub"), WType::Normal) => WType::Sub,
            (_, w) => w,
        };
        // a zero-size host cannot be compared against (and would print `inf%`)
        let sizechanged = host_tfs > 0.0 && (c.tfs - host_tfs).abs() > 1e-4;
        if !style_eq(&c.sty, &host_sty) || matches!(ntype, WType::Super | WType::Sub) || sizechanged
        {
            let mut s = (*c.sty).clone();
            match ntype {
                WType::Super | WType::Sub => {
                    // Inkscape's native super/subscript convention (P:3277–3282)
                    s.set(
                        "baseline-shift",
                        if ntype == WType::Super {
                            "super"
                        } else {
                            "sub"
                        },
                    );
                    s.set("font-size", "65%");
                    c.bshft = if ntype == WType::Super { 0.4 } else { -0.2 } * host_utfs;
                    c.utfs = 0.65 * host_utfs;
                }
                WType::Normal if sizechanged => {
                    let pct = (c.tfs / host_tfs * 100.0).round();
                    s.set("font-size", &format!("{}%", num::fmt(pct)));
                    c.utfs = host_utfs * pct / 100.0;
                }
                WType::Normal => {
                    s.set("font-size", "100%");
                    c.utfs = host_utfs;
                }
            }
            c.tfs = c.utfs * scf;
            c.cwd = c.prop.charw * c.utfs;
            c.caph = c.prop.caph * c.utfs;
            c.spw = c.prop.spacew * c.utfs;
            c.sty = Rc::new(s);
        }
        if first {
            c.dx = -prev_lsp;
        }
        prev_lsp = c.lsp;
        sum_wd += c.cwd + c.dx;
        let idx = pt.chars.len();
        pt.chars.push(c);
        if !pt.parsed_ut.is_empty() {
            pt.parsed_ut.push(ut);
            pt.parsed_t.push(t);
        }
        pt.lines[tli].chunks[tci].chars.push(idx);
    }
    if anfr != 0.0 {
        pt.lines[tli].chunks[tci].x += anfr * sum_wd;
    }
    reindex(pt);
}

/// P:1258–1440 (`split_off_characters`) minus the XML half: the requested characters leave this
/// element and become new `ParsedText`s (`Origin::SplitFrom`, one line, one chunk each), one per
/// maximal run of characters that were contiguous within one chunk. Each new element sits at
/// `x = anfr·max_right + (1−anfr)·min_left` of its run and `y` = its first character's baseline,
/// copies the source line's anchor/direction/transform, and both the remaining and the new chunks
/// get `dx`/`dy`/anchor corrections so every glyph stays exactly where it was (P:1404–1431). Each
/// new model records `split_src = Some(src)`. Returns the arena indices of the new `ParsedText`s,
/// in creation order.
pub fn split_off(pts: &mut Vec<ParsedText>, src: usize, chr_lists: &[Vec<usize>]) -> Vec<usize> {
    // current (left, right, base) of every character, by index (P:1267)
    let mut before: HashMap<usize, (f64, f64, f64)> = HashMap::new();
    for li in 0..pts[src].lines.len() {
        for ci in 0..pts[src].lines[li].chunks.len() {
            let p = chunk_char_pts(&pts[src], li, ci);
            for (wi, &c) in pts[src].lines[li].chunks[ci].chars.iter().enumerate() {
                before.insert(c, (p[wi][0].x, p[wi][3].x, p[wi][0].y));
            }
        }
    }
    // runs (P:1158–1187): bucket by chunk in first-seen order, sort by windex, cut where windex jumps
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for list in chr_lists {
        let mut order: Vec<(usize, usize)> = Vec::new();
        let mut by: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for &c in list {
            let k = (pts[src].chars[c].line, pts[src].chars[c].chunk);
            if !by.contains_key(&k) {
                order.push(k);
            }
            by.entry(k).or_default().push(c);
        }
        for k in order {
            let mut cs = by.remove(&k).unwrap_or_default();
            cs.sort_by_key(|&c| pts[src].chars[c].windex);
            let mut run: Vec<usize> = Vec::new();
            for &c in &cs {
                if let Some(&p) = run.last() {
                    if pts[src].chars[c].windex != pts[src].chars[p].windex + 1 {
                        runs.push(std::mem::take(&mut run));
                    }
                }
                run.push(c);
            }
            if !run.is_empty() {
                runs.push(run);
            }
        }
    }
    // one new ParsedText per run (P:1189–1256, P:1380–1402)
    let mut news: Vec<(usize, Vec<usize>)> = Vec::new();
    for run in &runs {
        let s = &pts[src];
        let f = &s.chars[run[0]];
        let ln = &s.lines[f.line];
        let anfr = ln.spec.anchor.anfr();
        let minx = run
            .iter()
            .map(|c| before[c].0)
            .fold(f64::INFINITY, f64::min);
        let maxx = run
            .iter()
            .map(|c| before[c].1)
            .fold(f64::NEG_INFINITY, f64::max);
        let xv = anfr * maxx + (1.0 - anfr) * minx;
        let yv = before[&run[0]].2;
        let mut spec = ln.spec.clone();
        spec.x = vec![Some(xv)];
        spec.y = vec![Some(yv)];
        spec.sprl = false;
        spec.continue_x = false;
        spec.continue_y = false;
        spec.style_node = f.loc.node;
        spec.first_run = 0;
        let snap = !s.parsed_ut.is_empty();
        let mut np = ParsedText {
            el: s.el,
            transform: s.transform,
            chars: Vec::with_capacity(run.len()),
            lines: Vec::new(),
            is_flow: false,
            has_text_path: false, // a split-off is regenerated from the model: never on a path
            is_inkscape: s.is_inkscape,
            is_ml_inkscape: s.is_ml_inkscape,
            text_length: None,
            any_dx: false,
            any_dy: false,
            origin: Origin::SplitFrom,
            split_src: Some(src),
            parsed_ut: Vec::new(),
            parsed_t: Vec::new(),
            transform_extra: s.transform_extra,
            text_anchor_override: None,
            text_length_removed: s.text_length_removed,
            next_chunk_id: 0,
        };
        let mut idx = Vec::with_capacity(run.len());
        for (i, &c) in run.iter().enumerate() {
            let mut tc = s.chars[c].clone();
            tc.line = 0;
            tc.chunk = 0;
            tc.windex = i;
            np.chars.push(tc);
            if snap {
                np.parsed_ut.push(s.parsed_ut.get(c).copied().flatten());
                np.parsed_t.push(s.parsed_t.get(c).copied().flatten());
            }
            idx.push(i);
        }
        let id = np.new_chunk_id();
        np.lines.push(TLine {
            spec,
            style: f.sty.clone(),
            chars: idx.clone(),
            chunks: vec![TChunk {
                id,
                x: xv,
                y: yv,
                chars: idx,
                next: None,
                prev: None,
                prev_same_tspan: false,
            }],
        });
        np.any_dx = np.chars.iter().any(|c| c.dx.abs() > XY_TOL);
        np.any_dy = np.chars.iter().any(|c| c.dy.abs() > XY_TOL);
        pts.push(np);
        news.push((pts.len() - 1, run.clone()));
    }
    // remove from the source, then absorb every position error (P:1404–1431)
    let all: Vec<usize> = chr_lists.iter().flatten().copied().collect();
    let map = remove_chars(&mut pts[src], &all);
    fix_positions(&mut pts[src], |i| {
        map.get(i).and_then(|o| before.get(o)).copied()
    });
    for (npi, olds) in &news {
        fix_positions(&mut pts[*npi], |i| {
            olds.get(i).and_then(|o| before.get(o)).copied()
        });
    }
    news.into_iter().map(|(i, _)| i).collect()
}

/// P:1404–1431: `old(i)` gives a character's previous `(left, right, base)`; the difference to its
/// current position goes into `dx`/`dy` (as differences between consecutive errors) and the
/// chunk anchor (the first error, anchor-weighted), rounded to `XY_TOL`.
fn fix_positions(pt: &mut ParsedText, old: impl Fn(usize) -> Option<(f64, f64, f64)>) {
    for li in 0..pt.lines.len() {
        let anfr = pt.lines[li].spec.anchor.anfr();
        for ci in 0..pt.lines[li].chunks.len() {
            let now = chunk_char_pts(pt, li, ci);
            let ids = pt.lines[li].chunks[ci].chars.clone();
            let err: Vec<(f64, f64)> = ids
                .iter()
                .zip(&now)
                .map(|(&c, p)| match old(c) {
                    Some((l, _, b)) => (l - p[0].x, b - p[0].y),
                    None => (0.0, 0.0),
                })
                .collect();
            let Some(&needed) = err.first() else {
                continue;
            };
            let mut dxs = vec![0.0; err.len()];
            let mut dys = vec![0.0; err.len()];
            for i in 1..err.len() {
                dxs[i] = err[i].0 - err[i - 1].0;
                dys[i] = err[i].1 - err[i - 1].1;
            }
            for (i, &c) in ids.iter().enumerate() {
                if dxs[i].abs() > XY_TOL {
                    pt.chars[c].dx += dxs[i];
                }
                if dys[i].abs() > XY_TOL {
                    pt.chars[c].dy += dys[i];
                }
            }
            let fc_dx = -anfr * dxs[1..].iter().sum::<f64>();
            let shift_x = ((needed.0 - fc_dx) / XY_TOL).round() * XY_TOL;
            let shift_y = if needed.1.is_nan() {
                0.0
            } else {
                (needed.1 / XY_TOL).round() * XY_TOL
            };
            let ch = &mut pt.lines[li].chunks[ci];
            if shift_x != 0.0 {
                ch.x += shift_x;
            }
            if shift_y != 0.0 {
                ch.y += shift_y;
            }
        }
    }
    pt.any_dx = pt.chars.iter().any(|c| c.dx.abs() > XY_TOL);
    pt.any_dy = pt.chars.iter().any(|c| c.dy.abs() > XY_TOL);
}

/// P:2751–2787 minus the DOM writes: give a line a new anchor while every glyph stays put — each
/// chunk's new `x` is `(1−anfr)·minx + anfr·maxx` of its current box (an unrendered trailing space
/// of the line's last chunk does not count). All boxes are measured with the OLD anchor first.
pub fn change_alignment(pt: &mut ParsedText, li: usize, newanch: Anchor) {
    if pt.lines[li].spec.anchor == newanch {
        return;
    }
    let anfr = newanch.anfr();
    let last = *pt.lines[li].chars.last().expect("non-empty line");
    let mut newx = Vec::with_capacity(pt.lines[li].chunks.len());
    for ci in 0..pt.lines[li].chunks.len() {
        let g = chunk_geom(pt, li, ci);
        let minx = g.pts_ut.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let mut maxx = g
            .pts_ut
            .iter()
            .map(|p| p.x)
            .fold(f64::NEG_INFINITY, f64::max);
        if unrendered_space(pt, li, ci) && pt.lines[li].chunks[ci].chars.contains(&last) {
            maxx -= pt.chars[last].cwd;
        }
        newx.push((1.0 - anfr) * minx + anfr * maxx);
    }
    let ln = &mut pt.lines[li];
    for (ci, x) in newx.into_iter().enumerate() {
        ln.chunks[ci].x = x;
    }
    ln.spec.anchor = newanch;
    ln.spec.continue_x = false;
    ln.spec.sprl = false;
    ln.spec.x = ln.chunks.iter().map(|c| Some(c.x)).collect();
}

/// P:3505–3529: after merges (and with the final anchor set), move the chunk so the anchor of its
/// non-space characters is back where the parsed positions had it.
pub fn fix_merged_position(pt: &mut ParsedText, li: usize, ci: usize) {
    let now = chunk_char_pts(pt, li, ci);
    let anfr = pt.lines[li].spec.anchor.anfr();
    let (mut omin, mut omax) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut nmin, mut nmax) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut any = false;
    for (wi, &c) in pt.lines[li].chunks[ci].chars.iter().enumerate() {
        if pt.chars[c].c == ' ' {
            continue;
        }
        let Some(p) = pt.parsed_ut.get(c).copied().flatten() else {
            continue;
        };
        any = true;
        omin = omin.min(p[0].x);
        omax = omax.max(p[3].x);
        nmin = nmin.min(now[wi][0].x);
        nmax = nmax.max(now[wi][3].x);
    }
    if !any {
        return;
    }
    let delta = (nmin * (1.0 - anfr) + nmax * anfr) - (omin * (1.0 - anfr) + omax * anfr);
    if delta.abs() > XY_TOL {
        pt.lines[li].chunks[ci].x -= delta;
    }
}
