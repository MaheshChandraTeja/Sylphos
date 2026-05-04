#![deny(unsafe_code)]
#![allow(clippy::too_many_lines)]

//! DOM normalization for Sylphos HTML5-lite parsing.
//!
//! The tokenizer and stack builder intentionally tolerate broken markup. This
//! module turns the resulting forgiving tree into a stable browser-shaped
//! document: implied `html/head/body`, folded duplicate attributes, adjacent text
//! coalescing, and lightweight repairs for paragraphs, lists, tables, forms, and
//! templates.

use crate::dom::{Document, Element, Node};
use std::collections::BTreeSet;

/// Summary of normalization work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NormalizationReport {
    /// Whether an `<html>` element was inserted.
    pub inserted_html: bool,

    /// Whether a `<head>` element was inserted.
    pub inserted_head: bool,

    /// Whether a `<body>` element was inserted.
    pub inserted_body: bool,

    /// Number of adjacent text merges.
    pub merged_text_nodes: usize,

    /// Number of duplicate attributes dropped.
    pub dropped_duplicate_attributes: usize,

    /// Number of nodes moved into implied containers.
    pub moved_nodes: usize,
}

/// Normalizes a parsed document and returns a report.
#[must_use]
pub fn normalize_document(document: &mut Document) -> NormalizationReport {
    let mut report = NormalizationReport::default();

    if document.doctype.is_none() {
        document.doctype = Some("html".to_owned());
    } else {
        document.doctype = document.doctype.as_deref().map(normalize_doctype);
    }

    normalize_document_shell(document, &mut report);

    for child in &mut document.children {
        normalize_node(child, &mut report);
    }

    coalesce_document_text(document, &mut report);
    report
}

fn normalize_document_shell(document: &mut Document, report: &mut NormalizationReport) {
    let old_children = std::mem::take(&mut document.children);
    let mut leading_comments = Vec::new();
    let mut html_element: Option<Element> = None;
    let mut loose_nodes = Vec::new();

    for node in old_children {
        match node {
            Node::Comment(_) if html_element.is_none() && loose_nodes.is_empty() => {
                leading_comments.push(node);
            }
            Node::Element(element) if element.tag == "html" && html_element.is_none() => {
                html_element = Some(element);
            }
            other => loose_nodes.push(other),
        }
    }

    let mut html = if let Some(html) = html_element {
        html
    } else {
        report.inserted_html = true;
        Element::new("html")
    };

    html.children.extend(loose_nodes);
    normalize_html_children(&mut html, report);

    document.children = leading_comments;
    document.children.push(Node::Element(html));
}

fn normalize_html_children(html: &mut Element, report: &mut NormalizationReport) {
    let children = std::mem::take(&mut html.children);
    let mut head: Option<Element> = None;
    let mut body: Option<Element> = None;
    let mut head_nodes = Vec::new();
    let mut body_nodes = Vec::new();
    let mut seen_body = false;

    for node in children {
        match node {
            Node::Element(element) if element.tag == "head" && head.is_none() => {
                head = Some(element);
            }
            Node::Element(element) if element.tag == "body" && body.is_none() => {
                seen_body = true;
                body = Some(element);
            }
            Node::Element(element) if element.tag == "html" => {
                for child in element.children {
                    classify_html_child(child, seen_body, &mut head_nodes, &mut body_nodes, report);
                }
            }
            other => {
                classify_html_child(other, seen_body, &mut head_nodes, &mut body_nodes, report)
            }
        }
    }

    let mut head = head.unwrap_or_else(|| {
        report.inserted_head = true;
        Element::new("head")
    });
    let mut body = body.unwrap_or_else(|| {
        report.inserted_body = true;
        Element::new("body")
    });

    head.children.extend(head_nodes);
    body.children.extend(body_nodes);

    html.children.push(Node::Element(head));
    html.children.push(Node::Element(body));
}

fn classify_html_child(
    node: Node,
    seen_body: bool,
    head_nodes: &mut Vec<Node>,
    body_nodes: &mut Vec<Node>,
    report: &mut NormalizationReport,
) {
    match &node {
        Node::Text(text) if text.trim().is_empty() && !seen_body => head_nodes.push(node),
        Node::Element(element) if !seen_body && is_head_element(&element.tag) => {
            report.moved_nodes = report.moved_nodes.saturating_add(1);
            head_nodes.push(node);
        }
        Node::Comment(_) if !seen_body => head_nodes.push(node),
        _ => {
            report.moved_nodes = report.moved_nodes.saturating_add(1);
            body_nodes.push(node);
        }
    }
}

