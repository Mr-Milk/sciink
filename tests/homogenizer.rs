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
        Some("font-family:DejaVu Sans".to_string())
    );
    assert_eq!(
        css("DejaVu Sans Bold"),
        Some("font-family:DejaVu Sans;font-weight:bold".to_string())
    );
    // punctuation and case are ignored (upstream strips punctuation without inserting spaces, so a
    // hyphenated "dejavu-sans" can never match "DejaVu Sans" — upstream rejects it too)
    assert_eq!(
        css("dejavu sans, Bold Italic"),
        Some("font-family:DejaVu Sans;font-weight:bold;font-style:italic".to_string())
    );
    assert_eq!(
        css("Bold DejaVu Sans"),
        css("DejaVu Sans Bold"),
        "the family may come last"
    );
    assert_eq!(
        css("Avenir Next Semi-Condensed"),
        Some("font-family:Avenir Next;font-stretch:semi-condensed".to_string())
    );
    assert_eq!(
        css("Roboto Weight500"),
        Some("font-family:Roboto;font-weight:500".to_string())
    );
    assert_eq!(
        css("Roboto Semi-Bold"),
        Some("font-family:Roboto;font-weight:600".to_string())
    );
    assert_eq!(
        css("Roboto Normal"),
        Some(
            "font-family:Roboto;font-weight:normal;font-style:normal;font-stretch:normal"
                .to_string()
        ),
        "Normal is a weight, a style and a stretch"
    );
    assert_eq!(
        css("Sans Light"),
        Some("font-family:Sans;font-weight:300".to_string()),
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
    // coefficients are re-read from the serialised output (`num::fmt`, 8 significant digits)
    let c = composed(&s, "t");
    let q = 2.0_f64.sqrt();
    assert!(
        close(c[0], q, 1e-6)
            && close(c[1], 0.0, 1e-6)
            && close(c[2], 0.0, 1e-6)
            && close(c[3], q, 1e-6),
        "uniform sqrt(det): {c:?}"
    );
    let c = composed(&s, "f");
    assert!(
        close(c[0], q, 1e-6) && close(c[3], -q, 1e-6),
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

/// Three texts of 10, 20 and 40 px (7.5, 15 and 30 pt at 1 px/uu) in a group.
fn three_texts() -> String {
    format!(
        r#"<svg {NS}><g id="g">
<text id="a" x="10" y="20" style="font-size:10px;{DV}">aaa</text>
<text id="b" x="10" y="50" style="font-size:20px;{DV}">bbb</text>
<text id="c" x="10" y="100" style="font-size:40px;{DV}">ccc</text>
</g></svg>"#
    )
}
fn font_size(s: &str, id_: &str) -> String {
    let d = roxmltree::Document::parse(s).unwrap();
    style_of(by_id(&d, id_))
        .get("font-size")
        .unwrap()
        .to_string()
}

#[test]
fn font_size_modes_follow_upstream() {
    let svg = three_texts();
    // 2: fixed 7 pt → 7 × 4/3 px = 9.33px everywhere
    let (s, _) = ok(
        &svg,
        &[
            "--setfontsize=true",
            "--fontmodes=2",
            "--fontsize=7",
            "--id=g",
        ],
    );
    for i in ["a", "b", "c"] {
        assert_eq!(font_size(&s, i), "9.33px", "{i}");
    }
    // 3: scale 50 %
    let (s, _) = ok(
        &svg,
        &[
            "--setfontsize=true",
            "--fontmodes=3",
            "--fontsize=50",
            "--id=g",
        ],
    );
    assert_eq!(
        (font_size(&s, "a"), font_size(&s, "b"), font_size(&s, "c")),
        ("5px".into(), "10px".into(), "20px".into())
    );
    // 4: scale so the largest becomes 15 pt (20 px): everything halves
    let (s, _) = ok(
        &svg,
        &[
            "--setfontsize=true",
            "--fontmodes=4",
            "--fontsize=15",
            "--id=g",
        ],
    );
    assert_eq!(
        (font_size(&s, "a"), font_size(&s, "b"), font_size(&s, "c")),
        ("5px".into(), "10px".into(), "20px".into())
    );
    // 5 mean 17.5 pt = 23.33px, 6 median 15 pt = 20px, 7 min 7.5 pt = 10px, 8 max 30 pt = 40px
    for (mode, want) in [
        ("5", "23.33px"),
        ("6", "20px"),
        ("7", "10px"),
        ("8", "40px"),
    ] {
        let (s, _) = ok(
            &svg,
            &[
                "--setfontsize=true",
                &format!("--fontmodes={mode}"),
                "--id=g",
            ],
        );
        for i in ["a", "b", "c"] {
            assert_eq!(font_size(&s, i), want, "mode {mode}, {i}");
        }
    }
    // small values keep three significant digits: 1 px × 50 % = 0.5px
    let svg = format!(r#"<svg {NS}><text id="t" style="font-size:1px;{DV}">x</text></svg>"#);
    let (s, _) = ok(
        &svg,
        &[
            "--setfontsize=true",
            "--fontmodes=3",
            "--fontsize=50",
            "--id=t",
        ],
    );
    assert_eq!(font_size(&s, "t"), "0.5px");
}

#[test]
fn font_size_respects_transforms_document_scale_and_relative_spans() {
    // 2 px per uu (width 200 px over a 100-unit viewBox): 7 pt = 9.333 px = 4.667 uu; the group
    // scales by 2, so the untransformed size written is 4.667 / 2 = 2.33px
    let svg = format!(
        r#"<svg {NS} width="200" height="100" viewBox="0 0 100 50"><g id="g" transform="scale(2)"><text id="t" x="5" y="10" style="font-size:10px;{DV}">Hi <tspan id="p" style="font-size:50%">half</tspan> <tspan id="s" style="font-size:65%;baseline-shift:super">2</tspan> <tspan id="k" style="font-size:20px">big</tspan></text></g></svg>"#
    );
    let before = vbox(&svg, "t");
    let (s, msgs) = ok(
        &svg,
        &[
            "--setfontsize=true",
            "--fontmodes=2",
            "--fontsize=7",
            "--id=g",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    assert_eq!(font_size(&s, "t"), "2.33px");
    // relative spans stay relative: their size as a percentage of the parent's
    assert_eq!(font_size(&s, "p"), "50.00%");
    assert_eq!(
        font_size(&s, "s"),
        "65.00%",
        "a superscript is relative even if its size were absolute"
    );
    // an absolute span becomes the target size too
    assert_eq!(font_size(&s, "k"), "2.33px");
    // and the text keeps its centre
    let after = vbox(&s, "t");
    assert!(
        close(after.center().x, before.center().x, 1e-6)
            && close(after.center().y, before.center().y, 1e-6),
        "{before:?} vs {after:?}"
    );
}

#[test]
fn plot_aware_recentring_keeps_the_scaled_distance_to_the_plot_area() {
    let svg = format!(
        r#"<svg {NS}><g id="plot">
  <path id="box" d="M40,10 H140 V80 H40 Z" style="fill:none;stroke:#000;stroke-width:0.5"/>
  <text id="yl" x="30" y="45" style="font-size:8px;text-anchor:end;{DV}">left</text>
  <text id="in" x="90" y="45" style="font-size:8px;text-anchor:middle;{DV}">inside</text>
  <text id="bl" x="90" y="95" style="font-size:8px;text-anchor:middle;{DV}">below</text>
</g></svg>"#
    );
    let (b_yl, b_in, b_bl) = (vbox(&svg, "yl"), vbox(&svg, "in"), vbox(&svg, "bl"));
    let (s, msgs) = ok(
        &svg,
        &[
            "--setfontsize=true",
            "--fontmodes=3",
            "--fontsize=200",
            "--plotaware=true",
            "--id=plot",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let (a_yl, a_in, a_bl) = (vbox(&s, "yl"), vbox(&s, "in"), vbox(&s, "bl"));
    // left of the plot area (x0 = 40): the gap to the area scales with the box width
    let gap_before = 40.0 - b_yl.x1;
    let gap_after = 40.0 - a_yl.x1;
    assert!(
        close(gap_after, gap_before * a_yl.width() / b_yl.width(), 1e-6),
        "{gap_before} → {gap_after}"
    );
    assert!(
        close(a_yl.center().y, b_yl.center().y, 1e-6),
        "vertically inside: centred"
    );
    // inside: centred both ways
    assert!(
        close(a_in.center().x, b_in.center().x, 1e-6)
            && close(a_in.center().y, b_in.center().y, 1e-6)
    );
    // below the plot area (y1 = 80): the gap scales with the box height
    let gap_before = b_bl.y0 - 80.0;
    let gap_after = a_bl.y0 - 80.0;
    assert!(
        close(gap_after, gap_before * a_bl.height() / b_bl.height(), 1e-6),
        "{gap_before} → {gap_after}"
    );
    // a group without a plot area warns and falls back to plain centring
    let svg = format!(
        r#"<svg {NS}><g id="plot"><text id="t" x="10" y="10" style="font-size:8px;{DV}">alone</text></g></svg>"#
    );
    let b = vbox(&svg, "t");
    let (s, msgs) = ok(
        &svg,
        &[
            "--setfontsize=true",
            "--fontmodes=3",
            "--fontsize=200",
            "--plotaware=true",
            "--id=plot",
        ],
    );
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        msgs[0].contains("on the 1st selected plot (group ID plot)"),
        "{}",
        msgs[0]
    );
    let a = vbox(&s, "t");
    assert!(close(a.center().x, b.center().x, 1e-6) && close(a.center().y, b.center().y, 1e-6));
}

#[test]
fn stroke_width_modes_follow_upstream_on_stroked_elements_only() {
    // 2 px per uu; the group scales by 4 (sf = 4)
    let svg = format!(
        r#"<svg {NS} width="200" height="100" viewBox="0 0 100 50"><g id="g" transform="scale(4)">
<path id="a" d="M0,0 H10" style="fill:none;stroke:#000;stroke-width:0.25"/>
<path id="b" d="M0,1 H10" style="fill:none;stroke:#000;stroke-width:0.5"/>
<path id="c" d="M0,2 H10" style="fill:none;stroke:#000;stroke-width:1.5"/>
<path id="n" d="M0,3 H10" style="fill:#000;stroke:none"/>
<path id="u" d="M0,4 H10"/>
</g></svg>"#
    );
    let sw = |s: &str, i: &str| -> Option<String> {
        let d = roxmltree::Document::parse(s).unwrap();
        style_of(by_id(&d, i))
            .get("stroke-width")
            .map(str::to_string)
    };
    // 2: fixed 3 px = 1.5 uu visual → written 1.5 / 4 = 0.375px
    let (s, msgs) = ok(
        &svg,
        &[
            "--setstroke=true",
            "--strokemodes=2",
            "--setstrokew=3",
            "--id=g",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    for i in ["a", "b", "c"] {
        assert_eq!(sw(&s, i).as_deref(), Some("0.375px"), "{i}");
    }
    assert_eq!(
        sw(&s, "n").as_deref(),
        None,
        "stroke:none gets no width (Deviation)"
    );
    assert_eq!(
        sw(&s, "u").as_deref(),
        None,
        "no stroke at all gets no width"
    );
    // 3: scale 50 %
    let (s, _) = ok(
        &svg,
        &[
            "--setstroke=true",
            "--strokemodes=3",
            "--setstrokew=50",
            "--id=g",
        ],
    );
    assert_eq!(
        (
            sw(&s, "a").unwrap(),
            sw(&s, "b").unwrap(),
            sw(&s, "c").unwrap()
        ),
        (
            "0.125px".to_string(),
            "0.25px".to_string(),
            "0.75px".to_string()
        )
    );
    // visual widths 1, 2, 6: mean 3 → 0.75px, median 2 → 0.5px, min 1 → 0.25px, max 6 → 1.5px
    for (mode, want) in [
        ("5", "0.75px"),
        ("6", "0.5px"),
        ("7", "0.25px"),
        ("8", "1.5px"),
    ] {
        let (s, _) = ok(
            &svg,
            &[
                "--setstroke=true",
                &format!("--strokemodes={mode}"),
                "--id=g",
            ],
        );
        for i in ["a", "b", "c"] {
            assert_eq!(sw(&s, i).as_deref(), Some(want), "mode {mode}, {i}");
        }
    }
    // no stroked element at all: a warning, nothing written
    let svg =
        format!(r#"<svg {NS}><path id="n" d="M0,0 H1" style="fill:#000;stroke:none"/></svg>"#);
    let (s, msgs) = ok(&svg, &["--setstroke=true", "--strokemodes=5", "--id=n"]);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("no stroked elements"), "{}", msgs[0]);
    assert_eq!(sw(&s, "n"), None);
}

#[test]
fn fuse_transforms_puts_path_data_in_global_coordinates_and_keeps_appearance() {
    let svg = format!(
        r#"<svg {NS}><g id="g" transform="scale(2)"><path id="p" transform="translate(1,1)" d="M0,0 L1,0" style="fill:none;stroke:#000;stroke-width:1"/><text id="t" transform="translate(3,3)" style="font-size:4px;{DV}">t</text></g></svg>"#
    );
    let (s, msgs) = ok(&svg, &["--fusetransforms=true", "--id=g"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let out = Doc::parse(s.as_bytes()).unwrap();
    let p = id(&out, "p");
    assert_ne!(
        out.attr(p, "d"),
        Some("M0,0 L1,0"),
        "the path data was rewritten"
    );
    let pts = sciink::tools::scaler::global_points(&out, p, None);
    assert!(
        close(pts[0].x, 2.0, 1e-9)
            && close(pts[0].y, 2.0, 1e-9)
            && close(pts[1].x, 4.0, 1e-9)
            && close(pts[1].y, 2.0, 1e-9),
        "same global geometry: {pts:?}"
    );
    let c = out.transform(p).as_coeffs();
    assert!(
        close(c[0], 0.5, 1e-9)
            && close(c[3], 0.5, 1e-9)
            && close(c[4], 0.0, 1e-9)
            && close(c[5], 0.0, 1e-9),
        "the inverse of the parent's composed transform: {c:?}"
    );
    let own: Vec<sciink::geom::Point> =
        sciink::geom::path::end_points(&sciink::geom::path::shape_path(&out, p).unwrap().path);
    assert!(
        close(own[0].x, 2.0, 1e-9) && close(own[0].y, 2.0, 1e-9) && close(own[1].x, 4.0, 1e-9),
        "the path data itself is in global coordinates: {own:?}"
    );
    let sw: f64 = out
        .specified(p, "stroke-width")
        .unwrap()
        .trim_end_matches("px")
        .parse()
        .unwrap();
    assert!(
        close(sw, 2.0, 1e-9),
        "stroke scaled with the fused transform: 1 × 2 before, 2 × 1 after — the visual width is unchanged: {sw}"
    );
    assert!(
        close(out.transform(id(&out, "t")).as_coeffs()[4], 3.0, 1e-9),
        "text is not fused"
    );
}

#[test]
fn clearing_clips_and_masks_removes_attributes_and_pins_stylesheet_rules() {
    let svg = format!(
        r##"<svg {NS}><style>#q{{clip-path:url(#c)}}</style><defs><clipPath id="c"><rect width="1" height="1"/></clipPath><mask id="m"><rect width="1" height="1"/></mask></defs>
<path id="p" d="M0,0 H1" clip-path="url(#c)" mask="url(#m)" style="clip-path:url(#c);stroke:#000"/>
<path id="q" d="M0,0 H1" clip-path="url(#c)"/></svg>"##
    );
    let (s, msgs) = ok(&svg, &["--clearclipmasks=true", "--id=p", "--id=q"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let p = by_id(&d, "p");
    assert_eq!(p.attribute("clip-path"), None);
    assert_eq!(p.attribute("mask"), None);
    assert_eq!(
        style_of(p).get("clip-path"),
        None,
        "no stylesheet rule: nothing to pin"
    );
    assert_eq!(
        style_of(p).get("stroke"),
        Some("#000"),
        "the rest of the style survives"
    );
    let q = by_id(&d, "q");
    assert_eq!(q.attribute("clip-path"), None);
    assert_eq!(
        style_of(q).get("clip-path"),
        Some("none"),
        "the stylesheet still supplies one: pinned to none"
    );
    assert!(
        s.contains(r#"<clipPath id="c">"#),
        "clips we did not create stay in defs"
    );
}
