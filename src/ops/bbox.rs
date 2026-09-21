//! Bounding boxes without Inkscape, and the rectangle test (spec §B.2 "Bounding boxes",
//! "is_rectangle"; upstream DH:1426–1552 `bounding_box2`, DH:644–659 `hasbbox`/`isdrawn`,
//! DH:663–696 `BB2`, U:180–242 `isrectangle`).

use std::collections::HashMap;

use kurbo::{Affine, Rect};

use crate::dom::{Doc, NodeId};
use crate::geom::path::{bbox_exact, bbox_rough, end_points, shape_path};
use crate::geom::{intersection, ipx, transform_rect, union, uniquetol};
use crate::text::layout::full_extent;
use crate::text::parse::ParsedText;

use super::cleanup::url_id;
use super::{ClipKind, Ctx, MAX_NEST, clip_ref, label};

/// Which box: `transform` = in root coordinates (else in the element's own frame, before its own
/// `transform`); `stroke` = grow shapes by half the stroke width; `rough` = control-point box
/// instead of the tight Bézier box; `clip` = clamp by `clip-path`/`mask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BboxOpts {
    pub transform: bool,
    pub stroke: bool,
    pub rough: bool,
    pub clip: bool,
}

/// Visual box in root coordinates (upstream's defaults).
pub const VISUAL: BboxOpts = BboxOpts {
    transform: true,
    stroke: true,
    rough: false,
    clip: true,
};
/// Visual box in the element's own frame (`dotransform=False`).
pub const LOCAL: BboxOpts = BboxOpts {
    transform: false,
    ..VISUAL
};

/// Tags Inkscape never renders, with everything under them (DH:561–583; local names).
pub const UNRENDERED: &[&str] = &[
    "namedview",
    "defs",
    "metadata",
    "foreignObject",
    "guide",
    "clipPath",
    "style",
    "tspan",
    "flowRegion",
    "flowPara",
    "mask",
    "RDF",
    "Work",
    "format",
    "type",
];
/// Containers whose box is the union of their children (DH:1414–1423, plus `a`/`switch`, spec §B.2).
pub const GROUPLIKE: &[&str] = &["svg", "g", "clipPath", "symbol", "mask", "a", "switch"];
/// Shapes `geom::path::shape_path` understands (C:489–497 `cpath_support`).
pub const SHAPES: &[&str] = &[
    "path", "rect", "circle", "ellipse", "line", "polyline", "polygon",
];
/// Tags `bb2` reports (DH:1564–1573).
pub const BB2_SUPPORT: &[&str] = &[
    "text", "flowRoot", "image", "use", "svg", "g", "path", "rect", "circle", "ellipse", "line",
    "polyline", "polygon",
];

/// DH:644–649: `n` and every ancestor is a rendered tag and the chain reaches the root `<svg>`.
pub fn has_bbox(doc: &Doc, n: NodeId) -> bool {
    let svg = doc.svg();
    let mut cur = n;
    loop {
        if cur == svg {
            return true;
        }
        if !doc.is_element(cur) || UNRENDERED.contains(&doc.tag(cur)) {
            return false;
        }
        match doc.parent(cur) {
            Some(p) => cur = p,
            None => return false,
        }
    }
}

/// DH:654–659: has a box, is not a container, and is not `display:none`.
pub fn is_drawn(doc: &Doc, n: NodeId) -> bool {
    doc.is_element(n)
        && !GROUPLIKE.contains(&doc.tag(n))
        && has_bbox(doc, n)
        && doc
            .specified(n, "display")
            .is_none_or(|v| v.trim() != "none")
}

type Memo = HashMap<(NodeId, BboxOpts), Option<Rect>>;

/// Bounding box of `n` (spec §B.2); `None` when it has no geometry or is clipped away.
pub fn bbox(doc: &mut Doc, ctx: &mut Ctx, n: NodeId, o: BboxOpts) -> Option<Rect> {
    let mut memo = Memo::new();
    bbox_rec(doc, ctx, n, o, &mut memo, 0)
}

