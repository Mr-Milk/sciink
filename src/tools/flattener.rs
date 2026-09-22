//! The Flattener (spec §B.3; upstream flatten_plots.py): deep ungroup, rectangle reversions,
//! the text pipeline, duplicate and white-rectangle removal, cleanup.

use std::collections::HashSet;
use std::ffi::OsString;

use clap::Parser;
use kurbo::{Affine, BezPath};

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::geom::inverse;
use crate::geom::path::{parse_d, path_eq};
use crate::num;
use crate::ops::Ctx;
use crate::ops::bbox::{BboxOpts, bbox, is_rectangle};
use crate::ops::cleanup::{strip_attr, strip_whitespace};
use crate::ops::clip::{deswitch, ui_language, ungroup, unlink};
use crate::ops::style::{remove_inline, strokefill};
use crate::ops::xform::object_to_path;
use crate::text::fonts::FontSystem;
use crate::text::kerning::{KerningOptions, remove_kerning};

use super::first_line;

/// Marks an element (and, when it is a container, what upstream calls its "flattening") as
/// excluded (spec §B.4): any non-empty value counts.
pub const EXCLUDE_ATTR: &str = "inkscape-scientific-flattenexclude";
/// Tags that are neither drawn nor flattened themselves (F:221 `gigtags`).
pub const CONTAINER_TAGS: &[&str] = &["namedview", "defs", "metadata", "foreignObject", "g"];

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct FlattenerCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports (`Options`, `Options2`, `Exclusions`).
    #[arg(long, default_value = "Options")]
    pub tab: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub deepungroup: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub fixtext: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub revertpaths: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removeduppaths: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removerectw: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub splitdistant: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergenearby: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removemanualkerning: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergesubsuper: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub reversions: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removetextclips: bool,
    /// 1 = centre, 2 = left, 3 = right, 4 = unchanged.
    #[arg(long, default_value_t = 1)]
    pub justification: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setreplacement: bool,
    #[arg(long, default_value = "Arial")]
    pub replacement: String,
    /// Exclusions page: 1 = mark the selection as not flattened, 2 = flattened again.
    #[arg(long, default_value_t = 1)]
    pub markexc: u8,
    /// Upstream's test switch: duplicate the selection, flatten the original's children with every
    /// fix on, `sans-serif` as the replacement family and centred justification (F:132–174).
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false", hide = true)]
    pub testmode: bool,
    /// Accepted for compatibility with upstream's test suite; the Text Highlight tool draws the same rectangles.
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false", hide = true)]
    pub debugparser: bool,
    /// Accepted for compatibility; unused.
    #[arg(long, default_value = "1.2", hide = true)]
    pub v: String,
}

/// The effective options: text sub-options are ANDed with `fixtext` (F:178–184).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub deepungroup: bool,
    pub fixtext: bool,
    pub revertpaths: bool,
    pub removeduppaths: bool,
    pub removerectw: bool,
    pub splitdistant: bool,
    pub mergenearby: bool,
    pub removemanualkerning: bool,
    pub mergesubsuper: bool,
    pub reversions: bool,
    pub removetextclips: bool,
    pub setreplacement: bool,
    pub replacement: String,
    pub justification: u8,
}

impl Options {
    pub fn from_cli(c: &FlattenerCli) -> Options {
        let t = c.fixtext;
        Options {
            deepungroup: c.deepungroup,
            fixtext: t,
            revertpaths: c.revertpaths,
            removeduppaths: c.removeduppaths,
            removerectw: c.removerectw,
            splitdistant: c.splitdistant && t,
            mergenearby: c.mergenearby && t,
            removemanualkerning: c.removemanualkerning && t,
            mergesubsuper: c.mergesubsuper && t,
            reversions: c.reversions && t,
            removetextclips: c.removetextclips && t,
            setreplacement: c.setreplacement && t,
            replacement: c.replacement.clone(),
            justification: c.justification,
        }
    }

