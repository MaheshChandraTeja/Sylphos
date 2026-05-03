#![doc = "Diagnostics for SylJS lexing and parsing."]

use crate::Span;
use thiserror::Error;

/// High-level diagnostic kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// Lexer error.
    Lex,

    /// Parser error.
    Parse,
}

/// Structured diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Diagnostic kind.
    pub kind: DiagnosticKind,

    /// Human-readable message.
    pub message: String,

    /// Source span.
    pub span: Span,
}

impl Diagnostic {
    /// Creates a lexer diagnostic.
    #[must_use]
    pub fn lex(message: impl Into<String>, span: Span) -> Self {
        Self {
            kind: DiagnosticKind::Lex,
            message: message.into(),
            span,
        }
    }

    /// Creates a parser diagnostic.
    #[must_use]
    pub fn parse(message: impl Into<String>, span: Span) -> Self {
        Self {
            kind: DiagnosticKind::Parse,
            message: message.into(),
            span,
        }
    }
}

/// SylJS frontend error.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message} at {start}..{end}")]
pub struct SylJsError {
    /// Human-readable message.
    pub message: String,

    /// Start byte offset.
    pub start: usize,

    /// End byte offset.
    pub end: usize,

    /// Diagnostics emitted before failure.
    pub diagnostics: Vec<Diagnostic>,
}

impl SylJsError {
    /// Creates an error from one diagnostic.
    #[must_use]
    pub fn from_diagnostic(diagnostic: Diagnostic) -> Self {
        Self {
            message: diagnostic.message.clone(),
            start: diagnostic.span.start,
            end: diagnostic.span.end,
            diagnostics: vec![diagnostic],
        }
    }

    /// Creates an error from many diagnostics.
    #[must_use]
    pub fn from_diagnostics(diagnostics: Vec<Diagnostic>) -> Self {
        let first = diagnostics.first().cloned().unwrap_or_else(|| {
            Diagnostic::parse("unknown SylJS frontend error", crate::Span::default())
        });

        Self {
            message: first.message,
            start: first.span.start,
            end: first.span.end,
            diagnostics,
        }
    }
}
