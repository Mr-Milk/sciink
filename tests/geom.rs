mod support;

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
