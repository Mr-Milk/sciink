//! The sweep replacements for the Flattener's all-pairs box tests must reproduce the pairwise
//! reference (copied verbatim from the pre-Plan-9 code) on random and adversarial input.

use kurbo::Rect;
use sciink::geom::grid::{BoxGrid, background_scan, duplicate_scan};
use sciink::geom::intersects;

fn duplicate_scan_ref(boxes: &[Rect], is_dup: &mut dyn FnMut(usize, usize) -> bool) -> Vec<usize> {
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
    let mut removed = Vec::new();
    let mut gone = std::collections::HashSet::new();
    for jj in (0..boxes.len()).rev() {
        for ii in 0..jj {
            if gone.contains(&ii) || !equal(ii, jj) {
                continue;
            }
            if is_dup(ii, jj) {
                gone.insert(ii);
                removed.push(ii);
            }
        }
    }
    removed
}

fn background_scan_ref(boxes: &[Rect], white: &[bool]) -> Vec<usize> {
    let mut deleted: Vec<usize> = Vec::new();
    for ii in 0..boxes.len() {
        if !white[ii] {
            continue;
        }
        let wb = boxes[ii];
        let behind = (0..ii).any(|k| !deleted.contains(&k) && intersects(boxes[k], wb));
        if !behind {
            deleted.push(ii);
        }
    }
    deleted
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn lattice_box(&mut self) -> Rect {
        // corners on a coarse lattice so exact duplicates are common; a jitter of 1e-9 (far below
        // the 1e-6 · size tolerance) on some boxes exercises the near-equal path
        let x0 = self.below(20) as f64;
        let y0 = self.below(20) as f64;
        let w = 1.0 + self.below(5) as f64;
        let h = 1.0 + self.below(5) as f64;
        let j = if self.below(4) == 0 { 1e-9 } else { 0.0 };
        Rect::new(x0 + j, y0, x0 + w, y0 + h + j)
    }
}

fn pseudo_dup(ii: usize, jj: usize) -> bool {
    (ii * 7 + jj * 13) % 3 != 0
}

#[test]
fn duplicate_scan_matches_the_pairwise_reference_on_random_boxes() {
    let mut rng = Lcg(42);
    let mut cases_with_removals = 0;
    for case in 0..200 {
        let boxes: Vec<Rect> = (0..300).map(|_| rng.lattice_box()).collect();
        let a = duplicate_scan_ref(&boxes, &mut pseudo_dup);
        let b = duplicate_scan(&boxes, &mut pseudo_dup);
        assert_eq!(a, b, "case {case}: removal sequences differ");
        if !a.is_empty() {
            cases_with_removals += 1;
        }
    }
    // At seed 42, 12 of the 200 cases produce zero removals by construction — pseudo_dup rejects
    // every equal pair those lattice draws form — so this checks the aggregate, not each case.
    assert!(
        cases_with_removals >= 150,
        "most cases must exercise the removal feedback (188 of 200 at seed 42)"
    );
}

#[test]
fn duplicate_scan_matches_at_the_tolerance_edge() {
    let base = Rect::new(0.0, 0.0, 10.0, 10.0); // size 10 → tol 1e-5
    let tol = 1e-5;
    let mut boxes = vec![base];
    for d in [
        tol,
        tol * (1.0 + 1e-12),
        tol * (1.0 - 1e-12),
        -tol,
        2.0 * tol,
    ] {
        boxes.push(Rect::new(base.x0 + d, base.y0, base.x1 + d, base.y1));
        boxes.push(Rect::new(base.x0, base.y0 + d, base.x1, base.y1 + d));
        boxes.push(Rect::new(
            base.x0 + d,
            base.y0 + d,
            base.x1 + d,
            base.y1 + d,
        ));
        boxes.push(Rect::new(base.x0, base.y0, base.x1 + d, base.y1));
    }
    let all = |_: usize, _: usize| true;
    assert_eq!(
        duplicate_scan_ref(&boxes, &mut { all }),
        duplicate_scan(&boxes, &mut { all })
    );
}

#[test]
fn duplicate_scan_ignores_degenerate_and_non_finite_boxes() {
    let mut rng = Lcg(7);
    let mut boxes: Vec<Rect> = (0..100).map(|_| rng.lattice_box()).collect();
    boxes.push(Rect::new(1.0, 1.0, 1.0, 5.0)); // zero width
    boxes.push(Rect::new(1.0, 1.0, 5.0, 1.0)); // zero height
    boxes.push(Rect::new(f64::NAN, 0.0, 1.0, 1.0));
    boxes.push(Rect::new(0.0, f64::NEG_INFINITY, 1.0, f64::INFINITY));
    boxes.push(Rect::new(-1e6, -1e6, 1e6, 1e6)); // 10⁶× the rest: inflates the bucket size
    boxes.extend((0..100).map(|_| rng.lattice_box()));
    let a = duplicate_scan_ref(&boxes, &mut pseudo_dup);
    let b = duplicate_scan(&boxes, &mut pseudo_dup);
    assert_eq!(a, b);
}

