#![allow(clippy::too_many_lines)]
#![doc = "Script pipeline integration for SylJS."]
#![doc = ""]
#![doc = "This module wires SylJS execution into a browser-like document lifecycle:"]
#![doc = "parser-blocking scripts, async scripts, defer scripts, document.currentScript,"]
#![doc = "DOMContentLoaded, load readiness, and reflow/repaint hooks."]

use crate::{
    compile_program, parse_module, parse_script, CompileOptions, EventLoopRunSummary,
    JsRuntimeError, JsValue, ProgramKind, ScheduledVm, SharedWebApiHost,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    rc::Rc,
};

/// Shared script resource loader pointer.
pub type SharedScriptResourceLoader = Rc<dyn ScriptResourceLoader>;

/// Shared script pipeline hook pointer.
pub type SharedScriptPipelineHooks = Rc<dyn ScriptPipelineHooks>;

/// Script kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    /// Classic JavaScript script.
    Classic,

    /// ES module script. SylJS parses this as module syntax.
    Module,
}

impl ScriptKind {
    fn program_kind(self) -> ProgramKind {
        match self {
            Self::Classic => ProgramKind::Script,
            Self::Module => ProgramKind::Module,
        }
    }
}

/// Script loading/execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptLoadMode {
    /// Parser-blocking classic behavior.
    ParserBlocking,

    /// Async script behavior.
    Async,

    /// Defer script behavior.
    Defer,

    /// Dynamically injected script behavior.
    Dynamic,
}

/// Script source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptSource {
    /// Inline source.
    Inline {
        /// Script source.
        source: String,
    },

    /// External URL.
    External {
        /// Script URL.
        url: String,
    },
}

impl ScriptSource {
    /// Returns a human-readable label.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Inline { .. } => "inline".to_owned(),
            Self::External { url } => url.clone(),
        }
    }
}

/// Script descriptor discovered by the parser or DOM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDescriptor {
    /// Stable script id.
    pub id: u64,

    /// Debug label.
    pub label: String,

    /// Script kind.
    pub kind: ScriptKind,

    /// Loading/execution mode.
    pub mode: ScriptLoadMode,

    /// Source.
    pub source: ScriptSource,

    /// Parser-inserted flag.
    pub parser_inserted: bool,

    /// Whether script is blocking render.
    pub render_blocking: bool,

    /// Optional nonce.
    pub nonce: Option<String>,

    /// Optional integrity field.
    pub integrity: Option<String>,
}

impl ScriptDescriptor {
    /// Creates an inline parser-blocking classic script.
    #[must_use]
    pub fn inline(id: u64, source: impl Into<String>) -> Self {
        Self {
            id,
            label: format!("inline:{id}"),
            kind: ScriptKind::Classic,
            mode: ScriptLoadMode::ParserBlocking,
            source: ScriptSource::Inline {
                source: source.into(),
            },
            parser_inserted: true,
            render_blocking: true,
            nonce: None,
            integrity: None,
        }
    }

    /// Creates an external script.
    #[must_use]
    pub fn external(id: u64, url: impl Into<String>, mode: ScriptLoadMode) -> Self {
        let url = url.into();
        Self {
            id,
            label: url.clone(),
            kind: ScriptKind::Classic,
            mode,
            source: ScriptSource::External { url },
            parser_inserted: true,
            render_blocking: mode == ScriptLoadMode::ParserBlocking,
            nonce: None,
            integrity: None,
        }
    }
}

/// Dirty flag type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DirtyFlag {
    /// DOM tree may have changed.
    Dom,

    /// CSS selectors or computed style may have changed.
    Style,

    /// Layout tree needs update.
    Layout,

    /// Paint plan needs update.
    Paint,

    /// Lifecycle state changed.
    Lifecycle,
}

/// Dirty flag set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirtyFlags {
    flags: BTreeSet<DirtyFlag>,
}

impl DirtyFlags {
    /// Creates a conservative post-script dirty state.
    #[must_use]
    pub fn conservative_script() -> Self {
        let mut flags = BTreeSet::new();
        flags.insert(DirtyFlag::Dom);
        flags.insert(DirtyFlag::Style);
        flags.insert(DirtyFlag::Layout);
        flags.insert(DirtyFlag::Paint);
        Self { flags }
    }

