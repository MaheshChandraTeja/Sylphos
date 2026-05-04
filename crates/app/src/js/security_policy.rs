#![allow(dead_code)]

//! Source-level security policy capture for the intrinsic app JS executor.
//!
//! This module scans document/script text for security-relevant declarations and
//! operations that the app-side pipeline can enforce or report. It complements
//! the SylJS runtime security host instead of pretending static scanning is a
//! JavaScript engine. We have standards for our lies now.

use syljs::{
    MixedContentPolicy, RequestDestination, RequestMode, SecurityContext, SecurityEnforcer,
    SecurityRequest,
};

/// Captured security effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SecurityPolicyEffect {
    /// `<meta http-equiv="Content-Security-Policy" content="...">` or equivalent string.
    Csp { value: String },
    /// Sandbox token string.
    Sandbox { tokens: String },
    /// Service worker registration attempt.
    ServiceWorkerRegister {
        script_url: String,
        scope_url: Option<String>,
    },
    /// Script attempted an eval-like feature.
    EvalLike { expression: String },
    /// Explicit mixed-content policy hint for tests/devtools.
    MixedContent { policy: MixedContentPolicy },
}

/// Capture result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SecurityPolicyCapture {
    pub effects: Vec<SecurityPolicyEffect>,
    pub warnings: Vec<String>,
}

/// Summary from applying security policy effects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SecurityPolicySummary {
    pub effects: usize,
    pub csp_updates: usize,
    pub sandbox_updates: usize,
    pub service_worker_registrations: usize,
    pub service_worker_blocked: usize,
    pub eval_like_blocked: usize,
    pub warnings: usize,
}

impl SecurityPolicySummary {
    /// Merges another summary.
    pub(crate) fn merge_from(&mut self, other: Self) {
        self.effects = self.effects.saturating_add(other.effects);
        self.csp_updates = self.csp_updates.saturating_add(other.csp_updates);
        self.sandbox_updates = self.sandbox_updates.saturating_add(other.sandbox_updates);
        self.service_worker_registrations = self
            .service_worker_registrations
            .saturating_add(other.service_worker_registrations);
        self.service_worker_blocked = self
            .service_worker_blocked
            .saturating_add(other.service_worker_blocked);
        self.eval_like_blocked = self
            .eval_like_blocked
            .saturating_add(other.eval_like_blocked);
        self.warnings = self.warnings.saturating_add(other.warnings);
    }

    /// Compact log string.
    #[must_use]
    pub(crate) fn compact(&self) -> String {
        format!(
            "security effects={} csp={} sandbox={} sw={} sw_blocked={} eval_blocked={} warnings={}",
            self.effects,
            self.csp_updates,
            self.sandbox_updates,
            self.service_worker_registrations,
            self.service_worker_blocked,
            self.eval_like_blocked,
            self.warnings
        )
    }
}

/// Host-side policy state for one app document script pass.
#[derive(Debug, Clone)]
pub(crate) struct ScriptSecurityPolicyHost {
    context: SecurityContext,
}

impl ScriptSecurityPolicyHost {
    /// Creates policy host.
    #[must_use]
    pub(crate) fn new(document_url: &str) -> Self {
        Self {
            context: SecurityContext::new(document_url),
        }
    }

    /// Returns context.
    #[must_use]
    pub(crate) const fn context(&self) -> &SecurityContext {
        &self.context
    }

    /// Applies captured effects.
    pub(crate) fn apply_effects(
        &mut self,
        effects: &[SecurityPolicyEffect],
    ) -> SecurityPolicySummary {
        let mut summary = SecurityPolicySummary::default();
        for effect in effects {
            summary.effects = summary.effects.saturating_add(1);
            match effect {
                SecurityPolicyEffect::Csp { value } => {
                    let current = self.context.clone();
                    self.context = current.with_csp(value);
                    summary.csp_updates = summary.csp_updates.saturating_add(1);
                }
                SecurityPolicyEffect::Sandbox { tokens } => {
                    let current = self.context.clone();
                    self.context = current.with_sandbox_tokens(tokens);
                    summary.sandbox_updates = summary.sandbox_updates.saturating_add(1);
                }
                SecurityPolicyEffect::MixedContent { policy } => {
                    self.context.mixed_content = *policy;
                }
                SecurityPolicyEffect::ServiceWorkerRegister {
                    script_url,
                    scope_url,
                } => {
                    summary.service_worker_registrations =
                        summary.service_worker_registrations.saturating_add(1);
                    let scope = scope_url
                        .as_deref()
                        .unwrap_or(self.context.document_url.as_str());
                    let enforcer = SecurityEnforcer::new(self.context.clone());
                    let decision = enforcer.evaluate_service_worker_registration(script_url, scope);
                    if !decision.is_allowed() {
                        summary.service_worker_blocked =
                            summary.service_worker_blocked.saturating_add(1);
                    }
                }
                SecurityPolicyEffect::EvalLike { expression: _ } => {
                    let enforcer = SecurityEnforcer::new(self.context.clone());
                    let mut request = SecurityRequest::new(
                        self.context.document_url.clone(),
                        RequestDestination::Script,
                    )
                    .mode(RequestMode::SameOrigin);
                    request.nonce = None;
                    let decision = enforcer.evaluate_request(&request);
                    if !decision.is_allowed() || !self.context.csp.is_empty() {
                        summary.eval_like_blocked = summary.eval_like_blocked.saturating_add(1);
                    }
                }
            }
        }
        summary
    }
}

