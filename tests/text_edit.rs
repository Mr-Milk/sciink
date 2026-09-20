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
    assert_pos(&positions(&pt), &before);
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

use sciink::text::edit::{make_next_chain, rechunk_absolute, unique_reps};

#[test]
fn unique_reps_keeps_the_first_of_each_cluster() {
    let r = unique_reps(&[3.0, 1.0, 1.0005, 2.0, 3.0004], 0.001);
    assert_eq!(r.len(), 3, "{r:?}");
    for (a, b) in r.iter().zip(&[1.0, 2.0, 3.0]) {
        assert!(close(*a, *b), "{r:?}");
    }
    assert!(unique_reps(&[], 0.1).is_empty());
}

#[test]
fn next_chain_links_chunks_on_one_baseline_in_x_order() {
    // three chunks of the root run written out of x order ("abe" at 60, 0, 90), a fourth chunk on
    // the same baseline from a positioned tspan (own x, inherited y), and one on another baseline.
    // (A position list longer than an element's OWN text is truncated by depathologize — Plan 3's
    // sanctioned simplification of P:4759–4835 — so per-character x values must sit on the run
    // whose characters they position.)
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="60 0 90" y="0">abe<tspan id="s" x="30">c</tspan><tspan x="0" y="20">d</tspan></text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    assert_eq!(pt.lines.len(), 3);
    assert!(
        pt.lines[1].spec.continue_y && close(pt.lines[1].chunks[0].y, 0.0),
        "'c' continues the baseline"
    );
    make_next_chain(&d, &mut pt);
    let by_char = |c: char| -> (usize, usize) {
        let tc = pt.chars.iter().find(|t| t.c == c).unwrap();
        (tc.line, tc.chunk)
    };
    let chunk = |c: char| pt.chunk(by_char(c).0, by_char(c).1).clone();
    // x order is b (0), c (30), a (60), e (90)
    assert_eq!(chunk('b').next, Some(chunk('c').id));
    assert_eq!(chunk('c').next, Some(chunk('a').id));
    assert_eq!(chunk('a').next, Some(chunk('e').id));
    assert_eq!(chunk('e').next, None);
    assert_eq!(chunk('b').prev, None);
    assert_eq!(chunk('a').prev, Some(chunk('c').id));
    assert_eq!(chunk('e').prev, Some(chunk('a').id));
    assert_eq!(
        (chunk('d').next, chunk('d').prev),
        (None, None),
        "other baseline"
    );
    // c sits in <tspan id="s">, its neighbours in the text node: different style nodes;
    // a and e share the text node
    assert!(!chunk('c').prev_same_tspan);
    assert!(!chunk('a').prev_same_tspan);
    assert!(chunk('e').prev_same_tspan);
}

#[test]
fn next_chain_swaps_a_space_sitting_on_the_next_chunk() {
    // PDF-import bug (P:717–720): a " " chunk with the same x as the following chunk is ordered after it
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0 20 20" y="0">a b</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    make_next_chain(&d, &mut pt);
    let ids: Vec<u32> = pt.chunks().map(|(l, c)| pt.chunk(l, c).id).collect();
    // chunks: a (id 0), " " (id 1), b (id 2); sorted by centre " " comes before b, then swapped
    assert_eq!(pt.chunk(0, 0).next, Some(ids[2]), "a → b");
    assert_eq!(pt.chunk(0, 2).next, Some(ids[1]), "b → space");
    assert_eq!(pt.chunk(0, 1).next, None);
}

