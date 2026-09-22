mod support;

use std::ffi::OsString;

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
