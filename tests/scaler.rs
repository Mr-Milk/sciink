mod support;

use std::collections::HashMap;
use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::{Point, Rect};
use sciink::ops::Ctx;
use sciink::ops::bbox::bb2;
use sciink::tools::scaler::{find_plot_area, geometric_bbox, global_points, ordinal};
use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap_or_else(|| panic!("no element {i}"))
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
/// Visual boxes of every element under the plot, then geometric boxes (`SP:271–275`).
fn boxes(doc: &mut Doc, plot: NodeId) -> (HashMap<NodeId, Rect>, HashMap<NodeId, Rect>) {
    let els: Vec<NodeId> = doc
        .descendants(plot)
        .filter(|&n| doc.is_element(n))
        .collect();
    let mut ctx = Ctx::new();
    let fbbs = with_vendored_fonts(|| bb2(doc, &mut ctx, &els, false));
    let gbbs = fbbs
        .iter()
        .map(|(&n, &v)| (n, geometric_bbox(doc, n, v, None)))
        .collect();
    (fbbs, gbbs)
}

/// A plot: a stroked box, two vertical ticks at the bottom edge, one horizontal tick at the
/// left edge, a fat data line, a solid white background rectangle, a tick label.
const PLOT: &str = r#"<g id="plot">
  <rect id="bg" x="0" y="0" width="120" height="100" style="fill:#ffffff;stroke:none"/>
  <path id="box" d="M20,10 H110 V80 H20 Z" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="t1" d="M40,80 V84" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="t2" d="M80,80 V84" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="t3" d="M16,45 H20" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="data" d="M25,70 L60,30 L105,50" style="fill:none;stroke:#1f77b4;stroke-width:3"/>
  <text id="lbl" x="40" y="92" style="font-size:6px;text-anchor:middle;FONT">0.5</text>
</g>"#;

fn plot_svg(extra_root_attrs: &str) -> String {
    format!(
        r#"<svg {NS} {extra_root_attrs}>{}</svg>"#,
        PLOT.replace("FONT", DV)
    )
}

