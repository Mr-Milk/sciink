//! The on-disk font-scan cache: written cold, read warm with no font file opened, invalidated by
//! any change to the font files or their directories, and never able to fail a scan.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use sciink::text::fontcache::{FONT_CACHE_FORMAT, cache_path, read};
use sciink::text::fonts::{FontSystem, ScanKey, cache_events, face_open_count};

static SERIAL: Mutex<()> = Mutex::new(());

fn vendored() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")
}

/// A fresh directory with copies of the vendored fonts (so tests may touch, add and remove files),
/// and a cache file path inside a sibling directory.
fn sandbox(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("sciink-fc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let fonts = root.join("fonts");
    std::fs::create_dir_all(&fonts).unwrap();
    for e in std::fs::read_dir(vendored()).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "ttf") {
            std::fs::copy(e.path(), fonts.join(e.file_name())).unwrap();
        }
    }
    (fonts, root.join("cache").join("fontcache-test.tsv"))
}

fn key(fonts: &Path) -> ScanKey {
    ScanKey {
        system: false,
        dirs: vec![fonts.to_path_buf()],
        bundled: None,
    }
}

fn touch(p: &Path) {
    let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(SystemTime::now() + Duration::from_secs(5))
        .unwrap();
}

#[test]
fn a_cold_scan_writes_the_cache_and_a_warm_scan_opens_no_font_file() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("cold-warm");
    let (h0, m0, w0) = cache_events();
    let fresh = FontSystem::scan_with_cache(&key(&fonts), Some(&cache));
    assert_eq!(fresh.face_count(), 4);
    assert_eq!(
        cache_events(),
        (h0, m0 + 1, w0 + 1),
        "cold: one miss, one write"
    );
    assert!(cache.is_file());
    let opens = face_open_count();
    let warm = FontSystem::scan_with_cache(&key(&fonts), Some(&cache));
    assert_eq!(
        cache_events(),
        (h0 + 1, m0 + 1, w0 + 1),
        "warm: one hit, no write"
    );
    assert_eq!(
        face_open_count(),
        opens,
        "the warm scan opened no font file"
    );
    assert_eq!(warm.face_count(), 4);
}

#[test]
fn metrics_from_the_cache_equal_metrics_from_a_fresh_scan() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("metrics");
    let fresh = FontSystem::scan_with_cache(&key(&fonts), None);
    FontSystem::scan_with_cache(&key(&fonts), Some(&cache)); // writes
    let mut cached = FontSystem::scan_with_cache(&key(&fonts), Some(&cache)); // reads
    assert_eq!(fresh.face_count(), cached.face_count());
    for k in fresh.faces() {
        let (a, b) = (fresh.face_info(k), cached.face_info(k));
        assert_eq!(a, b, "face {k:?} differs between fresh and cached scans");
        for (x, y) in [
            (a.upem, b.upem),
            (a.ascent, b.ascent),
            (a.descent, b.descent),
            (a.ascent_max, b.ascent_max),
            (a.descent_max, b.descent_max),
            (a.x_height, b.x_height),
            (a.cap_height, b.cap_height),
        ] {
            assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "metric not bit-identical for {k:?}"
            );
        }
    }
    assert_eq!(
        fresh.family_faces("dejavu sans"),
        cached.family_faces("dejavu sans")
    );
    // the cached system can still measure: resolving a family finds a face
    let spec = sciink::text::fonts::FontSpec::from_style(&sciink::style::Style::parse(
        "font-family:'DejaVu Sans'",
    ));
    assert!(cached.resolve(&spec).is_some());
}

