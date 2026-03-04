#![allow(clippy::needless_pass_by_value)]

use crate::dom::{Document, Element, Node};

pub fn serialize_document(doc: &Document) -> String {
    let mut out = String::new();
    if let Some(dt) = &doc.doctype {
        out.push_str("<!DOCTYPE ");
        out.push_str(dt);
        out.push('>');
    }
    for n in &doc.children {
        serialize_node(n, &mut out);
    }
    out
}

fn serialize_node(n: &Node, out: &mut String) {
    match n {
        Node::Text(t) => out.push_str(&escape_text(t)),
        Node::Comment(c) => {
            out.push_str("<!--");
            out.push_str(c);
            out.push_str("-->");
        }
        Node::Element(el) => serialize_element(el, out),
    }
}

fn serialize_element(el: &Element, out: &mut String) {
    out.push('<');
    out.push_str(&el.tag);
    if !el.attrs.is_empty() {
        let mut attrs = el.attrs.clone();
        attrs.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in attrs {
            out.push(' ');
            out.push_str(&k);
            out.push('=');
            out.push('"');
            out.push_str(&escape_attr(&v));
            out.push('"');
        }
    }
    if is_void_element(&el.tag) {
        out.push('>');
        return;
    }
    out.push('>');
    for child in &el.children {
        serialize_node(child, out);
    }
    out.push_str("</");
    out.push_str(&el.tag);
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

fn escape_text(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    for ch in t.chars() {
        match ch {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            _ => s.push(ch),
        }
    }
    s
}

fn escape_attr(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    for ch in t.chars() {
        match ch {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&apos;"),
            _ => s.push(ch),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use insta::assert_snapshot;

    #[test]
    fn comment_and_text() {
        let html = "<!-- hi --><div>ok<!--x--></div>";
        let doc = parse(html).unwrap();
        let out = serialize_document(&doc);
        assert_snapshot!("comment_and_text", @r###"<!-- hi --><div>ok<!--x--></div>"###);
        assert_eq!(out, "<!-- hi --><div>ok<!--x--></div>");
    }

    #[test]
    fn void_elements() {
        let html = "<div>a<br><img></div>";
        let doc = parse(html).unwrap();
        let out = serialize_document(&doc);
        assert_snapshot!("voids", @r###"<div>a<br><img></div>"###);
        assert_eq!(out, "<div>a<br><img></div>");
    }

    #[test]
    fn attrs_and_entities() {
        let html = r#"<a href=/x?q=1&y='2' title='a "b" &amp; c' data-k=v>lt:&lt; > &amp;</a>"#;
        let doc = parse(html).unwrap();
        let out = serialize_document(&doc);
        assert_snapshot!("attrs_entities", @r###"<a data-k="v" href="/x?q=1&y='2'" title="a &quot;b&quot; &amp; c">lt:&lt; &gt; &amp;</a>"###);
        assert_eq!(
            out,
            r#"<a data-k="v" href="/x?q=1&y='2'" title="a &quot;b&quot; &amp; c">lt:&lt; &gt; &amp;</a>"#
        );
    }

    #[test]
    fn doctype_normalization() {
        let html = "<!DOCTYPE HTML><html><body>x</body></html>";
        let doc = parse(html).unwrap();
        let out = serialize_document(&doc);
        assert_snapshot!("doctype", @r###"<!DOCTYPE html><html><body>x</body></html>"###);
        assert_eq!(out, "<!DOCTYPE html><html><body>x</body></html>");
    }
}
