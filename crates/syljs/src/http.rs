#![allow(clippy::too_many_lines, missing_docs)]
#![doc = "HTTP request/response semantics, MIME sniffing, redirect policy, and Cache-Control parsing for Sylphos."]
#![doc = ""]
#![doc = "This module is protocol logic, not a network client. It is intentionally"]
#![doc = "deterministic and dependency-light so the browser app, SylJS Fetch API,"]
#![doc = "Service Worker simulation, and compatibility harness can all agree on"]
#![doc = "headers, cacheability, MIME type, redirect mode, and response classification."]

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, time::Duration};

/// A normalized, insertion-safe HTTP header list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpHeaderList {
    inner: BTreeMap<String, String>,
}

impl HttpHeaderList {
    /// Creates an empty header list.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    /// Builds headers from string pairs.
    #[must_use]
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut headers = Self::new();
        for (name, value) in pairs {
            headers.insert(name, value);
        }
        headers
    }

    /// Inserts/replaces one header. Invalid or empty names are ignored.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = normalize_header_name(&name.into());
        if name.is_empty() {
            return;
        }
        self.inner
            .insert(name, normalize_header_value(&value.into()));
    }

    /// Appends a value to an existing comma-separated header.
    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = normalize_header_name(&name.into());
        if name.is_empty() {
            return;
        }
        let value = normalize_header_value(&value.into());
        self.inner
            .entry(name)
            .and_modify(|existing| {
                if !existing.is_empty() {
                    existing.push_str(", ");
                }
                existing.push_str(&value);
            })
            .or_insert(value);
    }

    /// Gets a header value by case-insensitive name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.inner
            .get(&normalize_header_name(name))
            .map(String::as_str)
    }

    /// Returns whether the header exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.inner.contains_key(&normalize_header_name(name))
    }

    /// Returns whether a comma-separated header contains a token.
    #[must_use]
    pub fn contains_token(&self, name: &str, token: &str) -> bool {
        let token = token.trim().to_ascii_lowercase();
        self.get(name)
            .map(|value| {
                value
                    .split(',')
                    .map(|part| part.trim().to_ascii_lowercase())
                    .any(|part| part == token)
            })
            .unwrap_or(false)
    }

    /// Iterates over normalized headers.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.inner.iter()
    }

    /// Returns cloned pairs.
    #[must_use]
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        self.inner
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    /// Returns true when no headers are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns total serialized-ish byte weight.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.inner
            .iter()
            .map(|(name, value)| name.len().saturating_add(value.len()).saturating_add(4))
            .sum()
    }
}

/// HTTP method supported by Sylphos protocol logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

impl Default for HttpMethod {
    fn default() -> Self {
        Self::Get
    }
}

impl HttpMethod {
    /// Parses a method string.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "GET" => Some(Self::Get),
            "HEAD" => Some(Self::Head),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "PATCH" => Some(Self::Patch),
            "DELETE" => Some(Self::Delete),
            "OPTIONS" => Some(Self::Options),
            _ => None,
        }
    }

    /// Returns wire text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }

    /// Returns whether this method is considered safe.
    #[must_use]
    pub const fn is_safe(self) -> bool {
        matches!(self, Self::Get | Self::Head | Self::Options)
    }

    /// Returns whether redirected 303 should be rewritten to GET.
    #[must_use]
    pub const fn rewrite_to_get_on_see_other(self) -> bool {
        !matches!(self, Self::Get | Self::Head)
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Fetch request mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpRequestMode {
    Navigate,
    SameOrigin,
    Cors,
    NoCors,
}

impl Default for HttpRequestMode {
    fn default() -> Self {
        Self::Cors
    }
}

/// Fetch credentials mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpCredentialsMode {
    Omit,
    SameOrigin,
    Include,
}

impl Default for HttpCredentialsMode {
    fn default() -> Self {
        Self::SameOrigin
    }
}

/// Fetch cache mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpCacheMode {
    Default,
    NoStore,
    Reload,
    NoCache,
    ForceCache,
    OnlyIfCached,
}

