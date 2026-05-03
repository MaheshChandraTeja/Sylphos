#![allow(dead_code)]

//! Web Platform API capture and host application.
//!
//! Module 25 adds a bounded host layer for APIs commonly touched by modern
//! pages: `fetch`, `XMLHttpRequest`, Web Storage, cookies, History API, timers,
//! `URL`, `URLSearchParams`, `navigator.userAgent`, and location/navigation
//! effects. The current executor is still intrinsic and conservative, but all
//! effects flow through a single async host boundary so a future V8-backed engine
//! can reuse the same browser services instead of smearing side effects through
//! the codebase like peanut butter on a server blade.

use crate::{
    browser::{ResourceRequest, ResourceScheduler},
    js::{CookieJar, HistoryApiState, StorageAreaKind, WebStorage},
};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::{debug, warn};
use url::Url;

const MAX_API_EFFECTS_PER_SCRIPT: usize = 96;
const MAX_API_FETCHES_PER_SCRIPT: usize = 8;
const MAX_API_RESPONSE_BYTES: usize = 512 * 1024;

/// Web Platform effect captured from one script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebApiEffect {
    /// `fetch(url, ...)` call.
    Fetch { url: String, method: String },

    /// `XMLHttpRequest#open(method, url)` followed by likely send.
    Xhr { url: String, method: String },

    /// `localStorage` / `sessionStorage` write.
    StorageSet {
        area: StorageAreaKind,
        key: String,
        value: String,
    },

    /// `localStorage` / `sessionStorage` remove.
    StorageRemove { area: StorageAreaKind, key: String },

    /// `localStorage.clear()` / `sessionStorage.clear()`.
    StorageClear { area: StorageAreaKind },

    /// `document.cookie = ...`.
    CookieSet { value: String },

    /// `history.pushState(...)`.
    HistoryPush {
        state_json: Option<String>,
        title: Option<String>,
        url: Option<String>,
    },

    /// `history.replaceState(...)`.
    HistoryReplace {
        state_json: Option<String>,
        title: Option<String>,
        url: Option<String>,
    },

    /// `setTimeout`, `setInterval`, or `requestAnimationFrame`.
    TimerScheduled {
        kind: TimerKind,
        delay_ms: Option<u64>,
    },

    /// `clearTimeout`, `clearInterval`, or `cancelAnimationFrame`.
    TimerCleared { kind: TimerKind },

    /// `location.href = ...`, `location.assign(...)`, or `location.replace(...)`.
    LocationNavigation { url: String, replace: bool },

    /// Access to a supported read-only browser value.
    ReadOnlyQuery { name: String },
}

/// Timer API kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimerKind {
    Timeout,
    Interval,
    AnimationFrame,
}

impl TimerKind {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "setTimeout",
            Self::Interval => "setInterval",
            Self::AnimationFrame => "requestAnimationFrame",
        }
    }
}

/// Capture output from scanning script source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WebApiCapture {
    pub effects: Vec<WebApiEffect>,
    pub warnings: Vec<String>,
}

/// Summary accumulated during one document script pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WebPlatformSummary {
    pub effects: usize,
    pub fetch_calls: usize,
    pub xhr_calls: usize,
    pub network_succeeded: usize,
    pub network_failed: usize,
    pub response_bytes: usize,
    pub storage_writes: usize,
    pub storage_removes: usize,
    pub storage_clears: usize,
    pub cookie_writes: usize,
    pub history_pushes: usize,
    pub history_replaces: usize,
    pub timers_scheduled: usize,
    pub timers_cleared: usize,
    pub read_only_queries: usize,
    pub navigation_requests: Vec<String>,
    pub warnings: usize,
    pub errors: usize,
}

