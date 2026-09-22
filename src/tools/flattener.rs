//! The Flattener (spec §B.3; upstream flatten_plots.py): deep ungroup, rectangle reversions,
//! the text pipeline, duplicate and white-rectangle removal, cleanup.

use std::collections::HashSet;
use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::ops::Ctx;
use crate::ops::cleanup::{strip_attr, strip_whitespace};

use super::first_line;

/// Marks an element (and, when it is a container, what upstream calls its "flattening") as
/// excluded (spec §B.4): any non-empty value counts.
pub const EXCLUDE_ATTR: &str = "inkscape-scientific-flattenexclude";
/// Tags that are neither drawn nor flattened themselves (F:221 `gigtags`).
pub const CONTAINER_TAGS: &[&str] = &["namedview", "defs", "metadata", "foreignObject", "g"];

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct FlattenerCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports (`Options`, `Options2`, `Exclusions`).
    #[arg(long, default_value = "Options")]
    pub tab: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub deepungroup: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub fixtext: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub revertpaths: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removeduppaths: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removerectw: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub splitdistant: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergenearby: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removemanualkerning: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergesubsuper: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub reversions: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removetextclips: bool,
    /// 1 = centre, 2 = left, 3 = right, 4 = unchanged.
    #[arg(long, default_value_t = 1)]
    pub justification: u8,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false")]
    pub setreplacement: bool,
    #[arg(long, default_value = "Arial")]
    pub replacement: String,
    /// Exclusions page: 1 = mark the selection as not flattened, 2 = flattened again.
    #[arg(long, default_value_t = 1)]
    pub markexc: u8,
    /// Upstream's test switch: duplicate the selection, flatten the original's children with every
    /// fix on, `sans-serif` as the replacement family and centred justification (F:132–174).
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false", hide = true)]
    pub testmode: bool,
    /// Accepted for compatibility with upstream's test suite; the Text Highlight tool draws the same rectangles.
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "false", hide = true)]
    pub debugparser: bool,
    /// Accepted for compatibility; unused.
    #[arg(long, default_value = "1.2", hide = true)]
    pub v: String,
}

/// The effective options: text sub-options are ANDed with `fixtext` (F:178–184).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub deepungroup: bool,
    pub fixtext: bool,
    pub revertpaths: bool,
    pub removeduppaths: bool,
    pub removerectw: bool,
    pub splitdistant: bool,
    pub mergenearby: bool,
    pub removemanualkerning: bool,
    pub mergesubsuper: bool,
    pub reversions: bool,
    pub removetextclips: bool,
    pub setreplacement: bool,
    pub replacement: String,
    pub justification: u8,
}

impl Options {
    pub fn from_cli(c: &FlattenerCli) -> Options {
        let t = c.fixtext;
        Options {
            deepungroup: c.deepungroup,
            fixtext: t,
            revertpaths: c.revertpaths,
            removeduppaths: c.removeduppaths,
            removerectw: c.removerectw,
            splitdistant: c.splitdistant && t,
            mergenearby: c.mergenearby && t,
            removemanualkerning: c.removemanualkerning && t,
            mergesubsuper: c.mergesubsuper && t,
            reversions: c.reversions && t,
            removetextclips: c.removetextclips && t,
            setreplacement: c.setreplacement && t,
            replacement: c.replacement.clone(),
            justification: c.justification,
        }
    }

    /// F:162–174: everything on, `sans-serif`, centred.
    pub fn testmode() -> Options {
        Options {
            deepungroup: true,
            fixtext: true,
            revertpaths: true,
            removeduppaths: true,
            removerectw: true,
            splitdistant: true,
            mergenearby: true,
            removemanualkerning: true,
            mergesubsuper: true,
            reversions: true,
            removetextclips: true,
            setreplacement: true,
            replacement: "sans-serif".to_string(),
            justification: 1,
        }
    }
}

/// F:187–194: the Exclusions page sets (`True`) or removes the marker on the selection.
pub fn mark_exclusions(doc: &mut Doc, sel: &[NodeId], exclude: bool) {
    for &el in sel {
        if exclude {
            doc.set_attr(el, EXCLUDE_ATTR, "True");
        } else {
            doc.remove_attr(el, EXCLUDE_ATTR);
        }
    }
}

