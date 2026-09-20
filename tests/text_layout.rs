mod support;

use std::path::PathBuf;

use kurbo::{Affine, Point};
use sciink::dom::{Doc, NodeId};
use sciink::text::Warnings;
use sciink::text::fonts::FontSystem;
use sciink::text::layout::{
    char_extents, char_pts_ut, chunk_extents, chunk_geom, full_extent, full_ink_bbox, line_extents,
    max_tfs, text_bbox, unrendered_space,
};
use sciink::text::parse::ParsedText;
use sciink::text::table::CharTable;

fn fonts() -> FontSystem {
    FontSystem::from_dirs(&[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")])
}
fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap()
}
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";
const DV: &str = "font-family:'DejaVu Sans'";

fn parsed(svg: &str, el: &str) -> (ParsedText, CharTable) {
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let mut w = Warnings::default();
    let n = id(&d, el);
    let mut ct = CharTable::build(&d, &[n], fonts(), &mut w);
    let pt = ParsedText::parse(&mut d, n, &mut ct, &mut w).expect("parsed");
    (pt, ct)
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn chunk_geometry_start_anchor_with_dx_letter_spacing_and_kerning() {
    let (pt, _) = parsed(
        &format!(
            r#"<svg {NS}><text id="t" style="{DV};font-size:10px;letter-spacing:1px" x="5" y="20" dx="0 2">AVx</text></svg>"#
        ),
        "t",
    );
    let g = chunk_geom(&pt, 0, 0);
    let [a, v, x] = [&pt.chars[0], &pt.chars[1], &pt.chars[2]];
    let kern_av = v.prop.dadvs[&'A'] * 10.0;
    assert!(kern_av < 0.0, "A–V kerns");
    // char 0: left = x + dx0 + dxlsp0(0) = 5 ; right = left + cwd
    assert!(close(g.left[0], 5.0));
    assert!(close(g.right[0], 5.0 + a.cwd));
    // char 1: dx=2 overrides the pair kerning; letter-spacing 1 added before it
    assert!(close(g.left[1], g.right[0] + 2.0 + 1.0));
    assert!(close(g.right[1], g.left[1] + v.cwd));
    // char 2: no dx → kerning applies (V–x, whatever it is) plus letter-spacing
    let kern_vx = x.prop.dadvs.get(&'V').copied().unwrap_or(0.0) * 10.0;
    assert!(close(g.left[2], g.right[1] + 1.0 + kern_vx));
    assert!(g.base.iter().all(|&b| close(b, 20.0)));
    assert!(g.top.iter().all(|&t| close(t, 20.0 - a.caph)));
    // chunk box: lx2 = min(left) − dx[0] − dxlsp[0] = 5
    assert!(close(g.pts_ut[0].x, 5.0) && close(g.pts_ut[0].y, 20.0));
    assert!(close(g.pts_ut[2].x, 5.0 + (g.right[2] - g.left[0])));
    assert!(close(g.pts_ut[1].y, 20.0 - a.caph));
    let p = char_pts_ut(&pt, &g, 1);
    assert_eq!(p[0], Point::new(g.left[1], g.base[1]));
    assert_eq!(p[2], Point::new(g.right[1], g.top[1]));
    assert!(!unrendered_space(&pt, 0, 0));
}

#[test]
fn middle_and_end_anchors_and_trailing_space() {
    let (pt, _) = parsed(
        &format!(
            r#"<svg {NS}><text id="t" xml:space="preserve" style="{DV};font-size:10px;text-anchor:middle" x="0" y="0">ab </text></svg>"#
        ),
        "t",
    );
    assert!(
        unrendered_space(&pt, 0, 0),
        "trailing space of the line's last chunk is not rendered"
    );
    let g = chunk_geom(&pt, 0, 0);
    // middle anchor centres the *rendered* width: the chunk width minus the unrendered trailing space
    let rendered = (g.right[2] - g.left[0]) - pt.chars[2].cwd;
    assert!(
        close(g.left[0], -rendered / 2.0),
        "{} vs {}",
        g.left[0],
        -rendered / 2.0
    );
    assert!(close(g.right[2] - pt.chars[2].cwd, rendered / 2.0));
    let (pt2, _) = parsed(
        &format!(
            r#"<svg {NS}><text id="t" style="{DV};font-size:10px;text-anchor:end" x="10" y="0">ab</text></svg>"#
        ),
        "t",
    );
    let g2 = chunk_geom(&pt2, 0, 0);
    assert!(close(g2.right[1], 10.0));
}

#[test]
fn dy_baseline_shift_and_multi_chunk_lines() {
    // x="0 30" on a two-character run → two chunks; the positioned tspan opens a second line
    let (pt, _) = parsed(
        &format!(
            r#"<svg {NS}><text id="t" style="{DV};font-size:10px" x="0 30" y="0" dy="0 5">ab<tspan id="s" x="60" style="font-size:50%;baseline-shift:super">c</tspan></text></svg>"#
        ),
        "t",
    );
    assert_eq!(pt.lines.len(), 2);
    assert_eq!(pt.lines[0].chunks.len(), 2);
    let g0 = chunk_geom(&pt, 0, 0);
    let g1 = chunk_geom(&pt, 0, 1);
    assert!(close(g0.base[0], 0.0));
    // chunk 1 starts at x=30; its char has dy=5
    assert!(close(g1.pts_ut[0].x, 30.0));
    assert!(close(g1.base[0], 5.0));
    // line 2: x=60, y continues (0); baseline-shift super = +40% of the parent's 10px → base −4
    let g2 = chunk_geom(&pt, 1, 0);
    assert!(pt.lines[1].spec.continue_y);
    assert!(close(g2.pts_ut[0].x, 60.0));
    assert!(close(g2.base[0], -4.0));
    assert!(close(g2.top[0], -4.0 - pt.chars[2].caph));
    assert!(close(pt.chars[2].caph, 0.729 * 5.0) || (pt.chars[2].caph - 3.645).abs() < 0.03);
    assert_eq!(chunk_extents(&pt).len(), 3);
    assert_eq!(line_extents(&pt).len(), 2);
    // char_extents carries each rect's index into pt.chars, in index order, so a consumer
    // keying anything off the character cannot drift if a NaN-baseline char is dropped
    let exts = char_extents(&pt);
    assert_eq!(exts.len(), 3);
    assert_eq!(
        exts.iter().map(|&(i, _)| i).collect::<Vec<_>>(),
        [0, 1, 2],
        "indices into pt.chars, ascending"
    );
    // char 2 lives on the second line, whose chunk starts at x=60
    assert!(close(exts[2].1.x0, g2.left[0]));
}

#[test]
fn extents_ink_and_transformed_bbox() {
    let (pt, _) = parsed(
        &format!(
            r#"<svg {NS}><g transform="translate(100,50) scale(2)"><text id="t" style="{DV};font-size:10px" x="0" y="0">I</text></g></svg>"#
        ),
        "t",
    );
    let ext = full_extent(&pt).unwrap();
    let c = &pt.chars[0];
    assert!(close(ext.x0, 0.0) && close(ext.x1, c.cwd));
    assert!(close(ext.y1, 0.0) && close(ext.y0, -c.caph));
    let ink = full_ink_bbox(&pt).unwrap();
    assert!(
        ink.x0 > 0.0 && ink.x1 < c.cwd,
        "I's ink is narrower than its advance: {ink:?}"
    );
    assert!(
        close(ink.y1, 0.0) && (ink.y0 + c.caph).abs() < 0.05,
        "I spans baseline to cap height: {ink:?}"
    );
    // transformed: translate(100,50) scale(2)
    let bb = text_bbox(&pt).unwrap();
    assert!(close(bb.x0, 100.0) && close(bb.x1, 100.0 + 2.0 * c.cwd));
    assert!(close(bb.y1, 50.0) && close(bb.y0, 50.0 - 2.0 * c.caph));
    assert_eq!(max_tfs(&pt), Some(20.0));
    assert_eq!(
        pt.transform,
        Affine::translate((100.0, 50.0)) * Affine::scale(2.0)
    );
}

#[test]
fn fixture_text_elements_all_have_finite_bboxes() {
    let Some(dir) = support::upstream_data_dir() else {
        return;
    };
    let src = std::fs::read(dir.join("svg/Text_tests.svg")).unwrap();
    let mut d = Doc::parse(&src).unwrap();
    let texts: Vec<NodeId> = d
        .descendants(d.svg())
        .filter(|&n| d.is_element(n) && d.tag(n) == "text")
        .collect();
    assert!(texts.len() > 150, "{}", texts.len());
    let mut w = Warnings::default();
    let mut ct = CharTable::build(&d, &texts, fonts(), &mut w);
    let (mut parsed_n, mut flows) = (0, 0);
    for &t in &texts {
        match ParsedText::parse(&mut d, t, &mut ct, &mut w) {
            Some(pt) if pt.is_flow => flows += 1,
            Some(pt) => {
                parsed_n += 1;
                let bb = text_bbox(&pt).expect("bbox");
                assert!(
                    bb.x0.is_finite() && bb.y0.is_finite() && bb.x1 >= bb.x0 && bb.y1 >= bb.y0,
                    "{bb:?}"
                );
                assert_eq!(char_extents(&pt).len(), pt.chars.len());
            }
            None => {}
        }
    }
    assert!(
        parsed_n > 120 && flows >= 2,
        "parsed {parsed_n}, flows {flows}"
    );
}
