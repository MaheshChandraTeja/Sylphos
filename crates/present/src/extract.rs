#![doc = "DOM-to-render-document extraction."]

use html_mvp::dom::Element;
use html_mvp::{Document, Node};

use crate::{parse_css_lite, Color, RenderBlock, RenderDocument, StyleSheetLite};

/// Extracts the first valid `<meta name="theme-color" content="#...">` color.
#[must_use]
pub fn extract_theme_color(doc: &Document) -> Option<Color> {
    find_theme_color_in_nodes(&doc.children)
}

/// Extracts a minimal style sheet from all inline `<style>` blocks.
#[must_use]
pub fn extract_style_sheet(doc: &Document) -> StyleSheetLite {
    let mut sheet = StyleSheetLite::default();
    collect_style_sheet_from_nodes(&doc.children, &mut sheet);
    sheet
}

/// Converts an `html_mvp` document into a lightweight render document.
#[must_use]
pub fn extract_render_document(doc: &Document) -> RenderDocument {
    let mut render_doc = RenderDocument {
        title: find_title_in_nodes(&doc.children),
        theme_color: extract_theme_color(doc),
        style_sheet: extract_style_sheet(doc),
        blocks: Vec::new(),
    };

    collect_blocks_from_nodes(&doc.children, &mut render_doc.blocks);
    render_doc
}

fn find_title_in_nodes(nodes: &[Node]) -> Option<String> {
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };

        if element.tag.eq_ignore_ascii_case("title") {
            let text = collect_text_from_nodes(&element.children);
            if !text.is_empty() {
                return Some(text);
            }
        }

        if is_script_like_subtree(&element.tag) || element.tag.eq_ignore_ascii_case("style") {
            continue;
        }

        if let Some(title) = find_title_in_nodes(&element.children) {
            return Some(title);
        }
    }

    None
}

fn find_theme_color_in_nodes(nodes: &[Node]) -> Option<Color> {
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };

        if element.tag.eq_ignore_ascii_case("meta") && is_theme_color_meta(element) {
            if let Some(content) = attr_value(&element.attrs, "content") {
                if let Some(color) = Color::from_css_hex(content) {
                    return Some(color);
                }
            }
        }

        if is_script_like_subtree(&element.tag) || element.tag.eq_ignore_ascii_case("style") {
            continue;
        }

        if let Some(color) = find_theme_color_in_nodes(&element.children) {
            return Some(color);
        }
    }

    None
}

fn collect_style_sheet_from_nodes(nodes: &[Node], sheet: &mut StyleSheetLite) {
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };

        if element.tag.eq_ignore_ascii_case("style") {
            let css = collect_raw_text_from_nodes(&element.children);
            if !css.trim().is_empty() {
                sheet.merge_from(parse_css_lite(&css));
            }
            continue;
        }

        if is_script_like_subtree(&element.tag) {
            continue;
        }

        collect_style_sheet_from_nodes(&element.children, sheet);
    }
}

fn collect_blocks_from_nodes(nodes: &[Node], blocks: &mut Vec<RenderBlock>) {
    for node in nodes {
        collect_blocks_from_node(node, blocks);
    }
}

fn collect_blocks_from_node(node: &Node, blocks: &mut Vec<RenderBlock>) {
    match node {
        Node::Text(text) => {
            let normalized = normalize_text(text);
            if !normalized.is_empty() {
                blocks.push(RenderBlock::Paragraph { text: normalized });
            }
        }
        Node::Comment(_) => {}
        Node::Element(element) => collect_blocks_from_element(element, blocks),
    }
}

