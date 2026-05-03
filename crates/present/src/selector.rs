#![doc = "CSS-lite selector parsing, matching, and declaration representation."]

use crate::box_model::{BoxStyleLite, DisplayLite, EdgeSizes};
use crate::Color;

const DEFAULT_FONT_SIZE: f32 = 16.0;

/// Raw CSS declaration retained for selector-based computed styles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssDeclarationLite {
    /// Lowercase CSS property name.
    pub property: String,

    /// Trimmed declaration value.
    pub value: String,

    /// Declaration order inside its source rule.
    pub declaration_order: usize,
}

/// One parsed CSS rule with one or more selectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRuleLite {
    /// Selectors in this rule.
    pub selectors: Vec<SelectorLite>,

    /// Supported and unsupported declarations are retained so later modules can expand coverage.
    pub declarations: Vec<CssDeclarationLite>,

    /// Stable source-order number across inline and external stylesheets.
    pub source_order: usize,
}

/// Parsed CSS selector for the tiny cascade engine.
///
/// Supports tag, class, id, universal, and descendant combinators. Pseudo selectors are stripped
/// to their base selector, so `a:visited` behaves as `a` for this renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorLite {
    /// Descendant selector parts from left to right.
    pub parts: Vec<SelectorPartLite>,

    /// CSS-like specificity tuple `(ids, classes/attrs, tags)`.
    pub specificity: Specificity,

    /// Normalized selector source.
    pub source: String,
}

impl SelectorLite {
    /// Parses a selector string.
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let normalized = normalize_selector_text(input);
        if normalized.is_empty() {
            return None;
        }

        let parts = normalized
            .split_whitespace()
            .filter_map(SelectorPartLite::parse)
            .collect::<Vec<_>>();

        if parts.is_empty() {
            return None;
        }

        let specificity = parts
            .iter()
            .fold(Specificity::default(), |acc, part| acc + part.specificity());

        Some(Self {
            parts,
            specificity,
            source: normalized,
        })
    }

    /// Returns true when this selector matches an element signature.
    #[must_use]
    pub fn matches(&self, element: &ElementSignature) -> bool {
        let Some(last) = self.parts.last() else {
            return false;
        };

        if !last.matches_element(element) {
            return false;
        }

        if self.parts.len() == 1 {
            return true;
        }

        let mut ancestor_index = element.ancestors.len();

        for selector_part in self.parts[..self.parts.len().saturating_sub(1)]
            .iter()
            .rev()
        {
            let mut found = false;

            while ancestor_index > 0 {
                ancestor_index -= 1;
                if selector_part.matches_ancestor(&element.ancestors[ancestor_index]) {
                    found = true;
                    break;
                }
            }

            if !found {
                return false;
            }
        }

        true
    }

    /// Returns the tag when this selector is exactly a simple tag selector.
    #[must_use]
    pub fn simple_tag_name(&self) -> Option<&str> {
        if self.parts.len() != 1 {
            return None;
        }

        self.parts[0].simple_tag_name()
    }
}

/// One simple selector part, e.g. `div.card#main`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorPartLite {
    /// Optional tag name. `None` means universal or class/id-only selector.
    pub tag: Option<String>,

    /// Optional id selector.
    pub id: Option<String>,

    /// Required classes.
    pub classes: Vec<String>,
}

impl SelectorPartLite {
    /// Parses one simple selector token.
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let token = strip_pseudo_and_attributes(input.trim());
        if token.is_empty() {
            return None;
        }

        if token == "*" {
            return Some(Self {
                tag: None,
                id: None,
                classes: Vec::new(),
            });
        }

        let mut tag = String::new();
        let mut id = None;
        let mut classes = Vec::new();
        let chars = token.chars().collect::<Vec<_>>();
        let mut index = 0usize;

        while index < chars.len() && chars[index] != '#' && chars[index] != '.' {
            tag.push(chars[index]);
            index += 1;
        }

