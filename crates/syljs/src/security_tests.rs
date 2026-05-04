use crate::security::{
    CredentialsMode, CspPolicyLite, MixedContentPolicy, RequestDestination, RequestMode,
    SandboxPolicy, SecurityContext, SecurityDecisionKind, SecurityEnforcer, SecurityOrigin,
    SecurityRequest,
};
use std::collections::BTreeMap;

#[test]
fn parses_default_ports_and_same_origin() {
    let left = SecurityOrigin::parse("https://Example.com/path");
    let right = SecurityOrigin::parse("https://example.com:443/elsewhere");
    assert_eq!(left, right);
    assert!(left.is_potentially_trustworthy());
}

#[test]
fn csp_blocks_cross_origin_script_when_only_self_allowed() {
    let context = SecurityContext::new("https://app.example/index.html")
        .with_csp("default-src 'self'; script-src 'self'");
    let enforcer = SecurityEnforcer::new(context);
    let request = SecurityRequest::new("https://cdn.example/app.js", RequestDestination::Script)
        .mode(RequestMode::Cors);
    let decision = enforcer.evaluate_request(&request);
    assert_eq!(decision.kind, SecurityDecisionKind::Blocked);
    assert_eq!(enforcer.metrics().csp_blocks, 1);
}

#[test]
fn cors_lite_allows_origin_header() {
    let context = SecurityContext::new("https://app.example/");
    let enforcer = SecurityEnforcer::new(context);
    let mut headers = BTreeMap::new();
    headers.insert(
        "Access-Control-Allow-Origin".to_owned(),
        "https://app.example:443".to_owned(),
    );
    let mut request = SecurityRequest::new("https://api.example/data", RequestDestination::Fetch)
        .mode(RequestMode::Cors)
        .credentials(CredentialsMode::Omit);
    request.response_headers = headers;
    let decision = enforcer.evaluate_request(&request);
    assert!(decision.is_allowed());
    assert!(!decision.same_origin);
}

#[test]
fn no_cors_cross_origin_is_opaque() {
    let enforcer = SecurityEnforcer::new(SecurityContext::new("https://app.example/"));
    let request = SecurityRequest::new("https://img.example/a.png", RequestDestination::Image)
        .mode(RequestMode::NoCors);
    let decision = enforcer.evaluate_request(&request);
    assert!(decision.is_allowed());
    assert!(decision.is_opaque());
}

#[test]
fn sandbox_blocks_scripts_without_allow_scripts() {
    let mut context = SecurityContext::new("https://app.example/");
    context.sandbox = SandboxPolicy::from_tokens("allow-same-origin");
    let enforcer = SecurityEnforcer::new(context);
    let request = SecurityRequest::new("https://app.example/app.js", RequestDestination::Script)
        .mode(RequestMode::SameOrigin);
    let decision = enforcer.evaluate_request(&request);
    assert_eq!(decision.kind, SecurityDecisionKind::Blocked);
    assert_eq!(enforcer.metrics().sandbox_blocks, 1);
}

#[test]
fn mixed_active_content_is_blocked_from_https() {
    let mut context = SecurityContext::new("https://app.example/");
    context.mixed_content = MixedContentPolicy::AllowPassive;
    let enforcer = SecurityEnforcer::new(context);
    let request = SecurityRequest::new("http://app.example/app.js", RequestDestination::Script)
        .mode(RequestMode::Cors);
    let decision = enforcer.evaluate_request(&request);
    assert_eq!(decision.kind, SecurityDecisionKind::Blocked);
    assert_eq!(enforcer.metrics().mixed_content_blocks, 1);
}

#[test]
fn service_worker_registration_requires_same_origin_scope() {
    let context = SecurityContext::new("https://app.example/");
    let enforcer = SecurityEnforcer::new(context);
    let decision = enforcer.evaluate_service_worker_registration(
        "https://app.example/sw.js",
        "https://other.example/",
    );
    assert_eq!(decision.kind, SecurityDecisionKind::Blocked);
}

#[test]
fn csp_allows_nonce_for_script() {
    let context =
        SecurityContext::new("https://app.example/").with_csp("script-src 'nonce-abc123'");
    let enforcer = SecurityEnforcer::new(context);
    let mut request =
        SecurityRequest::new("https://cdn.example/app.js", RequestDestination::Script)
            .mode(RequestMode::NoCors);
    request.nonce = Some("abc123".to_owned());
    let decision = enforcer.evaluate_request(&request);
    assert!(decision.is_allowed());
}

#[test]
fn csp_policy_parses_directives() {
    let policy = CspPolicyLite::parse("default-src 'self'; img-src * data:");
    let names = policy
        .directives()
        .into_iter()
        .map(|d| d.name)
        .collect::<Vec<_>>();
    assert!(names.contains(&"default-src".to_owned()));
    assert!(names.contains(&"img-src".to_owned()));
}
