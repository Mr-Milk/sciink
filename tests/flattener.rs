mod support;

use std::ffi::OsString;

use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";
const DV: &str = "font-family:'DejaVu Sans'";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn flatten(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=flattener"];
    a.extend(extra);
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes())).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn has(d: &roxmltree::Document, id: &str) -> bool {
    d.descendants().any(|n| n.attribute("id") == Some(id))
}
fn kids<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> Vec<roxmltree::Node<'a, 'i>> {
    n.children().filter(|c| c.is_element()).collect()
}
fn style_of(n: roxmltree::Node) -> sciink::style::Style {
    n.attribute("style")
        .map(sciink::style::Style::parse)
        .unwrap_or_default()
}
const INKSCAPE_NS: &str = "http://www.inkscape.org/namespaces/inkscape";
const SODIPODI_NS: &str = "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd";
/// roxmltree resolves prefixes: a namespaced attribute is looked up by (namespace, local name),
/// never by its prefixed spelling.
fn nsattr<'a>(n: roxmltree::Node<'a, '_>, ns: &str, local: &str) -> Option<&'a str> {
    n.attribute((ns, local))
}

#[test]
fn exclusions_tab_marks_and_unmarks_the_selection_and_nothing_else_runs() {
    let svg = format!(
        r#"<svg {NS}><g id="g"><path id="p" d="M0 0h1"/></g><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = flatten(
        &svg,
        &["--tab=Exclusions", "--markexc=1", "--id=g", "--id=r"],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        by_id(&d, "g").attribute("inkscape-scientific-flattenexclude"),
        Some("True")
    );
    assert_eq!(
        by_id(&d, "r").attribute("inkscape-scientific-flattenexclude"),
        Some("True")
    );
    assert!(
        has(&d, "p") && by_id(&d, "p").parent().unwrap().attribute("id") == Some("g"),
        "no flattening happened"
    );
    let (s, _) = flatten(&s, &["--tab=Exclusions", "--markexc=2", "--id=g"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        by_id(&d, "g").attribute("inkscape-scientific-flattenexclude"),
        None
    );
    assert_eq!(
        by_id(&d, "r").attribute("inkscape-scientific-flattenexclude"),
        Some("True"),
        "only the selection changes"
    );
}

#[test]
fn excluded_elements_are_left_alone_and_an_empty_selection_is_an_error() {
    let svg = format!(
        r#"<svg {NS}><g id="keep" inkscape-scientific-flattenexclude="True"><path id="p" d="M0 0h1"/></g><g id="flat"><path id="q" d="M0 0h1"/></g></svg>"#
    );
    let (s, _) = flatten(&svg, &["--id=keep", "--id=flat"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "keep") && by_id(&d, "p").parent().unwrap().attribute("id") == Some("keep"),
        "excluded group survives untouched"
    );
    assert!(!has(&d, "flat"), "the other group is ungrouped");
    assert_eq!(by_id(&d, "q").parent().unwrap().tag_name().name(), "svg");
    // only excluded/containers selected → upstream's message, document unchanged
    let a = args(&["--tool=flattener", "--tab=Options", "--id=keep"]);
    let err = with_vendored_fonts(|| sciink::run(&a, svg.as_bytes()))
        .err()
        .expect("an error");
    assert!(err.contains("No objects selected!"), "{err}");
    // a <defs> alone is neither a group nor an object (an empty <g> would count, as upstream)
    let empty = format!(r#"<svg {NS}><g id="e"/><defs id="d"/></svg>"#);
    let a = args(&["--tool=flattener", "--tab=Options", "--id=d"]);
    assert!(with_vendored_fonts(|| sciink::run(&a, empty.as_bytes())).is_err());
    let a = args(&["--tool=flattener", "--tab=Options", "--id=e"]);
    assert!(
        with_vendored_fonts(|| sciink::run(&a, empty.as_bytes())).is_ok(),
        "an empty group is 'an object' and simply dissolves"
    );
}

#[test]
fn test_mode_duplicates_the_selected_layer_and_flattens_the_original() {
    let svg = format!(
        r#"<svg {NS}><g id="layer1" inkscape:groupmode="layer" inkscape:label="Layer 1"><g id="inner"><path id="p" d="M0 0h1"/></g><rect id="r" width="1" height="1"/></g></svg>"#
    );
    let (s, msgs) = flatten(&svg, &["--id=layer1", "--testmode=true"]);
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let root = d.root_element();
    let top = kids(root);
    assert_eq!(
        top.len(),
        2,
        "a duplicate layer sits BEFORE the flattened original: {s}"
    );
    let (dup, orig) = (top[0], top[1]);
    assert_eq!(nsattr(dup, INKSCAPE_NS, "label"), Some("Layer 1 original"));
    assert_eq!(nsattr(dup, SODIPODI_NS, "insensitive"), Some("true"));
    assert_eq!(dup.attribute("opacity"), Some("0.3"));
    assert_eq!(dup.attribute("id"), None, "the copy carries no ids");
    assert_eq!(kids(dup).len(), 2, "the copy is the untouched layer");
    assert_eq!(kids(kids(dup)[0]).len(), 1, "…with its nested group intact");
    assert_eq!(
        (orig.attribute("id"), nsattr(orig, INKSCAPE_NS, "label")),
        (Some("layer1"), Some("Layer 1 flat"))
    );
    let names: Vec<&str> = kids(orig).iter().map(|n| n.tag_name().name()).collect();
    assert_eq!(
        names,
        vec!["path", "rect"],
        "the original's children were flattened in place: {s}"
    );
    assert!(!has(&d, "inner"));
}

#[test]
fn options_are_anded_with_fixtext_and_hidden_parameters_are_accepted() {
    use clap::Parser;
    use sciink::tools::flattener::{FlattenerCli, Options};
    let cli = FlattenerCli::try_parse_from(args(&[
        "--tool=flattener",
        "--fixtext=false",
        "--splitdistant=true",
        "--mergenearby=true",
        "--setreplacement=true",
        "--removetextclips=true",
        "--reversions=true",
        "--justification=3",
        "--id=x",
    ]))
    .unwrap();
    let o = Options::from_cli(&cli);
    assert!(
        !o.fixtext
            && !o.splitdistant
            && !o.mergenearby
            && !o.setreplacement
            && !o.removetextclips
            && !o.reversions,
        "text sub-options follow fixtext"
    );
    assert!(
        o.deepungroup && o.revertpaths && o.removeduppaths && o.removerectw,
        "defaults"
    );
    assert_eq!((o.justification, o.replacement.as_str()), (3, "Arial"));
    let t = Options::testmode();
    assert!(
        t.fixtext
            && t.splitdistant
            && t.mergenearby
            && t.setreplacement
            && t.reversions
            && t.removetextclips
            && t.removemanualkerning
            && t.mergesubsuper
            && t.deepungroup
            && t.revertpaths
            && t.removerectw
            && t.removeduppaths
    );
    assert_eq!((t.justification, t.replacement.as_str()), (1, "sans-serif"));
    // hidden upstream parameters are accepted and ignored
    let cli = FlattenerCli::try_parse_from(args(&[
        "--tool=flattener",
        "--debugparser=true",
        "--v=1.2",
        "--id=x",
    ]))
    .unwrap();
    assert!(cli.debugparser);
    let svg = format!(r#"<svg {NS}><g id="a"><path id="p" d="M0 0h1"/></g></svg>"#);
    let (s, _) = flatten(
        &svg,
        &[
            "--id=nosuch",
            "--id=a",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    assert!(
        !roxmltree::Document::parse(&s)
            .unwrap()
            .descendants()
            .any(|n| n.attribute("id") == Some("a")),
        "an unknown id is dropped, the known one is flattened: {s}"
    );
}

#[test]
fn deep_ungroup_composes_transforms_styles_and_clips_onto_leaves() {
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="10" height="10"/></clipPath></defs><g id="layer"><g id="a" transform="translate(1,2)" style="fill:red;opacity:0.5" clip-path="url(#c)"><g id="b" transform="scale(2)"><path id="p" d="M0 0h1" style="fill:blue"/></g><rect id="r" width="1" height="1"/></g></g></svg>"#
    );
    // selecting a layer dissolves the layer itself (upstream too: `seld` contains the selection);
    // select the group instead and check its parent
    let (s, msgs) = flatten(
        &svg,
        &[
            "--id=a",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let layer = by_id(&d, "layer");
    let names: Vec<Option<&str>> = kids(layer).iter().map(|n| n.attribute("id")).collect();
    assert_eq!(
        names,
        vec![Some("p"), Some("r")],
        "both groups dissolved, order kept: {s}"
    );
    let p = by_id(&d, "p");
    assert_eq!(p.attribute("transform"), Some("matrix(2,0,0,2,1,2)"));
    let st = style_of(p);
    assert_eq!(
        (st.get("fill"), st.get("opacity")),
        (Some("blue"), Some("0.5"))
    );
    assert!(
        p.attribute("clip-path")
            .is_some_and(|c| c.starts_with("url(#")),
        "the group's clip travels down: {s}"
    );
    assert_eq!(
        by_id(&d, "r").attribute("clip-path"),
        Some("url(#c)"),
        "an untransformed child points at the clip itself"
    );
    assert!(!has(&d, "a") && !has(&d, "b"));
    assert!(
        !s.contains("\n  "),
        "no indentation whitespace survives: {s}"
    );
}

#[test]
fn selected_defs_and_loose_clips_move_into_the_root_defs() {
    let svg = format!(
        r#"<svg {NS}><defs id="root"/><g id="layer"><defs id="inner"><path id="glyph" d="M0 0h1"/></defs><clipPath id="loose"><rect width="1" height="1"/></clipPath><path id="p" d="M0 0h1" clip-path="url(#loose)"/></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=inner",
            "--id=loose",
            "--id=p",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    let root = by_id(&d, "root");
    let moved: Vec<Option<&str>> = kids(root).iter().map(|n| n.attribute("id")).collect();
    assert_eq!(
        moved,
        vec![Some("inner"), Some("loose")],
        "appended to the root <defs> in document order: {s}"
    );
    assert_eq!(
        by_id(&d, "glyph").parent().unwrap().attribute("id"),
        Some("inner"),
        "the nested defs is moved whole"
    );
    let layer_kids: Vec<Option<&str>> = kids(by_id(&d, "layer"))
        .iter()
        .map(|n| n.attribute("id"))
        .collect();
    assert_eq!(layer_kids, vec![Some("p")]);
    assert_eq!(by_id(&d, "p").attribute("clip-path"), Some("url(#loose)"));
}

#[test]
fn clones_of_paths_are_unlinked_but_symbol_clones_stay() {
    let svg = format!(
        r##"<svg {NS} xmlns:xlink="http://www.w3.org/1999/xlink"><defs><path id="glyph" d="M0 0h1"/><symbol id="sym"><circle id="c" r="1"/></symbol></defs><g id="layer"><g id="g" transform="translate(5,0)"><use id="u" xlink:href="#glyph" x="1"/><use id="us" xlink:href="#sym"/><use id="dangling" xlink:href="#nope"/></g></g></svg>"##
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=g",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    let layer = by_id(&d, "layer");
    let k = kids(layer);
    assert_eq!(k.len(), 3, "{s}");
    assert_eq!(
        (k[0].tag_name().name(), k[0].attribute("id")),
        ("path", Some("u")),
        "the clone became a path with the clone's id"
    );
    assert_eq!(
        k[0].attribute("transform"),
        Some("translate(6,0)"),
        "x offset then the group's transform"
    );
    assert_eq!(
        k[0].attribute("unlinked_clone"),
        None,
        "the marker is stripped at the end"
    );
    assert_eq!(
        (k[1].tag_name().name(), k[1].attribute("id")),
        ("use", Some("us")),
        "symbol clones are not unlinked by the Flattener"
    );
    assert_eq!(
        (k[2].tag_name().name(), k[2].attribute("id")),
        ("use", Some("dangling")),
        "a clone of nothing is left alone"
    );
    assert!(has(&d, "glyph") && has(&d, "sym"), "definitions stay");
}

#[test]
fn matplotlib_glyph_groups_keep_their_comment_as_mpl_comment() {
    // first occurrence of a glyph: matplotlib puts its <path> in a <defs> INSIDE the text group
    let svg = format!(
        r##"<svg {NS} xmlns:xlink="http://www.w3.org/1999/xlink"><g id="layer"><g id="text_1"><!-- 0.5 --><defs><path id="DejaVuSans-30" d="M0 0h1v1z"/></defs><g transform="translate(10,20) scale(0.1,-0.1)"><use xlink:href="#DejaVuSans-30"/></g></g><g id="text_2"><!-- 1 --><g transform="translate(30,20)"><use xlink:href="#DejaVuSans-30"/><use xlink:href="#DejaVuSans-30" x="60"/><use xlink:href="#DejaVuSans-30" x="120"/></g></g><g id="already" mpl_comment="kept"><path id="k" d="M0 0h1"/></g></g></svg>"##
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    let t1 = by_id(&d, "text_1");
    assert_eq!(t1.attribute("mpl_comment"), Some("0.5"), "{s}");
    assert!(
        t1.children().all(|c| !c.is_comment()),
        "the comment itself is removed"
    );
    let t1k: Vec<&str> = kids(t1).iter().map(|n| n.tag_name().name()).collect();
    assert_eq!(
        t1k,
        vec!["path"],
        "the <defs> moved to the root defs first, the inner group dissolved next (1 child < 2), the glyph stays grouped: {s}"
    );
    let glyph = by_id(&d, "DejaVuSans-30");
    assert_eq!(glyph.parent().unwrap().tag_name().name(), "defs");
    assert_eq!(
        glyph.parent().unwrap().parent().unwrap().tag_name().name(),
        "defs",
        "…inside the root <defs>"
    );
    // upstream quirk: a text group with fewer children than its glyph group is dissolved first
    assert!(!has(&d, "text_2"), "{s}");
    assert_eq!(
        by_id(&d, "already").attribute("mpl_comment"),
        Some("kept"),
        "groups already marked are left grouped"
    );
    assert_eq!(
        by_id(&d, "k").parent().unwrap().attribute("id"),
        Some("already")
    );
}

#[test]
fn ungroup_of_a_clipped_out_child_does_not_panic_on_its_dissolved_group() {
    // `inner` (3 children) is processed AFTER `outer` (2 children); ungrouping `outer` clips it out
    // entirely (disjoint rectangles) and deletes it, so when its turn comes it is detached and
    // must be skipped, not touched
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="a"><rect width="1" height="1"/></clipPath><clipPath id="b"><rect x="50" y="50" width="1" height="1"/></clipPath></defs><g id="layer"><g id="outer" clip-path="url(#a)"><g id="inner" clip-path="url(#b)"><path id="p1" d="M0 0h1"/><path id="p2" d="M0 0h1"/><path id="p3" d="M0 0h1"/></g><rect id="r" width="1" height="1"/></g></g></svg>"#
    );
    let (s, msgs) = flatten(
        &svg,
        &[
            "--id=outer",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        !has(&d, "inner") && !has(&d, "p1") && !has(&d, "p2") && !has(&d, "p3"),
        "clipped out: {s}"
    );
    assert!(has(&d, "r") && has(&d, "layer"));
}

#[test]
fn matplotlib_minus_glyphs_become_minus_signs() {
    // a real matplotlib minus: the DejaVu glyph path under the usual flip
    let svg = format!(
        r#"<svg {NS}><g id="layer"><g id="t" transform="translate(50,60) scale(0.1,-0.1)"><path id="m" d="M 106,355 H 732 V 272 H 106 Z" style="fill:#336699;fill-opacity:0.5"/></g><path id="other" d="M 106,355 H 732 V 272 H 106 Z" transform="scale(0.1)" style="fill:#000000"/></g></svg>"#
    );
    // reversions and fixtext stay at their `true` defaults; the four kerning flags off keep
    // `remove_kerning` from touching the new <text>
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--revertpaths=false",
            "--splitdistant=false",
            "--mergenearby=false",
            "--removemanualkerning=false",
            "--mergesubsuper=false",
            "--removetextclips=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    let m = by_id(&d, "m");
    assert_eq!(m.tag_name().name(), "text", "{s}");
    assert_eq!(m.text(), Some("\u{2212}"));
    assert_eq!(
        (m.attribute("x"), m.attribute("y")),
        (Some("19.3964"), Some("626.924"))
    );
    let st = style_of(m);
    assert_eq!(st.get("font-size"), Some("999.997"));
    assert_eq!(st.get("font-family"), Some("sans-serif"));
    assert_eq!(st.get("fill"), Some("#336699"));
    assert_eq!(
        st.get("fill-opacity"),
        Some("0.5"),
        "translucent fill is kept (Deviation)"
    );
    // the glyph was drawn flipped (det < 0): the text is re-flipped about the glyph's centre so
    // it reads upright, and lands in the (now dissolved) group's frame
    let t = sciink::geom::parse_transform(m.attribute("transform").unwrap()).unwrap();
    assert!(t.determinant() > 0.0, "upright: {t:?}");
    let [a, b, c, dd, _, _] = t.as_coeffs();
    assert!(
        (a - 0.1).abs() < 1e-9 && b.abs() < 1e-9 && c.abs() < 1e-9 && (dd - 0.1).abs() < 1e-9,
        "{t:?}"
    );
    // an upright glyph keeps its transform as is
    let o = by_id(&d, "other");
    assert_eq!(o.tag_name().name(), "text");
    let ot = sciink::geom::parse_transform(o.attribute("transform").unwrap()).unwrap();
    assert!(
        sciink::geom::affine_eq(ot, sciink::geom::Affine::scale(0.1)),
        "{ot:?}"
    );
    assert_eq!(style_of(o).get("fill-opacity"), None);
}

#[test]
fn thin_dark_rectangles_become_strokes() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><path id="v" d="M10 0 h2 v100 h-2 z" style="fill:#0a0a0a"/><rect id="h" x="0" y="50" width="80" height="1" style="fill:#000000;fill-opacity:0.95"/><rect id="fat" width="10" height="10" style="fill:#000000"/><rect id="light" x="0" y="0" width="1" height="50" style="fill:#c0c0c0"/><path id="stroked" d="M0 0 h2 v100 h-2 z" style="fill:#000000;stroke:#ff0000"/></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--revertpaths=true",
            "--fixtext=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    let v = by_id(&d, "v");
    assert_eq!(v.tag_name().name(), "path");
    assert_eq!(
        v.attribute("d"),
        Some("M 11,0 L 11,100"),
        "vertical centre line: {s}"
    );
    let st = style_of(v);
    // #0a0a0a: L = 10 → effective lightness 10/255 < 16/255 (a #202020 fill, L = 32, is not "dark")
    assert_eq!(
        (
            st.get("stroke"),
            st.get("fill"),
            st.get("stroke-width"),
            st.get("stroke-linecap")
        ),
        (Some("#0a0a0a"), Some("none"), Some("2"), Some("butt"))
    );
    assert_eq!(st.get("stroke-opacity"), None);
    let h = by_id(&d, "h");
    assert_eq!(h.tag_name().name(), "path", "a <rect> is converted");
    assert_eq!(h.attribute("d"), Some("M 0,50.5 L 80,50.5"));
    let st = style_of(h);
    // black at 95 %: effective lightness 0.05 < 16/255, so it is dark AND translucent
    assert_eq!(
        (
            st.get("stroke-width"),
            st.get("stroke-opacity"),
            st.get("opacity")
        ),
        (Some("1"), Some("0.95"), Some("1"))
    );
    assert_eq!(h.attribute("width"), None, "shape attributes are gone");
    for i in ["fat", "light", "stroked"] {
        assert!(
            style_of(by_id(&d, i)).get("stroke-linecap").is_none(),
            "{i} is not thin, dark and unstroked"
        );
    }
}

#[test]
fn font_replacement_appends_the_family_and_drops_the_inkscape_spec() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><text id="a" style="font-family:'Franklin Gothic Book', serif;-inkscape-font-specification:'Franklin Gothic Book'" x="0" y="0">a<tspan id="s" style="font-family:Arial">b</tspan></text><text id="none" style="font-family:none" x="0" y="20">c</text><text id="bare" x="0" y="40">d</text><text id="same" style="font-family:arial" x="0" y="60">e</text></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--setreplacement=true",
            "--replacement=Arial",
            "--splitdistant=false",
            "--mergenearby=false",
            "--removemanualkerning=false",
            "--mergesubsuper=false",
            "--reversions=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    let st = style_of(by_id(&d, "a"));
    assert_eq!(
        st.get("font-family"),
        Some("Franklin Gothic Book,serif,Arial"),
        "{s}"
    );
    assert_eq!(st.get("-inkscape-font-specification"), None);
    assert_eq!(
        style_of(by_id(&d, "s")).get("font-family"),
        Some("Arial"),
        "a tspan already at the replacement is left alone"
    );
    assert_eq!(
        style_of(by_id(&d, "none")).get("font-family"),
        Some("Arial")
    );
    assert_eq!(
        style_of(by_id(&d, "bare")).get("font-family"),
        Some("Arial"),
        "no family at all → the replacement"
    );
    assert_eq!(
        style_of(by_id(&d, "same")).get("font-family"),
        Some("arial"),
        "case-insensitive match of the last entry: nothing appended"
    );
}

#[test]
fn text_phase_merges_split_words_and_removes_text_clips() {
    // "Hello" + " world" as two elements one space apart (DejaVu Sans 10 px), a clipped text, and a
    // language switch — the Flattener's text phase merges, strips the clip and resolves the switch
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1000" height="1000"/></clipPath></defs><g id="layer"><text id="h" xml:space="preserve" style="{DV};font-size:10px" x="0" y="0">Hello</text><text id="w" xml:space="preserve" style="{DV};font-size:10px" x="28.5" y="0">world</text><text id="clipped" style="{DV};font-size:10px" x="0" y="50" clip-path="url(#c)" mask="url(#c)">clipped</text><switch id="sw"><text id="de" systemLanguage="de" style="{DV};font-size:10px" x="0" y="80">Hallo</text><text id="en" style="{DV};font-size:10px" x="0" y="80">Hi</text></switch></g></svg>"#
    );
    let (s, msgs) = flatten(
        &svg,
        &[
            "--id=layer",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    assert!(msgs.iter().all(|m| m.starts_with("warning: ")), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let texts: Vec<String> = d
        .descendants()
        .filter(|n| n.has_tag_name("text"))
        .map(|n| {
            n.descendants()
                .filter(|c| c.is_text())
                .filter_map(|c| c.text())
                .collect::<String>()
        })
        .collect();
    assert!(
        texts
            .iter()
            .any(|t| t.split_whitespace().collect::<Vec<_>>().join(" ") == "Hello world"),
        "merged: {texts:?}"
    );
    let clipped = d
        .descendants()
        .find(|n| n.has_tag_name("text") && n.descendants().any(|c| c.text() == Some("clipped")))
        .expect("clipped text survives");
    assert_eq!(
        (clipped.attribute("clip-path"), clipped.attribute("mask")),
        (None, None),
        "text clips removed: {s}"
    );
    assert!(
        !has(&d, "sw") && !has(&d, "de"),
        "the switch is resolved to the matching child: {s}"
    );
    assert!(texts.iter().any(|t| t.trim() == "Hi"));
    assert!(!s.contains("<switch"));
}

#[test]
fn fixtext_off_leaves_text_untouched() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><text id="h" xml:space="preserve" style="{DV};font-size:10px;-inkscape-font-specification:x" x="0" y="0">Hello</text><text id="w" xml:space="preserve" style="{DV};font-size:10px" x="28.5" y="0">world</text></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--fixtext=false",
            "--setreplacement=true",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "h") && has(&d, "w"), "{s}");
    assert!(
        style_of(by_id(&d, "h"))
            .get("-inkscape-font-specification")
            .is_some(),
        "setreplacement is ANDed with fixtext"
    );
}

#[test]
fn overlapping_identical_paths_lose_the_one_underneath() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><path id="bottom" d="M0 0 L10 0 L10 5" style="stroke:#ff0000;fill:none"/><path id="top" d="M0 0 L10 0 L10 5" style="stroke:#ff0000;fill:none"/><path id="reversed" d="M10 5 L10 0 L0 0" style="stroke:#ff0000;fill:none"/><path id="other_style" d="M0 0 L10 0 L10 5" style="stroke:#ff0000;fill:none;stroke-width:3"/><path id="translucent_top" d="M20 0 L30 0" style="stroke:#0000ff;fill:none;stroke-opacity:0.5"/><path id="translucent_top2" d="M20 0 L30 0" style="stroke:#0000ff;fill:none;stroke-opacity:0.5"/><g id="wrap"><path id="moved" d="M0 0 L10 0 L10 5" transform="translate(40,0)" style="fill:#00ff00"/></g><path id="moved_dup" d="M40 0 L50 0 L50 5" style="fill:#00ff00"/></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--fixtext=false",
            "--revertpaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        !has(&d, "bottom") && !has(&d, "top"),
        "top ≡ reversed: `reversed` is the topmost of the three, it survives; the two below it go: {s}"
    );
    assert!(has(&d, "reversed"));
    assert!(
        has(&d, "other_style"),
        "a different stroke width is not a duplicate"
    );
    assert!(
        has(&d, "translucent_top") && has(&d, "translucent_top2"),
        "a translucent top element never deletes what is under it"
    );
    assert!(
        !has(&d, "moved") && has(&d, "moved_dup"),
        "duplicates are compared in root coordinates, through transforms: {s}"
    );
    assert!(!has(&d, "wrap"), "the emptied group went with it");
}

#[test]
fn white_background_rectangles_go_when_nothing_is_behind_them() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><rect id="bg" width="100" height="100" style="fill:#ffffff"/><path id="axis" d="M10 25 L90 25" style="stroke:#000000"/><rect id="cover" x="20" y="20" width="10" height="10" style="fill:#ffffff"/><rect id="alone" x="200" y="200" width="10" height="10" style="fill:#ffffff"/><rect id="stroked" x="300" y="300" width="10" height="10" style="fill:#ffffff;stroke:#000000"/><rect id="offwhite" x="400" y="400" width="10" height="10" style="fill:#fffffe"/></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "bg"), "nothing is behind the background: {s}");
    assert!(
        has(&d, "cover"),
        "the axis line is behind it → kept (it may be hiding something on purpose)"
    );
    assert!(!has(&d, "alone"));
    assert!(
        has(&d, "stroked") && has(&d, "offwhite"),
        "only unstroked pure-white fills are candidates"
    );
    assert!(has(&d, "axis"));
}

#[test]
fn the_shape_inside_target_of_a_text_is_never_a_duplicate_candidate() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><rect id="frame" width="10" height="10" style="fill:#000000"/><rect id="frame2" width="10" height="10" style="fill:#000000"/><text id="t" style="shape-inside:url(#frame2);{DV};font-size:3px" x="0" y="0"><tspan x="0" y="3">x</tspan></text></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--fixtext=false",
            "--revertpaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "frame") && has(&d, "frame2"),
        "frame2 is a text's shape-inside, so the pair is never considered: {s}"
    );
}

