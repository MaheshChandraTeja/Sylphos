#![allow(dead_code)]

//! Central resource scheduler and network pipeline manager.
//!
//! This module keeps document, stylesheet, image, and future font/script resource
//! loading behind one typed interface. It coordinates cache bucket selection,
//! byte limits, request priority metadata, timing diagnostics, batch de-duplication,
//! and network/cache source accounting.
//!
//! It is deliberately conservative. The current implementation executes resource
//! requests through the existing cache/fetch stack, but makes every resource load
//! explicit and observable so later modules can add true parallelism, cancellation,
//! origin limits, and devtools-style timing without rewriting call sites again.

use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{debug, warn};
use url::Url;

use crate::browser::{CacheBucket, CacheSource, CacheStore};

const DEFAULT_MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_STYLESHEET_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const DEFAULT_MAX_FONT_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_MAX_SCRIPT_BYTES: usize = 2 * 1024 * 1024;

/// Browser resource type handled by the network pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ResourceKind {
    /// Top-level HTML document.
    Document,

    /// External CSS stylesheet.
    Stylesheet,

    /// Image resource.
    Image,

    /// Font resource reserved for future font loading.
    Font,

    /// JavaScript source text.
    Script,
}

impl ResourceKind {
    /// Stable label for logs and diagnostics.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Stylesheet => "stylesheet",
            Self::Image => "image",
            Self::Font => "font",
            Self::Script => "script",
        }
    }

    #[must_use]
    pub(crate) const fn default_max_bytes(self) -> usize {
        match self {
            Self::Document => DEFAULT_MAX_DOCUMENT_BYTES,
            Self::Stylesheet => DEFAULT_MAX_STYLESHEET_BYTES,
            Self::Image => DEFAULT_MAX_IMAGE_BYTES,
            Self::Font => DEFAULT_MAX_FONT_BYTES,
            Self::Script => DEFAULT_MAX_SCRIPT_BYTES,
        }
    }

    #[must_use]
    const fn cache_bucket(self) -> Option<CacheBucket> {
        match self {
            Self::Document => Some(CacheBucket::Html),
            Self::Stylesheet => Some(CacheBucket::Stylesheet),
            Self::Image => Some(CacheBucket::Image),
            Self::Font => Some(CacheBucket::Font),
            Self::Script => Some(CacheBucket::Script),
        }
    }

    #[must_use]
    const fn is_text(self) -> bool {
        matches!(self, Self::Document | Self::Stylesheet | Self::Script)
    }
}

/// Scheduler priority hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ResourcePriority {
    /// Navigation-critical resource.
    High,

    /// Page rendering resource.
    Normal,

    /// Opportunistic resource.
    Low,
}

impl ResourcePriority {
    #[must_use]
    const fn sort_rank(self) -> u8 {
        match self {
            Self::High => 0,
            Self::Normal => 1,
            Self::Low => 2,
        }
    }

    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }
}

/// Typed request for a resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceRequest {
    /// Resource kind.
    pub kind: ResourceKind,

    /// Absolute URL.
    pub url: String,

    /// Priority hint.
    pub priority: ResourcePriority,

    /// Maximum accepted response body size.
    pub max_bytes: usize,

    /// Navigation id this resource belongs to.
    pub navigation_id: Option<u64>,
}

impl ResourceRequest {
    /// Creates a request with defaults based on resource kind.
    #[must_use]
    pub(crate) fn new(kind: ResourceKind, url: impl Into<String>) -> Self {
        Self {
            kind,
            url: url.into(),
            priority: default_priority(kind),
            max_bytes: kind.default_max_bytes(),
            navigation_id: None,
        }
    }

    /// Creates a document request.
    #[must_use]
    pub(crate) fn document(url: impl Into<String>) -> Self {
        Self::new(ResourceKind::Document, url).priority(ResourcePriority::High)
    }

    /// Creates a stylesheet request.
    #[must_use]
    pub(crate) fn stylesheet(url: impl Into<String>) -> Self {
        Self::new(ResourceKind::Stylesheet, url).priority(ResourcePriority::High)
    }

    /// Creates an image request.
    #[must_use]
    pub(crate) fn image(url: impl Into<String>) -> Self {
        Self::new(ResourceKind::Image, url).priority(ResourcePriority::Low)
    }

    /// Creates a JavaScript request.
    #[must_use]
    pub(crate) fn script(url: impl Into<String>) -> Self {
        Self::new(ResourceKind::Script, url).priority(ResourcePriority::High)
    }

