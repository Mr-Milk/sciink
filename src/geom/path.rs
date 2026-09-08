//! Path data: SVG `d` ↔ kurbo `BezPath` with the source-command index kept (spec §B.1).
//! Arcs become cubics (`kurbo::Arc::from_svg_arc`, tolerance 1e-4); degenerate arcs
//! become lines; H/V → L and S/T → C/Q. Output is always absolute `M L Q C Z`.
//! Never re-serialize a `d` you did not modify.

use kurbo::{Arc, BezPath, PathEl, Point, Rect, Shape, SvgArc, Vec2};
use svgtypes::{PathParser, PathSegment};

use super::ipx;
use crate::dom::{Doc, NodeId};
use crate::num;

#[derive(Debug, Clone)]
pub struct ParsedPath {
    pub path: BezPath,
    /// `cmd_start[i]` = index of the first `PathEl` produced by source command `i`;
    /// the last entry is the element count.
    pub cmd_start: Vec<usize>,
}

pub fn parse_d(d: &str) -> Option<ParsedPath> {
    let mut path = BezPath::new();
    let mut cmd_start = Vec::new();
    let mut cur = Point::ZERO;
    let mut start = Point::ZERO;
    let mut last_cubic_ctrl: Option<Point> = None;
    let mut last_quad_ctrl: Option<Point> = None;
    let mut seen_any = false;
    for seg in PathParser::from(d) {
        let seg = seg.ok()?;
        if !seen_any && !matches!(seg, PathSegment::MoveTo { .. }) {
            return None;
        }
        cmd_start.push(path.elements().len());
        let mut next_cubic = None;
        let mut next_quad = None;
        match seg {
            PathSegment::MoveTo { abs, x, y } => {
                let p = pt(abs, cur, x, y);
                path.move_to(p);
                cur = p;
                start = p;
            }
            PathSegment::LineTo { abs, x, y } => {
                let p = pt(abs, cur, x, y);
                path.line_to(p);
                cur = p;
            }
            PathSegment::HorizontalLineTo { abs, x } => {
                let p = Point::new(if abs { x } else { cur.x + x }, cur.y);
                path.line_to(p);
                cur = p;
            }
            PathSegment::VerticalLineTo { abs, y } => {
                let p = Point::new(cur.x, if abs { y } else { cur.y + y });
                path.line_to(p);
                cur = p;
            }
            PathSegment::CurveTo {
                abs,
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let (c1, c2, p) = (
                    pt(abs, cur, x1, y1),
                    pt(abs, cur, x2, y2),
                    pt(abs, cur, x, y),
                );
                path.curve_to(c1, c2, p);
                next_cubic = Some(c2);
                cur = p;
            }
            PathSegment::SmoothCurveTo { abs, x2, y2, x, y } => {
                let c1 = reflect(cur, last_cubic_ctrl);
                let (c2, p) = (pt(abs, cur, x2, y2), pt(abs, cur, x, y));
                path.curve_to(c1, c2, p);
                next_cubic = Some(c2);
                cur = p;
            }
            PathSegment::Quadratic { abs, x1, y1, x, y } => {
                let (c, p) = (pt(abs, cur, x1, y1), pt(abs, cur, x, y));
                path.quad_to(c, p);
                next_quad = Some(c);
                cur = p;
            }
            PathSegment::SmoothQuadratic { abs, x, y } => {
                let c = reflect(cur, last_quad_ctrl);
                let p = pt(abs, cur, x, y);
                path.quad_to(c, p);
                next_quad = Some(c);
                cur = p;
            }
            PathSegment::EllipticalArc {
                abs,
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x,
                y,
            } => {
                let p = pt(abs, cur, x, y);
                // Hostile radii (non-finite, or so large the ellipse is locally
                // flat) make `Arc::from_svg_arc`'s cubic-subdivision count grow
                // with radius/tolerance without bound, hanging the process; fall
                // back to a straight line instead of feeding it something absurd.
                let hostile = !rx.is_finite()
                    || !ry.is_finite()
                    || !x.is_finite()
                    || !y.is_finite()
                    || rx.abs() > 1e15
                    || ry.abs() > 1e15;
                if hostile {
                    path.line_to(p);
                } else {
                    let svg_arc = SvgArc {
                        from: cur,
                        to: p,
                        radii: Vec2::new(rx.abs(), ry.abs()),
                        x_rotation: x_axis_rotation.to_radians(),
                        large_arc,
                        sweep,
                    };
                    match Arc::from_svg_arc(&svg_arc) {
                        Some(arc) => {
                            for el in arc.append_iter(1e-4) {
                                path.push(el);
                            }
                        }
                        None => path.line_to(p),
                    }
                }
                cur = p;
            }
            PathSegment::ClosePath { .. } => {
                path.close_path();
                cur = start;
            }
        }
        last_cubic_ctrl = next_cubic;
        last_quad_ctrl = next_quad;
        seen_any = true;
    }
    if !seen_any {
        return None;
    }
    cmd_start.push(path.elements().len());
    Some(ParsedPath { path, cmd_start })
}

