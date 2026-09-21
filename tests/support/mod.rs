#![allow(dead_code)]
//! Shared helpers for integration tests.

use std::path::PathBuf;

/// Directory holding upstream's `svg/` and `refs/` test data, if available.
pub fn upstream_data_dir() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("SCIINK_UPSTREAM_TESTS").map(PathBuf::from),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/upstream/data")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|p| p.join("svg").is_dir())
}

/// All upstream fixture SVGs, sorted; empty (with a SKIP note) when unavailable.
pub fn upstream_svgs() -> Vec<PathBuf> {
    let Some(dir) = upstream_data_dir() else {
        eprintln!(
            "SKIP: upstream fixtures not found (set SCIINK_UPSTREAM_TESTS or symlink tests/upstream)"
        );
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("svg"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
    v.sort();
    v
}

/// Asserts two XML documents are structurally identical (element names,
/// attributes as sets, text and comment content, node order).
pub fn assert_same_tree(a: &str, b: &str, context: &str) {
    let da = roxmltree::Document::parse(a)
        .unwrap_or_else(|e| panic!("{context}: input does not parse: {e}"));
    let db = roxmltree::Document::parse(b)
        .unwrap_or_else(|e| panic!("{context}: output does not parse: {e}"));
    let na: Vec<_> = da.descendants().collect();
    let nb: Vec<_> = db.descendants().collect();
    assert_eq!(na.len(), nb.len(), "{context}: node count differs");
    for (x, y) in na.iter().zip(nb.iter()) {
        assert_eq!(
            x.node_type(),
            y.node_type(),
            "{context}: node type differs at {:?}",
            x.range()
        );
        if x.is_element() {
            assert_eq!(
                x.tag_name().name(),
                y.tag_name().name(),
                "{context}: tag differs"
            );
            let mut ax: Vec<(String, String)> = x
                .attributes()
                .map(|a| (a.name().to_string(), a.value().to_string()))
                .collect();
            let mut ay: Vec<(String, String)> = y
                .attributes()
                .map(|a| (a.name().to_string(), a.value().to_string()))
                .collect();
            ax.sort();
            ay.sort();
            assert_eq!(
                ax,
                ay,
                "{context}: attributes differ on <{}>",
                x.tag_name().name()
            );
        }
        if x.is_text() || x.is_comment() {
            assert_eq!(x.text(), y.text(), "{context}: text differs");
        }
    }
}

/// Transformed bottom-left corner of every non-space character of every `<text>` in `svg`, laid
/// out with the vendored fonts and sorted — the appearance-invariance oracle for the text pipeline:
/// a stage that only re-encodes text must leave this list unchanged.
pub fn text_positions(svg: &str) -> Vec<(char, f64, f64)> {
    use sciink::text::layout::{chunk_char_pts, transform_pts};
    use sciink::text::parse::ParsedText;
    use sciink::text::table::CharTable;
    use sciink::text::{Warnings, fonts::FontSystem};
    let mut d = sciink::dom::Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<_> = d
        .descendants(d.svg())
        .filter(|&n| d.is_element(n) && d.tag(n) == "text")
        .collect();
    let mut w = Warnings::default();
    let fonts =
        FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")]);
    let mut ct = CharTable::build(&d, &els, fonts, &mut w);
    let mut out = Vec::new();
    for el in els {
        let Some(pt) = ParsedText::parse(&mut d, el, &mut ct, &mut w) else {
            continue;
        };
        for (li, ln) in pt.lines.iter().enumerate() {
            for ci in 0..ln.chunks.len() {
                let ps = chunk_char_pts(&pt, li, ci);
                for (wi, &c) in ln.chunks[ci].chars.iter().enumerate() {
                    let ch = pt.chars[c].c;
                    if ch == ' ' {
                        continue;
                    }
                    let p = transform_pts(pt.transform, ps[wi])[0];
                    out.push((ch, p.x, p.y));
                }
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

pub fn assert_same_positions(
    before: &[(char, f64, f64)],
    after: &[(char, f64, f64)],
    tol: f64,
    context: &str,
) {
    assert_eq!(
        before.len(),
        after.len(),
        "{context}: character count changed\n{before:?}\n{after:?}"
    );
    for (b, a) in before.iter().zip(after) {
        assert!(
            b.0 == a.0 && (b.1 - a.1).abs() < tol && (b.2 - a.2).abs() < tol,
            "{context}: {b:?} moved to {a:?}"
        );
    }
}