#[test]
fn background_scan_matches_the_pairwise_reference_on_random_boxes() {
    let mut rng = Lcg(99);
    for case in 0..200 {
        let n = 200;
        let mut boxes: Vec<Rect> = (0..n).map(|_| rng.lattice_box()).collect();
        let mut white: Vec<bool> = (0..n).map(|_| rng.below(3) == 0).collect();
        match case % 5 {
            0 => white.iter_mut().for_each(|w| *w = true),
            1 => white.iter_mut().for_each(|w| *w = false),
            2 => boxes[0] = Rect::new(-100.0, -100.0, 100.0, 100.0), // one box covering everything
            3 => boxes[10] = Rect::new(3.0, 3.0, 3.0, 3.0),          // zero-size box inside others
            _ => {}
        }
        assert_eq!(
            background_scan_ref(&boxes, &white),
            background_scan(&boxes, &white),
            "case {case}"
        );
    }
}

#[test]
fn background_scan_respects_the_earlier_in_document_order_rule() {
    // w0 is deleted (nothing earlier); w1 overlaps only w0, which is gone → deleted too;
    // w2 overlaps the surviving path p → kept. Touching edges do not count (strict test).
    let boxes = vec![
        Rect::new(0.0, 0.0, 10.0, 10.0),   // w0
        Rect::new(5.0, 5.0, 15.0, 15.0),   // w1
        Rect::new(20.0, 0.0, 30.0, 10.0),  // p
        Rect::new(25.0, 5.0, 35.0, 15.0),  // w2
        Rect::new(30.0, 15.0, 40.0, 25.0), // w3: touches w2's corner only
    ];
    let white = vec![true, true, false, true, true];
    assert_eq!(background_scan(&boxes, &white), vec![0, 1, 4]);
    assert_eq!(background_scan_ref(&boxes, &white), vec![0, 1, 4]);
}

#[test]
fn box_grid_never_misses_an_overlap() {
    let mut rng = Lcg(2024);
    let boxes: Vec<Rect> = (0..2000).map(|_| rng.lattice_box()).collect();
    let extent = boxes.iter().copied().reduce(|a, b| a.union(b));
    let mut grid = BoxGrid::new(extent, boxes.len());
    for (i, r) in boxes.iter().enumerate() {
        grid.insert(i as u32, *r);
    }
    for _ in 0..10_000 {
        let q = rng.lattice_box();
        let mut seen = std::collections::HashSet::new();
        grid.query(q, &mut |id| {
            seen.insert(id);
            true
        });
        for (i, r) in boxes.iter().enumerate() {
            if intersects(*r, q) {
                assert!(
                    seen.contains(&(i as u32)),
                    "grid missed box {i} for query {q:?}"
                );
            }
        }
    }
}

#[test]
fn box_grid_overflow_and_fallback_paths_still_find_every_overlap() {
    let mut rng = Lcg(20260924);
    // side = ceil(sqrt(5000)) = 71 cells/axis over a 100-unit extent, so a box wider than
    // MAX_SPAN(32) * (100/71) ≈ 45 units spans more than MAX_SPAN cells and lands in `large`.
    let mut grid = BoxGrid::new(Some(Rect::new(0.0, 0.0, 100.0, 100.0)), 5000);
    let mut boxes: Vec<Rect> = (0..500).map(|_| rng.lattice_box()).collect();
    let big = Rect::new(1.0, 1.0, 99.0, 99.0); // width 98 > 45: overflows into `large`
    let infinite = Rect::new(f64::NEG_INFINITY, 0.0, 1.0, 1.0); // not finite: also `large`
    boxes.push(big);
    boxes.push(infinite);
    for (i, r) in boxes.iter().enumerate() {
        grid.insert(i as u32, *r);
    }
    let queries = [
        rng.lattice_box(), // small: normal cells, plus `large` unconditionally
        Rect::new(0.0, 0.0, 100.0, 100.0), // spans every cell on both axes: the `all` fallback
        Rect::new(f64::NAN, 0.0, 1.0, 1.0), // not finite: also the `all` fallback
    ];
    for q in queries {
        let mut seen = std::collections::HashSet::new();
        grid.query(q, &mut |id| {
            seen.insert(id);
            true
        });
        for (i, r) in boxes.iter().enumerate() {
            if intersects(*r, q) {
                assert!(
                    seen.contains(&(i as u32)),
                    "grid missed box {i} for query {q:?}"
                );
            }
        }
    }
}
