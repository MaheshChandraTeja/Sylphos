#![allow(dead_code)]
//! DevTools trace harness for the Sylphos browser shell.
//!
//! Module 50 gives the app a deterministic trace spine for resource waterfalls,
//! JavaScript timelines, DOM/CSSOM changes, reflow/paint work, image/font/SVG
//! activity, Service Worker interception, security decisions, and site-compat
//! suite scoring.
//!
//! The exported JSON intentionally follows Chrome's trace-event shape where it
//! helps (`traceEvents` with `cat`, `ph`, `ts`, `dur`, `args`), while also
//! carrying Sylphos-specific summaries so humans do not have to mentally grep
//! four thousand log lines like medieval accountants.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const TRACE_SCHEMA_VERSION: u32 = 1;
const DEFAULT_MAX_EVENTS: usize = 250_000;
const DEFAULT_PROCESS_ID: u32 = 1;
const DEFAULT_MAIN_THREAD_ID: u32 = 1;

/// Stable identifier for an in-flight resource request in the trace harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct TraceResourceId(pub u64);

/// Trace categories used by the app.
///
/// Keep this enum intentionally broad. Individual events should use descriptive
/// names and structured args rather than multiplying categories until the trace
/// viewer becomes soup with icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TraceCategory {
    Navigation,
    Resource,
    Cache,
    Http,
    Security,
    ServiceWorker,
    Script,
    EventLoop,
    Dom,
    Cssom,
    Layout,
    Reflow,
    Paint,
    Image,
    Font,
    Svg,
    Accessibility,
    Compatibility,
    Runtime,
    Diagnostics,
}

impl TraceCategory {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Navigation => "navigation",
            Self::Resource => "resource",
            Self::Cache => "cache",
            Self::Http => "http",
            Self::Security => "security",
            Self::ServiceWorker => "service-worker",
            Self::Script => "script",
            Self::EventLoop => "event-loop",
            Self::Dom => "dom",
            Self::Cssom => "cssom",
            Self::Layout => "layout",
            Self::Reflow => "reflow",
            Self::Paint => "paint",
            Self::Image => "image",
            Self::Font => "font",
            Self::Svg => "svg",
            Self::Accessibility => "accessibility",
            Self::Compatibility => "compatibility",
            Self::Runtime => "runtime",
            Self::Diagnostics => "diagnostics",
        }
    }
}

/// Chrome trace phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TracePhase {
    /// Complete event with duration.
    Complete,

    /// Instant event.
    Instant,

    /// Counter/sample event.
    Counter,

    /// Metadata event.
    Metadata,
}

impl TracePhase {
    #[must_use]
    pub(crate) const fn as_chrome(self) -> &'static str {
        match self {
            Self::Complete => "X",
            Self::Instant => "i",
            Self::Counter => "C",
            Self::Metadata => "M",
        }
    }
}

/// A single trace event serialized in Chrome trace-event form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TraceEvent {
    /// Event name.
    pub name: String,

    /// Event category string.
    #[serde(rename = "cat")]
    pub category: String,

    /// Chrome trace phase.
    #[serde(rename = "ph")]
    pub phase: String,

    /// Timestamp in microseconds from trace start.
    #[serde(rename = "ts")]
    pub timestamp_us: u64,

    /// Duration in microseconds for complete events.
    #[serde(rename = "dur", skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<u64>,

    /// Process id.
    #[serde(rename = "pid")]
    pub process_id: u32,

    /// Thread id.
    #[serde(rename = "tid")]
    pub thread_id: u32,

    /// Scope for instant events.
    #[serde(rename = "s", skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Structured event arguments.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub args: Map<String, Value>,
}

/// Trace runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DevToolsTraceConfig {
    /// Whether the trace harness records events.
    pub enabled: bool,

    /// Browser/session label.
    pub session_label: String,

    /// Maximum retained events.
    pub max_events: usize,

    /// Optional output path used by integration helpers.
    pub output_path: Option<PathBuf>,

    /// Whether complete JSON reports include the full event array.
    pub include_events: bool,

    /// Whether resource records are included in the report.
    pub include_waterfall: bool,

    /// Whether a Chrome-compatible trace payload is emitted.
    pub include_chrome_trace: bool,
}