#[test]
fn moving_defs_skips_the_root_defs_and_moves_masks() {
    let svg = format!(
        r#"<svg {NS}><defs id="root"><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><g id="layer"><mask id="m"><rect width="1" height="1"/></mask><path id="p" d="M0 0h1" mask="url(#m)"/></g></svg>"#
    );
    // selecting the root <defs> itself must not move it into itself; the loose <mask> moves
    let (s, _) = flatten(
        &svg,
        &[
            "--id=root",
            "--id=m",
            "--id=p",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    let root = by_id(&d, "root");
    assert_eq!(
        root.parent().unwrap().tag_name().name(),
        "svg",
        "the root defs stays where it is: {s}"
    );
    let ids: Vec<Option<&str>> = kids(root).iter().map(|n| n.attribute("id")).collect();
    assert_eq!(ids, vec![Some("c"), Some("m")], "{s}");
    assert_eq!(by_id(&d, "p").attribute("mask"), Some("url(#m)"));
}

#[test]
fn a_switch_is_left_alone_when_no_kerning_option_is_on() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><switch id="sw"><text id="de" systemLanguage="de" style="{DV};font-size:10px" x="0" y="0">Hallo</text><text id="en" style="{DV};font-size:10px" x="0" y="0">Hi</text></switch></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=sw",
            "--splitdistant=false",
            "--mergenearby=false",
            "--removemanualkerning=false",
            "--mergesubsuper=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "sw") && has(&d, "de") && has(&d, "en"),
        "upstream resolves switches only on the kerning path: {s}"
    );
}