    /// Creates lifecycle dirty state.
    #[must_use]
    pub fn lifecycle() -> Self {
        let mut flags = BTreeSet::new();
        flags.insert(DirtyFlag::Lifecycle);
        flags.insert(DirtyFlag::Paint);
        Self { flags }
    }

    /// Inserts a flag.
    pub fn insert(&mut self, flag: DirtyFlag) {
        self.flags.insert(flag);
    }

    /// Returns whether a flag is present.
    #[must_use]
    pub fn contains(&self, flag: DirtyFlag) -> bool {
        self.flags.contains(&flag)
    }

    /// Returns flags.
    #[must_use]
    pub fn flags(&self) -> Vec<DirtyFlag> {
        self.flags.iter().copied().collect()
    }

    /// Returns true if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }
}

/// Document lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineDocumentPhase {
    /// Parser is still running.
    Loading,

    /// Parser finished, defer scripts may run.
    Interactive,

    /// DOMContentLoaded fired.
    DomContentLoaded,

    /// Load fired or page is ready.
    Complete,
}

/// Async script scheduling policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncSchedulingPolicy {
    /// Execute async scripts as soon as they are discovered and fetched.
    AsSoonAsDiscovered,

    /// Execute async scripts after parsing but before defer scripts.
    AfterParsingBeforeDefer,

    /// Execute async scripts after DOMContentLoaded.
    AfterDomContentLoaded,
}

/// Script pipeline configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptPipelineConfig {
    /// Async scheduling policy.
    pub async_policy: AsyncSchedulingPolicy,

    /// Continue after script errors.
    pub continue_on_error: bool,

    /// Drain SylJS event loop after every script.
    pub drain_after_each_script: bool,

    /// Install and clear document.currentScript.
    pub manage_current_script: bool,

    /// Conservatively request style/layout/paint after every script.
    pub conservative_reflow_after_script: bool,

    /// Fire DOMContentLoaded during finish_parsing.
    pub fire_dom_content_loaded: bool,

    /// Fire load during finish_document.
    pub fire_load: bool,
}

impl Default for ScriptPipelineConfig {
    fn default() -> Self {
        Self {
            async_policy: AsyncSchedulingPolicy::AsSoonAsDiscovered,
            continue_on_error: true,
            drain_after_each_script: true,
            manage_current_script: true,
            conservative_reflow_after_script: true,
            fire_dom_content_loaded: true,
            fire_load: true,
        }
    }
}

/// Script pipeline metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptPipelineMetrics {
    /// Scripts discovered.
    pub scripts_discovered: u64,

    /// Inline scripts.
    pub inline_scripts: u64,

    /// External scripts.
    pub external_scripts: u64,

    /// External fetches attempted.
    pub external_fetches: u64,

    /// Parser-blocking scripts executed.
    pub parser_blocking_scripts: u64,

    /// Async scripts queued.
    pub async_scripts_queued: u64,

    /// Async scripts executed.
    pub async_scripts_executed: u64,

    /// Defer scripts queued.
    pub defer_scripts_queued: u64,

    /// Defer scripts executed.
    pub defer_scripts_executed: u64,

    /// Dynamic scripts executed.
    pub dynamic_scripts_executed: u64,

    /// Total scripts executed.
    pub scripts_executed: u64,

    /// Script execution failures.
    pub script_failures: u64,

    /// document.currentScript sets.
    pub current_script_sets: u64,

    /// document.currentScript clears.
    pub current_script_clears: u64,

    /// Reflow requests.
    pub reflow_requests: u64,

    /// DOMContentLoaded events.
    pub dom_content_loaded_events: u64,

    /// Load events.
    pub load_events: u64,
}

/// Script execution failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptExecutionFailure {
    /// Script id.
    pub script_id: u64,

    /// Script label.
    pub label: String,

    /// Error message.
    pub error: String,
}

/// Reflow request emitted by script pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflowRequest {
    /// Triggering script id, when relevant.
    pub script_id: Option<u64>,

    /// Triggering label.
    pub label: String,

    /// Dirty flags.
    pub dirty: DirtyFlags,

    /// Reason.
    pub reason: String,
}

