#![doc = "CSS Object Model-lite primitives for Sylphos."]

use crate::{parse_css_lite, RenderDocument, StyleSheetLite};

/// Property name used by the CSSOM-lite layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CssPropertyName(String);

impl CssPropertyName {
    /// Creates a normalized CSS property name.
    #[must_use]
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().trim().to_ascii_lowercase())
    }

    /// Returns the normalized name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns true if this property can affect style or layout in the current engine.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        matches!(
            self.0.as_str(),
            "background"
                | "background-color"
                | "color"
                | "display"
                | "font-size"
                | "width"
                | "height"
                | "min-width"
                | "max-width"
                | "min-height"
                | "max-height"
                | "margin"
                | "margin-top"
                | "margin-right"
                | "margin-bottom"
                | "margin-left"
                | "padding"
                | "padding-top"
                | "padding-right"
                | "padding-bottom"
                | "padding-left"
                | "border"
                | "border-width"
                | "border-color"
                | "border-style"
                | "opacity"
                | "visibility"
        )
    }
}

/// One CSS declaration in a CSSOM-lite rule or inline style mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssDeclarationLite {
    /// Normalized property name.
    pub property: CssPropertyName,

    /// Raw declaration value.
    pub value: String,

    /// Whether the declaration was marked as `!important`.
    pub important: bool,
}

impl CssDeclarationLite {
    /// Creates a declaration from raw source parts.
    #[must_use]
    pub fn new(property: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        let raw = value.as_ref().trim();
        let lower = raw.to_ascii_lowercase();
        let important = lower.ends_with("!important");
        let cleaned = if important {
            raw[..raw.len().saturating_sub("!important".len())]
                .trim()
                .to_owned()
        } else {
            raw.to_owned()
        };

        Self {
            property: CssPropertyName::new(property),
            value: cleaned,
            important,
        }
    }

    /// Serializes the declaration into CSS source.
    #[must_use]
    pub fn to_css(&self) -> String {
        if self.important {
            format!("{}: {} !important;", self.property.as_str(), self.value)
        } else {
            format!("{}: {};", self.property.as_str(), self.value)
        }
    }
}

/// One CSSOM-lite style rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssRuleLite {
    /// Selector list for this rule.
    pub selectors: Vec<String>,

    /// Declarations in source order.
    pub declarations: Vec<CssDeclarationLite>,

    /// Rule source order in the owning sheet.
    pub source_order: u32,
}

impl CssRuleLite {
    /// Creates a rule.
    #[must_use]
    pub const fn new(
        selectors: Vec<String>,
        declarations: Vec<CssDeclarationLite>,
        source_order: u32,
    ) -> Self {
        Self {
            selectors,
            declarations,
            source_order,
        }
    }

    /// Serializes this rule to CSS source.
    #[must_use]
    pub fn to_css(&self) -> String {
        let selectors = self.selectors.join(", ");
        let declarations = self
            .declarations
            .iter()
            .map(CssDeclarationLite::to_css)
            .collect::<Vec<_>>()
            .join(" ");

        format!("{selectors} {{ {declarations} }}")
    }
}

/// A mutable CSSOM-lite stylesheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssStyleSheetLite {
    /// Human-readable origin, usually `inline`, a URL, or `dynamic`.
    pub origin: String,

    /// Whether the stylesheet is disabled.
    pub disabled: bool,

    /// Style rules.
    pub rules: Vec<CssRuleLite>,
}