    /// Sets priority.
    #[must_use]
    pub(crate) fn priority(mut self, priority: ResourcePriority) -> Self {
        self.priority = priority;
        self
    }

    /// Sets byte limit.
    #[must_use]
    pub(crate) fn max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Tags the request with a navigation id.
    #[must_use]
    pub(crate) fn navigation_id(mut self, navigation_id: u64) -> Self {
        self.navigation_id = Some(navigation_id);
        self
    }

    #[must_use]
    fn key(&self) -> ResourceKey {
        ResourceKey {
            kind: self.kind,
            url: self.url.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ResourceKey {
    kind: ResourceKind,
    url: String,
}

/// Text resource response.
#[derive(Debug, Clone)]
pub(crate) struct ResourceText {
    /// Request kind.
    pub kind: ResourceKind,

    /// Final URL used for the resource.
    pub url: String,

    /// UTF-8 body.
    pub text: String,

    /// Body bytes.
    pub bytes: usize,

    /// Cache source.
    pub source: CacheSource,

    /// Timing data.
    pub timing: ResourceTiming,
}

/// Binary resource response.
#[derive(Debug, Clone)]
pub(crate) struct ResourceBytes {
    /// Request kind.
    pub kind: ResourceKind,

    /// Final URL used for the resource.
    pub url: String,

    /// Body bytes.
    pub bytes: Vec<u8>,

    /// Cache source.
    pub source: CacheSource,

    /// Timing data.
    pub timing: ResourceTiming,
}

/// Timing values captured by the scheduler.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ResourceTiming {
    /// Time spent waiting before execution. Currently zero unless future queueing is added.
    pub queued: Duration,

    /// Time spent in cache/network fetch.
    pub fetch: Duration,

    /// Full request time observed by the scheduler.
    pub total: Duration,
}

/// Resource pipeline diagnostics snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ResourceSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub bytes: usize,
    pub documents: usize,
    pub stylesheets: usize,
    pub images: usize,
    pub fonts: usize,
    pub scripts: usize,
    pub scripts_blocked: usize,
    pub memory_hits: usize,
    pub disk_hits: usize,
    pub network_fetches: usize,
    pub disabled_fetches: usize,
    pub total_fetch_ms: u128,
}

impl ResourceSummary {
    /// Returns a compact log-friendly summary string.
    #[must_use]
    pub(crate) fn compact(&self) -> String {
        format!(
            "total={} ok={} failed={} bytes={} scripts={} script_blocked={} net={} mem={} disk={} disabled={} fetch_ms={}",
            self.total,
            self.succeeded,
            self.failed,
            self.bytes,
            self.scripts,
            self.scripts_blocked,
            self.network_fetches,
            self.memory_hits,
            self.disk_hits,
            self.disabled_fetches,
            self.total_fetch_ms
        )
    }
}

/// Scheduler configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceSchedulerPolicy {
    /// Hard upper bound for batch fetch inputs after de-duplication.
    pub max_batch_items: usize,

    /// Whether script resources are allowed.
    pub allow_scripts: bool,
}

impl Default for ResourceSchedulerPolicy {
    fn default() -> Self {
        Self {
            max_batch_items: 128,
            allow_scripts: true,
        }
    }
}

/// Central network pipeline manager.
#[derive(Debug, Clone)]
pub(crate) struct ResourceScheduler {
    cache: CacheStore,
    policy: ResourceSchedulerPolicy,
    summary: Arc<Mutex<ResourceSummary>>,
}

impl ResourceScheduler {
    /// Creates a scheduler using the default policy.
    #[must_use]
    pub(crate) fn new(cache: CacheStore) -> Self {
        Self::with_policy(cache, ResourceSchedulerPolicy::default())
    }

    /// Creates a scheduler using a custom policy.
    #[must_use]
    pub(crate) fn with_policy(cache: CacheStore, policy: ResourceSchedulerPolicy) -> Self {
        Self {
            cache,
            policy,
            summary: Arc::new(Mutex::new(ResourceSummary::default())),
        }
    }

    /// Returns the underlying cache store.
    #[must_use]
    pub(crate) fn cache(&self) -> &CacheStore {
        &self.cache
    }

    /// Returns a diagnostic snapshot.
    #[must_use]
    pub(crate) fn summary(&self) -> ResourceSummary {
        self.summary
            .lock()
            .map_or_else(|_| ResourceSummary::default(), |summary| summary.clone())
    }

    /// Resets diagnostics. Useful at the start of a top-level navigation.
    pub(crate) fn reset_summary(&self) {
        if let Ok(mut summary) = self.summary.lock() {
            *summary = ResourceSummary::default();
        }
    }

