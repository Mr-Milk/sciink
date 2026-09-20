mod support;

use std::path::PathBuf;

use sciink::dom::{Doc, NodeId};
use sciink::text::Warnings;
use sciink::text::fonts::FontSystem;
use sciink::text::parse::ParsedText;
use sciink::text::table::CharTable;

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";
const DV: &str = "font-family:'DejaVu Sans'";

/// Parses `el` (and builds the char table over every `<text>` in the document).
fn parsed(d: &mut Doc, el: &str) -> (ParsedText, CharTable) {
    let els: Vec<NodeId> = d
        .descendants(d.svg())
        .filter(|&n| d.is_element(n) && d.tag(n) == "text")
        .collect();
    let mut w = Warnings::default();
    let n = id(d, el);
    let mut ct = CharTable::build(d, &els, fonts(), &mut w);
    let pt = ParsedText::parse(d, n, &mut ct, &mut w).expect("parsed");
    (pt, ct)
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

use sciink::style::Style;
use sciink::text::edit::{delete_char, reindex, remove_chars, remove_textlength, sel, style_eq};

fn positions(pt: &ParsedText) -> Vec<(char, f64, f64)> {
    let mut v = Vec::new();
    for (li, ln) in pt.lines.iter().enumerate() {
        for ci in 0..ln.chunks.len() {
            let p = sciink::text::layout::chunk_char_pts(pt, li, ci);
            for (wi, &c) in ln.chunks[ci].chars.iter().enumerate() {
                v.push((pt.chars[c].c, p[wi][0].x, p[wi][0].y));
            }
        }
    }
    v
}
/// Same characters at the same places (floats compared with `close`, never `==`).
fn assert_pos(a: &[(char, f64, f64)], b: &[(char, f64, f64)]) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!(
            x.0 == y.0 && close(x.1, y.1) && close(x.2, y.2),
            "{x:?} vs {y:?}"
        );
    }
}

#[test]
fn style_eq_ignores_order_and_sel_resolves_tails() {
    let a = Style::parse("fill:red;font-size:10px");
    let b = Style::parse("font-size:10px;fill:red");
    let c = Style::parse("font-size:10px;fill:blue");
    assert!(style_eq(&a, &b) && !style_eq(&a, &c) && !style_eq(&a, &Style::parse("fill:red")));
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0">a<tspan id="s">b</tspan>c</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (pt, _) = parsed(&mut d, "t");
    assert_eq!(sel(&d, &pt.chars[0].loc), id(&d, "t"));
    assert_eq!(sel(&d, &pt.chars[1].loc), id(&d, "s"));
    assert!(pt.chars[2].loc.tail);
    assert_eq!(
        sel(&d, &pt.chars[2].loc),
        id(&d, "t"),
        "a tail belongs to the parent"
    );
}

#[test]
fn remove_chars_prunes_and_reindexes() {
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0 30 60" y="0">abc<tspan x="0" y="20">d</tspan></text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    sciink::text::layout::snapshot_parsed(&mut pt);
    let before = positions(&pt);
    let map = remove_chars(&mut pt, &[1, 3]); // 'b' (own chunk) and 'd' (own line)
    assert_eq!(map, [0, 2], "new → old");
    assert_eq!(pt.text(), "ac");
    assert_eq!(pt.lines.len(), 1);
    assert_eq!(
        pt.lines[0].chunks.len(),
        2,
        "b's chunk is gone, a's and c's stay"
    );
    assert_eq!(pt.lines[0].chunks[1].id, 2, "ids survive");
    assert_eq!(pt.lines[0].chars, [0, 1]);
    for (i, c) in pt.chars.iter().enumerate() {
        assert_eq!((c.line, c.windex), (0, 0));
        assert_eq!(c.chunk, i);
    }
    assert_eq!(pt.parsed_ut.len(), 2);
    let after = positions(&pt);
    assert_pos(&after, &[before[0], before[2]]);
    // reindex on an untouched model is the identity
    assert_eq!(reindex(&mut pt), [0, 1]);
}

