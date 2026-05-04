#![allow(clippy::too_many_arguments)]

//! Script discovery, loading, and source-order execution.

use anyhow::{Context, Result};
use html_mvp::dom::Element;
use html_mvp::{Document, Node};
use present::RenderDocument;
use tracing::{debug, warn};
use url::Url;

use crate::browser::{CacheSource, ResourceRequest, ResourceScheduler};
use crate::js::{
    capture_service_worker_effects, BrowserEventLoop, JavaScriptRuntime, MediaCanvasWorkerHost,
    MediaCanvasWorkerSummary, ScriptProgram, ServiceWorkerHost, ServiceWorkerSummary,
    WebPlatformHost, WebPlatformSummary,
};

const MAX_INLINE_SCRIPT_BYTES: usize = 512 * 1024;
const MAX_EXTERNAL_SCRIPT_BYTES: usize = 2 * 1024 * 1024;
const MAX_SCRIPTS_PER_PAGE: usize = 96;

/// Script kind discovered from HTML attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptKind {
    /// Classic JavaScript script.
    Classic,

    /// ES module script. Discovery is supported; execution is deferred.
    Module,

    /// Unsupported script type.
    Unsupported,
}

impl ScriptKind {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Module => "module",
            Self::Unsupported => "unsupported",
        }
    }
}

/// A script source discovered in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptSource {
    /// Script kind.
    pub kind: ScriptKind,

    /// External `src`, if present.
    pub src: Option<String>,

    /// Inline body text, if present.
    pub inline_code: Option<String>,

    /// Source-order index.
    pub source_order: usize,

    /// Whether the script had `async`.
    pub async_attr: bool,

    /// Whether the script had `defer`.
    pub defer_attr: bool,
}

impl ScriptSource {
    #[must_use]
    fn is_executable_now(&self) -> bool {
        self.kind == ScriptKind::Classic
    }
}

/// Summary of one document script execution pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScriptExecutionSummary {
    /// Script tags discovered.
    pub discovered: usize,

    /// Classic scripts selected for execution.
    pub executable: usize,

    /// Inline scripts executed.
    pub inline_executed: usize,

    /// External scripts fetched and executed.
    pub external_executed: usize,

    /// Script tags skipped due to unsupported type, size, limits, or fetch failure.
    pub skipped: usize,

    /// Fatal execution failures.
    pub failed: usize,

    /// Fetched script bytes.
    pub bytes: usize,

    /// Captured console message count.
    pub console_messages: usize,

    /// Runtime warning count.
    pub warnings: usize,

    /// Runtime error count.
    pub errors: usize,

    /// Runtime title override, if any.
    pub title_override: Option<String>,

    /// Navigation requests emitted by script.
    pub navigation_requests: Vec<String>,

    /// Memory-cache hits for external scripts.
    pub memory_hits: usize,

    /// Disk-cache hits for external scripts.
    pub disk_hits: usize,

    /// Network fetches for external scripts.
    pub network_fetches: usize,

    /// Disabled-cache external fetches.
    pub disabled_fetches: usize,

    /// Browser event-loop tasks queued by script execution.
    pub event_tasks_queued: usize,

    /// Browser event-loop tasks executed by script execution.
    pub event_tasks_executed: usize,

    /// Browser microtasks queued by script execution.
    pub microtasks_queued: usize,

    /// Browser microtasks executed by script execution.
    pub microtasks_executed: usize,

    /// DOM binding mutations applied to the render document.
    pub dom_mutations: usize,

    /// DOM binding effects ignored because no target matched.
    pub dom_ignored: usize,

    /// Event listeners registered by script.
    pub registered_listeners: usize,

    /// Script-originated event dispatches recorded.
    pub dispatched_events: usize,

    /// Web Platform API host summary.
    pub web_platform: WebPlatformSummary,

    /// Media/canvas/worker host summary.
    pub media: MediaCanvasWorkerSummary,

    /// Service Worker and Cache API host summary.
    pub service_worker: ServiceWorkerSummary,
}