        while index < chars.len() {
            match chars[index] {
                '#' => {
                    index += 1;
                    let start = index;
                    while index < chars.len() && is_selector_ident_char(chars[index]) {
                        index += 1;
                    }
                    if start < index {
                        id = Some(
                            chars[start..index]
                                .iter()
                                .collect::<String>()
                                .to_ascii_lowercase(),
                        );
                    }
                }
                '.' => {
                    index += 1;
                    let start = index;
                    while index < chars.len() && is_selector_ident_char(chars[index]) {
                        index += 1;
                    }
                    if start < index {
                        classes.push(
                            chars[start..index]
                                .iter()
                                .collect::<String>()
                                .to_ascii_lowercase(),
                        );
                    }
                }
                _ => index += 1,
            }
        }

        let tag = if tag.trim().is_empty() || tag == "*" {
            None
        } else {
            Some(tag.trim().to_ascii_lowercase())
        };

        Some(Self { tag, id, classes })
    }

    /// Returns specificity for this selector part.
    #[must_use]
    pub fn specificity(&self) -> Specificity {
        Specificity {
            ids: u16::from(self.id.is_some()),
            classes: u16::try_from(self.classes.len()).unwrap_or(u16::MAX),
            tags: u16::from(self.tag.is_some()),
        }
    }

    /// Returns true if the selector part matches an element.
    #[must_use]
    pub fn matches_element(&self, element: &ElementSignature) -> bool {
        self.matches_core(&element.tag, element.id.as_deref(), &element.classes)
    }

    /// Returns true if the selector part matches an ancestor.
    #[must_use]
    pub fn matches_ancestor(&self, ancestor: &AncestorSignature) -> bool {
        self.matches_core(&ancestor.tag, ancestor.id.as_deref(), &ancestor.classes)
    }

    /// Returns the tag when this part is only a tag selector.
    #[must_use]
    pub fn simple_tag_name(&self) -> Option<&str> {
        if self.id.is_none() && self.classes.is_empty() {
            self.tag.as_deref()
        } else {
            None
        }
    }

    fn matches_core(&self, tag: &str, id: Option<&str>, classes: &[String]) -> bool {
        if let Some(required_tag) = &self.tag {
            if !required_tag.eq_ignore_ascii_case(tag) {
                return false;
            }
        }

        if let Some(required_id) = &self.id {
            if id.map(str::to_ascii_lowercase).as_deref() != Some(required_id.as_str()) {
                return false;
            }
        }

        self.classes
            .iter()
            .all(|required| classes.iter().any(|class_name| class_name == required))
    }
}

/// CSS selector specificity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    /// Number of id selectors.
    pub ids: u16,

    /// Number of class, attribute, or pseudo-class selectors supported by the tiny parser.
    pub classes: u16,

    /// Number of tag selectors.
    pub tags: u16,
}

impl std::ops::Add for Specificity {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            ids: self.ids.saturating_add(rhs.ids),
            classes: self.classes.saturating_add(rhs.classes),
            tags: self.tags.saturating_add(rhs.tags),
        }
    }
}

/// Metadata for a block-level render node used by selector matching.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementSignature {
    /// Element tag name.
    pub tag: String,

    /// Optional `id` attribute.
    pub id: Option<String>,

    /// Lowercase class names.
    pub classes: Vec<String>,

    /// Raw attributes retained for future selector expansion.
    pub attrs: Vec<(String, String)>,

    /// Ancestors from document root to immediate parent.
    pub ancestors: Vec<AncestorSignature>,
}

impl ElementSignature {
    /// Creates a synthetic signature, useful for loose text nodes.
    #[must_use]
    pub fn synthetic(tag: impl Into<String>, ancestors: &[AncestorSignature]) -> Self {
        Self {
            tag: tag.into().to_ascii_lowercase(),
            id: None,
            classes: Vec::new(),
            attrs: Vec::new(),
            ancestors: ancestors.to_vec(),
        }
    }

