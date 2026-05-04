#![deny(unsafe_code)]
#![allow(clippy::needless_pass_by_value)]

//! Stable HTML serializer for normalized Sylphos DOM trees.

use crate::dom::{Document, Element, Node};

/// Serializes a document into deterministic normalized HTML.
#[must_use]
pub fn serialize_document(doc: &Document) -> String {
    let mut out = String::new();

    if let Some(doctype) = &doc.doctype {
        out.push_str("<!DOCTYPE ");
        out.push_str(&doctype.trim().to_ascii_lowercase());
        out.push('>');
    }

    for node in &doc.children {
        serialize_node(node, &mut out, None);
    }

    out
}

fn serialize_node(node: &Node, out: &mut String, parent_tag: Option<&str>) {
    match node {
        Node::Text(text) => {
            if matches!(
                parent_tag,
                Some("script" | "style" | "xmp" | "iframe" | "noembed" | "noframes" | "plaintext")
            ) {
                out.push_str(text);
            } else {
                out.push_str(&escape_text(text));
            }
        }
        Node::Comment(comment) => {
            out.push_str("<!--");
            out.push_str(&sanitize_comment(comment));
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
            if key.trim().is_empty() {
                continue;
            }
            out.push(' ');
            out.push_str(&key.to_ascii_lowercase());

            if value.is_empty() && is_boolean_attribute(&key) {
                continue;
            }

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
        serialize_node(child, out, Some(&element.tag));
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

fn is_boolean_attribute(name: &str) -> bool {
    matches!(
        name,
        "allowfullscreen"
            | "async"
            | "autofocus"
            | "autoplay"
            | "checked"
            | "controls"
            | "defer"
            | "disabled"
            | "hidden"
            | "loop"
            | "multiple"
            | "muted"
            | "open"
            | "readonly"
            | "required"
            | "selected"
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
            '\'' => out.push_str("&#39;"),
            '\u{00A0}' => out.push_str("&nbsp;"),
            _ => out.push(ch),
        }
    }

    out
}

fn sanitize_comment(comment: &str) -> String {
    comment.replace("--", "- -")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use insta::assert_snapshot;

    fn parse_document(html: &str) -> Document {
        parse(html).unwrap_or_else(|error| panic!("test HTML failed to parse: {error}"))
    }

    #[test]
    fn serializes_normalized_shell() {
        let out = serialize_document(&parse_document("<title>x</title><p>Hello"));
        assert_snapshot!(out, @r###"<!DOCTYPE html><html><head><title>x</title></head><body><p>Hello</p></body></html>"###);
    }

    #[test]
    fn raw_script_text_is_not_escaped() {
        let out = serialize_document(&parse_document("<script>if (a < b) { x = '&'; }</script>"));
        assert!(out.contains("if (a < b)"));
    }

    #[test]
    fn attrs_and_entities_are_stable() {
        let html = r#"<a href=/x?q=1&y='2' title='a "b" &amp; c' data-k=v>lt:&lt; > &amp;</a>"#;
        let out = serialize_document(&parse_document(html));
        assert!(out.contains("data-k=\"v\""));
        assert!(out.contains("lt:&lt; &gt; &amp;"));
    }
}
