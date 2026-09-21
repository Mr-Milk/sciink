//! Port of upstream `remove_kerning.py` (RK): the stage drivers that run over a `Vec<ParsedText>`
//! arena. Geometry comes from `text::layout`, model edits from `text::edit`; nothing here touches
//! the DOM except by returning `ClipUnion` records for `text::write`.

use std::collections::{HashMap, HashSet};

use crate::dom::Doc;

use super::edit::{ChunkRef, Incoming, WType, append_chunks, split_off};
use super::edit::{change_alignment, delete_char, fix_merged_position};
use super::layout::chunk_char_pts;
use super::layout::{angle_deg, chunk_scf, chunk_tfs, chunk_utfs};
use super::layout::{chunk_mch, chunk_spw, get_ut_pts};
use super::parse::ParsedText;
use super::parse::TChar;
use super::style::Anchor;
use super::table::CharTable;
use super::write::{ClipUnion, clip_of};
use crate::geom::intersects;
use kurbo::Rect;

pub const NUM_SPACES: f64 = 1.0;
pub const XTOLEXT: f64 = 0.6;
pub const YTOLEXT: f64 = 0.1;
pub const XTOLMKN: f64 = 1.5;
pub const XTOLMKP: f64 = 0.99;
pub const YTOLMK: f64 = 0.01;
pub const XTOLSPLIT: f64 = 0.5;
pub const SUBSUPER_THR: f64 = 0.99;
pub const SUBSUPER_YTHR: f64 = 1.0 / 3.0;
pub const FONTSIZE_THR: f64 = 0.01;

/// RK:659–670: strip, `−` → `-`, drop thousands separators, parse as a float. `countminus` makes a
/// lone `-` count as a number. (Python's `float` also accepts `_` separators; tick labels never carry them.)
pub fn isnumeric(s: &str, countminus: bool) -> bool {
    let t: String = s.trim().replace('−', "-").replace(',', "");
    if countminus && t == "-" {
        return true;
    }
    !t.is_empty() && t.parse::<f64>().is_ok()
}

/// RK:674–676: drop spaces, tabs, CR, LF.
pub fn wstrip(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\n' | '\t' | '\r'))
        .collect()
}

/// RK:679–685: merging `a` and `b` would put two spaces in a row.
pub fn twospaces(a: &str, b: &str) -> bool {
    a.ends_with("  ") || (a.ends_with(' ') && b.starts_with(' ')) || b.starts_with("  ")
}

