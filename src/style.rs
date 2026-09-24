//! Style declarations and the "specified style" cascade (spec §C.2; upstream
//! `inkex/text/cache.py`).
//!
//! `specified_style(n)` = `specified_style(parent)` overridden by `cascaded_style(n)`,
//! where cascaded = presentation attributes < matching `<style>` rules (ordered by
//! `!important`, specificity, source order) < inline `style=""`. Every property
//! propagates down — upstream does not distinguish inherited properties and the
//! ported algorithms rely on that. Missing values come from `default_value`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::dom::{Doc, NodeId};

/// Ordered `(name, value)` declarations, serialized as `k:v;k:v` (Inkscape's own format).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style(pub Vec<(String, String)>);

impl Style {
    /// Parses `k:v;k:v`. Names are lower-cased and trimmed, empty or malformed
    /// declarations are skipped, the `font` shorthand is expanded.
    pub fn parse(css: &str) -> Style {
        let mut st = Style::default();
        for decl in split_declarations(css) {
            let Some((k, v)) = decl.split_once(':') else {
                continue;
            };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            if k.is_empty() || v.is_empty() {
                continue;
            }
            st.set_decl(&k, v);
        }
        st
    }

    fn set_decl(&mut self, k: &str, v: &str) {
        if k == "font" {
            expand_font_shorthand(self, v);
        } else {
            self.set(k, v);
        }
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    /// Replaces in place (keeping position) or appends.
    pub fn set(&mut self, k: &str, v: &str) {
        match self.0.iter_mut().find(|(n, _)| n == k) {
            Some(e) => e.1 = v.to_string(),
            None => self.0.push((k.to_string(), v.to_string())),
        }
    }

    pub fn remove(&mut self, k: &str) -> Option<String> {
        let i = self.0.iter().position(|(n, _)| n == k)?;
        Some(self.0.remove(i).1)
    }

    /// `other`'s declarations override ours.
    pub fn merge_over(&mut self, other: &Style) {
        for (k, v) in &other.0 {
            self.set(k, v);
        }
    }

    pub fn to_css(&self) -> String {
        self.0
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `font: [style] [variant] [weight] size[/line-height] family` (matplotlib emits
/// `font: 10px 'DejaVu Sans'` for every text when `svg.fonttype=none`).
fn expand_font_shorthand(st: &mut Style, v: &str) {
    let tokens = split_font_tokens(v);
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        match t {
            "italic" | "oblique" => st.set("font-style", t),
            "small-caps" => st.set("font-variant", t),
            "bold" | "bolder" | "lighter" | "100" | "200" | "300" | "400" | "500" | "600"
            | "700" | "800" | "900" => st.set("font-weight", t),
            "ultra-condensed" | "extra-condensed" | "condensed" | "semi-condensed"
            | "semi-expanded" | "expanded" | "extra-expanded" | "ultra-expanded" => {
                st.set("font-stretch", t)
            }
            "normal" => {}
            _ => break,
        }
        i += 1;
    }
    let Some(size) = tokens.get(i) else { return };
    match size.split_once('/') {
        Some((s, lh)) => {
            st.set("font-size", s);
            st.set("line-height", lh);
        }
        None => st.set("font-size", size),
    }
    let family = tokens[i + 1..].join(" ");
    if !family.is_empty() {
        st.set("font-family", &family);
    }
}

fn split_font_tokens(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in v.chars() {
        match quote {
            Some(q) => {
                cur.push(ch);
                if ch == q {
                    quote = None;
                }
            }
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                cur.push(ch);
            }
            None if ch.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Attributes that Inkscape treats as presentation attributes. A superset of
/// upstream `cache.py:267-332` (also minus `clip`, `clip-path`, `mask`,
/// `transform`, which are geometry, not cascade, there too): adds `font`,
/// `marker`, `paint-order`, `line-height`, `text-align`, `white-space`,
/// `inline-size`, `shape-*`, `mix-blend-mode`, `isolation`, `vertical-align`,
/// `font-kerning`, `font-variant-*`, `font-feature-settings`; omits
/// `color-profile`, `kerning`.
pub const PRESENTATION_ATTRS: &[&str] = &[
    "alignment-baseline",
    "baseline-shift",
    "clip-rule",
    "color",
    "color-interpolation",
    "color-interpolation-filters",
    "color-rendering",
    "cursor",
    "direction",
    "display",
    "dominant-baseline",
    "enable-background",
    "fill",
    "fill-opacity",
    "fill-rule",
    "filter",
    "flood-color",
    "flood-opacity",
    "font",
    "font-family",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-variant",
    "font-weight",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "image-rendering",
    "letter-spacing",
    "lighting-color",
    "marker",
    "marker-end",
    "marker-mid",
    "marker-start",
    "opacity",
    "overflow",
    "paint-order",
    "pointer-events",
    "shape-rendering",
    "stop-color",
    "stop-opacity",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "text-anchor",
    "text-decoration",
    "text-rendering",
    "unicode-bidi",
    "vector-effect",
    "visibility",
    "word-spacing",
    "writing-mode",
    "line-height",
    "text-align",
    "white-space",
    "inline-size",
    "shape-inside",
    "shape-padding",
    "shape-subtract",
    "mix-blend-mode",
    "isolation",
    "vertical-align",
    "font-feature-settings",
    "font-variant-ligatures",
    "font-variant-caps",
    "font-variant-numeric",
    "font-variant-east-asian",
    "font-variant-position",
    "font-variant-alternates",
    "font-kerning",
];

/// SVG initial values (spec §C.2; cross-checked with Inkscape's `inkex/properties.py`).
const DEFAULTS: &[(&str, &str)] = &[
    ("fill", "black"),
    ("stroke", "none"),
    ("stroke-width", "1"),
    ("font-size", "medium"),
    ("font-family", "sans-serif"),
    ("font-weight", "normal"),
    ("font-style", "normal"),
    ("font-stretch", "normal"),
    ("font-variant", "normal"),
    ("text-anchor", "start"),
    ("letter-spacing", "normal"),
    ("word-spacing", "normal"),
    ("line-height", "normal"),
    ("opacity", "1"),
    ("fill-opacity", "1"),
    ("stroke-opacity", "1"),
    ("stroke-linecap", "butt"),
    ("stroke-linejoin", "miter"),
    ("stroke-miterlimit", "4"),
    ("stroke-dasharray", "none"),
    ("stroke-dashoffset", "0"),
    ("display", "inline"),
    ("visibility", "visible"),
    ("clip-path", "none"),
    ("mask", "none"),
    ("filter", "none"),
    ("marker-start", "none"),
    ("marker-mid", "none"),
    ("marker-end", "none"),
    ("direction", "ltr"),
    ("writing-mode", "lr-tb"),
    ("white-space", "normal"),
    ("stop-color", "black"),
    ("stop-opacity", "1"),
    ("color", "black"),
    ("fill-rule", "nonzero"),
    ("clip-rule", "nonzero"),
    ("text-decoration", "none"),
    ("baseline-shift", "baseline"),
    ("text-align", "start"),
    ("font-variant-ligatures", "normal"),
    ("paint-order", "normal"),
    ("vector-effect", "none"),
    ("shape-rendering", "auto"),
    ("text-rendering", "auto"),
];

pub fn default_value(prop: &str) -> Option<&'static str> {
    DEFAULTS.iter().find(|(k, _)| *k == prop).map(|(_, v)| *v)
}

// ---------------------------------------------------------------------------
// Stylesheets: the selector subset exporters actually use (`*`, tag, .class,
// #id, compounds, comma lists, descendant and child combinators).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

impl Compound {
    fn matches(&self, doc: &Doc, n: NodeId) -> bool {
        if let Some(t) = &self.tag {
            if doc.tag(n) != t {
                return false;
            }
        }
        if let Some(id) = &self.id {
            if doc.attr(n, "id") != Some(id.as_str()) {
                return false;
            }
        }
        if !self.classes.is_empty() {
            let cls = doc.attr(n, "class").unwrap_or("");
            for c in &self.classes {
                if !cls.split_ascii_whitespace().any(|x| x == c) {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
}

#[derive(Debug, Clone)]
struct Selector {
    /// `parts[k].1` relates `parts[k]` to `parts[k-1]`; the first is `None`.
    parts: Vec<(Compound, Option<Combinator>)>,
    specificity: (u32, u32, u32),
}

impl Selector {
    fn matches(&self, doc: &Doc, n: NodeId) -> bool {
        self.match_from(doc, n, self.parts.len() - 1)
    }

    fn match_from(&self, doc: &Doc, n: NodeId, idx: usize) -> bool {
        let (comp, comb) = &self.parts[idx];
        if !comp.matches(doc, n) {
            return false;
        }
        if idx == 0 {
            return true;
        }
        match comb {
            Some(Combinator::Descendant) => {
                let mut a = doc.parent(n);
                while let Some(p) = a {
                    if doc.is_element(p) && self.match_from(doc, p, idx - 1) {
                        return true;
                    }
                    a = doc.parent(p);
                }
                false
            }
            _ => {
                let Some(p) = doc.parent(n) else { return false };
                doc.is_element(p) && self.match_from(doc, p, idx - 1)
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Rule {
    selector: Selector,
    /// `(name, value, important)`
    decls: Vec<(String, String, bool)>,
    order: usize,
    /// A lone `*` compound: matches every element unconditionally.
    universal: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
    /// The lone-`*` rules' declarations folded in source order (see `cascaded_style`); only
    /// meaningful when `universal_folded`.
    universal_normal: Style,
    universal_important: Style,
    /// `order` of the first lone-`*` rule (the folded block's position in the cascade).
    universal_order: usize,
    /// True when every rule of specificity (0,0,0) is a lone `*`, so those rules can be applied as
    /// one pre-merged block without changing the cascade order. A `* *` rule disables the fold.
    universal_folded: bool,
    /// Every property name declared anywhere in the sheet (`sheet_value` short-circuit).
    props: HashSet<String>,
    /// Non-empty selectors `parse_selector` could not represent (attribute selectors,
    /// pseudo-classes, sibling combinators), plus every `@` statement or block skipped whole.
    unsupported: usize,
}

impl Stylesheet {
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// How much of the sheet sciink could not analyse (see `unsupported`): a caller that needs to
    /// know whether it has seen the *whole* stylesheet checks this, not just `rule_count`.
    pub fn unsupported_rules(&self) -> usize {
        self.unsupported
    }

    /// True when every rule is a lone `*`: nothing can match one element and not another.
    pub fn only_universal_rules(&self) -> bool {
        self.rules.iter().all(|r| r.universal)
    }

    /// True when the sheet declares any of `props` (lower-case property names).
    pub fn declares_any(&self, props: &[&str]) -> bool {
        props.iter().any(|p| self.props.contains(*p))
    }
}

pub fn parse_stylesheet(css: &str) -> Stylesheet {
    let css = strip_comments(css);
    let bytes = css.as_bytes();
    let mut sheet = Stylesheet::default();
    let mut i = 0;
    let mut order = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'@' {
            let mut j = i;
            while j < bytes.len() && bytes[j] != b';' && bytes[j] != b'{' {
                j += 1;
            }
            i = if j < bytes.len() && bytes[j] == b'{' {
                skip_block(bytes, j)
            } else {
                j + 1
            };
            sheet.unsupported += 1;
            continue;
        }
        let Some(open) = css[i..].find('{') else {
            break;
        };
        let selector_text = &css[i..i + open];
        let close = skip_block(bytes, i + open);
        let body_end = if close > i + open + 1 && bytes[close - 1] == b'}' {
            close - 1
        } else {
            close
        };
        let decls = parse_declarations(&css[i + open + 1..body_end]);
        for sel_text in selector_text.split(',') {
            let sel_text = sel_text.trim();
            match parse_selector(sel_text) {
                Some(selector) => {
                    sheet.rules.push(Rule {
                        selector,
                        decls: decls.clone(),
                        order,
                        universal: false,
                    });
                    order += 1;
                }
                None if !sel_text.is_empty() => sheet.unsupported += 1,
                None => {}
            }
        }
        i = close;
    }
    finish_sheet(sheet)
}

/// Computes the per-rule `universal` flag, the folded universal blocks and the property index.
fn finish_sheet(mut sheet: Stylesheet) -> Stylesheet {
    let mut folded = true;
    let mut first: Option<usize> = None;
    for r in &mut sheet.rules {
        let c = &r.selector.parts[0].0;
        r.universal = r.selector.parts.len() == 1
            && c.tag.is_none()
            && c.id.is_none()
            && c.classes.is_empty();
        if r.selector.specificity == (0, 0, 0) && !r.universal {
            folded = false;
        }
        for (k, _, _) in &r.decls {
            sheet.props.insert(k.clone());
        }
    }
    if folded {
        for r in sheet.rules.iter().filter(|r| r.universal) {
            first.get_or_insert(r.order);
            for (k, v, imp) in &r.decls {
                if *imp {
                    sheet.universal_important.set_decl(k, v);
                } else {
                    sheet.universal_normal.set_decl(k, v);
                }
            }
        }
    }
    sheet.universal_folded = folded;
    sheet.universal_order = first.unwrap_or(0);
    sheet
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Index just past the `}` matching the `{` at `open` (or `bytes.len()`).
fn skip_block(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    let mut j = open;
    while j < bytes.len() {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    bytes.len()
}

fn parse_declarations(body: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for decl in split_declarations(body) {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        let (v, important) = strip_important(v.trim());
        if k.is_empty() || v.is_empty() {
            continue;
        }
        out.push((k, v.to_string(), important));
    }
    out
}

/// Splits a declaration list on `;`, the way CSS actually delimits
/// declarations: a `;` inside a quoted string (`content:'a;b'`) or inside
/// parentheses (`fill:url(data:image/png;base64,...)`) does not start a new
/// declaration. Quotes and parens are tracked independently of the other, so
/// a `;` is only a real separator when both are at depth zero.
fn split_declarations(css: &str) -> impl Iterator<Item = &str> {
    let bytes = css.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<u8> = None;
    let mut depth = 0u32;
    for (i, &b) in bytes.iter().enumerate() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'\'' | b'"' => quote = Some(b),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => {
                out.push(&css[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&css[start..]);
    out.into_iter()
}

fn strip_important(v: &str) -> (&str, bool) {
    let lower = v.to_ascii_lowercase();
    if lower.ends_with("important") {
        let rest = v[..v.len() - "important".len()].trim_end();
        if let Some(rest) = rest.strip_suffix('!') {
            return (rest.trim_end(), true);
        }
    }
    (v, false)
}

fn parse_selector(text: &str) -> Option<Selector> {
    if text.is_empty() {
        return None;
    }
    let mut parts: Vec<(Compound, Option<Combinator>)> = Vec::new();
    let mut cur = Compound::default();
    let mut have_cur = false;
    let mut pending: Option<Combinator> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if have_cur {
                    parts.push((std::mem::take(&mut cur), pending.take()));
                    have_cur = false;
                    pending = Some(Combinator::Descendant);
                }
            }
            '>' => {
                if have_cur {
                    parts.push((std::mem::take(&mut cur), pending.take()));
                    have_cur = false;
                }
                pending = Some(Combinator::Child);
            }
            '*' => have_cur = true,
            '.' | '#' => {
                let ident = read_ident(&mut chars);
                if ident.is_empty() {
                    return None;
                }
                if c == '.' {
                    cur.classes.push(ident);
                } else {
                    cur.id = Some(ident);
                }
                have_cur = true;
            }
            c if c.is_alphanumeric() || c == '_' || c == '-' => {
                let mut ident = String::from(c);
                ident.push_str(&read_ident(&mut chars));
                cur.tag = Some(ident);
                have_cur = true;
            }
            // attribute selectors, pseudo-classes, sibling combinators: not supported → drop the rule
            _ => return None,
        }
    }
    if have_cur {
        parts.push((cur, pending.take()));
    } else if pending.is_some() && !parts.is_empty() {
        // A trailing combinator with nothing after it (`svg >`): the selector
        // is malformed, not equivalent to dropping the combinator.
        return None;
    }
    if parts.is_empty() {
        return None;
    }
    let mut specificity = (0, 0, 0);
    for (comp, _) in &parts {
        specificity.0 += u32::from(comp.id.is_some());
        specificity.1 += comp.classes.len() as u32;
        specificity.2 += u32::from(comp.tag.is_some());
    }
    Some(Selector { parts, specificity })
}

fn read_ident(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '_' || c == '-' {
            s.push(c);
            chars.next();
        } else {
            break;
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Cascade on the document
// ---------------------------------------------------------------------------

/// Sort key for a cascaded declaration: `(important, is_inline, specificity, order)`.
/// Ties are broken lexicographically, so the highest key wins.
type DeclKey = (bool, bool, (u32, u32, u32), usize);

/// Sort key for a `sheet_value` match: `(important, specificity, order)` (no `is_inline`
/// component — inline `style=""` is not a sheet declaration and never competes here).
type SheetKey = (bool, (u32, u32, u32), usize);

/// Per-document caches, invalidated through `Doc`'s generation counters.
#[derive(Default)]
pub struct Caches {
    sheet: Option<(u64, Rc<Stylesheet>)>,
    specified: HashMap<NodeId, (u64, Rc<Style>)>,
}

impl Doc {
    /// The document stylesheet: every `<style>` element's text concatenated in document order
    /// (any depth), cached on `sheet_generation`.
    pub fn stylesheet(&self) -> Rc<Stylesheet> {
        let g = self.sheet_generation.get();
        if let Some((sg, s)) = &self.caches.borrow().sheet {
            if *sg == g {
                return s.clone();
            }
        }
        let mut css = String::new();
        for n in self.descendants(self.svg()) {
            if self.is_element(n) && self.tag(n) == "style" {
                css.push_str(&self.text_content(n));
                css.push('\n');
            }
        }
        let sheet = Rc::new(parse_stylesheet(&css));
        self.caches.borrow_mut().sheet = Some((g, sheet.clone()));
        sheet
    }

    /// The element's own declarations from all three sources, no inheritance.
    /// This is what `ungroup` pushes down onto children.
    pub fn cascaded_style(&self, n: NodeId) -> Style {
        // sort key: (important, is_inline, specificity, order) — later wins; the sort is stable,
        // so declarations pushed with equal keys keep their push order
        let sheet = self.stylesheet();
        let inline = self.attr(n, "style");
        let mut decls: Vec<(DeclKey, Cow<'_, str>, &str)> = Vec::new();
        for a in self.attrs(n) {
            if PRESENTATION_ATTRS.contains(&a.name.as_str()) {
                decls.push((
                    (false, false, (0, 0, 0), 0),
                    Cow::Borrowed(a.name.as_str()),
                    a.value.as_str(),
                ));
            }
        }
        if sheet.universal_folded {
            let key = (false, false, (0, 0, 0), sheet.universal_order + 1);
            for (k, v) in &sheet.universal_normal.0 {
                decls.push((key, Cow::Borrowed(k.as_str()), v.as_str()));
            }
            let key = (true, false, (0, 0, 0), sheet.universal_order + 1);
            for (k, v) in &sheet.universal_important.0 {
                decls.push((key, Cow::Borrowed(k.as_str()), v.as_str()));
            }
        }
        for rule in &sheet.rules {
            if (sheet.universal_folded && rule.universal) || !rule.selector.matches(self, n) {
                continue;
            }
            for (k, v, imp) in &rule.decls {
                decls.push((
                    (*imp, false, rule.selector.specificity, rule.order + 1),
                    Cow::Borrowed(k.as_str()),
                    v.as_str(),
                ));
            }
        }
        if let Some(inline) = inline {
            for (idx, decl) in split_declarations(inline).enumerate() {
                let Some((k, v)) = decl.split_once(':') else {
                    continue;
                };
                let k = k.trim();
                let (v, imp) = strip_important(v.trim());
                if k.is_empty() || v.is_empty() {
                    continue;
                }
                let k = if k.bytes().any(|b| b.is_ascii_uppercase()) {
                    Cow::Owned(k.to_ascii_lowercase())
                } else {
                    Cow::Borrowed(k)
                };
                decls.push(((imp, true, (0, 0, 0), idx), k, v));
            }
        }
        decls.sort_by_key(|d| d.0);
        let mut st = Style::default();
        for (_, k, v) in &decls {
            st.set_decl(k, v);
        }
        st
    }

    /// Parent's specified style overridden by this element's cascaded style (cached).
    pub fn specified_style(&self, n: NodeId) -> Rc<Style> {
        let g = self.style_generation.get();
        if let Some(s) = self.cached_specified(n, g) {
            return s;
        }
        let mut chain = vec![n];
        let mut base: Option<Rc<Style>> = None;
        let mut cur = self.parent(n);
        while let Some(p) = cur {
            if !self.is_element(p) {
                break;
            }
            if let Some(s) = self.cached_specified(p, g) {
                base = Some(s);
                break;
            }
            chain.push(p);
            cur = self.parent(p);
        }
        let mut acc: Style = base.as_ref().map(|s| (**s).clone()).unwrap_or_default();
        // `result` doubles as "the Rc for the node just processed": when a node
        // adds no declarations of its own, its specified style is identical to
        // its parent's, so it shares that Rc instead of cloning `acc` into a
        // fresh allocation (style-less `<g>` chains then cost one Rc total).
        let mut result: Option<Rc<Style>> = base;
        for &node in chain.iter().rev() {
            let cascaded = self.cascaded_style(node);
            let rc = if cascaded.is_empty() {
                result.clone().unwrap_or_else(|| Rc::new(Style::default()))
            } else {
                acc.merge_over(&cascaded);
                Rc::new(acc.clone())
            };
            self.caches
                .borrow_mut()
                .specified
                .insert(node, (g, rc.clone()));
            result = Some(rc);
        }
        result.expect("chain is never empty")
    }

    fn cached_specified(&self, n: NodeId, g: u64) -> Option<Rc<Style>> {
        self.caches
            .borrow()
            .specified
            .get(&n)
            .filter(|(cg, _)| *cg == g)
            .map(|(_, s)| s.clone())
    }

    pub fn specified(&self, n: NodeId, prop: &str) -> Option<String> {
        self.specified_style(n).get(prop).map(str::to_string)
    }

    /// Specified value or the SVG initial value (`""` for unknown properties).
    pub fn computed(&self, n: NodeId, prop: &str) -> String {
        self.specified(n, prop)
            .unwrap_or_else(|| default_value(prop).unwrap_or("").to_string())
    }

    /// Writes into the inline `style` attribute and drops a same-named presentation attribute.
    pub fn set_style(&mut self, n: NodeId, prop: &str, value: &str) {
        let mut st = self.attr(n, "style").map(Style::parse).unwrap_or_default();
        st.set(prop, value);
        self.set_attr(n, "style", st.to_css());
        if PRESENTATION_ATTRS.contains(&prop) {
            self.remove_attr(n, prop);
        }
    }

    /// Replaces the whole inline `style` attribute.
    pub fn set_style_map(&mut self, n: NodeId, st: &Style) {
        if st.is_empty() {
            self.remove_attr(n, "style");
        } else {
            self.set_attr(n, "style", st.to_css());
        }
    }

    /// Removes the property from the inline style and as a presentation attribute.
    pub fn remove_style(&mut self, n: NodeId, prop: &str) {
        if let Some(inline) = self.attr(n, "style") {
            let mut st = Style::parse(inline);
            if st.remove(prop).is_some() {
                self.set_style_map(n, &st);
            }
        }
        if PRESENTATION_ATTRS.contains(&prop) {
            self.remove_attr(n, prop);
        }
    }

    /// The value the `<style>` sheets alone give `prop` on `n` — no presentation attribute, no
    /// inline `style` — i.e. what Inkscape would apply over an attribute we write (upstream
    /// `svg.cssdict[id][prop]`, cache.py:1073–1164). Highest key `(!important, specificity,
    /// source order)` wins; later declarations win ties.
    pub fn sheet_value(&self, n: NodeId, prop: &str) -> Option<String> {
        let sheet = self.stylesheet();
        if !sheet.props.contains(prop) {
            return None;
        }
        let mut best: Option<(SheetKey, String)> = None;
        for rule in &sheet.rules {
            if !rule.selector.matches(self, n) {
                continue;
            }
            for (k, v, imp) in &rule.decls {
                if k != prop {
                    continue;
                }
                let key = (*imp, rule.selector.specificity, rule.order);
                if best.as_ref().is_none_or(|(bk, _)| key >= *bk) {
                    best = Some((key, v.clone()));
                }
            }
        }
        best.map(|(_, v)| v)
    }
}