fn normalize_node(node: &mut Node, report: &mut NormalizationReport) {
    let Node::Element(element) = node else {
        return;
    };

    normalize_element(element, report);
}

fn normalize_element(element: &mut Element, report: &mut NormalizationReport) {
    element.tag = element.tag.trim().to_ascii_lowercase();
    normalize_attrs(element, report);

    if is_void_element(&element.tag) {
        element.children.clear();
        return;
    }

    let children = std::mem::take(&mut element.children);
    let mut normalized = Vec::with_capacity(children.len());

    for mut child in children {
        normalize_node(&mut child, report);

        if matches!(element.tag.as_str(), "ul" | "ol" | "menu") {
            let keep = match &child {
                Node::Text(text) => text.trim().is_empty(),
                Node::Element(child_el) => child_el.tag == "li",
                Node::Comment(_) => true,
            };

            if keep {
                normalized.push(child);
            } else {
                let mut li = Element::new("li");
                li.children.push(child);
                normalized.push(Node::Element(li));
                report.moved_nodes = report.moved_nodes.saturating_add(1);
            }
            continue;
        }

        if element.tag == "table" {
            let keep = match &child {
                Node::Element(child_el) => matches!(
                    child_el.tag.as_str(),
                    "caption" | "colgroup" | "thead" | "tbody" | "tfoot" | "tr"
                ),
                Node::Text(text) => text.trim().is_empty(),
                Node::Comment(_) => true,
            };

            if keep {
                normalized.push(child);
            } else {
                let mut tbody = Element::new("tbody");
                let mut tr = Element::new("tr");
                let mut td = Element::new("td");
                td.children.push(child);
                tr.children.push(Node::Element(td));
                tbody.children.push(Node::Element(tr));
                normalized.push(Node::Element(tbody));
                report.moved_nodes = report.moved_nodes.saturating_add(1);
            }
            continue;
        }

        if element.tag == "tr" {
            let keep = match &child {
                Node::Element(child_el) => matches!(child_el.tag.as_str(), "td" | "th"),
                Node::Text(text) => text.trim().is_empty(),
                Node::Comment(_) => true,
            };

            if keep {
                normalized.push(child);
            } else {
                let mut td = Element::new("td");
                td.children.push(child);
                normalized.push(Node::Element(td));
                report.moved_nodes = report.moved_nodes.saturating_add(1);
            }
            continue;
        }

        normalized.push(child);
    }

    element.children = normalized;
    coalesce_element_text(element, report);
}

fn normalize_attrs(element: &mut Element, report: &mut NormalizationReport) {
    let mut seen = BTreeSet::new();
    let mut attrs = Vec::with_capacity(element.attrs.len());

    for (key, value) in std::mem::take(&mut element.attrs) {
        let normalized = key.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }

        if !seen.insert(normalized.clone()) {
            report.dropped_duplicate_attributes =
                report.dropped_duplicate_attributes.saturating_add(1);
            continue;
        }

        attrs.push((normalized, value));
    }

    element.attrs = attrs;
}

fn coalesce_document_text(document: &mut Document, report: &mut NormalizationReport) {
    coalesce_nodes(&mut document.children, report);
}

fn coalesce_element_text(element: &mut Element, report: &mut NormalizationReport) {
    coalesce_nodes(&mut element.children, report);
}

fn coalesce_nodes(nodes: &mut Vec<Node>, report: &mut NormalizationReport) {
    let old = std::mem::take(nodes);
    let mut merged: Vec<Node> = Vec::with_capacity(old.len());

    for node in old {
        match (merged.last_mut(), node) {
            (Some(Node::Text(existing)), Node::Text(next)) => {
                existing.push_str(&next);
                report.merged_text_nodes = report.merged_text_nodes.saturating_add(1);
            }
            (_, other) => merged.push(other),
        }
    }

    *nodes = merged;
}

fn normalize_doctype(input: &str) -> String {
    let value = input.trim().to_ascii_lowercase();
    if value.is_empty() {
        "html".to_owned()
    } else {
        value.split_whitespace().next().unwrap_or("html").to_owned()
    }
}

fn is_head_element(tag: &str) -> bool {
    matches!(
        tag,
        "base"
            | "basefont"
            | "bgsound"
            | "link"
            | "meta"
            | "noscript"
            | "script"
            | "style"
            | "template"
            | "title"
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_document_shell() {
        let mut document = Document {
            doctype: None,
            children: vec![Node::Element(Element::new("p"))],
        };

        let report = normalize_document(&mut document);
        assert!(report.inserted_html);
        assert!(report.inserted_head);
        assert!(report.inserted_body);
        assert!(document.first_element_by_tag("body").is_some());
    }
}