#[test]
fn duplicate_removal_and_white_rectangles_interact_through_the_working_set() {
    // the white rectangle's only backing element is the lower duplicate: once that is removed,
    // nothing is behind the rectangle and it goes too
    let svg = format!(
        r#"<svg {NS}><g id="layer"><path id="bottom" d="M0 0 L10 0 L10 5 Z" style="fill:#0000ff"/><rect id="white" x="2" y="1" width="3" height="3" style="fill:#ffffff"/><path id="top" d="M0 0 L10 0 L10 5 Z" style="fill:#0000ff"/></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &["--id=layer", "--fixtext=false", "--revertpaths=false"],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "bottom") && has(&d, "top"), "{s}");
    assert!(
        !has(&d, "white"),
        "its only backing element was the removed duplicate: {s}"
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "white"),
        "with the duplicate in place the rectangle has something behind it: {s}"
    );
}

#[test]
fn an_excluded_group_inside_the_selection_is_not_dissolved_but_its_contents_are_processed() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><g id="keepme" inkscape-scientific-flattenexclude="True"><g id="sub"><rect id="r1" width="1" height="1" style="fill:#000000"/></g></g><path id="p" d="M0 0h1"/></g></svg>"#
    );
    let (s, _) = flatten(
        &svg,
        &[
            "--id=layer",
            "--fixtext=false",
            "--revertpaths=false",
            "--removeduppaths=false",
            "--removerectw=false",
        ],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "keepme"),
        "the marked group itself is not ungrouped: {s}"
    );
    assert!(
        !has(&d, "sub"),
        "…but, as upstream, its descendants still are: {s}"
    );
    assert_eq!(
        by_id(&d, "r1").parent().unwrap().attribute("id"),
        Some("keepme")
    );
}
