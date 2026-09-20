//! Font discovery and selection (spec §A.3). fontdb enumerates faces; matching is
//! ours so it is case-insensitive and follows CSS weight rules like fontconfig does.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
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
    load_ms: f64,
}

impl FontSystem {
    /// System fonts (unless `SCIINK_NO_SYSTEM_FONTS=1`) plus every dir in `SCIINK_FONT_DIRS`.
    pub fn load() -> FontSystem {
        let mut db = fontdb::Database::new();
        if std::env::var_os("SCIINK_NO_SYSTEM_FONTS").is_none_or(|v| v != "1") {
            db.load_system_fonts();
        }
        if let Some(dirs) = std::env::var_os("SCIINK_FONT_DIRS") {
            for d in std::env::split_paths(&dirs) {
                db.load_fonts_dir(d);
            }
        }
        Self::from_db(db)
    }

    /// Only the given directories (tests).
    pub fn from_dirs(dirs: &[PathBuf]) -> FontSystem {
        let mut db = fontdb::Database::new();
        for d in dirs {
            db.load_fonts_dir(d);
        }
        Self::from_db(db)
    }

    fn from_db(db: fontdb::Database) -> FontSystem {
        let t0 = Instant::now();
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
            load_ms: t0.elapsed().as_secs_f64() * 1000.0,
        }
    }

    pub fn face_count(&self) -> usize {
        self.infos.len()
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
    pub fn face_data(&self, k: FaceKey) -> Option<(Rc<Vec<u8>>, u32)> {
        let info = self.face_info(k);
        if let Some(d) = self.data.borrow().get(&k) {
            return Some((d.clone(), info.index));
        }
        let bytes = self
            .db
            .with_face_data(self.ids[k.0 as usize], |data, _| data.to_vec())?;
        let rc = Rc::new(bytes);
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
