#![allow(clippy::too_many_lines)]
#![doc = "Isolated Worker VM host bindings for SylJS."]
#![doc = ""]
#![doc = "Module 37 upgrades Worker-lite from deterministic echo to isolated"]
#![doc = "SylJS VM execution contexts with WorkerGlobalScope, importScripts,"]
#![doc = "worker-local timers/microtasks, and explicit message bridges."]

use crate::{
    compile_program, parse_script, EventLoopConfig, EventLoopRunSummary, JsEventLoop, JsFunction,
    JsHostObject, JsObject, JsObjectKind, JsRuntimeError, JsValue, ScheduledVm, Vm, VmConfig,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fmt,
    rc::Rc,
};

const DEFAULT_ECHO_WORKER: &str = r#"
self.onmessage = function (event) {
    postMessage(event.data);
};
"#;

/// Shared worker host pointer.
pub type SharedWorkerHost = Rc<dyn WorkerHost>;

/// Stable worker id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkerId(pub u64);

/// Worker lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    /// Worker is active.
    Active,

    /// Worker has been terminated.
    Terminated,

    /// Worker initialization or dispatch failed.
    Failed,
}

/// Worker snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerSnapshot {
    /// Worker id.
    pub id: WorkerId,

    /// Script URL.
    pub script_url: String,

    /// Lifecycle state.
    pub state: WorkerState,

    /// Messages sent from main to worker.
    pub outbound_messages: usize,

    /// Messages sent from worker to main.
    pub inbound_messages: usize,

    /// Imported script URLs.
    pub imported_scripts: Vec<String>,

    /// Last error, if any.
    pub last_error: Option<String>,
}

/// Message record.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageRecord {
    /// Worker id.
    pub worker: WorkerId,

    /// Direction.
    pub direction: String,

    /// Message data summary.
    pub data: String,
}

/// Worker event record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerEventRecord {
    /// Worker id.
    pub worker: WorkerId,

    /// Event type.
    pub event_type: String,
}

/// Worker execution record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerExecutionRecord {
    /// Worker id.
    pub worker: WorkerId,

    /// Source URL or operation.
    pub label: String,

    /// VM instructions executed by this step.
    pub instructions: u64,

    /// Event-loop jobs executed by this step.
    pub jobs: u64,

    /// Whether the worker event loop hit a configured limit.
    pub hit_limit: bool,

    /// Error message, if any.
    pub error: Option<String>,
}

/// Delta produced by an isolated worker execution step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerRunDelta {
    /// VM instruction delta.
    pub instructions: u64,

    /// Worker event-loop jobs executed.
    pub jobs: u64,

    /// Microtasks executed.
    pub microtasks: u64,

    /// Promise reactions executed.
    pub promise_reactions: u64,

    /// Timers fired.
    pub timers_fired: u64,

    /// Whether limits were hit.
    pub hit_limit: bool,
}

/// Worker metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerMetrics {
    /// Workers created.
    pub workers_created: u64,

    /// Workers terminated.
    pub workers_terminated: u64,

    /// Workers failed.
    pub workers_failed: u64,

    /// Messages posted to workers.
    pub messages_to_worker: u64,

    /// Messages posted to main.
    pub messages_to_main: u64,

    /// Main listeners added.
    pub listeners_added: u64,

    /// Events queued on main event loop.
    pub events_queued: u64,

    /// Worker scripts loaded.
    pub scripts_loaded: u64,

    /// importScripts calls.
    pub import_scripts_calls: u64,

    /// Individual imported scripts loaded.
    pub imported_scripts_loaded: u64,

    /// Worker VM run steps.
    pub worker_vm_runs: u64,

    /// Worker VM instruction delta.
    pub worker_vm_instructions: u64,

    /// Worker-local event-loop jobs.
    pub worker_event_loop_jobs: u64,

    /// Worker-local timers fired.
    pub worker_timers_fired: u64,

    /// Worker-local microtasks executed.
    pub worker_microtasks_executed: u64,

    /// Worker-local promise reactions executed.
    pub worker_promise_reactions_executed: u64,
}

/// Worker script provider abstraction.
pub trait WorkerScriptProvider {
    /// Loads a worker script by URL.
    fn load_worker_script(&self, url: &str) -> Option<String>;
}

