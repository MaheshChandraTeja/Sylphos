//! App bridge for SylJS Canvas 2D and Worker-lite.
//!
//! Module 35 adds Canvas 2D command recording and Worker-lite message scheduling
//! to SylJS. This bridge is additive and can be installed beside DOM, CSSOM,
//! Web API, and media bridges. Mercifully, it does not try to decode pixels or
//! spawn OS threads yet, because a research browser still deserves a spine.

use std::rc::Rc;

use syljs::{
    install_canvas_globals, install_worker_globals, CanvasCommand, CanvasMetrics, EventLoopConfig,
    EventLoopRunSummary, JsRuntimeError, ProgramKind, ResearchCanvasHost, ResearchWorkerHost,
    ScheduledVm, SharedCanvasHost, SharedDomHost, SharedWorkerHost, VmConfig, WorkerEventRecord,
    WorkerMetrics,
};

/// Canvas/Worker-bound script input.
#[derive(Debug, Clone)]
pub(crate) struct CanvasWorkerBoundSylJsScript {
    /// Script label.
    pub label: String,

    /// Script source.
    pub source: String,

    /// Script kind.
    pub kind: ProgramKind,
}

/// Canvas/Worker-bound execution result.
#[derive(Debug, Clone)]
pub(crate) struct CanvasWorkerBoundSylJsResult {
    /// Event loop / VM summary.
    pub summary: EventLoopRunSummary,

    /// Failed script labels.
    pub failed_scripts: Vec<String>,

    /// Canvas metrics.
    pub canvas_metrics: CanvasMetrics,

    /// Worker metrics.
    pub worker_metrics: WorkerMetrics,

    /// Canvas commands.
    pub canvas_commands: Vec<CanvasCommand>,

    /// Worker events.
    pub worker_events: Vec<WorkerEventRecord>,
}

/// Executes scripts with Canvas 2D and Worker-lite globals installed.
pub(crate) fn execute_canvas_worker_bound_syljs_scripts<I>(
    scripts: I,
    canvas_host: SharedCanvasHost,
    worker_host: SharedWorkerHost,
    dom_host: Option<SharedDomHost>,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<CanvasWorkerBoundSylJsResult, JsRuntimeError>
where
    I: IntoIterator<Item = CanvasWorkerBoundSylJsScript>,
{
    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);

    if let Some(dom_host) = dom_host.clone() {
        syljs::install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom_host);
    }

    install_canvas_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        canvas_host.clone(),
        dom_host,
    );
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
                "failed to execute Canvas/Worker-bound SylJS script"
            );
            failed_scripts.push(script.label);
        }
    }

    let summary = scheduled.run_until_idle()?;

    Ok(CanvasWorkerBoundSylJsResult {
        summary,
        failed_scripts,
        canvas_metrics: canvas_host.metrics(),
        worker_metrics: worker_host.metrics(),
        canvas_commands: canvas_host.commands(),
        worker_events: worker_host.events(),
    })
}

/// Creates the default app-local canvas host.
pub(crate) fn create_app_research_canvas_host() -> Rc<ResearchCanvasHost> {
    Rc::new(ResearchCanvasHost::default())
}

/// Creates the default app-local worker host.
pub(crate) fn create_app_research_worker_host() -> Rc<ResearchWorkerHost> {
    Rc::new(ResearchWorkerHost::default())
}
