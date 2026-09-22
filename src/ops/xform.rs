//! Baking transforms into geometry and moving elements in root coordinates (spec §B.2 "fuse",
//! "global_transform", "combine_paths"; upstream AT:20–234 `fuseTransform`, DH:1065–1147).

use std::ops::Range;

use kurbo::{Affine, BezPath, PathEl, Point};

use crate::dom::{Doc, NodeId};
use crate::geom::path::{end_points, fmt_d, shape_path};
use crate::geom::{TOL, fmt_transform, inverse, ipx, is_identity, parse_transform, scale_factor};
use crate::num;
use crate::style::Style;
use crate::text::style::composed_width;

use super::cleanup::url_id;
use super::clip::duplicate_into_defs;
use super::style::{composed_list, fix_css_clipmask};
use super::{ClipKind, Ctx, clip_ref, label};

/// Elements whose geometry a transform can be baked into (C:489–497 `otp_support`).
pub const OTP_SUPPORT: &[&str] = &[
    "rect", "ellipse", "circle", "polygon", "polyline", "line", "path",
];

/// Slices of a path (indices into the `BezPath` elements of the written `d`) with the transform
/// each takes — the Scaler's combined-by-colour pieces.
pub type Ranges = Vec<(Range<usize>, Affine)>;

const SHAPE_ATTRS: &[&str] = &[
    "x", "y", "width", "height", "rx", "ry", "cx", "cy", "r", "points", "x1", "y1", "x2", "y2",
];

/// C:504–516 `object_to_path`: `d` from the shape's geometry, tag `path`, the shape attributes
/// removed (**Deviation**: upstream leaves them behind). No-op for `<path>` and non-shapes.
pub fn object_to_path(doc: &mut Doc, el: NodeId) {
    if doc.tag(el) == "path" || !OTP_SUPPORT.contains(&doc.tag(el)) {
        return;
    }
    let Some(pp) = shape_path(doc, el) else {
        return;
    };
    doc.set_attr(el, "d", fmt_d(&pp.path));
    for a in SHAPE_ATTRS {
        doc.remove_attr(el, a);
    }
    doc.set_tag(el, "path");
}

/// AT:67–86 `transform_clipmask`: a transformed element's clip/mask is duplicated and its
/// children take the element's transform, so the geometry can absorb the transform afterwards.
fn transform_clipmask(doc: &mut Doc, ctx: &mut Ctx, el: NodeId, kind: ClipKind) {
    let own = doc.transform(el);
    if is_identity(own) {
        return;
    }
    let Some(clip) = clip_ref(doc, el, kind) else {
        return;
    };
    let d = duplicate_into_defs(doc, ctx, clip);
    let id = doc.ensure_id(d);
    doc.set_attr(el, kind.attr(), format!("url(#{id})"));
    fix_css_clipmask(doc, el, kind);
    let kids: Vec<NodeId> = doc.children(d).filter(|&k| doc.is_element(k)).collect();
    for k in kids {
        let kt = doc.transform(k);
        doc.set_transform(k, own * kt);
    }
}

/// AT:20–32 `remove_attrs`: a `<path>` loses its `sodipodi:*`/`inkscape:*` attributes except
/// the `inkscape-academic*`/`inkscape-scientific*` compatibility ones.
fn remove_inkscape_attrs(doc: &mut Doc, el: NodeId) {
    let names: Vec<String> = doc
        .attrs(el)
        .iter()
        .map(|a| a.name.clone())
        .filter(|n| {
            (n.contains("sodipodi") || n.contains("inkscape"))
                && !n.contains("inkscape-academic")
                && !n.contains("inkscape-scientific")
        })
        .collect();
    for n in names {
        doc.remove_attr(el, &n);
    }
}

/// AT:36–64 `applyToStrokes` + spec §B.2: inline `stroke-width` and a non-`none` inline
/// `stroke-dasharray` are multiplied by the transform's scale factor; an element that only
/// inherits a paint stroke gets an explicit, scaled `stroke-width` (the specified one, default 1).
fn apply_to_strokes(doc: &mut Doc, el: NodeId, t: Affine) {
    let sf = scale_factor(t);
    let inline = doc.attr(el, "style").map(Style::parse).unwrap_or_default();
    let mut st = inline.clone();
    match inline.get("stroke-width").and_then(ipx) {
        Some(w) => st.set("stroke-width", &num::fmt(w * sf)),
        None => {
            let stroked = doc
                .specified(el, "stroke")
                .is_some_and(|s| s.trim() != "none");
            let w = match doc.specified(el, "stroke-width") {
                None => Some(1.0),
                Some(v) => ipx(&v),
            };
            if let (true, Some(w)) = (stroked, w) {
                st.set("stroke-width", &num::fmt(w * sf));
            }
        }
    }
    if let Some(dash) = inline.get("stroke-dasharray") {
        if !dash.trim().eq_ignore_ascii_case("none") {
            let vals: Option<Vec<f64>> = dash
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(ipx)
                .collect();
            if let Some(v) = vals {
                let s: Vec<String> = v.iter().map(|x| num::fmt(x * sf)).collect();
                st.set("stroke-dasharray", &s.join(","));
            }
        }
    }
    if st != inline {
        doc.set_style_map(el, &st);
    }
}