impl WebPlatformSummary {
    /// Merges another summary into this one.
    pub(crate) fn merge_from(&mut self, other: Self) {
        self.effects = self.effects.saturating_add(other.effects);
        self.fetch_calls = self.fetch_calls.saturating_add(other.fetch_calls);
        self.xhr_calls = self.xhr_calls.saturating_add(other.xhr_calls);
        self.network_succeeded = self
            .network_succeeded
            .saturating_add(other.network_succeeded);
        self.network_failed = self.network_failed.saturating_add(other.network_failed);
        self.response_bytes = self.response_bytes.saturating_add(other.response_bytes);
        self.storage_writes = self.storage_writes.saturating_add(other.storage_writes);
        self.storage_removes = self.storage_removes.saturating_add(other.storage_removes);
        self.storage_clears = self.storage_clears.saturating_add(other.storage_clears);
        self.cookie_writes = self.cookie_writes.saturating_add(other.cookie_writes);
        self.history_pushes = self.history_pushes.saturating_add(other.history_pushes);
        self.history_replaces = self.history_replaces.saturating_add(other.history_replaces);
        self.timers_scheduled = self.timers_scheduled.saturating_add(other.timers_scheduled);
        self.timers_cleared = self.timers_cleared.saturating_add(other.timers_cleared);
        self.read_only_queries = self
            .read_only_queries
            .saturating_add(other.read_only_queries);
        self.navigation_requests.extend(other.navigation_requests);
        self.warnings = self.warnings.saturating_add(other.warnings);
        self.errors = self.errors.saturating_add(other.errors);
    }

    /// Returns a compact diagnostics string.
    #[must_use]
    pub(crate) fn compact(&self) -> String {
        format!(
            "effects={} fetch={} xhr={} ok={} failed={} bytes={} storage_writes={} cookies={} history_push={} history_replace={} timers={}",
            self.effects,
            self.fetch_calls,
            self.xhr_calls,
            self.network_succeeded,
            self.network_failed,
            self.response_bytes,
            self.storage_writes,
            self.cookie_writes,
            self.history_pushes,
            self.history_replaces,
            self.timers_scheduled,
        )
    }
}

/// Host-side Web Platform state for one document runtime.
#[derive(Debug, Clone)]
pub(crate) struct WebPlatformHost {
    document_url: String,
    storage: WebStorage,
    cookies: CookieJar,
    history: HistoryApiState,
}

impl WebPlatformHost {
    /// Creates a platform host for one document.
    pub(crate) fn new(root: impl Into<PathBuf>, document_url: &str) -> Self {
        let root = root.into();
        Self {
            storage: WebStorage::load(&root, document_url),
            cookies: CookieJar::load(&root),
            history: HistoryApiState::new(document_url),
            document_url: document_url.to_owned(),
        }
    }

    /// Returns current document URL.
    #[must_use]
    pub(crate) fn document_url(&self) -> &str {
        &self.document_url
    }

