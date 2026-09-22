mod support;

use kurbo::Affine;
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

fn out(d: &Doc) -> String {
    let mut v = Vec::new();
    d.write(&mut v);
    String::from_utf8(v).unwrap()
}

fn kids(d: &Doc, n: NodeId) -> Vec<NodeId> {
    d.children(n).filter(|&c| d.is_element(c)).collect()
}

use sciink::ops::xform::{Ranges, combine_paths, fuse, global_transform, object_to_path};
use sciink::style::Style;

fn style_of(d: &Doc, n: NodeId) -> Style {
    d.attr(n, "style").map(Style::parse).unwrap_or_default()
}

#[test]
fn object_to_path_converts_shapes_and_drops_their_attributes() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="r" x="1" y="2" width="3" height="4" style="fill:red"/><line id="l" x1="0" y1="0" x2="1" y2="1"/><path id="p" d="M0 0h1"/><g id="g"/></svg>"#
    ));
    let r = id(&d, "r");
    object_to_path(&mut d, r);
    assert_eq!(d.tag(r), "path");
    assert_eq!(d.attr(r, "d"), Some("M 1,2 L 4,2 L 4,6 L 1,6 Z"));
    assert_eq!(
        (d.attr(r, "x"), d.attr(r, "width"), d.attr(r, "style")),
        (None, None, Some("fill:red"))
    );
    let l = id(&d, "l");
    object_to_path(&mut d, l);
    assert_eq!(
        (d.tag(l), d.attr(l, "d"), d.attr(l, "x1")),
        ("path", Some("M 0,0 L 1,1"), None)
    );
    let p = id(&d, "p");
    object_to_path(&mut d, p);
    assert_eq!(
        d.attr(p, "d"),
        Some("M0 0h1"),
        "an existing path is not re-serialized"
    );
    let g = id(&d, "g");
    object_to_path(&mut d, g);
    assert_eq!(d.tag(g), "g");
}

#[test]
fn fuse_bakes_the_transform_into_a_path_and_its_stroke() {
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="p" d="M0 0 L1 0" transform="scale(2)" style="stroke:#000;stroke-width:1;stroke-dasharray:1,2" sodipodi:nodetypes="cc" inkscape:label="keep me?" inkscape-scientific-combined-by-color="0 2"/><g id="g" transform="scale(2)"><path id="child" d="M0 0 L1 0"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::IDENTITY, None, true);
    assert_eq!(d.attr(p, "d"), Some("M 0,0 L 2,0"));
    assert_eq!(d.attr(p, "transform"), None);
    let st = style_of(&d, p);
    assert_eq!(
        (st.get("stroke-width"), st.get("stroke-dasharray")),
        (Some("2"), Some("2,4"))
    );
    assert_eq!(
        (d.attr(p, "sodipodi:nodetypes"), d.attr(p, "inkscape:label")),
        (None, None),
        "Inkscape's path metadata goes"
    );
    assert_eq!(
        d.attr(p, "inkscape-scientific-combined-by-color"),
        Some("0 2"),
        "…our compatibility attribute stays"
    );
    // groups are untouched, children not visited
    let g = id(&d, "g");
    fuse(&mut d, &mut ctx, g, Affine::IDENTITY, None, true);
    assert_eq!(d.attr(g, "transform"), Some("scale(2)"));
    assert_eq!(d.attr(id(&d, "child"), "d"), Some("M0 0 L1 0"));
    // identity and nothing to do: the d is not even re-serialized
    let mut d = doc(&format!(r#"<svg {NS}><path id="p" d="M0 0h1"/></svg>"#));
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::IDENTITY, None, true);
    assert_eq!(d.attr(p, "d"), Some("M0 0h1"));
}

#[test]
fn fuse_extra_transform_and_inherited_stroke_width() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g style="stroke:red"><path id="p" d="M0 0 L1 0"/><path id="q" d="M0 0 L1 0" style="stroke:none"/></g></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::scale(3.0), None, true);
    assert_eq!(d.attr(p, "d"), Some("M 0,0 L 3,0"));
    assert_eq!(
        d.attr(p, "style"),
        Some("stroke-width:3"),
        "inherited stroke: the default width 1 is made explicit and scaled"
    );
    let q = id(&d, "q");
    fuse(&mut d, &mut ctx, q, Affine::scale(3.0), None, true);
    assert_eq!(
        d.attr(q, "style"),
        Some("stroke:none"),
        "no stroke → no width written"
    );
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="p" d="M0 0 L1 0" style="stroke:red;stroke-width:2"/></svg>"#
    ));
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::scale(3.0), None, false);
    assert_eq!(
        style_of(&d, p).get("stroke-width"),
        Some("2"),
        "apply_to_stroke=false leaves strokes alone"
    );
}

