#![deny(unsafe_code)]
#![allow(clippy::too_many_lines)]

//! HTML5-lite tree builder.

use crate::dom::{Document, Element, Node};
use crate::normalizer::{normalize_document, NormalizationReport};
use crate::tokenizer::{Token, Tokenizer};
use anyhow::Result;

/// Parser options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Whether to normalize implied `html/head/body` and repair common tree cases.
    pub normalize: bool,

    /// Whether whitespace-only top-level text should be retained before normalization.
    pub retain_top_level_whitespace: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            normalize: true,
            retain_top_level_whitespace: false,
        }
    }
}

/// Detailed parse output.
#[derive(Debug, Clone)]
pub struct ParseOutput {
    /// Parsed document.
    pub document: Document,

    /// Normalization report.
    pub normalization: NormalizationReport,

    /// Token count emitted by the tokenizer.
    pub token_count: usize,
}

/// Parses input using default HTML5-lite normalization.
pub fn parse(input: &str) -> Result<Document> {
    Ok(parse_with_options(input, ParseOptions::default())?.document)
}

/// Parses input and returns diagnostics.
pub fn parse_with_options(input: &str, options: ParseOptions) -> Result<ParseOutput> {
    let tokens = Tokenizer::new(input).tokenize();
    let token_count = tokens.len();
    let mut document = build_tree(tokens, options)?;
    let normalization = if options.normalize {
        normalize_document(&mut document)
    } else {
        NormalizationReport::default()
    };

    Ok(ParseOutput {
        document,
        normalization,
        token_count,
    })
}

/// Parses a fragment and returns children under a synthetic container.
pub fn parse_fragment(input: &str, context_tag: &str) -> Result<Vec<Node>> {
    let wrapped = format!("<{}>{}</{}>", context_tag, input, context_tag);
    let document = parse(&wrapped)?;
    let context = document
        .first_element_by_tag(context_tag)
        .cloned()
        .unwrap_or_else(|| Element::new(context_tag));
    Ok(context.children)
}

fn build_tree(tokens: Vec<Token>, options: ParseOptions) -> Result<Document> {
    let mut document = Document::new();
    let mut stack = Vec::<Element>::new();

    for token in tokens {
        match token {
            Token::Doctype { name } => {
                if document.doctype.is_none() {
                    document.doctype = Some(name.trim().to_ascii_lowercase());
                }
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } => {
                let tag = name.to_ascii_lowercase();
                apply_implied_end_tags_for_start(&tag, &mut document, &mut stack);

                let mut element = Element::new(tag.clone());
                element.attrs = attrs;

                if is_void_element(&tag) || self_closing {
                    append(&mut document, &mut stack, Node::Element(element), options);
                } else {
                    stack.push(element);
                }
            }
            Token::EndTag { name } => {
                close_until(
                    &mut document,
                    &mut stack,
                    &name.to_ascii_lowercase(),
                    options,
                );
            }
            Token::Comment(comment) => {
                append(&mut document, &mut stack, Node::Comment(comment), options);
            }
            Token::Text(text) => {
                if text.is_empty() {
                    continue;
                }
                append(&mut document, &mut stack, Node::Text(text), options);
            }
        }
    }

    while let Some(top) = stack.pop() {
        append(&mut document, &mut stack, Node::Element(top), options);
    }

    Ok(document)
}

fn append(document: &mut Document, stack: &mut [Element], node: Node, options: ParseOptions) {
    if stack.is_empty() && !options.retain_top_level_whitespace && node.is_whitespace_text() {
        return;
    }

    if let Some(parent) = stack.last_mut() {
        parent.push(node);
    } else {
        document.children.push(node);
    }
}

fn apply_implied_end_tags_for_start(tag: &str, document: &mut Document, stack: &mut Vec<Element>) {
    if tag == "li" {
        close_open_list_item(document, stack);
        return;
    }

    if matches!(tag, "dt" | "dd") {
        close_open_element(document, stack, "dt");
        close_open_element(document, stack, "dd");
        return;
    }

    if tag == "option" {
        close_open_element(document, stack, "option");
        return;
    }

    if is_block_that_closes_p(tag) {
        close_open_element(document, stack, "p");
    }

    if tag == "tr" {
        close_open_element(document, stack, "tr");
    }

    if matches!(tag, "td" | "th") {
        close_open_element(document, stack, "td");
        close_open_element(document, stack, "th");
    }
}

fn close_open_list_item(document: &mut Document, stack: &mut Vec<Element>) {
    let Some(li_position) = stack.iter().rposition(|element| element.tag == "li") else {
        return;
    };
    let list_position = stack
        .iter()
        .rposition(|element| matches!(element.tag.as_str(), "ul" | "ol" | "menu"));

    if list_position.is_some_and(|position| position > li_position) {
        return;
    }

    close_open_element(document, stack, "li");
}

fn close_open_element(document: &mut Document, stack: &mut Vec<Element>, tag: &str) {
    let Some(position) = stack.iter().rposition(|element| element.tag == tag) else {
        return;
    };

    while stack.len() > position {
        let Some(top) = stack.pop() else {
            return;
        };
        append(document, stack, Node::Element(top), ParseOptions::default());
    }
}

fn close_until(
    document: &mut Document,
    stack: &mut Vec<Element>,
    tag: &str,
    options: ParseOptions,
) {
    let Some(position) = stack.iter().rposition(|element| element.tag == tag) else {
        // Unknown end tags are ignored, matching browser recovery instincts.
        return;
    };

    while stack.len() > position {
        let Some(top) = stack.pop() else {
            return;
        };
        let matched = top.tag == tag;
        append(document, stack, Node::Element(top), options);
        if matched {
            break;
        }
    }
}

fn is_block_that_closes_p(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "div"
            | "dl"
            | "fieldset"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "ul"
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
    fn parse_normalizes_shell() {
        let output =
            parse_with_options("<title>T</title><p>Hello", ParseOptions::default()).unwrap();
        assert_eq!(output.document.doctype.as_deref(), Some("html"));
        assert!(output.normalization.inserted_html);
        assert!(output.document.first_element_by_tag("head").is_some());
        assert!(output.document.first_element_by_tag("body").is_some());
    }

    #[test]
    fn p_is_closed_by_block() {
        let doc = parse("<p>a<div>b</div>").unwrap();
        let body = doc.first_element_by_tag("body").unwrap();
        assert_eq!(body.children.len(), 2);
    }
}
