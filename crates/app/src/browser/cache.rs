#![allow(
    clippy::cast_possible_truncation,
    clippy::missing_panics_doc,
    clippy::too_many_lines
)]

//! Persistent resource cache with HTTP-aware metadata.
//!
//! Module 47 upgrades the cache from "body blob plus TTL" to a small browser-ish
//! HTTP cache: status, final URL, headers, MIME classification, Cache-Control,
//! freshness, no-store handling, redirect diagnostics, and stable metadata. It
//! is still intentionally conservative. We are building a browser, not recreating
//! twenty years of cache invalidation trauma in one Tuesday evening.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use syljs::HttpHeaderList;
use tracing::{debug, warn};
use url::Url;

use crate::browser::http::{body_prefix, header_list_from_pairs, ResourceHttpResponse};
use crate::browser::{ResourceKind, ResourceRequest};

const DEFAULT_MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_DISK_ENTRY_BYTES: usize = 32 * 1024 * 1024;
const TEMP_FILE_SUFFIX: &str = ".tmp";

/// Cache category used for different resource lifetimes and folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum CacheBucket {
    Html,
    Stylesheet,
    Image,
    Font,
    Script,
}

impl CacheBucket {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Stylesheet => "stylesheets",
            Self::Image => "images",
            Self::Font => "fonts",
            Self::Script => "scripts",
        }
    }

    #[must_use]
    pub(crate) const fn resource_kind(self) -> ResourceKind {
        match self {
            Self::Html => ResourceKind::Document,
            Self::Stylesheet => ResourceKind::Stylesheet,
            Self::Image => ResourceKind::Image,
            Self::Font => ResourceKind::Font,
            Self::Script => ResourceKind::Script,
        }
    }
}

/// Where a resource came from during a load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheSource {
    Disabled,
    Network,
    Memory,
    Disk,
}

impl CacheSource {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled-network",
            Self::Network => "network",
            Self::Memory => "memory",
            Self::Disk => "disk",
        }
    }
}

/// Cache runtime configuration.
#[derive(Debug, Clone)]
pub(crate) struct CachePolicy {
    pub enabled: bool,
    pub memory_limit_bytes: usize,
    pub max_disk_entry_bytes: usize,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            memory_limit_bytes: DEFAULT_MEMORY_LIMIT_BYTES,
            max_disk_entry_bytes: DEFAULT_MAX_DISK_ENTRY_BYTES,
        }
    }
}

impl CachePolicy {
    /// Returns a disabled policy that still allows network fetches.
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }
}

/// Cached UTF-8 text response.
#[derive(Debug, Clone)]
pub(crate) struct CachedText {
    pub url: String,
    pub final_url: String,
    pub text: String,
    pub bytes: usize,
    pub source: CacheSource,
    pub status: u16,
    pub headers: HttpHeaderList,
    pub mime: String,
    pub redirects: usize,
}

/// Cached binary response bytes.
#[derive(Debug, Clone)]
pub(crate) struct CachedBytes {
    pub url: String,
    pub final_url: String,
    pub bytes: Vec<u8>,
    pub source: CacheSource,
    pub status: u16,
    pub headers: HttpHeaderList,
    pub mime: String,
    pub redirects: usize,
}

/// Thread-safe cache store used by the browser pipeline.
#[derive(Debug, Clone)]
pub(crate) struct CacheStore {
    inner: Arc<CacheInner>,
}

#[derive(Debug)]
struct CacheInner {
    root: PathBuf,
    policy: CachePolicy,
    memory: Mutex<MemoryCache>,
}

#[derive(Debug, Default)]
struct MemoryCache {
    entries: HashMap<String, MemoryEntry>,
    order: VecDeque<String>,
    total_bytes: usize,
}

#[derive(Debug, Clone)]
struct MemoryEntry {
    metadata: CacheMetadata,
    body: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    metadata: CacheMetadata,
    body: Vec<u8>,
    source: CacheSource,
}