/// In-memory worker script registry for deterministic research runs.
#[derive(Debug, Default)]
pub struct ResearchWorkerScriptRegistry {
    scripts: RefCell<BTreeMap<String, String>>,
}

impl ResearchWorkerScriptRegistry {
    /// Registers or replaces a worker script.
    pub fn register_script(&self, url: impl Into<String>, source: impl Into<String>) {
        self.scripts.borrow_mut().insert(url.into(), source.into());
    }

    /// Removes a script.
    pub fn remove_script(&self, url: &str) -> bool {
        self.scripts.borrow_mut().remove(url).is_some()
    }

    /// Returns all registered script URLs.
    #[must_use]
    pub fn script_urls(&self) -> Vec<String> {
        self.scripts.borrow().keys().cloned().collect()
    }
}

impl WorkerScriptProvider for ResearchWorkerScriptRegistry {
    fn load_worker_script(&self, url: &str) -> Option<String> {
        self.scripts.borrow().get(url).cloned()
    }
}

/// Worker host abstraction.
pub trait WorkerHost {
    /// Creates a worker from script URL.
    fn create_worker(&self, script_url: String) -> WorkerId;

    /// Returns worker snapshot.
    fn snapshot(&self, id: WorkerId) -> Option<WorkerSnapshot>;

    /// Terminates worker.
    fn terminate(&self, id: WorkerId);

    /// Sends message from main thread to worker and returns worker responses.
    fn post_message_to_worker(&self, id: WorkerId, message: JsValue) -> Vec<JsValue>;

    /// Adds main-thread event listener for worker events.
    fn add_main_event_listener(&self, id: WorkerId, event_type: &str, callback: JsValue);

    /// Replaces worker `onmessage`.
    fn set_onmessage(&self, id: WorkerId, callback: JsValue);

    /// Returns listeners for main-thread worker events.
    fn main_event_listeners(&self, id: WorkerId, event_type: &str) -> Vec<JsValue>;

    /// Records event.
    fn record_event(&self, event: WorkerEventRecord);

    /// Metrics.
    fn metrics(&self) -> WorkerMetrics;

    /// Message records.
    fn messages(&self) -> Vec<MessageRecord>;

    /// Event records.
    fn events(&self) -> Vec<WorkerEventRecord>;

    /// Worker execution records.
    fn execution_records(&self) -> Vec<WorkerExecutionRecord>;
}

/// Installs main-thread Worker globals.
pub fn install_worker_globals(
    vm: &mut Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    host: SharedWorkerHost,
) {
    vm.define_global(
        "Worker",
        create_worker_constructor(host.clone(), event_loop),
    );
    vm.define_global(
        "__sylphosWorkerMetrics",
        create_worker_metrics_function(host),
    );
}

fn create_worker_constructor(
    host: SharedWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Worker".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let script_url = args.first().map_or_else(String::new, JsValue::to_js_string);
            let worker = host.create_worker(script_url);
            Ok(create_worker_object(
                host.clone(),
                event_loop.clone(),
                worker,
            ))
        }),
    })
}

fn create_worker_object(
    host: SharedWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    worker: WorkerId,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(WorkerObjectHost {
            host,
            event_loop,
            worker,
        }),
        "[object Worker]",
    );
    object.set_property("__syljsWorkerId", JsValue::Number(worker.0 as f64));
    object
}

#[derive(Clone)]
struct WorkerObjectHost {
    host: SharedWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    worker: WorkerId,
}

impl JsHostObject for WorkerObjectHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "postMessage" => Some(worker_post_message(
                self.host.clone(),
                self.event_loop.clone(),
                self.worker,
            )),
            "terminate" => Some(worker_terminate(self.host.clone(), self.worker)),
            "addEventListener" => Some(worker_add_event_listener(self.host.clone(), self.worker)),
            "removeEventListener" => Some(native_noop("Worker.removeEventListener")),
            "dispatchEvent" => Some(native_return_bool("Worker.dispatchEvent", true)),
            "onmessage" => Some(JsValue::Null),
            "onerror" => Some(JsValue::Null),
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        match key {
            "onmessage" => {
                if value.as_function().is_some() {
                    self.host.set_onmessage(self.worker, value);
                }
                true
            }
            _ => false,
        }
    }
}

