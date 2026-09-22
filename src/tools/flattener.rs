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
use crate::ops::clip::{ungroup, unlink};

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
    let mut ctx = Ctx::new();
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
    if opts.deepungroup {
        move_defs_and_clips_to_root(&mut doc, &mut seld);
    }
    if groups(&doc, &seld).is_empty() && non_containers(&doc, &seld).is_empty() {
        return Err("No objects selected!".to_string());
    }
    if opts.deepungroup {
        unlink_clones(&mut doc, &mut ctx, &mut seld);
        deep_ungroup(&mut doc, &mut ctx, &seld, opts.removetextclips);
    }
    let mut ngs = non_containers(&doc, &seld);
    let _ = (&mut ngs, &opts); // consumed by the phases of Tasks 4–6
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

/// Attribute holding the joined comments of a matplotlib text group (spec §B.4).
pub const MPL_COMMENT: &str = "mpl_comment";

/// F:203–218: every selected `<defs>`, `<clipPath>` and `<mask>` is appended to the root `<defs>`
/// (a `<defs>` moves whole, nested); it and its descendants leave the working set. The root
/// `<defs>` itself (and anything containing it) is never moved.
pub fn move_defs_and_clips_to_root(doc: &mut Doc, seld: &mut Vec<NodeId>) {
    for pass in [&["defs"][..], &["clipPath", "mask"][..]] {
        let movers: Vec<NodeId> = seld
            .iter()
            .copied()
            .filter(|&n| doc.parent(n).is_some() && pass.contains(&doc.tag(n)))
            .collect();
        if movers.is_empty() {
            continue;
        }
        let root = doc.defs(); // created on demand — only when something has to move
        for m in movers {
            if m == root || doc.ancestors(root).any(|a| a == m) {
                continue;
            }
            doc.append_child(root, m);
            let gone: HashSet<NodeId> = doc.descendants(m).collect();
            seld.retain(|n| !gone.contains(n));
        }
    }
}

/// F:229–246: every `<use>` of the working set whose target exists and is not a `<symbol>` is
/// unlinked; the clone leaves the set and the copy's subtree joins it.
pub fn unlink_clones(doc: &mut Doc, ctx: &mut Ctx, seld: &mut Vec<NodeId>) {
    let uses: Vec<NodeId> = seld
        .iter()
        .copied()
        .filter(|&n| doc.parent(n).is_some() && doc.tag(n) == "use")
        .collect();
    for u in uses {
        let Some(target) = doc.resolve_href(u) else {
            continue; // a clone of nothing stays (upstream skips it too)
        };
        if doc.tag(target) == "symbol" {
            continue;
        }
        if let Some(copy) = unlink(doc, ctx, u) {
            seld.retain(|&n| n != u);
            seld.extend(doc.descendants(copy).filter(|&n| doc.is_element(n)));
        }
    }
}

/// F:248–274: groups in ascending order of their child count (comments count, text does not;
/// counted before any ungroup; ties in document order). A group with a comment child whose
/// children are all comments, `<defs>` or unlinked clones is a matplotlib text group: it keeps
/// its glyphs grouped, gets `mpl_comment` = its comments joined by `;`, and loses the comments.
/// A group already carrying `mpl_comment` is kept. Everything else is dissolved with `ungroup`.
pub fn deep_ungroup(doc: &mut Doc, ctx: &mut Ctx, seld: &[NodeId], remove_text_clip: bool) {
    let mut gs: Vec<(usize, NodeId)> = groups(doc, seld)
        .into_iter()
        .map(|g| {
            let n = doc
                .children(g)
                .filter(|&k| doc.is_element(k) || doc.is_comment(k))
                .count();
            (n, g)
        })
        .collect();
    gs.sort_by_key(|&(n, _)| n); // stable: ties keep document order
    for (_, g) in gs {
        if doc.parent(g).is_none() {
            continue; // dissolved or clipped out by an earlier ungroup
        }
        let kids: Vec<NodeId> = doc
            .children(g)
            .filter(|&k| doc.is_element(k) || doc.is_comment(k))
            .collect();
        let has_comment = kids.iter().any(|&k| doc.is_comment(k));
        let glyphish = kids.iter().all(|&k| {
            doc.is_comment(k)
                || doc.tag(k) == "defs"
                || doc.attr(k, "unlinked_clone") == Some("True")
        });
        if has_comment && glyphish {
            let cmnt: Vec<String> = kids
                .iter()
                .filter(|&&k| doc.is_comment(k))
                .map(|&k| {
                    doc.comment(k)
                        .unwrap_or("")
                        .trim_matches(|c| matches!(c, '<' | '!' | '-' | ' ' | '>'))
                        .to_string()
                })
                .collect();
            doc.set_attr(g, MPL_COMMENT, cmnt.join(";"));
            for &k in &kids {
                if doc.is_comment(k) {
                    doc.detach(k);
                }
            }
        } else if doc.attr(g, MPL_COMMENT).is_some() {
            // leave grouped
        } else {
            ungroup(doc, ctx, g, remove_text_clip);
        }
    }
}
