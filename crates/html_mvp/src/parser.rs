use crate::dom::{Document, Element, Node};
use crate::tokenizer::{Token, Tokenizer};
use anyhow::Result;

pub fn parse(input: &str) -> Result<Document> {
    let tokens = Tokenizer::new(input).tokenize();
    build_tree(tokens)
}

fn build_tree(tokens: Vec<Token>) -> Result<Document> {
    let mut doc = Document::new();
    let mut stack: Vec<Element> = Vec::new();

    for t in tokens {
        match t {
            Token::Doctype { name } => {
                doc.doctype = Some(name.trim().to_ascii_lowercase());
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } => {
                let mut el = Element::new(name);
                el.attrs = attrs;
                let is_void = is_void_element(&el.tag);
                if is_void || self_closing {
                    append(&mut doc, &mut stack, Node::Element(el));
                } else {
                    stack.push(el);
                }
            }
            Token::EndTag { name } => {
                let tag = name.to_ascii_lowercase();
                while let Some(top) = stack.pop() {
                    let matches = top.tag == tag;
                    append(&mut doc, &mut stack, Node::Element(top));
                    if matches {
                        break;
                    }
                }
            }
            Token::Comment(c) => append(&mut doc, &mut stack, Node::Comment(c)),
            Token::Text(text) => {
                if text.is_empty() {
                    continue;
                }
                append(&mut doc, &mut stack, Node::Text(text));
            }
        }
    }

    while let Some(top) = stack.pop() {
        append(&mut doc, &mut stack, Node::Element(top));
    }

    Ok(doc)
}

fn append(doc: &mut Document, stack: &mut [Element], node: Node) {
    if let Some(parent) = stack.last_mut() {
        parent.push(node);
    } else {
        doc.children.push(node);
    }
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