/// AT:224–231 + spec §B.2: a `userSpaceOnUse` gradient paint travels with the geometry — the
/// element gets a duplicate whose `gradientTransform` is `t · old`. An `objectBoundingBox`
/// gradient (the SVG default) follows the new box by itself and is left alone.
fn gradient_fixup(doc: &mut Doc, el: NodeId, t: Affine) {
    for prop in ["fill", "stroke"] {
        let Some(g) = doc
            .specified(el, prop)
            .as_deref()
            .and_then(url_id)
            .and_then(|id| doc.by_id(id))
        else {
            continue;
        };
        if !doc.tag(g).ends_with("Gradient")
            || doc.attr(g, "gradientUnits").map(str::trim) != Some("userSpaceOnUse")
        {
            continue;
        }
        let d = doc.deep_clone(g);
        let defs = doc.defs();
        doc.append_child(defs, d);
        let id = doc.ensure_id(d);
        let gt = doc
            .attr(d, "gradientTransform")
            .and_then(parse_transform)
            .unwrap_or(Affine::IDENTITY);
        match fmt_transform(t * gt) {
            Some(s) => doc.set_attr(d, "gradientTransform", s),
            None => {
                doc.remove_attr(d, "gradientTransform");
            }
        }
        doc.set_style(el, prop, &format!("url(#{id})"));
    }
}

fn transform_el(e: PathEl, t: Affine) -> PathEl {
    match e {
        PathEl::MoveTo(p) => PathEl::MoveTo(t * p),
        PathEl::LineTo(p) => PathEl::LineTo(t * p),
        PathEl::QuadTo(c, p) => PathEl::QuadTo(t * c, t * p),
        PathEl::CurveTo(c1, c2, p) => PathEl::CurveTo(t * c1, t * c2, t * p),
        PathEl::ClosePath => PathEl::ClosePath,
    }
}

