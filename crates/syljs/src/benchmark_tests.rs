use crate::{
    build_youtube_like_script, run_youtube_like_benchmark, BenchmarkConfig, BenchmarkProfile,
};

#[test]
fn benchmark_script_generation_is_deterministic_and_contains_core_apis() {
    let config = BenchmarkConfig::for_profile(BenchmarkProfile::Smoke);
    let script = build_youtube_like_script(&config);

    assert!(script.contains("new HTMLVideoElement()"));
    assert!(script.contains("new HTMLCanvasElement()"));
    assert!(script.contains("new MediaSource()"));
    assert!(script.contains("new Worker"));
    assert!(script.contains("fetch(\"/api/player/manifest\")"));
    assert!(script.contains("document.styleSheets[0].insertRule"));
    assert!(script.contains("setInterval"));
}

#[test]
fn youtube_like_smoke_benchmark_runs_and_collects_cross_stack_metrics() {
    let run = run_youtube_like_benchmark(BenchmarkConfig::for_profile(BenchmarkProfile::Smoke))
        .expect("benchmark run");

    assert_eq!(run.script.expected_frames, 4);
    assert_eq!(run.script.expected_segments, 2);

    assert!(run.aggregate.vm_instructions > 0);
    assert!(run.aggregate.event_loop_jobs > 0);
    assert!(run.aggregate.fetch_calls >= 3); // manifest + 2 segments
    assert!(run.aggregate.media_buffer_appends >= 2);
    assert!(run.aggregate.canvas_commands_recorded >= 4);
    assert!(run.aggregate.worker_messages_total >= 4);
    assert!(run.aggregate.cssom_mutations >= 3);
    assert!(run.aggregate.dom_mutations > 0);
    assert!(!run.aggregate.hit_limit);

    assert!(run
        .console
        .iter()
        .any(|record| record.line.contains("benchmark-start")));
    assert!(run
        .console
        .iter()
        .any(|record| record.line.contains("frames-complete")));
    assert!(run
        .console
        .iter()
        .any(|record| record.line.contains("buffer-complete")));

    assert_eq!(run.media_segments.len(), 2);
    assert!(!run.canvas_commands.is_empty());
    assert!(!run.style_mutations.is_empty());
}

#[test]
fn benchmark_profile_scaling_changes_workload_size() {
    let smoke = BenchmarkConfig::for_profile(BenchmarkProfile::Smoke);
    let standard = BenchmarkConfig::for_profile(BenchmarkProfile::Standard);
    let stress = BenchmarkConfig::for_profile(BenchmarkProfile::Stress);

    assert!(smoke.frame_count < standard.frame_count);
    assert!(standard.frame_count < stress.frame_count);
    assert!(smoke.segment_count < standard.segment_count);
    assert!(standard.segment_count < stress.segment_count);
    assert!(smoke.recommendation_count < standard.recommendation_count);
    assert!(standard.recommendation_count < stress.recommendation_count);
}

#[test]
fn benchmark_sanitizes_extreme_values() {
    let mut config = BenchmarkConfig::for_profile(BenchmarkProfile::Smoke);
    config.frame_count = 0;
    config.segment_count = 0;
    config.worker_message_count = 1_000_000;
    config.recommendation_count = 1_000_000;
    config.style_mutation_count = 1_000_000;
    config.instruction_budget = 1;

    let sanitized = config.sanitized();

    assert_eq!(sanitized.frame_count, 1);
    assert_eq!(sanitized.segment_count, 1);
    assert_eq!(sanitized.worker_message_count, 10_000);
    assert_eq!(sanitized.recommendation_count, 5_000);
    assert_eq!(sanitized.style_mutation_count, 10_000);
    assert_eq!(sanitized.instruction_budget, 1_000);
}

#[test]
fn standard_benchmark_has_expected_metric_relationships() {
    let run = run_youtube_like_benchmark(BenchmarkConfig::for_profile(BenchmarkProfile::Standard))
        .expect("benchmark run");

    assert!(run.aggregate.fetch_calls >= u64::from(run.config.segment_count + 1));
    assert!(run.aggregate.media_buffer_appends >= u64::from(run.config.segment_count));
    assert!(run.aggregate.canvas_commands_recorded >= u64::from(run.config.frame_count));
    assert!(
        run.aggregate.worker_messages_total
            >= u64::from(run.config.worker_message_count + run.config.segment_count) * 2
    );
    assert!(run.aggregate.console_lines > 0);
}
