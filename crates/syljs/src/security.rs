#![allow(clippy::too_many_lines, missing_docs)]
#![doc = "Origin security, CORS-lite, CSP-lite, sandbox, and mixed-content policy for Sylphos."]
#![doc = ""]
#![doc = "This module is deliberately dependency-light and deterministic. It is not a"]
#![doc = "full browser security engine, because those are less modules and more"]
#![doc = "civilizations with incident reports. It does provide stable origin parsing,"]
#![doc = "same-origin decisions, CORS-lite checks, CSP-lite matching, sandbox gates,"]
#![doc = "mixed-content handling, diagnostics, and JS-visible research hooks."]

use crate::{JsFunction, JsRuntimeError, JsValue, Vm};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap, fmt, rc::Rc};

/// Browser origin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SecurityOrigin {
    /// Tuple origin for network schemes.
    Tuple {
        /// Scheme, lower-case.
        scheme: String,
        /// Host, lower-case.
        host: String,
        /// Explicit or default port.
        port: u16,
    },
    /// Opaque origin for unsupported or unique-origin contexts.
    Opaque(String),
}

impl SecurityOrigin {
    /// Parses an origin from an absolute URL.
    #[must_use]
    pub fn parse(url: &str) -> Self {
        parse_origin(url).unwrap_or_else(|| Self::Opaque(stable_opaque_label(url)))
    }

    /// Returns true for tuple HTTP(S) origins.
    #[must_use]
    pub fn is_network_tuple(&self) -> bool {
        matches!(self, Self::Tuple { scheme, .. } if scheme == "http" || scheme == "https")
    }

    /// Returns whether this origin is potentially trustworthy for Sylphos policies.
    #[must_use]
    pub fn is_potentially_trustworthy(&self) -> bool {
        match self {
            Self::Tuple { scheme, host, .. } => {
                scheme == "https" || host == "localhost" || host.ends_with(".localhost")
            }
            Self::Opaque(_) => false,
        }
    }

    /// Returns true if origins are same-origin.
    #[must_use]
    pub fn same_origin(&self, other: &Self) -> bool {
        self == other
    }

    /// Serializes the origin to a stable label.
    #[must_use]
    pub fn serialize(&self) -> String {
        match self {
            Self::Tuple { scheme, host, port } => format!("{scheme}://{host}:{port}"),
            Self::Opaque(label) => format!("opaque:{label}"),
        }
    }
}

impl fmt::Display for SecurityOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.serialize())
    }
}

/// Request mode, matching Fetch concepts in a compact form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestMode {
    /// same-origin only.
    SameOrigin,
    /// CORS request.
    Cors,
    /// no-cors request, resulting in an opaque response for cross-origin.
    NoCors,
    /// navigation request.
    Navigate,
}

impl Default for RequestMode {
    fn default() -> Self {
        Self::Cors
    }
}

/// Credentials mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialsMode {
    /// Omit credentials.
    Omit,
    /// Same-origin credentials.
    SameOrigin,
    /// Include credentials.
    Include,
}

impl Default for CredentialsMode {
    fn default() -> Self {
        Self::SameOrigin
    }
}

/// Fetch destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RequestDestination {
    Document,
    Script,
    Style,
    Image,
    Font,
    Worker,
    ServiceWorker,
    Media,
    Manifest,
    Fetch,
    Xhr,
    Other,
}

impl RequestDestination {
    /// CSP directive fallback chain for this destination.
    #[must_use]
    pub fn csp_directives(self) -> &'static [&'static str] {
        match self {
            Self::Script | Self::Worker | Self::ServiceWorker => &["script-src", "default-src"],
            Self::Style => &["style-src", "default-src"],
            Self::Image => &["img-src", "default-src"],
            Self::Font => &["font-src", "default-src"],
            Self::Media => &["media-src", "default-src"],
            Self::Manifest => &["manifest-src", "default-src"],
            Self::Fetch | Self::Xhr => &["connect-src", "default-src"],
            Self::Document | Self::Other => &["default-src"],
        }
    }

    /// Stable label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Script => "script",
            Self::Style => "style",
            Self::Image => "image",
            Self::Font => "font",
            Self::Worker => "worker",
            Self::ServiceWorker => "service-worker",
            Self::Media => "media",
            Self::Manifest => "manifest",
            Self::Fetch => "fetch",
            Self::Xhr => "xhr",
            Self::Other => "other",
        }
    }
}