    /// F:162–174: everything on, `sans-serif`, centred.
    pub fn testmode() -> Options {
        Options {
            deepungroup: true,
            fixtext: true,
            revertpaths: true,
            removeduppaths: true,
            removerectw: true,
            splitdistant: true,
            mergenearby: true,
            removemanualkerning: true,
            mergesubsuper: true,
            reversions: true,
            removetextclips: true,
            setreplacement: true,
            replacement: "sans-serif".to_string(),
            justification: 1,
        }
    }
}

/// F:187–194: the Exclusions page sets (`True`) or removes the marker on the selection.
pub fn mark_exclusions(doc: &mut Doc, sel: &[NodeId], exclude: bool) {
    for &el in sel {
        if exclude {
            doc.set_attr(el, EXCLUDE_ATTR, "True");
        } else {
            doc.remove_attr(el, EXCLUDE_ATTR);
        }
    }
}

/// F:132–149 `duplicate_layer1`: every selected element gets a copy inserted right before it —
/// labelled `<label> original`, locked (`sodipodi:insensitive`), at opacity 0.3 — while the
/// original is labelled `<label> flat` and its element children become the selection to flatten.
/// **Deviation:** the copy carries no ids (`Doc::deep_clone` drops them; upstream assigns random ones).
pub fn duplicate_for_testmode(doc: &mut Doc, sel: &[NodeId]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for &el in sel {
        let d = doc.deep_clone(el);
        doc.insert_before(d, el);
        if let Some(label) = doc.attr(el, "inkscape:label").map(str::to_string) {
            doc.set_attr(el, "inkscape:label", format!("{label} flat"));
            doc.set_attr(d, "inkscape:label", format!("{label} original"));
        }
        doc.set_attr(d, "sodipodi:insensitive", "true");
        doc.set_attr(d, "opacity", "0.3");
        out.extend(doc.children(el).filter(|&k| doc.is_element(k)));
    }
    out
}

fn excluded(doc: &Doc, n: NodeId) -> bool {
    doc.attr(n, EXCLUDE_ATTR)
        .is_some_and(|v| !v.trim().is_empty())
}

/// F:195–201 `seld`: the selection (minus excluded elements) and every element under it, in
/// document order, deduplicated, minus the elements that carry the exclusion marker themselves.
pub fn working_set(doc: &Doc, sel: &[NodeId]) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &root in sel {
        if excluded(doc, root) {
            continue;
        }
        for n in doc.descendants(root) {
            if doc.is_element(n) && !excluded(doc, n) && seen.insert(n) {
                out.push(n);
            }
        }
    }
    out
}

/// The members of `els` still in the document.
pub fn attached(doc: &Doc, els: &[NodeId]) -> Vec<NodeId> {
    els.iter()
        .copied()
        .filter(|&n| doc.parent(n).is_some())
        .collect()
}

/// `ngs` (F:224): the attached members of `seld` that are neither containers nor unrendered.
pub fn non_containers(doc: &Doc, seld: &[NodeId]) -> Vec<NodeId> {
    attached(doc, seld)
        .into_iter()
        .filter(|&n| !CONTAINER_TAGS.contains(&doc.tag(n)))
        .collect()
}

