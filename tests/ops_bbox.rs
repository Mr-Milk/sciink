mod support;

use kurbo::Rect;
use sciink::dom::{Doc, NodeId};
use sciink::ops::Ctx;

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}

fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i)
        .unwrap_or_else(|| panic!("no element with id {i}"))
}

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn rect_close(r: Rect, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    rect_near(r, x0, y0, x1, y1, 1e-9)
}

use sciink::ops::bbox::{BboxOpts, LOCAL, VISUAL, bb2, bbox, has_bbox, is_drawn, is_rectangle};
use support::with_vendored_fonts;

fn rect_near(r: Rect, x0: f64, y0: f64, x1: f64, y1: f64, tol: f64) -> bool {
    (r.x0 - x0).abs() < tol
        && (r.y0 - y0).abs() < tol
        && (r.x1 - x1).abs() < tol
        && (r.y1 - y1).abs() < tol
}

#[test]
fn has_bbox_and_is_drawn_follow_the_unrendered_and_container_sets() {
    let d = doc(&format!(
        r#"<svg {NS}><defs><path id="in_defs"/></defs><g id="g"><path id="p"/><path id="hidden" style="display:none"/><text id="t"><tspan id="ts">x</tspan></text></g><sodipodi:namedview id="nv"/></svg>"#
    ));
    assert!(has_bbox(&d, id(&d, "p")) && has_bbox(&d, id(&d, "g")) && has_bbox(&d, d.svg()));
    assert!(
        !has_bbox(&d, id(&d, "in_defs"))
            && !has_bbox(&d, id(&d, "ts"))
            && !has_bbox(&d, id(&d, "nv"))
    );
    assert!(is_drawn(&d, id(&d, "p")) && is_drawn(&d, id(&d, "t")));
    assert!(
        !is_drawn(&d, id(&d, "g")),
        "containers are not drawn themselves"
    );
    assert!(!is_drawn(&d, id(&d, "hidden")) && !is_drawn(&d, id(&d, "in_defs")));
}

#[test]
fn shape_boxes_with_stroke_transform_and_rough_mode() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g transform="translate(10,20)"><path id="p" d="M0 0 L10 0 L10 5 Z" style="stroke:#000;stroke-width:2"/><path id="c" d="M0 0 C 0 10 10 10 10 0"/><path id="pct" d="M0 0 L10 0 L10 5 Z" style="stroke:#000;stroke-width:10%"/><path id="defaultw" d="M0 0 L10 0 L10 5 Z" style="stroke:#000"/><path id="nostroke" d="M0 0 L10 0 L10 5 Z" style="stroke:none;stroke-width:2"/><path id="empty" d=""/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, p, LOCAL).unwrap(),
        -1.0,
        -1.0,
        11.0,
        6.0
    ));
    assert!(rect_close(
        bbox(&mut d, &mut ctx, p, VISUAL).unwrap(),
        9.0,
        19.0,
        21.0,
        26.0
    ));
    let no_stroke = BboxOpts {
        stroke: false,
        ..LOCAL
    };
    assert!(rect_close(
        bbox(&mut d, &mut ctx, p, no_stroke).unwrap(),
        0.0,
        0.0,
        10.0,
        5.0
    ));
    let c = id(&d, "c");
    let exact = bbox(&mut d, &mut ctx, c, LOCAL).unwrap();
    assert!(
        rect_close(exact, 0.0, 0.0, 10.0, 7.5),
        "tight Bézier box: {exact:?}"
    );
    let rough = bbox(
        &mut d,
        &mut ctx,
        c,
        BboxOpts {
            rough: true,
            ..LOCAL
        },
    )
    .unwrap();
    assert!(
        rect_close(rough, 0.0, 0.0, 10.0, 10.0),
        "control-point box: {rough:?}"
    );
    let n_pct = id(&d, "pct");
    assert!(
        rect_close(
            bbox(&mut d, &mut ctx, n_pct, LOCAL).unwrap(),
            0.0,
            0.0,
            10.0,
            5.0
        ),
        "a % width counts as 0"
    );
    let n_defaultw = id(&d, "defaultw");
    assert!(
        rect_close(
            bbox(&mut d, &mut ctx, n_defaultw, LOCAL).unwrap(),
            0.0,
            0.0,
            10.0,
            5.0
        ),
        "unspecified width defaults to 0px here, not 1"
    );
    let n_nostroke = id(&d, "nostroke");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_nostroke, LOCAL).unwrap(),
        0.0,
        0.0,
        10.0,
        5.0
    ));
    let n_empty = id(&d, "empty");
    assert_eq!(bbox(&mut d, &mut ctx, n_empty, LOCAL), None);
}

