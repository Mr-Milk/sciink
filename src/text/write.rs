//! Stage 12 — the only place the text pipeline writes the DOM (spec §A.0 decision 1). The writer
//! itself lands in a later task; this file starts with the record the merge stages produce.

use crate::dom::{Doc, NodeId};

/// Elements whose `clip-path`s must be unioned onto `target` because their text was merged into it
/// (RK:640–656). Executed by `apply_clip_unions` before the elements are rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipUnion {
    pub target: NodeId,
    pub others: Vec<NodeId>,
}

/// The element a `clip-path` (attribute, else style) points at; `None` for no or a dangling reference.
///
/// **Deviation:** the brief's sketch reads `doc.specified(el, "clip-path")`, a convenience
/// accessor that does not exist on `Doc`; `clip-path` is deliberately excluded from the cascade
/// `PRESENTATION_ATTRS` models (see `style.rs`), so the equivalent lookup is
/// `doc.specified_style(el).get("clip-path")`, cloned to an owned `String`.
pub fn clip_of(doc: &Doc, el: NodeId) -> Option<NodeId> {
    let v = doc
        .attr(el, "clip-path")
        .map(str::to_string)
        .or_else(|| doc.specified_style(el).get("clip-path").map(str::to_string))?;
    let id = v.trim().strip_prefix("url(#")?.strip_suffix(')')?.trim();
    doc.by_id(id)
}
