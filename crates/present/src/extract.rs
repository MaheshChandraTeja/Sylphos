#![doc = "DOM-to-render-document extraction."]

use html_mvp::dom::Element;
use html_mvp::{Document, Node};

use crate::selector::{parse_css_rules_lite, AncestorSignature, ElementSignature, StyleRuleLite};
use crate::stylesheet::{StyleSourceLite, StylesheetLink};
use crate::{
    parse_css_lite, Color, FormBlock, FormControl, FormControlKind, FormMethod, InlineFragment,
    RenderBlock, RenderDocument, StyleSheetLite,
};

const CSS_RULE_ORDER_STRIDE: usize = 10_000;

/// Extracts the first valid `<meta name="theme-color" content="#...">` color.
#[must_use]
pub fn extract_theme_color(doc: &Document) -> Option<Color> {
    find_theme_color_in_nodes(&doc.children)
}

/// Extracts ordered inline and external stylesheet sources from the document.
#[must_use]
pub fn extract_style_sources(doc: &Document) -> Vec<StyleSourceLite> {
    let mut context = ExtractContext::default();
    let mut sources = Vec::new();
    collect_style_sources_from_nodes(&doc.children, &mut sources, &mut context);
    sources.sort_by_key(StyleSourceLite::source_order);
    sources
}

/// Extracts external stylesheet links from the document.
#[must_use]
pub fn extract_stylesheet_links(doc: &Document) -> Vec<StylesheetLink> {
    extract_style_sources(doc)
        .into_iter()
        .filter_map(|source| match source {
            StyleSourceLite::External(link) => Some(link),
            StyleSourceLite::Inline { .. } => None,
        })
        .collect()
}

/// Extracts a minimal style sheet from inline `<style>` blocks.
#[must_use]
pub fn extract_style_sheet(doc: &Document) -> StyleSheetLite {
    style_sheet_from_sources(&extract_style_sources(doc), false)
}

/// Converts an `html_mvp` document into a lightweight render document.
#[must_use]
pub fn extract_render_document(doc: &Document) -> RenderDocument {
    let mut context = ExtractContext::default();
    let style_sources = extract_style_sources(doc);
    let external_stylesheets = style_sources
        .iter()
        .filter_map(|source| match source {
            StyleSourceLite::External(link) => Some(link.clone()),
            StyleSourceLite::Inline { .. } => None,
        })
        .collect::<Vec<_>>();

    let mut render_doc = RenderDocument {
        title: find_title_in_nodes(&doc.children),
        theme_color: extract_theme_color(doc),
        style_sheet: style_sheet_from_sources(&style_sources, false),
        style_rules: style_rules_from_sources(&style_sources, false),
        style_sources,
        external_stylesheets,
        blocks: Vec::new(),
        block_elements: Vec::new(),
        style_tree: crate::StyledDocument::default(),
    };

    collect_blocks_from_nodes(&doc.children, &mut render_doc, &mut context);
    render_doc.recompute_style_tree();
    render_doc
}

#[derive(Debug, Default)]
struct ExtractContext {
    next_form_id: u64,
    next_control_id: u64,
    next_style_order: usize,
    ancestors: Vec<AncestorSignature>,
}

impl ExtractContext {
    fn form_id(&mut self) -> u64 {
        self.next_form_id = self.next_form_id.saturating_add(1);
        self.next_form_id
    }

    fn control_id(&mut self) -> u64 {
        self.next_control_id = self.next_control_id.saturating_add(1);
        self.next_control_id
    }

    fn style_order(&mut self) -> usize {
        let order = self.next_style_order;
        self.next_style_order = self.next_style_order.saturating_add(1);
        order
    }

    fn signature_for(&self, element: &Element) -> ElementSignature {
        ElementSignature::from_attrs(&element.tag, &element.attrs, &self.ancestors)
    }

    fn enter(&mut self, element: &Element) {
        let signature = self.signature_for(element);
        self.ancestors.push(signature.as_ancestor());
    }

