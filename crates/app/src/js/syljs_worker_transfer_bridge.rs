//! App bridge for Module 38 Worker Web APIs + Transferable ArrayBuffer runtime.
//!
//! This bridge exposes a clean app-facing API for creating a combined Web API,
//! transfer, and isolated-worker execution environment. The runtime is still
//! deterministic and research-grade; it is not trying to become Chromium in a
//! suspiciously small trench coat.

use std::rc::Rc;

use syljs::{
    install_transfer_globals, install_web_api_globals, install_worker_globals, EventLoopConfig,
    EventLoopRunSummary, JsRuntimeError, ProgramKind, ResearchTransferHost, ResearchWebApiHost,
    ResearchWorkerHost, ScheduledVm, TransferMetrics, VmConfig, WebApiMetrics, WorkerMetrics,
};

/// Script asset used by Worker/importScripts.
#[derive(Debug, Clone)]
pub(crate) struct WorkerRuntimeScriptAsset {
    /// URL.
    pub url: String,

    /// JS source.
    pub source: String,
}

/// Page script input.
#[derive(Debug, Clone)]
pub(crate) struct WorkerWebApiTransferScript {
    /// Label.
    pub label: String,

    /// Source.
    pub source: String,

    /// Script kind.
    pub kind: ProgramKind,
}

/// Combined runtime result.
#[derive(Debug, Clone)]
pub(crate) struct WorkerWebApiTransferResult {
    /// Event-loop summary.
    pub summary: EventLoopRunSummary,

    /// Failed script labels.
    pub failed_scripts: Vec<String>,

    /// Main Web API metrics.
    pub web_api_metrics: WebApiMetrics,

    /// Transfer metrics.
    pub transfer_metrics: TransferMetrics,

    /// Worker metrics.
    pub worker_metrics: WorkerMetrics,
}

/// Combined deterministic host bundle.
#[derive(Debug, Clone)]
pub(crate) struct WorkerWebApiTransferHosts {
    /// Main Web API host.
    pub web: Rc<ResearchWebApiHost>,

    /// Transfer host.
    pub transfer: Rc<ResearchTransferHost>,

    /// Worker host.
    pub workers: Rc<ResearchWorkerHost>,
}

/// Creates the default host bundle.
pub(crate) fn create_worker_webapi_transfer_hosts(
    base_url: &str,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> WorkerWebApiTransferHosts {
    WorkerWebApiTransferHosts {
        web: Rc::new(ResearchWebApiHost::new(base_url)),
        transfer: Rc::new(ResearchTransferHost::default()),
        workers: Rc::new(ResearchWorkerHost::new(vm_config, event_loop_config)),
    }
}

/// Registers worker scripts.
pub(crate) fn register_worker_runtime_script_assets(
    workers: &ResearchWorkerHost,
    assets: impl IntoIterator<Item = WorkerRuntimeScriptAsset>,
) {
    for asset in assets {
        workers.register_script(asset.url, asset.source);
    }
}

/// Executes scripts with Web APIs, transfer globals, and Worker globals installed.
pub(crate) fn execute_worker_webapi_transfer_scripts<I>(
    scripts: I,
    hosts: WorkerWebApiTransferHosts,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<WorkerWebApiTransferResult, JsRuntimeError>
where
    I: IntoIterator<Item = WorkerWebApiTransferScript>,
{
    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);

    install_web_api_globals(&mut scheduled.vm, scheduled.event_loop.clone(), hosts.web.clone());
    install_transfer_globals(&mut scheduled.vm, hosts.transfer.clone());
    install_worker_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        hosts.workers.clone(),
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
                "failed to execute Worker/WebAPI/Transfer SylJS script"
            );
            failed_scripts.push(script.label);
        }
    }

    let summary = scheduled.run_until_idle()?;

    Ok(WorkerWebApiTransferResult {
        summary,
        failed_scripts,
        web_api_metrics: hosts.web.metrics(),
        transfer_metrics: hosts.transfer.metrics(),
        worker_metrics: hosts.workers.metrics(),
    })
}
