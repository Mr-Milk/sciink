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
