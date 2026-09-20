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
