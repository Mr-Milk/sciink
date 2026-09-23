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
    assert!(fs.load_ms() > 0.0);
}

#[test]
fn load_ms_covers_the_directory_scan_not_just_the_face_pass() {
    // The timer must start before the database is populated: reading the four vendored
    // files off disk is the bulk of the cost and used to be excluded entirely.
    let t0 = std::time::Instant::now();
    let fs = fonts();
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(fs.face_count(), 4);
    assert!(fs.load_ms() > 0.0, "load_ms = {}", fs.load_ms());
    assert!(
        fs.load_ms() <= wall_ms,
        "load_ms {} exceeds the wall time {wall_ms} of the whole call",
        fs.load_ms()
    );
}

#[test]
fn face_bytes_are_cached_per_source_file() {
    let fs = fonts();
    let dv = fs.family_faces("DejaVu Sans");
    let reg = fs.pick(dv, 400, FontStyle::Normal, 5).unwrap();
    let bold = fs.pick(dv, 700, FontStyle::Normal, 5).unwrap();
    let (a, _) = fs.face_data(reg).unwrap();
    let (b, _) = fs.face_data(reg).unwrap();
    assert!(std::rc::Rc::ptr_eq(&a, &b), "second read hits the cache");
    // DejaVuSans.ttf and DejaVuSans-Bold.ttf are separate files, so separate buffers.
    let (c, _) = fs.face_data(bold).unwrap();
    assert!(
        !std::rc::Rc::ptr_eq(&a, &c),
        "distinct files must not share a buffer"
    );
    // No `.ttc` is vendored, so the "faces of one collection share one buffer" half of the
    // per-file cache is exercised only by system fonts; nothing here can assert it cheaply.
}

use sciink::style::Style;
use sciink::text::fonts::FontSpec;

#[test]
fn font_spec_from_style_follows_upstream_font_style() {
    let st = Style::parse(
        "font-family: 'DejaVu Sans' , Arial ;font-weight:bold;font-style:italic;font-stretch:condensed",
    );
    let spec = FontSpec::from_style(&st);
    assert_eq!(spec.families, ["DejaVu Sans", "Arial"]);
    assert_eq!(spec.weight, 700);
    assert_eq!(spec.style, FontStyle::Italic);
    assert_eq!(spec.width, 3);
    assert_eq!(spec.key(), "'DejaVu Sans','Arial'|700|italic|3");
    let dflt = FontSpec::from_style(&Style::parse("fill:red"));
    assert_eq!(dflt.families, ["sans-serif"]);
    assert_eq!(
        (dflt.weight, dflt.style, dflt.width),
        (400, FontStyle::Normal, 5)
    );
    // numeric weights pass through; unknown keywords (bolder/lighter/semibold) → 400 like Inkscape
    assert_eq!(
        FontSpec::from_style(&Style::parse("font-weight:300")).weight,
        300
    );
    assert_eq!(
        FontSpec::from_style(&Style::parse("font-weight:bolder")).weight,
        400
    );
    assert_eq!(
        FontSpec::from_style(&Style::parse("font-weight:semibold")).weight,
        400
    );
    assert_eq!(
        FontSpec::from_style(&Style::parse("font-style:oblique")).style,
        FontStyle::Oblique
    );
    assert_eq!(
        FontSpec::from_style(&Style::parse("font-stretch:ultra-expanded")).width,
        9
    );
}