/// `gs` (F:223): the attached groups of `seld`.
pub fn groups(doc: &Doc, seld: &[NodeId]) -> Vec<NodeId> {
    attached(doc, seld)
        .into_iter()
        .filter(|&n| doc.tag(n) == "g")
        .collect()
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = FlattenerCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut ctx = Ctx::new();
    let mut sel = doc.selection(&cli.common.ids);
    if cli.tab == "Exclusions" {
        mark_exclusions(&mut doc, &sel, cli.markexc == 1);
        return finish(doc, ctx, false);
    }
    let opts = if cli.testmode {
        sel = duplicate_for_testmode(&mut doc, &sel);
        Options::testmode()
    } else {
        Options::from_cli(&cli)
    };
    let mut seld = working_set(&doc, &sel);
    if opts.deepungroup {
        move_defs_and_clips_to_root(&mut doc, &mut seld);
    }
    if groups(&doc, &seld).is_empty() && non_containers(&doc, &seld).is_empty() {
        return Err("No objects selected!".to_string());
    }
    if opts.deepungroup {
        unlink_clones(&mut doc, &mut ctx, &mut seld);
        deep_ungroup(&mut doc, &mut ctx, &seld, opts.removetextclips);
    }
    let mut ngs = non_containers(&doc, &seld);
    let wrects = if opts.removerectw || opts.reversions || opts.revertpaths {
        rect_passes(&mut doc, &mut ctx, &mut ngs, &opts)
    } else {
        Vec::new()
    };
    if opts.fixtext {
        text_phase(&mut doc, &mut ctx, &mut ngs, &opts);
    }
    let _ = (&wrects, &ngs); // consumed by Task 6
    finish(doc, ctx, true)
}

/// End of every run: created-clip gc and dangling-reference sweep (`Ctx::finish`), whitespace and
/// `unlinked_clone` markers when the document was flattened (F:513–548, spec §B.3 step 9).
fn finish(mut doc: Doc, mut ctx: Ctx, flattened: bool) -> Result<Output, String> {
    ctx.finish(&mut doc);
    if flattened {
        strip_whitespace(&mut doc);
        strip_attr(&mut doc, "unlinked_clone");
    }
    let messages = ctx.warn.0.iter().map(|w| format!("warning: {w}")).collect();
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}

/// Attribute holding the joined comments of a matplotlib text group (spec §B.4).
pub const MPL_COMMENT: &str = "mpl_comment";

/// F:203–218: every selected `<defs>`, `<clipPath>` and `<mask>` is appended to the root `<defs>`
/// (a `<defs>` moves whole, nested); it and its descendants leave the working set. The root
/// `<defs>` itself (and anything containing it) is never moved.
pub fn move_defs_and_clips_to_root(doc: &mut Doc, seld: &mut Vec<NodeId>) {
    for pass in [&["defs"][..], &["clipPath", "mask"][..]] {
        let movers: Vec<NodeId> = seld
            .iter()
            .copied()
            .filter(|&n| doc.parent(n).is_some() && pass.contains(&doc.tag(n)))
            .collect();
        if movers.is_empty() {
            continue;
        }
        let root = doc.defs(); // created on demand — only when something has to move
        for m in movers {
            if m == root || doc.ancestors(root).any(|a| a == m) {
                continue;
            }
            doc.append_child(root, m);
            let gone: HashSet<NodeId> = doc.descendants(m).collect();
            seld.retain(|n| !gone.contains(n));
        }
    }
}

/// F:229–246: every `<use>` of the working set whose target exists and is not a `<symbol>` is
/// unlinked; the clone leaves the set and the copy's subtree joins it.
pub fn unlink_clones(doc: &mut Doc, ctx: &mut Ctx, seld: &mut Vec<NodeId>) {
    let uses: Vec<NodeId> = seld
        .iter()
        .copied()
        .filter(|&n| doc.parent(n).is_some() && doc.tag(n) == "use")
        .collect();
    for u in uses {
        let Some(target) = doc.resolve_href(u) else {
            continue; // a clone of nothing stays (upstream skips it too)
        };
        if doc.tag(target) == "symbol" {
            continue;
        }
        if let Some(copy) = unlink(doc, ctx, u) {
            seld.retain(|&n| n != u);
            seld.extend(doc.descendants(copy).filter(|&n| doc.is_element(n)));
        }
    }
}

