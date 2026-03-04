#![deny(unsafe_code)]

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct Document {
    pub doctype: Option<String>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
    Comment(String),
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Element {
    pub fn new<T: Into<String>>(tag: T) -> Self {
        Self {
            tag: tag.into().to_ascii_lowercase(),
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn attr<K: Into<String>, V: Into<String>>(mut self, k: K, v: V) -> Self {
        self.attrs.push((k.into(), v.into()));
        self
    }

    pub fn push(&mut self, node: Node) {
        self.children.push(node);
    }
}

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
    for (na, nb) in a.iter().zip(b.iter()) {
        if !node_eq(na, nb) {
            return false;
        }
    }
    true
}

fn node_eq(a: &Node, b: &Node) -> bool {
    match (a, b) {
        (Node::Text(ta), Node::Text(tb)) => ta == tb,
        (Node::Comment(ca), Node::Comment(cb)) => ca == cb,
        (Node::Element(ea), Node::Element(eb)) => {
            if ea.tag != eb.tag {
                return false;
            }
            if !attrs_eq(&ea.attrs, &eb.attrs) {
                return false;
            }
            nodes_eq(&ea.children, &eb.children)
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

fn norm(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}
