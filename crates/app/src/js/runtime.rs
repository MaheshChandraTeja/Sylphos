//! Script runtime host.
//!
//! This module is intentionally architected around a runtime boundary rather
//! than scattering script behavior across the browser shell. The first engine is
//! a conservative intrinsic executor that safely recognizes common bootstrap
//! effects (`console.*`, `document.title`, and location navigation requests).
//! Later modules can replace the executor internals with V8 while preserving
//! the host API, diagnostics, and resource pipeline integration.

use crate::js::{
    capture_dom_binding_effects, capture_media_canvas_worker_effects, capture_web_platform_effects,
    ConsoleLevel, ConsoleMessage, DomBindingEffect, MediaCanvasWorkerEffect, WebApiEffect,
};
use tracing::{debug, warn};

/// Executable script unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptProgram {
    /// Script source code.
    pub source: String,

    /// URL or synthetic inline label.
    pub source_name: String,

    /// Source order in the page.
    pub source_order: usize,

    /// Whether this was fetched externally.
    pub external: bool,
}

impl ScriptProgram {
    /// Creates a script program.
    #[must_use]
    pub(crate) fn new(
        source: impl Into<String>,
        source_name: impl Into<String>,
        source_order: usize,
        external: bool,
    ) -> Self {
        Self {
            source: source.into(),
            source_name: source_name.into(),
            source_order,
            external,
        }
    }
}

/// Runtime side effect emitted by script execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeEffect {
    /// `document.title = ...` requested a title change.
    SetDocumentTitle(String),

    /// `location.href = ...` or equivalent requested navigation.
    RequestNavigation(String),
}

/// Result of one script execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScriptExecution {
    /// Console messages captured during execution.
    pub console: Vec<ConsoleMessage>,

    /// Effects emitted during execution.
    pub effects: Vec<RuntimeEffect>,

    /// DOM binding effects captured during execution.
    pub dom_effects: Vec<DomBindingEffect>,

    /// Web Platform API effects captured during execution.
    pub web_api_effects: Vec<WebApiEffect>,

    /// Media/canvas/worker effects captured during execution.
    pub media_effects: Vec<MediaCanvasWorkerEffect>,

    /// Event listeners registered by this script.
    pub registered_listeners: usize,

    /// Script-originated event dispatches queued by this script.
    pub queued_events: usize,

    /// Non-fatal runtime warnings.
    pub warnings: Vec<String>,

    /// Fatal errors. The intrinsic executor is intentionally forgiving, so these
    /// mostly represent oversized or structurally invalid host inputs.
    pub errors: Vec<String>,
}

