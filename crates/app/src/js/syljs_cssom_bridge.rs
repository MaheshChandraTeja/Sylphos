//! App bridge for SylJS CSSOM.
//!
//! This module connects the `syljs::CssomHost` abstraction to app-side script
//! execution. It is intentionally additive: install it alongside the existing
//! DOM bridge when you want `element.style`, `getComputedStyle`, and
//! `document.styleSheets` for script execution.

use std::rc::Rc;

use syljs::{
    install_cssom_globals, CssStyleMutation, CssomHost, CssomMetrics, EventLoopConfig,
    EventLoopRunSummary, JsRuntimeError, ProgramKind, ResearchCssomHost, ScheduledVm,
    SharedCssomHost, SharedDomHost, VmConfig,
};

/// CSSOM-bound script input.
#[derive(Debug, Clone)]
pub(crate) struct CssomBoundSylJsScript {
    /// Script label.
    pub label: String,

    /// Script source.
    pub source: String,

    /// Script kind.
    pub kind: ProgramKind,
}

/// CSSOM-bound execution result.
#[derive(Debug, Clone)]
pub(crate) struct CssomBoundSylJsResult {
    /// Event-loop/VM summary.
    pub summary: EventLoopRunSummary,

    /// Failed script labels.
    pub failed_scripts: Vec<String>,

    /// CSSOM metrics.
    pub cssom_metrics: CssomMetrics,

    /// Style mutations observed during execution.
    pub style_mutations: Vec<CssStyleMutation>,
}

/// Executes scripts with CSSOM globals installed.
///
/// Pass the same DOM host used by your DOM bridge so `getComputedStyle(element)`
/// and `element.style` resolve against the same node identities.
pub(crate) fn execute_cssom_bound_syljs_scripts<I>(
    scripts: I,
    dom_host: SharedDomHost,
    cssom_host: SharedCssomHost,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<CssomBoundSylJsResult, JsRuntimeError>
where
    I: IntoIterator<Item = CssomBoundSylJsScript>,
{
    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);

    syljs::install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom_host.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom_host,
        cssom_host.clone(),
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
                "failed to execute CSSOM-bound SylJS script"
            );
            failed_scripts.push(script.label);
        }
    }

    let summary = scheduled.run_until_idle()?;

    Ok(CssomBoundSylJsResult {
        summary,
        failed_scripts,
        cssom_metrics: cssom_host.metrics(),
        style_mutations: cssom_host.mutations(),
    })
}

/// Creates the default CSSOM host used by app-local fixtures.
pub(crate) fn create_app_research_cssom_host() -> Rc<ResearchCssomHost> {
    Rc::new(ResearchCssomHost::new())
}
