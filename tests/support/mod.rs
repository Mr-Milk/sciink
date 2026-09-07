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