/// RK:687–690: `(trailing spaces of a, leading spaces of b)`.
pub fn trailing_leading(a: &str, b: &str) -> (usize, usize) {
    (
        a.chars().rev().take_while(|&c| c == ' ').count(),
        b.chars().take_while(|&c| c == ' ').count(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeType {
    Same,
    Sub,
    Super,
    SubReturn,
    SuperReturn,
}

/// A possible merge of chunk `to` after the chunk that owns the candidate list; `br1x`/`bl2x` are
/// the pen positions the choice is scored on (RK:539–544).
#[derive(Debug, Clone, Copy)]
pub struct Cand {
    pub to: ChunkRef,
    pub mtype: MergeType,
    pub br1x: f64,
    pub bl2x: f64,
}

/// RK:566–611: walk a chain's merge types through the normal/sub/super state machine; `None` when
/// the chain is inconsistent (upstream "bail") and must be dropped. Element `i+1` of the result is
/// the type of the chain's `i`-th chunk; element 0 is the head (`Normal`).
pub fn merge_wtypes(chain: &[(ChunkRef, MergeType)]) -> Option<Vec<WType>> {
    let mut ctype = WType::Normal;
    let mut out = vec![ctype];
    for (_, mt) in chain {
        ctype = match (ctype, mt) {
            (WType::Normal, MergeType::Same) => WType::Normal,
            (WType::Normal, MergeType::Sub) => WType::Sub,
            (WType::Normal, MergeType::Super) => WType::Super,
            (WType::Super, MergeType::Same) => WType::Super,
            (WType::Super, MergeType::SuperReturn) => WType::Normal,
            (WType::Sub, MergeType::Same) => WType::Sub,
            (WType::Sub, MergeType::SubReturn) => WType::Normal,
            _ => return None,
        };
        out.push(ctype);
    }
    Some(out)
}

/// RK:535–656. Each chunk keeps its closest candidate; chains are followed from every unmerged
/// head; chains with an inconsistent sub/super sequence are dropped; the rest are executed with
/// `append_chunks`, capping inserted spaces at 0 when the head already ends in a space, when the
/// merged chunk is a sub/superscript, or (manual-kerning mode) when the combined text has a space
/// and the merged chunk comes from the same style node as its predecessor. Merges that pull text
/// from other elements record a `ClipUnion`.
pub fn perform_merges(
    doc: &Doc,
    pts: &mut [ParsedText],
    ct: &mut CharTable,
    cands: &[(ChunkRef, Vec<Cand>)],
    mk: bool,
    clips: &mut Vec<ClipUnion>,
) {
    // 1. best candidate per chunk: the one whose start pen is nearest the head's end pen
    let mut link: HashMap<ChunkRef, (ChunkRef, MergeType)> = HashMap::new();
    for (w, cs) in cands {
        let mut best: Option<(f64, &Cand)> = None;
        for c in cs {
            let d = (c.bl2x - c.br1x).abs();
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, c));
            }
        }
        if let Some((_, c)) = best {
            link.insert(*w, (c.to, c.mtype));
        }
    }
    // 2. chains from unmerged heads, following the links transitively (RK:553–564)
    let mut merged: HashSet<ChunkRef> = HashSet::new();
    let mut chains: Vec<(ChunkRef, Vec<(ChunkRef, MergeType)>)> = Vec::new();
    for (w, _) in cands {
        if merged.contains(w) {
            continue;
        }
        let Some(&(mut next, mut mt)) = link.get(w) else {
            continue;
        };
        let mut seen: HashSet<ChunkRef> = HashSet::from([*w]);
        let mut chain = Vec::new();
        loop {
            if !seen.insert(next) {
                break; // a cycle cannot arise geometrically; upstream would spin forever
            }
            merged.insert(next);
            chain.push((next, mt));
            match link.get(&next) {
                Some(&(n2, m2)) => {
                    next = n2;
                    mt = m2;
                }
                None => break,
            }
        }
        chains.push((*w, chain));
    }
    // 3.+4. plan and execute (RK:566–656)
    for (w, chain) in chains {
        if merged.contains(&w) {
            continue; // became part of an earlier head's chain
        }
        let Some(types) = merge_wtypes(&chain) else {
            continue;
        };
        let Some((wli, wci)) = pts[w.0].find_chunk(w.1) else {
            continue;
        };
        let wtxt = pts[w.0].chunk_text(wli, wci);
        let mut alltxt = wtxt.clone();
        for (c, _) in &chain {
            if let Some((l, k)) = pts[c.0].find_chunk(c.1) {
                alltxt.push_str(&pts[c.0].chunk_text(l, k));
            }
        }
        let hasspaces = alltxt.contains(' ');
        let mut incoming = Vec::new();
        let mut mels = Vec::new();
        for (i, (mrg, _)) in chain.iter().enumerate() {
            let Some((li, ci)) = pts[mrg.0].find_chunk(mrg.1) else {
                continue; // already merged elsewhere (RK:623–624)
            };
            let mut max_spaces = None;
            if mk && hasspaces && pts[mrg.0].lines[li].chunks[ci].prev_same_tspan {
                max_spaces = Some(0);
            }
            if wtxt.ends_with(' ') || matches!(types[i + 1], WType::Super | WType::Sub) {
                max_spaces = Some(0);
            }
            if !mels.contains(&pts[mrg.0].el) {
                mels.push(pts[mrg.0].el);
            }
            incoming.push(Incoming {
                chunk: *mrg,
                wtype: types[i + 1],
                max_spaces,
            });
        }
        if incoming.is_empty() {
            continue;
        }
        append_chunks(doc, pts, ct, w, &incoming);
        let target = pts[w.0].el;
        let others: Vec<_> = mels.into_iter().filter(|&e| e != target).collect();
        // RK:640–656 runs after every cross-element merge, but when no participant carries a
        // clip its only action — dropping the target's clip — is a no-op, so nothing is
        // recorded. **Deviation:** a dangling `clip-path` reference on an otherwise unclipped
        // set is left alone (upstream would clear the target's).
        if !others.is_empty()
            && std::iter::once(target)
                .chain(others.iter().copied())
                .any(|e| clip_of(doc, e).is_some())
        {
            clips.push(ClipUnion { target, others });
        }
    }
}