    /// Fetches a text resource.
    pub(crate) async fn fetch_text(&self, request: ResourceRequest) -> Result<ResourceText> {
        self.validate_request(&request)?;
        if !request.kind.is_text() {
            self.record_failure(request.kind);
            bail!(
                "resource kind `{}` is not a text resource",
                request.kind.as_str()
            );
        }

        if request.kind == ResourceKind::Script && !self.policy.allow_scripts {
            self.record_script_block();
            bail!("script resources are disabled by policy");
        }

        let bucket = request
            .kind
            .cache_bucket()
            .context("text resource has no cache bucket")?;

        let started = Instant::now();
        debug!(
            kind = request.kind.as_str(),
            priority = request.priority.as_str(),
            url = %request.url,
            "fetching text resource"
        );

        let cached = match self
            .cache
            .get_or_fetch_text_in_bucket(bucket, &request.url)
            .await
        {
            Ok(cached) => cached,
            Err(error) => {
                self.record_failure(request.kind);
                return Err(error);
            }
        };

        if cached.bytes > request.max_bytes {
            self.record_failure(request.kind);
            bail!(
                "{} resource `{}` exceeded byte limit: {} > {}",
                request.kind.as_str(),
                request.url,
                cached.bytes,
                request.max_bytes
            );
        }

        let timing = ResourceTiming {
            queued: Duration::ZERO,
            fetch: started.elapsed(),
            total: started.elapsed(),
        };

        self.record_success(request.kind, cached.source, cached.bytes, timing);

        Ok(ResourceText {
            kind: request.kind,
            url: cached.url,
            text: cached.text,
            bytes: cached.bytes,
            source: cached.source,
            timing,
        })
    }

    /// Fetches a binary resource.
    pub(crate) async fn fetch_bytes(&self, request: ResourceRequest) -> Result<ResourceBytes> {
        self.validate_request(&request)?;
        if request.kind.is_text() {
            self.record_failure(request.kind);
            bail!(
                "resource kind `{}` is not a binary resource",
                request.kind.as_str()
            );
        }

        let bucket = request
            .kind
            .cache_bucket()
            .context("binary resource has no cache bucket")?;

        let started = Instant::now();
        debug!(
            kind = request.kind.as_str(),
            priority = request.priority.as_str(),
            max_bytes = request.max_bytes,
            url = %request.url,
            "fetching binary resource"
        );

        let cached = match self
            .cache
            .get_or_fetch_bytes_in_bucket(bucket, &request.url, request.max_bytes)
            .await
        {
            Ok(cached) => cached,
            Err(error) => {
                self.record_failure(request.kind);
                return Err(error);
            }
        };

        let timing = ResourceTiming {
            queued: Duration::ZERO,
            fetch: started.elapsed(),
            total: started.elapsed(),
        };

        self.record_success(request.kind, cached.source, cached.bytes.len(), timing);

        Ok(ResourceBytes {
            kind: request.kind,
            url: cached.url,
            bytes: cached.bytes,
            source: cached.source,
            timing,
        })
    }

    /// Fetches a batch of text resources with de-duplication and priority ordering.
    pub(crate) async fn fetch_text_batch(
        &self,
        requests: impl IntoIterator<Item = ResourceRequest>,
    ) -> Vec<ResourceBatchTextResult> {
        let mut deduped = BTreeMap::<BatchSortKey, ResourceRequest>::new();

        for request in requests {
            let sort_key = BatchSortKey {
                priority: request.priority.sort_rank(),
                key: request.key(),
            };
            deduped.entry(sort_key).or_insert(request);
            if deduped.len() >= self.policy.max_batch_items {
                break;
            }
        }

        let mut results = Vec::with_capacity(deduped.len());
        for request in deduped.into_values() {
            let result = self
                .fetch_text(request.clone())
                .await
                .map_err(|error| error.to_string());
            results.push(ResourceBatchTextResult { request, result });
        }
        results
    }

    /// Fetches a batch of binary resources with de-duplication and priority ordering.
    pub(crate) async fn fetch_bytes_batch(
        &self,
        requests: impl IntoIterator<Item = ResourceRequest>,
    ) -> Vec<ResourceBatchBytesResult> {
        let mut deduped = BTreeMap::<BatchSortKey, ResourceRequest>::new();

        for request in requests {
            let sort_key = BatchSortKey {
                priority: request.priority.sort_rank(),
                key: request.key(),
            };
            deduped.entry(sort_key).or_insert(request);
            if deduped.len() >= self.policy.max_batch_items {
                break;
            }
        }

        let mut results = Vec::with_capacity(deduped.len());
        for request in deduped.into_values() {
            let result = self
                .fetch_bytes(request.clone())
                .await
                .map_err(|error| error.to_string());
            results.push(ResourceBatchBytesResult { request, result });
        }
        results
    }

