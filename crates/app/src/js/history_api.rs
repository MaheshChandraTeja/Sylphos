#![allow(dead_code)]

//! History API host state.
//!
//! This module models script-visible `history.pushState` and `replaceState` as
//! same-document URL mutations. It does not trigger network navigation by itself;
//! the browser shell can later decide when these entries should surface as full
//! navigations.

use anyhow::{Context, Result};
use url::Url;

const MAX_HISTORY_STATE_BYTES: usize = 64 * 1024;
const MAX_HISTORY_ENTRIES: usize = 256;

/// One script-created history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryApiEntry {
    pub url: String,
    pub title: Option<String>,
    pub state_json: Option<String>,
}

/// Per-document History API state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryApiState {
    entries: Vec<HistoryApiEntry>,
    current: usize,
}

impl HistoryApiState {
    /// Creates initial state for a document URL.
    pub(crate) fn new(document_url: &str) -> Self {
        Self {
            entries: vec![HistoryApiEntry {
                url: document_url.to_owned(),
                title: None,
                state_json: None,
            }],
            current: 0,
        }
    }

    /// Pushes a same-document history entry.
    pub(crate) fn push_state(
        &mut self,
        base_url: &str,
        state_json: Option<String>,
        title: Option<String>,
        url: Option<String>,
    ) -> Result<String> {
        let resolved = resolve_history_url(base_url, url.as_deref())?;
        let state_json = sanitize_state(state_json)?;

        self.entries.truncate(self.current.saturating_add(1));
        self.entries.push(HistoryApiEntry {
            url: resolved.clone(),
            title,
            state_json,
        });
        if self.entries.len() > MAX_HISTORY_ENTRIES {
            self.entries.remove(0);
        }
        self.current = self.entries.len().saturating_sub(1);
        Ok(resolved)
    }

    /// Replaces the current same-document history entry.
    pub(crate) fn replace_state(
        &mut self,
        base_url: &str,
        state_json: Option<String>,
        title: Option<String>,
        url: Option<String>,
    ) -> Result<String> {
        let resolved = resolve_history_url(base_url, url.as_deref())?;
        let state_json = sanitize_state(state_json)?;

        if let Some(entry) = self.entries.get_mut(self.current) {
            *entry = HistoryApiEntry {
                url: resolved.clone(),
                title,
                state_json,
            };
        }
        Ok(resolved)
    }

    /// Returns the current URL.
    #[must_use]
    pub(crate) fn current_url(&self) -> &str {
        self.entries
            .get(self.current)
            .map_or("about:blank", |entry| entry.url.as_str())
    }

    /// Returns entry count.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

fn resolve_history_url(base_url: &str, candidate: Option<&str>) -> Result<String> {
    let base = Url::parse(base_url).with_context(|| format!("invalid base URL `{base_url}`"))?;
    let resolved = candidate.map_or_else(|| Ok(base.clone()), |value| base.join(value))?;

    if resolved.origin() != base.origin() {
        anyhow::bail!("history API URL must remain same-origin: `{resolved}`");
    }

    Ok(resolved.to_string())
}

fn sanitize_state(state_json: Option<String>) -> Result<Option<String>> {
    let Some(value) = state_json else {
        return Ok(None);
    };
    if value.len() > MAX_HISTORY_STATE_BYTES {
        anyhow::bail!("history state exceeds {MAX_HISTORY_STATE_BYTES} bytes");
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_state_resolves_relative_url() {
        let mut history = HistoryApiState::new("https://example.com/a/b");
        let result = history.push_state(
            "https://example.com/a/b",
            None,
            None,
            Some("/next".to_owned()),
        );
        match result {
            Ok(value) => assert_eq!(value, "https://example.com/next"),
            Err(error) => panic!("pushState should resolve relative URL: {error}"),
        }
        assert_eq!(history.current_url(), "https://example.com/next");
    }

    #[test]
    fn push_state_blocks_cross_origin_url() {
        let mut history = HistoryApiState::new("https://example.com/a/b");
        assert!(history
            .push_state(
                "https://example.com/a/b",
                None,
                None,
                Some("https://evil.example/".to_owned()),
            )
            .is_err());
    }
}
