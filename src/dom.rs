//! Arena DOM for Inkscape SVG documents (spec §C.1).
//!
//! Lossless for everything Inkscape writes: XML declaration, DOCTYPE, comments,
//! processing instructions, CDATA, attribute order, the whitespace before each
//! attribute, the ` />` form, namespace prefixes as literal strings, all text.
//! Normalized on output: attribute quotes are always `"`, text is re-escaped
//! canonically (`& < >` in text, plus `"` and newline/tab/CR as char refs in
//! attributes), and the DOCTYPE keyword is followed by exactly one space.
//! Namespace prefixes are never resolved: `svg:path` and `path`
//! compare equal by local name (ponytail: standard prefixes are enforced at parse).

use std::cell::Cell;
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
    #[allow(dead_code)]
    next_auto_id: u32,
    /// Bumped on every mutation; consumers cache derived data keyed by it.
    #[allow(dead_code)]
    pub(crate) generation: Cell<u64>,
    /// Bumped when a `<style>` element or its text changes.
    #[allow(dead_code)]
    pub(crate) sheet_generation: Cell<u64>,
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
            generation: Cell::new(0),
            sheet_generation: Cell::new(0),
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

fn escape_text(s: &str, out: &mut Vec<u8>) {
    for b in s.bytes() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            _ => out.push(b),
        }
    }
}

fn escape_attr(s: &str, out: &mut Vec<u8>) {
    for b in s.bytes() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\n' => out.extend_from_slice(b"&#10;"),
            b'\r' => out.extend_from_slice(b"&#13;"),
            b'\t' => out.extend_from_slice(b"&#9;"),
            _ => out.push(b),
        }
    }
}
