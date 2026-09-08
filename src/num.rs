//! The one number formatter. Every number written into an SVG goes through `fmt`
//! so output is byte-stable across runs and platforms (spec §C.1).

/// 8 significant digits, shortest representation, no exponent, `-0` → `0`,
/// NaN/±inf → `0`.
pub fn fmt(v: f64) -> String {
    if !v.is_finite() || v == 0.0 {
        return "0".to_string();
    }
    // `{:.7e}` rounds to 8 significant digits; re-parsing gives the nearest f64,
    // whose `Display` is the shortest round-tripping decimal without exponent.
    let rounded: f64 = format!("{v:.7e}").parse().unwrap_or(0.0);
    if rounded == 0.0 {
        return "0".to_string();
    }
    format!("{rounded}")
}

/// Lenient float parse used for attribute values: trims whitespace.
pub fn parse(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}
