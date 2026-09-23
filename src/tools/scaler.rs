//! Scaler (spec §B.3 "Scaler"; upstream scale_plots.py): corrects manually scaled plots and
//! matches plots to a first selection without distorting text, ticks and groups.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::ops::Range;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::geom::path::shape_path;
use crate::geom::{
    Affine, BezPath, PathEl, Point, Rect, inverse, transform_rect, union, uniquetol,
};
use crate::ops::Ctx;
use crate::ops::bbox::bb2;
use crate::ops::style::strokefill;
use crate::ops::xform::{Ranges, global_transform};
use crate::text::Warnings;

use super::first_line;

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

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct ScalerCli {
    #[command(flatten)]
    pub common: Common,
    /// `correction` | `matching` | `options` (anything else is the Advanced tab, as upstream)
    #[arg(long, default_value = "correction")]
    pub tab: String,
    /// Fixed mode is gone upstream and here; its parameters are accepted and ignored.
    #[arg(long, default_value_t = 100.0)]
    pub hscale: f64,
    #[arg(long, default_value_t = 100.0)]
    pub vscale: f64,
    /// 1 = maintain the plot area, 2 = maintain the bounding box
    #[arg(long, default_value_t = 1)]
    pub figuremode: u8,
    /// 1 = match plot areas, 2 = match bounding boxes
    #[arg(long, default_value_t = 1)]
    pub matchprop: u8,
    /// 1 = do not match, 2 = match, 3 = match and align
    #[arg(long, default_value_t = 1)]
    pub hmatchopts: u8,
    #[arg(long, default_value_t = 1)]
    pub vmatchopts: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub deletematch: bool,
    /// 1 scale_free, 2 aspect_locked, 3 normal, 4 plot_area, 5 clear
    #[arg(long, default_value_t = 1)]
    pub marksf: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub tickcorrect: bool,
    /// Percent of the plot area (the `.inx` says float, upstream parses int; both spellings parse)
    #[arg(long, default_value_t = 10.0)]
    pub tickthreshold: f64,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub wholeplot1: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub wholeplot2: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub wholeplot3: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Correction,
    Matching {
        hmatch: bool,
        vmatch: bool,
        alignx: bool,
        aligny: bool,
        bbox: bool,
        deletematch: bool,
    },
    /// The Advanced tab: mark the selection (`None` clears) and stop.
    Advanced {
        mark: Option<&'static str>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub mode: Mode,
    /// `figuremode == 2`; read by every correction, including Matching's pre-pass (SP:345).
    pub figure: bool,
    pub wholesel: bool,
    pub tickcorrect: bool,
    pub tickthr: f64,
}

impl Options {
    /// SP:239–269. Deviation: option values outside upstream's tables are tolerated
    /// (`figuremode`/`matchprop` ≠ 2 mean the first option, `marksf` outside 1–4 clears).
    pub fn from_cli(c: &ScalerCli) -> Options {
        let (mode, wholesel) = match c.tab.as_str() {
            "matching" => (
                Mode::Matching {
                    hmatch: matches!(c.hmatchopts, 2 | 3),
                    vmatch: matches!(c.vmatchopts, 2 | 3),
                    alignx: c.hmatchopts == 3,
                    aligny: c.vmatchopts == 3,
                    bbox: c.matchprop == 2,
                    deletematch: c.deletematch,
                },
                c.wholeplot2,
            ),
            "correction" => (Mode::Correction, c.wholeplot3),
            _ => (
                Mode::Advanced {
                    mark: match c.marksf {
                        1 => Some("scale_free"),
                        2 => Some("aspect_locked"),
                        3 => Some("normal"),
                        4 => Some("plot_area"),
                        _ => None,
                    },
                },
                false,
            ),
        };
        Options {
            mode,
            figure: c.figuremode == 2,
            wholesel,
            tickcorrect: c.tickcorrect && !wholesel,
            tickthr: c.tickthreshold / 100.0,
        }
    }
}

/// Visual (`f`) and geometric (`g`) boxes in root coordinates, SP:271–275.
pub(crate) struct Boxes {
    pub f: HashMap<NodeId, Rect>,
    pub g: HashMap<NodeId, Rect>,
}

impl Boxes {
    /// Boxes of every element under `roots` (each once).
    pub(crate) fn compute(doc: &mut Doc, ctx: &mut Ctx, roots: &[NodeId]) -> Boxes {
        let mut els: Vec<NodeId> = Vec::new();
        let mut seen: HashSet<NodeId> = HashSet::new();
        for &r in roots {
            for n in doc.descendants(r).filter(|&n| doc.is_element(n)) {
                if seen.insert(n) {
                    els.push(n);
                }
            }
        }
        let f = bb2(doc, ctx, &els, false);
        let g = f
            .iter()
            .map(|(&n, &v)| (n, geometric_bbox(doc, n, v, None)))
            .collect();
        Boxes { f, g }
    }

    /// Re-measures one plot's subtree (after Matching's correction pre-pass).
    /// Deviation (spec R9): upstream matches against the boxes measured BEFORE the pre-pass.
    pub(crate) fn refresh(&mut self, doc: &mut Doc, ctx: &mut Ctx, plot: NodeId) {
        let fresh = Boxes::compute(doc, ctx, &[plot]);
        self.f.extend(fresh.f);
        self.g.extend(fresh.g);
    }
}

/// Upstream's `bbox2`: the geometric and the visual union of a set of elements.
#[derive(Default, Clone, Copy)]
struct Bb2 {
    g: Option<Rect>,
    f: Option<Rect>,
}

impl Bb2 {
    fn add(&mut self, g: Rect, f: Rect) {
        self.g = union(self.g, Some(g));
        self.f = union(self.f, Some(f));
    }
}

/// `(sx, sy)` of a group's own transform, SP:306–307 / 336–337: `sx = sqrt(a² + b²)`,
/// `sy = det / sx` (negative for a flip).
fn own_scale(t: Affine) -> (f64, f64) {
    let [a, b, c, d, _, _] = t.as_coeffs();
    let sx = (a * a + b * b).sqrt();
    (sx, if sx > 0.0 { (a * d - b * c) / sx } else { 0.0 })
}

/// A scale factor safe to invert: finite and non-zero, else `1` with a warning (hostile input;
/// upstream divides by zero).
fn sane(s: f64, what: &str, warn: &mut Warnings) -> f64 {
    if s.is_finite() && s.abs() > 1e-12 {
        s
    } else {
        warn.push(format!("{what} is degenerate ({s}); using 1"));
        1.0
    }
}

fn t(x: f64, y: f64) -> Affine {
    Affine::translate((x, y))
}
fn s(x: f64, y: f64) -> Affine {
    Affine::scale_non_uniform(x, y)
}

/// SP:297–613 for one grouped plot. `cmode` = Correction; `first` is the Matching target.
#[allow(clippy::too_many_arguments)]
pub(crate) fn scale_plot(
    doc: &mut Doc,
    ctx: &mut Ctx,
    o: &Options,
    boxes: &mut Boxes,
    first: NodeId,
    plot: NodeId,
    i: usize,
    cmode: bool,
) {
    if !cmode {
        // SP:303–309: a plot carrying a scale is corrected first
        let (sx, sy) = own_scale(doc.transform(plot));
        if (sx - 1.0).abs() > 1e-5 || (sy - 1.0).abs() > 1e-5 {
            scale_plot(doc, ctx, o, boxes, first, plot, i, true);
            boxes.refresh(doc, ctx, plot);
        }
    }
    let pid = doc.attr(plot, "id").unwrap_or("").to_string();
    let pels: Vec<NodeId> = doc
        .children(plot)
        .filter(|&k| doc.is_element(k) && boxes.f.contains_key(&k))
        .collect();
    let pa = find_plot_area(doc, &pels, &boxes.g);
    let (noplotarea, lvel, lhel) = if pa.lvel.is_none() || pa.lhel.is_none() || o.wholesel {
        if !o.wholesel {
            warn_non_plot(&mut ctx.warn, i, &pid);
        }
        (true, None, None)
    } else {
        (false, pa.lvel, pa.lhel)
    };
    let in_area = |el: NodeId| noplotarea || Some(el) == lvel || Some(el) == lhel;
    let mut bba = Bb2::default();
    let mut bbp = Bb2::default();
    for &el in &pels {
        bba.add(boxes.g[&el], boxes.f[&el]);
        if in_area(el) {
            bbp.add(boxes.g[&el], boxes.f[&el]);
        }
    }
    // the boxes the per-child step reads (SP:358–367 / 410–411)
    let mut f2: HashMap<NodeId, Rect> = pels.iter().map(|&e| (e, boxes.f[&e])).collect();
    let mut g2: HashMap<NodeId, Rect> = pels.iter().map(|&e| (e, boxes.g[&e])).collect();
    let (Some(_), Some(_)) = (bbp.g, bba.f) else {
        return;
    }; // nothing with a box: nothing to scale

    let (mut scalex, mut scaley, mut refx, mut refy): (f64, f64, f64, f64);
    let mut bbmatch: Option<Rect> = None;
    if cmode {
        // SP:331–408
        let (sx, sy) = own_scale(doc.transform(plot));
        scalex = sane(sx, "the plot's horizontal scale", &mut ctx.warn);
        scaley = sane(sy, "the plot's vertical scale", &mut ctx.warn);
        let (bbp_g, bba_f) = (
            bbp.g.expect("guarded: pels has a box"),
            bba.f.expect("guarded: pels has a box"),
        );
        (refx, refy) = if !o.figure {
            (bbp_g.center().x, bbp_g.center().y)
        } else {
            (bba_f.x0, bba_f.y0)
        };
        let iextr = t(refx, refy) * s(1.0 / scalex, 1.0 / scaley) * t(-refx, -refy);
        global_transform(doc, ctx, plot, iextr, None, true);
        for v in f2.values_mut() {
            *v = transform_rect(iextr, *v);
        }
        for v in g2.values_mut() {
            *v = transform_rect(iextr, *v);
        }
        let tr_bba = bba;
        bba = Bb2::default();
        bbp = Bb2::default();
        for &el in &pels {
            bba.add(g2[&el], f2[&el]);
            if in_area(el) {
                bbp.add(g2[&el], f2[&el]);
            }
        }
        if o.figure {
            // SP:380–408: keep the figure's visual size and top-left corner
            let (oscalex, oscaley) = (scalex, scaley);
            let (trf, bbaf, bbpg) = (tr_bba.f.unwrap(), bba.f.unwrap(), bbp.g.unwrap());
            scalex = sane(
                (trf.width() - (bbaf.width() - bbpg.width())) / bbpg.width(),
                "the figure's horizontal scale",
                &mut ctx.warn,
            );
            scaley = sane(
                (trf.height() - (bbaf.height() - bbpg.height())) / bbpg.height(),
                "the figure's vertical scale",
                &mut ctx.warn,
            );
            let tlx = (trf.x0 - refx) / oscalex + refx;
            let dxl = bbpg.x0 - tlx;
            refx = if scalex != 1.0 {
                (trf.x0 + dxl - bbpg.x0 * scalex) / (1.0 - scalex)
            } else {
                trf.x0 + dxl
            };
            let tly = (trf.y0 - refy) / oscaley + refy;
            let dyl = bbpg.y0 - tly;
            refy = if scaley != 1.0 {
                (trf.y0 + dyl - bbpg.y0 * scaley) / (1.0 - scaley)
            } else {
                trf.y0 + dyl
            };
        }
    } else {
        // SP:413–440
        let Mode::Matching {
            hmatch,
            vmatch,
            bbox: matchbbox,
            ..
        } = o.mode
        else {
            return;
        };
        bbmatch = if matchbbox {
            boxes.g.get(&first).copied()
        } else {
            let els: Vec<NodeId> = if doc.tag(first) == "g" {
                doc.children(first).filter(|&k| doc.is_element(k)).collect()
            } else {
                vec![first]
            };
            let pa0 = find_plot_area(doc, &els, &boxes.g);
            match (pa0.lvel, pa0.lhel) {
                (Some(v), Some(h)) => union(boxes.g.get(&v).copied(), boxes.g.get(&h).copied()),
                _ => {
                    if doc.tag(first) != "image" {
                        let fid = doc.attr(first, "id").unwrap_or("").to_string();
                        warn_non_plot(&mut ctx.warn, 0, &fid);
                    }
                    boxes.g.get(&first).copied()
                }
            }
        };
        let (Some(bm), Some(bbpg), Some(bbag)) = (bbmatch, bbp.g, bba.g) else {
            // the first selection has no box: nothing to match against (upstream crashes)
            ctx.warn
                .push("the first selection has no bounding box; nothing was matched".to_string());
            return;
        };
        scalex = 1.0;
        scaley = 1.0;
        if hmatch {
            scalex = if !matchbbox {
                bm.width() / bbpg.width()
            } else {
                (bm.width() + bbpg.width() - bbag.width()) / bbpg.width()
            };
        }
        if vmatch {
            scaley = if !matchbbox {
                bm.height() / bbpg.height()
            } else {
                (bm.height() + bbpg.height() - bbag.height()) / bbpg.height()
            };
        }
        scalex = sane(scalex, "the horizontal match scale", &mut ctx.warn);
        scaley = sane(scaley, "the vertical match scale", &mut ctx.warn);
        // SP:443–449
        (refx, refy) = if !matchbbox {
            (bbpg.center().x, bbpg.center().y)
        } else {
            (bbag.center().x, bbag.center().y)
        };
    }
    let (bbpg, bbag) = (
        bbp.g.expect("guarded: pels has a box"),
        bba.g.expect("guarded: pels has a box"),
    );
    // SP:442–466
    let (mut finx, mut finy) = (refx, refy);
    if let (
        false,
        Mode::Matching {
            alignx,
            aligny,
            bbox,
            ..
        },
        Some(bm),
    ) = (cmode, o.mode, bbmatch)
    {
        if alignx {
            finx = bm.center().x;
        }
        if aligny {
            finy = bm.center().y;
        }
        if bbox {
            finx -= 0.5 * ((bbag.x1 - bbpg.x1) - (bbpg.x0 - bbag.x0)) * (1.0 - scalex);
            finy -= 0.5 * ((bbag.y1 - bbpg.y1) - (bbpg.y0 - bbag.y0)) * (1.0 - scaley);
        }
    }
    let gtr = t(finx, finy) * s(scalex, scaley) * t(-refx, -refy);
    let iscl = s(1.0 / scalex, 1.0 / scaley);
    let l = (scalex * scaley).abs().sqrt();
    let liscl = s(l, l) * iscl;
    let trul = gtr * Point::new(bbpg.x0, bbpg.y0);
    let trbr = gtr * Point::new(bbpg.x1, bbpg.y1);
    let thr = o.tickthr;
    for &el in &pels {
        global_transform(doc, ctx, el, gtr, None, true);
        let (fbb, gbb) = (f2[&el], g2[&el]);
        let stype: String = doc
            .attr(el, SCALETYPE)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if SCALEFREE_DEFAULT.contains(&doc.tag(el)) {
                    "scale_free"
                } else {
                    "normal"
                }
                .to_string()
            });
        // SP:504–530: a tick is a short line at an edge of the plot area
        let (mut vtickt, mut vtickb, mut htickl, mut htickr) = (false, false, false, false);
        if o.tickcorrect && (pa.vl.contains(&el) || pa.hl.contains(&el)) {
            if pa.vl.contains(&el) && gbb.height() < thr * bbpg.height() {
                if gbb.y1 < bbpg.y0 + thr * bbpg.height() {
                    vtickt = true;
                } else if gbb.y0 > bbpg.y1 - thr * bbpg.height() {
                    vtickb = true;
                }
            }
            if pa.hl.contains(&el) && gbb.width() < thr * bbpg.width() {
                if gbb.x1 < bbpg.x0 + thr * bbpg.width() {
                    htickl = true;
                } else if gbb.x0 > bbpg.x1 - thr * bbpg.width() {
                    htickr = true;
                }
            }
        }
        if vtickt || vtickb || htickl || htickr {
            // SP:532–548: unscale about the edge the tick hangs on
            let gbb_tr = transform_rect(gtr, gbb);
            let (cx, cy) = (gbb_tr.center().x, gbb_tr.center().y);
            let p = if vtickt {
                Point::new(cx, if cy > trul.y { gbb_tr.y0 } else { gbb_tr.y1 })
            } else if vtickb {
                Point::new(cx, if cy < trbr.y { gbb_tr.y1 } else { gbb_tr.y0 })
            } else if htickl {
                Point::new(if cx > trul.x { gbb_tr.x0 } else { gbb_tr.x1 }, cy)
            } else {
                Point::new(if cx < trbr.x { gbb_tr.x1 } else { gbb_tr.x0 }, cy)
            };
            global_transform(doc, ctx, el, t(p.x, p.y) * iscl * t(-p.x, -p.y), None, true);
        } else if stype == "scale_free" || stype == "aspect_locked" {
            let inv = if stype == "scale_free" { iscl } else { liscl };
            // SP:562–576: an element outside the plot area keeps its distance to the area
            let offset = |gbb: Rect, cx: f64, cy: f64| -> (f64, f64) {
                let (mut dx, mut dy) = (0.0, 0.0);
                if cx < trul.x {
                    dx = (gbb.center().x - bbpg.x0) - (cx - trul.x);
                }
                if cx > trbr.x {
                    dx = (gbb.center().x - bbpg.x1) - (cx - trbr.x);
                }
                if cy < trul.y {
                    dy = (gbb.center().y - bbpg.y0) - (cy - trul.y);
                }
                if cy > trbr.y {
                    dy = (gbb.center().y - bbpg.y1) - (cy - trbr.y);
                }
                (dx, dy)
            };
            let cbc: Option<Vec<usize>> = doc.attr(el, COMBINED).map(|v| {
                v.split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect()
            });
            match cbc {
                None => {
                    let gbb_tr = transform_rect(gtr, gbb);
                    let (cx, cy) = (gbb_tr.center().x, gbb_tr.center().y);
                    let tr1 = t(cx, cy) * inv * t(-cx, -cy);
                    let (dx, dy) = offset(gbb, cx, cy);
                    global_transform(doc, ctx, el, t(dx, dy) * tr1, None, true);
                }
                Some(idx) => {
                    // SP:580–613: previously combined paths are unscaled piece by piece; the
                    // element already carries `gtr`, so its global points are the scaled ones
                    let Some(igtr) = inverse(gtr) else { continue };
                    let fbb_tr = transform_rect(gtr, fbb);
                    let mut ranges: Ranges = Vec::new();
                    for w in idx.windows(2) {
                        let range = w[0]..w[1];
                        let gbb_tr = geometric_bbox(doc, el, fbb_tr, Some(range.clone()));
                        let gbb = transform_rect(igtr, gbb_tr);
                        let (cx, cy) = (gbb_tr.center().x, gbb_tr.center().y);
                        let tr1 = t(cx, cy) * inv * t(-cx, -cy);
                        let (dx, dy) = offset(gbb, cx, cy);
                        ranges.push((range, t(dx, dy) * tr1));
                    }
                    global_transform(doc, ctx, el, Affine::IDENTITY, Some(ranges), true);
                }
            }
        }
        // "normal": scaled with the plot, nothing more
    }
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = ScalerCli::try_parse_from(argv).map_err(first_line)?;
    let o = Options::from_cli(&cli);
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    // SP:231–232: selection order matters (the first selection is the Matching target)
    let sel: Vec<NodeId> = doc
        .selection_ordered(&cli.common.ids)
        .into_iter()
        .filter(|&n| !EXCLUDE_TAGS.contains(&doc.tag(n)))
        .collect();
    let mut ctx = Ctx::for_roots(sel.clone());
    if let Mode::Advanced { mark } = o.mode {
        // SP:250–260
        for &el in &sel {
            match mark {
                Some(m) => doc.set_attr(el, SCALETYPE, m),
                None => {
                    doc.remove_attr(el, SCALETYPE);
                }
            }
        }
        return finish(doc, ctx);
    }
    // Deviation: upstream shows IMAGE_ERR for an EMPTY selection too (`all([])` is true)
    if sel.is_empty() {
        return Err("No objects selected!".to_string());
    }
    if sel.iter().all(|&n| doc.tag(n) == "image") {
        return Err(IMAGE_ERR.to_string());
    }
    let cmode = matches!(o.mode, Mode::Correction);
    let first = sel[0];
    let plots: Vec<NodeId> = if cmode {
        sel.clone()
    } else {
        sel[1..].to_vec()
    };
    if plots.iter().any(|&p| doc.tag(p) != "g") {
        return Err("Non-Group objects detected in selection. Objects in a plot should be grouped prior to scaling.".to_string());
    }
    let mut boxes = Boxes::compute(&mut doc, &mut ctx, &sel);
    for (i, &plot) in plots.iter().enumerate() {
        scale_plot(&mut doc, &mut ctx, &o, &mut boxes, first, plot, i, cmode);
    }
    if let Mode::Matching {
        deletematch: true, ..
    } = o.mode
    {
        // SP:294–295: a plain delete (not delete_up); references to it are dropped by finish
        if let Some(id) = doc.attr(first, "id") {
            ctx.deleted.insert(id.to_string());
        }
        doc.detach(first);
    }
    finish(doc, ctx)
}

fn finish(mut doc: Doc, mut ctx: Ctx) -> Result<Output, String> {
    ctx.finish(&mut doc);
    let messages = ctx.warn.0.iter().map(|w| format!("warning: {w}")).collect();
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
