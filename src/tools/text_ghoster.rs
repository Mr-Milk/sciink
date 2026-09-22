//! Text Ghoster (spec §B.3; upstream text_ghoster.py): a blurred, semi-transparent white
//! rounded rectangle behind each selected element, sized from its bounding box and font size.

use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::Common;
use crate::dom::{Doc, NodeId};
use crate::geom::{inverse, ipx, scale_factor};
use crate::num;
use crate::ops::bbox::{LOCAL, bbox};
use crate::ops::{Ctx, label};
use crate::text::style::composed_font_size;

use super::first_line;

/// How far the rectangle extends beyond the element, in units of the font size (TG:20).
pub const EXTENT: f64 = 0.5;
/// Opacity of the rectangle (TG:22).
pub const OPACITY: f64 = 0.75;
/// Standard deviation of the blur as a fraction of the border (TG:24).
pub const STDDEV: f64 = 0.5;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct TextGhosterCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports; unused.
    #[arg(long, default_value = "scaling")]
    pub tab: String,
}

/// TG:48–125 for one element: wrap `el` in a `<g>` appended at the end of its parent, move
/// `el`'s `transform` onto the group, and put the blurred rectangle first in the group, sized
/// from `el`'s own-frame bounding box grown by `EXTENT × font size`. Returns the group; `None`
/// (element wrapped, no rectangle) when the composed transform is singular or `el` has no box.
pub fn ghost(doc: &mut Doc, ctx: &mut Ctx, el: NodeId) -> Option<NodeId> {
    let parent = doc.parent(el).filter(|&p| doc.is_element(p))?;
    let g = doc.new_element("g");
    doc.append_child(parent, g);
    doc.append_child(g, el);
    let t = doc.transform(el);
    doc.set_transform(g, t);
    doc.remove_attr(el, "transform");
    let ct = doc.composed_transform(g);
    inverse(ct)?;
    let bb = bbox(doc, ctx, el, LOCAL)?;
    // TG:89–98: the largest transformed font size among el and its descendants that specify
    // one, in the group's frame (upstream measures with the group's composed transform removed)
    let scf_g = scale_factor(ct);
    let mut fs: Option<f64> = None;
    let nodes: Vec<NodeId> = doc.descendants(el).filter(|&n| doc.is_element(n)).collect();
    for n in nodes {
        if doc.specified(n, "font-size").is_none() {
            continue;
        }
        let w = composed_font_size(doc, n);
        let local = if scf_g > 0.0 { w.tfs / scf_g } else { w.tfs };
        fs = Some(fs.map_or(local, |m| m.max(local)));
    }
    let fs = fs.unwrap_or_else(|| ipx("8pt").unwrap_or(32.0 / 3.0));
    let border = fs * EXTENT;
    let defs = doc.defs();
    let f = doc.new_element("filter");
    doc.prepend_child(defs, f);
    let fid = doc.new_id("filter");
    doc.set_attr(f, "id", fid.clone());
    let blur = doc.new_element("feGaussianBlur");
    doc.set_attr(blur, "stdDeviation", num::fmt(border * STDDEV));
    doc.append_child(f, blur);
    let r = doc.new_element("rect");
    doc.set_attr(r, "x", num::fmt(bb.x0 - border));
    doc.set_attr(r, "y", num::fmt(bb.y0 - border));
    doc.set_attr(r, "width", num::fmt(bb.width() + 2.0 * border));
    doc.set_attr(r, "height", num::fmt(bb.height() + 2.0 * border));
    doc.set_attr(r, "rx", num::fmt(border));
    doc.set_attr(
        r,
        "style",
        format!(
            "fill:#ffffff;stroke:none;filter:url(#{fid});opacity:{}",
            num::fmt(OPACITY)
        ),
    );
    doc.prepend_child(g, r);
    Some(g)
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = TextGhosterCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    let mut ctx = Ctx::new();
    let sel = doc.selection(&cli.common.ids);
    if sel.is_empty() {
        messages.push("text-ghoster: nothing selected".to_string());
    }
    for el in sel {
        if ghost(&mut doc, &mut ctx, el).is_none() {
            ctx.warn.push(format!(
                "{}: no bounding box or singular transform, no rectangle added",
                label(&doc, el)
            ));
        }
    }
    ctx.finish(&mut doc);
    messages.extend(ctx.warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
