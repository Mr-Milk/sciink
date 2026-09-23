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
fn external_merges_reject_a_double_space_and_a_blank_participant() {
    let mut clips = Vec::new();
    // blank: the second chunk is a single space. Same x as a visible "x", so only the
    // `wstrip(...).is_empty()` gate (RK:412) can be what rejects it.
    let (d, mut pts, mut ct) = pair("x", 1.0, 0.0, "", "");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(
        pts[0].text(),
        "Hello x",
        "control: the geometry is in range"
    );
    let (d, mut pts, mut ct) = pair(" ", 1.0, 0.0, "", "");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(
        pts[0].text(),
        "Hello",
        "a whitespace-only chunk never merges"
    );

    // double space: " world" and "  world" start at the same x, so the only difference is
    // `twospaces` (RK:412) — one joins, the other does not
    let placed = |t2: &str| {
        let right = lefts("Hello ")[5];
        let spw = lefts(" a")[1];
        arena(&format!(
            r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="b" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">{t2}</text></svg>"#,
            right - 0.5 * spw
        ))
    };
    let (d, mut pts, mut ct) = placed(" world");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(
        pts[0].text(),
        "Hello world",
        "control: one leading space joins"
    );
    let (d, mut pts, mut ct) = placed("  world");
    external_merges(&d, &mut pts, &mut ct, true, true, &mut clips);
    assert_eq!(
        pts[0].text(),
        "Hello",
        "merging would put two spaces in a row"
    );
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

use sciink::text::kerning::{split_distant_chunks, split_distant_intrachunk, split_lines};

#[test]
fn distant_chunks_and_lines_become_their_own_elements() {
    let spw = lefts(" a")[1];
    // a short x list positions only the first N characters; the rest continue naturally as part
    // of the last positioned character's chunk — "a" | "b" | "cdef" on one line, three chunks.
    let ra = lefts("a ")[1]; // right edge of 'a'
    let rb = lefts("b ")[1]; // advance of 'b'

    // sub-case 1 (one split): gap a→b is one space (no split); gap b→cdef is four (split), so the
    // multi-character chunk "cdef" is what gets split off as a single element.
    let x1 = ra + spw;
    let x2 = x1 + rb + 4.0 * spw;
    let xs = fmt_list(&[0.0, x1, x2]);
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{xs}" y="0">abcdef</text></svg>"#
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 3);
    let before = all_positions(&pts);
    split_distant_chunks(&mut pts);
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[0].text(), "ab");
    assert_eq!(pts[1].text(), "cdef");
    assert_eq!(pts[1].split_src, Some(0));
    assert_same(&before, &all_positions(&pts), 1e-6);

    // sub-case 2: every character has its own x, so 'e' and 'f' are separate chunks and each
    // chunk-run of the split range becomes its own element (upstream P:1158–1200, chrs_to_textel);
    // written in natural order — **Deviation:** upstream's addnext-on-source emits the runs of one
    // range reversed ('f' before 'e').
    let ab = lefts("ab");
    let ab_r = lefts("ab ")[2];
    let x_cd = ab_r + spw;
    let cd = lefts("cd");
    let cd_r = x_cd + lefts("cd ")[2];
    let x_ef = cd_r + 4.0 * spw;
    let ef = lefts("ef");
    let xs = vec![
        ab[0],
        ab[1],
        x_cd + cd[0],
        x_cd + cd[1],
        x_ef + ef[0],
        x_ef + ef[1],
    ];
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{}" y="0">abcdef</text></svg>"#,
        fmt_list(&xs)
    ));
    let before = all_positions(&pts);
    split_distant_chunks(&mut pts);
    assert_eq!(pts.len(), 3);
    let texts: Vec<String> = pts.iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["abcd", "e", "f"]);
    assert_same(&before, &all_positions(&pts), 1e-6);

    // sub-case 3 (two ranges in one call pin natural order): both gaps now four spaces, so a, b
    // and cdef each split off — texts must come out in natural (document) order, not reversed.
    let x1 = ra + 4.0 * spw;
    let x2 = x1 + rb + 4.0 * spw;
    let xs = fmt_list(&[0.0, x1, x2]);
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="{xs}" y="0">abcdef</text></svg>"#
    ));
    let before = all_positions(&pts);
    split_distant_chunks(&mut pts);
    let texts: Vec<String> = pts.iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["a", "b", "cdef"]);
    assert_eq!(pts[1].split_src, Some(0));
    assert_eq!(pts[2].split_src, Some(0));
    assert_same(&before, &all_positions(&pts), 1e-6);

    // lines: every line after the first becomes an element (skipping multi-line Inkscape text)
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">one<tspan x="0" y="12">two</tspan><tspan x="0" y="24">three</tspan></text></svg>"#
    ));
    let before = all_positions(&pts);
    split_lines(&mut pts);
    let texts: Vec<String> = pts.iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["one", "two", "three"]);
    assert_same(&before, &all_positions(&pts), 1e-6);

    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;-inkscape-font-specification:'DejaVu Sans'" x="0" y="0"><tspan sodipodi:role="line" x="0" y="0" style="-inkscape-font-specification:'DejaVu Sans'">one</tspan><tspan sodipodi:role="line" x="0" y="12" style="-inkscape-font-specification:'DejaVu Sans'">two</tspan></text></svg>"#
    ));
    assert!(pts[0].is_ml_inkscape);
    split_lines(&mut pts);
    assert_eq!(
        pts.len(),
        1,
        "Inkscape-generated multi-line text is left alone"
    );
}

