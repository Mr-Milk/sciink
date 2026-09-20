//! Stage 0: which face draws each character, and which pairs need kerning (P:4328–4400).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::dom::{Doc, NodeId};

use super::Warnings;
use super::fonts::{FaceKey, FontSpec, FontSystem};
use super::metrics::{CProp, Metrics};
use super::tree::{TextTree, run_text};

pub struct CharTable {
    pub fonts: FontSystem,
    pub metrics: Metrics,
    true_style: HashMap<FontSpec, Option<FaceKey>>,
    char_style: HashMap<(FontSpec, char), Option<FaceKey>>,
    preceders: HashMap<(FaceKey, char), Vec<char>>,
}

fn is_generic(f: &str) -> bool {
    matches!(
        f.to_ascii_lowercase().as_str(),
        "sans-serif"
            | "sans"
            | "serif"
            | "monospace"
            | "mono"
            | "cursive"
            | "fantasy"
            | "system-ui"
    )
}

impl CharTable {
    pub fn build(
        doc: &Doc,
        els: &[NodeId],
        mut fonts: FontSystem,
        warn: &mut Warnings,
    ) -> CharTable {
        // 1. chars per font spec, and the (text, spec) runs for pair collection
        let mut per_spec: HashMap<FontSpec, HashSet<char>> = HashMap::new();
        let mut runs_txt: Vec<(String, FontSpec)> = Vec::new();
        for &el in els {
            let tree = TextTree::new(doc, el);
            for r in tree.runs(doc) {
                let Some(txt) = run_text(doc, &r) else {
                    continue;
                };
                if txt.is_empty() {
                    continue;
                }
                let spec = FontSpec::from_style(&doc.specified_style(r.style_node));
                let set = per_spec.entry(spec.clone()).or_default();
                set.extend(txt.chars());
                set.insert(' ');
                runs_txt.push((txt, spec));
            }
        }
        // 2. true face per spec and face per char
        let mut true_style = HashMap::new();
        let mut char_style: HashMap<(FontSpec, char), Option<FaceKey>> = HashMap::new();
        for (spec, chars) in &per_spec {
            let tf = fonts.resolve(spec);
            true_style.insert(spec.clone(), tf);
            if let (Some(k), Some(first)) = (tf, spec.families.first()) {
                let fam = &fonts.face_info(k).family;
                if !is_generic(first) && !fam.eq_ignore_ascii_case(first) {
                    warn.push(format!(
                        "font-family \"{first}\" not installed; measured with \"{fam}\""
                    ));
                }
            }
            for &c in chars {
                let f = fonts.resolve_for_char(spec, c);
                if f.is_none() {
                    warn.push(format!(
                        "no installed font has the character U+{:04X}",
                        c as u32
                    ));
                }
                char_style.insert((spec.clone(), c), f);
            }
        }
        // 3. preceders per (face, char): the previous char when drawn by the same face, plus ' '
        let mut preceders: HashMap<(FaceKey, char), Vec<char>> = HashMap::new();
        for (txt, spec) in &runs_txt {
            let cs: Vec<char> = txt.chars().collect();
            for j in 1..cs.len() {
                let Some(face) = char_style[&(spec.clone(), cs[j])] else {
                    continue;
                };
                if char_style[&(spec.clone(), cs[j - 1])] == Some(face) {
                    let v = preceders.entry((face, cs[j])).or_default();
                    for p in [cs[j - 1], ' '] {
                        if !v.contains(&p) {
                            v.push(p);
                        }
                    }
                }
            }
        }
        CharTable {
            fonts,
            metrics: Metrics::new(),
            true_style,
            char_style,
            preceders,
        }
    }

    pub fn spec_count(&self) -> usize {
        self.true_style.len()
    }

    pub fn true_face(&self, spec: &FontSpec) -> Option<FaceKey> {
        self.true_style.get(spec).copied().flatten()
    }

    pub fn char_face(&mut self, spec: &FontSpec, c: char) -> Option<FaceKey> {
        if let Some(f) = self.char_style.get(&(spec.clone(), c)) {
            return *f;
        }
        let f = self.fonts.resolve_for_char(spec, c);
        self.char_style.insert((spec.clone(), c), f);
        f
    }

    pub fn prop(&mut self, face: Option<FaceKey>, c: char) -> Rc<CProp> {
        match face {
            None => Metrics::unrendered(c),
            Some(k) => {
                let prev = self.preceders.get(&(k, c)).cloned().unwrap_or_default();
                self.metrics.prop(&self.fonts, k, c, &prev)
            }
        }
    }
}
