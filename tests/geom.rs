mod support;

use kurbo::PathEl;
use sciink::dom::Doc;
use sciink::geom::path::*;
use sciink::geom::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn ipx_converts_units() {
    assert!(close(ipx("10pt").unwrap(), 13.333333333));
    assert_eq!(ipx("2in"), Some(192.0));
    assert!(close(ipx("1mm").unwrap(), 3.7795275591));
    assert!(close(ipx("1cm").unwrap(), 37.795275591));
    assert_eq!(ipx("5"), Some(5.0));
    assert_eq!(ipx(" 5px "), Some(5.0));
    assert_eq!(ipx("2pc"), Some(32.0));
    assert_eq!(ipx("50%"), None);
    assert_eq!(ipx("1em"), None);
    assert_eq!(ipx("abc"), None);
}

#[test]
fn transform_parse_and_format() {
    let t = parse_transform("translate(10,20) scale(2)").unwrap();
    let c = t.as_coeffs();
    assert!(close(c[0], 2.0) && close(c[3], 2.0) && close(c[4], 10.0) && close(c[5], 20.0));
    assert_eq!(parse_transform(""), Some(Affine::IDENTITY));
    assert_eq!(parse_transform("garbage("), None);
    assert_eq!(fmt_transform(Affine::IDENTITY), None);
    assert_eq!(
        fmt_transform(Affine::new([1.0, 0.0, 0.0, 1.0, 3.0, -4.5])),
        Some("translate(3,-4.5)".to_string())
    );
    assert_eq!(
        fmt_transform(Affine::new([2.0, 0.0, 0.0, 0.5, 0.0, 0.0])),
        Some("scale(2,0.5)".to_string())
    );
    assert_eq!(
        fmt_transform(Affine::new([1.0, 0.5, 0.0, 1.0, 0.0, 0.0])),
        Some("matrix(1,0.5,0,1,0,0)".to_string())
    );
    assert!(is_identity(Affine::new([
        1.000001, 0.0, 0.0, 1.0, 0.0, 0.0
    ])));
    assert!(!affine_eq(
        Affine::IDENTITY,
        Affine::new([1.0001, 0.0, 0.0, 1.0, 0.0, 0.0])
    ));
}

#[test]
fn scale_factor_and_inverse() {
    let t = Affine::new([2.0, 0.0, 0.0, 3.0, 5.0, 6.0]);
    assert!(close(scale_factor(t), 6f64.sqrt()));
    let inv = inverse(t).unwrap();
    assert!(affine_eq(t * inv, Affine::IDENTITY));
    assert_eq!(inverse(Affine::new([1.0, 2.0, 2.0, 4.0, 0.0, 0.0])), None);
}

#[test]
fn rect_algebra() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 20.0, 20.0);
    assert_eq!(
        union(Some(a), Some(b)),
        Some(Rect::new(0.0, 0.0, 20.0, 20.0))
    );
    assert_eq!(union(None, Some(b)), Some(b));
    assert_eq!(union(None, None), None);
    assert_eq!(
        intersection(Some(a), Some(b)),
        Some(Rect::new(5.0, 5.0, 10.0, 10.0))
    );
    assert_eq!(
        intersection(None, Some(b)),
        Some(b),
        "upstream quirk: null first operand returns the second"
    );
    assert_eq!(intersection(Some(a), None), None);
    assert_eq!(
        intersection(Some(a), Some(Rect::new(11.0, 0.0, 12.0, 1.0))),
        None
    );
    assert_eq!(
        intersection(Some(a), Some(Rect::new(10.0, 0.0, 12.0, 1.0))),
        Some(Rect::new(10.0, 0.0, 10.0, 1.0)),
        "touching edges give a zero-width box"
    );
    assert!(intersects(a, b));
    assert!(
        !intersects(a, Rect::new(10.0, 0.0, 12.0, 1.0)),
        "touching is not intersecting"
    );
    let r = transform_rect(
        Affine::new([0.0, 1.0, -1.0, 0.0, 0.0, 0.0]),
        Rect::new(0.0, 0.0, 2.0, 1.0),
    );
    assert!(close(r.x0, -1.0) && close(r.y0, 0.0) && close(r.x1, 0.0) && close(r.y1, 2.0));
}

