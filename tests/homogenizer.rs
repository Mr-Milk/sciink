mod support;

use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::Rect;
use sciink::ops::Ctx;
use sciink::ops::bbox::bb2;
use sciink::tools::homogenizer::inkscape_spec_to_css;
use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn homog(svg: &str, extra: &[&str]) -> Result<(String, Vec<String>), String> {
    let mut a = vec!["--tool=homogenizer", "--tab=scaling"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes()))?;
    Ok((String::from_utf8(out.svg).unwrap(), out.messages))
}
fn ok(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    homog(svg, extra).unwrap_or_else(|e| panic!("homogenizer failed: {e}"))
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap_or_else(|| panic!("no element {i}"))
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn style_of(n: roxmltree::Node) -> sciink::style::Style {
    n.attribute("style")
        .map(sciink::style::Style::parse)
        .unwrap_or_default()
}
fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
/// Visual box of one element, measured by the crate itself.
fn vbox(svg: &str, id_: &str) -> Rect {
    let mut doc = Doc::parse(svg.as_bytes()).unwrap();
    let n = id(&doc, id_);
    let mut ctx = Ctx::new();
    let m = with_vendored_fonts(|| bb2(&mut doc, &mut ctx, &[n], false));
    m[&n]
}
fn composed(svg: &str, id_: &str) -> [f64; 6] {
    let doc = Doc::parse(svg.as_bytes()).unwrap();
    doc.composed_transform(id(&doc, id_)).as_coeffs()
}

#[test]
fn inkscape_font_specifications_become_css() {
    let fams: Vec<String> = ["DejaVu Sans", "Roboto", "Avenir Next"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let css = |s: &str| inkscape_spec_to_css(s, &fams).map(|st| st.to_css());
    assert_eq!(
        css("DejaVu Sans"),
        Some("font-family:DejaVu Sans;".to_string())
    );
    assert_eq!(
        css("DejaVu Sans Bold"),
        Some("font-family:DejaVu Sans;font-weight:bold;".to_string())
    );
    assert_eq!(
        css("dejavu-sans, Bold Italic"),
        Some("font-family:DejaVu Sans;font-weight:bold;font-style:italic;".to_string())
    );
    assert_eq!(
        css("Bold DejaVu Sans"),
        css("DejaVu Sans Bold"),
        "the family may come last"
    );
    assert_eq!(
        css("Avenir Next Semi-Condensed"),
        Some("font-family:Avenir Next;font-stretch:semi-condensed;".to_string())
    );
    assert_eq!(
        css("Roboto Weight500"),
        Some("font-family:Roboto;font-weight:500;".to_string())
    );
    assert_eq!(
        css("Roboto Semi-Bold"),
        Some("font-family:Roboto;font-weight:600;".to_string())
    );
    assert_eq!(
        css("Roboto Normal"),
        Some(
            "font-family:Roboto;font-weight:normal;font-style:normal;font-stretch:normal;"
                .to_string()
        ),
        "Normal is a weight, a style and a stretch"
    );
    assert_eq!(
        css("Sans Light"),
        Some("font-family:Sans;font-weight:300;".to_string()),
        "generic families are always known"
    );
    assert_eq!(css("Nope Sans"), None, "no family and an unknown word");
    assert_eq!(
        css("Roboto Sparkly"),
        None,
        "an unknown style word rejects the whole specification"
    );
    assert_eq!(
        css(""),
        Some(String::new()),
        "an empty specification sets nothing"
    );
}

#[test]
fn errors_follow_upstream() {
    let svg = format!(
        r#"<svg {NS}><image id="i" width="1" height="1"/><rect id="r" width="1" height="1"/><g id="g"><text id="t" style="{DV}">x</text></g></svg>"#
    );
    let e = homog(&svg, &["--id=i"]).unwrap_err();
    assert!(
        e.starts_with("Thanks for using Scientific Inkscape!"),
        "{e}"
    );
    let e = homog(&svg, &["--plotaware=true", "--id=r"]).unwrap_err();
    assert_eq!(
        e,
        "Plot-aware scaling requires that every selected object be a grouped plot."
    );
    let e = homog(
        &svg,
        &["--setfontfamily=true", "--fontfamily=Nope Sans", "--id=g"],
    )
    .unwrap_err();
    assert_eq!(e, "Font seems to be invalid—check its spelling.");
    // an empty selection is a no-op with a message, not an error
    let (s, msgs) = ok(&svg, &[]);
    assert_eq!(msgs, vec!["homogenizer: nothing selected".to_string()]);
    assert!(s.contains(r#"id="t""#));
}

#[test]
fn set_font_family_rewrites_the_family_drops_the_specification_and_keeps_the_centre() {
    let svg = format!(
        r#"<svg {NS}><g id="g"><text id="t" x="30" y="20" style="font-size:10px;font-family:Roboto;-inkscape-font-specification:'Roboto Bold';text-anchor:start">Hello <tspan id="s" style="font-family:Roboto;font-weight:bold">world</tspan></text></g></svg>"#
    );
    let before = vbox(&svg, "t");
    let (s, msgs) = ok(
        &svg,
        &[
            "--setfontfamily=true",
            "--fontfamily=DejaVu Sans Bold",
            "--id=g",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    for i in ["t", "s"] {
        let st = style_of(by_id(&d, i));
        assert_eq!(st.get("font-family"), Some("DejaVu Sans"), "{i}");
        assert_eq!(st.get("font-weight"), Some("bold"), "{i}");
        assert_eq!(
            st.get("font-style"),
            Some("normal"),
            "{i}: the other font properties are reset"
        );
        assert_eq!(st.get("font-stretch"), Some("normal"), "{i}");
        assert_eq!(st.get("-inkscape-font-specification"), None, "{i}");
    }
    // the text moved so that its visual box keeps its centre (DejaVu Sans is wider than Roboto)
    let after = vbox(&s, "t");
    assert!(
        !close(after.width(), before.width(), 1e-3),
        "the box did change: {before:?} vs {after:?}"
    );
    assert!(
        close(after.center().x, before.center().x, 1e-6)
            && close(after.center().y, before.center().y, 1e-6),
        "{before:?} vs {after:?}"
    );
    // font-size alone does not move a text whose family stays
    let (s2, _) = ok(
        &svg,
        &["--setfontfamily=true", "--fontfamily=Roboto", "--id=g"],
    );
    let again = vbox(&s2, "t");
    assert!(close(again.center().x, before.center().x, 1e-6));
}

#[test]
fn distorted_text_becomes_conformal_and_keeps_its_centre() {
    let svg = format!(
        r#"<svg {NS}><g id="g"><text id="t" x="10" y="20" transform="matrix(2,0,0,1,10,20)" style="font-size:10px;{DV}">Hi</text><text id="f" x="10" y="20" transform="matrix(1,0,0,-2,0,0)" style="font-size:10px;{DV}">flip</text></g></svg>"#
    );
    let (bt, bf) = (vbox(&svg, "t"), vbox(&svg, "f"));
    let (s, msgs) = ok(&svg, &["--fixtextdistortion=true", "--id=g"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let c = composed(&s, "t");
    let q = 2.0_f64.sqrt();
    assert!(
        close(c[0], q, 1e-9)
            && close(c[1], 0.0, 1e-9)
            && close(c[2], 0.0, 1e-9)
            && close(c[3], q, 1e-9),
        "uniform sqrt(det): {c:?}"
    );
    let c = composed(&s, "f");
    assert!(
        close(c[0], q, 1e-9) && close(c[3], -q, 1e-9),
        "a flip stays a flip: {c:?}"
    );
    let (at, af) = (vbox(&s, "t"), vbox(&s, "f"));
    assert!(
        close(at.center().x, bt.center().x, 1e-6) && close(at.center().y, bt.center().y, 1e-6),
        "{bt:?} vs {at:?}"
    );
    assert!(close(af.center().x, bf.center().x, 1e-6) && close(af.center().y, bf.center().y, 1e-6));
    // a tspan never gets a transform of its own (Deviation: upstream writes one)
    let svg = format!(
        r#"<svg {NS}><text id="t" transform="scale(2,1)" style="font-size:10px;{DV}">a<tspan id="s">b</tspan></text></svg>"#
    );
    let (s, _) = ok(&svg, &["--fixtextdistortion=true", "--id=t"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(by_id(&d, "s").attribute("transform"), None);
}
