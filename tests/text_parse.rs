mod support;

use std::path::PathBuf;

use sciink::dom::{Doc, NodeId};
use sciink::style::Style;
use sciink::text::Warnings;
use sciink::text::fonts::{FontSpec, FontSystem};
use sciink::text::table::CharTable;

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}
fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

#[test]
fn char_table_collects_faces_preceders_and_warnings() {
    let c1 = '\u{23A3}';
    let c2 = '\u{10348}';
    let d = doc(&format!(
        r#"<svg {NS}>
      <text id="a" style="font-family:Roboto">AV a<tspan style="font-weight:bold">B</tspan></text>
      <text id="b" style="font-family:Helvetica">x{c1}{c2}</text></svg>"#
    ));
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &[id(&d, "a"), id(&d, "b")], fonts(), &mut w);
    assert_eq!(ct.spec_count(), 3, "Roboto/400, Roboto/700, Helvetica/400");
    let rob = FontSpec::from_style(&Style::parse("font-family:Roboto"));
    let robb = FontSpec::from_style(&Style::parse("font-family:Roboto;font-weight:bold"));
    let helv = FontSpec::from_style(&Style::parse("font-family:Helvetica"));
    let rk = ct.true_face(&rob).unwrap();
    assert_eq!(ct.fonts.face_info(rk).family, "Roboto");
    assert_eq!(ct.fonts.face_info(ct.true_face(&robb).unwrap()).weight, 700);
    let hk = ct.true_face(&helv).unwrap();
    assert_eq!(
        ct.fonts.face_info(hk).family,
        "DejaVu Sans",
        "Helvetica absent → alias/generic → DejaVu Sans"
    );
    // per-char: ⎣ under Helvetica is drawn by DejaVu (same as true face); U+10348 by nobody
    assert_eq!(ct.char_face(&helv, '\u{23A3}'), Some(hk));
    assert_eq!(ct.char_face(&helv, '\u{10348}'), None);
    // props: 'V' preceded by 'A' in Roboto gets that pair's dadv; the unrendered char is zero-width
    let v = ct.prop(Some(rk), 'V');
    assert!(
        v.dadvs.contains_key(&'A') && v.dadvs.contains_key(&' '),
        "{:?}",
        v.dadvs.keys().collect::<Vec<_>>()
    );
    assert!(v.dadvs[&'A'] < 0.0, "Roboto kerns A–V");
    let u = ct.prop(None, '\u{10348}');
    assert_eq!(u.charw, 0.0);
    // 'a' follows a space in "AV a": space is a preceder, 'V' is not
    let a = ct.prop(Some(rk), 'a');
    assert!(a.dadvs.contains_key(&' ') && !a.dadvs.contains_key(&'V'));
    assert_eq!(w.0.len(), 2, "{:?}", w.0);
    assert!(
        w.0.iter()
            .any(|m| m == "font-family \"Helvetica\" not installed; measured with \"DejaVu Sans\"")
    );
    assert!(
        w.0.iter()
            .any(|m| m == "no installed font has the character U+10348")
    );
}

use sciink::text::parse::{SprlType, line_specs, positions};
use sciink::text::style::Anchor;
use sciink::text::tree::TextTree;

