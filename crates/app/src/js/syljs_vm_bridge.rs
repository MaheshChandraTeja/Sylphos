//! SylJS bytecode VM bridge for the browser script pipeline.
//!
//! Module 29 compiles parsed SylJS scripts to bytecode and executes them inside
//! a research VM. This bridge intentionally stays thin: host DOM bindings remain
//! in later modules, while this layer provides console capture, metrics, and
//! safe execution budgets.

use syljs::{
    compile_program, parse_module, parse_script, CompileOptions, ExecutionOutcome, JsRuntimeError,
    ProgramKind, Vm, VmConfig,
};
use tracing::{debug, warn};

/// One script execution input.
#[derive(Debug, Clone)]
pub(crate) struct SylJsScriptInput {
    /// Source label, usually URL or inline-script id.
    pub label: String,

    /// Script source.
    pub source: String,

    /// Script/module mode.
    pub kind: ProgramKind,
}

/// One script execution result.
#[derive(Debug, Clone)]
pub(crate) struct SylJsScriptExecution {
    /// Source label.
    pub label: String,

    /// Execution outcome if successful.
    pub outcome: Option<ExecutionOutcome>,

    /// Error message if failed.
    pub error: Option<String>,
}

/// Batch execution summary.
#[derive(Debug, Clone, Default)]
pub(crate) struct SylJsVmSummary {
    /// Scripts attempted.
    pub attempted: usize,

    /// Scripts successfully executed.
    pub executed: usize,

    /// Scripts failed during parse/compile/runtime.
    pub failed: usize,

    /// Total bytecode instructions executed.
    pub instructions_executed: u64,

    /// Total function calls.
    pub calls: u64,

    /// Total native calls.
    pub native_calls: u64,

    /// Total bytecode calls.
    pub bytecode_calls: u64,

    /// Total property reads.
    pub property_reads: u64,

    /// Total property writes.
    pub property_writes: u64,

    /// Captured console lines.
    pub console_lines: usize,
}

impl SylJsVmSummary {
    /// Compact log string.
    #[must_use]
    pub(crate) fn as_log_string(&self) -> String {
        format!(
            "attempted={} executed={} failed={} instructions={} calls={} native_calls={} bytecode_calls={} property_reads={} property_writes={} console_lines={}",
            self.attempted,
            self.executed,
            self.failed,
            self.instructions_executed,
            self.calls,
            self.native_calls,
            self.bytecode_calls,
            self.property_reads,
            self.property_writes,
            self.console_lines
        )
    }

    fn record_success(&mut self, outcome: &ExecutionOutcome) {
        self.attempted = self.attempted.saturating_add(1);
        self.executed = self.executed.saturating_add(1);
        self.instructions_executed = self
            .instructions_executed
            .saturating_add(outcome.metrics.instructions_executed);
        self.calls = self.calls.saturating_add(outcome.metrics.calls);
        self.native_calls = self.native_calls.saturating_add(outcome.metrics.native_calls);
        self.bytecode_calls = self
            .bytecode_calls
            .saturating_add(outcome.metrics.bytecode_calls);
        self.property_reads = self
            .property_reads
            .saturating_add(outcome.metrics.property_reads);
        self.property_writes = self
            .property_writes
            .saturating_add(outcome.metrics.property_writes);
        self.console_lines = self.console_lines.saturating_add(outcome.console.len());
    }

    fn record_failure(&mut self) {
        self.attempted = self.attempted.saturating_add(1);
        self.failed = self.failed.saturating_add(1);
    }
}

/// Executes scripts in source order using one shared VM.
///
/// Later DOM-binding modules can install host objects into the VM before this
/// call. For Module 29, this gives you arithmetic/control-flow/function runtime
/// behavior plus console capture and metrics.
pub(crate) fn execute_syljs_scripts<I>(
    scripts: I,
    config: VmConfig,
) -> (Vec<SylJsScriptExecution>, SylJsVmSummary)
where
    I: IntoIterator<Item = SylJsScriptInput>,
{
    let mut vm = Vm::with_config(config);
    let mut results = Vec::new();
    let mut summary = SylJsVmSummary::default();

    for script in scripts {
        let result = execute_one(&mut vm, &script);

        match &result.outcome {
            Some(outcome) => {
                summary.record_success(outcome);
                debug!(
                    label = %script.label,
                    instructions = outcome.metrics.instructions_executed,
                    calls = outcome.metrics.calls,
                    console_lines = outcome.console.len(),
                    "SylJS VM executed script"
                );
            }
            None => {
                summary.record_failure();
                warn!(
                    label = %script.label,
                    error = result.error.as_deref().unwrap_or("unknown"),
                    "SylJS VM failed script"
                );
            }
        }

        results.push(result);
    }

    (results, summary)
}

fn execute_one(vm: &mut Vm, script: &SylJsScriptInput) -> SylJsScriptExecution {
    let parsed = match script.kind {
        ProgramKind::Script => parse_script(&script.source),
        ProgramKind::Module => parse_module(&script.source),
    };

    let outcome = parsed
        .map_err(JsRuntimeError::from_frontend_error)
        .and_then(|program| compile_program(&program, CompileOptions::default()).map_err(Into::into))
        .and_then(|bytecode| vm.execute(&bytecode));

    match outcome {
        Ok(outcome) => SylJsScriptExecution {
            label: script.label.clone(),
            outcome: Some(outcome),
            error: None,
        },
        Err(error) => SylJsScriptExecution {
            label: script.label.clone(),
            outcome: None,
            error: Some(error.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_script_batch() {
        let scripts = vec![
            SylJsScriptInput {
                label: "inline:1".to_owned(),
                source: "function add(a,b){ return a + b; }".to_owned(),
                kind: ProgramKind::Script,
            },
            SylJsScriptInput {
                label: "inline:2".to_owned(),
                source: "console.log(add(2, 3));".to_owned(),
                kind: ProgramKind::Script,
            },
        ];

        let (_results, summary) = execute_syljs_scripts(scripts, VmConfig::default());

        assert_eq!(summary.attempted, 2);
        assert_eq!(summary.executed, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.console_lines, 1);
    }
}
