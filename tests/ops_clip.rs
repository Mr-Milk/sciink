mod support;

use kurbo::Affine;
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

fn kids(d: &Doc, n: NodeId) -> Vec<NodeId> {
    d.children(n).filter(|&c| d.is_element(c)).collect()
}

use sciink::ops::ClipKind;
use sciink::ops::clip::{
    compose_all, deswitch, group, lang_matches, merge_clipmask, preferences_language, ungroup,
    unlink,
};
use sciink::style::Style;

fn clip_target(d: &Doc, n: NodeId) -> NodeId {
    let v = d.attr(n, "clip-path").expect("clip-path attribute");
    id(d, v.trim_start_matches("url(#").trim_end_matches(')'))
}

#[test]
fn compose_all_pushes_style_transform_and_clip_onto_a_child() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><g id="g" transform="translate(1,2)" style="fill:red"><path id="p" d="M0 0h1" transform="scale(2)" style="fill:blue"/><path id="q" d="M0 0h1"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (g, p, q, c) = (id(&d, "g"), id(&d, "p"), id(&d, "q"), id(&d, "c"));
    let gst = d.cascaded_style(g);
    let t = d.transform(g);
    assert!(
        !compose_all(&mut d, &mut ctx, p, None, None, t, Some(&gst), false),
        "no clip → never clipped out"
    );
    assert_eq!(
        d.attr(p, "transform"),
        Some("matrix(2,0,0,2,1,2)"),
        "group transform first, then the child's own"
    );
    assert_eq!(d.attr(p, "clip-path"), None);
    assert_eq!(
        Style::parse(d.attr(p, "style").unwrap()).get("fill"),
        Some("blue")
    );
    // an untransformed child simply points at the group's clip
    assert!(
        !compose_all(&mut d, &mut ctx, q, Some(c), None, t, Some(&gst), false),
        "a clip that was merely attached never clips out"
    );
    assert_eq!(d.attr(q, "clip-path"), Some("url(#c)"));
    assert_eq!(d.attr(q, "transform"), Some("translate(1,2)"));
    assert_eq!(
        Style::parse(d.attr(q, "style").unwrap()).get("fill"),
        Some("red")
    );
    // identity transform: attribute untouched; text with remove_text_clip loses its clips
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><text id="t" clip-path="url(#c)" mask="url(#c)">x</text></svg>"#
    ));
    let t = id(&d, "t");
    let c = id(&d, "c");
    assert!(!compose_all(
        &mut d,
        &mut ctx,
        t,
        Some(c),
        None,
        Affine::IDENTITY,
        None,
        true
    ));
    assert_eq!(
        (
            d.attr(t, "clip-path"),
            d.attr(t, "mask"),
            d.attr(t, "transform")
        ),
        (None, None, None)
    );
}

#[test]
fn merge_clipmask_attaches_a_new_clip_or_intersects_rectangles() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c1"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="c2"><path d="M5 5 L20 5 L20 20 L5 20 Z"/></clipPath><clipPath id="c3"><rect x="20" y="20" width="5" height="5"/></clipPath></defs><rect id="plain" width="1" height="1"/><rect id="clipped" width="1" height="1" clip-path="url(#c1)"/><rect id="gone" width="1" height="1" clip-path="url(#c1)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (c1, c2, c3) = (id(&d, "c1"), id(&d, "c2"), id(&d, "c3"));
    // no existing clip: point at the new one
    let plain = id(&d, "plain");
    assert!(!merge_clipmask(
        &mut d,
        &mut ctx,
        plain,
        c2,
        ClipKind::Clip,
        0
    ));
    assert_eq!(d.attr(plain, "clip-path"), Some("url(#c2)"));
    assert!(ctx.created.is_empty(), "nothing duplicated");
    // existing rectangular clip ∩ new rectangular clip → one path with the intersection box
    let clipped = id(&d, "clipped");
    assert!(!merge_clipmask(
        &mut d,
        &mut ctx,
        clipped,
        c2,
        ClipKind::Clip,
        0
    ));
    let dup = clip_target(&d, clipped);
    assert_ne!(dup, c1, "the original clip is untouched");
    assert_eq!(ctx.created, vec![dup]);
    assert_eq!(d.parent(dup), Some(d.defs()));
    let k = kids(&d, dup);
    assert_eq!(k.len(), 1);
    assert_eq!(d.tag(k[0]), "path");
    assert_eq!(d.attr(k[0], "d"), Some("M 5,5 L 10,5 L 10,10 L 5,10 Z"));
    assert_eq!(kids(&d, c1).len(), 1, "c1 still has its rect");
    assert_eq!(d.tag(kids(&d, c1)[0]), "rect");
    // disjoint rectangles → the child goes and the node is reported clipped out
    let gone = id(&d, "gone");
    assert!(merge_clipmask(
        &mut d,
        &mut ctx,
        gone,
        c3,
        ClipKind::Clip,
        0
    ));
    let dup2 = clip_target(&d, gone);
    assert!(kids(&d, dup2).is_empty());
    assert_eq!(ctx.created.len(), 2);
}