/// Referrer policy subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferrerPolicyLite {
    NoReferrer,
    Origin,
    SameOrigin,
    StrictOriginWhenCrossOrigin,
    UnsafeUrl,
}

impl Default for ReferrerPolicyLite {
    fn default() -> Self {
        Self::StrictOriginWhenCrossOrigin
    }
}

/// Mixed content behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MixedContentPolicy {
    /// Block active and optionally-blockable content.
    BlockAll,
    /// Allow passive mixed image/media loads only.
    AllowPassive,
    /// Allow all mixed content. Useful for local synthetic tests only.
    AllowAll,
}

impl Default for MixedContentPolicy {
    fn default() -> Self {
        Self::AllowPassive
    }
}

/// Sandbox flags. `true` means capability is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPolicy {
    pub allow_scripts: bool,
    pub allow_same_origin: bool,
    pub allow_forms: bool,
    pub allow_popups: bool,
    pub allow_top_navigation: bool,
    pub allow_downloads: bool,
    pub allow_modals: bool,
    pub allow_pointer_lock: bool,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self::unsandboxed()
    }
}

impl SandboxPolicy {
    /// No sandbox restrictions.
    #[must_use]
    pub const fn unsandboxed() -> Self {
        Self {
            allow_scripts: true,
            allow_same_origin: true,
            allow_forms: true,
            allow_popups: true,
            allow_top_navigation: true,
            allow_downloads: true,
            allow_modals: true,
            allow_pointer_lock: true,
        }
    }

    /// Fully sandboxed default for `<iframe sandbox>` without tokens.
    #[must_use]
    pub const fn locked_down() -> Self {
        Self {
            allow_scripts: false,
            allow_same_origin: false,
            allow_forms: false,
            allow_popups: false,
            allow_top_navigation: false,
            allow_downloads: false,
            allow_modals: false,
            allow_pointer_lock: false,
        }
    }

    /// Parses sandbox tokens.
    #[must_use]
    pub fn from_tokens(tokens: &str) -> Self {
        let mut policy = Self::locked_down();
        for token in tokens.split_ascii_whitespace().map(str::trim) {
            match token {
                "allow-scripts" => policy.allow_scripts = true,
                "allow-same-origin" => policy.allow_same_origin = true,
                "allow-forms" => policy.allow_forms = true,
                "allow-popups" => policy.allow_popups = true,
                "allow-top-navigation" | "allow-top-navigation-by-user-activation" => {
                    policy.allow_top_navigation = true;
                }
                "allow-downloads" => policy.allow_downloads = true,
                "allow-modals" => policy.allow_modals = true,
                "allow-pointer-lock" => policy.allow_pointer_lock = true,
                _ => {}
            }
        }
        policy
    }

    /// Returns true if this policy imposes any restriction.
    #[must_use]
    pub const fn is_sandboxed(self) -> bool {
        !(self.allow_scripts
            && self.allow_same_origin
            && self.allow_forms
            && self.allow_popups
            && self.allow_top_navigation
            && self.allow_downloads
            && self.allow_modals
            && self.allow_pointer_lock)
    }
}

/// One CSP directive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CspDirective {
    pub name: String,
    pub sources: Vec<String>,
}

/// CSP-lite policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CspPolicyLite {
    directives: BTreeMap<String, Vec<String>>,
    report_only: bool,
}

impl CspPolicyLite {
    /// Empty policy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses a CSP header/meta value.
    #[must_use]
    pub fn parse(input: &str) -> Self {
        let mut policy = Self::new();
        for raw in input.split(';') {
            let mut parts = raw.split_ascii_whitespace();
            let Some(name) = parts.next() else {
                continue;
            };
            let key = name.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            let values = parts
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            policy.directives.insert(key, values);
        }
        policy
    }

    /// Marks policy report-only. Report-only still returns AllowedWithWarnings.
    #[must_use]
    pub const fn report_only(mut self, report_only: bool) -> Self {
        self.report_only = report_only;
        self
    }