    /// Creates a signature from tag and attributes.
    #[must_use]
    pub fn from_attrs(
        tag: impl Into<String>,
        attrs: &[(String, String)],
        ancestors: &[AncestorSignature],
    ) -> Self {
        let tag = tag.into().to_ascii_lowercase();
        let id = attr_value(attrs, "id").map(|value| value.trim().to_ascii_lowercase());
        let classes = attr_value(attrs, "class")
            .map(parse_classes)
            .unwrap_or_default();

        Self {
            tag,
            id,
            classes,
            attrs: attrs.to_vec(),
            ancestors: ancestors.to_vec(),
        }
    }

    /// Returns a compact ancestor signature for this element.
    #[must_use]
    pub fn as_ancestor(&self) -> AncestorSignature {
        AncestorSignature {
            tag: self.tag.clone(),
            id: self.id.clone(),
            classes: self.classes.clone(),
        }
    }
}

/// Compact ancestor selector metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AncestorSignature {
    /// Ancestor tag.
    pub tag: String,

    /// Optional id.
    pub id: Option<String>,

    /// Lowercase class names.
    pub classes: Vec<String>,
}

/// Per-block selector-computed style overrides.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComputedNodeStyle {
    /// Optional text color override.
    pub text_color: Option<Color>,

    /// Optional font size override.
    pub font_size: Option<f32>,

    /// Optional box style override.
    pub box_style: BoxStyleLite,
}

impl ComputedNodeStyle {
    /// Returns true when this computed override has no useful data.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.text_color.is_none() && self.font_size.is_none() && self.box_style.is_empty()
    }
}

/// Parses CSS rules with selector/declaration retention.
#[must_use]
pub fn parse_css_rules_lite(source: &str, source_order_base: usize) -> Vec<StyleRuleLite> {
    let css = strip_comments(source);
    let mut rules = Vec::new();
    let mut cursor = 0usize;
    let mut rule_index = 0usize;

    while let Some(open_rel) = css[cursor..].find('{') {
        let open = cursor + open_rel;
        let selector_text = css[cursor..open].trim();

        let Some(close_rel) = css[open + 1..].find('}') else {
            break;
        };

        let close = open + 1 + close_rel;
        let declaration_text = &css[open + 1..close];
        cursor = close + 1;

        if selector_text.starts_with('@') {
            continue;
        }

        let selectors = selector_text
            .split(',')
            .filter_map(SelectorLite::parse)
            .collect::<Vec<_>>();
        let declarations = parse_declarations(declaration_text);

        if selectors.is_empty() || declarations.is_empty() {
            continue;
        }

        rules.push(StyleRuleLite {
            selectors,
            declarations,
            source_order: source_order_base.saturating_add(rule_index),
        });
        rule_index = rule_index.saturating_add(1);
    }

    rules
}

/// Computes per-node selector style overrides.
#[must_use]
pub fn compute_node_styles(
    nodes: &[ElementSignature],
    rules: &[StyleRuleLite],
) -> Vec<ComputedNodeStyle> {
    nodes
        .iter()
        .map(|node| compute_node_style(node, rules))
        .collect()
}

