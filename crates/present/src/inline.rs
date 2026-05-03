#![doc = "Inline-flow primitives for mixed text and links."]

/// A lightweight inline item extracted from HTML text-level content.
///
/// Module 19 deliberately keeps this small: text and links are enough to give
/// Sylphos real paragraph inline flow without pretending to be a complete DOM
/// inline formatting context. Images and controls remain blockified elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineFragment {
    /// Plain text fragment.
    Text {
        /// Normalized visible text.
        text: String,
    },

    /// Linked text fragment.
    Link {
        /// Normalized visible text.
        text: String,

        /// Optional href exactly as extracted from HTML.
        href: Option<String>,
    },
}

impl InlineFragment {
    /// Creates a text fragment when the text is non-empty after trimming.
    #[must_use]
    pub fn text(value: impl Into<String>) -> Option<Self> {
        let text = normalize_fragment_text(&value.into());
        if text.is_empty() {
            None
        } else {
            Some(Self::Text { text })
        }
    }

    /// Creates a link fragment when either visible text or href exists.
    #[must_use]
    pub fn link(value: impl Into<String>, href: Option<String>) -> Option<Self> {
        let text = normalize_fragment_text(&value.into());
        if text.is_empty() && href.as_deref().unwrap_or_default().trim().is_empty() {
            None
        } else {
            Some(Self::Link { text, href })
        }
    }

    /// Returns visible text for this fragment.
    #[must_use]
    pub fn text_content(&self) -> &str {
        match self {
            Self::Text { text } | Self::Link { text, .. } => text,
        }
    }

    /// Returns href when this fragment is a link.
    #[must_use]
    pub fn href(&self) -> Option<&str> {
        match self {
            Self::Text { .. } => None,
            Self::Link { href, .. } => href.as_deref(),
        }
    }

    /// Returns `true` when the fragment is link-like.
    #[must_use]
    pub const fn is_link(&self) -> bool {
        matches!(self, Self::Link { .. })
    }

    /// Returns a stable plain-text projection.
    #[must_use]
    pub fn plain_text(fragments: &[Self]) -> String {
        let mut output = String::new();
        for fragment in fragments {
            let text = fragment.text_content().trim();
            if text.is_empty() {
                continue;
            }
            if !output.is_empty() {
                output.push(' ');
            }
            output.push_str(text);
        }
        output
    }

    /// Returns whether this fragment list contains at least one link.
    #[must_use]
    pub fn contains_link(fragments: &[Self]) -> bool {
        fragments.iter().any(Self::is_link)
    }
}

fn normalize_fragment_text(input: &str) -> String {
    let mut output = String::new();
    for part in input.split_whitespace() {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(part);
    }
    output
}
