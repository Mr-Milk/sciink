//! Character positions and extents (spec §A.1 "Geometry"; upstream parser.py:3560–3651, 4202–4210, 1701–1793).

use kurbo::{Affine, Point, Rect};

use crate::geom::union;

use super::parse::{ParsedText, TChar};

pub struct ChunkGeom {
    pub left: Vec<f64>,
    pub right: Vec<f64>,
    pub base: Vec<f64>,
    pub top: Vec<f64>,
    pub pts_ut: [Point; 4],
}

/// Last char of a multi-char chunk is the line's last char and a space → not rendered (non-flow rule).
pub fn unrendered_space(pt: &ParsedText, li: usize, ci: usize) -> bool {
    let ln = &pt.lines[li];
    let ch = &ln.chunks[ci];
    let Some(&last) = ch.chars.last() else {
        return false;
    };
    ch.chars.len() > 1
        && ln.chars.last() == Some(&last)
        && matches!(pt.chars[last].c, ' ' | '\u{A0}')
}

fn dadv(prev: &TChar, cur: &TChar) -> f64 {
    if prev.loc.node == cur.loc.node && prev.loc.tail == cur.loc.tail {
        cur.prop.dadvs.get(&prev.c).copied().unwrap_or(0.0) * cur.utfs
    } else {
        0.0
    }
}

pub fn chunk_geom(pt: &ParsedText, li: usize, ci: usize) -> ChunkGeom {
    let ln = &pt.lines[li];
    let ch = &ln.chunks[ci];
    let cs: Vec<&TChar> = ch.chars.iter().map(|&i| &pt.chars[i]).collect();
    let n = cs.len();
    let anfr = ln.spec.anchor.anfr();
    let mut cstop = Vec::with_capacity(n);
    let mut acc = 0.0;
    for i in 0..n {
        let dx = cs[i].dx;
        let dxlsp = if i == 0 { 0.0 } else { cs[i - 1].lsp };
        let da = if i == 0 { 0.0 } else { dadv(cs[i - 1], cs[i]) };
        acc += cs[i].cwd + dx + dxlsp + if dx == 0.0 { da } else { 0.0 };
        cstop.push(acc);
    }
    let chkw = cstop[n - 1];
    let sum_dx: f64 = cs.iter().map(|c| c.dx).sum();
    let offx = -anfr
        * (chkw
            - if unrendered_space(pt, li, ci) {
                cs[n - 1].cwd
            } else {
                0.0
            }
            - if ln.spec.rtl { 2.0 * sum_dx } else { 0.0 });
    let left: Vec<f64> = (0..n)
        .map(|i| ch.x + (cstop[i] - cs[i].cwd) + offx)
        .collect();
    let right: Vec<f64> = (0..n).map(|i| ch.x + cstop[i] + offx).collect();
    let mut ady = 0.0;
    let base: Vec<f64> = cs
        .iter()
        .map(|c| {
            ady += c.dy;
            ch.y + ady - c.bshft
        })
        .collect();
    let top: Vec<f64> = base.iter().zip(&cs).map(|(b, c)| b - c.caph).collect();
    // dxlsp[0] is 0 by definition, so lx2 only needs to subtract dx[0] from each left.
    let lx2 = left
        .iter()
        .map(|l| l - cs[0].dx)
        .fold(f64::INFINITY, f64::min);
    let rx2 = lx2 + (right[n - 1] - left[0]);
    let by2 = base.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let ty2 = top.iter().copied().fold(f64::INFINITY, f64::min);
    ChunkGeom {
        left,
        right,
        base,
        top,
        pts_ut: [
            Point::new(lx2, by2),
            Point::new(lx2, ty2),
            Point::new(rx2, ty2),
            Point::new(rx2, by2),
        ],
    }
}

pub fn char_pts_ut(_pt: &ParsedText, g: &ChunkGeom, windex: usize) -> [Point; 4] {
    let (l, r, b, t) = (
        g.left[windex],
        g.right[windex],
        g.base[windex],
        g.top[windex],
    );
    [
        Point::new(l, b),
        Point::new(l, t),
        Point::new(r, t),
        Point::new(r, b),
    ]
}

