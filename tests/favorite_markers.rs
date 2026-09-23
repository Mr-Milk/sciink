mod support;

use std::ffi::OsString;
use std::path::PathBuf;

use sciink::tools::favorite_markers::{MarkerData, Store, Template, store_path};

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn by_id<'a, 'i>(d: &'a roxmltree::Document<'i>, id: &str) -> roxmltree::Node<'a, 'i> {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn tmp_store(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sciink-fm-{}-{name}.svg", std::process::id()))
}
fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn builtins_are_upstreams_three_templates() {
    let s = Store::builtins();
    assert_eq!(
        s.templates(),
        vec![
            "Arrow".to_string(),
            "Triangle".to_string(),
            "Distance".to_string()
        ]
    );
    let arrow = s.get("Arrow").unwrap();
    let start = arrow[0].as_ref().unwrap();
    assert_eq!(
        start.attrs,
        kv(&[
            ("style", "overflow:visible"),
            ("refX", "0.0"),
            ("refY", "0.0"),
            ("orient", "auto"),
            ("inkscape:stockid", "Arrow2Mstart"),
            ("inkscape:isstock", "true")
        ])
    );
    assert_eq!(start.paths.len(), 1);
    assert_eq!(
        start.paths[0][0],
        ("transform".to_string(), "scale(0.6, 0.6)".to_string())
    );
    assert!(
        start.paths[0].iter().all(|(k, _)| k != "id"),
        "ids are never stored"
    );
    let tri = s.get("Triangle").unwrap();
    assert_eq!(
        tri[1].as_ref().unwrap().paths[0][0],
        ("transform".to_string(), "scale(0.4, 0.4)".to_string())
    );
    assert_eq!(
        tri[0]
            .as_ref()
            .unwrap()
            .attrs
            .iter()
            .find(|(k, _)| k == "inkscape:stockid")
            .unwrap()
            .1,
        "TriangleInM"
    );
    let dist = s.get("Distance").unwrap();
    assert_eq!(
        dist[0].as_ref().unwrap().paths.len(),
        3,
        "the distance start marker has three paths"
    );
    assert_eq!(
        dist[1]
            .as_ref()
            .unwrap()
            .attrs
            .iter()
            .find(|(k, _)| k == "inkscape:stockid")
            .unwrap()
            .1,
        "StopL"
    );
    assert_eq!(
        dist[2].as_ref().unwrap().paths[1][0].1,
        "M 0,0 L -13,4 L -9,0 -13,-4 L 0,0 z "
    );
    assert!(s.get("Nope").is_none());
}

#[test]
fn store_round_trips_set_replace_remove_and_seeds_from_builtins() {
    let path = tmp_store("roundtrip");
    let _ = std::fs::remove_file(&path);
    let mut s = Store::load(&path).unwrap();
    assert_eq!(
        s.templates().len(),
        3,
        "a missing file yields the built-ins"
    );
    let md = MarkerData {
        attrs: kv(&[("orient", "auto"), ("refX", "1"), ("id", "dropped")]),
        paths: vec![
            kv(&[
                ("d", "M0,0 L1,1"),
                ("style", "fill:#f00"),
                ("id", "dropped"),
            ]),
            kv(&[("d", "M0,0 h2")]),
        ],
    };
    let t: Template = [
        Some(md.clone()),
        None,
        Some(MarkerData {
            attrs: vec![],
            paths: vec![kv(&[("d", "M0,0 v2")])],
        }),
    ];
    s.set("Mine", &t);
    s.save(&path).unwrap();
    let s2 = Store::load(&path).unwrap();
    assert_eq!(
        s2.templates(),
        vec![
            "Arrow".to_string(),
            "Triangle".to_string(),
            "Distance".to_string(),
            "Mine".to_string()
        ]
    );
    let got = s2.get("Mine").unwrap();
    assert_eq!(
        got[0].as_ref().unwrap().attrs,
        kv(&[("orient", "auto"), ("refX", "1")]),
        "id dropped, order kept"
    );
    assert_eq!(
        got[0].as_ref().unwrap().paths,
        vec![
            kv(&[("d", "M0,0 L1,1"), ("style", "fill:#f00")]),
            kv(&[("d", "M0,0 h2")])
        ]
    );
    assert!(got[1].is_none());
    assert_eq!(
        got[2].as_ref().unwrap().paths,
        vec![kv(&[("d", "M0,0 v2")])]
    );
    // replacing keeps one copy and the original position in the list
    let mut s3 = s2;
    s3.set("Mine", &[None, Some(md), None]);
    assert_eq!(s3.templates().len(), 4);
    let got = s3.get("Mine").unwrap();
    assert!(got[0].is_none() && got[1].is_some() && got[2].is_none());
    assert!(s3.remove("Mine"));
    assert!(!s3.remove("Mine"), "already gone");
    assert!(s3.get("Mine").is_none());
    assert_eq!(s3.templates().len(), 3);
    s3.save(&path).unwrap();
    assert_eq!(Store::load(&path).unwrap().templates().len(), 3);
    std::fs::remove_file(&path).unwrap();
    // an unreadable store is an error, not silently the built-ins
    let dir = tmp_store("dir-not-file");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(Store::load(&dir).is_err());
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn save_creates_the_parent_directory_and_the_store_is_svg() {
    let path = tmp_store("nested")
        .with_extension("")
        .join("deeper")
        .join("favorite_markers.svg");
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    Store::builtins().save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let d = roxmltree::Document::parse(&text).unwrap();
    assert_eq!(d.root_element().tag_name().name(), "svg");
    assert_eq!(
        d.descendants().filter(|n| n.has_tag_name("marker")).count(),
        9,
        "3 templates × 3 positions"
    );
    std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
}

#[test]
fn store_path_prefers_the_override() {
    let p = PathBuf::from("/tmp/x/fm.svg");
    assert_eq!(store_path(Some(&p)), p);
    assert!(store_path(None).ends_with("favorite_markers.svg"));
}

fn fm(svg: &str, store: &std::path::Path, extra: &[&str]) -> Result<(String, Vec<String>), String> {
    let store = format!("--store={}", store.display());
    let mut a = vec!["--tool=favorite-markers", store.as_str()];
    a.extend(extra);
    let out = sciink::run(&args(&a), svg.as_bytes())?;
    Ok((String::from_utf8(out.svg).unwrap(), out.messages))
}
fn ok(svg: &str, store: &std::path::Path, extra: &[&str]) -> (String, Vec<String>) {
    fm(svg, store, extra).unwrap_or_else(|e| panic!("favorite-markers failed: {e}"))
}
fn style_of(n: roxmltree::Node) -> sciink::style::Style {
    n.attribute("style")
        .map(sciink::style::Style::parse)
        .unwrap_or_default()
}
fn markers<'a, 'i>(d: &'a roxmltree::Document<'i>) -> Vec<roxmltree::Node<'a, 'i>> {
    d.descendants()
        .filter(|n| n.has_tag_name("marker"))
        .collect()
}
const SHAPES: &str = r##"<g id="g"><path id="p" d="M0,0 L10,0" style="fill:none;stroke:#000"/><rect id="r" x="0" y="5" width="4" height="4" style="stroke:#000;marker-mid:url(#old)"/><text id="t" style="font-size:4px">no markers</text></g><line id="l" x1="0" y1="20" x2="10" y2="20" style="stroke:#00f"/>"##;

#[test]
fn apply_creates_one_marker_per_position_and_size_and_reuses_it() {
    let store = tmp_store("apply");
    let _ = std::fs::remove_file(&store);
    let svg = format!(r#"<svg {NS}>{SHAPES}</svg>"#);
    // Triangle (index 1), start + end, size 100
    let (s, msgs) = ok(
        &svg,
        &store,
        &[
            "--tab=markers",
            "--template=1",
            "--smarker=true",
            "--emarker=true",
            "--id=g",
            "--id=l",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let mk = markers(&d);
    assert_eq!(
        mk.len(),
        2,
        "one start and one end marker, shared by the three shapes"
    );
    let start = mk
        .iter()
        .find(|m| m.attribute("id").unwrap().contains("FMTrianglestart"))
        .unwrap();
    let end = mk
        .iter()
        .find(|m| m.attribute("id").unwrap().contains("FMTriangleend"))
        .unwrap();
    assert_eq!(
        start.attribute(("http://www.inkscape.org/namespaces/inkscape", "stockid")),
        Some("TriangleInM")
    );
    assert_eq!(start.attribute("orient"), Some("auto"));
    assert_eq!(
        start.parent().unwrap().tag_name().name(),
        "defs",
        "markers live in the root defs"
    );
    let g = start.first_element_child().unwrap();
    assert_eq!(g.tag_name().name(), "g");
    assert_eq!(
        g.attribute("transform"),
        None,
        "size 100 % is the identity: no attribute"
    );
    let p = g.first_element_child().unwrap();
    assert_eq!(p.attribute("transform"), Some("scale(-0.4, -0.4)"));
    assert_eq!(
        p.attribute("d"),
        Some("M 5.77,0.0 L -2.88,5.0 L -2.88,-5.0 L 5.77,0.0 z ")
    );
    for id in ["p", "r", "l"] {
        let st = style_of(by_id(&d, id));
        assert_eq!(
            st.get("marker-start"),
            Some(format!("url(#{})", start.attribute("id").unwrap()).as_str()),
            "{id}"
        );
        assert_eq!(
            st.get("marker-end"),
            Some(format!("url(#{})", end.attribute("id").unwrap()).as_str()),
            "{id}"
        );
        assert_eq!(
            st.get("marker-mid"),
            None,
            "{id}: an unchecked position is removed"
        );
    }
    assert_eq!(
        style_of(by_id(&d, "t")).get("marker-start"),
        None,
        "text takes no markers"
    );
    // a second run at 50 % adds new markers with the scale; a third run at 50 % reuses them
    let (s2, _) = ok(
        &s,
        &store,
        &[
            "--tab=markers",
            "--template=1",
            "--smarker=true",
            "--size=50",
            "--id=p",
        ],
    );
    let d2 = roxmltree::Document::parse(&s2).unwrap();
    assert_eq!(markers(&d2).len(), 3);
    let half = markers(&d2)
        .into_iter()
        .find(|m| {
            m.first_element_child()
                .unwrap()
                .attribute("transform")
                .is_some()
        })
        .unwrap();
    assert_eq!(
        half.first_element_child().unwrap().attribute("transform"),
        Some("scale(0.5,0.5)")
    );
    assert_eq!(
        style_of(by_id(&d2, "p")).get("marker-start"),
        Some(format!("url(#{})", half.attribute("id").unwrap()).as_str())
    );
    assert_eq!(
        style_of(by_id(&d2, "p")).get("marker-end"),
        None,
        "end unchecked this time: removed"
    );
    let (s3, _) = ok(
        &s2,
        &store,
        &[
            "--tab=markers",
            "--template=1",
            "--smarker=true",
            "--size=50",
            "--id=r",
        ],
    );
    assert_eq!(
        markers(&roxmltree::Document::parse(&s3).unwrap()).len(),
        3,
        "reused"
    );
    // an empty selection is a message, not an error
    let (_, msgs) = ok(&svg, &store, &["--tab=markers", "--template=0"]);
    assert_eq!(msgs, vec!["favorite-markers: nothing selected".to_string()]);
}

#[test]
fn template_selection_and_errors() {
    let store = tmp_store("errors");
    let _ = std::fs::remove_file(&store);
    let svg = format!(r#"<svg {NS}>{SHAPES}</svg>"#);
    let e = fm(
        &svg,
        &store,
        &["--tab=markers", "--template=3", "--smarker=true", "--id=p"],
    )
    .unwrap_err();
    assert!(e.contains("template name"), "custom without a name: {e}");
    let e = fm(
        &svg,
        &store,
        &[
            "--tab=markers",
            "--template=3",
            "--custom_name=Nope",
            "--smarker=true",
            "--id=p",
        ],
    )
    .unwrap_err();
    assert!(
        e.contains("'Nope'") && e.contains("Arrow, Triangle, Distance"),
        "{e}"
    );
    let e = fm(
        &svg,
        &store,
        &["--tab=markers", "--template=7", "--smarker=true", "--id=p"],
    )
    .unwrap_err();
    assert!(e.contains("template"), "{e}");
    // Arrow (0) and Distance (2) resolve to the built-ins; Distance start has three paths
    let (s, _) = ok(
        &svg,
        &store,
        &["--tab=markers", "--template=2", "--smarker=true", "--id=p"],
    );
    let d = roxmltree::Document::parse(&s).unwrap();
    let mk = &markers(&d)[0];
    assert!(mk.attribute("id").unwrap().starts_with("FMDistancestart"));
    assert_eq!(
        mk.first_element_child()
            .unwrap()
            .children()
            .filter(|c| c.has_tag_name("path"))
            .count(),
        3
    );
    let e = fm(
        &svg,
        &store,
        &[
            "--tab=addremove",
            "--addt=true",
            "--template_name=X",
            "--id=t",
        ],
    )
    .unwrap_err();
    assert!(e.contains("Select a path"), "{e}");
    let e = fm(&svg, &store, &["--tab=addremove", "--addt=true", "--id=p"]).unwrap_err();
    assert!(e.contains("name"), "{e}");
}

#[test]
fn add_remove_and_list_round_trip_through_the_store() {
    let store = tmp_store("addremove");
    let _ = std::fs::remove_file(&store);
    let svg = format!(
        r##"<svg {NS}><defs><marker id="m1" orient="auto" refX="1"><g transform="scale(2)"><path id="mp" d="M0,0 L1,1" style="fill:#f00"/></g></marker><marker id="m2"><path d="M0,0 h2"/><rect width="1" height="1"/></marker></defs><path id="p" d="M0,0 L10,0" style="stroke:#000;marker-start:url(#m1);marker-end:url(#m2)"/></svg>"##
    );
    let (s, msgs) = ok(
        &svg,
        &store,
        &[
            "--tab=addremove",
            "--addt=true",
            "--template_name= My Arrows ",
            "--id=p",
        ],
    );
    assert_eq!(msgs, vec!["Templates successfully updated!".to_string()]);
    assert_eq!(s, svg, "the add/remove page never edits the document");
    let st = Store::load(&store).unwrap();
    assert_eq!(
        st.templates().last().map(String::as_str),
        Some("My Arrows"),
        "trimmed"
    );
    let t = st.get("My Arrows").unwrap();
    assert_eq!(
        t[0].as_ref().unwrap().attrs,
        kv(&[("orient", "auto"), ("refX", "1")])
    );
    assert_eq!(
        t[0].as_ref().unwrap().paths,
        vec![kv(&[("d", "M0,0 L1,1"), ("style", "fill:#f00")])],
        "paths from the first-child group, ids dropped"
    );
    assert!(t[1].is_none(), "no mid marker on the source");
    assert_eq!(
        t[2].as_ref().unwrap().paths,
        vec![kv(&[("d", "M0,0 h2")])],
        "paths directly under the marker; the rect is not a path"
    );
    // apply the new template elsewhere: custom name, whitespace removed from the marker id
    let target = format!(r#"<svg {NS}><path id="q" d="M0,0 L5,5" style="stroke:#000"/></svg>"#);
    let (s2, _) = ok(
        &target,
        &store,
        &[
            "--tab=markers",
            "--template=3",
            "--custom_name=My Arrows",
            "--smarker=true",
            "--size=200",
            "--id=q",
        ],
    );
    let d = roxmltree::Document::parse(&s2).unwrap();
    let mk = &markers(&d)[0];
    assert!(
        mk.attribute("id").unwrap().starts_with("FMMyArrowsstart"),
        "{}",
        mk.attribute("id").unwrap()
    );
    assert_eq!(mk.attribute("refX"), Some("1"));
    assert_eq!(
        mk.first_element_child().unwrap().attribute("transform"),
        Some("scale(2,2)")
    );
    // list, remove, remove again
    let (_, msgs) = ok(&svg, &store, &["--tab=addremove", "--list=true"]);
    assert_eq!(
        msgs,
        vec![
            "favorite-markers: stored templates: Arrow, Triangle, Distance, My Arrows".to_string()
        ]
    );
    let (_, msgs) = ok(
        &svg,
        &store,
        &["--tab=addremove", "--remt=true", "--template_rem=My Arrows"],
    );
    assert_eq!(msgs, vec!["Templates successfully updated!".to_string()]);
    assert!(Store::load(&store).unwrap().get("My Arrows").is_none());
    let (_, msgs) = ok(
        &svg,
        &store,
        &["--tab=addremove", "--remt=true", "--template_rem=My Arrows"],
    );
    assert_eq!(
        msgs.len(),
        1,
        "a failed removal yields the warning alone, no success message: {msgs:?}"
    );
    assert!(
        msgs[0].starts_with("warning: ") && msgs[0].contains("My Arrows"),
        "{}",
        msgs[0]
    );
    std::fs::remove_file(&store).unwrap();
}

#[test]
fn prefixed_attributes_travel_with_their_namespace_or_are_dropped() {
    let store = tmp_store("prefixes");
    let _ = std::fs::remove_file(&store);
    // the source declares sodipodi and an unknown prefix; the marker's path uses both
    let svg = format!(
        r##"<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd" xmlns:foo="urn:foo"><defs><marker id="m" orient="auto"><path d="M0,0 L1,1" sodipodi:nodetypes="cc" foo:bar="1"/></marker></defs><path id="p" d="M0,0 L10,0" style="stroke:#000;marker-start:url(#m)"/></svg>"##
    );
    ok(
        &svg,
        &store,
        &[
            "--tab=addremove",
            "--addt=true",
            "--template_name=Pfx",
            "--id=p",
        ],
    );
    let text = std::fs::read_to_string(&store).unwrap();
    let d = roxmltree::Document::parse(&text).expect("the store declares every prefix it uses");
    let path = d
        .descendants()
        .find(|n| {
            n.has_tag_name("path")
                && n.attribute((
                    "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
                    "nodetypes",
                ))
                .is_some()
        })
        .expect("sodipodi:nodetypes kept, its namespace declared on the store root");
    assert!(
        path.attributes().all(|a| a.name() != "bar"),
        "the unknown prefix's attribute is dropped"
    );
    // applying into a document that declares neither prefix declares sodipodi there
    let target = format!(r#"<svg {NS}><path id="q" d="M0,0 L5,5" style="stroke:#000"/></svg>"#);
    let (s, _) = ok(
        &target,
        &store,
        &[
            "--tab=markers",
            "--template=3",
            "--custom_name=Pfx",
            "--smarker=true",
            "--id=q",
        ],
    );
    let d = roxmltree::Document::parse(&s).expect("the output declares every prefix it uses");
    assert!(
        d.root_element()
            .namespaces()
            .any(|ns| ns.name() == Some("sodipodi")
                && ns.uri() == "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"),
        "the root declares sodipodi"
    );
    assert!(d.descendants().any(|n| n.has_tag_name("path")
        && n.attribute((
            "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
            "nodetypes"
        )) == Some("cc")));
    std::fs::remove_file(&store).unwrap();
}

#[test]
fn add_stores_the_first_selected_shape_not_the_first_in_document_order() {
    let store = tmp_store("order");
    let _ = std::fs::remove_file(&store);
    let svg = format!(
        r##"<svg {NS}><defs><marker id="m1"><path d="M0,0 h1"/></marker><marker id="m2"><path d="M0,0 v1"/></marker></defs><path id="a" d="M0,0 L1,0" style="marker-start:url(#m1)"/><path id="b" d="M0,1 L1,1" style="marker-start:url(#m2)"/></svg>"##
    );
    ok(
        &svg,
        &store,
        &[
            "--tab=addremove",
            "--addt=true",
            "--template_name=Second",
            "--id=b",
            "--id=a",
        ],
    );
    let t = Store::load(&store).unwrap().get("Second").unwrap();
    assert_eq!(
        t[0].as_ref().unwrap().paths,
        vec![kv(&[("d", "M0,0 v1")])],
        "b was selected first"
    );
    std::fs::remove_file(&store).unwrap();
}

#[test]
fn apply_removes_an_unchecked_positions_presentation_attribute_too() {
    // `marker-mid` here is a presentation attribute, not inline style: `remove_inline` would
    // leave it in place, unlike `Doc::remove_style`.
    let store = tmp_store("presentation-attr");
    let _ = std::fs::remove_file(&store);
    let svg = format!(r#"<svg {NS}><path id="r" d="M0,0 L1,0" marker-mid="url(#x)"/></svg>"#);
    let (s, msgs) = ok(
        &svg,
        &store,
        &[
            "--tab=markers",
            "--template=0",
            "--smarker=false",
            "--mmarker=false",
            "--emarker=false",
            "--id=r",
        ],
    );
    assert!(msgs.is_empty(), "{msgs:?}");
    let d = roxmltree::Document::parse(&s).unwrap();
    let r = by_id(&d, "r");
    assert_eq!(
        r.attribute("marker-mid"),
        None,
        "an unchecked position removes the presentation attribute too"
    );
    assert_eq!(style_of(r).get("marker-mid"), None);
    // the Markers page never writes the store: nothing was ever created here
    let _ = std::fs::remove_file(&store);
}

#[test]
fn adding_from_an_unmarked_path_and_applying_an_empty_template_are_errors() {
    let store = tmp_store("empty-template");
    let _ = std::fs::remove_file(&store);
    // F1: the selected path carries no markers
    let svg = format!(r#"<svg {NS}><path id="p" d="M0,0 L10,0" style="stroke:#000"/></svg>"#);
    let e = fm(
        &svg,
        &store,
        &[
            "--tab=addremove",
            "--addt=true",
            "--template_name=Empty",
            "--id=p",
        ],
    )
    .unwrap_err();
    assert_eq!(e, "The selected path has no markers to store.");
    assert!(!store.exists(), "nothing was saved");
    // F2: a hand-edited store whose template has a marker without a position resolves to no markers
    std::fs::write(&store, r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sciink="https://github.com/Mr-Milk/sciink"><marker sciink:template="Broken" orient="auto"><path d="M0,0 h1"/></marker></svg>"#).unwrap();
    let e = fm(
        &svg,
        &store,
        &[
            "--tab=markers",
            "--template=3",
            "--custom_name=Broken",
            "--smarker=true",
            "--id=p",
        ],
    )
    .unwrap_err();
    assert_eq!(e, "Template 'Broken' has no markers stored.");
    std::fs::remove_file(&store).unwrap();
}
