//! Combine by Color (spec §B.3; upstream combine_by_color.py): merges selected path-like
//! elements that share stroke, fill, width, dashes and markers into one path each, leaving
//! dark ones (axes, ticks) alone.

use std::collections::HashSet;
use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::Common;
use crate::dom::{Doc, NodeId};
use crate::ops::Ctx;
use crate::ops::style::{Rgba, StrokeFill, strokefill};
use crate::ops::xform::combine_paths;

use super::first_line;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct CombineByColorCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports; unused.
    #[arg(long, default_value = "scaling")]
    pub tab: String,
    /// Lightness threshold in percent: strokes or fills darker than this are left alone.
    #[arg(long, default_value_t = 15.0)]
    pub lightnessth: f64,
}

const SKIP_TAGS: &[&str] = &[
    "namedview",
    "defs",
    "metadata",
    "foreignObject",
    "g",
    "missing-glyph",
];

/// CBC:39–60: the selection and its descendants (document order, deduplicated), keeping the
/// path-like elements — not a skipped tag, and carrying `d`, `points` or `x1`.
pub fn candidates(doc: &Doc, ids: &[String]) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in doc.selection(ids) {
        for n in doc.descendants(root) {
            if !doc.is_element(n) || SKIP_TAGS.contains(&doc.tag(n)) {
                continue;
            }
            let pathlike = ["d", "points", "x1"]
                .iter()
                .any(|a| doc.attr(n, a).is_some());
            if pathlike && seen.insert(n) {
                out.push(n);
            }
        }
    }
    out
}

fn same_width(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() < 0.001,
        _ => false,
    }
}

fn same_paint(a: Option<Rgba>, b: Option<Rgba>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            (x.r, x.g, x.b) == (y.r, y.g, y.b) && (x.alpha - y.alpha).abs() < 0.001
        }
        _ => false,
    }
}

fn same_dashes(a: &Option<Vec<f64>>, b: &Option<Vec<f64>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| (p - q).abs() < 1e-3)
        }
        _ => false,
    }
}

/// CBC:80–112: same stroke width (±0.001), same stroke and fill (rgb exact, alpha ±0.001),
/// same dashes (±0.001 each) and identical raw markers.
pub fn mergeable(a: &StrokeFill, b: &StrokeFill) -> bool {
    same_width(a.stroke_width, b.stroke_width)
        && same_paint(a.stroke, b.stroke)
        && same_paint(a.fill, b.fill)
        && same_dashes(&a.dasharray, &b.dasharray)
        && a.marker_start == b.marker_start
        && a.marker_mid == b.marker_mid
        && a.marker_end == b.marker_end
}

/// CBC:62–125 over `els` (document order): from the last element backwards, every light element
/// (no paint darker than `threshold`) gathers the earlier, still unmerged, mergeable ones and
/// becomes their `combine_paths` target (it is the topmost). Returns how many paths were merged
/// away.
pub fn combine_by_color(doc: &mut Doc, ctx: &mut Ctx, els: &[NodeId], threshold: f64) -> usize {
    let sfs: Vec<StrokeFill> = els.iter().map(|&e| strokefill(doc, e)).collect();
    let is_url = |sf: &StrokeFill| sf.stroke_is_url || sf.fill_is_url;
    let mut merged = vec![false; els.len()];
    let mut removed = 0;
    for ii in (0..els.len()).rev() {
        // Deviation: an element already welded into a later one is gone from the document
        if merged[ii] || is_url(&sfs[ii]) {
            continue;
        }
        let sf1 = &sfs[ii];
        let light = |p: &Option<Rgba>| p.is_none_or(|c| c.efflightness >= threshold);
        if !(light(&sf1.stroke) && light(&sf1.fill)) {
            continue;
        }
        let mut merges = vec![ii];
        merged[ii] = true;
        for jj in 0..ii {
            if !merged[jj] && !is_url(&sfs[jj]) && mergeable(sf1, &sfs[jj]) {
                merges.push(jj);
                merged[jj] = true;
            }
        }
        if merges.len() > 1 {
            let group: Vec<NodeId> = merges.iter().map(|&k| els[k]).collect();
            if combine_paths(doc, ctx, &group, 0) {
                removed += group.len() - 1;
            }
        }
    }
    removed
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = CombineByColorCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    let mut ctx = Ctx::new();
    if doc.selection(&cli.common.ids).is_empty() {
        messages.push("combine-by-color: nothing selected".to_string());
    } else {
        let els = candidates(&doc, &cli.common.ids);
        combine_by_color(&mut doc, &mut ctx, &els, cli.lightnessth / 100.0);
        ctx.finish(&mut doc);
    }
    messages.extend(ctx.warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
