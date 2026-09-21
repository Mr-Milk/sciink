//! Port of upstream `remove_kerning.py` (RK): the stage drivers that run over a `Vec<ParsedText>`
//! arena. Geometry comes from `text::layout`, model edits from `text::edit`; nothing here touches
//! the DOM except by returning `ClipUnion` records for `text::write`.

use std::collections::{HashMap, HashSet};

use crate::dom::Doc;

use super::edit::{ChunkRef, Incoming, WType, append_chunks, split_off};
use super::layout::{chunk_mch, chunk_spw, get_ut_pts};
use super::parse::ParsedText;
use super::table::CharTable;
use super::write::ClipUnion;

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
        if !others.is_empty() {
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
            .flat_map(|ln| ln.chunks.iter().skip(1).rev().map(|ch| ch.chars.clone()))
            .collect();
        if !lists.is_empty() {
            split_off(pts, pi, &lists);
        }
    }
}
