//! Minimal browser-style navigation history and URL normalization.

use anyhow::{bail, Context, Result};
use url::Url;

/// In-process blank page URL used for new tabs.
pub(crate) const ABOUT_BLANK: &str = "about:blank";

/// Tracks current URL plus back/forward history.
#[derive(Debug, Clone)]
pub(crate) struct NavigationController {
    entries: Vec<String>,
    current_index: usize,
}

impl NavigationController {
    /// Creates a new history stack with one initial URL.
    pub(crate) fn new(initial_url: String) -> Self {
        Self {
            entries: vec![initial_url],
            current_index: 0,
        }
    }

    /// Returns the current URL.
    pub(crate) fn current_url(&self) -> &str {
        self.entries
            .get(self.current_index)
            .map_or(ABOUT_BLANK, String::as_str)
    }

    /// Returns whether back navigation is possible.
    pub(crate) const fn can_go_back(&self) -> bool {
        self.current_index > 0
    }

    /// Returns whether forward navigation is possible.
    pub(crate) fn can_go_forward(&self) -> bool {
        self.current_index + 1 < self.entries.len()
    }

    /// Navigates to a new URL and truncates forward history.
    pub(crate) fn navigate_to(&mut self, url: String) -> String {
        if self.current_url() == url {
            return url;
        }

        let next_index = self.current_index.saturating_add(1);
        self.entries.truncate(next_index);
        self.entries.push(url.clone());
        self.current_index = self.entries.len().saturating_sub(1);
        url
    }

    /// Moves backward and returns the target URL.
    pub(crate) fn go_back(&mut self) -> Option<String> {
        if !self.can_go_back() {
            return None;
        }

        self.current_index = self.current_index.saturating_sub(1);
        Some(self.current_url().to_owned())
    }

    /// Moves forward and returns the target URL.
    pub(crate) fn go_forward(&mut self) -> Option<String> {
        if !self.can_go_forward() {
            return None;
        }

        self.current_index = self.current_index.saturating_add(1);
        Some(self.current_url().to_owned())
    }

    /// Returns the current URL for reload.
    pub(crate) fn reload_url(&self) -> String {
        self.current_url().to_owned()
    }
}

/// Normalizes human-entered URL bar text into a fetchable URL.
///
/// `about:blank` is supported only as an internal new-tab page. Everything else
/// must normalize to HTTP or HTTPS for the fetch layer.
pub(crate) fn normalize_user_url(input: &str) -> Result<String> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        bail!("URL cannot be empty");
    }

    if trimmed.eq_ignore_ascii_case(ABOUT_BLANK) {
        return Ok(ABOUT_BLANK.to_owned());
    }

    if has_explicit_scheme(trimmed) {
        return ensure_supported_fetch_scheme(trimmed);
    }

    if trimmed.starts_with("localhost")
        || trimmed.starts_with("127.0.0.1")
        || trimmed.starts_with("[::1]")
    {
        return Ok(format!("http://{trimmed}"));
    }

    Ok(format!("https://{trimmed}"))
}

/// Resolves an HTML link target against the current page URL.
///
/// This supports absolute, root-relative, path-relative, query, and fragment
/// hrefs. Non-fetchable schemes such as `javascript:`, `mailto:`, and `tel:`
/// are rejected before they reach the fetcher.
pub(crate) fn resolve_link_url(base_url: &str, href: &str) -> Result<String> {
    let trimmed = href.trim();

    if trimmed.is_empty() {
        bail!("link target is empty");
    }

    if base_url.eq_ignore_ascii_case(ABOUT_BLANK) {
        bail!("cannot resolve page link from about:blank");
    }

    let base = Url::parse(base_url).with_context(|| format!("invalid base URL `{base_url}`"))?;
    let resolved = base
        .join(trimmed)
        .with_context(|| format!("failed to resolve link `{trimmed}` from `{base_url}`"))?;

    ensure_supported_url(&resolved)?;
    Ok(resolved.to_string())
}

fn ensure_supported_fetch_scheme(value: &str) -> Result<String> {
    let parsed = Url::parse(value).with_context(|| format!("invalid URL `{value}`"))?;
    ensure_supported_url(&parsed)?;
    Ok(parsed.to_string())
}

fn ensure_supported_url(url: &Url) -> Result<()> {
    match url.scheme() {
        "http" | "https" => Ok(()),
        other => bail!("unsupported link scheme `{other}`"),
    }
}

fn has_explicit_scheme(value: &str) -> bool {
    let Some(colon_index) = value.find(':') else {
        return false;
    };

    let scheme = &value[..colon_index];

    !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_plain_domain_to_https() {
        let normalized = normalize_user_url("example.com");
        match normalized {
            Ok(value) => assert_eq!(value, "https://example.com"),
            Err(error) => panic!("unexpected normalization error: {error}"),
        }
    }

    #[test]
    fn preserves_explicit_scheme() {
        let normalized = normalize_user_url("http://localhost:8080");
        match normalized {
            Ok(value) => assert_eq!(value, "http://localhost:8080/"),
            Err(error) => panic!("unexpected normalization error: {error}"),
        }
    }

    #[test]
    fn allows_about_blank_for_new_tabs() {
        let normalized = normalize_user_url("about:blank");
        match normalized {
            Ok(value) => assert_eq!(value, ABOUT_BLANK),
            Err(error) => panic!("unexpected about:blank normalization error: {error}"),
        }
    }

    #[test]
    fn rejects_javascript_urls() {
        assert!(normalize_user_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn navigation_history_tracks_back_forward() {
        let mut nav = NavigationController::new("https://a.test".to_owned());
        assert_eq!(nav.current_url(), "https://a.test");
        nav.navigate_to("https://b.test".to_owned());
        nav.navigate_to("https://c.test".to_owned());

        assert_eq!(nav.go_back().as_deref(), Some("https://b.test"));
        assert_eq!(nav.go_forward().as_deref(), Some("https://c.test"));
    }

    #[test]
    fn resolves_root_relative_links() {
        let resolved = resolve_link_url("https://example.com/docs/page.html", "/domains/example");
        match resolved {
            Ok(value) => assert_eq!(value, "https://example.com/domains/example"),
            Err(error) => panic!("unexpected link resolution error: {error}"),
        }
    }

    #[test]
    fn resolves_path_relative_links() {
        let resolved = resolve_link_url("https://example.com/docs/page.html", "next.html");
        match resolved {
            Ok(value) => assert_eq!(value, "https://example.com/docs/next.html"),
            Err(error) => panic!("unexpected link resolution error: {error}"),
        }
    }

    #[test]
    fn rejects_non_fetchable_link_schemes() {
        assert!(resolve_link_url("https://example.com", "mailto:test@example.com").is_err());
        assert!(resolve_link_url("https://example.com", "javascript:alert(1)").is_err());
    }
}
