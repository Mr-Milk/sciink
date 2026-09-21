//! Stage 12 — the only place the text pipeline writes the DOM (spec §A.0 decision 1). The writer
//! itself lands in a later task; this file starts with the record the merge stages produce.

use crate::dom::NodeId;

/// Elements whose `clip-path`s must be unioned onto `target` because their text was merged into it
/// (RK:640–656). Executed by `apply_clip_unions` before the elements are rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipUnion {
    pub target: NodeId,
    pub others: Vec<NodeId>,
}