    /// Returns directives for diagnostics.
    #[must_use]
    pub fn directives(&self) -> Vec<CspDirective> {
        self.directives
            .iter()
            .map(|(name, sources)| CspDirective {
                name: name.clone(),
                sources: sources.clone(),
            })
            .collect()
    }

    /// Returns true when no CSP is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.directives.is_empty()
    }

    /// Checks a URL against destination directive fallback chain.
    #[must_use]
    pub fn allows(
        &self,
        document_origin: &SecurityOrigin,
        destination: RequestDestination,
        target_url: &str,
        nonce: Option<&str>,
    ) -> CspCheckResult {
        if self.is_empty() {
            return CspCheckResult::Allowed;
        }

        for directive in destination.csp_directives() {
            if let Some(sources) = self.directives.get(*directive) {
                let allowed = source_list_allows(sources, document_origin, target_url, nonce);
                if allowed {
                    return CspCheckResult::Allowed;
                }
                return if self.report_only {
                    CspCheckResult::ReportOnlyViolation((*directive).to_owned())
                } else {
                    CspCheckResult::Blocked((*directive).to_owned())
                };
            }
        }

        CspCheckResult::Allowed
    }
}

/// CSP decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CspCheckResult {
    Allowed,
    ReportOnlyViolation(String),
    Blocked(String),
}

/// Security context for one document or worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityContext {
    pub document_url: String,
    pub origin: SecurityOrigin,
    pub top_level_origin: SecurityOrigin,
    pub csp: CspPolicyLite,
    pub sandbox: SandboxPolicy,
    pub referrer_policy: ReferrerPolicyLite,
    pub mixed_content: MixedContentPolicy,
    pub secure_context_required_for_service_worker: bool,
}

impl SecurityContext {
    /// Creates default context from URL.
    #[must_use]
    pub fn new(document_url: impl Into<String>) -> Self {
        let document_url = document_url.into();
        let origin = SecurityOrigin::parse(&document_url);
        Self {
            document_url,
            top_level_origin: origin.clone(),
            origin,
            csp: CspPolicyLite::new(),
            sandbox: SandboxPolicy::unsandboxed(),
            referrer_policy: ReferrerPolicyLite::default(),
            mixed_content: MixedContentPolicy::default(),
            secure_context_required_for_service_worker: true,
        }
    }

    /// Applies CSP header/meta text.
    #[must_use]
    pub fn with_csp(mut self, csp: impl AsRef<str>) -> Self {
        self.csp = CspPolicyLite::parse(csp.as_ref());
        self
    }

    /// Applies sandbox tokens.
    #[must_use]
    pub fn with_sandbox_tokens(mut self, tokens: impl AsRef<str>) -> Self {
        self.sandbox = SandboxPolicy::from_tokens(tokens.as_ref());
        if !self.sandbox.allow_same_origin {
            self.origin = SecurityOrigin::Opaque(stable_opaque_label(&self.document_url));
        }
        self
    }

    /// Returns whether document is secure-context-like.
    #[must_use]
    pub fn is_secure_context(&self) -> bool {
        self.origin.is_potentially_trustworthy()
    }

    /// Builds a referrer value for a target URL.
    #[must_use]
    pub fn referrer_for(&self, target_url: &str) -> Option<String> {
        match self.referrer_policy {
            ReferrerPolicyLite::NoReferrer => None,
            ReferrerPolicyLite::UnsafeUrl => Some(self.document_url.clone()),
            ReferrerPolicyLite::Origin => Some(self.origin.serialize()),
            ReferrerPolicyLite::SameOrigin => {
                let target_origin = SecurityOrigin::parse(target_url);
                self.origin
                    .same_origin(&target_origin)
                    .then(|| self.document_url.clone())
            }
            ReferrerPolicyLite::StrictOriginWhenCrossOrigin => {
                let target_origin = SecurityOrigin::parse(target_url);
                if self.origin.same_origin(&target_origin) {
                    Some(self.document_url.clone())
                } else if !is_downgrade(&self.document_url, target_url) {
                    Some(self.origin.serialize())
                } else {
                    None
                }
            }
        }
    }
}

/// Security request input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityRequest {
    pub url: String,
    pub method: String,
    pub destination: RequestDestination,
    pub mode: RequestMode,
    pub credentials: CredentialsMode,
    pub initiator: String,
    pub response_headers: BTreeMap<String, String>,
    pub nonce: Option<String>,
}

