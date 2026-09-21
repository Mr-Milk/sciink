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

use std::collections::HashMap;

use sciink::style::Style;
use sciink::text::edit::{remove_chars, split_off};
use sciink::text::layout::{chunk_char_pts, snapshot_parsed, transform_pts};
use sciink::text::write::{ClipUnion, apply_clip_unions, specified_diff, write_clean_text};

fn all_texts(d: &mut Doc) -> Vec<NodeId> {
    d.descendants(d.svg())
        .filter(|&n| d.is_element(n) && d.tag(n) == "text")
        .collect()
}
fn arena(d: &mut Doc) -> (Vec<ParsedText>, CharTable) {
    let els = all_texts(d);
    let mut w = Warnings::default();
    let mut ct = CharTable::build(d, &els, fonts(), &mut w);
    let mut pts: Vec<ParsedText> = els
        .iter()
        .filter_map(|&e| ParsedText::parse(d, e, &mut ct, &mut w))
        .collect();
    for pt in pts.iter_mut() {
        snapshot_parsed(pt);
    }
    (pts, ct)
}
fn positions(pts: &[ParsedText]) -> Vec<(char, f64, f64)> {
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
fn write_all(d: &mut Doc, pts: &[ParsedText], ct: &CharTable) -> Vec<Option<NodeId>> {
    let mut rep = HashMap::new();
    (0..pts.len())
        .map(|i| write_clean_text(d, pts, i, ct, &mut rep))
        .collect()
}
fn out(d: &Doc) -> String {
    let mut v = Vec::new();
    d.write(&mut v);
    String::from_utf8(v).unwrap()
}

#[test]
fn specified_diff_follows_cache_py() {
    let desired = Style::parse("fill:red;font-size:10px;font-weight:bold");
    let inherited = Style::parse("fill:red;font-size:12px;stroke:blue");
    let diff = specified_diff(&desired, &inherited);
    assert_eq!(diff.get("fill"), None, "already inherited");
    assert_eq!(diff.get("font-size"), Some("10px"));
    assert_eq!(diff.get("font-weight"), Some("bold"));
    assert_eq!(
        diff.get("stroke"),
        Some("none"),
        "inherited but unwanted → initial value"
    );
}

#[test]
fn writer_regenerates_a_clean_element_that_reparses_to_the_same_positions() {
    let svg = format!(
        r#"<svg {NS}><g transform="translate(5,5)"><text id="t" class="k" data-x="1" style="{DV};font-size:10px;baseline-shift:0;direction:ltr;fill:red" x="0" y="0">ab<tspan style="font-weight:bold">c</tspan> <tspan x="0" y="20" style="font-size:6px;baseline-shift:super">d</tspan>ef</text></g></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (pts, ct) = arena(&mut d);
    let before = positions(&pts);
    let new = write_all(&mut d, &pts, &ct);
    assert_eq!(new.len(), 1);
    let te = new[0].expect("rewritten");
    // structure
    assert_eq!(d.attr(te, "id"), Some("t"));
    assert_eq!(d.attr(te, "xml:space"), Some("preserve"));
    assert_eq!(d.attr(te, "class"), Some("k"));
    assert_eq!(d.attr(te, "data-x"), Some("1"));
    assert_eq!(all_texts(&mut d).len(), 1, "old element gone");
    let st = Style::parse(d.attr(te, "style").unwrap());
    assert_eq!(st.get("font-family"), Some("'DejaVu Sans'"));
    assert_eq!(st.get("fill"), Some("red"));
    assert!(st.get("baseline-shift").is_none() && st.get("direction").is_none());
    let tspans: Vec<NodeId> = d.children(te).filter(|&n| d.is_element(n)).collect();
    assert_eq!(tspans.len(), 2, "one tspan per chunk");
    assert_eq!(d.attr(tspans[0], "x"), Some("0"));
    assert_eq!(d.attr(tspans[1], "y"), Some("20"));
    let s0 = Style::parse(d.attr(tspans[0], "style").unwrap());
    assert_eq!(s0.get("font-size"), Some("10"));
    assert_eq!(
        (s0.get("text-anchor"), s0.get("text-align")),
        (Some("start"), Some("start"))
    );
    // nested runs: "ab" | "c" (bold) | " "
    let nested: Vec<NodeId> = d.children(tspans[0]).filter(|&n| d.is_element(n)).collect();
    assert_eq!(nested.len(), 3);
    assert_eq!(d.text_content(nested[0]), "ab");
    assert_eq!(d.text_content(nested[1]), "c");
    let sc = Style::parse(d.attr(nested[1], "style").unwrap());
    assert_eq!(sc.get("font-weight"), Some("bold"));
    assert!(sc.get("font-size").is_none() && sc.get("baseline-shift").is_none());
    // second chunk: "d" (60 %, super) | "ef"
    let nested: Vec<NodeId> = d.children(tspans[1]).filter(|&n| d.is_element(n)).collect();
    assert_eq!(nested.len(), 2);
    let sd = Style::parse(d.attr(nested[0], "style").unwrap());
    assert_eq!(sd.get("font-size"), Some("60%"));
    assert_eq!(sd.get("baseline-shift"), Some("super"));
    // both chunks at x=0 with one consistent y step → sodipodi:role="line" everywhere
    assert!(
        tspans
            .iter()
            .all(|&t| d.attr(t, "sodipodi:role") == Some("line"))
    );
    assert_eq!((d.attr(te, "x"), d.attr(te, "y")), (Some("0"), Some("0")));
    assert_eq!(st.get("font-size"), Some("10"));
    assert_eq!(st.get("line-height"), Some("2"));
    // appearance: re-parse the written document
    let s = out(&d);
    let mut d2 = Doc::parse(s.as_bytes()).unwrap();
    let (pts2, _) = arena(&mut d2);
    let after = positions(&pts2);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!(
            b.0 == a.0 && (b.1 - a.1).abs() < 1e-6 && (b.2 - a.2).abs() < 1e-6,
            "{b:?} vs {a:?}\n{s}"
        );
    }
}

#[test]
fn writer_omits_dx_when_zero_and_skips_role_line_for_ragged_chunks() {
    let svg = format!(
        r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="0 40" y="0 3" dx="0 2 0">abc</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (pts, ct) = arena(&mut d);
    let te = write_all(&mut d, &pts, &ct)[0].unwrap();
    let tspans: Vec<NodeId> = d.children(te).filter(|&n| d.is_element(n)).collect();
    assert_eq!(tspans.len(), 2);
    assert_eq!(d.attr(tspans[0], "dx"), None, "chunk 'a' has no dx");
    assert_eq!(d.attr(tspans[1], "dx"), Some("2"), "trailing zero trimmed");
    assert_eq!(d.attr(tspans[1], "y"), Some("3"));
    assert!(
        tspans.iter().all(|&t| d.attr(t, "sodipodi:role").is_none()),
        "x differs → no role=line"
    );
    assert_eq!(d.attr(te, "x"), None);
    let s1 = Style::parse(d.attr(tspans[1], "style").unwrap());
    assert_eq!(
        (s1.get("text-anchor"), s1.get("text-align")),
        (Some("middle"), Some("center"))
    );
    // a single chunk always qualifies: line-height 1.25
    let svg = format!(
        r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="7" y="9">ab</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (pts, ct) = arena(&mut d);
    let te = write_all(&mut d, &pts, &ct)[0].unwrap();
    let st = Style::parse(d.attr(te, "style").unwrap());
    assert_eq!(st.get("line-height"), Some("1.25"));
    assert_eq!((d.attr(te, "x"), d.attr(te, "y")), (Some("7"), Some("9")));
    let root = d.svg();
    assert_eq!(
        d.attr(root, "xmlns:sodipodi"),
        Some("http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"),
        "role=line needs its namespace on a matplotlib-style document"
    );
}

#[test]
fn writer_places_split_offs_reuses_ids_and_removes_emptied_elements() {
    let svg = format!(
        r#"<svg {NS}><g id="g"><rect id="r"/><text id="t" style="{DV};font-size:10px" x="0" y="0">ab</text><text id="u" style="{DV};font-size:10px" x="0" y="30">cd</text></g></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (mut pts, ct) = arena(&mut d);
    let news = split_off(&mut pts, 0, &[vec![1]]); // 'b' leaves t
    assert_eq!(news, [2]);
    let all: Vec<usize> = (0..pts[1].chars.len()).collect();
    remove_chars(&mut pts[1], &all); // u loses everything (as if merged elsewhere)
    let res = write_all(&mut d, &pts, &ct);
    assert!(res[0].is_some() && res[1].is_none() && res[2].is_some());
    let g = id(&d, "g");
    let kids: Vec<(String, Option<String>)> = d
        .children(g)
        .filter(|&n| d.is_element(n))
        .map(|n| (d.tag(n).to_string(), d.attr(n, "id").map(str::to_string)))
        .collect();
    assert_eq!(kids.len(), 3, "{kids:?}");
    assert_eq!(kids[0], ("rect".into(), Some("r".into())));
    assert_eq!(
        kids[1],
        ("text".into(), Some("t".into())),
        "rewritten in place, id reused"
    );
    assert_eq!(kids[2].0, "text");
    assert!(
        kids[2].1.as_deref().unwrap().starts_with("sciink-"),
        "split-off gets a fresh id"
    );
    assert!(d.by_id("u").is_none(), "emptied element removed");
    assert_eq!(d.text_content(res[2].unwrap()), "b");

    // a split-off whose source was emptied lands where the source was
    let svg = format!(
        r#"<svg {NS}><g id="g"><rect id="r"/><text id="t" style="{DV};font-size:10px" x="0" y="0">ab</text><rect id="s"/></g></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (mut pts, ct) = arena(&mut d);
    split_off(&mut pts, 0, &[vec![1]]);
    let rest: Vec<usize> = (0..pts[0].chars.len()).collect();
    remove_chars(&mut pts[0], &rest);
    let res = write_all(&mut d, &pts, &ct);
    assert!(res[0].is_none() && res[1].is_some());
    let g = id(&d, "g");
    let tags: Vec<String> = d
        .children(g)
        .filter(|&n| d.is_element(n))
        .map(|n| d.tag(n).to_string())
        .collect();
    assert_eq!(tags, ["rect", "text", "rect"]);
}

#[test]
fn clip_unions_duplicate_and_transform_or_drop() {
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="c1"><rect width="50" height="50"/></clipPath><clipPath id="c2"><rect x="10" width="50" height="50"/></clipPath></defs><text id="a" clip-path="url(#c1)" transform="translate(1,2)" style="{DV};font-size:10px" x="0" y="0">a</text><text id="b" clip-path="url(#c2)" transform="translate(3,4)" style="{DV};font-size:10px" x="0" y="0">b</text><text id="c" style="{DV};font-size:10px" x="0" y="0">c</text></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (a, b, c) = (id(&d, "a"), id(&d, "b"), id(&d, "c"));
    apply_clip_unions(
        &mut d,
        &[ClipUnion {
            target: a,
            others: vec![b],
        }],
    );
    let cp = d.attr(a, "clip-path").unwrap().to_string();
    assert_ne!(cp, "url(#c1)");
    let dc = d
        .by_id(cp.trim_start_matches("url(#").trim_end_matches(')'))
        .expect("new clipPath");
    assert_eq!(d.tag(dc), "clipPath");
    let kids: Vec<NodeId> = d.children(dc).filter(|&n| d.is_element(n)).collect();
    assert_eq!(kids.len(), 2);
    assert_eq!(d.tag(kids[0]), "rect");
    assert_eq!(d.tag(kids[1]), "g");
    assert_eq!(
        d.attr(kids[1], "transform"),
        Some("translate(2,2)"),
        "(a)⁻¹ · (b)"
    );
    let inner: Vec<NodeId> = d.children(kids[1]).filter(|&n| d.is_element(n)).collect();
    assert_eq!(d.attr(inner[0], "x"), Some("10"));
    assert_eq!(
        d.children(id(&d, "c1"))
            .filter(|&n| d.is_element(n))
            .count(),
        1,
        "original untouched"
    );
    // any unclipped participant → the target loses its clip
    apply_clip_unions(
        &mut d,
        &[ClipUnion {
            target: b,
            others: vec![c],
        }],
    );
    assert_eq!(d.attr(b, "clip-path"), None);
}

#[test]
fn writer_places_a_split_off_of_a_split_off_after_its_own_source() {
    // The ordering ruling (spec: "a split-off of a split-off after its own source, pre-order")
    // exists for exactly this shape, and upstream's `addnext` gets it wrong.
    let svg = format!(
        r#"<svg {NS}><g id="g"><rect id="r"/><text id="t" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">abcdef</text><rect id="s"/></g></svg>"#
    );
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let (mut pts, ct) = arena(&mut d);
    assert_eq!(split_off(&mut pts, 0, &[vec![2, 3, 4, 5]]), [1]); // "cdef" leaves t
    assert_eq!(split_off(&mut pts, 1, &[vec![1]]), [2]); // "d" leaves the split-off …
    assert_eq!(split_off(&mut pts, 1, &[vec![1]]), [3]); // … and so does "e"
    assert_eq!(
        pts.iter().map(|p| p.text()).collect::<Vec<_>>(),
        ["ab", "cf", "d", "e"]
    );
    assert_eq!((pts[2].split_src, pts[3].split_src), (Some(1), Some(1)));
    let res = write_all(&mut d, &pts, &ct);
    assert!(res.iter().all(|r| r.is_some()), "{res:?}");
    let order: Vec<String> = d
        .children(id(&d, "g"))
        .filter(|&n| d.is_element(n))
        .map(|n| match d.tag(n) {
            "text" => d.text_content(n),
            t => t.to_string(),
        })
        .collect();
    // "d" after its own source "cf" (not after "ab"), and "e" after "d": the second split-off of
    // "cf" only lands there because writing "d" advanced its source's slot.
    assert_eq!(
        order,
        ["rect", "ab", "cf", "d", "e", "rect"],
        "source, split, split-of-split"
    );
}
