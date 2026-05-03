//! App bridge for SylJS isolated Worker VM + importScripts.
//!
//! Module 37 replaces Worker-lite echo-only behavior with isolated SylJS worker
//! execution contexts. This bridge provides a stable wrapper for registering
//! worker scripts and running page scripts against the Worker API.

use std::rc::Rc;

use syljs::{
    install_worker_globals, EventLoopConfig, EventLoopRunSummary, JsRuntimeError, MessageRecord,
    ProgramKind, ResearchWorkerHost, ScheduledVm, SharedWorkerHost, VmConfig, WorkerEventRecord,
    WorkerExecutionRecord, WorkerMetrics,
};

/// Worker script registration.
#[derive(Debug, Clone)]
pub(crate) struct WorkerScriptAsset {
    /// URL used by `new Worker(url)` or `importScripts(url)`.
    pub url: String,

    /// Script source.
    pub source: String,
}

/// Worker-bound script input.
#[derive(Debug, Clone)]
pub(crate) struct IsolatedWorkerBoundSylJsScript {
    /// Script label.
    pub label: String,

    /// Script source.
    pub source: String,

    /// Script kind.
    pub kind: ProgramKind,
}

/// Worker-bound execution result.
#[derive(Debug, Clone)]
pub(crate) struct IsolatedWorkerBoundSylJsResult {
    /// Event loop / VM summary.
    pub summary: EventLoopRunSummary,

    /// Failed main script labels.
    pub failed_scripts: Vec<String>,

    /// Worker metrics.
    pub worker_metrics: WorkerMetrics,

    /// Worker message records.
    pub messages: Vec<MessageRecord>,

    /// Worker main-thread event records.
    pub events: Vec<WorkerEventRecord>,

    /// Worker VM execution records.
    pub execution_records: Vec<WorkerExecutionRecord>,
}

/// Creates the default app-local isolated worker host.
pub(crate) fn create_app_isolated_worker_host(
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Rc<ResearchWorkerHost> {
    Rc::new(ResearchWorkerHost::new(vm_config, event_loop_config))
}

/// Registers worker scripts in a host.
pub(crate) fn register_worker_script_assets(
    host: &ResearchWorkerHost,
    assets: impl IntoIterator<Item = WorkerScriptAsset>,
) {
    for asset in assets {
        host.register_script(asset.url, asset.source);
    }
}

/// Executes scripts with isolated Worker globals installed.
pub(crate) fn execute_isolated_worker_bound_syljs_scripts<I>(
    scripts: I,
    worker_host: SharedWorkerHost,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<IsolatedWorkerBoundSylJsResult, JsRuntimeError>
where
    I: IntoIterator<Item = IsolatedWorkerBoundSylJsScript>,
{
    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);
    install_worker_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        worker_host.clone(),
    );

    let mut failed_scripts = Vec::new();

    for script in scripts {
        let parsed = match script.kind {
            ProgramKind::Script => syljs::parse_script(&script.source),
            ProgramKind::Module => syljs::parse_module(&script.source),
        };

        let result = parsed
            .map_err(JsRuntimeError::from_frontend_error)
            .and_then(|program| syljs::compile_program(&program, Default::default()).map_err(Into::into))
            .and_then(|bytecode| scheduled.vm.execute(&bytecode));

        if let Err(error) = result {
            tracing::warn!(
                label = %script.label,
                error = %error,
                "failed to execute isolated-worker-bound SylJS script"
            );
            failed_scripts.push(script.label);
        }
    }

    let summary = scheduled.run_until_idle()?;

    Ok(IsolatedWorkerBoundSylJsResult {
        summary,
        failed_scripts,
        worker_metrics: worker_host.metrics(),
        messages: worker_host.messages(),
        events: worker_host.events(),
        execution_records: worker_host.execution_records(),
    })
}