#[derive(Debug, Clone)]
struct CachePaths {
    body: PathBuf,
    meta: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheMetadata {
    version: u32,
    url: String,
    final_url: String,
    bucket: String,
    fetched_at_ms: u64,
    expires_at_ms: u64,
    revalidate_on_use: bool,
    status: u16,
    headers: Vec<(String, String)>,
    mime: String,
    mime_sniffed: bool,
    redirects: usize,
    byte_len: u64,
}

impl CacheStore {
    /// Creates an enabled cache store using the platform default cache path.
    #[must_use]
    pub(crate) fn new(policy: CachePolicy) -> Self {
        Self::with_root(default_cache_root(), policy)
    }

    /// Creates a cache store rooted at a caller-provided path.
    #[must_use]
    pub(crate) fn with_root(root: PathBuf, policy: CachePolicy) -> Self {
        Self {
            inner: Arc::new(CacheInner {
                root,
                policy,
                memory: Mutex::new(MemoryCache::default()),
            }),
        }
    }

    /// Creates a cache store that bypasses reads and writes.
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self::with_root(default_cache_root(), CachePolicy::disabled())
    }

    /// Returns true when cache reads/writes are enabled.
    #[must_use]
    pub(crate) fn is_enabled(&self) -> bool {
        self.inner.policy.enabled
    }