#[test]
fn distant_characters_inside_a_chunk_split_including_tick_numbers() {
    // "ab" then a 2-space hole then "cd" inside ONE chunk, made with dx on 'c'
    let spw = lefts(" a")[1];
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0" dx="0 0 {}">abcd</text></svg>"#,
        2.0 * spw
    ));
    let before = all_positions(&pts);
    split_distant_intrachunk(&mut pts);
    assert_eq!(pts.len(), 2);
    assert_eq!(
        (pts[0].text().as_str(), pts[1].text().as_str()),
        ("ab", "cd")
    );
    assert_same(&before, &all_positions(&pts), 1e-6);

    // numbers separated by a single space always split (tick labels, RK:280–295)
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">0.5 1.0</text></svg>"#
    ));
    split_distant_intrachunk(&mut pts);
    let texts: Vec<String> = pts.iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["0.5", " 1.0"]);

    // words separated by one space do not
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">ab cd</text></svg>"#
    ));
    split_distant_intrachunk(&mut pts);
    assert_eq!(pts.len(), 1);

    // … and neither do two numbers whose separating space lives in ANOTHER XML node: the
    // same-node clause (RK:288) is what stops "0.5" + a styled " 1.0" from being torn apart
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">0.5<tspan style="fill:red"> 1.0</tspan></text></svg>"#
    ));
    assert_eq!(pts[0].lines[0].chunks.len(), 1, "one chunk, two nodes");
    split_distant_intrachunk(&mut pts);
    assert_eq!(
        pts.iter().map(|p| p.text()).collect::<Vec<_>>(),
        ["0.5 1.0"]
    );
}

#[test]
fn split_distant_chunks_handles_several_lines_in_one_call() {
    // Two lines, each "x" | 4-space hole | "yzw": the line loop splits line 0 and then line 1,
    // and line indices only stay valid because no line is ever emptied by a split here.
    let spw = lefts(" a")[1];
    let x1 = lefts("a ")[1] + 4.0 * spw;
    let x3 = lefts("e ")[1] + 4.0 * spw;
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0 {x1}" y="0">abcd<tspan x="0 {x3}" y="20">efgh</tspan></text></svg>"#
    ));
    assert_eq!(pts[0].lines.len(), 2);
    assert_eq!(
        (pts[0].lines[0].chunks.len(), pts[0].lines[1].chunks.len()),
        (2, 2)
    );
    let before = all_positions(&pts);
    split_distant_chunks(&mut pts);
    assert_eq!(
        pts.iter().map(|p| p.text()).collect::<Vec<_>>(),
        ["ae", "bcd", "fgh"],
        "both lines split, in line order"
    );
    assert_eq!(pts[0].lines.len(), 2, "the source keeps both lines");
    assert_eq!((pts[1].split_src, pts[2].split_src), (Some(0), Some(0)));
    assert_same(&before, &all_positions(&pts), 1e-6);
}

use sciink::text::kerning::{
    change_justification, fix_merge_positions, remove_trailing_leading_spaces,
};
use sciink::text::style::Anchor;

