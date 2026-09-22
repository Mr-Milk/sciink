//! Font discovery and selection (spec §A.3). fontdb enumerates faces; matching is
//! ours so it is case-insensitive and follows CSS weight rules like fontconfig does.

use crate::style::Style;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceKey(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone)]
pub struct FaceInfo {
    pub family: String,
    pub path: Option<PathBuf>,
    pub index: u32,
    pub weight: u16,
    pub style: FontStyle,
    pub width: u16,
    pub upem: f64,
    pub ascent: f64,
    pub descent: f64,
    pub ascent_max: f64,
    pub descent_max: f64,
    pub x_height: f64,
    pub cap_height: f64,
}

pub struct FontSystem {
    db: fontdb::Database,
    ids: Vec<fontdb::ID>,
    infos: Vec<FaceInfo>,
    by_family: HashMap<String, Vec<FaceKey>>,
    data: RefCell<HashMap<FaceKey, Rc<Vec<u8>>>>,
    /// Bytes keyed by source file, so the faces of a `.ttc` collection share one copy.
    file_data: RefCell<HashMap<PathBuf, Rc<Vec<u8>>>>,
    load_ms: f64,
    ladder_memo: HashMap<FontSpec, Vec<FaceKey>>,
    char_memo: HashMap<(FontSpec, char), Option<FaceKey>>,
}

/// One filesystem scan per environment: the scan (`load_system_fonts`/`load_fonts_dir`) and the
/// face pass (`FaceInfo` per face, which re-reads every font file for its metrics) are the whole
/// cost of `load()`, and a tool run may build two font systems (`remove_kerning` and the bbox
/// stage's `Ctx`); later loads clone the first result (`Database` and `FaceInfo` are `Clone`).
type ScanKey = (bool, Vec<PathBuf>);
type Scan = (fontdb::Database, Vec<(fontdb::ID, FaceInfo)>);
static SCANS: OnceLock<Mutex<HashMap<ScanKey, Arc<Scan>>>> = OnceLock::new();
static SCAN_COUNT: AtomicUsize = AtomicUsize::new(0);

/// How many filesystem font scans this process has run (tests; About prints it).
pub fn scan_count() -> usize {
    SCAN_COUNT.load(Ordering::SeqCst)
}

fn scan_key() -> ScanKey {
    let system = std::env::var_os("SCIINK_NO_SYSTEM_FONTS").is_none_or(|v| v != "1");
    let dirs = std::env::var_os("SCIINK_FONT_DIRS")
        .map(|d| std::env::split_paths(&d).collect())
        .unwrap_or_default();
    (system, dirs)
}

/// The scan for the current environment, from the cache or freshly made (and then cached).
fn scanned() -> Arc<Scan> {
    let key = scan_key();
    let cache = SCANS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = guard.get(&key) {
        return s.clone();
    }
    SCAN_COUNT.fetch_add(1, Ordering::SeqCst);
    let mut db = fontdb::Database::new();
    if key.0 {
        db.load_system_fonts();
    }
    for d in &key.1 {
        db.load_fonts_dir(d);
    }
    let entries = FontSystem::scan_entries(&db);
    let scan = Arc::new((db, entries));
    guard.insert(key, scan.clone());
    scan
}

impl FontSystem {
    /// System fonts (unless `SCIINK_NO_SYSTEM_FONTS=1`) plus every dir in `SCIINK_FONT_DIRS`.
    /// The filesystem is scanned once per process and environment (`scan_count`).
    pub fn load() -> FontSystem {
        let t0 = Instant::now();
        let scan = scanned();
        Self::from_entries(scan.0.clone(), scan.1.clone(), t0)
    }

    /// Only the given directories (tests); never cached.
    pub fn from_dirs(dirs: &[PathBuf]) -> FontSystem {
        let t0 = Instant::now();
        let mut db = fontdb::Database::new();
        for d in dirs {
            db.load_fonts_dir(d);
        }
        let entries = Self::scan_entries(&db);
        Self::from_entries(db, entries, t0)
    }