/// DH:663–696 `BB2`: visual boxes in root coordinates of every supported element of `els` that
/// has a box, sharing one memo. Elements without a box are absent.
pub fn bb2(doc: &mut Doc, ctx: &mut Ctx, els: &[NodeId], rough: bool) -> HashMap<NodeId, Rect> {
    let mut memo = Memo::new();
    let o = BboxOpts { rough, ..VISUAL };
    let mut out = HashMap::new();
    for &n in els {
        if BB2_SUPPORT.contains(&doc.tag(n)) && has_bbox(doc, n) {
            if let Some(r) = bbox_rec(doc, ctx, n, o, &mut memo, 0) {
                out.insert(n, r);
            }
        }
    }
    out
}

fn bbox_rec(
    doc: &mut Doc,
    ctx: &mut Ctx,
    n: NodeId,
    o: BboxOpts,
    memo: &mut Memo,
    depth: usize,
) -> Option<Rect> {
    if depth > MAX_NEST {
        ctx.warn.push(format!(
            "{}: nested deeper than {MAX_NEST} levels, its bounding box is ignored",
            label(doc, n)
        ));
        return None;
    }
    if let Some(r) = memo.get(&(n, o)) {
        return *r;
    }
    let mut ret = local_bbox(doc, ctx, n, o, memo, depth);
    if ret.is_some() && o.clip {
        // DH:1525–1541: clamp by the clip's/mask's own box (no stroke), ignoring a clipPath
        // child that references its own parent
        for kind in [ClipKind::Clip, ClipKind::Mask] {
            let Some(c) = clip_ref(doc, n, kind) else {
                continue;
            };
            if doc.parent(n) == Some(c) {
                continue;
            }
            let copts = BboxOpts {
                transform: false,
                stroke: false,
                ..o
            };
            ret = match bbox_rec(doc, ctx, c, copts, memo, depth + 1) {
                Some(cb) => intersection(ret, Some(cb)),
                None => None,
            };
        }
    }
    if o.transform {
        ret = ret.map(|r| transform_rect(doc.composed_transform(n), r));
    }
    memo.insert((n, o), ret);
    ret
}

/// DH:1454–1458: the specified `stroke-width` (default `0px`, not the CSS initial `1`) when the
/// element has a paint stroke and the caller asked for it; `%` widths count as 0.
fn stroke_pad(doc: &Doc, n: NodeId, include: bool) -> f64 {
    if !include
        || doc
            .specified(n, "stroke")
            .is_none_or(|s| s.trim() == "none")
    {
        return 0.0;
    }
    doc.specified(n, "stroke-width")
        .and_then(|w| ipx(&w))
        .unwrap_or(0.0)
}

/// The element's own box in its own frame, before its `transform` and before clip clamping.
fn local_bbox(
    doc: &mut Doc,
    ctx: &mut Ctx,
    n: NodeId,
    o: BboxOpts,
    memo: &mut Memo,
    depth: usize,
) -> Option<Rect> {
    let tag = doc.tag(n).to_string();
    match tag.as_str() {
        "text" | "flowRoot" => {
            ctx.ensure_char_table(doc);
            let Ctx { text, warn, .. } = ctx;
            let ct = text.as_mut().expect("built by ensure_char_table");
            let pt = ParsedText::parse(doc, n, ct, warn)?;
            full_extent(&pt)
        }
        "line" => {
            let get = |a: &str| ipx(doc.attr(n, a).unwrap_or("0"));
            let (x1, y1, x2, y2) = (get("x1")?, get("y1")?, get("x2")?, get("y2")?);
            let half = stroke_pad(doc, n, o.stroke) / 2.0;
            Some(Rect::new(
                x1.min(x2) - half,
                y1.min(y2) - half,
                x1.max(x2) + half,
                y1.max(y2) + half,
            ))
        }
        t if SHAPES.contains(&t) => {
            let pp = shape_path(doc, n)?;
            let r = if o.rough {
                bbox_rough(&pp.path)
            } else {
                bbox_exact(&pp.path)
            }?;
            let half = stroke_pad(doc, n, o.stroke) / 2.0;
            Some(r.inflate(half, half))
        }
        t if GROUPLIKE.contains(&t) => {
            let kids: Vec<NodeId> = doc.children(n).filter(|&k| doc.is_element(k)).collect();
            let mut acc = None;
            for k in kids {
                let kopts = BboxOpts {
                    transform: false,
                    ..o
                };
                if let Some(b) = bbox_rec(doc, ctx, k, kopts, memo, depth + 1) {
                    acc = union(acc, Some(transform_rect(doc.transform(k), b)));
                }
            }
            acc
        }
        "image" => {
            // C:520–541 `xywh`: `%` is a fraction of the viewBox width (x, width) or height
            let vb = doc.viewbox();
            let len = |a: &str, along_x: bool| -> Option<f64> {
                let v = doc.attr(n, a).unwrap_or("0").trim();
                match v.strip_suffix('%') {
                    Some(p) => {
                        let f = p.trim().parse::<f64>().ok()? / 100.0;
                        let vb = vb?;
                        Some(f * if along_x { vb.width() } else { vb.height() })
                    }
                    None => ipx(v),
                }
            };
            let (x, y) = (len("x", true)?, len("y", false)?);
            let (w, h) = (len("width", true)?, len("height", false)?);
            Some(Rect::new(x, y, x + w, y + h))
        }
        "use" => {
            // DH:1509–1523: the target's box (stroke and clips included) under
            // translate(x, y) · target.transform
            let target = doc.resolve_href(n)?;
            let topts = BboxOpts {
                transform: false,
                stroke: true,
                clip: true,
                rough: o.rough,
            };
            let tb = bbox_rec(doc, ctx, target, topts, memo, depth + 1)?;
            let x = ipx(doc.attr(n, "x").unwrap_or("0"))?;
            let y = ipx(doc.attr(n, "y").unwrap_or("0"))?;
            Some(transform_rect(
                Affine::translate((x, y)) * doc.transform(target),
                tb,
            ))
        }
        _ => None,
    }
}

