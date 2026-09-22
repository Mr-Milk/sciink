mod support;

use std::ffi::OsString;

use sciink::geom::{affine_eq, parse_transform};
use support::with_vendored_fonts;

const NS: &str =
    "xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn run(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=text-ghoster", "--tab=scaling"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes())).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}
fn num(n: roxmltree::Node, a: &str) -> f64 {
    n.attribute(a).unwrap().parse().unwrap()
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}
/// `(x, y, width, height, rx)` of the ghost rectangle in the group wrapping `id`, the group's
/// transform, the rectangle's style and the blur's stdDeviation.
fn ghost_of(d: &roxmltree::Document, id: &str) -> (Vec<f64>, String, String, f64) {
    let el = by_id(d, id);
    let g = el.parent().unwrap();
    assert_eq!(g.tag_name().name(), "g", "{id} is wrapped in a group");
    let r = g
        .children()
        .find(|n| {
            n.has_tag_name("rect")
                && n.attribute("style")
                    .is_some_and(|s| s.contains("filter:url(#"))
        })
        .expect("ghost rectangle");
    let vals: Vec<f64> = ["x", "y", "width", "height", "rx"]
        .iter()
        .map(|a| num(r, a))
        .collect();
    let style = r.attribute("style").unwrap().to_string();
    let fid = style
        .split("filter:url(#")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    let blur = by_id(d, fid).children().find(|n| n.is_element()).unwrap();
    assert_eq!(blur.tag_name().name(), "feGaussianBlur");
    (
        vals,
        g.attribute("transform").unwrap_or("").to_string(),
        style,
        num(blur, "stdDeviation"),
    )
}