impl ScriptExecution {
    /// Returns true if execution produced no fatal errors.
    #[must_use]
    pub(crate) fn is_success(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Per-document JavaScript host runtime.
#[derive(Debug, Clone)]
pub(crate) struct JavaScriptRuntime {
    document_url: String,
    executed: usize,
    console: Vec<ConsoleMessage>,
    effects: Vec<RuntimeEffect>,
    dom_effects: Vec<DomBindingEffect>,
    web_api_effects: Vec<WebApiEffect>,
    media_effects: Vec<MediaCanvasWorkerEffect>,
    warnings: Vec<String>,
    errors: Vec<String>,
}

impl JavaScriptRuntime {
    /// Creates a new runtime for one document navigation.
    #[must_use]
    pub(crate) fn new(document_url: impl Into<String>) -> Self {
        Self {
            document_url: document_url.into(),
            executed: 0,
            console: Vec::new(),
            effects: Vec::new(),
            dom_effects: Vec::new(),
            web_api_effects: Vec::new(),
            media_effects: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Executes a script program.
    ///
    /// The current executor is safe and side-effect-limited. It does not expose
    /// arbitrary host objects yet. DOM bindings, timers, Fetch/XHR, storage, and
    /// real ECMAScript evaluation are intentionally scheduled for Modules 24-27.
    pub(crate) fn execute(&mut self, program: &ScriptProgram) -> ScriptExecution {
        self.executed = self.executed.saturating_add(1);
        debug!(
            source = %program.source_name,
            bytes = program.source.len(),
            external = program.external,
            source_order = program.source_order,
            "executing JavaScript program"
        );

        let mut execution = ScriptExecution::default();
        let clean = strip_js_comments(&program.source);

        capture_console_calls(&clean, program, &mut execution);
        capture_document_title_assignments(&clean, &mut execution);
        capture_location_assignments(&clean, &mut execution);
        let dom_capture = capture_dom_binding_effects(&clean);
        execution.registered_listeners = dom_capture.registered_listeners;
        execution.queued_events = dom_capture.queued_events;
        execution.dom_effects.extend(dom_capture.effects);
        execution.warnings.extend(dom_capture.warnings);

        let web_capture = capture_web_platform_effects(&clean);
        execution.web_api_effects.extend(web_capture.effects);
        execution.warnings.extend(web_capture.warnings);

        let media_capture = capture_media_canvas_worker_effects(&clean);
        execution.media_effects.extend(media_capture.effects);
        execution.warnings.extend(media_capture.warnings);

        if execution.console.is_empty()
            && execution.effects.is_empty()
            && execution.dom_effects.is_empty()
            && execution.web_api_effects.is_empty()
            && execution.media_effects.is_empty()
            && looks_like_active_javascript(&clean)
        {
            execution.warnings.push(
                "script contains active JavaScript that requires Module 24+ DOM bindings or a full JS engine"
                    .to_owned(),
            );
        }

        for message in &execution.console {
            match message.level {
                ConsoleLevel::Log | ConsoleLevel::Info => debug!(
                    source = %message.source_name,
                    line = message.line,
                    level = message.level.as_str(),
                    message = %message.text,
                    "js console"
                ),
                ConsoleLevel::Warn => warn!(
                    source = %message.source_name,
                    line = message.line,
                    message = %message.text,
                    "js console warning"
                ),
                ConsoleLevel::Error => warn!(
                    source = %message.source_name,
                    line = message.line,
                    message = %message.text,
                    "js console error"
                ),
            }
        }

        self.console.extend(execution.console.clone());
        self.effects.extend(execution.effects.clone());
        self.dom_effects.extend(execution.dom_effects.clone());
        self.web_api_effects
            .extend(execution.web_api_effects.clone());
        self.media_effects.extend(execution.media_effects.clone());
        self.warnings.extend(execution.warnings.clone());
        self.errors.extend(execution.errors.clone());

        execution
    }

    /// Returns the latest requested document title, if any.
    #[must_use]
    pub(crate) fn latest_title_override(&self) -> Option<String> {
        self.effects.iter().rev().find_map(|effect| match effect {
            RuntimeEffect::SetDocumentTitle(title) => Some(title.clone()),
            RuntimeEffect::RequestNavigation(_) => None,
        })
    }

    /// Returns requested navigation effects.
    #[must_use]
    pub(crate) fn navigation_requests(&self) -> Vec<String> {
        self.effects
            .iter()
            .filter_map(|effect| match effect {
                RuntimeEffect::RequestNavigation(url) => Some(url.clone()),
                RuntimeEffect::SetDocumentTitle(_) => None,
            })
            .collect()
    }

    /// Number of executed scripts.
    #[must_use]
    pub(crate) const fn executed_count(&self) -> usize {
        self.executed
    }

    /// Captured console messages.
    #[must_use]
    pub(crate) fn console(&self) -> &[ConsoleMessage] {
        &self.console
    }

    /// Captured DOM binding effects across all executed scripts.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn dom_effects(&self) -> &[DomBindingEffect] {
        &self.dom_effects
    }

    /// Captured Web Platform API effects across all executed scripts.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn web_api_effects(&self) -> &[WebApiEffect] {
        &self.web_api_effects
    }

    /// Captured media/canvas/worker effects across all executed scripts.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn media_effects(&self) -> &[MediaCanvasWorkerEffect] {
        &self.media_effects
    }

    /// Runtime warnings.
    #[must_use]
    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Runtime errors.
    #[must_use]
    pub(crate) fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Document URL associated with this runtime.
    #[must_use]
    pub(crate) fn document_url(&self) -> &str {
        &self.document_url
    }
}

fn capture_console_calls(source: &str, program: &ScriptProgram, execution: &mut ScriptExecution) {
    for (needle, level) in [
        ("console.log", ConsoleLevel::Log),
        ("console.info", ConsoleLevel::Info),
        ("console.warn", ConsoleLevel::Warn),
        ("console.error", ConsoleLevel::Error),
    ] {
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(needle) {
            let absolute = offset + relative;
            let after = absolute + needle.len();
            if let Some((args, end)) = extract_parenthesized(source, after) {
                let text = summarize_js_arguments(args);
                let line = line_number(source, absolute);
                execution.console.push(ConsoleMessage::new(
                    level,
                    text,
                    program.source_name.clone(),
                    line,
                ));
                offset = end;
            } else {
                break;
            }
        }
    }
}

fn capture_document_title_assignments(source: &str, execution: &mut ScriptExecution) {
    for needle in ["document.title", "window.document.title"] {
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(needle) {
            let start = offset + relative + needle.len();
            if let Some((value, end)) = extract_assignment_string(source, start) {
                execution
                    .effects
                    .push(RuntimeEffect::SetDocumentTitle(value));
                offset = end;
            } else {
                offset = start;
            }
        }
    }
}

fn capture_location_assignments(source: &str, execution: &mut ScriptExecution) {
    for needle in [
        "location.href",
        "window.location.href",
        "location.assign",
        "window.location.assign",
    ] {
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(needle) {
            let start = offset + relative + needle.len();
            if needle.ends_with("assign") {
                if let Some((args, end)) = extract_parenthesized(source, start) {
                    if let Some(value) = first_js_string(args) {
                        execution
                            .effects
                            .push(RuntimeEffect::RequestNavigation(value));
                    }
                    offset = end;
                } else {
                    offset = start;
                }
            } else if let Some((value, end)) = extract_assignment_string(source, start) {
                execution
                    .effects
                    .push(RuntimeEffect::RequestNavigation(value));
                offset = end;
            } else {
                offset = start;
            }
        }
    }
}

fn extract_assignment_string(source: &str, start: usize) -> Option<(String, usize)> {
    let mut index = skip_ascii_ws(source, start);
    if source[index..].chars().next()? != '=' {
        return None;
    }
    index += 1;
    index = skip_ascii_ws(source, index);
    let (value, end) = parse_js_string(source, index)?;
    Some((value, end))
}

fn extract_parenthesized(source: &str, open_start: usize) -> Option<(&str, usize)> {
    let mut index = skip_ascii_ws(source, open_start);
    if source[index..].chars().next()? != '(' {
        return None;
    }

    index += 1;
    let content_start = index;
    let mut depth = 1usize;
    let mut in_string: Option<char> = None;
    let mut escape = false;

    for (relative, ch) in source[index..].char_indices() {
        let absolute = index + relative;

        if let Some(quote) = in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => in_string = Some(ch),
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((&source[content_start..absolute], absolute + ch.len_utf8()));
                }
            }
            _ => {}
        }
    }

    None
}

