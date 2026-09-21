mod support;

use support::with_vendored_fonts;

use std::ffi::OsString;
use std::path::PathBuf;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

fn run_fix(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=text-fix"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes())).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}
/// Left edge of each character and the total advance of `text` (DejaVu Sans 10 px, one chunk at 0).
fn layout(text: &str) -> (Vec<f64>, f64) {
    use sciink::text::{
        Warnings, fonts::FontSystem, layout::chunk_geom, parse::ParsedText, table::CharTable,
    };
    let svg = format!(
        r#"<svg {NS}><text id="p" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">{text}</text></svg>"#
    );
    let mut d = sciink::dom::Doc::parse(svg.as_bytes()).unwrap();
    let n = d.by_id("p").unwrap();
    let mut w = Warnings::default();
    let fonts =
        FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")]);
    let mut ct = CharTable::build(&d, &[n], fonts, &mut w);
    let pt = ParsedText::parse(&mut d, n, &mut ct, &mut w).unwrap();
    let g = chunk_geom(&pt, 0, 0);
    let n = g.left.len();
    (g.left.clone(), g.right[n - 1])
}
fn texts(svg: &str) -> Vec<(Option<String>, String)> {
    let d = roxmltree::Document::parse(svg).unwrap();
    d.descendants()
        .filter(|n| n.has_tag_name("text"))
        .map(|n| {
            let s: String = n
                .descendants()
                .filter(|c| c.is_text())
                .filter_map(|c| c.text())
                .collect();
            (n.attribute("id").map(str::to_string), s)
        })
        .collect()
}

#[test]
fn text_fix_merges_words_splits_ticks_and_keeps_every_glyph_in_place() {
    let (_, hello_w) = layout("Hello");
    let (sp, _) = layout(" a");
    let x2 = hello_w + sp[1];
    let svg = format!(
        r#"<svg {NS}><g id="layer1"><text id="w1" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="w2" xml:space="preserve" style="{DV};font-size:10px" x="{x2}" y="0">world</text><text id="ticks" xml:space="preserve" style="{DV};font-size:10px" x="0" y="30">0 1 2</text><text id="k" xml:space="preserve" style="{DV};font-size:10px" x="0" y="60" dx="0 -1 1 0">kern </text></g></svg>"#
    );
    let before = support::text_positions(&svg);
    let (out, msgs) = run_fix(&svg, &["--justification=2", "--id=layer1"]);
    assert!(
        msgs[0].starts_with("text-fix: 4 text elements in, 5 out"),
        "{msgs:?}"
    );
    let after = support::text_positions(&out);
    // words and ticks (y < 50) are only re-encoded: exact invariance
    let keep =
        |v: &[(char, f64, f64)]| v.iter().copied().filter(|p| p.2 < 50.0).collect::<Vec<_>>();
    support::assert_same_positions(&keep(&before), &keep(&after), 1e-3, "text-fix");
    // "kern " had manual kerning: removing it moves e/r/n to the font's natural advances (that is
    // the feature), so compare against a fresh layout of "kern" instead of the input
    let (natural, _) = layout("kern");
    let mut kern: Vec<(char, f64, f64)> = after.iter().copied().filter(|p| p.2 > 50.0).collect();
    kern.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    assert_eq!(kern.iter().map(|p| p.0).collect::<String>(), "kern");
    for (p, x) in kern.iter().zip(&natural) {
        assert!(
            (p.1 - x).abs() < 1e-3 && (p.2 - 60.0).abs() < 1e-3,
            "{p:?} vs natural x {x}"
        );
    }
    let mut t: Vec<String> = texts(&out).into_iter().map(|(_, s)| s).collect();
    t.sort();
    assert_eq!(t, ["0", "1", "2", "Hello world", "kern"], "{out}");
    let ids: Vec<Option<String>> = texts(&out).into_iter().map(|(i, _)| i).collect();
    assert!(
        ids.contains(&Some("w1".into()))
            && ids.contains(&Some("ticks".into()))
            && ids.contains(&Some("k".into()))
    );
    assert!(!ids.contains(&Some("w2".into())), "merged away");
    assert_eq!(
        ids.iter()
            .filter(|i| i.as_deref().is_some_and(|s| s.starts_with("sciink-")))
            .count(),
        2
    );
    assert!(!out.contains(" dx="), "manual kerning gone:\n{out}");
    assert!(out.matches("sodipodi:role=\"line\"").count() >= 5);
    assert!(out.contains("xml:space=\"preserve\""));
    // no selection → the document comes back unchanged with a message
    let err = with_vendored_fonts(|| {
        sciink::run(&args(&["--tool=text-fix", "--id=nope"]), svg.as_bytes())
    });
    assert!(err.is_err());
}

#[test]
fn text_fix_on_a_real_inkscape_document_is_appearance_invariant() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/corpus/Simple_text.svg");
    let svg = std::fs::read_to_string(path).unwrap();
    let root_id = roxmltree::Document::parse(&svg)
        .unwrap()
        .root_element()
        .attribute("id")
        .unwrap()
        .to_string();
    let before = support::text_positions(&svg);
    assert!(!before.is_empty());
    // without merges and manual-kerning removal every remaining stage is exact
    let (out, _) = run_fix(
        &svg,
        &[
            "--mergenearby=false",
            "--mergesubsuper=false",
            "--removemanualkerning=false",
            "--justification=4",
            &format!("--id={root_id}"),
        ],
    );
    support::assert_same_positions(
        &before,
        &support::text_positions(&out),
        1e-3,
        "no-merge run",
    );
    // re-anchoring every line (justification=1 → middle) is appearance-preserving too, as long
    // as no chunk carries a leading dx — stage 5 guarantees that, and this document has none.
    let (out, _) = run_fix(
        &svg,
        &[
            "--mergenearby=false",
            "--mergesubsuper=false",
            "--removemanualkerning=false",
            "--justification=1",
            &format!("--id={root_id}"),
        ],
    );
    assert!(out.contains("text-anchor:middle"), "the anchor did change");
    support::assert_same_positions(
        &before,
        &support::text_positions(&out),
        1e-3,
        "justification=1 run",
    );
    // with merges glyphs may snap to whole spaces, but none may appear or vanish
    let (out, _) = run_fix(&svg, &[&format!("--id={root_id}")]);
    let mut b: Vec<char> = before.iter().map(|p| p.0).collect();
    let mut a: Vec<char> = support::text_positions(&out).iter().map(|p| p.0).collect();
    b.sort();
    a.sort();
    assert_eq!(a, b);
}