impl Default for DevToolsTraceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            session_label: "Sylphos".to_owned(),
            max_events: DEFAULT_MAX_EVENTS,
            output_path: None,
            include_events: true,
            include_waterfall: true,
            include_chrome_trace: true,
        }
    }
}

impl DevToolsTraceConfig {
    /// Builds trace config from environment variables.
    ///
    /// Supported:
    /// - `SYLPHOS_TRACE=1|true`
    /// - `SYLPHOS_TRACE_JSON=F:\trace\sylphos-trace.json`
    /// - `SYLPHOS_TRACE_MAX_EVENTS=50000`
    #[must_use]
    pub(crate) fn from_env() -> Self {
        let mut config = Self::default();
        config.enabled = env_flag("SYLPHOS_TRACE");

        if let Ok(path) = std::env::var("SYLPHOS_TRACE_JSON") {
            if !path.trim().is_empty() {
                config.enabled = true;
                config.output_path = Some(PathBuf::from(path));
            }
        }

        if let Ok(value) = std::env::var("SYLPHOS_TRACE_MAX_EVENTS") {
            if let Ok(max_events) = value.trim().parse::<usize>() {
                config.max_events = max_events.clamp(1_000, 1_000_000);
            }
        }

        config
    }
}

/// One resource waterfall row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ResourceWaterfallRecord {
    pub id: TraceResourceId,
    pub navigation_id: Option<u64>,
    pub kind: String,
    pub method: String,
    pub url: String,
    pub final_url: Option<String>,
    pub status: Option<u16>,
    pub mime: Option<String>,
    pub cache_source: Option<String>,
    pub cache_control: Option<String>,
    pub bytes: Option<usize>,
    pub redirect_count: usize,
    pub blocked_reason: Option<String>,
    pub error: Option<String>,
    pub started_us: u64,
    pub duration_us: Option<u64>,
}

impl ResourceWaterfallRecord {
    #[must_use]
    pub(crate) fn is_finished(&self) -> bool {
        self.duration_us.is_some() || self.error.is_some() || self.blocked_reason.is_some()
    }

    #[must_use]
    pub(crate) fn succeeded(&self) -> bool {
        self.error.is_none()
            && self.blocked_reason.is_none()
            && self.status.is_some_and(|status| (200..400).contains(&status))
    }
}

/// Input for starting a resource trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceTraceStart {
    pub navigation_id: Option<u64>,
    pub kind: String,
    pub method: String,
    pub url: String,
}

impl ResourceTraceStart {
    #[must_use]
    pub(crate) fn new(kind: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            navigation_id: None,
            kind: kind.into(),
            method: "GET".to_owned(),
            url: url.into(),
        }
    }

    #[must_use]
    pub(crate) fn method(mut self, method: impl Into<String>) -> Self {
        self.method = method.into();
        self
    }

    #[must_use]
    pub(crate) const fn navigation_id(mut self, navigation_id: u64) -> Self {
        self.navigation_id = Some(navigation_id);
        self
    }
}

/// Input for finishing a resource trace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ResourceTraceFinish {
    pub final_url: Option<String>,
    pub status: Option<u16>,
    pub mime: Option<String>,
    pub cache_source: Option<String>,
    pub cache_control: Option<String>,
    pub bytes: Option<usize>,
    pub redirect_count: usize,
    pub blocked_reason: Option<String>,
    pub error: Option<String>,
}

impl ResourceTraceFinish {
    #[must_use]
    pub(crate) fn ok(status: u16, bytes: usize) -> Self {
        Self {
            status: Some(status),
            bytes: Some(bytes),
            ..Self::default()
        }
    }

    #[must_use]
    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self {
            error: Some(message.into()),
            ..Self::default()
        }
    }

    #[must_use]
    pub(crate) fn blocked(reason: impl Into<String>) -> Self {
        Self {
            blocked_reason: Some(reason.into()),
            ..Self::default()
        }
    }
}

/// Summary for the resource waterfall.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResourceWaterfallSummary {
    pub total: usize,
    pub finished: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub blocked: usize,
    pub bytes: usize,
    pub redirects: usize,
    pub memory_hits: usize,
    pub disk_hits: usize,
    pub network_fetches: usize,
    pub disabled_fetches: usize,
    pub total_duration_us: u64,
}

