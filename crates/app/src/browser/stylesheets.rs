//! External stylesheet discovery, loading, and CSS cascade-lite integration.

use anyhow::{Context, Result};
use present::{
    parse_css_lite, parse_css_rules_lite, RenderDocument, StyleRuleLite, StyleSheetLite,
    StyleSourceLite, StylesheetLink,
};
use std::collections::HashSet;
use tracing::{debug, warn};
use url::Url;

use crate::browser::{CacheSource, CacheStore, ResourceRequest, ResourceScheduler};

const MAX_STYLESHEET_BYTES: usize = 1024 * 1024;
const MAX_STYLESHEETS_PER_PAGE: usize = 24;
const MAX_IMPORT_DEPTH: usize = 3;
const CSS_RULE_ORDER_STRIDE: usize = 10_000;

/// Summary of external stylesheet loading for diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StylesheetLoadSummary {
    pub discovered: usize,
    pub loaded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub imported: usize,
    pub bytes: usize,
    pub rule_count: usize,
    pub memory_hits: usize,
    pub disk_hits: usize,
    pub network_fetches: usize,
    pub disabled_fetches: usize,
}

#[derive(Debug, Clone, Default)]
struct LoadedStyleSheetTree {
    sheet: StyleSheetLite,
    rules: Vec<StyleRuleLite>,
}

impl LoadedStyleSheetTree {
    fn merge_from(&mut self, other: Self) {
        self.sheet.merge_from(other.sheet);
        self.rules.extend(other.rules);
    }
}

/// Loads external stylesheets referenced by a render document and replaces its computed styles.
///
/// Inline and external sources are applied in document order. `@import` rules inside fetched CSS
/// are loaded before the containing stylesheet, matching the useful part of CSS cascade behavior
/// without pretending to be a standards-complete browser engine. A bold refusal to become Chrome.
#[allow(dead_code)]
pub(crate) async fn load_and_apply_stylesheets(
    base_url: &str,
    document: &mut RenderDocument,
    cache: &CacheStore,
) -> StylesheetLoadSummary {
    let scheduler = ResourceScheduler::new(cache.clone());
    load_and_apply_stylesheets_with_scheduler(base_url, document, &scheduler).await
}

/// Loads external stylesheets using the shared resource scheduler.
pub(crate) async fn load_and_apply_stylesheets_with_scheduler(
    base_url: &str,
    document: &mut RenderDocument,
    scheduler: &ResourceScheduler,
) -> StylesheetLoadSummary {
    let sources = document.style_sources.clone();
    if sources.is_empty() {
        document.recompute_style_tree();
        return StylesheetLoadSummary::default();
    }

    let mut summary = StylesheetLoadSummary {
        discovered: document.external_stylesheets.len(),
        ..StylesheetLoadSummary::default()
    };
    let mut visited = HashSet::new();
    let mut loaded = LoadedStyleSheetTree::default();
    let mut external_seen = 0usize;

    for source in sources {
        match source {
            StyleSourceLite::Inline { css, source_order } => {
                loaded.sheet.merge_from(parse_css_lite(&css));
                loaded.rules.extend(parse_css_rules_lite(
                    &css,
                    source_order.saturating_mul(CSS_RULE_ORDER_STRIDE),
                ));
            }
            StyleSourceLite::External(link) => {
                if external_seen >= MAX_STYLESHEETS_PER_PAGE {
                    summary.skipped = summary.skipped.saturating_add(1);
                    continue;
                }
                external_seen = external_seen.saturating_add(1);

                match load_stylesheet_link(base_url, &link, scheduler, &mut visited, &mut summary)
                    .await
                {
                    Ok(loaded_sheet) => loaded.merge_from(loaded_sheet),
                    Err(error) => {
                        summary.failed = summary.failed.saturating_add(1);
                        warn!(href = %link.href, error = %error, "failed to load external stylesheet");
                    }
                }
            }
        }
    }

    loaded.rules.sort_by_key(|rule| rule.source_order);
    summary.rule_count = loaded.rules.len();
    document.set_style_sheet_and_rules(loaded.sheet, loaded.rules);
    summary
}

async fn load_stylesheet_link(
    base_url: &str,
    link: &StylesheetLink,
    scheduler: &ResourceScheduler,
    visited: &mut HashSet<String>,
    summary: &mut StylesheetLoadSummary,
) -> Result<LoadedStyleSheetTree> {
    if !link.applies_to_screen() {
        summary.skipped = summary.skipped.saturating_add(1);
        return Ok(LoadedStyleSheetTree::default());
    }

    let url = resolve_stylesheet_url(base_url, &link.href)
        .with_context(|| format!("failed to resolve stylesheet `{}`", link.href))?;

    load_stylesheet_tree(
        &url,
        scheduler,
        visited,
        summary,
        0,
        link.source_order.saturating_mul(CSS_RULE_ORDER_STRIDE),
    )
    .await
}

