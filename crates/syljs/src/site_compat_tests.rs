use crate::{
    build_site_compatibility_script, run_site_compatibility_harness,
    SiteCompatibilityHarnessConfig, SiteCompatibilityProfile, SiteCompatibilitySuite,
};

#[test]
fn builds_all_synthetic_site_scripts() {
    let config = SiteCompatibilityHarnessConfig::for_profile(SiteCompatibilityProfile::Smoke);

    for suite in SiteCompatibilitySuite::ALL {
        let script = build_site_compatibility_script(suite, &config);
        assert!(script.contains("document.title"));
        assert!(script.contains("fetch("));
        assert!(script.contains("console.log"));
    }
}

#[test]
fn smoke_harness_runs_all_suites() {
    let config = SiteCompatibilityHarnessConfig::for_profile(SiteCompatibilityProfile::Smoke);
    let run = run_site_compatibility_harness(config).expect("compat harness should run");

    assert_eq!(run.suites.len(), 3);
    assert_eq!(run.aggregate.suites, 3);
    assert!(run.aggregate.average_score_percent > 0);
    assert!(run.aggregate.total_vm_instructions > 0);
}

#[test]
fn can_run_single_github_suite() {
    let mut config = SiteCompatibilityHarnessConfig::for_profile(SiteCompatibilityProfile::Smoke);
    config.suites = vec![SiteCompatibilitySuite::GitHubRepository];

    let run = run_site_compatibility_harness(config).expect("github suite should run");

    assert_eq!(run.suites.len(), 1);
    assert_eq!(
        run.suites[0].suite,
        SiteCompatibilitySuite::GitHubRepository
    );
    assert!(run.suites[0].metrics.fetch_calls >= 3);
    assert!(run.suites[0].metrics.worker_messages >= 1);
}

#[test]
fn empty_suite_list_defaults_to_all() {
    let mut config = SiteCompatibilityHarnessConfig::for_profile(SiteCompatibilityProfile::Smoke);
    config.suites.clear();

    let run = run_site_compatibility_harness(config).expect("harness should run default suites");

    assert_eq!(run.suites.len(), SiteCompatibilitySuite::ALL.len());
}