    /// Returns the disk cache root path.
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.inner.root
    }

    /// Returns a displayable cache root string.
    #[must_use]
    pub(crate) fn root_display(&self) -> String {
        self.root().display().to_string()
    }

    /// Clears disk and memory cache contents.
    pub(crate) fn clear(&self) -> Result<()> {
        if let Ok(mut memory) = self.inner.memory.lock() {
            memory.clear();
        }
        if self.inner.root.exists() {
            fs::remove_dir_all(&self.inner.root)
                .with_context(|| format!("failed to clear cache `{}`", self.root_display()))?;
        }
        Ok(())
    }

    /// Legacy document fetch helper.
    #[allow(dead_code)]
    pub(crate) async fn get_or_fetch_text(&self, url: &str) -> Result<CachedText> {
        self.get_or_fetch_text_in_bucket(CacheBucket::Html, url)
            .await
    }

    /// Legacy text fetch helper using default HTTP headers.
    pub(crate) async fn get_or_fetch_text_in_bucket(
        &self,
        bucket: CacheBucket,
        url: &str,
    ) -> Result<CachedText> {
        let request = ResourceRequest::new(bucket.resource_kind(), url.to_owned());
        self.get_or_fetch_text_in_bucket_with_http(
            bucket,
            url,
            &HttpHeaderList::new(),
            request.max_bytes,
        )
        .await
    }

    /// HTTP-aware text fetch helper.
    pub(crate) async fn get_or_fetch_text_in_bucket_with_http(
        &self,
        bucket: CacheBucket,
        url: &str,
        request_headers: &HttpHeaderList,
        max_bytes: usize,
    ) -> Result<CachedText> {
        let bytes = self
            .get_or_fetch_bytes_in_bucket_with_http(bucket, url, request_headers, max_bytes)
            .await?;
        let text = String::from_utf8(bytes.bytes.clone())
            .with_context(|| format!("text resource `{}` was not valid UTF-8", bytes.final_url))?;
        let byte_len = text.len();
        Ok(CachedText {
            url: bytes.url,
            final_url: bytes.final_url,
            text,
            bytes: byte_len,
            source: bytes.source,
            status: bytes.status,
            headers: bytes.headers,
            mime: bytes.mime,
            redirects: bytes.redirects,
        })
    }

    /// Legacy bytes helper.
    #[allow(dead_code)]
    pub(crate) async fn get_or_fetch_bytes(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<CachedBytes> {
        self.get_or_fetch_bytes_in_bucket(CacheBucket::Image, url, max_bytes)
            .await
    }

    /// Legacy bytes helper using default HTTP headers.
    pub(crate) async fn get_or_fetch_bytes_in_bucket(
        &self,
        bucket: CacheBucket,
        url: &str,
        max_bytes: usize,
    ) -> Result<CachedBytes> {
        self.get_or_fetch_bytes_in_bucket_with_http(bucket, url, &HttpHeaderList::new(), max_bytes)
            .await
    }

    /// HTTP-aware bytes fetch helper.
    pub(crate) async fn get_or_fetch_bytes_in_bucket_with_http(
        &self,
        bucket: CacheBucket,
        url: &str,
        request_headers: &HttpHeaderList,
        max_bytes: usize,
    ) -> Result<CachedBytes> {
        ensure_fetchable_url(url)?;
        if max_bytes == 0 {
            bail!("resource `{url}` has zero byte limit");
        }

        if self.inner.policy.enabled {
            if let Some(entry) = self.read(bucket, url) {
                if entry.body.len() <= max_bytes {
                    return Ok(bytes_from_entry(url, entry));
                }
                warn!(url = %url, bytes = entry.body.len(), max_bytes, bucket = bucket.as_str(), "cached resource exceeded caller limit; refetching");
            }
        }

        let network = self
            .fetch_network(bucket, url, request_headers, max_bytes)
            .await?;
        if self.inner.policy.enabled
            && network.metadata.expires_at_ms > network.metadata.fetched_at_ms
        {
            self.write(bucket, url, &network.metadata, &network.body);
        }

        Ok(bytes_from_entry(
            url,
            CacheEntry {
                source: if self.inner.policy.enabled {
                    CacheSource::Network
                } else {
                    CacheSource::Disabled
                },
                ..network
            },
        ))
    }

    async fn fetch_network(
        &self,
        bucket: CacheBucket,
        url: &str,
        request_headers: &HttpHeaderList,
        max_bytes: usize,
    ) -> Result<CacheEntry> {
        let request =
            ResourceRequest::new(bucket.resource_kind(), url.to_owned()).max_bytes(max_bytes);
        let mut options = fetch::RequestOptions::get(url)
            .max_body_bytes(max_bytes)
            .headers(request_headers.to_pairs());

        if !request_headers.contains("accept") {
            options = options.header(
                "Accept",
                crate::browser::http::destination_for_resource_kind(request.kind).accept_header(),
            );
        }

        fetch::init(Default::default());
        let response = fetch::request(options).await?;
        let status = response.status_u16();
        let final_url = response.final_url().to_owned();
        let redirects = response.redirect_chain().len();
        let headers = header_list_from_pairs(response.header_pairs());
        let body = response.get_bytes().await?;

        if !syljs::is_success_status(status) {
            bail!("resource request returned status {status}");
        }
        if body.len() > max_bytes {
            bail!("resource exceeds byte limit of {max_bytes} bytes");
        }

        let now = now_ms();
        let classified = ResourceHttpResponse::classify(
            &request,
            final_url.clone(),
            status,
            headers.clone(),
            body_prefix(&body),
            now,
        );

        if !classified.mime_allowed {
            bail!(
                "resource `{final_url}` MIME `{}` is not allowed for `{}`",
                classified.mime.essence,
                request.kind.as_str()
            );
        }

        let metadata = CacheMetadata {
            version: 2,
            url: url.to_owned(),
            final_url,
            bucket: bucket.as_str().to_owned(),
            fetched_at_ms: now,
            expires_at_ms: if classified.freshness.storable {
                classified.freshness.expires_at_ms
            } else {
                now
            },
            revalidate_on_use: classified.freshness.revalidate_on_use,
            status,
            headers: headers.to_pairs(),
            mime: classified.mime.essence,
            mime_sniffed: classified.mime.sniffed,
            redirects,
            byte_len: usize_to_u64(body.len()),
        };

        Ok(CacheEntry {
            metadata,
            body,
            source: CacheSource::Network,
        })
    }

    fn read(&self, bucket: CacheBucket, url: &str) -> Option<CacheEntry> {
        let key = cache_key(bucket, url);
        let now = now_ms();

        if let Some(entry) = self.read_memory(&key, bucket, now) {
            return Some(CacheEntry {
                source: CacheSource::Memory,
                ..entry
            });
        }

        match self.read_disk(bucket, url, now) {
            Ok(Some(entry)) => {
                self.write_memory(key, entry.metadata.clone(), entry.body.clone());
                Some(CacheEntry {
                    source: CacheSource::Disk,
                    ..entry
                })
            }
            Ok(None) => None,
            Err(error) => {
                debug!(url = %url, error = %error, "disk cache read failed; treating as miss");
                None
            }
        }
    }

    fn read_memory(&self, key: &str, bucket: CacheBucket, now: u64) -> Option<CacheEntry> {
        let mut memory = self.inner.memory.lock().ok()?;
        memory.get(key, bucket, now)
    }

    fn write(&self, bucket: CacheBucket, url: &str, metadata: &CacheMetadata, body: &[u8]) {
        if metadata.expires_at_ms <= metadata.fetched_at_ms
            || body.len() > self.inner.policy.max_disk_entry_bytes
        {
            debug!(url = %url, bytes = body.len(), "resource not stored by HTTP cache policy");
            return;
        }

        let key = cache_key(bucket, url);
        self.write_memory(key, metadata.clone(), body.to_vec());

        if let Err(error) = self.write_disk(bucket, url, metadata, body) {
            debug!(url = %url, error = %error, "disk cache write failed");
        }
    }

    fn write_memory(&self, key: String, metadata: CacheMetadata, body: Vec<u8>) {
        if body.len() > self.inner.policy.memory_limit_bytes {
            return;
        }
        if let Ok(mut memory) = self.inner.memory.lock() {
            memory.put(
                key,
                MemoryEntry { metadata, body },
                self.inner.policy.memory_limit_bytes,
            );
        }
    }

    fn read_disk(&self, bucket: CacheBucket, url: &str, now: u64) -> Result<Option<CacheEntry>> {
        let paths = self.entry_paths(bucket, url);
        if !paths.body.exists() || !paths.meta.exists() {
            return Ok(None);
        }

        let metadata_bytes = fs::read(&paths.meta)
            .with_context(|| format!("failed to read cache metadata `{}`", paths.meta.display()))?;
        let metadata: CacheMetadata =
            serde_json::from_slice(&metadata_bytes).with_context(|| {
                format!("failed to parse cache metadata `{}`", paths.meta.display())
            })?;

        if metadata.version != 2 || metadata.url != url || metadata.bucket != bucket.as_str() {
            return Ok(None);
        }
        if metadata.revalidate_on_use || metadata.expires_at_ms <= now {
            debug!(url = %url, bucket = bucket.as_str(), "HTTP cache entry is stale or requires revalidation");
            return Ok(None);
        }
        if metadata.byte_len > usize_to_u64(self.inner.policy.max_disk_entry_bytes) {
            return Ok(None);
        }

        let body = fs::read(&paths.body)
            .with_context(|| format!("failed to read cache body `{}`", paths.body.display()))?;
        if usize_to_u64(body.len()) != metadata.byte_len {
            return Ok(None);
        }

        Ok(Some(CacheEntry {
            metadata,
            body,
            source: CacheSource::Disk,
        }))
    }

    fn write_disk(
        &self,
        bucket: CacheBucket,
        url: &str,
        metadata: &CacheMetadata,
        body: &[u8],
    ) -> Result<()> {
        let paths = self.entry_paths(bucket, url);
        if let Some(parent) = paths.body.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create cache directory `{}`", parent.display())
            })?;
        }

        let metadata_bytes =
            serde_json::to_vec_pretty(metadata).context("failed to serialize cache metadata")?;
        write_atomic(&paths.body, body)?;
        write_atomic(&paths.meta, &metadata_bytes)?;
        Ok(())
    }

    fn entry_paths(&self, bucket: CacheBucket, url: &str) -> CachePaths {
        let key = stable_hash_hex(url.as_bytes());
        let dir = self.inner.root.join(bucket.as_str());
        CachePaths {
            body: dir.join(format!("{key}.body")),
            meta: dir.join(format!("{key}.json")),
        }
    }
}

