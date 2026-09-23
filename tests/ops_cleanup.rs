mod support;

use sciink::dom::{Doc, NodeId};
use sciink::ops::Ctx;

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}

fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i)
        .unwrap_or_else(|| panic!("no element with id {i}"))
}

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn out(d: &Doc) -> String {
    let mut v = Vec::new();
    d.write(&mut v);
    String::from_utf8(v).unwrap()
}

use std::collections::HashSet;

use sciink::ops::cleanup::{
    delete_up, drop_dangling_refs, gc_created_clips, strip_attr, strip_whitespace, url_id,
};

#[test]
fn url_id_parses_url_references() {
    assert_eq!(url_id("url(#abc)"), Some("abc"));
    assert_eq!(url_id(" url(# abc ) "), Some("abc"));
    assert_eq!(url_id("none"), None);
    assert_eq!(url_id("#abc"), None);
}

#[test]
fn delete_up_removes_emptied_ancestors_below_the_root_and_records_ids() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="layer"><g id="inner"><path id="p"/></g><rect id="keep"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_p = id(&d, "p");
    delete_up(&mut d, &mut ctx, n_p);
    assert_eq!(d.by_id("p"), None);
    assert_eq!(
        d.by_id("inner"),
        None,
        "left without element children → deleted too"
    );
    assert!(d.by_id("layer").is_some(), "still holds the rect");
    assert_eq!(
        ctx.deleted,
        HashSet::from(["p".to_string(), "inner".to_string()])
    );
    let n_keep = id(&d, "keep");
    delete_up(&mut d, &mut ctx, n_keep);
    assert_eq!(d.by_id("layer"), None);
    assert!(ctx.deleted.contains("layer") && ctx.deleted.contains("keep"));
    assert_eq!(
        out(&d),
        format!(r#"<svg {NS}></svg>"#),
        "the root is never deleted"
    );
}

#[test]
fn delete_up_counts_comments_as_children_and_records_subtree_ids() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="g"><!-- note --><g id="s"><path id="a"/><path id="b"/></g></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_s = id(&d, "s");
    delete_up(&mut d, &mut ctx, n_s);
    assert!(
        d.by_id("g").is_some(),
        "a comment keeps the group alive (lxml len counts it)"
    );
    assert_eq!(
        ctx.deleted,
        HashSet::from(["s".to_string(), "a".to_string(), "b".to_string()])
    );
}

#[test]
fn drop_dangling_refs_touches_only_deleted_ids() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="a" clip-path="url(#gone)"/><rect id="b" mask="url(#gone)" clip-path="url(#other)"/><rect id="c" style="clip-path:url(#gone);fill:red"/></svg>"#
    ));
    let deleted: HashSet<String> = HashSet::from(["gone".to_string()]);
    drop_dangling_refs(&mut d, &deleted);
    assert_eq!(d.attr(id(&d, "a"), "clip-path"), None);
    assert_eq!(d.attr(id(&d, "b"), "mask"), None);
    assert_eq!(
        d.attr(id(&d, "b"), "clip-path"),
        Some("url(#other)"),
        "pre-existing dangling refs are not ours to fix"
    );
    assert_eq!(
        d.attr(id(&d, "c"), "style"),
        Some("fill:red"),
        "inline copies are dropped too"
    );
    drop_dangling_refs(&mut d, &HashSet::new());
    assert_eq!(d.attr(id(&d, "b"), "clip-path"), Some("url(#other)"));
}

#[test]
fn gc_created_clips_removes_unreferenced_clips_and_chains() {
    let mut d = doc(&format!(
        r#"<svg {NS}><style>#r{{clip-path:url(#c4)}}</style><defs><clipPath id="c1"><path d="M0 0h1v1z"/></clipPath><clipPath id="c2"><path clip-path="url(#c1)" d="M0 0h1v1z"/></clipPath><clipPath id="c3"><path d="M0 0h1v1z"/></clipPath><clipPath id="c4"><path d="M0 0h1v1z"/></clipPath><clipPath id="c5"><path d="M0 0h1v1z"/></clipPath></defs><rect id="r" clip-path="url(#c3)"/><rect id="s" style="mask:url(#c5)"/></svg>"#
    ));
    let mut created = vec![
        id(&d, "c1"),
        id(&d, "c2"),
        id(&d, "c3"),
        id(&d, "c4"),
        id(&d, "c5"),
    ];
    gc_created_clips(&mut d, &mut created);
    assert_eq!(d.by_id("c2"), None, "nothing references c2");
    assert_eq!(
        d.by_id("c1"),
        None,
        "only the dead c2 referenced c1 → second pass"
    );
    assert!(d.by_id("c3").is_some(), "attribute reference");
    assert!(d.by_id("c4").is_some(), "stylesheet reference");
    assert!(d.by_id("c5").is_some(), "inline style reference");
    assert_eq!(created, vec![id(&d, "c3"), id(&d, "c4"), id(&d, "c5")]);
}