/// Discovers, loads, and executes document scripts in conservative source order.
pub(crate) async fn execute_document_scripts(
    parsed_document: &Document,
    render_document: &mut RenderDocument,
    scheduler: &ResourceScheduler,
    base_url: &str,
    navigation_id: Option<u64>,
    web_platform: &mut WebPlatformHost,
    media_host: &mut MediaCanvasWorkerHost,
    service_worker: &mut ServiceWorkerHost,
) -> ScriptExecutionSummary {
    let scripts = collect_script_sources(parsed_document);
    let mut summary = ScriptExecutionSummary {
        discovered: scripts.len(),
        ..ScriptExecutionSummary::default()
    };

    if scripts.is_empty() {
        return summary;
    }

    let mut runtime = JavaScriptRuntime::new(base_url.to_owned());
    let mut event_loop = BrowserEventLoop::new(base_url.to_owned());

    for script in scripts.into_iter().take(MAX_SCRIPTS_PER_PAGE) {
        if !script.is_executable_now() {
            summary.skipped = summary.skipped.saturating_add(1);
            debug!(
                kind = script.kind.as_str(),
                source_order = script.source_order,
                "skipped non-classic script"
            );
            continue;
        }

        summary.executable = summary.executable.saturating_add(1);

        if let Some(src) = &script.src {
            match execute_external_script(
                src,
                &script,
                &mut runtime,
                scheduler,
                base_url,
                navigation_id,
                &mut summary,
                render_document,
                &mut event_loop,
                web_platform,
                media_host,
                service_worker,
            )
            .await
            {
                Ok(()) => {}
                Err(error) => {
                    summary.failed = summary.failed.saturating_add(1);
                    warn!(src = %src, error = %error, "failed to execute external script");
                }
            }
            continue;
        }

        if let Some(code) = &script.inline_code {
            if code.len() > MAX_INLINE_SCRIPT_BYTES {
                summary.skipped = summary.skipped.saturating_add(1);
                warn!(
                    bytes = code.len(),
                    limit = MAX_INLINE_SCRIPT_BYTES,
                    "skipped oversized inline script"
                );
                continue;
            }

            let program = ScriptProgram::new(
                code.clone(),
                format!("inline-script:{}", script.source_order),
                script.source_order,
                false,
            );
            event_loop.queue_script_task(program.source_name.clone());
            let execution = runtime.execute(&program);
            apply_web_platform_effects(&execution, web_platform, scheduler, &mut summary).await;
            apply_media_effects(&execution, media_host, scheduler, &mut summary).await;
            apply_service_worker_effects(&program.source, service_worker, scheduler, &mut summary)
                .await;
            event_loop.after_script(&execution, render_document);
            summary.inline_executed = summary.inline_executed.saturating_add(1);
            record_execution(&execution, &mut summary);
        }
    }

    if summary.discovered > MAX_SCRIPTS_PER_PAGE {
        summary.skipped = summary
            .skipped
            .saturating_add(summary.discovered - MAX_SCRIPTS_PER_PAGE);
    }

    if let Some(title) = runtime.latest_title_override() {
        render_document.title = Some(title.clone());
        summary.title_override = Some(title);
    }

    let event_report = event_loop.drain_report();
    summary.event_tasks_queued = event_report.tasks_queued;
    summary.event_tasks_executed = event_report.tasks_executed;
    summary.microtasks_queued = event_report.microtasks_queued;
    summary.microtasks_executed = event_report.microtasks_executed;
    summary.dom_mutations = event_report.dom_mutations;
    summary.dom_ignored = event_report.dom_ignored;
    summary.registered_listeners = event_report.registered_listeners;
    summary.dispatched_events = event_report.dispatched_events;
    let binding_warning_count = event_report.diagnostics.len();

    summary.navigation_requests = runtime.navigation_requests();
    summary
        .navigation_requests
        .extend(summary.web_platform.navigation_requests.clone());
    summary.console_messages = runtime.console().len();
    summary.warnings = runtime
        .warnings()
        .len()
        .saturating_add(binding_warning_count)
        .saturating_add(summary.web_platform.warnings);
    summary.errors = runtime
        .errors()
        .len()
        .saturating_add(summary.web_platform.errors);

    debug!(
        runtime_url = %runtime.document_url(),
        executed = runtime.executed_count(),
        console_messages = summary.console_messages,
        warnings = summary.warnings,
        errors = summary.errors,
        dom_mutations = summary.dom_mutations,
        event_tasks = summary.event_tasks_executed,
        microtasks = summary.microtasks_executed,
        document_url = %event_loop.document_url(),
        "completed JavaScript script pass"
    );

    summary
}