#[test]
fn line_and_image_boxes() {
    let mut d = doc(&format!(
        r#"<svg {NS} viewBox="0 0 200 100"><line id="l" x1="0" y1="0" x2="4" y2="3" style="stroke:red;stroke-width:1"/><line id="l2" x2="4" y2="-3"/><image id="i" x="10%" width="50%" height="100%"/><image id="j" x="1" y="2" width="3mm" height="4"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_l = id(&d, "l");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_l, LOCAL).unwrap(),
        -0.5,
        -0.5,
        4.5,
        3.5
    ));
    let n_l2 = id(&d, "l2");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_l2, LOCAL).unwrap(),
        0.0,
        -3.0,
        4.0,
        0.0
    ));
    let n_i = id(&d, "i");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_i, LOCAL).unwrap(),
        20.0,
        0.0,
        120.0,
        100.0
    ));
    let n_j = id(&d, "j");
    let j = bbox(&mut d, &mut ctx, n_j, LOCAL).unwrap();
    assert!(
        rect_near(j, 1.0, 2.0, 1.0 + 3.0 * 96.0 / 25.4, 6.0, 1e-9),
        "{j:?}"
    );
}

#[test]
fn group_use_and_root_boxes() {
    let mut d = doc(&format!(
        r##"<svg {NS}><defs><rect id="r" width="2" height="3" transform="scale(2)"/></defs><g id="g" transform="translate(100,0)"><rect id="a" width="2" height="2"/><rect id="b" width="1" height="1" transform="translate(5,5)"/><!-- comment --></g><use id="u" xlink:href="#r" x="1" y="1" transform="translate(10,0)"/><use id="dangling" xlink:href="#nope"/><g id="empty"/></svg>"##
    ));
    let mut ctx = Ctx::new();
    let n_g = id(&d, "g");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_g, LOCAL).unwrap(),
        0.0,
        0.0,
        6.0,
        6.0
    ));
    let n_g = id(&d, "g");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_g, VISUAL).unwrap(),
        100.0,
        0.0,
        106.0,
        6.0
    ));
    // translate(x,y) · target.transform on the target's own box, then the use's own transform
    let n_u = id(&d, "u");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_u, LOCAL).unwrap(),
        1.0,
        1.0,
        5.0,
        7.0
    ));
    let n_u = id(&d, "u");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_u, VISUAL).unwrap(),
        11.0,
        1.0,
        15.0,
        7.0
    ));
    let n_dangling = id(&d, "dangling");
    assert_eq!(bbox(&mut d, &mut ctx, n_dangling, LOCAL), None);
    let n_empty = id(&d, "empty");
    assert_eq!(bbox(&mut d, &mut ctx, n_empty, LOCAL), None);
    // the root is a container too; <defs> contributes nothing
    let root = d.svg();
    let whole = bbox(&mut d, &mut ctx, root, VISUAL).unwrap();
    assert!(rect_close(whole, 11.0, 0.0, 106.0, 7.0), "{whole:?}");
}

