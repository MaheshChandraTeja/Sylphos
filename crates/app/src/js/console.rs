//! Console capture for the JavaScript host.

/// Console severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsoleLevel {
    /// `console.log`.
    Log,

    /// `console.info`.
    Info,

    /// `console.warn`.
    Warn,

    /// `console.error`.
    Error,
}

impl ConsoleLevel {
    /// Stable log label.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Captured console output from a script execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsoleMessage {
    /// Severity.
    pub level: ConsoleLevel,

    /// Message text.
    pub text: String,

    /// Script URL or inline label.
    pub source_name: String,

    /// 1-based line number when known.
    pub line: usize,
}

impl ConsoleMessage {
    /// Creates a console message.
    #[must_use]
    pub(crate) fn new(
        level: ConsoleLevel,
        text: impl Into<String>,
        source_name: impl Into<String>,
        line: usize,
    ) -> Self {
        Self {
            level,
            text: text.into(),
            source_name: source_name.into(),
            line: line.max(1),
        }
    }
}