#[test]
fn merge_clipmask_counter_transforms_the_new_clip_and_nests_non_rectangles() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="tri"><path d="M0 0 L10 0 L12 10 Z"/></clipPath><mask id="m"><rect width="5" height="5"/></mask></defs><rect id="moved" width="1" height="1" transform="translate(10,0)"/><rect id="odd" width="1" height="1" clip-path="url(#tri)"/><rect id="masked" width="1" height="1" mask="url(#m)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (c, tri, m) = (id(&d, "c"), id(&d, "tri"), id(&d, "m"));
    // a transformed node: a copy of the new clip gets the inverse transform on its children
    let moved = id(&d, "moved");
    assert!(!merge_clipmask(
        &mut d,
        &mut ctx,
        moved,
        c,
        ClipKind::Clip,
        0
    ));
    let dup = clip_target(&d, moved);
    assert_ne!(dup, c);
    assert_eq!(
        d.attr(kids(&d, dup)[0], "transform"),
        Some("translate(-10,0)")
    );
    assert_eq!(d.attr(kids(&d, c)[0], "transform"), None);
    // a non-rectangular existing clip: its children are clipped by the new clip recursively
    let odd = id(&d, "odd");
    assert!(!merge_clipmask(&mut d, &mut ctx, odd, c, ClipKind::Clip, 0));
    let dup = clip_target(&d, odd);
    assert_ne!(dup, tri);
    let inner = kids(&d, dup)[0];
    assert_eq!(d.tag(inner), "path");
    assert_eq!(d.attr(inner, "clip-path"), Some("url(#c)"));
    // masks are never rectangle-intersected
    let masked = id(&d, "masked");
    assert!(!merge_clipmask(
        &mut d,
        &mut ctx,
        masked,
        c,
        ClipKind::Mask,
        0
    ));
    let v = d.attr(masked, "mask").unwrap().to_string();
    let dup = id(&d, v.trim_start_matches("url(#").trim_end_matches(')'));
    assert_ne!(dup, m);
    assert_eq!(d.attr(kids(&d, dup)[0], "mask"), Some("url(#c)"));
    assert_eq!(ctx.created.len(), 3);
}

#[test]
fn merge_clipmask_unlinks_clones_inside_clips_and_stops_at_max_nest() {
    let mut d = doc(&format!(
        r##"<svg {NS}><defs><rect id="src" width="10" height="10"/><clipPath id="c"><use xlink:href="#src"/></clipPath><clipPath id="loop"><rect id="lr" width="1" height="1" clip-path="url(#loop)"/></clipPath></defs><rect id="r" width="1" height="1"/><rect id="cyc" width="3" height="3" clip-path="url(#loop)"/></svg>"##
    ));
    let mut ctx = Ctx::new();
    let c = id(&d, "c");
    let n_r = id(&d, "r");
    assert!(!merge_clipmask(&mut d, &mut ctx, n_r, c, ClipKind::Clip, 0));
    assert_eq!(
        d.tag(kids(&d, c)[0]),
        "rect",
        "the <use> child of the clip was unlinked in place"
    );
    assert!(d.by_id("src").is_some());
    // a self-referencing clip chain cannot overflow the stack
    let loop_ = id(&d, "loop");
    let n_cyc = id(&d, "cyc");
    assert!(!merge_clipmask(
        &mut d,
        &mut ctx,
        n_cyc,
        loop_,
        ClipKind::Clip,
        0
    ));
    assert!(
        ctx.warn.0.iter().any(|w| w.contains("64 levels")),
        "{:?}",
        ctx.warn.0
    );
}

