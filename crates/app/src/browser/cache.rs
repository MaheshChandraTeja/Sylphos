#![allow(
    clippy::cast_possible_truncation,
    clippy::missing_panics_doc,
    clippy::too_many_lines
)]

//! Persistent resource cache for Sylphos page and image fetches.
//!
//! The cache intentionally stores decoded HTTP response bodies rather than raw
//! transfer bytes. The `fetch` crate already handles redirects and compression,
//! so this module focuses on safe reuse, deterministic keys, small metadata,
//! byte limits, and conservative TTLs.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::{debug, warn};
use url::Url;

const DEFAULT_HTML_TTL_SECS: u64 = 5 * 60;
const DEFAULT_IMAGE_TTL_SECS: u64 = 7 * 24 * 60 * 60;
const DEFAULT_MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_DISK_ENTRY_BYTES: usize = 32 * 1024 * 1024;
const TEMP_FILE_SUFFIX: &str = ".tmp";
const CACHE_METADATA_VERSION: u32 = 2;

/// Cache category used for different resource lifetimes and folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CacheBucket {
    /// Main document HTML/text resources.
    Html,

    /// Image resources fetched from `<img>` tags.
    Image,
}

impl CacheBucket {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Image => "images",
        }
    }
}

/// Where a resource came from during a load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheSource {
    /// Cache was disabled for this run; the resource was fetched from network.
    Disabled,

    /// Resource was fetched from network and written to cache when possible.
    Network,

    /// Resource was served from the in-process memory cache.
    Memory,

    /// Resource was served from the persistent disk cache.
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
    /// Enables memory and disk reuse when true.
    pub enabled: bool,

    /// Time-to-live for HTML documents.
    pub html_ttl: Duration,

    /// Time-to-live for image bytes.
    pub image_ttl: Duration,

    /// Maximum bytes kept in the in-memory cache.
    pub memory_limit_bytes: usize,

    /// Maximum single entry size accepted from disk.
    pub max_disk_entry_bytes: usize,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            html_ttl: Duration::from_secs(DEFAULT_HTML_TTL_SECS),
            image_ttl: Duration::from_secs(DEFAULT_IMAGE_TTL_SECS),
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

    #[must_use]
    fn ttl_for(&self, bucket: CacheBucket) -> Duration {
        match bucket {
            CacheBucket::Html => self.html_ttl,
            CacheBucket::Image => self.image_ttl,
        }
    }
}

/// Cached UTF-8 document text.
#[derive(Debug, Clone)]
pub(crate) struct CachedText {
    /// Source URL.
    pub url: String,

    /// UTF-8 body text.
    pub text: String,

    /// Body size in bytes.
    pub bytes: usize,

    /// Cache source used for this load.
    pub source: CacheSource,
}

/// Cached opaque response bytes.
#[derive(Debug, Clone)]
pub(crate) struct CachedBytes {
    /// Source URL.
    pub url: String,

    /// Response body bytes.
    pub bytes: Vec<u8>,