impl CssStyleSheetLite {
    /// Creates an empty sheet.
    #[must_use]
    pub fn new(origin: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
            disabled: false,
            rules: Vec::new(),
        }
    }

    /// Parses a source string into a mutable CSSOM-lite sheet.
    #[must_use]
    pub fn parse(origin: impl Into<String>, source: &str) -> Self {
        let mut sheet = Self::new(origin);
        let mut source_order = 0_u32;
        let css = strip_comments(source);
        let mut cursor = 0_usize;

        while let Some(open_rel) = css[cursor..].find('{') {
            let open = cursor + open_rel;
            let selector_text = css[cursor..open].trim();
            let Some(close_rel) = css[open + 1..].find('}') else {
                break;
            };
            let close = open + 1 + close_rel;
            let declarations = parse_declarations(&css[open + 1..close]);
            let selectors = selector_text
                .split(',')
                .map(str::trim)
                .filter(|selector| !selector.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();

            if !selectors.is_empty() && !declarations.is_empty() {
                sheet
                    .rules
                    .push(CssRuleLite::new(selectors, declarations, source_order));
                source_order = source_order.saturating_add(1);
            }

            cursor = close + 1;
        }

        sheet
    }

    /// Appends a rule to the sheet.
    pub fn insert_rule(
        &mut self,
        selector: impl AsRef<str>,
        declarations: Vec<CssDeclarationLite>,
    ) {
        if declarations.is_empty() {
            return;
        }

        let selectors = selector
            .as_ref()
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        if selectors.is_empty() {
            return;
        }

        let source_order = u32::try_from(self.rules.len()).unwrap_or(u32::MAX);
        self.rules
            .push(CssRuleLite::new(selectors, declarations, source_order));
    }

    /// Deletes a rule by index.
    pub fn delete_rule(&mut self, index: usize) -> bool {
        if index >= self.rules.len() {
            return false;
        }
        self.rules.remove(index);
        for (order, rule) in self.rules.iter_mut().enumerate() {
            rule.source_order = u32::try_from(order).unwrap_or(u32::MAX);
        }
        true
    }

    /// Serializes the sheet to CSS source.
    #[must_use]
    pub fn to_css(&self) -> String {
        if self.disabled {
            return String::new();
        }

        self.rules
            .iter()
            .map(CssRuleLite::to_css)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Converts this CSSOM sheet into the existing `StyleSheetLite` summary.
    #[must_use]
    pub fn to_style_sheet_lite(&self) -> StyleSheetLite {
        parse_css_lite(&self.to_css())
    }
}

/// Dynamic CSSOM mutation emitted by scripts or host APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssomMutation {
    /// Insert a new stylesheet.
    InsertStyleSheet {
        /// Stable stylesheet origin or diagnostic source label.
        origin: String,

        /// Stylesheet source text to insert.
        css: String,
    },

    /// Enable or disable a stylesheet by origin.
    SetStyleSheetDisabled {
        /// Stable stylesheet origin or diagnostic source label.
        origin: String,

        /// Whether matching stylesheets should be disabled.
        disabled: bool,
    },

    /// Insert a rule into the dynamic sheet.
    InsertRule {
        /// Selector text for the inserted rule.
        selector: String,

        /// Declarations attached to the inserted rule.
        declarations: Vec<CssDeclarationLite>,
    },

    /// Set an inline style declaration on a selector target.
    SetInlineStyle {
        /// Selector identifying the target elements.
        selector: String,

        /// CSS property to set.
        property: CssPropertyName,

        /// CSS value to assign.
        value: String,

        /// Whether this inline declaration is important.
        important: bool,
    },

    /// Remove an inline style declaration on a selector target.
    RemoveInlineStyle {
        /// Selector identifying the target elements.
        selector: String,

        /// CSS property to remove.
        property: CssPropertyName,
    },

    /// Add a class to elements matching selector.
    AddClass {
        /// Selector identifying the target elements.
        selector: String,

        /// Class name to add.
        class_name: String,
    },

    /// Remove a class from elements matching selector.
    RemoveClass {
        /// Selector identifying the target elements.
        selector: String,

        /// Class name to remove.
        class_name: String,
    },

    /// Toggle a class on elements matching selector.
    ToggleClass {
        /// Selector identifying the target elements.
        selector: String,

        /// Class name to toggle.
        class_name: String,
    },
}