impl Default for HttpCacheMode {
    fn default() -> Self {
        Self::Default
    }
}

/// Redirect handling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpRedirectMode {
    Follow,
    Error,
    Manual,
}

impl Default for HttpRedirectMode {
    fn default() -> Self {
        Self::Follow
    }
}

/// Request destination for Accept header and MIME policy decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HttpDestination {
    Document,
    Style,
    Script,
    Image,
    Font,
    Media,
    Worker,
    Manifest,
    Fetch,
    Xhr,
    Other,
}

impl HttpDestination {
    /// Default Accept header for this destination.
    #[must_use]
    pub const fn accept_header(self) -> &'static str {
        match self {
            Self::Document => "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            Self::Style => "text/css,*/*;q=0.1",
            Self::Script | Self::Worker => {
                "text/javascript,application/javascript,application/ecmascript,*/*;q=0.1"
            }
            Self::Image => "image/avif,image/webp,image/png,image/svg+xml,image/*,*/*;q=0.8",
            Self::Font => "font/woff2,font/woff,application/font-woff,*/*;q=0.1",
            Self::Media => "video/*,audio/*,*/*;q=0.2",
            Self::Manifest => "application/manifest+json,application/json,*/*;q=0.1",
            Self::Fetch | Self::Xhr | Self::Other => "*/*",
        }
    }

    /// Stable label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Style => "style",
            Self::Script => "script",
            Self::Image => "image",
            Self::Font => "font",
            Self::Media => "media",
            Self::Worker => "worker",
            Self::Manifest => "manifest",
            Self::Fetch => "fetch",
            Self::Xhr => "xhr",
            Self::Other => "other",
        }
    }
}

/// Response tainting/type visible to script-level APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpResponseType {
    Basic,
    Cors,
    Opaque,
    OpaqueRedirect,
    Error,
}

impl Default for HttpResponseType {
    fn default() -> Self {
        Self::Basic
    }
}

/// Parsed Content-Type essence and optional charset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpMimeType {
    /// Lowercase type/subtype, e.g. `text/html`.
    pub essence: String,
    /// Optional charset value.
    pub charset: Option<String>,
    /// Whether this value came from sniffing rather than an explicit header.
    pub sniffed: bool,
}

impl HttpMimeType {
    /// Creates a MIME type from essence.
    #[must_use]
    pub fn new(essence: impl Into<String>) -> Self {
        Self {
            essence: essence.into().trim().to_ascii_lowercase(),
            charset: None,
            sniffed: false,
        }
    }

    /// Marks this MIME type as sniffed.
    #[must_use]
    pub fn sniffed(mut self) -> Self {
        self.sniffed = true;
        self
    }

    /// Parses Content-Type.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split(';');
        let essence = parts.next()?.trim().to_ascii_lowercase();
        if !essence.contains('/') || essence.contains(' ') || essence.is_empty() {
            return None;
        }
        let mut mime = Self::new(essence);
        for part in parts {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("charset") {
                mime.charset = Some(value.trim().trim_matches('"').to_owned());
            }
        }
        Some(mime)
    }

    /// Returns true for JavaScript MIME types accepted by browsers.
    #[must_use]
    pub fn is_javascript(&self) -> bool {
        matches!(
            self.essence.as_str(),
            "text/javascript"
                | "application/javascript"
                | "application/ecmascript"
                | "text/ecmascript"
                | "application/x-javascript"
        )
    }

    /// Returns true for CSS.
    #[must_use]
    pub fn is_css(&self) -> bool {
        self.essence == "text/css"
    }

    /// Returns true for HTML-ish documents.
    #[must_use]
    pub fn is_html(&self) -> bool {
        matches!(self.essence.as_str(), "text/html" | "application/xhtml+xml")
    }
}

/// Parsed Cache-Control fields relevant to Sylphos.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpCacheControl {
    pub no_store: bool,
    pub no_cache: bool,
    pub must_revalidate: bool,
    pub public: bool,
    pub private: bool,
    pub immutable: bool,
    pub max_age_secs: Option<u64>,
    pub stale_while_revalidate_secs: Option<u64>,
}

