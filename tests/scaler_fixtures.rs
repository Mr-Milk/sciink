//! Scaler against upstream's references (`tests/upstream/data/refs/scale_plots__*.out`).
//! Geometry oracle: every path-like descendant of each plot has the same global end points
//! (± 0.02 uu — the reference prints 6 significant digits), the same visual stroke width, and
//! every text has the same global anchor (± TEXT_TOL: its pivot is its font-dependent box).
mod support;

use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::{Point, scale_factor};
use sciink::tools::scaler::global_points;
use support::with_vendored_fonts;

/// Reference coordinates carry 6 significant digits (`{:.6g}`): ± 0.02 uu on ~100 uu values.
const PATH_TOL: f64 = 0.02;
/// Text pivots are text boxes: the reference was produced with other fonts.
const TEXT_TOL: f64 = 0.5;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}

fn scale_fixture(svg_name: &str, ref_name: &str, extra: &[&str]) -> Option<(Doc, Doc)> {
    let dir = support::upstream_data_dir()?;
    let input = std::fs::read(dir.join(format!("svg/{svg_name}.svg"))).unwrap();
    let reference = std::fs::read(dir.join(format!("refs/{ref_name}"))).unwrap();
    let mut a = vec!["--tool=scaler"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), &input)).unwrap();
    // font warnings are expected with the vendored fonts; a plot-area warning is not
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

fn ids_under(doc: &Doc, root: &str) -> Vec<String> {
    let r = doc.by_id(root).unwrap_or_else(|| panic!("no {root}"));
    doc.descendants(r)
        .filter(|&n| doc.is_element(n) && n != r)
        .filter_map(|n| doc.attr(n, "id").map(str::to_string))
        .collect()
}

fn anchor(doc: &Doc, n: NodeId) -> Option<Point> {
    let x: f64 = doc
        .attr(n, "x")?
        .split([' ', ','])
        .next()?
        .trim()
        .parse()
        .ok()?;
    let y: f64 = doc
        .attr(n, "y")?
        .split([' ', ','])
        .next()?
        .trim()
        .parse()
        .ok()?;
    Some(doc.composed_transform(n) * Point::new(x, y))
}

fn visual_stroke(doc: &Doc, n: NodeId) -> Option<f64> {
    let sw = doc.specified(n, "stroke-width")?;
    let w: f64 = sw.trim().trim_end_matches("px").parse().ok()?;
    Some(w * scale_factor(doc.composed_transform(n)))
}

/// Compares our output with the reference under each plot id; returns the largest text-anchor
/// deviation seen (the path assertions are hard).
fn compare(ours: &Doc, reference: &Doc, plots: &[&str]) -> f64 {
    let mut max_text = 0.0_f64;
    let mut paths = 0usize;
    for plot in plots {
        for id in ids_under(reference, plot) {
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
                if let (Some(wa), Some(wb)) = (visual_stroke(ours, a), visual_stroke(reference, b))
                {
                    assert!(
                        (wa - wb).abs() <= 1e-3 * wb.max(1.0),
                        "{id}: visual stroke {wa} vs {wb}"
                    );
                }
                paths += 1;
            } else if tag == "text" {
                if let (Some(p), Some(q)) = (anchor(ours, a), anchor(reference, b)) {
                    max_text = max_text.max((p.x - q.x).abs().max((p.y - q.y).abs()));
                }
            }
        }
    }
    assert!(
        paths > 20,
        "the oracle compared {paths} shapes — the ids did not line up"
    );
    max_text
}

#[test]
fn correction_matches_the_reference_on_other_tests() {
    let Some((ours, reference)) = scale_fixture(
        "Other_tests",
        "scale_plots__--id__g5224__--tab__correction__Other_tests__svg.out",
        &["--tab=correction", "--id=g5224"],
    ) else {
        return;
    };
    let m = compare(&ours, &reference, &["g5224"]);
    eprintln!("correction g5224: max text-anchor deviation {m:.4} uu");
    assert!(m <= TEXT_TOL, "text anchors: {m}");
}

#[test]
fn matching_matches_the_reference_on_other_tests() {
    let Some((ours, reference)) = scale_fixture(
        "Other_tests",
        "scale_plots__--id__rect5248__--id__g4982__--tab__matching__--hmatchopts__2__--vmatchopts__3__Other_tests__svg.out",
        &[
            "--tab=matching",
            "--hmatchopts=2",
            "--vmatchopts=3",
            "--id=rect5248",
            "--id=g4982",
        ],
    ) else {
        return;
    };
    let m = compare(&ours, &reference, &["g4982"]);
    eprintln!("matching g4982: max text-anchor deviation {m:.4} uu");
    assert!(m <= TEXT_TOL, "text anchors: {m}");
    // the target rectangle is untouched
    let (a, b) = (
        ours.by_id("rect5248").unwrap(),
        reference.by_id("rect5248").unwrap(),
    );
    assert_eq!(
        global_points(&ours, a, None).len(),
        global_points(&reference, b, None).len()
    );
}

#[test]
fn correction_matches_the_reference_on_the_non_uniform_document() {
    let Some((ours, reference)) = scale_fixture(
        "Other_tests_nonuniform",
        "scale_plots__--id__g109153__--id__g109019__--tab__correction__Other_tests_nonuniform__svg.out",
        &["--tab=correction", "--id=g109153", "--id=g109019"],
    ) else {
        return;
    };
    let m = compare(&ours, &reference, &["g109153", "g109019"]);
    eprintln!("nonuniform correction: max text-anchor deviation {m:.4} uu");
    assert!(m <= TEXT_TOL, "text anchors: {m}");
}