fn worker_post_message(
    host: SharedWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    worker: WorkerId,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Worker.postMessage".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let message = args.first().cloned().unwrap_or(JsValue::Undefined);
            let responses = host.post_message_to_worker(worker, message);

            for response in responses {
                queue_worker_message(host.clone(), event_loop.clone(), worker, response);
            }

            Ok(JsValue::Undefined)
        }),
    })
}

fn worker_terminate(host: SharedWorkerHost, worker: WorkerId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Worker.terminate".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            host.terminate(worker);
            Ok(JsValue::Undefined)
        }),
    })
}

fn worker_add_event_listener(host: SharedWorkerHost, worker: WorkerId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Worker.addEventListener".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let event_type = args.first().map_or_else(String::new, JsValue::to_js_string);
            let Some(callback) = args.get(1).cloned() else {
                return Ok(JsValue::Undefined);
            };

            if callback.as_function().is_none() {
                return Err(JsRuntimeError::new("worker listener is not callable"));
            }

            host.add_main_event_listener(worker, &event_type, callback);
            Ok(JsValue::Undefined)
        }),
    })
}

fn queue_worker_message(
    host: SharedWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    worker: WorkerId,
    data: JsValue,
) {
    let event = create_message_event(data);
    host.record_event(WorkerEventRecord {
        worker,
        event_type: "message".to_owned(),
    });

    for callback in host.main_event_listeners(worker, "message") {
        event_loop
            .borrow_mut()
            .queue_task(callback, vec![event.clone()]);
    }
}

fn create_message_event(data: JsValue) -> JsValue {
    let event = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
    event.set_property("type", JsValue::String("message".to_owned()));
    event.set_property("data", data);
    event
}

fn create_worker_metrics_function(host: SharedWorkerHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosWorkerMetrics".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let metrics = host.metrics();
            let object = JsValue::object();
            object.set_property(
                "workersCreated",
                JsValue::Number(metrics.workers_created as f64),
            );
            object.set_property(
                "workersTerminated",
                JsValue::Number(metrics.workers_terminated as f64),
            );
            object.set_property(
                "workersFailed",
                JsValue::Number(metrics.workers_failed as f64),
            );
            object.set_property(
                "messagesToWorker",
                JsValue::Number(metrics.messages_to_worker as f64),
            );
            object.set_property(
                "messagesToMain",
                JsValue::Number(metrics.messages_to_main as f64),
            );
            object.set_property(
                "scriptsLoaded",
                JsValue::Number(metrics.scripts_loaded as f64),
            );
            object.set_property(
                "importScriptsCalls",
                JsValue::Number(metrics.import_scripts_calls as f64),
            );
            object.set_property(
                "workerVmInstructions",
                JsValue::Number(metrics.worker_vm_instructions as f64),
            );
            Ok(object)
        }),
    })
}

fn native_noop(name: &'static str) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Undefined)),
    })
}

fn native_return_bool(name: &'static str, value: bool) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Boolean(value))),
    })
}

/// Installs the WorkerGlobalScope API into an isolated worker VM.
pub fn install_worker_scope_globals(
    vm: &mut Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    scope: Rc<WorkerScopeState>,
) -> JsValue {
    let self_object = create_worker_scope_object(scope.clone(), event_loop);
    vm.define_global("self", self_object.clone());
    vm.define_global("globalThis", self_object.clone());
    vm.define_global("postMessage", scope_post_message(scope.clone()));
    vm.define_global("importScripts", scope_import_scripts(scope.clone()));
    vm.define_global("close", scope_close(scope.clone()));
    vm.define_global("addEventListener", scope_add_event_listener(scope.clone()));
    vm.define_global(
        "removeEventListener",
        native_noop("WorkerGlobalScope.removeEventListener"),
    );
    vm.define_global(
        "dispatchEvent",
        native_return_bool("WorkerGlobalScope.dispatchEvent", true),
    );
    vm.define_global("DedicatedWorkerGlobalScope", JsValue::object());
    self_object
}

