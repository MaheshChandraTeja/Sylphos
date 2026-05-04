use crate::{parse, parse_fragment, parse_with_options, serialize_document, ParseOptions};

#[test]
fn inserts_html_head_body_around_loose_nodes() {
    let document = parse("<title>Sylphos</title><h1>Hello</h1>").expect("parse");
    let html = serialize_document(&document);

    assert!(html.contains("<html>"));
    assert!(html.contains("<head><title>Sylphos</title></head>"));
    assert!(html.contains("<body><h1>Hello</h1></body>"));
}

#[test]
fn script_and_style_are_raw_text() {
    let document = parse(
        "<script>if (a < b) { document.write('<x>'); }</script><style>a > b { color:red }</style>",
    )
    .expect("parse");
    let serialized = serialize_document(&document);

    assert!(serialized.contains("if (a < b)"));
    assert!(serialized.contains("document.write('<x>')"));
    assert!(serialized.contains("a > b { color:red }"));
}

#[test]
fn textarea_and_title_decode_entities_but_not_tags() {
    let document = parse("<title>A &amp; B</title><textarea>1 &lt; 2</textarea>").expect("parse");
    let serialized = serialize_document(&document);

    assert!(serialized.contains("<title>A &amp; B</title>"));
    assert!(serialized.contains("<textarea>1 &lt; 2</textarea>"));
}

#[test]
fn repairs_unclosed_paragraphs_and_list_items() {
    let document = parse("<p>one<p>two<ul><li>a<li>b</ul>").expect("parse");
    let serialized = serialize_document(&document);

    assert!(serialized.contains("<p>one</p><p>two</p>"));
    assert!(serialized.contains("<li>a</li><li>b</li>"));
}

#[test]
fn reports_normalization_activity() {
    let output =
        parse_with_options("<meta charset=utf-8><p>Hello", ParseOptions::default()).expect("parse");

    assert!(output.normalization.inserted_html);
    assert!(output.normalization.inserted_head);
    assert!(output.normalization.inserted_body);
    assert!(output.token_count >= 2);
}

#[test]
fn parses_fragments_under_context() {
    let nodes = parse_fragment("<li>A<li>B", "ul").expect("fragment");
    assert!(!nodes.is_empty());
}