/// F:248–274: groups in ascending order of their child count (comments count, text does not;
/// counted before any ungroup; ties in document order). A group with a comment child whose
/// children are all comments, `<defs>` or unlinked clones is a matplotlib text group: it keeps
/// its glyphs grouped, gets `mpl_comment` = its comments joined by `;`, and loses the comments.
/// A group already carrying `mpl_comment` is kept. Everything else is dissolved with `ungroup`.
pub fn deep_ungroup(doc: &mut Doc, ctx: &mut Ctx, seld: &[NodeId], remove_text_clip: bool) {
    let mut gs: Vec<(usize, NodeId)> = groups(doc, seld)
        .into_iter()
        .map(|g| {
            let n = doc
                .children(g)
                .filter(|&k| doc.is_element(k) || doc.is_comment(k))
                .count();
            (n, g)
        })
        .collect();
    gs.sort_by_key(|&(n, _)| n); // stable: ties keep document order
    for (_, g) in gs {
        if doc.parent(g).is_none() {
            continue; // dissolved or clipped out by an earlier ungroup
        }
        let kids: Vec<NodeId> = doc
            .children(g)
            .filter(|&k| doc.is_element(k) || doc.is_comment(k))
            .collect();
        let has_comment = kids.iter().any(|&k| doc.is_comment(k));
        let glyphish = kids.iter().all(|&k| {
            doc.is_comment(k)
                || doc.tag(k) == "defs"
                || doc.attr(k, "unlinked_clone") == Some("True")
        });
        if has_comment && glyphish {
            let cmnt: Vec<String> = kids
                .iter()
                .filter(|&&k| doc.is_comment(k))
                .map(|&k| {
                    doc.comment(k)
                        .unwrap_or("")
                        .trim_matches(|c| matches!(c, '<' | '!' | '-' | ' ' | '>'))
                        .to_string()
                })
                .collect();
            doc.set_attr(g, MPL_COMMENT, cmnt.join(";"));
            for &k in &kids {
                if doc.is_comment(k) {
                    doc.detach(k);
                }
            }
        } else if doc.attr(g, MPL_COMMENT).is_some() {
            // leave grouped
        } else {
            ungroup(doc, ctx, g, remove_text_clip);
        }
    }
}

/// Aspect ratio beyond which a dark filled rectangle is really a line (F:288).
pub const RECT_THRESHOLD: f64 = 2.49;
/// The matplotlib minus-sign glyph (F:285–287); only its first three commands are compared.
pub const MINUS_D: &str = "M 106,355 H 732 V 272 H 106 Z";
/// Tags the rectangle passes look at (F:277).
const RECT_TAGS: &[&str] = &["path", "rect", "line"];
/// Parents that disqualify an element from the rectangle passes (F:281).
const FLOW_TAGS: &[&str] = &["flowPara", "flowRegion", "flowRoot"];

fn first_three(p: &BezPath) -> BezPath {
    BezPath::from_vec(p.elements().iter().take(3).copied().collect())
}