impl SecurityRequest {
    /// Creates a request with browser-like defaults.
    #[must_use]
    pub fn new(url: impl Into<String>, destination: RequestDestination) -> Self {
        Self {
            url: url.into(),
            method: "GET".to_owned(),
            destination,
            mode: RequestMode::Cors,
            credentials: CredentialsMode::SameOrigin,
            initiator: String::new(),
            response_headers: BTreeMap::new(),
            nonce: None,
        }
    }

    #[must_use]
    pub fn mode(mut self, mode: RequestMode) -> Self {
        self.mode = mode;
        self
    }

    #[must_use]
    pub fn credentials(mut self, credentials: CredentialsMode) -> Self {
        self.credentials = credentials;
        self
    }

    #[must_use]
    pub fn method(mut self, method: impl Into<String>) -> Self {
        self.method = method.into().to_ascii_uppercase();
        self
    }
}

/// Final security decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityDecisionKind {
    Allowed,
    AllowedOpaque,
    AllowedWithWarnings,
    Blocked,
}

/// Security violation kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityViolationKind {
    InvalidUrl,
    SameOrigin,
    Cors,
    Csp,
    Sandbox,
    MixedContent,
    SecureContext,
    Method,
}

/// One security violation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityViolation {
    pub kind: SecurityViolationKind,
    pub url: String,
    pub message: String,
    pub directive: Option<String>,
}

/// Full decision with diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityDecision {
    pub kind: SecurityDecisionKind,
    pub request_url: String,
    pub target_origin: SecurityOrigin,
    pub same_origin: bool,
    pub credentials_allowed: bool,
    pub referrer: Option<String>,
    pub violations: Vec<SecurityViolation>,
    pub warnings: Vec<String>,
}

impl SecurityDecision {
    /// Returns true if the request may proceed.
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        !matches!(self.kind, SecurityDecisionKind::Blocked)
    }

    /// Returns true if the response should be opaque to script.
    #[must_use]
    pub fn is_opaque(&self) -> bool {
        matches!(self.kind, SecurityDecisionKind::AllowedOpaque)
    }

    fn blocked(
        url: String,
        target_origin: SecurityOrigin,
        same_origin: bool,
        violation: SecurityViolation,
    ) -> Self {
        Self {
            kind: SecurityDecisionKind::Blocked,
            request_url: url,
            target_origin,
            same_origin,
            credentials_allowed: false,
            referrer: None,
            violations: vec![violation],
            warnings: Vec::new(),
        }
    }
}

/// Security metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityMetrics {
    pub decisions: u64,
    pub allowed: u64,
    pub allowed_opaque: u64,
    pub allowed_with_warnings: u64,
    pub blocked: u64,
    pub same_origin_requests: u64,
    pub cross_origin_requests: u64,
    pub cors_preflight_like_checks: u64,
    pub csp_blocks: u64,
    pub sandbox_blocks: u64,
    pub mixed_content_blocks: u64,
    pub secure_context_blocks: u64,
}

/// Stateful security enforcer with metrics and violation log.
#[derive(Debug, Clone)]
pub struct SecurityEnforcer {
    context: SecurityContext,
    metrics: RefCell<SecurityMetrics>,
    violations: RefCell<Vec<SecurityViolation>>,
}

impl SecurityEnforcer {
    /// Creates an enforcer for a context.
    #[must_use]
    pub fn new(context: SecurityContext) -> Self {
        Self {
            context,
            metrics: RefCell::new(SecurityMetrics::default()),
            violations: RefCell::new(Vec::new()),
        }
    }

    /// Returns immutable context.
    #[must_use]
    pub fn context(&self) -> &SecurityContext {
        &self.context
    }