const PATH_LETTERS: &str = "MmZzLlHhVvCcSsQqTtAa";

/// U:180–242 `isrectangle`: rect-like (`path` with 1–6 command letters, `rect`, `line`,
/// `polyline`; a `<use>` defers to its target) with at least 4 commands whose end points —
/// after the element's own `transform` when `including_transform` — take exactly two distinct
/// x's and two distinct y's (tolerance `1e-3 · max(range)`); a `<rect>` qualifies outright
/// without its transform. Rejected when masked, filtered by an existing filter, or clipped by a
/// non-rectangular clip child.
pub fn is_rectangle(doc: &Doc, n: NodeId, including_transform: bool) -> bool {
    is_rect_rec(doc, n, including_transform, 0)
}

fn is_rect_rec(doc: &Doc, n: NodeId, inc: bool, depth: usize) -> bool {
    if depth > MAX_NEST || !doc.is_element(n) {
        return false;
    }
    let tag = doc.tag(n);
    let shape_ok = if !inc && tag == "rect" {
        true
    } else if matches!(tag, "path" | "rect" | "line" | "polyline") {
        if tag == "path" {
            let letters = doc
                .attr(n, "d")
                .unwrap_or("")
                .chars()
                .filter(|c| PATH_LETTERS.contains(*c))
                .count();
            if !(1..=6).contains(&letters) {
                return false;
            }
        }
        let Some(pp) = shape_path(doc, n) else {
            return false;
        };
        if pp.cmd_start.len() < 5 {
            return false; // fewer than 4 source commands
        }
        let mut pts = end_points(&pp.path);
        if inc {
            let t = doc.transform(n);
            for p in pts.iter_mut() {
                *p = t * *p;
            }
        }
        let xs: Vec<f64> = pts.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
        let range = |v: &[f64]| {
            v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - v.iter().copied().fold(f64::INFINITY, f64::min)
        };
        let tol = 1e-3 * range(&xs).max(range(&ys));
        uniquetol(&xs, tol) == 2 && uniquetol(&ys, tol) == 2
    } else if tag == "use" {
        // upstream quirk kept: a clone of a missing target stays "rectangular"
        match doc.resolve_href(n) {
            Some(t) => is_rect_rec(doc, t, true, depth + 1),
            None => true,
        }
    } else {
        false
    };
    if !shape_ok || clip_ref(doc, n, ClipKind::Mask).is_some() {
        return false;
    }
    if doc
        .specified(n, "filter")
        .as_deref()
        .and_then(url_id)
        .and_then(|id| doc.by_id(id))
        .is_some()
    {
        return false;
    }
    if let Some(c) = clip_ref(doc, n, ClipKind::Clip) {
        let kids: Vec<NodeId> = doc.children(c).filter(|&k| doc.is_element(k)).collect();
        if kids.iter().any(|&k| !is_rect_rec(doc, k, true, depth + 1)) {
            return false;
        }
    }
    true
}
