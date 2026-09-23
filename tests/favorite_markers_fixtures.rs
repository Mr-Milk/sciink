//! Favorite Markers against upstream's reference (`--id=path6928 --id=path6952 --smarker=True
//! --tab=markers` on Other_tests.svg, template index 1 = Triangle): one shared start marker,
//! same attributes, same path, both paths pointing at it.
mod support;

use std::ffi::OsString;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn attrs_sans_id(n: roxmltree::Node) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = n
        .attributes()
        .filter(|a| a.name() != "id")
        .map(|a| {
            (
                format!("{}:{}", a.namespace().unwrap_or(""), a.name()),
                a.value().to_string(),
            )
        })
        .collect();
    v.sort();
    v
}
/// The one `<marker>` in `d` whose id is not in `ids` (a plain `fn`, not a closure: a closure's
/// inferred type ties the parameter's and the return value's lifetimes to one fixed instantiation,
/// which does not typecheck against `roxmltree`'s two-lifetime `Node<'a, 'input>` here).
fn new_marker<'d, 'i>(
    d: &'d roxmltree::Document<'i>,
    ids: &std::collections::HashSet<&str>,
) -> roxmltree::Node<'d, 'i> {
    d.descendants()
        .find(|n| n.has_tag_name("marker") && !ids.contains(n.attribute("id").unwrap_or("")))
        .unwrap()
}

#[test]
fn start_marker_matches_the_reference() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join("svg/Other_tests.svg")).unwrap();
    let reference = std::fs::read_to_string(dir.join("refs/favorite_markers__--id__path6928__--id__path6952__--smarker__True__--tab__markers__Other_tests__svg.out")).unwrap();
    let store = std::env::temp_dir().join(format!("sciink-fm-{}-oracle.svg", std::process::id()));
    let _ = std::fs::remove_file(&store);
    let store_arg = format!("--store={}", store.display());
    let out = sciink::run(
        &args(&[
            "--tool=favorite-markers",
            "--tab=markers",
            "--template=1",
            "--smarker=true",
            "--id=path6928",
            "--id=path6952",
            &store_arg,
        ]),
        &input,
    )
    .unwrap();
    assert!(out.messages.is_empty(), "{:?}", out.messages);
    let ours = String::from_utf8(out.svg).unwrap();
    let (d_in, d_ours, d_ref) = (
        roxmltree::Document::parse(std::str::from_utf8(&input).unwrap()).unwrap(),
        roxmltree::Document::parse(&ours).unwrap(),
        roxmltree::Document::parse(&reference).unwrap(),
    );
    let count =
        |d: &roxmltree::Document| d.descendants().filter(|n| n.has_tag_name("marker")).count();
    assert_eq!(count(&d_ours), count(&d_in) + 1, "exactly one marker added");
    assert_eq!(count(&d_ref), count(&d_in) + 1);
    let ids: std::collections::HashSet<&str> = d_in
        .descendants()
        .filter_map(|n| n.attribute("id"))
        .collect();
    let (mo, mr) = (new_marker(&d_ours, &ids), new_marker(&d_ref, &ids));
    assert!(mo.attribute("id").unwrap().starts_with("FMTrianglestart"));
    assert_eq!(attrs_sans_id(mo), attrs_sans_id(mr), "marker attributes");
    let (go, gr) = (
        mo.first_element_child().unwrap(),
        mr.first_element_child().unwrap(),
    );
    assert_eq!(go.tag_name().name(), "g");
    assert_eq!(
        go.attribute("transform"),
        gr.attribute("transform"),
        "size 100 %: no transform on either"
    );
    assert_eq!(
        attrs_sans_id(go.first_element_child().unwrap()),
        attrs_sans_id(gr.first_element_child().unwrap()),
        "path attributes"
    );
    for id in ["path6928", "path6952"] {
        let n = d_ours
            .descendants()
            .find(|n| n.attribute("id") == Some(id))
            .unwrap();
        let st = sciink::style::Style::parse(n.attribute("style").unwrap());
        assert_eq!(
            st.get("marker-start"),
            Some(format!("url(#{})", mo.attribute("id").unwrap()).as_str()),
            "{id}"
        );
        assert_eq!(st.get("marker-mid"), None);
        assert_eq!(st.get("marker-end"), None);
        let r = d_ref
            .descendants()
            .find(|n| n.attribute("id") == Some(id))
            .unwrap();
        let rs = sciink::style::Style::parse(r.attribute("style").unwrap());
        assert!(
            rs.get("marker-start")
                .is_some_and(|v| v.starts_with("url(#FMTrianglestart")),
            "{id}: the reference points at its marker too"
        );
    }
    // The "markers" tab only reads the store (only `addt`/`remt` on the "addremove" tab call
    // `Store::save`, per `run()`), so the override path is never actually written; tolerate that
    // like the pre-clean above instead of assuming a side effect this invocation never produces.
    let _ = std::fs::remove_file(&store);
}