fn create_worker_scope_object(
    scope: Rc<WorkerScopeState>,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(WorkerScopeHost { scope, event_loop }),
        "[object DedicatedWorkerGlobalScope]",
    );
    object.set_property("name", JsValue::String("SylJSWorker".to_owned()));
    object
}

#[derive(Clone)]
struct WorkerScopeHost {
    scope: Rc<WorkerScopeState>,
    event_loop: Rc<RefCell<JsEventLoop>>,
}

impl JsHostObject for WorkerScopeHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "self" | "globalThis" => None,
            "postMessage" => Some(scope_post_message(self.scope.clone())),
            "importScripts" => Some(scope_import_scripts(self.scope.clone())),
            "close" => Some(scope_close(self.scope.clone())),
            "addEventListener" => Some(scope_add_event_listener(self.scope.clone())),
            "removeEventListener" => Some(native_noop("WorkerGlobalScope.removeEventListener")),
            "dispatchEvent" => Some(native_return_bool("WorkerGlobalScope.dispatchEvent", true)),
            "onmessage" => Some(
                self.scope
                    .onmessage
                    .borrow()
                    .clone()
                    .unwrap_or(JsValue::Null),
            ),
            "location" => Some(create_worker_location(self.scope.script_url.clone())),
            "navigator" => Some(create_worker_navigator()),
            _ => {
                let _ = &self.event_loop;
                None
            }
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        match key {
            "onmessage" => {
                if value.as_function().is_some() {
                    *self.scope.onmessage.borrow_mut() = Some(value);
                } else {
                    *self.scope.onmessage.borrow_mut() = None;
                }
                true
            }
            _ => false,
        }
    }
}

fn scope_post_message(scope: Rc<WorkerScopeState>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "WorkerGlobalScope.postMessage".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let value = args.first().cloned().unwrap_or(JsValue::Undefined);
            scope.outbox.borrow_mut().push(value);
            Ok(JsValue::Undefined)
        }),
    })
}

fn scope_import_scripts(scope: Rc<WorkerScopeState>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "WorkerGlobalScope.importScripts".to_owned(),
        function: Rc::new(move |vm, _this, args| {
            scope
                .import_scripts_calls
                .set(scope.import_scripts_calls.get().saturating_add(1));

            for arg in args {
                let url = arg.to_js_string();
                let Some(source) = scope.scripts.load_worker_script(&url) else {
                    let message = format!("importScripts could not load `{url}`");
                    scope.errors.borrow_mut().push(message.clone());
                    return Err(JsRuntimeError::new(message));
                };

                scope.imported_scripts.borrow_mut().push(url.clone());
                scope
                    .imported_scripts_loaded
                    .set(scope.imported_scripts_loaded.get().saturating_add(1));
                execute_source_on_vm(vm, &source)?;
            }

            Ok(JsValue::Undefined)
        }),
    })
}

fn scope_close(scope: Rc<WorkerScopeState>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "WorkerGlobalScope.close".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            scope.close_requested.set(true);
            Ok(JsValue::Undefined)
        }),
    })
}