/// Pipeline event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineEvent {
    /// Script discovered.
    ScriptDiscovered {
        /// Script id.
        script_id: u64,

        /// Script label.
        label: String,
    },

    /// External script fetched.
    ScriptFetched {
        /// Script id.
        script_id: u64,

        /// Script label.
        label: String,

        /// Fetched source length in bytes.
        bytes: usize,
    },

    /// Script queued.
    ScriptQueued {
        /// Script id.
        script_id: u64,

        /// Script label.
        label: String,

        /// Script load mode.
        mode: ScriptLoadMode,
    },

    /// Script started.
    ScriptStarted {
        /// Script id.
        script_id: u64,

        /// Script label.
        label: String,

        /// Script load mode.
        mode: ScriptLoadMode,
    },

    /// Script finished.
    ScriptFinished {
        /// Script id.
        script_id: u64,

        /// Script label.
        label: String,
    },

    /// Script failed.
    ScriptFailed(ScriptExecutionFailure),

    /// Reflow requested.
    ReflowRequested(ReflowRequest),

    /// Document phase changed.
    DocumentPhaseChanged(PipelineDocumentPhase),
}

/// One script run record.
#[derive(Debug, Clone)]
pub struct PipelineScriptRun {
    /// Script descriptor.
    pub descriptor: ScriptDescriptor,

    /// Source bytes.
    pub source_bytes: usize,

    /// Event-loop summary after execution, if drained.
    pub summary: Option<EventLoopRunSummary>,
}

/// Script pipeline run result.
#[derive(Debug, Clone)]
pub struct ScriptPipelineRun {
    /// Metrics.
    pub metrics: ScriptPipelineMetrics,

    /// Events.
    pub events: Vec<PipelineEvent>,

    /// Failures.
    pub failures: Vec<ScriptExecutionFailure>,

    /// Script run records.
    pub scripts: Vec<PipelineScriptRun>,

    /// Last event-loop summary.
    pub last_summary: Option<EventLoopRunSummary>,

    /// Final phase.
    pub phase: PipelineDocumentPhase,
}

/// Script resource loader.
pub trait ScriptResourceLoader {
    /// Loads script source by URL.
    fn load_script(&self, url: &str) -> Result<String, JsRuntimeError>;
}

/// Script pipeline hooks.
pub trait ScriptPipelineHooks {
    /// Called before script execution.
    fn before_script(&self, descriptor: &ScriptDescriptor);

    /// Called after successful script execution.
    fn after_script(&self, descriptor: &ScriptDescriptor);

    /// Called after script failure.
    fn script_failed(&self, descriptor: &ScriptDescriptor, error: &JsRuntimeError);

    /// Called when reflow is requested.
    fn request_reflow(&self, request: &ReflowRequest);

    /// Called when DOMContentLoaded fires.
    fn dom_content_loaded(&self);

    /// Called when load fires.
    fn load(&self);
}

/// Research script loader.
#[derive(Debug, Default)]
pub struct ResearchScriptResourceLoader {
    scripts: RefCell<BTreeMap<String, String>>,
}

impl ResearchScriptResourceLoader {
    /// Registers a script URL.
    pub fn register_script(&self, url: impl Into<String>, source: impl Into<String>) {
        self.scripts.borrow_mut().insert(url.into(), source.into());
    }

    /// Returns registered URLs.
    #[must_use]
    pub fn urls(&self) -> Vec<String> {
        self.scripts.borrow().keys().cloned().collect()
    }
}

impl ScriptResourceLoader for ResearchScriptResourceLoader {
    fn load_script(&self, url: &str) -> Result<String, JsRuntimeError> {
        self.scripts
            .borrow()
            .get(url)
            .cloned()
            .ok_or_else(|| JsRuntimeError::new(format!("script resource not found: {url}")))
    }
}

/// Web API backed script loader.
#[derive(Clone)]
pub struct WebApiScriptResourceLoader {
    host: SharedWebApiHost,
}

impl fmt::Debug for WebApiScriptResourceLoader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebApiScriptResourceLoader")
            .finish_non_exhaustive()
    }
}

impl WebApiScriptResourceLoader {
    /// Creates a loader from a WebApiHost.
    #[must_use]
    pub fn new(host: SharedWebApiHost) -> Self {
        Self { host }
    }
}