    /// Applies captured effects through browser services.
    pub(crate) async fn apply_effects(
        &mut self,
        effects: &[WebApiEffect],
        scheduler: &ResourceScheduler,
    ) -> WebPlatformSummary {
        let mut summary = WebPlatformSummary::default();
        let mut api_fetches_used = 0usize;

        for effect in effects.iter().take(MAX_API_EFFECTS_PER_SCRIPT) {
            summary.effects = summary.effects.saturating_add(1);

            match effect {
                WebApiEffect::Fetch { url, method } => {
                    summary.fetch_calls = summary.fetch_calls.saturating_add(1);
                    if api_fetches_used >= MAX_API_FETCHES_PER_SCRIPT {
                        summary.warnings = summary.warnings.saturating_add(1);
                        warn!(url = %url, "skipped fetch() due to per-script API fetch limit");
                        continue;
                    }
                    api_fetches_used = api_fetches_used.saturating_add(1);
                    self.perform_api_text_fetch("fetch", method, url, scheduler, &mut summary)
                        .await;
                }
                WebApiEffect::Xhr { url, method } => {
                    summary.xhr_calls = summary.xhr_calls.saturating_add(1);
                    if api_fetches_used >= MAX_API_FETCHES_PER_SCRIPT {
                        summary.warnings = summary.warnings.saturating_add(1);
                        warn!(url = %url, "skipped XMLHttpRequest due to per-script API fetch limit");
                        continue;
                    }
                    api_fetches_used = api_fetches_used.saturating_add(1);
                    self.perform_api_text_fetch("xhr", method, url, scheduler, &mut summary)
                        .await;
                }
                WebApiEffect::StorageSet { area, key, value } => {
                    match self.storage.set_item(*area, key, value) {
                        Ok(changed) => {
                            if changed {
                                summary.storage_writes = summary.storage_writes.saturating_add(1);
                            }
                        }
                        Err(error) => {
                            summary.errors = summary.errors.saturating_add(1);
                            warn!(error = %error, area = area.as_str(), "storage setItem failed");
                        }
                    }
                }
                WebApiEffect::StorageRemove { area, key } => {
                    if self.storage.remove_item(*area, key) {
                        summary.storage_removes = summary.storage_removes.saturating_add(1);
                    }
                }
                WebApiEffect::StorageClear { area } => {
                    if self.storage.clear(*area) {
                        summary.storage_clears = summary.storage_clears.saturating_add(1);
                    }
                }
                WebApiEffect::CookieSet { value } => {
                    match self.cookies.set_from_script(&self.document_url, value) {
                        Ok(changed) => {
                            if changed {
                                summary.cookie_writes = summary.cookie_writes.saturating_add(1);
                            }
                        }
                        Err(error) => {
                            summary.errors = summary.errors.saturating_add(1);
                            warn!(error = %error, "document.cookie assignment failed");
                        }
                    }
                }
                WebApiEffect::HistoryPush {
                    state_json,
                    title,
                    url,
                } => match self.history.push_state(
                    &self.document_url,
                    state_json.clone(),
                    title.clone(),
                    url.clone(),
                ) {
                    Ok(new_url) => {
                        self.document_url = new_url;
                        summary.history_pushes = summary.history_pushes.saturating_add(1);
                    }
                    Err(error) => {
                        summary.errors = summary.errors.saturating_add(1);
                        warn!(error = %error, "history.pushState failed");
                    }
                },
                WebApiEffect::HistoryReplace {
                    state_json,
                    title,
                    url,
                } => match self.history.replace_state(
                    &self.document_url,
                    state_json.clone(),
                    title.clone(),
                    url.clone(),
                ) {
                    Ok(new_url) => {
                        self.document_url = new_url;
                        summary.history_replaces = summary.history_replaces.saturating_add(1);
                    }
                    Err(error) => {
                        summary.errors = summary.errors.saturating_add(1);
                        warn!(error = %error, "history.replaceState failed");
                    }
                },
                WebApiEffect::TimerScheduled { kind, delay_ms } => {
                    summary.timers_scheduled = summary.timers_scheduled.saturating_add(1);
                    debug!(kind = kind.as_str(), delay_ms = ?delay_ms, "scheduled web timer placeholder");
                }
                WebApiEffect::TimerCleared { kind } => {
                    summary.timers_cleared = summary.timers_cleared.saturating_add(1);
                    debug!(kind = kind.as_str(), "cleared web timer placeholder");
                }
                WebApiEffect::LocationNavigation { url, replace } => match self.resolve_url(url) {
                    Ok(resolved) => {
                        if *replace {
                            let _ = self.history.replace_state(
                                &self.document_url,
                                None,
                                None,
                                Some(resolved.clone()),
                            );
                        }
                        summary.navigation_requests.push(resolved);
                    }
                    Err(error) => {
                        summary.errors = summary.errors.saturating_add(1);
                        warn!(error = %error, url = %url, "location navigation failed");
                    }
                },
                WebApiEffect::ReadOnlyQuery { name } => {
                    summary.read_only_queries = summary.read_only_queries.saturating_add(1);
                    debug!(name = %name, "answered read-only web platform query through host defaults");
                }
            }
        }

        if effects.len() > MAX_API_EFFECTS_PER_SCRIPT {
            summary.warnings = summary.warnings.saturating_add(1);
        }

        if let Err(error) = self.storage.flush() {
            summary.errors = summary.errors.saturating_add(1);
            warn!(error = %error, "failed to flush Web Storage");
        }
        if let Err(error) = self.cookies.flush() {
            summary.errors = summary.errors.saturating_add(1);
            warn!(error = %error, "failed to flush CookieJar");
        }

        summary
    }

    async fn perform_api_text_fetch(
        &self,
        label: &str,
        method: &str,
        url: &str,
        scheduler: &ResourceScheduler,
        summary: &mut WebPlatformSummary,
    ) {
        if !method.eq_ignore_ascii_case("GET") {
            summary.warnings = summary.warnings.saturating_add(1);
            warn!(method = %method, url = %url, api = label, "non-GET API request recorded but not fetched yet");
            return;
        }

        let resolved = match self.resolve_url(url) {
            Ok(resolved) => resolved,
            Err(error) => {
                summary.errors = summary.errors.saturating_add(1);
                warn!(error = %error, url = %url, api = label, "failed to resolve API URL");
                return;
            }
        };

        let request = ResourceRequest::document(resolved.clone()).max_bytes(MAX_API_RESPONSE_BYTES);
        match scheduler.fetch_text(request).await {
            Ok(resource) => {
                summary.network_succeeded = summary.network_succeeded.saturating_add(1);
                summary.response_bytes = summary.response_bytes.saturating_add(resource.bytes);
                debug!(
                    api = label,
                    url = %resource.url,
                    bytes = resource.bytes,
                    cache_source = resource.source.as_str(),
                    "completed Web Platform API fetch"
                );
            }
            Err(error) => {
                summary.network_failed = summary.network_failed.saturating_add(1);
                warn!(api = label, url = %resolved, error = %error, "Web Platform API fetch failed");
            }
        }
    }

