//! SylJS event-loop bridge for the browser script pipeline.
//!
//! Module 30 adds deterministic task/microtask/timer/Promise scheduling on top
//! of the SylJS VM. The app can use this bridge to execute loaded scripts in a
//! scheduled VM and report metrics without depending on VM internals.

use syljs::{
    compile_program, parse_module, parse_script, EventLoopConfig, EventLoopRunSummary,
    JsRuntimeError, ProgramKind, ScheduledVm, VmConfig,
};
use tracing::{debug, warn};

/// One scheduled script input.
#[derive(Debug, Clone)]
pub(crate) struct ScheduledSylJsScript {
    /// Script label, usually URL or inline index.
    pub label: String,

    /// Script source text.
    pub source: String,

    /// Script/module kind.
    pub kind: ProgramKind,
}

/// Scheduled execution result.
#[derive(Debug, Clone)]
pub(crate) struct ScheduledSylJsResult {
    /// Per-run summary after all scripts and event-loop jobs drain.
    pub summary: EventLoopRunSummary,

    /// Script labels that failed before/during execution.
    pub failed_scripts: Vec<String>,
}

/// Executes scripts in source order, then drains the SylJS event loop.
///
/// This is intentionally deterministic: virtual timers auto-advance by default,
/// so test results and research metrics are stable.
pub(crate) fn execute_scheduled_syljs_scripts<I>(
    scripts: I,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<ScheduledSylJsResult, JsRuntimeError>
where
    I: IntoIterator<Item = ScheduledSylJsScript>,
{
    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);
    let mut failed_scripts = Vec::new();

    for script in scripts {
        let parsed = match script.kind {
            ProgramKind::Script => parse_script(&script.source),
            ProgramKind::Module => parse_module(&script.source),
        };

        let result = parsed
            .map_err(JsRuntimeError::from_frontend_error)
            .and_then(|program| compile_program(&program, Default::default()).map_err(Into::into))
            .and_then(|bytecode| scheduled.vm.execute(&bytecode));

        match result {
            Ok(outcome) => {
                debug!(
                    label = %script.label,
                    instructions = outcome.metrics.instructions_executed,
                    calls = outcome.metrics.calls,
                    console_lines = outcome.console.len(),
                    "executed SylJS script before event-loop drain"
                );
            }
            Err(error) => {
                warn!(
                    label = %script.label,
                    error = %error,
                    "failed to execute scheduled SylJS script"
                );
                failed_scripts.push(script.label);
            }
        }
    }

    let summary = scheduled.run_until_idle()?;

    Ok(ScheduledSylJsResult {
        summary,
        failed_scripts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_executes_timers_and_promises() {
        let scripts = vec![ScheduledSylJsScript {
            label: "inline:1".to_owned(),
            source: r#"
                Promise.resolve("p").then(function (value) {
                    console.log(value);
                });
                setTimeout(function () {
                    console.log("t");
                }, 0);
                console.log("s");
            "#
            .to_owned(),
            kind: ProgramKind::Script,
        }];

        let result = execute_scheduled_syljs_scripts(
            scripts,
            VmConfig::default(),
            EventLoopConfig::default(),
        )
        .expect("execute");

        assert!(result.failed_scripts.is_empty());
        assert_eq!(result.summary.console, vec!["s", "p", "t"]);
    }
}