#[test]
fn fuse_handles_rect_circle_ellipse_line_polyline_polygon() {
    let mut d = doc(&format!(
        r#"<svg {NS}><rect id="r" x="1" y="1" width="2" height="3" rx="0.5" transform="matrix(2,0,0,-1,0,10)"/><rect id="rr" width="1" height="1" transform="matrix(0,1,-1,0,0,0)"/><circle id="c" cx="1" cy="1" r="1" transform="scale(2,3)"/><ellipse id="e" cx="0" cy="0" rx="1" ry="2" transform="scale(2,1)"/><line id="l" x1="0" y1="0" x2="1" y2="1" transform="translate(5,6)"/><polyline id="pl" points="0,0 1,1" transform="scale(2)"/><polygon id="pg" points="0,0 1,0 1,1" transform="translate(1,1)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    for i in ["r", "rr", "c", "e", "l", "pl", "pg"] {
        let n = id(&d, i);
        fuse(&mut d, &mut ctx, n, Affine::IDENTITY, None, true);
        assert_eq!(d.attr(n, "transform"), None, "{i}");
    }
    let r = id(&d, "r");
    assert_eq!(d.tag(r), "rect");
    let get = |n: NodeId, a: &str| d.attr(n, a).map(str::to_string);
    assert_eq!(
        (get(r, "x"), get(r, "y"), get(r, "width"), get(r, "height")),
        (
            Some("2".into()),
            Some("6".into()),
            Some("4".into()),
            Some("3".into())
        )
    );
    assert_eq!(
        (get(r, "rx"), get(r, "ry")),
        (Some("1".into()), Some("0.5".into())),
        "radii follow the axis scales"
    );
    let rr = id(&d, "rr");
    assert_eq!(d.tag(rr), "path", "a rotated rect becomes a path");
    assert_eq!(get(rr, "d"), Some("M 0,0 L 0,1 L -1,1 L -1,0 Z".into()));
    assert_eq!(get(rr, "width"), None);
    let c = id(&d, "c");
    assert_eq!(
        d.tag(c),
        "ellipse",
        "non-uniform scale turns a circle into an ellipse"
    );
    assert_eq!(
        (
            get(c, "cx"),
            get(c, "cy"),
            get(c, "rx"),
            get(c, "ry"),
            get(c, "r")
        ),
        (
            Some("2".into()),
            Some("3".into()),
            Some("2".into()),
            Some("3".into()),
            None
        )
    );
    let e = id(&d, "e");
    assert_eq!(
        d.tag(e),
        "circle",
        "…and equal edges turn an ellipse into a circle"
    );
    assert_eq!(
        (get(e, "cx"), get(e, "cy"), get(e, "r"), get(e, "rx")),
        (Some("0".into()), Some("0".into()), Some("2".into()), None)
    );
    let l = id(&d, "l");
    assert_eq!(
        (get(l, "x1"), get(l, "y1"), get(l, "x2"), get(l, "y2")),
        (
            Some("5".into()),
            Some("6".into()),
            Some("6".into()),
            Some("7".into())
        )
    );
    assert_eq!(get(id(&d, "pl"), "points"), Some("0,0 2,2".into()));
    // upstream quirk kept: a polygon's closing command contributes the start point once more
    assert_eq!(get(id(&d, "pg"), "points"), Some("1,1 2,1 2,2 1,1".into()));
}

#[test]
fn fuse_with_ranges_transforms_each_slice_on_its_own() {
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="p" d="M0 0 L1 0 M0 5 L1 5" transform="scale(2)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    let ranges: Ranges = vec![
        (0..2, Affine::translate((0.0, 1.0))),
        (2..4, Affine::translate((0.0, -1.0))),
    ];
    fuse(&mut d, &mut ctx, p, Affine::IDENTITY, Some(&ranges), true);
    assert_eq!(
        d.attr(p, "d"),
        Some("M 0,1 L 1,1 M 0,4 L 1,4"),
        "the element's own transform is NOT applied to ranged geometry — the ranges carry it"
    );
    assert_eq!(d.attr(p, "transform"), None);
}

