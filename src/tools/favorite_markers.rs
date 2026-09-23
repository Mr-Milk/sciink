//! Favorite Markers (spec §B.3; upstream favorite_markers.py): apply stored start/mid/end marker
//! templates to the selected shapes, store the selection's markers as a template, remove one.
//! The store is a small SVG document (see `BUILTINS`) — no JSON, no self-modifying `.inx`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::geom::Affine;
use crate::ops::cleanup::url_id;

use super::first_line;

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

/// Copies attributes onto `dst`, skipping `skip`, declaring a known namespace prefix on the
/// destination's root when needed and dropping an attribute whose prefix cannot be declared.
fn copy_attrs(doc: &mut Doc, dst: NodeId, attrs: &[(String, String)], skip: &[&str]) {
    for (k, v) in attrs {
        if skip.contains(&k.as_str()) {
            continue;
        }
        if let Some((prefix, _)) = k.split_once(':') {
            if prefix != "xmlns" && !doc.ensure_prefix(prefix) {
                continue;
            }
        }
        doc.set_attr(dst, k, v.clone());
    }
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
            .descendants(svg)
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
            copy_attrs(
                &mut self.doc,
                mk,
                &m.attrs,
                &["id", TEMPLATE_ATTR, POSITION_ATTR],
            );
            for p in &m.paths {
                let pe = self.doc.new_element("path");
                copy_attrs(&mut self.doc, pe, p, &["id"]);
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
    if let Some(dir) = crate::paths::data_dir() {
        return dir.join("favorite_markers.svg");
    }
    crate::paths::inx_dir().join("favorite_markers.svg")
}

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct FavoriteMarkersCli {
    #[command(flatten)]
    pub common: Common,
    /// `markers` | `addremove`
    #[arg(long, default_value = "markers")]
    pub tab: String,
    /// 0 Arrow, 1 Triangle, 2 Distance, 3 the custom name below (upstream's argparse default is 1)
    #[arg(long, default_value_t = 1)]
    pub template: u8,
    #[arg(long = "custom_name", default_value = "")]
    pub custom_name: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub smarker: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub mmarker: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub emarker: bool,
    /// Percent
    #[arg(long, default_value_t = 100.0)]
    pub size: f64,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub addt: bool,
    #[arg(long = "template_name", default_value = "")]
    pub template_name: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub remt: bool,
    /// The name to remove (upstream: an index into a self-rewritten dropdown)
    #[arg(long = "template_rem", default_value = "")]
    pub template_rem: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub list: bool,
    /// Store file (tests; default `store_path(None)`).
    #[arg(long, hide = true)]
    pub store: Option<PathBuf>,
}

/// `FM:290–313`: the marker a `marker-*` value points at, as template data; `None` for `none`,
/// an empty value, a missing marker or a marker without children.
pub fn marker_props(doc: &Doc, murl: Option<&str>) -> Option<MarkerData> {
    let id = url_id(murl?.trim())?;
    let mk = doc.by_id(id)?;
    let first = doc.children(mk).find(|&c| doc.is_element(c))?;
    let parent = if doc.tag(first) == "g" { first } else { mk };
    let paths = doc
        .children(parent)
        .filter(|&k| doc.is_element(k) && doc.tag(k) == "path")
        .map(|k| attrs_of(doc, k, &["id"]))
        .collect();
    Some(MarkerData {
        attrs: attrs_of(doc, mk, &["id"]),
        paths,
    })
}

/// `FM:317–350`: the id of a marker named `name` (whitespace removed) at scale `s` under the root
/// `<defs>` — an existing one whose first child `<g>` has that scale, else a new one.
pub fn ensure_marker(doc: &mut Doc, name: &str, m: &MarkerData, s: f64) -> String {
    let name: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let defs = doc.defs();
    let candidates: Vec<NodeId> = doc
        .descendants(defs)
        .filter(|&n| {
            doc.is_element(n)
                && doc.tag(n) == "marker"
                && doc.attr(n, "id").is_some_and(|i| i.contains(&name))
        })
        .collect();
    for mk in candidates {
        let Some(g) = doc.children(mk).find(|&c| doc.is_element(c)) else {
            continue;
        };
        if doc.tag(g) != "g" {
            continue;
        }
        let [a, _, _, d, _, _] = doc.transform(g).as_coeffs();
        if (a - s).abs() < 0.01 && (d - s).abs() < 0.01 {
            return doc.attr(mk, "id").unwrap_or_default().to_string();
        }
    }
    let mk = doc.new_element("marker");
    copy_attrs(doc, mk, &m.attrs, &["id"]);
    let g = doc.new_element("g");
    doc.append_child(mk, g);
    doc.set_transform(g, Affine::scale(s));
    for p in &m.paths {
        let pe = doc.new_element("path");
        copy_attrs(doc, pe, p, &["id"]);
        doc.append_child(g, pe);
    }
    doc.append_child(defs, mk);
    let id = doc.new_id(&name);
    doc.set_attr(mk, "id", id.clone());
    id
}

/// `FM:446–472` over `shapes`: checked positions get the template's marker, unchecked ones lose
/// their inline `marker-*`.
pub fn apply(
    doc: &mut Doc,
    store: &Store,
    shapes: &[NodeId],
    tname: &str,
    flags: [bool; 3],
    size: f64,
) -> Result<(), String> {
    let t = store.get(tname).ok_or_else(|| {
        format!(
            "Template '{tname}' is not stored. Stored templates: {}",
            store.templates().join(", ")
        )
    })?;
    if t.iter().all(Option::is_none) {
        return Err(format!("Template '{tname}' has no markers stored."));
    }
    let s = size / 100.0;
    for &el in shapes {
        for (i, pos) in POSITIONS.iter().enumerate() {
            let prop = format!("marker-{pos}");
            match (flags[i], &t[i]) {
                (true, Some(m)) => {
                    let id = ensure_marker(doc, &format!("FM{tname}{pos}"), m, s);
                    doc.set_style(el, &prop, &format!("url(#{id})"));
                }
                _ => doc.remove_style(el, &prop),
            }
        }
    }
    Ok(())
}

/// The template the Markers page selects: three fixed built-in names or the custom name.
pub fn template_name(cli: &FavoriteMarkersCli) -> Result<String, String> {
    match cli.template {
        0 => Ok("Arrow".to_string()),
        1 => Ok("Triangle".to_string()),
        2 => Ok("Distance".to_string()),
        3 => {
            let n = cli.custom_name.trim();
            if n.is_empty() {
                Err("Custom template selected: type its template name in the box below the template list.".to_string())
            } else {
                Ok(n.to_string())
            }
        }
        other => Err(format!("unknown template option {other} (0–3)")),
    }
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = FavoriteMarkersCli::try_parse_from(argv).map_err(first_line)?;
    let mut t = crate::log::Timer::new("favorite-markers");
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    t.phase("parse", || {
        format!("bytes={} elements={}", input.len(), doc.element_count())
    });
    let mut messages: Vec<String> = Vec::new();
    let path = store_path(cli.store.as_deref());
    let mut store = Store::load(&path)?;
    // FM:368–370: the selection and its descendants, shapes only, each once
    let mut shapes: Vec<NodeId> = Vec::new();
    for r in doc.selection_ordered(&cli.common.ids) {
        for n in doc
            .descendants(r)
            .filter(|&n| doc.is_element(n) && SHAPE_TAGS.contains(&doc.tag(n)))
        {
            if !shapes.contains(&n) {
                shapes.push(n);
            }
        }
    }
    t.phase("selection", || {
        format!("ids={} sel={}", cli.common.ids.len(), shapes.len())
    });
    if cli.tab == "addremove" {
        let mut changed = false;
        if cli.addt {
            let name = cli.template_name.trim();
            if name.is_empty() {
                return Err("Give the new template a name.".to_string());
            }
            let Some(&first) = shapes.first() else {
                return Err("Select a path whose markers should become the template.".to_string());
            };
            let sty = doc.specified_style(first);
            let t: Template = [
                marker_props(&doc, sty.get("marker-start")),
                marker_props(&doc, sty.get("marker-mid")),
                marker_props(&doc, sty.get("marker-end")),
            ];
            if t.iter().all(Option::is_none) {
                return Err("The selected path has no markers to store.".to_string());
            }
            store.set(name, &t);
            changed = true;
        }
        if cli.remt {
            let name = cli.template_rem.trim();
            if store.remove(name) {
                changed = true;
            } else {
                messages.push(format!(
                    "warning: template '{name}' is not stored; nothing removed"
                ));
            }
        }
        if changed {
            store.save(&path)?;
            messages.push("Templates successfully updated!".to_string());
        }
        if cli.list {
            messages.push(format!(
                "favorite-markers: stored templates: {}",
                store.templates().join(", ")
            ));
        }
    } else if shapes.is_empty() {
        messages.push("favorite-markers: nothing selected".to_string());
    } else {
        let tname = template_name(&cli)?;
        apply(
            &mut doc,
            &store,
            &shapes,
            &tname,
            [cli.smarker, cli.mmarker, cli.emarker],
            cli.size,
        )?;
        t.phase("apply", || format!("shapes={}", shapes.len()));
    }
    t.phase("cleanup", String::new);
    let mut svg = Vec::new();
    doc.write(&mut svg);
    t.phase("write", || format!("bytes={}", svg.len()));
    t.total(String::new);
    Ok(Output { svg, messages })
}
