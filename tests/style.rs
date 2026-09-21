mod support;

use sciink::dom::{Doc, NodeId};
use sciink::style::Style;

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}

fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i)
        .unwrap_or_else(|| panic!("no element with id {i}"))
}

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

#[test]
fn parse_and_serialize_declarations() {
    let s = Style::parse("fill:#fff; stroke : none ;;bad; Opacity:0.5");
    assert_eq!(
        s.0,
        vec![
            ("fill".to_string(), "#fff".to_string()),
            ("stroke".to_string(), "none".to_string()),
            ("opacity".to_string(), "0.5".to_string())
        ]
    );
    assert_eq!(s.to_css(), "fill:#fff;stroke:none;opacity:0.5");
    let mut t = s.clone();
    t.set("fill", "red");
    t.set("stroke-width", "2");
    assert_eq!(
        t.to_css(),
        "fill:red;stroke:none;opacity:0.5;stroke-width:2"
    );
    assert_eq!(t.remove("stroke"), Some("none".to_string()));
    assert_eq!(t.get("stroke"), None);
}

#[test]
fn precedence_presentation_sheet_inline() {
    let d = doc(&format!(
        "<svg {NS}><style>rect{{fill:red}} .c{{fill:green}} #r{{fill:blue}}</style>\
         <rect id=\"r\" class=\"c\" fill=\"black\" style=\"stroke:red\"/>\
         <rect id=\"s\" class=\"c\" fill=\"black\" style=\"fill:yellow\"/>\
         <rect id=\"t\" fill=\"black\"/>\
         <rect id=\"u\" class=\"c\" fill=\"black\"/></svg>"
    ));
    assert_eq!(
        d.computed(id(&d, "r"), "fill"),
        "blue",
        "id rule beats class and tag"
    );
    assert_eq!(
        d.computed(id(&d, "s"), "fill"),
        "yellow",
        "inline beats every rule"
    );
    assert_eq!(
        d.computed(id(&d, "t"), "fill"),
        "red",
        "tag rule beats presentation attribute"
    );
    assert_eq!(d.computed(id(&d, "u"), "fill"), "green", "class beats tag");
    assert_eq!(d.computed(id(&d, "r"), "stroke"), "red");
}

#[test]
fn important_in_sheet_beats_inline() {
    let d = doc(&format!(
        "<svg {NS}><style>rect{{fill:red !important}}</style><rect id=\"r\" style=\"fill:blue\"/></svg>"
    ));
    assert_eq!(d.computed(id(&d, "r"), "fill"), "red");
}

#[test]
fn specified_style_propagates_every_property_from_ancestors() {
    let d = doc(&format!(
        "<svg {NS}><g style=\"fill:red;opacity:0.5\"><g stroke=\"blue\"><path id=\"p\" style=\"fill:green\"/></g></g></svg>"
    ));
    let p = id(&d, "p");
    assert_eq!(d.specified(p, "fill"), Some("green".to_string()));
    assert_eq!(
        d.specified(p, "opacity"),
        Some("0.5".to_string()),
        "non-inherited props propagate too (upstream semantics)"
    );
    assert_eq!(d.specified(p, "stroke"), Some("blue".to_string()));
    assert_eq!(d.specified(p, "stroke-width"), None);
    assert_eq!(d.computed(p, "stroke-width"), "1", "defaults fill the gaps");
    assert_eq!(d.computed(p, "font-size"), "medium");
    let own = d.cascaded_style(p);
    assert_eq!(
        own.to_css(),
        "fill:green",
        "cascaded style is the element's own declarations only"
    );
}

#[test]
fn svglite_cdata_sheet_with_descendant_selectors() {
    let d = doc(&format!(
        "<svg {NS}><style><![CDATA[.svglite line, .svglite polyline {{ fill: none; stroke: #000000; }}\n.svglite text {{ white-space: pre; }}]]></style>\
         <g class=\"svglite\"><g><line id=\"l\"/></g><text id=\"t\"/></g><line id=\"outside\"/></svg>"
    ));
    assert_eq!(d.computed(id(&d, "l"), "stroke"), "#000000");
    assert_eq!(d.computed(id(&d, "l"), "fill"), "none");
    assert_eq!(d.computed(id(&d, "t"), "white-space"), "pre");
    assert_eq!(d.computed(id(&d, "outside"), "stroke"), "none");
}

