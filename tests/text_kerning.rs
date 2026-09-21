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

#[test]
fn manual_kerning_numeric_pairs_do_not_bridge_a_space_sized_gap() {
    // numeric chunks use dx = 0 (RK:341-342): a full space-width gap must NOT bridge them, even
    // though the identical gap between ordinary letters does (dx = spw, window up to 1.99*spw).
    let spw = lefts(" a")[1];
    let right1 = lefts("1 ")[1];
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0 {}" y="0">12</text></svg>"#,
        right1 + 1.5 * spw
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 2);
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(
        pts.len(),
        2,
        "a numeric pair across a space-sized gap must not merge"
    );
    assert_eq!(pts[0].text(), "1");
    assert_eq!(pts[1].text(), "2");
    assert_eq!(pts[1].origin, sciink::text::parse::Origin::SplitFrom);

    // control: the same gap between ordinary letters does merge.
    let right_a = lefts("a ")[1];
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0 {}" y="0">ab</text></svg>"#,
        right_a + 1.5 * spw
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 2);
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(pts.len(), 1, "the same gap between letters must merge");
    assert_eq!(pts[0].text().replace(' ', ""), "ab");
}

#[test]
fn manual_kerning_retries_a_weirdly_kerned_space_against_the_previous_chunk() {
    // a lone " " chunk whose primary check fails re-tests against its own predecessor (RK:349-355).
    let spw = lefts(" a")[1];
    let right_m = lefts("m ")[1];
    let x1 = right_m - 1.4 * spw;
    let x2 = right_m + spw;
    let (d, mut pts, mut ct) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0 {x1} {x2}" y="0">m b</text></svg>"#
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 3);
    let mut clips = Vec::new();
    remove_manual_kerning(&d, &mut pts, &mut ct, &mut clips);
    assert_eq!(
        pts.len(),
        1,
        "the retry must let all three chunks merge into one element"
    );
    let text = pts[0].text();
    assert_eq!(text.replace(' ', ""), "mb");
    assert!(
        text.contains(' '),
        "merged text should retain a space: {text:?}"
    );
}

use sciink::text::kerning::external_merges;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// "Hello" at (0,0) and a second element `text2` whose left edge is `gap` space-widths after it,
/// with `dy` baseline offset and extra style/attributes.
fn pair(
    text2: &str,
    gap: f64,
    dy: f64,
    style2: &str,
    attrs2: &str,
) -> (Doc, Vec<ParsedText>, CharTable) {
    let right = lefts("Hello ")[5];
    let spw = lefts(" a")[1];
    let x2 = right + gap * spw;
    arena(&format!(
        r#"<svg {NS}><defs><clipPath id="c1"><rect width="50" height="50"/></clipPath><clipPath id="c2"><rect x="10" width="50" height="50"/></clipPath></defs><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="b" xml:space="preserve" style="{DV};font-size:10px;{style2}" x="{x2}" y="{dy}" {attrs2}>{text2}</text></svg>"#
    ))
}

#[test]
fn external_merges_join_adjacent_elements_with_a_space() {
    let (d, mut pts, mut ct) = pair("world", 1.0, 0.0, "", "");
    let before = all_positions(&pts);
    let mut clips = Vec::new();
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello world");
    assert!(pts[1].chars.is_empty());
    assert_same(&before, &all_positions(&pts), 1e-6);
    assert!(clips.is_empty(), "neither element is clipped");

    // too far (3 spaces > 1 + 0.6): untouched
    let (d, mut pts, mut ct) = pair("world", 3.0, 0.0, "", "");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(
        (pts[0].text().as_str(), pts[1].text().as_str()),
        ("Hello", "world")
    );

    // mergenearby off: same-line merges are disabled (sub/super still allowed)
    let (d, mut pts, mut ct) = pair("world", 1.0, 0.0, "", "");
    external_merges(&d, &mut pts, &mut ct, false, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello");

    // different rotation: never merged
    let (d, mut pts, mut ct) = pair("world", 1.0, 0.0, "", r#"transform="rotate(1)""#);
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello");

    // two numbers one space apart stay apart (tick labels, RK:458–462) …
    let (d, mut pts, mut ct) = {
        let right = lefts("0.5 ")[3];
        let spw = lefts(" a")[1];
        arena(&format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">0.5</text><text id="b" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">1.0</text></svg>"#,
            right + spw
        ))
    };
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "0.5");
    // … but a minus sign touching a number joins it
    let (d, mut pts, mut ct) = {
        let right = lefts("−0")[1];
        arena(&format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">−</text><text id="b" xml:space="preserve" style="{DV};font-size:10px" x="{right}" y="0">0.5</text></svg>"#
        ))
    };
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "−0.5");
}

#[test]
fn external_merges_detect_superscripts_and_union_clips() {
    // "2" at 6px, raised by 4 (= 40 % of 10px), touching the end of "Hello": a superscript
    let (d, mut pts, mut ct) = pair("2", 0.0, -4.0, "font-size:6px", r#"clip-path="url(#c2)""#);
    let mut clips = Vec::new();
    // give the first element a clip too, so the union is recorded
    let a = id(&d, "a");
    let mut d = d;
    d.set_attr(a, "clip-path", "url(#c1)");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello2");
    let two = pts[0].chars.iter().find(|c| c.c == '2').unwrap();
    assert!(close(two.utfs, 6.5) && close(two.bshft, 4.0));
    assert_eq!(two.sty.get("baseline-shift"), Some("super"));
    assert_eq!(
        clips,
        vec![ClipUnion {
            target: id(&d, "a"),
            others: vec![id(&d, "b")]
        }]
    );

    // mergesupersub off: no superscript merge
    let (d, mut pts, mut ct) = pair("2", 0.0, -4.0, "font-size:6px", "");
    external_merges(&d, &mut pts, &mut ct, true, false, &mut clips);
    assert_eq!(pts[0].text(), "Hello");

    // a bold superscript candidate does not merge (weight mismatch, RK:446)
    let (d, mut pts, mut ct) = pair("2", 0.0, -4.0, "font-size:6px;font-weight:bold", "");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello");

    // subscript: "2" lowered so its cap top sits below 1/3 of the line
    let (d, mut pts, mut ct) = pair("2", 0.0, 2.0, "font-size:6px", "");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello2");
    let two = pts[0].chars.iter().find(|c| c.c == '2').unwrap();
    assert!(close(two.bshft, -2.0));
    assert_eq!(two.sty.get("baseline-shift"), Some("sub"));

    // "(a)" never takes a sub/superscript (subfigure labels, RK:449)
    let (d, mut pts, mut ct) = {
        arena(&format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">(a)</text><text id="b" xml:space="preserve" style="{DV};font-size:6px" x="{}" y="-4">2</text></svg>"#,
            lefts("(a) ")[3]
        ))
    };
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "(a)");
}

#[test]
fn clip_union_is_recorded_when_only_one_participant_is_clipped() {
    let (d, mut pts, mut ct) = pair("world", 1.0, 0.0, "", r#"clip-path="url(#c2)""#);
    let mut clips = Vec::new();
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(pts[0].text(), "Hello world");
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].target, id(&d, "a"));
    assert_eq!(clips[0].others, vec![id(&d, "b")]);
}
