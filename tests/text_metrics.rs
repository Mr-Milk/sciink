mod support;

use std::path::PathBuf;

use sciink::text::fonts::{FontStyle, FontSystem};
use sciink::text::metrics::Metrics;

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}

#[test]
fn single_char_advance_matches_the_font_tables() {
    let fs = fonts();
    let dv = fs
        .pick(fs.family_faces("DejaVu Sans"), 400, FontStyle::Normal, 5)
        .unwrap();
    let mut m = Metrics::new();
    // hmtx advance of 'I' straight from ttf-parser, in em
    let expect = fs
        .with_face(dv, |f| {
            let g = f.glyph_index('I').unwrap();
            f.glyph_hor_advance(g).unwrap() as f64 / f.units_per_em() as f64
        })
        .unwrap();
    let p = m.prop(&fs, dv, 'I', &[]);
    assert!((p.charw - expect).abs() < 1e-12, "{} vs {expect}", p.charw);
    assert!(
        p.charw > 0.29 && p.charw < 0.30,
        "DejaVu Sans 'I' is 604/2048 em: {}",
        p.charw
    );
    assert!(
        p.spacew > 0.31 && p.spacew < 0.32,
        "DejaVu Sans space is 651/2048 em: {}",
        p.spacew
    );
    assert!((p.caph - 0.729).abs() < 0.005);
    // ink box of 'I': narrow, from the baseline up to the cap height
    let [x, y, w, h] = p.inkbb;
    assert!(x > 0.0 && w > 0.0 && w < p.charw, "{:?}", p.inkbb);
    assert!((y + 0.729).abs() < 0.005, "y = -y_max: {y}");
    assert!((h - 0.729).abs() < 0.005, "I sits on the baseline: {h}");
    // a space has no ink
    let sp = m.prop(&fs, dv, ' ', &[]);
    assert_eq!(sp.inkbb, [0.0; 4]);
    assert!((sp.charw - sp.spacew).abs() < 1e-12);
    // unrendered placeholder
    let u = Metrics::unrendered('\u{10348}');
    assert_eq!(
        (u.charw, u.spacew, u.caph, u.inkbb),
        (0.0, 0.0, 0.0, [0.0; 4])
    );
}

#[test]
fn pair_adjustments_capture_kerning_and_ligatures() {
    let fs = fonts();
    let dv = fs
        .pick(fs.family_faces("DejaVu Sans"), 400, FontStyle::Normal, 5)
        .unwrap();
    let mut m = Metrics::new();
    let av = m.pair_adv(&fs, dv, 'A', 'V');
    assert!(
        av < -0.01 && av > -0.2,
        "DejaVu Sans kerns A–V negative: {av}"
    );
    let ii = m.pair_adv(&fs, dv, 'I', 'I');
    assert!(ii.abs() < 1e-9, "no kerning between I and I: {ii}");
    // dadvs of a prop are filled only for the requested predecessors (+ always finite)
    let p = m.prop(&fs, dv, 'V', &['A', ' ']);
    assert_eq!(p.dadvs.len(), 2);
    assert!((p.dadvs[&'A'] - av).abs() < 1e-12);
    assert!(p.dadvs[&' '].abs() < 1e-9);
    // string advance is additive up to kerning
    let a = m.advance(&fs, dv, "A");
    let v = m.advance(&fs, dv, "V");
    let both = m.advance(&fs, dv, "AV");
    assert!((both - (a + v + av)).abs() < 1e-12);
    // the same request twice hits the cache and stays identical
    assert_eq!(m.prop(&fs, dv, 'V', &['A', ' ']), p);
}

#[test]
fn bold_face_is_wider_than_regular() {
    let fs = fonts();
    let reg = fs
        .pick(fs.family_faces("DejaVu Sans"), 400, FontStyle::Normal, 5)
        .unwrap();
    let bold = fs
        .pick(fs.family_faces("DejaVu Sans"), 700, FontStyle::Normal, 5)
        .unwrap();
    let mut m = Metrics::new();
    assert!(m.advance(&fs, bold, "Hamburgefonstiv") > m.advance(&fs, reg, "Hamburgefonstiv"));
}
