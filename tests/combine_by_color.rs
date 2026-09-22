mod support;

use std::collections::BTreeSet;
use std::ffi::OsString;

use sciink::geom::affine_eq;
use sciink::geom::parse_transform;
use sciink::geom::path::{end_points, parse_d};

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn run(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=combine-by-color", "--tab=scaling"];
    a.extend(extra);
    let out = sciink::run(&args(&a), svg.as_bytes()).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}
fn attr<'a>(d: &'a roxmltree::Document, id: &str, name: &str) -> Option<&'a str> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .and_then(|n| n.attribute(name))
}
fn has(d: &roxmltree::Document, id: &str) -> bool {
    d.descendants().any(|n| n.attribute("id") == Some(id))
}

#[test]
fn merges_same_style_paths_into_the_topmost_and_skips_dark_ones() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><path id="p1" d="M0 0 L1 0" style="stroke:#ff0000;stroke-width:1;fill:none"/><path id="dark" d="M0 5 L1 5" style="stroke:#000000;stroke-width:1;fill:none"/><path id="p2" d="M0 1 L1 1" style="stroke:#ff0000;stroke-width:1;fill:none"/><rect id="r" width="1" height="1" style="fill:#ff0000"/><path id="f1" d="M0 2 L1 2 Z" style="fill:#ff0000"/><path id="f2" d="M0 3 L1 3 Z" style="fill:#ff0000"/><text id="t">x</text></g></svg>"#
    );
    let (s, msgs) = run(&svg, &["--id=layer", "--lightnessth=15"]);
    assert!(msgs.is_empty(), "silent on success: {msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        !has(&d, "p1") && has(&d, "p2"),
        "p1 is welded into the topmost of its style, p2"
    );
    assert_eq!(
        attr(&d, "p2", "inkscape-scientific-combined-by-color"),
        Some("0 2 4")
    );
    assert_eq!(
        attr(&d, "p2", "d"),
        Some("M 0,1 L 1,1 M 0,0 L 1,0"),
        "the leader's geometry first, then the earlier ones"
    );
    assert_eq!(
        (attr(&d, "p2", "clip-path"), attr(&d, "p2", "mask")),
        (Some("none"), Some("none"))
    );
    assert!(!has(&d, "f1") && has(&d, "f2"));
    assert_eq!(
        attr(&d, "f2", "inkscape-scientific-combined-by-color"),
        Some("0 3 6")
    );
    assert!(
        has(&d, "dark") && attr(&d, "dark", "inkscape-scientific-combined-by-color").is_none(),
        "black is below the lightness threshold"
    );
    assert!(
        has(&d, "r") && has(&d, "t"),
        "rects (no d) and text are not candidates"
    );
    // threshold 0: black qualifies, but has no partner
    let (s, _) = run(&svg, &["--id=layer", "--lightnessth=0"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "dark") && attr(&d, "dark", "inkscape-scientific-combined-by-color").is_none());
    // a white stroke with a black fill: the fill is dark → skipped (both paints must be light)
    let svg = format!(
        r#"<svg {NS}><path id="a" d="M0 0 L1 0 Z" style="stroke:#fff;fill:#000"/><path id="b" d="M0 1 L1 1 Z" style="stroke:#fff;fill:#000"/></svg>"#
    );
    let (s, _) = run(&svg, &["--id=a", "--id=b"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "a") && has(&d, "b"));
}