impl MemoryCache {
    fn get(&mut self, key: &str, bucket: CacheBucket, now: u64) -> Option<CacheEntry> {
        let entry = self.entries.get(key)?;
        if entry.metadata.bucket != bucket.as_str()
            || entry.metadata.revalidate_on_use
            || entry.metadata.expires_at_ms <= now
        {
            self.remove(key);
            return None;
        }
        let output = CacheEntry {
            metadata: entry.metadata.clone(),
            body: entry.body.clone(),
            source: CacheSource::Memory,
        };
        self.touch(key);
        Some(output)
    }

    fn put(&mut self, key: String, entry: MemoryEntry, limit_bytes: usize) {
        self.remove(&key);
        self.total_bytes = self.total_bytes.saturating_add(entry.body.len());
        self.order.push_back(key.clone());
        self.entries.insert(key, entry);
        self.evict_to_limit(limit_bytes);
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.total_bytes = 0;
    }

    fn remove(&mut self, key: &str) {
        if let Some(entry) = self.entries.remove(key) {
            self.total_bytes = self.total_bytes.saturating_sub(entry.body.len());
        }
        self.order.retain(|existing| existing != key);
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|existing| existing != key);
        self.order.push_back(key.to_owned());
    }

    fn evict_to_limit(&mut self, limit_bytes: usize) {
        while self.total_bytes > limit_bytes {
            let Some(oldest) = self.order.pop_front() else {
                self.total_bytes = 0;
                self.entries.clear();
                return;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.body.len());
            }
        }
    }
}