/// JavaScript timeline summary from the script pipeline.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JsTimelineSummary {
    pub scripts_discovered: usize,
    pub scripts_executed: usize,
    pub inline_scripts: usize,
    pub external_scripts: usize,
    pub script_bytes: usize,
    pub console_messages: usize,
    pub warnings: usize,
    pub errors: usize,
    pub tasks_executed: usize,
    pub microtasks_executed: usize,
    pub dom_mutations: usize,
    pub web_api_effects: usize,
    pub media_effects: usize,
}

/// Reflow/paint summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RenderingTraceSummary {
    pub reflow_events: usize,
    pub paint_events: usize,
    pub full_reflows: usize,
    pub paint_only_reflows: usize,
    pub reused_reflows: usize,
    pub dirty_regions: usize,
    pub full_repaints: usize,
    pub paint_commands: usize,
    pub rect_commands: usize,
    pub text_commands: usize,
    pub image_commands: usize,
    pub other_commands: usize,
}

/// Full trace report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DevToolsTraceReport {
    pub schema_version: u32,
    pub generated_at_ms: u64,
    pub session_label: String,
    pub duration_us: u64,
    pub summary: DevToolsTraceSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<TraceEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_waterfall: Vec<ResourceWaterfallRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chrome_trace: Option<ChromeTracePayload>,
}

/// Chrome-compatible payload wrapper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChromeTracePayload {
    #[serde(rename = "traceEvents")]
    pub trace_events: Vec<TraceEvent>,
    pub metadata: BTreeMap<String, String>,
}

/// Top-level summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DevToolsTraceSummary {
    pub events: usize,
    pub dropped_events: usize,
    pub resource_waterfall: ResourceWaterfallSummary,
    pub js_timeline: JsTimelineSummary,
    pub rendering: RenderingTraceSummary,
    pub navigation_events: usize,
    pub security_events: usize,
    pub service_worker_events: usize,
    pub accessibility_events: usize,
    pub compatibility_events: usize,
}

/// Thread-safe trace recorder used by the browser shell.
#[derive(Debug, Clone)]
pub(crate) struct DevToolsTraceRecorder {
    inner: Arc<Mutex<TraceInner>>,
}

#[derive(Debug)]
struct TraceInner {
    config: DevToolsTraceConfig,
    started: Instant,
    started_wall_ms: u64,
    next_resource_id: u64,
    events: Vec<TraceEvent>,
    resources: BTreeMap<TraceResourceId, ResourceWaterfallRecord>,
    dropped_events: usize,
    summary: DevToolsTraceSummary,
}

impl DevToolsTraceRecorder {
    /// Creates a recorder from explicit config.
    #[must_use]
    pub(crate) fn new(config: DevToolsTraceConfig) -> Self {
        let recorder = Self {
            inner: Arc::new(Mutex::new(TraceInner {
                config,
                started: Instant::now(),
                started_wall_ms: now_ms(),
                next_resource_id: 1,
                events: Vec::new(),
                resources: BTreeMap::new(),
                dropped_events: 0,
                summary: DevToolsTraceSummary::default(),
            })),
        };

        recorder.metadata("process_name", map_from_pairs([("name", json!("Sylphos"))]));
        recorder.metadata("thread_name", map_from_pairs([("name", json!("main"))]));
        recorder
    }