#[test]
fn delete_char_shifts_the_chunk_by_the_anchor_rule() {
    // digits: DejaVu Sans has no kerning pairs between them, so the expectations below are exact
    // start anchor: deleting the LAST char changes nothing else; deleting the FIRST moves x right
    let svg = |anchor: &str| {
        format!(
            r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:{anchor}" x="10" y="0">123</text></svg>"#
        )
    };
    let mut d = Doc::parse(svg("start").as_bytes()).unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    delete_char(&mut pt, 2);
    assert_eq!(pt.text(), "12");
    assert!(close(pt.lines[0].chunks[0].x, 10.0));
    assert_pos(&positions(&pt), &before[..2]);
    delete_char(&mut pt, 0);
    assert_eq!(pt.text(), "2");
    let (_, bx, _) = before[1];
    assert!(
        close(pt.lines[0].chunks[0].x, bx),
        "'2' stays where it was: {} vs {bx}",
        pt.lines[0].chunks[0].x
    );

    // end anchor: deleting the last char moves x left by its width; positions of the rest are kept
    let mut d = Doc::parse(svg("end").as_bytes()).unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    let cw = pt.chars[2].cwd;
    let kern = sciink::text::layout::chunk_geom(&pt, 0, 0);
    let gap = kern.left[2] - kern.right[1]; // pair kerning 2→3 (0 for digits)
    delete_char(&mut pt, 2);
    assert!(
        close(pt.lines[0].chunks[0].x, 10.0 - cw - gap),
        "{}",
        pt.lines[0].chunks[0].x
    );
    assert_pos(&positions(&pt), &before[..2]);

    // middle anchor: deleting the last char moves x by half its width
    let mut d = Doc::parse(svg("middle").as_bytes()).unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    delete_char(&mut pt, 2);
    let after = positions(&pt);
    for (b, a) in before.iter().zip(&after) {
        assert!(close(b.1, a.1), "{b:?} vs {a:?}");
    }

    // an unrendered trailing space costs nothing when deleted (P:4047–4051)
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:end" x="10" y="0">12 </text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    delete_char(&mut pt, 2);
    assert!(close(pt.lines[0].chunks[0].x, 10.0));
    assert_pos(&positions(&pt), &before[..2]);

    // deleting the only char of a line removes the line
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0">a<tspan x="0" y="20">b</tspan></text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    delete_char(&mut pt, 1);
    assert_eq!(pt.lines.len(), 1);
    assert_eq!(pt.text(), "a");
}

#[test]
fn remove_textlength_restores_widths_and_records_the_transform() {
    use sciink::text::parse::TextLengthAdj;
    // spacingAndGlyphs: 4 chars stretched to 100 units
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0" textLength="100" lengthAdjust="spacingAndGlyphs">abcd</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let Some(TextLengthAdj::SpacingAndGlyphs(adj)) = pt.text_length else {
        panic!("parsed adj")
    };
    let stretched: Vec<f64> = pt.chars.iter().map(|c| c.cwd).collect();
    let with = sciink::text::layout::full_extent(&pt).unwrap();
    remove_textlength(&mut pt);
    assert_eq!(pt.text_length, None);
    assert!(pt.text_length_removed);
    for (c, s) in pt.chars.iter().zip(&stretched) {
        assert!(
            close(c.cwd * adj, *s),
            "cwd restored: {} × {adj} vs {s}",
            c.cwd
        );
    }
    let without = sciink::text::layout::full_extent(&pt).unwrap();
    // translate(cx_with,0) scale(adj,1) translate(-cx_without,0)
    let expect = kurbo::Affine::translate((with.center().x, 0.0))
        * kurbo::Affine::scale_non_uniform(adj, 1.0)
        * kurbo::Affine::translate((-without.center().x, 0.0));
    assert!(sciink::geom::affine_eq(pt.transform_extra, expect));
    assert!(
        sciink::geom::affine_eq(pt.transform, expect),
        "element had no transform of its own"
    );
    // the stretched extent is reproduced by transform ∘ restored widths
    let mapped = sciink::geom::transform_rect(pt.transform_extra, without);
    assert!(
        (mapped.width() - with.width()).abs() < 1e-6
            && (mapped.center().x - with.center().x).abs() < 1e-6
    );

    // spacing: letter-spacing is written into every character's style, nothing else moves
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0" textLength="100">abcd</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let lsp = pt.chars[0].lsp;
    assert!(lsp > 0.0);
    let before = positions(&pt);
    remove_textlength(&mut pt);
    assert_eq!(positions(&pt), before);
    assert_eq!(
        pt.chars[0].sty.get("letter-spacing"),
        Some(sciink::num::fmt(lsp).as_str())
    );
    assert!(sciink::geom::is_identity(pt.transform_extra));

    // nothing to do without textLength
    let mut d = Doc::parse(
        format!(
            r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0">ab</text></svg>"#
        )
        .as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    remove_textlength(&mut pt);
    assert!(!pt.text_length_removed);
}
