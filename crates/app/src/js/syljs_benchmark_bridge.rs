//! App bridge for SylJS YouTube-like benchmark harness.
//!
//! This is the app-facing wrapper around `syljs::run_youtube_like_benchmark`.
//! It gives the desktop/browser shell a stable entry point for paper metrics,
//! diagnostics panels, CLI commands, or future benchmark UI. Because apparently
//! even benchmarks need middleware now. Nature is healing, badly.

use syljs::{
    run_youtube_like_benchmark, BenchmarkAggregateMetrics, BenchmarkConfig, BenchmarkConsoleRecord,
    BenchmarkProfile, BenchmarkRun, JsRuntimeError,
};

/// App benchmark request.
#[derive(Debug, Clone)]
pub(crate) struct AppBenchmarkRequest {
    /// Profile.
    pub profile: BenchmarkProfile,

    /// Optional override for frame count.
    pub frame_count: Option<u32>,

    /// Optional override for segment count.
    pub segment_count: Option<u32>,

    /// Optional override for worker message count.
    pub worker_message_count: Option<u32>,

    /// Optional override for recommendation count.
    pub recommendation_count: Option<u32>,

    /// Optional override for style mutation count.
    pub style_mutation_count: Option<u32>,
}

impl Default for AppBenchmarkRequest {
    fn default() -> Self {
        Self {
            profile: BenchmarkProfile::Standard,
            frame_count: None,
            segment_count: None,
            worker_message_count: None,
            recommendation_count: None,
            style_mutation_count: None,
        }
    }
}

/// App benchmark response.
#[derive(Debug, Clone)]
pub(crate) struct AppBenchmarkResponse {
    /// Profile used.
    pub profile: BenchmarkProfile,

    /// Aggregate metrics.
    pub aggregate: BenchmarkAggregateMetrics,

    /// Console records.
    pub console: Vec<BenchmarkConsoleRecord>,

    /// Number of canvas commands recorded.
    pub canvas_command_count: usize,

    /// Number of media segment records.
    pub media_segment_count: usize,

    /// Number of style mutations.
    pub style_mutation_count: usize,
}

/// Runs the synthetic YouTube-like benchmark from app code.
pub(crate) fn run_app_youtube_like_benchmark(
    request: AppBenchmarkRequest,
) -> Result<AppBenchmarkResponse, JsRuntimeError> {
    let mut config = BenchmarkConfig::for_profile(request.profile);

    if let Some(value) = request.frame_count {
        config.frame_count = value;
    }

    if let Some(value) = request.segment_count {
        config.segment_count = value;
    }

    if let Some(value) = request.worker_message_count {
        config.worker_message_count = value;
    }

    if let Some(value) = request.recommendation_count {
        config.recommendation_count = value;
    }

    if let Some(value) = request.style_mutation_count {
        config.style_mutation_count = value;
    }

    let run = run_youtube_like_benchmark(config)?;

    Ok(response_from_run(run))
}

/// Converts a raw SylJS benchmark run to an app-facing response.
pub(crate) fn response_from_run(run: BenchmarkRun) -> AppBenchmarkResponse {
    AppBenchmarkResponse {
        profile: run.config.profile,
        aggregate: run.aggregate,
        console: run.console,
        canvas_command_count: run.canvas_commands.len(),
        media_segment_count: run.media_segments.len(),
        style_mutation_count: run.style_mutations.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bridge_runs_smoke_profile() {
        let response = run_app_youtube_like_benchmark(AppBenchmarkRequest {
            profile: BenchmarkProfile::Smoke,
            ..Default::default()
        })
        .expect("benchmark");

        assert!(response.aggregate.vm_instructions > 0);
        assert!(response.canvas_command_count > 0);
        assert!(response.media_segment_count > 0);
        assert!(response.style_mutation_count > 0);
    }
}
