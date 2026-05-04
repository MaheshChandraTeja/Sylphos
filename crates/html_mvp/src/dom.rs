#![deny(unsafe_code)]

//! DOM structures for the `html_mvp` crate.
//!
//! Module 48 keeps the public DOM deliberately compact so existing Sylphos
//! presentation and script-extraction code does not have to be rewritten. The
//! upgrade lives in better tree construction and normalization, not in turning
//! this crate into a full browser DOM clone. Humanity has suffered enough.

use std::collections::BTreeMap;

/// Parsed HTML document.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// Normalized document type. Usually `html`.
    pub doctype: Option<String>,

    /// Top-level nodes. After Module 48 normalization this is usually one
    /// `<html>` element, with comments preserved where possible.
    pub children: Vec<Node>,
}

/// HTML element node.
#[derive(Debug, Clone)]
pub struct Element {
    /// Lowercase tag name.
    pub tag: String,

    /// Lowercase attributes in source order, with duplicate attributes folded
    /// by the normalizer using first-wins semantics.
    pub attrs: Vec<(String, String)>,

    /// Child nodes.
    pub children: Vec<Node>,
}

/// HTML node.
#[derive(Debug, Clone)]
pub enum Node {
    /// Element node.
    Element(Element),

    /// Text node.
    Text(String),

    /// Comment node.
    Comment(String),
}

impl Document {
    /// Creates an empty document.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the first element with the requested tag.
    #[must_use]
    pub fn first_element_by_tag(&self, tag: &str) -> Option<&Element> {
        self.children
            .iter()
            .find_map(|node| node.first_element_by_tag(tag))
    }

    /// Returns all visible text content in document order.
    #[must_use]
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            child.append_text_content(&mut out);
        }
        out
    }
}

impl Element {
    /// Creates an element with a normalized lowercase tag name.
    #[must_use]
    pub fn new<T: Into<String>>(tag: T) -> Self {
        Self {
            tag: normalize_name(tag.into()),
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Adds an attribute and returns the element.
    #[must_use]
    pub fn attr<K: Into<String>, V: Into<String>>(mut self, k: K, v: V) -> Self {
        self.attrs.push((normalize_name(k.into()), v.into()));
        self
    }

    /// Appends one child node.
    pub fn push(&mut self, node: Node) {
        self.children.push(node);
    }

    /// Returns an attribute value using case-insensitive name matching.
    #[must_use]
    pub fn get_attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
    }

    /// Returns true if this element has an attribute.
    #[must_use]
    pub fn has_attr(&self, name: &str) -> bool {
        self.get_attr(name).is_some()
    }

    /// Sets or inserts an attribute.
    pub fn set_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = normalize_name(name.into());
        let value = value.into();

        if let Some((_, existing)) = self.attrs.iter_mut().find(|(key, _)| *key == name) {
            *existing = value;
        } else {
            self.attrs.push((name, value));
        }
    }

    /// Returns the first descendant element with the requested tag, including self.
    #[must_use]
    pub fn first_element_by_tag(&self, tag: &str) -> Option<&Element> {
        if self.tag.eq_ignore_ascii_case(tag) {
            return Some(self);
        }

        self.children
            .iter()
            .find_map(|node| node.first_element_by_tag(tag))
    }

    /// Returns text content for this element.
    #[must_use]
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.append_text_content(&mut out);
        out
    }

    fn append_text_content(&self, out: &mut String) {
        for child in &self.children {
            child.append_text_content(out);
        }
    }
}

impl Node {
    /// Returns the first descendant element with the requested tag, including self.
    #[must_use]
    pub fn first_element_by_tag(&self, tag: &str) -> Option<&Element> {
        match self {
            Self::Element(element) => element.first_element_by_tag(tag),
            Self::Text(_) | Self::Comment(_) => None,
        }
    }

    /// Returns true if this node is whitespace-only text.
    #[must_use]
    pub fn is_whitespace_text(&self) -> bool {
        matches!(self, Self::Text(text) if text.trim().is_empty())
    }

    fn append_text_content(&self, out: &mut String) {
        match self {
            Self::Text(text) => out.push_str(text),
            Self::Comment(_) => {}
            Self::Element(element) => {
                if matches!(element.tag.as_str(), "script" | "style" | "template") {
                    return;
                }
                element.append_text_content(out);
            }
        }
    }
}

/// Structural DOM equality used by parser round-trip tests.
#[must_use]
pub fn dom_eq(a: &Document, b: &Document) -> bool {
    if a.doctype.as_deref().map(norm) != b.doctype.as_deref().map(norm) {
        return false;
    }
    nodes_eq(&a.children, &b.children)
}

fn nodes_eq(a: &[Node], b: &[Node]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(left, right)| node_eq(left, right))
}

fn node_eq(a: &Node, b: &Node) -> bool {
    match (a, b) {
        (Node::Text(ta), Node::Text(tb)) => ta == tb,
        (Node::Comment(ca), Node::Comment(cb)) => ca == cb,
        (Node::Element(ea), Node::Element(eb)) => {
            ea.tag == eb.tag
                && attrs_eq(&ea.attrs, &eb.attrs)
                && nodes_eq(&ea.children, &eb.children)
        }
        _ => false,
    }
}

fn attrs_eq(a: &[(String, String)], b: &[(String, String)]) -> bool {
    let to_map = |v: &[(String, String)]| -> BTreeMap<String, String> {
        v.iter().map(|(k, val)| (k.clone(), val.clone())).collect()
    };
    to_map(a) == to_map(b)
}

fn normalize_name(input: String) -> String {
    input.trim().to_ascii_lowercase()
}

fn norm(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}