#[test]
fn fuse_duplicates_transformed_clips_and_user_space_gradients() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1" height="1"/></clipPath><linearGradient id="g" gradientUnits="userSpaceOnUse" gradientTransform="scale(2)"/><linearGradient id="obb"/></defs><path id="p" d="M0 0 L1 0" transform="translate(1,0)" clip-path="url(#c)" style="fill:url(#g);stroke:url(#obb)"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let p = id(&d, "p");
    fuse(&mut d, &mut ctx, p, Affine::IDENTITY, None, true);
    assert_eq!(d.attr(p, "d"), Some("M 1,0 L 2,0"));
    let cv = d.attr(p, "clip-path").unwrap().to_string();
    let cdup = id(&d, cv.trim_start_matches("url(#").trim_end_matches(')'));
    assert_ne!(cdup, id(&d, "c"));
    assert_eq!(ctx.created, vec![cdup]);
    assert_eq!(
        d.attr(kids(&d, cdup)[0], "transform"),
        Some("translate(1,0)"),
        "the clip keeps following the geometry"
    );
    let st = style_of(&d, p);
    let fill = st.get("fill").unwrap().to_string();
    assert_ne!(fill, "url(#g)");
    let gdup = id(&d, fill.trim_start_matches("url(#").trim_end_matches(')'));
    assert_eq!(d.tag(gdup), "linearGradient");
    assert_eq!(
        d.attr(gdup, "gradientTransform"),
        Some("matrix(2,0,0,2,1,0)"),
        "translate(1,0) · scale(2)"
    );
    assert_eq!(
        d.attr(id(&d, "g"), "gradientTransform"),
        Some("scale(2)"),
        "the original is untouched"
    );
    assert_eq!(
        st.get("stroke"),
        Some("url(#obb)"),
        "an objectBoundingBox gradient follows the box by itself"
    );
    assert_eq!(out(&d).matches("<linearGradient").count(), 3);
}

#[test]
fn global_transform_works_in_the_parent_frame_and_preserves_strokes() {
    let mut d = doc(&format!(
        r#"<svg {NS}><g transform="scale(2)"><g id="k" transform="translate(1,0)" style="stroke-width:1;stroke-dasharray:2,4"><path id="p" d="M0 0 L1 0"/></g></g><g id="k2" style="stroke-width:1"/><path id="s" d="M0 0 L1 0" transform="translate(1,1)" style="stroke:#000;stroke-width:1"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let k = id(&d, "k");
    global_transform(&mut d, &mut ctx, k, Affine::scale(3.0), None, true);
    assert_eq!(
        d.attr(k, "transform"),
        Some("matrix(3,0,0,3,3,0)"),
        "P⁻¹ · scale(3) · P · translate(1,0)"
    );
    let st = style_of(&d, k);
    assert_eq!(
        st.get("stroke-width"),
        Some("0.33333333"),
        "visual width 2 kept under the new scale factor 6"
    );
    assert_eq!(st.get("stroke-dasharray"), Some("0.66666667,1.3333333"));
    assert_eq!(
        d.attr(id(&d, "p"), "d"),
        Some("M0 0 L1 0"),
        "children of a group are not fused"
    );
    // a pure translation keeps the width as it is: nothing is rewritten
    let k2 = id(&d, "k2");
    global_transform(
        &mut d,
        &mut ctx,
        k2,
        Affine::translate((5.0, 5.0)),
        None,
        true,
    );
    assert_eq!(d.attr(k2, "transform"), Some("translate(5,5)"));
    assert_eq!(d.attr(k2, "style"), Some("stroke-width:1"));
    // a shape is fused: the transform disappears into d, the stroke stays visually 1 wide
    let s = id(&d, "s");
    global_transform(&mut d, &mut ctx, s, Affine::scale(2.0), None, true);
    assert_eq!(d.attr(s, "transform"), None);
    assert_eq!(d.attr(s, "d"), Some("M 2,2 L 4,2"));
    assert_eq!(
        style_of(&d, s).get("stroke-width"),
        Some("1"),
        "fused and then restored to the same visual width"
    );
    // without preservation the stroke scales with the geometry
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="s" d="M0 0 L1 0" style="stroke:#000;stroke-width:1"/></svg>"#
    ));
    let s = id(&d, "s");
    global_transform(&mut d, &mut ctx, s, Affine::scale(2.0), None, false);
    assert_eq!(style_of(&d, s).get("stroke-width"), Some("2"));
}