#[allow(clippy::too_many_arguments)]
async fn execute_external_script(
    src: &str,
    script: &ScriptSource,
    runtime: &mut JavaScriptRuntime,
    scheduler: &ResourceScheduler,
    base_url: &str,
    navigation_id: Option<u64>,
    summary: &mut ScriptExecutionSummary,
    render_document: &mut RenderDocument,
    event_loop: &mut BrowserEventLoop,
    web_platform: &mut WebPlatformHost,
    media_host: &mut MediaCanvasWorkerHost,
    service_worker: &mut ServiceWorkerHost,
) -> Result<()> {
    let url = resolve_script_url(base_url, src)?;
    let mut request = ResourceRequest::script(url.clone()).max_bytes(MAX_EXTERNAL_SCRIPT_BYTES);
    if let Some(id) = navigation_id {
        request = request.navigation_id(id);
    }

    let resource = scheduler.fetch_text(request).await?;
    summary.bytes = summary.bytes.saturating_add(resource.bytes);
    record_cache_source(resource.source, summary);

    let program = ScriptProgram::new(resource.text, resource.url, script.source_order, true);
    event_loop.queue_script_task(program.source_name.clone());
    let execution = runtime.execute(&program);
    apply_web_platform_effects(&execution, web_platform, scheduler, summary).await;
    apply_media_effects(&execution, media_host, scheduler, summary).await;
    apply_service_worker_effects(&program.source, service_worker, scheduler, summary).await;
    event_loop.after_script(&execution, render_document);
    summary.external_executed = summary.external_executed.saturating_add(1);
    record_execution(&execution, summary);
    Ok(())
}

async fn apply_service_worker_effects(
    source: &str,
    service_worker: &mut ServiceWorkerHost,
    scheduler: &ResourceScheduler,
    summary: &mut ScriptExecutionSummary,
) {
    let capture = capture_service_worker_effects(source);
    if capture.effects.is_empty() {
        summary.service_worker.warnings = summary
            .service_worker
            .warnings
            .saturating_add(capture.warnings.len());
        return;
    }

    let report = service_worker
        .apply_effects(&capture.effects, scheduler)
        .await;
    summary.service_worker.merge_from(report);
    summary.service_worker.warnings = summary
        .service_worker
        .warnings
        .saturating_add(capture.warnings.len());
}

async fn apply_media_effects(
    execution: &crate::js::ScriptExecution,
    media_host: &mut MediaCanvasWorkerHost,
    scheduler: &ResourceScheduler,
    summary: &mut ScriptExecutionSummary,
) {
    if execution.media_effects.is_empty() {
        return;
    }

    let report = media_host
        .apply_effects(&execution.media_effects, scheduler)
        .await;
    summary.media.merge_from(report);
}

async fn apply_web_platform_effects(
    execution: &crate::js::ScriptExecution,
    web_platform: &mut WebPlatformHost,
    scheduler: &ResourceScheduler,
    summary: &mut ScriptExecutionSummary,
) {
    if execution.web_api_effects.is_empty() {
        return;
    }

    let report = web_platform
        .apply_effects(&execution.web_api_effects, scheduler)
        .await;
    summary.web_platform.merge_from(report);
}

fn record_execution(execution: &crate::js::ScriptExecution, summary: &mut ScriptExecutionSummary) {
    summary.console_messages = summary
        .console_messages
        .saturating_add(execution.console.len());
    if !execution.is_success() {
        summary.failed = summary.failed.saturating_add(1);
    }
    summary.warnings = summary.warnings.saturating_add(execution.warnings.len());
    summary.errors = summary.errors.saturating_add(execution.errors.len());
}