/// F:132–149 `duplicate_layer1`: every selected element gets a copy inserted right before it —
/// labelled `<label> original`, locked (`sodipodi:insensitive`), at opacity 0.3 — while the
/// original is labelled `<label> flat` and its element children become the selection to flatten.
/// **Deviation:** the copy carries no ids (`Doc::deep_clone` drops them; upstream assigns random ones).
pub fn duplicate_for_testmode(doc: &mut Doc, sel: &[NodeId]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for &el in sel {
        let d = doc.deep_clone(el);
        doc.insert_before(d, el);
        if let Some(label) = doc.attr(el, "inkscape:label").map(str::to_string) {
            doc.set_attr(el, "inkscape:label", format!("{label} flat"));
            doc.set_attr(d, "inkscape:label", format!("{label} original"));
        }
        doc.set_attr(d, "sodipodi:insensitive", "true");
        doc.set_attr(d, "opacity", "0.3");
        out.extend(doc.children(el).filter(|&k| doc.is_element(k)));
    }
    out
}

fn excluded(doc: &Doc, n: NodeId) -> bool {
    doc.attr(n, EXCLUDE_ATTR)
        .is_some_and(|v| !v.trim().is_empty())
}

/// F:195–201 `seld`: the selection (minus excluded elements) and every element under it, in
/// document order, deduplicated, minus the elements that carry the exclusion marker themselves.
pub fn working_set(doc: &Doc, sel: &[NodeId]) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &root in sel {
        if excluded(doc, root) {
            continue;
        }
        for n in doc.descendants(root) {
            if doc.is_element(n) && !excluded(doc, n) && seen.insert(n) {
                out.push(n);
            }
        }
    }
    out
}

/// The members of `els` still in the document.
pub fn attached(doc: &Doc, els: &[NodeId]) -> Vec<NodeId> {
    els.iter()
        .copied()
        .filter(|&n| doc.parent(n).is_some())
        .collect()
}

/// `ngs` (F:224): the attached members of `seld` that are neither containers nor unrendered.
pub fn non_containers(doc: &Doc, seld: &[NodeId]) -> Vec<NodeId> {
    attached(doc, seld)
        .into_iter()
        .filter(|&n| !CONTAINER_TAGS.contains(&doc.tag(n)))
        .collect()
}

/// `gs` (F:223): the attached groups of `seld`.
pub fn groups(doc: &Doc, seld: &[NodeId]) -> Vec<NodeId> {
    attached(doc, seld)
        .into_iter()
        .filter(|&n| doc.tag(n) == "g")
        .collect()
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = FlattenerCli::try_parse_from(argv).map_err(first_line)?;
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let ctx = Ctx::new();
    let mut sel = doc.selection(&cli.common.ids);
    if cli.tab == "Exclusions" {
        mark_exclusions(&mut doc, &sel, cli.markexc == 1);
        return finish(doc, ctx, false);
    }
    let opts = if cli.testmode {
        sel = duplicate_for_testmode(&mut doc, &sel);
        Options::testmode()
    } else {
        Options::from_cli(&cli)
    };
    let mut seld = working_set(&doc, &sel);
    // (Task 3 inserts: defs/clips to root when deepungroup)
    if groups(&doc, &seld).is_empty() && non_containers(&doc, &seld).is_empty() {
        return Err("No objects selected!".to_string());
    }
    // (Task 3 inserts: unlink clones, deep ungroup)
    let mut ngs = non_containers(&doc, &seld);
    let _ = (&mut seld, &mut ngs, &opts); // consumed by the phases of Tasks 3–6
    finish(doc, ctx, true)
}

/// End of every run: created-clip gc and dangling-reference sweep (`Ctx::finish`), whitespace and
/// `unlinked_clone` markers when the document was flattened (F:513–548, spec §B.3 step 9).
fn finish(mut doc: Doc, mut ctx: Ctx, flattened: bool) -> Result<Output, String> {
    ctx.finish(&mut doc);
    if flattened {
        strip_whitespace(&mut doc);
        strip_attr(&mut doc, "unlinked_clone");
    }
    let messages = ctx.warn.0.iter().map(|w| format!("warning: {w}")).collect();
    let mut svg = Vec::new();
    doc.write(&mut svg);
    Ok(Output { svg, messages })
}
