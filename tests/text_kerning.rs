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

use sciink::text::edit::{WType, make_next_chain};
use sciink::text::kerning::{
    MergeType, isnumeric, merge_wtypes, remove_manual_kerning, trailing_leading, twospaces, wstrip,
};
use sciink::text::layout::{chunk_char_pts, chunk_geom, snapshot_parsed, transform_pts};
use sciink::text::write::ClipUnion;

fn all_positions(pts: &[ParsedText]) -> Vec<(char, f64, f64)> {
    let mut v = Vec::new();
    for pt in pts {
        for (li, ln) in pt.lines.iter().enumerate() {
            for ci in 0..ln.chunks.len() {
                let p = chunk_char_pts(pt, li, ci);
                for (wi, &c) in ln.chunks[ci].chars.iter().enumerate() {
                    if pt.chars[c].c != ' ' {
                        let q = transform_pts(pt.transform, p[wi])[0];
                        v.push((pt.chars[c].c, q.x, q.y));
                    }
                }
            }
        }
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}
fn assert_same(before: &[(char, f64, f64)], after: &[(char, f64, f64)], tol: f64) {
    assert_eq!(before.len(), after.len(), "\n{before:?}\n{after:?}");
    for (b, a) in before.iter().zip(after) {
        assert!(
            b.0 == a.0 && (b.1 - a.1).abs() < tol && (b.2 - a.2).abs() < tol,
            "{b:?} vs {a:?}"
        );
    }
}
/// Parse every <text> of `svg` into an arena (snapshot taken, next chain built).
fn arena(svg: &str) -> (Doc, Vec<ParsedText>, CharTable) {
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<NodeId> = d
        .descendants(d.svg())
        .filter(|&n| d.is_element(n) && d.tag(n) == "text")
        .collect();
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &els, fonts(), &mut w);
    let mut pts: Vec<ParsedText> = els
        .iter()
        .filter_map(|&e| ParsedText::parse(&mut d, e, &mut ct, &mut w))
        .collect();
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
        make_next_chain(&d, pt);
    }
    (d, pts, ct)
}
/// Left edges of the characters of `text` laid out as one chunk at x=0 (DejaVu Sans 10px).
fn lefts(text: &str) -> Vec<f64> {
    let mut d = Doc::parse(format!(r#"<svg {NS}><text id="p" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">{text}</text></svg>"#).as_bytes()).unwrap();
    let (pt, _) = parsed(&mut d, "p");
    chunk_geom(&pt, 0, 0).left
}
fn fmt_list(v: &[f64]) -> String {
    v.iter()
        .map(|x| format!("{x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn text_helpers_follow_upstream() {
    assert!(isnumeric(" 1,000.5 ", false) && isnumeric("−3e2", false) && isnumeric("-", true));
    assert!(!isnumeric("-", false) && !isnumeric("1a", false) && !isnumeric("", false));
    assert_eq!(wstrip(" a\tb\n"), "ab");
    assert!(
        twospaces("a  ", "b")
            && twospaces("a ", " b")
            && twospaces("a", "  b")
            && !twospaces("a ", "b")
    );
    assert_eq!(trailing_leading("ab  ", " cd"), (2, 1));
}

#[test]
fn merge_type_state_machine_matches_rk566_611() {
    let r = |ts: &[MergeType]| {
        merge_wtypes(&ts.iter().map(|&t| ((0usize, 0u32), t)).collect::<Vec<_>>())
    };
    use MergeType::*;
    assert_eq!(r(&[Same, Same]), Some(vec![WType::Normal; 3]));
    assert_eq!(
        r(&[Super, Same, SuperReturn, Same]),
        Some(vec![
            WType::Normal,
            WType::Super,
            WType::Super,
            WType::Normal,
            WType::Normal
        ])
    );
    assert_eq!(
        r(&[Sub, SubReturn]),
        Some(vec![WType::Normal, WType::Sub, WType::Normal])
    );
    assert_eq!(r(&[SuperReturn]), None, "return without a super");
    assert_eq!(r(&[Super, Sub]), None, "sub inside a super");
    assert_eq!(r(&[Sub, SuperReturn]), None);
    assert_eq!(r(&[]), Some(vec![WType::Normal]));
}

#[test]
fn manual_kerning_removal_rejoins_pdf_style_x_arrays() {
    // PDF import: every glyph positioned by its own x, laid out with our own metrics
    let xs = lefts("Hello");
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">Hello</text></svg>"#,
        fmt_list(&xs)
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 5);
    let before = all_positions(&pts);
    let mut clips: Vec<ClipUnion> = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts.len(), 1, "everything merged, nothing to split off");
    assert_eq!(pts[0].lines[0].chunks.len(), 1);
    assert_eq!(pts[0].text(), "Hello");
    assert_same(&before, &all_positions(&pts), 1e-6);
    assert!(clips.is_empty(), "one element: no clip union");

    // two words one space apart merge with a real space; a far chunk is split into its own element.
    // Per-character x list: "Hello" glyph-by-glyph, then "world" one space later, then "far" six later.
    let hello = lefts("Hello");
    let spw = {
        let mut d = Doc::parse(
            format!(
                r#"<svg {NS}><text id="p" style="{DV};font-size:10px" x="0" y="0">a</text></svg>"#
            )
            .as_bytes(),
        )
        .unwrap();
        parsed(&mut d, "p").0.chars[0].spw
    };
    let hello_right = lefts("Hello ")[5];
    let world = lefts("world");
    let far = lefts("far");
    let mut xs: Vec<f64> = hello.clone();
    xs.extend(world.iter().map(|x| hello_right + spw + x));
    let world_right = hello_right + spw + lefts("world ")[5];
    xs.extend(far.iter().map(|x| world_right + 6.0 * spw + x));
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">Helloworldfar</text></svg>"#,
        fmt_list(&xs)
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 13);
    let before = all_positions(&pts);
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts.len(), 2, "'far' is too far: its own element");
    assert_eq!(pts[0].text(), "Hello world");
    assert_eq!(pts[1].text(), "far");
    assert_eq!(pts[1].origin, sciink::text::parse::Origin::SplitFrom);
    assert_same(&before, &all_positions(&pts), 1e-6);

    // numbers: "−" right before "0.5" merges with dx = 0 (RK:341–342)
    let minus = lefts("−");
    let minus_right = lefts("−0")[1];
    let mut xs = minus.clone();
    xs.extend(lefts("0.5").iter().map(|x| minus_right + x));
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">−0.5</text></svg>"#,
        fmt_list(&xs)
    ));
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts.len(), 1);
    assert_eq!(pts[0].text(), "−0.5");
}

#[test]
fn manual_kerning_removal_records_clip_unions_only_across_elements() {
    // intra-element merging never touches clips; the record is exercised by external merges (Task 7)
    let xs = lefts("ab");
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="9" height="9"/></clipPath></defs><text id="t" clip-path="url(#c)" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">ab</text></svg>"#,
        fmt_list(&xs)
    ));
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts[0].text(), "ab");
    assert!(clips.is_empty());
}
