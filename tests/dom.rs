mod support;

use sciink::dom::{Doc, DomError};

const EDGE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
<!-- top comment -->\n\
<?xml-stylesheet href=\"a.css\"?>\n\
<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n\
<svg\n   xmlns=\"http://www.w3.org/2000/svg\"\n   xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"\n   id=\"root\">\n\
  <style><![CDATA[.a { fill: red; }]]></style>\n\
  <g id=\"g1\" inkscape:label=\"L &amp; M&#10;N &quot;q&quot; &gt;\">\n\
    <path id=\"p1\" d=\"M 0,0 L 1,1\" />\n\
    <text id=\"t1\" xml:space=\"preserve\">a &lt; b &gt; c &amp; d</text>\n\
  </g>\n\
  <!-- inner --><g id=\"g2\"></g>\n\
</svg>\n";

fn roundtrip(s: &str) -> String {
    let doc = Doc::parse(s.as_bytes()).unwrap();
    let mut out = Vec::new();
    doc.write(&mut out);
    String::from_utf8(out).unwrap()
}

#[test]
fn edge_document_round_trips_byte_identically() {
    assert_eq!(roundtrip(EDGE), EDGE);
}

#[test]
fn quote_in_text_content_round_trips_byte_identically() {
    // Inkscape's own serializer writes `&quot;` for `"` inside element text
    // (not just attribute values); we must match it byte-for-byte.
    let s = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>a &quot;b&quot;</text></svg>";
    assert_eq!(roundtrip(s), s);
    let doc = Doc::parse(s.as_bytes()).unwrap();
    let t = doc.children(doc.svg()).next().unwrap();
    let txt = doc.first_child(t).unwrap();
    assert_eq!(
        doc.text(txt),
        Some("a \"b\""),
        "decoded in memory as a literal quote"
    );
}

#[test]
fn entities_are_decoded_in_memory() {
    let doc = Doc::parse(EDGE.as_bytes()).unwrap();
    let g1 = doc.by_id("g1").unwrap();
    assert_eq!(doc.attr(g1, "inkscape:label"), Some("L & M\nN \"q\" >"));
    let t1 = doc.by_id("t1").unwrap();
    assert_eq!(doc.text_content(t1), "a < b > c & d");
}

#[test]
fn single_quoted_attributes_are_normalized() {
    let out = roundtrip("<svg xmlns=\"http://www.w3.org/2000/svg\" id='a' title='say \"hi\"'/>");
    assert_eq!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" id=\"a\" title=\"say &quot;hi&quot;\"/>"
    );
}