    /// Evaluates a request.
    #[must_use]
    pub fn evaluate_request(&self, request: &SecurityRequest) -> SecurityDecision {
        let mut metrics = self.metrics.borrow_mut();
        metrics.decisions = metrics.decisions.saturating_add(1);

        let target_origin = SecurityOrigin::parse(&request.url);
        let same_origin = self.context.origin.same_origin(&target_origin);
        if same_origin {
            metrics.same_origin_requests = metrics.same_origin_requests.saturating_add(1);
        } else {
            metrics.cross_origin_requests = metrics.cross_origin_requests.saturating_add(1);
        }

        if !target_origin.is_network_tuple() {
            let decision = SecurityDecision::blocked(
                request.url.clone(),
                target_origin,
                same_origin,
                violation(
                    SecurityViolationKind::InvalidUrl,
                    &request.url,
                    "unsupported or opaque target URL",
                    None,
                ),
            );
            metrics.blocked = metrics.blocked.saturating_add(1);
            self.record_decision(&decision);
            return decision;
        }

        if let Some(block) = self.sandbox_block(request) {
            metrics.blocked = metrics.blocked.saturating_add(1);
            metrics.sandbox_blocks = metrics.sandbox_blocks.saturating_add(1);
            let decision =
                SecurityDecision::blocked(request.url.clone(), target_origin, same_origin, block);
            self.record_decision(&decision);
            return decision;
        }

        if self.is_mixed_content_blocked(request) {
            metrics.blocked = metrics.blocked.saturating_add(1);
            metrics.mixed_content_blocks = metrics.mixed_content_blocks.saturating_add(1);
            let decision = SecurityDecision::blocked(
                request.url.clone(),
                target_origin,
                same_origin,
                violation(
                    SecurityViolationKind::MixedContent,
                    &request.url,
                    "blocked mixed content downgrade",
                    None,
                ),
            );
            self.record_decision(&decision);
            return decision;
        }

        if request.destination == RequestDestination::ServiceWorker
            && self.context.secure_context_required_for_service_worker
            && !self.context.is_secure_context()
        {
            metrics.blocked = metrics.blocked.saturating_add(1);
            metrics.secure_context_blocks = metrics.secure_context_blocks.saturating_add(1);
            let decision = SecurityDecision::blocked(
                request.url.clone(),
                target_origin,
                same_origin,
                violation(
                    SecurityViolationKind::SecureContext,
                    &request.url,
                    "service workers require a secure context",
                    None,
                ),
            );
            self.record_decision(&decision);
            return decision;
        }

        match self.context.csp.allows(
            &self.context.origin,
            request.destination,
            &request.url,
            request.nonce.as_deref(),
        ) {
            CspCheckResult::Allowed => {}
            CspCheckResult::ReportOnlyViolation(directive) => {
                let mut decision = self.allowed_decision(request, target_origin, same_origin);
                decision.kind = SecurityDecisionKind::AllowedWithWarnings;
                decision
                    .warnings
                    .push(format!("CSP report-only violation: {directive}"));
                metrics.allowed_with_warnings = metrics.allowed_with_warnings.saturating_add(1);
                self.record_decision(&decision);
                return decision;
            }
            CspCheckResult::Blocked(directive) => {
                metrics.blocked = metrics.blocked.saturating_add(1);
                metrics.csp_blocks = metrics.csp_blocks.saturating_add(1);
                let decision = SecurityDecision::blocked(
                    request.url.clone(),
                    target_origin,
                    same_origin,
                    violation(
                        SecurityViolationKind::Csp,
                        &request.url,
                        "blocked by CSP-lite",
                        Some(directive),
                    ),
                );
                self.record_decision(&decision);
                return decision;
            }
        }

        if !same_origin {
            match request.mode {
                RequestMode::SameOrigin => {
                    metrics.blocked = metrics.blocked.saturating_add(1);
                    let decision = SecurityDecision::blocked(
                        request.url.clone(),
                        target_origin,
                        false,
                        violation(
                            SecurityViolationKind::SameOrigin,
                            &request.url,
                            "same-origin request targeted a different origin",
                            None,
                        ),
                    );
                    self.record_decision(&decision);
                    return decision;
                }
                RequestMode::NoCors => {
                    metrics.allowed_opaque = metrics.allowed_opaque.saturating_add(1);
                    let mut decision = self.allowed_decision(request, target_origin, false);
                    decision.kind = SecurityDecisionKind::AllowedOpaque;
                    self.record_decision(&decision);
                    return decision;
                }
                RequestMode::Cors => {
                    metrics.cors_preflight_like_checks =
                        metrics.cors_preflight_like_checks.saturating_add(1);
                    if !cors_lite_allows(&self.context.origin, request) {
                        metrics.blocked = metrics.blocked.saturating_add(1);
                        let decision = SecurityDecision::blocked(
                            request.url.clone(),
                            target_origin,
                            false,
                            violation(
                                SecurityViolationKind::Cors,
                                &request.url,
                                "CORS-lite response headers did not allow this origin",
                                None,
                            ),
                        );
                        self.record_decision(&decision);
                        return decision;
                    }
                }
                RequestMode::Navigate => {}
            }
        }

        let mut decision = self.allowed_decision(request, target_origin, same_origin);
        if !same_origin
            && request.credentials == CredentialsMode::Include
            && !cors_credentials_allowed(request)
        {
            decision.kind = SecurityDecisionKind::AllowedWithWarnings;
            decision.credentials_allowed = false;
            decision.warnings.push(
                "credentials requested but Access-Control-Allow-Credentials was not true"
                    .to_owned(),
            );
            metrics.allowed_with_warnings = metrics.allowed_with_warnings.saturating_add(1);
        } else {
            metrics.allowed = metrics.allowed.saturating_add(1);
        }
        self.record_decision(&decision);
        decision
    }