#[test]
fn style_differences_and_url_paints_block_merging() {
    let base = |id: &str, extra: &str| {
        format!(
            r#"<path id="{id}" d="M0 0 L1 0" style="stroke:#ff0000;stroke-width:1;fill:none{extra}"/>"#
        )
    };
    let svg = format!(
        "<svg {NS}><g id=\"layer\">{}{}{}{}{}{}{}{}</g></svg>",
        base("ref", ""),
        base("width", ";stroke-width:1.01"),
        base("alpha", ";stroke-opacity:0.5"),
        base("dash", ";stroke-dasharray:1,2"),
        base("marker", ";marker-end:url(#m)"),
        base("grad", ";stroke:url(#g)"),
        base("fillgrad", ";fill:url(#g)"),
        base("same", ";stroke-width:1.0005"),
    );
    let (s, _) = run(&svg, &["--id=layer"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for i in ["width", "alpha", "dash", "marker", "grad", "fillgrad"] {
        assert!(
            has(&d, i) && attr(&d, i, "inkscape-scientific-combined-by-color").is_none(),
            "{i} must stay separate"
        );
    }
    assert!(
        !has(&d, "ref") && has(&d, "same"),
        "a width within 0.001 still matches, and the later element leads"
    );
    assert_eq!(
        attr(&d, "same", "inkscape-scientific-combined-by-color"),
        Some("0 2 4")
    );
}

#[test]
fn empty_selection_is_reported_and_nothing_changes() {
    let svg =
        format!(r#"<svg {NS}><path id="a" d="M0 0 L1 0"/><path id="b" d="M0 1 L1 1"/></svg>"#);
    let (s, msgs) = run(&svg, &[]);
    assert_eq!(s, svg);
    assert_eq!(msgs, vec!["combine-by-color: nothing selected".to_string()]);
    let (s, msgs) = run(&svg, &["--id=nope"]);
    assert_eq!(s, svg);
    assert_eq!(msgs.len(), 1);
}

/// Upstream's own reference output for Other_tests.svg (paths only, no fonts involved). Skipped
/// when the fixtures are absent (CI).
#[test]
fn matches_the_upstream_reference_for_other_tests() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read_to_string(dir.join("svg/Other_tests.svg")).unwrap();
    let reference = std::fs::read_to_string(
        dir.join("refs/combine_by_color__--id__layer1__Other_tests__svg.out"),
    )
    .unwrap();
    let (ours, msgs) = run(&input, &["--id=layer1", "--lightnessth=15"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let ours = roxmltree::Document::parse(&ours).unwrap();
    let theirs = roxmltree::Document::parse(&reference).unwrap();
    let path_ids = |d: &roxmltree::Document| -> BTreeSet<String> {
        d.descendants()
            .filter(|n| n.has_tag_name("path"))
            .filter_map(|n| n.attribute("id").map(str::to_string))
            .collect()
    };
    assert_eq!(path_ids(&ours), path_ids(&theirs), "the same paths survive");
    assert_eq!(
        ours.descendants().filter(|n| n.has_tag_name("g")).count(),
        theirs.descendants().filter(|n| n.has_tag_name("g")).count(),
        "the same groups survive delete_up"
    );
    let mut combined = 0;
    for t in theirs.descendants().filter(|n| {
        n.attribute("inkscape-scientific-combined-by-color")
            .is_some()
    }) {
        let id = t.attribute("id").unwrap();
        combined += 1;
        assert_eq!(
            attr(&ours, id, "inkscape-scientific-combined-by-color"),
            t.attribute("inkscape-scientific-combined-by-color"),
            "{id}: piece indices"
        );
        assert_eq!(attr(&ours, id, "clip-path"), Some("none"), "{id}");
        assert_eq!(attr(&ours, id, "mask"), Some("none"), "{id}");
        let ta = parse_transform(t.attribute("transform").unwrap_or("")).unwrap();
        let oa = parse_transform(attr(&ours, id, "transform").unwrap_or("")).unwrap();
        assert!(affine_eq(ta, oa), "{id}: transform");
        let tp = end_points(&parse_d(t.attribute("d").unwrap()).unwrap().path);
        let op = end_points(&parse_d(attr(&ours, id, "d").unwrap()).unwrap().path);
        assert_eq!(tp.len(), op.len(), "{id}: same number of commands");
        for (a, b) in tp.iter().zip(&op) {
            // the reference prints 6 significant digits
            assert!(
                (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01,
                "{id}: {a:?} vs {b:?}"
            );
        }
    }
    assert_eq!(combined, 15, "the reference welds fifteen groups of paths");
}