#[test]
fn uniquetol_counts_clusters() {
    assert_eq!(uniquetol(&[1.0, 1.0005, 2.0, 2.0004, 5.0], 0.001), 3);
    assert_eq!(uniquetol(&[3.0, 1.0, 2.0], 0.5), 3);
    assert_eq!(uniquetol(&[], 0.5), 0);
    assert_eq!(
        uniquetol(&[1.0, 1.4, 1.8], 0.5),
        2,
        "tolerance is measured from the last kept value"
    );
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

#[test]
fn parse_d_handles_relative_and_shorthand_commands() {
    let p = parse_d("M 0 0 h 10 v 10 z").unwrap();
    assert_eq!(
        p.path.elements(),
        &[
            PathEl::MoveTo(pt(0.0, 0.0)),
            PathEl::LineTo(pt(10.0, 0.0)),
            PathEl::LineTo(pt(10.0, 10.0)),
            PathEl::ClosePath
        ]
    );
    assert_eq!(p.cmd_start, vec![0, 1, 2, 3, 4]);
    assert_eq!(
        end_points(&p.path),
        vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 0.0)]
    );
    let q = parse_d("m 1,2 3,4 l -1,-1").unwrap();
    assert_eq!(
        q.path.elements(),
        &[
            PathEl::MoveTo(pt(1.0, 2.0)),
            PathEl::LineTo(pt(4.0, 6.0)),
            PathEl::LineTo(pt(3.0, 5.0))
        ]
    );
    let s = parse_d("M0 0 C 0 10 10 10 10 0 S 20 -10 20 0").unwrap();
    assert_eq!(
        s.path.elements()[2],
        PathEl::CurveTo(pt(10.0, -10.0), pt(20.0, -10.0), pt(20.0, 0.0)),
        "S reflects the previous control point"
    );
    let t = parse_d("M0 0 Q 5 10 10 0 T 20 0").unwrap();
    assert_eq!(
        t.path.elements()[2],
        PathEl::QuadTo(pt(15.0, -10.0), pt(20.0, 0.0))
    );
    let after_close = parse_d("M 0 0 L 10 0 Z l 5 5").unwrap();
    assert_eq!(
        after_close.path.elements()[3],
        PathEl::LineTo(pt(5.0, 5.0)),
        "relative after Z starts from the subpath start"
    );
    assert!(parse_d("").is_none());
    assert!(parse_d("L 1 1").is_none(), "must start with a moveto");
}

#[test]
fn arcs_become_cubics_and_degenerate_arcs_become_lines() {
    let a = parse_d("M 0 0 A 10 10 0 0 1 20 0").unwrap();
    assert!(
        a.path
            .elements()
            .iter()
            .skip(1)
            .all(|e| matches!(e, PathEl::CurveTo(..)))
    );
    let bb = bbox_exact(&a.path).unwrap();
    assert!(
        (bb.width() - 20.0).abs() < 1e-3 && (bb.height() - 10.0).abs() < 1e-3,
        "{bb:?}"
    );
    assert_eq!(a.cmd_start.len(), 3);
    let d = parse_d("M 0 0 A 0 0 0 0 1 20 0").unwrap();
    assert_eq!(d.path.elements()[1], PathEl::LineTo(pt(20.0, 0.0)));
}

#[test]
fn hostile_arc_radii_fall_back_to_a_line_instead_of_hanging() {
    let t0 = std::time::Instant::now();
    let p = parse_d("M 0 0 A 1e308 1e308 0 0 1 20 0").unwrap();
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(2),
        "must fall back to a line instantly instead of subdividing an absurd arc"
    );
    assert_eq!(p.path.elements()[1], PathEl::LineTo(pt(20.0, 0.0)));
}

#[test]
fn fmt_d_is_absolute_and_round_trips() {
    let p = parse_d("M 0 0 h 10 v 10 z").unwrap();
    assert_eq!(fmt_d(&p.path), "M 0,0 L 10,0 L 10,10 Z");
    let q = parse_d("M0 0 C 0 10 10 10 10 0 Q 15 5 20 0").unwrap();
    assert_eq!(fmt_d(&q.path), "M 0,0 C 0,10 10,10 10,0 Q 15,5 20,0");
    let again = parse_d(&fmt_d(&q.path)).unwrap();
    assert!(path_eq(&q.path, &again.path, 1e-9));
}

