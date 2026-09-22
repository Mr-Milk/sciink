mod support;

use sciink::dom::{Doc, NodeId};

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}

fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i)
        .unwrap_or_else(|| panic!("no element with id {i}"))
}

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn out(d: &Doc) -> String {
    let mut v = Vec::new();
    d.write(&mut v);
    String::from_utf8(v).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

use sciink::ops::ClipKind;
use sciink::ops::style::{
    compose_style, composed_list, fix_css_clipmask, remove_inline, strokefill,
};
use sciink::style::Style;

#[test]
fn compose_style_pushes_group_declarations_under_the_childs_own() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="g" style="fill:red;opacity:0.5" stroke="blue"><path id="p" style="fill:green;opacity:0.5"/><path id="q" fill="yellow"/></g></svg>"#
    ));
    let gst = d.cascaded_style(id(&d, "g"));
    let p = id(&d, "p");
    compose_style(&mut d, p, &gst);
    let st = Style::parse(d.attr(p, "style").unwrap());
    assert_eq!(st.get("fill"), Some("green"), "the child's own value wins");
    assert_eq!(
        st.get("stroke"),
        Some("blue"),
        "the group's presentation attribute is pushed down"
    );
    assert_eq!(st.get("opacity"), Some("0.25"), "opacities multiply");
    let q = id(&d, "q");
    compose_style(&mut d, q, &gst);
    let st = Style::parse(d.attr(q, "style").unwrap());
    assert_eq!(
        st.get("fill"),
        Some("yellow"),
        "a presentation attribute is the child's own declaration"
    );
    assert_eq!(st.get("opacity"), Some("0.5"));
}

#[test]
fn compose_style_writes_no_opacity_when_neither_side_has_one() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="g" style="fill:red"><path id="p"/></g></svg>"#
    ));
    let gst = d.cascaded_style(id(&d, "g"));
    let p = id(&d, "p");
    compose_style(&mut d, p, &gst);
    assert_eq!(d.attr(p, "style"), Some("fill:red"));
}

#[test]
fn remove_inline_leaves_the_attribute_alone() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="r" clip-path="url(#a)" style="clip-path:url(#b);fill:red"/></svg>"#
    ));
    let r = id(&d, "r");
    remove_inline(&mut d, r, "clip-path");
    assert_eq!(d.attr(r, "style"), Some("fill:red"));
    assert_eq!(d.attr(r, "clip-path"), Some("url(#a)"));
    remove_inline(&mut d, r, "clip-path");
    assert_eq!(d.attr(r, "style"), Some("fill:red"));
}