/// CSSOM-lite engine state attached to a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssomEngine {
    revision: u64,
    sheets: Vec<CssStyleSheetLite>,
    dynamic_sheet: CssStyleSheetLite,
    inline_rules: Vec<CssRuleLite>,
    class_mutations: Vec<CssomMutation>,
}

impl Default for CssomEngine {
    fn default() -> Self {
        Self {
            revision: 0,
            sheets: Vec::new(),
            dynamic_sheet: CssStyleSheetLite::new("dynamic"),
            inline_rules: Vec::new(),
            class_mutations: Vec::new(),
        }
    }
}

impl CssomEngine {
    /// Creates a new engine from inline/external CSS sources.
    #[must_use]
    pub fn from_sources(sources: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut engine = Self::default();
        for (origin, css) in sources {
            engine.add_sheet(CssStyleSheetLite::parse(origin, &css));
        }
        engine
    }

    /// Returns the current CSSOM revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns immutable stylesheets.
    #[must_use]
    pub fn sheets(&self) -> &[CssStyleSheetLite] {
        &self.sheets
    }

    /// Adds a sheet.
    pub fn add_sheet(&mut self, sheet: CssStyleSheetLite) {
        self.sheets.push(sheet);
        self.bump();
    }

    /// Applies one mutation.
    pub fn apply_mutation(&mut self, mutation: CssomMutation) -> CssomInvalidation {
        let invalidation = CssomInvalidation::from_mutation(&mutation);

        match mutation {
            CssomMutation::InsertStyleSheet { origin, css } => {
                self.sheets.push(CssStyleSheetLite::parse(origin, &css));
            }
            CssomMutation::SetStyleSheetDisabled { origin, disabled } => {
                for sheet in &mut self.sheets {
                    if sheet.origin == origin {
                        sheet.disabled = disabled;
                    }
                }
            }
            CssomMutation::InsertRule {
                selector,
                declarations,
            } => {
                self.dynamic_sheet.insert_rule(selector, declarations);
            }
            CssomMutation::SetInlineStyle {
                selector,
                property,
                value,
                important,
            } => self.set_inline_style(selector, property, value, important),
            CssomMutation::RemoveInlineStyle { selector, property } => {
                self.remove_inline_style(&selector, &property);
            }
            mutation @ (CssomMutation::AddClass { .. }
            | CssomMutation::RemoveClass { .. }
            | CssomMutation::ToggleClass { .. }) => {
                self.class_mutations.push(mutation);
            }
        }

        self.bump();
        invalidation
    }

    /// Applies many mutations and combines invalidation.
    pub fn apply_mutations(
        &mut self,
        mutations: impl IntoIterator<Item = CssomMutation>,
    ) -> CssomInvalidation {
        let mut combined = CssomInvalidation::none();
        for mutation in mutations {
            combined.merge(self.apply_mutation(mutation));
        }
        combined
    }

    /// Merges all active sheets into a `StyleSheetLite` summary consumed by the current layout engine.
    #[must_use]
    pub fn compute_summary_sheet(&self, base: StyleSheetLite) -> StyleSheetLite {
        let mut output = base;

        for sheet in &self.sheets {
            if !sheet.disabled {
                output.merge_from(sheet.to_style_sheet_lite());
            }
        }

        if !self.dynamic_sheet.disabled {
            output.merge_from(self.dynamic_sheet.to_style_sheet_lite());
        }

        if !self.inline_rules.is_empty() {
            let mut inline_sheet = CssStyleSheetLite::new("inline-style");
            inline_sheet.rules.clone_from(&self.inline_rules);
            output.merge_from(inline_sheet.to_style_sheet_lite());
        }

        output
    }

    /// Applies the CSSOM summary to a render document in-place.
    pub fn apply_to_document(&self, document: &mut RenderDocument) {
        document.style_sheet = self.compute_summary_sheet(document.style_sheet);
    }

