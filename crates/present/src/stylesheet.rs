#![doc = "Stylesheet source descriptors extracted from HTML."]

/// External stylesheet reference discovered from `<link rel="stylesheet">`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StylesheetLink {
    /// Raw href exactly as found in the document.
    pub href: String,

    /// Optional media attribute.
    pub media: Option<String>,

    /// Whether the stylesheet was disabled in markup.
    pub disabled: bool,

    /// Source-order index in the document.
    pub source_order: usize,
}

impl StylesheetLink {
    /// Returns true when this link should be considered for the current screen renderer.
    #[must_use]
    pub fn applies_to_screen(&self) -> bool {
        if self.disabled {
            return false;
        }

        let Some(media) = self.media.as_deref() else {
            return true;
        };

        let normalized = media.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return true;
        }

        normalized
            .split(',')
            .map(str::trim)
            .any(|item| item == "all" || item == "screen")
    }
}

/// Ordered stylesheet source from the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleSourceLite {
    /// Inline CSS from a `<style>` block.
    Inline {
        /// CSS source text.
        css: String,

        /// Source-order index in the document.
        source_order: usize,
    },

    /// External stylesheet link.
    External(StylesheetLink),
}

impl StyleSourceLite {
    /// Source-order index for stable cascade ordering.
    #[must_use]
    pub const fn source_order(&self) -> usize {
        match self {
            Self::Inline { source_order, .. } => *source_order,
            Self::External(link) => link.source_order,
        }
    }
}
