//! App bridge for SylJS media simulation.
//!
//! Module 34 gives SylJS a research-friendly MediaSource/video simulation layer.
//! This bridge installs those globals beside the existing DOM/CSSOM/Web API
//! stack. It is intentionally additive so the app shell does not get flattened
//! by yet another module zip, because apparently software needs self-preservation
//! instincts now.

use std::rc::Rc;

use syljs::{
    install_media_globals, EventLoopConfig, EventLoopRunSummary, JsRuntimeError, MediaEventRecord,
    MediaMetrics, MediaSegmentRecord, ProgramKind, ResearchMediaHost, ScheduledVm, SharedDomHost,
    SharedMediaHost, VmConfig,
};

/// Media-bound script input.
#[derive(Debug, Clone)]
pub(crate) struct MediaBoundSylJsScript {
    /// Script label.
    pub label: String,

    /// Script source.
    pub source: String,

    /// Script kind.
    pub kind: ProgramKind,
}

/// Media-bound execution result.
#[derive(Debug, Clone)]
pub(crate) struct MediaBoundSylJsResult {
    /// Event loop / VM summary.
    pub summary: EventLoopRunSummary,

    /// Failed script labels.
    pub failed_scripts: Vec<String>,

    /// Media metrics.
    pub media_metrics: MediaMetrics,

    /// Media event records.
    pub media_events: Vec<MediaEventRecord>,

    /// SourceBuffer segment records.
    pub media_segments: Vec<MediaSegmentRecord>,
}

/// Executes scripts with media globals installed.
///
/// Pass `dom_host` when `document.createElement("video")` should return a
/// DOM-backed media element. Without it, `new HTMLVideoElement()` still works,
/// but DOM append/query behavior obviously cannot, because reality remains
/// annoyingly causal.
pub(crate) fn execute_media_bound_syljs_scripts<I>(
    scripts: I,
    media_host: SharedMediaHost,
    dom_host: Option<SharedDomHost>,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<MediaBoundSylJsResult, JsRuntimeError>
where
    I: IntoIterator<Item = MediaBoundSylJsScript>,
{
    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);

    if let Some(dom_host) = dom_host.clone() {
        syljs::install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom_host);
    }

    install_media_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        media_host.clone(),
        dom_host,
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
                "failed to execute media-bound SylJS script"
            );
            failed_scripts.push(script.label);
        }
    }

    let summary = scheduled.run_until_idle()?;

    Ok(MediaBoundSylJsResult {
        summary,
        failed_scripts,
        media_metrics: media_host.metrics(),
        media_events: media_host.events(),
        media_segments: media_host.segments(),
    })
}

/// Creates the default app-local media simulation host.
pub(crate) fn create_app_research_media_host() -> Rc<ResearchMediaHost> {
    Rc::new(ResearchMediaHost::default())
}