    fn set_inline_style(
        &mut self,
        selector: String,
        property: CssPropertyName,
        value: String,
        important: bool,
    ) {
        let source_order = u32::try_from(self.inline_rules.len()).unwrap_or(u32::MAX);
        self.inline_rules.push(CssRuleLite::new(
            vec![selector],
            vec![CssDeclarationLite {
                property,
                value,
                important,
            }],
            source_order,
        ));
    }

    fn remove_inline_style(&mut self, selector: &str, property: &CssPropertyName) {
        for rule in &mut self.inline_rules {
            if rule.selectors.iter().any(|candidate| candidate == selector) {
                rule.declarations
                    .retain(|declaration| &declaration.property != property);
            }
        }
        self.inline_rules
            .retain(|rule| !rule.declarations.is_empty());
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

/// Invalidation emitted by CSSOM changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CssomInvalidation {
    /// Style must be recomputed.
    pub style: bool,

    /// Layout must be recomputed.
    pub layout: bool,

    /// Paint must be rebuilt.
    pub paint: bool,
}

impl CssomInvalidation {
    /// No invalidation.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            style: false,
            layout: false,
            paint: false,
        }
    }

    /// Full style/layout/paint invalidation.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            style: true,
            layout: true,
            paint: true,
        }
    }

    /// Paint-only invalidation.
    #[must_use]
    pub const fn paint_only() -> Self {
        Self {
            style: true,
            layout: false,
            paint: true,
        }
    }

    /// Combines invalidation flags.
    pub fn merge(&mut self, other: Self) {
        self.style |= other.style;
        self.layout |= other.layout;
        self.paint |= other.paint;
    }

    #[must_use]
    fn from_mutation(mutation: &CssomMutation) -> Self {
        match mutation {
            CssomMutation::SetInlineStyle { property, .. }
            | CssomMutation::RemoveInlineStyle { property, .. } => {
                if layout_affecting_property(property.as_str()) {
                    Self::full()
                } else {
                    Self::paint_only()
                }
            }
            _ => Self::full(),
        }
    }
}

impl Default for CssomInvalidation {
    fn default() -> Self {
        Self::none()
    }
}

/// Applies a CSSOM engine to a document and returns the invalidation category.
pub fn apply_cssom_to_render_document(
    document: &mut RenderDocument,
    engine: &CssomEngine,
) -> CssomInvalidation {
    engine.apply_to_document(document);
    CssomInvalidation::full()
}

fn parse_declarations(source: &str) -> Vec<CssDeclarationLite> {
    source
        .split(';')
        .filter_map(|entry| {
            let (property, value) = entry.split_once(':')?;
            let declaration = CssDeclarationLite::new(property, value);
            declaration.property.is_supported().then_some(declaration)
        })
        .collect()
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

fn layout_affecting_property(property: &str) -> bool {
    matches!(
        property,
        "display"
            | "font-size"
            | "width"
            | "height"
            | "min-width"
            | "max-width"
            | "min-height"
            | "max-height"
            | "margin"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "padding"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
            | "border"
            | "border-width"
            | "border-style"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn parses_cssom_sheet_and_computes_summary() {
        let sheet = CssStyleSheetLite::parse(
            "inline",
            "body { background: #eee; color: #111; } a { color: #348; }",
        );
        let summary = sheet.to_style_sheet_lite();

        assert_eq!(summary.body_background, Color::from_css_hex("#eee"));
        assert_eq!(summary.body_color, Color::from_css_hex("#111"));
        assert_eq!(summary.link_color, Color::from_css_hex("#348"));
    }

    #[test]
    fn cssom_mutation_updates_revision() {
        let mut engine = CssomEngine::default();
        let revision = engine.revision();
        let invalidation = engine.apply_mutation(CssomMutation::SetInlineStyle {
            selector: "body".to_owned(),
            property: CssPropertyName::new("background-color"),
            value: "#fff".to_owned(),
            important: false,
        });

        assert!(engine.revision() > revision);
        assert!(invalidation.paint);
    }
}