    /// Creates a disabled recorder.
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self::new(DevToolsTraceConfig::default())
    }

    /// Creates a recorder from environment variables.
    #[must_use]
    pub(crate) fn from_env() -> Self {
        Self::new(DevToolsTraceConfig::from_env())
    }

    /// Returns whether this recorder is enabled.
    #[must_use]
    pub(crate) fn enabled(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.config.enabled)
            .unwrap_or(false)
    }

    /// Starts a scoped complete event. Dropping the span records the duration.
    #[must_use]
    pub(crate) fn span(
        &self,
        category: TraceCategory,
        name: impl Into<String>,
        args: Map<String, Value>,
    ) -> DevToolsTraceSpan {
        let start_us = self.now_us();
        DevToolsTraceSpan {
            recorder: self.clone(),
            category,
            name: name.into(),
            start_us,
            args,
            closed: false,
        }
    }

    /// Records an instant event.
    pub(crate) fn instant(
        &self,
        category: TraceCategory,
        name: impl Into<String>,
        args: Map<String, Value>,
    ) {
        self.push_event(TraceEvent {
            name: name.into(),
            category: category.as_str().to_owned(),
            phase: TracePhase::Instant.as_chrome().to_owned(),
            timestamp_us: self.now_us(),
            duration_us: None,
            process_id: DEFAULT_PROCESS_ID,
            thread_id: DEFAULT_MAIN_THREAD_ID,
            scope: Some("t".to_owned()),
            args,
        });

        self.bump_category(category);
    }

    /// Records a counter event.
    pub(crate) fn counter(
        &self,
        category: TraceCategory,
        name: impl Into<String>,
        args: Map<String, Value>,
    ) {
        self.push_event(TraceEvent {
            name: name.into(),
            category: category.as_str().to_owned(),
            phase: TracePhase::Counter.as_chrome().to_owned(),
            timestamp_us: self.now_us(),
            duration_us: None,
            process_id: DEFAULT_PROCESS_ID,
            thread_id: DEFAULT_MAIN_THREAD_ID,
            scope: None,
            args,
        });
        self.bump_category(category);
    }

    /// Records metadata event.
    pub(crate) fn metadata(&self, name: impl Into<String>, args: Map<String, Value>) {
        self.push_event(TraceEvent {
            name: name.into(),
            category: TraceCategory::Diagnostics.as_str().to_owned(),
            phase: TracePhase::Metadata.as_chrome().to_owned(),
            timestamp_us: self.now_us(),
            duration_us: None,
            process_id: DEFAULT_PROCESS_ID,
            thread_id: DEFAULT_MAIN_THREAD_ID,
            scope: None,
            args,
        });
    }

    /// Starts a resource waterfall entry.
    pub(crate) fn start_resource(&self, start: ResourceTraceStart) -> TraceResourceId {
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(_) => return TraceResourceId(0),
        };

        let id = TraceResourceId(guard.next_resource_id);
        guard.next_resource_id = guard.next_resource_id.saturating_add(1);
        let started_us = elapsed_us(guard.started.elapsed());

        if guard.config.enabled {
            guard.resources.insert(
                id,
                ResourceWaterfallRecord {
                    id,
                    navigation_id: start.navigation_id,
                    kind: start.kind.clone(),
                    method: start.method.clone(),
                    url: start.url.clone(),
                    final_url: None,
                    status: None,
                    mime: None,
                    cache_source: None,
                    cache_control: None,
                    bytes: None,
                    redirect_count: 0,
                    blocked_reason: None,
                    error: None,
                    started_us,
                    duration_us: None,
                },
            );
        }

        drop(guard);

        self.instant(
            TraceCategory::Resource,
            "resource:start",
            map_from_pairs([
                ("id", json!(id.0)),
                ("kind", json!(start.kind)),
                ("method", json!(start.method)),
                ("url", json!(start.url)),
                ("navigationId", json!(start.navigation_id)),
            ]),
        );

        id
    }

    /// Finishes a resource waterfall entry.
    pub(crate) fn finish_resource(&self, id: TraceResourceId, finish: ResourceTraceFinish) {
        let mut event_args = map_from_pairs([
            ("id", json!(id.0)),
            ("status", json!(finish.status)),
            ("mime", json!(finish.mime)),
            ("bytes", json!(finish.bytes)),
            ("cacheSource", json!(finish.cache_source)),
            ("redirectCount", json!(finish.redirect_count)),
            ("blockedReason", json!(finish.blocked_reason)),
            ("error", json!(finish.error)),
        ]);

        let mut complete_event = None;

        if let Ok(mut guard) = self.inner.lock() {
            let finished_us = elapsed_us(guard.started.elapsed());
            if let Some(record) = guard.resources.get_mut(&id) {
                record.final_url = finish.final_url.clone();
                record.status = finish.status;
                record.mime = finish.mime.clone();
                record.cache_source = finish.cache_source.clone();
                record.cache_control = finish.cache_control.clone();
                record.bytes = finish.bytes;
                record.redirect_count = finish.redirect_count;
                record.blocked_reason = finish.blocked_reason.clone();
                record.error = finish.error.clone();
                record.duration_us = Some(finished_us.saturating_sub(record.started_us));

                event_args.insert("url".to_owned(), json!(record.url));
                event_args.insert("finalUrl".to_owned(), json!(record.final_url));

                complete_event = Some(TraceEvent {
                    name: format!("resource:{} {}", record.method, record.kind),
                    category: TraceCategory::Resource.as_str().to_owned(),
                    phase: TracePhase::Complete.as_chrome().to_owned(),
                    timestamp_us: record.started_us,
                    duration_us: record.duration_us,
                    process_id: DEFAULT_PROCESS_ID,
                    thread_id: DEFAULT_MAIN_THREAD_ID,
                    scope: None,
                    args: event_args.clone(),
                });
            }
        }

        if let Some(event) = complete_event {
            self.push_event(event);
        } else {
            self.instant(TraceCategory::Resource, "resource:finish", event_args);
        }

        self.recompute_resource_summary();
    }

    /// Records a top-level navigation state transition.
    pub(crate) fn record_navigation(
        &self,
        name: impl Into<String>,
        navigation_id: Option<u64>,
        url: impl Into<String>,
        args: Map<String, Value>,
    ) {
        let mut args = args;
        args.insert("navigationId".to_owned(), json!(navigation_id));
        args.insert("url".to_owned(), json!(url.into()));
        self.instant(TraceCategory::Navigation, name, args);
    }

    /// Records a JavaScript/script-pipeline summary.
    pub(crate) fn record_js_timeline(&self, summary: JsTimelineSummary) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.summary.js_timeline = merge_js_summary(inner.summary.js_timeline.clone(), summary.clone());
        }

        self.instant(
            TraceCategory::Script,
            "js:summary",
            map_from_pairs([
                ("scriptsDiscovered", json!(summary.scripts_discovered)),
                ("scriptsExecuted", json!(summary.scripts_executed)),
                ("inlineScripts", json!(summary.inline_scripts)),
                ("externalScripts", json!(summary.external_scripts)),
                ("bytes", json!(summary.script_bytes)),
                ("console", json!(summary.console_messages)),
                ("warnings", json!(summary.warnings)),
                ("errors", json!(summary.errors)),
                ("tasksExecuted", json!(summary.tasks_executed)),
                ("microtasksExecuted", json!(summary.microtasks_executed)),
                ("domMutations", json!(summary.dom_mutations)),
                ("webApiEffects", json!(summary.web_api_effects)),
                ("mediaEffects", json!(summary.media_effects)),
            ]),
        );
    }

    /// Records one script execution duration.
    pub(crate) fn record_script_execution(
        &self,
        source_name: impl Into<String>,
        source_order: usize,
        bytes: usize,
        duration: Duration,
        warnings: usize,
        errors: usize,
    ) {
        self.complete(
            TraceCategory::Script,
            "script:execute",
            self.now_us().saturating_sub(elapsed_us(duration)),
            elapsed_us(duration),
            map_from_pairs([
                ("source", json!(source_name.into())),
                ("sourceOrder", json!(source_order)),
                ("bytes", json!(bytes)),
                ("warnings", json!(warnings)),
                ("errors", json!(errors)),
            ]),
        );
    }

    /// Records a reflow event.
    pub(crate) fn record_reflow(
        &self,
        mode: impl Into<String>,
        reason: impl Into<String>,
        generation: u64,
        previous_commands: usize,
        current_commands: usize,
        dirty_regions: usize,
        full_repaint: bool,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
            let rendering = &mut inner.summary.rendering;
            rendering.reflow_events = rendering.reflow_events.saturating_add(1);
            rendering.dirty_regions = rendering.dirty_regions.saturating_add(dirty_regions);
            if full_repaint {
                rendering.full_repaints = rendering.full_repaints.saturating_add(1);
            }

            match mode.into().as_str() {
                "Full" | "full" => rendering.full_reflows = rendering.full_reflows.saturating_add(1),
                "PaintOnly" | "paint-only" | "paint_only" => {
                    rendering.paint_only_reflows = rendering.paint_only_reflows.saturating_add(1);
                }
                "Reused" | "reused" => rendering.reused_reflows = rendering.reused_reflows.saturating_add(1),
                _ => {}
            }
        }

        self.instant(
            TraceCategory::Reflow,
            "reflow",
            map_from_pairs([
                ("reason", json!(reason.into())),
                ("generation", json!(generation)),
                ("previousCommands", json!(previous_commands)),
                ("currentCommands", json!(current_commands)),
                ("dirtyRegions", json!(dirty_regions)),
                ("fullRepaint", json!(full_repaint)),
            ]),
        );
    }

    /// Records a paint-plan summary.
    pub(crate) fn record_paint_plan(
        &self,
        command_count: usize,
        rect_commands: usize,
        text_commands: usize,
        image_commands: usize,
        other_commands: usize,
    ) {
        if let Ok(mut inner) = self.inner.lock() {
            let rendering = &mut inner.summary.rendering;
            rendering.paint_events = rendering.paint_events.saturating_add(1);
            rendering.paint_commands = rendering.paint_commands.saturating_add(command_count);
            rendering.rect_commands = rendering.rect_commands.saturating_add(rect_commands);
            rendering.text_commands = rendering.text_commands.saturating_add(text_commands);
            rendering.image_commands = rendering.image_commands.saturating_add(image_commands);
            rendering.other_commands = rendering.other_commands.saturating_add(other_commands);
        }

        self.instant(
            TraceCategory::Paint,
            "paint:plan",
            map_from_pairs([
                ("commands", json!(command_count)),
                ("rects", json!(rect_commands)),
                ("texts", json!(text_commands)),
                ("images", json!(image_commands)),
                ("other", json!(other_commands)),
            ]),
        );
    }

    /// Records a generic diagnostic policy/security/service-worker event.
    pub(crate) fn record_policy_event(
        &self,
        category: TraceCategory,
        name: impl Into<String>,
        allowed: bool,
        args: Map<String, Value>,
    ) {
        let mut args = args;
        args.insert("allowed".to_owned(), json!(allowed));
        self.instant(category, name, args);
    }

    /// Finalizes a report snapshot.
    #[must_use]
    pub(crate) fn report(&self) -> DevToolsTraceReport {
        let guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(_) => {
                return DevToolsTraceReport {
                    schema_version: TRACE_SCHEMA_VERSION,
                    generated_at_ms: now_ms(),
                    session_label: "poisoned-trace".to_owned(),
                    duration_us: 0,
                    summary: DevToolsTraceSummary::default(),
                    events: Vec::new(),
                    resource_waterfall: Vec::new(),
                    chrome_trace: None,
                }
            }
        };

        let duration_us = elapsed_us(guard.started.elapsed());
        let mut summary = guard.summary.clone();
        summary.events = guard.events.len();
        summary.dropped_events = guard.dropped_events;
        summary.resource_waterfall = summarize_resources(guard.resources.values());

        let events = if guard.config.include_events {
            guard.events.clone()
        } else {
            Vec::new()
        };
        let resource_waterfall = if guard.config.include_waterfall {
            guard.resources.values().cloned().collect()
        } else {
            Vec::new()
        };
        let chrome_trace = guard.config.include_chrome_trace.then(|| ChromeTracePayload {
            trace_events: guard.events.clone(),
            metadata: BTreeMap::from([
                ("session".to_owned(), guard.config.session_label.clone()),
                ("schemaVersion".to_owned(), TRACE_SCHEMA_VERSION.to_string()),
                ("generatedAtMs".to_owned(), now_ms().to_string()),
            ]),
        });

        DevToolsTraceReport {
            schema_version: TRACE_SCHEMA_VERSION,
            generated_at_ms: now_ms(),
            session_label: guard.config.session_label.clone(),
            duration_us,
            summary,
            events,
            resource_waterfall,
            chrome_trace,
        }
    }

    /// Serializes the report to pretty JSON.
    pub(crate) fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.report()).context("failed to serialize DevTools trace")
    }

    /// Writes report JSON to a file.
    pub(crate) fn write_json(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create trace directory `{}`", parent.display()))?;
        }

        let json = self.to_json_pretty()?;
        fs::write(path, json)
            .with_context(|| format!("failed to write trace report `{}`", path.display()))
    }

    /// Writes configured output path if tracing is enabled and a path exists.
    pub(crate) fn flush_configured_output(&self) -> Result<Option<PathBuf>> {
        let output = self
            .inner
            .lock()
            .ok()
            .and_then(|inner| inner.config.enabled.then(|| inner.config.output_path.clone()))
            .flatten();

        let Some(path) = output else {
            return Ok(None);
        };

        self.write_json(&path)?;
        Ok(Some(path))
    }

    fn complete(
        &self,
        category: TraceCategory,
        name: impl Into<String>,
        started_us: u64,
        duration_us: u64,
        args: Map<String, Value>,
    ) {
        self.push_event(TraceEvent {
            name: name.into(),
            category: category.as_str().to_owned(),
            phase: TracePhase::Complete.as_chrome().to_owned(),
            timestamp_us: started_us,
            duration_us: Some(duration_us),
            process_id: DEFAULT_PROCESS_ID,
            thread_id: DEFAULT_MAIN_THREAD_ID,
            scope: None,
            args,
        });
        self.bump_category(category);
    }

    fn push_event(&self, event: TraceEvent) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        if !inner.config.enabled {
            return;
        }

        if inner.events.len() >= inner.config.max_events {
            inner.dropped_events = inner.dropped_events.saturating_add(1);
            return;
        }

        inner.events.push(event);
    }

    fn now_us(&self) -> u64 {
        self.inner
            .lock()
            .map(|inner| elapsed_us(inner.started.elapsed()))
            .unwrap_or(0)
    }

    fn bump_category(&self, category: TraceCategory) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        match category {
            TraceCategory::Navigation => {
                inner.summary.navigation_events = inner.summary.navigation_events.saturating_add(1);
            }
            TraceCategory::Security => {
                inner.summary.security_events = inner.summary.security_events.saturating_add(1);
            }
            TraceCategory::ServiceWorker => {
                inner.summary.service_worker_events =
                    inner.summary.service_worker_events.saturating_add(1);
            }
            TraceCategory::Accessibility => {
                inner.summary.accessibility_events =
                    inner.summary.accessibility_events.saturating_add(1);
            }
            TraceCategory::Compatibility => {
                inner.summary.compatibility_events =
                    inner.summary.compatibility_events.saturating_add(1);
            }
            _ => {}
        }
    }

    fn recompute_resource_summary(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.summary.resource_waterfall = summarize_resources(inner.resources.values());
        }
    }
}