    fn resolve_url(&self, candidate: &str) -> Result<String> {
        let base = Url::parse(&self.document_url)
            .with_context(|| format!("invalid document URL `{}`", self.document_url))?;
        let resolved = base
            .join(candidate.trim())
            .with_context(|| format!("invalid relative URL `{candidate}`"))?;

        match resolved.scheme() {
            "http" | "https" => Ok(resolved.to_string()),
            scheme => anyhow::bail!("unsupported URL scheme `{scheme}`"),
        }
    }
}

/// Captures Web Platform API calls from source.
#[must_use]
pub(crate) fn capture_web_platform_effects(source: &str) -> WebApiCapture {
    let mut capture = WebApiCapture::default();
    let clean = strip_line_comments(source);

    capture.effects.extend(capture_fetch_calls(&clean));
    capture.effects.extend(capture_xhr_open_calls(&clean));
    capture.effects.extend(capture_storage_calls(
        &clean,
        StorageAreaKind::Local,
        "localStorage",
    ));
    capture.effects.extend(capture_storage_calls(
        &clean,
        StorageAreaKind::Session,
        "sessionStorage",
    ));
    capture.effects.extend(capture_cookie_assignments(&clean));
    capture.effects.extend(capture_history_calls(&clean));
    capture.effects.extend(capture_timer_calls(&clean));
    capture.effects.extend(capture_location_effects(&clean));
    capture.effects.extend(capture_read_only_queries(&clean));

    if clean.contains("new URL(") {
        capture.effects.push(WebApiEffect::ReadOnlyQuery {
            name: "URL".to_owned(),
        });
    }
    if clean.contains("new URLSearchParams(") {
        capture.effects.push(WebApiEffect::ReadOnlyQuery {
            name: "URLSearchParams".to_owned(),
        });
    }

    capture
}

fn capture_fetch_calls(source: &str) -> Vec<WebApiEffect> {
    capture_function_first_string_arg(source, "fetch")
        .into_iter()
        .map(|url| WebApiEffect::Fetch {
            url,
            method: "GET".to_owned(),
        })
        .collect()
}

fn capture_xhr_open_calls(source: &str) -> Vec<WebApiEffect> {
    let mut effects = Vec::new();
    let mut cursor = 0usize;

    while let Some(index) = source[cursor..].find(".open(") {
        let open = cursor + index + ".open".len();
        if let Some(args) = extract_parenthesized(source, open) {
            let values = string_literals(&args);
            if values.len() >= 2 {
                effects.push(WebApiEffect::Xhr {
                    method: values[0].clone(),
                    url: values[1].clone(),
                });
            }
            cursor = open.saturating_add(args.len()).saturating_add(2);
        } else {
            break;
        }
    }

    effects
}

fn capture_storage_calls(
    source: &str,
    area: StorageAreaKind,
    object_name: &str,
) -> Vec<WebApiEffect> {
    let mut effects = Vec::new();

    for args in capture_method_args(source, object_name, "setItem") {
        let values = string_literals(&args);
        if values.len() >= 2 {
            effects.push(WebApiEffect::StorageSet {
                area,
                key: values[0].clone(),
                value: values[1].clone(),
            });
        }
    }

    for args in capture_method_args(source, object_name, "removeItem") {
        let values = string_literals(&args);
        if let Some(key) = values.first() {
            effects.push(WebApiEffect::StorageRemove {
                area,
                key: key.clone(),
            });
        }
    }

    for _ in capture_method_args(source, object_name, "clear") {
        effects.push(WebApiEffect::StorageClear { area });
    }

    effects
}