#[test]
fn reverse_and_equality() {
    let p = parse_d("M 0 0 L 10 0 L 10 10").unwrap();
    let r = reverse(&p.path);
    assert_eq!(end_points(&r).first(), Some(&pt(10.0, 10.0)));
    assert_eq!(end_points(&r).last(), Some(&pt(0.0, 0.0)));
    assert!(path_eq(&p.path, &reverse(&r), 1e-9));
    assert!(!path_eq(&p.path, &r, 1e-9));
    let shifted = parse_d("M 0 0.0000001 L 10 0 L 10 10").unwrap();
    assert!(path_eq(&p.path, &shifted.path, 1e-6));
    assert!(!path_eq(&p.path, &shifted.path, 1e-9));
}

#[test]
fn rough_and_exact_bboxes() {
    let p = parse_d("M 0 0 C 0 10 10 10 10 0").unwrap();
    let exact = bbox_exact(&p.path).unwrap();
    let rough = bbox_rough(&p.path).unwrap();
    assert!(
        (exact.y1 - 7.5).abs() < 1e-9,
        "cubic extremum, got {exact:?}"
    );
    assert!((rough.y1 - 10.0).abs() < 1e-9, "control box, got {rough:?}");
    assert_eq!(bbox_exact(&BezPath::new()), None);
}

#[test]
fn shapes_convert_to_paths() {
    let d = Doc::parse(
        "<svg xmlns=\"http://www.w3.org/2000/svg\">\
         <rect id=\"r\" x=\"1\" y=\"2\" width=\"10\" height=\"5\"/>\
         <rect id=\"rr\" x=\"0\" y=\"0\" width=\"10\" height=\"10\" rx=\"2\"/>\
         <circle id=\"c\" cx=\"5\" cy=\"5\" r=\"5\"/>\
         <ellipse id=\"e\" cx=\"0\" cy=\"0\" rx=\"4\" ry=\"2\"/>\
         <line id=\"l\" x1=\"1\" y1=\"1\" x2=\"3\" y2=\"4\"/>\
         <polyline id=\"pl\" points=\"0,0 10,0 10,10\"/>\
         <polygon id=\"pg\" points=\"0,0 10,0 10,10\"/>\
         <path id=\"p\" d=\"M 0 0 L 1 1\"/>\
         <text id=\"t\">x</text>\
         <rect id=\"bad\" width=\"50%\" height=\"1\"/>\
         <rect id=\"bad2\" x=\"50%\" y=\"0\" width=\"1\" height=\"1\"/>\
         <circle id=\"bad3\" cx=\"1\" cy=\"1\" r=\"10%\"/></svg>"
            .as_bytes(),
    )
    .unwrap();
    let bb = |id: &str| bbox_exact(&shape_path(&d, d.by_id(id).unwrap()).unwrap().path).unwrap();
    assert_eq!(bb("r"), Rect::new(1.0, 2.0, 11.0, 7.0));
    let rr = bb("rr");
    assert!(
        (rr.x0).abs() < 1e-3 && (rr.x1 - 10.0).abs() < 1e-3 && (rr.y1 - 10.0).abs() < 1e-3,
        "{rr:?}"
    );
    let c = bb("c");
    assert!(
        (c.x0).abs() < 1e-3
            && (c.x1 - 10.0).abs() < 1e-3
            && (c.y0).abs() < 1e-3
            && (c.y1 - 10.0).abs() < 1e-3,
        "{c:?}"
    );
    let e = bb("e");
    assert!(
        (e.x0 + 4.0).abs() < 1e-3 && (e.y1 - 2.0).abs() < 1e-3,
        "{e:?}"
    );
    assert_eq!(bb("l"), Rect::new(1.0, 1.0, 3.0, 4.0));
    assert_eq!(bb("pl"), Rect::new(0.0, 0.0, 10.0, 10.0));
    let pg = shape_path(&d, d.by_id("pg").unwrap()).unwrap();
    assert_eq!(pg.path.elements().last(), Some(&PathEl::ClosePath));
    assert_eq!(
        shape_path(&d, d.by_id("p").unwrap())
            .unwrap()
            .path
            .elements()
            .len(),
        2
    );
    assert!(shape_path(&d, d.by_id("t").unwrap()).is_none());
    assert!(
        shape_path(&d, d.by_id("bad").unwrap()).is_none(),
        "percent lengths are unsupported"
    );
    assert!(
        shape_path(&d, d.by_id("bad2").unwrap()).is_none(),
        "a percent x must fail the whole shape, not silently default to 0"
    );
    assert!(
        shape_path(&d, d.by_id("bad3").unwrap()).is_none(),
        "a percent r must fail the whole shape"
    );
}