/// RAII trace span.
#[derive(Debug)]
pub(crate) struct DevToolsTraceSpan {
    recorder: DevToolsTraceRecorder,
    category: TraceCategory,
    name: String,
    start_us: u64,
    args: Map<String, Value>,
    closed: bool,
}

impl DevToolsTraceSpan {
    /// Explicitly closes the span and records it once.
    pub(crate) fn close(mut self) {
        self.close_inner();
    }

    fn close_inner(&mut self) {
        if self.closed {
            return;
        }
        let end = self.recorder.now_us();
        self.recorder.complete(
            self.category,
            self.name.clone(),
            self.start_us,
            end.saturating_sub(self.start_us),
            self.args.clone(),
        );
        self.closed = true;
    }
}

impl Drop for DevToolsTraceSpan {
    fn drop(&mut self) {
        self.close_inner();
    }
}

/// Builds a JSON object map from key-value pairs.
#[must_use]
pub(crate) fn map_from_pairs<const N: usize>(pairs: [(&str, Value); N]) -> Map<String, Value> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

/// Convenience for a string arg map.
#[must_use]
pub(crate) fn single_arg(key: &str, value: impl Into<Value>) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(key.to_owned(), value.into());
    map
}

fn summarize_resources<'a>(
    resources: impl IntoIterator<Item = &'a ResourceWaterfallRecord>,
) -> ResourceWaterfallSummary {
    let mut summary = ResourceWaterfallSummary::default();

    for record in resources {
        summary.total = summary.total.saturating_add(1);
        if record.is_finished() {
            summary.finished = summary.finished.saturating_add(1);
        }
        if record.succeeded() {
            summary.succeeded = summary.succeeded.saturating_add(1);
        }
        if record.error.is_some() {
            summary.failed = summary.failed.saturating_add(1);
        }
        if record.blocked_reason.is_some() {
            summary.blocked = summary.blocked.saturating_add(1);
        }
        summary.bytes = summary.bytes.saturating_add(record.bytes.unwrap_or(0));
        summary.redirects = summary.redirects.saturating_add(record.redirect_count);
        summary.total_duration_us = summary
            .total_duration_us
            .saturating_add(record.duration_us.unwrap_or(0));

        match record.cache_source.as_deref().unwrap_or_default() {
            "memory" => summary.memory_hits = summary.memory_hits.saturating_add(1),
            "disk" => summary.disk_hits = summary.disk_hits.saturating_add(1),
            "network" => summary.network_fetches = summary.network_fetches.saturating_add(1),
            "disabled-network" | "disabled" => {
                summary.disabled_fetches = summary.disabled_fetches.saturating_add(1);
            }
            _ => {}
        }
    }

    summary
}

