mod support;

use sciink::dom::{Doc, NodeId};
use sciink::text::style::{
    Anchor, baseline_shift, composed_font_size, composed_line_height, composed_width,
    letter_spacing,
};

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

#[test]
fn font_size_defaults_keywords_units_and_transforms() {
    let d = doc(&format!(
        r#"<svg {NS}>
      <g transform="scale(2)">
        <text id="a">x</text>
        <text id="b" style="font-size:medium">x</text>
        <text id="c" style="font-size:large">x</text>
        <text id="d" style="font-size:12pt">x</text>
        <text id="e" font-size="9" transform="scale(3,3)">x</text>
        <text id="f" style="font-size:xx-large">x</text>
      </g></svg>"#
    ));
    let fs = composed_font_size(&d, id(&d, "a"));
    assert_eq!((fs.utfs, fs.scf, fs.tfs), (12.0, 2.0, 24.0));
    assert_eq!(composed_font_size(&d, id(&d, "b")).utfs, 12.0);
    assert_eq!(composed_font_size(&d, id(&d, "c")).utfs, 14.0);
    assert!((composed_font_size(&d, id(&d, "d")).utfs - 16.0).abs() < 1e-9);
    let e = composed_font_size(&d, id(&d, "e"));
    assert_eq!((e.utfs, e.scf, e.tfs), (9.0, 6.0, 54.0));
    assert_eq!(
        composed_font_size(&d, id(&d, "f")).utfs,
        12.0,
        "unknown keyword → 12"
    );
}

#[test]
fn relative_font_sizes_resolve_against_the_ancestor_that_set_them() {
    let d = doc(&format!(
        r#"<svg {NS}>
      <text id="t" style="font-size:10px"><tspan id="s" style="font-size:65%"><tspan id="u">x</tspan><tspan id="v" style="font-size:2em">y</tspan></tspan></text>
      <text id="w" style="font-size:150%">z</text></svg>"#
    ));
    let s = composed_font_size(&d, id(&d, "s"));
    assert!((s.utfs - 6.5).abs() < 1e-9 && s.scf == 1.0);
    // u only inherits the 65% string → resolved where it was set (s) → 6.5
    assert!((composed_font_size(&d, id(&d, "u")).utfs - 6.5).abs() < 1e-9);
    // v: 2em of its parent's (s) size
    assert!((composed_font_size(&d, id(&d, "v")).utfs - 13.0).abs() < 1e-9);
    // % on a root-level text: relative to the root's default 12px
    assert!((composed_font_size(&d, id(&d, "w")).utfs - 18.0).abs() < 1e-9);
    // composed_width works for stroke-width too
    let d2 = doc(&format!(
        r#"<svg {NS}><g transform="scale(2)"><path id="p" style="stroke-width:3"/><path id="q"/></g></svg>"#
    ));
    let p = composed_width(&d2, id(&d2, "p"), "stroke-width");
    assert_eq!((p.utfs, p.tfs), (3.0, 6.0));
    assert_eq!(composed_width(&d2, id(&d2, "q"), "stroke-width").utfs, 1.0);
}

#[test]
fn line_height_variants() {
    let d = doc(&format!(
        r#"<svg {NS}><g transform="scale(2)">
      <text id="a" style="font-size:10px">x</text>
      <text id="b" style="font-size:10px;line-height:1.5">x</text>
      <text id="c" style="font-size:10px;line-height:125%">x</text>
      <text id="d" style="font-size:10px;line-height:1.1em">x</text>
      <text id="e" style="font-size:10px;line-height:15px">x</text></g></svg>"#
    ));
    assert!(
        (composed_line_height(&d, id(&d, "a")) - 25.0).abs() < 1e-9,
        "normal = 1.25 × tfs(20)"
    );
    assert!((composed_line_height(&d, id(&d, "b")) - 30.0).abs() < 1e-9);
    assert!((composed_line_height(&d, id(&d, "c")) - 25.0).abs() < 1e-9);
    assert!((composed_line_height(&d, id(&d, "d")) - 22.0).abs() < 1e-9);
    assert!(
        (composed_line_height(&d, id(&d, "e")) - 30.0).abs() < 1e-9,
        "15px / utfs 10 × tfs 20"
    );
}

#[test]
fn letter_spacing_and_baseline_shift() {
    let d = doc(&format!(
        r#"<svg {NS}>
      <text id="t" style="font-size:10px;letter-spacing:0.1em"><tspan id="a">x</tspan><tspan id="b" style="letter-spacing:2px">y</tspan><tspan id="c" style="letter-spacing:normal">z</tspan></text>
      <text id="u" style="font-size:10px">as<tspan id="sup" style="font-size:65%;baseline-shift:super">f<tspan id="inh">g</tspan><tspan id="sub" style="baseline-shift:sub">h</tspan></tspan><tspan id="pct" style="baseline-shift:-30%">i</tspan><tspan id="len" style="baseline-shift:3px">j</tspan></text></svg>"#
    ));
    let st = |i: &str| d.specified_style(id(&d, i));
    assert!(
        (letter_spacing(&d, id(&d, "a"), &st("a")) - 1.0).abs() < 1e-9,
        "0.1em × 10px"
    );
    assert!((letter_spacing(&d, id(&d, "b"), &st("b")) - 2.0).abs() < 1e-9);
    assert_eq!(letter_spacing(&d, id(&d, "c"), &st("c")), 0.0);
    assert_eq!(baseline_shift(&d, id(&d, "u"), &st("u")), 0.0);
    // super: +40% of the PARENT's untransformed font size (10px) = 4
    assert!((baseline_shift(&d, id(&d, "sup"), &st("sup")) - 4.0).abs() < 1e-9);
    // inherited only: the running sum is added again (Inkscape's compounding) → 8
    assert!((baseline_shift(&d, id(&d, "inh"), &st("inh")) - 8.0).abs() < 1e-9);
    // sub inside super: 4 + (−20% of parent size 6.5 = −1.3) = 2.7
    assert!((baseline_shift(&d, id(&d, "sub"), &st("sub")) - 2.7).abs() < 1e-9);
    assert!((baseline_shift(&d, id(&d, "pct"), &st("pct")) + 3.0).abs() < 1e-9);
    assert!((baseline_shift(&d, id(&d, "len"), &st("len")) - 3.0).abs() < 1e-9);
    assert_eq!(Anchor::parse("middle"), Some(Anchor::Middle));
    assert_eq!(Anchor::parse("weird"), None);
    assert_eq!(
        (
            Anchor::Start.anfr(),
            Anchor::Middle.anfr(),
            Anchor::End.anfr()
        ),
        (0.0, 0.5, 1.0)
    );
    assert_eq!(Anchor::End.css(), "end");
}