#[test]
fn positions_effective_sprl_types_and_inheritance() {
    // dds: 0 text, 1 tspan(role line, x,y) , 2 inner tspan (no pos), 3 tspan (role line but 2 x values) , 4 tspan (no role, x only)
    let d = doc(&format!(
        r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
      <text id="t" style="font-size:10px"><tspan id="a" sodipodi:role="line" x="5" y="10"><tspan id="b">ab</tspan></tspan><tspan id="c" sodipodi:role="line" x="1 2" y="20">cd</tspan><tspan id="e" x="7">e</tspan></text></svg>"#
    ));
    let tree = TextTree::new(&d, id(&d, "t"));
    let p = positions(&d, &tree);
    assert_eq!(p.esprl, [false, true, false, false, false]);
    assert_eq!(
        p.types,
        [
            SprlType::Normal,
            SprlType::TlvlSprl,
            SprlType::Normal,
            SprlType::Normal,
            SprlType::Normal
        ]
    );
    // b cannot inherit from a: an effective sprl blocks the window (its chars will join a's line instead)
    assert_eq!(p.x[2], [None]);
    assert_eq!(p.xsrc[2], 2);
    assert_eq!(p.x[3], [Some(1.0), Some(2.0)]);
    assert_eq!(p.x[0], [Some(0.0)], "root without x gets [0]");
    // e has x but no y; nothing in its window supplies one (only node 0 gets the [0] default)
    assert_eq!(p.y[4], [None]);
    assert_eq!(p.ysrc[4], 4);
    assert_eq!(p.x[4], [Some(7.0)]);
    // an sprl whose only text sits in a positioned descendant is disabled
    let d2 = doc(&format!(
        r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
      <text id="t"><tspan id="a" sodipodi:role="line" x="5" y="10"><tspan id="b" x="9">ab</tspan></tspan></text></svg>"#
    ));
    let tree2 = TextTree::new(&d2, id(&d2, "t"));
    assert_eq!(positions(&d2, &tree2).esprl, [false, false, false]);
}

#[test]
fn line_starts_for_inkscape_multiline_text() {
    // Text_tests-style element: three sodipodi lines; line 2 has no y → sprl inherits y + line height
    let d = doc(&format!(
        r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
      <text id="t" style="font-size:10px;line-height:1.25;text-anchor:middle" x="3" y="4" transform="scale(2)"><tspan id="a" sodipodi:role="line" x="3" y="4">one</tspan><tspan id="b" sodipodi:role="line" x="3" y="16.5">two</tspan><tspan id="c" sodipodi:role="line" x="3" y="29">three</tspan></text></svg>"#
    ));
    let tree = TextTree::new(&d, id(&d, "t"));
    let runs = tree.runs(&d);
    let pos = positions(&d, &tree);
    let lines = line_specs(&d, &tree, &runs, &pos);
    assert_eq!(lines.len(), 3);
    assert!(
        lines
            .iter()
            .all(|l| l.sprl && l.anchor == Anchor::Middle && !l.rtl)
    );
    assert_eq!(lines[0].x, [Some(3.0)]);
    assert_eq!(lines[0].y, [Some(4.0)]);
    // sprl lines ignore their own y: y = previous sprl y + line height (1.25 × 10) in untransformed units
    assert_eq!(lines[1].y, [Some(16.5)]);
    assert_eq!(lines[2].y, [Some(29.0)]);
    assert_eq!(
        lines.iter().map(|l| l.tlvlno).collect::<Vec<_>>(),
        [Some(0), Some(1), Some(2)]
    );
    assert_eq!(
        lines[1].xsrc,
        id(&d, "t"),
        "sprl lines inherit x from the first line's source"
    );
    // a normal (non-sprl) positioned tspan after sprl lines opens a line with continue flags
    let d2 = doc(&format!(
        r#"<svg {NS}><text id="t" x="1" y="2" style="direction:rtl;text-anchor:start">ab<tspan id="s" x="5">cd</tspan><tspan id="u" y="9">ef</tspan></text></svg>"#
    ));
    let tree2 = TextTree::new(&d2, id(&d2, "t"));
    let runs2 = tree2.runs(&d2);
    let pos2 = positions(&d2, &tree2);
    let l2 = line_specs(&d2, &tree2, &runs2, &pos2);
    assert_eq!(l2.len(), 3);
    assert_eq!(
        (l2[0].anchor, l2[0].rtl),
        (Anchor::End, true),
        "rtl swaps start/end"
    );
    // Upstream's double swap (parser.py:542–556): a non-sprl line inherits the previous
    // line's *already swapped* anchor and then swaps it again under its own inherited
    // direction:rtl, so `end` comes back to `start` and alternates down the element.
    assert_eq!(
        (l2[1].anchor, l2[2].anchor),
        (Anchor::Start, Anchor::End),
        "inherited anchor is swapped a second time by the line's own direction:rtl"
    );
    assert_eq!(
        (l2[1].x.clone(), l2[1].continue_x, l2[1].continue_y),
        (vec![Some(5.0)], false, true)
    );
    assert_eq!(l2[1].y, [Some(2.0)], "y continues from the previous line");
    assert_eq!((l2[2].y.clone(), l2[2].continue_x), (vec![Some(9.0)], true));
    assert_eq!(l2[2].x, [Some(5.0)]);
    assert_eq!(l2[0].first_run, 0);
    assert_eq!(l2[0].tlvlno, Some(0));
    assert_eq!(l2[1].tlvlno, Some(0), "s is the first direct child");
}