fn load_stylesheet_tree<'a>(
    url: &'a str,
    scheduler: &'a ResourceScheduler,
    visited: &'a mut HashSet<String>,
    summary: &'a mut StylesheetLoadSummary,
    depth: usize,
    source_order_base: usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<LoadedStyleSheetTree>> + Send + 'a>>
{
    Box::pin(async move {
        if depth > MAX_IMPORT_DEPTH {
            summary.skipped = summary.skipped.saturating_add(1);
            return Ok(LoadedStyleSheetTree::default());
        }

        if !visited.insert(url.to_owned()) {
            summary.skipped = summary.skipped.saturating_add(1);
            return Ok(LoadedStyleSheetTree::default());
        }

        let cached = scheduler
            .fetch_text(ResourceRequest::stylesheet(url.to_owned()).max_bytes(MAX_STYLESHEET_BYTES))
            .await?;
        summary.bytes = summary.bytes.saturating_add(cached.bytes);
        record_cache_source(cached.source, summary);

        if cached.text.len() > MAX_STYLESHEET_BYTES {
            summary.skipped = summary.skipped.saturating_add(1);
            warn!(url = %url, bytes = cached.text.len(), "stylesheet exceeded CSS-lite byte limit");
            return Ok(LoadedStyleSheetTree::default());
        }

        let mut tree = LoadedStyleSheetTree::default();

        for (index, import_href) in extract_css_import_urls(&cached.text)
            .into_iter()
            .enumerate()
        {
            match resolve_stylesheet_url(url, &import_href) {
                Ok(import_url) => match load_stylesheet_tree(
                    &import_url,
                    scheduler,
                    visited,
                    summary,
                    depth.saturating_add(1),
                    source_order_base.saturating_add(index.saturating_add(1).saturating_mul(100)),
                )
                .await
                {
                    Ok(import_sheet) => {
                        summary.imported = summary.imported.saturating_add(1);
                        tree.merge_from(import_sheet);
                    }
                    Err(error) => {
                        summary.failed = summary.failed.saturating_add(1);
                        warn!(href = %import_href, error = %error, "failed to load imported stylesheet");
                    }
                },
                Err(error) => {
                    summary.failed = summary.failed.saturating_add(1);
                    warn!(href = %import_href, error = %error, "failed to resolve imported stylesheet");
                }
            }
        }

        tree.sheet.merge_from(parse_css_lite(&cached.text));
        tree.rules
            .extend(parse_css_rules_lite(&cached.text, source_order_base));
        summary.loaded = summary.loaded.saturating_add(1);
        debug!(url = %url, cache_source = cached.source.as_str(), rules = tree.rules.len(), "loaded stylesheet");
        Ok(tree)
    })
}

fn resolve_stylesheet_url(base_url: &str, href: &str) -> Result<String> {
    let base = Url::parse(base_url).with_context(|| format!("invalid base URL `{base_url}`"))?;
    let resolved = base
        .join(href.trim())
        .with_context(|| format!("invalid stylesheet href `{href}`"))?;

    match resolved.scheme() {
        "http" | "https" => Ok(resolved.to_string()),
        scheme => anyhow::bail!("unsupported stylesheet URL scheme `{scheme}`"),
    }
}

fn record_cache_source(source: CacheSource, summary: &mut StylesheetLoadSummary) {
    match source {
        CacheSource::Disabled => {
            summary.disabled_fetches = summary.disabled_fetches.saturating_add(1)
        }
        CacheSource::Network => summary.network_fetches = summary.network_fetches.saturating_add(1),
        CacheSource::Memory => summary.memory_hits = summary.memory_hits.saturating_add(1),
        CacheSource::Disk => summary.disk_hits = summary.disk_hits.saturating_add(1),
    }
}

fn extract_css_import_urls(css: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = css;

    while let Some(index) = find_case_insensitive(rest, "@import") {
        rest = &rest[index + "@import".len()..];
        let Some(end) = rest.find(';') else {
            break;
        };

        let rule = rest[..end].trim();
        if let Some(url) = parse_import_rule_url(rule) {
            urls.push(url);
        }
        rest = &rest[end + 1..];
    }

    urls
}

fn parse_import_rule_url(rule: &str) -> Option<String> {
    let trimmed = rule.trim();

    if let Some(after_url) = trimmed.strip_prefix("url(") {
        let close = after_url.find(')')?;
        return clean_css_url_token(&after_url[..close]);
    }

    if let Some(after_url) = trimmed.strip_prefix("URL(") {
        let close = after_url.find(')')?;
        return clean_css_url_token(&after_url[..close]);
    }

    let first = trimmed.split_whitespace().next()?;
    clean_css_url_token(first)
}

fn clean_css_url_token(token: &str) -> Option<String> {
    let cleaned = token.trim().trim_matches('"').trim_matches('\'').trim();

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_owned())
    }
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let hay = haystack.as_bytes();
    let needle = needle.as_bytes();

    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }

    hay.windows(needle.len()).position(|window| {
        window
            .iter()
            .zip(needle.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_import_urls() {
        let css = r#"
            @import url("base.css");
            @import 'theme.css';
            body { color: #111; }
        "#;

        let imports = extract_css_import_urls(css);
        assert_eq!(imports, vec!["base.css".to_owned(), "theme.css".to_owned()]);
    }
}
