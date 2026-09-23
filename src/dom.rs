//! Arena DOM for Inkscape SVG documents (spec §C.1).
//!
//! Lossless for everything Inkscape writes: XML declaration, DOCTYPE, comments,
//! processing instructions, CDATA, attribute order, the whitespace before each
//! attribute, the ` />` form, namespace prefixes as literal strings, all text.
//! Normalized on output: attribute quotes are always `"`, text is re-escaped
//! canonically (`& < > "` in text), attribute values keep `&#9;`/`&#10;`/`&#13;`
//! character references for tab/newline/CR (plus `"`) — kept because XML
//! attribute-value normalization would otherwise turn them into spaces on
//! re-parse — and the DOCTYPE keyword is followed by exactly one space.
//! Namespace prefixes are never resolved: `svg:path` and `path`
//! compare equal by local name (ponytail: standard prefixes are enforced at parse).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;

use quick_xml::escape::EscapeError;
use quick_xml::events::{BytesStart, Event};

pub type NodeId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    pub value: String,
    /// Whitespace that preceded this attribute in the source (`" "` for new attributes).
    pub ws: String,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Document,
    Element {
        name: String,
        attrs: Vec<Attr>,
        self_closing: bool,
        /// Whitespace between the last attribute and `>` / `/>`.
        close_ws: String,
    },
    Text(String),
    CData(String),
    Comment(String),
    /// Everything between `<?` and `?>`.
    PI(String),
    /// Everything between `<!DOCTYPE ` and `>`.
    DocType(String),
    /// Everything between `<?` and `?>` of the XML declaration.
    Decl(String),
}

#[derive(Debug, Clone)]
struct Node {
    parent: Option<NodeId>,
    first: Option<NodeId>,
    last: Option<NodeId>,
    prev: Option<NodeId>,
    next: Option<NodeId>,
    kind: Kind,
}

impl Node {
    fn new(kind: Kind) -> Node {
        Node {
            parent: None,
            first: None,
            last: None,
            prev: None,
            next: None,
            kind,
        }
    }
}

#[derive(Debug)]
pub enum DomError {
    /// Not well-formed XML.
    Xml(String),
    /// Well-formed but outside what sciink handles (encoding, entities, prefixes, root).
    Unsupported(String),
}

impl fmt::Display for DomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DomError::Xml(m) => write!(f, "invalid XML: {m}"),
            DomError::Unsupported(m) => write!(f, "unsupported document: {m}"),
        }
    }
}

impl std::error::Error for DomError {}

/// Namespaces whose prefix we rely on being the conventional one.
const KNOWN_NS: &[(&str, &str)] = &[
    ("http://www.w3.org/2000/svg", "svg"),
    ("http://www.inkscape.org/namespaces/inkscape", "inkscape"),
    (
        "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
        "sodipodi",
    ),
    ("http://www.w3.org/1999/xlink", "xlink"),
    ("http://www.w3.org/1999/02/22-rdf-syntax-ns#", "rdf"),
    ("http://creativecommons.org/ns#", "cc"),
    ("http://purl.org/dc/elements/1.1/", "dc"),
];

pub struct Doc {
    nodes: Vec<Node>,
    root: NodeId,
    svg: NodeId,
    ids: HashMap<String, NodeId>,
    // Read/written starting in Task 4 (navigation/mutation); laid down here so
    // the struct shape doesn't change under later tasks.
    next_auto_id: u32,
    /// Byte length of the parsed source; `write` pre-sizes its buffer from it (0 for built documents).
    source_len: usize,
    /// Bumped on every mutation; consumers cache derived data keyed by it.
    pub(crate) generation: Cell<u64>,
    /// Bumped when a `<style>` element or its text changes.
    pub(crate) sheet_generation: Cell<u64>,
    /// Bumped only when a mutation can change some node's specified style: any
    /// attach/detach (tree shape feeds selector combinators), a `<style>` text
    /// change (via `bump_sheet`), or a `set_attr`/`remove_attr` touching
    /// `style`/`class`/`id`/a presentation attribute. Geometry-only writes
    /// (`d`, `transform`, …) leave it alone, so `Caches.specified` survives them.
    pub(crate) style_generation: Cell<u64>,
    /// Style-cascade caches (owned here so `style.rs` can keep them on the document).
    pub(crate) caches: RefCell<crate::style::Caches>,
}

