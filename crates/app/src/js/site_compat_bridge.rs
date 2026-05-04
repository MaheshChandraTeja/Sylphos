//! App bridge for SylJS site compatibility harness.
//!
//! This wraps `syljs::run_site_compatibility_harness` for the native shell,
//! CLI, future diagnostics panels, and CI-style compatibility checks. The suites
//! are synthetic mirrors of Google, GitHub, and Wikipedia interaction patterns,
//! because depending on live production pages for regression tests is how one
//! accidentally starts a small religion around flaky builds.

use syljs::{
    run_site_compatibility_harness, JsRuntimeError, SiteCompatibilityAggregate,
    SiteCompatibilityHarnessConfig, SiteCompatibilityHarnessRun, SiteCompatibilityProfile,
    SiteCompatibilitySuite, SiteCompatibilitySuiteRun,
};

/// App-facing compatibility harness request.
#[derive(Debug, Clone)]
pub(crate) struct AppSiteCompatibilityRequest {
    /// Profile.
    pub profile: SiteCompatibilityProfile,

    /// Suites. Empty means all suites.
    pub suites: Vec<SiteCompatibilitySuite>,

    /// Optional DOM loop override.
    pub dom_nodes_per_suite: Option<u32>,

    /// Optional style loop override.
    pub style_mutations_per_suite: Option<u32>,

    /// Optional worker-message override.
    pub worker_messages_per_suite: Option<u32>,

    /// Optional timer tick override.
    pub timer_ticks_per_suite: Option<u32>,

    /// Optional canvas draw override.
    pub canvas_draws_per_suite: Option<u32>,

    /// Optional VM instruction budget override.
    pub instruction_budget: Option<u64>,

    /// Optional event-loop job budget override.
    pub max_jobs_per_run: Option<u64>,
}

impl Default for AppSiteCompatibilityRequest {
    fn default() -> Self {
        Self {
            profile: SiteCompatibilityProfile::Standard,
            suites: Vec::new(),
            dom_nodes_per_suite: None,
            style_mutations_per_suite: None,
            worker_messages_per_suite: None,
            timer_ticks_per_suite: None,
            canvas_draws_per_suite: None,
            instruction_budget: None,
            max_jobs_per_run: None,
        }
    }
}

/// App-facing compatibility response.
#[derive(Debug, Clone)]
pub(crate) struct AppSiteCompatibilityResponse {
    /// Profile used.
    pub profile: SiteCompatibilityProfile,

    /// Aggregate summary.
    pub aggregate: SiteCompatibilityAggregate,

    /// Suite reports.
    pub suites: Vec<SiteCompatibilitySuiteRun>,

    /// Raw serializable harness run.
    pub raw: SiteCompatibilityHarnessRun,
}

impl AppSiteCompatibilityResponse {
    /// Returns a compact status line.
    #[must_use]
    pub(crate) fn compact(&self) -> String {
        format!(
            "profile={} suites={} passed={} failed={} score={} instr={} dom={} cssom={} fetch={} workers={} canvas={} limit={}",
            self.profile.as_str(),
            self.aggregate.suites,
            self.aggregate.passed,
            self.aggregate.failed,
            self.aggregate.average_score_percent,
            self.aggregate.total_vm_instructions,
            self.aggregate.total_dom_mutations,
            self.aggregate.total_cssom_mutations,
            self.aggregate.total_fetch_calls,
            self.aggregate.total_worker_messages,
            self.aggregate.total_canvas_commands,
            self.aggregate.any_hit_limit,
        )
    }
}

/// Runs the site compatibility harness from app code.
pub(crate) fn run_app_site_compatibility_harness(
    request: AppSiteCompatibilityRequest,
) -> Result<AppSiteCompatibilityResponse, JsRuntimeError> {
    let mut config = SiteCompatibilityHarnessConfig::for_profile(request.profile);

    if !request.suites.is_empty() {
        config.suites = request.suites;
    }
    if let Some(value) = request.dom_nodes_per_suite {
        config.dom_nodes_per_suite = value;
    }
    if let Some(value) = request.style_mutations_per_suite {
        config.style_mutations_per_suite = value;
    }
    if let Some(value) = request.worker_messages_per_suite {
        config.worker_messages_per_suite = value;
    }
    if let Some(value) = request.timer_ticks_per_suite {
        config.timer_ticks_per_suite = value;
    }
    if let Some(value) = request.canvas_draws_per_suite {
        config.canvas_draws_per_suite = value;
    }
    if let Some(value) = request.instruction_budget {
        config.instruction_budget = value;
    }
    if let Some(value) = request.max_jobs_per_run {
        config.max_jobs_per_run = value;
    }

    let run = run_site_compatibility_harness(config)?;
    Ok(response_from_run(run))
}

/// Converts a raw SylJS compatibility run to an app response.
pub(crate) fn response_from_run(run: SiteCompatibilityHarnessRun) -> AppSiteCompatibilityResponse {
    AppSiteCompatibilityResponse {
        profile: run.config.profile,
        aggregate: run.aggregate.clone(),
        suites: run.suites.clone(),
        raw: run,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bridge_runs_smoke_suite() {
        let response = run_app_site_compatibility_harness(AppSiteCompatibilityRequest {
            profile: SiteCompatibilityProfile::Smoke,
            suites: vec![SiteCompatibilitySuite::GoogleSearch],
            ..Default::default()
        })
        .expect("compat harness");

        assert_eq!(response.aggregate.suites, 1);
        assert!(response.aggregate.average_score_percent > 0);
        assert!(response.compact().contains("suites=1"));
    }
}
