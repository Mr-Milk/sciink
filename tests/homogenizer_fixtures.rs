//! Homogenizer against upstream's references. Geometry and numbers are compared, not attribute
//! text: upstream writes `stroke-width` on every element and splits Avenir non-letters into
//! tspans (`character_fixer`, not ported); we write the same geometry with fewer edits.
mod support;

use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::{Point, scale_factor};
use sciink::tools::scaler::global_points;
use support::with_vendored_fonts;

const PATH_TOL: f64 = 0.02;
const TEXT_TOL: f64 = 0.5;
const ARGS: &[&str] = &[
    "--tool=homogenizer",
    "--tab=scaling",
    "--id=layer1",
    "--fontsize=7",
    "--setfontsize=true",
    "--fixtextdistortion=true",
    "--fontmodes=2",
    "--setfontfamily=true",
    "--setstroke=true",
    "--setstrokew=0.75",
    "--strokemodes=2",
    "--fusetransforms=true",
];

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}

fn fixture(svg_name: &str, ref_name: &str, family: &str, vendored: bool) -> Option<(Doc, Doc)> {
    let dir = support::upstream_data_dir()?;
    let input = std::fs::read(dir.join(format!("svg/{svg_name}.svg"))).unwrap();
    let reference = std::fs::read(dir.join(format!("refs/{ref_name}"))).unwrap();
    let fam = format!("--fontfamily={family}");
    let mut a = ARGS.to_vec();
    a.push(&fam);
    let out = if vendored {
        with_vendored_fonts(|| sciink::run(&args(&a), &input))
    } else {
        sciink::run(&args(&a), &input)
    }
    .unwrap();
    // font warnings are expected with the vendored fonts; a plot-area warning would not be
    assert!(
        !out.messages
            .iter()
            .any(|m| m.contains("could not be automatically detected")),
        "{:?}",
        out.messages
    );
    Some((
        Doc::parse(&out.svg).unwrap(),
        Doc::parse(&reference).unwrap(),
    ))
}

/// Only the tags `compare` inspects: upstream's unported `character_fixer` (see the module doc)
/// wraps non-letter characters in synthetic tspans (`tspan0`, `tspan1`, …) that never existed in
/// the input and that we never write, so a tag-blind id scan would panic on their absence even
/// though nothing checks a tspan's geometry.
fn ids_under(doc: &Doc, root: &str) -> Vec<String> {
    let r = doc.by_id(root).unwrap_or_else(|| panic!("no {root}"));
    doc.descendants(r)
        .filter(|&n| doc.is_element(n) && n != r)
        .filter(|&n| {
            matches!(
                doc.tag(n),
                "path" | "rect" | "line" | "polyline" | "polygon" | "circle" | "ellipse" | "text"
            )
        })
        .filter_map(|n| doc.attr(n, "id").map(str::to_string))
        .collect()
}
fn num_attr(v: &str) -> Option<f64> {
    v.trim()
        .trim_end_matches("px")
        .trim_end_matches('%')
        .parse()
        .ok()
}
fn visual_stroke(doc: &Doc, n: NodeId) -> Option<f64> {
    let s = doc.specified(n, "stroke")?;
    if s.trim() == "none" {
        return None;
    }
    let w = num_attr(&doc.specified(n, "stroke-width")?)?;
    Some(w * scale_factor(doc.composed_transform(n)))
}
fn anchor(doc: &Doc, n: NodeId) -> Option<Point> {
    let x = num_attr(doc.attr(n, "x")?.split([' ', ',']).next()?)?;
    let y = num_attr(doc.attr(n, "y")?.split([' ', ',']).next()?)?;
    Some(doc.composed_transform(n) * Point::new(x, y))
}

