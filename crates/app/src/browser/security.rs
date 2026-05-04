#![allow(dead_code, clippy::arc_with_non_send_sync)]

//! Browser-facing origin security policy bridge.
//!
//! This wraps `syljs::security` for native resource loading. The goal is to keep
//! origin, CSP, CORS-lite, mixed-content, sandbox, and service-worker policy in
//! one place instead of letting every loader invent its own little security
//! monarchy. That way lies bugs, and bugs already have enough land.

use crate::browser::{ResourceKind, ResourceRequest};
use anyhow::{bail, Result};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use syljs::{
    CredentialsMode, MixedContentPolicy, RequestDestination, RequestMode, SecurityContext,
    SecurityDecision, SecurityDecisionKind, SecurityEnforcer, SecurityMetrics, SecurityRequest,
    SecurityViolation,
};
use tracing::{debug, warn};

/// App-wide security configuration for one top-level document.
#[derive(Debug, Clone)]
pub(crate) struct BrowserSecurityConfig {
    pub document_url: String,
    pub csp_header: Option<String>,
    pub sandbox_tokens: Option<String>,
    pub mixed_content: MixedContentPolicy,
}

impl BrowserSecurityConfig {
    /// Creates a config for a top-level document.
    #[must_use]
    pub(crate) fn new(document_url: impl Into<String>) -> Self {
        Self {
            document_url: document_url.into(),
            csp_header: None,
            sandbox_tokens: None,
            mixed_content: MixedContentPolicy::default(),
        }
    }

    /// Adds CSP text.
    #[must_use]
    pub(crate) fn csp(mut self, value: Option<String>) -> Self {
        self.csp_header = value;
        self
    }

    /// Adds sandbox tokens.
    #[must_use]
    pub(crate) fn sandbox(mut self, value: Option<String>) -> Self {
        self.sandbox_tokens = value;
        self
    }

    /// Converts to SylJS security context.
    #[must_use]
    pub(crate) fn into_context(self) -> SecurityContext {
        let mut context = SecurityContext::new(self.document_url);
        if let Some(csp) = self.csp_header {
            context = context.with_csp(csp);
        }
        if let Some(sandbox) = self.sandbox_tokens {
            context = context.with_sandbox_tokens(sandbox);
        }
        context.mixed_content = self.mixed_content;
        context
    }
}

/// Shared resource security guard.
#[derive(Debug, Clone)]
pub(crate) struct ResourceSecurityGuard {
    enforcer: Arc<SecurityEnforcer>,
    summary: Arc<Mutex<AppSecuritySummary>>,
}

impl ResourceSecurityGuard {
    /// Creates a guard from config.
    #[must_use]
    pub(crate) fn new(config: BrowserSecurityConfig) -> Self {
        Self {
            enforcer: Arc::new(SecurityEnforcer::new(config.into_context())),
            summary: Arc::new(Mutex::new(AppSecuritySummary::default())),
        }
    }

    /// Creates a permissive guard for about:blank or tests.
    #[must_use]
    pub(crate) fn permissive(document_url: impl Into<String>) -> Self {
        Self::new(BrowserSecurityConfig::new(document_url))
    }

    /// Evaluates a resource request before the scheduler touches cache/network.
    pub(crate) fn check_resource(&self, request: &ResourceRequest) -> Result<SecurityDecision> {
        let security_request = SecurityRequest::new(
            request.url.clone(),
            destination_for_resource_kind(request.kind),
        )
        .mode(mode_for_resource_kind(request.kind))
        .credentials(credentials_for_resource_kind(request.kind));
        let decision = self.enforcer.evaluate_request(&security_request);
        self.record(&decision);
        if !decision.is_allowed() {
            warn!(
                url = %request.url,
                kind = request.kind.as_str(),
                decision = ?decision.kind,
                violations = decision.violations.len(),
                "blocked resource by origin security policy"
            );
            bail!("resource blocked by security policy: {}", request.url);
        }
        debug!(
            url = %request.url,
            kind = request.kind.as_str(),
            same_origin = decision.same_origin,
            opaque = decision.is_opaque(),
            "resource allowed by origin security policy"
        );
        Ok(decision)
    }

    /// Evaluates a script fetch as a Service Worker registration.
    pub(crate) fn check_service_worker_registration(
        &self,
        script_url: &str,
        scope_url: &str,
    ) -> Result<SecurityDecision> {
        let decision = self
            .enforcer
            .evaluate_service_worker_registration(script_url, scope_url);
        self.record(&decision);
        if !decision.is_allowed() {
            warn!(script_url = %script_url, scope_url = %scope_url, "blocked service worker registration by security policy");
            bail!("service worker registration blocked by security policy");
        }
        Ok(decision)
    }