#[test]
fn bom_is_stripped() {
    let out = roundtrip("\u{feff}<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
    assert_eq!(out, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
}

#[test]
fn unknown_entity_is_unsupported() {
    let err = Doc::parse(b"<svg><text>a&nbsp;b</text></svg>")
        .err()
        .unwrap();
    assert!(matches!(err, DomError::Unsupported(_)), "{err}");
}

#[test]
fn malformed_char_ref_in_attribute_is_xml_error() {
    let err = Doc::parse(b"<svg id=\"&#zz;\"/>").err().unwrap();
    assert!(matches!(err, DomError::Xml(_)), "{err}");
}

#[test]
fn undefined_entity_in_attribute_is_unsupported() {
    let err = Doc::parse(b"<svg id=\"a&nbsp;b\"/>").err().unwrap();
    assert!(matches!(err, DomError::Unsupported(_)), "{err}");
}

#[test]
fn non_utf8_is_unsupported() {
    let err = Doc::parse(b"<svg>\xff\xfe</svg>").err().unwrap();
    assert!(matches!(err, DomError::Unsupported(_)), "{err}");
}

#[test]
fn malformed_xml_is_an_xml_error() {
    let err = Doc::parse(b"<svg><g></svg>").err().unwrap();
    assert!(matches!(err, DomError::Xml(_)), "{err}");
}

#[test]
fn root_must_be_svg() {
    let err = Doc::parse(b"<html/>").err().unwrap();
    assert!(err.to_string().contains("not <svg>"), "{err}");
}

#[test]
fn nonstandard_prefix_for_known_namespace_is_unsupported() {
    let err = Doc::parse(b"<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:ink=\"http://www.inkscape.org/namespaces/inkscape\"/>").err().unwrap();
    assert!(matches!(err, DomError::Unsupported(_)), "{err}");
}

#[test]
fn nonstandard_prefix_on_descendant_is_unsupported() {
    let err = Doc::parse(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><g xmlns:ink=\"http://www.inkscape.org/namespaces/inkscape\" ink:label=\"x\"/></svg>").err().unwrap();
    assert!(matches!(err, DomError::Unsupported(_)), "{err}");
}

#[test]
fn prefixed_root_is_accepted() {
    let doc = Doc::parse(
        b"<svg:svg xmlns:svg=\"http://www.w3.org/2000/svg\"><svg:g id=\"a\"/></svg:svg>",
    )
    .unwrap();
    assert_eq!(doc.tag(doc.svg()), "svg");
    assert_eq!(doc.tag(doc.by_id("a").unwrap()), "g");
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    let depth = 50_000;
    let mut s = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    for _ in 0..depth {
        s.push_str("<g>");
    }
    for _ in 0..depth {
        s.push_str("</g>");
    }
    s.push_str("</svg>");
    assert_eq!(roundtrip(&s), s);
}

#[test]
fn duplicate_ids_first_wins() {
    let doc = Doc::parse(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"x\" class=\"first\"/><g id=\"x\" class=\"second\"/></svg>").unwrap();
    let n = doc.by_id("x").unwrap();
    assert_eq!(doc.attr(n, "class"), Some("first"));
}

#[test]
fn upstream_fixtures_round_trip_semantically() {
    for p in support::upstream_svgs() {
        let input = std::fs::read_to_string(&p).unwrap();
        let out = roundtrip(&input);
        support::assert_same_tree(&input, &out, &p.display().to_string());
    }
}

#[test]
fn upstream_fixtures_round_trip_byte_identically() {
    // Fixtures that legitimately cannot be byte-identical go here with a reason.
    const KNOWN_DIFFERENT: &[&str] = &[];
    for p in support::upstream_svgs() {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if KNOWN_DIFFERENT.contains(&name.as_str()) {
            continue;
        }
        let input = std::fs::read(&p).unwrap();
        let doc = Doc::parse(&input).unwrap();
        let mut out = Vec::new();
        doc.write(&mut out);
        if out != input {
            let i = out
                .iter()
                .zip(&input)
                .position(|(a, b)| a != b)
                .unwrap_or(out.len().min(input.len()));
            let lo = i.saturating_sub(60);
            panic!(
                "{name}: first difference at byte {i}\n input: {:?}\noutput: {:?}",
                String::from_utf8_lossy(&input[lo..(i + 60).min(input.len())]),
                String::from_utf8_lossy(&out[lo..(i + 60).min(out.len())])
            );
        }
    }
}

const DOC: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">\
<defs id=\"d\"/><g id=\"g\"><path id=\"p\" d=\"M 0,0\"/>tail<use id=\"u\" xlink:href=\"#p\"/></g><text id=\"t\">hi</text><style id=\"s\">g{fill:red}</style></svg>";

fn parse(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}

fn out(doc: &Doc) -> String {
    let mut v = Vec::new();
    doc.write(&mut v);
    String::from_utf8(v).unwrap()
}

#[test]
fn navigation_basics() {
    let d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let p = d.by_id("p").unwrap();
    let u = d.by_id("u").unwrap();
    assert_eq!(d.parent(p), Some(g));
    assert_eq!(d.first_child(g), Some(p));
    assert_eq!(d.last_child(g), Some(u));
    let tail = d.tail(p).unwrap();
    assert_eq!(d.text(tail), Some("tail"));
    assert_eq!(d.next_sibling(tail), Some(u));
    assert_eq!(d.prev_sibling(u), Some(tail));
    assert_eq!(d.tail(u), None);
    assert_eq!(d.href(u), Some("#p"));
    assert_eq!(
        d.ancestors(p).collect::<Vec<_>>(),
        vec![g, d.svg(), d.root()]
    );
    let tags: Vec<&str> = d
        .descendants(d.svg())
        .filter(|&n| d.is_element(n))
        .map(|n| d.tag(n))
        .collect();
    assert_eq!(
        tags,
        vec!["svg", "defs", "g", "path", "use", "text", "style"]
    );
    assert_eq!(d.element_count(), 6);
}

#[test]
fn set_and_remove_attr_bump_generation_and_keep_id_index() {
    let mut d = parse(DOC);
    let p = d.by_id("p").unwrap();
    let before = out(&d);
    d.set_attr(p, "d", "M 1,1");
    d.set_attr(p, "stroke", "red");
    assert_eq!(d.attr(p, "d"), Some("M 1,1"));
    assert!(
        out(&d).contains("<path id=\"p\" d=\"M 1,1\" stroke=\"red\"/>"),
        "{}",
        out(&d)
    );
    assert_ne!(out(&d), before);
    d.set_attr(p, "id", "p2");
    assert_eq!(d.by_id("p2"), Some(p));
    assert_eq!(d.by_id("p"), None);
    assert_eq!(d.remove_attr(p, "stroke"), Some("red".to_string()));
    assert_eq!(d.remove_attr(p, "stroke"), None);
    d.remove_attr(p, "id");
    assert_eq!(d.by_id("p2"), None);
}

#[test]
fn detach_and_reattach_moves_ids_with_the_subtree() {
    let mut d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let t = d.by_id("t").unwrap();
    d.detach(g);
    assert_eq!(d.by_id("p"), None);
    assert_eq!(d.parent(g), None);
    assert!(!out(&d).contains("<g"));
    d.insert_after(g, t);
    assert!(d.by_id("p").is_some());
    let order: Vec<&str> = d.children(d.svg()).map(|n| d.tag(n)).collect();
    assert_eq!(order, vec!["defs", "text", "g", "style"]);
    d.prepend_child(d.svg(), g);
    let order: Vec<&str> = d.children(d.svg()).map(|n| d.tag(n)).collect();
    assert_eq!(order, vec!["g", "defs", "text", "style"]);
    d.insert_before(t, g);
    let order: Vec<&str> = d.children(d.svg()).map(|n| d.tag(n)).collect();
    assert_eq!(order, vec!["text", "g", "defs", "style"]);
}

#[test]
fn append_child_moves_an_attached_node() {
    let mut d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let t = d.by_id("t").unwrap();
    d.append_child(g, t);
    assert_eq!(d.parent(t), Some(g));
    assert_eq!(d.last_child(g), Some(t));
    assert_eq!(d.children(d.svg()).count(), 3);
}

#[test]
fn replace_swaps_nodes_in_place() {
    let mut d = parse(DOC);
    let p = d.by_id("p").unwrap();
    let r = d.new_element("rect");
    d.set_attr(r, "id", "r");
    d.replace(p, r);
    assert_eq!(d.parent(p), None);
    assert_eq!(d.first_child(d.by_id("g").unwrap()), Some(r));
    assert_eq!(d.by_id("p"), None);
    assert!(
        out(&d).contains("<g id=\"g\"><rect id=\"r\"/>tail<use"),
        "{}",
        out(&d)
    );
}

#[test]
fn deep_clone_copies_subtree_without_ids() {
    let mut d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let c = d.deep_clone(g);
    assert_eq!(d.parent(c), None);
    assert_eq!(d.attr(c, "id"), None);
    let kinds: Vec<String> = d
        .descendants(c)
        .map(|n| {
            if d.is_element(n) {
                d.tag(n).to_string()
            } else {
                format!("#{}", d.text(n).unwrap_or(""))
            }
        })
        .collect();
    assert_eq!(kinds, vec!["g", "path", "#tail", "use"]);
    assert_eq!(d.attr(d.first_child(c).unwrap(), "d"), Some("M 0,0"));
    assert!(d.by_id("g").is_some(), "original ids untouched");
    d.append_child(d.svg(), c);
    assert_eq!(d.children(d.svg()).count(), 5);
}

#[test]
fn ensure_id_generates_unique_deterministic_ids() {
    let mut d =
        parse("<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"sciink-1\"/><g/><g/></svg>");
    let kids: Vec<_> = d.children(d.svg()).collect();
    assert_eq!(d.ensure_id(kids[0]), "sciink-1");
    assert_eq!(d.ensure_id(kids[1]), "sciink-2");
    assert_eq!(d.ensure_id(kids[2]), "sciink-3");
    assert_eq!(d.by_id("sciink-3"), Some(kids[2]));
}

#[test]
fn set_text_and_new_nodes() {
    let mut d = parse(DOC);
    let t = d.by_id("t").unwrap();
    let txt = d.first_child(t).unwrap();
    assert!(d.is_text(txt));
    d.set_text(txt, "a < b");
    assert!(out(&d).contains("<text id=\"t\">a &lt; b</text>"));
    let c = d.new_comment(" note ");
    assert!(d.is_comment(c));
    d.append_child(t, c);
    let n = d.new_text("x");
    d.append_child(t, n);
    assert!(out(&d).contains("<text id=\"t\">a &lt; b<!-- note -->x</text>"));
}

#[test]
fn defs_is_found_or_created_first() {
    let mut d = parse(DOC);
    assert_eq!(d.defs(), d.by_id("d").unwrap());
    let mut e = parse("<svg xmlns=\"http://www.w3.org/2000/svg\"><g/></svg>");
    let defs = e.defs();
    assert_eq!(e.tag(defs), "defs");
    assert_eq!(e.first_child(e.svg()), Some(defs));
    assert_eq!(e.defs(), defs);
}

#[test]
fn selection_is_in_document_order_and_ignores_unknown_ids() {
    let d = parse(DOC);
    let sel = d.selection(&[
        "t".to_string(),
        "nope".to_string(),
        "p".to_string(),
        "g".to_string(),
    ]);
    let tags: Vec<&str> = sel.iter().map(|&n| d.tag(n)).collect();
    assert_eq!(tags, vec!["g", "path", "text"]);
}

#[test]
#[should_panic(expected = "own subtree")]
fn attaching_a_node_inside_its_own_subtree_panics() {
    let mut d = parse(DOC);
    let g = d.by_id("g").unwrap();
    // g has children, so prepend_child(g, g) resolves to insert_before(g, <first child of g>),
    // which would make g its own parent (and hang `descendants` walking a self-referential
    // node) if not caught.
    d.prepend_child(g, g);
}

#[test]
#[should_panic(expected = "own subtree")]
fn insert_before_its_own_descendant_panics() {
    let mut d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let p = d.by_id("p").unwrap();
    // p is a descendant of g; inserting g before p would make g its own ancestor.
    d.insert_before(g, p);
}

#[test]
fn mutations_bump_generation_and_style_changes_bump_sheet_generation() {
    let mut d = parse(DOC);
    let p = d.by_id("p").unwrap();
    let t = d.by_id("t").unwrap();
    let s = d.by_id("s").unwrap();
    let txt = d.first_child(t).unwrap();
    let style_txt = d.first_child(s).unwrap();

    let before = d.generation();
    d.set_attr(p, "stroke", "red");
    assert!(d.generation() > before, "set_attr must bump generation");
    assert_eq!(
        d.sheet_generation(),
        0,
        "set_attr on <path> must not bump sheet_generation"
    );

    let before = d.generation();
    d.remove_attr(p, "stroke");
    assert!(d.generation() > before, "remove_attr must bump generation");

    let before = d.generation();
    d.set_text(txt, "bye");
    assert!(d.generation() > before, "set_text must bump generation");

    let before = d.generation();
    d.detach(p);
    assert!(d.generation() > before, "detach must bump generation");

    let before = d.generation();
    d.append_child(t, p);
    assert!(d.generation() > before, "append_child must bump generation");

    assert_eq!(
        d.sheet_generation(),
        0,
        "no <style> mutation has happened yet"
    );

    let sheet = d.sheet_generation();
    d.set_text(style_txt, "g{fill:blue}");
    assert!(
        d.sheet_generation() > sheet,
        "set_text on <style>'s text must bump sheet_generation"
    );

    let sheet = d.sheet_generation();
    d.detach(s);
    assert!(
        d.sheet_generation() > sheet,
        "detaching <style> must bump sheet_generation"
    );
}

#[test]
fn xml_space_href_and_new_id_accessors() {
    let mut d = Doc::parse(
        br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <defs><path id="m1" d="M0,0"/></defs>
  <text id="t" xml:space="preserve"><tspan id="s"> a </tspan></text>
  <text id="u" xml:space="default"><tspan id="v">b</tspan></text>
  <use id="c" xlink:href="#m1"/><use id="e" href="#nope"/><use id="f" href="m1"/>
  <path id="FMArrowstart1" d="M0,0"/></svg>"##,
    )
    .unwrap();
    let id = |d: &Doc, i: &str| d.by_id(i).unwrap();
    assert!(d.xml_space_preserve(id(&d, "s")));
    assert!(d.xml_space_preserve(id(&d, "t")));
    assert!(!d.xml_space_preserve(id(&d, "v")));
    assert!(!d.xml_space_preserve(id(&d, "m1")));
    assert_eq!(d.resolve_href(id(&d, "c")), Some(id(&d, "m1")));
    assert_eq!(d.resolve_href(id(&d, "e")), None);
    assert_eq!(
        d.resolve_href(id(&d, "f")),
        None,
        "only fragment hrefs resolve"
    );
    assert_eq!(d.new_id("FMArrowstart"), "FMArrowstart2");
    assert_eq!(d.new_id("sciink-x"), "sciink-x1");
    // new_id does not reserve: the same answer twice until something takes it
    assert_eq!(d.new_id("FMArrowstart"), "FMArrowstart2");
    let n = d.new_element("path");
    d.set_attr(n, "id", "FMArrowstart2");
    d.append_child(d.svg(), n);
    assert_eq!(d.new_id("FMArrowstart"), "FMArrowstart3");
}

#[test]
fn set_tag_keeps_the_prefix_and_invalidates_styles() {
    let mut d = Doc::parse(
        br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:svg="http://www.w3.org/2000/svg"><style>path{fill:red}</style><svg:line id="l" x1="0"/><rect id="r"/></svg>"#,
    )
    .unwrap();
    let l = d.by_id("l").unwrap();
    let r = d.by_id("r").unwrap();
    assert_eq!(d.specified(l, "fill"), None);
    d.set_tag(l, "path");
    assert_eq!(d.qname(l), "svg:path");
    assert_eq!(d.tag(l), "path");
    assert_eq!(
        d.specified(l, "fill").as_deref(),
        Some("red"),
        "tag selectors re-match"
    );
    d.set_tag(r, "path");
    assert_eq!(d.qname(r), "path");
    assert_eq!(d.by_id("l"), Some(l), "ids survive a rename");
    let mut v = Vec::new();
    d.write(&mut v);
    let s = String::from_utf8(v).unwrap();
    assert!(
        s.contains(r#"<svg:path id="l" x1="0"/>"#) && s.contains(r#"<path id="r"/>"#),
        "{s}"
    );
}

#[test]
fn set_tag_to_or_from_style_rebuilds_the_stylesheet() {
    let mut d = Doc::parse(
        br#"<svg xmlns="http://www.w3.org/2000/svg"><foo id="s">rect{fill:red}</foo><rect id="r"/></svg>"#,
    )
    .unwrap();
    let (s, r) = (d.by_id("s").unwrap(), d.by_id("r").unwrap());
    assert_eq!(d.specified(r, "fill"), None);
    d.set_tag(s, "style");
    assert_eq!(d.specified(r, "fill").as_deref(), Some("red"));
    d.set_tag(s, "foo");
    assert_eq!(d.specified(r, "fill"), None);
}

#[test]
fn comment_returns_the_comment_text() {
    let d =
        Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg"><!-- Text --><g id="g"/></svg>"#)
            .unwrap();
    let c = d.children(d.svg()).find(|&n| d.is_comment(n)).unwrap();
    assert_eq!(d.comment(c), Some(" Text "));
    assert_eq!(d.comment(d.by_id("g").unwrap()), None);
}

#[test]
fn selection_ordered_keeps_argument_order_and_drops_unknown_and_repeated_ids() {
    let d = sciink::dom::Doc::parse(
        br#"<svg xmlns="http://www.w3.org/2000/svg"><rect id="a"/><rect id="b"/><rect id="c"/></svg>"#,
    )
    .unwrap();
    let ids: Vec<String> = ["c", "nope", "a", "c"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let sel = d.selection_ordered(&ids);
    assert_eq!(sel, vec![d.by_id("c").unwrap(), d.by_id("a").unwrap()]);
    // the document-order variant is unchanged
    assert_eq!(
        d.selection(&ids),
        vec![d.by_id("a").unwrap(), d.by_id("c").unwrap()]
    );
}

#[test]
fn ensure_prefix_declares_known_prefixes_once_and_refuses_unknown_ones() {
    let mut d =
        sciink::dom::Doc::parse(br#"<svg xmlns="http://www.w3.org/2000/svg"><rect id="r"/></svg>"#)
            .unwrap();
    assert!(d.ensure_prefix("xml"));
    assert!(d.ensure_prefix("sodipodi"));
    let svg = d.svg();
    assert_eq!(
        d.attr(svg, "xmlns:sodipodi"),
        Some("http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd")
    );
    assert!(d.ensure_prefix("sodipodi"), "already declared");
    assert_eq!(
        d.attrs(svg)
            .iter()
            .filter(|a| a.name == "xmlns:sodipodi")
            .count(),
        1
    );
    assert!(!d.ensure_prefix("foo"));
    assert_eq!(d.attr(svg, "xmlns:foo"), None);
}

/// The byte-at-a-time escapers this plan replaces, kept as the reference.
fn escape_text_ref(s: &str, out: &mut Vec<u8>) {
    for b in s.bytes() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            _ => out.push(b),
        }
    }
}
fn escape_attr_ref(s: &str, out: &mut Vec<u8>) {
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

/// Every string of length ≤ 4 over the special bytes plus three ordinary chars, as an
/// attribute value and as text: the document must serialise exactly as the reference escapers say.
#[test]
fn escaping_is_unchanged_for_every_special_byte_position() {
    let alphabet: Vec<&str> = vec!["&", "<", ">", "\"", "\n", "\r", "\t", "a", "é", "𝄞"];
    let mut cases: Vec<String> = vec![String::new()];
    for len in 1..=4 {
        let mut next = Vec::new();
        for c in &cases {
            if c.chars().count() == len - 1 {
                for a in &alphabet {
                    next.push(format!("{c}{a}"));
                }
            }
        }
        cases.extend(next);
    }
    for s in &cases {
        // attribute: build the document from the escaped reference form so parse() sees the value `s`
        let mut esc_attr = Vec::new();
        escape_attr_ref(s, &mut esc_attr);
        let mut esc_text = Vec::new();
        // XML parsers may normalise a raw carriage return in text content, so that byte is only
        // exercised inside the attribute (where it is written as &#13;)
        if !s.contains('\r') {
            escape_text_ref(s, &mut esc_text);
        }
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"g\" data-v=\"{}\">{}</g></svg>",
            String::from_utf8(esc_attr).unwrap(),
            String::from_utf8(esc_text).unwrap()
        );
        let doc = Doc::parse(svg.as_bytes()).unwrap();
        let g = doc.by_id("g").unwrap();
        assert_eq!(
            doc.attr(g, "data-v"),
            Some(s.as_str()),
            "value round trip for {s:?}"
        );
        let mut out = Vec::new();
        doc.write(&mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            svg,
            "serialisation for {s:?}"
        );
    }
}

#[test]
fn a_large_attribute_round_trips_byte_for_byte() {
    let payload: String = (0..4_000_000u32)
        .map(|i| {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"[(i % 64) as usize]
                as char
        })
        .collect();
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><image id=\"i\" href=\"data:image/png;base64,{payload}\"/></svg>"
    );
    let doc = Doc::parse(svg.as_bytes()).unwrap();
    let mut out = Vec::new();
    doc.write(&mut out);
    assert_eq!(out.len(), svg.len());
    assert!(out == svg.as_bytes(), "large attribute changed");
}

#[test]
fn setting_an_attribute_to_its_current_value_bumps_no_generation() {
    let mut doc = Doc::parse(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"r\" style=\"fill:red\" width=\"3\"/></svg>",
    )
    .unwrap();
    let r = doc.by_id("r").unwrap();
    let (g0, s0) = (doc.generation(), doc.style_generation());
    doc.set_attr(r, "style", "fill:red");
    doc.set_attr(r, "width", "3");
    doc.set_attr(r, "id", "r");
    assert_eq!(
        doc.generation(),
        g0,
        "identical writes must not bump generation"
    );
    assert_eq!(
        doc.style_generation(),
        s0,
        "identical writes must not bump style_generation"
    );
    doc.set_attr(r, "style", "fill:blue");
    assert!(doc.generation() > g0 && doc.style_generation() > s0);
}

#[test]
fn a_duplicate_id_keeps_pointing_at_the_first_node() {
    let mut doc = Doc::parse(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"dup\" width=\"1\"/><rect id=\"dup\" width=\"2\"/></svg>",
    )
    .unwrap();
    let first = doc.by_id("dup").unwrap();
    assert_eq!(doc.attr(first, "width"), Some("1"));
    let g0 = doc.generation();
    doc.set_attr(first, "id", "dup");
    assert_eq!(doc.by_id("dup"), Some(first));
    assert_eq!(doc.generation(), g0);
}

const MOVE_DOC: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"a\"><g id=\"b\"><rect id=\"c\"/><rect id=\"d\"/></g></g><g id=\"z\"/></svg>";

#[test]
fn moving_a_subtree_keeps_every_id_resolvable() {
    let mut doc = Doc::parse(MOVE_DOC.as_bytes()).unwrap();
    let (b, z) = (doc.by_id("b").unwrap(), doc.by_id("z").unwrap());
    let before: Vec<_> = ["a", "b", "c", "d", "z"]
        .iter()
        .map(|i| doc.by_id(i).unwrap())
        .collect();
    doc.append_child(z, b);
    let after: Vec<_> = ["a", "b", "c", "d", "z"]
        .iter()
        .map(|i| doc.by_id(i).unwrap())
        .collect();
    assert_eq!(before, after);
    assert_eq!(doc.parent(b), Some(z));
    doc.insert_before(b, doc.by_id("a").unwrap());
    let again: Vec<_> = ["a", "b", "c", "d", "z"]
        .iter()
        .map(|i| doc.by_id(i).unwrap())
        .collect();
    assert_eq!(before, again);
}

#[test]
fn moving_a_subtree_bumps_each_generation_exactly_once() {
    let mut doc = Doc::parse(MOVE_DOC.as_bytes()).unwrap();
    let (b, z) = (doc.by_id("b").unwrap(), doc.by_id("z").unwrap());
    let (g, s, sh) = (
        doc.generation(),
        doc.style_generation(),
        doc.sheet_generation(),
    );
    doc.append_child(z, b);
    assert_eq!(doc.generation(), g + 1);
    assert_eq!(doc.style_generation(), s + 1);
    assert_eq!(doc.sheet_generation(), sh, "no <style> moved");
    let mut with_style = Doc::parse(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"a\"><style id=\"s\">*{fill:red}</style></g><g id=\"z\"/></svg>".as_bytes(),
    )
    .unwrap();
    let (a, z) = (
        with_style.by_id("a").unwrap(),
        with_style.by_id("z").unwrap(),
    );
    let sh = with_style.sheet_generation();
    with_style.append_child(z, a);
    assert!(
        with_style.sheet_generation() > sh,
        "a moved <style> re-orders the sheet"
    );
}

#[test]
fn detaching_then_reattaching_a_subtree_restores_the_id_index() {
    let mut doc = Doc::parse(MOVE_DOC.as_bytes()).unwrap();
    let (b, z) = (doc.by_id("b").unwrap(), doc.by_id("z").unwrap());
    doc.detach(b);
    assert_eq!(doc.by_id("b"), None);
    assert_eq!(doc.by_id("c"), None);
    doc.append_child(z, b);
    assert_eq!(doc.by_id("b"), Some(b));
    assert_eq!(doc.by_id("c").map(|c| doc.parent(c)), Some(Some(b)));
}

#[test]
fn a_duplicate_id_keeps_pointing_at_the_first_node_after_a_move() {
    let mut doc = Doc::parse(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"first\"><rect id=\"dup\" width=\"1\"/></g><g id=\"second\"><rect id=\"dup\" width=\"2\"/></g><g id=\"z\"/></svg>".as_bytes(),
    )
    .unwrap();
    let first_dup = doc.by_id("dup").unwrap();
    assert_eq!(doc.attr(first_dup, "width"), Some("1"));
    let (second, z) = (doc.by_id("second").unwrap(), doc.by_id("z").unwrap());
    doc.append_child(z, second);
    assert_eq!(
        doc.by_id("dup"),
        Some(first_dup),
        "moving the shadowed node changes nothing"
    );
    let first = doc.by_id("first").unwrap();
    doc.append_child(z, first);
    assert_eq!(
        doc.by_id("dup"),
        Some(first_dup),
        "moving the indexed node keeps it indexed"
    );
}
