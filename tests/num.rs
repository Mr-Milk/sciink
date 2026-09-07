use sciink::num::{fmt, parse};

#[test]
fn fmt_table() {
    let cases: &[(f64, &str)] = &[
        (0.30000000000000004, "0.3"),
        (0.0, "0"),
        (-0.0, "0"),
        (1e-9, "0.000000001"),
        (f64::NAN, "0"),
        (f64::INFINITY, "0"),
        (123456789.0, "123456790"),
        (1234.56789012, "1234.5679"),
        (2.0, "2"),
        (-1.5, "-1.5"),
        (1e15, "1000000000000000"),
        (100.0, "100"),
        (-0.00000000001, "-0.00000000001"),
    ];
    for (v, want) in cases {
        assert_eq!(fmt(*v), *want, "fmt({v})");
    }
}

#[test]
fn parse_trims_and_rejects_garbage() {
    assert_eq!(parse(" 1.5 "), Some(1.5));
    assert_eq!(parse("1e3"), Some(1000.0));
    assert_eq!(parse("abc"), None);
    assert_eq!(parse(""), None);
}