#[test]
fn rechunk_absolute_turns_dx_into_new_lines_without_moving_glyphs() {
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="50" y="0" dx="0 0 3 0 -2">abcde</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    assert!(pt.any_dx);
    let before = positions(&pt);
    rechunk_absolute(&mut pt);
    assert!(!pt.any_dx);
    assert!(pt.chars.iter().all(|c| c.dx == 0.0));
    // 'c' and 'e' carried dx → each opens a new line: "ab" | "cd" | "e"
    let texts: Vec<String> = (0..pt.lines.len()).map(|li| pt.line_text(li)).collect();
    assert_eq!(texts, ["ab", "cd", "e"]);
    assert!(pt.lines.iter().all(|l| l.chunks.len() == 1));
    assert!(!pt.lines[1].spec.sprl && !pt.lines[1].spec.continue_x);
    let after = positions(&pt);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!(close(b.1, a.1) && close(b.2, a.2), "{b:?} vs {a:?}");
    }
    // middle anchor: the new chunk's x is the anchor point of its glyph run (P:1676)
    let g = sciink::text::layout::chunk_geom(&pt, 1, 0);
    assert!(close(
        pt.lines[1].chunks[0].x,
        0.5 * (g.left[0] + g.right[1])
    ));

    // a dx'd character right after a positioned one opens a CHUNK on the same line (both have a
    // coordinate, P:2704–2716); a coordinate after a character WITHOUT one opens a LINE
    // (P:886–893) whose missing x continues from the end of the previous line's last chunk.
    // Digits: no pair kerning, so the continued pen position is exact.
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0" dx="0 1" dy="0 0 4">123</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    rechunk_absolute(&mut pt);
    let texts: Vec<String> = (0..pt.lines.len()).map(|li| pt.line_text(li)).collect();
    assert_eq!(texts, ["12", "3"]);
    assert_eq!(
        pt.lines[0].chunks.len(),
        2,
        "'2' carried a dx: its own chunk, same line"
    );
    assert_eq!(pt.chunk_text(0, 1), "2");
    assert!(pt.lines[1].spec.continue_x && !pt.lines[1].spec.continue_y);
    assert!(close(pt.lines[1].chunks[0].y, 4.0));
    let g2 = sciink::text::layout::chunk_geom(&pt, 0, 1);
    assert!(
        close(pt.lines[1].chunks[0].x, g2.pts_ut[3].x),
        "'3' starts where '2' ends"
    );
    let after = positions(&pt);
    for (b, a) in before.iter().zip(&after) {
        assert!(close(b.1, a.1) && close(b.2, a.2), "{b:?} vs {a:?}");
    }

    // no dx: untouched (dy alone does not trigger the conversion, P:1667)
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0" dy="0 4">ab</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    rechunk_absolute(&mut pt);
    assert_eq!(pt.lines.len(), 1);
    assert!(close(pt.chars[1].dy, 4.0));
}

#[test]
fn rechunk_absolute_keeps_adjacent_positioned_characters_on_one_line() {
    // two consecutive dx'd characters: every character has a coordinate, so nothing opens a new
    // line — three chunks on one line (upstream P:875–893 splits only after a None)
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0" dx="0 1 1">123</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    let before = positions(&pt);
    rechunk_absolute(&mut pt);
    assert_eq!(pt.lines.len(), 1);
    assert_eq!(pt.lines[0].chunks.len(), 3);
    assert!(pt.chars.iter().all(|c| c.dx == 0.0));
    let after = positions(&pt);
    for (b, a) in before.iter().zip(&after) {
        assert!(close(b.1, a.1) && close(b.2, a.2), "{b:?} vs {a:?}");
    }
}

#[test]
fn rechunk_absolute_y_only_chunks_carry_x_and_continue_lines_read_the_last_chunk() {
    // "1234": dy on '2' (+4) and '3' (−4), dx on '4'. '2' and '3' keep a coordinate (their y), so
    // they open chunks on the first line whose x is carried forward from the chunk before
    // (upstream `x[min(i, len(x)−1)]`, P:2710 — a quirk shared with parse time: such characters
    // sit at the line's x, not at the pen); '4' opens a line whose missing y comes from the
    // previous line's LAST chunk ('3', back on the baseline).
    let mut d = Doc::parse(
        format!(r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="7" y="0" dx="0 0 0 1" dy="0 4 -4 0">1234</text></svg>"#).as_bytes(),
    )
    .unwrap();
    let (mut pt, _) = parsed(&mut d, "t");
    rechunk_absolute(&mut pt);
    let texts: Vec<String> = (0..pt.lines.len()).map(|li| pt.line_text(li)).collect();
    assert_eq!(texts, ["123", "4"]);
    let l0 = &pt.lines[0];
    assert_eq!(l0.chunks.len(), 3);
    assert!(
        close(l0.chunks[1].x, 7.0) && close(l0.chunks[1].y, 4.0),
        "'2': own y, carried x"
    );
    assert!(
        close(l0.chunks[2].x, 7.0) && close(l0.chunks[2].y, 0.0),
        "'3': back on the baseline"
    );
    assert!(pt.lines[1].spec.continue_y && !pt.lines[1].spec.continue_x);
    assert!(
        close(pt.lines[1].chunks[0].y, 0.0),
        "y from the previous line's LAST chunk"
    );
    assert!(pt.chars.iter().all(|c| c.dx == 0.0 && c.dy == 0.0));
}

use kurbo::Point;
use sciink::text::edit::{Incoming, WType, append_chunks};
use sciink::text::layout::{chunk_geom, snapshot_parsed};

/// Two <text> elements, the second placed `gap_spaces` space-widths after the first.
fn two_texts(gap_spaces: f64, second_style: &str) -> (Doc, Vec<ParsedText>, CharTable) {
    let probe = format!(
        r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text></svg>"#
    );
    let mut pd = Doc::parse(probe.as_bytes()).unwrap();
    let (ppt, _) = parsed(&mut pd, "a");
    let g = chunk_geom(&ppt, 0, 0);
    let x2 = g.right[4] + gap_spaces * ppt.chars[0].spw;
    let svg = format!(
        r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="b" xml:space="preserve" style="{DV};font-size:10px;{second_style}" x="{x2}" y="0">world</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<NodeId> = ["a", "b"].iter().map(|i| id(&d, i)).collect();
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &els, fonts(), &mut w);
    let mut pts: Vec<ParsedText> = els
        .iter()
        .map(|&e| ParsedText::parse(&mut d, e, &mut ct, &mut w).unwrap())
        .collect();
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
    }
    (d, pts, ct)
}