    /// The face pass: one `FaceInfo` per parsable face, sorted by (family, weight, style, width,
    /// path, index). This is the part of `from_db` up to and including `entries.sort_by(...)`.
    fn scan_entries(db: &fontdb::Database) -> Vec<(fontdb::ID, FaceInfo)> {
        let mut entries: Vec<(fontdb::ID, FaceInfo)> = Vec::new();
        for f in db.faces() {
            let family = f
                .families
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| f.post_script_name.clone());
            let (path, index) = match &f.source {
                fontdb::Source::File(p) => (Some(p.clone()), f.index),
                fontdb::Source::SharedFile(p, _) => (Some(p.clone()), f.index),
                fontdb::Source::Binary(_) => (None, f.index),
            };
            let style = match f.style {
                fontdb::Style::Normal => FontStyle::Normal,
                fontdb::Style::Italic => FontStyle::Italic,
                fontdb::Style::Oblique => FontStyle::Oblique,
            };
            let metrics = db.with_face_data(f.id, |data, idx| {
                ttf_parser::Face::parse(data, idx)
                    .ok()
                    .map(|face| face_metrics(&face))
            });
            let Some(Some(m)) = metrics else { continue }; // unparsable face: skip
            entries.push((
                f.id,
                FaceInfo {
                    family,
                    path,
                    index,
                    weight: f.weight.0,
                    style,
                    width: f.stretch.to_number(),
                    upem: m.0,
                    ascent: m.1,
                    descent: m.2,
                    ascent_max: m.3,
                    descent_max: m.4,
                    x_height: m.5,
                    cap_height: m.6,
                },
            ));
        }
        entries.sort_by(|a, b| {
            let ka = (
                &a.1.family,
                a.1.weight,
                a.1.style as u8,
                a.1.width,
                &a.1.path,
                a.1.index,
            );
            let kb = (
                &b.1.family,
                b.1.weight,
                b.1.style as u8,
                b.1.width,
                &b.1.path,
                b.1.index,
            );
            ka.cmp(&kb)
        });
        entries
    }

    /// The rest of the old `from_db`: `by_family` index and the struct literal.
    fn from_entries(
        db: fontdb::Database,
        entries: Vec<(fontdb::ID, FaceInfo)>,
        t0: Instant,
    ) -> FontSystem {
        let mut by_family: HashMap<String, Vec<FaceKey>> = HashMap::new();
        for (i, (id, _)) in entries.iter().enumerate() {
            if let Some(f) = db.face(*id) {
                for (name, _) in &f.families {
                    by_family
                        .entry(name.trim().to_lowercase())
                        .or_default()
                        .push(FaceKey(i as u32));
                }
            }
        }
        for v in by_family.values_mut() {
            v.sort();
            v.dedup();
        }
        let (ids, infos): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        FontSystem {
            db,
            ids,
            infos,
            by_family,
            data: RefCell::new(HashMap::new()),
            file_data: RefCell::new(HashMap::new()),
            load_ms: t0.elapsed().as_secs_f64() * 1000.0,
            ladder_memo: HashMap::new(),
            char_memo: HashMap::new(),
        }
    }

    pub fn face_count(&self) -> usize {
        self.infos.len()
    }

    /// Every family name once, in its original spelling, sorted case-insensitively.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for info in &self.infos {
            if !v.iter().any(|f| f.eq_ignore_ascii_case(&info.family)) {
                v.push(info.family.clone());
            }
        }
        v.sort_by_key(|f| f.to_lowercase());
        v
    }

    pub fn load_ms(&self) -> f64 {
        self.load_ms
    }

    pub fn face_info(&self, k: FaceKey) -> &FaceInfo {
        &self.infos[k.0 as usize]
    }

    pub fn faces(&self) -> impl Iterator<Item = FaceKey> + '_ {
        (0..self.infos.len() as u32).map(FaceKey)
    }

    /// Faces whose font reports `family` (case-insensitive); empty when unknown.
    pub fn family_faces(&self, family: &str) -> &[FaceKey] {
        self.by_family
            .get(&family.trim().to_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// CSS Fonts §5.2 matching: width, then style (italic↔oblique, then normal), then weight.
    pub fn pick(
        &self,
        cands: &[FaceKey],
        weight: u16,
        style: FontStyle,
        width: u16,
    ) -> Option<FaceKey> {
        if cands.is_empty() {
            return None;
        }
        // width: nearest; ties prefer narrower when width <= 5, wider otherwise
        let best_w = cands
            .iter()
            .map(|&k| {
                let w = self.face_info(k).width;
                let d = (w as i32 - width as i32).abs();
                let side = if width <= 5 {
                    (w > width) as i32
                } else {
                    (w < width) as i32
                };
                (d, side)
            })
            .min()?;
        let cands: Vec<FaceKey> = cands
            .iter()
            .copied()
            .filter(|&k| {
                let w = self.face_info(k).width;
                let d = (w as i32 - width as i32).abs();
                let side = if width <= 5 {
                    (w > width) as i32
                } else {
                    (w < width) as i32
                };
                (d, side) == best_w
            })
            .collect();
        let style_pref: &[FontStyle] = match style {
            FontStyle::Normal => &[FontStyle::Normal, FontStyle::Oblique, FontStyle::Italic],
            FontStyle::Italic => &[FontStyle::Italic, FontStyle::Oblique, FontStyle::Normal],
            FontStyle::Oblique => &[FontStyle::Oblique, FontStyle::Italic, FontStyle::Normal],
        };
        let styled: Vec<FaceKey> = style_pref
            .iter()
            .find_map(|s| {
                let v: Vec<FaceKey> = cands
                    .iter()
                    .copied()
                    .filter(|&k| self.face_info(k).style == *s)
                    .collect();
                (!v.is_empty()).then_some(v)
            })
            .unwrap_or(cands);
        let ws: Vec<(u16, FaceKey)> = styled
            .iter()
            .map(|&k| (self.face_info(k).weight, k))
            .collect();
        let exact = ws.iter().find(|(w, _)| *w == weight).map(|(_, k)| *k);
        if exact.is_some() {
            return exact;
        }
        let lighter = || {
            ws.iter()
                .filter(|(w, _)| *w < weight)
                .max_by_key(|(w, _)| *w)
                .map(|(_, k)| *k)
        };
        let heavier = || {
            ws.iter()
                .filter(|(w, _)| *w > weight)
                .min_by_key(|(w, _)| *w)
                .map(|(_, k)| *k)
        };
        if (400..=500).contains(&weight) {
            let up_to_500 = ws
                .iter()
                .filter(|(w, _)| *w > weight && *w <= 500)
                .min_by_key(|(w, _)| *w)
                .map(|(_, k)| *k);
            up_to_500.or_else(lighter).or_else(heavier)
        } else if weight < 400 {
            lighter().or_else(heavier)
        } else {
            heavier().or_else(lighter)
        }
    }

    /// Font bytes and face index, read once and cached.
    ///
    /// The bytes are cached per *source file*, not per face: every face of a `.ttc`
    /// collection is backed by the whole file, so keying by `FaceKey` alone would keep
    /// one private copy of the collection per face (24 faces share one `PingFang.ttc` on
    /// macOS). Faces with no path (`Source::Binary`) keep a per-face entry.
    pub fn face_data(&self, k: FaceKey) -> Option<(Rc<Vec<u8>>, u32)> {
        let info = self.face_info(k);
        if let Some(d) = self.data.borrow().get(&k) {
            return Some((d.clone(), info.index));
        }
        if let Some(p) = &info.path {
            if let Some(d) = self.file_data.borrow().get(p) {
                self.data.borrow_mut().insert(k, d.clone());
                return Some((d.clone(), info.index));
            }
        }
        let bytes = self
            .db
            .with_face_data(self.ids[k.0 as usize], |data, _| data.to_vec())?;
        let rc = Rc::new(bytes);
        if let Some(p) = &info.path {
            self.file_data.borrow_mut().insert(p.clone(), rc.clone());
        }
        self.data.borrow_mut().insert(k, rc.clone());
        Some((rc, info.index))
    }

    pub fn with_face<T>(
        &self,
        k: FaceKey,
        f: impl FnOnce(&ttf_parser::Face<'_>) -> T,
    ) -> Option<T> {
        let (data, index) = self.face_data(k)?;
        let face = ttf_parser::Face::parse(&data, index).ok()?;
        Some(f(&face))
    }

    pub fn has_glyph(&self, k: FaceKey, c: char) -> bool {
        self.with_face(k, |f| f.glyph_index(c).is_some())
            .unwrap_or(false)
    }
}

/// Port of Inkscape's `find_font_metrics` (spec §A.3): returns
/// (upem, ascent, descent, ascent_max, descent_max, x_height, cap_height).
fn face_metrics(face: &ttf_parser::Face<'_>) -> (f64, f64, f64, f64, f64, f64, f64) {
    let upem = face.units_per_em() as f64;
    let (mut asc, mut desc) = match (face.typographic_ascender(), face.typographic_descender()) {
        (Some(a), Some(d)) => ((a as f64 / upem).abs(), (d as f64 / upem).abs()),
        _ => (
            (face.ascender() as f64 / upem).abs(),
            (face.descender() as f64 / upem).abs(),
        ),
    };
    let asc_max = (face.ascender() as f64 / upem).abs();
    let desc_max = (face.descender() as f64 / upem).abs();
    let em = asc + desc;
    if em > 0.0 {
        asc /= em;
        desc /= em;
    }
    let x_height = match face.x_height() {
        Some(x) if x != 0 => (x as f64 / upem).abs(),
        _ => face
            .glyph_index('x')
            .and_then(|g| face.glyph_bounding_box(g))
            .map(|b| (b.y_max as f64 / upem).abs())
            .unwrap_or(0.5),
    };
    let cap_height = face
        .glyph_index('I')
        .and_then(|g| face.glyph_bounding_box(g))
        .map(|b| b.y_max as f64 / upem)
        .filter(|v| *v > 0.0)
        .or_else(|| {
            face.capital_height()
                .filter(|v| *v != 0)
                .map(|v| v as f64 / upem)
        })
        .unwrap_or(0.7);
    (upem, asc, desc, asc_max, desc_max, x_height, cap_height)
}

/// fontconfig 60-latin.conf preference order (spec §A.3 step 3).
pub const GENERIC_SANS: &[&str] = &[
    "DejaVu Sans",
    "Bitstream Vera Sans",
    "Verdana",
    "Arial",
    "Albany AMT",
    "Luxi Sans",
    "Nimbus Sans L",
    "Nimbus Sans",
    "Helvetica",
    "Lucida Sans Unicode",
    "Tahoma",
    "Noto Sans",
];
pub const GENERIC_SERIF: &[&str] = &[
    "DejaVu Serif",
    "Bitstream Vera Serif",
    "Times New Roman",
    "Thorndale AMT",
    "Luxi Serif",
    "Nimbus Roman No9 L",
    "Nimbus Roman",
    "Times",
    "Noto Serif",
];
pub const GENERIC_MONO: &[&str] = &[
    "DejaVu Sans Mono",
    "Bitstream Vera Sans Mono",
    "Inconsolata",
    "Andale Mono",
    "Courier New",
    "Cumberland AMT",
    "Luxi Mono",
    "Nimbus Mono L",
    "Nimbus Mono PS",
    "Courier",
    "Noto Sans Mono",
];
/// fontconfig 30-metric-aliases.conf groups (spec §A.3 step 2).
pub const METRIC_ALIASES: &[&[&str]] = &[
    &[
        "Helvetica",
        "Arial",
        "Liberation Sans",
        "Nimbus Sans",
        "Nimbus Sans L",
        "Arimo",
        "Albany",
        "Albany AMT",
    ],
    &[
        "Times",
        "Times New Roman",
        "Liberation Serif",
        "Nimbus Roman",
        "Nimbus Roman No9 L",
        "Tinos",
        "Thorndale",
        "Thorndale AMT",
    ],
    &[
        "Courier",
        "Courier New",
        "Liberation Mono",
        "Nimbus Mono",
        "Nimbus Mono L",
        "Nimbus Mono PS",
        "Cousine",
        "Cumberland",
        "Cumberland AMT",
    ],
    &["Calibri", "Carlito"],
    &["Cambria", "Caladea"],
    &["Georgia", "Gelasio"],
];
/// Curated wide-coverage fallbacks tried before "any face".
pub const WIDE_COVERAGE: &[&str] = &[
    "Noto Sans",
    "Noto Sans Symbols",
    "Noto Sans Symbols 2",
    "Noto Sans Math",
    "DejaVu Sans",
    "Arial Unicode MS",
    "Segoe UI Symbol",
    "Cambria Math",
    "Apple Symbols",
    "STIX Two Math",
    "Symbola",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontSpec {
    pub families: Vec<String>,
    pub weight: u16,
    pub style: FontStyle,
    pub width: u16,
}

impl FontSpec {
    /// The four properties that select a font (upstream `font_style`, FP:357–370).
    pub fn from_style(st: &Style) -> FontSpec {
        let fam = st.get("font-family").unwrap_or("sans-serif");
        let families: Vec<String> = fam
            .split(',')
            .map(|f| {
                f.trim()
                    .trim_matches(|c| c == '\'' || c == '"')
                    .trim()
                    .to_string()
            })
            .filter(|f| !f.is_empty())
            .collect();
        let families = if families.is_empty() {
            vec!["sans-serif".to_string()]
        } else {
            families
        };
        let weight = match st.get("font-weight").map(str::trim).unwrap_or("normal") {
            "bold" => 700,
            w => w
                .parse::<u16>()
                .ok()
                .filter(|v| (100..=1000).contains(v) && v % 50 == 0)
                .unwrap_or(400), // normal, bolder, lighter, semibold, … → Inkscape uses normal
        };
        let style = match st.get("font-style").map(str::trim).unwrap_or("normal") {
            "italic" => FontStyle::Italic,
            "oblique" => FontStyle::Oblique,
            _ => FontStyle::Normal,
        };
        let width = match st.get("font-stretch").map(str::trim).unwrap_or("normal") {
            "ultra-condensed" => 1,
            "extra-condensed" => 2,
            "condensed" => 3,
            "semi-condensed" => 4,
            "semi-expanded" => 6,
            "expanded" => 7,
            "extra-expanded" => 8,
            "ultra-expanded" => 9,
            _ => 5,
        };
        FontSpec {
            families,
            weight,
            style,
            width,
        }
    }

    /// Upstream's `fsty` key: quoted, comma-joined families plus weight/style/width.
    pub fn key(&self) -> String {
        let fams: Vec<String> = self.families.iter().map(|f| format!("'{f}'")).collect();
        let sty = match self.style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        };
        format!("{}|{}|{}|{}", fams.join(","), self.weight, sty, self.width)
    }
}

fn generic_list(family: &str) -> Option<&'static [&'static str]> {
    match family.to_ascii_lowercase().as_str() {
        "sans-serif" | "sans" | "system-ui" | "ui-sans-serif" => Some(GENERIC_SANS),
        "serif" | "ui-serif" => Some(GENERIC_SERIF),
        "monospace" | "mono" | "ui-monospace" => Some(GENERIC_MONO),
        "cursive" | "fantasy" => Some(GENERIC_SANS),
        _ => None,
    }
}

impl FontSystem {
    fn push_family(&self, out: &mut Vec<FaceKey>, family: &str, spec: &FontSpec) {
        if let Some(k) = self.pick(
            self.family_faces(family),
            spec.weight,
            spec.style,
            spec.width,
        ) {
            if !out.contains(&k) {
                out.push(k);
            }
        }
    }

    /// The whole ordered fallback ladder for `spec` (spec §A.3), each face once.
    pub fn candidates(&self, spec: &FontSpec) -> Vec<FaceKey> {
        let mut out = Vec::new();
        for fam in &spec.families {
            self.push_family(&mut out, fam, spec);
            for group in METRIC_ALIASES {
                if group.iter().any(|g| g.eq_ignore_ascii_case(fam)) {
                    for g in *group {
                        self.push_family(&mut out, g, spec);
                    }
                }
            }
            if let Some(list) = generic_list(fam) {
                for g in list {
                    self.push_family(&mut out, g, spec);
                }
            }
        }
        for g in GENERIC_SANS.iter().chain(WIDE_COVERAGE) {
            self.push_family(&mut out, g, spec);
        }
        // last resort: every remaining face, same style first, nearest weight, then family.
        // `out` can already hold every family/generic/alias face tried above, so snapshot it
        // into a set once rather than doing an O(L) `Vec::contains` scan per candidate face.
        let seen: HashSet<FaceKey> = out.iter().copied().collect();
        let mut rest: Vec<FaceKey> = self.faces().filter(|k| !seen.contains(k)).collect();
        rest.sort_by_key(|&k| {
            let i = self.face_info(k);
            (
                (i.style != spec.style) as u8,
                (i.weight as i32 - spec.weight as i32).abs(),
                i.family.clone(),
                k,
            )
        });
        out.extend(rest);
        out
    }

    /// The face Inkscape would pick for the whole run (`true_style`).
    pub fn resolve(&mut self, spec: &FontSpec) -> Option<FaceKey> {
        self.ladder(spec).first().copied()
    }

    /// The face that actually renders `c` under `spec`; `None` = no installed font has the glyph.
    pub fn resolve_for_char(&mut self, spec: &FontSpec, c: char) -> Option<FaceKey> {
        if let Some(r) = self.char_memo.get(&(spec.clone(), c)) {
            return *r;
        }
        let r = self
            .ladder(spec)
            .iter()
            .copied()
            .find(|&k| self.has_glyph(k, c));
        self.char_memo.insert((spec.clone(), c), r);
        r
    }

    fn ladder(&mut self, spec: &FontSpec) -> Vec<FaceKey> {
        if let Some(v) = self.ladder_memo.get(spec) {
            return v.clone();
        }
        let v = self.candidates(spec);
        self.ladder_memo.insert(spec.clone(), v.clone());
        v
    }
}