#[test]
fn unlink_replaces_a_clone_with_a_composed_copy() {
    let mut d = doc(&format!(
        r##"<svg {NS}><defs><g id="sym"><rect id="r" width="1" height="1" style="fill:red"/><use id="nested" xlink:href="#r" x="2"/></g><symbol id="s"><circle id="c" r="1"/></symbol><clipPath id="cp"><rect width="1" height="1"/></clipPath></defs><use id="u" xlink:href="#sym" x="3" y="4" transform="scale(2)" style="opacity:0.5" clip-path="url(#cp)"/><use id="dangling" xlink:href="#nope"/><use id="us" xlink:href="#s" transform="translate(1,1)"/></svg>"##
    ));
    let mut ctx = Ctx::new();
    let u = id(&d, "u");
    let copy = unlink(&mut d, &mut ctx, u).unwrap();
    assert_eq!(d.tag(copy), "g");
    assert_eq!(
        d.attr(copy, "id"),
        Some("u"),
        "the copy takes the clone's id"
    );
    assert_eq!(d.attr(copy, "unlinked_clone"), Some("True"));
    assert_eq!(
        d.attr(copy, "transform"),
        Some("matrix(2,0,0,2,6,8)"),
        "scale(2) · translate(3,4)"
    );
    assert_eq!(d.attr(copy, "style"), Some("opacity:0.5"));
    // the copy carried translate(3,4) when the clip was merged, so the clip is a
    // counter-transformed duplicate, not `cp` itself
    let cpv = d.attr(copy, "clip-path").expect("clip-path").to_string();
    let cdup = id(&d, cpv.trim_start_matches("url(#").trim_end_matches(')'));
    assert_ne!(cdup, id(&d, "cp"));
    assert_eq!(
        d.attr(kids(&d, cdup)[0], "transform"),
        Some("translate(-3,-4)")
    );
    let k = kids(&d, copy);
    assert_eq!(k.len(), 2);
    assert_eq!(d.tag(k[0]), "rect");
    assert_eq!(
        d.attr(k[0], "id"),
        None,
        "descendants of a copy carry no ids"
    );
    assert_eq!(
        d.tag(k[1]),
        "rect",
        "the nested clone inside the copy was unlinked too"
    );
    assert_eq!(d.attr(k[1], "transform"), Some("translate(2,0)"));
    assert!(
        d.by_id("sym").is_some() && d.by_id("r").is_some() && d.by_id("nested").is_some(),
        "originals stay"
    );
    assert_eq!(d.parent(copy), Some(d.svg()));
    assert!(!out(&d).contains(r#"<use id="u""#));
    // a clone of nothing is deleted
    let n_dangling = id(&d, "dangling");
    assert_eq!(unlink(&mut d, &mut ctx, n_dangling), None);
    assert_eq!(d.by_id("dangling"), None);
    // a symbol becomes a group (Inkscape's Unlink Clone behaviour)
    let n_us = id(&d, "us");
    let g = unlink(&mut d, &mut ctx, n_us).unwrap();
    assert_eq!(d.tag(g), "g");
    assert_eq!(d.attr(g, "transform"), Some("translate(1,1)"));
    assert_eq!(d.tag(kids(&d, g)[0]), "circle");
    assert_eq!(
        out(&d).matches("<symbol").count(),
        1,
        "only the original symbol remains: {}",
        out(&d)
    );
    assert!(d.by_id("s").is_some(), "…in defs, untouched");
}

#[test]
fn group_wraps_elements_in_place() {
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="a"/><path id="b"/><path id="c"/></svg>"#
    ));
    let (a, c) = (id(&d, "a"), id(&d, "c"));
    let g = group(&mut d, &[a, c]);
    assert_eq!(
        out(&d),
        format!(r#"<svg {NS}><g><path id="a"/><path id="c"/></g><path id="b"/></svg>"#)
    );
    assert_eq!(d.parent(a), Some(g));
    let empty = group(&mut d, &[]);
    assert_eq!(d.parent(empty), None, "an empty group is returned detached");
}

#[test]
fn ungroup_composes_onto_children_in_order_and_keeps_unungroupables() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g id="g" transform="translate(1,1)" style="fill:red;opacity:0.5" xml:space="preserve"><!-- note --><path id="a" d="M0 0h1"/><path id="b" d="M0 0h1" transform="scale(2)" style="fill:blue" xml:space="default"/><defs id="dd"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let g = id(&d, "g");
    ungroup(&mut d, &mut ctx, g, false);
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    let svg_kids = kids(&d, d.svg());
    assert_eq!(
        svg_kids,
        vec![g, a, b],
        "children follow the group in their original order"
    );
    assert_eq!(
        kids(&d, g),
        vec![id(&d, "dd")],
        "<defs> stays inside, so the group survives"
    );
    assert!(!out(&d).contains("<!-- note -->"), "comments are dropped");
    assert_eq!(d.attr(a, "transform"), Some("translate(1,1)"));
    assert_eq!(d.attr(b, "transform"), Some("matrix(2,0,0,2,1,1)"));
    let sa = Style::parse(d.attr(a, "style").unwrap());
    assert_eq!(
        (sa.get("fill"), sa.get("opacity")),
        (Some("red"), Some("0.5"))
    );
    let sb = Style::parse(d.attr(b, "style").unwrap());
    assert_eq!(
        (sb.get("fill"), sb.get("opacity")),
        (Some("blue"), Some("0.5"))
    );
    assert_eq!(d.attr(a, "xml:space"), Some("preserve"));
    assert_eq!(
        d.attr(b, "xml:space"),
        Some("default"),
        "an own xml:space wins"
    );
    // an emptied group disappears; a clipped-out child is deleted, not moved
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c1"><rect width="10" height="10"/></clipPath><clipPath id="c2"><rect x="20" y="20" width="1" height="1"/></clipPath></defs><g id="g" clip-path="url(#c1)"><path id="keep" d="M0 0h1"/><path id="drop" d="M0 0h1" clip-path="url(#c2)"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_g = id(&d, "g");
    ungroup(&mut d, &mut ctx, n_g, false);
    assert_eq!(d.by_id("g"), None);
    assert_eq!(d.by_id("drop"), None);
    let keep = id(&d, "keep");
    assert_eq!(d.attr(keep, "clip-path"), Some("url(#c1)"));
    assert_eq!(d.parent(keep), Some(d.svg()));
}

#[test]
fn deswitch_keeps_the_language_match_and_language_helpers_work() {
    assert!(
        lang_matches("en", "en") && lang_matches("en-US", "en") && lang_matches("de, en-GB", "en")
    );
    assert!(lang_matches("EN", "en") && lang_matches("en", "en_US"));
    assert!(!lang_matches("de", "en") && !lang_matches("", "en"));
    assert_eq!(
        preferences_language(
            r#"<inkscape version="1"><group id="options"/><group foo="1" id="ui" language="de" bar="2"/></inkscape>"#
        ),
        Some("de".to_string())
    );
    assert_eq!(
        preferences_language(r#"<inkscape><group id="ui" language=""/></inkscape>"#),
        None
    );
    assert_eq!(
        preferences_language(r#"<inkscape><group id="ui"/></inkscape>"#),
        None
    );
    let src = format!(
        r#"<svg {NS}><switch id="s" transform="translate(1,0)"><text id="de" systemLanguage="de">Hallo</text><text id="en" systemLanguage="en-US">Hello</text><text id="x">Fallback</text></switch></svg>"#
    );
    let mut d = doc(&src);
    let mut ctx = Ctx::new();
    let n_s = id(&d, "s");
    deswitch(&mut d, &mut ctx, n_s, "en");
    assert_eq!(d.by_id("s"), None);
    assert_eq!((d.by_id("de"), d.by_id("x")), (None, None));
    let en = id(&d, "en");
    assert_eq!(d.attr(en, "systemLanguage"), None);
    assert_eq!(d.attr(en, "transform"), Some("translate(1,0)"));
    assert_eq!(d.parent(en), Some(d.svg()));
    // no match: the attribute-less child is the survivor; nothing matches at all: the first
    let mut d = doc(&src);
    let n_s = id(&d, "s");
    deswitch(&mut d, &mut ctx, n_s, "fr");
    assert!(d.by_id("x").is_some() && d.by_id("en").is_none() && d.by_id("de").is_none());
    let mut d = doc(&format!(
        r#"<svg {NS}><switch id="s"><text id="de" systemLanguage="de">a</text><text id="it" systemLanguage="it">b</text></switch></svg>"#
    ));
    let n_s = id(&d, "s");
    deswitch(&mut d, &mut ctx, n_s, "fr");
    assert!(d.by_id("de").is_some() && d.by_id("it").is_none());
}
