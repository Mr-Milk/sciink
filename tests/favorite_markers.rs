mod support;

use std::ffi::OsString;
use std::path::PathBuf;

use sciink::tools::favorite_markers::{MarkerData, Store, Template, store_path};

#[allow(dead_code)]
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

#[allow(dead_code)]
fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
#[allow(dead_code)]
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