fn merge_js_summary(mut left: JsTimelineSummary, right: JsTimelineSummary) -> JsTimelineSummary {
    left.scripts_discovered = left.scripts_discovered.saturating_add(right.scripts_discovered);
    left.scripts_executed = left.scripts_executed.saturating_add(right.scripts_executed);
    left.inline_scripts = left.inline_scripts.saturating_add(right.inline_scripts);
    left.external_scripts = left.external_scripts.saturating_add(right.external_scripts);
    left.script_bytes = left.script_bytes.saturating_add(right.script_bytes);
    left.console_messages = left.console_messages.saturating_add(right.console_messages);
    left.warnings = left.warnings.saturating_add(right.warnings);
    left.errors = left.errors.saturating_add(right.errors);
    left.tasks_executed = left.tasks_executed.saturating_add(right.tasks_executed);
    left.microtasks_executed = left.microtasks_executed.saturating_add(right.microtasks_executed);
    left.dom_mutations = left.dom_mutations.saturating_add(right.dom_mutations);
    left.web_api_effects = left.web_api_effects.saturating_add(right.web_api_effects);
    left.media_effects = left.media_effects.saturating_add(right.media_effects);
    left
}

fn elapsed_us(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    u64::try_from(millis).unwrap_or(u64::MAX)
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("True") | Ok("yes") | Ok("YES")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_recorder_keeps_empty_report() {
        let recorder = DevToolsTraceRecorder::disabled();
        recorder.instant(TraceCategory::Navigation, "nav", Map::new());
        let report = recorder.report();
        assert_eq!(report.events.len(), 0);
    }

    #[test]
    fn records_resource_waterfall() {
        let recorder = DevToolsTraceRecorder::new(DevToolsTraceConfig {
            enabled: true,
            ..DevToolsTraceConfig::default()
        });

        let id = recorder.start_resource(ResourceTraceStart::new("document", "https://example.com"));
        recorder.finish_resource(
            id,
            ResourceTraceFinish {
                final_url: Some("https://example.com/".to_owned()),
                status: Some(200),
                mime: Some("text/html".to_owned()),
                cache_source: Some("network".to_owned()),
                bytes: Some(512),
                ..ResourceTraceFinish::default()
            },
        );

        let report = recorder.report();
        assert_eq!(report.resource_waterfall.len(), 1);
        assert_eq!(report.summary.resource_waterfall.succeeded, 1);
        assert!(report.events.iter().any(|event| event.name.starts_with("resource:")));
    }

    #[test]
    fn span_records_complete_event() {
        let recorder = DevToolsTraceRecorder::new(DevToolsTraceConfig {
            enabled: true,
            ..DevToolsTraceConfig::default()
        });

        {
            let _span = recorder.span(TraceCategory::Paint, "paint:test", Map::new());
        }

        let report = recorder.report();
        assert!(report
            .events
            .iter()
            .any(|event| event.name == "paint:test" && event.phase == "X"));
    }
}
