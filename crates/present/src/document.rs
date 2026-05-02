#![doc = "Lightweight render document structures."]

use crate::{Color, StyleSheetLite};

/// Extracted document model used by the present layer.
///
/// This is intentionally much smaller than a real browser DOM. It contains only
/// the information needed for the current Sylphos render pipeline.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderDocument {
    /// Optional document title extracted from `<title>`.
    pub title: Option<String>,

    /// Optional theme color extracted from `<meta name="theme-color">`.
    pub theme_color: Option<Color>,

    /// Minimal style sheet extracted from inline `<style>` blocks.
    pub style_sheet: StyleSheetLite,

    /// Ordered render blocks extracted from visible document content.
    pub blocks: Vec<RenderBlock>,
}

impl RenderDocument {
    /// Creates an empty render document.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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

    /// Image placeholder block.
    Image {
        /// Optional `alt` attribute.
        alt: Option<String>,

        /// Optional `src` attribute.
        src: Option<String>,
    },

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
