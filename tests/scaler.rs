mod support;

use std::collections::HashMap;
use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::Rect;
use sciink::ops::Ctx;
use sciink::ops::bbox::bb2;
use sciink::tools::scaler::{find_plot_area, geometric_bbox, global_points, ordinal};
use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

#[allow(dead_code)]
fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap_or_else(|| panic!("no element {i}"))
}
#[allow(dead_code)]
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