impl HttpCacheControl {
    /// Parses Cache-Control.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        let mut parsed = Self::default();
        for directive in value.split(',') {
            let directive = directive.trim();
            if directive.is_empty() {
                continue;
            }
            let (name, raw_value) = directive.split_once('=').unwrap_or((directive, ""));
            let name = name.trim().to_ascii_lowercase();
            let raw_value = raw_value.trim().trim_matches('"');
            match name.as_str() {
                "no-store" => parsed.no_store = true,
                "no-cache" => parsed.no_cache = true,
                "must-revalidate" | "proxy-revalidate" => parsed.must_revalidate = true,
                "public" => parsed.public = true,
                "private" => parsed.private = true,
                "immutable" => parsed.immutable = true,
                "max-age" => parsed.max_age_secs = parse_u64(raw_value),
                "stale-while-revalidate" => {
                    parsed.stale_while_revalidate_secs = parse_u64(raw_value)
                }
                _ => {}
            }
        }
        parsed
    }
}

/// Cache freshness decision for a response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpFreshnessDecision {
    pub storable: bool,
    pub expires_at_ms: u64,
    pub revalidate_on_use: bool,
    pub reason: String,
}

impl HttpFreshnessDecision {
    /// Non-storable response.
    #[must_use]
    pub fn no_store(now_ms: u64, reason: impl Into<String>) -> Self {
        Self {
            storable: false,
            expires_at_ms: now_ms,
            revalidate_on_use: false,
            reason: reason.into(),
        }
    }
}

/// Redirect record used by fetch/app diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRedirectRecord {
    pub from_url: String,
    pub to_url: String,
    pub status: u16,
}

/// Computes whether a response is storable and how long it is fresh.
#[must_use]
pub fn compute_freshness(
    status: u16,
    headers: &HttpHeaderList,
    default_ttl: Duration,
    now_ms: u64,
) -> HttpFreshnessDecision {
    if !is_cacheable_status(status) {
        return HttpFreshnessDecision::no_store(
            now_ms,
            format!("status {status} is not cacheable"),
        );
    }

    let cache_control = headers
        .get("cache-control")
        .map(HttpCacheControl::parse)
        .unwrap_or_default();

    if cache_control.no_store {
        return HttpFreshnessDecision::no_store(now_ms, "Cache-Control: no-store");
    }

    let ttl_ms = if let Some(max_age) = cache_control.max_age_secs {
        max_age.saturating_mul(1000)
    } else if let Some(expires) = parse_http_date_lite(headers.get("expires")) {
        expires.saturating_sub(now_ms)
    } else {
        duration_ms(default_ttl)
    };

    let revalidate = cache_control.no_cache || cache_control.must_revalidate;

    HttpFreshnessDecision {
        storable: true,
        expires_at_ms: now_ms.saturating_add(ttl_ms),
        revalidate_on_use: revalidate,
        reason: if cache_control.max_age_secs.is_some() {
            "Cache-Control max-age".to_owned()
        } else if headers.contains("expires") {
            "Expires header".to_owned()
        } else {
            "default resource ttl".to_owned()
        },
    }
}

/// Returns whether status is a redirect response.
#[must_use]
pub const fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Returns whether status is a success response.
#[must_use]
pub const fn is_success_status(status: u16) -> bool {
    status >= 200 && status < 300
}

/// Returns whether status is cacheable by default or with explicit controls.
#[must_use]
pub const fn is_cacheable_status(status: u16) -> bool {
    matches!(
        status,
        200 | 203 | 204 | 206 | 300 | 301 | 308 | 404 | 405 | 410 | 414 | 501
    )
}