    /// Cache source used for this load.
    pub source: CacheSource,
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
    bucket: CacheBucket,
    fetched_at_ms: u64,
    body: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CachePaths {
    body: PathBuf,
    meta: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheMetadata {
    version: u32,
    url: String,
    bucket: String,
    fetched_at_ms: u64,
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

    /// Returns cached document text or fetches it from network.
    pub(crate) async fn get_or_fetch_text(&self, url: &str) -> Result<CachedText> {
        ensure_fetchable_url(url)?;

        if !self.inner.policy.enabled {
            let text = fetch_text_from_network(url).await?;
            let bytes = text.len();
            return Ok(CachedText {
                url: url.to_owned(),
                text,
                bytes,
                source: CacheSource::Disabled,
            });
        }

        if let Some(bytes) = self.read(CacheBucket::Html, url) {
            let source = bytes.1;
            let text = String::from_utf8(bytes.0)
                .with_context(|| format!("cached document for `{url}` was not valid UTF-8"))?;
            let byte_len = text.len();
            return Ok(CachedText {
                url: url.to_owned(),
                text,
                bytes: byte_len,
                source,
            });
        }

        let text = fetch_text_from_network(url).await?;
        let bytes = text.as_bytes().to_vec();
        self.write(CacheBucket::Html, url, &bytes);

        Ok(CachedText {
            url: url.to_owned(),
            bytes: bytes.len(),
            text,
            source: CacheSource::Network,
        })
    }

    /// Returns cached bytes or fetches them from network with a hard byte cap.
    pub(crate) async fn get_or_fetch_bytes(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<CachedBytes> {
        ensure_fetchable_url(url)?;

        if !self.inner.policy.enabled {
            let bytes = fetch_bytes_from_network(url, max_bytes).await?;
            return Ok(CachedBytes {
                url: url.to_owned(),
                bytes,
                source: CacheSource::Disabled,
            });
        }

        if let Some((bytes, source)) = self.read(CacheBucket::Image, url) {
            if bytes.len() <= max_bytes {
                return Ok(CachedBytes {
                    url: url.to_owned(),
                    bytes,
                    source,
                });
            }

            warn!(url = %url, bytes = bytes.len(), max_bytes, "cached resource exceeded caller limit; refetching");
        }

        let bytes = fetch_bytes_from_network(url, max_bytes).await?;
        self.write(CacheBucket::Image, url, &bytes);

        Ok(CachedBytes {
            url: url.to_owned(),
            bytes,
            source: CacheSource::Network,
        })
    }

    fn read(&self, bucket: CacheBucket, url: &str) -> Option<(Vec<u8>, CacheSource)> {
        let key = cache_key(bucket, url);
        let ttl = self.inner.policy.ttl_for(bucket);
        let now = now_ms();

        if let Some(bytes) = self.read_memory(&key, bucket, ttl, now) {
            return Some((bytes, CacheSource::Memory));
        }

        match self.read_disk(bucket, url, ttl, now) {
            Ok(Some(bytes)) => {
                self.write_memory(key, bucket, now, bytes.clone());
                Some((bytes, CacheSource::Disk))
            }
            Ok(None) => None,
            Err(error) => {
                debug!(url = %url, error = %error, "disk cache read failed; treating as miss");
                None
            }
        }
    }

    fn read_memory(
        &self,
        key: &str,
        bucket: CacheBucket,
        ttl: Duration,
        now: u64,
    ) -> Option<Vec<u8>> {
        let mut memory = self.inner.memory.lock().ok()?;
        memory.get(key, bucket, ttl, now)
    }

    fn write(&self, bucket: CacheBucket, url: &str, body: &[u8]) {
        if body.len() > self.inner.policy.max_disk_entry_bytes {
            debug!(url = %url, bytes = body.len(), "resource too large for disk cache");
            return;
        }

        let now = now_ms();
        let key = cache_key(bucket, url);
        self.write_memory(key, bucket, now, body.to_vec());

        if let Err(error) = self.write_disk(bucket, url, body, now) {
            debug!(url = %url, error = %error, "disk cache write failed");
        }
    }

    fn write_memory(&self, key: String, bucket: CacheBucket, fetched_at_ms: u64, body: Vec<u8>) {
        if body.len() > self.inner.policy.memory_limit_bytes {
            return;
        }

        if let Ok(mut memory) = self.inner.memory.lock() {
            memory.put(
                key,
                MemoryEntry {
                    bucket,
                    fetched_at_ms,
                    body,
                },
                self.inner.policy.memory_limit_bytes,
            );
        }
    }

    fn read_disk(
        &self,
        bucket: CacheBucket,
        url: &str,
        ttl: Duration,
        now: u64,
    ) -> Result<Option<Vec<u8>>> {
        let paths = self.entry_paths(bucket, url);

        if !paths.body.exists() || !paths.meta.exists() {
            return Ok(None);
        }

        let meta_bytes = fs::read(&paths.meta)
            .with_context(|| format!("failed to read cache metadata `{}`", paths.meta.display()))?;
        let metadata: CacheMetadata = serde_json::from_slice(&meta_bytes).with_context(|| {
            format!("failed to parse cache metadata `{}`", paths.meta.display())
        })?;

        if metadata.version != CACHE_METADATA_VERSION
            || metadata.url != url
            || metadata.bucket != bucket.as_str()
        {
            return Ok(None);
        }

        if is_stale(metadata.fetched_at_ms, ttl, now) {
            debug!(url = %url, bucket = bucket.as_str(), "cache entry is stale");
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

        Ok(Some(body))
    }

    fn write_disk(
        &self,
        bucket: CacheBucket,
        url: &str,
        body: &[u8],
        fetched_at_ms: u64,
    ) -> Result<()> {
        let paths = self.entry_paths(bucket, url);

        if let Some(parent) = paths.body.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create cache directory `{}`", parent.display())
            })?;
        }

        let metadata = CacheMetadata {
            version: CACHE_METADATA_VERSION,
            url: url.to_owned(),
            bucket: bucket.as_str().to_owned(),
            fetched_at_ms,
            byte_len: usize_to_u64(body.len()),
        };

        let metadata_bytes =
            serde_json::to_vec_pretty(&metadata).context("failed to serialize cache metadata")?;
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

fn ensure_fetchable_url(url: &str) -> Result<()> {
    let parsed = Url::parse(url).with_context(|| format!("invalid cache URL `{url}`"))?;

    match parsed.scheme() {
        "http" | "https" => Ok(()),
        other => bail!("cache only supports http/https URLs, got `{other}`"),
    }
}

impl MemoryCache {
    fn get(&mut self, key: &str, bucket: CacheBucket, ttl: Duration, now: u64) -> Option<Vec<u8>> {
        let entry = self.entries.get(key)?;

        if entry.bucket != bucket || is_stale(entry.fetched_at_ms, ttl, now) {
            self.remove(key);
            return None;
        }

        let body = self.entries.get(key)?.body.clone();
        self.touch(key);
        Some(body)
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

async fn fetch_text_from_network(url: &str) -> Result<String> {
    fetch::init(Default::default());

    let response = fetch::get(url).await?;

    if !response.status().is_success() {
        bail!("document request returned status {}", response.status());
    }

    response.get_text().await
}

async fn fetch_bytes_from_network(url: &str, max_bytes: usize) -> Result<Vec<u8>> {
    fetch::init(Default::default());

    let mut response = fetch::get(url).await?;

    if !response.status().is_success() {
        bail!("resource request returned status {}", response.status());
    }

    let mut bytes = Vec::new();

    while let Some(chunk) = response.body_mut().next().await {
        let chunk = chunk?;
        bytes.extend_from_slice(&chunk);

        if bytes.len() > max_bytes {
            bail!("resource exceeds byte limit of {max_bytes} bytes");
        }
    }

    Ok(bytes)
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
            .join("Sylphos")
            .join("cache");
    }

    if let Some(xdg_cache_home) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(xdg_cache_home).join("sylphos");
    }

    if let Some(home) = env::var_os("HOME") {
        let home_path = PathBuf::from(home);

        if cfg!(target_os = "macos") {
            return home_path
                .join("Library")
                .join("Caches")
                .join("Kairais")
                .join("Sylphos");
        }

        return home_path.join(".cache").join("sylphos");
    }

    PathBuf::from(".sylphos-cache")
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
                if let Err(remove_error) = fs::remove_file(path) {
                    debug!(
                        path = %path.display(),
                        error = %remove_error,
                        "failed to remove old cache file before replace"
                    );
                }
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

fn ttl_ms(ttl: Duration) -> u64 {
    u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX)
}

fn is_stale(fetched_at_ms: u64, ttl: Duration, now: u64) -> bool {
    now.saturating_sub(fetched_at_ms) > ttl_ms(ttl)
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
    fn memory_cache_respects_ttl() {
        let mut cache = MemoryCache::default();
        cache.put(
            "html:key".to_owned(),
            MemoryEntry {
                bucket: CacheBucket::Html,
                fetched_at_ms: 100,
                body: b"hello".to_vec(),
            },
            1024,
        );

        let fresh = cache.get(
            "html:key",
            CacheBucket::Html,
            Duration::from_millis(200),
            250,
        );
        assert_eq!(fresh, Some(b"hello".to_vec()));

        let stale = cache.get(
            "html:key",
            CacheBucket::Html,
            Duration::from_millis(200),
            500,
        );
        assert!(stale.is_none());
    }

    #[test]
    fn memory_cache_evicts_to_limit() {
        let mut cache = MemoryCache::default();

        cache.put(
            "a".to_owned(),
            MemoryEntry {
                bucket: CacheBucket::Image,
                fetched_at_ms: 1,
                body: vec![1; 8],
            },
            16,
        );
        cache.put(
            "b".to_owned(),
            MemoryEntry {
                bucket: CacheBucket::Image,
                fetched_at_ms: 1,
                body: vec![2; 8],
            },
            16,
        );
        cache.put(
            "c".to_owned(),
            MemoryEntry {
                bucket: CacheBucket::Image,
                fetched_at_ms: 1,
                body: vec![3; 8],
            },
            16,
        );

        assert!(!cache.entries.contains_key("a"));
        assert!(cache.entries.contains_key("b"));
        assert!(cache.entries.contains_key("c"));
    }

    #[test]
    fn disabled_policy_is_disabled() {
        assert!(!CachePolicy::disabled().enabled);
    }
}