fn pt(abs: bool, cur: Point, x: f64, y: f64) -> Point {
    if abs {
        Point::new(x, y)
    } else {
        Point::new(cur.x + x, cur.y + y)
    }
}

fn reflect(cur: Point, ctrl: Option<Point>) -> Point {
    match ctrl {
        Some(c) => Point::new(2.0 * cur.x - c.x, 2.0 * cur.y - c.y),
        None => cur,
    }
}

/// Absolute `M x,y L x,y Q … C … Z`, numbers through `num::fmt`.
pub fn fmt_d(path: &BezPath) -> String {
    let mut s = String::new();
    for el in path.elements() {
        if !s.is_empty() {
            s.push(' ');
        }
        match el {
            PathEl::MoveTo(p) => {
                s.push_str("M ");
                push_pt(&mut s, *p);
            }
            PathEl::LineTo(p) => {
                s.push_str("L ");
                push_pt(&mut s, *p);
            }
            PathEl::QuadTo(c, p) => {
                s.push_str("Q ");
                push_pt(&mut s, *c);
                s.push(' ');
                push_pt(&mut s, *p);
            }
            PathEl::CurveTo(c1, c2, p) => {
                s.push_str("C ");
                push_pt(&mut s, *c1);
                s.push(' ');
                push_pt(&mut s, *c2);
                s.push(' ');
                push_pt(&mut s, *p);
            }
            PathEl::ClosePath => s.push('Z'),
        }
    }
    s
}

fn push_pt(s: &mut String, p: Point) {
    s.push_str(&num::fmt(p.x));
    s.push(',');
    s.push_str(&num::fmt(p.y));
}

/// One point per element; `Z` yields the subpath start (`inkex/paths.py:1446-1457`).
pub fn end_points(path: &BezPath) -> Vec<Point> {
    let mut out = Vec::with_capacity(path.elements().len());
    let mut start = Point::ZERO;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                start = *p;
                out.push(*p);
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => out.push(*p),
            PathEl::ClosePath => out.push(start),
        }
    }
    out
}

pub fn reverse(path: &BezPath) -> BezPath {
    path.reverse_subpaths()
}

/// Same element kinds, every point within `tol` (upstream compared floats exactly).
pub fn path_eq(a: &BezPath, b: &BezPath, tol: f64) -> bool {
    let (ea, eb) = (a.elements(), b.elements());
    if ea.len() != eb.len() {
        return false;
    }
    let close = |p: Point, q: Point| (p.x - q.x).abs() <= tol && (p.y - q.y).abs() <= tol;
    ea.iter().zip(eb).all(|(x, y)| match (x, y) {
        (PathEl::MoveTo(p), PathEl::MoveTo(q)) | (PathEl::LineTo(p), PathEl::LineTo(q)) => {
            close(*p, *q)
        }
        (PathEl::QuadTo(a1, a2), PathEl::QuadTo(b1, b2)) => close(*a1, *b1) && close(*a2, *b2),
        (PathEl::CurveTo(a1, a2, a3), PathEl::CurveTo(b1, b2, b3)) => {
            close(*a1, *b1) && close(*a2, *b2) && close(*a3, *b3)
        }
        (PathEl::ClosePath, PathEl::ClosePath) => true,
        _ => false,
    })
}

/// Tight box from Bézier extrema (`paths.py:679-687`).
pub fn bbox_exact(path: &BezPath) -> Option<Rect> {
    if path.elements().is_empty() {
        None
    } else {
        Some(path.bounding_box())
    }
}

/// Control-point box (upstream `roughpath=True`, `dhelpers.py:1482-1494`).
pub fn bbox_rough(path: &BezPath) -> Option<Rect> {
    if path.elements().is_empty() {
        None
    } else {
        Some(path.control_box())
    }
}

