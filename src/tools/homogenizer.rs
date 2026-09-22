//! Homogenizer (spec §B.3 "Homogenizer"; upstream homogenizer.py): sets font size, font family,
//! stroke width and transform hygiene on a selection without moving anything's centre.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::geom::{Affine, Rect, inverse, union};
use crate::num;
use crate::ops::Ctx;
use crate::ops::bbox::bb2;
use crate::ops::style::remove_inline;
use crate::ops::xform::{OTP_SUPPORT, fuse, global_transform};
use crate::style::Style;
use crate::text::fonts::FontSystem;
use crate::text::style::{baseline_shift, composed_width};

use super::first_line;
use super::scaler::{find_plot_area, geometric_bbox, warn_non_plot};

/// Never restyled (`HG:31–39`).
pub const BAD_TAGS: &[&str] = &[
    "namedview",
    "defs",
    "metadata",
    "foreignObject",
    "font",
    "font-face",
    "missing-glyph",
];
/// Text-like elements the text options touch (`HG:139–143`).
pub const TEXTLIKE: &[&str] = &["text", "tspan", "flowRoot", "flowPara", "flowSpan"];
/// `HG:122–130` (whitespace normalised).
pub const IMAGE_ERR: &str = "Thanks for using Scientific Inkscape!\n\nIt appears that you're attempting to homogenize a raster Image object. Please note that Inkscape is mainly for working with vector images, not raster images. Vector images preserve all of the information used to generate them, whereas raster images do not. Read about the difference here:\nhttps://en.wikipedia.org/wiki/Vector_graphics\n\nUnfortunately, this means that there is not much the Homogenizer can do to edit raster images. If you want to edit a raster image, you will need to use a program like Photoshop or GIMP.";
/// `HG:231`.
pub const INVALID_FONT: &str = "Font seems to be invalid—check its spelling.";

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct HomogenizerCli {
    #[command(flatten)]
    pub common: Common,
    #[arg(long, default_value = "scaling")]
    pub tab: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setfontsize: bool,
    #[arg(long, default_value_t = 8.0)]
    pub fontsize: f64,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub fixtextdistortion: bool,
    /// 2 fixed pt, 3 scale %, 4 scale max to pt, 5 mean, 6 median, 7 min, 8 max
    #[arg(long, default_value_t = 1)]
    pub fontmodes: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setfontfamily: bool,
    #[arg(long, default_value = "")]
    pub fontfamily: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setstroke: bool,
    #[arg(long, default_value_t = 1.0)]
    pub setstrokew: f64,
    /// 2 fixed px, 3 scale %, 5 mean, 6 median, 7 min, 8 max
    #[arg(long, default_value_t = 1)]
    pub strokemodes: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub clearclipmasks: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub fusetransforms: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub plotaware: bool,
}