    /// Evaluates service worker registration scope and script URL.
    #[must_use]
    pub fn evaluate_service_worker_registration(
        &self,
        script_url: &str,
        scope_url: &str,
    ) -> SecurityDecision {
        let script = SecurityRequest::new(script_url, RequestDestination::ServiceWorker)
            .mode(RequestMode::SameOrigin)
            .credentials(CredentialsMode::SameOrigin);
        let decision = self.evaluate_request(&script);
        if !decision.is_allowed() {
            return decision;
        }

        let scope_origin = SecurityOrigin::parse(scope_url);
        if !scope_origin.same_origin(&self.context.origin) {
            return SecurityDecision::blocked(
                scope_url.to_owned(),
                scope_origin,
                false,
                violation(
                    SecurityViolationKind::SameOrigin,
                    scope_url,
                    "service worker scope must be same-origin",
                    None,
                ),
            );
        }

        decision
    }

    /// Returns metrics snapshot.
    #[must_use]
    pub fn metrics(&self) -> SecurityMetrics {
        self.metrics.borrow().clone()
    }

    /// Returns violation log.
    #[must_use]
    pub fn violations(&self) -> Vec<SecurityViolation> {
        self.violations.borrow().clone()
    }

    fn allowed_decision(
        &self,
        request: &SecurityRequest,
        target_origin: SecurityOrigin,
        same_origin: bool,
    ) -> SecurityDecision {
        let credentials_allowed = match request.credentials {
            CredentialsMode::Omit => false,
            CredentialsMode::SameOrigin => same_origin,
            CredentialsMode::Include => same_origin || cors_credentials_allowed(request),
        };
        SecurityDecision {
            kind: SecurityDecisionKind::Allowed,
            request_url: request.url.clone(),
            target_origin,
            same_origin,
            credentials_allowed,
            referrer: self.context.referrer_for(&request.url),
            violations: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn sandbox_block(&self, request: &SecurityRequest) -> Option<SecurityViolation> {
        let sandbox = self.context.sandbox;
        if request.destination == RequestDestination::Script && !sandbox.allow_scripts {
            return Some(violation(
                SecurityViolationKind::Sandbox,
                &request.url,
                "sandbox blocks script execution/loading",
                None,
            ));
        }
        if request.destination == RequestDestination::ServiceWorker && !sandbox.allow_scripts {
            return Some(violation(
                SecurityViolationKind::Sandbox,
                &request.url,
                "sandbox blocks service worker registration",
                None,
            ));
        }
        if request.destination == RequestDestination::Document
            && request.mode == RequestMode::Navigate
            && !sandbox.allow_top_navigation
        {
            return Some(violation(
                SecurityViolationKind::Sandbox,
                &request.url,
                "sandbox blocks top navigation",
                None,
            ));
        }
        None
    }

    fn is_mixed_content_blocked(&self, request: &SecurityRequest) -> bool {
        if !self
            .context
            .document_url
            .to_ascii_lowercase()
            .starts_with("https://")
        {
            return false;
        }
        if !request.url.to_ascii_lowercase().starts_with("http://") {
            return false;
        }
        match self.context.mixed_content {
            MixedContentPolicy::AllowAll => false,
            MixedContentPolicy::AllowPassive => !matches!(
                request.destination,
                RequestDestination::Image | RequestDestination::Media
            ),
            MixedContentPolicy::BlockAll => true,
        }
    }

    fn record_decision(&self, decision: &SecurityDecision) {
        if !decision.violations.is_empty() {
            self.violations
                .borrow_mut()
                .extend(decision.violations.clone());
        }
    }
}

/// Shared security host pointer.
pub type SharedSecurityHost = Rc<SecurityEnforcer>;

/// Installs security diagnostics into SylJS globals.
pub fn install_security_globals(vm: &mut Vm, host: SharedSecurityHost) {
    let object = JsValue::object();
    object.set_property("origin", JsValue::String(host.context().origin.serialize()));
    object.set_property(
        "isSecureContext",
        JsValue::Boolean(host.context().is_secure_context()),
    );
    object.set_property("evaluate", create_security_evaluate_function(host.clone()));
    object.set_property("metrics", create_security_metrics_function(host.clone()));
    object.set_property("violations", create_security_violations_function(host));
    vm.define_global("__sylphosSecurity", object);
}

fn create_security_evaluate_function(host: SharedSecurityHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosSecurity.evaluate".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let url = args.first().map_or_else(String::new, JsValue::to_js_string);
            let destination = args.get(1).map_or(RequestDestination::Fetch, |value| {
                destination_from_js(&value.to_js_string())
            });
            let mode = args.get(2).map_or(RequestMode::Cors, |value| {
                request_mode_from_js(&value.to_js_string())
            });
            let decision =
                host.evaluate_request(&SecurityRequest::new(url, destination).mode(mode));
            Ok(decision_to_js(&decision))
        }),
    })
}

