#![allow(dead_code)]

//! Browser-facing HTTP request/response semantics adapter.
//!
//! This module maps Sylphos resource kinds into the shared `syljs::http`
//! protocol model: Accept headers, redirect modes, MIME sniffing, cache-control,
//! and diagnostics. The network stack should not be guessing whether a `.css`
//! file is CSS in five different places, because apparently computers already
//! have enough ways to disappoint us.

use crate::browser::{ResourceKind, ResourceRequest};
use std::time::Duration;
use syljs::{
    compute_freshness, mime_allowed_for_destination, sniff_mime, HttpCacheMode, HttpDestination,
    HttpFreshnessDecision, HttpHeaderList, HttpMethod, HttpMimeType, HttpRedirectMode,
};

/// Browser HTTP diagnostics accumulated by the resource scheduler.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HttpSemanticsSummary {
    pub requests: usize,
    pub redirects: usize,
    pub cacheable: usize,
    pub not_cacheable: usize,
    pub mime_sniffed: usize,
    pub mime_blocked: usize,
    pub headers_bytes: usize,
}

impl HttpSemanticsSummary {
    /// Compact log text.
    #[must_use]
    pub(crate) fn compact(&self) -> String {
        format!(
            "http requests={} redirects={} cacheable={} not_cacheable={} mime_sniffed={} mime_blocked={} header_bytes={}",
            self.requests,
            self.redirects,
            self.cacheable,
            self.not_cacheable,
            self.mime_sniffed,
            self.mime_blocked,
            self.headers_bytes
        )
    }
}

/// Resolved HTTP semantics for one resource request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceHttpRequest {
    pub method: HttpMethod,
    pub destination: HttpDestination,
    pub headers: HttpHeaderList,
    pub cache_mode: HttpCacheMode,
    pub redirect_mode: HttpRedirectMode,
}

impl ResourceHttpRequest {
    /// Builds request semantics for a resource load.
    #[must_use]
    pub(crate) fn for_resource(request: &ResourceRequest) -> Self {
        let destination = destination_for_resource_kind(request.kind);
        let mut headers = HttpHeaderList::new();
        headers.insert("accept", destination.accept_header());
        headers.insert("sec-fetch-dest", destination.as_str());
        headers.insert("sec-fetch-mode", "cors");
        headers.insert("sec-fetch-site", "same-origin");

        Self {
            method: HttpMethod::Get,
            destination,
            headers,
            cache_mode: HttpCacheMode::Default,
            redirect_mode: HttpRedirectMode::Follow,
        }
    }

    /// Converts headers to fetch crate header pairs.
    #[must_use]
    pub(crate) fn header_pairs(&self) -> Vec<(String, String)> {
        self.headers.to_pairs()
    }
}

/// HTTP response classification for a fetched resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceHttpResponse {
    pub status: u16,
    pub final_url: String,
    pub headers: HttpHeaderList,
    pub mime: HttpMimeType,
    pub freshness: HttpFreshnessDecision,
    pub mime_allowed: bool,
}

impl ResourceHttpResponse {
    /// Creates a response classification.
    #[must_use]
    pub(crate) fn classify(
        request: &ResourceRequest,
        final_url: impl Into<String>,
        status: u16,
        headers: HttpHeaderList,
        body_prefix: &[u8],
        now_ms: u64,
    ) -> Self {
        let destination = destination_for_resource_kind(request.kind);
        let final_url = final_url.into();
        let mime = sniff_mime(destination, &final_url, &headers, body_prefix);
        let freshness = compute_freshness(
            status,
            &headers,
            default_ttl_for_resource_kind(request.kind),
            now_ms,
        );
        let mime_allowed = mime_allowed_for_destination(destination, &mime);

        Self {
            status,
            final_url,
            headers,
            mime,
            freshness,
            mime_allowed,
        }
    }
}

/// Maps app resource kind to shared HTTP destination.
#[must_use]
pub(crate) const fn destination_for_resource_kind(kind: ResourceKind) -> HttpDestination {
    match kind {
        ResourceKind::Document => HttpDestination::Document,
        ResourceKind::Stylesheet => HttpDestination::Style,
        ResourceKind::Image => HttpDestination::Image,
        ResourceKind::Font => HttpDestination::Font,
        ResourceKind::Script => HttpDestination::Script,
    }
}

/// Conservative default TTL used when response headers do not specify freshness.
#[must_use]
pub(crate) const fn default_ttl_for_resource_kind(kind: ResourceKind) -> Duration {
    match kind {
        ResourceKind::Document => Duration::from_secs(5 * 60),
        ResourceKind::Stylesheet => Duration::from_secs(24 * 60 * 60),
        ResourceKind::Image => Duration::from_secs(7 * 24 * 60 * 60),
        ResourceKind::Font => Duration::from_secs(30 * 24 * 60 * 60),
        ResourceKind::Script => Duration::from_secs(60 * 60),
    }
}

/// Converts fetch crate header pairs into normalized HTTP headers.
#[must_use]
pub(crate) fn header_list_from_pairs(
    pairs: impl IntoIterator<Item = (String, String)>,
) -> HttpHeaderList {
    HttpHeaderList::from_pairs(pairs)
}

/// Returns a body prefix suitable for MIME sniffing.
#[must_use]
pub(crate) fn body_prefix(bytes: &[u8]) -> &[u8] {
    let end = bytes.len().min(512);
    &bytes[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_resource_kind_to_accept_header() {
        let request = ResourceRequest::stylesheet("https://example.com/a.css");
        let http = ResourceHttpRequest::for_resource(&request);
        assert_eq!(http.destination, HttpDestination::Style);
        assert!(http
            .headers
            .get("accept")
            .is_some_and(|value| value.contains("text/css")));
    }

    #[test]
    fn classifies_css_response() {
        let request = ResourceRequest::stylesheet("https://example.com/app.css");
        let headers = HttpHeaderList::from_pairs([
            ("content-type", "text/css"),
            ("cache-control", "max-age=30"),
        ]);
        let response = ResourceHttpResponse::classify(
            &request,
            "https://example.com/app.css",
            200,
            headers,
            b"body { color: red; }",
            1_000,
        );
        assert_eq!(response.mime.essence, "text/css");
        assert!(response.freshness.storable);
        assert!(response.mime_allowed);
    }
}
