//! Slimmer (Plan 10): every default step is rendering-exact; the guard tests name what must stay.

mod support;

use std::collections::HashSet;
use std::ffi::OsString;

use sciink::dom::Doc;

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";
const XLINK: &str = "xmlns:xlink=\"http://www.w3.org/1999/xlink\"";
const INK: &str = "xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}

/// Runs the Slimmer with the default options plus `extra`; returns the document and the messages.
fn slim(svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec!["--tool=slimmer", "--tab=Options"];
    a.extend(extra);
    let out = sciink::run(&args(&a), svg.as_bytes()).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}

fn has(d: &roxmltree::Document, id: &str) -> bool {
    d.descendants().any(|n| n.attribute("id") == Some(id))
}

fn by_id<'a>(d: &'a roxmltree::Document<'a>, id: &str) -> roxmltree::Node<'a, 'a> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element with id {id}"))
}

/// Ids the document points at (`url(#x)` in any attribute, `#x` hrefs) that have no element.
fn dangling_references(d: &roxmltree::Document) -> HashSet<String> {
    let ids: HashSet<&str> = d.descendants().filter_map(|n| n.attribute("id")).collect();
    let mut out = HashSet::new();
    for n in d.descendants().filter(|n| n.is_element()) {
        for a in n.attributes() {
            let v = a.value();
            for piece in v.split("url(#").skip(1) {
                if let Some(id) = piece.split(')').next() {
                    let id = id.trim().trim_matches(['\'', '"']);
                    if !ids.contains(id) {
                        out.insert(id.to_string());
                    }
                }
            }
            if a.name() == "href" {
                if let Some(id) = v.strip_prefix('#') {
                    if !ids.contains(id) {
                        out.insert(id.to_string());
                    }
                }
            }
        }
    }
    out
}

/// Every `url(#x)` and `#x` href in the document has a target.
fn every_reference_resolves(d: &roxmltree::Document) -> bool {
    dangling_references(d).is_empty()
}

#[test]
fn duplicate_stylesheets_keep_the_last_copy_and_the_cascade() {
    let svg = format!(
        r#"<svg {NS}><style id="s1">*{{fill:red}}</style><g id="wrap"><style id="s2">*{{fill:blue}}</style></g><style id="s3">*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        !has(&d, "s1") && has(&d, "s2") && has(&d, "s3"),
        "A B A → B A: the first copy goes, the last stays: {s}"
    );
    assert!(
        msgs[0].contains("duplicate stylesheets removed: 1"),
        "{msgs:?}"
    );
    let fill = |svg: &str| {
        let d = Doc::parse(svg.as_bytes()).unwrap();
        d.computed(d.by_id("r").unwrap(), "fill")
    };
    assert_eq!(fill(&svg), "red", "the last rule wins before");
    assert_eq!(fill(&s), "red", "and after");
}