#[test]
fn wraps_the_text_moves_its_transform_and_sizes_the_rectangle_from_the_extent() {
    let svg = format!(
        r#"<svg {NS}><defs><linearGradient id="lg"/></defs><g id="layer" transform="translate(100,0)"><rect id="other" width="1" height="1"/><text id="t" style="font-family:'DejaVu Sans';font-size:10px" x="0" y="0" transform="translate(5,5)">Hi</text><rect id="after" width="1" height="1"/></g></svg>"#
    );
    // the expected extent, from the engine itself, in the text's own frame
    let (x1, cap) = {
        let mut d = sciink::dom::Doc::parse(svg.as_bytes()).unwrap();
        let t = d.by_id("t").unwrap();
        let mut ctx = sciink::ops::Ctx::new();
        let bb = with_vendored_fonts(|| {
            sciink::ops::bbox::bbox(&mut d, &mut ctx, t, sciink::ops::bbox::LOCAL)
        })
        .unwrap();
        assert!(
            (bb.y0 + 7.29).abs() < 0.05 && bb.y1.abs() < 1e-9 && bb.x0.abs() < 1e-9,
            "{bb:?}"
        );
        (bb.x1, -bb.y0)
    };
    let (s, msgs) = run(&svg, &["--id=t"]);
    assert!(msgs.is_empty(), "silent on success: {msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let layer = by_id(&d, "layer");
    let kids: Vec<_> = layer.children().filter(|n| n.is_element()).collect();
    assert_eq!(kids.len(), 3);
    assert_eq!(
        (kids[0].attribute("id"), kids[1].attribute("id")),
        (Some("other"), Some("after"))
    );
    let g = kids[2];
    assert_eq!(
        g.tag_name().name(),
        "g",
        "the wrapper sits last in the layer"
    );
    let gk: Vec<_> = g.children().filter(|n| n.is_element()).collect();
    assert_eq!(gk.len(), 2);
    assert_eq!(
        (gk[0].tag_name().name(), gk[1].attribute("id")),
        ("rect", Some("t"))
    );
    assert_eq!(
        gk[1].attribute("transform"),
        None,
        "the text's transform moved onto the group"
    );
    let (v, gt, style, std) = ghost_of(&d, "t");
    assert_eq!(gt, "translate(5,5)");
    // border = EXTENT · 10px = 5
    assert!(
        (v[0] + 5.0).abs() < 1e-6 && (v[1] + cap + 5.0).abs() < 1e-6,
        "{v:?}"
    );
    assert!(
        (v[2] - (x1 + 10.0)).abs() < 1e-6 && (v[3] - (cap + 10.0)).abs() < 1e-6,
        "{v:?}"
    );
    assert!((v[4] - 5.0).abs() < 1e-9);
    let fid = style
        .split("filter:url(#")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap()
        .to_string();
    assert_eq!(
        style,
        format!("fill:#ffffff;stroke:none;filter:url(#{fid});opacity:0.75")
    );
    assert!((std - 2.5).abs() < 1e-9, "STDDEV · border");
    let defs = d.descendants().find(|n| n.has_tag_name("defs")).unwrap();
    let first = defs.children().find(|n| n.is_element()).unwrap();
    assert_eq!(
        (first.tag_name().name(), first.attribute("id")),
        ("filter", Some(fid.as_str())),
        "the filter goes first in <defs>"
    );
    assert!(by_id(&d, "lg").is_element(), "existing defs content stays");
}

#[test]
fn font_size_is_the_largest_in_the_groups_frame_or_falls_back_to_8pt() {
    let svg = format!(
        r#"<svg {NS}><g id="scaled" transform="scale(2)"><g id="grp"><text id="a" style="font-family:'DejaVu Sans';font-size:6px">a</text><text id="b" style="font-family:'DejaVu Sans';font-size:12px" transform="scale(2)">b</text></g><rect id="r" width="4" height="2"/></g></svg>"#
    );
    let (s, msgs) = run(&svg, &["--id=grp", "--id=r"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    // grp: 6 and 12·2 in the group's own frame (the outer scale(2) does not count) → 24 → border 12
    let (v, _, _, std) = ghost_of(&d, "grp");
    assert!(
        (v[4] - 12.0).abs() < 1e-9 && (std - 6.0).abs() < 1e-9,
        "{v:?}"
    );
    // a plain rect has no font size anywhere: 8pt = 10.6667 → border 5.3333333
    let (v, _, _, _) = ghost_of(&d, "r");
    assert!(
        (v[4] - 16.0 / 3.0).abs() < 1e-6 && (v[0] + 16.0 / 3.0).abs() < 1e-6,
        "{v:?}"
    );
    assert!(
        (v[2] - (4.0 + 32.0 / 3.0)).abs() < 1e-6 && (v[3] - (2.0 + 32.0 / 3.0)).abs() < 1e-6,
        "{v:?}"
    );
    assert_eq!(
        d.descendants()
            .filter(|n| n.has_tag_name("feGaussianBlur"))
            .count(),
        2
    );
    let scaled = by_id(&d, "scaled");
    let sk: Vec<_> = scaled.children().filter(|n| n.is_element()).collect();
    assert_eq!(
        sk.len(),
        2,
        "both wrappers, in selection (document) order: {s}"
    );
    assert!(sk.iter().all(|n| n.has_tag_name("g")));
}

#[test]
fn singular_or_boxless_elements_are_wrapped_without_a_rectangle() {
    let svg = format!(
        r#"<svg {NS}><text id="t" style="font-family:'DejaVu Sans';font-size:10px" transform="scale(0)">x</text><g id="empty"/></svg>"#
    );
    let (s, msgs) = run(&svg, &["--id=t", "--id=empty"]);
    assert!(!s.contains("<rect") && !s.contains("<filter"), "{s}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let t = by_id(&d, "t");
    assert_eq!(t.parent().unwrap().tag_name().name(), "g");
    assert_eq!(
        t.parent().unwrap().attribute("transform"),
        Some("scale(0,0)")
    );
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(msgs.iter().all(|m| m.starts_with("warning: ")), "{msgs:?}");
    let (s, msgs) = run(&svg, &[]);
    assert_eq!(s, svg);
    assert_eq!(msgs, vec!["text-ghoster: nothing selected".to_string()]);
}

#[test]
fn font_warnings_concern_only_the_selected_text() {
    let svg = format!(
        r#"<svg {NS}><text id="t" style="font-family:'DejaVu Sans';font-size:10px">a</text><text id="u" style="font-family:'No Such Font';font-size:10px">b</text></svg>"#
    );
    let (_, msgs) = run(&svg, &["--id=t"]);
    assert!(
        msgs.is_empty(),
        "the unselected text's missing font is not this run's business: {msgs:?}"
    );
    let (_, msgs) = run(&svg, &["--id=u"]);
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("warning: ") && m.contains("No Such Font")),
        "the selected text's missing font is: {msgs:?}"
    );
}

/// Upstream's reference output for `text28136` (Tahoma). Run with the installed fonts only:
/// `SCIINK_SYSTEM_FONTS=1 cargo test --test text_ghoster -- --ignored --nocapture`
/// (not `--include-ignored`: the other tests pin the vendored fonts for the whole binary).
#[test]
#[ignore]
fn matches_the_upstream_reference_for_text28136() {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        eprintln!(
            "SKIP: set SCIINK_SYSTEM_FONTS=1 to compare against the upstream reference (Tahoma)"
        );
        return;
    }
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read_to_string(dir.join("svg/Other_tests.svg")).unwrap();
    let reference = std::fs::read_to_string(
        dir.join("refs/text_ghoster__--id__text28136__Other_tests__svg.out"),
    )
    .unwrap();
    let out = sciink::run(
        &args(&["--tool=text-ghoster", "--tab=scaling", "--id=text28136"]),
        input.as_bytes(),
    )
    .unwrap();
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    let ours = String::from_utf8(out.svg).unwrap();
    let ours = roxmltree::Document::parse(&ours).unwrap();
    let theirs = roxmltree::Document::parse(&reference).unwrap();
    let (v, gt, style, std) = ghost_of(&ours, "text28136");
    let (v2, gt2, style2, std2) = ghost_of(&theirs, "text28136");
    assert!(
        affine_eq(
            parse_transform(&gt).unwrap(),
            parse_transform(&gt2).unwrap()
        ),
        "{gt} vs {gt2}"
    );
    for (a, b) in v.iter().zip(&v2) {
        assert!((a - b).abs() < 0.05, "ours {v:?} vs upstream {v2:?}");
    }
    assert!((std - std2).abs() < 1e-3, "{std} vs {std2}");
    assert!(style.ends_with("opacity:0.75") && style2.ends_with("opacity:0.75"));
}