/// Geometry of a shape element as a path in its own coordinates (`cache.py:426-467`,
/// `inkex/elements/_polygons.py:350-362`). `None` for non-shapes or unusable attributes.
pub fn shape_path(doc: &Doc, n: NodeId) -> Option<ParsedPath> {
    // `None` = attribute absent (a caller may apply its own default);
    // `Some(None)` = attribute present but unusable (e.g. a `%` length, which
    // needs a viewport we don't compute) — every caller must propagate that as
    // a hard failure of the whole shape, never silently substitute a default.
    let raw = |name: &str| doc.attr(n, name).map(ipx);
    // Required attribute: bail (return `None` from `shape_path`) on absent OR unusable.
    let req = |name: &str| raw(name).flatten();
    // Attribute that defaults to `default` when absent; still bails when
    // present but unusable.
    let opt = |name: &str, default: f64| match raw(name) {
        None => Some(default),
        Some(v) => v,
    };
    let f = num::fmt;
    match doc.tag(n) {
        "path" => parse_d(doc.attr(n, "d")?),
        "rect" => {
            let (x, y) = (opt("x", 0.0)?, opt("y", 0.0)?);
            let (w, h) = (req("width")?, req("height")?);
            let (rx_raw, ry_raw) = (raw("rx"), raw("ry"));
            if matches!(rx_raw, Some(None)) || matches!(ry_raw, Some(None)) {
                return None;
            }
            let (rx0, ry0) = (rx_raw.flatten(), ry_raw.flatten());
            let d = match (rx0, ry0) {
                (None, None) => format!("M {},{} h {} v {} h {} z", f(x), f(y), f(w), f(h), f(-w)),
                _ => {
                    let rx = rx0.filter(|v| *v > 0.0).or(ry0).unwrap_or(0.0).min(w / 2.0);
                    let ry = ry0.filter(|v| *v > 0.0).or(rx0).unwrap_or(0.0).min(h / 2.0);
                    format!(
                        "M {},{} h {} a {},{} 0 0 1 {},{} v {} a {},{} 0 0 1 {},{} h {} a {},{} 0 0 1 {},{} v {} a {},{} 0 0 1 {},{} z",
                        f(x + rx),
                        f(y),
                        f(w - 2.0 * rx),
                        f(rx),
                        f(ry),
                        f(rx),
                        f(ry),
                        f(h - 2.0 * ry),
                        f(rx),
                        f(ry),
                        f(-rx),
                        f(ry),
                        f(-(w - 2.0 * rx)),
                        f(rx),
                        f(ry),
                        f(-rx),
                        f(-ry),
                        f(-(h - 2.0 * ry)),
                        f(rx),
                        f(ry),
                        f(rx),
                        f(-ry)
                    )
                }
            };
            parse_d(&d)
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (opt("cx", 0.0)?, opt("cy", 0.0)?);
            let (rx, ry) = if doc.tag(n) == "circle" {
                let r = req("r")?;
                (r, r)
            } else {
                (req("rx")?, req("ry")?)
            };
            parse_d(&format!(
                "M {},{} a {},{} 0 1 0 {},{} a {},{} 0 0 0 {},{} z",
                f(cx),
                f(cy - ry),
                f(rx),
                f(ry),
                f(rx),
                f(ry),
                f(rx),
                f(ry),
                f(-rx),
                f(-ry)
            ))
        }
        "line" => parse_d(&format!(
            "M {},{} L {},{}",
            f(opt("x1", 0.0)?),
            f(opt("y1", 0.0)?),
            f(opt("x2", 0.0)?),
            f(opt("y2", 0.0)?)
        )),
        "polyline" | "polygon" => {
            let pts: Vec<f64> = doc
                .attr(n, "points")?
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f64>().ok())
                .collect::<Option<Vec<_>>>()?;
            if pts.len() < 4 {
                return None;
            }
            let mut d = String::new();
            for (i, xy) in pts.chunks_exact(2).enumerate() {
                d.push_str(if i == 0 { "M " } else { " L " });
                d.push_str(&format!("{},{}", f(xy[0]), f(xy[1])));
            }
            if doc.tag(n) == "polygon" {
                d.push_str(" Z");
            }
            parse_d(&d)
        }
        _ => None,
    }
}
