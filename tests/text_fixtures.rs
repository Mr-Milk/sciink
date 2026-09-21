mod support;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

/// Whitespace-collapsed text of every <text> under the element with `layer_id`.
fn layer_texts(svg: &str, layer_id: &str) -> Vec<String> {
    let d = roxmltree::Document::parse(svg).unwrap();
    let Some(layer) = d
        .descendants()
        .find(|n| n.attribute("id") == Some(layer_id))
    else {
        return Vec::new();
    };
    layer
        .descendants()
        .filter(|n| n.has_tag_name("text"))
        .map(|n| {
            let s: String = n
                .descendants()
                .filter(|c| c.is_text())
                .filter_map(|c| c.text())
                .collect();
            s.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Content parity with upstream's Flattener reference for Text_tests.svg: the multiset of text
/// strings in the processed layer. Positions are not compared (that reference was produced by an
/// older writer and with fonts this machine may lack), so only merge/split DECISIONS are checked.
/// Run: `SCIINK_SYSTEM_FONTS=1 cargo test --test text_fixtures -- --ignored --nocapture`.
#[test]
#[ignore]
fn text_tests_content_matches_the_upstream_reference() {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        eprintln!("SKIP: set SCIINK_SYSTEM_FONTS=1 to run against the installed fonts");
        return;
    }
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    // vendored DejaVu Sans on top of the system fonts (the fixture uses it 96 times)
    // SAFETY: single test in this binary; set before any FontSystem::load().
    unsafe {
        std::env::set_var(
            "SCIINK_FONT_DIRS",
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fonts")
                .display()
                .to_string(),
        );
    }
    let svg = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let reference = std::fs::read_to_string(
        dir.join("refs/flatten_plots__--id__layer1__--testmode__True__Text_tests__svg.out"),
    )
    .unwrap();
    let argv: Vec<OsString> = [
        "sciink",
        "--tool=text-fix",
        "--justification=1",
        "--id=layer1",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let out = sciink::run(&argv, &svg).unwrap();
    let ours = layer_texts(std::str::from_utf8(&out.svg).unwrap(), "layer1");
    let theirs = layer_texts(&reference, "layer1"); // "Layer 1 flat" keeps id layer1 in the reference
    assert!(!theirs.is_empty() && !ours.is_empty());
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for t in &theirs {
        *counts.entry(t.as_str()).or_default() += 1;
    }
    let mut matched = 0usize;
    let mut extra: Vec<&str> = Vec::new();
    for t in &ours {
        match counts.get_mut(t.as_str()) {
            Some(c) if *c > 0 => {
                *c -= 1;
                matched += 1;
            }
            _ => extra.push(t),
        }
    }
    let missing: Vec<&str> = counts
        .iter()
        .filter(|(_, c)| **c > 0)
        .map(|(t, _)| *t)
        .collect();
    eprintln!(
        "Text_tests: {matched}/{} reference strings matched; {} ours unmatched; missing: {missing:?}; extra: {extra:?}",
        theirs.len(),
        extra.len()
    );
    assert!(
        matched as f64 >= 0.8 * theirs.len() as f64,
        "fewer than 80 % of the reference strings reproduced ({matched}/{})",
        theirs.len()
    );
}