#[test]
fn combine_paths_concatenates_global_geometry_into_the_target_frame() {
    let mut d = doc(&format!(
        r#"<svg {NS}><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><g id="wrap" transform="translate(10,0)"><path id="a" d="M0 0 L1 0"/></g><path id="b" d="M0 0 L0 1 Z" transform="scale(2)" clip-path="url(#c)" style="fill:red"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    assert!(combine_paths(&mut d, &mut ctx, &[b, a], 0));
    // b: 3 elements (M L Z) in root coordinates, then a's 2 → starts 0, 3, total 5
    assert_eq!(
        d.attr(b, "inkscape-scientific-combined-by-color"),
        Some("0 3 5")
    );
    // written back in b's own frame (inverse of scale(2)): a's global M10 0 L11 0 → M5 0 L5.5 0
    assert_eq!(d.attr(b, "d"), Some("M 0,0 L 0,1 Z M 5,0 L 5.5,0"));
    assert_eq!(
        d.attr(b, "transform"),
        Some("scale(2)"),
        "the target keeps its transform attribute as written"
    );
    assert_eq!(
        (d.attr(b, "clip-path"), d.attr(b, "mask")),
        (Some("none"), Some("none")),
        "clips and masks are released"
    );
    assert_eq!(d.attr(b, "style"), Some("fill:red"));
    assert_eq!(d.by_id("a"), None);
    assert_eq!(
        d.by_id("wrap"),
        None,
        "the emptied group went with it (delete_up)"
    );
    assert!(ctx.deleted.contains("a") && ctx.deleted.contains("wrap"));
}

#[test]
fn combine_paths_welds_existing_indices_and_converts_a_line_target() {
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="a" d="M0 0 L1 0 M0 1 L1 1" inkscape-scientific-combined-by-color="0 2 4"/><path id="b" d="M5 5 L6 5 Z"/><line id="l" x1="0" y1="0" x2="1" y2="1" style="stroke:#000"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (a, b, l) = (id(&d, "a"), id(&d, "b"), id(&d, "l"));
    assert!(combine_paths(&mut d, &mut ctx, &[a, b], 0));
    assert_eq!(
        d.attr(a, "inkscape-scientific-combined-by-color"),
        Some("0 2 4 7"),
        "a's own pieces stay separate pieces"
    );
    assert_eq!(
        d.attr(a, "d"),
        Some("M 0,0 L 1,0 M 0,1 L 1,1 M 5,5 L 6,5 Z")
    );
    assert_eq!(d.by_id("b"), None);
    // a <line> target becomes a <path>
    assert!(combine_paths(&mut d, &mut ctx, &[a, l], 1));
    assert_eq!(d.tag(l), "path");
    assert_eq!(d.attr(l, "x1"), None);
    assert_eq!(
        d.attr(l, "d"),
        Some("M 0,0 L 1,0 M 0,1 L 1,1 M 5,5 L 6,5 Z M 0,0 L 1,1"),
        "geometry follows the list order, a first"
    );
    assert_eq!(
        d.attr(l, "inkscape-scientific-combined-by-color"),
        Some("0 2 4 7 9")
    );
    assert_eq!(d.attr(l, "style"), Some("stroke:#000"));
    assert_eq!(d.by_id("a"), None);
    // a singular target is refused and nothing changes
    let mut d = doc(&format!(
        r#"<svg {NS}><path id="a" d="M0 0 L1 0"/><path id="b" d="M0 0 L1 0" transform="scale(0)"/></svg>"#
    ));
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    assert!(!combine_paths(&mut d, &mut ctx, &[a, b], 1));
    assert!(d.by_id("a").is_some() && d.attr(b, "inkscape-scientific-combined-by-color").is_none());
    assert!(
        ctx.warn.0.iter().any(|w| w.contains("singular")),
        "{:?}",
        ctx.warn.0
    );
}

#[test]
fn combine_paths_pins_released_clips_against_a_stylesheet() {
    let mut d = doc(&format!(
        r#"<svg {NS}><style>#a{{clip-path:url(#c)}}</style><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><path id="a" d="M0 0 L1 0" clip-path="url(#c)"/><path id="b" d="M2 0 L3 0"/></svg>"#
    ));
    let mut ctx = Ctx::new();
    let (a, b) = (id(&d, "a"), id(&d, "b"));
    assert!(combine_paths(&mut d, &mut ctx, &[a, b], 0));
    assert!(
        out(&d).contains("\n#a{clip-path:none}</style>"),
        "{}",
        out(&d)
    );
    assert!(
        !out(&d).contains("#a{mask"),
        "the sheet says nothing about masks → nothing pinned"
    );
}
