mod support;

use std::path::PathBuf;

use sciink::text::fonts::{FontStyle, FontSystem};

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}

#[test]
fn enumerates_vendored_faces_with_metrics() {
    let fs = fonts();
    assert_eq!(fs.face_count(), 4, "DejaVu Sans ×2 + Roboto ×2");
    let dv = fs.family_faces("dejavu sans"); // case-insensitive
    assert_eq!(dv.len(), 2);
    let reg = fs.pick(dv, 400, FontStyle::Normal, 5).unwrap();
    let bold = fs.pick(dv, 700, FontStyle::Normal, 5).unwrap();
    assert_ne!(reg, bold);
    let info = fs.face_info(reg);
    assert_eq!(info.family, "DejaVu Sans");
    assert_eq!(info.weight, 400);
    assert_eq!(info.style, FontStyle::Normal);
    assert_eq!(info.upem, 2048.0);
    assert!(
        (info.ascent + info.descent - 1.0).abs() < 1e-12,
        "normalised ascent+descent"
    );
    assert!(info.ascent > 0.7 && info.ascent < 0.85, "{}", info.ascent);
    // cap height from the 'I' glyph: DejaVu Sans caps are 0.729 em
    assert!(
        (info.cap_height - 0.729).abs() < 0.005,
        "{}",
        info.cap_height
    );
    assert!(
        info.x_height > 0.5 && info.x_height < 0.6,
        "{}",
        info.x_height
    );
    assert_eq!(fs.face_info(bold).weight, 700);
    assert!(fs.family_faces("No Such Family").is_empty());
    assert!(fs.has_glyph(reg, 'A'));
    assert!(fs.has_glyph(reg, '\u{23A3}'), "DejaVu Sans covers ⎣");
    let rob = fs
        .pick(fs.family_faces("Roboto"), 400, FontStyle::Normal, 5)
        .unwrap();
    assert!(!fs.has_glyph(rob, '\u{23A3}'), "Roboto lacks ⎣");
    assert!(fs.face_data(reg).is_some());
    assert!(fs.with_face(reg, |f| f.units_per_em()).unwrap() == 2048);
}

#[test]
fn pick_follows_css_weight_and_style_rules() {
    let fs = fonts();
    let dv = fs.family_faces("DejaVu Sans");
    let reg = fs.pick(dv, 400, FontStyle::Normal, 5).unwrap();
    let bold = fs.pick(dv, 700, FontStyle::Normal, 5).unwrap();
    // below 400: look lighter first, then heavier → only 400/700 exist → 400
    assert_eq!(fs.pick(dv, 300, FontStyle::Normal, 5), Some(reg));
    // 400..=500 tries up to 500 first, then lighter, then heavier → 400
    assert_eq!(fs.pick(dv, 500, FontStyle::Normal, 5), Some(reg));
    // above 500: heavier first → 700
    assert_eq!(fs.pick(dv, 600, FontStyle::Normal, 5), Some(bold));
    assert_eq!(fs.pick(dv, 900, FontStyle::Normal, 5), Some(bold));
    // no italic faces vendored: italic request falls back to normal of the same weight
    assert_eq!(fs.pick(dv, 700, FontStyle::Italic, 5), Some(bold));
    assert_eq!(fs.pick(&[], 400, FontStyle::Normal, 5), None);
}

#[test]
fn faces_iterate_in_stable_order_and_env_is_respected() {
    let fs = fonts();
    let fams: Vec<String> = fs.faces().map(|k| fs.face_info(k).family.clone()).collect();
    assert_eq!(fams, ["DejaVu Sans", "DejaVu Sans", "Roboto", "Roboto"]);
    // from_dirs never loads system fonts, so counts are exact regardless of the host.
    let empty = FontSystem::from_dirs(&[]);
    assert_eq!(empty.face_count(), 0);
    assert!(fs.load_ms() >= 0.0);
}