    fn exit(&mut self) {
        let _ = self.ancestors.pop();
    }
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

fn style_sheet_from_sources(sources: &[StyleSourceLite], include_external: bool) -> StyleSheetLite {
    let mut sheet = StyleSheetLite::default();

    for source in sources {
        match source {
            StyleSourceLite::Inline { css, .. } => sheet.merge_from(parse_css_lite(css)),
            StyleSourceLite::External(_) if include_external => {}
            StyleSourceLite::External(_) => {}
        }
    }

    sheet
}

fn style_rules_from_sources(
    sources: &[StyleSourceLite],
    include_external: bool,
) -> Vec<StyleRuleLite> {
    let mut rules = Vec::new();

    for source in sources {
        match source {
            StyleSourceLite::Inline { css, source_order } => {
                rules.extend(parse_css_rules_lite(
                    css,
                    source_order.saturating_mul(CSS_RULE_ORDER_STRIDE),
                ));
            }
            StyleSourceLite::External(_) if include_external => {}
            StyleSourceLite::External(_) => {}
        }
    }

    rules
}

fn collect_style_sources_from_nodes(
    nodes: &[Node],
    sources: &mut Vec<StyleSourceLite>,
    context: &mut ExtractContext,
) {
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };

        if element.tag.eq_ignore_ascii_case("style") {
            let css = collect_raw_text_from_nodes(&element.children);
            if !css.trim().is_empty() {
                sources.push(StyleSourceLite::Inline {
                    css,
                    source_order: context.style_order(),
                });
            }
            continue;
        }

        if let Some(link) = stylesheet_link_from_element(element, context) {
            sources.push(StyleSourceLite::External(link));
            continue;
        }

        if is_script_like_subtree(&element.tag) {
            continue;
        }

        collect_style_sources_from_nodes(&element.children, sources, context);
    }
}

fn stylesheet_link_from_element(
    element: &Element,
    context: &mut ExtractContext,
) -> Option<StylesheetLink> {
    if !element.tag.eq_ignore_ascii_case("link") {
        return None;
    }

    let rel = attr_value(&element.attrs, "rel")?;
    let has_stylesheet = rel
        .split_ascii_whitespace()
        .any(|token| token.eq_ignore_ascii_case("stylesheet"));
    let is_alternate = rel
        .split_ascii_whitespace()
        .any(|token| token.eq_ignore_ascii_case("alternate"));

    if !has_stylesheet || is_alternate {
        return None;
    }

    let href = attr_value(&element.attrs, "href")?.trim();
    if href.is_empty() {
        return None;
    }

    Some(StylesheetLink {
        href: href.to_owned(),
        media: attr_value(&element.attrs, "media").map(ToOwned::to_owned),
        disabled: attr_exists(&element.attrs, "disabled"),
        source_order: context.style_order(),
    })
}

fn collect_blocks_from_nodes(
    nodes: &[Node],
    document: &mut RenderDocument,
    context: &mut ExtractContext,
) {
    for node in nodes {
        collect_blocks_from_node(node, document, context);
    }
}

fn collect_blocks_from_node(
    node: &Node,
    document: &mut RenderDocument,
    context: &mut ExtractContext,
) {
    match node {
        Node::Text(text) => {
            let normalized = normalize_text(text);
            if !normalized.is_empty() {
                document.push_block(
                    RenderBlock::Paragraph { text: normalized },
                    ElementSignature::synthetic("p", &context.ancestors),
                );
            }
        }
        Node::Comment(_) => {}
        Node::Element(element) => collect_blocks_from_element(element, document, context),
    }
}