#[test]
fn clip_and_mask_clamp_the_box() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect x="5" y="5" width="15" height="15" style="stroke:#000;stroke-width:4"/></clipPath><clipPath id="emptyc"><g/></clipPath><clipPath id="far"><rect x="50" y="50" width="1" height="1"/></clipPath><mask id="m"><rect x="0" y="0" width="7" height="100"/></mask><clipPath id="selfc"><rect id="inner" clip-path="url(#selfc)" width="3" height="3"/></clipPath></defs><rect id="r" width="10" height="10" clip-path="url(#c)"/><rect id="gone" width="10" height="10" clip-path="url(#emptyc)"/><rect id="away" width="10" height="10" clip-path="url(#far)"/><rect id="both" width="10" height="10" clip-path="url(#c)" mask="url(#m)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let n_r = id(&d, "r");
    assert!(
        rect_close(
            bbox(&mut d, &mut ctx, n_r, LOCAL).unwrap(),
            5.0,
            5.0,
            10.0,
            10.0
        ),
        "the clip's own stroke does not count"
    );
    let n_gone = id(&d, "gone");
    assert_eq!(
        bbox(&mut d, &mut ctx, n_gone, LOCAL),
        None,
        "an empty clip clips everything away"
    );
    let n_away = id(&d, "away");
    assert_eq!(
        bbox(&mut d, &mut ctx, n_away, LOCAL),
        None,
        "a disjoint clip too"
    );
    let n_both = id(&d, "both");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_both, LOCAL).unwrap(),
        5.0,
        5.0,
        7.0,
        10.0
    ));
    let unclipped = BboxOpts {
        clip: false,
        ..LOCAL
    };
    let n_r = id(&d, "r");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_r, unclipped).unwrap(),
        0.0,
        0.0,
        10.0,
        10.0
    ));
    // a clipPath child clipped by its own parent: the self-reference is ignored
    let n_inner = id(&d, "inner");
    assert!(rect_close(
        bbox(&mut d, &mut ctx, n_inner, LOCAL).unwrap(),
        0.0,
        0.0,
        3.0,
        3.0
    ));
}

#[test]
fn text_boxes_come_from_the_char_table() {
    let mut d = doc(&format!(
        r##"<svg {NS}><g transform="translate(5,0)"><text id="t" style="font-family:'DejaVu Sans';font-size:10px" x="0" y="0">Hi</text><text id="tp" style="font-family:'DejaVu Sans';font-size:10px"><textPath xlink:href="#p">on a path</textPath></text><path id="p" d="M0 0 L100 0"/></g></svg>"##
    ));
    let mut ctx = Ctx::new();
    let t = id(&d, "t");
    let local = with_vendored_fonts(|| bbox(&mut d, &mut ctx, t, LOCAL)).unwrap();
    assert!(
        close(local.x0, 0.0) && local.x1 > 8.0 && local.x1 < 20.0,
        "{local:?}"
    );
    assert!(
        (local.y0 + 7.29).abs() < 0.05 && close(local.y1, 0.0),
        "cap height 0.729 × 10: {local:?}"
    );
    let visual = bbox(&mut d, &mut ctx, t, VISUAL).unwrap();
    assert!(close(visual.x0, 5.0) && close(visual.x1, local.x1 + 5.0));
    let n_tp = id(&d, "tp");
    assert_eq!(
        bbox(&mut d, &mut ctx, n_tp, LOCAL),
        None,
        "text on a path has no box (Plan 4 residual)"
    );
}

#[test]
fn bb2_reports_supported_rendered_elements_only() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><rect id="in_defs" width="1" height="1"/></defs><g id="g"><path id="p" d="M0 0 L2 0 L2 2 Z"/><rect id="r" x="10" width="1" height="1"/><g id="hidden" style="display:none"><rect id="h" width="1" height="1"/></g><polyline id="pl" points="0,0 1,1"/><circle id="c" cx="5" cy="5" r="1"/><ellipse id="e" cx="5" cy="5" rx="1" ry="2"/><polygon id="pg" points="0,0 1,0 1,1"/><a id="a"><rect id="inlink" width="1" height="1"/></a></g><sodipodi:namedview id="nv"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let els: Vec<NodeId> = d
        .descendants(d.svg())
        .filter(|&n| d.is_element(n))
        .collect();
    let bbs = bb2(&mut d, &mut ctx, &els, false);
    for i in ["g", "p", "r", "h", "pl", "c", "e", "pg", "inlink"] {
        assert!(bbs.contains_key(&id(&d, i)), "{i} should be reported");
    }
    for i in ["in_defs", "nv", "a"] {
        assert!(!bbs.contains_key(&id(&d, i)), "{i} should not be reported");
    }
    assert!(bbs.contains_key(&d.svg()));
    // arcs become cubics (tolerance 1e-4), so round shapes are only near their exact box
    assert!(
        rect_near(bbs[&id(&d, "g")], 0.0, 0.0, 11.0, 7.0, 1e-3),
        "{:?}",
        bbs[&id(&d, "g")]
    );
    assert!(rect_near(bbs[&id(&d, "c")], 4.0, 4.0, 6.0, 6.0, 1e-3));
    assert!(rect_near(bbs[&id(&d, "e")], 4.0, 3.0, 6.0, 7.0, 1e-3));
    assert!(
        rect_close(bbs[&id(&d, "h")], 0.0, 0.0, 1.0, 1.0),
        "display:none is not bb2's concern"
    );
}