impl ScriptResourceLoader for WebApiScriptResourceLoader {
    fn load_script(&self, url: &str) -> Result<String, JsRuntimeError> {
        let response = self.host.fetch(url, "GET")?;
        if response.ok() {
            Ok(response.body)
        } else {
            Err(JsRuntimeError::new(format!(
                "script fetch failed for {url}: {} {}",
                response.status, response.status_text
            )))
        }
    }
}

/// Research reflow hook collector.
#[derive(Debug, Default)]
pub struct ResearchReflowHooks {
    before: RefCell<Vec<u64>>,
    after: RefCell<Vec<u64>>,
    failed: RefCell<Vec<ScriptExecutionFailure>>,
    reflows: RefCell<Vec<ReflowRequest>>,
    dom_content_loaded_count: RefCell<u64>,
    load_count: RefCell<u64>,
}

impl ResearchReflowHooks {
    /// Before-script ids.
    #[must_use]
    pub fn before_script_ids(&self) -> Vec<u64> {
        self.before.borrow().clone()
    }

    /// After-script ids.
    #[must_use]
    pub fn after_script_ids(&self) -> Vec<u64> {
        self.after.borrow().clone()
    }

    /// Failures.
    #[must_use]
    pub fn failures(&self) -> Vec<ScriptExecutionFailure> {
        self.failed.borrow().clone()
    }

    /// Reflow requests.
    #[must_use]
    pub fn reflow_requests(&self) -> Vec<ReflowRequest> {
        self.reflows.borrow().clone()
    }

    /// DOMContentLoaded count.
    #[must_use]
    pub fn dom_content_loaded_count(&self) -> u64 {
        *self.dom_content_loaded_count.borrow()
    }

    /// Load count.
    #[must_use]
    pub fn load_count(&self) -> u64 {
        *self.load_count.borrow()
    }
}

impl ScriptPipelineHooks for ResearchReflowHooks {
    fn before_script(&self, descriptor: &ScriptDescriptor) {
        self.before.borrow_mut().push(descriptor.id);
    }

    fn after_script(&self, descriptor: &ScriptDescriptor) {
        self.after.borrow_mut().push(descriptor.id);
    }

    fn script_failed(&self, descriptor: &ScriptDescriptor, error: &JsRuntimeError) {
        self.failed.borrow_mut().push(ScriptExecutionFailure {
            script_id: descriptor.id,
            label: descriptor.label.clone(),
            error: error.to_string(),
        });
    }

    fn request_reflow(&self, request: &ReflowRequest) {
        self.reflows.borrow_mut().push(request.clone());
    }

    fn dom_content_loaded(&self) {
        *self.dom_content_loaded_count.borrow_mut() =
            self.dom_content_loaded_count().saturating_add(1);
    }

    fn load(&self) {
        *self.load_count.borrow_mut() = self.load_count().saturating_add(1);
    }
}

/// Script pipeline.
pub struct ScriptPipeline {
    config: ScriptPipelineConfig,
    loader: SharedScriptResourceLoader,
    hooks: SharedScriptPipelineHooks,
    metrics: ScriptPipelineMetrics,
    events: Vec<PipelineEvent>,
    failures: Vec<ScriptExecutionFailure>,
    script_runs: Vec<PipelineScriptRun>,
    async_queue: VecDeque<ScriptDescriptor>,
    defer_queue: VecDeque<ScriptDescriptor>,
    phase: PipelineDocumentPhase,
    last_summary: Option<EventLoopRunSummary>,
}

impl fmt::Debug for ScriptPipeline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptPipeline")
            .field("config", &self.config)
            .field("metrics", &self.metrics)
            .field("phase", &self.phase)
            .field("async_queue_len", &self.async_queue.len())
            .field("defer_queue_len", &self.defer_queue.len())
            .finish()
    }
}

impl ScriptPipeline {
    /// Creates a pipeline.
    #[must_use]
    pub fn new(
        config: ScriptPipelineConfig,
        loader: SharedScriptResourceLoader,
        hooks: SharedScriptPipelineHooks,
    ) -> Self {
        Self {
            config,
            loader,
            hooks,
            metrics: ScriptPipelineMetrics::default(),
            events: Vec::new(),
            failures: Vec::new(),
            script_runs: Vec::new(),
            async_queue: VecDeque::new(),
            defer_queue: VecDeque::new(),
            phase: PipelineDocumentPhase::Loading,
            last_summary: None,
        }
    }