fn collect_blocks_from_element(element: &Element, blocks: &mut Vec<RenderBlock>) {
    let tag = element.tag.as_str();

    if is_ignored_visible_subtree(tag) || is_head_only_tag(tag) {
        return;
    }

    if let Some(level) = heading_level(tag) {
        let text = collect_visible_text_from_nodes(&element.children);
        if !text.is_empty() {
            blocks.push(RenderBlock::Heading { level, text });
        }
        return;
    }

    if is_inline_text_container(tag) {
        let text = collect_visible_text_from_nodes(&element.children);
        if !text.is_empty() {
            blocks.push(RenderBlock::Paragraph { text });
        }
        return;
    }

    match tag {
        "p" => {
            if let Some(link_block) = single_link_child_as_block(&element.children) {
                blocks.push(link_block);
                return;
            }

            let text = collect_visible_text_from_nodes(&element.children);
            if !text.is_empty() {
                blocks.push(RenderBlock::Paragraph { text });
            }
        }
        "a" => {
            let text = collect_visible_text_from_nodes(&element.children);
            let href = attr_value(&element.attrs, "href").map(ToOwned::to_owned);

            if !text.is_empty() || href.is_some() {
                blocks.push(RenderBlock::Link { text, href });
            }
        }
        "img" => {
            let src = attr_value(&element.attrs, "src").map(ToOwned::to_owned);
            let alt = attr_value(&element.attrs, "alt").map(ToOwned::to_owned);

            if src.is_some() || alt.is_some() {
                blocks.push(RenderBlock::Image { alt, src });
            }
        }
        "html" | "body" | "main" | "section" | "article" | "div" | "span" | "center" | "font"
        | "form" | "label" | "nav" | "header" | "footer" | "aside" | "ul" | "ol" | "li"
        | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "small" | "strong"
        | "em" | "b" | "i" | "u" => {
            collect_blocks_from_nodes(&element.children, blocks);
        }
        _ => {
            let text = collect_visible_text_from_nodes(&element.children);
            if !text.is_empty() {
                blocks.push(RenderBlock::Generic {
                    tag: tag.to_ascii_lowercase(),
                    text,
                });
            }
        }
    }
}

fn single_link_child_as_block(nodes: &[Node]) -> Option<RenderBlock> {
    let mut link: Option<&Element> = None;

    for node in nodes {
        match node {
            Node::Element(element) => {
                if link.is_some() {
                    return None;
                }
                link = Some(element);
            }
            Node::Text(text) if normalize_text(text).is_empty() => {}
            Node::Comment(_) => {}
            Node::Text(_) => return None,
        }
    }

    let element = link?;
    if !element.tag.eq_ignore_ascii_case("a") {
        return None;
    }

    let text = collect_visible_text_from_nodes(&element.children);
    let href = attr_value(&element.attrs, "href").map(ToOwned::to_owned);

    Some(RenderBlock::Link { text, href })
}

fn collect_text_from_nodes(nodes: &[Node]) -> String {
    let mut buffer = String::new();
    append_text_from_nodes(nodes, &mut buffer);
    normalize_text(&buffer)
}

fn collect_visible_text_from_nodes(nodes: &[Node]) -> String {
    let mut buffer = String::new();
    append_visible_text_from_nodes(nodes, &mut buffer);
    normalize_text(&buffer)
}

fn collect_raw_text_from_nodes(nodes: &[Node]) -> String {
    let mut buffer = String::new();
    append_text_from_nodes(nodes, &mut buffer);
    buffer
}

fn append_text_from_nodes(nodes: &[Node], buffer: &mut String) {
    for node in nodes {
        match node {
            Node::Text(text) => {
                buffer.push_str(text);
            }
            Node::Comment(_) => {}
            Node::Element(element) => {
                append_text_from_nodes(&element.children, buffer);
            }
        }
    }
}

fn append_visible_text_from_nodes(nodes: &[Node], buffer: &mut String) {
    for node in nodes {
        match node {
            Node::Text(text) => {
                buffer.push(' ');
                buffer.push_str(text);
            }
            Node::Comment(_) => {}
            Node::Element(element) => {
                if !is_ignored_visible_subtree(&element.tag) && !is_head_only_tag(&element.tag) {
                    append_visible_text_from_nodes(&element.children, buffer);
                }
            }
        }
    }
}

fn normalize_text(input: &str) -> String {
    let mut output = String::new();

    for part in input.split_whitespace() {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(part);
    }

    output
}

fn is_theme_color_meta(element: &Element) -> bool {
    attr_value(&element.attrs, "name").is_some_and(|name| name.eq_ignore_ascii_case("theme-color"))
}

fn attr_value<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case(name) {
            Some(value.as_str())
        } else {
            None
        }
    })
}

fn heading_level(tag: &str) -> Option<u8> {
    let bytes = tag.as_bytes();

    if bytes.len() == 2 && bytes[0].eq_ignore_ascii_case(&b'h') && (b'1'..=b'6').contains(&bytes[1])
    {
        return Some(bytes[1] - b'0');
    }

    None
}

fn is_script_like_subtree(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "script" | "noscript" | "template"
    )
}

fn is_ignored_visible_subtree(tag: &str) -> bool {
    is_script_like_subtree(tag) || tag.eq_ignore_ascii_case("style")
}

fn is_inline_text_container(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "center" | "font" | "span" | "label" | "small" | "strong" | "em" | "b" | "i" | "u"
    )
}

fn is_head_only_tag(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "head"
            | "title"
            | "meta"
            | "link"
            | "base"
            | "input"
            | "button"
            | "select"
            | "option"
            | "textarea"
    )
}