#[test]
fn is_rectangle_cases() {
    let d = doc(&format!(
        r##"<svg {NS}><defs><clipPath id="rc"><rect width="1" height="1"/></clipPath><clipPath id="sc"><path d="M0 0 L1 0 L2 1 Z"/></clipPath><mask id="m"><rect width="1" height="1"/></mask><filter id="f"/><rect id="target" width="1" height="1"/></defs><path id="p1" d="M0 0 L10 0 L10 5 L0 5 Z"/><path id="p2" d="M0 0 h10 v5 h-10 z"/><path id="tri" d="M0 0 L10 0 L10 5 Z"/><path id="skew" d="M0 0 L10 0 L12 5 L2 5 Z"/><path id="seven" d="M0 0 L10 0 L10 5 L0 5 L0 0 L0 0 Z"/><path id="rot" d="M0 0 L10 0 L10 5 L0 5 Z" transform="rotate(45)"/><path id="scaled" d="M0 0 L10 0 L10 5 L0 5 Z" transform="scale(2,3)"/><path id="three" d="M0 0 L10 0 Z"/><rect id="r" width="2" height="1" transform="rotate(30)"/><rect id="rr" width="2" height="1" rx="0.2"/><line id="l" x2="1" y2="1"/><polyline id="pl" points="0,0 4,0 4,3 0,3 0,0"/><polygon id="pg" points="0,0 4,0 4,3 0,3"/><use id="u" xlink:href="#target"/><use id="ud" xlink:href="#nope"/><path id="masked" d="M0 0 L10 0 L10 5 L0 5 Z" mask="url(#m)"/><path id="filtered" d="M0 0 L10 0 L10 5 L0 5 Z" style="filter:url(#f)"/><path id="filtered_dangling" d="M0 0 L10 0 L10 5 L0 5 Z" style="filter:url(#nofilter)"/><path id="rectclip" d="M0 0 L10 0 L10 5 L0 5 Z" clip-path="url(#rc)"/><path id="skewclip" d="M0 0 L10 0 L10 5 L0 5 Z" clip-path="url(#sc)"/><path id="near" d="M0 0 L10 0 L10.005 5 L0 5 Z"/><path id="far" d="M0 0 L10 0 L10.02 5 L0 5 Z"/></svg>"##
    ));
    // upstream's test is "two distinct x's and two distinct y's among the end points": a right
    // triangle passes it (tri), a parallelogram does not (skew); polygons are not rect-like tags
    let yes = [
        "p1",
        "p2",
        "tri",
        "pl",
        "u",
        "ud",
        "filtered_dangling",
        "rectclip",
        "near",
    ];
    let no = [
        "skew", "seven", "three", "l", "pg", "masked", "filtered", "skewclip", "far", "rr",
    ];
    for i in yes {
        assert!(
            is_rectangle(&d, id(&d, i), true),
            "{i} should be a rectangle"
        );
    }
    for i in no {
        assert!(
            !is_rectangle(&d, id(&d, i), true),
            "{i} should not be a rectangle"
        );
    }
    assert!(!is_rectangle(&d, id(&d, "rot"), true) && is_rectangle(&d, id(&d, "rot"), false));
    assert!(
        is_rectangle(&d, id(&d, "scaled"), true),
        "axis-aligned scaling keeps it rectangular"
    );
    assert!(
        !is_rectangle(&d, id(&d, "r"), true),
        "a rotated <rect> is not one with its transform"
    );
    assert!(
        is_rectangle(&d, id(&d, "r"), false) && is_rectangle(&d, id(&d, "rr"), false),
        "…but a <rect> is one by definition without it"
    );
}

#[test]
fn is_rectangle_survives_a_branching_self_referencing_clip() {
    let d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="loop"><rect clip-path="url(#loop)" width="1" height="1"/><rect clip-path="url(#loop)" width="1" height="1"/></clipPath></defs><rect id="r" width="3" height="3" clip-path="url(#loop)"/></svg>"#
    ));
    let t0 = std::time::Instant::now();
    assert!(
        !is_rectangle(&d, id(&d, "r"), true),
        "an unresolvable clip is not a rectangle"
    );
    assert!(
        t0.elapsed().as_secs() < 10,
        "must terminate, took {:?}",
        t0.elapsed()
    );
}
