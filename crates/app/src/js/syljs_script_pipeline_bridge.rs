//! App bridge for SylJS script pipeline integration.
//!
//! Module 39 moves SylJS execution from "we ran some scripts in a lab jar" to
//! "scripts participate in document loading and request reflow." The web still
//! remains a swamp, naturally, but now the swamp has order forms.

#![allow(dead_code)]

use std::rc::Rc;

use syljs::{
    run_research_script_pipeline, AsyncSchedulingPolicy, EventLoopConfig, JsRuntimeError,
    PipelineEvent, ResearchReflowHooks, ResearchScriptResourceLoader, ScheduledVm,
    ScriptDescriptor, ScriptKind, ScriptLoadMode, ScriptPipelineConfig, ScriptPipelineMetrics,
    ScriptPipelineRun, ScriptSource, SharedScriptPipelineHooks, SharedScriptResourceLoader,
    VmConfig,
};

/// App-facing script input.
#[derive(Debug, Clone)]
pub(crate) struct AppScriptPipelineInput {
    /// Script id.
    pub id: u64,

    /// Label.
    pub label: String,

    /// Script source or URL.
    pub source: AppScriptPipelineSource,

    /// Load mode.
    pub mode: ScriptLoadMode,

    /// Script kind.
    pub kind: ScriptKind,
}

/// App-facing source.
#[derive(Debug, Clone)]
pub(crate) enum AppScriptPipelineSource {
    /// Inline source.
    Inline(String),

    /// External URL.
    External(String),
}

/// App-facing script pipeline request.
#[derive(Debug, Clone)]
pub(crate) struct AppScriptPipelineRequest {
    /// Scripts in parser discovery order.
    pub scripts: Vec<AppScriptPipelineInput>,

    /// External script assets for deterministic app/test use.
    pub external_assets: Vec<(String, String)>,

    /// Async scheduling policy.
    pub async_policy: AsyncSchedulingPolicy,

    /// Continue after script errors.
    pub continue_on_error: bool,
}

impl Default for AppScriptPipelineRequest {
    fn default() -> Self {
        Self {
            scripts: Vec::new(),
            external_assets: Vec::new(),
            async_policy: AsyncSchedulingPolicy::AsSoonAsDiscovered,
            continue_on_error: true,
        }
    }
}

/// App-facing script pipeline response.
#[derive(Debug, Clone)]
pub(crate) struct AppScriptPipelineResponse {
    /// Pipeline metrics.
    pub metrics: ScriptPipelineMetrics,

    /// Pipeline events.
    pub events: Vec<PipelineEvent>,

    /// Console output from last drain.
    pub console: Vec<String>,

    /// Failure count.
    pub failures: usize,

    /// Reflow request count.
    pub reflow_requests: usize,
}

/// Executes the script pipeline against a supplied ScheduledVm.
///
/// The caller should install DOM/CSSOM/WebAPI/media/canvas/worker globals on
/// `scheduled.vm` before calling this if those APIs are needed by the page.
pub(crate) fn execute_app_script_pipeline(
    scheduled: &mut ScheduledVm,
    request: AppScriptPipelineRequest,
) -> Result<AppScriptPipelineResponse, JsRuntimeError> {
    let loader = Rc::new(ResearchScriptResourceLoader::default());

    for (url, source) in request.external_assets {
        loader.register_script(url, source);
    }

    let hooks = Rc::new(ResearchReflowHooks::default());

    let config = ScriptPipelineConfig {
        async_policy: request.async_policy,
        continue_on_error: request.continue_on_error,
        ..ScriptPipelineConfig::default()
    };

    let scripts = request
        .scripts
        .into_iter()
        .map(app_script_to_descriptor)
        .collect::<Vec<_>>();

    let run = run_research_script_pipeline(scheduled, scripts, loader, hooks.clone(), config)?;

    Ok(response_from_run(run, hooks))
}

/// Executes a self-contained script pipeline with a fresh ScheduledVm.
pub(crate) fn execute_standalone_app_script_pipeline(
    request: AppScriptPipelineRequest,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<AppScriptPipelineResponse, JsRuntimeError> {
    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);
    execute_app_script_pipeline(&mut scheduled, request)
}

/// Creates a shared loader from app assets.
pub(crate) fn create_app_script_loader(
    assets: impl IntoIterator<Item = (String, String)>,
) -> SharedScriptResourceLoader {
    let loader = Rc::new(ResearchScriptResourceLoader::default());
    for (url, source) in assets {
        loader.register_script(url, source);
    }
    loader
}

/// Creates default app reflow hooks.
pub(crate) fn create_app_reflow_hooks() -> SharedScriptPipelineHooks {
    Rc::new(ResearchReflowHooks::default())
}

fn app_script_to_descriptor(input: AppScriptPipelineInput) -> ScriptDescriptor {
    ScriptDescriptor {
        id: input.id,
        label: input.label,
        kind: input.kind,
        mode: input.mode,
        source: match input.source {
            AppScriptPipelineSource::Inline(source) => ScriptSource::Inline { source },
            AppScriptPipelineSource::External(url) => ScriptSource::External { url },
        },
        parser_inserted: input.mode != ScriptLoadMode::Dynamic,
        render_blocking: input.mode == ScriptLoadMode::ParserBlocking,
        nonce: None,
        integrity: None,
    }
}

fn response_from_run(
    run: ScriptPipelineRun,
    hooks: Rc<ResearchReflowHooks>,
) -> AppScriptPipelineResponse {
    AppScriptPipelineResponse {
        metrics: run.metrics,
        events: run.events,
        console: run
            .last_summary
            .as_ref()
            .map_or_else(Vec::new, |summary| summary.console.clone()),
        failures: run.failures.len(),
        reflow_requests: hooks.reflow_requests().len(),
    }
}

/// Utility for quickly creating inline app scripts.
pub(crate) fn inline_app_script(
    id: u64,
    label: impl Into<String>,
    source: impl Into<String>,
    mode: ScriptLoadMode,
) -> AppScriptPipelineInput {
    AppScriptPipelineInput {
        id,
        label: label.into(),
        source: AppScriptPipelineSource::Inline(source.into()),
        mode,
        kind: ScriptKind::Classic,
    }
}

/// Utility for quickly creating external app scripts.
pub(crate) fn external_app_script(
    id: u64,
    label: impl Into<String>,
    url: impl Into<String>,
    mode: ScriptLoadMode,
) -> AppScriptPipelineInput {
    AppScriptPipelineInput {
        id,
        label: label.into(),
        source: AppScriptPipelineSource::External(url.into()),
        mode,
        kind: ScriptKind::Classic,
    }
}