fn compute_node_style(node: &ElementSignature, rules: &[StyleRuleLite]) -> ComputedNodeStyle {
    let mut builder = NodeStyleBuilder::default();

    for rule in rules {
        let Some(selector) = rule
            .selectors
            .iter()
            .filter(|selector| selector.matches(node))
            .max_by_key(|selector| selector.specificity)
        else {
            continue;
        };

        for declaration in &rule.declarations {
            let priority = DeclarationPriority {
                specificity: selector.specificity,
                source_order: rule.source_order,
                declaration_order: declaration.declaration_order,
            };
            builder.apply(declaration, priority);
        }
    }

    builder.finish()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct DeclarationPriority {
    specificity: Specificity,
    source_order: usize,
    declaration_order: usize,
}

#[derive(Debug, Clone)]
struct CascadeSlot<T> {
    value: Option<T>,
    priority: Option<DeclarationPriority>,
}

impl<T> Default for CascadeSlot<T> {
    fn default() -> Self {
        Self {
            value: None,
            priority: None,
        }
    }
}

impl<T> CascadeSlot<T> {
    fn set_if_wins(&mut self, value: T, priority: DeclarationPriority) {
        if self.priority.map_or(true, |current| priority >= current) {
            self.value = Some(value);
            self.priority = Some(priority);
        }
    }

    fn into_option(self) -> Option<T> {
        self.value
    }
}

#[derive(Debug, Clone, Default)]
struct NodeStyleBuilder {
    text_color: CascadeSlot<Color>,
    font_size: CascadeSlot<f32>,
    margin: CascadeSlot<EdgeSizes>,
    padding: CascadeSlot<EdgeSizes>,
    border_width: CascadeSlot<EdgeSizes>,
    border_color: CascadeSlot<Color>,
    background: CascadeSlot<Color>,
    display: CascadeSlot<DisplayLite>,
}

impl NodeStyleBuilder {
    fn apply(&mut self, declaration: &CssDeclarationLite, priority: DeclarationPriority) {
        match declaration.property.as_str() {
            "color" => {
                if let Some(color) = parse_color_value(&declaration.value) {
                    self.text_color.set_if_wins(color, priority);
                }
            }
            "font-size" => {
                if let Some(size) = parse_font_size(&declaration.value, DEFAULT_FONT_SIZE) {
                    self.font_size.set_if_wins(size, priority);
                }
            }
            "background" | "background-color" => {
                if let Some(color) = parse_color_value(&declaration.value) {
                    self.background.set_if_wins(color, priority);
                }
            }
            "display" => {
                if let Some(display) = parse_display(&declaration.value) {
                    self.display.set_if_wins(display, priority);
                }
            }
            "margin" => {
                if let Some(edges) = parse_edges(&declaration.value) {
                    self.margin.set_if_wins(edges, priority);
                }
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                if let Some(px) = parse_px(&declaration.value) {
                    let mut current = self.margin.value.unwrap_or_else(EdgeSizes::zero);
                    apply_edge(&mut current, &declaration.property, px);
                    self.margin.set_if_wins(current, priority);
                }
            }
            "padding" => {
                if let Some(edges) = parse_edges(&declaration.value) {
                    self.padding.set_if_wins(edges, priority);
                }
            }
            "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
                if let Some(px) = parse_px(&declaration.value) {
                    let mut current = self.padding.value.unwrap_or_else(EdgeSizes::zero);
                    apply_edge(&mut current, &declaration.property, px);
                    self.padding.set_if_wins(current, priority);
                }
            }
            "border" => {
                if let Some(width) = first_px_token(&declaration.value) {
                    self.border_width
                        .set_if_wins(EdgeSizes::all(width), priority);
                }
                if let Some(color) = parse_color_value(&declaration.value) {
                    self.border_color.set_if_wins(color, priority);
                }
            }
            "border-width" => {
                if let Some(edges) = parse_edges(&declaration.value) {
                    self.border_width.set_if_wins(edges, priority);
                }
            }
            "border-color" => {
                if let Some(color) = parse_color_value(&declaration.value) {
                    self.border_color.set_if_wins(color, priority);
                }
            }
            _ => {}
        }
    }

    fn finish(self) -> ComputedNodeStyle {
        ComputedNodeStyle {
            text_color: self.text_color.into_option(),
            font_size: self.font_size.into_option(),
            box_style: BoxStyleLite {
                margin: self.margin.into_option(),
                padding: self.padding.into_option(),
                border_width: self.border_width.into_option(),
                border_color: self.border_color.into_option(),
                background: self.background.into_option(),
                display: self.display.into_option(),
            },
        }
    }
}

/// Parses raw declaration text into stable declaration records.
#[must_use]
pub fn parse_declarations_lite(source: &str) -> Vec<CssDeclarationLite> {
    parse_declarations(source)
}

fn parse_declarations(source: &str) -> Vec<CssDeclarationLite> {
    source
        .split(';')
        .enumerate()
        .filter_map(|(index, entry)| {
            let (property, value) = entry.split_once(':')?;
            let property = property.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();

            if property.is_empty() || value.is_empty() {
                None
            } else {
                Some(CssDeclarationLite {
                    property,
                    value,
                    declaration_order: index,
                })
            }
        })
        .collect()
}

