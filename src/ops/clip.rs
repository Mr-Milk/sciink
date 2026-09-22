//! Composing a container's properties onto its children, clip/mask merging, ungrouping,
//! unlinking clones, language switches (spec §B.2; upstream DH:238–352 `unlink2`/`ungroup`/
//! `group`/`deswitch`, DH:359–391 `compose_all`, DH:416–504 clip merging).

use kurbo::{Affine, Rect, Shape};

use crate::dom::{Doc, NodeId};
use crate::geom::path::{end_points, fmt_d, shape_path};
use crate::geom::{inverse, is_identity};
use crate::style::Style;

use super::bbox::is_rectangle;
use super::style::{compose_style, fix_css_clipmask};
use super::{ClipKind, Ctx, MAX_NEST, clip_ref, label};

pub const TEXT_TAGS: &[&str] = &["text", "flowRoot"];
/// Children `ungroup` leaves inside the group (DH:285–288).
pub const UNUNGROUPABLE: &[&str] = &["namedview", "defs", "metadata", "foreignObject"];

fn element_children(doc: &Doc, n: NodeId) -> Vec<NodeId> {
    doc.children(n).filter(|&k| doc.is_element(k)).collect()
}

/// DH:359–391. Style first (a CSS clip may depend on it), then clip and mask, then the
/// transform is prepended to the element's own. Returns whether the given `clip` clipped the
/// element out entirely (`false` whenever `clip` is `None`).
#[allow(clippy::too_many_arguments)]
pub fn compose_all(
    doc: &mut Doc,
    ctx: &mut Ctx,
    el: NodeId,
    clip: Option<NodeId>,
    mask: Option<NodeId>,
    t: Affine,
    style: Option<&Style>,
    remove_text_clip: bool,
) -> bool {
    if let Some(st) = style {
        compose_style(doc, el, st);
    }
    let mut cout = false;
    if remove_text_clip && TEXT_TAGS.contains(&doc.tag(el)) {
        doc.remove_attr(el, "clip-path");
        doc.remove_attr(el, "mask");
    } else {
        if let Some(c) = clip {
            cout = merge_clipmask(doc, ctx, el, c, ClipKind::Clip, 0);
        }
        if let Some(m) = mask {
            merge_clipmask(doc, ctx, el, m, ClipKind::Mask, 0);
        }
        if clip.is_some() {
            fix_css_clipmask(doc, el, ClipKind::Clip);
        }
        if mask.is_some() {
            fix_css_clipmask(doc, el, ClipKind::Mask);
        }
    }
    if !is_identity(t) {
        let own = doc.transform(el);
        doc.set_transform(el, t * own);
    }
    clip.is_some() && cout
}

/// A copy of `n` appended to the root `<defs>` with a fresh id, recorded in `ctx.created`.
pub fn duplicate_into_defs(doc: &mut Doc, ctx: &mut Ctx, n: NodeId) -> NodeId {
    let d = doc.deep_clone(n);
    let defs = doc.defs();
    doc.append_child(defs, d);
    doc.ensure_id(d);
    ctx.created.push(d);
    d
}

/// Axis-aligned box of a shape's end points after its own transform.
fn end_point_box(doc: &Doc, n: NodeId) -> Option<Rect> {
    let pp = shape_path(doc, n)?;
    let t = doc.transform(n);
    let mut r: Option<Rect> = None;
    for p in end_points(&pp.path) {
        let q = t * p;
        r = Some(r.map_or(Rect::from_points(q, q), |r| r.union_pt(q)));
    }
    r
}

/// DH:416–443 `intersect_paths` + `compose_clips`: replace the rectangular clip child `k` by the
/// intersection of its box with the new rectangular clip's box (a path appended to `d`), or
/// delete it when the boxes do not overlap. Returns `true` when clipped out.
fn compose_clips(doc: &mut Doc, d: NodeId, k: NodeId, newrect: NodeId) -> bool {
    let inter = match (end_point_box(doc, newrect), end_point_box(doc, k)) {
        (Some(a), Some(b)) => Rect::new(
            a.x0.max(b.x0),
            a.y0.max(b.y0),
            a.x1.min(b.x1),
            a.y1.min(b.y1),
        ),
        _ => Rect::ZERO,
    };
    let keep = inter.width() > 0.0 && inter.height() > 0.0;
    if keep {
        let p = doc.new_element("path");
        doc.set_attr(p, "d", fmt_d(&inter.to_path(0.1)));
        doc.append_child(d, p);
    }
    doc.detach(k);
    !keep
}

