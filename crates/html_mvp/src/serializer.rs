#![allow(clippy::needless_pass_by_value)]

use crate::dom::{Document, Element, Node};

pub fn serialize_document(doc: &Document) -> String {
    let mut out = String::new();

    if let Some(doctype) = &doc.doctype {
        out.push_str("<!DOCTYPE ");
        out.push_str(doctype);
        out.push('>');
    }

    for node in &doc.children {
        serialize_node(node, &mut out);
    }

    out
}

fn serialize_node(node: &Node, out: &mut String) {
    match node {
        Node::Text(text) => out.push_str(&escape_text(text)),
        Node::Comment(comment) => {
            out.push_str("<!--");
            out.push_str(comment);
            out.push_str("-->");
        }
        Node::Element(element) => serialize_element(element, out),
    }
}

fn serialize_element(element: &Element, out: &mut String) {
    out.push('<');
    out.push_str(&element.tag);

    if !element.attrs.is_empty() {
        let mut attrs = element.attrs.clone();
        attrs.sort_by(|left, right| left.0.cmp(&right.0));

        for (key, value) in attrs {
            out.push(' ');
            out.push_str(&key);
            out.push('=');
            out.push('"');
            out.push_str(&escape_attr(&value));
            out.push('"');
        }
    }

    out.push('>');

    if is_void_element(&element.tag) {
        return;
    }

    for child in &element.children {
        serialize_node(child, out);
    }

    out.push_str("</");
    out.push_str(&element.tag);
    out.push('>');
}

fn is_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }

    out
}

fn escape_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push('\''),
            _ => out.push(ch),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use insta::assert_snapshot;

    fn parse_document(html: &str) -> Document {
        match parse(html) {
            Ok(document) => document,
            Err(error) => panic!("test HTML failed to parse: {error}"),
        }
    }

    #[test]
    fn comment_and_text() {
        let html = "<!-- hi --><div>ok<!--x--></div>";
        let doc = parse_document(html);
        let out = serialize_document(&doc);

        assert_snapshot!(out, @r###"<!-- hi --><div>ok<!--x--></div>"###);
    }

    #[test]
    fn void_elements() {
        let html = "<div>a<br><img></div>";
        let doc = parse_document(html);
        let out = serialize_document(&doc);

        assert_snapshot!(out, @r###"<div>a<br><img></div>"###);
    }

    #[test]
    fn attrs_and_entities() {
        let html = r#"<a href=/x?q=1&y='2' title='a "b" &amp; c' data-k=v>lt:&lt; > &amp;</a>"#;
        let doc = parse_document(html);
        let out = serialize_document(&doc);

        assert_snapshot!(out, @r###"<a data-k="v" href="/x?q=1&amp;y='2'" title="a &quot;b&quot; &amp; c">lt:&lt; &gt; &amp;</a>"###);
    }

    #[test]
    fn doctype_normalization() {
        let html = "<!DOCTYPE HTML><html><body>x</body></html>";
        let doc = parse_document(html);
        let out = serialize_document(&doc);

        assert_snapshot!(out, @r###"<!DOCTYPE html><html><body>x</body></html>"###);
    }
}
