#![doc = "Lightweight render document structures."]

use crate::selector::{ComputedNodeStyle, ElementSignature, StyleRuleLite};
use crate::styled::{compute_styled_document, StyledDocument};
use crate::stylesheet::{StyleSourceLite, StylesheetLink};
use crate::{Color, FormBlock, InlineFragment, StyleSheetLite};

/// Extracted document model used by the presentation layer.
///
/// This remains much smaller than a browser DOM, but Module 18 adds selector-aware
/// block metadata and a computed style tree so layout can apply tag, class, id,
/// and descendant CSS rules without throwing away previous modules. Imagine that:
/// structure surviving extraction. Wild concept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderDocument {
    /// Optional document title extracted from `<title>`.
    pub title: Option<String>,

    /// Optional theme color extracted from `<meta name="theme-color">`.
    pub theme_color: Option<Color>,

    /// Minimal global style sheet extracted from ordered inline and external sources.
    pub style_sheet: StyleSheetLite,

    /// Selector-aware CSS rules used for computed per-block styles.
    pub style_rules: Vec<StyleRuleLite>,

    /// Ordered stylesheet sources discovered in the document.
    pub style_sources: Vec<StyleSourceLite>,

    /// Convenience list of external stylesheets discovered in the document.
    pub external_stylesheets: Vec<StylesheetLink>,

    /// Ordered render blocks extracted from visible document content.
    pub blocks: Vec<RenderBlock>,

    /// Element signatures aligned with `blocks` by index.
    pub block_elements: Vec<ElementSignature>,

    /// Computed selector-aware style tree aligned with `blocks`.
    pub style_tree: StyledDocument,
}

impl RenderDocument {
    /// Creates an empty render document.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the computed global stylesheet after external CSS has loaded.
    pub fn set_style_sheet(&mut self, style_sheet: StyleSheetLite) {
        self.style_sheet = style_sheet;
        self.recompute_style_tree();
    }

    /// Replaces global style and selector rules after external CSS has loaded.
    pub fn set_style_sheet_and_rules(
        &mut self,
        style_sheet: StyleSheetLite,
        style_rules: Vec<StyleRuleLite>,
    ) {
        self.style_sheet = style_sheet;
        self.style_rules = style_rules;
        self.recompute_style_tree();
    }

    /// Pushes a render block and its selector-matching element signature.
    pub fn push_block(&mut self, block: RenderBlock, element: ElementSignature) {
        self.blocks.push(block);
        self.block_elements.push(element);
    }

    /// Recomputes selector-aware block styles.
    pub fn recompute_style_tree(&mut self) {
        self.style_tree =
            compute_styled_document(&self.blocks, &self.block_elements, &self.style_rules);
    }

    /// Returns selector-computed style for one block index.
    #[must_use]
    pub fn computed_style_for_block(&self, block_index: usize) -> Option<&ComputedNodeStyle> {
        self.style_tree.style_for_block(block_index)
    }
}

/// Coarse presentation block extracted from HTML.
///
/// These variants are deliberately simple so the GPU layer can consume them
/// without knowing about the original HTML parser internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderBlock {
    /// Heading block for `h1` through `h6`.
    Heading {
        /// Heading level in the `1..=6` range.
        level: u8,

        /// Normalized heading text.
        text: String,
    },

    /// Paragraph block.
    Paragraph {
        /// Normalized paragraph text.
        text: String,
    },

    /// Link block.
    Link {
        /// Normalized link text.
        text: String,

        /// Optional `href` attribute.
        href: Option<String>,
    },

    /// Paragraph-like block containing mixed inline fragments.
    ///
    /// This is used for content such as `Hello <a>world</a>`, where the old
    /// block extractor would flatten the link and lose click targets.
    InlineFlow {
        /// Ordered inline fragments.
        fragments: Vec<InlineFragment>,
    },

    /// Image placeholder block.
    Image {
        /// Optional `alt` attribute.
        alt: Option<String>,

        /// Optional `src` attribute.
        src: Option<String>,
    },

    /// Minimal form block with text controls and submit buttons.
    Form(FormBlock),

    /// Fallback visible block for supported-but-not-specialized tags.
    Generic {
        /// Lowercase tag name.
        tag: String,

        /// Normalized visible text.
        text: String,
    },
}

/// Small text wrapper reserved for future inline text planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderText {
    /// Normalized text content.
    pub text: String,
}