fn create_security_metrics_function(host: SharedSecurityHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosSecurity.metrics".to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(metrics_to_js(&host.metrics()))),
    })
}

fn create_security_violations_function(host: SharedSecurityHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosSecurity.violations".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            Ok(JsValue::array(
                host.violations()
                    .into_iter()
                    .map(|entry| JsValue::String(format!("{:?}:{}", entry.kind, entry.url)))
                    .collect(),
            ))
        }),
    })
}

fn decision_to_js(decision: &SecurityDecision) -> JsValue {
    let object = JsValue::object();
    object.set_property("allowed", JsValue::Boolean(decision.is_allowed()));
    object.set_property("opaque", JsValue::Boolean(decision.is_opaque()));
    object.set_property("kind", JsValue::String(format!("{:?}", decision.kind)));
    object.set_property("sameOrigin", JsValue::Boolean(decision.same_origin));
    object.set_property(
        "targetOrigin",
        JsValue::String(decision.target_origin.serialize()),
    );
    object.set_property(
        "credentialsAllowed",
        JsValue::Boolean(decision.credentials_allowed),
    );
    object.set_property(
        "violations",
        JsValue::Number(decision.violations.len() as f64),
    );
    object
}

fn metrics_to_js(metrics: &SecurityMetrics) -> JsValue {
    let object = JsValue::object();
    object.set_property("decisions", JsValue::Number(metrics.decisions as f64));
    object.set_property("allowed", JsValue::Number(metrics.allowed as f64));
    object.set_property("blocked", JsValue::Number(metrics.blocked as f64));
    object.set_property(
        "crossOriginRequests",
        JsValue::Number(metrics.cross_origin_requests as f64),
    );
    object.set_property("cspBlocks", JsValue::Number(metrics.csp_blocks as f64));
    object.set_property(
        "sandboxBlocks",
        JsValue::Number(metrics.sandbox_blocks as f64),
    );
    object.set_property(
        "mixedContentBlocks",
        JsValue::Number(metrics.mixed_content_blocks as f64),
    );
    object
}

fn destination_from_js(value: &str) -> RequestDestination {
    match value {
        "document" => RequestDestination::Document,
        "script" => RequestDestination::Script,
        "style" | "stylesheet" => RequestDestination::Style,
        "image" => RequestDestination::Image,
        "font" => RequestDestination::Font,
        "worker" => RequestDestination::Worker,
        "service-worker" => RequestDestination::ServiceWorker,
        "media" => RequestDestination::Media,
        "manifest" => RequestDestination::Manifest,
        "xhr" => RequestDestination::Xhr,
        "fetch" => RequestDestination::Fetch,
        _ => RequestDestination::Other,
    }
}

