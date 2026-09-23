//! Debug tool: which face every font specification in the document resolves to (spec §A.5-1).

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::Write as _;

use clap::Parser;

use crate::Output;
use crate::cli::Common;
use crate::dom::{Doc, NodeId};
use crate::style::Style;
use crate::text::Warnings;
use crate::text::fonts::{FontSpec, FontStyle, FontSystem};
use crate::text::table::CharTable;
use crate::text::tree::{TextTree, run_text};

use super::first_line;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct FontProbeCli {
    #[command(flatten)]
    pub common: Common,
}

/// `<family> (<file>)` or `(no font found)`.
pub fn describe_face(fs: &FontSystem, k: Option<crate::text::fonts::FaceKey>) -> String {
    match k {
        None => "(no font found)".to_string(),
        Some(k) => {
            let i = fs.face_info(k);
            let file = i
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "memory".into());
            format!("{} ({file})", i.family)
        }
    }
}

/// `sans-serif → DejaVu Sans (DejaVuSans.ttf)`
pub fn describe_resolution(fs: &mut FontSystem, css_family: &str) -> String {
    let spec = FontSpec::from_style(&Style::parse(&format!("font-family:{css_family}")));
    let k = fs.resolve(&spec);
    format!("{css_family} → {}", describe_face(fs, k))
}

/// Text elements in the selection (with descendants) or the whole document, in document order.
pub fn text_elements(doc: &Doc, ids: &[String]) -> Vec<NodeId> {
    let roots: Vec<NodeId> = if ids.is_empty() {
        vec![doc.svg()]
    } else {
        doc.selection(ids)
    };
    let mut out = Vec::new();
    // `roots` can overlap (e.g. `--id=layer1 --id=t` where `t` is inside `layer1`), so the same
    // node can turn up from more than one root; a `HashSet` keeps membership checks O(1) instead
    // of an O(N) `Vec::contains` scan per candidate, while `out` still preserves document order.
    let mut seen = HashSet::new();
    for r in roots {
        for n in doc.descendants(r) {
            if doc.is_element(n) && matches!(doc.tag(n), "text" | "flowRoot") && seen.insert(n) {
                out.push(n);
            }
        }
    }
    out
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = FontProbeCli::try_parse_from(argv).map_err(first_line)?;
    let mut t = crate::log::Timer::new("font-probe");
    let doc = Doc::parse(input).map_err(|e| e.to_string())?;
    t.phase("parse", || {
        format!("bytes={} elements={}", input.len(), doc.element_count())
    });
    let els = text_elements(&doc, &cli.common.ids);
    let mut specs: Vec<FontSpec> = Vec::new();
    for &el in &els {
        let tree = TextTree::new(&doc, el);
        for r in tree.runs(&doc) {
            if run_text(&doc, &r).is_some_and(|t| !t.is_empty()) {
                let s = FontSpec::from_style(&doc.specified_style(r.style_node));
                if !specs.contains(&s) {
                    specs.push(s);
                }
            }
        }
    }
    let mut warn = Warnings::default();
    let mut ct = CharTable::build(&doc, &els, FontSystem::load(), &mut warn);
    let mut rep = String::new();
    for s in &specs {
        let fams: Vec<String> = s.families.iter().map(|f| format!("'{f}'")).collect();
        let sty = match s.style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        };
        let k = ct.true_face(s);
        let _ = writeln!(
            rep,
            "{} weight {} {sty} → {}",
            fams.join(","),
            s.weight,
            describe_face(&ct.fonts, k)
        );
    }
    for g in ["sans-serif", "serif", "monospace"] {
        let _ = writeln!(rep, "{}", describe_resolution(&mut ct.fonts, g));
    }
    let _ = writeln!(
        rep,
        "faces: {} in {:.0} ms",
        ct.fonts.face_count(),
        ct.fonts.load_ms()
    );
    for w in &warn.0 {
        let _ = writeln!(rep, "warning: {w}");
    }
    t.phase("write", || format!("bytes={}", input.len()));
    t.total(String::new);
    Ok(Output {
        svg: input.to_vec(),
        messages: vec![rep.trim_end().to_string()],
    })
}
