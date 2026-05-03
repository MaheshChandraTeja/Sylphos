#![doc = "Source identity and span utilities."]

use serde::{Deserialize, Serialize};

/// Logical source identifier used by diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct SourceId(pub u32);

/// Byte-oriented source span.
///
/// Spans are half-open: `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Span {
    /// Source file/script id.
    pub source: SourceId,

    /// Start byte offset.
    pub start: usize,

    /// End byte offset.
    pub end: usize,
}

impl Span {
    /// Creates a new span.
    #[must_use]
    pub const fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self { source, start, end }
    }

    /// Creates a zero-width span.
    #[must_use]
    pub const fn point(source: SourceId, offset: usize) -> Self {
        Self {
            source,
            start: offset,
            end: offset,
        }
    }

    /// Returns a span covering both inputs.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self {
            source: self.source,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Returns true when the span has zero width.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Returns the byte length.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}