/// Performs deterministic MIME sniffing from Content-Type, destination, URL, and body prefix.
#[must_use]
pub fn sniff_mime(
    destination: HttpDestination,
    url: &str,
    headers: &HttpHeaderList,
    prefix: &[u8],
) -> HttpMimeType {
    if let Some(content_type) = headers.get("content-type") {
        if let Some(parsed) = HttpMimeType::parse(content_type) {
            return parsed;
        }
    }

    let lower_url = url.to_ascii_lowercase();
    for (extension, mime) in extension_mime_table(destination) {
        if lower_url.ends_with(extension) {
            return HttpMimeType::new(*mime).sniffed();
        }
    }

    let trimmed = trim_ascii_prefix(prefix);
    if starts_case_insensitive(trimmed, b"<!doctype html")
        || starts_case_insensitive(trimmed, b"<html")
        || starts_case_insensitive(trimmed, b"<script")
    {
        return HttpMimeType::new("text/html").sniffed();
    }
    if starts_case_insensitive(trimmed, b"<?xml") || starts_case_insensitive(trimmed, b"<svg") {
        if destination == HttpDestination::Image || lower_url.ends_with(".svg") {
            return HttpMimeType::new("image/svg+xml").sniffed();
        }
        return HttpMimeType::new("application/xml").sniffed();
    }
    if prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        return HttpMimeType::new("image/png").sniffed();
    }
    if prefix.starts_with(&[0xff, 0xd8, 0xff]) {
        return HttpMimeType::new("image/jpeg").sniffed();
    }
    if prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a") {
        return HttpMimeType::new("image/gif").sniffed();
    }
    if prefix.starts_with(b"RIFF") && prefix.get(8..12) == Some(b"WEBP") {
        return HttpMimeType::new("image/webp").sniffed();
    }
    if looks_like_json(trimmed) {
        return HttpMimeType::new("application/json").sniffed();
    }

    match destination {
        HttpDestination::Document => HttpMimeType::new("text/html").sniffed(),
        HttpDestination::Style => HttpMimeType::new("text/css").sniffed(),
        HttpDestination::Script | HttpDestination::Worker => {
            HttpMimeType::new("text/javascript").sniffed()
        }
        HttpDestination::Manifest => HttpMimeType::new("application/manifest+json").sniffed(),
        HttpDestination::Image => HttpMimeType::new("application/octet-stream").sniffed(),
        _ => HttpMimeType::new("application/octet-stream").sniffed(),
    }
}

/// Validates that a response MIME is usable for a destination.
#[must_use]
pub fn mime_allowed_for_destination(destination: HttpDestination, mime: &HttpMimeType) -> bool {
    match destination {
        HttpDestination::Document => {
            mime.is_html() || mime.essence == "text/plain" || mime.essence == "application/xml"
        }
        HttpDestination::Style => mime.is_css() || mime.sniffed,
        HttpDestination::Script | HttpDestination::Worker => mime.is_javascript() || mime.sniffed,
        HttpDestination::Image => {
            mime.essence.starts_with("image/") || mime.essence == "application/octet-stream"
        }
        HttpDestination::Font => {
            mime.essence.starts_with("font/")
                || mime.essence.contains("font")
                || mime.essence == "application/octet-stream"
        }
        HttpDestination::Media => {
            mime.essence.starts_with("video/")
                || mime.essence.starts_with("audio/")
                || mime.essence == "application/octet-stream"
        }
        HttpDestination::Manifest => {
            mime.essence == "application/manifest+json"
                || mime.essence == "application/json"
                || mime.sniffed
        }
        HttpDestination::Fetch | HttpDestination::Xhr | HttpDestination::Other => true,
    }
}