fn request_mode_from_js(value: &str) -> RequestMode {
    match value {
        "same-origin" => RequestMode::SameOrigin,
        "no-cors" => RequestMode::NoCors,
        "navigate" => RequestMode::Navigate,
        _ => RequestMode::Cors,
    }
}

fn parse_origin(url: &str) -> Option<SecurityOrigin> {
    let trimmed = url.trim();
    let (scheme, rest) = trimmed.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if authority.is_empty() {
        return None;
    }

    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let host = authority[..=end].to_ascii_lowercase();
        let port = authority[end + 1..]
            .strip_prefix(':')
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or_else(|| default_port(&scheme));
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if port.chars().all(|ch| ch.is_ascii_digit()) {
            (
                host.to_ascii_lowercase(),
                port.parse().unwrap_or_else(|_| default_port(&scheme)),
            )
        } else {
            (authority.to_ascii_lowercase(), default_port(&scheme))
        }
    } else {
        (authority.to_ascii_lowercase(), default_port(&scheme))
    };

    if host.is_empty() {
        return None;
    }

    Some(SecurityOrigin::Tuple { scheme, host, port })
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "https" {
        443
    } else {
        80
    }
}

fn stable_opaque_label(input: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn violation(
    kind: SecurityViolationKind,
    url: &str,
    message: &str,
    directive: Option<String>,
) -> SecurityViolation {
    SecurityViolation {
        kind,
        url: url.to_owned(),
        message: message.to_owned(),
        directive,
    }
}

fn is_downgrade(document_url: &str, target_url: &str) -> bool {
    document_url.to_ascii_lowercase().starts_with("https://")
        && target_url.to_ascii_lowercase().starts_with("http://")
}

fn cors_lite_allows(document_origin: &SecurityOrigin, request: &SecurityRequest) -> bool {
    let Some(allow_origin) = header_ci(&request.response_headers, "access-control-allow-origin")
    else {
        return false;
    };
    let origin = document_origin.serialize();
    allow_origin == "*" || allow_origin.eq_ignore_ascii_case(&origin)
}

fn cors_credentials_allowed(request: &SecurityRequest) -> bool {
    header_ci(
        &request.response_headers,
        "access-control-allow-credentials",
    )
    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn header_ci(headers: &BTreeMap<String, String>, name: &str) -> Option<String> {
    headers.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn source_list_allows(
    sources: &[String],
    document_origin: &SecurityOrigin,
    target_url: &str,
    nonce: Option<&str>,
) -> bool {
    if sources.is_empty() {
        return false;
    }
    let target_origin = SecurityOrigin::parse(target_url);
    let target_origin_string = target_origin.serialize();
    let scheme = target_url
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    for source in sources {
        let value = source.trim().trim_matches('"');
        let unquoted = value.trim_matches('\'');
        if value == "*" {
            return true;
        }
        if value == "'none'" || unquoted == "none" {
            return false;
        }
        if (value == "'self'" || unquoted == "self") && document_origin.same_origin(&target_origin)
        {
            return true;
        }
        if let Some(required_nonce) = value
            .strip_prefix("'nonce-")
            .and_then(|v| v.strip_suffix('\''))
            .or_else(|| unquoted.strip_prefix("nonce-"))
        {
            if nonce == Some(required_nonce) {
                return true;
            }
        }
        if value.ends_with(':') && value.trim_end_matches(':').eq_ignore_ascii_case(&scheme) {
            return true;
        }
        if value.contains("://") {
            let allowed_origin = SecurityOrigin::parse(value).serialize();
            if allowed_origin == target_origin_string {
                return true;
            }
        } else if host_matches_source(value, target_url) {
            return true;
        }
    }
    false
}

fn host_matches_source(source: &str, target_url: &str) -> bool {
    let SecurityOrigin::Tuple { host, .. } = SecurityOrigin::parse(target_url) else {
        return false;
    };
    if let Some(rest) = source.strip_prefix("*.") {
        return host == rest || host.ends_with(&format!(".{rest}"));
    }
    host == source.to_ascii_lowercase()
}

impl From<serde_json::Error> for JsRuntimeError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}