fn capture_cookie_assignments(source: &str) -> Vec<WebApiEffect> {
    let mut effects = Vec::new();
    for marker in ["document.cookie", "window.document.cookie"] {
        let mut cursor = 0usize;
        while let Some(index) = source[cursor..].find(marker) {
            let start = cursor + index + marker.len();
            let Some(eq_index) = source[start..].find('=') else {
                break;
            };
            let value_start = start + eq_index + 1;
            if let Some((value, end)) = parse_string_literal_at(source, value_start) {
                effects.push(WebApiEffect::CookieSet { value });
                cursor = end;
            } else {
                cursor = value_start;
            }
        }
    }
    effects
}

fn capture_history_calls(source: &str) -> Vec<WebApiEffect> {
    let mut effects = Vec::new();

    for (method, replace) in [("pushState", false), ("replaceState", true)] {
        for args in capture_method_args(source, "history", method) {
            let values = string_literals(&args);
            effects.push(if replace {
                WebApiEffect::HistoryReplace {
                    state_json: values.first().cloned(),
                    title: values.get(1).cloned(),
                    url: values.get(2).cloned(),
                }
            } else {
                WebApiEffect::HistoryPush {
                    state_json: values.first().cloned(),
                    title: values.get(1).cloned(),
                    url: values.get(2).cloned(),
                }
            });
        }
    }

    effects
}

fn capture_timer_calls(source: &str) -> Vec<WebApiEffect> {
    let mut effects = Vec::new();

    for args in capture_function_args(source, "setTimeout") {
        effects.push(WebApiEffect::TimerScheduled {
            kind: TimerKind::Timeout,
            delay_ms: numeric_argument_after_first(&args),
        });
    }
    for args in capture_function_args(source, "setInterval") {
        effects.push(WebApiEffect::TimerScheduled {
            kind: TimerKind::Interval,
            delay_ms: numeric_argument_after_first(&args),
        });
    }
    for _ in capture_function_args(source, "requestAnimationFrame") {
        effects.push(WebApiEffect::TimerScheduled {
            kind: TimerKind::AnimationFrame,
            delay_ms: None,
        });
    }
    for _ in capture_function_args(source, "clearTimeout") {
        effects.push(WebApiEffect::TimerCleared {
            kind: TimerKind::Timeout,
        });
    }
    for _ in capture_function_args(source, "clearInterval") {
        effects.push(WebApiEffect::TimerCleared {
            kind: TimerKind::Interval,
        });
    }
    for _ in capture_function_args(source, "cancelAnimationFrame") {
        effects.push(WebApiEffect::TimerCleared {
            kind: TimerKind::AnimationFrame,
        });
    }

    effects
}

fn capture_location_effects(source: &str) -> Vec<WebApiEffect> {
    let mut effects = Vec::new();

    for marker in ["location.href", "window.location.href"] {
        let mut cursor = 0usize;
        while let Some(index) = source[cursor..].find(marker) {
            let start = cursor + index + marker.len();
            let Some(eq_index) = source[start..].find('=') else {
                break;
            };
            let value_start = start + eq_index + 1;
            if let Some((url, end)) = parse_string_literal_at(source, value_start) {
                effects.push(WebApiEffect::LocationNavigation {
                    url,
                    replace: false,
                });
                cursor = end;
            } else {
                cursor = value_start;
            }
        }
    }

    for url in capture_function_first_string_arg(source, "location.assign") {
        effects.push(WebApiEffect::LocationNavigation {
            url,
            replace: false,
        });
    }
    for url in capture_function_first_string_arg(source, "window.location.assign") {
        effects.push(WebApiEffect::LocationNavigation {
            url,
            replace: false,
        });
    }
    for url in capture_function_first_string_arg(source, "location.replace") {
        effects.push(WebApiEffect::LocationNavigation { url, replace: true });
    }
    for url in capture_function_first_string_arg(source, "window.location.replace") {
        effects.push(WebApiEffect::LocationNavigation { url, replace: true });
    }

    effects
}

fn capture_read_only_queries(source: &str) -> Vec<WebApiEffect> {
    let mut effects = Vec::new();
    for name in [
        "navigator.userAgent",
        "navigator.language",
        "window.innerWidth",
        "window.innerHeight",
        "document.cookie",
        "location.href",
    ] {
        if source.contains(name) {
            effects.push(WebApiEffect::ReadOnlyQuery {
                name: name.to_owned(),
            });
        }
    }
    effects
}

fn capture_function_first_string_arg(source: &str, name: &str) -> Vec<String> {
    capture_function_args(source, name)
        .into_iter()
        .filter_map(|args| string_literals(&args).first().cloned())
        .collect()
}

