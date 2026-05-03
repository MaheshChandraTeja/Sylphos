#![allow(dead_code)]

//! Minimal cookie jar for script-visible `document.cookie` support.
//!
//! This is not a complete RFC6265 implementation. It intentionally implements a
//! safe, deterministic subset: script-set name/value cookies, domain/path scoping,
//! max-age deletion, secure flag tracking, and persistent storage.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

const COOKIE_FILE_VERSION: u32 = 1;
const MAX_COOKIES: usize = 512;
const MAX_COOKIE_NAME_LEN: usize = 256;
const MAX_COOKIE_VALUE_LEN: usize = 4096;

/// One persisted cookie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CookieRecord {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub expires_at_ms: Option<u64>,
}

/// Script cookie jar.
#[derive(Debug, Clone)]
pub(crate) struct CookieJar {
    path: PathBuf,
    cookies: Vec<CookieRecord>,
    dirty: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredCookies {
    version: u32,
    cookies: Vec<CookieRecord>,
}

impl CookieJar {
    /// Loads a cookie jar from disk.
    pub(crate) fn load(root: impl AsRef<Path>) -> Self {
        let path = root.as_ref().join("cookies").join("cookies.json");
        let cookies = read_cookies(&path).unwrap_or_default();
        Self {
            path,
            cookies,
            dirty: false,
        }
    }

    /// Applies a script assignment such as `document.cookie = "a=b; path=/"`.
    pub(crate) fn set_from_script(&mut self, document_url: &str, assignment: &str) -> Result<bool> {
        let url = Url::parse(document_url).with_context(|| {
            format!("invalid document URL for cookie assignment `{document_url}`")
        })?;
        let Some(mut cookie) = parse_cookie_assignment(&url, assignment)? else {
            return Ok(false);
        };

        if cookie.name.len() > MAX_COOKIE_NAME_LEN || cookie.value.len() > MAX_COOKIE_VALUE_LEN {
            anyhow::bail!("cookie exceeds Sylphos cookie size limits");
        }

        if cookie
            .expires_at_ms
            .is_some_and(|expires| expires <= now_ms())
        {
            let before = self.cookies.len();
            self.cookies
                .retain(|existing| !same_cookie(existing, &cookie));
            let changed = before != self.cookies.len();
            self.dirty |= changed;
            return Ok(changed);
        }

        normalize_cookie_domain(&url, &mut cookie)?;
        self.cookies
            .retain(|existing| !same_cookie(existing, &cookie));

        if self.cookies.len() >= MAX_COOKIES {
            self.cookies.remove(0);
        }

        self.cookies.push(cookie);
        self.dirty = true;
        Ok(true)
    }

    /// Returns the script-visible `document.cookie` string for a URL.
    #[must_use]
    pub(crate) fn document_cookie(&self, document_url: &str) -> String {
        let Ok(url) = Url::parse(document_url) else {
            return String::new();
        };

        self.cookies
            .iter()
            .filter(|cookie| cookie_matches_url(cookie, &url))
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Flushes changed cookies to disk.
    pub(crate) fn flush(&mut self) -> Result<()> {
        self.remove_expired();

        if !self.dirty {
            return Ok(());
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create cookie directory `{}`", parent.display())
            })?;
        }

        let stored = StoredCookies {
            version: COOKIE_FILE_VERSION,
            cookies: self.cookies.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&stored).context("failed to serialize cookies")?;
        fs::write(&self.path, bytes)
            .with_context(|| format!("failed to write cookies `{}`", self.path.display()))?;
        self.dirty = false;
        Ok(())
    }

    fn remove_expired(&mut self) {
        let now = now_ms();
        let before = self.cookies.len();
        self.cookies
            .retain(|cookie| cookie.expires_at_ms.map_or(true, |expires| expires > now));
        self.dirty |= before != self.cookies.len();
    }
}

fn read_cookies(path: &Path) -> Result<Vec<CookieRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes =
        fs::read(path).with_context(|| format!("failed to read cookies `{}`", path.display()))?;
    let stored: StoredCookies = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse cookies `{}`", path.display()))?;

    if stored.version != COOKIE_FILE_VERSION {
        return Ok(Vec::new());
    }

    Ok(stored.cookies)
}

fn parse_cookie_assignment(url: &Url, assignment: &str) -> Result<Option<CookieRecord>> {
    let mut parts = assignment.split(';').map(str::trim);
    let Some(pair) = parts.next() else {
        return Ok(None);
    };
    let Some((name, value)) = pair.split_once('=') else {
        return Ok(None);
    };
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }

    let mut cookie = CookieRecord {
        name: name.to_owned(),
        value: value.trim().to_owned(),
        domain: url.host_str().unwrap_or_default().to_ascii_lowercase(),
        path: default_cookie_path(url),
        secure: false,
        expires_at_ms: None,
    };

    for attr in parts {
        let (key, value) = attr
            .split_once('=')
            .map_or((attr, ""), |(key, value)| (key, value));
        match key.trim().to_ascii_lowercase().as_str() {
            "domain" => cookie.domain = value.trim().trim_start_matches('.').to_ascii_lowercase(),
            "path" => cookie.path = normalize_path(value.trim()),
            "secure" => cookie.secure = true,
            "max-age" => {
                if let Ok(seconds) = value.trim().parse::<i64>() {
                    cookie.expires_at_ms = if seconds <= 0 {
                        Some(0)
                    } else {
                        let ms = u64::try_from(seconds)
                            .unwrap_or(u64::MAX)
                            .saturating_mul(1000);
                        Some(now_ms().saturating_add(ms))
                    };
                }
            }
            // `expires` parsing is intentionally deferred; max-age is enough for deletion
            // and common script tests without hauling in date parsing dependency drama.
            "expires" | "samesite" | "httponly" => {}
            _ => {}
        }
    }

    Ok(Some(cookie))
}

fn normalize_cookie_domain(url: &Url, cookie: &mut CookieRecord) -> Result<()> {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if cookie.domain.is_empty() {
        cookie.domain = host;
        return Ok(());
    }

    if host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain)) {
        return Ok(());
    }

    anyhow::bail!(
        "cookie domain `{}` does not match host `{host}`",
        cookie.domain
    )
}

fn cookie_matches_url(cookie: &CookieRecord, url: &Url) -> bool {
    if cookie
        .expires_at_ms
        .is_some_and(|expires| expires <= now_ms())
    {
        return false;
    }
    if cookie.secure && url.scheme() != "https" {
        return false;
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let domain_matches = host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain));
    let path_matches = url.path().starts_with(&cookie.path);
    domain_matches && path_matches
}

fn same_cookie(left: &CookieRecord, right: &CookieRecord) -> bool {
    left.name == right.name && left.domain == right.domain && left.path == right.path
}

fn default_cookie_path(url: &Url) -> String {
    let path = url.path();
    if path.is_empty() || path == "/" {
        return "/".to_owned();
    }
    match path.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(index) => path[..index].to_owned(),
    }
}

fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        "/".to_owned()
    }
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!("sylphos-cookie-test-{stamp}"))
    }

    #[test]
    fn cookie_round_trip_for_origin() {
        let root = temp_root();
        let mut jar = CookieJar::load(&root);
        assert!(jar
            .set_from_script("https://example.com/path/page", "a=b; path=/")
            .is_ok());
        assert!(jar.flush().is_ok());

        let loaded = CookieJar::load(&root);
        assert_eq!(loaded.document_cookie("https://example.com/other"), "a=b");
        assert_eq!(loaded.document_cookie("https://example.org/other"), "");

        let _ = fs::remove_dir_all(root);
    }
}
