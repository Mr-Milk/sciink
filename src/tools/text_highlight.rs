//! Debug tool: draw the parser's character/chunk/line extents as rectangles (upstream make_highlights).

use std::ffi::OsString;

use clap::Parser;
use kurbo::Rect;

use crate::Output;
use crate::cli::Common;
use crate::dom::Doc;
use crate::geom::fmt_transform;
use crate::num;
use crate::text::Warnings;
use crate::text::fonts::FontSystem;
use crate::text::layout::{
    char_extents, char_pts_ink_ut, chunk_extents, chunk_geom, full_extent, full_ink_bbox,
    line_extents, pts_bbox,
};
use crate::text::parse::ParsedText;
use crate::text::table::CharTable;

use super::font_probe::text_elements;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct TextHighlightCli {
    #[command(flatten)]
    pub common: Common,
    #[arg(long, default_value = "char", value_parser = ["char", "charink", "chunk", "line", "full", "fullink"])]
    pub htype: String,
}

const STYLE_EVEN: &str = "fill:#007575;fill-opacity:0.4675";
const STYLE_ODD: &str = "fill:#007575;fill-opacity:0.5675";

/// Untransformed ink box per character, in `pt.chars` order.
fn char_ink_extents(pt: &ParsedText) -> Vec<Rect> {
    let mut out: Vec<(usize, Rect)> = Vec::new();
    for (li, ln) in pt.lines.iter().enumerate() {
        for (ci, ch) in ln.chunks.iter().enumerate() {
            let g = chunk_geom(pt, li, ci);
            for (wi, &c) in ch.chars.iter().enumerate() {
                out.push((c, pts_bbox(&char_pts_ink_ut(pt, &g, c, wi))));
            }
        }
    }
    out.sort_by_key(|(c, _)| *c);
    out.into_iter().map(|(_, r)| r).collect()
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = TextHighlightCli::try_parse_from(argv).map_err(|e| {
        e.to_string()
            .lines()
            .next()
            .unwrap_or("invalid arguments")
            .to_string()
    })?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let els = text_elements(&doc, &cli.common.ids);
    let mut warn = Warnings::default();
    let mut ct = CharTable::build(&doc, &els, FontSystem::load(), &mut warn);
    let (mut nrect, mut ntext, mut nflow) = (0usize, 0usize, 0usize);
    let root = doc.svg();
    for el in els {
        let Some(pt) = ParsedText::parse(&mut doc, el, &mut ct, &mut warn) else {
            continue;
        };
        // ponytail: brief's sample counted `ntext` only for non-flow elements, which
        // undercounts against its own test ("2 text elements, 1 flows skipped" for one
        // flow + one non-flow element parsed here); `ntext` is every parsed <text>/
        // <flowRoot> (flows included), and `nflow` is how many of those were flows.
        ntext += 1;
        if pt.is_flow {
            nflow += 1;
            continue;
        }
        let per_char = matches!(cli.htype.as_str(), "char" | "charink");
        let exts: Vec<Rect> = match cli.htype.as_str() {
            "char" => char_extents(&pt),
            "charink" => char_ink_extents(&pt),
            "chunk" => chunk_extents(&pt),
            "line" => line_extents(&pt),
            "full" => full_extent(&pt).into_iter().collect(),
            _ => full_ink_bbox(&pt).into_iter().collect(),
        };
        let tr = fmt_transform(pt.transform);
        for (i, e) in exts.iter().enumerate() {
            let r = doc.new_element("rect");
            doc.set_attr(r, "x", num::fmt(e.x0));
            doc.set_attr(r, "y", num::fmt(e.y0));
            doc.set_attr(r, "width", num::fmt(e.width()));
            doc.set_attr(r, "height", num::fmt(e.height()));
            if let Some(t) = &tr {
                doc.set_attr(r, "transform", t.clone());
            }
            doc.set_attr(r, "style", if i % 2 == 0 { STYLE_EVEN } else { STYLE_ODD });
            if per_char {
                if let Some(face) = pt.chars.get(i).and_then(|c| c.face) {
                    doc.set_attr(r, "data-family", ct.fonts.face_info(face).family.clone());
                }
            }
            doc.append_child(root, r);
            nrect += 1;
        }
    }
    let mut messages = vec![format!(
        "highlighted {nrect} rectangles ({ntext} text elements, {nflow} flows skipped)"
    )];
    messages.extend(warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