    fn validate_request(&self, request: &ResourceRequest) -> Result<()> {
        if request.max_bytes == 0 {
            bail!("resource `{}` has a zero byte limit", request.url);
        }

        let parsed = Url::parse(&request.url)
            .with_context(|| format!("invalid resource URL `{}`", request.url))?;
        match parsed.scheme() {
            "http" | "https" => Ok(()),
            other => bail!("unsupported resource URL scheme `{other}`"),
        }
    }

    fn record_success(
        &self,
        kind: ResourceKind,
        source: CacheSource,
        bytes: usize,
        timing: ResourceTiming,
    ) {
        if let Ok(mut summary) = self.summary.lock() {
            summary.total = summary.total.saturating_add(1);
            summary.succeeded = summary.succeeded.saturating_add(1);
            summary.bytes = summary.bytes.saturating_add(bytes);
            summary.total_fetch_ms = summary
                .total_fetch_ms
                .saturating_add(timing.fetch.as_millis());

            match kind {
                ResourceKind::Document => summary.documents = summary.documents.saturating_add(1),
                ResourceKind::Stylesheet => {
                    summary.stylesheets = summary.stylesheets.saturating_add(1);
                }
                ResourceKind::Image => summary.images = summary.images.saturating_add(1),
                ResourceKind::Font => summary.fonts = summary.fonts.saturating_add(1),
                ResourceKind::Script => summary.scripts = summary.scripts.saturating_add(1),
            }

            match source {
                CacheSource::Disabled => {
                    summary.disabled_fetches = summary.disabled_fetches.saturating_add(1);
                }
                CacheSource::Network => {
                    summary.network_fetches = summary.network_fetches.saturating_add(1);
                }
                CacheSource::Memory => {
                    summary.memory_hits = summary.memory_hits.saturating_add(1);
                }
                CacheSource::Disk => summary.disk_hits = summary.disk_hits.saturating_add(1),
            }
        }
    }

    fn record_failure(&self, kind: ResourceKind) {
        if let Ok(mut summary) = self.summary.lock() {
            summary.total = summary.total.saturating_add(1);
            summary.failed = summary.failed.saturating_add(1);
            if kind == ResourceKind::Script {
                summary.scripts_blocked = summary.scripts_blocked.saturating_add(1);
            }
        }
    }

    fn record_script_block(&self) {
        self.record_failure(ResourceKind::Script);
        warn!("blocked script resource by resource scheduler policy");
    }
}

/// Batch text result.
#[derive(Debug)]
pub(crate) struct ResourceBatchTextResult {
    pub request: ResourceRequest,
    pub result: std::result::Result<ResourceText, String>,
}

/// Batch binary result.
#[derive(Debug)]
pub(crate) struct ResourceBatchBytesResult {
    pub request: ResourceRequest,
    pub result: std::result::Result<ResourceBytes, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BatchSortKey {
    priority: u8,
    key: ResourceKey,
}

#[must_use]
const fn default_priority(kind: ResourceKind) -> ResourcePriority {
    match kind {
        ResourceKind::Document | ResourceKind::Stylesheet => ResourcePriority::High,
        ResourceKind::Image | ResourceKind::Font => ResourcePriority::Low,
        ResourceKind::Script => ResourcePriority::High,
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_defaults_follow_resource_kind() {
        let doc = ResourceRequest::document("https://example.com/");
        assert_eq!(doc.kind, ResourceKind::Document);
        assert_eq!(doc.priority, ResourcePriority::High);
        assert_eq!(doc.max_bytes, DEFAULT_MAX_DOCUMENT_BYTES);

        let image = ResourceRequest::image("https://example.com/a.png");
        assert_eq!(image.kind, ResourceKind::Image);
        assert_eq!(image.priority, ResourcePriority::Low);
        assert_eq!(image.max_bytes, DEFAULT_MAX_IMAGE_BYTES);
    }

    #[test]
    fn summary_compact_is_stable() {
        let summary = ResourceSummary {
            total: 2,
            succeeded: 1,
            failed: 1,
            bytes: 42,
            network_fetches: 1,
            ..ResourceSummary::default()
        };
        assert!(summary.compact().contains("total=2"));
        assert!(summary.compact().contains("bytes=42"));
    }
}