/// Captures security-relevant effects from script/source text.
#[must_use]
pub(crate) fn capture_security_policy_effects(source: &str) -> SecurityPolicyCapture {
    let mut capture = SecurityPolicyCapture::default();
    let clean = strip_line_comments(source);

    capture
        .effects
        .extend(capture_service_worker_register(&clean));
    capture.effects.extend(capture_cache_security_hints(&clean));
    capture.effects.extend(capture_eval_like(&clean));
    capture.effects.extend(capture_csp_literals(&clean));
    capture
}

fn capture_service_worker_register(source: &str) -> Vec<SecurityPolicyEffect> {
    let mut effects = Vec::new();
    for args in capture_function_args(source, "navigator.serviceWorker.register") {
        let strings = string_literals(&args);
        if let Some(script_url) = strings.first() {
            effects.push(SecurityPolicyEffect::ServiceWorkerRegister {
                script_url: script_url.clone(),
                scope_url: strings.get(1).cloned(),
            });
        }
    }
    effects
}

fn capture_cache_security_hints(source: &str) -> Vec<SecurityPolicyEffect> {
    let mut effects = Vec::new();
    if source.contains("__sylphosAllowMixedContent") {
        effects.push(SecurityPolicyEffect::MixedContent {
            policy: MixedContentPolicy::AllowAll,
        });
    }
    effects
}

fn capture_eval_like(source: &str) -> Vec<SecurityPolicyEffect> {
    let mut effects = Vec::new();
    for name in ["eval", "new Function", "setTimeout", "setInterval"] {
        if source.contains(name) {
            effects.push(SecurityPolicyEffect::EvalLike {
                expression: name.to_owned(),
            });
        }
    }
    effects
}

fn capture_csp_literals(source: &str) -> Vec<SecurityPolicyEffect> {
    let mut effects = Vec::new();
    for marker in ["Content-Security-Policy", "content-security-policy"] {
        let mut cursor = 0usize;
        while let Some(index) = source[cursor..].find(marker) {
            let start = cursor + index + marker.len();
            let slice = &source[start..source.len().min(start + 512)];
            if let Some(value) = first_string_literal(slice) {
                effects.push(SecurityPolicyEffect::Csp { value });
            }
            cursor = start;
        }
    }
    effects
}

fn capture_function_args(source: &str, name: &str) -> Vec<String> {
    let mut args = Vec::new();
    let needle = format!("{name}(");
    let mut cursor = 0usize;
    while let Some(index) = source[cursor..].find(&needle) {
        let open = cursor + index + name.len();
        if let Some(value) = extract_parenthesized(source, open) {
            cursor = open.saturating_add(value.len()).saturating_add(2);
            args.push(value);
        } else {
            break;
        }
    }
    args
}

fn extract_parenthesized(source: &str, open_paren_index: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if bytes.get(open_paren_index) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut start = None;
    let mut in_quote = None::<u8>;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(open_paren_index) {
        if let Some(quote) = in_quote {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == quote {
                in_quote = None;
            }
            continue;
        }
        match *byte {
            b'\'' | b'"' | b'`' => in_quote = Some(*byte),
            b'(' => {
                if depth == 0 {
                    start = Some(index + 1);
                }
                depth = depth.saturating_add(1);
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return start.map(|start| source[start..index].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

fn string_literals(source: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0usize;
    while index < source.len() {
        if let Some((value, end)) = parse_string_literal_at(source, index) {
            values.push(value);
            index = end;
        } else {
            index += 1;
        }
    }
    values
}

fn first_string_literal(source: &str) -> Option<String> {
    string_literals(source).into_iter().next()
}

fn parse_string_literal_at(source: &str, start: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let mut index = skip_ws(bytes, start);
    let quote = *bytes.get(index)?;
    if quote != b'\'' && quote != b'"' && quote != b'`' {
        return None;
    }
    index += 1;
    let mut value = String::new();
    let mut escaped = false;
    while let Some(byte) = bytes.get(index) {
        if escaped {
            value.push(char::from(*byte));
            escaped = false;
            index += 1;
            continue;
        }
        if *byte == b'\\' {
            escaped = true;
            index += 1;
            continue;
        }
        if *byte == quote {
            return Some((value, index + 1));
        }
        value.push(char::from(*byte));
        index += 1;
    }
    None
}

fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        index += 1;
    }
    index
}

fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(before, _)| before))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_service_worker_registration() {
        let capture =
            capture_security_policy_effects("navigator.serviceWorker.register('/sw.js', '/app/')");
        assert!(capture
            .effects
            .iter()
            .any(|effect| matches!(effect, SecurityPolicyEffect::ServiceWorkerRegister { .. })));
    }

    #[test]
    fn applies_service_worker_block_for_insecure_context() {
        let mut host = ScriptSecurityPolicyHost::new("http://example.com/");
        let summary = host.apply_effects(&[SecurityPolicyEffect::ServiceWorkerRegister {
            script_url: "http://example.com/sw.js".to_owned(),
            scope_url: Some("http://example.com/".to_owned()),
        }]);
        assert_eq!(summary.service_worker_blocked, 1);
    }
}
