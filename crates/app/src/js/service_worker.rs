#![allow(dead_code)]
#![allow(clippy::too_many_lines)]

//! App-side Service Worker and Cache API effect bridge.
//!
//! SylJS now has real host objects for `caches` and `navigator.serviceWorker`.
//! The native app still runs the conservative source scanner for page scripts,
//! so this bridge captures the same API surface from source and applies it
//! through the existing resource scheduler. It is intentionally deterministic:
//! registrations, cache opens, deletes, and precache fetches are visible in logs
//! and summaries instead of becoming magical browser confetti. 🎪

use crate::browser::{ResourceRequest, ResourceScheduler};
use anyhow::{Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use tracing::{debug, warn};
use url::Url;

const MAX_SERVICE_WORKER_EFFECTS_PER_SCRIPT: usize = 64;
const MAX_SERVICE_WORKER_SCRIPT_BYTES: usize = 512 * 1024;
const MAX_PRECACHE_URLS_PER_WORKER: usize = 128;
const MAX_PRECACHE_BYTES_PER_RESOURCE: usize = 2 * 1024 * 1024;
const DEFAULT_CACHE_NAME: &str = "default";

/// Service Worker / Cache API effect captured from a page script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServiceWorkerEffect {
    /// `navigator.serviceWorker.register(script, { scope })`.
    Register {
        script_url: String,
        scope: Option<String>,
    },

    /// `caches.open(name)`.
    CacheOpen { name: String },

    /// `caches.delete(name)`.
    CacheDelete { name: String },

    /// `cache.add(url)` or `cache.addAll([...])` detected in page-side scripts.
    CacheAdd {
        cache_name: Option<String>,
        url: String,
    },

    /// `caches.match(url)`.
    CacheMatch { url: String },
}

/// Captured Service Worker effects and parser warnings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ServiceWorkerCapture {
    pub effects: Vec<ServiceWorkerEffect>,
    pub warnings: Vec<String>,
}

/// One registration snapshot for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceWorkerRegistrationSnapshot {
    pub script_url: String,
    pub scope: String,
    pub cache_name: String,
    pub has_fetch_listener: bool,
    pub precache_urls: Vec<String>,
}

/// App-side summary accumulated during script execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ServiceWorkerSummary {
    pub effects: usize,
    pub registrations: usize,
    pub registration_updates: usize,
    pub script_fetches: usize,
    pub script_fetch_failures: usize,
    pub cache_opens: usize,
    pub cache_deletes: usize,
    pub cache_adds: usize,
    pub cache_matches: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub precache_fetches: usize,
    pub precache_failures: usize,
    pub precache_bytes: usize,
    pub warnings: usize,
    pub errors: usize,
}

impl ServiceWorkerSummary {
    pub(crate) fn merge_from(&mut self, other: Self) {
        self.effects = self.effects.saturating_add(other.effects);
        self.registrations = self.registrations.saturating_add(other.registrations);
        self.registration_updates = self
            .registration_updates
            .saturating_add(other.registration_updates);
        self.script_fetches = self.script_fetches.saturating_add(other.script_fetches);
        self.script_fetch_failures = self
            .script_fetch_failures
            .saturating_add(other.script_fetch_failures);
        self.cache_opens = self.cache_opens.saturating_add(other.cache_opens);
        self.cache_deletes = self.cache_deletes.saturating_add(other.cache_deletes);
        self.cache_adds = self.cache_adds.saturating_add(other.cache_adds);
        self.cache_matches = self.cache_matches.saturating_add(other.cache_matches);
        self.cache_hits = self.cache_hits.saturating_add(other.cache_hits);
        self.cache_misses = self.cache_misses.saturating_add(other.cache_misses);
        self.precache_fetches = self.precache_fetches.saturating_add(other.precache_fetches);
        self.precache_failures = self
            .precache_failures
            .saturating_add(other.precache_failures);
        self.precache_bytes = self.precache_bytes.saturating_add(other.precache_bytes);
        self.warnings = self.warnings.saturating_add(other.warnings);
        self.errors = self.errors.saturating_add(other.errors);
    }