#[test]
fn fix_css_clipmask_pins_the_attribute_with_an_id_rule() {
    let mut d = doc(&format!(
        r#"<svg {NS}><style>#r{{clip-path:url(#a)}}</style><defs><clipPath id="a"/><clipPath id="b"/></defs><rect id="r" clip-path="url(#b)" style="clip-path:url(#a);fill:red"/></svg>"#
    ));
    let r = id(&d, "r");
    fix_css_clipmask(&mut d, r, ClipKind::Clip);
    let s = out(&d);
    assert!(
        s.contains("#r{clip-path:url(#a)}\n#r{clip-path:url(#b)}</style>"),
        "{s}"
    );
    assert_eq!(d.attr(r, "style"), Some("fill:red"));
    assert_eq!(d.attr(r, "clip-path"), Some("url(#b)"));
    assert_eq!(
        d.sheet_value(r, "clip-path").as_deref(),
        Some("url(#b)"),
        "the appended rule now wins"
    );
    // an agreeing sheet appends nothing; a removed attribute is pinned as `none`
    fix_css_clipmask(&mut d, r, ClipKind::Clip);
    assert_eq!(out(&d).matches("#r{").count(), 2);
    d.remove_attr(r, "clip-path");
    fix_css_clipmask(&mut d, r, ClipKind::Clip);
    assert!(
        out(&d).contains("\n#r{clip-path:none}</style>"),
        "{}",
        out(&d)
    );
    // the mask flavour on a document without a sheet: only the inline copy goes
    let mut d = doc(&format!(
        r#"<svg {NS}><rect mask="url(#m)" style="mask:url(#n)"/></svg>"#
    ));
    let r = d.children(d.svg()).find(|&c| d.is_element(c)).unwrap();
    fix_css_clipmask(&mut d, r, ClipKind::Mask);
    assert_eq!(d.attr(r, "style"), None);
    assert!(
        !out(&d).contains("<style"),
        "no disagreement → no sheet created"
    );
    // a disagreeing sheet elsewhere, an id-less element: id assigned, root <style> created first
    let mut d = doc(&format!(
        r#"<svg {NS}><g><style>rect{{mask:url(#x)}}</style></g><defs/><rect mask="url(#m)"/></svg>"#
    ));
    let r = d
        .descendants(d.svg())
        .find(|&c| d.is_element(c) && d.tag(c) == "rect")
        .unwrap();
    fix_css_clipmask(&mut d, r, ClipKind::Mask);
    let s = out(&d);
    let rid = d.attr(r, "id").expect("an id was assigned").to_string();
    assert!(
        s.starts_with(&format!("<svg {NS}><style>")),
        "root style is the first child: {s}"
    );
    assert!(
        s.contains(&format!("\n#{rid}{{mask:url(#m)}}</style>")),
        "{s}"
    );
    assert_eq!(
        d.sheet_value(r, "mask").as_deref(),
        Some("url(#m)"),
        "the new sheet is seen"
    );
    // a crafted value is never copied into the stylesheet (braces would inject rules)
    let mut d = doc(&format!(
        r#"<svg {NS}><style>#r{{clip-path:url(#a)}}</style><rect id="r" clip-path="url(#a)}} * {{display:none}} #z{{"/></svg>"#
    ));
    let r = id(&d, "r");
    fix_css_clipmask(&mut d, r, ClipKind::Clip);
    assert_eq!(
        d.sheet_value(r, "display"),
        None,
        "no rule was injected into the stylesheet"
    );
    assert_eq!(
        d.sheet_value(r, "clip-path").as_deref(),
        Some("url(#a)"),
        "the sheet still says what it said"
    );
    assert_eq!(out(&d).matches("#r{").count(), 1, "nothing appended");
    assert!(
        out(&d).contains(r#"clip-path="url(#a)} * {display:none} #z{""#),
        "the attribute itself is the document's business and stays"
    );
}

#[test]
fn composed_list_scales_dashes_and_handles_none() {
    let d = doc(&format!(
        r#"<svg {NS}><g transform="scale(2)"><path id="a" style="stroke-dasharray:1, 2 3"/><path id="b" style="stroke-dasharray:none"/><path id="c"/><path id="d" style="stroke-dasharray:1,abc"/></g></svg>"#
    ));
    assert_eq!(
        composed_list(&d, id(&d, "a"), "stroke-dasharray"),
        Some(vec![2.0, 4.0, 6.0])
    );
    assert_eq!(composed_list(&d, id(&d, "b"), "stroke-dasharray"), None);
    assert_eq!(composed_list(&d, id(&d, "c"), "stroke-dasharray"), None);
    assert_eq!(composed_list(&d, id(&d, "d"), "stroke-dasharray"), None);
}

#[test]
fn strokefill_resolves_paints_widths_dashes_and_markers() {
    let d = doc(&format!(
        r#"<svg {NS}><g transform="scale(2)" style="color:#ffffff"><path id="p" style="stroke:#204060;stroke-opacity:0.5;opacity:0.5;stroke-width:2;fill:none;stroke-dasharray:1,2;marker-end:url(#m)"/><path id="q"/><path id="u" style="fill:url(#g);stroke:url(#h);stroke-width:1"/><path id="c" style="fill:currentColor;stroke:white;stroke-width:0"/><path id="w" style="stroke:red;stroke-width:1;stroke-dasharray:1"/></g></svg>"#
    ));
    let p = strokefill(&d, id(&d, "p"));
    let s = p.stroke.unwrap();
    assert_eq!((s.r, s.g, s.b), (0x20, 0x40, 0x60));
    assert!(close(s.alpha, 0.25));
    // L = floor((0x60 + 0x20) / 2) = 64
    assert!(
        close(s.efflightness, 0.25 * 64.0 / 255.0 + 0.75),
        "{}",
        s.efflightness
    );
    assert_eq!(p.fill, None);
    assert!(!p.fill_is_url && !p.stroke_is_url);
    assert!(close(p.stroke_width.unwrap(), 4.0), "2 × scale 2");
    assert_eq!(p.dasharray, Some(vec![2.0, 4.0]));
    assert_eq!(p.marker_end.as_deref(), Some("url(#m)"));
    assert_eq!(p.marker_start, None);
    // defaults: fill black (opaque, lightness 0); no stroke → no width, no dashes
    let q = strokefill(&d, id(&d, "q"));
    let f = q.fill.unwrap();
    assert_eq!((f.r, f.g, f.b), (0, 0, 0));
    assert!(close(f.alpha, 1.0) && close(f.efflightness, 0.0));
    assert_eq!((q.stroke, q.stroke_width, q.dasharray), (None, None, None));
    // url paints
    let u = strokefill(&d, id(&d, "u"));
    assert!(u.fill_is_url && u.stroke_is_url);
    assert_eq!((u.fill, u.stroke, u.stroke_width), (None, None, None));
    // currentColor resolves through `color`; a zero-width stroke is no stroke
    let c = strokefill(&d, id(&d, "c"));
    let f = c.fill.unwrap();
    assert_eq!((f.r, f.g, f.b), (255, 255, 255));
    assert!(close(f.efflightness, 1.0));
    assert_eq!((c.stroke, c.stroke_width), (None, None));
    // a real stroke keeps its dashes; opaque red has L = floor(255 / 2) = 127
    let w = strokefill(&d, id(&d, "w"));
    assert!(close(w.stroke.unwrap().efflightness, 127.0 / 255.0));
    assert_eq!(w.dasharray, Some(vec![2.0]));
}
