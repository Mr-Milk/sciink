mod support;

use std::ffi::OsString;

use sciink::dom::{Doc, NodeId};
use sciink::geom::{Rect, transform_rect, union};
use support::{pixel_diff_fraction, render_png, with_vendored_fonts};

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn run(tool_args: &[&str], svg: &[u8]) -> Vec<u8> {
    with_vendored_fonts(|| sciink::run(&args(tool_args), svg))
        .unwrap()
        .svg
}
fn diff(before: &[u8], after: &[u8]) -> f64 {
    let (a, b) = (render_png(before, 1500), render_png(after, 1500));
    pixel_diff_fraction(&a, &b, 32)
}

#[test]
fn the_metric_sees_a_moved_rectangle_and_nothing_in_an_identical_render() {
    let a = format!(
        r##"<svg {NS} width="100" height="100" viewBox="0 0 100 100"><rect x="10" y="10" width="30" height="30" fill="#000"/></svg>"##
    );
    let b = a.replace(r#"x="10""#, r#"x="50""#);
    assert_eq!(diff(a.as_bytes(), a.as_bytes()), 0.0);
    let d = diff(a.as_bytes(), b.as_bytes());
    assert!(
        d > 0.1,
        "two 30×30 squares out of 100×100 → about 18 %: {d}"
    );
}

/// Ungroup + clip/mask composition + clone unlinking only: the document must look the same.
#[test]
fn ungroup_only_flattener_is_visually_invariant_on_text_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let out = run(
        &[
            "--tool=flattener",
            "--tab=Options",
            "--id=layer1",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
        &input,
    );
    let d = diff(&input, &out);
    eprintln!("Text_tests ungroup-only pixel diff: {:.4} %", d * 100.0);
    assert!(d <= 0.005, "{d}");
}

#[test]
fn combine_by_color_is_visually_invariant_on_other_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Other_tests.svg")).unwrap();
    let out = run(
        &["--tool=combine-by-color", "--tab=scaling", "--id=layer1"],
        &input,
    );
    let d = diff(&input, &out);
    eprintln!(
        "Other_tests Combine by Color pixel diff: {:.4} %",
        d * 100.0
    );
    assert!(d <= 0.003, "{d}");
}

/// The big one (5.9 MB, 18 000 elements): slow in a debug build, so opt in.
/// Run: `cargo test --release --test invariance -- --ignored --nocapture`
#[test]
#[ignore]
fn ungroup_only_flattener_is_visually_invariant_on_acid_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Acid_tests.svg")).unwrap();
    let out = run(
        &[
            "--tool=flattener",
            "--tab=Options",
            "--id=layer1",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
        &input,
    );
    let d = diff(&input, &out);
    eprintln!("Acid_tests ungroup-only pixel diff: {:.4} %", d * 100.0);
    assert!(d <= 0.005, "{d}");
}

/// Duplicate removal deletes only what an identical opaque element already covers: invisible by construction.
#[test]
fn duplicate_removal_is_visually_invariant_on_text_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let out = run(
        &[
            "--tool=flattener",
            "--tab=Options",
            "--id=layer1",
            "--fixtext=false",
            "--revertpaths=false",
            "--removerectw=false",
        ],
        &input,
    );
    let d = diff(&input, &out);
    eprintln!(
        "Text_tests duplicate-removal pixel diff: {:.4} %",
        d * 100.0
    );
    assert!(d <= 0.005, "{d}");
}

/// Correcting a plot that carries no scale (g4982: a pure translation) is geometrically a no-op:
/// every child's transform is fused into its path, but nothing moves.
#[test]
fn scaler_correction_of_an_unscaled_plot_is_visually_invariant() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Other_tests.svg")).unwrap();
    let out = with_vendored_fonts(|| {
        sciink::run(
            &args(&["--tool=scaler", "--tab=correction", "--id=g4982"]),
            &input,
        )
    })
    .unwrap();
    eprintln!(
        "scaler identity correction: {} messages (font warnings expected)",
        out.messages.len()
    );
    let (a, b) = (render_png(&input, 1500), render_png(&out.svg, 1500));
    let d = pixel_diff_fraction(&a, &b, 32);
    eprintln!(
        "scaler identity correction: {:.4} % pixels differ",
        d * 100.0
    );
    assert!(d <= 0.001, "{d}");
}

/// Root-coordinate box of every clipped element's clip region, sorted by the element's id: the
/// union over the clip's element children of `composed(el) · child.transform · own-frame box`.
fn clip_boxes(svg: &[u8]) -> Vec<(String, Rect)> {
    use sciink::ops::bbox::{LOCAL, bbox};
    use sciink::ops::{ClipKind, Ctx, clip_ref};
    let mut doc = Doc::parse(svg).unwrap();
    let mut ctx = Ctx::new();
    let els: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|&n| doc.is_element(n))
        .collect();
    let mut out = Vec::new();
    for el in els {
        let Some(clip) = clip_ref(&doc, el, ClipKind::Clip) else {
            continue;
        };
        let Some(id) = doc.attr(el, "id").map(str::to_string) else {
            continue;
        };
        let ct = doc.composed_transform(el);
        let kids: Vec<NodeId> = doc.children(clip).filter(|&k| doc.is_element(k)).collect();
        let mut acc: Option<Rect> = None;
        for k in kids {
            if let Some(b) = bbox(&mut doc, &mut ctx, k, LOCAL) {
                acc = union(acc, Some(transform_rect(ct * doc.transform(k), b)));
            }
        }
        if let Some(r) = acc {
            out.push((id, r));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Fusing transforms is a geometric no-op except where a stroke was anisotropic: Other_tests has
/// non-uniformly scaled plots (`g5224` at 0.748 × 0.520, `rect3230`) whose strokes become
/// uniform by design, so pixel identity is impossible there (measured 2026-09-23: 0.2303 %).
/// The precise invariant is that every clip region stays where it was.
#[test]
fn homogenizer_fuse_transforms_keeps_clips_in_place_and_changes_only_anisotropic_strokes() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Other_tests.svg")).unwrap();
    let out = with_vendored_fonts(|| {
        sciink::run(
            &args(&[
                "--tool=homogenizer",
                "--tab=scaling",
                "--fusetransforms=true",
                "--id=layer1",
            ]),
            &input,
        )
    })
    .unwrap();
    assert!(
        out.messages.is_empty(),
        "fusing alone loads no fonts: {:?}",
        out.messages
    );
    let (a, b) = (render_png(&input, 1500), render_png(&out.svg, 1500));
    let d = pixel_diff_fraction(&a, &b, 32);
    eprintln!("homogenizer fuse: {:.4} % pixels differ", d * 100.0);
    assert!(d <= 0.005, "{d}");
    let (before, after) = (clip_boxes(&input), clip_boxes(&out.svg));
    assert_eq!(before.len(), after.len(), "same clipped elements");
    // 11 clipped elements; `image270`'s clip is a `<use>` whose target `bbox` does not measure,
    // so it drops out of both lists alike
    assert!(
        before.len() >= 10,
        "the fixture has clipped elements: {}",
        before.len()
    );
    for ((ida, ra), (idb, rb)) in before.iter().zip(&after) {
        assert_eq!(ida, idb);
        for (x, y) in [
            (ra.x0, rb.x0),
            (ra.y0, rb.y0),
            (ra.x1, rb.x1),
            (ra.y1, rb.y1),
        ] {
            assert!(
                (x - y).abs() <= 1e-3,
                "{ida}: clip region moved: {ra:?} vs {rb:?}"
            );
        }
    }
}