#[test]
fn matplotlib_universal_rule_and_font_shorthand() {
    let d = doc(&format!(
        "<svg {NS}><style>*{{stroke-linecap:butt;stroke-linejoin:round;}}</style>\
         <g id=\"g\"><text id=\"t\" style=\"font: italic bold 12px/30px 'DejaVu Sans', sans-serif\">x</text></g></svg>"
    ));
    assert_eq!(d.computed(id(&d, "g"), "stroke-linecap"), "butt");
    let t = id(&d, "t");
    assert_eq!(d.specified(t, "font-style"), Some("italic".to_string()));
    assert_eq!(d.specified(t, "font-weight"), Some("bold".to_string()));
    assert_eq!(d.specified(t, "font-size"), Some("12px".to_string()));
    assert_eq!(d.specified(t, "line-height"), Some("30px".to_string()));
    assert_eq!(
        d.specified(t, "font-family"),
        Some("'DejaVu Sans', sans-serif".to_string())
    );
    assert_eq!(
        Style::parse("font: 10px 'DejaVu Sans'").get("font-family"),
        Some("'DejaVu Sans'")
    );
}

#[test]
fn declarations_split_on_semicolons_outside_quotes_and_parens() {
    let s = Style::parse("fill:url(\"data:image/png;base64,AAAA\");stroke:red");
    assert_eq!(
        s.0.len(),
        2,
        "the ';' inside url(\"...\") must not start a new declaration: {s:?}"
    );
    assert_eq!(
        s.get("fill"),
        Some("url(\"data:image/png;base64,AAAA\")"),
        "the fill value must survive intact"
    );
    assert_eq!(s.get("stroke"), Some("red"));
}

#[test]
fn font_shorthand_accepts_font_stretch_keywords() {
    let s = Style::parse("font: condensed 10px Arial");
    assert_eq!(s.get("font-stretch"), Some("condensed"));
    assert_eq!(s.get("font-size"), Some("10px"));
    assert_eq!(s.get("font-family"), Some("Arial"));
}

#[test]
fn child_combinator_and_unsupported_selectors() {
    let d = doc(&format!(
        "<svg {NS}><style>svg > rect{{fill:red}} a:hover{{fill:pink}} rect[x]{{fill:pink}} g rect{{stroke:blue}}</style>\
         <rect id=\"a\"/><g><rect id=\"b\"/></g></svg>"
    ));
    assert_eq!(d.computed(id(&d, "a"), "fill"), "red");
    assert_eq!(
        d.computed(id(&d, "b"), "fill"),
        "black",
        "child combinator does not match grandchildren"
    );
    assert_eq!(d.computed(id(&d, "b"), "stroke"), "blue");
    assert_eq!(d.computed(id(&d, "a"), "stroke"), "none");
}

#[test]
fn dangling_combinator_drops_the_rule() {
    let d = doc(&format!(
        "<svg {NS}><style>svg > {{fill:red}} rect{{fill:blue}}</style><rect id=\"r\"/></svg>"
    ));
    assert_eq!(d.computed(id(&d, "r"), "fill"), "blue");
    assert_eq!(
        d.computed(d.svg(), "fill"),
        "black",
        "a trailing '>' with nothing after it must not fall back to matching 'svg' alone"
    );
}

#[test]
fn set_style_moves_presentation_attribute_into_style() {
    let mut d = doc(&format!(
        "<svg {NS}><rect id=\"r\" fill=\"black\" stroke=\"red\"/></svg>"
    ));
    let r = id(&d, "r");
    assert_eq!(d.computed(r, "fill"), "black");
    d.set_style(r, "fill", "red");
    assert_eq!(d.attr(r, "fill"), None);
    assert_eq!(d.attr(r, "style"), Some("fill:red"));
    assert_eq!(
        d.computed(r, "fill"),
        "red",
        "cache invalidated by the mutation"
    );
    d.remove_style(r, "stroke");
    assert_eq!(d.attr(r, "stroke"), None);
    assert_eq!(d.computed(r, "stroke"), "none");
    d.remove_style(r, "fill");
    assert_eq!(d.attr(r, "style"), None, "empty style attribute is removed");
    d.set_style_map(r, &Style::parse("fill:blue;opacity:0.5"));
    assert_eq!(d.attr(r, "style"), Some("fill:blue;opacity:0.5"));
}

#[test]
fn sheet_changes_invalidate_the_cache() {
    let mut d = doc(&format!(
        "<svg {NS}><style id=\"s\">rect{{fill:red}}</style><rect id=\"r\"/></svg>"
    ));
    let r = id(&d, "r");
    assert_eq!(d.computed(r, "fill"), "red");
    let txt = d.first_child(id(&d, "s")).unwrap();
    d.set_text(txt, "rect{fill:green}");
    assert_eq!(d.computed(r, "fill"), "green");
    let s = id(&d, "s");
    d.detach(s);
    assert_eq!(d.computed(r, "fill"), "black");
}