fn summarize_js_arguments(args: &str) -> String {
    let mut values = Vec::new();
    let mut rest = args.trim();

    while !rest.is_empty() {
        if let Some(value) = first_js_string(rest) {
            values.push(value);
        } else {
            let token = rest.split(',').next().map(str::trim).unwrap_or_default();
            if !token.is_empty() {
                values.push(token.to_owned());
            }
        }

        let Some(comma) = find_top_level_comma(rest) else {
            break;
        };
        rest = rest[comma + 1..].trim();
    }

    values.join(" ")
}

fn first_js_string(input: &str) -> Option<String> {
    let index = input.find(['\'', '"', '`'])?;
    parse_js_string(input, index).map(|(value, _)| value)
}

fn parse_js_string(source: &str, start: usize) -> Option<(String, usize)> {
    let quote = source[start..].chars().next()?;
    if !matches!(quote, '\'' | '"' | '`') {
        return None;
    }

    let mut value = String::new();
    let mut escape = false;
    let mut index = start + quote.len_utf8();

    for ch in source[index..].chars() {
        index += ch.len_utf8();
        if escape {
            match ch {
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                '\\' => value.push('\\'),
                '\'' => value.push('\''),
                '"' => value.push('"'),
                '`' => value.push('`'),
                other => value.push(other),
            }
            escape = false;
            continue;
        }

        if ch == '\\' {
            escape = true;
        } else if ch == quote {
            return Some((value, index));
        } else {
            value.push(ch);
        }
    }

    None
}

fn find_top_level_comma(input: &str) -> Option<usize> {
    let mut in_string: Option<char> = None;
    let mut escape = false;
    let mut depth = 0usize;

    for (index, ch) in input.char_indices() {
        if let Some(quote) = in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => in_string = Some(ch),
            '(' | '[' | '{' => depth = depth.saturating_add(1),
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some(index),
            _ => {}
        }
    }

    None
}

fn skip_ascii_ws(source: &str, mut index: usize) -> usize {
    while let Some(ch) = source[index..].chars().next() {
        if !ch.is_ascii_whitespace() {
            break;
        }
        index += ch.len_utf8();
        if index >= source.len() {
            break;
        }
    }
    index
}

fn strip_js_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_string: Option<char> = None;
    let mut escape = false;

    while let Some(ch) = chars.next() {
        if let Some(quote) = in_string {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        if matches!(ch, '\'' | '"' | '`') {
            in_string = Some(ch);
            output.push(ch);
            continue;
        }

        if ch == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    let _ = chars.next();
                    for next in chars.by_ref() {
                        if next == '\n' {
                            output.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    let _ = chars.next();
                    let mut previous = '\0';
                    for next in chars.by_ref() {
                        if previous == '*' && next == '/' {
                            break;
                        }
                        previous = next;
                    }
                }
                _ => output.push(ch),
            }
        } else {
            output.push(ch);
        }
    }

    output
}

fn looks_like_active_javascript(source: &str) -> bool {
    [
        "function",
        "=>",
        "addEventListener",
        "querySelector",
        "createElement",
        "fetch(",
        "XMLHttpRequest",
        "Promise",
        "setTimeout",
    ]
    .iter()
    .any(|needle| source.contains(needle))
}

fn line_number(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .chars()
        .filter(|ch| *ch == '\n')
        .count()
        .saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_console_and_title() {
        let mut runtime = JavaScriptRuntime::new("https://example.com/");
        let program = ScriptProgram::new(
            "console.log('hello', 'world'); document.title = 'Changed';",
            "inline:1",
            1,
            false,
        );

        let execution = runtime.execute(&program);
        assert!(execution.is_success());
        assert_eq!(runtime.latest_title_override().as_deref(), Some("Changed"));
        assert_eq!(runtime.console().len(), 1);
    }
}
