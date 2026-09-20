mod support;

use std::ffi::OsString;
use std::path::PathBuf;

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink")
        .chain(v.iter().copied())
        .map(OsString::from)
        .collect()
}
fn fontdir() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fonts")
        .display()
        .to_string()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

// The library reads SCIINK_FONT_DIRS / SCIINK_NO_SYSTEM_FONTS through FontSystem::load();
// set them once for the whole test binary (behind a `Once`) so every `set_var` completes
// before any test thread reaches `FontSystem::load()`, then every test enters through this
// function so the vendored fonts are visible everywhere they are needed.
static INIT: std::sync::Once = std::sync::Once::new();
fn with_vendored_fonts<T>(f: impl FnOnce() -> T) -> T {
    INIT.call_once(|| {
        // SAFETY: runs once, before any test in this binary reads the environment (every
        // test enters through this function); the values never change afterwards.
        unsafe {
            std::env::set_var("SCIINK_NO_SYSTEM_FONTS", "1");
            std::env::set_var("SCIINK_FONT_DIRS", fontdir());
        }
    });
    f()
}

#[test]
fn text_highlight_appends_one_rect_per_character() {
    let svg = format!(
        r#"<svg {NS}><g id="layer1" transform="translate(10,20)"><text id="t" style="font-family:'DejaVu Sans';font-size:10px" x="0" y="0">Test</text><text id="f" style="font-size:3px;inline-size:24"><tspan x="0" y="1">flow</tspan></text></g></svg>"#
    );
    let out = with_vendored_fonts(|| {
        sciink::run(
            &args(&["--tool=text-highlight", "--htype=char", "--id=layer1"]),
            svg.as_bytes(),
        )
    })
    .unwrap();
    let s = String::from_utf8(out.svg).unwrap();
    let d = roxmltree::Document::parse(&s).unwrap();
    let rects: Vec<_> = d.descendants().filter(|n| n.has_tag_name("rect")).collect();
    assert_eq!(rects.len(), 4, "{s}");
    for (i, r) in rects.iter().enumerate() {
        assert_eq!(r.attribute("transform"), Some("translate(10,20)"));
        let expect = if i % 2 == 0 {
            "fill:#007575;fill-opacity:0.4675"
        } else {
            "fill:#007575;fill-opacity:0.5675"
        };
        assert_eq!(r.attribute("style"), Some(expect));
        let w: f64 = r.attribute("width").unwrap().parse().unwrap();
        let h: f64 = r.attribute("height").unwrap().parse().unwrap();
        assert!(
            w > 0.0 && (h - 7.29).abs() < 0.05,
            "cap height 0.729 × 10: {h}"
        );
        // rects are appended at the end of the root
        assert_eq!(r.parent().unwrap().tag_name().name(), "svg");
    }
    let x0: f64 = rects[0].attribute("x").unwrap().parse().unwrap();
    let x1: f64 = rects[1].attribute("x").unwrap().parse().unwrap();
    assert!(x0 == 0.0 && x1 > x0);
    assert!(
        out.messages
            .iter()
            .any(|m| m.contains("highlighted 4 rectangles (2 text elements, 1 flows skipped)")),
        "{:?}",
        out.messages
    );
    // whole-document mode and the other htypes
    let full = with_vendored_fonts(|| {
        sciink::run(
            &args(&["--tool=text-highlight", "--htype=full"]),
            svg.as_bytes(),
        )
    })
    .unwrap();
    let s = String::from_utf8(full.svg).unwrap();
    assert_eq!(s.matches("<rect").count(), 1);
    for ht in ["charink", "chunk", "line", "fullink"] {
        let o = with_vendored_fonts(|| {
            sciink::run(
                &args(&["--tool=text-highlight", &format!("--htype={ht}")]),
                svg.as_bytes(),
            )
        })
        .unwrap();
        assert!(String::from_utf8(o.svg).unwrap().contains("<rect"), "{ht}");
    }
    assert!(
        sciink::run(
            &args(&["--tool=text-highlight", "--htype=bogus"]),
            svg.as_bytes()
        )
        .is_err()
    );
}

#[test]
fn font_probe_reports_resolutions_and_generics() {
    let svg = format!(
        r#"<svg {NS}><text id="t" style="font-family:Helvetica;font-weight:bold">a</text><text style="font-family:Roboto">b</text></svg>"#
    );
    let out =
        with_vendored_fonts(|| sciink::run(&args(&["--tool=font-probe"]), svg.as_bytes())).unwrap();
    assert_eq!(out.svg, svg.as_bytes(), "font-probe echoes the document");
    let report = out.messages.join("\n");
    assert!(
        report.contains("'Helvetica' weight 700 normal → DejaVu Sans (DejaVuSans-Bold.ttf)"),
        "{report}"
    );
    assert!(
        report.contains("'Roboto' weight 400 normal → Roboto (Roboto-Regular.ttf)"),
        "{report}"
    );
    assert!(
        report.contains("sans-serif → DejaVu Sans (DejaVuSans.ttf)"),
        "{report}"
    );
    assert!(report.contains("serif → DejaVu Sans"), "{report}");
    assert!(report.contains("faces: 4 in "), "{report}");
    assert!(
        report.contains("font-family \"Helvetica\" not installed; measured with \"DejaVu Sans\""),
        "{report}"
    );
}