/// Stage 6 (RK:318–376): every chunk considers merging its `next` chunk on the same baseline —
/// valid when the next chunk's start pen lies within `[br1 − 1.5·spw, br1 + dx + 0.99·spw]` and
/// within `0.01·mch` vertically, `dx = spw·(1 − trailing − leading spaces)` (0 when both texts are
/// numbers). A lone `" "` chunk that fails re-tests against its own predecessor (a weirdly kerned
/// space). After the merges, every chunk that is not the first of its line is split into its own
/// element (RK:360–376).
pub fn remove_manual_kerning(
    doc: &Doc,
    pts: &mut Vec<ParsedText>,
    ct: &mut CharTable,
    clips: &mut Vec<ClipUnion>,
) {
    let n0 = pts.len();
    let mut cands: Vec<(ChunkRef, Vec<Cand>)> = Vec::new();
    for (pi, pt) in pts.iter().enumerate().take(n0) {
        for (li, ci) in pt.chunks() {
            let w = &pt.lines[li].chunks[ci];
            let mut mw = Vec::new();
            if let Some((l2, c2)) = w.next.and_then(|nid| pt.find_chunk(nid)) {
                let nid = pt.lines[l2].chunks[c2].id;
                let (wtxt, w2txt) = (pt.chunk_text(li, ci), pt.chunk_text(l2, c2));
                if !twospaces(&wtxt, &w2txt) {
                    if let Some([_, br1, _, bl2]) = get_ut_pts(pt, (li, ci), pt, (l2, c2), true) {
                        let (trl, ldg) = trailing_leading(&wtxt, &w2txt);
                        let spw = chunk_spw(pt, li, ci);
                        let mut dx = spw * (NUM_SPACES - trl as f64 - ldg as f64);
                        let xtoln = XTOLMKN * spw;
                        let xtolp = XTOLMKP * spw;
                        let ytol = YTOLMK * chunk_mch(pt, li, ci);
                        if isnumeric(&wtxt, false) && isnumeric(&w2txt, true) {
                            dx = 0.0;
                        }
                        let mut valid = br1.x - xtoln <= bl2.x
                            && bl2.x <= br1.x + dx + xtolp
                            && br1.y - ytol <= bl2.y
                            && bl2.y <= br1.y + ytol;
                        if wtxt == " " && !valid {
                            if let Some((lp, cp)) = w.prev.and_then(|pid| pt.find_chunk(pid)) {
                                if let Some([_, br1p, _, bl2p]) =
                                    get_ut_pts(pt, (lp, cp), pt, (l2, c2), true)
                                {
                                    let dx = spw * (NUM_SPACES - trl as f64 - ldg as f64 + 1.0);
                                    valid =
                                        br1p.x - xtoln <= bl2p.x && bl2p.x <= br1p.x + dx + xtolp;
                                }
                            }
                        }
                        if valid {
                            mw.push(Cand {
                                to: (pi, nid),
                                mtype: MergeType::Same,
                                br1x: br1.x,
                                bl2x: bl2.x,
                            });
                        }
                    }
                }
            }
            cands.push(((pi, w.id), mw));
        }
    }
    perform_merges(doc, pts, ct, &cands, true, clips);
    for pi in 0..n0 {
        let lists: Vec<Vec<usize>> = pts[pi]
            .lines
            .iter()
            .flat_map(|ln| ln.chunks.iter().skip(1).map(|ch| ch.chars.clone()))
            .collect();
        if !lists.is_empty() {
            split_off(pts, pi, &lists);
        }
    }
}