/// DH:445–504 (from Deep Ungroup): apply `newclip` to `node` on top of any clip/mask it already
/// carries. Returns `true` when the composition proves the node fully clipped out.
///
/// 1. A transformed node: clips live in the node's user space, so a copy of the new clip gets
///    the inverse transform on its children (recorded for gc).
/// 2. `<use>` children of the new clip are unlinked in place.
/// 3. No existing clip → point at the new one. Otherwise duplicate the existing clip, point the
///    node at the duplicate and compose child by child: rectangle ∩ rectangle becomes one box
///    (clips only, never masks), anything else is clipped recursively.
pub fn merge_clipmask(
    doc: &mut Doc,
    ctx: &mut Ctx,
    node: NodeId,
    newclip: NodeId,
    kind: ClipKind,
    depth: usize,
) -> bool {
    if depth > MAX_NEST {
        ctx.warn.push(format!(
            "{}: clips nested deeper than {MAX_NEST} levels are not merged",
            label(doc, node)
        ));
        return false;
    }
    let mut newclip = newclip;
    let own = doc.transform(node);
    if !is_identity(own) {
        match inverse(own) {
            Some(inv) => {
                let d = duplicate_into_defs(doc, ctx, newclip);
                for k in element_children(doc, d) {
                    compose_all(doc, ctx, k, None, None, inv, None, false);
                }
                newclip = d;
            }
            None => ctx.warn.push(format!(
                "{}: singular transform, its clip is applied untransformed",
                label(doc, node)
            )),
        }
    }
    for u in element_children(doc, newclip) {
        if doc.tag(u) == "use" {
            unlink(doc, ctx, u);
        }
    }
    let Some(old) = clip_ref(doc, node, kind) else {
        let id = doc.ensure_id(newclip);
        doc.set_attr(node, kind.attr(), format!("url(#{id})"));
        return false;
    };
    for u in element_children(doc, old) {
        if doc.tag(u) == "use" {
            unlink(doc, ctx, u);
        }
    }
    let d = duplicate_into_defs(doc, ctx, old);
    let id = doc.ensure_id(d);
    doc.set_attr(node, kind.attr(), format!("url(#{id})"));
    let new_kids = element_children(doc, newclip);
    let newclip_is_rect = new_kids.len() == 1 && is_rectangle(doc, new_kids[0], true);
    let mut all_out = true; // `all([])` is true
    for k in element_children(doc, d).into_iter().rev() {
        let cout = if newclip_is_rect && kind == ClipKind::Clip && is_rectangle(doc, k, true) {
            compose_clips(doc, d, k, new_kids[0])
        } else {
            merge_clipmask(doc, ctx, k, newclip, kind, depth + 1)
        };
        all_out &= cout;
    }
    all_out
}

/// DH:238–282 `unlink2`: replace a `<use>` by a copy of its target that takes the clone's place
/// and id, composed with `translate(x, y)` first, then the clone's clip, mask, transform and
/// cascaded style; nested clones inside the copy are unlinked too. A `<symbol>` copy becomes a
/// `<g>` (Inkscape's Unlink Clone). Returns the replacement, or `None` for a clone of a missing
/// target (the clone is then deleted). Non-`<use>` elements are returned unchanged.
pub fn unlink(doc: &mut Doc, ctx: &mut Ctx, u: NodeId) -> Option<NodeId> {
    if doc.tag(u) != "use" {
        return Some(u);
    }
    let mut result: Option<NodeId> = None;
    let mut work = vec![u];
    let mut steps = 0usize;
    while let Some(u) = work.pop() {
        steps += 1;
        if steps > 10_000 {
            ctx.warn.push(
                "clone chain too long (a symbol cloning itself?), unlinking stopped".to_string(),
            );
            break;
        }
        let Some(target) = doc.resolve_href(u) else {
            doc.detach(u);
            continue;
        };
        let d = doc.deep_clone(target);
        doc.insert_after(d, u);
        let x = doc.attr(u, "x").and_then(crate::geom::ipx).unwrap_or(0.0);
        let y = doc.attr(u, "y").and_then(crate::geom::ipx).unwrap_or(0.0);
        compose_all(
            doc,
            ctx,
            d,
            None,
            None,
            Affine::translate((x, y)),
            None,
            false,
        );
        let clip = clip_ref(doc, u, ClipKind::Clip);
        let mask = clip_ref(doc, u, ClipKind::Mask);
        let st = doc.cascaded_style(u);
        let t = doc.transform(u);
        compose_all(doc, ctx, d, clip, mask, t, Some(&st), false);
        let id = doc.attr(u, "id").map(str::to_string);
        doc.detach(u);
        if let Some(id) = id {
            doc.set_attr(d, "id", id);
        }
        doc.set_attr(d, "unlinked_clone", "True");
        let mut d = d;
        if doc.tag(d) == "symbol" {
            let g = group(doc, &element_children(doc, d));
            ungroup(doc, ctx, d, false);
            d = g;
        }
        // nested clones inside the copy (the copy itself is never a <use>: its target was not)
        let nested: Vec<NodeId> = doc
            .descendants(d)
            .filter(|&k| k != d && doc.is_element(k) && doc.tag(k) == "use")
            .collect();
        work.extend(nested);
        if result.is_none() {
            result = Some(d);
        }
    }
    result
}