/// Parses CSS color value tokens.
#[must_use]
pub fn parse_color_value(value: &str) -> Option<Color> {
    value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ')' | '('))
        .find_map(|token| {
            let cleaned = token.trim_matches(|ch: char| matches!(ch, ';' | ','));
            Color::from_css_hex(cleaned)
        })
}

/// Parses font size with px/em/rem support.
#[must_use]
pub fn parse_font_size(value: &str, base: f32) -> Option<f32> {
    let trimmed = value.trim().to_ascii_lowercase();

    if let Some(px) = parse_px(&trimmed) {
        return Some(px.max(1.0));
    }

    if let Some(raw) = trimmed
        .strip_suffix("rem")
        .or_else(|| trimmed.strip_suffix("em"))
    {
        let factor = raw.trim().parse::<f32>().ok()?;
        return Some((factor * base).max(1.0));
    }

    trimmed.parse::<f32>().ok().map(|size| size.max(1.0))
}

/// Parses a CSS px number or unitless number.
#[must_use]
pub fn parse_px(value: &str) -> Option<f32> {
    let trimmed = value.trim().to_ascii_lowercase();

    if trimmed.eq_ignore_ascii_case("auto") {
        return None;
    }

    if let Some(raw) = trimmed.strip_suffix("px") {
        return raw.trim().parse::<f32>().ok();
    }

    trimmed.parse::<f32>().ok()
}

/// Parses viewport fractions like `60vw` or `15vh`.
#[must_use]
pub fn parse_viewport_fraction(value: &str, unit: &str) -> Option<f32> {
    let trimmed = value.trim().to_ascii_lowercase();
    let raw = trimmed.strip_suffix(unit)?;
    let percentage = raw.trim().parse::<f32>().ok()?;

    Some((percentage / 100.0).clamp(0.0, 1.0))
}

fn parse_display(value: &str) -> Option<DisplayLite> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(DisplayLite::None),
        "inline" | "inline-block" => Some(DisplayLite::Inline),
        "block" | "flex" | "grid" | "table" | "list-item" => Some(DisplayLite::Block),
        _ => None,
    }
}

fn parse_edges(value: &str) -> Option<EdgeSizes> {
    let values = value
        .split_whitespace()
        .filter_map(parse_px)
        .collect::<Vec<_>>();

    match values.as_slice() {
        [all] => Some(EdgeSizes::all(*all)),
        [vertical, horizontal] => Some(EdgeSizes::new(
            *vertical,
            *horizontal,
            *vertical,
            *horizontal,
        )),
        [top, horizontal, bottom] => Some(EdgeSizes::new(*top, *horizontal, *bottom, *horizontal)),
        [top, right, bottom, left, ..] => Some(EdgeSizes::new(*top, *right, *bottom, *left)),
        _ => None,
    }
}

fn first_px_token(value: &str) -> Option<f32> {
    value.split_whitespace().find_map(parse_px)
}

fn apply_edge(edges: &mut EdgeSizes, property: &str, value: f32) {
    match property {
        "margin-top" | "padding-top" => edges.top = value,
        "margin-right" | "padding-right" => edges.right = value,
        "margin-bottom" | "padding-bottom" => edges.bottom = value,
        "margin-left" | "padding-left" => edges.left = value,
        _ => {}
    }
}

fn parse_classes(value: &str) -> Vec<String> {
    value
        .split_ascii_whitespace()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
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

fn normalize_selector_text(selector: &str) -> String {
    selector
        .split_whitespace()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

fn strip_pseudo_and_attributes(input: &str) -> &str {
    let attr_index = input.find('[').unwrap_or(input.len());
    let pseudo_index = input.find(':').unwrap_or(input.len());
    &input[..attr_index.min(pseudo_index)]
}

const fn is_selector_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')
}

fn strip_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;

    loop {
        let Some(start) = rest.find("/*") else {
            output.push_str(rest);
            break;
        };

        output.push_str(&rest[..start]);

        let Some(end_rel) = rest[start + 2..].find("*/") else {
            break;
        };

        rest = &rest[start + 2 + end_rel + 2..];
    }

    output
}
