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
use crate::text::parse::ParsedText;
use crate::text::table::CharTable;

/// Nesting depth past which the recursive helpers (`bbox`, `clip::merge_clipmask`,
/// `bbox::is_rectangle`) stop following groups, clips and clones: deeper is a cycle or a hostile
/// document, and a Rust stack overflow cannot be caught (`main` can only catch panics).
pub const MAX_NEST: usize = 64;

/// Work budget for the recursive helpers that cannot be memoised: `clip::merge_clipmask` clones as
/// it descends and `bbox::is_rectangle` has no per-node cache, so a clip tree that references
/// itself from several children grows exponentially with depth and a depth bound alone
/// (`MAX_NEST`) does not stop it. Also `clip::unlink`'s clone-chain guard.
pub const MAX_STEPS: usize = 10_000;

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
    /// Character table over the `<text>`/`<flowRoot>` elements under `text_roots` (the whole
    /// document when `None`), built on first use so fonts load only when a tool measures text.
    text: Option<CharTable>,
    /// Elements whose text descendants the character table covers; `None` = the whole document.
    /// Tools that measure only their selection set it (upstream builds its table over the
    /// selection's text: `BB2(svg, sel)` → `make_char_table(els=tels)`), so font warnings
    /// concern only the text being measured and large documents cost nothing extra.
    text_roots: Option<Vec<NodeId>>,
}

impl Ctx {
    pub fn new() -> Ctx {
        Ctx::default()
    }

    /// A context whose character table covers only the text under `roots`. Text outside `roots`
    /// still measures (the table falls back to the font system) but without its kerning pairs and
    /// without font warnings — measure only what is under `roots`.
    pub fn for_roots(roots: Vec<NodeId>) -> Ctx {
        Ctx {
            text_roots: Some(roots),
            ..Ctx::default()
        }
    }

    /// Builds the character table if it does not exist yet.
    pub fn ensure_char_table(&mut self, doc: &Doc) {
        if self.text.is_none() {
            let roots: Vec<NodeId> = match &self.text_roots {
                Some(r) => r.clone(),
                None => vec![doc.svg()],
            };
            let mut seen = HashSet::new();
            let mut els: Vec<NodeId> = Vec::new();
            for r in roots {
                for n in doc.descendants(r) {
                    if doc.is_element(n)
                        && matches!(doc.tag(n), "text" | "flowRoot")
                        && seen.insert(n)
                    {
                        els.push(n);
                    }
                }
            }
            let ct = CharTable::build(doc, &els, FontSystem::load(), &mut self.warn);
            self.text = Some(ct);
        }
    }

    pub fn char_table(&mut self, doc: &Doc) -> &mut CharTable {
        self.ensure_char_table(doc);
        self.text.as_mut().expect("built by ensure_char_table")
    }

    /// Drops the character table so the next measurement rebuilds it — after a tool restyles
    /// text (new families or sizes need new entries and their own kerning pairs and warnings).
    pub fn reset_char_table(&mut self) {
        self.text = None;
    }

    /// Parses one `<text>`/`<flowRoot>` against the character table (built on first use), the
    /// way the bbox code does; `None` for other elements or unparsable text.
    pub fn parse_text(&mut self, doc: &mut Doc, el: NodeId) -> Option<ParsedText> {
        if !matches!(doc.tag(el), "text" | "flowRoot") {
            return None;
        }
        self.ensure_char_table(doc);
        let Ctx { text, warn, .. } = self;
        let ct = text.as_mut().expect("built by ensure_char_table");
        ParsedText::parse(doc, el, ct, warn)
    }

    /// End-of-run housekeeping; call once after all edits.
    pub fn finish(&mut self, doc: &mut Doc) {
        cleanup::drop_dangling_refs(doc, &self.deleted);
        cleanup::gc_created_clips(doc, &mut self.created);
    }
}