#[test]
fn a_lone_surviving_stylesheet_moves_to_the_root_and_at_rules_or_extra_attributes_block_dedup() {
    let svg = format!(
        r#"<svg {NS}><g id="fig1"><defs><style id="a">*{{fill:red}}</style></defs><rect id="r1" width="1" height="1"/></g><g id="fig2"><defs><style id="b">*{{fill:red}}</style></defs><rect id="r2" width="1" height="1"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "a") && has(&d, "b"), "{s}");
    let first = d
        .root_element()
        .children()
        .find(|c| c.is_element())
        .unwrap();
    assert_eq!(
        first.attribute("id"),
        Some("b"),
        "the lone sheet now leads the root, out of figure 2's <defs>: {s}"
    );
    assert!(
        msgs[0].contains("(1 kept, moved to the document root)"),
        "{msgs:?}"
    );
    let svg = format!(
        r#"<svg {NS}><style id="m">@import url(x.css); *{{fill:red}}</style><style id="n">@import url(x.css); *{{fill:red}}</style><style id="p" media="print">*{{fill:red}}</style><style id="q" media="print">*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["m", "n", "p", "q"] {
        assert!(
            has(&d, id),
            "{id}: an @-rule or an extra attribute blocks dedup: {s}"
        );
    }
    // The @import sheets make the whole stylesheet opaque (F5): nothing is dedup-eligible or
    // repositioned, but the report is no longer silent — it now explains why nothing ran.
    assert!(
        msgs[0].contains("rule(s) or @-rules sciink cannot analyse"),
        "{msgs:?}"
    );
    assert_eq!(s, svg, "nothing changed → byte-identical");
}

#[test]
fn the_report_lists_bytes_elements_and_each_step_and_says_nothing_to_do_on_a_clean_document() {
    let svg = format!(r#"<svg {NS}><rect id="r" width="1" height="1"/></svg>"#);
    let (s, msgs) = slim(&svg, &[]);
    assert_eq!(s, svg);
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
    let (_, msgs) = slim(&svg, &["--report=false"]);
    assert!(msgs.is_empty(), "silent without the report: {msgs:?}");
    let svg = format!(
        r#"<svg {NS}><style>*{{fill:red}}</style><style>*{{fill:red}}</style><rect id="r" width="1" height="1"/></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let lines: Vec<&str> = msgs[0].lines().collect();
    assert!(
        lines[0].starts_with("Slimmer: ")
            && lines[0].contains(" → ")
            && lines[0].ends_with("3 → 2 elements"),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].contains(&format!("{} B → {} B", svg.len(), s.len())),
        "{}",
        lines[0]
    );
    assert_eq!(lines[1], "  duplicate stylesheets removed: 1");
    assert_eq!(lines[2], "  empty or invisible elements removed: 0");
    assert_eq!(lines[3], "  wrapper groups collapsed: 0");
    assert_eq!(lines[4], "  unused definitions removed: 0");
    assert_eq!(lines[5], "  identical definitions merged: 0");
    assert_eq!(lines[6], "  coordinate precision: unchanged");
    assert_eq!(lines.len(), 7);
    let (_, msgs) = slim(&svg, &["--dedupstyles=false"]);
    assert_eq!(
        msgs,
        vec!["Slimmer: nothing to do".to_string()],
        "the step can be switched off"
    );
}

#[test]
fn empty_paths_zero_size_shapes_empty_text_and_empty_groups_are_removed() {
    let svg = format!(
        r#"<svg {NS}><g id="layer">
<path id="nod"/><path id="blank" d="  "/><path id="moveonly" d="M 1 2 M 3 4"/><path id="dot" d="M0 0L0 0" style="stroke:#000;stroke-linecap:round"/><path id="closed" d="M0 0Z" style="stroke:#000"/>
<rect id="flat" x="0" y="0" width="0" height="5"/><rect id="nowidth" y="0" height="5"/><circle id="r0" cx="1" cy="1" r="0"/><ellipse id="e0" cx="1" cy="1" rx="0" ry="2"/><line id="zl" x1="0" y1="0" x2="0" y2="0" style="stroke:#000"/>
<polyline id="nopts" points=" "/><rect id="ghost" width="5" height="5" style="fill:none;stroke:none"/><rect id="thin" width="5" height="5" style="fill:none;stroke:#000;stroke-width:0"/><rect id="attrnone" width="5" height="5" fill="none"/>
<text id="et"> </text><text id="wt" xml:space="preserve"> </text><text id="ok">a</text>
<g id="eg"/><g id="eg2"><g id="eg3"/></g><g id="cg"><!-- kept --></g>
<rect id="vis" width="5" height="5"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in [
        "nod", "blank", "moveonly", "flat", "nowidth", "r0", "e0", "nopts", "et", "eg", "eg2",
        "eg3",
    ] {
        assert!(!has(&d, id), "{id} paints nothing and should be gone: {s}");
    }
    for id in [
        "layer", "dot", "closed", "zl", "wt", "ok", "cg", "vis", "ghost", "thin", "attrnone",
    ] {
        assert!(
            has(&d, id),
            "{id} is invisible, not empty: kept without --removeinvisible: {s}"
        );
    }
    assert!(
        msgs[0].contains("empty or invisible elements removed: 12"),
        "{msgs:?}"
    );

    let (s, msgs) = slim(&svg, &["--removeinvisible=true"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in [
        "nod", "blank", "moveonly", "flat", "nowidth", "r0", "e0", "nopts", "et", "eg", "eg2",
        "eg3", "ghost", "thin", "attrnone",
    ] {
        assert!(
            !has(&d, id),
            "{id} should be gone with --removeinvisible=true: {s}"
        );
    }
    for id in ["layer", "dot", "closed", "zl", "wt", "ok", "cg", "vis"] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert!(
        msgs[0].contains("empty or invisible elements removed: 15"),
        "{msgs:?}"
    );
}

#[test]
fn emptied_groups_are_removed_only_with_removeempty() {
    let svg = format!(r#"<svg {NS}><g id="placeholder"/></svg>"#);
    let (s, _msgs) = slim(&svg, &["--removeempty=false"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "placeholder"),
        "emptied groups survive with --removeempty=false: {s}"
    );
    let (s, _msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(!has(&d, "placeholder"), "emptied groups go by default: {s}");
}

#[test]
fn hidden_objects_layers_labelled_spacers_switch_children_markers_filters_and_referenced_shapes_are_kept()
 {
    let svg = format!(
        r##"<svg {NS} {XLINK} {INK}>
<defs><marker id="m"><path d="M0 0h1"/></marker><filter id="f"><feFlood flood-color="red"/></filter><clipPath id="c"><rect id="clipr" width="1" height="1" style="fill:none;stroke:none"/></clipPath></defs>
<g id="hiddenlayer" inkscape:groupmode="layer" style="display:none"/>
<g id="emptylayer" inkscape:groupmode="layer"/>
<rect id="hidden" width="1" height="1" style="fill:none;stroke:none;display:none"/>
<rect id="spacer" inkscape:label="spacer" width="9" height="9" style="fill:none;stroke:none"/>
<switch><rect id="sw" width="0" height="0"/><text>fallback</text></switch>
<path id="marked" d="M0 0" style="marker-start:url(#m)"/>
<rect id="filtered" width="0" height="0" style="filter:url(#f)"/>
<use xlink:href="#target"/><rect id="target" width="0" height="0"/>
<g id="clipped" clip-path="url(#c)"><rect width="1" height="1"/></g>
<ellipse id="autor" cx="1" cy="1" ry="8" fill="red"/><ellipse id="negr" cx="1" cy="1" rx="-1" ry="8" fill="red"/>
</svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in [
        "hiddenlayer",
        "emptylayer",
        "hidden",
        "spacer",
        "sw",
        "marked",
        "filtered",
        "target",
        "clipr",
        "m",
        "f",
        "c",
        "clipped",
        "autor",
        "negr",
    ] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
    assert_eq!(s, svg, "byte-identical when nothing is removed");
}

#[test]
fn shapes_inside_cloned_groups_and_symbols_are_kept() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><g id="icon"><rect id="r1" x="2" y="2" width="14" height="14" fill="none"/></g><use xlink:href="#icon" x="20" stroke="blue" stroke-width="2"/><symbol id="s"><rect id="r2" width="1" height="1" fill="none"/></symbol><use xlink:href="#s"/></svg>"##
    );
    let (s, _msgs) = slim(&svg, &["--removeinvisible=true"]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "r1"),
        "r1 may render with the clone's own stroke: {s}"
    );
    assert!(has(&d, "r2"), "r2 only ever renders through a <use>: {s}");
}

#[test]
fn text_built_from_a_tref_is_kept() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><defs><text id="t">Hello</text></defs><text id="u" x="2" y="15"><tref xlink:href="#t"/></text></svg>"##
    );
    let (s, _msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "u"), "u's characters come from the tref: {s}");
    assert!(
        has(&d, "t"),
        "t is not pruned: the tref's xlink:href already counts as a reference: {s}"
    );
}

#[test]
fn wrapper_groups_with_only_an_id_collapse_and_the_child_keeps_its_place_and_gets_the_id() {
    let svg = format!(
        r#"<svg {NS}><g id="layer"><rect id="before" width="1" height="1"/><g id="patch_1">
  <path d="M0 0h1"/>
</g><g id="outer"><g id="inner"><rect id="kid" width="1" height="1"/></g></g><g id="two"><rect width="1" height="1"/><rect width="1" height="1"/></g><g id="styled" style="opacity:.5"><rect width="1" height="1"/></g><g id="xf" transform="translate(1)"><rect width="1" height="1"/></g><rect id="after" width="1" height="1"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    let kids: Vec<(String, String)> = by_id(&d, "layer")
        .children()
        .filter(|c| c.is_element())
        .map(|c| {
            (
                c.tag_name().name().to_string(),
                c.attribute("id").unwrap_or("").to_string(),
            )
        })
        .collect();
    let want = [
        ("rect", "before"),
        ("path", "patch_1"),
        ("rect", "kid"),
        ("g", "two"),
        ("g", "styled"),
        ("g", "xf"),
        ("rect", "after"),
    ];
    assert_eq!(
        kids,
        want.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<Vec<_>>(),
        "the wrapper's id moves to a child without one; nested wrappers collapse in one pass: {s}"
    );
    assert!(msgs[0].contains("wrapper groups collapsed: 3"), "{msgs:?}");
}

#[test]
fn wrapper_groups_are_kept_under_a_tag_rule_a_combinator_a_universal_opacity_rule_or_an_at_rule() {
    for sheet in [
        "g{fill:red}",
        "g > path{fill:red}",
        "*{opacity:.5}",
        "@media print{*{fill:red}} *{fill:red}",
    ] {
        let svg = format!(
            r#"<svg {NS}><style>{sheet}</style><g id="w"><path id="p" d="M0 0h1"/></g></svg>"#
        );
        let (s, msgs) = slim(&svg, &[]);
        let d = roxmltree::Document::parse(&s).unwrap();
        assert!(has(&d, "w"), "{sheet}: the wrapper stays: {s}");
        // The @-media case is caught by the opaque-stylesheet gate (F5), which skips wrappers
        // along with empty-element removal and merging and gives its own, differently worded
        // note; the other three are refused by collapse_wrappers' own universal-rule gate. Both
        // notes explain the block the same way.
        assert!(
            msgs.iter().any(|m| m.contains("kept: the stylesheet has")),
            "{sheet}: {msgs:?}"
        );
    }
    let svg = format!(
        r#"<svg {NS}><style>*{{stroke-linejoin:round}}</style><g id="w"><path id="p" d="M0 0h1"/></g></svg>"#
    );
    let (s, _) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        !has(&d, "w") && has(&d, "p"),
        "a lone * rule without group-level properties is safe: {s}"
    );
}

#[test]
fn layers_labelled_groups_use_targets_switch_children_and_title_wrappers_are_never_collapsed() {
    let svg = format!(
        r##"<svg {NS} {XLINK} {INK}>
<g id="layer" inkscape:groupmode="layer"><rect width="1" height="1"/></g>
<g id="named" inkscape:label="Panel A"><rect width="1" height="1"/></g>
<g id="cloned"><rect width="1" height="1"/></g><use xlink:href="#cloned"/>
<switch><g id="insw"><rect width="1" height="1"/></g></switch>
<g id="titled"><title>only a title</title></g>
<g id="commented"><!-- glyph group --><rect width="1" height="1"/></g>
<clipPath id="cp"><g id="inclip"><rect width="1" height="1"/></g></clipPath><rect clip-path="url(#cp)" width="1" height="1"/>
</svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in [
        "layer",
        "named",
        "cloned",
        "insw",
        "titled",
        "commented",
        "inclip",
        "cp",
    ] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert_eq!(msgs, vec!["Slimmer: nothing to do".to_string()]);
}

#[test]
fn unused_definitions_are_pruned_to_a_fixpoint_including_nested_defs_and_gradient_chains() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><defs id="root"><clipPath id="used"><rect width="1" height="1"/></clipPath><clipPath id="dead"><rect width="1" height="1"/></clipPath><linearGradient id="base"/><linearGradient id="chain" xlink:href="#base"/><linearGradient id="live" xlink:href="#base"/><linearGradient id="g1" xlink:href="#g2"/><linearGradient id="g2"/><path id="glyph" d="M0 0h1"/><rect id="loose" width="1" height="1"/></defs>
<g id="fig"><defs id="nested"><clipPath id="deadn"><rect width="1" height="1"/></clipPath></defs><rect id="r" clip-path="url(#used)" width="1" height="1" style="fill:url(#live)"/></g>
<mask id="strayfree"><rect width="1" height="1"/></mask>
</svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["root", "used", "base", "live", "r", "fig"] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    for id in [
        "dead",
        "chain",
        "g1",
        "g2",
        "glyph",
        "loose",
        "deadn",
        "nested",
        "strayfree",
    ] {
        assert!(!has(&d, id), "{id} is unused and should be gone: {s}");
    }
    assert!(
        msgs[0].contains("unused definitions removed: 8 in 3 round(s) (1 emptied containers)"),
        "g2 is only freed once g1 is gone, so it takes a second round; the third finds nothing: {msgs:?}"
    );
    assert!(every_reference_resolves(&d));
}

#[test]
fn referenced_definitions_style_glyph_script_children_text_paths_and_the_root_defs_survive_pruning()
{
    let svg = format!(
        r##"<svg {NS} {XLINK}><defs id="root"><style id="sheet">.x{{fill:red}}</style><script id="js">//</script><title id="tt">t</title><font id="fnt"><font-face id="ff"/><glyph id="gl"/></font><path id="curve" d="M0 0h1"/><marker id="mk"><path d="M0 0h1"/></marker><pattern id="pat"><rect width="1" height="1"/></pattern></defs>
<text><textPath xlink:href="#curve">on a curve</textPath></text>
<path d="M0 0h1" style="marker-end:url(#mk)"/><rect width="1" height="1" fill="url(#pat)"/>
<g id="emptyroot"><defs id="emptydefs"/></g></svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in [
        "root", "sheet", "js", "tt", "fnt", "ff", "gl", "curve", "mk", "pat",
    ] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert!(
        !has(&d, "emptydefs") && !has(&d, "emptyroot"),
        "an empty nested <defs> goes, then the group it emptied: {s}"
    );
    assert!(
        msgs[0].contains("unused definitions removed: 0 in 2 round(s) (2 emptied containers)"),
        "{msgs:?}"
    );
    assert!(
        has(&d, "root"),
        "the root <defs> is never removed even when empty"
    );
}

#[test]
fn stylesheets_inside_unused_definitions_are_kept() {
    let svg = format!(
        r#"<svg {NS}><style>.b{{fill:blue}}</style><defs><symbol id="s"><style>.a{{fill:red}}</style></symbol></defs><rect id="r" class="a" width="20" height="20"/></svg>"#
    );
    let (s, _msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "s"),
        "the symbol holding a document-wide <style> is never unused: {s}"
    );
    let after = Doc::parse(s.as_bytes()).unwrap();
    assert_eq!(
        after.computed(after.by_id("r").unwrap(), "fill"),
        "red",
        "the nested sheet's rule still applies"
    );

    let svg2 = format!(
        r#"<svg {NS}><style>rect{{fill:red}}</style><style>.b{{fill:blue}}</style><defs><symbol id="s"><style>rect{{fill:red}}</style></symbol></defs><rect id="r" width="20" height="20"/></svg>"#
    );
    let (s2, _msgs2) = slim(&svg2, &[]);
    let after2 = Doc::parse(s2.as_bytes()).unwrap();
    assert_eq!(
        after2.computed(after2.by_id("r").unwrap(), "fill"),
        "red",
        "whichever copy of the rule survives, r is still red: {s2}"
    );
}

#[test]
fn an_opaque_stylesheet_keeps_empty_elements_wrappers_and_definitions_and_says_so() {
    // (i) an unsupported pseudo-class selector: the wrapper group stays
    let svg = format!(
        r#"<svg {NS}><style>path:first-child{{fill:red}}</style><rect width="1" height="1"/><g id="w"><path id="p" d="M20 0h20v20h-20z"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "w"), "{s}");
    assert!(
        msgs[0].contains("rule(s) or @-rules sciink cannot analyse"),
        "{msgs:?}"
    );

    // (ii) an unsupported pseudo-class selector: empty-element removal is skipped
    let svg = format!(
        r#"<svg {NS}><style>rect:first-child{{fill:red}}</style><g class="k"><path id="e" d=""/><rect id="r" width="20" height="20"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "e"), "{s}");
    assert!(
        msgs[0].contains("rule(s) or @-rules sciink cannot analyse"),
        "{msgs:?}"
    );

    // (iii) an unsupported attribute selector: definition merging is skipped
    let svg = format!(
        r##"<svg {NS} {XLINK}><style>[id="p2"] rect{{fill:red}}</style><defs><pattern id="p1"><rect width="1" height="1"/></pattern><pattern id="p2"><rect width="1" height="1"/></pattern></defs><rect fill="url(#p1)" width="10" height="10"/><rect fill="url(#p2)" width="10" height="10"/></svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "p1") && has(&d, "p2"), "{s}");
    assert!(
        msgs[0].contains("rule(s) or @-rules sciink cannot analyse"),
        "{msgs:?}"
    );

    // (iv) an @ block, nothing else: the wrapper group stays
    let svg = format!(
        r#"<svg {NS}><style>@media all{{g > path{{fill:red}}}}</style><g id="w"><path d="M0 0h1"/></g></svg>"#
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(has(&d, "w"), "{s}");
    assert!(
        msgs[0].contains("rule(s) or @-rules sciink cannot analyse"),
        "{msgs:?}"
    );
}

#[test]
fn swatches_and_color_profiles_are_never_pruned() {
    let svg = format!(
        r##"<svg {NS} {INK}><defs><linearGradient id="sw" inkscape:swatch="solid"/><color-profile id="cp"/></defs></svg>"##
    );
    let (s, _msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    assert!(
        has(&d, "sw") && has(&d, "cp"),
        "swatches are referenced by name and color-profile is a kept defs child: {s}"
    );
}

#[test]
fn identical_clip_paths_merge_and_every_reference_form_is_repointed() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><defs><clipPath id="c1"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="c2"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="c3"><rect x="0" y="0" width="10" height="10"/></clipPath><clipPath id="c10"><rect x="1" y="0" width="10" height="10"/></clipPath><linearGradient id="g1"><stop offset="0" stop-color="red"/></linearGradient><linearGradient id="g2" xlink:href="#g1"/><linearGradient id="g3" xlink:href="#g1"/></defs>
<rect id="z" clip-path="url(#c1)" width="1" height="1"/><rect id="a" clip-path="url(#c2)" width="1" height="1"/><rect id="b" style="clip-path:url( '#c3' );fill:url(#g3)" width="1" height="1"/><rect id="k" clip-path="url(#c10)" width="1" height="1"/><rect id="f" fill="url(#g2)" width="1" height="1"/></svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["c1", "c10", "g1", "g2"] {
        assert!(has(&d, id), "{id} survives: {s}");
    }
    for id in ["c2", "c3", "g3"] {
        assert!(!has(&d, id), "{id} is a copy of an earlier definition: {s}");
    }
    assert_eq!(by_id(&d, "a").attribute("clip-path"), Some("url(#c1)"));
    let b = by_id(&d, "b").attribute("style").unwrap();
    assert!(
        b.contains("clip-path:url(#c1)") && b.contains("fill:url(#g2)"),
        "quoted and inline references are repointed: {b}"
    );
    assert_eq!(
        by_id(&d, "k").attribute("clip-path"),
        Some("url(#c10)"),
        "c10 is not c1"
    );
    assert!(
        msgs[0].contains("identical definitions merged: 3 (2 attributes repointed)"),
        "{msgs:?}"
    );
    assert!(every_reference_resolves(&d), "{s}");
}

#[test]
fn definitions_with_referenced_inner_ids_style_mentions_or_duplicate_ids_are_not_merged() {
    let svg = format!(
        r##"<svg {NS} {XLINK}><style>#c2 rect{{fill:red}}</style><defs><clipPath id="c1"><rect width="1" height="1"/></clipPath><clipPath id="c2"><rect width="1" height="1"/></clipPath><clipPath id="c3"><rect id="inner" width="1" height="1"/></clipPath><clipPath id="c4"><rect width="1" height="1"/></clipPath><clipPath id="c4"><rect width="1" height="1"/></clipPath></defs>
<rect clip-path="url(#c1)" width="1" height="1"/><rect clip-path="url(#c2)" width="1" height="1"/><rect clip-path="url(#c3)" width="1" height="1"/><rect clip-path="url(#c4)" width="1" height="1"/><use xlink:href="#inner"/></svg>"##
    );
    let (s, msgs) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["c1", "c2", "c3", "c4"] {
        assert!(has(&d, id), "{id} must stay: {s}");
    }
    assert_eq!(
        d.descendants()
            .filter(|n| n.attribute("id") == Some("c4"))
            .count(),
        2,
        "duplicate ids are left alone"
    );
    assert_eq!(
        msgs,
        vec!["Slimmer: nothing to do".to_string()],
        "nothing merged, nothing else to do (no wrapper groups, so no stylesheet note): {msgs:?}"
    );
}

#[test]
fn definitions_in_different_style_contexts_are_not_merged() {
    let svg = format!(
        r#"<svg {NS}><defs><clipPath id="a"><rect width="1" height="1"/></clipPath></defs><g style="clip-rule:evenodd"><defs><clipPath id="b"><rect width="1" height="1"/></clipPath></defs></g><defs><clipPath id="c"><rect width="1" height="1" style="clip-rule:evenodd"/></clipPath></defs>
<rect clip-path="url(#a)" width="1" height="1"/><rect clip-path="url(#b)" width="1" height="1"/><rect clip-path="url(#c)" width="1" height="1"/></svg>"#
    );
    let (s, _) = slim(&svg, &[]);
    let d = roxmltree::Document::parse(&s).unwrap();
    for id in ["a", "b", "c"] {
        assert!(
            has(&d, id),
            "{id}: inherited or inline clip-rule makes it a different clip: {s}"
        );
    }
}

#[test]
fn precision_rounds_significant_digits_and_keeps_integers_flags_exponents_and_leading_dots() {
    use sciink::tools::slimmer::round_numbers;
    let (s, k) = round_numbers(
        "M .5 1e-5 -0.1234567 123456.789 L1-2 a1 1 0 01.5.5 z",
        6,
        true,
    )
    .unwrap();
    assert_eq!(s, "M .5 1e-5 -0.123457 123457 L1-2 a1 1 0 01.5.5 z");
    assert_eq!(
        k, 2,
        "only the two long numbers changed; .5 and 1e-5 would not get shorter"
    );
    assert_eq!(
        round_numbers("10.000001 20", 4, false).unwrap(),
        ("10 20".to_string(), 1)
    );
    assert_eq!(
        round_numbers("1em", 6, false),
        None,
        "units: leave the value alone"
    );
    assert_eq!(round_numbers("50%", 6, false), None);
    assert_eq!(
        round_numbers("M0 0", 6, true).unwrap(),
        ("M0 0".to_string(), 0)
    );
    assert_eq!(
        round_numbers("-0.0", 6, false).unwrap(),
        ("0".to_string(), 1)
    );
    assert_eq!(
        round_numbers("a 5 5 0 1 0 10 0", 4, true).unwrap(),
        ("a 5 5 0 1 0 10 0".to_string(), 0),
        "spaced arc flags"
    );
}

#[test]
fn precision_is_off_by_default_and_never_touches_transform_viewbox_style_or_text_positions() {
    let svg = format!(
        r#"<svg {NS} viewBox="0 0 10.123456789 10"><g transform="translate(0.123456789)"><path id="p" d="M0.123456789 0h1" style="stroke-width:0.123456789"/><rect id="r" x="0.123456789" width="1" height="1"/><text id="t" x="0.123456789" y="1">a</text></g></svg>"#
    );
    let (s0, msgs0) = slim(&svg, &[]);
    assert_eq!(s0, svg, "the default keeps every digit: {msgs0:?}");
    let (s, msgs) = slim(&svg, &["--precision=6"]);
    assert!(
        s.contains(r#"d="M0.123457 0h1""#) && s.contains(r#"x="0.123457" width"#),
        "{s}"
    );
    for kept in [
        r#"viewBox="0 0 10.123456789 10""#,
        "translate(0.123456789)",
        "stroke-width:0.123456789",
        r#"<text id="t" x="0.123456789""#,
    ] {
        assert!(s.contains(kept), "{kept} is untouched: {s}");
    }
    assert!(
        msgs[0].contains("coordinate precision: 6 significant digits, 2 numbers changed"),
        "{msgs:?}"
    );
}

#[test]
fn big_doc_loses_style_rules_minus_one_sheets_merges_its_identical_clips_and_renders_identically() {
    let big = support::BigDoc::default();
    let svg = big.svg();
    let (s, msgs) = slim(&svg, &[]);
    assert!(
        msgs[0].contains(&format!(
            "duplicate stylesheets removed: {}",
            big.style_rules - 1
        )),
        "{msgs:?}"
    );
    assert!(
        msgs[0].contains(&format!(
            "identical definitions merged: {}",
            big.figures - 1
        )),
        "every figure's clip is the same rectangle: {msgs:?}"
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    assert_eq!(
        d.descendants().filter(|n| n.has_tag_name("style")).count(),
        1
    );
    assert_eq!(
        d.descendants()
            .filter(|n| n.has_tag_name("clipPath"))
            .count(),
        1
    );
    assert!(every_reference_resolves(&d));
    assert!(s.len() < svg.len(), "{} → {}", svg.len(), s.len());
    let diff = support::pixel_diff_fraction(
        &support::render_png(svg.as_bytes(), 1500),
        &support::render_png(s.as_bytes(), 1500),
        32,
    );
    assert_eq!(diff, 0.0, "rendering-exact by construction");
}

/// Renders `f` before and after a Slimmer run with `extra` options; the pixel-diff fraction must
/// not exceed `max`, and every reference in the output must resolve.
fn check_exact(f: &std::path::Path, extra: &[&str], max: f64) {
    let input = std::fs::read(f).unwrap();
    let mut a = vec!["--tool=slimmer", "--tab=Options"];
    a.extend(extra);
    let out = sciink::run(&args(&a), &input).unwrap().svg;
    let d = support::pixel_diff_fraction(
        &support::render_png(&input, 1500),
        &support::render_png(&out, 1500),
        32,
    );
    eprintln!(
        "{}: pixel diff {:.5} % with {extra:?}",
        f.display(),
        d * 100.0
    );
    assert!(d <= max, "{}: {d} > {max}", f.display());
    let before = roxmltree::Document::parse(std::str::from_utf8(&input).unwrap()).unwrap();
    let after = roxmltree::Document::parse(std::str::from_utf8(&out).unwrap()).unwrap();
    let introduced: Vec<String> = dangling_references(&after)
        .difference(&dangling_references(&before))
        .cloned()
        .collect();
    assert!(
        introduced.is_empty(),
        "{}: the Slimmer introduced dangling references {introduced:?} (an upstream fixture may carry its own; those are not ours)",
        f.display()
    );
}

#[test]
fn default_slimmer_renders_every_upstream_fixture_identically() {
    for f in support::upstream_svgs() {
        if f.file_name().is_some_and(|n| n == "Acid_tests.svg") {
            continue; // the 5.9 MB one is opt-in below
        }
        check_exact(&f, &[], 0.0);
    }
}

/// Run: `cargo test --release --test slimmer -- --ignored --nocapture`
#[test]
#[ignore = "5.9 MB fixture; slow in a debug build"]
fn default_slimmer_renders_acid_tests_identically() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    check_exact(&dir.join("svg/Acid_tests.svg"), &[], 0.0);
}

#[test]
fn precision_six_changes_at_most_a_tenth_of_a_percent_of_pixels_on_upstream_fixtures() {
    for f in support::upstream_svgs() {
        if f.file_name().is_some_and(|n| n == "Acid_tests.svg") {
            continue;
        }
        check_exact(&f, &["--precision=6"], 0.001);
    }
}