#[test]
fn justification_changes_anchor_without_moving_glyphs() {
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0 40" y="0">ab cd </text></svg>"#
    ));
    let before = all_positions(&pts);
    change_justification(&mut pts, Some(Anchor::Middle));
    assert_eq!(pts[0].lines[0].spec.anchor, Anchor::Middle);
    assert_eq!(pts[0].text_anchor_override, Some(Anchor::Middle));
    assert_same(&before, &all_positions(&pts), 1e-6);
    // x="0 40" positions the 1st and 2nd characters, so the chunks are "a" and "b cd ": the second
    // chunk's new anchor is the centre of "b cd" — its trailing unrendered space does not count
    let g = chunk_geom(&pts[0], 0, 1);
    assert_eq!(pts[0].chunk_text(0, 1), "b cd ");
    assert!(
        close(pts[0].lines[0].chunks[1].x, 0.5 * (g.left[0] + g.right[3])),
        "{}",
        pts[0].lines[0].chunks[1].x
    );
    // None → untouched
    let (_d, mut pts2, _) = arena(&format!(
        r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0" y="0">ab</text></svg>"#
    ));
    change_justification(&mut pts2, None);
    assert_eq!(pts2[0].text_anchor_override, None);
    assert_eq!(pts2[0].lines[0].spec.anchor, Anchor::Start);
}

#[test]
fn stray_spaces_go_and_positions_stay() {
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:end" x="80" y="0">  ab  <tspan x="0" y="20">   </tspan></text></svg>"#
    ));
    let before = all_positions(&pts);
    assert!(remove_trailing_leading_spaces(&mut pts));
    assert_eq!(pts[0].text(), "ab");
    assert_eq!(pts[0].lines.len(), 1, "an all-space line disappears");
    assert_same(&before, &all_positions(&pts), 1e-6);
    assert!(
        !remove_trailing_leading_spaces(&mut pts),
        "nothing left to remove"
    );
}

#[test]
fn fix_merge_positions_restores_the_parsed_anchor() {
    let (_d, mut pts, _) = arena(&format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="50" y="0">abc</text></svg>"#
    ));
    let before = all_positions(&pts);
    pts[0].lines[0].chunks[0].x += 3.0; // simulate drift left behind by a merge
    fix_merge_positions(&mut pts);
    assert_same(&before, &all_positions(&pts), 1e-9);
    assert!(close(pts[0].lines[0].chunks[0].x, 50.0));
}

use sciink::text::kerning::{KerningOptions, remove_kerning};

fn all_opts() -> KerningOptions {
    KerningOptions::from_inx(true, true, true, true, 1)
}
fn serialize(d: &Doc) -> String {
    let mut v = Vec::new();
    d.write(&mut v);
    String::from_utf8(v).unwrap()
}
/// `remove_kerning` over every `<text>` of `svg`, returning (serialized document, result, warnings).
fn run_kerning(svg: &str, pick: &[&str]) -> (String, Vec<NodeId>, Vec<NodeId>, Vec<String>) {
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<NodeId> = pick.iter().map(|i| id(&d, i)).collect();
    let mut w = Warnings::default();
    let out = remove_kerning(&mut d, &els, &all_opts(), fonts(), &mut w);
    let attached: Vec<NodeId> = out
        .iter()
        .copied()
        .filter(|&n| n == d.root() || d.ancestors(n).any(|a| a == d.root()))
        .collect();
    (serialize(&d), out, attached, w.0)
}

#[test]
fn text_on_a_path_is_measured_but_never_edited() {
    let svg = format!(
        r##"<svg {NS} xmlns:xlink="http://www.w3.org/1999/xlink"><defs><path id="p" d="M 0,50 H 200"/></defs><text id="t" style="{DV};font-size:10px"><textPath xlink:href="#p">Hello curve</textPath></text></svg>"##
    );
    let d = Doc::parse(svg.as_bytes()).unwrap();
    let before = serialize(&d);
    let (after, out, attached, warns) = run_kerning(&svg, &["t"]);
    assert_eq!(after, before, "text on a path comes back byte-identical");
    assert_eq!(out.len(), 1, "the element is still returned");
    assert_eq!(attached, out, "and is still in the tree");
    assert_eq!(out[0], id(&d, "t"));
    assert!(
        warns.iter().any(|w| w == "t: text on a path is not edited"),
        "{warns:?}"
    );
}

