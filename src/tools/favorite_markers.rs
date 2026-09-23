//! Favorite Markers (spec §B.3; upstream favorite_markers.py): apply stored start/mid/end marker
//! templates to the selected shapes, store the selection's markers as a template, remove one.
//! The store is a small SVG document (see `BUILTINS`) — no JSON, no self-modifying `.inx`.

use std::path::{Path, PathBuf};

use crate::dom::{Doc, NodeId};

pub const TEMPLATE_ATTR: &str = "sciink:template";
pub const POSITION_ATTR: &str = "sciink:position";
pub const POSITIONS: [&str; 3] = ["start", "mid", "end"];
/// Elements that take markers (`FM:446–457`).
pub const SHAPE_TAGS: &[&str] = &["path", "line", "polyline", "rect", "circle", "ellipse"];

/// One marker of a template: its attributes and its paths' attributes, never an `id`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarkerData {
    pub attrs: Vec<(String, String)>,
    pub paths: Vec<Vec<(String, String)>>,
}

/// Start, mid, end.
pub type Template = [Option<MarkerData>; 3];

/// The three built-in templates (`FM:24–214`, ids dropped, upstream's attribute order kept).
pub const BUILTINS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" xmlns:sciink="https://github.com/Mr-Milk/sciink">
<marker sciink:template="Arrow" sciink:position="start" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="Arrow2Mstart" inkscape:isstock="true"><path transform="scale(0.6, 0.6)" d="M 8.7185878,4.0337352 L -2.2072895,0.016013256 L 8.7185884,-4.0017078 C 6.9730900,-1.6296469 6.9831476,1.6157441 8.7185878,4.0337352 z " style="stroke:context-stroke;fill-rule:evenodd;fill:context-stroke;stroke-width:0.62500000;stroke-linejoin:round"/></marker>
<marker sciink:template="Arrow" sciink:position="mid" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="Arrow2Mend" inkscape:isstock="true"><path transform="scale(-0.6, -0.6)" d="M 8.7185878,4.0337352 L -2.2072895,0.016013256 L 8.7185884,-4.0017078 C 6.9730900,-1.6296469 6.9831476,1.6157441 8.7185878,4.0337352 z " style="stroke:context-stroke;fill-rule:evenodd;fill:context-stroke;stroke-width:0.62500000;stroke-linejoin:round"/></marker>
<marker sciink:template="Arrow" sciink:position="end" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="Arrow2Mend" inkscape:isstock="true"><path transform="scale(-0.6, -0.6)" d="M 8.7185878,4.0337352 L -2.2072895,0.016013256 L 8.7185884,-4.0017078 C 6.9730900,-1.6296469 6.9831476,1.6157441 8.7185878,4.0337352 z " style="stroke:context-stroke;fill-rule:evenodd;fill:context-stroke;stroke-width:0.62500000;stroke-linejoin:round"/></marker>
<marker sciink:template="Triangle" sciink:position="start" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="TriangleInM" inkscape:isstock="true"><path transform="scale(-0.4, -0.4)" style="fill-rule:evenodd;fill:context-stroke;stroke:context-stroke;stroke-width:1.0pt" d="M 5.77,0.0 L -2.88,5.0 L -2.88,-5.0 L 5.77,0.0 z "/></marker>
<marker sciink:template="Triangle" sciink:position="mid" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="TriangleOutM" inkscape:isstock="true"><path transform="scale(0.4, 0.4)" style="fill-rule:evenodd;fill:context-stroke;stroke:context-stroke;stroke-width:1.0pt" d="M 5.77,0.0 L -2.88,5.0 L -2.88,-5.0 L 5.77,0.0 z "/></marker>
<marker sciink:template="Triangle" sciink:position="end" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="TriangleOutM" inkscape:isstock="true"><path transform="scale(0.4, 0.4)" style="fill-rule:evenodd;fill:context-stroke;stroke:context-stroke;stroke-width:1.0pt" d="M 5.77,0.0 L -2.88,5.0 L -2.88,-5.0 L 5.77,0.0 z "/></marker>
<marker sciink:template="Distance" sciink:position="start" inkscape:stockid="DistanceStart" orient="auto" refY="0.0" refX="0.0" style="overflow:visible" inkscape:isstock="true"><path d="M 0,0 L 2,0" style="fill:none;stroke:context-fill;stroke-width:1.15;stroke-linecap:square"/><path d="M 0,0 L 13,4 L 9,0 13,-4 L 0,0 z " style="fill:context-stroke;fill-rule:evenodd;stroke:none"/><path d="M 0,-4 L 0,40" style="fill:none;stroke:context-stroke;stroke-width:1;stroke-linecap:square"/></marker>
<marker sciink:template="Distance" sciink:position="mid" style="overflow:visible" refX="0.0" refY="0.0" orient="auto" inkscape:stockid="StopL" inkscape:isstock="true"><path transform="scale(0.8, 0.8)" style="fill:none;fill-opacity:0.75000000;fill-rule:evenodd;stroke:context-stroke;stroke-width:1.0pt" d="M 0.0,5.65 L 0.0,-5.65"/></marker>
<marker sciink:template="Distance" sciink:position="end" inkscape:stockid="DistanceEnd" orient="auto" refY="0.0" refX="0.0" style="overflow:visible" inkscape:isstock="true"><path d="M 0,0 L -2,0" style="fill:none;stroke:context-fill;stroke-width:1.15;stroke-linecap:square"/><path d="M 0,0 L -13,4 L -9,0 -13,-4 L 0,0 z " style="fill:context-stroke;fill-rule:evenodd;stroke:none"/><path d="M 0,-4 L 0,40" style="fill:none;stroke:context-stroke;stroke-width:1;stroke-linecap:square"/></marker>
</svg>
"#;

