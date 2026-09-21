//! Document operations shared by the tools (spec docs/spec/02-geometry-tools.md §B.2): style
//! composition, clip/mask merging, ungrouping, unlinking, bounding boxes, transform fusing.

pub mod bbox;
pub mod cleanup;
pub mod clip;
pub mod style;
pub mod xform;

use std::collections::HashSet;

use crate::dom::{Doc, NodeId};
use crate::text::Warnings;
use crate::text::fonts::FontSystem;
use crate::text::table::CharTable;

/// Nesting depth past which the recursive helpers (`bbox`, `clip::merge_clipmask`,
/// `bbox::is_rectangle`) stop following groups, clips and clones: deeper is a cycle or a hostile
/// document, and a Rust stack overflow cannot be caught (`main` can only catch panics).
pub const MAX_NEST: usize = 64;

/// The two url-referencing attributes the ops manage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipKind {
    Clip,
    Mask,
}

impl ClipKind {
    pub fn attr(self) -> &'static str {
        match self {
            ClipKind::Clip => "clip-path",
            ClipKind::Mask => "mask",
        }
    }
}

/// The `<clipPath>`/`<mask>` the element's **attribute** points at (`url(#id)`), if it resolves.
/// Attributes only (upstream `get_link(…, llget=True)`); see `text::write::clip_of` for the
/// style-aware variant the text writer uses.
pub fn clip_ref(doc: &Doc, n: NodeId, kind: ClipKind) -> Option<NodeId> {
    doc.attr(n, kind.attr())
        .and_then(cleanup::url_id)
        .and_then(|id| doc.by_id(id))
}

/// `<tag id="…">` for messages.
pub fn label(doc: &Doc, n: NodeId) -> String {
    match doc.attr(n, "id") {
        Some(id) => format!("<{} id=\"{id}\">", doc.tag(n)),
        None => format!("<{}>", doc.tag(n)),
    }
}

/// Per-run state a tool threads through the ops.
#[derive(Default)]
pub struct Ctx {
    /// `<clipPath>`/`<mask>` elements duplicated by the ops; the ones nothing references any
    /// more are removed by `finish` (`cleanup::gc_created_clips`).
    pub created: Vec<NodeId>,
    /// ids of elements removed by `cleanup::delete_up`; `finish` strips `clip-path`/`mask`
    /// references that still point at them (`cleanup::drop_dangling_refs`).
    pub deleted: HashSet<String>,
    pub warn: Warnings,
    /// Character table over every `<text>`/`<flowRoot>` of the document, built on first use so
    /// fonts load only when a tool measures text.
    text: Option<CharTable>,
}

impl Ctx {
    pub fn new() -> Ctx {
        Ctx::default()
    }

    /// Builds the character table if it does not exist yet.
    pub fn ensure_char_table(&mut self, doc: &Doc) {
        if self.text.is_none() {
            let els: Vec<NodeId> = doc
                .descendants(doc.svg())
                .filter(|&n| doc.is_element(n) && matches!(doc.tag(n), "text" | "flowRoot"))
                .collect();
            let ct = CharTable::build(doc, &els, FontSystem::load(), &mut self.warn);
            self.text = Some(ct);
        }
    }

    pub fn char_table(&mut self, doc: &Doc) -> &mut CharTable {
        self.ensure_char_table(doc);
        self.text.as_mut().expect("built by ensure_char_table")
    }

    /// End-of-run housekeeping; call once after all edits.
    pub fn finish(&mut self, doc: &mut Doc) {
        cleanup::drop_dangling_refs(doc, &self.deleted);
        cleanup::gc_created_clips(doc, &mut self.created);
    }
}