#[test]
fn about_reports_fonts() {
    let svg = format!(r#"<svg {NS}><text>x</text></svg>"#);
    let out =
        with_vendored_fonts(|| sciink::run(&args(&["--tool=about"]), svg.as_bytes())).unwrap();
    let report = out.messages.join("\n");
    assert!(report.contains("fonts: 4 faces in "), "{report}");
    assert!(
        report.contains("Arial → DejaVu Sans (DejaVuSans.ttf)"),
        "{report}"
    );
    assert!(
        report.contains("DejaVu Sans → DejaVu Sans (DejaVuSans.ttf)"),
        "{report}"
    );
    assert!(
        report.contains("sans-serif → DejaVu Sans (DejaVuSans.ttf)"),
        "{report}"
    );
}

/// Experiment A.5-2A: compare our per-character extents with upstream's `--debugparser`
/// reference (rendered by Inkscape/Pango on the author's machine). Needs the same font
/// families installed, so it runs only with SCIINK_SYSTEM_FONTS=1 and prints a per-family table.
#[test]
#[ignore]
fn debugparser_reference_agreement() {
    if std::env::var_os("SCIINK_SYSTEM_FONTS").is_none() {
        eprintln!("SKIP: set SCIINK_SYSTEM_FONTS=1 to compare against the upstream reference");
        return;
    }
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let src = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let reference = std::fs::read_to_string(
        dir.join("refs/flatten_plots__--id__layer1__--testmode__True__--debugparser__True__Text_tests__svg.out"),
    )
    .unwrap();
    // reference rects in root coordinates
    let rd = roxmltree::Document::parse(&reference).unwrap();
    let ref_boxes: Vec<kurbo::Rect> = rd
        .descendants()
        .filter(|n| {
            n.has_tag_name("rect")
                && n.attribute("style")
                    .is_some_and(|s| s.starts_with("fill:#007575"))
        })
        .map(|n| {
            let g = |a: &str| n.attribute(a).unwrap().parse::<f64>().unwrap();
            let t = n
                .attribute("transform")
                .and_then(sciink::geom::parse_transform)
                .unwrap_or(kurbo::Affine::IDENTITY);
            sciink::geom::transform_rect(
                t,
                kurbo::Rect::new(g("x"), g("y"), g("x") + g("width"), g("y") + g("height")),
            )
        })
        .collect();
    assert!(ref_boxes.len() > 3000, "{}", ref_boxes.len());
    // ours: same tool, whole document, system fonts
    let out = sciink::run(&args(&["--tool=text-highlight", "--htype=char"]), &src).unwrap();
    let od = roxmltree::Document::parse(std::str::from_utf8(&out.svg).unwrap()).unwrap();
    let ours: Vec<(kurbo::Rect, String)> = od
        .descendants()
        .filter(|n| {
            n.has_tag_name("rect")
                && n.attribute("style")
                    .is_some_and(|s| s.starts_with("fill:#007575"))
        })
        .map(|n| {
            let g = |a: &str| n.attribute(a).unwrap().parse::<f64>().unwrap();
            let t = n
                .attribute("transform")
                .and_then(sciink::geom::parse_transform)
                .unwrap_or(kurbo::Affine::IDENTITY);
            (
                sciink::geom::transform_rect(
                    t,
                    kurbo::Rect::new(g("x"), g("y"), g("x") + g("width"), g("y") + g("height")),
                ),
                n.attribute("data-family").unwrap_or("?").to_string(),
            )
        })
        .collect();
    // nearest-neighbour deviation per family (px, root coordinates)
    let mut per_family: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    for (r, fam) in &ours {
        let best = ref_boxes
            .iter()
            .map(|q| {
                (q.x0 - r.x0)
                    .abs()
                    .max((q.y1 - r.y1).abs())
                    .max((q.width() - r.width()).abs())
                    .max((q.height() - r.height()).abs())
            })
            .fold(f64::INFINITY, f64::min);
        per_family.entry(fam.clone()).or_default().push(best);
    }
    eprintln!(
        "{:<24}{:>6}{:>10}{:>10}",
        "family", "chars", "median", "p90"
    );
    for (fam, mut v) in per_family {
        v.sort_by(f64::total_cmp);
        let med = v[v.len() / 2];
        let p90 = v[(v.len() * 9 / 10).min(v.len() - 1)];
        eprintln!("{fam:<24}{:>6}{med:>10.3}{p90:>10.3}", v.len());
        if ["Arial", "Tahoma", "Verdana", "Roboto"].contains(&fam.as_str()) {
            assert!(med < 0.5, "{fam}: median deviation {med} px");
        }
    }
}