    /// Returns metrics from the core enforcer.
    #[must_use]
    pub(crate) fn metrics(&self) -> SecurityMetrics {
        self.enforcer.metrics()
    }

    /// Returns app summary.
    #[must_use]
    pub(crate) fn summary(&self) -> AppSecuritySummary {
        self.summary
            .lock()
            .map_or_else(|_| AppSecuritySummary::default(), |summary| summary.clone())
    }

    /// Returns violation log.
    #[must_use]
    pub(crate) fn violations(&self) -> Vec<SecurityViolation> {
        self.enforcer.violations()
    }

    fn record(&self, decision: &SecurityDecision) {
        if let Ok(mut summary) = self.summary.lock() {
            summary.decisions = summary.decisions.saturating_add(1);
            if decision.same_origin {
                summary.same_origin = summary.same_origin.saturating_add(1);
            } else {
                summary.cross_origin = summary.cross_origin.saturating_add(1);
            }
            match decision.kind {
                SecurityDecisionKind::Allowed => {
                    summary.allowed = summary.allowed.saturating_add(1)
                }
                SecurityDecisionKind::AllowedOpaque => {
                    summary.opaque = summary.opaque.saturating_add(1)
                }
                SecurityDecisionKind::AllowedWithWarnings => {
                    summary.warnings = summary
                        .warnings
                        .saturating_add(decision.warnings.len().max(1));
                }
                SecurityDecisionKind::Blocked => {
                    summary.blocked = summary.blocked.saturating_add(1)
                }
            }
        }
    }
}

/// App summary for logs and future DevTools.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AppSecuritySummary {
    pub decisions: usize,
    pub allowed: usize,
    pub opaque: usize,
    pub blocked: usize,
    pub same_origin: usize,
    pub cross_origin: usize,
    pub warnings: usize,
}

impl AppSecuritySummary {
    /// Compact log text.
    #[must_use]
    pub(crate) fn compact(&self) -> String {
        format!(
            "security decisions={} allowed={} opaque={} blocked={} same_origin={} cross_origin={} warnings={}",
            self.decisions,
            self.allowed,
            self.opaque,
            self.blocked,
            self.same_origin,
            self.cross_origin,
            self.warnings
        )
    }
}

/// Extracts a CSP string from response headers, if present.
#[must_use]
pub(crate) fn csp_from_headers(headers: &BTreeMap<String, String>) -> Option<String> {
    headers.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case("content-security-policy")
            .then(|| value.clone())
    })
}

fn destination_for_resource_kind(kind: ResourceKind) -> RequestDestination {
    match kind {
        ResourceKind::Document => RequestDestination::Document,
        ResourceKind::Stylesheet => RequestDestination::Style,
        ResourceKind::Image => RequestDestination::Image,
        ResourceKind::Font => RequestDestination::Font,
        ResourceKind::Script => RequestDestination::Script,
    }
}

fn mode_for_resource_kind(kind: ResourceKind) -> RequestMode {
    match kind {
        ResourceKind::Document => RequestMode::Navigate,
        ResourceKind::Image | ResourceKind::Font => RequestMode::NoCors,
        ResourceKind::Stylesheet | ResourceKind::Script => RequestMode::Cors,
    }
}

fn credentials_for_resource_kind(kind: ResourceKind) -> CredentialsMode {
    match kind {
        ResourceKind::Document => CredentialsMode::Include,
        ResourceKind::Stylesheet | ResourceKind::Script => CredentialsMode::SameOrigin,
        ResourceKind::Image | ResourceKind::Font => CredentialsMode::Omit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::ResourceRequest;

    #[test]
    fn guard_blocks_mixed_active_script() {
        let mut config = BrowserSecurityConfig::new("https://app.example/");
        config.mixed_content = MixedContentPolicy::AllowPassive;
        let guard = ResourceSecurityGuard::new(config);
        let result = guard.check_resource(&ResourceRequest::script("http://app.example/app.js"));
        assert!(result.is_err());
        assert_eq!(guard.summary().blocked, 1);
    }

    #[test]
    fn guard_allows_same_origin_script() {
        let guard = ResourceSecurityGuard::permissive("https://app.example/");
        let result = guard.check_resource(&ResourceRequest::script("https://app.example/app.js"));
        assert!(result.is_ok());
        assert_eq!(guard.summary().allowed, 1);
    }
}