#[test]
fn upstream_paths_survive_parse_format_parse() {
    for file in support::upstream_svgs() {
        let doc = Doc::parse(&std::fs::read(&file).unwrap()).unwrap();
        let mut failures = Vec::new();
        let mut count = 0;
        for n in doc.descendants(doc.svg()) {
            if doc.tag(n) != "path" {
                continue;
            }
            let Some(d) = doc.attr(n, "d") else { continue };
            if d.trim().is_empty() {
                continue;
            }
            let Some(pp) = parse_d(d) else {
                failures.push(format!("unparsable: {:?}", doc.attr(n, "id")));
                continue;
            };
            let again = parse_d(&fmt_d(&pp.path)).unwrap();
            if !path_eq(&pp.path, &again.path, 1e-3) {
                failures.push(format!("changed after fmt/parse: {:?}", doc.attr(n, "id")));
            }
            count += 1;
        }
        assert!(
            failures.is_empty(),
            "{}: {} of {} paths failed:\n{}",
            file.display(),
            failures.len(),
            count,
            failures.join("\n")
        );
        eprintln!("{}: {count} paths ok", file.display());
    }
}

#[test]
fn hostile_arc_endpoints_degrade_to_a_line_quickly() {
    use sciink::geom::path::parse_d;
    // Endpoint 1e60 away: caught by first-stage hostile check on endpoint coordinates.
    let p = parse_d("M 0 0 A 5 5 0 0 1 1e60 1").expect("parses");
    assert_eq!(
        p.path.elements().len(),
        2,
        "expected MoveTo+LineTo only, got {} elements",
        p.path.elements().len()
    );
    // Huge but finite coordinates on both endpoints: caught by first-stage check.
    let p = parse_d("M 1e300 0 A 1 1 0 0 1 -1e300 0").expect("parses");
    assert_eq!(p.path.elements().len(), 2);
    // Both endpoints are inside the per-axis limit (9.99e14 < 1e15), so the first
    // guard passes; the chord is ~2e15 long, so kurbo scales the radii to ~1.41e15,
    // which the second guard (converted radii) must reject.
    let p =
        parse_d("M 999999999999999 999999999999999 A 5 5 0 0 1 -999999999999999 -999999999999999")
            .expect("parses");
    assert_eq!(
        p.path.elements().len(),
        2,
        "converted-radii guard must degrade the arc to a line"
    );
    // Radii exceeding LIMIT: caught by first-stage check on radii magnitude.
    let p = parse_d("M 0 0 A 1e16 1e16 0 0 1 10 0").expect("parses");
    assert_eq!(p.path.elements().len(), 2);
    // Non-finite coordinates cannot be written as literals (svgtypes rejects them),
    // but a relative move can overflow the current point to +inf; the arc that
    // follows must then degrade to a line: MoveTo + LineTo (the `l`) + LineTo (the arc).
    let p = parse_d("M 1e308 0 l 1e308 0 A 5 5 0 0 1 10 0").expect("parses");
    assert_eq!(
        p.path.elements().len(),
        3,
        "non-finite current point must degrade the arc to a line"
    );
    // Normal arc: still subdivides into cubics (no regression).
    let p = parse_d("M 0 0 A 5 5 0 0 1 10 0").expect("parses");
    assert!(
        p.path.elements().len() > 3,
        "a normal arc must be subdivided into cubics"
    );
}

#[test]
fn composed_transform_excludes_root_svg_and_composes_outer_first() {
    use kurbo::{Affine, Point};
    use sciink::dom::Doc;
    let d = Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg" transform="scale(100)">
  <g id="a" transform="translate(10,20)"><g id="b" transform="scale(2)"><path id="p" d="M0,0" transform="translate(1,1)"/></g></g>
  <path id="q" d="M0,0"/></svg>"#).unwrap();
    let p = d.by_id("p").unwrap();
    let t = d.composed_transform(p);
    // outer-first: translate(10,20) * scale(2) * translate(1,1) maps (0,0) -> (12, 22)
    let got = t * Point::new(0.0, 0.0);
    assert!(
        (got.x - 12.0).abs() < 1e-9 && (got.y - 22.0).abs() < 1e-9,
        "{got:?}"
    );
    assert_eq!(
        d.composed_transform(d.by_id("q").unwrap()),
        Affine::IDENTITY
    );
    assert_eq!(d.composed_transform(d.svg()), Affine::IDENTITY);
    assert_eq!(
        d.transform(d.by_id("a").unwrap()),
        Affine::translate((10.0, 20.0))
    );
    assert_eq!(d.transform(d.by_id("q").unwrap()), Affine::IDENTITY);
}