fn all_positions(pts: &[ParsedText]) -> Vec<(char, f64, f64)> {
    let mut v: Vec<(char, f64, f64)> = pts
        .iter()
        .flat_map(|pt| {
            positions(pt).into_iter().map(|(c, x, y)| {
                let p = pt.transform * Point::new(x, y);
                (c, p.x, p.y)
            })
        })
        .filter(|p| p.0 != ' ')
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

#[test]
fn append_chunks_inserts_the_right_number_of_spaces_and_keeps_glyphs_in_place() {
    let (d, mut pts, mut ct) = two_texts(1.0, "");
    let before = all_positions(&pts);
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Normal,
        max_spaces: None,
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Hello world");
    assert!(
        pts[1].chars.is_empty() && pts[1].lines.is_empty(),
        "source emptied"
    );
    assert_eq!(pts[0].lines[0].chunks.len(), 1);
    let after = all_positions(&pts);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!(
            (b.1 - a.1).abs() < 1e-6 && (b.2 - a.2).abs() < 1e-6,
            "{b:?} vs {a:?}"
        );
    }
    // the inserted space is a copy of 'o' with c=' ', dx = −lsp (0 here), no snapshot
    let sp = &pts[0].chars[5];
    assert_eq!(sp.c, ' ');
    assert!(close(sp.dx, 0.0) && close(sp.dy, 0.0));
    assert_eq!(pts[0].parsed_ut[5], None);
    assert!(
        pts[0].parsed_ut[6].is_some(),
        "moved chars keep their snapshot"
    );
    // moved chars' parsed points were re-expressed in the target frame (identity here → unchanged)
    assert_eq!(pts[0].parsed_ut[6], pts[0].parsed_t[6]);
    // model indices are consistent
    for (i, c) in pts[0].chars.iter().enumerate() {
        assert_eq!((c.line, c.chunk, c.windex), (0, 0, i));
    }

    // max_spaces = Some(0) drops the gap: text has no space, 'w' now touches 'o'
    let (d, mut pts, mut ct) = two_texts(1.0, "");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Normal,
        max_spaces: Some(0),
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Helloworld");

    // a 2.4-space gap rounds to 2 spaces
    let (d, mut pts, mut ct) = two_texts(2.4, "");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Normal,
        max_spaces: None,
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Hello  world");
}

#[test]
fn append_chunks_nativizes_superscripts_and_percent_sizes() {
    // superscript: smaller text merged as Super gets 65 % size and +40 % baseline of the host
    let (d, mut pts, mut ct) = two_texts(0.0, "font-size:6px");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Super,
        max_spaces: Some(0),
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    let w = &pts[0].chars[5];
    assert_eq!(w.c, 'w');
    assert!(close(w.utfs, 6.5) && close(w.bshft, 4.0));
    assert_eq!(w.sty.get("baseline-shift"), Some("super"));
    assert_eq!(w.sty.get("font-size"), Some("65%"));
    assert!(close(w.cwd, w.prop.charw * 6.5));
    assert!(!sciink::text::edit::style_eq(&w.sty, &pts[0].chars[4].sty));

    // a differently sized Normal merge is size-corrected to a whole percent of the host size
    let (d, mut pts, mut ct) = two_texts(1.0, "font-size:8px");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Normal,
        max_spaces: None,
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    let w = pts[0].chars.iter().find(|c| c.c == 'w').unwrap();
    assert_eq!(w.sty.get("font-size"), Some("80%"));
    assert!(close(w.utfs, 8.0) && close(w.bshft, 0.0));

    // same style, same size → untouched style (no "100%" needed)
    let (d, mut pts, mut ct) = two_texts(1.0, "");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Normal,
        max_spaces: None,
    };
    let sty_before = pts[1].chars[0].sty.clone();
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    let w = pts[0].chars.iter().find(|c| c.c == 'w').unwrap();
    assert!(sciink::text::edit::style_eq(&w.sty, &sty_before));

    // a merge that no longer exists (chunk id gone) is skipped silently
    let (d, mut pts, mut ct) = two_texts(1.0, "");
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, 99),
        wtype: WType::Normal,
        max_spaces: None,
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Hello");
}