fn extension_mime_table(destination: HttpDestination) -> &'static [(&'static str, &'static str)] {
    match destination {
        HttpDestination::Document => &[
            (".html", "text/html"),
            (".htm", "text/html"),
            (".xhtml", "application/xhtml+xml"),
            (".xml", "application/xml"),
        ],
        HttpDestination::Style => &[(".css", "text/css")],
        HttpDestination::Script | HttpDestination::Worker => &[
            (".js", "text/javascript"),
            (".mjs", "text/javascript"),
            (".json", "application/json"),
        ],
        HttpDestination::Image => &[
            (".png", "image/png"),
            (".jpg", "image/jpeg"),
            (".jpeg", "image/jpeg"),
            (".gif", "image/gif"),
            (".webp", "image/webp"),
            (".svg", "image/svg+xml"),
            (".bmp", "image/bmp"),
            (".ico", "image/x-icon"),
        ],
        HttpDestination::Font => &[
            (".woff2", "font/woff2"),
            (".woff", "font/woff"),
            (".ttf", "font/ttf"),
            (".otf", "font/otf"),
        ],
        HttpDestination::Media => &[
            (".mp4", "video/mp4"),
            (".webm", "video/webm"),
            (".mp3", "audio/mpeg"),
            (".wav", "audio/wav"),
        ],
        HttpDestination::Manifest => &[
            (".webmanifest", "application/manifest+json"),
            (".json", "application/json"),
        ],
        HttpDestination::Fetch | HttpDestination::Xhr | HttpDestination::Other => &[],
    }
}

fn normalize_header_name(name: &str) -> String {
    let trimmed = name.trim().to_ascii_lowercase();
    if trimmed.is_empty()
        || trimmed
            .bytes()
            .any(|byte| !matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'))
    {
        String::new()
    } else {
        trimmed
    }
}

fn normalize_header_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '\0' | '\r' | '\n'))
        .collect::<String>()
        .trim()
        .to_owned()
}

fn parse_u64(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

fn parse_http_date_lite(_value: Option<&str>) -> Option<u64> {
    // Intentionally not parsing full RFC 9110 dates yet. Module 47 keeps this
    // stable and conservative; Cache-Control max-age is the authoritative path.
    None
}

fn duration_ms(value: Duration) -> u64 {
    u64::try_from(value.as_millis()).unwrap_or(u64::MAX)
}

fn trim_ascii_prefix(bytes: &[u8]) -> &[u8] {
    let mut index = 0usize;
    while matches!(
        bytes.get(index),
        Some(b' ' | b'\n' | b'\r' | b'\t' | 0xef | 0xbb | 0xbf)
    ) {
        index = index.saturating_add(1);
        if index >= bytes.len() {
            break;
        }
    }
    &bytes[index.min(bytes.len())..]
}

fn starts_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack[..needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

fn looks_like_json(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(b'{' | b'['))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_case_insensitive() {
        let mut headers = HttpHeaderList::new();
        headers.insert("Content-Type", "Text/HTML; charset=utf-8");
        assert_eq!(
            headers.get("content-type"),
            Some("Text/HTML; charset=utf-8")
        );
        assert_eq!(
            headers.get("CONTENT-TYPE"),
            Some("Text/HTML; charset=utf-8")
        );
    }

    #[test]
    fn cache_control_parses_core_directives() {
        let parsed = HttpCacheControl::parse("max-age=60, no-cache, immutable");
        assert_eq!(parsed.max_age_secs, Some(60));
        assert!(parsed.no_cache);
        assert!(parsed.immutable);
    }

    #[test]
    fn freshness_uses_max_age() {
        let headers = HttpHeaderList::from_pairs([("cache-control", "max-age=10")]);
        let decision = compute_freshness(200, &headers, Duration::from_secs(30), 1_000);
        assert!(decision.storable);
        assert_eq!(decision.expires_at_ms, 11_000);
    }

    #[test]
    fn sniff_mime_prefers_header() {
        let headers = HttpHeaderList::from_pairs([("content-type", "text/css; charset=utf-8")]);
        let mime = sniff_mime(
            HttpDestination::Style,
            "https://x.test/a",
            &headers,
            b"body{}",
        );
        assert_eq!(mime.essence, "text/css");
        assert_eq!(mime.charset.as_deref(), Some("utf-8"));
        assert!(!mime.sniffed);
    }

    #[test]
    fn sniff_mime_uses_magic_bytes() {
        let headers = HttpHeaderList::new();
        let mime = sniff_mime(
            HttpDestination::Image,
            "https://x.test/no-ext",
            &headers,
            b"\x89PNG\r\n\x1a\nabc",
        );
        assert_eq!(mime.essence, "image/png");
        assert!(mime.sniffed);
    }
}
