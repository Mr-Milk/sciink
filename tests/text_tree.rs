mod support;

use sciink::dom::{Doc, NodeId};
use sciink::text::tree::{Run, TextTree, run_text, set_run_text};

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn describe(d: &Doc, runs: &[Run]) -> Vec<String> {
    runs.iter()
        .map(|r| {
            let who = d.attr(r.node, "id").map(str::to_string).unwrap_or_else(|| {
                if d.is_comment(r.node) {
                    "<!-->".into()
                } else {
                    d.tag(r.node).to_string()
                }
            });
            format!(
                "{}{}={:?}",
                if r.is_tail { "tail:" } else { "text:" },
                who,
                run_text(d, r)
            )
        })
        .collect()
}

#[test]
fn runs_follow_lxml_text_tail_order() {
    let d = doc(&format!(
        r#"<svg {NS}><text id="t">A<tspan id="s">B<tspan id="u">C</tspan>D</tspan>E<!-- c -->F<tspan id="v"/>G</text><text id="w"><tspan id="x">H</tspan></text></svg>"#
    ));
    let t = TextTree::new(&d, id(&d, "t"));
    assert_eq!(t.dds.len(), 5, "t, s, u, comment, v");
    assert!(t.is_top_level(1) && !t.is_top_level(2) && t.is_top_level(3));
    let runs = t.runs(&d);
    assert_eq!(
        describe(&d, &runs),
        [
            "text:t=Some(\"A\")",
            "text:s=Some(\"B\")",
            "text:u=Some(\"C\")",
            "tail:u=Some(\"D\")",
            "tail:s=Some(\"E\")",
            "tail:<!-->=Some(\"F\")",
            "text:v=None",
            "tail:v=Some(\"G\")",
        ]
    );
    // style sources: text → the node, tail → its parent
    assert_eq!(runs[3].style_node, id(&d, "s"));
    assert_eq!(runs[5].style_node, id(&d, "t"));
    assert_eq!(runs[0].ddi, 0);
    assert_eq!(runs[2].ddi, 2);
    let w = TextTree::new(&d, id(&d, "w"));
    assert_eq!(
        describe(&d, &w.runs(&d)),
        ["text:w=None", "text:x=Some(\"H\")", "tail:x=None"]
    );
}

#[test]
fn set_run_text_creates_replaces_and_removes() {
    let mut d = doc(&format!(
        r#"<svg {NS}><text id="t"><tspan id="s">B</tspan>E</text></svg>"#
    ));
    let t = TextTree::new(&d, id(&d, "t"));
    let runs = t.runs(&d);
    assert_eq!(
        describe(&d, &runs),
        ["text:t=None", "text:s=Some(\"B\")", "tail:s=Some(\"E\")"]
    );
    set_run_text(&mut d, &runs[0], Some("A")); // create leading text
    set_run_text(&mut d, &runs[1], Some("bb")); // replace
    set_run_text(&mut d, &runs[2], None); // remove the tail
    let runs = t.runs(&d);
    assert_eq!(
        describe(&d, &runs),
        ["text:t=Some(\"A\")", "text:s=Some(\"bb\")", "tail:s=None"]
    );
    let mut out = Vec::new();
    d.write(&mut out);
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains(r#"<text id="t">A<tspan id="s">bb</tspan></text>"#)
    );
}

use sciink::text::Warnings;
use sciink::text::whitespace::{depathologize, get_xy};

fn text_of(d: &Doc, i: &str) -> String {
    d.text_content(id(d, i))
}

#[test]
fn get_xy_parses_lists_units_and_none() {
    let d = doc(&format!(
        r#"<svg {NS}><text id="t" x="1 2.5 none 1in" y="" dx=" 3 "/></svg>"#
    ));
    let t = id(&d, "t");
    assert_eq!(get_xy(&d, t, "x"), [Some(1.0), Some(2.5), None, Some(96.0)]);
    assert_eq!(get_xy(&d, t, "y"), [None]);
    assert_eq!(get_xy(&d, t, "dy"), [None]);
    assert_eq!(get_xy(&d, t, "dx"), [Some(3.0)]);
}

