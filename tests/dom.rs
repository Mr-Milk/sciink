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