/// DH:320–338: wraps `els` in a new `<g>` placed right after the first element; an empty list
/// returns a detached group.
pub fn group(doc: &mut Doc, els: &[NodeId]) -> NodeId {
    let g = doc.new_element("g");
    if let Some(&first) = els.first() {
        doc.insert_after(g, first);
        for &e in els {
            doc.append_child(g, e);
        }
    }
    g
}

/// DH:292–316: dissolves `g`, composing its transform, clip, mask and cascaded style onto every
/// element child (`<defs>`/metadata/namedview/foreignObject stay inside; comments are removed;
/// text nodes are left where they are). Children the clip proves clipped out are deleted; the
/// rest follow the group in document order. The group is deleted once it has no element or
/// comment children left.
pub fn ungroup(doc: &mut Doc, ctx: &mut Ctx, g: NodeId, remove_text_clip: bool) {
    let gt = doc.transform(g);
    let gclip = clip_ref(doc, g, ClipKind::Clip);
    let gmask = clip_ref(doc, g, ClipKind::Mask);
    let gstyle = doc.cascaded_style(g);
    let gspace = doc.attr(g, "xml:space").map(str::to_string);
    let kids: Vec<NodeId> = doc.children(g).collect();
    for k in kids.into_iter().rev() {
        if doc.is_comment(k) {
            doc.detach(k);
            continue;
        }
        if !doc.is_element(k) || UNUNGROUPABLE.contains(&doc.tag(k)) {
            continue;
        }
        let cout = compose_all(
            doc,
            ctx,
            k,
            gclip,
            gmask,
            gt,
            Some(&gstyle),
            remove_text_clip,
        );
        if let Some(sp) = &gspace {
            if doc.attr(k, "xml:space").is_none() {
                doc.set_attr(k, "xml:space", sp.clone());
            }
        }
        if cout {
            doc.detach(k);
        } else {
            doc.insert_after(k, g);
        }
    }
    if !doc
        .children(g)
        .any(|c| doc.is_element(c) || doc.is_comment(c))
    {
        doc.detach(g);
    }
}

/// `systemLanguage` matching. **Deviation:** any comma-separated token equal to `lang`
/// (case-insensitive) or sharing its primary subtag (`en-US` ~ `en`) matches; upstream compares
/// the raw strings.
pub fn lang_matches(attr: &str, lang: &str) -> bool {
    let primary = |s: &str| {
        s.trim()
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let want = lang.trim().to_ascii_lowercase();
    if want.is_empty() {
        return false;
    }
    attr.split(',').any(|tok| {
        let tok = tok.trim();
        !tok.is_empty() && (tok.eq_ignore_ascii_case(&want) || primary(tok) == primary(&want))
    })
}

/// The `language` of `<group id="ui">` in Inkscape's `preferences.xml` (U:396–443), if set.
pub fn preferences_language(xml: &str) -> Option<String> {
    for tag in xml.split('<').skip(1) {
        let tag = tag.split('>').next()?;
        if !tag.trim_start().starts_with("group") || !tag.contains(r#"id="ui""#) {
            continue;
        }
        let rest = tag.split("language=\"").nth(1)?;
        let lang = rest.split('"').next()?.trim();
        return (!lang.is_empty()).then(|| lang.to_string());
    }
    None
}

/// Inkscape's UI language: `preferences.xml` under `INKSCAPE_PROFILE_DIR` (exported by Inkscape
/// to its extensions), else the locale variables' primary subtag, else `en`.
pub fn ui_language() -> String {
    if let Some(dir) = std::env::var_os("INKSCAPE_PROFILE_DIR") {
        let p = std::path::Path::new(&dir).join("preferences.xml");
        if let Some(l) = std::fs::read_to_string(p)
            .ok()
            .and_then(|s| preferences_language(&s))
        {
            return l;
        }
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            let primary = v.split(['_', '.', '-', '@']).next().unwrap_or("").trim();
            if !primary.is_empty() && primary != "C" && primary != "POSIX" {
                return primary.to_string();
            }
        }
    }
    "en".to_string()
}

/// DH:341–352: keep the `<switch>` children whose `systemLanguage` matches `lang` (children
/// without the attribute always match), never deleting the last one; then keep only the first,
/// strip its `systemLanguage`, and ungroup the switch.
pub fn deswitch(doc: &mut Doc, ctx: &mut Ctx, sw: NodeId, lang: &str) {
    let kids = element_children(doc, sw);
    let mut remaining = kids.len();
    for &k in kids.iter().rev() {
        let matches = doc
            .attr(k, "systemLanguage")
            .is_none_or(|v| lang_matches(v, lang));
        if !matches && remaining > 1 {
            doc.detach(k);
            remaining -= 1;
        }
    }
    let kids = element_children(doc, sw);
    for &k in kids.iter().skip(1) {
        doc.detach(k);
    }
    if let Some(&first) = kids.first() {
        doc.remove_attr(first, "systemLanguage");
    }
    ungroup(doc, ctx, sw, false);
}