/// AT:95–234 `fuseTransform`: bakes `extra · el.transform` into a shape's geometry and removes
/// its `transform`. Groups, text, clones and images are untouched (children are never visited).
/// With `ranges`, the path is rebuilt from the listed element slices, each under its own
/// transform (which must already include everything — `global_transform` prepares them), and
/// `extra · el.transform` is used only for strokes and gradients.
pub fn fuse(
    doc: &mut Doc,
    ctx: &mut Ctx,
    el: NodeId,
    extra: Affine,
    ranges: Option<&Ranges>,
    apply_to_stroke: bool,
) {
    if !OTP_SUPPORT.contains(&doc.tag(el)) {
        return;
    }
    transform_clipmask(doc, ctx, el, ClipKind::Clip);
    transform_clipmask(doc, ctx, el, ClipKind::Mask);
    let transf = extra * doc.transform(el);
    doc.remove_attr(el, "transform");
    if doc.tag(el) == "path" {
        remove_inkscape_attrs(doc, el);
    }
    let [a, b, c, d, _, _] = transf.as_coeffs();
    if (b.abs() > TOL || c.abs() > TOL) && matches!(doc.tag(el), "rect" | "ellipse" | "circle") {
        // rotation or shear: only a path can hold the result
        object_to_path(doc, el);
    }
    if is_identity(transf) && ranges.is_none() {
        return;
    }
    let get =
        |doc: &Doc, name: &str, default: f64| doc.attr(el, name).and_then(ipx).unwrap_or(default);
    let tag = doc.tag(el).to_string();
    match tag.as_str() {
        "polygon" | "polyline" => {
            if let Some(pp) = shape_path(doc, el) {
                let pts: Vec<String> = end_points(&pp.path)
                    .iter()
                    .map(|p| {
                        let q = transf * *p;
                        format!("{},{}", num::fmt(q.x), num::fmt(q.y))
                    })
                    .collect();
                doc.set_attr(el, "points", pts.join(" "));
            }
        }
        "ellipse" | "circle" => {
            let (cx, cy) = (get(doc, "cx", 0.0), get(doc, "cy", 0.0));
            let (rx, ry) = if tag == "circle" {
                let r = get(doc, "r", 0.0);
                (r, r)
            } else {
                (get(doc, "rx", 0.0), get(doc, "ry", 0.0))
            };
            let p1 = transf * Point::new(cx - rx, cy - ry);
            let p2 = transf * Point::new(cx + rx, cy - ry);
            let p3 = transf * Point::new(cx + rx, cy + ry);
            let (edgex, edgey) = (p1.distance(p2), p2.distance(p3));
            doc.set_attr(el, "cx", num::fmt((p1.x + p3.x) / 2.0));
            doc.set_attr(el, "cy", num::fmt((p1.y + p3.y) / 2.0));
            if (edgex - edgey).abs() <= TOL {
                doc.set_tag(el, "circle");
                doc.remove_attr(el, "rx");
                doc.remove_attr(el, "ry");
                doc.set_attr(el, "r", num::fmt(edgex / 2.0));
            } else {
                doc.set_tag(el, "ellipse");
                doc.remove_attr(el, "r");
                doc.set_attr(el, "rx", num::fmt(edgex / 2.0));
                doc.set_attr(el, "ry", num::fmt(edgey / 2.0));
            }
        }
        "line" => {
            let p1 = transf * Point::new(get(doc, "x1", 0.0), get(doc, "y1", 0.0));
            let p2 = transf * Point::new(get(doc, "x2", 0.0), get(doc, "y2", 0.0));
            for (k, v) in [("x1", p1.x), ("y1", p1.y), ("x2", p2.x), ("y2", p2.y)] {
                doc.set_attr(el, k, num::fmt(v));
            }
        }
        "rect" => {
            let (x, y) = (get(doc, "x", 0.0), get(doc, "y", 0.0));
            let (w, h) = (get(doc, "width", 0.0), get(doc, "height", 0.0));
            let corners = [
                Point::new(x, y),
                Point::new(x + w, y),
                Point::new(x + w, y + h),
                Point::new(x, y + h),
            ]
            .map(|p| transf * p);
            let (mut x0, mut y0, mut x1, mut y1) = (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            );
            for p in corners {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            doc.set_attr(el, "x", num::fmt(x0));
            doc.set_attr(el, "y", num::fmt(y0));
            doc.set_attr(el, "width", num::fmt(x1 - x0));
            doc.set_attr(el, "height", num::fmt(y1 - y0));
            // Deviation (spec §B.2): the radii follow the axis scales; upstream leaves them
            let (rx, ry) = (
                doc.attr(el, "rx").and_then(ipx),
                doc.attr(el, "ry").and_then(ipx),
            );
            if rx.is_some() || ry.is_some() {
                let rx0 = rx.or(ry).unwrap_or(0.0);
                let ry0 = ry.or(rx).unwrap_or(0.0);
                doc.set_attr(el, "rx", num::fmt(rx0 * a.abs()));
                doc.set_attr(el, "ry", num::fmt(ry0 * d.abs()));
            }
        }
        _ => {
            if let Some(pp) = shape_path(doc, el) {
                let path = match ranges {
                    None => transf * pp.path,
                    Some(rs) => {
                        let els = pp.path.elements();
                        let mut out = BezPath::new();
                        for (r, t) in rs {
                            let end = r.end.min(els.len());
                            for e in &els[r.start.min(end)..end] {
                                out.push(transform_el(*e, *t));
                            }
                        }
                        out
                    }
                };
                doc.set_attr(el, "d", fmt_d(&path));
            }
        }
    }
    if apply_to_stroke {
        apply_to_strokes(doc, el, transf);
    }
    gradient_fixup(doc, el, transf);
}

/// DH:1065–1107: applies `t` (a root-coordinate transform) to `el` by rewriting `el.transform`
/// as `P⁻¹ · t · P · el.transform` (`P` = the parent's composed transform), then fuses shapes.
/// `ranges` transforms are rewritten the same way and handed to `fuse`. With `preserve_stroke`,
/// the visual stroke width and dashes are kept: `stroke-width := visual_before / sf_after`
/// (**Deviation**: written only when that differs from the current specified width; upstream
/// always writes `stroke-width`, even `1.0` on an untouched group).
pub fn global_transform(
    doc: &mut Doc,
    ctx: &mut Ctx,
    el: NodeId,
    t: Affine,
    ranges: Option<Ranges>,
    preserve_stroke: bool,
) {
    let prt = doc
        .parent(el)
        .filter(|&p| doc.is_element(p))
        .map(|p| doc.composed_transform(p))
        .unwrap_or(Affine::IDENTITY);
    let Some(iprt) = inverse(prt) else {
        ctx.warn.push(format!(
            "{}: singular parent transform, not moved",
            label(doc, el)
        ));
        return;
    };
    let myt = doc.transform(el);
    let newtr = iprt * t * prt * myt;
    let ranges: Option<Ranges> = ranges.map(|rs| {
        rs.into_iter()
            .map(|(r, ti)| (r, iprt * ti * prt * myt))
            .collect()
    });
    let before = composed_width(doc, el, "stroke-width");
    let dashes = composed_list(doc, el, "stroke-dasharray");
    doc.set_transform(el, newtr);
    fuse(doc, ctx, el, Affine::IDENTITY, ranges.as_ref(), true);
    if preserve_stroke {
        let after = composed_width(doc, el, "stroke-width");
        if after.scf > 0.0 {
            let new_w = before.tfs / after.scf;
            if (new_w - after.utfs).abs() > 1e-9 * new_w.abs().max(1.0) {
                doc.set_style(el, "stroke-width", &num::fmt(new_w));
                if let Some(sd) = dashes {
                    let s: Vec<String> = sd.iter().map(|v| num::fmt(v / after.scf)).collect();
                    doc.set_style(el, "stroke-dasharray", &s.join(","));
                }
            }
        }
    }
}