    /// Current phase.
    #[must_use]
    pub const fn phase(&self) -> PipelineDocumentPhase {
        self.phase
    }

    /// Metrics snapshot.
    #[must_use]
    pub fn metrics(&self) -> ScriptPipelineMetrics {
        self.metrics.clone()
    }

    /// Events snapshot.
    #[must_use]
    pub fn events(&self) -> Vec<PipelineEvent> {
        self.events.clone()
    }

    /// Discovers and handles a script as parser would.
    pub fn discover_script(
        &mut self,
        scheduled: &mut ScheduledVm,
        descriptor: ScriptDescriptor,
    ) -> Result<(), JsRuntimeError> {
        self.metrics.scripts_discovered = self.metrics.scripts_discovered.saturating_add(1);
        match descriptor.source {
            ScriptSource::Inline { .. } => {
                self.metrics.inline_scripts = self.metrics.inline_scripts.saturating_add(1);
            }
            ScriptSource::External { .. } => {
                self.metrics.external_scripts = self.metrics.external_scripts.saturating_add(1);
            }
        }

        self.events.push(PipelineEvent::ScriptDiscovered {
            script_id: descriptor.id,
            label: descriptor.label.clone(),
        });

        match descriptor.mode {
            ScriptLoadMode::ParserBlocking => {
                self.execute_or_record_error(scheduled, descriptor)?;
            }
            ScriptLoadMode::Async => {
                self.metrics.async_scripts_queued =
                    self.metrics.async_scripts_queued.saturating_add(1);
                self.events.push(PipelineEvent::ScriptQueued {
                    script_id: descriptor.id,
                    label: descriptor.label.clone(),
                    mode: ScriptLoadMode::Async,
                });
                self.async_queue.push_back(descriptor);

                if self.config.async_policy == AsyncSchedulingPolicy::AsSoonAsDiscovered {
                    self.flush_async_scripts(scheduled)?;
                }
            }
            ScriptLoadMode::Defer => {
                self.metrics.defer_scripts_queued =
                    self.metrics.defer_scripts_queued.saturating_add(1);
                self.events.push(PipelineEvent::ScriptQueued {
                    script_id: descriptor.id,
                    label: descriptor.label.clone(),
                    mode: ScriptLoadMode::Defer,
                });
                self.defer_queue.push_back(descriptor);
            }
            ScriptLoadMode::Dynamic => {
                self.execute_or_record_error(scheduled, descriptor)?;
            }
        }

        Ok(())
    }

    /// Finishes parsing and runs queued defer scripts.
    pub fn finish_parsing(&mut self, scheduled: &mut ScheduledVm) -> Result<(), JsRuntimeError> {
        self.phase = PipelineDocumentPhase::Interactive;
        self.events
            .push(PipelineEvent::DocumentPhaseChanged(self.phase));

        if self.config.async_policy == AsyncSchedulingPolicy::AfterParsingBeforeDefer {
            self.flush_async_scripts(scheduled)?;
        }

        self.flush_defer_scripts(scheduled)?;

        self.phase = PipelineDocumentPhase::DomContentLoaded;
        self.events
            .push(PipelineEvent::DocumentPhaseChanged(self.phase));

        if self.config.fire_dom_content_loaded {
            self.metrics.dom_content_loaded_events =
                self.metrics.dom_content_loaded_events.saturating_add(1);
            self.hooks.dom_content_loaded();
            self.request_reflow(None, "DOMContentLoaded", DirtyFlags::lifecycle());
        }

        if self.config.async_policy == AsyncSchedulingPolicy::AfterDomContentLoaded {
            self.flush_async_scripts(scheduled)?;
        }

        Ok(())
    }

    /// Finishes document load lifecycle.
    pub fn finish_document(&mut self) {
        self.phase = PipelineDocumentPhase::Complete;
        self.events
            .push(PipelineEvent::DocumentPhaseChanged(self.phase));

        if self.config.fire_load {
            self.metrics.load_events = self.metrics.load_events.saturating_add(1);
            self.hooks.load();
            self.request_reflow(None, "load", DirtyFlags::lifecycle());
        }
    }

