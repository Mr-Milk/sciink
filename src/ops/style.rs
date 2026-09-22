//! Style-level operations (spec §B.2 "Style composition", "fix_css_clipmask", "Stroke/fill";
//! upstream DH:359–366, 397–413, 182–194, 1171–1225).

use crate::dom::{Doc, NodeId};
use crate::geom::{ipx, scale_factor};
use crate::num;
use crate::style::Style;
use crate::text::style::composed_width;

use super::ClipKind;
use super::cleanup::url_id;

/// Pushes a group's own declarations under a child's (DH:359–366): the child's inline style
/// becomes `group_style` overridden by `cascaded(child)`, and opacities multiply.
///
/// **Deviation:** `opacity` is written only when either side specified one (upstream writes
/// `opacity:1.0` on every ungrouped child).
pub fn compose_style(doc: &mut Doc, child: NodeId, group_style: &Style) {
    let own = doc.cascaded_style(child);
    let opacity = |st: &Style| st.get("opacity").and_then(|v| v.trim().parse::<f64>().ok());
    let mut merged = group_style.clone();
    merged.merge_over(&own);
    match (opacity(&own), opacity(group_style)) {
        (None, None) => {
            merged.remove("opacity");
        }
        (a, b) => merged.set("opacity", &num::fmt(a.unwrap_or(1.0) * b.unwrap_or(1.0))),
    }
    doc.set_style_map(child, &merged);
}

/// Removes `prop` from the inline `style` attribute only — a same-named presentation attribute
/// (`clip-path="url(#…)"`) is left alone, unlike `Doc::remove_style`.
pub fn remove_inline(doc: &mut Doc, n: NodeId, prop: &str) {
    if let Some(inline) = doc.attr(n, "style") {
        let mut st = Style::parse(inline);
        if st.remove(prop).is_some() {
            doc.set_style_map(n, &st);
        }
    }
}

/// The root `<style>` (first `<style>` child of `<svg>`), created as the first child of `<svg>`
/// when absent (C:915–927).
fn root_style(doc: &mut Doc) -> NodeId {
    let svg = doc.svg();
    if let Some(s) = doc
        .children(svg)
        .find(|&c| doc.is_element(c) && doc.tag(c) == "style")
    {
        return s;
    }
    let s = doc.new_element("style");
    doc.prepend_child(svg, s);
    s
}

/// Appends `add` to the sheet text of `sty` (its last text/CDATA child, or a new text node).
/// Goes through `Doc::set_text` in both cases so the stylesheet cache is invalidated.
fn append_sheet_text(doc: &mut Doc, sty: NodeId, add: &str) {
    let last = doc.children(sty).filter(|&c| doc.text(c).is_some()).last();
    let t = match last {
        Some(t) => t,
        None => {
            let t = doc.new_text("");
            doc.append_child(sty, t);
            t
        }
    };
    let s = format!("{}{add}", doc.text(t).unwrap_or(""));
    doc.set_text(t, &s);
}

/// `none` or `url(#name)` with an XML-name id — the only values worth pinning. Anything else is
/// left unpinned: a crafted attribute value could otherwise inject rules into the document's
/// stylesheet.
fn pinnable(v: &str) -> bool {
    if v == "none" {
        return true;
    }
    url_id(v).is_some_and(|id| {
        let mut cs = id.chars();
        cs.next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && cs.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    })
}

/// DH:397–413: Inkscape lets a stylesheet `clip-path`/`mask` override the attribute, so when the
/// sheet disagrees with the attribute we just wrote, pin the attribute's value with an id rule
/// appended to the root `<style>` (`\n#id{clip-path:url(#x)}`; `none` when the attribute is
/// absent — upstream writes Python's `None` there) and clear any inline-style copy. The rule is
/// appended only when the value is `pinnable`; an unpinnable value is left unpinned rather than
/// copied verbatim into the stylesheet.
pub fn fix_css_clipmask(doc: &mut Doc, n: NodeId, kind: ClipKind) {
    let att = kind.attr();
    if let Some(css) = doc.sheet_value(n, att) {
        let value = doc
            .attr(n, att)
            .map(str::trim)
            .unwrap_or("none")
            .to_string();
        if css.trim() != value && pinnable(&value) {
            let id = doc.ensure_id(n);
            let sty = root_style(doc);
            append_sheet_text(doc, sty, &format!("\n#{id}{{{att}:{value}}}"));
        }
    }
    remove_inline(doc, n, att);
}

