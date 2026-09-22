//! Scaler (spec §B.3 "Scaler"; upstream scale_plots.py): corrects manually scaled plots and
//! matches plots to a first selection without distorting text, ticks and groups.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::dom::{Doc, NodeId};
use crate::geom::path::shape_path;
use crate::geom::{BezPath, PathEl, Point, Rect, uniquetol};
use crate::ops::style::strokefill;
use crate::text::Warnings;

/// Elements whose end points, not their visual box, describe their extent (`SP:31–36`).
pub const PATHLIKE: &[&str] = &["path", "rect", "line", "polyline"];
/// Elements that are not scaled unless marked otherwise (`SP:38–40`).
pub const SCALEFREE_DEFAULT: &[&str] = &["text", "flowRoot", "g"];
/// Selection members that are never plots (`SP:41–47`).
pub const EXCLUDE_TAGS: &[&str] = &["tspan", "namedview", "defs", "metadata", "foreignObject"];
pub const SCALETYPE: &str = "inkscape-scientific-scaletype";
pub const COMBINED: &str = "inkscape-scientific-combined-by-color";
/// `SP:148–157`, shown when every selected object is a raster image.
pub const IMAGE_ERR: &str = "Thanks for using Scientific Inkscape!\n\nIt appears that you're attempting to scale a raster Image object. Please note that Inkscape is mainly for working with vector images, not raster images. Vector images preserve all of the information used to generate them, whereas raster images do not. Read about the difference here: \nhttps://en.wikipedia.org/wiki/Vector_graphics\n\nWhile raster images can be embedded in vector images, they cannot be modified directly. If you want to edit a raster image, you will need to use a program like Photoshop or GIMP.";

