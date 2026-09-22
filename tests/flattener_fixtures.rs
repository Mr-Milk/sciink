mod support;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use support::with_vendored_fonts;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}

/// Element counts by local name under the element with `id`.
fn counts(d: &roxmltree::Document, id: &str) -> HashMap<String, usize> {
    let mut c = HashMap::new();
    if let Some(layer) = d.descendants().find(|n| n.attribute("id") == Some(id)) {
        for n in layer.descendants().filter(|n| n.is_element()) {
            *c.entry(n.tag_name().name().to_string()).or_default() += 1;
        }
    }
    c
}
fn attr_count(d: &roxmltree::Document, id: &str, attr: &str) -> usize {
    d.descendants()
        .find(|n| n.attribute("id") == Some(id))
        .map(|l| {
            l.descendants()
                .filter(|n| n.attribute(attr).is_some())
                .count()
        })
        .unwrap_or(0)
}
/// Whitespace-collapsed text of every <text> under `id`.
fn layer_texts(d: &roxmltree::Document, id: &str) -> Vec<String> {
    let Some(layer) = d.descendants().find(|n| n.attribute("id") == Some(id)) else {
        return Vec::new();
    };
    layer
        .descendants()
        .filter(|n| n.has_tag_name("text"))
        .map(|n| {
            let s: String = n
                .descendants()
                .filter(|c| c.is_text())
                .filter_map(|c| c.text())
                .collect();
            s.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|s| !s.is_empty())
        .collect()
}
fn minus_signs(texts: &[String]) -> usize {
    texts.iter().map(|t| t.matches('\u{2212}').count()).sum()
}
const INKSCAPE_NS: &str = "http://www.inkscape.org/namespaces/inkscape";
const SODIPODI_NS: &str = "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd";
/// roxmltree looks namespaced attributes up by (namespace, local name).
fn nsattr<'a>(n: roxmltree::Node<'a, '_>, ns: &str, local: &str) -> Option<&'a str> {
    n.attribute((ns, local))
}

fn flatten_fixture(name: &str) -> Option<(String, String)> {
    let dir = support::upstream_data_dir()?;
    let input = std::fs::read(dir.join(format!("svg/{name}.svg"))).unwrap();
    let reference = std::fs::read_to_string(dir.join(format!(
        "refs/flatten_plots__--id__layer1__--testmode__True__{name}__svg.out"
    )))
    .unwrap();
    let t0 = std::time::Instant::now();
    let out = with_vendored_fonts(|| {
        sciink::run(
            &args(&[
                "--tool=flattener",
                "--tab=Options",
                "--id=layer1",
                "--testmode=true",
            ]),
            &input,
        )
    })
    .unwrap();
    eprintln!(
        "{name}: flattened in {:?}, {} warnings",
        t0.elapsed(),
        out.messages.len()
    );
    assert!(
        out.messages.iter().all(|m| m.starts_with("warning: ")),
        "{:?}",
        out.messages
    );
    Some((String::from_utf8(out.svg).unwrap(), reference))
}

fn structural_oracle(name: &str) {
    let Some((ours, reference)) = flatten_fixture(name) else {
        return;
    };
    let od = roxmltree::Document::parse(&ours).unwrap();
    let rd = roxmltree::Document::parse(&reference).unwrap();
    let (oc, rc) = (counts(&od, "layer1"), counts(&rd, "layer1"));
    let get = |c: &HashMap<String, usize>, k: &str| c.get(k).copied().unwrap_or(0);
    eprintln!("{name}: ours {oc:?}\n{name}: ref  {rc:?}");
    for k in ["g", "clipPath", "use", "image", "line"] {
        assert_eq!(get(&oc, k), get(&rc, k), "{name}: {k} count");
    }
    assert_eq!(
        get(&oc, "clipPath"),
        0,
        "{name}: clips moved to the root defs"
    );
    let (op, rp) = (get(&oc, "path") as f64, get(&rc, "path") as f64);
    assert!((op - rp).abs() <= 0.03 * rp, "{name}: paths {op} vs {rp}");
    assert!(
        (get(&oc, "rect") as i64 - get(&rc, "rect") as i64).abs() <= 3,
        "{name}: rects {} vs {}",
        get(&oc, "rect"),
        get(&rc, "rect")
    );
    assert_eq!(
        attr_count(&od, "layer1", "mpl_comment"),
        attr_count(&rd, "layer1", "mpl_comment"),
        "{name}: matplotlib glyph groups"
    );
    assert_eq!(
        minus_signs(&layer_texts(&od, "layer1")),
        minus_signs(&layer_texts(&rd, "layer1")),
        "{name}: minus-sign reversions"
    );
    assert_eq!(
        attr_count(&od, "layer1", "unlinked_clone"),
        0,
        "{name}: markers are stripped"
    );
    let flat = od
        .descendants()
        .find(|n| n.attribute("id") == Some("layer1"))
        .unwrap();
    assert_eq!(nsattr(flat, INKSCAPE_NS, "label"), Some("Layer 1 flat"));
    // `prev_siblings()` is self-inclusive (roxmltree 0.21 `AxisIter` starts at `*self`); skip `flat`.
    let orig = flat
        .prev_siblings()
        .skip(1)
        .find(|n| n.is_element())
        .expect("the duplicate precedes the flattened layer");
    assert_eq!(nsattr(orig, INKSCAPE_NS, "label"), Some("Layer 1 original"));
    assert_eq!(
        (
            nsattr(orig, SODIPODI_NS, "insensitive"),
            orig.attribute("opacity")
        ),
        (Some("true"), Some("0.3"))
    );
}

#[test]
fn text_tests_structure_matches_the_upstream_reference() {
    structural_oracle("Text_tests");
}

#[test]
fn text_tests_dx_structure_matches_the_upstream_reference() {
    structural_oracle("Text_tests_dx");
}

#[test]
fn acid_tests_structure_matches_the_upstream_reference() {
    structural_oracle("Acid_tests");
}

fn content_oracle(name: &str) {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        eprintln!("SKIP: set SCIINK_SYSTEM_FONTS=1 to run against the installed fonts");
        return;
    }
    // vendored DejaVu Sans on top of the system fonts; SAFETY: set before any FontSystem::load()
    // in this binary's ignored tests, which run alone (`-- --ignored`)
    unsafe {
        std::env::set_var(
            "SCIINK_FONT_DIRS",
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fonts")
                .display()
                .to_string(),
        );
    }
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let input = std::fs::read(dir.join(format!("svg/{name}.svg"))).unwrap();
    let reference = std::fs::read_to_string(dir.join(format!(
        "refs/flatten_plots__--id__layer1__--testmode__True__{name}__svg.out"
    )))
    .unwrap();
    let out = sciink::run(
        &args(&[
            "--tool=flattener",
            "--tab=Options",
            "--id=layer1",
            "--testmode=true",
        ]),
        &input,
    )
    .unwrap();
    let od = roxmltree::Document::parse(std::str::from_utf8(&out.svg).unwrap()).unwrap();
    let rd = roxmltree::Document::parse(&reference).unwrap();
    let (ours, theirs) = (layer_texts(&od, "layer1"), layer_texts(&rd, "layer1"));
    let mut pool: HashMap<&str, i64> = HashMap::new();
    for t in &theirs {
        *pool.entry(t.as_str()).or_default() += 1;
    }
    let mut matched = 0usize;
    for t in &ours {
        if let Some(c) = pool.get_mut(t.as_str()) {
            if *c > 0 {
                *c -= 1;
                matched += 1;
            }
        }
    }
    eprintln!(
        "{name}: {matched}/{} reference strings reproduced ({} ours)",
        theirs.len(),
        ours.len()
    );
    assert!(
        matched as f64 >= 0.85 * theirs.len() as f64,
        "{name}: {matched}/{}",
        theirs.len()
    );
}

/// Run: `SCIINK_SYSTEM_FONTS=1 cargo test --test flattener_fixtures -- --ignored --nocapture`
#[test]
#[ignore]
fn text_tests_content_matches_the_upstream_reference() {
    content_oracle("Text_tests");
}

#[test]
#[ignore]
fn text_tests_dx_content_matches_the_upstream_reference() {
    content_oracle("Text_tests_dx");
}
