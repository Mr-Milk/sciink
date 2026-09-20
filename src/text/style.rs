//! Font-size / line-height / letter-spacing / baseline-shift semantics as Inkscape computes
//! them (upstream utils.py:45–114, parser.py:3928–4019; spec §A.1 stage 1e).

use crate::dom::{Doc, NodeId};
use crate::geom::{ipx, scale_factor};
use crate::style::{Style, default_value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontSize {
    pub tfs: f64,
    pub scf: f64,
    pub utfs: f64,
}

fn keyword_px(prop: &str, v: &str) -> f64 {
    if prop == "font-size" {
        match v {
            "small" => 10.0,
            "large" => 14.0,
            _ => 12.0, // medium and every other keyword
        }
    } else {
        default_value(prop).and_then(ipx).unwrap_or(1.0)
    }
}

/// Transformed size, composed scale factor and untransformed size of a length property.
pub fn composed_width(doc: &Doc, n: NodeId, prop: &str) -> FontSize {
    let dflt = if prop == "font-size" {
        "medium"
    } else {
        default_value(prop).unwrap_or("0")
    };
    let satt = doc.specified(n, prop).unwrap_or_else(|| dflt.to_string());
    let satt = satt.trim().to_string();
    let rel = satt
        .strip_suffix('%')
        .map(|s| (s, 0.01))
        .or_else(|| satt.strip_suffix("em").map(|s| (s, 1.0)));
    if let Some((num, mul)) = rel {
        if let Ok(f) = num.trim().parse::<f64>() {
            // find the element that set this exact string, then resolve against its parent
            let mut cel = Some(n);
            while let Some(c) = cel {
                let own = doc
                    .cascaded_style(c)
                    .get(prop)
                    .map(|v| v.trim() == satt)
                    .unwrap_or(false)
                    || doc.attr(c, prop).map(|v| v.trim() == satt).unwrap_or(false);
                if own {
                    break;
                }
                cel = doc.parent(c).filter(|&p| doc.is_element(p));
            }
            let f = f * mul;
            let par = cel
                .and_then(|c| doc.parent(c))
                .filter(|&p| doc.is_element(p));
            return match par {
                Some(p) => {
                    let base = composed_width(doc, p, prop);
                    FontSize {
                        tfs: base.tfs * f,
                        scf: base.scf,
                        utfs: base.utfs * f,
                    }
                }
                None => {
                    // The relative value was set on the root (or above): resolve against the
                    // initial value instead of recursing on the root forever.
                    let utsz = ipx(dflt).unwrap_or_else(|| keyword_px(prop, dflt));
                    let scf = scale_factor(doc.composed_transform(n));
                    FontSize {
                        tfs: utsz * f * scf,
                        scf,
                        utfs: utsz * f,
                    }
                }
            };
        }
    }
    let utfs = ipx(&satt).unwrap_or_else(|| keyword_px(prop, &satt));
    let scf = scale_factor(doc.composed_transform(n));
    FontSize {
        tfs: utfs * scf,
        scf,
        utfs,
    }
}

pub fn composed_font_size(doc: &Doc, n: NodeId) -> FontSize {
    composed_width(doc, n, "font-size")
}

/// Absolute line height in (transformed) user units.
pub fn composed_line_height(doc: &Doc, n: NodeId) -> f64 {
    let satt = doc
        .specified(n, "line-height")
        .unwrap_or_else(|| "normal".to_string());
    let satt = satt.trim();
    let fs = composed_font_size(doc, n);
    let factor = if satt == "normal" {
        1.25
    } else if let Some(p) = satt.strip_suffix('%') {
        p.trim().parse::<f64>().map(|v| v / 100.0).unwrap_or(1.25)
    } else if let Ok(v) = satt.trim_end_matches("em").trim().parse::<f64>() {
        v
    } else {
        match (ipx(satt), fs.utfs) {
            (Some(px), u) if u > 0.0 => px / u,
            _ => 1.25,
        }
    };
    factor * fs.tfs
}

/// Letter spacing of a character whose specified style is `style`, in untransformed user units.
pub fn letter_spacing(doc: &Doc, style_node: NodeId, style: &Style) -> f64 {
    match style.get("letter-spacing").map(str::trim) {
        None | Some("normal") => 0.0,
        Some(v) => match v.strip_suffix("em") {
            Some(num) => {
                num.trim().parse::<f64>().unwrap_or(0.0) * composed_font_size(doc, style_node).utfs
            }
            None => ipx(v).unwrap_or(0.0),
        },
    }
}

fn local_baseline(doc: &Doc, el: NodeId) -> f64 {
    let own = doc.cascaded_style(el);
    let v = own.get("baseline-shift").unwrap_or("0").trim();
    let v = match v {
        "super" => "40%",
        "sub" => "-20%",
        other => other,
    };
    if let Some(p) = v.strip_suffix('%') {
        let par = doc
            .parent(el)
            .filter(|&p| doc.is_element(p))
            .unwrap_or_else(|| doc.svg());
        let f = composed_font_size(doc, par);
        (f.tfs / f.scf) * p.trim().parse::<f64>().unwrap_or(0.0) / 100.0
    } else {
        ipx(v).unwrap_or(0.0)
    }
}

/// Baseline shift of a character in untransformed user units (Inkscape's compounding kept).
pub fn baseline_shift(doc: &Doc, style_node: NodeId, style: &Style) -> f64 {
    if style.get("baseline-shift").is_none() {
        return 0.0;
    }
    let mut chain: Vec<NodeId> = Vec::new();
    let mut cel = Some(style_node);
    while let Some(c) = cel {
        if !doc.is_element(c) || doc.specified_style(c).get("baseline-shift").is_none() {
            break;
        }
        chain.push(c);
        cel = doc.parent(c);
    }
    let mut rel: Vec<f64> = Vec::new();
    for &el in chain.iter().rev() {
        if doc.cascaded_style(el).get("baseline-shift").is_some() {
            rel.push(local_baseline(doc, el));
        } else {
            let s: f64 = rel.iter().sum();
            rel.push(s);
        }
    }
    rel.iter().sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

impl Anchor {
    pub fn parse(s: &str) -> Option<Anchor> {
        match s.trim() {
            "start" => Some(Anchor::Start),
            "middle" => Some(Anchor::Middle),
            "end" => Some(Anchor::End),
            _ => None,
        }
    }
    pub fn anfr(self) -> f64 {
        match self {
            Anchor::Start => 0.0,
            Anchor::Middle => 0.5,
            Anchor::End => 1.0,
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            Anchor::Start => "start",
            Anchor::Middle => "middle",
            Anchor::End => "end",
        }
    }
}