#[test]
fn an_inactive_sodipodi_role_does_not_block_anchor_inheritance() {
    // `x="1 2"` has two values, so tspan `a`'s role=line is *inactive*: upstream strips the
    // attribute in depathologize (P:396–401) and the line therefore inherits the previous
    // line's anchor instead of using its own `text-anchor:end`.
    let d = doc(&format!(
        r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
      <text id="t" style="text-anchor:start" x="0" y="0"><tspan id="p" x="0" y="0">first</tspan><tspan id="a" sodipodi:role="line" x="1 2" y="20" style="text-anchor:end">second</tspan></text></svg>"#
    ));
    let tree = TextTree::new(&d, id(&d, "t"));
    let runs = tree.runs(&d);
    let pos = positions(&d, &tree);
    assert_eq!(pos.esprl, [false, false, false], "a's role is inactive");
    let lines = line_specs(&d, &tree, &runs, &pos);
    assert_eq!(lines.len(), 2);
    assert_eq!(
        (lines[0].anchor, lines[1].anchor),
        (Anchor::Start, Anchor::Start),
        "the inactive role must not keep the line's own text-anchor:end"
    );
}

#[test]
fn a_tail_that_opens_the_first_line_takes_its_parents_position() {
    // The empty tspan's x="9" positions nothing; "Hello" is the <text>'s tail-of-child text
    // and starts at the <text>'s own x/y (upstream: edi = dds.index(parent) for tails).
    let d = doc(&format!(
        r#"<svg {NS}><text id="t" x="5" y="7"><tspan id="e" x="9"/>Hello</text></svg>"#
    ));
    let tree = TextTree::new(&d, id(&d, "t"));
    let runs = tree.runs(&d);
    let pos = positions(&d, &tree);
    let lines = line_specs(&d, &tree, &runs, &pos);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].x, [Some(5.0)]);
    assert_eq!(lines[0].y, [Some(7.0)]);
    assert!(
        runs[lines[0].first_run].is_tail,
        "the line was opened by the tail run"
    );
}

use sciink::text::parse::{ParsedText, TextLengthAdj};

fn parsed(svg: &str, el: &str) -> (Doc, ParsedText, CharTable) {
    let mut d = doc(svg);
    let mut w = Warnings::default();
    let n = id(&d, el);
    let mut ct = CharTable::build(&d, &[n], fonts(), &mut w);
    let pt = ParsedText::parse(&mut d, n, &mut ct, &mut w).expect("parsed");
    (d, pt, ct)
}

#[test]
fn chars_chunks_and_flags_for_a_simple_element() {
    let (_, pt, _) = parsed(
        &format!(
            r#"<svg {NS}><g transform="scale(2)"><text id="t" style="font-family:'DejaVu Sans';font-size:10px" x="1 2" y="3">AV <tspan id="s" style="font-size:50%;letter-spacing:1px" dx="0.5 0.25">bc</tspan></text></g></svg>"#
        ),
        "t",
    );
    assert_eq!(pt.text(), "AV bc");
    assert_eq!(pt.lines.len(), 1);
    let ln = &pt.lines[0];
    // x="1 2": the second char opens a new chunk; the tspan without x/y joins the current chunk
    assert_eq!(ln.chunks.len(), 2);
    assert_eq!((ln.chunks[0].x, ln.chunks[0].y), (1.0, 3.0));
    assert_eq!((ln.chunks[1].x, ln.chunks[1].y), (2.0, 3.0));
    assert_eq!(ln.chunks[1].chars.len(), 4, "V, space, b, c");
    let a = &pt.chars[0];
    assert_eq!((a.c, a.utfs, a.tfs), ('A', 10.0, 20.0));
    assert!((a.cwd - a.prop.charw * 10.0).abs() < 1e-12);
    assert!((a.caph - 7.29).abs() < 0.05);
    assert_eq!((a.dx, a.dy, a.lsp, a.bshft), (0.0, 0.0, 0.0, 0.0));
    let b = &pt.chars[3];
    assert_eq!((b.c, b.utfs), ('b', 5.0));
    assert_eq!((b.dx, b.lsp), (0.5, 1.0));
    assert_eq!(pt.chars[4].dx, 0.25);
    assert_eq!((b.line, b.chunk, b.windex), (0, 1, 2));
    assert_eq!(b.loc.node, pt.chars[4].loc.node);
    assert_eq!((b.loc.tail, b.loc.idx), (false, 0));
    assert!(pt.any_dx && !pt.any_dy);
    assert!(!pt.is_flow && !pt.is_inkscape && !pt.is_ml_inkscape);
    assert_eq!(pt.transform, kurbo::Affine::scale(2.0));
    assert_eq!(pt.text_length, None);
    // the 'V' after 'A' carries the pair adjustment
    assert!(pt.chars[1].prop.dadvs.contains_key(&'A'));
}

