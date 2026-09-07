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
    const KNOWN_DIFFERENT: &[&str] = &[
        // Two <tspan> text nodes (byte offsets ~124248 and ~124368 in the source)
        // spell a literal `"` as `&quot;` inside element text, e.g.
        // `>Check with &quot;Insert SVG 1.1 </tspan>`. `"` needs no escaping in
        // XML text content (only in attribute values), so we decode it and
        // canonically re-emit the literal character; the source's redundant
        // escape choice for that one character is the only difference in the
        // whole 155KB file.
        "Flow_tests.svg",
    ];
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