#[test]
fn append_chunks_middle_anchor_moves_the_anchor_by_half_the_added_width() {
    let probe = format!(
        r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="50" y="0">Hello</text></svg>"#
    );
    let mut pd = Doc::parse(probe.as_bytes()).unwrap();
    let (ppt, _) = parsed(&mut pd, "a");
    let g = chunk_geom(&ppt, 0, 0);
    let x2 = g.right[4] + ppt.chars[0].spw; // start of "12345", one space later (digits: no pair
    // kerning, so Σ(cwd + dx) is the exact appended width and the anchor correction is exact)
    let svg = format!(
        r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="50" y="0">Hello</text><text id="b" xml:space="preserve" style="{DV};font-size:10px" x="{x2}" y="0">12345</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<NodeId> = ["a", "b"].iter().map(|i| id(&d, i)).collect();
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &els, fonts(), &mut w);
    let mut pts: Vec<ParsedText> = els
        .iter()
        .map(|&e| ParsedText::parse(&mut d, e, &mut ct, &mut w).unwrap())
        .collect();
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
    }
    let before = all_positions(&pts);
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Normal,
        max_spaces: None,
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    let after = all_positions(&pts);
    for (b, a) in before.iter().zip(&after) {
        assert!((b.1 - a.1).abs() < 1e-6, "{b:?} vs {a:?}");
    }
    assert!(
        pts[0].lines[0].chunks[0].x > 50.0,
        "anchor moved right by half the appended width"
    );
}

#[test]
fn append_chunks_host_follows_upstream_for_nested_and_shallow_chunk_ends() {
    // `inner` is the content of <text id="a">; <text id="b">yo</text> sits one space after a's end
    let build = |inner: &str| -> (Doc, Vec<ParsedText>, CharTable) {
        let probe = format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">{inner}</text></svg>"#
        );
        let mut pd = Doc::parse(probe.as_bytes()).unwrap();
        let (ppt, _) = parsed(&mut pd, "a");
        let last = &ppt.chars[ppt.chars.len() - 1];
        let g = chunk_geom(&ppt, last.line, last.chunk);
        let x2 = g.right[g.right.len() - 1] + last.spw;
        let svg = format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">{inner}</text><text id="b" xml:space="preserve" style="{DV};font-size:10px" x="{x2}" y="0">yo</text></svg>"#
        );
        let mut d = Doc::parse(svg.as_bytes()).unwrap();
        let els: Vec<NodeId> = ["a", "b"].iter().map(|i| id(&d, i)).collect();
        let mut w = Warnings::default();
        let mut ct = CharTable::build(&d, &els, fonts(), &mut w);
        let mut pts: Vec<ParsedText> = els
            .iter()
            .map(|&e| ParsedText::parse(&mut d, e, &mut ct, &mut w).unwrap())
            .collect();
        for pt in pts.iter_mut() {
            snapshot_parsed(pt);
        }
        (d, pts, ct)
    };
    // the chunk ends INSIDE a nested tspan while it starts in the text's own run: the new
    // characters are typed into the tspan's tail and sized by the <text> (host = first node),
    // not by the 20px tspan
    let (d, mut pts, mut ct) = build(r#"H<tspan id="s" style="font-size:20px">i</tspan>"#);
    assert_eq!(
        pts[0].lines[0].chunks.len(),
        1,
        "one chunk spanning text and tspan"
    );
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Normal,
        max_spaces: None,
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Hi yo");
    let o = pts[0].chars.iter().find(|c| c.c == 'o').unwrap();
    assert_eq!(
        (o.loc.node, o.loc.tail),
        (id(&d, "s"), true),
        "typed into the tspan's tail"
    );
    assert_eq!(sel(&d, &o.loc), id(&d, "a"));
    assert!(
        close(o.utfs, 10.0),
        "sized by the <text>, not the tspan: {}",
        o.utfs
    );
    assert_ne!(
        o.sty.get("font-size"),
        Some("50%"),
        "no size correction against the tspan's 20px"
    );
    // the chunk ends in the element's OWN tail text after a nested tspan (last character LESS
    // nested than the first): upstream's climb runs off the document, so the characters join the
    // last character's own node — the tspan's tail — never a node outside the element
    let (d, mut pts, mut ct) = build(r#"<tspan id="s">Hi</tspan> ya"#);
    assert_eq!(pts[0].lines[0].chunks.len(), 1);
    let target = (0usize, pts[0].chunk(0, 0).id);
    let inc = Incoming {
        chunk: (1, pts[1].chunk(0, 0).id),
        wtype: WType::Normal,
        max_spaces: None,
    };
    append_chunks(&d, &mut pts, &mut ct, target, &[inc]);
    assert_eq!(pts[0].text(), "Hi ya yo");
    let o = pts[0].chars.iter().find(|c| c.c == 'o').unwrap();
    assert_eq!((o.loc.node, o.loc.tail), (id(&d, "s"), true));
    assert_eq!(sel(&d, &o.loc), id(&d, "a"));
    assert!(close(o.utfs, 10.0));
}
