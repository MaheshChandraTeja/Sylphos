//! SylJS parser bridge for the app-side JavaScript pipeline.
//!
//! Module 28 intentionally adds only the JavaScript frontend: lexer, parser,
//! AST, and metrics. Execution still belongs to the next SylJS VM module.
//! This bridge lets the existing script loader/runtime collect parse metrics
//! without coupling browser code directly to parser internals.

use syljs::{parse_module, parse_script, AstStats, Program, ProgramKind};
use tracing::{debug, warn};

/// Parsed script artifact emitted by the SylJS frontend.
#[derive(Debug, Clone)]
pub(crate) struct ParsedSylJsScript {
    /// Script/module mode.
    pub kind: ProgramKind,

    /// Source label, usually URL or inline script id.
    pub label: String,

    /// Parsed AST.
    pub program: Program,

    /// AST statistics useful for research metrics.
    pub stats: AstStats,
}

/// Frontend parse summary.
#[derive(Debug, Clone, Default)]
pub(crate) struct SylJsParseSummary {
    /// Number of scripts attempted.
    pub attempted: usize,

    /// Number of scripts parsed successfully.
    pub parsed: usize,

    /// Number of scripts that failed to parse.
    pub failed: usize,

    /// Total statements.
    pub statements: usize,

    /// Total expressions.
    pub expressions: usize,

    /// Total function declarations/expressions.
    pub functions: usize,

    /// Total call expressions.
    pub calls: usize,

    /// Total member expressions.
    pub member_accesses: usize,

    /// Total assignment expressions.
    pub assignments: usize,
}

impl SylJsParseSummary {
    /// Records one parsed program.
    pub(crate) fn record_success(&mut self, stats: AstStats) {
        self.attempted = self.attempted.saturating_add(1);
        self.parsed = self.parsed.saturating_add(1);
        self.statements = self.statements.saturating_add(stats.statements);
        self.expressions = self.expressions.saturating_add(stats.expressions);
        self.functions = self.functions.saturating_add(stats.functions);
        self.calls = self.calls.saturating_add(stats.calls);
        self.member_accesses = self.member_accesses.saturating_add(stats.member_accesses);
        self.assignments = self.assignments.saturating_add(stats.assignments);
    }

    /// Records one parse failure.
    pub(crate) fn record_failure(&mut self) {
        self.attempted = self.attempted.saturating_add(1);
        self.failed = self.failed.saturating_add(1);
    }

    /// Compact summary string for logs and paper diagnostics.
    #[must_use]
    pub(crate) fn as_log_string(&self) -> String {
        format!(
            "attempted={} parsed={} failed={} statements={} expressions={} functions={} calls={} members={} assignments={}",
            self.attempted,
            self.parsed,
            self.failed,
            self.statements,
            self.expressions,
            self.functions,
            self.calls,
            self.member_accesses,
            self.assignments
        )
    }
}

/// Parses one script source using SylJS.
///
/// This is non-fatal by design. A production browser must survive script parse
/// failures without taking the page down, and a research browser should record
/// the failure rather than hiding it like a tiny academic goblin.
pub(crate) fn parse_syljs_script(
    label: impl Into<String>,
    source: &str,
    kind: ProgramKind,
) -> Option<ParsedSylJsScript> {
    let label = label.into();
    let result = match kind {
        ProgramKind::Script => parse_script(source),
        ProgramKind::Module => parse_module(source),
    };

    match result {
        Ok(program) => {
            let stats = AstStats::collect(&program);
            debug!(
                label = %label,
                kind = ?kind,
                statements = stats.statements,
                expressions = stats.expressions,
                functions = stats.functions,
                calls = stats.calls,
                member_accesses = stats.member_accesses,
                assignments = stats.assignments,
                "SylJS parsed script"
            );

            Some(ParsedSylJsScript {
                kind,
                label,
                program,
                stats,
            })
        }
        Err(error) => {
            warn!(
                label = %label,
                error = %error,
                diagnostics = error.diagnostics.len(),
                "SylJS parse failed"
            );
            None
        }
    }
}

/// Parses a batch of scripts and returns successfully parsed artifacts plus metrics.
pub(crate) fn parse_syljs_scripts<I>(scripts: I) -> (Vec<ParsedSylJsScript>, SylJsParseSummary)
where
    I: IntoIterator<Item = (String, String, ProgramKind)>,
{
    let mut parsed = Vec::new();
    let mut summary = SylJsParseSummary::default();

    for (label, source, kind) in scripts {
        if let Some(script) = parse_syljs_script(label, &source, kind) {
            summary.record_success(script.stats);
            parsed.push(script);
        } else {
            summary.record_failure();
        }
    }

    (parsed, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_batch_and_reports_summary() {
        let scripts = vec![
            (
                "inline:1".to_owned(),
                "document.title = 'Changed';".to_owned(),
                ProgramKind::Script,
            ),
            (
                "inline:2".to_owned(),
                "function x(){ return 1; }".to_owned(),
                ProgramKind::Script,
            ),
        ];

        let (parsed, summary) = parse_syljs_scripts(scripts);

        assert_eq!(parsed.len(), 2);
        assert_eq!(summary.parsed, 2);
        assert_eq!(summary.failed, 0);
        assert!(summary.assignments >= 1);
        assert!(summary.functions >= 1);
    }
}