fn collect_blocks_from_element(
    element: &Element,
    document: &mut RenderDocument,
    context: &mut ExtractContext,
) {
    let tag = element.tag.as_str();

    if is_ignored_visible_subtree(tag) || is_head_only_tag(tag) {
        return;
    }

    let signature = context.signature_for(element);

    if let Some(level) = heading_level(tag) {
        let text = collect_visible_text_from_nodes(&element.children);
        if !text.is_empty() {
            document.push_block(RenderBlock::Heading { level, text }, signature);
        }
        return;
    }

    if is_inline_text_container(tag) {
        let fragments = collect_inline_fragments(&element.children);
        if InlineFragment::contains_link(&fragments) || fragments.len() > 1 {
            if !InlineFragment::plain_text(&fragments).is_empty() {
                document.push_block(RenderBlock::InlineFlow { fragments }, signature);
            }
        } else {
            let text = collect_visible_text_from_nodes(&element.children);
            if !text.is_empty() {
                document.push_block(RenderBlock::Paragraph { text }, signature);
            }
        }
        return;
    }

    match tag {
        "form" => {
            let form = extract_form_block(element, context);
            if !form.is_empty() {
                document.push_block(RenderBlock::Form(form), signature);
            }
        }
        "input" | "button" | "textarea" => {
            let form = synthetic_form_for_control(element, context);
            if !form.is_empty() {
                document.push_block(RenderBlock::Form(form), signature);
            }
        }
        "p" => {
            if let Some((link_block, link_signature)) =
                single_link_child_as_block(&element.children, context)
            {
                document.push_block(link_block, link_signature);
                return;
            }

            let fragments = collect_inline_fragments(&element.children);
            if InlineFragment::contains_link(&fragments) || fragments.len() > 1 {
                if !InlineFragment::plain_text(&fragments).is_empty() {
                    document.push_block(RenderBlock::InlineFlow { fragments }, signature);
                }
                return;
            }

            let text = collect_visible_text_from_nodes(&element.children);
            if !text.is_empty() {
                document.push_block(RenderBlock::Paragraph { text }, signature);
            }
        }
        "a" => {
            let text = collect_visible_text_from_nodes(&element.children);
            let href = attr_value(&element.attrs, "href").map(ToOwned::to_owned);

            if !text.is_empty() || href.is_some() {
                document.push_block(RenderBlock::Link { text, href }, signature);
            }
        }
        "img" => {
            let src = attr_value(&element.attrs, "src").map(ToOwned::to_owned);
            let alt = attr_value(&element.attrs, "alt").map(ToOwned::to_owned);

            if src.is_some() || alt.is_some() {
                document.push_block(RenderBlock::Image { alt, src }, signature);
            }
        }
        "html" | "body" | "main" | "section" | "article" | "div" | "span" | "center" | "font"
        | "label" | "nav" | "header" | "footer" | "aside" | "ul" | "ol" | "li" | "table"
        | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "small" | "strong" | "em" | "b"
        | "i" | "u" => {
            context.enter(element);
            collect_blocks_from_nodes(&element.children, document, context);
            context.exit();
        }
        _ => {
            let text = collect_visible_text_from_nodes(&element.children);
            if !text.is_empty() {
                document.push_block(
                    RenderBlock::Generic {
                        tag: tag.to_ascii_lowercase(),
                        text,
                    },
                    signature,
                );
            }
        }
    }
}

fn extract_form_block(element: &Element, context: &mut ExtractContext) -> FormBlock {
    let mut controls = Vec::new();
    collect_form_controls(&element.children, &mut controls, context);

    FormBlock {
        id: context.form_id(),
        action: attr_value(&element.attrs, "action").map(ToOwned::to_owned),
        method: FormMethod::from_attr(attr_value(&element.attrs, "method")),
        controls,
    }
}

fn synthetic_form_for_control(element: &Element, context: &mut ExtractContext) -> FormBlock {
    let controls = extract_control(element, context).into_iter().collect();
    FormBlock {
        id: context.form_id(),
        action: None,
        method: FormMethod::Get,
        controls,
    }
}

fn collect_form_controls(
    nodes: &[Node],
    controls: &mut Vec<FormControl>,
    context: &mut ExtractContext,
) {
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };

        if let Some(control) = extract_control(element, context) {
            controls.push(control);
        }

        if !is_script_like_subtree(&element.tag) && !element.tag.eq_ignore_ascii_case("style") {
            collect_form_controls(&element.children, controls, context);
        }
    }
}

fn extract_control(element: &Element, context: &mut ExtractContext) -> Option<FormControl> {
    match element.tag.as_str() {
        "input" => {
            let kind = FormControlKind::from_input_type(attr_value(&element.attrs, "type"));
            let default_label = if kind == FormControlKind::Submit {
                "Submit"
            } else if kind == FormControlKind::Button {
                "Button"
            } else {
                ""
            };
            let value = attr_value(&element.attrs, "value")
                .unwrap_or(default_label)
                .to_owned();

            Some(FormControl {
                id: context.control_id(),
                kind,
                name: attr_value(&element.attrs, "name").map(ToOwned::to_owned),
                value,
                placeholder: attr_value(&element.attrs, "placeholder").map(ToOwned::to_owned),
                label: attr_value(&element.attrs, "aria-label")
                    .or_else(|| attr_value(&element.attrs, "title"))
                    .map(ToOwned::to_owned),
                disabled: has_attr(&element.attrs, "disabled"),
                focused: false,
            })
        }
        "button" => {
            let value = attr_value(&element.attrs, "value").map_or_else(
                || collect_visible_text_from_nodes(&element.children),
                ToOwned::to_owned,
            );
            let label = if value.trim().is_empty() {
                "Submit".to_owned()
            } else {
                value.clone()
            };
            let kind = match attr_value(&element.attrs, "type")
                .unwrap_or("submit")
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "button" | "reset" => FormControlKind::Button,
                _ => FormControlKind::Submit,
            };

            Some(FormControl {
                id: context.control_id(),
                kind,
                name: attr_value(&element.attrs, "name").map(ToOwned::to_owned),
                value: if value.trim().is_empty() {
                    label.clone()
                } else {
                    value
                },
                placeholder: None,
                label: Some(label),
                disabled: has_attr(&element.attrs, "disabled"),
                focused: false,
            })
        }
        "textarea" => {
            let value = attr_value(&element.attrs, "value").map_or_else(
                || collect_text_from_nodes(&element.children),
                ToOwned::to_owned,
            );

            Some(FormControl {
                id: context.control_id(),
                kind: FormControlKind::TextArea,
                name: attr_value(&element.attrs, "name").map(ToOwned::to_owned),
                value,
                placeholder: attr_value(&element.attrs, "placeholder").map(ToOwned::to_owned),
                label: attr_value(&element.attrs, "aria-label")
                    .or_else(|| attr_value(&element.attrs, "title"))
                    .map(ToOwned::to_owned),
                disabled: has_attr(&element.attrs, "disabled"),
                focused: false,
            })
        }
        _ => None,
    }
}

