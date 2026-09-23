//! Conservative spatial index for axis-aligned boxes, and the two Flattener sweeps built on it
//! (Plan 9 Task 9). `BoxGrid::query` visits a superset of the boxes overlapping the query and
//! never misses one; the callers keep `geom::intersects` / the tolerance test as ground truth.

use std::collections::HashMap;

use kurbo::Rect;

use super::intersects;

/// A box covering more than this many cells on an axis goes to the overflow list.
const MAX_SPAN: usize = 32;

fn finite(r: &Rect) -> bool {
    r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
}

/// Uniform bucket grid over a fixed extent. Ids may be visited more than once by `query`.
pub struct BoxGrid {
    x0: f64,
    y0: f64,
    cw: f64,
    ch: f64,
    cols: usize,
    rows: usize,
    cells: Vec<Vec<u32>>,
    /// Boxes spanning too many cells, or not finite: checked on every query.
    large: Vec<u32>,
    /// Every inserted id, for queries that span too many cells.
    all: Vec<u32>,
}

impl BoxGrid {
    /// `extent` must contain every finite box inserted or queried; `n` sizes the grid
    /// (`clamp(ceil(sqrt(n)), 1, 512)` cells per axis; a zero-extent axis collapses to one cell).
    pub fn new(extent: Option<Rect>, n: usize) -> BoxGrid {
        let side = ((n as f64).sqrt().ceil() as usize).clamp(1, 512);
        let (x0, y0, w, h) = match extent {
            Some(r) if finite(&r) => (r.x0, r.y0, r.width(), r.height()),
            _ => (0.0, 0.0, 0.0, 0.0),
        };
        let cols = if w > 0.0 { side } else { 1 };
        let rows = if h > 0.0 { side } else { 1 };
        BoxGrid {
            x0,
            y0,
            cw: if w > 0.0 { w / cols as f64 } else { 1.0 },
            ch: if h > 0.0 { h / rows as f64 } else { 1.0 },
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
            large: Vec::new(),
            all: Vec::new(),
        }
    }

    /// Cell range of `r`, or `None` when it is not finite or spans more than `MAX_SPAN` cells.
    fn span(&self, r: Rect) -> Option<(usize, usize, usize, usize)> {
        if !finite(&r) {
            return None;
        }
        let col = |x: f64| (((x - self.x0) / self.cw).floor().max(0.0) as usize).min(self.cols - 1);
        let row = |y: f64| (((y - self.y0) / self.ch).floor().max(0.0) as usize).min(self.rows - 1);
        let (c0, c1) = (col(r.x0.min(r.x1)), col(r.x0.max(r.x1)));
        let (r0, r1) = (row(r.y0.min(r.y1)), row(r.y0.max(r.y1)));
        if c1 - c0 + 1 > MAX_SPAN || r1 - r0 + 1 > MAX_SPAN {
            return None;
        }
        Some((c0, c1, r0, r1))
    }

    pub fn insert(&mut self, id: u32, r: Rect) {
        self.all.push(id);
        match self.span(r) {
            Some((c0, c1, r0, r1)) => {
                for row in r0..=r1 {
                    for col in c0..=c1 {
                        self.cells[row * self.cols + col].push(id);
                    }
                }
            }
            None => self.large.push(id),
        }
    }

    /// Calls `f` for every candidate (a superset of the boxes overlapping `r`); stops when `f`
    /// returns `false`. Two finite boxes that overlap share a point, and that point maps to the
    /// same cell for both (clamping is monotone), so a gridded box is always found; overflow boxes
    /// are always visited; an oversize query falls back to every id.
    pub fn query(&self, r: Rect, f: &mut dyn FnMut(u32) -> bool) {
        let Some((c0, c1, r0, r1)) = self.span(r) else {
            for &id in &self.all {
                if !f(id) {
                    return;
                }
            }
            return;
        };
        for &id in &self.large {
            if !f(id) {
                return;
            }
        }
        for row in r0..=r1 {
            for col in c0..=c1 {
                for &id in &self.cells[row * self.cols + col] {
                    if !f(id) {
                        return;
                    }
                }
            }
        }
    }
}