#[test]
fn ctx_finish_runs_both_sweeps() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><path d="M0 0h1v1z"/></clipPath></defs><g id="g"><path id="p" clip-path="url(#q)"/></g><rect id="q"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    ctx.created.push(id(&d, "c"));
    let n_q = id(&d, "q");
    delete_up(&mut d, &mut ctx, n_q);
    ctx.finish(&mut d);
    assert_eq!(d.by_id("c"), None);
    assert_eq!(d.attr(id(&d, "p"), "clip-path"), None);
}

#[test]
fn delete_up_never_touches_the_root() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="r" clip-path="url(#r)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let root = d.svg();
    delete_up(&mut d, &mut ctx, root);
    assert!(
        ctx.deleted.is_empty(),
        "nothing is deleted, so nothing is recorded"
    );
    assert!(d.by_id("r").is_some());
    ctx.finish(&mut d);
    assert_eq!(
        d.attr(id(&d, "r"), "clip-path"),
        Some("url(#r)"),
        "finish() has nothing to strip"
    );
}

#[test]
fn strip_whitespace_keeps_text_only_where_inkscape_needs_it() {
    let mut d = doc(&format!(
        "<svg {NS}>\n  <style>rect{{fill:red}}</style>\n  <g id=\"g\">\n    <path id=\"p\"/>\n    <!-- c -->\n    tail of comment\n  </g>\n  <text id=\"c\">A<!--note-->B</text>\n  <text id=\"t\" xml:space=\"preserve\">a<tspan id=\"s\">b</tspan> c<textPath id=\"tp\">d</textPath> e</text>\n  <flowRoot id=\"f\">x<flowPara id=\"fp\">y</flowPara> z</flowRoot>\n</svg>"
    ));
    strip_whitespace(&mut d);
    let s = out(&d);
    assert!(
        s.contains("<style>rect{fill:red}</style>"),
        "a <style>'s text stays: {s}"
    );
    assert!(
        s.contains("<g id=\"g\"><path id=\"p\"/><!-- c --></g>"),
        "group whitespace and the comment's tail go: {s}"
    );
    assert!(
        s.contains("<text id=\"c\">A<!--note-->B</text>"),
        "text after a comment inside <text> stays: {s}"
    );
    assert!(s.contains("<text id=\"t\" xml:space=\"preserve\">a<tspan id=\"s\">b</tspan> c<textPath id=\"tp\">d</textPath> e</text>"), "text runs and tspan/textPath tails stay: {s}");
    assert!(
        s.contains("<flowRoot id=\"f\">x<flowPara id=\"fp\">y</flowPara> z</flowRoot>"),
        "{s}"
    );
    assert!(
        !s.contains("\n"),
        "no stray whitespace between elements: {s}"
    );
}

#[test]
fn strip_attr_removes_an_attribute_everywhere() {
    let mut d = doc(&format!(
        r#"<svg {NS} unlinked_clone="True"><g unlinked_clone="True"><path id="p" unlinked_clone="True" d="M0 0"/></g><rect id="r"/></svg>"#
    ));
    assert_eq!(strip_attr(&mut d, "unlinked_clone"), 3);
    assert!(!out(&d).contains("unlinked_clone"), "{}", out(&d));
    assert_eq!(d.attr(id(&d, "p"), "d"), Some("M0 0"));
    assert_eq!(strip_attr(&mut d, "unlinked_clone"), 0);
}

#[test]
fn ops_cleanup_still_drops_an_inline_clip_path_reference_to_a_deleted_id() {
    let mut d = sciink::dom::Doc::parse(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><rect id="a" style="fill:red;clip-path:url(#c)"/><rect id="b" style="fill:blue"/></svg>"#
            .as_bytes(),
    )
    .unwrap();
    let mut deleted = std::collections::HashSet::new();
    deleted.insert("c".to_string());
    sciink::ops::cleanup::drop_dangling_refs(&mut d, &deleted);
    let a = d.by_id("a").unwrap();
    let b = d.by_id("b").unwrap();
    assert_eq!(d.attr(a, "style"), Some("fill:red"));
    assert_eq!(
        d.attr(b, "style"),
        Some("fill:blue"),
        "untouched style is not re-serialised"
    );
}

#[test]
fn an_upper_case_inline_clip_path_reference_is_still_dropped() {
    let mut d = sciink::dom::Doc::parse(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><rect id="a" style="fill:red;CLIP-PATH:url(#c)"/><rect id="b" style="fill:blue"/></svg>"#
            .as_bytes(),
    )
    .unwrap();
    let mut deleted = std::collections::HashSet::new();
    deleted.insert("c".to_string());
    sciink::ops::cleanup::drop_dangling_refs(&mut d, &deleted);
    let a = d.by_id("a").unwrap();
    assert_eq!(
        d.attr(a, "style"),
        Some("fill:red"),
        "an upper-case CLIP-PATH in the inline style must still be found and dropped"
    );
}
