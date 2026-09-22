mod support;

use std::ffi::OsString;

use support::with_vendored_fonts;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

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
fn unknown_ids_are_dropped_and_options_are_anded_with_fixtext() {
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