fn single_link_child_as_block(
    nodes: &[Node],
    context: &ExtractContext,
) -> Option<(RenderBlock, ElementSignature)> {
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
    let signature = context.signature_for(element);

    Some((RenderBlock::Link { text, href }, signature))
}

fn collect_inline_fragments(nodes: &[Node]) -> Vec<InlineFragment> {
    let mut fragments = Vec::new();
    append_inline_fragments(nodes, &mut fragments);
    compact_inline_fragments(fragments)
}

fn append_inline_fragments(nodes: &[Node], fragments: &mut Vec<InlineFragment>) {
    for node in nodes {
        match node {
            Node::Text(text) => {
                if let Some(fragment) = InlineFragment::text(text) {
                    fragments.push(fragment);
                }
            }
            Node::Comment(_) => {}
            Node::Element(element) => {
                if is_ignored_visible_subtree(&element.tag) || is_head_only_tag(&element.tag) {
                    continue;
                }

                if element.tag.eq_ignore_ascii_case("a") {
                    let text = collect_visible_text_from_nodes(&element.children);
                    let href = attr_value(&element.attrs, "href").map(ToOwned::to_owned);
                    if let Some(fragment) = InlineFragment::link(text, href) {
                        fragments.push(fragment);
                    }
                    continue;
                }

                if element.tag.eq_ignore_ascii_case("br") {
                    if let Some(fragment) = InlineFragment::text(" ") {
                        fragments.push(fragment);
                    }
                    continue;
                }

                if is_inline_text_container(&element.tag) {
                    append_inline_fragments(&element.children, fragments);
                } else {
                    let text = collect_visible_text_from_nodes(&element.children);
                    if let Some(fragment) = InlineFragment::text(text) {
                        fragments.push(fragment);
                    }
                }
            }
        }
    }
}

fn compact_inline_fragments(fragments: Vec<InlineFragment>) -> Vec<InlineFragment> {
    let mut compacted: Vec<InlineFragment> = Vec::new();

    for fragment in fragments {
        match (compacted.last_mut(), fragment) {
            (Some(InlineFragment::Text { text: existing }), InlineFragment::Text { text }) => {
                if !existing.is_empty() && !text.is_empty() {
                    existing.push(' ');
                }
                existing.push_str(&text);
            }
            (
                Some(InlineFragment::Link {
                    text: existing,
                    href: existing_href,
                }),
                InlineFragment::Link { text, href },
            ) if *existing_href == href => {
                if !existing.is_empty() && !text.is_empty() {
                    existing.push(' ');
                }
                existing.push_str(&text);
            }
            (_, fragment) => compacted.push(fragment),
        }
    }

    compacted
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

fn attr_exists(attrs: &[(String, String)], name: &str) -> bool {
    attrs.iter().any(|(key, _)| key.eq_ignore_ascii_case(name))
}

fn has_attr(attrs: &[(String, String)], name: &str) -> bool {
    attrs.iter().any(|(key, _)| key.eq_ignore_ascii_case(name))
}

fn heading_level(tag: &str) -> Option<u8> {
    let bytes = tag.as_bytes();

    if bytes.len() == 2 && bytes[0].eq_ignore_ascii_case(&b'h') && (b'1'..=b'6').contains(&bytes[1])
    {
        Some(bytes[1] - b'0')
    } else {
        None
    }
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
        "head" | "title" | "meta" | "link" | "base"
    )
}