/// The weight of the face that actually renders `c` (upstream `tsty['font-weight']`).
fn char_weight(ct: &CharTable, c: &TChar) -> u16 {
    c.face
        .map(|f| ct.fonts.face_info(f).weight)
        .unwrap_or(c.spec.weight)
}

/// Stage 7 (RK:382–532): every pair of chunks (any elements) with the same rotation whose
/// bounding boxes come within `spw·scf·1.6` of each other is tested in the first chunk's frame:
/// the second chunk's start pen must lie within `[br1 − 0.6·spw, br1 + dx + 0.6·spw]`, neither
/// text may be blank and merging may not create a double space. Then: same baseline (±0.1·mch)
/// and same transformed size (±1 %) → `Same` (numbers only when the gap is < 0.25 spaces);
/// otherwise a smaller chunk starting above 1/3 of the cap height → `Super`, a bigger one →
/// `SubReturn`; a smaller chunk whose cap top sits below 1/3 → `Sub`, a bigger one →
/// `SuperReturn`. Sub/superscripts need equal font weights and never attach to a "(a)" label.
pub fn external_merges(
    doc: &Doc,
    pts: &mut [ParsedText],
    ct: &mut CharTable,
    merge_nearby: bool,
    merge_supersub: bool,
    clips: &mut Vec<ClipUnion>,
) {
    struct Info {
        r: ChunkRef,
        li: usize,
        ci: usize,
        bb: Rect,
        bb_big: Rect,
        angle: f64,
    }
    let mut chks: Vec<Info> = Vec::new();
    for (pi, pt) in pts.iter().enumerate() {
        for (li, ci) in pt.chunks() {
            let corners: Vec<kurbo::Point> = pt.lines[li].chunks[ci]
                .chars
                .iter()
                .filter_map(|&c| pt.parsed_t.get(c).copied().flatten())
                .flatten()
                .collect();
            let Some(first) = corners.first() else {
                continue;
            };
            let bb = corners
                .iter()
                .fold(Rect::from_points(*first, *first), |r, p| r.union_pt(*p));
            let dx = chunk_spw(pt, li, ci) * chunk_scf(pt, li, ci) * (NUM_SPACES + XTOLEXT);
            chks.push(Info {
                r: (pi, pt.lines[li].chunks[ci].id),
                li,
                ci,
                bb,
                bb_big: bb.inflate(dx, dx),
                angle: angle_deg(pt.transform),
            });
        }
    }
    let mut cands: Vec<(ChunkRef, Vec<Cand>)> = Vec::with_capacity(chks.len());
    for (i, w) in chks.iter().enumerate() {
        let pw = &pts[w.r.0];
        let wtxt = pw.chunk_text(w.li, w.ci);
        let spw = chunk_spw(pw, w.li, w.ci);
        let mch = chunk_mch(pw, w.li, w.ci);
        let size = |p: &ParsedText, li: usize, ci: usize| -> (f64, f64) {
            let [a, b, c, d, _, _] = p.transform.as_coeffs();
            let u = chunk_utfs(p, li, ci);
            (u * (a * a + b * b).sqrt(), u * (c * c + d * d).sqrt())
        };
        let w1fs = size(pw, w.li, w.ci);
        let wtfs = chunk_tfs(pw, w.li, w.ci);
        let w_last = &pw.chars[*pw.lines[w.li].chunks[w.ci].chars.last().expect("non-empty")];
        let letterinpar = {
            let cs: Vec<char> = wtxt.chars().collect();
            cs.len() == 3 && cs[0] == '(' && cs[2] == ')' && cs[1].is_ascii_alphabetic()
        };
        let mut mw = Vec::new();
        for (j, w2) in chks.iter().enumerate() {
            if i == j || (w.angle - w2.angle).abs() >= 0.001 || !intersects(w.bb_big, w2.bb) {
                continue;
            }
            let p2 = &pts[w2.r.0];
            let w2txt = p2.chunk_text(w2.li, w2.ci);
            let (trl, ldg) = trailing_leading(&wtxt, &w2txt);
            let dx = spw * (NUM_SPACES - trl as f64 - ldg as f64);
            let xtol = XTOLEXT * spw;
            let ytol = YTOLEXT * mch;
            let Some([tr1, br1, tl2, bl2]) = get_ut_pts(pw, (w.li, w.ci), p2, (w2.li, w2.ci), true)
            else {
                continue;
            };
            let xpen = br1.x - xtol <= bl2.x && bl2.x <= br1.x + dx + xtol;
            let neither_empty = !wstrip(&wtxt).is_empty() && !wstrip(&w2txt).is_empty();
            if !(xpen && neither_empty && !twospaces(&wtxt, &w2txt)) {
                continue;
            }
            let w2_first = &p2.chars[p2.lines[w2.li].chunks[w2.ci].chars[0]];
            let weight_match = char_weight(ct, w_last) == char_weight(ct, w2_first);
            let w2fs = size(p2, w2.li, w2.ci);
            let w2tfs = chunk_tfs(p2, w2.li, w2.ci);
            let mut mtype = None;
            if (bl2.y - br1.y).abs() < ytol
                && (w1fs.0 - w2fs.0).abs() < FONTSIZE_THR * w1fs.0
                && (w1fs.1 - w2fs.1).abs() < FONTSIZE_THR * w1fs.1
                && merge_nearby
            {
                if isnumeric(&pw.line_text(w.li), false) && isnumeric(&p2.line_text(w2.li), true) {
                    if ((bl2.x - br1.x) / spw).abs() < 0.25 {
                        mtype = Some(MergeType::Same);
                    }
                } else {
                    mtype = Some(MergeType::Same);
                }
            } else if br1.y + ytol >= bl2.y
                && bl2.y >= tr1.y - ytol
                && merge_supersub
                && weight_match
                && !letterinpar
            {
                let aboveline =
                    br1.y * (1.0 - SUBSUPER_YTHR) + tr1.y * SUBSUPER_YTHR + ytol >= bl2.y;
                if w2tfs < wtfs * SUBSUPER_THR {
                    if aboveline {
                        mtype = Some(MergeType::Super);
                    }
                } else if wtfs < w2tfs * SUBSUPER_THR {
                    mtype = Some(MergeType::SubReturn);
                }
            } else if br1.y + ytol >= tl2.y
                && tl2.y >= tr1.y - ytol
                && merge_supersub
                && weight_match
                && !letterinpar
            {
                let belowline =
                    tl2.y >= br1.y * SUBSUPER_YTHR + tr1.y * (1.0 - SUBSUPER_YTHR) - ytol;
                if w2tfs < wtfs * SUBSUPER_THR {
                    if belowline {
                        mtype = Some(MergeType::Sub);
                    }
                } else if wtfs < w2tfs * SUBSUPER_THR {
                    mtype = Some(MergeType::SuperReturn);
                }
            }
            if let Some(m) = mtype {
                mw.push(Cand {
                    to: w2.r,
                    mtype: m,
                    br1x: br1.x,
                    bl2x: bl2.x,
                });
            }
        }
        cands.push((w.r, mw));
    }
    perform_merges(doc, pts, ct, &cands, false, clips);
}