impl Doc {
    pub fn parse(bytes: &[u8]) -> Result<Doc, DomError> {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let text = std::str::from_utf8(bytes)
            .map_err(|_| DomError::Unsupported("input is not valid UTF-8".to_string()))?;
        let mut reader = quick_xml::Reader::from_str(text);
        {
            let cfg = reader.config_mut();
            cfg.trim_text_start = false;
            cfg.trim_text_end = false;
            cfg.expand_empty_elements = false;
            cfg.check_end_names = true;
        }
        let mut doc = Doc {
            nodes: Vec::with_capacity(1024),
            root: 0,
            svg: 0,
            ids: HashMap::new(),
            next_auto_id: 1,
            source_len: bytes.len(),
            generation: Cell::new(0),
            sheet_generation: Cell::new(0),
            style_generation: Cell::new(0),
            caches: RefCell::new(crate::style::Caches::default()),
        };
        doc.nodes.push(Node::new(Kind::Document));
        let mut stack: Vec<NodeId> = vec![0];
        let mut text_buf = String::new();
        let mut text_pending = false;
        loop {
            let ev = reader
                .read_event()
                .map_err(|e| DomError::Xml(e.to_string()))?;
            let continues_text = matches!(ev, Event::Text(_) | Event::GeneralRef(_));
            if text_pending && !continues_text {
                let id = doc.alloc(Kind::Text(std::mem::take(&mut text_buf)));
                doc.link_last(*stack.last().unwrap(), id);
                text_pending = false;
            }
            let parent = *stack.last().unwrap();
            match ev {
                Event::Text(t) => {
                    text_buf.push_str(&t);
                    text_pending = true;
                }
                Event::GeneralRef(r) => {
                    let name: &str = &r;
                    let ch = match name {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "quot" => '"',
                        "apos" => '\'',
                        _ => r
                            .resolve_char_ref()
                            .map_err(|e| DomError::Xml(e.to_string()))?
                            .ok_or_else(|| {
                                DomError::Unsupported(format!("undefined entity &{name};"))
                            })?,
                    };
                    text_buf.push(ch);
                    text_pending = true;
                }
                Event::Start(s) => {
                    let id = doc.element_from_start(&s, false)?;
                    doc.link_last(parent, id);
                    stack.push(id);
                }
                Event::Empty(s) => {
                    let id = doc.element_from_start(&s, true)?;
                    doc.link_last(parent, id);
                }
                Event::End(_) => {
                    if stack.len() > 1 {
                        stack.pop();
                    }
                }
                Event::CData(c) => {
                    let id = doc.alloc(Kind::CData(c.into_inner().into_owned()));
                    doc.link_last(parent, id);
                }
                Event::Comment(c) => {
                    let id = doc.alloc(Kind::Comment(c.into_inner().into_owned()));
                    doc.link_last(parent, id);
                }
                Event::PI(p) => {
                    let id = doc.alloc(Kind::PI(p.into_inner().into_owned()));
                    doc.link_last(parent, id);
                }
                Event::Decl(d) => {
                    let raw: &str = &d;
                    let id = doc.alloc(Kind::Decl(raw.to_string()));
                    doc.link_last(parent, id);
                }
                Event::DocType(t) => {
                    let id = doc.alloc(Kind::DocType(t.into_inner().into_owned()));
                    doc.link_last(parent, id);
                }
                Event::Eof => break,
            }
        }
        let svg = doc
            .children(doc.root)
            .find(|&c| doc.is_element(c))
            .ok_or_else(|| DomError::Unsupported("document has no root element".to_string()))?;
        if doc.tag(svg) != "svg" {
            return Err(DomError::Unsupported(format!(
                "root element is <{}>, not <svg>",
                doc.qname(svg)
            )));
        }
        doc.svg = svg;
        doc.check_namespaces()?;
        Ok(doc)
    }