fn bytes_from_entry(request_url: &str, entry: CacheEntry) -> CachedBytes {
    let headers = HttpHeaderList::from_pairs(entry.metadata.headers.clone());
    CachedBytes {
        url: request_url.to_owned(),
        final_url: entry.metadata.final_url,
        bytes: entry.body,
        source: entry.source,
        status: entry.metadata.status,
        headers,
        mime: entry.metadata.mime,
        redirects: entry.metadata.redirects,
    }
}

fn ensure_fetchable_url(url: &str) -> Result<()> {
    let parsed = Url::parse(url).with_context(|| format!("invalid cache URL `{url}`"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        other => bail!("cache only supports http/https URLs, got `{other}`"),
    }
}

fn cache_key(bucket: CacheBucket, url: &str) -> String {
    format!("{}:{}", bucket.as_str(), stable_hash_hex(url.as_bytes()))
}

fn stable_hash_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn default_cache_root() -> PathBuf {
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("Kairais")
            .join("Syphos")
            .join("cache");
    }
    if let Some(xdg_cache_home) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(xdg_cache_home).join("syphos");
    }
    if let Some(home) = env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        if cfg!(target_os = "macos") {
            return home_path
                .join("Library")
                .join("Caches")
                .join("Kairais")
                .join("Syphos");
        }
        return home_path.join(".cache").join("syphos");
    }
    PathBuf::from(".syphos-cache")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp_path = path.with_extension(format!(
        "{}{}",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("cache"),
        TEMP_FILE_SUFFIX
    ));
    fs::write(&temp_path, bytes).with_context(|| {
        format!(
            "failed to write temporary cache file `{}`",
            temp_path.display()
        )
    })?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(first_error) => {
            if path.exists() {
                let _ = fs::remove_file(path);
            }
            fs::rename(&temp_path, path).with_context(|| {
                format!(
                    "failed to move temporary cache file `{}` to `{}` after first error `{}`",
                    temp_path.display(),
                    path.display(),
                    first_error
                )
            })
        }
    }
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    u64::try_from(millis).unwrap_or(u64::MAX)
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hash_is_deterministic() {
        assert_eq!(
            stable_hash_hex(b"https://example.com"),
            stable_hash_hex(b"https://example.com")
        );
        assert_ne!(
            stable_hash_hex(b"https://example.com"),
            stable_hash_hex(b"https://example.org")
        );
    }

    #[test]
    fn memory_cache_respects_http_expiry() {
        let mut cache = MemoryCache::default();
        cache.put(
            "html:key".to_owned(),
            MemoryEntry {
                metadata: CacheMetadata {
                    version: 2,
                    url: "https://example.com".to_owned(),
                    final_url: "https://example.com".to_owned(),
                    bucket: "html".to_owned(),
                    fetched_at_ms: 100,
                    expires_at_ms: 500,
                    revalidate_on_use: false,
                    status: 200,
                    headers: vec![],
                    mime: "text/html".to_owned(),
                    mime_sniffed: false,
                    redirects: 0,
                    byte_len: 5,
                },
                body: b"hello".to_vec(),
            },
            1024,
        );
        assert!(cache.get("html:key", CacheBucket::Html, 250).is_some());
        assert!(cache.get("html:key", CacheBucket::Html, 700).is_none());
    }

    #[test]
    fn disabled_policy_is_disabled() {
        assert!(!CachePolicy::disabled().enabled);
    }
}
