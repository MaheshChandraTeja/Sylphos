//! Sylphos benchmark module.
//!
//! This module is intentionally tiny: it re-exports the app-side JS benchmark
//! bridge so the shell, CLI, debug overlay, or future benchmark panel can call
//! one stable API.

pub(crate) use crate::js::syljs_benchmark_bridge::{
    run_app_youtube_like_benchmark, AppBenchmarkRequest, AppBenchmarkResponse,
};
