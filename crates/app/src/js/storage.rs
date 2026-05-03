#![allow(dead_code)]

//! Web Storage API host state.
//!
//! This is a conservative host-side implementation for `localStorage` and
//! `sessionStorage` effects discovered by the intrinsic script executor. Local
//! storage is persisted per origin; session storage is scoped to the current
//! document runtime and intentionally not written to disk.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tracing::debug;
use url::Url;

const STORAGE_VERSION: u32 = 1;
const MAX_KEY_LEN: usize = 512;
const MAX_VALUE_LEN: usize = 256 * 1024;
const MAX_ITEMS_PER_ORIGIN: usize = 1024;

/// Storage area selected by script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StorageAreaKind {
    /// Persistent origin storage.
    Local,

    /// Runtime-only origin storage.
    Session,
}

impl StorageAreaKind {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "localStorage",
            Self::Session => "sessionStorage",
        }
    }
}

/// Host-side Web Storage manager for one document origin.
#[derive(Debug, Clone)]
pub(crate) struct WebStorage {
    origin: String,
    local_path: PathBuf,
    local: BTreeMap<String, String>,
    session: BTreeMap<String, String>,
    dirty_local: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredOriginStorage {
    version: u32,
    origin: String,
    values: BTreeMap<String, String>,
}

impl WebStorage {
    /// Creates storage for a document URL under the supplied root directory.
    pub(crate) fn load(root: impl AsRef<Path>, document_url: &str) -> Self {
        let origin = origin_key(document_url);
        let local_path = root
            .as_ref()
            .join("storage")
            .join(format!("{}.json", safe_file_component(&origin)));

        let local = read_local_storage(&local_path, &origin).unwrap_or_default();

        Self {
            origin,
            local_path,
            local,
            session: BTreeMap::new(),
            dirty_local: false,
        }
    }

    /// Returns the serialized origin key.
    #[must_use]
    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    /// Sets an item in a storage area.
    pub(crate) fn set_item(
        &mut self,
        area: StorageAreaKind,
        key: &str,
        value: &str,
    ) -> Result<bool> {
        let key = sanitize_key(key)?;
        let value = sanitize_value(value)?;
        let origin = self.origin.clone();
        let target = self.area_mut(area);

        if !target.contains_key(&key) && target.len() >= MAX_ITEMS_PER_ORIGIN {
            anyhow::bail!("{} quota exceeded for origin `{}`", area.as_str(), origin);
        }

        let changed = target.get(&key) != Some(&value);
        target.insert(key, value);

        if area == StorageAreaKind::Local && changed {
            self.dirty_local = true;
        }

        Ok(changed)
    }

    /// Removes an item from a storage area.
    pub(crate) fn remove_item(&mut self, area: StorageAreaKind, key: &str) -> bool {
        let target = self.area_mut(area);
        let removed = target.remove(key).is_some();

        if area == StorageAreaKind::Local && removed {
            self.dirty_local = true;
        }

        removed
    }

    /// Clears a storage area.
    pub(crate) fn clear(&mut self, area: StorageAreaKind) -> bool {
        let target = self.area_mut(area);
        let changed = !target.is_empty();
        target.clear();

        if area == StorageAreaKind::Local && changed {
            self.dirty_local = true;
        }

        changed
    }

    /// Gets an item from a storage area.
    #[must_use]
    pub(crate) fn get_item(&self, area: StorageAreaKind, key: &str) -> Option<&str> {
        self.area(area).get(key).map(String::as_str)
    }

    /// Persists local storage when needed.
    pub(crate) fn flush(&mut self) -> Result<()> {
        if !self.dirty_local {
            return Ok(());
        }

        if let Some(parent) = self.local_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create storage directory `{}`", parent.display())
            })?;
        }

        let stored = StoredOriginStorage {
            version: STORAGE_VERSION,
            origin: self.origin.clone(),
            values: self.local.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&stored).context("failed to serialize storage")?;
        fs::write(&self.local_path, bytes).with_context(|| {
            format!(
                "failed to write storage file `{}`",
                self.local_path.display()
            )
        })?;
        self.dirty_local = false;
        debug!(origin = %self.origin, path = %self.local_path.display(), "flushed localStorage");
        Ok(())
    }

    fn area(&self, area: StorageAreaKind) -> &BTreeMap<String, String> {
        match area {
            StorageAreaKind::Local => &self.local,
            StorageAreaKind::Session => &self.session,
        }
    }

    fn area_mut(&mut self, area: StorageAreaKind) -> &mut BTreeMap<String, String> {
        match area {
            StorageAreaKind::Local => &mut self.local,
            StorageAreaKind::Session => &mut self.session,
        }
    }
}

fn read_local_storage(path: &Path, expected_origin: &str) -> Result<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let bytes = fs::read(path)
        .with_context(|| format!("failed to read storage file `{}`", path.display()))?;
    let stored: StoredOriginStorage = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse storage file `{}`", path.display()))?;

    if stored.version != STORAGE_VERSION || stored.origin != expected_origin {
        return Ok(BTreeMap::new());
    }

    Ok(stored.values)
}

fn sanitize_key(key: &str) -> Result<String> {
    if key.len() > MAX_KEY_LEN {
        anyhow::bail!("storage key exceeds {MAX_KEY_LEN} bytes");
    }
    Ok(key.to_owned())
}

fn sanitize_value(value: &str) -> Result<String> {
    if value.len() > MAX_VALUE_LEN {
        anyhow::bail!("storage value exceeds {MAX_VALUE_LEN} bytes");
    }
    Ok(value.to_owned())
}

pub(crate) fn origin_key(document_url: &str) -> String {
    let Ok(url) = Url::parse(document_url) else {
        return "opaque".to_owned();
    };

    let host = url.host_str().unwrap_or("opaque");
    let port = url.port().map_or(String::new(), |port| format!(":{port}"));
    format!("{}://{}{}", url.scheme(), host.to_ascii_lowercase(), port)
}

fn safe_file_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!("sylphos-storage-test-{stamp}"))
    }

    #[test]
    fn local_storage_persists_per_origin() {
        let root = temp_root();
        let mut storage = WebStorage::load(&root, "https://example.com/page");
        assert!(storage
            .set_item(StorageAreaKind::Local, "answer", "42")
            .is_ok());
        assert!(storage.flush().is_ok());

        let loaded = WebStorage::load(&root, "https://example.com/other");
        assert_eq!(
            loaded.get_item(StorageAreaKind::Local, "answer"),
            Some("42")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_storage_does_not_persist() {
        let root = temp_root();
        let mut storage = WebStorage::load(&root, "https://example.com/page");
        assert!(storage
            .set_item(StorageAreaKind::Session, "answer", "42")
            .is_ok());
        assert!(storage.flush().is_ok());

        let loaded = WebStorage::load(&root, "https://example.com/page");
        assert_eq!(loaded.get_item(StorageAreaKind::Session, "answer"), None);

        let _ = fs::remove_dir_all(root);
    }
}