#[test]
fn global_points_and_geometric_bbox_clamp_to_the_visual_box() {
    let svg = format!(
        r#"<svg {NS}><g transform="translate(10,20) scale(2)"><path id="p" d="M0,0 L5,0 L5,5 Z" style="stroke:#000;stroke-width:1"/></g></svg>"#
    );
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let p = id(&doc, "p");
    let pts = global_points(&doc, p, None);
    // M, L, L, Z(=start): four points in root coordinates
    assert_eq!(pts.len(), 4);
    assert!(
        close(pts[1].x, 20.0, 1e-9) && close(pts[1].y, 20.0, 1e-9),
        "{:?}",
        pts[1]
    );
    assert!(close(pts[2].x, 20.0, 1e-9) && close(pts[2].y, 30.0, 1e-9));
    assert!(
        close(pts[3].x, 10.0, 1e-9) && close(pts[3].y, 20.0, 1e-9),
        "Z ends at the start"
    );
    // a range covers only some elements: [1, 3) = the two L's
    let pts = global_points(&doc, p, Some(1..3));
    assert_eq!(pts.len(), 2);
    // the visual box (stroke 1 × scale 2 → 1 unit of padding) is clamped away: the geometric box
    // is the end-point box 10..20 × 20..30
    let (fbbs, _) = boxes(&mut doc, p);
    let vis = fbbs[&p];
    assert!(
        close(vis.x0, 9.0, 1e-6) && close(vis.x1, 21.0, 1e-6),
        "{vis:?}"
    );
    let g = geometric_bbox(&doc, p, vis, None);
    assert!(
        close(g.x0, 10.0, 1e-9)
            && close(g.x1, 20.0, 1e-9)
            && close(g.y0, 20.0, 1e-9)
            && close(g.y1, 30.0, 1e-9),
        "{g:?}"
    );
    // a clipped element: points beyond the visual box are clamped to it
    let g = geometric_bbox(&doc, p, Rect::new(12.0, 22.0, 18.0, 28.0), None);
    assert!(
        close(g.x0, 12.0, 1e-9)
            && close(g.x1, 18.0, 1e-9)
            && close(g.y0, 22.0, 1e-9)
            && close(g.y1, 28.0, 1e-9)
    );
    // non-path-like elements keep the visual box
    let svg = format!(r#"<svg {NS}><text id="t" x="3" y="4" style="{DV}">Hi</text></svg>"#);
    let doc = Doc::parse(svg.as_bytes()).unwrap();
    let vis = Rect::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(geometric_bbox(&doc, id(&doc, "t"), vis, None), vis);
}

#[test]
fn find_plot_area_picks_the_stroked_box_and_classifies_ticks() {
    let svg = plot_svg("");
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let plot = id(&doc, "plot");
    let (_, gbbs) = boxes(&mut doc, plot);
    let kids: Vec<NodeId> = doc.children(plot).filter(|&n| doc.is_element(n)).collect();
    let pa = find_plot_area(&doc, &kids, &gbbs);
    assert_eq!(
        pa.lvel,
        Some(id(&doc, "box")),
        "the framed rectangle is the largest vertical extent"
    );
    assert_eq!(pa.lhel, Some(id(&doc, "box")));
    assert!(
        pa.vl.contains(&id(&doc, "t1")) && pa.vl.contains(&id(&doc, "t2")),
        "vertical ticks"
    );
    assert!(pa.hl.contains(&id(&doc, "t3")), "horizontal tick");
    assert!(!pa.vl.contains(&id(&doc, "data")) && !pa.hl.contains(&id(&doc, "data")));
    assert!(
        !pa.vl.contains(&id(&doc, "bg")),
        "the solid white rectangle is neither a line nor a box"
    );
}

#[test]
fn find_plot_area_falls_back_to_lines_and_honours_plot_area_marks() {
    // no box: the longest vertical and horizontal LINES define the plot area
    let svg = format!(
        r#"<svg {NS}><g id="plot">
  <path id="ax" d="M10,90 H110" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="ay" d="M10,90 V10" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="tick" d="M50,90 V93" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="data" d="M20,80 L60,20 L100,60" style="fill:none;stroke:#f00;stroke-width:2"/>
</g></svg>"#
    );
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let plot = id(&doc, "plot");
    let (_, gbbs) = boxes(&mut doc, plot);
    let kids: Vec<NodeId> = doc.children(plot).filter(|&n| doc.is_element(n)).collect();
    let pa = find_plot_area(&doc, &kids, &gbbs);
    assert_eq!(pa.lvel, Some(id(&doc, "ay")));
    assert_eq!(pa.lhel, Some(id(&doc, "ax")));
    assert!(pa.vl.contains(&id(&doc, "tick")));
    // a marked element wins when it is the largest; a stroked box does not count when a
    // taller marked element exists
    let svg = format!(
        r#"<svg {NS}><g id="plot">
  <rect id="box" x="20" y="20" width="50" height="40" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <rect id="marked" x="10" y="10" width="80" height="70" style="fill:#eee;stroke:none" inkscape-scientific-scaletype="plot_area"/>
</g></svg>"#
    );
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let plot = id(&doc, "plot");
    let (_, gbbs) = boxes(&mut doc, plot);
    let kids: Vec<NodeId> = doc.children(plot).filter(|&n| doc.is_element(n)).collect();
    let pa = find_plot_area(&doc, &kids, &gbbs);
    assert_eq!(pa.lvel, Some(id(&doc, "marked")));
    assert_eq!(pa.lhel, Some(id(&doc, "marked")));
    // nothing box-like at all → None
    let svg = format!(r#"<svg {NS}><g id="plot"><text id="t" style="{DV}">x</text></g></svg>"#);
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let plot = id(&doc, "plot");
    let (_, gbbs) = boxes(&mut doc, plot);
    let kids: Vec<NodeId> = doc.children(plot).filter(|&n| doc.is_element(n)).collect();
    let pa = find_plot_area(&doc, &kids, &gbbs);
    assert_eq!((pa.lvel, pa.lhel), (None, None));
}

#[test]
fn ordinals_follow_upstream() {
    assert_eq!(ordinal(1), "1st");
    assert_eq!(ordinal(2), "2nd");
    assert_eq!(ordinal(3), "3rd");
    assert_eq!(ordinal(4), "4th");
    assert_eq!(ordinal(11), "11th");
    assert_eq!(ordinal(12), "12th");
    assert_eq!(ordinal(13), "13th");
    assert_eq!(ordinal(21), "21st");
    assert_eq!(ordinal(112), "112th");
}

fn scale(svg: &str, extra: &[&str]) -> Result<(String, Vec<String>), String> {
    let mut a = vec!["--tool=scaler"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes()))?;
    Ok((String::from_utf8(out.svg).unwrap(), out.messages))
}
fn ok(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    scale(svg, extra).unwrap_or_else(|e| panic!("scaler failed: {e}"))
}
/// x- and y-extent of an element's global end points in an output document.
fn extent(doc: &Doc, id_: &str) -> (f64, f64, Rect) {
    let pts = global_points(doc, id(doc, id_), None);
    assert!(!pts.is_empty(), "{id_} has geometry");
    let r = Rect::from_points(pts[0], pts[0]);
    let r = pts.iter().fold(r, |r, p| r.union_pt(*p));
    (r.width(), r.height(), r)
}
fn composed(doc: &Doc, id_: &str) -> [f64; 6] {
    doc.composed_transform(id(doc, id_)).as_coeffs()
}
fn is_translation(c: [f64; 6]) -> bool {
    close(c[0], 1.0, 1e-9)
        && close(c[1], 0.0, 1e-9)
        && close(c[2], 0.0, 1e-9)
        && close(c[3], 1.0, 1e-9)
}
fn visual_stroke(doc: &Doc, id_: &str) -> f64 {
    let n = id(doc, id_);
    let w: f64 = doc
        .specified(n, "stroke-width")
        .unwrap()
        .trim_end_matches("px")
        .parse()
        .unwrap();
    w * sciink::geom::scale_factor(doc.composed_transform(n))
}
/// The plot manually scaled by (2, 0.5): sx = 2, sy = 0.5.
fn scaled_plot(extra_group_attrs: &str) -> String {
    plot_svg("").replace(
        r#"<g id="plot">"#,
        &format!(r#"<g id="plot" transform="matrix(2,0,0,0.5,5,7)" {extra_group_attrs}>"#),
    )
}

#[test]
fn advanced_tab_marks_and_clears_the_selection_and_changes_nothing_else() {
    let svg = plot_svg("");
    let (s, msgs) = ok(
        &svg,
        &["--tab=options", "--marksf=2", "--id=box", "--id=lbl"],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        by_id(&d, "box").attribute("inkscape-scientific-scaletype"),
        Some("aspect_locked")
    );
    assert_eq!(
        by_id(&d, "lbl").attribute("inkscape-scientific-scaletype"),
        Some("aspect_locked")
    );
    assert_eq!(
        by_id(&d, "data").attribute("d"),
        Some("M25,70 L60,30 L105,50"),
        "untouched"
    );
    for (m, v) in [("1", "scale_free"), ("3", "normal"), ("4", "plot_area")] {
        let (s, _) = ok(
            &svg,
            &["--tab=options", &format!("--marksf={m}"), "--id=box"],
        );
        let d = roxmltree::Document::parse(&s).unwrap();
        assert_eq!(
            by_id(&d, "box").attribute("inkscape-scientific-scaletype"),
            Some(v)
        );
    }
    let (s, _) = ok(&s, &["--tab=options", "--marksf=5", "--id=box", "--id=lbl"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        by_id(&d, "box").attribute("inkscape-scientific-scaletype"),
        None,
        "cleared"
    );
    assert_eq!(
        by_id(&d, "lbl").attribute("inkscape-scientific-scaletype"),
        None
    );
}

#[test]
fn errors_follow_upstream() {
    let svg = format!(
        r#"<svg {NS}><image id="i" width="1" height="1"/><rect id="r" width="1" height="1"/><g id="g"/></svg>"#
    );
    let e = scale(&svg, &["--tab=correction", "--id=i"]).unwrap_err();
    assert!(
        e.starts_with("Thanks for using Scientific Inkscape!"),
        "{e}"
    );
    let e = scale(&svg, &["--tab=correction", "--id=r"]).unwrap_err();
    assert!(
        e.starts_with("Non-Group objects detected in selection."),
        "{e}"
    );
    let e = scale(&svg, &["--tab=correction"]).unwrap_err();
    assert_eq!(e, "No objects selected!");
    // matching: the first selection may be anything, the plots must be groups
    let e = scale(&svg, &["--tab=matching", "--id=r", "--id=i"]).unwrap_err();
    assert!(
        e.starts_with("Non-Group objects detected in selection."),
        "{e}"
    );
    // the Advanced tab never errors on images
    ok(&svg, &["--tab=options", "--marksf=1", "--id=i"]);
}

#[test]
fn correction_restores_text_and_ticks_and_keeps_the_plot_area_size() {
    let svg = scaled_plot("");
    let (s, msgs) = ok(&svg, &["--tab=correction", "--figuremode=1", "--id=plot"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    // the group's scale is gone (a pure translation is left), the plot area keeps its manual size
    assert!(
        is_translation(out.transform(id(&out, "plot")).as_coeffs()),
        "{:?}",
        out.transform(id(&out, "plot"))
    );
    let (bw, bh, bbox) = extent(&out, "box");
    assert!(
        close(bw, 180.0, 1e-6) && close(bh, 35.0, 1e-6),
        "box {bw} × {bh}"
    );
    // data scales with the plot area, its visual stroke width is preserved (3 × sqrt(2 × 0.5) = 3)
    let (dw, dh, _) = extent(&out, "data");
    assert!(
        close(dw, 160.0, 1e-6) && close(dh, 20.0, 1e-6),
        "data {dw} × {dh}"
    );
    assert!(close(visual_stroke(&out, "data"), 3.0, 1e-6));
    // the label is unscaled again
    assert!(
        is_translation(composed(&out, "lbl")),
        "{:?}",
        composed(&out, "lbl")
    );
    // the bottom ticks keep their length and stay attached to the box's bottom edge
    let (_, th, tbox) = extent(&out, "t1");
    assert!(close(th, 4.0, 1e-6), "tick length {th}");
    assert!(
        close(tbox.y0, bbox.y1, 1e-6),
        "tick top {} on box bottom {}",
        tbox.y0,
        bbox.y1
    );
    // the left tick keeps its length and touches the box's left edge
    let (tw, _, tbox) = extent(&out, "t3");
    assert!(close(tw, 4.0, 1e-6) && close(tbox.x1, bbox.x0, 1e-6));
    // the solid background rectangle is a "normal" element: it scales with the plot
    let (gw, gh, _) = extent(&out, "bg");
    assert!(close(gw, 240.0, 1e-6) && close(gh, 50.0, 1e-6));
}

#[test]
fn tick_correction_can_be_disabled_and_wholeplot_skips_detection() {
    let svg = scaled_plot("");
    let (s, _) = ok(
        &svg,
        &["--tab=correction", "--tickcorrect=false", "--id=plot"],
    );
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (_, th, _) = extent(&out, "t1");
    assert!(close(th, 2.0, 1e-6), "ticks scale with the plot: {th}");
    let (s, msgs) = ok(
        &svg,
        &["--tab=correction", "--wholeplot3=true", "--id=plot"],
    );
    assert!(msgs.is_empty(), "no plot-area warning: {msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    assert!(
        is_translation(composed(&out, "lbl")),
        "text is still scale-free"
    );
    let (_, th, _) = extent(&out, "t1");
    assert!(close(th, 2.0, 1e-6), "no tick correction either");
}

#[test]
fn figure_mode_keeps_the_figure_bounding_box() {
    // labels outside the plot area supply the margins; no background rectangle
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="matrix(2,0,0,0.5,5,7)">
  <path id="box" d="M20,10 H110 V80 H20 Z" style="fill:none;stroke:#000000;stroke-width:0.5"/>
  <path id="data" d="M25,70 L60,30 L105,50" style="fill:none;stroke:#1f77b4;stroke-width:3"/>
  <text id="xl" x="65" y="95" style="font-size:6px;text-anchor:middle;{DV}">time</text>
  <text id="yl" x="8" y="45" style="font-size:6px;text-anchor:middle;{DV}" transform="rotate(-90,8,45)">value</text>
</g></svg>"#
    );
    let before = {
        let mut d = Doc::parse(svg.as_bytes()).unwrap();
        let plot = id(&d, "plot");
        let (f, _) = boxes(&mut d, plot);
        f.values()
            .fold(None, |acc: Option<Rect>, r| {
                Some(acc.map_or(*r, |a| a.union(*r)))
            })
            .unwrap()
    };
    let (s, msgs) = ok(&svg, &["--tab=correction", "--figuremode=2", "--id=plot"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let mut out = Doc::parse(s.as_bytes()).unwrap();
    let plot = id(&out, "plot");
    let (f, _) = boxes(&mut out, plot);
    let after = f
        .values()
        .fold(None, |acc: Option<Rect>, r| {
            Some(acc.map_or(*r, |a| a.union(*r)))
        })
        .unwrap();
    // `after` is re-measured from the serialised output: `num::fmt` writes 8 significant digits,
    // so coordinates near 200 carry ~1e-5 of rounding — 1e-3 is still 5 ppm of the figure
    assert!(
        close(after.x0, before.x0, 1e-3) && close(after.y0, before.y0, 1e-3),
        "top-left kept: {after:?} vs {before:?}"
    );
    assert!(
        close(after.width(), before.width(), 1e-3) && close(after.height(), before.height(), 1e-3),
        "size kept: {after:?} vs {before:?}"
    );
    assert!(is_translation(composed(&out, "xl")), "labels unscaled");
}

#[test]
fn a_plot_without_a_box_warns_and_is_still_scaled() {
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="scale(2,1)">
  <path id="data" d="M0,0 L50,40 L100,10" style="fill:none;stroke:#f00;stroke-width:1"/>
  <text id="t" x="50" y="60" style="font-size:6px;{DV}">x</text>
</g></svg>"#
    );
    let (s, msgs) = ok(&svg, &["--tab=correction", "--id=plot"]);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].starts_with("warning: A box-like plot area could not be automatically detected on the 1st selected plot (group ID plot)."), "{}", msgs[0]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (dw, _, _) = extent(&out, "data");
    assert!(
        close(dw, 200.0, 1e-6),
        "everything is the plot area: data keeps its manual width {dw}"
    );
    assert!(is_translation(composed(&out, "t")));
}

#[test]
fn combined_by_colour_pieces_are_unscaled_one_by_one() {
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="scale(2,1)">
  <path id="box" d="M0,0 H200 V100 H0 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="mk" d="M0,50 L10,50 M50,50 L60,50" style="fill:none;stroke:#00f;stroke-width:1" inkscape-scientific-scaletype="scale_free" inkscape-scientific-combined-by-color="0 2 4"/>
</g></svg>"#
    );
    let (s, msgs) = ok(&svg, &["--tab=correction", "--id=plot"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let mk = id(&out, "mk");
    assert_eq!(
        out.attr(mk, "inkscape-scientific-combined-by-color"),
        Some("0 2 4"),
        "ranges kept"
    );
    let a = global_points(&out, mk, Some(0..2));
    let b = global_points(&out, mk, Some(2..4));
    let len = |p: &[Point]| (p[1].x - p[0].x).abs();
    assert!(
        close(len(&a), 10.0, 1e-6) && close(len(&b), 10.0, 1e-6),
        "each piece keeps its length: {a:?} {b:?}"
    );
    let ca = (a[0].x + a[1].x) / 2.0;
    let cb = (b[0].x + b[1].x) / 2.0;
    assert!(
        close(cb - ca, 100.0, 1e-6),
        "piece centres follow the plot's scale (gap 50 → 100): {}",
        cb - ca
    );
    // the same path WITHOUT the ranges attribute is unscaled as one piece: gap stays 50
    let svg = svg.replace(r#" inkscape-scientific-combined-by-color="0 2 4""#, "");
    let (s, _) = ok(&svg, &["--tab=correction", "--id=plot"]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let mk = id(&out, "mk");
    let (a, b) = (
        global_points(&out, mk, Some(0..2)),
        global_points(&out, mk, Some(2..4)),
    );
    assert!(close(
        (b[0].x + b[1].x) / 2.0 - (a[0].x + a[1].x) / 2.0,
        50.0,
        1e-6
    ));
}

#[test]
fn aspect_locked_children_scale_uniformly() {
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="matrix(4,0,0,1,0,0)">
  <path id="box" d="M0,0 H200 V100 H0 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <rect id="m" x="95" y="45" width="10" height="10" style="fill:#0f0" inkscape-scientific-scaletype="aspect_locked"/>
</g></svg>"#
    );
    let (s, _) = ok(&svg, &["--tab=correction", "--id=plot"]);
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, _) = extent(&out, "m");
    // sqrt(4 × 1) = 2: the marker is scaled by 2 in both directions
    assert!(close(w, 20.0, 1e-6) && close(h, 20.0, 1e-6), "{w} × {h}");
}

/// Two plots side by side: `a` (the target, 90 × 70 plot area) and `b` (60 × 40 plot area,
/// with a label under it). `b_attrs` goes on plot b's group.
fn two_plots(b_attrs: &str) -> String {
    format!(
        r#"<svg {NS}>
<g id="a">
  <path id="abox" d="M20,10 H110 V80 H20 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="adata" d="M25,70 L60,30 L105,50" style="fill:none;stroke:#1f77b4;stroke-width:2"/>
  <text id="al" x="65" y="92" style="font-size:6px;text-anchor:middle;{DV}">a</text>
</g>
<g id="b" {b_attrs}>
  <path id="bbox" d="M200,30 H260 V70 H200 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <path id="bdata" d="M205,60 L230,35 L255,50" style="fill:none;stroke:#d62728;stroke-width:2"/>
  <path id="bt" d="M230,70 V73" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <text id="bl" x="230" y="82" style="font-size:6px;text-anchor:middle;{DV}">b</text>
</g>
<rect id="r" x="300" y="10" width="45" height="35" style="fill:none;stroke:#000;stroke-width:1"/>
</svg>"#
    )
}

#[test]
fn matching_scales_the_plot_area_to_the_first_selection() {
    let svg = two_plots("");
    // match width only (plot areas): b's box becomes 90 wide, height unchanged, text unscaled
    let (s, msgs) = ok(
        &svg,
        &[
            "--tab=matching",
            "--hmatchopts=2",
            "--vmatchopts=1",
            "--matchprop=1",
            "--id=a",
            "--id=b",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, bb) = extent(&out, "bbox");
    assert!(close(w, 90.0, 1e-6) && close(h, 40.0, 1e-6), "{w} × {h}");
    assert!(is_translation(composed(&out, "bl")), "label unscaled");
    assert!(
        close(visual_stroke(&out, "bdata"), 2.0, 1e-6),
        "visual stroke kept"
    );
    let (_, th, tb) = extent(&out, "bt");
    assert!(
        close(th, 3.0, 1e-6) && close(tb.y0, bb.y1, 1e-6),
        "tick length kept, attached to the box"
    );
    // the target plot is untouched
    let (aw, ah, abb) = extent(&out, "abox");
    assert!(close(aw, 90.0, 1e-9) && close(ah, 70.0, 1e-9) && close(abb.x0, 20.0, 1e-9));
    // match height and align vertically: b's box centre y equals a's, heights equal
    let (s, _) = ok(
        &svg,
        &[
            "--tab=matching",
            "--hmatchopts=1",
            "--vmatchopts=3",
            "--id=a",
            "--id=b",
        ],
    );
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, bb) = extent(&out, "bbox");
    assert!(close(w, 60.0, 1e-6) && close(h, 70.0, 1e-6), "{w} × {h}");
    assert!(
        close(bb.center().y, 45.0, 1e-6),
        "aligned to a's plot-area centre y (10..80): {}",
        bb.center().y
    );
    assert!(close(bb.center().x, 230.0, 1e-6), "x untouched");
}

#[test]
fn matching_can_target_a_plain_rectangle_and_delete_it() {
    let svg = two_plots("");
    let (s, msgs) = ok(
        &svg,
        &[
            "--tab=matching",
            "--hmatchopts=3",
            "--vmatchopts=3",
            "--deletematch=true",
            "--id=r",
            "--id=b",
        ],
    );
    assert!(
        msgs.is_empty(),
        "a stroked rectangle IS a plot area: {msgs:?}"
    );
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, bb) = extent(&out, "bbox");
    assert!(close(w, 45.0, 1e-6) && close(h, 35.0, 1e-6), "{w} × {h}");
    assert!(
        close(bb.center().x, 322.5, 1e-6) && close(bb.center().y, 27.5, 1e-6),
        "aligned on the rectangle's centre: {:?}",
        bb.center()
    );
    assert!(out.by_id("r").is_none(), "the first selection was deleted");
    // an unstroked filled rectangle has no plot area: warning, its box is used instead
    let svg = svg.replace(
        r#"style="fill:none;stroke:#000;stroke-width:1""#,
        r#"style="fill:#ccc;stroke:none""#,
    );
    let (s, msgs) = ok(
        &svg,
        &[
            "--tab=matching",
            "--hmatchopts=2",
            "--vmatchopts=2",
            "--id=r",
            "--id=b",
        ],
    );
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        msgs[0].contains("on the 1st selected plot (group ID r)"),
        "{}",
        msgs[0]
    );
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, _) = extent(&out, "bbox");
    assert!(close(w, 45.0, 1e-6) && close(h, 35.0, 1e-6));
}

#[test]
fn matching_bounding_boxes_matches_the_whole_figure() {
    let svg = two_plots("");
    let (s, _) = ok(
        &svg,
        &[
            "--tab=matching",
            "--hmatchopts=2",
            "--vmatchopts=2",
            "--matchprop=2",
            "--id=a",
            "--id=b",
        ],
    );
    let mut out = Doc::parse(s.as_bytes()).unwrap();
    let (a, b) = (id(&out, "a"), id(&out, "b"));
    let union_of = |m: &std::collections::HashMap<NodeId, Rect>| {
        m.values()
            .fold(None, |acc: Option<Rect>, r| {
                Some(acc.map_or(*r, |u| u.union(*r)))
            })
            .unwrap()
    };
    // the match target of a group is its visual box (geometric_bbox of a non-path-like element);
    // after matching, the geometric union of b's CHILDREN has that size: the margins (label
    // below) are kept and the box grew by exactly the difference. The group's own entry is its
    // visual box (stroke-padded), so it is left out; 1e-3 covers num::fmt's 8-digit round trip.
    let (fa, _) = boxes(&mut out, a);
    let (_, mut gb) = boxes(&mut out, b);
    gb.remove(&b);
    let (ua, gb) = (union_of(&fa), union_of(&gb));
    assert!(
        close(gb.width(), ua.width(), 1e-3) && close(gb.height(), ua.height(), 1e-3),
        "{gb:?} vs {ua:?}"
    );
}

#[test]
fn a_scaled_plot_is_corrected_before_matching_with_fresh_boxes() {
    let svg = two_plots(r#"transform="matrix(2,0,0,2,-300,-40)""#);
    let (s, msgs) = ok(
        &svg,
        &[
            "--tab=matching",
            "--hmatchopts=2",
            "--vmatchopts=2",
            "--id=a",
            "--id=b",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let (w, h, _) = extent(&out, "bbox");
    assert!(
        close(w, 90.0, 1e-6) && close(h, 70.0, 1e-6),
        "matched after the correction pre-pass: {w} × {h}"
    );
    assert!(
        is_translation(composed(&out, "bl")),
        "the pre-pass unscaled the label: {:?}",
        composed(&out, "bl")
    );
    let (_, th, _) = extent(&out, "bt");
    assert!(close(th, 3.0, 1e-6), "and the tick: {th}");
}

#[test]
fn a_degenerate_group_transform_warns_and_does_not_panic() {
    let svg = format!(
        r#"<svg {NS}><g id="plot" transform="matrix(0,0,0,0,10,10)"><path id="box" d="M0,0 H10 V10 H0 Z" style="fill:none;stroke:#000;stroke-width:0.5"/><text id="t" style="font-size:4px;{DV}">x</text></g></svg>"#
    );
    let (_, msgs) = ok(&svg, &["--tab=correction", "--id=plot"]);
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("warning: ") && m.contains("degenerate")),
        "{msgs:?}"
    );
}
