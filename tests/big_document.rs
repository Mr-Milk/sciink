//! The default pipelines on a many-figure document without upstream data: they finish, touch only
//! the selection, measure only the selection's text, and carry a multi-megabyte raster untouched.

mod support;

use std::ffi::OsString;

use support::{BigDoc, with_vendored_fonts};

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}

fn run(tool: &str, svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec![format!("--tool={tool}")];
    a.extend(extra.iter().map(|s| s.to_string()));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes())).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}

/// The exact source text of the element with `id`, taken from the document's own bytes.
fn slice_of<'a>(doc: &'a roxmltree::Document<'a>, id: &str) -> &'a str {
    let n = doc
        .descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"));
    &doc.input_text()[n.range()]
}

#[test]
fn flattener_defaults_survive_a_big_document() {
    let big = BigDoc::default();
    let svg = big.svg();
    let paths_before = roxmltree::Document::parse(&svg)
        .unwrap()
        .descendants()
        .filter(|n| n.has_tag_name("path"))
        .count();
    let (out, msgs) = run("flattener", &svg, &["--id=layer1"]);
    let d = roxmltree::Document::parse(&out).expect("output parses");
    // the total element count is not asserted: clip merging legitimately adds one clipPath copy
    // per transformed child
    assert!(
        !d.descendants()
            .any(|n| n.has_tag_name("g") && n.attribute("id").is_some_and(|i| i.starts_with("fig"))),
        "figure groups dissolved"
    );
    assert!(
        !d.descendants()
            .any(|n| n.attribute("id").is_some_and(|i| i.starts_with("bg"))),
        "white backgrounds removed"
    );
    // every 5th shape duplicated the previous one → shapes/5 duplicates per figure are removed;
    // every <use> clone is unlinked into a path of its own
    let dups = big.shapes / 5;
    let paths_after = d.descendants().filter(|n| n.has_tag_name("path")).count();
    assert_eq!(
        paths_after,
        paths_before - dups * big.figures + big.clones * big.figures,
        "duplicates removed, clones unlinked"
    );
    assert!(
        !d.descendants().any(|n| n.has_tag_name("use")),
        "clones unlinked"
    );
    assert!(
        !msgs
            .iter()
            .any(|m| m.contains("nest") || m.contains("internal error")),
        "{msgs:?}"
    );
}

#[test]
fn flattener_on_one_figure_leaves_the_others_byte_identical() {
    let svg = BigDoc::default().svg();
    let (out, _) = run("flattener", &svg, &["--id=fig7"]);
    let (a, b) = (
        roxmltree::Document::parse(&svg).unwrap(),
        roxmltree::Document::parse(&out).unwrap(),
    );
    for id in ["fig0", "fig6", "fig8", "fig99"] {
        assert_eq!(slice_of(&a, id), slice_of(&b, id), "{id} changed");
    }
    assert!(
        b.descendants().all(|n| n.attribute("id") != Some("fig7")),
        "fig7's group was dissolved by the flattener"
    );
}

#[test]
fn homogenizer_font_size_on_one_figure_leaves_the_others_byte_identical() {
    let svg = BigDoc::default().svg();
    let (out, _) = run(
        "homogenizer",
        &svg,
        &[
            "--id=fig7",
            "--setfontsize=true",
            "--fontsize=6",
            "--fontmodes=2",
        ],
    );
    let (a, b) = (
        roxmltree::Document::parse(&svg).unwrap(),
        roxmltree::Document::parse(&out).unwrap(),
    );
    for id in ["fig0", "fig6", "fig8", "fig99"] {
        assert_eq!(slice_of(&a, id), slice_of(&b, id), "{id} changed");
    }
    assert_ne!(slice_of(&a, "fig7"), slice_of(&b, "fig7"));
}

#[test]
fn a_multi_megabyte_base64_image_round_trips_byte_for_byte() {
    let big = BigDoc {
        figures: 3,
        image_kb: 4096,
        ..BigDoc::default()
    };
    let svg = big.svg();
    let payload_in = roxmltree::Document::parse(&svg)
        .unwrap()
        .descendants()
        .find(|n| n.attribute("id") == Some("img"))
        .unwrap()
        .attribute("href")
        .unwrap()
        .to_string();
    let (out, _) = run(
        "flattener",
        &svg,
        &[
            "--id=fig0",
            "--fixtext=false",
            "--removeduppaths=false",
            "--removerectw=false",
            "--deepungroup=false",
        ],
    );
    let d = roxmltree::Document::parse(&out).unwrap();
    let payload_out = d
        .descendants()
        .find(|n| n.attribute("id") == Some("img"))
        .unwrap()
        .attribute("href")
        .unwrap();
    assert_eq!(payload_out.len(), payload_in.len());
    assert!(payload_out == payload_in, "the image payload changed");
}
