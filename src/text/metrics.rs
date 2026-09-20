//! Character measurement with rustybuzz (shaping = HarfBuzz = what Pango uses) and
//! ttf-parser (outlines). All values in em units; callers scale by the font size (spec §A.3).

use std::collections::HashMap;
use std::rc::Rc;

use super::fonts::{FaceKey, FontSystem};

#[derive(Debug, Clone, PartialEq)]
pub struct CProp {
    pub c: char,
    pub charw: f64,
    pub spacew: f64,
    pub caph: f64,
    pub inkbb: [f64; 4],
    pub dadvs: HashMap<char, f64>,
}

#[derive(Default)]
pub struct Metrics {
    adv: HashMap<(FaceKey, String), f64>,
    ink: HashMap<(FaceKey, char), [f64; 4]>,
    pairs: HashMap<(FaceKey, char, char), f64>,
    props: HashMap<(FaceKey, char), Rc<CProp>>,
}

struct NoopBuilder;
impl ttf_parser::OutlineBuilder for NoopBuilder {
    fn move_to(&mut self, _x: f32, _y: f32) {}
    fn line_to(&mut self, _x: f32, _y: f32) {}
    fn quad_to(&mut self, _x1: f32, _y1: f32, _x: f32, _y: f32) {}
    fn curve_to(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x: f32, _y: f32) {}
    fn close(&mut self) {}
}

impl Metrics {
    pub fn new() -> Metrics {
        Metrics::default()
    }

    /// Shaped advance of `s` in em (default features: kerning and standard ligatures on).
    pub fn advance(&mut self, fs: &FontSystem, k: FaceKey, s: &str) -> f64 {
        if let Some(v) = self.adv.get(&(k, s.to_string())) {
            return *v;
        }
        let v = fs
            .face_data(k)
            .and_then(|(data, index)| {
                let face = rustybuzz::Face::from_slice(&data, index)?;
                let mut buf = rustybuzz::UnicodeBuffer::new();
                buf.push_str(s);
                let out = rustybuzz::shape(&face, &[], buf);
                let total: i64 = out
                    .glyph_positions()
                    .iter()
                    .map(|p| p.x_advance as i64)
                    .sum();
                Some(total as f64 / face.units_per_em() as f64)
            })
            .unwrap_or(0.0);
        self.adv.insert((k, s.to_string()), v);
        v
    }

    fn ink(&mut self, fs: &FontSystem, k: FaceKey, c: char) -> [f64; 4] {
        if let Some(v) = self.ink.get(&(k, c)) {
            return *v;
        }
        let v = fs
            .with_face(k, |f| {
                let upem = f.units_per_em() as f64;
                f.glyph_index(c)
                    .and_then(|g| f.outline_glyph(g, &mut NoopBuilder))
                    .map(|r| {
                        [
                            r.x_min as f64 / upem,
                            -(r.y_max as f64) / upem,
                            (r.x_max as f64 - r.x_min as f64) / upem,
                            (r.y_max as f64 - r.y_min as f64) / upem,
                        ]
                    })
                    .unwrap_or([0.0; 4])
            })
            .unwrap_or([0.0; 4]);
        self.ink.insert((k, c), v);
        v
    }

    /// adv(prev + c) − adv(prev) − adv(c): GPOS kerning and ligature effects (spec §A.3).
    pub fn pair_adv(&mut self, fs: &FontSystem, k: FaceKey, prev: char, c: char) -> f64 {
        if let Some(v) = self.pairs.get(&(k, prev, c)) {
            return *v;
        }
        let mut s = String::new();
        s.push(prev);
        s.push(c);
        let v = self.advance(fs, k, &s)
            - self.advance(fs, k, &prev.to_string())
            - self.advance(fs, k, &c.to_string());
        let v = if v.is_finite() { v } else { 0.0 };
        self.pairs.insert((k, prev, c), v);
        v
    }

    /// Properties of `c` in face `k`, with `dadvs` for every `prev` requested (union across calls).
    pub fn prop(&mut self, fs: &FontSystem, k: FaceKey, c: char, prev: &[char]) -> Rc<CProp> {
        let need_more = match self.props.get(&(k, c)) {
            Some(p) => prev.iter().any(|q| !p.dadvs.contains_key(q)),
            None => true,
        };
        if !need_more {
            return self.props[&(k, c)].clone();
        }
        let mut dadvs = self
            .props
            .get(&(k, c))
            .map(|p| p.dadvs.clone())
            .unwrap_or_default();
        for &q in prev {
            let v = self.pair_adv(fs, k, q, c);
            dadvs.insert(q, v);
        }
        let p = Rc::new(CProp {
            c,
            charw: self.advance(fs, k, &c.to_string()),
            spacew: self.advance(fs, k, " "),
            caph: fs.face_info(k).cap_height,
            inkbb: self.ink(fs, k, c),
            dadvs,
        });
        self.props.insert((k, c), p.clone());
        p
    }

    /// Placeholder for a character no installed font can draw.
    pub fn unrendered(c: char) -> Rc<CProp> {
        Rc::new(CProp {
            c,
            charw: 0.0,
            spacew: 0.0,
            caph: 0.0,
            inkbb: [0.0; 4],
            dadvs: HashMap::new(),
        })
    }
}