#[test]
fn inkscape_multiline_flags_and_sprl_chunks() {
    let (_, pt, _) = parsed(
        &format!(
            r#"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd">
          <text id="t" style="font-size:10px;line-height:1.25;font-family:'DejaVu Sans';-inkscape-font-specification:'DejaVu Sans'" x="0" y="0"><tspan id="a" sodipodi:role="line" x="0" y="0">ab</tspan><tspan id="b" sodipodi:role="line" x="0" y="12.5">cd</tspan></text></svg>"#
        ),
        "t",
    );
    assert_eq!(pt.lines.len(), 2);
    assert!(pt.is_inkscape && pt.is_ml_inkscape);
    assert_eq!(pt.lines[1].chunks[0].y, 12.5);
    assert_eq!(pt.lines[1].chars, [2, 3]);
    assert_eq!(pt.chars[2].line, 1);
}

#[test]
fn text_length_adjustments_and_flows() {
    let (_, pt, _) = parsed(
        &format!(
            r#"<svg {NS}><text id="t" style="font-family:'DejaVu Sans';font-size:10px" textLength="100" lengthAdjust="spacingAndGlyphs">ab</text></svg>"#
        ),
        "t",
    );
    let natural: f64 = pt.chars.iter().map(|c| c.prop.charw * 10.0).sum();
    let Some(TextLengthAdj::SpacingAndGlyphs(s)) = pt.text_length else {
        panic!("{:?}", pt.text_length)
    };
    assert!((s - 100.0 / natural).abs() < 1e-9);
    assert!((pt.chars.iter().map(|c| c.cwd).sum::<f64>() - 100.0).abs() < 1e-9);
    let (_, pt2, _) = parsed(
        &format!(
            r#"<svg {NS}><text id="t" style="font-family:'DejaVu Sans';font-size:10px" textLength="100">abc</text></svg>"#
        ),
        "t",
    );
    let natural2: f64 = pt2.chars.iter().map(|c| c.prop.charw * 10.0).sum();
    let Some(TextLengthAdj::Spacing(extra)) = pt2.text_length else {
        panic!()
    };
    assert!(
        (extra - (100.0 - natural2) / 2.0).abs() < 1e-9,
        "spread over nchars − nchunks = 2 gaps"
    );
    assert!(pt2.chars.iter().all(|c| (c.lsp - extra).abs() < 1e-9));
    // flows are recognised but not parsed in v1
    let (_, fl, _) = parsed(
        &format!(
            r#"<svg {NS}><text id="t" style="font-size:3px;inline-size:24;font-family:'DejaVu Sans'"><tspan x="0" y="1">flowed</tspan></text></svg>"#
        ),
        "t",
    );
    assert!(fl.is_flow && fl.lines.is_empty() && fl.chars.is_empty());
    let mut d = doc(&format!(r#"<svg {NS}><text id="e"/></svg>"#));
    let mut w = Warnings::default();
    let n = id(&d, "e");
    let mut ct = CharTable::build(&d, &[n], fonts(), &mut w);
    assert!(
        ParsedText::parse(&mut d, n, &mut ct, &mut w).is_none(),
        "no text → no model"
    );
}