/// `d` starts with the minus glyph's `M 106,355 H 732 V 272` (F:312–315).
pub fn is_minus_glyph(d: &str) -> bool {
    let (Some(p), Some(m)) = (parse_d(d), parse_d(MINUS_D)) else {
        return false;
    };
    p.path.elements().len() >= 3 && path_eq(&first_three(&p.path), &first_three(&m.path), 1e-9)
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// F:326–340: the glyph path becomes a `<text>` holding U+2212 in the group's frame, re-flipped
/// about the glyph's centre when the composed transform mirrors (matplotlib draws glyphs with a
/// negative y scale), with upstream's literal position and size. Returns the new element.
fn revert_minus(
    doc: &mut Doc,
    ctx: &mut Ctx,
    el: NodeId,
    fill: (u8, u8, u8, f64),
) -> Option<NodeId> {
    let parent = doc.parent(el)?;
    let mut t0 = doc.composed_transform(el);
    if t0.determinant() < 0.0 {
        let o = BboxOpts {
            transform: false,
            stroke: false,
            rough: false,
            clip: true,
        };
        if let Some(bb) = bbox(doc, ctx, el, o) {
            let c = bb.center().to_vec2();
            t0 = t0 * Affine::translate(c) * Affine::FLIP_Y * Affine::translate(-c);
        }
    }
    let inv = inverse(doc.composed_transform(parent))?;
    let nt = doc.new_element("text");
    doc.insert_before(nt, el);
    let id = doc.attr(el, "id").map(str::to_string);
    doc.detach(el);
    if let Some(id) = id {
        doc.set_attr(nt, "id", id);
    }
    let txt = doc.new_text("\u{2212}");
    doc.append_child(nt, txt);
    doc.set_transform(nt, inv * t0);
    doc.set_attr(nt, "x", "19.3964");
    doc.set_attr(nt, "y", "626.924");
    let (r, g, b, a) = fill;
    let mut style = format!(
        "font-size:999.997;font-family:sans-serif;fill:{}",
        hex(r, g, b)
    );
    if a != 1.0 {
        // Deviation: upstream drops a translucent fill's alpha
        style.push_str(&format!(";fill-opacity:{}", num::fmt(a)));
    }
    doc.set_attr(nt, "style", style);
    Some(nt)
}

/// F:342–368: a dark, unstroked rectangle at least `RECT_THRESHOLD` times taller than wide (or
/// wider than tall) becomes a stroked centre line of the same colour and thickness.
fn revert_thin(doc: &mut Doc, el: NodeId, bb: kurbo::Rect, fill: (u8, u8, u8, f64)) {
    let (r, g, b, a) = fill;
    let (w, h) = (bb.width(), bb.height());
    let (d, width) = if w < h / RECT_THRESHOLD {
        let xc = bb.center().x;
        (
            format!(
                "M {},{} L {},{}",
                num::fmt(xc),
                num::fmt(bb.y0),
                num::fmt(xc),
                num::fmt(bb.y1)
            ),
            w,
        )
    } else if h < w / RECT_THRESHOLD {
        let yc = bb.center().y;
        (
            format!(
                "M {},{} L {},{}",
                num::fmt(bb.x0),
                num::fmt(yc),
                num::fmt(bb.x1),
                num::fmt(yc)
            ),
            h,
        )
    } else {
        return;
    };
    object_to_path(doc, el);
    doc.set_attr(el, "d", d);
    doc.set_style(el, "stroke", &hex(r, g, b));
    if a != 1.0 {
        doc.set_style(el, "stroke-opacity", &num::fmt(a));
        doc.set_style(el, "opacity", "1");
    }
    doc.set_style(el, "fill", "none");
    doc.set_style(el, "stroke-width", &num::fmt(width));
    doc.set_style(el, "stroke-linecap", "butt");
}

/// F:277–368: over the non-container working set, every unstroked filled rectangle-like element
/// (not inside flowed text) is a white-rectangle candidate when its fill is opaque white; with
/// `reversions` a matplotlib minus glyph becomes a `<text>`; with `revertpaths` a dark thin
/// rectangle becomes a stroke. Returns the white-rectangle candidates; `ngs` gets the reverted
/// texts in place of their glyph paths.
pub fn rect_passes(
    doc: &mut Doc,
    ctx: &mut Ctx,
    ngs: &mut Vec<NodeId>,
    o: &Options,
) -> Vec<NodeId> {
    let mut wrects = Vec::new();
    for el in attached(doc, ngs) {
        if !RECT_TAGS.contains(&doc.tag(el)) {
            continue;
        }
        let Some(parent) = doc.parent(el) else {
            continue;
        };
        if FLOW_TAGS.contains(&doc.tag(parent)) || !is_rectangle(doc, el, false) {
            continue;
        }
        let stroke = doc
            .specified(el, "stroke")
            .unwrap_or_else(|| "none".to_string());
        let fill = doc
            .specified(el, "fill")
            .unwrap_or_else(|| "black".to_string());
        if stroke.trim() != "none" || fill.trim() == "none" {
            continue;
        }
        let sf = strokefill(doc, el);
        let Some(f) = sf.fill else { continue };
        let rgba = (f.r, f.g, f.b, f.alpha);
        if (f.r, f.g, f.b) == (255, 255, 255) && f.alpha == 1.0 {
            wrects.push(el);
        }
        if o.reversions && doc.attr(el, "d").is_some_and(is_minus_glyph) {
            if let Some(nt) = revert_minus(doc, ctx, el, rgba) {
                if let Some(i) = ngs.iter().position(|&n| n == el) {
                    ngs.remove(i);
                }
                ngs.push(nt);
                continue;
            }
        }
        if o.revertpaths && !sf.fill_is_url && f.efflightness < 16.0 / 255.0 {
            let lo = BboxOpts {
                transform: false,
                stroke: false,
                rough: false,
                clip: false,
            };
            if let Some(bb) = bbox(doc, ctx, el, lo) {
                revert_thin(doc, el, bb, rgba);
            }
        }
    }
    wrects
}

/// F:372–388 `setreplacement`: every `<text>`/`<tspan>` of the working set loses its inline
/// `-inkscape-font-specification` and gets `replacement` appended to its family list (or as its
/// family when it has none), unless the list already ends with it.
pub fn replace_fonts(doc: &mut Doc, ngs: &[NodeId], replacement: &str) {
    for el in attached(doc, ngs) {
        if !matches!(doc.tag(el), "text" | "tspan") {
            continue;
        }
        let ff = doc.specified(el, "font-family");
        remove_inline(doc, el, "-inkscape-font-specification");
        let ff = ff.map(|s| s.trim().to_string()).unwrap_or_default();
        if ff.is_empty() || ff == "none" {
            doc.set_style(el, "font-family", replacement);
        } else if ff == replacement {
            // nothing to do
        } else {
            let mut fams: Vec<String> = ff
                .split(',')
                .map(|f| {
                    f.trim_matches(|c| matches!(c, '\'' | '"' | ' '))
                        .to_string()
                })
                .collect();
            if !fams
                .last()
                .is_some_and(|l| l.eq_ignore_ascii_case(replacement))
            {
                fams.push(replacement.to_string());
            }
            doc.set_style(el, "font-family", &fams.join(","));
        }
    }
}

/// F:370–413: font replacement, language switches, the kerning pipeline, text clips.
pub fn text_phase(doc: &mut Doc, ctx: &mut Ctx, ngs: &mut Vec<NodeId>, o: &Options) {
    if o.setreplacement {
        replace_fonts(doc, ngs, &o.replacement);
    }
    if o.removemanualkerning || o.mergesubsuper || o.splitdistant || o.mergenearby {
        let lang = ui_language();
        for sw in attached(doc, ngs) {
            if doc.tag(sw) == "switch" {
                deswitch(doc, ctx, sw, &lang);
            }
        }
        *ngs = attached(doc, ngs);
        let tels: Vec<NodeId> = ngs
            .iter()
            .copied()
            .filter(|&n| matches!(doc.tag(n), "text" | "flowRoot"))
            .collect();
        let kopts = KerningOptions::from_inx(
            o.removemanualkerning,
            o.mergesubsuper,
            o.splitdistant,
            o.mergenearby,
            o.justification,
        );
        let out = remove_kerning(doc, &tels, &kopts, FontSystem::load(), &mut ctx.warn);
        let tset: HashSet<NodeId> = tels.into_iter().collect();
        ngs.retain(|n| !tset.contains(n));
        *ngs = attached(doc, ngs); // tspans of rewritten texts are gone
        ngs.extend(out.into_iter().filter(|&n| doc.parent(n).is_some()));
    }
    if o.removetextclips {
        for el in attached(doc, ngs) {
            if matches!(doc.tag(el), "text" | "flowRoot") {
                doc.remove_attr(el, "clip-path");
                doc.remove_attr(el, "mask");
            }
        }
    }
}