#[test]
fn geometry_attribute_writes_keep_the_style_cache() {
    let mut d = doc(&format!(
        "<svg {NS}><g style=\"fill:red\"><path id=\"p\"/></g></svg>"
    ));
    let p = id(&d, "p");
    assert_eq!(d.specified(p, "fill"), Some("red".to_string()));
    let sg0 = d.style_generation();
    let gen0 = d.generation();

    d.set_attr(p, "transform", "translate(1,2)");
    d.set_attr(p, "d", "M 0,0");
    assert_eq!(
        d.style_generation(),
        sg0,
        "geometry-only attribute writes must not bump style_generation"
    );
    assert!(
        d.generation() > gen0,
        "geometry-only attribute writes must still bump generation"
    );
    assert_eq!(
        d.specified(p, "fill"),
        Some("red".to_string()),
        "the cached specified style survived the geometry writes"
    );

    let sg1 = d.style_generation();
    d.set_attr(p, "fill", "blue");
    assert!(
        d.style_generation() > sg1,
        "a presentation-attribute write must bump style_generation"
    );
    assert_eq!(d.specified(p, "fill"), Some("blue".to_string()));

    let sg2 = d.style_generation();
    d.set_attr(p, "class", "c");
    assert!(
        d.style_generation() > sg2,
        "writing class must bump style_generation"
    );

    let sg3 = d.style_generation();
    d.set_attr(p, "style", "fill:green");
    assert!(
        d.style_generation() > sg3,
        "writing style must bump style_generation"
    );
    assert_eq!(d.specified(p, "fill"), Some("green".to_string()));
}

#[test]
fn style_less_child_shares_the_parent_specified_style_allocation() {
    let d = doc(&format!(
        "<svg {NS}><g id=\"g\" style=\"fill:red\"><g id=\"child\"/></g></svg>"
    ));
    let g = id(&d, "g");
    let child = id(&d, "child");
    assert!(
        std::rc::Rc::ptr_eq(&d.specified_style(child), &d.specified_style(g)),
        "a <g> with no declarations of its own must share its parent's Rc<Style>"
    );
}

#[test]
fn upstream_fixture_styles_resolve() {
    // Text_tests.svg uses class sheets (`class="st38 st39"`); most text elements
    // resolve their font-family from a class rule via the cascade, not the SVG
    // default — so unlike `computed` (which always falls back to `sans-serif`
    // and so can never fail here), this checks `specified` directly.
    let Some(dir) = support::upstream_data_dir() else {
        eprintln!("SKIP: upstream fixtures not found");
        return;
    };
    let d = Doc::parse(&std::fs::read(dir.join("svg/Text_tests.svg")).unwrap()).unwrap();
    let mut texts = 0;
    let mut with_font_family = 0;
    for n in d.descendants(d.svg()) {
        if d.tag(n) == "text" {
            texts += 1;
            if d.specified(n, "font-family").is_some() {
                with_font_family += 1;
            }
        }
    }
    assert!(texts > 100, "expected many text elements, found {texts}");
    assert!(
        with_font_family >= 170,
        "expected most texts to resolve a font-family from a class rule, found {with_font_family} of {texts}"
    );

    // The fixture's <style> sheets define `.st1{font-family:'DejaVu Sans';}`;
    // confirm a real element wearing that class resolves it through the cascade.
    let has_st1 = |n: NodeId| {
        d.attr(n, "class")
            .is_some_and(|c| c.split_ascii_whitespace().any(|cls| cls == "st1"))
    };
    let st1 = d
        .descendants(d.svg())
        .find(|&n| matches!(d.tag(n), "text" | "tspan") && has_st1(n))
        .expect("no text/tspan with class \"st1\" in Text_tests.svg");
    assert_eq!(
        d.specified(st1, "font-family"),
        Some("'DejaVu Sans'".to_string())
    );
}

#[test]
fn sheet_value_reports_only_stylesheet_declarations() {
    let d = doc(&format!(
        "<svg {NS}><style>#r{{clip-path:url(#a)}} rect{{clip-path:url(#b);fill:red}}</style>\
         <rect id=\"r\" clip-path=\"url(#c)\" style=\"clip-path:url(#d);fill:blue\"/><rect id=\"q\"/></svg>"
    ));
    let r = id(&d, "r");
    // the id rule beats the tag rule; neither the attribute nor the inline style count
    assert_eq!(d.sheet_value(r, "clip-path").as_deref(), Some("url(#a)"));
    assert_eq!(d.sheet_value(r, "fill").as_deref(), Some("red"));
    assert_eq!(d.sheet_value(r, "stroke"), None);
    assert_eq!(
        d.sheet_value(id(&d, "q"), "clip-path").as_deref(),
        Some("url(#b)")
    );
    // later rules of equal weight win, `!important` beats everything
    let d = doc(&format!(
        "<svg {NS}><style>rect{{fill:red !important}} rect{{fill:green}} #r{{fill:blue}}</style><rect id=\"r\"/></svg>"
    ));
    assert_eq!(d.sheet_value(id(&d, "r"), "fill").as_deref(), Some("red"));
}
