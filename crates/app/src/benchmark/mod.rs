//! Sylphos benchmark and compatibility harness module.
//!
//! This module intentionally re-exports app-side SylJS benchmark/compatibility
//! bridges so the shell, CLI, debug overlay, or future benchmark panel can call
//! one stable API instead of spelunking through JS internals like a raccoon in a
//! datacenter ceiling.

pub(crate) use crate::js::site_compat_bridge::{
    run_app_site_compatibility_harness, AppSiteCompatibilityRequest,
    AppSiteCompatibilityResponse,
};
pub(crate) use crate::js::syljs_benchmark_bridge::{
    run_app_youtube_like_benchmark, AppBenchmarkRequest, AppBenchmarkResponse,
};