    /// Consumes pipeline and returns run result.
    #[must_use]
    pub fn into_run(self) -> ScriptPipelineRun {
        ScriptPipelineRun {
            metrics: self.metrics,
            events: self.events,
            failures: self.failures,
            scripts: self.script_runs,
            last_summary: self.last_summary,
            phase: self.phase,
        }
    }

    fn flush_async_scripts(&mut self, scheduled: &mut ScheduledVm) -> Result<(), JsRuntimeError> {
        while let Some(descriptor) = self.async_queue.pop_front() {
            self.execute_or_record_error(scheduled, descriptor)?;
        }
        Ok(())
    }

    fn flush_defer_scripts(&mut self, scheduled: &mut ScheduledVm) -> Result<(), JsRuntimeError> {
        while let Some(descriptor) = self.defer_queue.pop_front() {
            self.execute_or_record_error(scheduled, descriptor)?;
        }
        Ok(())
    }

    fn execute_or_record_error(
        &mut self,
        scheduled: &mut ScheduledVm,
        descriptor: ScriptDescriptor,
    ) -> Result<(), JsRuntimeError> {
        match self.execute_script(scheduled, descriptor.clone()) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.metrics.script_failures = self.metrics.script_failures.saturating_add(1);
                self.hooks.script_failed(&descriptor, &error);
                let failure = ScriptExecutionFailure {
                    script_id: descriptor.id,
                    label: descriptor.label.clone(),
                    error: error.to_string(),
                };
                self.failures.push(failure.clone());
                self.events.push(PipelineEvent::ScriptFailed(failure));

                if self.config.continue_on_error {
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }

    fn execute_script(
        &mut self,
        scheduled: &mut ScheduledVm,
        descriptor: ScriptDescriptor,
    ) -> Result<(), JsRuntimeError> {
        let source = self.load_source(&descriptor)?;

        self.events.push(PipelineEvent::ScriptStarted {
            script_id: descriptor.id,
            label: descriptor.label.clone(),
            mode: descriptor.mode,
        });

        self.hooks.before_script(&descriptor);

        if self.config.manage_current_script {
            set_current_script(&mut scheduled.vm, Some(&descriptor));
            self.metrics.current_script_sets = self.metrics.current_script_sets.saturating_add(1);
        }

        let result = execute_source(
            scheduled,
            descriptor.kind,
            &source,
            self.config.drain_after_each_script,
        );

        if self.config.manage_current_script {
            set_current_script(&mut scheduled.vm, None);
            self.metrics.current_script_clears =
                self.metrics.current_script_clears.saturating_add(1);
        }

        let summary = match result {
            Ok(summary) => summary,
            Err(error) => return Err(error),
        };

        self.hooks.after_script(&descriptor);
        self.events.push(PipelineEvent::ScriptFinished {
            script_id: descriptor.id,
            label: descriptor.label.clone(),
        });

        self.bump_execution_counter(descriptor.mode);

        if let Some(mut latest_summary) = summary.clone() {
            if let Some(previous_summary) = &self.last_summary {
                let mut console = previous_summary.console.clone();
                console.extend(latest_summary.console);
                latest_summary.console = console;
            }
            self.last_summary = Some(latest_summary);
        }

        if self.config.conservative_reflow_after_script {
            self.request_reflow(
                Some(descriptor.id),
                format!("script-complete:{}", descriptor.label),
                DirtyFlags::conservative_script(),
            );
        }

        self.script_runs.push(PipelineScriptRun {
            descriptor,
            source_bytes: source.len(),
            summary,
        });

        Ok(())
    }

    fn load_source(&mut self, descriptor: &ScriptDescriptor) -> Result<String, JsRuntimeError> {
        match &descriptor.source {
            ScriptSource::Inline { source } => Ok(source.clone()),
            ScriptSource::External { url } => {
                self.metrics.external_fetches = self.metrics.external_fetches.saturating_add(1);
                let source = self.loader.load_script(url)?;
                self.events.push(PipelineEvent::ScriptFetched {
                    script_id: descriptor.id,
                    label: descriptor.label.clone(),
                    bytes: source.len(),
                });
                Ok(source)
            }
        }
    }

    fn bump_execution_counter(&mut self, mode: ScriptLoadMode) {
        self.metrics.scripts_executed = self.metrics.scripts_executed.saturating_add(1);

        match mode {
            ScriptLoadMode::ParserBlocking => {
                self.metrics.parser_blocking_scripts =
                    self.metrics.parser_blocking_scripts.saturating_add(1);
            }
            ScriptLoadMode::Async => {
                self.metrics.async_scripts_executed =
                    self.metrics.async_scripts_executed.saturating_add(1);
            }
            ScriptLoadMode::Defer => {
                self.metrics.defer_scripts_executed =
                    self.metrics.defer_scripts_executed.saturating_add(1);
            }
            ScriptLoadMode::Dynamic => {
                self.metrics.dynamic_scripts_executed =
                    self.metrics.dynamic_scripts_executed.saturating_add(1);
            }
        }
    }

    fn request_reflow(
        &mut self,
        script_id: Option<u64>,
        reason: impl Into<String>,
        dirty: DirtyFlags,
    ) {
        if dirty.is_empty() {
            return;
        }

        let request = ReflowRequest {
            script_id,
            label: script_id.map_or_else(|| "lifecycle".to_owned(), |id| format!("script:{id}")),
            dirty,
            reason: reason.into(),
        };

        self.metrics.reflow_requests = self.metrics.reflow_requests.saturating_add(1);
        self.hooks.request_reflow(&request);
        self.events.push(PipelineEvent::ReflowRequested(request));
    }
}

fn execute_source(
    scheduled: &mut ScheduledVm,
    kind: ScriptKind,
    source: &str,
    drain_after: bool,
) -> Result<Option<EventLoopRunSummary>, JsRuntimeError> {
    let program = match kind.program_kind() {
        ProgramKind::Script => parse_script(source),
        ProgramKind::Module => parse_module(source),
    }
    .map_err(JsRuntimeError::from_frontend_error)?;

    let bytecode = compile_program(&program, CompileOptions::default())?;
    let _ = scheduled.vm.execute(&bytecode)?;

    if drain_after {
        Ok(Some(scheduled.run_until_idle()?))
    } else {
        Ok(None)
    }
}

fn set_current_script(vm: &mut crate::Vm, descriptor: Option<&ScriptDescriptor>) {
    let document = vm.get_name("document");
    if matches!(document, JsValue::Undefined | JsValue::Null) {
        return;
    }

    let value = descriptor.map_or(JsValue::Null, create_current_script_object);
    document.set_property("currentScript", value);
}

fn create_current_script_object(descriptor: &ScriptDescriptor) -> JsValue {
    let object = JsValue::object();
    object.set_property(
        "id",
        JsValue::String(format!("syljs-script-{}", descriptor.id)),
    );
    object.set_property(
        "src",
        JsValue::String(match &descriptor.source {
            ScriptSource::External { url } => url.clone(),
            ScriptSource::Inline { .. } => String::new(),
        }),
    );
    object.set_property(
        "async",
        JsValue::Boolean(descriptor.mode == ScriptLoadMode::Async),
    );
    object.set_property(
        "defer",
        JsValue::Boolean(descriptor.mode == ScriptLoadMode::Defer),
    );
    object.set_property(
        "type",
        JsValue::String(
            match descriptor.kind {
                ScriptKind::Classic => "text/javascript",
                ScriptKind::Module => "module",
            }
            .to_owned(),
        ),
    );
    object.set_property(
        "parserInserted",
        JsValue::Boolean(descriptor.parser_inserted),
    );
    object
}

/// Runs a complete research script pipeline.
pub fn run_research_script_pipeline(
    scheduled: &mut ScheduledVm,
    scripts: impl IntoIterator<Item = ScriptDescriptor>,
    loader: SharedScriptResourceLoader,
    hooks: SharedScriptPipelineHooks,
    config: ScriptPipelineConfig,
) -> Result<ScriptPipelineRun, JsRuntimeError> {
    let mut pipeline = ScriptPipeline::new(config, loader, hooks);

    for script in scripts {
        pipeline.discover_script(scheduled, script)?;
    }

    pipeline.finish_parsing(scheduled)?;
    pipeline.finish_document();

    Ok(pipeline.into_run())
}