fn scope_add_event_listener(scope: Rc<WorkerScopeState>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "WorkerGlobalScope.addEventListener".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let event_type = args.first().map_or_else(String::new, JsValue::to_js_string);
            let Some(callback) = args.get(1).cloned() else {
                return Ok(JsValue::Undefined);
            };

            if callback.as_function().is_none() {
                return Err(JsRuntimeError::new("worker scope listener is not callable"));
            }

            scope
                .listeners
                .borrow_mut()
                .entry(event_type)
                .or_default()
                .push(callback);
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_worker_location(script_url: String) -> JsValue {
    let object = JsValue::object();
    object.set_property("href", JsValue::String(script_url));
    object
}

fn create_worker_navigator() -> JsValue {
    let object = JsValue::object();
    object.set_property(
        "userAgent",
        JsValue::String("Sylphos/SylJS IsolatedWorker".to_owned()),
    );
    object.set_property("hardwareConcurrency", JsValue::Number(4.0));
    object
}

fn execute_source_on_vm(vm: &mut Vm, source: &str) -> Result<(), JsRuntimeError> {
    let program = parse_script(source).map_err(JsRuntimeError::from_frontend_error)?;
    let bytecode = compile_program(&program, Default::default())?;
    let _ = vm.execute(&bytecode)?;
    Ok(())
}

/// Shared state backing a worker global scope.
pub struct WorkerScopeState {
    script_url: String,
    scripts: Rc<dyn WorkerScriptProvider>,
    outbox: RefCell<Vec<JsValue>>,
    onmessage: RefCell<Option<JsValue>>,
    listeners: RefCell<BTreeMap<String, Vec<JsValue>>>,
    imported_scripts: RefCell<Vec<String>>,
    errors: RefCell<Vec<String>>,
    close_requested: Cell<bool>,
    import_scripts_calls: Cell<u64>,
    imported_scripts_loaded: Cell<u64>,
}

impl WorkerScopeState {
    fn new(script_url: String, scripts: Rc<dyn WorkerScriptProvider>) -> Self {
        Self {
            script_url,
            scripts,
            outbox: RefCell::new(Vec::new()),
            onmessage: RefCell::new(None),
            listeners: RefCell::new(BTreeMap::new()),
            imported_scripts: RefCell::new(Vec::new()),
            errors: RefCell::new(Vec::new()),
            close_requested: Cell::new(false),
            import_scripts_calls: Cell::new(0),
            imported_scripts_loaded: Cell::new(0),
        }
    }

    fn drain_outbox(&self) -> Vec<JsValue> {
        self.outbox.borrow_mut().drain(..).collect()
    }

    fn imported_scripts(&self) -> Vec<String> {
        self.imported_scripts.borrow().clone()
    }

    fn last_error(&self) -> Option<String> {
        self.errors.borrow().last().cloned()
    }
}

struct IsolatedWorkerRuntime {
    scheduled: ScheduledVm,
    scope: Rc<WorkerScopeState>,
    self_value: JsValue,
    last_vm_instructions: u64,
    last_jobs: u64,
    last_microtasks: u64,
    last_promise_reactions: u64,
    last_timers: u64,
}

impl IsolatedWorkerRuntime {
    fn boot(
        script_url: String,
        scripts: Rc<dyn WorkerScriptProvider>,
        vm_config: VmConfig,
        event_loop_config: EventLoopConfig,
    ) -> (Self, Option<JsRuntimeError>, WorkerRunDelta) {
        let source = scripts
            .load_worker_script(&script_url)
            .unwrap_or_else(|| DEFAULT_ECHO_WORKER.to_owned());

        let scope = Rc::new(WorkerScopeState::new(script_url.clone(), scripts));
        let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);
        let self_value = install_worker_scope_globals(
            &mut scheduled.vm,
            scheduled.event_loop.clone(),
            scope.clone(),
        );

        let init_error = execute_source_on_vm(&mut scheduled.vm, &source).err();

        let summary_result = scheduled.run_until_idle();
        let (summary, drain_error) = match summary_result {
            Ok(summary) => (Some(summary), None),
            Err(error) => (None, Some(error)),
        };

        let error = init_error.or(drain_error);

        if let Some(error) = &error {
            scope.errors.borrow_mut().push(error.to_string());
        }

        let mut runtime = Self {
            scheduled,
            scope,
            self_value,
            last_vm_instructions: 0,
            last_jobs: 0,
            last_microtasks: 0,
            last_promise_reactions: 0,
            last_timers: 0,
        };

        let delta = summary
            .as_ref()
            .map_or_else(WorkerRunDelta::default, |summary| {
                runtime.delta_from_summary(summary)
            });

        (runtime, error, delta)
    }

    fn dispatch_message(
        &mut self,
        data: JsValue,
    ) -> (Vec<JsValue>, Option<JsRuntimeError>, WorkerRunDelta) {
        let event = create_message_event(data);
        let mut error = None;

        for callback in self.message_callbacks() {
            if let Err(err) = self.scheduled.vm.call_function(
                callback,
                self.self_value.clone(),
                vec![event.clone()],
            ) {
                self.scope.errors.borrow_mut().push(err.to_string());
                error = Some(err);
                break;
            }
        }

        let summary = match self.scheduled.run_until_idle() {
            Ok(summary) => Some(summary),
            Err(err) => {
                self.scope.errors.borrow_mut().push(err.to_string());
                error = Some(err);
                None
            }
        };

        let delta = summary
            .as_ref()
            .map_or_else(WorkerRunDelta::default, |summary| {
                self.delta_from_summary(summary)
            });

        (self.scope.drain_outbox(), error, delta)
    }

    fn message_callbacks(&self) -> Vec<JsValue> {
        let mut callbacks = Vec::new();

        if let Some(callback) = self.scope.onmessage.borrow().clone() {
            callbacks.push(callback);
        }

        callbacks.extend(
            self.scope
                .listeners
                .borrow()
                .get("message")
                .cloned()
                .unwrap_or_default(),
        );

        callbacks
    }

    fn delta_from_summary(&mut self, summary: &EventLoopRunSummary) -> WorkerRunDelta {
        let instructions = summary
            .vm
            .instructions_executed
            .saturating_sub(self.last_vm_instructions);
        let jobs = summary.jobs_executed.saturating_sub(self.last_jobs);
        let microtasks = summary
            .event_loop
            .microtasks_executed
            .saturating_sub(self.last_microtasks);
        let promise_reactions = summary
            .event_loop
            .promise_reactions_executed
            .saturating_sub(self.last_promise_reactions);
        let timers_fired = summary
            .event_loop
            .timers_fired
            .saturating_sub(self.last_timers);

        self.last_vm_instructions = summary.vm.instructions_executed;
        self.last_jobs = summary.jobs_executed;
        self.last_microtasks = summary.event_loop.microtasks_executed;
        self.last_promise_reactions = summary.event_loop.promise_reactions_executed;
        self.last_timers = summary.event_loop.timers_fired;

        WorkerRunDelta {
            instructions,
            jobs,
            microtasks,
            promise_reactions,
            timers_fired,
            hit_limit: summary.hit_limit,
        }
    }
}