/// `FP:1323–1383`: an Inkscape font specification ("DejaVu Sans Bold Italic") to CSS. The
/// longest run of words at the start or the end that names an installed family (or Serif, Sans,
/// System-ui, Monospace) is the family; every remaining word must be a Pango weight, style or
/// stretch word (or `weightNNN`), else `None`. Punctuation and case are ignored.
pub fn inkscape_spec_to_css(fstr: &str, families: &[String]) -> Option<Style> {
    fn clean(s: &str) -> String {
        let kept: String = s
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || c.is_whitespace())
            .collect();
        kept.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
    const WEIGHTS: &[(&str, &str)] = &[
        ("ultralight", "200"),
        ("light", "300"),
        ("semilight", "350"),
        ("medium", "500"),
        ("semibold", "600"),
        ("bold", "bold"),
        ("ultrabold", "800"),
        ("heavy", "900"),
        ("normal", "normal"),
        ("book", "380"),
        ("thin", "100"),
        ("ultraheavy", "1000"),
    ];
    const STRETCHES: &[(&str, &str)] = &[
        ("ultracondensed", "ultra-condensed"),
        ("extracondensed", "extra-condensed"),
        ("condensed", "condensed"),
        ("semicondensed", "semi-condensed"),
        ("normal", "normal"),
        ("semiexpanded", "semi-expanded"),
        ("expanded", "expanded"),
        ("extraexpanded", "extra-expanded"),
        ("ultraexpanded", "ultra-expanded"),
    ];
    const STYLES: &[(&str, &str)] = &[
        ("italic", "italic"),
        ("oblique", "oblique"),
        ("normal", "normal"),
    ];
    // A closure here cannot express the borrow (its return type's lifetime must be tied to `t`,
    // which closures cannot express via elision); a plain fn item can.
    fn look<'a>(t: &[(&'a str, &'a str)], w: &str) -> Option<&'a str> {
        t.iter().find(|(k, _)| *k == w).map(|(_, v)| *v)
    }

    let cstr = clean(fstr);
    let mut fullfams: Vec<String> = families.to_vec();
    fullfams.extend(["Serif", "Sans", "System-ui", "Monospace"].map(String::from));
    let fmnames: Vec<String> = fullfams.iter().map(|f| clean(f)).collect();
    let words: Vec<&str> = cstr.split(' ').filter(|w| !w.is_empty()).collect();
    let (mut longest, mut match_len, mut prefix) = (String::new(), 0usize, true);
    for i in 1..=words.len() {
        let cur = words[..i].join(" ");
        if fmnames.contains(&cur) && cur.len() > longest.len() {
            (longest, match_len, prefix) = (cur, i, true);
        }
    }
    for i in 1..=words.len() {
        let cur = words[words.len() - i..].join(" ");
        if fmnames.contains(&cur) && cur.len() > longest.len() {
            (longest, match_len, prefix) = (cur, i, false);
        }
    }
    let (fam, stylews): (Option<&str>, Vec<&str>) = if longest.is_empty() {
        (None, words.clone())
    } else {
        let idx = fmnames
            .iter()
            .position(|n| *n == longest)
            .expect("matched above");
        let rest = if prefix {
            words[match_len..].to_vec()
        } else {
            words[..words.len() - match_len].to_vec()
        };
        (Some(fullfams[idx].as_str()), rest)
    };
    let (mut weight, mut style, mut stretch): (Option<String>, Option<&str>, Option<&str>) =
        (None, None, None);
    for w in stylews {
        let mut understood = false;
        if let Some(v) = look(WEIGHTS, w) {
            weight = Some(v.to_string());
            understood = true;
        } else if let Some(d) = w
            .strip_prefix("weight")
            .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
        {
            weight = Some(d.to_string());
            understood = true;
        }
        if let Some(v) = look(STYLES, w) {
            style = Some(v);
            understood = true;
        }
        if let Some(v) = look(STRETCHES, w) {
            stretch = Some(v);
            understood = true;
        }
        if !understood {
            return None;
        }
    }
    let mut sty = Style::default();
    if let Some(f) = fam {
        sty.set("font-family", f);
    }
    if let Some(w) = weight {
        sty.set("font-weight", &w);
    }
    if let Some(s) = style {
        sty.set("font-style", s);
    }
    if let Some(s) = stretch {
        sty.set("font-stretch", s);
    }
    Some(sty)
}

