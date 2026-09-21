//! Stage 12 — the only place the text pipeline writes the DOM (spec §A.0 decision 1):
//! `write_clean_text` regenerates one element from its model (P:2385–2444 + P:3456–3503, and
//! P:1189–1256 for split-offs); `apply_clip_unions` performs the clip-path unions merges asked for
//! (RK:640–656).

use std::collections::HashMap;

use crate::dom::{Doc, NodeId};
use crate::geom::{fmt_transform, inverse, is_identity};
use crate::num;
use crate::style::{Style, default_value};

use super::layout::chunk_utfs;
use super::parse::{Origin, ParsedText, TChar, XY_TOL};
use super::style::Anchor;
use super::table::CharTable;

/// Elements whose `clip-path`s must be unioned onto `target` because their text was merged into it
/// (RK:640–656). Executed by `apply_clip_unions` before the elements are rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipUnion {
    pub target: NodeId,
    pub others: Vec<NodeId>,
}

/// The position after a model's most recently written element or descendant: where its next
/// split-off goes. An emptied source records the node that preceded it, or first-in-parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    After(NodeId),
    FirstIn(NodeId),
}

fn align_of(a: Anchor) -> &'static str {
    match a {
        Anchor::Start => "start",
        Anchor::Middle => "center",
        Anchor::End => "end",
    }
}

/// C:206–221 (`set_cspecified_style`): the local declarations that make a fresh child's specified
/// style equal `desired` when it inherits `inherited` — every declaration of `desired` that differs
/// or is missing, plus the initial value of every inherited property `desired` does not mention
/// (skipped when no initial value is known).
pub fn specified_diff(desired: &Style, inherited: &Style) -> Style {
    let mut out = Style::default();
    for (k, v) in &desired.0 {
        if inherited.get(k) != Some(v.as_str()) {
            out.set(k, v);
        }
    }
    for (k, _) in &inherited.0 {
        if desired.get(k).is_none() {
            if let Some(d) = default_value(k) {
                out.set(k, d);
            }
        }
    }
    out
}