/// Every character of every `<text>` in the document, sorted — nothing may be lost.
fn all_glyphs(svg: &str) -> Vec<char> {
    let d = roxmltree::Document::parse(svg).unwrap();
    let mut v: Vec<char> = d
        .descendants()
        .filter(|n| n.is_text() && n.ancestors().any(|a| a.has_tag_name("text")))
        .filter_map(|n| n.text())
        .flat_map(|t| t.chars().collect::<Vec<_>>())
        .filter(|c| !c.is_whitespace())
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn duplicate_and_nested_selections_neither_panic_nor_leave_orphans() {
    // (a) the same element twice: `remove_kerning` is a public API (Plan 6's Flattener calls it
    //     directly), and two models sharing one `el` made the second write insert after an
    //     anchor the first had already detached — `Doc::insert_after`'s `expect` fires.
    let svg = format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">ab</text></svg>"#
    );
    let (after, out, attached, _) = run_kerning(&svg, &["t", "t"]);
    assert_eq!(out.len(), 1, "one element out, not two: {out:?}");
    assert_eq!(attached, out, "every returned id is still in the tree");
    assert_eq!(after.matches("<text").count(), 1, "{after}");
    assert_eq!(all_glyphs(&after), ['a', 'b']);

    // (b) nested <text>: the outer's parse already absorbs the inner's characters, so writing
    //     both put the new inner element inside the detached old outer subtree — it vanished
    //     from the document while its id stayed in the result.
    let svg = format!(
        r#"<svg {NS}><text id="a" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">outer<text id="b" x="0" y="20">inner</text></text></svg>"#
    );
    let (after, out, attached, warns) = run_kerning(&svg, &["a", "b"]);
    assert_eq!(
        attached, out,
        "every returned id is still in the tree:\n{after}"
    );
    assert!(!out.is_empty(), "the outer element survives");
    assert!(
        warns
            .iter()
            .any(|w| w == "b: nested in another selected text element; not edited"),
        "{warns:?}"
    );
    assert_eq!(
        all_glyphs(&after),
        all_glyphs(&svg),
        "no glyph lost:\n{after}"
    );
}

#[test]
fn a_thousand_sibling_texts_are_all_edited_and_a_nested_one_is_skipped() {
    use sciink::text::kerning::{KerningOptions, remove_kerning};
    let mut body = String::new();
    for i in 0..1000 {
        body.push_str(&format!(
            r#"<text id="t{i}" x="{}" y="{}" style="{DV};font-size:4px">w{i} x</text>"#,
            (i % 40) * 20,
            (i / 40) * 8 + 5
        ));
    }
    body.push_str(&format!(
        r#"<text id="outer" x="0" y="300" style="{DV};font-size:4px">o<text id="inner" x="10" y="300">i</text></text>"#
    ));
    let svg = format!(r#"<svg {NS} width="900" height="400">{body}</svg>"#);
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let mut els: Vec<_> = (0..1000)
        .map(|i| d.by_id(&format!("t{i}")).unwrap())
        .collect();
    let outer0 = d.by_id("outer").unwrap();
    els.push(outer0);
    els.push(d.by_id("inner").unwrap());
    let mut w = Warnings::default();
    let t0 = std::time::Instant::now();
    let out = remove_kerning(
        &mut d,
        &els,
        &KerningOptions::from_inx(true, true, true, true, 1),
        fonts(),
        &mut w,
    );
    assert!(t0.elapsed().as_secs() < 20, "quadratic nested-text check");

    let outer_after = d.by_id("outer").unwrap();
    let inner0 = els[1001];

    assert_ne!(
        outer_after, outer0,
        "outer IS rewritten: new element replaces old"
    );

    // Verify the mechanism: what are the 1002 returned elements?
    // The original outer was rewritten (new outer exists with different NodeId).
    // The original inner was not rewritten (skipped as nested).
    // Parsing the 1000 siblings and outer into tels creates split-offs.

    assert!(
        out.contains(&outer_after),
        "new outer (rewritten) is in the returned list"
    );
    assert!(
        !out.contains(&outer0),
        "old outer is not returned (was rewritten and detached)"
    );
    assert!(
        !out.contains(&inner0),
        "old inner is not returned (nested, skipped, and parent detached)"
    );

    // Count original input elements in output
    let returned_originals = els.iter().take(1002).filter(|e| out.contains(e)).count();

    // The 1002 elements are:
    // - 1 new outer (the rewritten version of the original outer)
    // - ~1001 split-offs created during parsing/writing of the 1000 siblings
    // The original outer is not returned (rewritten to a new NodeId).
    // The original inner is not returned (nested, skipped, not attached).
    assert_eq!(
        returned_originals, 0,
        "none of the original input NodeIds are returned (outer rewritten, inner skipped)"
    );
    assert_eq!(
        out.len(),
        1002,
        "new outer (1) + split-offs from parsing siblings and outer (~1001) = 1002"
    );

    let nested: Vec<&String> =
        w.0.iter()
            .filter(|m| m.contains("nested in another selected text"))
            .collect();
    assert_eq!(nested.len(), 1, "{:?}", w.0);
    assert!(nested[0].contains("inner"), "{}", nested[0]);
}
