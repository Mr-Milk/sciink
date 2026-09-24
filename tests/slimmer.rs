//! Slimmer (Plan 10): every default step is rendering-exact; the guard tests name what must stay.
#![allow(dead_code)] // helpers here are for later Plan 10 tasks (T3–T6); unused until then

mod support;

use std::collections::HashSet;
use std::ffi::OsString;

use sciink::dom::Doc;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";
const XLINK: &str = "xmlns:xlink=\"http://www.w3.org/1999/xlink\"";
const INK: &str = "xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}

/// Runs the Slimmer with the default options plus `extra`; returns the document and the messages.
fn slim(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=slimmer", "--tab=Options"];
    a.extend(extra);
    let out = sciink::run(&args(&a), svg.as_bytes()).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}

fn has(d: &roxmltree::Document, id: &str) -> bool {
    d.descendants().any(|n| n.attribute("id") == Some(id))
}

fn by_id<'a>(d: &'a roxmltree::Document<'a>, id: &str) -> roxmltree::Node<'a, 'a> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element with id {id}"))
}

/// Every `url(#x)` and `#x` href in the document has a target.
fn every_reference_resolves(d: &roxmltree::Document) -> bool {
    let ids: HashSet<&str> = d.descendants().filter_map(|n| n.attribute("id")).collect();
    d.descendants().filter(|n| n.is_element()).all(|n| {
        n.attributes().all(|a| {
            let v = a.value();
            let urls = v.split("url(#").skip(1).all(|piece| {
                piece
                    .split(')')
                    .next()
                    .is_some_and(|id| ids.contains(id.trim().trim_matches(['\'', '"'])))
            });
            let href = a.name() != "href" || !v.starts_with('#') || ids.contains(&v[1..]);
            urls && href
        })
    })
}

#[test]
fn duplicate_stylesheets_keep_the_last_copy_and_the_cascade() {
    let svg = format!(
        r#"<svg {NS}><style id="s1">*{{fill:red}}</style><g id="wrap"><style id="s2">*{{fill:blue}}</style></g><style id="s3">*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        !has(&d, "s1") && has(&d, "s2") && has(&d, "s3"),
        "A B A → B A: the first copy goes, the last stays: {s}"
    );
    assert!(
        msgs[0].contains("duplicate stylesheets removed: 1"),
        "{msgs:?}"
    );
    let fill = |svg: &str| {
        let d = Doc::parse(svg.as_bytes()).unwrap();
        d.computed(d.by_id("r").unwrap(), "fill")
    };
    assert_eq!(fill(&svg), "red", "the last rule wins before");
    assert_eq!(fill(&s), "red", "and after");
}

#[test]
fn a_lone_surviving_stylesheet_moves_to_the_root_and_at_rules_or_extra_attributes_block_dedup() {
    let svg = format!(
        r#"<svg {NS}><g id="fig1"><defs><style id="a">*{{fill:red}}</style></defs><rect id="r1" width="1" height="1"/></g><g id="fig2"><defs><style id="b">*{{fill:red}}</style></defs><rect id="r2" width="1" height="1"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "a") && has(&d, "b"), "{s}");
    let first = d
        .root_element()
        .children()
        .find(|c| c.is_element())
        .unwrap();
    assert_eq!(
        first.attribute("id"),
        Some("b"),
        "the lone sheet now leads the root, out of figure 2's <defs>: {s}"
    );
    assert!(
        msgs[0].contains("(1 kept, moved to the document root)"),
        "{msgs:?}"
    );
    let svg = format!(
        r#"<svg {NS}><style id="m">@import url(x.css); *{{fill:red}}</style><style id="n">@import url(x.css); *{{fill:red}}</style><style id="p" media="print">*{{fill:red}}</style><style id="q" media="print">*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["m", "n", "p", "q"] {
        assert!(
            has(&d, id),
            "{id}: an @-rule or an extra attribute blocks dedup: {s}"
        );
    }
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
    assert_eq!(s, svg, "nothing changed → byte-identical");
}

#[test]
fn the_report_lists_bytes_elements_and_each_step_and_says_nothing_to_do_on_a_clean_document() {
    let svg = format!(r#"<svg {NS}><rect id="r" width="1" height="1"/></svg>"#);
    let (s, msgs) = slim(&svg, &[]);
    assert_eq!(s, svg);
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
    let (_, msgs) = slim(&svg, &["--report=false"]);
    assert!(msgs.is_empty(), "silent without the report: {msgs:?}");
    let svg = format!(
        r#"<svg {NS}><style>*{{fill:red}}</style><style>*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let lines: Vec<&str> = msgs[0].lines().collect();
    assert!(
        lines[0].starts_with("Slimmer: ")
            && lines[0].contains(" → ")
            && lines[0].ends_with("3 → 2 elements"),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].contains(&format!("{} B → {} B", svg.len(), s.len())),
        "{}",
        lines[0]
    );
    assert_eq!(lines[1], "  duplicate stylesheets removed: 1");
    assert_eq!(lines[2], "  empty or invisible elements removed: 0");
    assert_eq!(lines[3], "  wrapper groups collapsed: 0");
    assert_eq!(lines[4], "  unused definitions removed: 0");
    assert_eq!(lines[5], "  identical definitions merged: 0");
    assert_eq!(lines[6], "  coordinate precision: unchanged");
    assert_eq!(lines.len(), 7);
    let (_, msgs) = slim(&svg, &["--dedupstyles=false"]);
    assert_eq!(
        msgs,
        vec!["Slimmer: nothing to do".to_string()],
        "the step can be switched off"
    );
}