/// `dx`/`dy` lists: trailing zeros trimmed, attribute omitted when nothing is left (P:3462–3463).
fn set_list(doc: &mut Doc, n: NodeId, name: &str, vals: impl Iterator<Item = f64>) {
    let mut v: Vec<f64> = vals.collect();
    while v.last() == Some(&0.0) {
        v.pop();
    }
    if v.is_empty() {
        doc.remove_attr(n, name);
    } else {
        let s: Vec<String> = v.iter().map(|x| num::fmt(*x)).collect();
        doc.set_attr(n, name, s.join(" "));
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

const SODIPODI_NS: &str = "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd";

/// `sodipodi:role` needs its namespace declared: matplotlib/svglite documents do not carry it
/// (inkex declares it implicitly through lxml's nsmap).
fn ensure_sodipodi_ns(doc: &mut Doc) {
    let svg = doc.svg();
    if doc.attr(svg, "xmlns:sodipodi").is_none() {
        doc.set_attr(svg, "xmlns:sodipodi", SODIPODI_NS);
    }
}

/// P:3456–3503 (`make_tspan`): one `<tspan>` for a chunk, with nested tspans per style run.
fn make_tspan(doc: &mut Doc, pt: &ParsedText, li: usize, ci: usize, te: NodeId) {
    let ln = &pt.lines[li];
    let ch = &ln.chunks[ci];
    let ts = doc.new_element("tspan");
    doc.append_child(te, ts);
    doc.set_attr(ts, "x", num::fmt(ch.x));
    doc.set_attr(ts, "y", num::fmt(ch.y));
    let cs: Vec<&TChar> = ch.chars.iter().map(|&c| &pt.chars[c]).collect();
    set_list(doc, ts, "dx", cs.iter().map(|c| c.dx));
    set_list(doc, ts, "dy", cs.iter().map(|c| c.dy));
    let utfs = chunk_utfs(pt, li, ci);
    let mut bounds = vec![0usize];
    for i in 1..cs.len() {
        if !super::edit::style_eq(&cs[i].sty, &cs[i - 1].sty)
            || cs[i].utfs != cs[i - 1].utfs
            || cs[i].bshft != cs[i - 1].bshft
        {
            bounds.push(i);
        }
    }
    bounds.push(cs.len());
    let nested = bounds.len() > 2 || cs[0].bshft.abs() > XY_TOL;
    let chars: Vec<char> = cs.iter().map(|c| c.c).collect();
    let inherited = doc.specified_style(ts);
    let mut st = specified_diff(&cs[0].sty, &inherited);
    st.set("font-size", &num::fmt(utfs));
    st.set("text-align", align_of(ln.spec.anchor));
    st.set("text-anchor", ln.spec.anchor.css());
    for k in ["line-height", "direction", "baseline-shift", "shape-inside"] {
        st.remove(k);
    }
    doc.set_style_map(ts, &st);
    if !nested {
        let t = doc.new_text(&chars.iter().collect::<String>());
        doc.append_child(ts, t);
        return;
    }
    let ts_spec = doc.specified_style(ts);
    for w in bounds.windows(2) {
        let tsn = doc.new_element("tspan");
        doc.append_child(ts, tsn);
        let t = doc.new_text(&chars[w[0]..w[1]].iter().collect::<String>());
        doc.append_child(tsn, t);
        let c = cs[w[0]];
        let mut ns = specified_diff(&c.sty, &ts_spec);
        if (c.utfs - utfs).abs() > 0.001 {
            ns.set(
                "font-size",
                &format!("{}%", num::fmt(round3(c.utfs / utfs * 100.0))),
            );
        } else {
            ns.remove("font-size");
        }
        if c.bshft.abs() > 0.001 {
            let r = c.bshft / utfs;
            let v = if (r - 0.4).abs() < 0.001 {
                "super".to_string()
            } else if (r + 0.2).abs() < 0.001 {
                "sub".to_string()
            } else {
                format!("{}%", num::fmt(round3(r * 100.0)))
            };
            ns.set("baseline-shift", &v);
        } else {
            ns.remove("baseline-shift");
        }
        ns.0.retain(|(k, v)| ts_spec.get(k) != Some(v.as_str()));
        for k in ["text-align", "text-anchor", "direction", "shape-inside"] {
            ns.remove(k);
        }
        doc.set_style_map(tsn, &ns);
    }
}

/// P:2385–2444 (`make_clean_textelement`), P:1189–1256 for split-offs. Returns the new element,
/// or `None` when the model has no characters (an existing element is then removed).
pub fn write_clean_text(
    doc: &mut Doc,
    pts: &[ParsedText],
    idx: usize,
    ct: &CharTable,
    slots: &mut HashMap<usize, Slot>,
) -> Option<NodeId> {
    let pt = &pts[idx];
    let old = pt.el;
    if pt.chars.is_empty() {
        if pt.origin == Origin::Existing {
            if let Some(parent) = doc.parent(old) {
                let slot = match doc.prev_sibling(old) {
                    Some(p) => Slot::After(p),
                    None => Slot::FirstIn(parent),
                };
                slots.insert(idx, slot);
                doc.detach(old);
            }
        }
        return None;
    }
    let slot = match pt.origin {
        Origin::Existing => Slot::After(old),
        Origin::SplitFrom => {
            // the nearest ancestor that recorded a slot (an emptied split-off records none)
            let mut a = pt.split_src;
            let mut found = None;
            while let Some(s) = a {
                if let Some(&sl) = slots.get(&s) {
                    found = Some(sl);
                    break;
                }
                a = pts[s].split_src;
            }
            found.unwrap_or(Slot::After(old))
        }
    };
    let te = doc.new_element("text");
    // upstream's five exclusions plus the per-character lists (see the task description)
    const SKIP: [&str; 10] = [
        "baseline-shift",
        "shape-inside",
        "direction",
        "style",
        "font-family",
        "x",
        "y",
        "dx",
        "dy",
        "rotate",
    ];
    for a in doc.attrs(old).to_vec() {
        if SKIP.contains(&a.name.as_str()) || a.name == "id" {
            continue;
        }
        if pt.text_length_removed && matches!(a.name.as_str(), "textLength" | "lengthAdjust") {
            continue;
        }
        doc.set_attr(te, &a.name, a.value);
    }
    let mut style = doc.attr(old, "style").map(Style::parse).unwrap_or_default();
    for k in ["baseline-shift", "shape-inside", "direction"] {
        style.remove(k);
    }
    let first = &pt.chars[0];
    let fam = first
        .face
        .map(|f| ct.fonts.face_info(f).family.clone())
        .or_else(|| first.spec.families.first().cloned())
        .unwrap_or_else(|| "sans-serif".to_string());
    style.set("font-family", &format!("'{fam}'"));
    if let Some(a) = pt.text_anchor_override {
        style.set("text-anchor", a.css());
        style.set("text-align", align_of(a));
    }
    if !is_identity(pt.transform_extra) {
        match fmt_transform(doc.transform(old) * pt.transform_extra) {
            Some(t) => doc.set_attr(te, "transform", t),
            None => {
                doc.remove_attr(te, "transform");
            }
        }
    }
    doc.set_attr(te, "xml:space", "preserve");
    match slot {
        Slot::After(a) => doc.insert_after(te, a),
        Slot::FirstIn(p) => doc.prepend_child(p, te),
    }
    match pt.origin {
        Origin::Existing => {
            let id = doc.attr(old, "id").map(str::to_string);
            doc.detach(old);
            match id {
                Some(id) => doc.set_attr(te, "id", id),
                None => {
                    doc.ensure_id(te);
                }
            }
            slots.insert(idx, Slot::After(te));
        }
        Origin::SplitFrom => {
            doc.ensure_id(te);
            slots.insert(idx, Slot::After(te));
            // pre-order: every ancestor whose subtree ended exactly where this element went now
            // ends here (an emptied split-off recorded nothing — keep climbing past it)
            let mut a = pt.split_src;
            while let Some(s) = a {
                match slots.get(&s) {
                    Some(sl) if *sl == slot => {
                        slots.insert(s, Slot::After(te));
                    }
                    Some(_) => break,
                    None => {}
                }
                a = pts[s].split_src;
            }
        }
    }
    doc.set_style_map(te, &style);

    let (mut xs, mut ys, mut fszs) = (Vec::new(), Vec::new(), Vec::new());
    for (li, ci) in pt.chunks() {
        let ch = pt.chunk(li, ci);
        if ch.y.is_nan() {
            continue;
        }
        xs.push(ch.x);
        ys.push(ch.y);
        fszs.push(chunk_utfs(pt, li, ci));
        make_tspan(doc, pt, li, ci, te);
    }
    // sodipodi:role="line" when Inkscape's own re-layout would reproduce these positions (P:2427–2437)
    if !xs.is_empty() {
        let tefsz = fszs.iter().copied().fold(f64::INFINITY, f64::min);
        let step = |i: usize| (ys[i + 1] - ys[i]) / fszs[i + 1].max(tefsz);
        let same_x = xs.iter().all(|x| (x - xs[0]).abs() < 0.001);
        let same_step = (0..ys.len().saturating_sub(1)).all(|i| (step(i) - step(0)).abs() < 0.001);
        if same_x && same_step {
            let lh = if ys.len() > 1 { step(0) } else { 1.25 };
            ensure_sodipodi_ns(doc);
            doc.set_style(te, "font-size", &num::fmt(tefsz));
            doc.set_style(te, "line-height", &num::fmt(lh));
            doc.set_attr(te, "x", num::fmt(xs[0]));
            doc.set_attr(te, "y", num::fmt(ys[0]));
            let kids: Vec<NodeId> = doc.children(te).filter(|&k| doc.is_element(k)).collect();
            for k in kids {
                doc.set_attr(k, "sodipodi:role", "line");
            }
        }
    }
    Some(te)
}

/// The element a `clip-path` (attribute, else style) points at; `None` for no or a dangling reference.
/// Shared with `kerning::perform_merges` (Task 6/7 ruling) — stays `pub`.
pub fn clip_of(doc: &Doc, el: NodeId) -> Option<NodeId> {
    let v = doc
        .attr(el, "clip-path")
        .map(str::to_string)
        .or_else(|| doc.specified(el, "clip-path"))?;
    let id = v.trim().strip_prefix("url(#")?.strip_suffix(')')?.trim();
    doc.by_id(id)
}

/// RK:640–656: when merged elements carry different clips, the target gets a duplicate of its own
/// clip with every other participant's clip contents appended as a `<g>` transformed by
/// `(target ctm)⁻¹ · (other ctm)`; if any participant is unclipped, the target's clip is dropped.
pub fn apply_clip_unions(doc: &mut Doc, unions: &[ClipUnion]) {
    for u in unions {
        let mut els = vec![u.target];
        for &o in &u.others {
            if !els.contains(&o) {
                els.push(o);
            }
        }
        if els.len() < 2 {
            continue;
        }
        let clips: Vec<Option<NodeId>> = els.iter().map(|&e| clip_of(doc, e)).collect();
        let Some(all): Option<Vec<NodeId>> = clips.into_iter().collect() else {
            doc.remove_attr(u.target, "clip-path");
            doc.remove_style(u.target, "clip-path");
            continue;
        };
        let Some(inv) = inverse(doc.composed_transform(u.target)) else {
            continue;
        };
        let dc = doc.deep_clone(all[0]);
        doc.insert_after(dc, all[0]);
        let dcid = doc.ensure_id(dc);
        for (i, &e) in els.iter().enumerate().skip(1) {
            let ng = doc.new_element("g");
            let kids: Vec<NodeId> = doc.children(all[i]).collect();
            for k in kids {
                let kc = doc.deep_clone(k);
                doc.append_child(ng, kc);
            }
            if let Some(t) = fmt_transform(inv * doc.composed_transform(e)) {
                doc.set_attr(ng, "transform", t);
            }
            doc.append_child(dc, ng);
        }
        doc.set_attr(u.target, "clip-path", format!("url(#{dcid})"));
        doc.remove_style(u.target, "clip-path");
    }
}