/// Upstream's `font-size` rounding (`HG:207–208`): two decimals when `|v| > 1`, else three
/// significant digits, trailing zeros trimmed, `px` appended.
pub fn fmt_font_size(v: f64) -> String {
    let rounded = if v.abs() > 1.0 {
        (v * 100.0).round() / 100.0
    } else {
        format!("{v:.2e}").parse::<f64>().unwrap_or(v)
    };
    format!("{}px", num::fmt(rounded))
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}
fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// `HG:153–208`: the largest character size (pt) of every text, the target from `mode`
/// (2 fixed pt, 3 scale %, 4 scale so the largest becomes `fontsize` pt, 5–8 mean/median/min/max
/// of the sizes), then every text and every descendant whose SPECIFIED (inherited) font-size
/// resolves is rewritten: relative spans (`%` or a baseline shift) as a percentage of their
/// parent, the rest absolute.
pub(crate) fn set_font_size(
    doc: &mut Doc,
    ctx: &mut Ctx,
    tels: &[NodeId],
    fontsize: f64,
    mode: u8,
) {
    let onept = (4.0 / 3.0) / doc.px_per_uu(); // 1 pt in user units (`cdocsize.unittouu("1pt")`)
    let mut szs: Vec<(NodeId, f64)> = Vec::new();
    for &el in tels {
        let Some(pt) = ctx.parse_text(doc, el) else {
            continue;
        };
        let max = pt
            .chars
            .iter()
            .map(|c| c.tfs / onept)
            .fold(f64::NEG_INFINITY, f64::max);
        if max.is_finite() {
            szs.push((el, max));
        }
    }
    let values: Vec<f64> = szs.iter().map(|(_, v)| *v).collect();
    if values.is_empty() {
        ctx.warn
            .push("font size: no text could be measured; nothing changed".to_string());
        return;
    }
    let (mut fontsize, mut fixedscale) = (fontsize, false);
    let stat = |f: fn(&[f64]) -> f64| f(&values);
    match mode {
        3 => fixedscale = true,
        4 => {
            fixedscale = true;
            let m = stat(|v| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
            fontsize = fontsize / m * 100.0;
        }
        5 => fontsize = stat(mean),
        6 => fontsize = stat(median),
        7 => fontsize = stat(|v| v.iter().cloned().fold(f64::INFINITY, f64::min)),
        8 => fontsize = stat(|v| v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
        _ => {}
    }
    for (el, _) in szs {
        let nodes: Vec<NodeId> = doc.descendants(el).filter(|&n| doc.is_element(n)).collect();
        for &d in nodes.iter().rev() {
            let sty = doc.specified_style(d);
            if d != el && sty.get("font-size").is_none() {
                continue;
            }
            let fs = composed_width(doc, d, "font-size");
            if fs.tfs == 0.0 {
                continue;
            }
            let bshift = baseline_shift(doc, d, &sty);
            let relative = bshift != 0.0 || sty.get("font-size").is_some_and(|v| v.contains('%'));
            if relative {
                // a sub/superscript stays relative to its parent
                let Some(parent) = doc.parent(d).filter(|&p| doc.is_element(p)) else {
                    continue;
                };
                let pfs = composed_width(doc, parent, "font-size").tfs;
                if pfs == 0.0 {
                    continue;
                }
                doc.set_style(d, "font-size", &format!("{:.2}%", fs.tfs / pfs * 100.0));
            } else {
                let scl = if fixedscale {
                    fontsize / 100.0
                } else {
                    fontsize * onept / fs.tfs
                };
                doc.set_style(d, "font-size", &fmt_font_size(fs.utfs * scl));
            }
        }
    }
    ctx.reset_char_table();
}

/// `HG:210–225`: replace each text's composed transform by the conformal one with the same
/// area (`sqrt|det|`), rotation and flip. Deviation: only `text`/`flowRoot` — upstream also
/// "fixes" tspans, writing `transform` attributes they cannot carry.
pub(crate) fn fix_distortion(doc: &mut Doc, ctx: &mut Ctx, tels: &[NodeId]) {
    for &el in tels {
        let ct = doc.composed_transform(el);
        let [a, b, c, d, e, f] = ct.as_coeffs();
        let det = a * d - b * c;
        let m = (a * a + b * b).sqrt();
        if det == 0.0 || m == 0.0 {
            continue;
        }
        let sgn = if det < 0.0 { -1.0 } else { 1.0 };
        let q = det.abs().sqrt();
        let ctnew = Affine::new([
            a * q / m,
            b * q / m,
            -b * q * sgn / m,
            a * q * sgn / m,
            e,
            f,
        ]);
        let Some(ict) = inverse(ct) else { continue };
        global_transform(doc, ctx, el, ctnew * ict, None, true);
    }
}

/// `HG:227–246`: the specification's CSS onto every text-like element (children last, as
/// upstream's `reversed(sel)`), the Inkscape specification dropped. Deviation: upstream's
/// `character_fixer` (Avenir/Whitney non-letters into 'Avenir Next'/'Arial' tspans) is not
/// ported — the text engine falls back per character when a face lacks a glyph.
pub(crate) fn set_font_family(
    doc: &mut Doc,
    ctx: &mut Ctx,
    sel_text: &[NodeId],
    spec: &str,
) -> Result<(), String> {
    let fonts = FontSystem::load();
    let Some(mut sty) = inkscape_spec_to_css(spec, &fonts.families()) else {
        return Err(INVALID_FONT.to_string());
    };
    const FACE: [&str; 3] = ["font-weight", "font-style", "font-stretch"];
    if FACE.iter().any(|k| sty.get(k).is_some()) {
        for k in FACE {
            if sty.get(k).is_none() {
                sty.set(k, "normal");
            }
        }
    }
    for &el in sel_text.iter().rev() {
        for (k, v) in &sty.0 {
            doc.set_style(el, k, v);
        }
        remove_inline(doc, el, "-inkscape-font-specification");
    }
    ctx.reset_char_table();
    Ok(())
}

/// `HG:248–319`: after restyling, move every text so its visual box keeps the centre it had
/// (`bbs` = boxes before). Plot-aware (Task 8): texts outside a plot area keep their scaled
/// distance to it instead.
pub(crate) fn recentre(
    doc: &mut Doc,
    ctx: &mut Ctx,
    sel0: &[NodeId],
    tels: &[NodeId],
    bbs: &HashMap<NodeId, Rect>,
    plotaware: bool,
) {
    ctx.reset_char_table(); // sizes and families changed: measure with a fresh table (BB2(…, True))
    let bbs2 = bb2(doc, ctx, tels, false);
    if !plotaware {
        for &el in tels {
            let (Some(b1), Some(b2)) = (bbs.get(&el), bbs2.get(&el)) else {
                continue;
            };
            let d = b1.center() - b2.center();
            global_transform(doc, ctx, el, Affine::translate((d.x, d.y)), None, true);
        }
        return;
    }
    let gbbs: HashMap<NodeId, Rect> = bbs
        .iter()
        .map(|(&n, &v)| (n, geometric_bbox(doc, n, v, None)))
        .collect();
    for (i0, &g) in sel0.iter().enumerate() {
        let pels: Vec<NodeId> = doc
            .children(g)
            .filter(|&k| doc.is_element(k) && bbs.contains_key(&k))
            .collect();
        let pa = find_plot_area(doc, &pels, &gbbs);
        let (lvel, lhel) = match (pa.lvel, pa.lhel) {
            (Some(v), Some(h)) => (Some(v), Some(h)),
            _ => {
                let gid = doc.attr(g, "id").unwrap_or("").to_string();
                warn_non_plot(&mut ctx.warn, i0, &gid);
                (None, None)
            }
        };
        let mut bbp: Option<Rect> = None;
        for &el in &pels {
            if Some(el) == lvel || Some(el) == lhel {
                bbp = union(bbp, gbbs.get(&el).copied());
            }
        }
        let texts: Vec<NodeId> = doc.descendants(g).filter(|n| tels.contains(n)).collect();
        for el in texts {
            let (Some(&b1), Some(&b2)) = (bbs.get(&el), bbs2.get(&el)) else {
                continue;
            };
            let centred = (b1.center().x - b2.center().x, b1.center().y - b2.center().y);
            let (dx, dy) = match bbp {
                Some(p) if b1.width() > 0.0 && b1.height() > 0.0 => {
                    let dx = if b1.center().x < p.x0 {
                        (p.x0 - b2.x1) - (p.x0 - b1.x1) * b2.width() / b1.width()
                    } else if b1.center().x > p.x1 {
                        (b1.x0 - p.x1) * b2.width() / b1.width() - (b2.x0 - p.x1)
                    } else {
                        centred.0
                    };
                    let dy = if b1.center().y < p.y0 {
                        (p.y0 - b2.y1) - (p.y0 - b1.y1) * b2.height() / b1.height()
                    } else if b1.center().y > p.y1 {
                        (b1.y0 - p.y1) * b2.height() / b1.height() - (b2.y0 - p.y1)
                    } else {
                        centred.1
                    };
                    (dx, dy)
                }
                _ => centred,
            };
            global_transform(doc, ctx, el, Affine::translate((dx, dy)), None, true);
        }
    }
}

/// `HG:321–361` with the spec's restriction to elements whose specified stroke is not `none`
/// (upstream writes a width on every element, unstroked ones included, and its statistics count
/// them at the default width 1). Mode 2 = fixed px (converted to user units), 3 = scale %,
/// 5–8 = mean/median/min/max of the visual widths. Written as `visual / sf` + `px`.
pub(crate) fn set_stroke(doc: &mut Doc, ctx: &mut Ctx, sela: &[NodeId], setstrokew: f64, mode: u8) {
    let stroked: Vec<(NodeId, f64, f64)> = sela
        .iter()
        .filter(|&&n| {
            doc.specified(n, "stroke")
                .is_some_and(|s| s.trim() != "none")
        })
        .map(|&n| {
            let w = composed_width(doc, n, "stroke-width");
            (n, w.tfs, w.scf)
        })
        .collect();
    if stroked.is_empty() {
        ctx.warn.push(
            "stroke width: no stroked elements in the selection; nothing changed".to_string(),
        );
        return;
    }
    let widths: Vec<f64> = stroked.iter().map(|(_, w, _)| *w).collect();
    let mut fixedscale = false;
    let target = match mode {
        2 => setstrokew / doc.px_per_uu(),
        3 => {
            fixedscale = true;
            setstrokew
        }
        5 => mean(&widths),
        6 => median(&widths),
        7 => widths.iter().cloned().fold(f64::INFINITY, f64::min),
        8 => widths.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        _ => setstrokew,
    };
    for (n, w, sf) in stroked {
        if sf == 0.0 {
            continue;
        }
        let new = if fixedscale {
            w * target / 100.0
        } else {
            target
        };
        doc.set_style(n, "stroke-width", &format!("{}px", num::fmt(new / sf)));
    }
}

/// `HG:363–369`: bake each shape's composed transform into its geometry (and stroke), then give
/// it the inverse of its parent's composed transform — the path data ends up in global
/// coordinates. Elements under a singular transform are left alone with a warning.
pub(crate) fn fuse_all(doc: &mut Doc, ctx: &mut Ctx, sela: &[NodeId]) {
    for &el in sela {
        if !OTP_SUPPORT.contains(&doc.tag(el)) {
            continue;
        }
        let Some(parent) = doc.parent(el) else {
            continue;
        };
        let parent_ct = doc.composed_transform(parent);
        let Some(inv) = inverse(parent_ct) else {
            ctx.warn.push(format!(
                "{}: singular parent transform; not fused",
                crate::ops::label(doc, el)
            ));
            continue;
        };
        // upstream (HG:367–369) puts the COMPOSED transform on the element, fuses, then leaves the
        // parent's inverse: `fuse` adjusts clips and masks by the element's own transform only
        // (its `extra` reaches geometry, strokes and gradients but not `transform_clipmask`), so
        // the whole composed transform must sit on the element when it runs
        doc.set_transform(el, parent_ct * doc.transform(el));
        fuse(doc, ctx, el, Affine::IDENTITY, None, true);
        doc.set_transform(el, inv);
    }
}

/// `HG:372–375` as the spec reads it: the `clip-path`/`mask` attributes and inline values go;
/// an inline `none` is written only where a stylesheet rule would otherwise still apply one.
pub(crate) fn clear_clipmasks(doc: &mut Doc, sela: &[NodeId]) {
    for &el in sela {
        for prop in ["clip-path", "mask"] {
            doc.remove_attr(el, prop);
            remove_inline(doc, el, prop);
            if doc.sheet_value(el, prop).is_some() {
                doc.set_style(el, prop, "none");
            }
        }
    }
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = HomogenizerCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let mut messages: Vec<String> = Vec::new();
    let sel0 = doc.selection(&cli.common.ids);
    let mut ctx = Ctx::for_roots(sel0.clone());
    // HG:118: the selection and every descendant, each once, document order
    let mut sel: Vec<NodeId> = Vec::new();
    let mut seen: HashSet<NodeId> = HashSet::new();
    for &r in &sel0 {
        for n in doc.descendants(r).filter(|&n| doc.is_element(n)) {
            if seen.insert(n) {
                sel.push(n);
            }
        }
    }
    if sel0.is_empty() {
        // Deviation: upstream shows IMAGE_ERR for an empty selection (`all([])` is true)
        messages.push("homogenizer: nothing selected".to_string());
        return finish(doc, ctx, messages);
    }
    if sel.iter().all(|&n| doc.tag(n) == "image") {
        return Err(IMAGE_ERR.to_string());
    }
    if cli.plotaware && sel0.iter().any(|&n| doc.tag(n) != "g") {
        return Err(
            "Plot-aware scaling requires that every selected object be a grouped plot.".to_string(),
        );
    }
    let sela: Vec<NodeId> = sel
        .iter()
        .copied()
        .filter(|&n| !BAD_TAGS.contains(&doc.tag(n)))
        .collect();
    let sel_text: Vec<NodeId> = sel
        .iter()
        .copied()
        .filter(|&n| TEXTLIKE.contains(&doc.tag(n)))
        .collect();
    let tels: Vec<NodeId> = sel_text
        .iter()
        .copied()
        .filter(|&n| matches!(doc.tag(n), "text" | "flowRoot"))
        .collect();
    let text_opts = cli.setfontfamily || cli.setfontsize || cli.fixtextdistortion;
    // HG:145–151: the boxes before any change
    let bbs: HashMap<NodeId, Rect> = if !text_opts {
        HashMap::new()
    } else if cli.plotaware {
        bb2(&mut doc, &mut ctx, &sel, false)
    } else {
        bb2(&mut doc, &mut ctx, &tels, false)
    };
    if cli.setfontsize {
        set_font_size(&mut doc, &mut ctx, &tels, cli.fontsize, cli.fontmodes);
    }
    if cli.fixtextdistortion {
        fix_distortion(&mut doc, &mut ctx, &tels);
    }
    if cli.setfontfamily {
        set_font_family(&mut doc, &mut ctx, &sel_text, &cli.fontfamily)?;
    }
    if text_opts {
        recentre(&mut doc, &mut ctx, &sel0, &tels, &bbs, cli.plotaware);
    }
    if cli.setstroke {
        set_stroke(&mut doc, &mut ctx, &sela, cli.setstrokew, cli.strokemodes);
    }
    if cli.fusetransforms {
        fuse_all(&mut doc, &mut ctx, &sela);
    }
    if cli.clearclipmasks {
        clear_clipmasks(&mut doc, &sela);
    }
    finish(doc, ctx, messages)
}

fn finish(mut doc: Doc, mut ctx: Ctx, mut messages: Vec<String>) -> Result<Output, String> {
    ctx.finish(&mut doc);
    messages.extend(ctx.warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