#[test]
fn touching_adding_or_removing_a_font_file_invalidates_the_cache() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("invalidate");
    let k = key(&fonts);
    FontSystem::scan_with_cache(&k, Some(&cache));
    let (h, m, w) = cache_events();
    touch(&fonts.join("DejaVuSans.ttf"));
    FontSystem::scan_with_cache(&k, Some(&cache));
    assert_eq!(
        cache_events(),
        (h, m + 1, w + 1),
        "a touched file forces a rescan and rewrite"
    );
    std::fs::copy(vendored().join("Roboto-Bold.ttf"), fonts.join("Extra.ttf")).unwrap();
    let fs = FontSystem::scan_with_cache(&k, Some(&cache));
    assert_eq!(
        cache_events(),
        (h, m + 2, w + 2),
        "a new file changes the directory mtime"
    );
    assert_eq!(fs.face_count(), 5);
    std::fs::remove_file(fonts.join("Extra.ttf")).unwrap();
    let fs = FontSystem::scan_with_cache(&k, Some(&cache));
    assert_eq!(
        cache_events(),
        (h, m + 3, w + 3),
        "a removed file is noticed"
    );
    assert_eq!(fs.face_count(), 4);
    FontSystem::scan_with_cache(&k, Some(&cache));
    assert_eq!(cache_events(), (h + 1, m + 3, w + 3), "stable again: a hit");
}

#[test]
fn a_corrupt_cache_file_is_ignored_and_rewritten() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("corrupt");
    let k = key(&fonts);
    FontSystem::scan_with_cache(&k, Some(&cache));
    let good = std::fs::read_to_string(&cache).unwrap();
    let corruptions: Vec<String> = vec![
        good[..good.len() / 2].to_string(),           // truncated mid-line
        good.replacen("\t2048", "\tnot-a-number", 1), // garbage number
        good.replacen(&format!("\t{FONT_CACHE_FORMAT}\t"), "\t999\t", 1), // wrong format
        String::new(),                                // empty
        (0..1_000_000u32)
            .map(|i| (b'!' + (i % 90) as u8) as char)
            .collect(), // 1 MB of noise
    ];
    for (i, c) in corruptions.iter().enumerate() {
        std::fs::write(&cache, c).unwrap();
        assert!(read(&cache, &k).is_none(), "corruption {i} accepted");
        let (h, m, w) = cache_events();
        let fs = FontSystem::scan_with_cache(&k, Some(&cache));
        assert_eq!(fs.face_count(), 4, "corruption {i}");
        assert_eq!(
            cache_events(),
            (h, m + 1, w + 1),
            "corruption {i}: rescanned and rewritten"
        );
        assert!(
            read(&cache, &k).is_some(),
            "corruption {i}: rewritten file is valid"
        );
    }
}

#[test]
fn a_different_scan_key_does_not_reuse_the_cache() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("key");
    FontSystem::scan_with_cache(&key(&fonts), Some(&cache));
    let other = ScanKey {
        system: false,
        dirs: vec![fonts.clone(), fonts.parent().unwrap().to_path_buf()],
        bundled: None,
    };
    let (h, m, w) = cache_events();
    FontSystem::scan_with_cache(&other, Some(&cache));
    assert_eq!(cache_events(), (h, m + 1, w + 1));
    assert!(
        read(&cache, &key(&fonts)).is_none(),
        "the file now belongs to the other key"
    );
}

#[test]
fn an_unwritable_cache_location_does_not_fail_the_scan() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, _) = sandbox("unwritable");
    let bad = PathBuf::from("/nonexistent-sciink-dir/deeper/fontcache.tsv");
    let fs = FontSystem::scan_with_cache(&key(&fonts), Some(&bad));
    assert_eq!(fs.face_count(), 4);
}

#[test]
fn the_cache_path_honours_the_environment_switches() {
    let _g = SERIAL.lock().unwrap();
    // SAFETY: tests in this binary are serialised by SERIAL and restore the variables.
    unsafe {
        std::env::set_var("SCIINK_NO_FONT_CACHE", "1");
        assert_eq!(cache_path(), None);
        std::env::remove_var("SCIINK_NO_FONT_CACHE");
        std::env::set_var("SCIINK_FONT_CACHE", "/tmp/x.tsv");
        assert_eq!(cache_path(), Some(PathBuf::from("/tmp/x.tsv")));
        std::env::remove_var("SCIINK_FONT_CACHE");
    }
    assert!(
        cache_path().is_some_and(|p| p.ends_with(format!("fontcache-{FONT_CACHE_FORMAT}.tsv")))
    );
}