#[test]
fn resolution_ladder_with_vendored_fonts_only() {
    let mut fs = fonts();
    let spec = |css: &str| FontSpec::from_style(&Style::parse(css));
    let dv = fs
        .pick(fs.family_faces("DejaVu Sans"), 400, FontStyle::Normal, 5)
        .unwrap();
    let dvb = fs
        .pick(fs.family_faces("DejaVu Sans"), 700, FontStyle::Normal, 5)
        .unwrap();
    let rob = fs
        .pick(fs.family_faces("Roboto"), 400, FontStyle::Normal, 5)
        .unwrap();
    assert_eq!(fs.resolve(&spec("font-family:DejaVu Sans")), Some(dv));
    assert_eq!(
        fs.resolve(&spec("font-family:'dejavu sans';font-weight:bold")),
        Some(dvb)
    );
    assert_eq!(fs.resolve(&spec("font-family:Roboto")), Some(rob));
    // generic sans-serif → first present family of the fontconfig list (DejaVu Sans)
    assert_eq!(fs.resolve(&spec("font-family:sans-serif")), Some(dv));
    // metric alias: Helvetica ↔ Arial ↔ Liberation Sans ↔ Nimbus Sans — none present → generic sans → DejaVu
    assert_eq!(fs.resolve(&spec("font-family:Helvetica")), Some(dv));
    // family list order wins before any fallback
    assert_eq!(
        fs.resolve(&spec("font-family:Nope, Roboto, 'DejaVu Sans'")),
        Some(rob)
    );
    // unknown family, serif and monospace all fall through to sans-serif → DejaVu Sans
    assert_eq!(fs.resolve(&spec("font-family:Zapf Chancery")), Some(dv));
    assert_eq!(fs.resolve(&spec("font-family:serif")), Some(dv));
    assert_eq!(fs.resolve(&spec("font-family:monospace")), Some(dv));
    // per-char fallback: Roboto lacks ⎣, DejaVu has it; nothing has U+10348
    assert_eq!(
        fs.resolve_for_char(&spec("font-family:Roboto"), 'a'),
        Some(rob)
    );
    assert_eq!(
        fs.resolve_for_char(&spec("font-family:Roboto"), '\u{23A3}'),
        Some(dv)
    );
    assert_eq!(
        fs.resolve_for_char(&spec("font-family:Roboto"), '\u{10348}'),
        None
    );
    // the ladder lists every face exactly once
    let c = fs.candidates(&spec("font-family:Roboto;font-weight:bold"));
    assert_eq!(c.len(), 4, "{c:?}");
    assert_eq!(
        c[0],
        fs.pick(fs.family_faces("Roboto"), 700, FontStyle::Normal, 5)
            .unwrap()
    );
    let empty = FontSystem::from_dirs(&[]);
    let mut empty = empty;
    assert_eq!(empty.resolve(&spec("font-family:Arial")), None);
}

#[test]
fn load_scans_the_filesystem_once_per_environment() {
    // `with_vendored_fonts` pins SCIINK_NO_SYSTEM_FONTS / SCIINK_FONT_DIRS for the whole binary
    support::with_vendored_fonts(|| {
        let a = FontSystem::load();
        let scans = sciink::text::fonts::scan_count();
        assert!(scans >= 1, "the first load scanned: {scans}");
        let b = FontSystem::load();
        assert_eq!(
            sciink::text::fonts::scan_count(),
            scans,
            "a second load with the same environment reuses the scan"
        );
        assert_eq!(a.face_count(), b.face_count());
        assert_eq!(a.face_count(), 4, "DejaVu Sans ×2 + Roboto ×2");
        assert!(
            b.load_ms() < a.load_ms() + 1e-9 || b.load_ms() < 50.0,
            "cached load is cheap"
        );
    });
}

#[test]
fn families_lists_each_family_once_in_original_spelling() {
    let fs = fonts();
    assert_eq!(
        fs.families(),
        vec!["DejaVu Sans".to_string(), "Roboto".to_string()]
    );
}

use sciink::text::fonts::ScanKey;

fn bundled_copy(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sciink-bundled-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf");
    std::fs::copy(src, dir.join("DejaVuSans.ttf")).unwrap();
    dir
}

#[test]
fn an_installed_face_wins_a_tie_against_a_bundled_face_of_the_same_metadata() {
    let bundled = bundled_copy("tie");
    let mut fs = FontSystem::scan_with_cache(
        &ScanKey {
            system: false,
            dirs: vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")],
            bundled: Some(bundled.clone()),
        },
        None,
    );
    assert_eq!(
        fs.face_count(),
        5,
        "4 vendored + 1 bundled copy of DejaVu Sans Book"
    );
    let spec = sciink::text::fonts::FontSpec::from_style(&sciink::style::Style::parse(
        "font-family:'DejaVu Sans'",
    ));
    let k = fs.resolve(&spec).expect("DejaVu Sans resolves");
    assert!(!fs.is_bundled(k), "the non-bundled face wins the tie");
    assert!(
        fs.face_info(k)
            .path
            .as_ref()
            .unwrap()
            .starts_with(env!("CARGO_MANIFEST_DIR"))
    );
}

#[test]
fn is_bundled_reports_the_source_directory() {
    let bundled = bundled_copy("flag");
    let fs = FontSystem::scan_with_cache(
        &ScanKey {
            system: false,
            dirs: vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")],
            bundled: Some(bundled.clone()),
        },
        None,
    );
    let flagged: Vec<_> = fs.faces().filter(|&k| fs.is_bundled(k)).collect();
    assert_eq!(flagged.len(), 1);
    assert!(
        fs.face_info(flagged[0])
            .path
            .as_ref()
            .unwrap()
            .starts_with(&bundled)
    );
}
