//! Browser event-loop skeleton for DOM-bound script execution.
//!
//! The current engine still uses the safe intrinsic executor introduced in
//! Module 23, but Module 24 gives script effects a real browser-style task
//! boundary: script task, microtask checkpoint, DOM mutation application, and
//! event-dispatch placeholders. This keeps future V8 integration from leaking
//! runtime state all over the browser shell like soup in a laptop bag.

use std::collections::VecDeque;

use present::RenderDocument;
use tracing::debug;

use crate::js::{apply_dom_binding_effect, DomBindingEffect, ScriptExecution};

/// Browser task kind handled by the JS event loop bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrowserTaskKind {
    /// Execute one classic script program.
    Script,

    /// Apply a captured DOM mutation effect.
    ApplyDomEffect(DomBindingEffect),

    /// Dispatch a captured DOM event placeholder.
    DispatchEvent { target: String, event_type: String },

    /// Timer callback placeholder.
    #[allow(dead_code)]
    Timer { id: u64 },

    /// Animation frame callback placeholder.
    #[allow(dead_code)]
    AnimationFrame { id: u64 },
}

/// One queued task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserTask {
    /// Monotonic id.
    pub id: u64,

    /// Task kind.
    pub kind: BrowserTaskKind,

    /// Human-readable source.
    pub source: String,
}

/// Microtask placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Microtask {
    /// Monotonic id.
    pub id: u64,

    /// Human-readable description.
    pub description: String,
}

/// Event-loop report emitted after script processing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BrowserEventLoopReport {
    /// Tasks queued.
    pub tasks_queued: usize,

    /// Tasks executed.
    pub tasks_executed: usize,

    /// Microtasks queued.
    pub microtasks_queued: usize,

    /// Microtasks executed.
    pub microtasks_executed: usize,

    /// DOM effects applied.
    pub dom_mutations: usize,

    /// DOM effects ignored.
    pub dom_ignored: usize,

    /// Event listener placeholders registered.
    pub registered_listeners: usize,

    /// Script-originated event dispatch placeholders.
    pub dispatched_events: usize,

    /// Diagnostics produced by the binding layer.
    pub diagnostics: Vec<String>,
}

/// Per-document browser event-loop bridge.
#[derive(Debug, Clone)]
pub(crate) struct BrowserEventLoop {
    document_url: String,
    next_task_id: u64,
    next_microtask_id: u64,
    tasks: VecDeque<BrowserTask>,
    microtasks: VecDeque<Microtask>,
    report: BrowserEventLoopReport,
}

impl BrowserEventLoop {
    /// Creates an event loop for one document navigation.
    #[must_use]
    pub(crate) fn new(document_url: impl Into<String>) -> Self {
        Self {
            document_url: document_url.into(),
            next_task_id: 1,
            next_microtask_id: 1,
            tasks: VecDeque::new(),
            microtasks: VecDeque::new(),
            report: BrowserEventLoopReport::default(),
        }
    }

    /// Queues a script task before execution.
    pub(crate) fn queue_script_task(&mut self, source_name: impl Into<String>) {
        self.push_task(BrowserTaskKind::Script, source_name.into());
    }

    /// Processes execution effects at a microtask checkpoint and mutates the render document.
    pub(crate) fn after_script(
        &mut self,
        execution: &ScriptExecution,
        document: &mut RenderDocument,
    ) {
        self.report.registered_listeners = self
            .report
            .registered_listeners
            .saturating_add(execution.registered_listeners);
        self.report.dispatched_events = self
            .report
            .dispatched_events
            .saturating_add(execution.queued_events);

        for effect in &execution.dom_effects {
            self.push_task(
                BrowserTaskKind::ApplyDomEffect(effect.clone()),
                "dom-binding",
            );
            if matches!(effect, DomBindingEffect::RegisterEventListener { .. }) {
                self.push_microtask("listener registration checkpoint");
            }
        }

        self.run_ready_tasks(document);
        self.run_microtask_checkpoint();
    }

    /// Returns and clears the cumulative report.
    #[must_use]
    pub(crate) fn drain_report(&mut self) -> BrowserEventLoopReport {
        std::mem::take(&mut self.report)
    }

    /// Returns document URL associated with this event loop.
    #[must_use]
    pub(crate) fn document_url(&self) -> &str {
        &self.document_url
    }

    fn push_task(&mut self, kind: BrowserTaskKind, source: impl Into<String>) {
        let id = self.next_task_id;
        self.next_task_id = self.next_task_id.wrapping_add(1).max(1);
        self.tasks.push_back(BrowserTask {
            id,
            kind,
            source: source.into(),
        });
        self.report.tasks_queued = self.report.tasks_queued.saturating_add(1);
    }

    fn push_microtask(&mut self, description: impl Into<String>) {
        let id = self.next_microtask_id;
        self.next_microtask_id = self.next_microtask_id.wrapping_add(1).max(1);
        self.microtasks.push_back(Microtask {
            id,
            description: description.into(),
        });
        self.report.microtasks_queued = self.report.microtasks_queued.saturating_add(1);
    }

    fn run_ready_tasks(&mut self, document: &mut RenderDocument) {
        while let Some(task) = self.tasks.pop_front() {
            self.report.tasks_executed = self.report.tasks_executed.saturating_add(1);
            match task.kind {
                BrowserTaskKind::Script => {
                    debug!(source = %task.source, task_id = task.id, "completed script task");
                }
                BrowserTaskKind::ApplyDomEffect(effect) => match &effect {
                    DomBindingEffect::DispatchEvent { target, event_type } => {
                        self.report.dispatched_events =
                            self.report.dispatched_events.saturating_add(1);
                        self.push_task(
                            BrowserTaskKind::DispatchEvent {
                                target: target.clone(),
                                event_type: event_type.clone(),
                            },
                            "dispatchEvent",
                        );
                    }
                    _ => {
                        let applied = apply_dom_binding_effect(document, &effect);
                        self.report.dom_mutations =
                            self.report.dom_mutations.saturating_add(applied.applied);
                        self.report.dom_ignored =
                            self.report.dom_ignored.saturating_add(applied.ignored);
                        self.report.diagnostics.extend(applied.diagnostics);
                    }
                },
                BrowserTaskKind::DispatchEvent { target, event_type } => {
                    debug!(target = %target, event_type = %event_type, "processed script-originated event placeholder");
                }
                BrowserTaskKind::Timer { id } => {
                    debug!(timer_id = id, "processed timer placeholder");
                }
                BrowserTaskKind::AnimationFrame { id } => {
                    debug!(frame_id = id, "processed animation frame placeholder");
                }
            }
        }
    }

    fn run_microtask_checkpoint(&mut self) {
        while let Some(task) = self.microtasks.pop_front() {
            self.report.microtasks_executed = self.report.microtasks_executed.saturating_add(1);
            debug!(microtask_id = task.id, description = %task.description, "processed JS microtask");
        }
    }
}
