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
