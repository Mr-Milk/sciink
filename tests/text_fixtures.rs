mod support;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use sciink::dom::Doc;

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
    // The reference is a Flattener run with every text fix on, and the Flattener's
    // `setreplacement` pass (flatten_plots.py:372-388) DELETES `-inkscape-font-specification`
    // from every <text>/<tspan> BEFORE it calls remove_kerning (F:400). That property is the
    // second half of upstream's `isinkscape` test (parser.py:308-316), so in the run that
    // produced the reference `ismlinkscape` was false for every element and Split_Lines /
    // Split_Distant_Intrachunk were never skipped — the reference layer carries the property on
    // 0 of its 982 text/tspan nodes, the untouched "Layer 1 original" duplicate on 177 of 615.
    // Reproduce the same pre-pass, or the comparison is against a document we never fed the
    // pipeline. (`setreplacement`'s other half — appending the replacement family to
    // `font-family` — changes no split/merge decision on this fixture, measured.)
    let svg = {
        let mut doc = Doc::parse(&svg).unwrap();
        let root = doc.root();
        let els: Vec<_> = doc
            .descendants(root)
            .filter(|&n| doc.is_element(n) && matches!(doc.tag(n), "text" | "tspan"))
            .collect();
        for el in els {
            doc.remove_style(el, "-inkscape-font-specification");
        }
        let mut out = Vec::new();
        doc.write(&mut out);
        out
    };
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
    // 396/436 = 90.8 % on this machine; the residue is the sanctioned dx/x-overflow deviation
    // (spec §A.1 "position overflows are truncated, not redistributed") plus one merge that
    // flips on the substitute for the missing `Franklin Gothic Book`. 85 % keeps headroom for
    // machines with a different font set.
    assert!(
        matched as f64 >= 0.85 * theirs.len() as f64,
        "fewer than 85 % of the reference strings reproduced ({matched}/{})",
        theirs.len()
    );
}