/// RK:205–253: within each line, sort the chunks by x and split before a chunk whose start pen is
/// more than one space (+0.5 tolerance, minus existing spaces) past the previous chunk's end pen,
/// judged on CURRENT positions. Each split range becomes its own element.
pub fn split_distant_chunks(pts: &mut Vec<ParsedText>) {
    let n0 = pts.len();
    for pi in 0..n0 {
        for li in 0..pts[pi].lines.len() {
            let n = pts[pi].lines[li].chunks.len();
            if n < 2 {
                continue;
            }
            let mut sws: Vec<usize> = (0..n).collect();
            sws.sort_by(|&a, &b| {
                let (xa, xb) = (pts[pi].lines[li].chunks[a].x, pts[pi].lines[li].chunks[b].x);
                xa.partial_cmp(&xb).unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut splits: Vec<usize> = Vec::new();
            for ii in 1..n {
                let (a, b) = (sws[ii - 1], sws[ii]);
                let pt = &pts[pi];
                let (wtxt, w2txt) = (pt.chunk_text(li, a), pt.chunk_text(li, b));
                let (trl, ldg) = trailing_leading(&wtxt, &w2txt);
                let spw = chunk_spw(pt, li, a);
                let dx = spw * (NUM_SPACES - trl as f64 - ldg as f64);
                let xtol = XTOLSPLIT * spw;
                if let Some([_, br1, _, bl2]) = get_ut_pts(pt, (li, a), pt, (li, b), false) {
                    if bl2.x > br1.x + dx + xtol {
                        splits.push(ii);
                    }
                }
            }
            if splits.is_empty() {
                continue;
            }
            let mut lists: Vec<Vec<usize>> = Vec::new();
            for k in 0..splits.len() {
                let (sstart, sstop) = (splits[k], splits.get(k + 1).copied().unwrap_or(n));
                lists.push(
                    sws[sstart..sstop]
                        .iter()
                        .flat_map(|&ci| pts[pi].lines[li].chunks[ci].chars.clone())
                        .collect(),
                );
            }
            split_off(pts, pi, &lists);
        }
    }
}

/// RK:257–315 (skipped for multi-line Inkscape text and flows): within each chunk, characters in
/// x order; compare each to the last non-space one and split when the gap exceeds one space
/// (+0.5), or when a space/hyphen separates two numbers in the same text node (tick labels).
/// Upstream slices the chunk text by the SORTED index — kept as is.
pub fn split_distant_intrachunk(pts: &mut Vec<ParsedText>) {
    let n0 = pts.len();
    for pi in 0..n0 {
        if pts[pi].is_ml_inkscape || pts[pi].is_flow {
            continue;
        }
        let ids: Vec<u32> = pts[pi]
            .chunks()
            .map(|(l, c)| pts[pi].chunk(l, c).id)
            .collect();
        for cid in ids {
            let Some((li, ci)) = pts[pi].find_chunk(cid) else {
                continue;
            };
            let lists = {
                let pt = &pts[pi];
                let ch = &pt.lines[li].chunks[ci];
                let now = chunk_char_pts(pt, li, ci);
                let mut order: Vec<usize> = (0..ch.chars.len()).collect();
                order.sort_by(|&a, &b| {
                    now[a][0]
                        .x
                        .partial_cmp(&now[b][0].x)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let txt: Vec<char> = ch.chars.iter().map(|&c| pt.chars[c].c).collect();
                let spw = chunk_spw(pt, li, ci);
                let (dx, xtol) = (spw * NUM_SPACES, XTOLSPLIT * spw);
                let is_space = |c: char| matches!(c, ' ' | '\u{a0}');
                let mut lastnspc: Option<usize> = (!is_space(txt[order[0]])).then_some(order[0]);
                let mut splitiis: Vec<usize> = Vec::new();
                let mut prevsplit = 0usize;
                for ii in 1..order.len() {
                    if let Some(cw) = lastnspc {
                        let c2w = order[ii];
                        let rest: String = txt[ii..].iter().collect();
                        let remaining_numeric = rest
                            .split([' ', '-', '−'])
                            .find(|s| !s.is_empty())
                            .is_some_and(|s| isnumeric(s, false));
                        let seg: String = txt[prevsplit..ii].iter().collect();
                        let (c, c2) = (&pt.chars[ch.chars[cw]], &pt.chars[ch.chars[c2w]]);
                        let numbersplit = isnumeric(&seg, false)
                            && matches!(c2.c, ' ' | '-' | '−')
                            && remaining_numeric
                            && c.loc.node == c2.loc.node;
                        if now[c2w][0].x > now[cw][3].x + dx + xtol || numbersplit {
                            splitiis.push(ii);
                            prevsplit = ii;
                        }
                    }
                    if !is_space(txt[order[ii]]) {
                        lastnspc = Some(order[ii]);
                    }
                }
                let mut lists: Vec<Vec<usize>> = Vec::new();
                for k in 0..splitiis.len() {
                    let (sstart, sstop) = (
                        splitiis[k],
                        splitiis.get(k + 1).copied().unwrap_or(order.len()),
                    );
                    let sel: HashSet<usize> = order[sstart..sstop].iter().copied().collect();
                    lists.push(
                        ch.chars
                            .iter()
                            .enumerate()
                            .filter(|(w, _)| sel.contains(w))
                            .map(|(_, &c)| c)
                            .collect(),
                    );
                }
                lists
            };
            if !lists.is_empty() {
                split_off(pts, pi, &lists);
            }
        }
    }
}

/// RK:183–201: every line after the first becomes its own element (not for multi-line Inkscape
/// text or flows).
pub fn split_lines(pts: &mut Vec<ParsedText>) {
    let n0 = pts.len();
    for pi in 0..n0 {
        let pt = &pts[pi];
        if pt.lines.len() < 2 || pt.is_ml_inkscape || pt.is_flow {
            continue;
        }
        let lists: Vec<Vec<usize>> = (1..pt.lines.len())
            .map(|li| pt.lines[li].chars.clone())
            .collect();
        split_off(pts, pi, &lists);
    }
}

/// Stage 9 (RK:167–179): re-anchor every line of every element (not multi-line Inkscape text or
/// flows) and remember to write `text-anchor`/`text-align` on the `<text>` itself.
pub fn change_justification(pts: &mut [ParsedText], j: Option<Anchor>) {
    let Some(a) = j else {
        return;
    };
    for pt in pts.iter_mut() {
        if pt.is_ml_inkscape || pt.is_flow {
            continue;
        }
        for li in 0..pt.lines.len() {
            change_alignment(pt, li, a);
        }
        pt.text_anchor_override = Some(a);
    }
}

/// Stage 10 (RK:139–159): delete trailing, then leading `' '` characters of every line (not
/// multi-line Inkscape text or flows). Returns whether anything was removed.
pub fn remove_trailing_leading_spaces(pts: &mut [ParsedText]) -> bool {
    let mut removed = false;
    for pt in pts.iter_mut() {
        if pt.is_ml_inkscape || pt.is_flow {
            continue;
        }
        let mut li = 0;
        while li < pt.lines.len() {
            let n_before = pt.lines.len();
            while let Some(&last) = pt.lines.get(li).and_then(|l| l.chars.last()) {
                if pt.chars[last].c != ' ' || pt.lines.len() < n_before {
                    break;
                }
                delete_char(pt, last);
                removed = true;
            }
            while let Some(&first) = pt.lines.get(li).and_then(|l| l.chars.first()) {
                if pt.chars[first].c != ' ' || pt.lines.len() < n_before {
                    break;
                }
                delete_char(pt, first);
                removed = true;
            }
            if pt.lines.len() == n_before {
                li += 1; // otherwise the line vanished and `li` already names the next one
            }
        }
    }
    removed
}

/// Stage 11 (RK:132–136).
pub fn fix_merge_positions(pts: &mut [ParsedText]) {
    for pt in pts.iter_mut() {
        for li in 0..pt.lines.len() {
            for ci in 0..pt.lines[li].chunks.len() {
                fix_merged_position(pt, li, ci);
            }
        }
    }
}