    #[must_use]
    pub(crate) fn compact(&self) -> String {
        format!(
            "effects={} registrations={} sw_fetch={} sw_failed={} cache_open={} cache_add={} cache_hits={} cache_misses={} precache={} precache_failed={} bytes={}",
            self.effects,
            self.registrations,
            self.script_fetches,
            self.script_fetch_failures,
            self.cache_opens,
            self.cache_adds,
            self.cache_hits,
            self.cache_misses,
            self.precache_fetches,
            self.precache_failures,
            self.precache_bytes,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedResponse {
    url: String,
    body: String,
    bytes: usize,
}

/// App-side Service Worker host for one document/navigation.
#[derive(Debug, Clone)]
pub(crate) struct ServiceWorkerHost {
    root: PathBuf,
    document_url: String,
    registrations: BTreeMap<String, ServiceWorkerRegistrationSnapshot>,
    caches: BTreeMap<String, BTreeMap<String, CachedResponse>>,
}

impl ServiceWorkerHost {
    pub(crate) fn new(root: impl Into<PathBuf>, document_url: &str) -> Self {
        Self {
            root: root.into(),
            document_url: document_url.to_owned(),
            registrations: BTreeMap::new(),
            caches: BTreeMap::new(),
        }
    }

    #[must_use]
    pub(crate) fn registrations(&self) -> Vec<ServiceWorkerRegistrationSnapshot> {
        self.registrations.values().cloned().collect()
    }

    pub(crate) async fn apply_effects(
        &mut self,
        effects: &[ServiceWorkerEffect],
        scheduler: &ResourceScheduler,
    ) -> ServiceWorkerSummary {
        let mut summary = ServiceWorkerSummary::default();

        for effect in effects.iter().take(MAX_SERVICE_WORKER_EFFECTS_PER_SCRIPT) {
            summary.effects = summary.effects.saturating_add(1);
            match effect {
                ServiceWorkerEffect::Register { script_url, scope } => {
                    self.register(script_url, scope.as_deref(), scheduler, &mut summary)
                        .await;
                }
                ServiceWorkerEffect::CacheOpen { name } => {
                    self.open_cache(name);
                    summary.cache_opens = summary.cache_opens.saturating_add(1);
                }
                ServiceWorkerEffect::CacheDelete { name } => {
                    if self.caches.remove(name).is_some() {
                        summary.cache_deletes = summary.cache_deletes.saturating_add(1);
                    }
                }
                ServiceWorkerEffect::CacheAdd { cache_name, url } => {
                    let cache_name = cache_name.as_deref().unwrap_or(DEFAULT_CACHE_NAME);
                    match self.resolve_url(url) {
                        Ok(resolved) => {
                            self.put_synthetic(cache_name, &resolved);
                            summary.cache_adds = summary.cache_adds.saturating_add(1);
                        }
                        Err(error) => {
                            summary.errors = summary.errors.saturating_add(1);
                            warn!(error = %error, url = %url, "failed to resolve Cache.add URL");
                        }
                    }
                }
                ServiceWorkerEffect::CacheMatch { url } => {
                    summary.cache_matches = summary.cache_matches.saturating_add(1);
                    match self.resolve_url(url) {
                        Ok(resolved) => {
                            if self.match_any(&resolved).is_some() {
                                summary.cache_hits = summary.cache_hits.saturating_add(1);
                            } else {
                                summary.cache_misses = summary.cache_misses.saturating_add(1);
                            }
                        }
                        Err(error) => {
                            summary.errors = summary.errors.saturating_add(1);
                            warn!(error = %error, url = %url, "failed to resolve Cache.match URL");
                        }
                    }
                }
            }
        }

        if effects.len() > MAX_SERVICE_WORKER_EFFECTS_PER_SCRIPT {
            summary.warnings = summary.warnings.saturating_add(1);
            warn!(
                total = effects.len(),
                limit = MAX_SERVICE_WORKER_EFFECTS_PER_SCRIPT,
                "truncated Service Worker effects for one script"
            );
        }

        summary
    }

    async fn register(
        &mut self,
        script_url: &str,
        scope: Option<&str>,
        scheduler: &ResourceScheduler,
        summary: &mut ServiceWorkerSummary,
    ) {
        let script_url = match self.resolve_url(script_url) {
            Ok(url) => url,
            Err(error) => {
                summary.errors = summary.errors.saturating_add(1);
                warn!(error = %error, script_url = %script_url, "invalid Service Worker script URL");
                return;
            }
        };
        let scope = scope
            .and_then(|value| self.resolve_url(value).ok())
            .unwrap_or_else(|| infer_scope_from_script_url(&script_url));

        summary.script_fetches = summary.script_fetches.saturating_add(1);
        let request =
            ResourceRequest::script(script_url.clone()).max_bytes(MAX_SERVICE_WORKER_SCRIPT_BYTES);
        let source = match scheduler.fetch_text(request).await {
            Ok(resource) => resource.text,
            Err(error) => {
                summary.script_fetch_failures = summary.script_fetch_failures.saturating_add(1);
                warn!(error = %error, script_url = %script_url, "failed to fetch Service Worker script");
                return;
            }
        };

        let analysis = syljs::analyze_service_worker_script(&source);
        let cache_name = analysis
            .cache_names
            .first()
            .cloned()
            .unwrap_or_else(|| DEFAULT_CACHE_NAME.to_owned());
        let snapshot = ServiceWorkerRegistrationSnapshot {
            script_url: script_url.clone(),
            scope: scope.clone(),
            cache_name: cache_name.clone(),
            has_fetch_listener: analysis.has_fetch_listener,
            precache_urls: analysis.precache_urls.clone(),
        };

        let existed = self.registrations.insert(scope.clone(), snapshot).is_some();
        if existed {
            summary.registration_updates = summary.registration_updates.saturating_add(1);
        } else {
            summary.registrations = summary.registrations.saturating_add(1);
        }

        self.open_cache(&cache_name);
        for url in analysis
            .precache_urls
            .iter()
            .take(MAX_PRECACHE_URLS_PER_WORKER)
        {
            let resolved = match self.resolve_against_scope(&scope, url) {
                Ok(value) => value,
                Err(error) => {
                    summary.precache_failures = summary.precache_failures.saturating_add(1);
                    warn!(error = %error, url = %url, scope = %scope, "failed to resolve Service Worker precache URL");
                    continue;
                }
            };

            let request = ResourceRequest::document(resolved.clone())
                .max_bytes(MAX_PRECACHE_BYTES_PER_RESOURCE);
            match scheduler.fetch_text(request).await {
                Ok(resource) => {
                    summary.precache_fetches = summary.precache_fetches.saturating_add(1);
                    summary.precache_bytes = summary.precache_bytes.saturating_add(resource.bytes);
                    self.put_response(&cache_name, resource.url, resource.text, resource.bytes);
                }
                Err(error) => {
                    summary.precache_failures = summary.precache_failures.saturating_add(1);
                    warn!(error = %error, url = %resolved, "failed to precache Service Worker resource");
                }
            }
        }

        debug!(
            script_url = %script_url,
            scope = %scope,
            cache = %cache_name,
            precache_urls = analysis.precache_urls.len(),
            fetch_listener = analysis.has_fetch_listener,
            root = %self.root.display(),
            "registered simulated Service Worker"
        );
    }

    fn open_cache(&mut self, name: &str) {
        let name = sanitize_cache_name(name);
        self.caches.entry(name).or_default();
    }

    fn put_synthetic(&mut self, cache_name: &str, url: &str) {
        self.put_response(
            cache_name,
            url.to_owned(),
            format!("synthetic-cache-add:{url}"),
            url.len(),
        );
    }

    fn put_response(&mut self, cache_name: &str, url: String, body: String, bytes: usize) {
        let cache = self
            .caches
            .entry(sanitize_cache_name(cache_name))
            .or_default();
        cache.insert(url.clone(), CachedResponse { url, body, bytes });
    }

    fn match_any(&self, url: &str) -> Option<&CachedResponse> {
        self.caches.values().find_map(|cache| cache.get(url))
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

    fn resolve_against_scope(&self, scope: &str, candidate: &str) -> Result<String> {
        let base = Url::parse(scope).with_context(|| format!("invalid scope URL `{scope}`"))?;
        let resolved = base
            .join(candidate.trim())
            .with_context(|| format!("invalid relative URL `{candidate}`"))?;
        match resolved.scheme() {
            "http" | "https" => Ok(resolved.to_string()),
            scheme => anyhow::bail!("unsupported URL scheme `{scheme}`"),
        }
    }
}

/// Captures Service Worker and Cache API calls from source.
#[must_use]
pub(crate) fn capture_service_worker_effects(source: &str) -> ServiceWorkerCapture {
    let clean = strip_line_comments(source);
    let mut capture = ServiceWorkerCapture::default();

    capture
        .effects
        .extend(capture_service_worker_registers(&clean));
    capture.effects.extend(capture_cache_opens(&clean));
    capture.effects.extend(capture_cache_deletes(&clean));
    capture.effects.extend(capture_cache_adds(&clean));
    capture.effects.extend(capture_cache_matches(&clean));

    if clean.contains("serviceWorker") && capture.effects.is_empty() {
        capture.warnings.push(
            "Service Worker API usage was detected but no supported register/cache call was captured"
                .to_owned(),
        );
    }

    capture
}

fn capture_service_worker_registers(source: &str) -> Vec<ServiceWorkerEffect> {
    let mut effects = Vec::new();
    for name in [
        "navigator.serviceWorker.register",
        "window.navigator.serviceWorker.register",
    ] {
        for args in capture_function_args(source, name) {
            let strings = string_literals(&args);
            let Some(script_url) = strings.first().cloned() else {
                continue;
            };
            let scope = capture_scope_option(&args);
            effects.push(ServiceWorkerEffect::Register { script_url, scope });
        }
    }
    effects
}

fn capture_scope_option(args: &str) -> Option<String> {
    for marker in ["scope:", "\"scope\":", "'scope':"] {
        let Some(index) = args.find(marker) else {
            continue;
        };
        if let Some((value, _)) = parse_string_literal_at(args, index + marker.len()) {
            return Some(value);
        }
    }
    None
}

fn capture_cache_opens(source: &str) -> Vec<ServiceWorkerEffect> {
    capture_function_first_string_arg(source, "caches.open")
        .into_iter()
        .map(|name| ServiceWorkerEffect::CacheOpen { name })
        .collect()
}

fn capture_cache_deletes(source: &str) -> Vec<ServiceWorkerEffect> {
    capture_function_first_string_arg(source, "caches.delete")
        .into_iter()
        .map(|name| ServiceWorkerEffect::CacheDelete { name })
        .collect()
}

fn capture_cache_adds(source: &str) -> Vec<ServiceWorkerEffect> {
    let mut effects = Vec::new();
    for args in capture_method_args(source, "cache", "add") {
        if let Some(url) = string_literals(&args).first().cloned() {
            effects.push(ServiceWorkerEffect::CacheAdd {
                cache_name: None,
                url,
            });
        }
    }
    for args in capture_method_args(source, "cache", "addAll") {
        effects.extend(string_literals(&args).into_iter().map(|url| {
            ServiceWorkerEffect::CacheAdd {
                cache_name: None,
                url,
            }
        }));
    }
    effects
}

fn capture_cache_matches(source: &str) -> Vec<ServiceWorkerEffect> {
    capture_function_first_string_arg(source, "caches.match")
        .into_iter()
        .map(|url| ServiceWorkerEffect::CacheMatch { url })
        .collect()
}

fn capture_function_first_string_arg(source: &str, name: &str) -> Vec<String> {
    capture_function_args(source, name)
        .into_iter()
        .filter_map(|args| string_literals(&args).first().cloned())
        .collect()
}

fn capture_method_args(source: &str, object_name: &str, method: &str) -> Vec<String> {
    capture_function_args(source, &format!("{object_name}.{method}"))
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
            b'\'' | b'"' | b'`' => in_quote = Some(*byte),
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
    if quote != b'\'' && quote != b'"' && quote != b'`' {
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
                b'`' => '`',
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

fn sanitize_cache_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        DEFAULT_CACHE_NAME.to_owned()
    } else {
        trimmed
            .chars()
            .filter(|ch| *ch != '\0' && *ch != '/' && *ch != '\\')
            .collect::<String>()
    }
}

fn infer_scope_from_script_url(script_url: &str) -> String {
    let Ok(url) = Url::parse(script_url) else {
        return script_url
            .rsplit_once('/')
            .map_or_else(|| "/".to_owned(), |(prefix, _)| format!("{prefix}/"));
    };

    let mut url = url;
    let path = url.path().to_owned();
    let scope = path.rsplit_once('/').map_or(
        "/",
        |(prefix, _)| if prefix.is_empty() { "/" } else { prefix },
    );
    url.set_path(scope);
    if !url.path().ends_with('/') {
        let fixed = format!("{}/", url.path());
        url.set_path(&fixed);
    }
    url.to_string()
}

fn dedupe_effects(effects: &mut Vec<ServiceWorkerEffect>) {
    let mut seen = BTreeSet::new();
    effects.retain(|effect| seen.insert(format!("{effect:?}")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_register_and_cache_open() {
        let source = r#"
            navigator.serviceWorker.register('/sw.js', { scope: '/' });
            caches.open('shell-v1');
        "#;
        let mut capture = capture_service_worker_effects(source);
        dedupe_effects(&mut capture.effects);
        assert!(capture
            .effects
            .iter()
            .any(|effect| matches!(effect, ServiceWorkerEffect::Register { .. })));
        assert!(capture.effects.iter().any(
            |effect| matches!(effect, ServiceWorkerEffect::CacheOpen { name } if name == "shell-v1")
        ));
    }
}
