//! Persistent visit history for Sylphos.
//!
//! This is intentionally conservative: it records successful top-level page
//! visits only. It does not store response bodies, form state, cookies, or other
//! little privacy landmines humans keep inventing and then acting surprised by.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::warn;

const HISTORY_VERSION: u32 = 1;
const DEFAULT_MAX_HISTORY_ENTRIES: usize = 1_000;
const TEMP_FILE_SUFFIX: &str = ".tmp";

/// One successful top-level page visit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HistoryEntry {
    pub url: String,
    pub title: Option<String>,
    pub visited_at_ms: u64,
}

/// Persistent browser history store.
#[derive(Debug, Clone)]
pub(crate) struct BrowserHistory {
    path: PathBuf,
    max_entries: usize,
    entries: VecDeque<HistoryEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HistoryFile {
    version: u32,
    entries: Vec<HistoryEntry>,
}

impl BrowserHistory {
    /// Loads a history file or creates an empty store when no file exists.
    pub(crate) fn load(path: PathBuf) -> Self {
        match Self::load_from_disk(&path) {
            Ok(entries) => Self {
                path,
                max_entries: DEFAULT_MAX_HISTORY_ENTRIES,
                entries,
            },
            Err(error) => {
                warn!(error = %error, path = %path.display(), "history file could not be loaded; starting empty history");
                Self {
                    path,
                    max_entries: DEFAULT_MAX_HISTORY_ENTRIES,
                    entries: VecDeque::new(),
                }
            }
        }
    }

    /// Records a successful visit and persists the history file.
    pub(crate) fn record_visit(&mut self, url: &str, title: Option<&str>) {
        if url.eq_ignore_ascii_case("about:blank") {
            return;
        }

        if self
            .entries
            .back()
            .is_some_and(|entry| entry.url == url && entry.title.as_deref() == title)
        {
            return;
        }

        self.entries.push_back(HistoryEntry {
            url: url.to_owned(),
            title: title.map(ToOwned::to_owned),
            visited_at_ms: now_ms(),
        });

        while self.entries.len() > self.max_entries {
            let _ = self.entries.pop_front();
        }

        if let Err(error) = self.save() {
            warn!(error = %error, path = %self.path.display(), "failed to persist browser history");
        }
    }

    fn load_from_disk(path: &Path) -> Result<VecDeque<HistoryEntry>> {
        if !path.exists() {
            return Ok(VecDeque::new());
        }

        let bytes =
            fs::read(path).with_context(|| format!("failed to read `{}`", path.display()))?;
        let file = serde_json::from_slice::<HistoryFile>(&bytes)
            .with_context(|| format!("failed to parse `{}`", path.display()))?;

        if file.version != HISTORY_VERSION {
            return Ok(VecDeque::new());
        }

        Ok(file.entries.into())
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }

        let file = HistoryFile {
            version: HISTORY_VERSION,
            entries: self.entries.iter().cloned().collect(),
        };

        let bytes = serde_json::to_vec_pretty(&file).context("failed to serialize history")?;
        let temp_path = self
            .path
            .with_extension(TEMP_FILE_SUFFIX.trim_start_matches('.'));
        fs::write(&temp_path, bytes)
            .with_context(|| format!("failed to write `{}`", temp_path.display()))?;
        fs::rename(&temp_path, &self.path).with_context(|| {
            format!(
                "failed to atomically move `{}` to `{}`",
                temp_path.display(),
                self.path.display()
            )
        })?;

        Ok(())
    }
}

fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_successful_visits() {
        let path = std::env::temp_dir().join(format!("sylphos-history-test-{}.json", now_ms()));
        let mut history = BrowserHistory::load(path.clone());

        history.record_visit("https://example.com/", Some("Example Domain"));
        history.record_visit("about:blank", None);
        history.record_visit("https://iana.org/", Some("IANA"));

        let reloaded = BrowserHistory::load(path.clone());
        assert_eq!(reloaded.entries.len(), 2);

        let _ = fs::remove_file(path);
    }
}