#[test]
fn whitespace_is_collapsed_unless_preserved() {
    let mut d = doc(&format!(
        "<svg {NS}><text id=\"t\">  Hello \n  <tspan id=\"s\">big\tworld  </tspan>\n   again </text><text id=\"p\" xml:space=\"preserve\">  a  \n b</text></svg>"
    ));
    let mut w = Warnings::default();
    let t = id(&d, "t");
    depathologize(&mut d, t, false, &mut w);
    let p = id(&d, "p");
    depathologize(&mut d, p, false, &mut w);
    // text with element children keeps one trailing space; tail keeps one leading space
    assert_eq!(text_of(&d, "t"), "Hello big world again");
    assert_eq!(text_of(&d, "s"), "big world");
    // preserved: whitespace kept, but the first newline of a run becomes a space and the rest vanish
    assert_eq!(text_of(&d, "p"), "  a    b");
    assert!(w.0.is_empty());
}

#[test]
fn preserved_newlines_and_last_span_rule() {
    // "a\n\nb" in the parent (has children): first newline → space, second dropped → "a b". A
    // trailing newline in a leaf span's text ("c\n") and in a last-child tail ("d\n") is DROPPED,
    // not converted (upstream cleanup_returns, last_span rule).
    let mut d = doc(&format!(
        "<svg {NS}><text id=\"t\" xml:space=\"preserve\">a\n\nb<tspan id=\"s\">c\n</tspan>d\n</text></svg>"
    ));
    let mut w = Warnings::default();
    let t = id(&d, "t");
    depathologize(&mut d, t, false, &mut w);
    assert_eq!(text_of(&d, "t"), "a bcd");
    assert_eq!(text_of(&d, "s"), "c");
}

#[test]
fn comment_tails_are_condensed_and_overflows_truncated() {
    let mut d = doc(&format!(
        r#"<svg {NS}><text id="t" x="1 2 3 4 5" dx="1 2 3">ab<!-- note -->cd<tspan id="s" x="7 8"/></text></svg>"#
    ));
    let mut w = Warnings::default();
    let t = id(&d, "t");
    depathologize(&mut d, t, false, &mut w);
    assert_eq!(
        d.attr(t, "x"),
        Some("1 2"),
        "5 values for 2 chars → truncated"
    );
    assert_eq!(d.attr(t, "dx"), Some("1 2"));
    assert_eq!(
        d.attr(id(&d, "s"), "x"),
        None,
        "positions on an empty tspan are dropped"
    );
    // `t`'s own text "ab" is followed by the comment's tail "cd" inside `t`'s subtree, so
    // truncating its x/dx really does drop positions upstream would have redistributed → 2
    // warnings. `s` is an empty leaf with nothing after it, so its surplus x is dropped
    // silently.
    assert_eq!(w.0.len(), 2, "{:?}", w.0);
    assert!(w.0[0].contains("t: x has more values than characters"));
    assert!(w.0[1].contains("t: dx has more values than characters"));
    // the comment's tail "cd" moved onto the parent's text
    assert_eq!(d.text_content(t), "abcd");
    let runs = TextTree::new(&d, t).runs(&d);
    assert_eq!(
        describe(&d, &runs),
        [
            "text:t=Some(\"abcd\")",
            "tail:<!-->=None",
            "text:s=None",
            "tail:s=None"
        ]
    );
}

#[test]
fn a_leaf_with_one_surplus_position_is_truncated_silently() {
    // The Acid_tests PDF-import shape: a role=line tspan with one more x value than it has
    // characters and no following text anywhere inside it. Upstream's redistribution has
    // nowhere to put the surplus, so truncation loses nothing and must not warn.
    let mut d = doc(&format!(
        r#"<svg {NS}><text id="t"><tspan id="s" x="1 2 3 4" y="9">abc</tspan></text></svg>"#
    ));
    let mut w = Warnings::default();
    let t = id(&d, "t");
    depathologize(&mut d, t, false, &mut w);
    assert_eq!(d.attr(id(&d, "s"), "x"), Some("1 2 3"));
    assert!(w.0.is_empty(), "{:?}", w.0);
}