/// DH:182–194 `composed_list`: a list-valued length property (`stroke-dasharray`) in visual
/// (transformed) user units; `None` for `none`, absent, or an unparsable entry.
pub fn composed_list(doc: &Doc, n: NodeId, prop: &str) -> Option<Vec<f64>> {
    let v = doc.specified(n, prop)?;
    let v = v.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("none") {
        return None;
    }
    let sf = scale_factor(doc.composed_transform(n));
    v.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| ipx(s).map(|x| x * sf))
        .collect()
}

/// A resolved paint with its effective alpha and lightness against a white background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// `(stroke|fill)-opacity × opacity` (× the colour's own alpha for `rgba()` values).
    pub alpha: f64,
    /// `alpha·L/255 + (1 − alpha)` with `L = floor((max + min)/2)` over the channels — inkex's
    /// integer HSL lightness — so 0 is opaque black and 1 is white or fully transparent.
    pub efflightness: f64,
}

/// What upstream `get_strokefill` (DH:1171–1225) reports about an element's specified style.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrokeFill {
    pub stroke: Option<Rgba>,
    pub fill: Option<Rgba>,
    /// The paint was a `url(#…)` (gradient/pattern): the colour is `None` and this is set.
    pub stroke_is_url: bool,
    pub fill_is_url: bool,
    /// Visual (transformed) stroke width; `None` when there is no stroke colour or it is zero wide.
    pub stroke_width: Option<f64>,
    /// Visual dash lengths; `None` when absent, `none`, or there is no stroke.
    pub dasharray: Option<Vec<f64>>,
    /// Raw specified values, compared verbatim by Combine by Color.
    pub marker_start: Option<String>,
    pub marker_mid: Option<String>,
    pub marker_end: Option<String>,
}

fn rgba(c: svgtypes::Color, alpha: f64) -> Rgba {
    let alpha = alpha * f64::from(c.alpha) / 255.0;
    let max = f64::from(c.red.max(c.green).max(c.blue));
    let min = f64::from(c.red.min(c.green).min(c.blue));
    let l = ((max + min) / 2.0).floor();
    Rgba {
        r: c.red,
        g: c.green,
        b: c.blue,
        alpha,
        efflightness: alpha * l / 255.0 + (1.0 - alpha),
    }
}

/// Parses a paint value into `(colour, is_url)`. `currentColor` resolves through the specified
/// `color` (**Deviation**: upstream treats it as no paint); `none`, `inherit`, `context-*` and
/// unparsable values are no paint.
fn paint(doc: &Doc, n: NodeId, v: &str, alpha: f64) -> (Option<Rgba>, bool) {
    use svgtypes::Paint;
    match Paint::from_str(v.trim()) {
        Ok(Paint::Color(c)) => (Some(rgba(c, alpha)), false),
        Ok(Paint::CurrentColor) => {
            let c = doc
                .specified(n, "color")
                .and_then(|s| s.trim().parse::<svgtypes::Color>().ok());
            (c.map(|c| rgba(c, alpha)), false)
        }
        Ok(Paint::FuncIRI(..)) => (None, true),
        _ => (None, false),
    }
}

fn opacity_of(st: &Style, prop: &str) -> f64 {
    st.get(prop)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(1.0)
}

/// DH:1171–1225 `get_strokefill`. Defaults: stroke `none`, fill `black`, every opacity 1. A
/// zero-width or colourless stroke clears `stroke`, `stroke_width` and `dasharray` together.
pub fn strokefill(doc: &Doc, n: NodeId) -> StrokeFill {
    let sty = doc.specified_style(n);
    let op = opacity_of(&sty, "opacity");
    let (mut stroke, stroke_is_url) = paint(
        doc,
        n,
        sty.get("stroke").unwrap_or("none"),
        opacity_of(&sty, "stroke-opacity") * op,
    );
    let (fill, fill_is_url) = paint(
        doc,
        n,
        sty.get("fill").unwrap_or("black"),
        opacity_of(&sty, "fill-opacity") * op,
    );
    let mut stroke_width = Some(composed_width(doc, n, "stroke-width").tfs);
    let mut dasharray = composed_list(doc, n, "stroke-dasharray");
    if stroke.is_none() || stroke_width.is_none_or(|w| w == 0.0) {
        stroke = None;
        stroke_width = None;
        dasharray = None;
    }
    let raw = |k: &str| sty.get(k).map(|v| v.trim().to_string());
    StrokeFill {
        stroke,
        fill,
        stroke_is_url,
        fill_is_url,
        stroke_width,
        dasharray,
        marker_start: raw("marker-start"),
        marker_mid: raw("marker-mid"),
        marker_end: raw("marker-end"),
    }
}