/// Font-independent comparisons; returns (shapes compared, texts compared, max anchor deviation).
fn compare(ours: &Doc, reference: &Doc) -> (usize, usize, f64) {
    let (mut shapes, mut texts, mut max_anchor) = (0usize, 0usize, 0.0_f64);
    let (mut strokes, mut sizes) = (0usize, 0usize);
    for id in ids_under(reference, "layer1") {
        let (Some(a), Some(b)) = (ours.by_id(&id), reference.by_id(&id)) else {
            panic!("{id}: present in the reference but not in our output")
        };
        let tag = reference.tag(b);
        if matches!(
            tag,
            "path" | "rect" | "line" | "polyline" | "polygon" | "circle" | "ellipse"
        ) {
            let (pa, pb) = (
                global_points(ours, a, None),
                global_points(reference, b, None),
            );
            assert_eq!(pa.len(), pb.len(), "{id}: point count");
            for (p, q) in pa.iter().zip(&pb) {
                assert!(
                    (p.x - q.x).abs() <= PATH_TOL && (p.y - q.y).abs() <= PATH_TOL,
                    "{id}: {p:?} vs {q:?}"
                );
            }
            if let (Some(wa), Some(wb)) = (visual_stroke(ours, a), visual_stroke(reference, b)) {
                assert!(
                    (wa - wb).abs() <= 1e-3 * wb.max(1.0),
                    "{id}: visual stroke {wa} vs {wb}"
                );
                strokes += 1;
            }
            shapes += 1;
        } else if tag == "text" {
            // the size written depends only on transforms and the document scale
            if let (Some(fa), Some(fb)) = (
                ours.specified(a, "font-size"),
                reference.specified(b, "font-size"),
            ) {
                if let (Some(x), Some(y)) = (num_attr(&fa), num_attr(&fb)) {
                    assert!((x - y).abs() <= 0.011, "{id}: font-size {fa} vs {fb}");
                    sizes += 1;
                }
            }
            // the distortion fix depends only on transforms
            let ca = ours.composed_transform(a).as_coeffs();
            let cb = reference.composed_transform(b).as_coeffs();
            for k in 0..4 {
                assert!(
                    (ca[k] - cb[k]).abs() <= 1e-3,
                    "{id}: transform {ca:?} vs {cb:?}"
                );
            }
            assert_eq!(
                ours.specified(a, "-inkscape-font-specification"),
                None,
                "{id}"
            );
            if let (Some(p), Some(q)) = (anchor(ours, a), anchor(reference, b)) {
                max_anchor = max_anchor.max((p.x - q.x).abs().max((p.y - q.y).abs()));
            }
            texts += 1;
        }
    }
    assert!(
        shapes > 20 && texts > 5 && strokes > 20 && sizes > 5,
        "compared {shapes} shapes, {texts} texts, {strokes} strokes, {sizes} font-sizes — \
         a value dropped on our side must not silently skip the comparison"
    );
    (shapes, texts, max_anchor)
}

#[test]
fn homogenizer_matches_the_reference_geometry_on_other_tests() {
    let Some((ours, reference)) = fixture(
        "Other_tests",
        "homogenizer__5a83c74b7209db59f18b4ea3633bdcc5.out",
        "DejaVu Sans",
        true,
    ) else {
        return;
    };
    let (s, t, m) = compare(&ours, &reference);
    eprintln!(
        "homogenizer Other_tests: {s} shapes, {t} texts, max anchor deviation {m:.4} uu (vendored fonts)"
    );
    // regression guard under the vendored fonts (measured 2026-09-23: 0.5927 uu on Other_tests,
    // 0.0153 uu on the non-uniform document); the reference was produced with other fonts, so
    // this cannot be TEXT_TOL — it only catches a text that stops being re-centred
    assert!(m <= 1.0, "text anchors moved: {m}");
}

#[test]
fn homogenizer_matches_the_reference_geometry_on_the_non_uniform_document() {
    let Some((ours, reference)) = fixture(
        "Other_tests_nonuniform",
        "homogenizer__b1a8ca25564328c974db2cf0d2072b84.out",
        "DejaVu Sans",
        true,
    ) else {
        return;
    };
    let (s, t, m) = compare(&ours, &reference);
    eprintln!(
        "homogenizer Other_tests_nonuniform: {s} shapes, {t} texts, max anchor deviation {m:.4} uu (vendored fonts)"
    );
    // regression guard under the vendored fonts (measured 2026-09-23: 0.5927 uu on Other_tests,
    // 0.0153 uu on the non-uniform document); the reference was produced with other fonts, so
    // this cannot be TEXT_TOL — it only catches a text that stops being re-centred
    assert!(m <= 1.0, "text anchors moved: {m}");
}

/// With the installed fonts (Avenir): family and anchors too.
/// Run: `SCIINK_SYSTEM_FONTS=1 cargo test --test homogenizer_fixtures -- --ignored --nocapture --test-threads=1`
/// Run with `--ignored`, never `--include-ignored`: the sibling tests pin the vendored fonts for
/// the whole binary.
#[test]
#[ignore = "needs the installed Avenir; run with SCIINK_SYSTEM_FONTS=1"]
fn homogenizer_matches_the_reference_with_avenir() {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        return;
    }
    for (svg, r) in [
        (
            "Other_tests",
            "homogenizer__5a83c74b7209db59f18b4ea3633bdcc5.out",
        ),
        (
            "Other_tests_nonuniform",
            "homogenizer__b1a8ca25564328c974db2cf0d2072b84.out",
        ),
    ] {
        let Some((ours, reference)) = fixture(svg, r, "Avenir", false) else {
            return;
        };
        let (_, _, m) = compare(&ours, &reference);
        eprintln!("homogenizer {svg} with Avenir: max anchor deviation {m:.4} uu");
        assert!(m <= TEXT_TOL, "{svg}: text anchors {m}");
        for id in ids_under(&reference, "layer1") {
            let (Some(a), Some(b)) = (ours.by_id(&id), reference.by_id(&id)) else {
                continue;
            };
            if reference.tag(b) == "text"
                && reference
                    .specified(b, "font-family")
                    .is_some_and(|f| f.contains("Avenir"))
            {
                assert!(
                    ours.specified(a, "font-family")
                        .is_some_and(|f| f.contains("Avenir")),
                    "{id}"
                );
            }
        }
    }
}
