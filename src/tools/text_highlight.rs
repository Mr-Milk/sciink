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

use super::first_line;
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

/// Per-character rectangles paired with their `pt.chars` index (extent or ink box), so
/// `data-family` can never drift if a character is skipped. The extent variant is
/// `layout::char_extents`; only the ink box has no public twin.
fn char_rects(pt: &ParsedText, ink: bool) -> Vec<(usize, Rect)> {
    if !ink {
        return char_extents(pt);
    }
    let mut out: Vec<(usize, Rect)> = Vec::new();
    for (li, ln) in pt.lines.iter().enumerate() {
        for (ci, ch) in ln.chunks.iter().enumerate() {
            let g = chunk_geom(pt, li, ci);
            for (wi, &c) in ch.chars.iter().enumerate() {
                let p = char_pts_ink_ut(pt, &g, c, wi);
                if !p[0].y.is_nan() {
                    out.push((c, pts_bbox(&p)));
                }
            }
        }
    }
    out.sort_by_key(|(c, _)| *c);
    out
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = TextHighlightCli::try_parse_from(argv).map_err(first_line)?;
    let mut t = crate::log::Timer::new("text-highlight");
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    t.phase("parse", || {
        format!("bytes={} elements={}", input.len(), doc.element_count())
    });
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
        if pt.has_text_path {
            // measured for the char table only; nothing to draw (spec §A.2 skip)
            warn.push(format!(
                "{}: text on a path is not measured",
                doc.attr(el, "id").unwrap_or("<no id>")
            ));
            continue;
        }
        // `Option<usize>` carries each rect's original `pt.chars` index for the two
        // per-char modes, so `data-family` stays attached to the right character even if
        // `char_rects` ever drops an entry (a NaN baseline) ahead of it in the list.
        let exts: Vec<(Option<usize>, Rect)> = match cli.htype.as_str() {
            "char" => char_rects(&pt, false)
                .into_iter()
                .map(|(c, r)| (Some(c), r))
                .collect(),
            "charink" => char_rects(&pt, true)
                .into_iter()
                .map(|(c, r)| (Some(c), r))
                .collect(),
            "chunk" => chunk_extents(&pt).into_iter().map(|r| (None, r)).collect(),
            "line" => line_extents(&pt).into_iter().map(|r| (None, r)).collect(),
            "full" => full_extent(&pt).into_iter().map(|r| (None, r)).collect(),
            _ => full_ink_bbox(&pt).into_iter().map(|r| (None, r)).collect(),
        };
        let tr = fmt_transform(pt.transform);
        for (i, &(idx, e)) in exts.iter().enumerate() {
            let r = doc.new_element("rect");
            doc.set_attr(r, "x", num::fmt(e.x0));
            doc.set_attr(r, "y", num::fmt(e.y0));
            doc.set_attr(r, "width", num::fmt(e.width()));
            doc.set_attr(r, "height", num::fmt(e.height()));
            if let Some(t) = &tr {
                doc.set_attr(r, "transform", t.clone());
            }
            doc.set_attr(r, "style", if i % 2 == 0 { STYLE_EVEN } else { STYLE_ODD });
            if let Some(c) = idx {
                if let Some(face) = pt.chars[c].face {
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
    t.phase("write", || format!("bytes={}", svg.len()));
    t.total(String::new);
    Ok(Output { svg, messages })
}
