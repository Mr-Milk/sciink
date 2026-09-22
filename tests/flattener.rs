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
#[ignore = "Task 3"]
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
#[ignore = "Task 3"]
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