fn record_cache_source(source: CacheSource, summary: &mut ScriptExecutionSummary) {
    match source {
        CacheSource::Disabled => {
            summary.disabled_fetches = summary.disabled_fetches.saturating_add(1)
        }
        CacheSource::Network => summary.network_fetches = summary.network_fetches.saturating_add(1),
        CacheSource::Memory => summary.memory_hits = summary.memory_hits.saturating_add(1),
        CacheSource::Disk => summary.disk_hits = summary.disk_hits.saturating_add(1),
    }
}

fn collect_script_sources(document: &Document) -> Vec<ScriptSource> {
    let mut scripts = Vec::new();
    collect_script_sources_from_nodes(&document.children, &mut scripts);
    scripts
}

fn collect_script_sources_from_nodes(nodes: &[Node], scripts: &mut Vec<ScriptSource>) {
    for node in nodes {
        let Node::Element(element) = node else {
            continue;
        };

        if element.tag.eq_ignore_ascii_case("script") {
            let source_order = scripts.len();
            scripts.push(script_from_element(element, source_order));
            continue;
        }

        collect_script_sources_from_nodes(&element.children, scripts);
    }
}

fn script_from_element(element: &Element, source_order: usize) -> ScriptSource {
    let type_attr = attr_value(&element.attrs, "type")
        .or_else(|| attr_value(&element.attrs, "language"))
        .unwrap_or_default();
    let kind = classify_script_type(type_attr);
    let src = attr_value(&element.attrs, "src").map(ToOwned::to_owned);
    let inline_code = if src.is_some() {
        None
    } else {
        Some(collect_raw_text_from_nodes(&element.children))
    };

    ScriptSource {
        kind,
        src,
        inline_code,
        source_order,
        async_attr: has_attr(&element.attrs, "async"),
        defer_attr: has_attr(&element.attrs, "defer"),
    }
}

fn classify_script_type(value: &str) -> ScriptKind {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty()
        || matches!(
            trimmed.as_str(),
            "text/javascript"
                | "application/javascript"
                | "application/ecmascript"
                | "text/ecmascript"
                | "javascript"
                | "module"
        )
    {
        if trimmed == "module" {
            ScriptKind::Module
        } else {
            ScriptKind::Classic
        }
    } else {
        ScriptKind::Unsupported
    }
}

fn collect_raw_text_from_nodes(nodes: &[Node]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            Node::Text(text) | Node::Comment(text) => output.push_str(text),
            Node::Element(element) => {
                output.push_str(&collect_raw_text_from_nodes(&element.children))
            }
        }
    }
    output
}

fn attr_value<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case(name) {
            Some(value.as_str())
        } else {
            None
        }
    })
}

fn has_attr(attrs: &[(String, String)], name: &str) -> bool {
    attrs.iter().any(|(key, _)| key.eq_ignore_ascii_case(name))
}

fn resolve_script_url(base_url: &str, src: &str) -> Result<String> {
    let base = Url::parse(base_url).with_context(|| format!("invalid base URL `{base_url}`"))?;
    let resolved = base
        .join(src.trim())
        .with_context(|| format!("invalid script src `{src}`"))?;

    match resolved.scheme() {
        "http" | "https" => Ok(resolved.to_string()),
        scheme => anyhow::bail!("unsupported script URL scheme `{scheme}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_inline_and_external_scripts() {
        let parsed = match html_mvp::parse(
            r#"<html><head><script src="/a.js"></script></head><body><script>document.title='x';</script></body></html>"#,
        ) {
            Ok(doc) => doc,
            Err(error) => panic!("test document parses: {error}"),
        };
        let scripts = collect_script_sources(&parsed);
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts[0].src.as_deref(), Some("/a.js"));
        assert!(scripts[1]
            .inline_code
            .as_deref()
            .is_some_and(|code| code.contains("document.title")));
    }
}