    /// Checks every element in the document (not just the root) for a known
    /// namespace URI bound to a non-conventional prefix, e.g. a descendant
    /// declaring `xmlns:ink="...inkscape..."` instead of `xmlns:inkscape`.
    fn check_namespaces(&self) -> Result<(), DomError> {
        for n in self.descendants(self.root) {
            if !self.is_element(n) {
                continue;
            }
            for a in self.attrs(n) {
                let Some(prefix) = a.name.strip_prefix("xmlns:") else {
                    continue;
                };
                if let Some((uri, std_prefix)) = KNOWN_NS.iter().find(|(uri, _)| *uri == a.value) {
                    if prefix != *std_prefix {
                        return Err(DomError::Unsupported(format!(
                            "namespace {uri} is bound to prefix '{prefix}' (expected '{std_prefix}')"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn element_from_start(
        &mut self,
        s: &BytesStart<'_>,
        self_closing: bool,
    ) -> Result<NodeId, DomError> {
        let name = s.name().0.to_string();
        let (attrs, close_ws) = parse_attributes(s.attributes_raw())?;
        let id = self.alloc(Kind::Element {
            name,
            attrs,
            self_closing,
            close_ws,
        });
        if let Some(idv) = self.attr(id, "id").map(str::to_string) {
            self.ids.entry(idv).or_insert(id);
        }
        Ok(id)
    }

    fn alloc(&mut self, kind: Kind) -> NodeId {
        self.nodes.push(Node::new(kind));
        (self.nodes.len() - 1) as NodeId
    }

    fn link_last(&mut self, parent: NodeId, child: NodeId) {
        let last = self.nodes[parent as usize].last;
        {
            let c = &mut self.nodes[child as usize];
            c.parent = Some(parent);
            c.prev = last;
            c.next = None;
        }
        match last {
            Some(l) => self.nodes[l as usize].next = Some(child),
            None => self.nodes[parent as usize].first = Some(child),
        }
        self.nodes[parent as usize].last = Some(child);
    }

    /// Serializes the document; never pretty-prints, never reorders anything.
    pub fn write(&self, out: &mut Vec<u8>) {
        enum Step {
            Open(NodeId),
            Close(NodeId),
        }
        if out.is_empty() {
            out.reserve(self.source_len + self.source_len / 16);
        }
        let mut stack: Vec<Step> = Vec::new();
        let mut c = self.nodes[self.root as usize].last;
        while let Some(n) = c {
            stack.push(Step::Open(n));
            c = self.nodes[n as usize].prev;
        }
        while let Some(step) = stack.pop() {
            match step {
                Step::Close(n) => {
                    if let Kind::Element { name, .. } = &self.nodes[n as usize].kind {
                        out.extend_from_slice(b"</");
                        out.extend_from_slice(name.as_bytes());
                        out.push(b'>');
                    }
                }
                Step::Open(n) => {
                    let node = &self.nodes[n as usize];
                    match &node.kind {
                        Kind::Document => {}
                        Kind::Element {
                            name,
                            attrs,
                            self_closing,
                            close_ws,
                        } => {
                            out.push(b'<');
                            out.extend_from_slice(name.as_bytes());
                            for a in attrs {
                                out.extend_from_slice(a.ws.as_bytes());
                                out.extend_from_slice(a.name.as_bytes());
                                out.extend_from_slice(b"=\"");
                                escape_attr(&a.value, out);
                                out.push(b'"');
                            }
                            out.extend_from_slice(close_ws.as_bytes());
                            if node.first.is_none() && *self_closing {
                                out.extend_from_slice(b"/>");
                            } else {
                                out.push(b'>');
                                stack.push(Step::Close(n));
                                let mut c = node.last;
                                while let Some(k) = c {
                                    stack.push(Step::Open(k));
                                    c = self.nodes[k as usize].prev;
                                }
                            }
                        }
                        Kind::Text(t) => escape_text(t, out),
                        Kind::CData(t) => {
                            out.extend_from_slice(b"<![CDATA[");
                            out.extend_from_slice(t.as_bytes());
                            out.extend_from_slice(b"]]>");
                        }
                        Kind::Comment(t) => {
                            out.extend_from_slice(b"<!--");
                            out.extend_from_slice(t.as_bytes());
                            out.extend_from_slice(b"-->");
                        }
                        Kind::PI(t) | Kind::Decl(t) => {
                            out.extend_from_slice(b"<?");
                            out.extend_from_slice(t.as_bytes());
                            out.extend_from_slice(b"?>");
                        }
                        Kind::DocType(t) => {
                            out.extend_from_slice(b"<!DOCTYPE ");
                            out.extend_from_slice(t.as_bytes());
                            out.push(b'>');
                        }
                    }
                }
            }
        }
    }

    // ---- minimal accessors used by parse/write; the full API is Task 4 ----

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn svg(&self) -> NodeId {
        self.svg
    }

    /// Bumped by every mutation of the tree reachable from `svg()`; consumers
    /// cache derived data keyed by it to know when to recompute.
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Bumped whenever a `<style>` element is attached, detached, or its text
    /// changes; consumers of the parsed stylesheet key their cache on this
    /// instead of `generation()` so unrelated edits don't force a re-parse.
    pub fn sheet_generation(&self) -> u64 {
        self.sheet_generation.get()
    }

    /// Bumped whenever a mutation could change some node's specified style
    /// (see the field doc on `style_generation`); unaffected by geometry-only
    /// attribute writes. `style.rs` keys `Caches.specified` on this instead of
    /// `generation()` so e.g. a `transform`/`d` write doesn't evict every
    /// cached style under the mutated node.
    pub fn style_generation(&self) -> u64 {
        self.style_generation.get()
    }

    pub fn kind(&self, n: NodeId) -> &Kind {
        &self.nodes[n as usize].kind
    }

    pub fn is_element(&self, n: NodeId) -> bool {
        matches!(self.kind(n), Kind::Element { .. })
    }

    /// Qualified name as written (`inkscape:page`), or `""` for non-elements.
    pub fn qname(&self, n: NodeId) -> &str {
        match self.kind(n) {
            Kind::Element { name, .. } => name,
            _ => "",
        }
    }

    /// Local name (`page` for `inkscape:page`), or `""` for non-elements.
    pub fn tag(&self, n: NodeId) -> &str {
        let q = self.qname(n);
        q.rsplit(':').next().unwrap_or(q)
    }

    pub fn attrs(&self, n: NodeId) -> &[Attr] {
        match self.kind(n) {
            Kind::Element { attrs, .. } => attrs,
            _ => &[],
        }
    }

    pub fn attr(&self, n: NodeId, name: &str) -> Option<&str> {
        self.attrs(n)
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    pub fn by_id(&self, id: &str) -> Option<NodeId> {
        self.ids.get(id).copied()
    }

    pub fn children(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut c = self.nodes[n as usize].first;
        std::iter::from_fn(move || {
            let cur = c?;
            c = self.nodes[cur as usize].next;
            Some(cur)
        })
    }

    /// Pre-order traversal including `n` itself. Iterative, so depth is unbounded.
    pub fn descendants(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut stack = vec![n];
        std::iter::from_fn(move || {
            let cur = stack.pop()?;
            let mut c = self.nodes[cur as usize].last;
            while let Some(k) = c {
                stack.push(k);
                c = self.nodes[k as usize].prev;
            }
            Some(cur)
        })
    }

    /// Content of a Text/CData node.
    pub fn text(&self, n: NodeId) -> Option<&str> {
        match self.kind(n) {
            Kind::Text(t) | Kind::CData(t) => Some(t),
            _ => None,
        }
    }

    /// Content of a Comment node (without the `<!--`/`-->`).
    pub fn comment(&self, n: NodeId) -> Option<&str> {
        match self.kind(n) {
            Kind::Comment(t) => Some(t),
            _ => None,
        }
    }

    /// Concatenated text of all Text/CData descendants (used for `<style>` sheets).
    pub fn text_content(&self, n: NodeId) -> String {
        let mut s = String::new();
        for d in self.descendants(n) {
            if let Some(t) = self.text(d) {
                s.push_str(t);
            }
        }
        s
    }

    pub fn parent(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].parent
    }

    pub fn first_child(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].first
    }

    pub fn last_child(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].last
    }

    pub fn next_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].next
    }

    pub fn prev_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].prev
    }

    /// Parent chain, nearest first, ending with the Document node.
    pub fn ancestors(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut c = self.nodes[n as usize].parent;
        std::iter::from_fn(move || {
            let cur = c?;
            c = self.nodes[cur as usize].parent;
            Some(cur)
        })
    }

    pub fn is_text(&self, n: NodeId) -> bool {
        matches!(self.kind(n), Kind::Text(_) | Kind::CData(_))
    }

    pub fn is_comment(&self, n: NodeId) -> bool {
        matches!(self.kind(n), Kind::Comment(_))
    }

    /// Number of elements below the root `<svg>` (excluding it).
    pub fn element_count(&self) -> usize {
        self.descendants(self.svg)
            .skip(1)
            .filter(|&n| self.is_element(n))
            .count()
    }

    pub fn set_attr(&mut self, n: NodeId, name: &str, value: impl Into<String>) {
        let value = value.into();
        if name == "id" {
            if let Some(old) = self.attr(n, "id").map(str::to_string) {
                if self.ids.get(&old) == Some(&n) {
                    self.ids.remove(&old);
                }
            }
            self.ids.insert(value.clone(), n);
        }
        if let Kind::Element { attrs, .. } = &mut self.nodes[n as usize].kind {
            match attrs.iter_mut().find(|a| a.name == name) {
                Some(a) => a.value = value,
                None => attrs.push(Attr {
                    name: name.to_string(),
                    value,
                    ws: " ".to_string(),
                }),
            }
        }
        if attr_affects_style(name) {
            self.bump_style();
        }
        self.bump();
    }

    /// Makes `prefix` usable on this document: `true` when the root `<svg>` already declares
    /// `xmlns:<prefix>`, when it is `xml`, or when the prefix is one of the standard ones
    /// (`KNOWN_NS`) — then the declaration is added to the root. `false` for an unknown prefix.
    pub fn ensure_prefix(&mut self, prefix: &str) -> bool {
        if prefix == "xml" {
            return true;
        }
        let svg = self.svg();
        let decl = format!("xmlns:{prefix}");
        if self.attr(svg, &decl).is_some() {
            return true;
        }
        match KNOWN_NS.iter().find(|(_, p)| *p == prefix) {
            Some((uri, _)) => {
                self.set_attr(svg, &decl, uri.to_string());
                true
            }
            None => false,
        }
    }

    pub fn remove_attr(&mut self, n: NodeId, name: &str) -> Option<String> {
        if name == "id" {
            if let Some(old) = self.attr(n, "id").map(str::to_string) {
                if self.ids.get(&old) == Some(&n) {
                    self.ids.remove(&old);
                }
            }
        }
        let Kind::Element { attrs, .. } = &mut self.nodes[n as usize].kind else {
            return None;
        };
        let i = attrs.iter().position(|a| a.name == name)?;
        let removed = attrs.remove(i).value;
        if attr_affects_style(name) {
            self.bump_style();
        }
        self.bump();
        Some(removed)
    }

    /// `xlink:href` or SVG 2 `href`.
    pub fn href(&self, n: NodeId) -> Option<&str> {
        self.attr(n, "xlink:href").or_else(|| self.attr(n, "href"))
    }

    pub fn set_text(&mut self, n: NodeId, s: &str) {
        match &mut self.nodes[n as usize].kind {
            Kind::Text(t) | Kind::CData(t) => *t = s.to_string(),
            _ => return,
        }
        if self.parent(n).is_some_and(|p| self.tag(p) == "style") {
            self.bump_sheet();
        }
        self.bump();
    }

    /// The Text node directly following `n` (lxml's `.tail`), if any.
    pub fn tail(&self, n: NodeId) -> Option<NodeId> {
        let nx = self.next_sibling(n)?;
        matches!(self.kind(nx), Kind::Text(_)).then_some(nx)
    }

    /// Returns the element's id, assigning `sciink-N` if it has none.
    pub fn ensure_id(&mut self, n: NodeId) -> String {
        if let Some(id) = self.attr(n, "id") {
            return id.to_string();
        }
        loop {
            let cand = format!("sciink-{}", self.next_auto_id);
            self.next_auto_id += 1;
            if !self.ids.contains_key(&cand) {
                self.set_attr(n, "id", cand.clone());
                return cand;
            }
        }
    }

    /// Renames an element, keeping its namespace prefix (`svg:line` → `svg:path`). Style caches
    /// are invalidated because tag selectors may now match differently.
    pub fn set_tag(&mut self, n: NodeId, local: &str) {
        let was_style = self.tag(n) == "style";
        let Kind::Element { name, .. } = &mut self.nodes[n as usize].kind else {
            return;
        };
        *name = match name.rsplit_once(':') {
            Some((prefix, _)) => format!("{prefix}:{local}"),
            None => local.to_string(),
        };
        if was_style || local == "style" {
            self.bump_sheet();
        }
        self.bump_style();
        self.bump();
    }

    /// `true` when the nearest `xml:space` on the element or an ancestor is `preserve`.
    pub fn xml_space_preserve(&self, n: NodeId) -> bool {
        std::iter::once(n)
            .chain(self.ancestors(n))
            .filter(|&a| self.is_element(a))
            .find_map(|a| self.attr(a, "xml:space"))
            .is_some_and(|v| v.trim() == "preserve")
    }

    /// Target of an `href`/`xlink:href` of the form `#id`.
    pub fn resolve_href(&self, n: NodeId) -> Option<NodeId> {
        let h = self.href(n)?.trim();
        let id = h.strip_prefix('#')?;
        self.by_id(id)
    }

    /// `prefix` + the smallest positive integer giving an unused id (not reserved).
    pub fn new_id(&mut self, prefix: &str) -> String {
        let mut i = 1u32;
        loop {
            let cand = format!("{prefix}{i}");
            if !self.ids.contains_key(&cand) {
                return cand;
            }
            i += 1;
        }
    }

    /// New detached element written as `<name/>` until children are added.
    pub fn new_element(&mut self, name: &str) -> NodeId {
        self.alloc(Kind::Element {
            name: name.to_string(),
            attrs: Vec::new(),
            self_closing: true,
            close_ws: String::new(),
        })
    }

    pub fn new_text(&mut self, s: &str) -> NodeId {
        self.alloc(Kind::Text(s.to_string()))
    }

    pub fn new_comment(&mut self, s: &str) -> NodeId {
        self.alloc(Kind::Comment(s.to_string()))
    }

    /// Detached copy of `n` and its subtree; `id` attributes are dropped.
    pub fn deep_clone(&mut self, n: NodeId) -> NodeId {
        fn copy_kind(k: &Kind) -> Kind {
            match k {
                Kind::Element {
                    name,
                    attrs,
                    self_closing,
                    close_ws,
                } => Kind::Element {
                    name: name.clone(),
                    attrs: attrs.iter().filter(|a| a.name != "id").cloned().collect(),
                    self_closing: *self_closing,
                    close_ws: close_ws.clone(),
                },
                other => other.clone(),
            }
        }
        let root_copy = self.alloc(copy_kind(&self.nodes[n as usize].kind));
        let mut stack: Vec<(NodeId, NodeId)> = vec![(n, root_copy)];
        while let Some((src, dst)) = stack.pop() {
            let kids: Vec<NodeId> = self.children(src).collect();
            for k in kids {
                let kc = self.alloc(copy_kind(&self.nodes[k as usize].kind));
                self.link_last(dst, kc);
                stack.push((k, kc));
            }
        }
        root_copy
    }

    /// Unlinks `n` from its parent (keeping its subtree). No-op if detached.
    pub fn detach(&mut self, n: NodeId) {
        let Some(p) = self.nodes[n as usize].parent else {
            return;
        };
        let (prev, next) = (self.nodes[n as usize].prev, self.nodes[n as usize].next);
        match prev {
            Some(x) => self.nodes[x as usize].next = next,
            None => self.nodes[p as usize].first = next,
        }
        match next {
            Some(x) => self.nodes[x as usize].prev = prev,
            None => self.nodes[p as usize].last = prev,
        }
        {
            let node = &mut self.nodes[n as usize];
            node.parent = None;
            node.prev = None;
            node.next = None;
        }
        self.unindex_subtree(n);
        if self.subtree_has_style(n) {
            self.bump_sheet();
        }
        // Detaching changes the (now former) ancestor chain the subtree saw;
        // see the comment in `after_attach`.
        self.bump_style();
        self.bump();
    }

    /// Panics if attaching `n` at `target` would make `n` its own ancestor:
    /// `target` is the future parent for `append_child`/`prepend_child`, or the
    /// anchor for `insert_before`/`insert_after`. A real `assert!` (not
    /// `debug_assert!`): misuse must panic even in release builds, because the
    /// alternative — `descendants` looping forever over a node that is its own
    /// ancestor — hangs the process instead of failing loudly. `main` catches
    /// panics and echoes the document back; it cannot recover from a hang.
    fn assert_can_attach(&self, n: NodeId, target: NodeId) {
        assert!(
            target != n && !self.ancestors(target).any(|a| a == n),
            "cannot attach a node inside its own subtree"
        );
    }

    pub fn append_child(&mut self, parent: NodeId, n: NodeId) {
        self.assert_can_attach(n, parent);
        self.detach(n);
        self.link_last(parent, n);
        self.after_attach(n);
    }

    pub fn prepend_child(&mut self, parent: NodeId, n: NodeId) {
        match self.nodes[parent as usize].first {
            Some(f) if f != n => self.insert_before(n, f),
            Some(_) => {}
            None => self.append_child(parent, n),
        }
    }

    pub fn insert_before(&mut self, n: NodeId, anchor: NodeId) {
        self.assert_can_attach(n, anchor);
        self.detach(n);
        let p = self.nodes[anchor as usize]
            .parent
            .expect("anchor must be attached");
        let prev = self.nodes[anchor as usize].prev;
        {
            let node = &mut self.nodes[n as usize];
            node.parent = Some(p);
            node.prev = prev;
            node.next = Some(anchor);
        }
        self.nodes[anchor as usize].prev = Some(n);
        match prev {
            Some(x) => self.nodes[x as usize].next = Some(n),
            None => self.nodes[p as usize].first = Some(n),
        }
        self.after_attach(n);
    }

    pub fn insert_after(&mut self, n: NodeId, anchor: NodeId) {
        self.assert_can_attach(n, anchor);
        match self.nodes[anchor as usize].next {
            Some(nx) if nx != n => self.insert_before(n, nx),
            Some(_) => {}
            None => {
                let p = self.nodes[anchor as usize]
                    .parent
                    .expect("anchor must be attached");
                self.append_child(p, n);
            }
        }
    }

    /// Puts `new` where `old` is and detaches `old`.
    pub fn replace(&mut self, old: NodeId, new: NodeId) {
        self.insert_before(new, old);
        self.detach(old);
    }

    /// First direct `<defs>` child of the root, created (and prepended) if absent.
    pub fn defs(&mut self) -> NodeId {
        if let Some(d) = self
            .children(self.svg)
            .find(|&c| self.is_element(c) && self.tag(c) == "defs")
        {
            return d;
        }
        let d = self.new_element("defs");
        self.prepend_child(self.svg, d);
        d
    }

    /// Nodes for the given ids in document order; unknown ids are dropped.
    pub fn selection(&self, ids: &[String]) -> Vec<NodeId> {
        let wanted: std::collections::HashSet<NodeId> =
            ids.iter().filter_map(|i| self.by_id(i)).collect();
        self.descendants(self.svg)
            .filter(|n| wanted.contains(n))
            .collect()
    }

    /// Nodes for the given ids in the order given (Inkscape's selection order), each once;
    /// unknown ids are dropped. The Scaler's match target is the FIRST selected object.
    pub fn selection_ordered(&self, ids: &[String]) -> Vec<NodeId> {
        let mut out: Vec<NodeId> = Vec::new();
        for id in ids {
            if let Some(n) = self.by_id(id) {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        out
    }

    pub(crate) fn bump(&self) {
        self.generation.set(self.generation.get() + 1);
    }

    fn bump_sheet(&self) {
        self.sheet_generation.set(self.sheet_generation.get() + 1);
        // A changed sheet can change what matches on any node, same as a sheet
        // attach/detach.
        self.bump_style();
    }

    fn bump_style(&self) {
        self.style_generation.set(self.style_generation.get() + 1);
    }

    fn after_attach(&mut self, n: NodeId) {
        self.index_subtree(n);
        if self.subtree_has_style(n) {
            self.bump_sheet();
        }
        // Attaching changes the tree shape the attached subtree sees (its
        // ancestor chain, and thus which descendant/child selectors match),
        // regardless of whether a <style> element is involved.
        self.bump_style();
        self.bump();
    }

    fn index_subtree(&mut self, n: NodeId) {
        let ids: Vec<(String, NodeId)> = self
            .descendants(n)
            .filter_map(|d| self.attr(d, "id").map(|id| (id.to_string(), d)))
            .collect();
        for (id, d) in ids {
            self.ids.entry(id).or_insert(d);
        }
    }

    fn unindex_subtree(&mut self, n: NodeId) {
        let ids: Vec<(String, NodeId)> = self
            .descendants(n)
            .filter_map(|d| self.attr(d, "id").map(|id| (id.to_string(), d)))
            .collect();
        for (id, d) in ids {
            if self.ids.get(&id) == Some(&d) {
                self.ids.remove(&id);
            }
        }
    }

    fn subtree_has_style(&self, n: NodeId) -> bool {
        self.descendants(n)
            .any(|d| self.is_element(d) && self.tag(d) == "style")
    }
}

/// Splits quick-xml's raw attribute string (everything after the tag name) into
/// attributes, remembering the whitespace before each one and after the last.
fn parse_attributes(raw: &str) -> Result<(Vec<Attr>, String), DomError> {
    let mut attrs = Vec::new();
    let mut rest = raw;
    loop {
        let trimmed = rest.trim_start();
        let ws = &rest[..rest.len() - trimmed.len()];
        if trimmed.is_empty() {
            return Ok((attrs, ws.to_string()));
        }
        let name_end = trimmed
            .find(|c: char| c == '=' || c.is_whitespace())
            .ok_or_else(|| DomError::Xml(format!("attribute without value: {trimmed}")))?;
        let name = &trimmed[..name_end];
        let after_eq = trimmed[name_end..]
            .trim_start()
            .strip_prefix('=')
            .ok_or_else(|| DomError::Xml(format!("attribute '{name}' has no '='")))?
            .trim_start();
        let quote = after_eq
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
            .ok_or_else(|| DomError::Xml(format!("attribute '{name}' value is not quoted")))?;
        let body = &after_eq[1..];
        let end = body
            .find(quote)
            .ok_or_else(|| DomError::Xml(format!("unterminated value for attribute '{name}'")))?;
        let value = quick_xml::escape::unescape(&body[..end])
            .map_err(|e| match e {
                // A well-formed but undefined/unsupported reference (e.g. `&nbsp;`).
                EscapeError::UnrecognizedEntity(..) => {
                    DomError::Unsupported(format!("attribute '{name}': {e}"))
                }
                // Everything else (unterminated `&...`, an invalid `&#...;` char
                // ref, runaway nested-entity expansion) is malformed XML.
                _ => DomError::Xml(format!("attribute '{name}': {e}")),
            })?
            .into_owned();
        attrs.push(Attr {
            name: name.to_string(),
            value,
            ws: ws.to_string(),
        });
        rest = &body[end + 1..];
    }
}

/// Whether writing/removing attribute `name` can change some node's cascade
/// (a presentation attribute, or one a selector can key off: `style`, `class`, `id`).
fn attr_affects_style(name: &str) -> bool {
    matches!(name, "style" | "class" | "id") || crate::style::PRESENTATION_ATTRS.contains(&name)
}

/// Copies `s` into `out` escaping `& < > "` — runs of ordinary bytes are copied in bulk.
fn escape_text(s: &str, out: &mut Vec<u8>) {
    let b = s.as_bytes();
    let mut start = 0usize;
    for i in 0..b.len() {
        let rep: &[u8] = match b[i] {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            b'"' => b"&quot;",
            _ => continue,
        };
        out.extend_from_slice(&b[start..i]);
        out.extend_from_slice(rep);
        start = i + 1;
    }
    out.extend_from_slice(&b[start..]);
}

/// Attribute values additionally escape the whitespace characters an attribute cannot hold raw.
fn escape_attr(s: &str, out: &mut Vec<u8>) {
    let b = s.as_bytes();
    let mut start = 0usize;
    for i in 0..b.len() {
        let rep: &[u8] = match b[i] {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            b'"' => b"&quot;",
            b'\n' => b"&#10;",
            b'\r' => b"&#13;",
            b'\t' => b"&#9;",
            _ => continue,
        };
        out.extend_from_slice(&b[start..i]);
        out.extend_from_slice(rep);
        start = i + 1;
    }
    out.extend_from_slice(&b[start..]);
}