/// End points of `path`'s elements in `range` (all when `None`), one per element; a `Z` yields
/// the start of its subpath when that start lies inside the range (`inkex/paths.py:1446–1457`).
fn range_end_points(path: &BezPath, range: Option<Range<usize>>) -> Vec<Point> {
    let els = path.elements();
    let range = range.unwrap_or(0..els.len());
    let end = range.end.min(els.len());
    let mut out = Vec::new();
    let mut start: Option<Point> = None;
    for el in &els[range.start.min(end)..end] {
        match *el {
            PathEl::MoveTo(p) => {
                start = Some(p);
                out.push(p);
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => out.push(p),
            PathEl::ClosePath => {
                if let Some(s) = start {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// The element's end points in root coordinates (`DH.get_points`): its own geometry (`shape_path`,
/// absolute), optionally one BezPath-element range, through its composed transform. Empty for
/// elements without geometry.
pub fn global_points(doc: &Doc, el: NodeId, range: Option<Range<usize>>) -> Vec<Point> {
    let Some(pp) = shape_path(doc, el) else {
        return Vec::new();
    };
    let ct = doc.composed_transform(el);
    range_end_points(&pp.path, range)
        .into_iter()
        .map(|p| ct * p)
        .collect()
}

/// `SP:52–65`: for path-like elements the box of the global end points, clamped to the visual box
/// (a clipped element's points overshoot its visible extent); otherwise the visual box.
pub fn geometric_bbox(doc: &Doc, el: NodeId, vis: Rect, range: Option<Range<usize>>) -> Rect {
    if !PATHLIKE.contains(&doc.tag(el)) {
        return vis;
    }
    let pts = global_points(doc, el, range);
    if pts.is_empty() {
        return vis;
    }
    let (mut minx, mut maxx, mut miny, mut maxy) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for p in &pts {
        minx = minx.min(p.x);
        maxx = maxx.max(p.x);
        miny = miny.min(p.y);
        maxy = maxy.max(p.y);
    }
    // upstream keeps a negative width when the clamps cross; kurbo's Rect does the same
    Rect::new(
        minx.max(vis.x0),
        miny.max(vis.y0),
        maxx.min(vis.x1),
        maxy.min(vis.y1),
    )
}

/// Result of `find_plot_area` (`SP:69–123`): the vertical and horizontal lines among `els`, and
/// the elements with the largest vertical (`lvel`) and horizontal (`lhel`) extents among the
/// lines, the framed rectangles and the elements marked `plot_area`.
#[derive(Debug, Default)]
pub struct PlotArea {
    pub vl: HashSet<NodeId>,
    pub hl: HashSet<NodeId>,
    pub lvel: Option<NodeId>,
    pub lhel: Option<NodeId>,
}

fn is_opaque_white(c: &crate::ops::style::Rgba) -> bool {
    (c.r, c.g, c.b) == (255, 255, 255) && c.alpha == 1.0
}

/// `SP:69–123` over `els` (a plot's direct element children) and their geometric boxes; elements
/// without a box are skipped. Ties for the largest extent go to the first candidate in upstream's
/// insertion order: lines (in reversed `els` order), then framed rectangles, then marked elements.
pub fn find_plot_area(doc: &Doc, els: &[NodeId], gbbs: &HashMap<NodeId, Rect>) -> PlotArea {
    let mut pa = PlotArea::default();
    let mut vl: Vec<(NodeId, Rect)> = Vec::new();
    let mut hl: Vec<(NodeId, Rect)> = Vec::new();
    let mut boxes: Vec<(NodeId, Rect)> = Vec::new();
    let mut plotareas: Vec<(NodeId, Rect)> = Vec::new();
    for &el in els.iter().rev() {
        let Some(&gbb) = gbbs.get(&el) else { continue };
        let tag = doc.tag(el);
        let mut isrect = false;
        if PATHLIKE.contains(&tag) {
            let pts = global_points(doc, el, None);
            if !pts.is_empty() {
                let xs: Vec<f64> = pts.iter().map(|p| p.x).collect();
                let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
                let (xmin, xmax) = (
                    xs.iter().cloned().fold(f64::INFINITY, f64::min),
                    xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                );
                let (ymin, ymax) = (
                    ys.iter().cloned().fold(f64::INFINITY, f64::min),
                    ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                );
                if xmax - xmin < 0.001 * gbb.height() {
                    vl.push((el, gbb));
                    pa.vl.insert(el);
                }
                if ymax - ymin < 0.001 * gbb.width() {
                    hl.push((el, gbb));
                    pa.hl.insert(el);
                }
                let tol = 1e-3 * (xmax - xmin).max(ymax - ymin);
                isrect = (3..=5).contains(&pts.len())
                    && uniquetol(&xs, tol) == 2
                    && uniquetol(&ys, tol) == 2;
            }
        }
        if isrect || tag == "rect" {
            let sf = strokefill(doc, el);
            let hasfill = sf.fill.as_ref().is_some_and(|f| !is_opaque_white(f));
            let hasstroke = sf.stroke.as_ref().is_some_and(|s| !is_opaque_white(s));
            let same = match (&sf.stroke, &sf.fill) {
                (Some(s), Some(f)) => {
                    (s.r, s.g, s.b) == (f.r, f.g, f.b) && (s.alpha - f.alpha).abs() < 1e-9
                }
                _ => false,
            };
            if hasfill && (!hasstroke || same) {
                // solid rectangle: unused by upstream too
            } else if hasstroke {
                boxes.push((el, gbb));
            }
        }
        if doc.attr(el, SCALETYPE) == Some("plot_area") {
            plotareas.push((el, gbb));
        }
    }
    // largest vertical extent among lines (by height), boxes and marked elements (by height)
    let mut vels: Vec<(NodeId, f64)> = vl.iter().map(|(n, b)| (*n, b.height())).collect();
    let mut hels: Vec<(NodeId, f64)> = hl.iter().map(|(n, b)| (*n, b.width())).collect();
    for (n, b) in boxes.iter().chain(plotareas.iter()) {
        hels.push((*n, b.width()));
        vels.push((*n, b.height()));
    }
    let first_max = |v: &[(NodeId, f64)]| -> Option<NodeId> {
        let mut best: Option<(NodeId, f64)> = None;
        for &(n, x) in v {
            if best.is_none_or(|(_, bx)| x > bx) {
                best = Some((n, x));
            }
        }
        best.map(|(n, _)| n)
    };
    pa.lvel = first_max(&vels);
    pa.lhel = first_max(&hels);
    pa
}

/// `SP:127–132`.
pub fn ordinal(n: usize) -> String {
    let suffix = if (10..=20).contains(&(n % 100)) {
        "th"
    } else {
        match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{n}{suffix}")
}

/// `SP:135–145`: the "no plot area" message for the `idx`-th (0-based) selected plot.
pub fn warn_non_plot(warn: &mut Warnings, idx: usize, gid: &str) {
    warn.push(format!(
        "A box-like plot area could not be automatically detected on the {} selected plot (group ID {gid}).\n\nDraw a box with a stroke to define the plot area or mark objects as plot area-determining in the Advanced tab.\nScaling will still be performed, but the results may not be ideal.",
        ordinal(idx + 1)
    ));
}