struct WorkerStateData {
    id: WorkerId,
    script_url: String,
    state: WorkerState,
    outbound_messages: usize,
    inbound_messages: usize,
    runtime: Option<IsolatedWorkerRuntime>,
}

impl WorkerStateData {
    fn imported_scripts(&self) -> Vec<String> {
        self.runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.scope.imported_scripts())
    }

    fn last_error(&self) -> Option<String> {
        self.runtime
            .as_ref()
            .and_then(|runtime| runtime.scope.last_error())
    }
}

/// Deterministic research Worker host with isolated worker VMs.
pub struct ResearchWorkerHost {
    scripts: Rc<ResearchWorkerScriptRegistry>,
    inner: RefCell<ResearchWorkerInner>,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
}

impl fmt::Debug for ResearchWorkerHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResearchWorkerHost")
            .field("registered_scripts", &self.scripts.script_urls())
            .field("metrics", &self.inner.borrow().metrics)
            .finish()
    }
}

struct ResearchWorkerInner {
    next_worker: u64,
    workers: BTreeMap<WorkerId, WorkerStateData>,
    listeners: BTreeMap<(WorkerId, String), Vec<JsValue>>,
    messages: Vec<MessageRecord>,
    events: Vec<WorkerEventRecord>,
    execution_records: Vec<WorkerExecutionRecord>,
    metrics: WorkerMetrics,
}

impl Default for ResearchWorkerHost {
    fn default() -> Self {
        Self::new(VmConfig::default(), EventLoopConfig::default())
    }
}

impl ResearchWorkerHost {
    /// Creates a worker host with explicit VM and event-loop configs.
    #[must_use]
    pub fn new(vm_config: VmConfig, event_loop_config: EventLoopConfig) -> Self {
        Self {
            scripts: Rc::new(ResearchWorkerScriptRegistry::default()),
            inner: RefCell::new(ResearchWorkerInner {
                next_worker: 1,
                workers: BTreeMap::new(),
                listeners: BTreeMap::new(),
                messages: Vec::new(),
                events: Vec::new(),
                execution_records: Vec::new(),
                metrics: WorkerMetrics::default(),
            }),
            vm_config,
            event_loop_config,
        }
    }

    /// Registers a worker script.
    pub fn register_script(&self, url: impl Into<String>, source: impl Into<String>) {
        self.scripts.register_script(url, source);
    }