/// The Flattener's duplicate pass (F:422–497) as a pure index walk. Visits exactly the pairs
/// `(ii, jj)`, `ii < jj`, whose boxes are equal within `1e-6 · max(size_ii, size_jj)` on every
/// coordinate (`size = max(width, height)`, degenerate boxes never equal), in upstream's order —
/// `jj` descending, `ii` ascending within each `jj`, skipping an `ii` already removed — and asks
/// `is_dup(ii, jj)`; an `ii` for which it returns `true` is removed. Returns the removals in order.
///
/// Candidates come from a 4-D bucket index with cell `2 · 1e-6 · maxsize`, `maxsize` being the
/// largest size among the *bucketable* boxes (see below): if `equal(i, j)` is decided by a finite
/// pairwise tolerance, that tolerance is `1e-6 · size(i).max(size(j)) ≤ 1e-6 · maxsize = cell / 2`,
/// so the floor quotients differ by at most one and `key(i)` lies in the 3⁴ neighbourhood of
/// `key(j)` — no such pair is missed. Saturating casts or one giant (but finite) box only add
/// candidates; `equal` still decides.
///
/// A box with a non-finite coordinate has no meaningful bucket key (`floor` of `inf`/`NaN` is not
/// a useful cell), so it is never bucketed — but it cannot simply be dropped either. A `NaN`
/// coordinate makes `equal` false against everything (the difference is `NaN`, and `NaN <= tol`
/// is always false), so dropping it changes nothing. An infinite *width or height* is different:
/// `size` for that box is infinite too (unlike `NaN`, `f64::max` does not ignore a real infinity),
/// so `equal` computes an infinite `tol` for any pairing that involves it and can be true against
/// almost any other box, independent of `maxsize`. Such a box (`overflow`) is therefore always a
/// candidate in both roles: added to every bucketable `jj`'s candidates, and — having no key to
/// search neighbours from — matched against literally every `ii < jj` when it is itself `jj`.
/// `equal` remains the sole arbiter throughout, so this only ever adds candidates, never misses one.
pub fn duplicate_scan(boxes: &[Rect], is_dup: &mut dyn FnMut(usize, usize) -> bool) -> Vec<usize> {
    let dead = |r: &Rect| r.width() == 0.0 || r.height() == 0.0;
    let bucketable = |r: &Rect| finite(r) && !dead(r);
    let size = |r: &Rect| r.width().max(r.height());
    let equal = |i: usize, j: usize| -> bool {
        let (a, b) = (&boxes[i], &boxes[j]);
        if a.width() == 0.0 || a.height() == 0.0 || b.width() == 0.0 || b.height() == 0.0 {
            return false;
        }
        let tol = 1e-6 * size(a).max(size(b));
        (a.x0 - b.x0).abs() <= tol
            && (a.y0 - b.y0).abs() <= tol
            && (a.x1 - b.x1).abs() <= tol
            && (a.y1 - b.y1).abs() <= tol
    };
    let mut maxsize = 0.0_f64;
    let mut overflow: Vec<u32> = Vec::new();
    for (i, r) in boxes.iter().enumerate() {
        if bucketable(r) {
            maxsize = maxsize.max(size(r));
        } else if !dead(r) {
            overflow.push(i as u32);
        }
    }
    let mut removed = Vec::new();
    if maxsize <= 0.0 && overflow.is_empty() {
        return removed;
    }
    let cell = 2.0 * 1e-6 * maxsize;
    let key = |r: &Rect| -> [i64; 4] {
        [
            (r.x0 / cell).floor() as i64,
            (r.y0 / cell).floor() as i64,
            (r.x1 / cell).floor() as i64,
            (r.y1 / cell).floor() as i64,
        ]
    };
    let mut buckets: HashMap<[i64; 4], Vec<u32>> = HashMap::new();
    for (i, r) in boxes.iter().enumerate() {
        if bucketable(r) {
            buckets.entry(key(r)).or_default().push(i as u32);
        }
    }
    let mut gone = vec![false; boxes.len()];
    let mut cands: Vec<u32> = Vec::new();
    for jj in (0..boxes.len()).rev() {
        if dead(&boxes[jj]) {
            continue;
        }
        cands.clear();
        if bucketable(&boxes[jj]) {
            let k = key(&boxes[jj]);
            for d0 in -1..=1 {
                for d1 in -1..=1 {
                    for d2 in -1..=1 {
                        for d3 in -1..=1 {
                            let nk = [
                                k[0].saturating_add(d0),
                                k[1].saturating_add(d1),
                                k[2].saturating_add(d2),
                                k[3].saturating_add(d3),
                            ];
                            if let Some(v) = buckets.get(&nk) {
                                cands.extend(v.iter().copied().filter(|&i| (i as usize) < jj));
                            }
                        }
                    }
                }
            }
            cands.extend(overflow.iter().copied().filter(|&i| (i as usize) < jj));
        } else {
            // No bucket key of its own: every earlier box is a candidate (still just a superset —
            // `equal` decides).
            cands.extend(0..jj as u32);
        }
        cands.sort_unstable();
        cands.dedup();
        for &ii in &cands {
            let ii = ii as usize;
            if gone[ii] || !equal(ii, jj) {
                continue;
            }
            if is_dup(ii, jj) {
                gone[ii] = true;
                removed.push(ii);
            }
        }
    }
    removed
}

/// The Flattener's white-rectangle pass (F:499–509) as a pure index walk: the positions `ii` with
/// `white[ii]` that no EARLIER, still-alive box strictly intersects (`geom::intersects`); a
/// returned position is dead for the positions after it. Returns the deletions in ascending order.
pub fn background_scan(boxes: &[Rect], white: &[bool]) -> Vec<usize> {
    let extent = boxes
        .iter()
        .filter(|r| finite(r))
        .copied()
        .reduce(|a, b| a.union(b));
    let mut grid = BoxGrid::new(extent, boxes.len());
    let mut deleted = Vec::new();
    for ii in 0..boxes.len() {
        if white[ii] {
            let wb = boxes[ii];
            let mut behind = false;
            grid.query(wb, &mut |k| {
                if intersects(boxes[k as usize], wb) {
                    behind = true;
                    false
                } else {
                    true
                }
            });
            if !behind {
                deleted.push(ii);
                continue; // never inserted: it is gone for every later position
            }
        }
        grid.insert(ii as u32, boxes[ii]);
    }
    deleted
}