pub fn char_pts_ink_ut(pt: &ParsedText, g: &ChunkGeom, ci: usize, windex: usize) -> [Point; 4] {
    let c = &pt.chars[ci];
    let [ix, iy, iw, ih] = c.prop.inkbb;
    let (w, h) = (iw * c.utfs, ih * c.utfs);
    let x = g.left[windex] + ix * c.utfs;
    let y = g.base[windex] + iy * c.utfs + h;
    [
        Point::new(x, y),
        Point::new(x, y - h),
        Point::new(x + w, y - h),
        Point::new(x + w, y),
    ]
}

pub fn transform_pts(t: Affine, p: [Point; 4]) -> [Point; 4] {
    [t * p[0], t * p[1], t * p[2], t * p[3]]
}

pub fn pts_bbox(p: &[Point; 4]) -> Rect {
    let xs = p.iter().map(|q| q.x);
    let ys = p.iter().map(|q| q.y);
    Rect::new(
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

fn each_char(pt: &ParsedText, mut f: impl FnMut(usize, &ChunkGeom, usize)) {
    for (li, ln) in pt.lines.iter().enumerate() {
        for (ci, ch) in ln.chunks.iter().enumerate() {
            let g = chunk_geom(pt, li, ci);
            for (wi, &c) in ch.chars.iter().enumerate() {
                f(c, &g, wi);
            }
        }
    }
}

/// One rectangle per character in `pt.chars` order (chars with a NaN baseline are skipped).
pub fn char_extents(pt: &ParsedText) -> Vec<Rect> {
    let mut out: Vec<(usize, Rect)> = Vec::new();
    each_char(pt, |c, g, wi| {
        let p = char_pts_ut(pt, g, wi);
        if !p[0].y.is_nan() {
            out.push((c, pts_bbox(&p)));
        }
    });
    out.sort_by_key(|(c, _)| *c);
    out.into_iter().map(|(_, r)| r).collect()
}

pub fn chunk_extents(pt: &ParsedText) -> Vec<Rect> {
    pt.chunks()
        .map(|(li, ci)| pts_bbox(&chunk_geom(pt, li, ci).pts_ut))
        .filter(|r| !r.y0.is_nan())
        .collect()
}

pub fn line_extents(pt: &ParsedText) -> Vec<Rect> {
    pt.lines
        .iter()
        .enumerate()
        .filter_map(|(li, ln)| {
            (0..ln.chunks.len()).fold(None, |acc, ci| {
                union(acc, Some(pts_bbox(&chunk_geom(pt, li, ci).pts_ut)))
            })
        })
        .collect()
}

pub fn full_extent(pt: &ParsedText) -> Option<Rect> {
    char_extents(pt)
        .into_iter()
        .fold(None, |acc, r| union(acc, Some(r)))
}

pub fn full_ink_bbox(pt: &ParsedText) -> Option<Rect> {
    let mut acc = None;
    each_char(pt, |c, g, wi| {
        let p = char_pts_ink_ut(pt, g, c, wi);
        if !p[0].y.is_nan() {
            acc = union(acc, Some(pts_bbox(&p)));
        }
    });
    acc
}

/// Bounding box in root coordinates: union over characters of the box of their transformed corners.
pub fn text_bbox(pt: &ParsedText) -> Option<Rect> {
    let mut acc = None;
    each_char(pt, |_, g, wi| {
        let p = transform_pts(pt.transform, char_pts_ut(pt, g, wi));
        if !p[0].y.is_nan() {
            acc = union(acc, Some(pts_bbox(&p)));
        }
    });
    acc
}

pub fn max_tfs(pt: &ParsedText) -> Option<f64> {
    pt.chars
        .iter()
        .map(|c| c.tfs)
        .fold(None, |m, v| Some(m.map_or(v, |m: f64| m.max(v))))
}