    /// Returns registered script URLs.
    #[must_use]
    pub fn script_urls(&self) -> Vec<String> {
        self.scripts.script_urls()
    }

    fn accumulate_delta(metrics: &mut WorkerMetrics, delta: &WorkerRunDelta) {
        metrics.worker_vm_runs = metrics.worker_vm_runs.saturating_add(1);
        metrics.worker_vm_instructions = metrics
            .worker_vm_instructions
            .saturating_add(delta.instructions);
        metrics.worker_event_loop_jobs = metrics.worker_event_loop_jobs.saturating_add(delta.jobs);
        metrics.worker_microtasks_executed = metrics
            .worker_microtasks_executed
            .saturating_add(delta.microtasks);
        metrics.worker_promise_reactions_executed = metrics
            .worker_promise_reactions_executed
            .saturating_add(delta.promise_reactions);
        metrics.worker_timers_fired = metrics
            .worker_timers_fired
            .saturating_add(delta.timers_fired);
    }

    fn push_execution_record(
        inner: &mut ResearchWorkerInner,
        worker: WorkerId,
        label: impl Into<String>,
        delta: &WorkerRunDelta,
        error: Option<String>,
    ) {
        inner.execution_records.push(WorkerExecutionRecord {
            worker,
            label: label.into(),
            instructions: delta.instructions,
            jobs: delta.jobs,
            hit_limit: delta.hit_limit,
            error,
        });
    }
}

impl WorkerHost for ResearchWorkerHost {
    fn create_worker(&self, script_url: String) -> WorkerId {
        let id = {
            let mut inner = self.inner.borrow_mut();
            let id = WorkerId(inner.next_worker);
            inner.next_worker = inner.next_worker.saturating_add(1);
            inner.metrics.workers_created = inner.metrics.workers_created.saturating_add(1);
            id
        };

        let (runtime, init_error, delta) = IsolatedWorkerRuntime::boot(
            script_url.clone(),
            self.scripts.clone(),
            self.vm_config.clone(),
            self.event_loop_config.clone(),
        );

        let imported = runtime.scope.imported_scripts_loaded.get();
        let import_calls = runtime.scope.import_scripts_calls.get();

        let mut inner = self.inner.borrow_mut();

        inner.metrics.scripts_loaded = inner.metrics.scripts_loaded.saturating_add(1);
        inner.metrics.import_scripts_calls = inner
            .metrics
            .import_scripts_calls
            .saturating_add(import_calls);
        inner.metrics.imported_scripts_loaded = inner
            .metrics
            .imported_scripts_loaded
            .saturating_add(imported);
        Self::accumulate_delta(&mut inner.metrics, &delta);

        let state = if init_error.is_some() {
            inner.metrics.workers_failed = inner.metrics.workers_failed.saturating_add(1);
            WorkerState::Failed
        } else {
            WorkerState::Active
        };

        let error_string = init_error.as_ref().map(ToString::to_string);
        Self::push_execution_record(
            &mut inner,
            id,
            format!("boot:{script_url}"),
            &delta,
            error_string,
        );

        inner.workers.insert(
            id,
            WorkerStateData {
                id,
                script_url,
                state,
                outbound_messages: 0,
                inbound_messages: 0,
                runtime: Some(runtime),
            },
        );

        id
    }

    fn snapshot(&self, id: WorkerId) -> Option<WorkerSnapshot> {
        self.inner
            .borrow()
            .workers
            .get(&id)
            .map(|worker| WorkerSnapshot {
                id: worker.id,
                script_url: worker.script_url.clone(),
                state: worker.state,
                outbound_messages: worker.outbound_messages,
                inbound_messages: worker.inbound_messages,
                imported_scripts: worker.imported_scripts(),
                last_error: worker.last_error(),
            })
    }

    fn terminate(&self, id: WorkerId) {
        let mut inner = self.inner.borrow_mut();

        if let Some(worker) = inner.workers.get_mut(&id) {
            if worker.state != WorkerState::Terminated {
                worker.state = WorkerState::Terminated;
                worker.runtime = None;
                inner.metrics.workers_terminated =
                    inner.metrics.workers_terminated.saturating_add(1);
            }
        }
    }