fn capture_function_args(source: &str, name: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cursor = 0usize;
    let needle = format!("{name}(");

    while let Some(index) = source[cursor..].find(&needle) {
        let open = cursor + index + name.len();
        if let Some(value) = extract_parenthesized(source, open) {
            cursor = open.saturating_add(value.len()).saturating_add(2);
            args.push(value);
        } else {
            break;
        }
    }

    args
}

fn capture_method_args(source: &str, object_name: &str, method: &str) -> Vec<String> {
    capture_function_args(source, &format!("{object_name}.{method}"))
}

fn extract_parenthesized(source: &str, open_paren_index: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if bytes.get(open_paren_index) != Some(&b'(') {
        return None;
    }

    let mut depth = 0usize;
    let mut in_quote: Option<u8> = None;
    let mut escaped = false;
    let mut start = None;

    for (index, byte) in bytes.iter().enumerate().skip(open_paren_index) {
        if let Some(quote) = in_quote {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == quote {
                in_quote = None;
            }
            continue;
        }

        match *byte {
            b'\'' | b'"' => in_quote = Some(*byte),
            b'(' => {
                if depth == 0 {
                    start = Some(index + 1);
                }
                depth = depth.saturating_add(1);
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return start.map(|start| source[start..index].to_owned());
                }
            }
            _ => {}
        }
    }

    None
}

fn string_literals(source: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0usize;

    while index < source.len() {
        if let Some((value, end)) = parse_string_literal_at(source, index) {
            values.push(value);
            index = end;
        } else {
            index += 1;
        }
    }

    values
}

fn parse_string_literal_at(source: &str, start: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let mut index = skip_ws(bytes, start);
    let quote = *bytes.get(index)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    index += 1;

    let mut value = String::new();
    let mut escaped = false;

    while let Some(byte) = bytes.get(index) {
        if escaped {
            let ch = match *byte {
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'\\' => '\\',
                b'\'' => '\'',
                b'"' => '"',
                other => char::from(other),
            };
            value.push(ch);
            escaped = false;
            index += 1;
            continue;
        }

        if *byte == b'\\' {
            escaped = true;
            index += 1;
            continue;
        }

        if *byte == quote {
            return Some((value, index + 1));
        }

        value.push(char::from(*byte));
        index += 1;
    }

    None
}

fn numeric_argument_after_first(args: &str) -> Option<u64> {
    let parts = split_top_level_commas(args);
    let value = parts.get(1)?.trim();
    value.parse::<u64>().ok()
}

fn split_top_level_commas(source: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    let mut escaped = false;

    for ch in source.chars() {
        if let Some(quote) = in_quote {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                in_quote = Some(ch);
                current.push(ch);
            }
            ',' => {
                parts.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }

    parts
}

fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        index += 1;
    }
    index
}

fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(before, _)| before))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_storage_cookie_history_and_fetch() {
        let source = r#"
            localStorage.setItem('theme', 'dark');
            sessionStorage.removeItem('tmp');
            document.cookie = "a=b; path=/";
            history.pushState('{"x":1}', '', '/next');
            fetch('/api/data');
        "#;
        let capture = capture_web_platform_effects(source);

        assert!(capture
            .effects
            .iter()
            .any(|effect| matches!(effect, WebApiEffect::StorageSet { .. })));
        assert!(capture
            .effects
            .iter()
            .any(|effect| matches!(effect, WebApiEffect::StorageRemove { .. })));
        assert!(capture
            .effects
            .iter()
            .any(|effect| matches!(effect, WebApiEffect::CookieSet { .. })));
        assert!(capture
            .effects
            .iter()
            .any(|effect| matches!(effect, WebApiEffect::HistoryPush { .. })));
        assert!(capture
            .effects
            .iter()
            .any(|effect| matches!(effect, WebApiEffect::Fetch { .. })));
    }

    #[test]
    fn captures_xhr_and_timers() {
        let source = r#"
            let xhr = new XMLHttpRequest();
            xhr.open('GET', '/api/search');
            xhr.send();
            setTimeout(() => {}, 25);
            requestAnimationFrame(() => {});
        "#;
        let capture = capture_web_platform_effects(source);
        assert!(capture
            .effects
            .iter()
            .any(|effect| matches!(effect, WebApiEffect::Xhr { .. })));
        assert!(
            capture
                .effects
                .iter()
                .filter(|effect| matches!(effect, WebApiEffect::TimerScheduled { .. }))
                .count()
                >= 2
        );
    }
}