/// The template store: `<marker sciink:template sciink:position …>` children of one `<svg>`.
pub struct Store {
    doc: Doc,
}

fn attrs_of(doc: &Doc, n: NodeId, skip: &[&str]) -> Vec<(String, String)> {
    doc.attrs(n)
        .iter()
        .filter(|a| !skip.contains(&a.name.as_str()))
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect()
}

impl Store {
    pub fn builtins() -> Store {
        Store {
            doc: Doc::parse(BUILTINS.as_bytes()).expect("BUILTINS is a valid document"),
        }
    }

    /// The store at `path`; a missing file means the built-ins, anything else unreadable is an error.
    pub fn load(path: &Path) -> Result<Store, String> {
        match std::fs::read(path) {
            Ok(bytes) => Doc::parse(&bytes)
                .map(|doc| Store { doc })
                .map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::builtins()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let mut out = Vec::new();
        self.doc.write(&mut out);
        std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn markers(&self) -> Vec<NodeId> {
        let svg = self.doc.svg();
        self.doc
            .children(svg)
            .filter(|&n| self.doc.is_element(n) && self.doc.tag(n) == "marker")
            .collect()
    }

    /// Template names in order of first appearance.
    pub fn templates(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in self.markers() {
            if let Some(t) = self.doc.attr(m, TEMPLATE_ATTR) {
                if !out.iter().any(|x| x == t) {
                    out.push(t.to_string());
                }
            }
        }
        out
    }

    pub fn get(&self, name: &str) -> Option<Template> {
        let mut t: Template = [None, None, None];
        let mut found = false;
        for m in self.markers() {
            if self.doc.attr(m, TEMPLATE_ATTR) != Some(name) {
                continue;
            }
            found = true;
            let Some(i) = POSITIONS
                .iter()
                .position(|p| Some(*p) == self.doc.attr(m, POSITION_ATTR))
            else {
                continue;
            };
            let paths = self
                .doc
                .children(m)
                .filter(|&k| self.doc.is_element(k) && self.doc.tag(k) == "path")
                .map(|k| attrs_of(&self.doc, k, &["id"]))
                .collect();
            t[i] = Some(MarkerData {
                attrs: attrs_of(&self.doc, m, &["id", TEMPLATE_ATTR, POSITION_ATTR]),
                paths,
            });
        }
        found.then_some(t)
    }

    /// Stores `t` under `name`, replacing an existing template of that name in place (its
    /// markers are replaced where the first of them stood, so the list order is stable).
    pub fn set(&mut self, name: &str, t: &Template) {
        let svg = self.doc.svg();
        let old: Vec<NodeId> = self
            .markers()
            .into_iter()
            .filter(|&m| self.doc.attr(m, TEMPLATE_ATTR) == Some(name))
            .collect();
        let anchor = old.first().and_then(|&m| {
            // the element sibling before the first old marker, so the new ones go back there
            let kids: Vec<NodeId> = self.doc.children(svg).collect();
            let idx = kids.iter().position(|&k| k == m)?;
            kids[..idx]
                .iter()
                .rev()
                .copied()
                .find(|&k| self.doc.is_element(k))
        });
        for m in &old {
            self.doc.detach(*m);
        }
        let mut prev = anchor;
        for (i, m) in t.iter().enumerate() {
            let Some(m) = m else { continue };
            let mk = self.doc.new_element("marker");
            self.doc.set_attr(mk, TEMPLATE_ATTR, name);
            self.doc.set_attr(mk, POSITION_ATTR, POSITIONS[i]);
            for (k, v) in &m.attrs {
                if k != "id" && k != TEMPLATE_ATTR && k != POSITION_ATTR {
                    self.doc.set_attr(mk, k, v.clone());
                }
            }
            for p in &m.paths {
                let pe = self.doc.new_element("path");
                for (k, v) in p {
                    if k != "id" {
                        self.doc.set_attr(pe, k, v.clone());
                    }
                }
                self.doc.append_child(mk, pe);
            }
            match prev {
                Some(p) => self.doc.insert_after(mk, p), // (node, anchor)
                None if !old.is_empty() => self.doc.prepend_child(svg, mk),
                None => self.doc.append_child(svg, mk),
            }
            prev = Some(mk);
        }
    }

    /// Removes a template; `false` when no marker carried that name.
    pub fn remove(&mut self, name: &str) -> bool {
        let old: Vec<NodeId> = self
            .markers()
            .into_iter()
            .filter(|&m| self.doc.attr(m, TEMPLATE_ATTR) == Some(name))
            .collect();
        for m in &old {
            self.doc.detach(*m);
        }
        !old.is_empty()
    }
}

/// Where the store lives: the override (`--store`), else `$INKSCAPE_PROFILE_DIR/sciink/`
/// (Inkscape exports the variable for extensions), else next to the `.inx` files.
pub fn store_path(override_: Option<&Path>) -> PathBuf {
    if let Some(p) = override_ {
        return p.to_path_buf();
    }
    if let Some(dir) = std::env::var_os("INKSCAPE_PROFILE_DIR") {
        return PathBuf::from(dir)
            .join("sciink")
            .join("favorite_markers.svg");
    }
    crate::paths::inx_dir().join("favorite_markers.svg")
}