    fn post_message_to_worker(&self, id: WorkerId, message: JsValue) -> Vec<JsValue> {
        let mut inner = self.inner.borrow_mut();

        let Some(mut worker) = inner.workers.remove(&id) else {
            return Vec::new();
        };

        if worker.state != WorkerState::Active {
            inner.workers.insert(id, worker);
            return Vec::new();
        }

        worker.outbound_messages = worker.outbound_messages.saturating_add(1);
        inner.metrics.messages_to_worker = inner.metrics.messages_to_worker.saturating_add(1);

        inner.messages.push(MessageRecord {
            worker: id,
            direction: "main-to-worker".to_owned(),
            data: message.to_js_string(),
        });

        let Some(runtime) = worker.runtime.as_mut() else {
            worker.state = WorkerState::Failed;
            inner.metrics.workers_failed = inner.metrics.workers_failed.saturating_add(1);
            inner.workers.insert(id, worker);
            return Vec::new();
        };

        let before_imports = runtime.scope.imported_scripts_loaded.get();
        let before_import_calls = runtime.scope.import_scripts_calls.get();

        let (responses, error, delta) = runtime.dispatch_message(message);
        let after_imports = runtime.scope.imported_scripts_loaded.get();
        let after_import_calls = runtime.scope.import_scripts_calls.get();

        inner.metrics.imported_scripts_loaded = inner
            .metrics
            .imported_scripts_loaded
            .saturating_add(after_imports.saturating_sub(before_imports));
        inner.metrics.import_scripts_calls = inner
            .metrics
            .import_scripts_calls
            .saturating_add(after_import_calls.saturating_sub(before_import_calls));

        Self::accumulate_delta(&mut inner.metrics, &delta);

        if let Some(error) = error {
            worker.state = WorkerState::Failed;
            inner.metrics.workers_failed = inner.metrics.workers_failed.saturating_add(1);
            Self::push_execution_record(&mut inner, id, "message", &delta, Some(error.to_string()));
            inner.workers.insert(id, worker);
            return Vec::new();
        }

        for response in &responses {
            worker.inbound_messages = worker.inbound_messages.saturating_add(1);
            inner.metrics.messages_to_main = inner.metrics.messages_to_main.saturating_add(1);
            inner.messages.push(MessageRecord {
                worker: id,
                direction: "worker-to-main".to_owned(),
                data: response.to_js_string(),
            });
        }

        if runtime.scope.close_requested.get() {
            worker.state = WorkerState::Terminated;
            worker.runtime = None;
            inner.metrics.workers_terminated = inner.metrics.workers_terminated.saturating_add(1);
        }

        Self::push_execution_record(&mut inner, id, "message", &delta, None);

        inner.workers.insert(id, worker);

        responses
    }

    fn add_main_event_listener(&self, id: WorkerId, event_type: &str, callback: JsValue) {
        let mut inner = self.inner.borrow_mut();
        inner
            .listeners
            .entry((id, event_type.to_owned()))
            .or_default()
            .push(callback);
        inner.metrics.listeners_added = inner.metrics.listeners_added.saturating_add(1);
    }

    fn set_onmessage(&self, id: WorkerId, callback: JsValue) {
        self.add_main_event_listener(id, "message", callback);
    }

    fn main_event_listeners(&self, id: WorkerId, event_type: &str) -> Vec<JsValue> {
        self.inner
            .borrow()
            .listeners
            .get(&(id, event_type.to_owned()))
            .cloned()
            .unwrap_or_default()
    }

    fn record_event(&self, event: WorkerEventRecord) {
        let mut inner = self.inner.borrow_mut();
        inner.events.push(event);
        inner.metrics.events_queued = inner.metrics.events_queued.saturating_add(1);
    }

    fn metrics(&self) -> WorkerMetrics {
        self.inner.borrow().metrics.clone()
    }

    fn messages(&self) -> Vec<MessageRecord> {
        self.inner.borrow().messages.clone()
    }

    fn events(&self) -> Vec<WorkerEventRecord> {
        self.inner.borrow().events.clone()
    }

    fn execution_records(&self) -> Vec<WorkerExecutionRecord> {
        self.inner.borrow().execution_records.clone()
    }
}
