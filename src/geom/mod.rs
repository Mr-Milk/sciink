//! Geometry primitives shared by every tool (spec §B.1). kurbo's
//! `Affine::new([a,b,c,d,e,f])` has exactly the SVG `matrix(a b c d e f)` meaning;
//! upstream's `A @ B` is kurbo's `A * B` (apply `B` first) and `-A` is `inverse(A)`.

pub mod path;

use std::str::FromStr;

pub use kurbo::{Affine, BezPath, Point, Rect, Vec2};

use crate::num;

/// Transform components closer than this are equal (`inkex/transforms.py`).
pub const TOL: f64 = 1e-5;

/// Length with unit → px. `%`, `em`, `ex` and unknown units → `None`.
pub fn ipx(s: &str) -> Option<f64> {
    use svgtypes::LengthUnit as U;
    let l = svgtypes::Length::from_str(s.trim()).ok()?;
    let factor = match l.unit {
        U::None | U::Px => 1.0,
        U::In => 96.0,
        U::Cm => 96.0 / 2.54,
        U::Mm => 96.0 / 25.4,
        U::Pt => 96.0 / 72.0,
        U::Pc => 16.0,
        U::Em | U::Ex | U::Percent => return None,
    };
    Some(l.number * factor)
}

/// Parses a `transform` attribute (any list of transform functions). Empty → identity.
pub fn parse_transform(s: &str) -> Option<Affine> {
    if s.trim().is_empty() {
        return Some(Affine::IDENTITY);
    }
    let t = svgtypes::Transform::from_str(s).ok()?;
    Some(Affine::new([t.a, t.b, t.c, t.d, t.e, t.f]))
}

/// `translate(e,f)` / `scale(a,d)` / `matrix(…)`; `None` for identity (write no attribute).
pub fn fmt_transform(t: Affine) -> Option<String> {
    if is_identity(t) {
        return None;
    }
    let [a, b, c, d, e, f] = t.as_coeffs();
    let z = |v: f64| v.abs() <= TOL;
    if z(a - 1.0) && z(d - 1.0) && z(b) && z(c) {
        return Some(format!("translate({},{})", num::fmt(e), num::fmt(f)));
    }
    if z(e) && z(f) && z(b) && z(c) {
        return Some(format!("scale({},{})", num::fmt(a), num::fmt(d)));
    }
    Some(format!(
        "matrix({},{},{},{},{},{})",
        num::fmt(a),
        num::fmt(b),
        num::fmt(c),
        num::fmt(d),
        num::fmt(e),
        num::fmt(f)
    ))
}

pub fn affine_eq(a: Affine, b: Affine) -> bool {
    a.as_coeffs()
        .iter()
        .zip(b.as_coeffs().iter())
        .all(|(x, y)| (x - y).abs() <= TOL)
}

pub fn is_identity(t: Affine) -> bool {
    affine_eq(t, Affine::IDENTITY)
}

/// `sqrt(|det|)`: the uniform scale a transform applies to lengths.
pub fn scale_factor(t: Affine) -> f64 {
    t.determinant().abs().sqrt()
}

/// `None` for singular transforms (upstream raised `ZeroDivisionError`).
pub fn inverse(t: Affine) -> Option<Affine> {
    if t.determinant().abs() < 1e-12 {
        None
    } else {
        Some(t.inverse())
    }
}

/// Bounding box of the four transformed corners.
pub fn transform_rect(t: Affine, r: Rect) -> Rect {
    let pts = [
        t * Point::new(r.x0, r.y0),
        t * Point::new(r.x1, r.y0),
        t * Point::new(r.x0, r.y1),
        t * Point::new(r.x1, r.y1),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Rect::new(x0, y0, x1, y1)
}

/// Null-absorbing union (`utils.py:655-667`).
pub fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(a.union(b)),
    }
}

/// Upstream quirk preserved (`utils.py:669-681`): a null first operand returns the
/// second; a null second operand gives null; touching edges give a zero-size box.
pub fn intersection(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    let Some(a) = a else { return b };
    let b = b?;
    let r = Rect::new(
        a.x0.max(b.x0),
        a.y0.max(b.y0),
        a.x1.min(b.x1),
        a.y1.min(b.y1),
    );
    if r.x1 < r.x0 || r.y1 < r.y0 {
        None
    } else {
        Some(r)
    }
}

/// Strict overlap test (`utils.py:649-653`).
pub fn intersects(a: Rect, b: Rect) -> bool {
    let (ac, bc) = (a.center(), b.center());
    (ac.x - bc.x).abs() * 2.0 < a.width() + b.width()
        && (ac.y - bc.y).abs() * 2.0 < a.height() + b.height()
}

/// Number of distinct values, where a value counts as new when it is more than
/// `tol` above the last kept value (`utils.py:130-153`).
pub fn uniquetol(xs: &[f64], tol: f64) -> usize {
    let mut v: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut count = 0;
    let mut last = f64::NAN;
    for x in v {
        if count == 0 || (x - last) > tol {
            count += 1;
            last = x;
        }
    }
    count
}

use crate::dom::{Doc, NodeId};

impl Doc {
    /// The element's own `transform` attribute (identity when absent or invalid).
    pub fn transform(&self, n: NodeId) -> Affine {
        self.attr(n, "transform")
            .and_then(parse_transform)
            .unwrap_or(Affine::IDENTITY)
    }

    /// Product of every ancestor's transform below the root `<svg>` (outermost first)
    /// and the element's own transform; the root's transform is excluded (inkex semantics).
    pub fn composed_transform(&self, n: NodeId) -> Affine {
        let svg = self.svg();
        if n == svg {
            return Affine::IDENTITY;
        }
        let mut chain: Vec<NodeId> = vec![n];
        for a in self.ancestors(n) {
            if a == svg || !self.is_element(a) {
                break;
            }
            chain.push(a);
        }
        let mut t = Affine::IDENTITY;
        for &node in chain.iter().rev() {
            t *= self.transform(node);
        }
        t
    }

    /// Writes `transform` (`translate`/`scale`/`matrix` form); an identity removes the attribute.
    pub fn set_transform(&mut self, n: NodeId, t: Affine) {
        match fmt_transform(t) {
            Some(s) => self.set_attr(n, "transform", s),
            None => {
                self.remove_attr(n, "transform");
            }
        }
    }

    /// The root `viewBox` as a rectangle; without one, `[0, 0, width, height]` of the root
    /// (`cache.py:1185–1192`); `None` when neither is usable.
    pub fn viewbox(&self) -> Option<Rect> {
        let svg = self.svg();
        if let Some(vb) = self.attr(svg, "viewBox") {
            let v: Vec<f64> = vb
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f64>().ok())
                .collect::<Option<Vec<_>>>()?;
            if v.len() == 4 && v[2] > 0.0 && v[3] > 0.0 {
                return Some(Rect::new(v[0], v[1], v[0] + v[2], v[1] + v[3]));
            }
            return None;
        }
        let w = ipx(self.attr(svg, "width")?)?;
        let h = ipx(self.attr(svg, "height")?)?;
        (w > 0.0 && h > 0.0).then(|| Rect::new(0.0, 0.0, w, h))
    }
}
