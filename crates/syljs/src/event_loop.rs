#![doc = "JavaScript event loop, tasks, microtasks, timers, RAF, and scheduled VM host integration."]

use crate::{
    compile_program, create_promise_constructor, parse_script, CompileOptions, JsNativeFunction,
    JsRuntimeError, JsValue, Vm, VmConfig,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
};

/// Monotonic task id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub u64);

/// Timer id returned by `setTimeout` / `setInterval`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimerId(pub u64);

/// RequestAnimationFrame id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RafId(pub u64);

/// Event-loop runtime configuration.
#[derive(Debug, Clone)]
pub struct EventLoopConfig {
    /// Maximum jobs executed in one `run_until_idle` call.
    pub max_jobs_per_run: u64,

    /// Maximum timer auto-advance iterations.
    pub max_timer_advances: u64,

    /// Enables auto-advancing virtual time to the next timer when queues are empty.
    pub auto_advance_time: bool,
}

impl Default for EventLoopConfig {
    fn default() -> Self {
        Self {
            max_jobs_per_run: 10_000,
            max_timer_advances: 10_000,
            auto_advance_time: true,
        }
    }
}

/// Event-loop metrics for research instrumentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventLoopMetrics {
    /// Tasks scheduled.
    pub tasks_scheduled: u64,

    /// Tasks executed.
    pub tasks_executed: u64,

    /// Microtasks scheduled.
    pub microtasks_scheduled: u64,

    /// Microtasks executed.
    pub microtasks_executed: u64,

    /// Timers scheduled.
    pub timers_scheduled: u64,

    /// Timers fired.
    pub timers_fired: u64,

    /// Timers cancelled.
    pub timers_cancelled: u64,

    /// Intervals rescheduled.
    pub intervals_rescheduled: u64,

    /// Animation frames scheduled.
    pub animation_frames_scheduled: u64,

    /// Animation frames fired.
    pub animation_frames_fired: u64,

    /// Promise reactions scheduled.
    pub promise_reactions_scheduled: u64,

    /// Promise reactions executed.
    pub promise_reactions_executed: u64,
}

/// Summary returned after draining the event loop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventLoopRunSummary {
    /// Jobs executed by this drain.
    pub jobs_executed: u64,

    /// Whether the run stopped because a limit was reached.
    pub hit_limit: bool,

    /// Current virtual time in milliseconds.
    pub virtual_time_ms: u64,

    /// Event-loop metrics snapshot.
    pub event_loop: EventLoopMetrics,

    /// VM metrics snapshot.
    pub vm: crate::VmMetrics,

    /// Captured console lines.
    pub console: Vec<String>,
}

/// Queued job kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedJobKind {
    /// Macrotask.
    Task,

    /// Microtask.
    Microtask,

    /// Timer callback.
    Timer,

    /// Interval callback.
    Interval,

    /// RequestAnimationFrame callback.
    AnimationFrame,

    /// Promise reaction.
    PromiseReaction,
}

/// Queued executable callback.
#[derive(Debug, Clone)]
pub struct QueuedJob {
    /// Job id.
    pub id: TaskId,

    /// Job kind.
    pub kind: QueuedJobKind,

    /// Callback function value.
    pub callback: JsValue,

    /// Callback arguments.
    pub args: Vec<JsValue>,

    /// Associated timer id when applicable.
    pub timer_id: Option<TimerId>,

    /// Repeat delay for intervals.
    pub repeat_delay_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct TimerEntry {
    timer_id: TimerId,
    due_ms: u64,
    delay_ms: u64,
    repeat: bool,
    callback: JsValue,
    args: Vec<JsValue>,
}

#[derive(Debug, Clone)]
struct RafEntry {
    id: RafId,
    callback: JsValue,
}

/// JavaScript event loop.
///
/// This is a deterministic research event loop using virtual time. It does not
/// sleep. When configured, it auto-advances to the next timer deadline while
/// draining, which makes tests and page-load instrumentation stable instead of
/// outsourcing correctness to wall-clock chaos.
#[derive(Debug, Clone)]
pub struct JsEventLoop {
    virtual_time_ms: u64,
    next_task_id: u64,
    next_timer_id: u64,
    next_raf_id: u64,
    tasks: VecDeque<QueuedJob>,
    microtasks: VecDeque<QueuedJob>,
    timers: BTreeMap<TimerId, TimerEntry>,
    cancelled_timers: BTreeSet<TimerId>,
    raf_callbacks: BTreeMap<RafId, RafEntry>,
    metrics: EventLoopMetrics,
}

impl Default for JsEventLoop {
    fn default() -> Self {
        Self {
            virtual_time_ms: 0,
            next_task_id: 1,
            next_timer_id: 1,
            next_raf_id: 1,
            tasks: VecDeque::new(),
            microtasks: VecDeque::new(),
            timers: BTreeMap::new(),
            cancelled_timers: BTreeSet::new(),
            raf_callbacks: BTreeMap::new(),
            metrics: EventLoopMetrics::default(),
        }
    }
}

impl JsEventLoop {
    /// Returns current virtual time in milliseconds.
    #[must_use]
    pub const fn virtual_time_ms(&self) -> u64 {
        self.virtual_time_ms
    }

    /// Returns event-loop metrics.
    #[must_use]
    pub fn metrics(&self) -> EventLoopMetrics {
        self.metrics.clone()
    }

    /// Advances virtual time.
    pub fn advance_time(&mut self, delta_ms: u64) {
        self.virtual_time_ms = self.virtual_time_ms.saturating_add(delta_ms);
        self.promote_due_timers();
    }

    /// Queues a macrotask.
    pub fn queue_task(&mut self, callback: JsValue, args: Vec<JsValue>) -> TaskId {
        let id = self.next_task_id();
        self.tasks.push_back(QueuedJob {
            id,
            kind: QueuedJobKind::Task,
            callback,
            args,
            timer_id: None,
            repeat_delay_ms: None,
        });
        self.metrics.tasks_scheduled = self.metrics.tasks_scheduled.saturating_add(1);
        id
    }

    /// Queues a microtask.
    pub fn queue_microtask(&mut self, callback: JsValue, args: Vec<JsValue>) -> TaskId {
        let id = self.next_task_id();
        self.microtasks.push_back(QueuedJob {
            id,
            kind: QueuedJobKind::Microtask,
            callback,
            args,
            timer_id: None,
            repeat_delay_ms: None,
        });
        self.metrics.microtasks_scheduled = self.metrics.microtasks_scheduled.saturating_add(1);
        id
    }

    /// Queues a Promise reaction microtask.
    pub fn queue_promise_reaction(&mut self, callback: JsValue, args: Vec<JsValue>) -> TaskId {
        let id = self.next_task_id();
        self.microtasks.push_back(QueuedJob {
            id,
            kind: QueuedJobKind::PromiseReaction,
            callback,
            args,
            timer_id: None,
            repeat_delay_ms: None,
        });
        self.metrics.promise_reactions_scheduled =
            self.metrics.promise_reactions_scheduled.saturating_add(1);
        id
    }

    /// Registers a timeout.
    pub fn set_timeout(&mut self, callback: JsValue, delay_ms: u64, args: Vec<JsValue>) -> TimerId {
        self.set_timer(callback, delay_ms, args, false)
    }

    /// Registers an interval.
    pub fn set_interval(
        &mut self,
        callback: JsValue,
        delay_ms: u64,
        args: Vec<JsValue>,
    ) -> TimerId {
        self.set_timer(callback, delay_ms, args, true)
    }

    /// Clears a timeout/interval.
    pub fn clear_timer(&mut self, id: TimerId) -> bool {
        let existed = self.timers.remove(&id).is_some();
        self.cancelled_timers.insert(id);
        if existed {
            self.metrics.timers_cancelled = self.metrics.timers_cancelled.saturating_add(1);
        }
        existed
    }

    /// Registers a requestAnimationFrame callback.
    pub fn request_animation_frame(&mut self, callback: JsValue) -> RafId {
        let id = RafId(self.next_raf_id);
        self.next_raf_id = self.next_raf_id.saturating_add(1);
        self.raf_callbacks.insert(id, RafEntry { id, callback });
        self.metrics.animation_frames_scheduled =
            self.metrics.animation_frames_scheduled.saturating_add(1);
        id
    }

    /// Cancels a requestAnimationFrame callback.
    pub fn cancel_animation_frame(&mut self, id: RafId) -> bool {
        self.raf_callbacks.remove(&id).is_some()
    }

    /// Moves pending animation-frame callbacks into the task queue.
    pub fn tick_animation_frame(&mut self) {
        if self.raf_callbacks.is_empty() {
            return;
        }

        let callbacks = std::mem::take(&mut self.raf_callbacks);
        let timestamp = JsValue::Number(self.virtual_time_ms as f64);

        for (_id, entry) in callbacks {
            let task_id = self.next_task_id();
            self.tasks.push_back(QueuedJob {
                id: task_id,
                kind: QueuedJobKind::AnimationFrame,
                callback: entry.callback,
                args: vec![timestamp.clone()],
                timer_id: None,
                repeat_delay_ms: None,
            });
            let _ = entry.id;
        }
    }

    /// Returns true when the event loop has pending work.
    #[must_use]
    pub fn has_pending_work(&self) -> bool {
        !self.microtasks.is_empty()
            || !self.tasks.is_empty()
            || !self.timers.is_empty()
            || !self.raf_callbacks.is_empty()
    }

    /// Pops the next ready job. Microtasks always run before tasks.
    pub fn pop_ready_job(&mut self, config: &EventLoopConfig) -> Option<QueuedJob> {
        self.promote_due_timers();

        if let Some(job) = self.microtasks.pop_front() {
            return Some(job);
        }

        if let Some(job) = self.tasks.pop_front() {
            return Some(job);
        }

        if !self.raf_callbacks.is_empty() {
            self.tick_animation_frame();
            if let Some(job) = self.tasks.pop_front() {
                return Some(job);
            }
        }

        if config.auto_advance_time {
            if let Some(next_due) = self.next_timer_due_ms() {
                if next_due > self.virtual_time_ms {
                    self.virtual_time_ms = next_due;
                }
                self.promote_due_timers();
                return self
                    .microtasks
                    .pop_front()
                    .or_else(|| self.tasks.pop_front());
            }
        }

        None
    }

    /// Marks metrics for a completed job and reschedules intervals.
    pub fn complete_job(&mut self, job: &QueuedJob) {
        match job.kind {
            QueuedJobKind::Task => {
                self.metrics.tasks_executed = self.metrics.tasks_executed.saturating_add(1);
            }
            QueuedJobKind::Microtask => {
                self.metrics.microtasks_executed =
                    self.metrics.microtasks_executed.saturating_add(1);
            }
            QueuedJobKind::PromiseReaction => {
                self.metrics.promise_reactions_executed =
                    self.metrics.promise_reactions_executed.saturating_add(1);
            }
            QueuedJobKind::Timer => {
                self.metrics.timers_fired = self.metrics.timers_fired.saturating_add(1);
            }
            QueuedJobKind::Interval => {
                self.metrics.timers_fired = self.metrics.timers_fired.saturating_add(1);
                if let (Some(timer_id), Some(delay)) = (job.timer_id, job.repeat_delay_ms) {
                    if !self.cancelled_timers.contains(&timer_id) {
                        self.timers.insert(
                            timer_id,
                            TimerEntry {
                                timer_id,
                                due_ms: self.virtual_time_ms.saturating_add(delay),
                                delay_ms: delay,
                                repeat: true,
                                callback: job.callback.clone(),
                                args: job.args.clone(),
                            },
                        );
                        self.metrics.intervals_rescheduled =
                            self.metrics.intervals_rescheduled.saturating_add(1);
                    }
                }
            }
            QueuedJobKind::AnimationFrame => {
                self.metrics.animation_frames_fired =
                    self.metrics.animation_frames_fired.saturating_add(1);
            }
        }
    }

    fn set_timer(
        &mut self,
        callback: JsValue,
        delay_ms: u64,
        args: Vec<JsValue>,
        repeat: bool,
    ) -> TimerId {
        let id = TimerId(self.next_timer_id);
        self.next_timer_id = self.next_timer_id.saturating_add(1);
        self.cancelled_timers.remove(&id);

        let delay = delay_ms.min(24 * 60 * 60 * 1000);
        self.timers.insert(
            id,
            TimerEntry {
                timer_id: id,
                due_ms: self.virtual_time_ms.saturating_add(delay),
                delay_ms: delay,
                repeat,
                callback,
                args,
            },
        );

        self.metrics.timers_scheduled = self.metrics.timers_scheduled.saturating_add(1);
        id
    }

    fn promote_due_timers(&mut self) {
        let due_ids = self
            .timers
            .iter()
            .filter_map(|(id, timer)| (timer.due_ms <= self.virtual_time_ms).then_some(*id))
            .collect::<Vec<_>>();

        for id in due_ids {
            let Some(timer) = self.timers.remove(&id) else {
                continue;
            };

            if self.cancelled_timers.contains(&timer.timer_id) {
                continue;
            }

            let task_id = self.next_task_id();
            self.tasks.push_back(QueuedJob {
                id: task_id,
                kind: if timer.repeat {
                    QueuedJobKind::Interval
                } else {
                    QueuedJobKind::Timer
                },
                callback: timer.callback,
                args: timer.args,
                timer_id: Some(timer.timer_id),
                repeat_delay_ms: timer.repeat.then_some(timer.delay_ms),
            });
        }
    }

    fn next_timer_due_ms(&self) -> Option<u64> {
        self.timers.values().map(|timer| timer.due_ms).min()
    }

    fn next_task_id(&mut self) -> TaskId {
        let id = TaskId(self.next_task_id);
        self.next_task_id = self.next_task_id.saturating_add(1);
        id
    }
}

/// Installs browser scheduling globals into a VM.
pub fn install_event_loop_globals(vm: &mut Vm, event_loop: Rc<RefCell<JsEventLoop>>) {
    install_timers(vm, event_loop.clone());
    install_microtasks(vm, event_loop.clone());
    install_animation_frame(vm, event_loop.clone());

    let promise = create_promise_constructor(event_loop);
    vm.define_global("Promise", promise);
}

fn install_timers(vm: &mut Vm, event_loop: Rc<RefCell<JsEventLoop>>) {
    let set_timeout_loop = event_loop.clone();
    let set_timeout: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        let Some(callback) = args.first().cloned() else {
            return Ok(JsValue::Number(0.0));
        };

        if callback.as_function().is_none() {
            return Err(JsRuntimeError::new("setTimeout callback is not callable"));
        }

        let delay = args.get(1).map_or(0, js_delay_ms);
        let rest = args.iter().skip(2).cloned().collect::<Vec<_>>();
        let id = set_timeout_loop
            .borrow_mut()
            .set_timeout(callback, delay, rest);
        Ok(JsValue::Number(id.0 as f64))
    });
    vm.define_native_function("setTimeout", set_timeout);

    let set_interval_loop = event_loop.clone();
    let set_interval: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        let Some(callback) = args.first().cloned() else {
            return Ok(JsValue::Number(0.0));
        };

        if callback.as_function().is_none() {
            return Err(JsRuntimeError::new("setInterval callback is not callable"));
        }

        let delay = args.get(1).map_or(0, js_delay_ms);
        let rest = args.iter().skip(2).cloned().collect::<Vec<_>>();
        let id = set_interval_loop
            .borrow_mut()
            .set_interval(callback, delay, rest);
        Ok(JsValue::Number(id.0 as f64))
    });
    vm.define_native_function("setInterval", set_interval);

    let clear_timeout_loop = event_loop.clone();
    let clear_timeout: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        if let Some(id) = args.first().and_then(js_timer_id) {
            clear_timeout_loop.borrow_mut().clear_timer(id);
        }
        Ok(JsValue::Undefined)
    });
    vm.define_native_function("clearTimeout", clear_timeout.clone());
    vm.define_native_function("clearInterval", clear_timeout);
}

fn install_microtasks(vm: &mut Vm, event_loop: Rc<RefCell<JsEventLoop>>) {
    let queue_loop = event_loop;
    let queue_microtask: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        let Some(callback) = args.first().cloned() else {
            return Ok(JsValue::Undefined);
        };

        if callback.as_function().is_none() {
            return Err(JsRuntimeError::new(
                "queueMicrotask callback is not callable",
            ));
        }

        queue_loop
            .borrow_mut()
            .queue_microtask(callback, Vec::new());
        Ok(JsValue::Undefined)
    });
    vm.define_native_function("queueMicrotask", queue_microtask);
}

fn install_animation_frame(vm: &mut Vm, event_loop: Rc<RefCell<JsEventLoop>>) {
    let request_loop = event_loop.clone();
    let request: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        let Some(callback) = args.first().cloned() else {
            return Ok(JsValue::Number(0.0));
        };

        if callback.as_function().is_none() {
            return Err(JsRuntimeError::new(
                "requestAnimationFrame callback is not callable",
            ));
        }

        let id = request_loop.borrow_mut().request_animation_frame(callback);
        Ok(JsValue::Number(id.0 as f64))
    });
    vm.define_native_function("requestAnimationFrame", request);

    let cancel_loop = event_loop;
    let cancel: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        if let Some(id) = args.first().and_then(js_raf_id) {
            cancel_loop.borrow_mut().cancel_animation_frame(id);
        }
        Ok(JsValue::Undefined)
    });
    vm.define_native_function("cancelAnimationFrame", cancel);
}

/// Scheduled VM composed of a VM and deterministic event loop.
pub struct ScheduledVm {
    /// VM.
    pub vm: Vm,

    /// Shared event loop.
    pub event_loop: Rc<RefCell<JsEventLoop>>,

    /// Event loop config.
    pub event_loop_config: EventLoopConfig,
}

impl Default for ScheduledVm {
    fn default() -> Self {
        let event_loop = Rc::new(RefCell::new(JsEventLoop::default()));
        let mut vm = Vm::default();
        install_event_loop_globals(&mut vm, event_loop.clone());

        Self {
            vm,
            event_loop,
            event_loop_config: EventLoopConfig::default(),
        }
    }
}

impl ScheduledVm {
    /// Creates a scheduled VM with explicit configs.
    #[must_use]
    pub fn with_config(vm_config: VmConfig, event_loop_config: EventLoopConfig) -> Self {
        let event_loop = Rc::new(RefCell::new(JsEventLoop::default()));
        let mut vm = Vm::with_config(vm_config);
        install_event_loop_globals(&mut vm, event_loop.clone());

        Self {
            vm,
            event_loop,
            event_loop_config,
        }
    }

    /// Executes a script synchronously, then leaves queued jobs pending.
    pub fn execute_script(
        &mut self,
        source: &str,
    ) -> Result<crate::ExecutionOutcome, JsRuntimeError> {
        let program = parse_script(source).map_err(JsRuntimeError::from_frontend_error)?;
        let function = compile_program(&program, CompileOptions::default())?;
        self.vm.execute(&function)
    }

    /// Drains queued tasks and microtasks until idle or limit.
    pub fn run_until_idle(&mut self) -> Result<EventLoopRunSummary, JsRuntimeError> {
        let mut jobs_executed = 0_u64;
        let mut timer_advances = 0_u64;

        while self.event_loop.borrow().has_pending_work() {
            if jobs_executed >= self.event_loop_config.max_jobs_per_run {
                return Ok(self.summary(jobs_executed, true));
            }

            let before_time = self.event_loop.borrow().virtual_time_ms();

            let job = {
                self.event_loop
                    .borrow_mut()
                    .pop_ready_job(&self.event_loop_config)
            };

            let Some(job) = job else {
                break;
            };

            let after_time = self.event_loop.borrow().virtual_time_ms();
            if after_time > before_time {
                timer_advances = timer_advances.saturating_add(1);
                if timer_advances > self.event_loop_config.max_timer_advances {
                    return Ok(self.summary(jobs_executed, true));
                }
            }

            self.vm
                .call_function(job.callback.clone(), JsValue::Undefined, job.args.clone())?;
            self.event_loop.borrow_mut().complete_job(&job);
            jobs_executed = jobs_executed.saturating_add(1);
        }

        Ok(self.summary(jobs_executed, false))
    }

    fn summary(&self, jobs_executed: u64, hit_limit: bool) -> EventLoopRunSummary {
        EventLoopRunSummary {
            jobs_executed,
            hit_limit,
            virtual_time_ms: self.event_loop.borrow().virtual_time_ms(),
            event_loop: self.event_loop.borrow().metrics(),
            vm: self.vm.metrics(),
            console: self.vm.console().to_vec(),
        }
    }
}

fn js_delay_ms(value: &JsValue) -> u64 {
    let delay = value.to_number();

    if delay.is_finite() && delay > 0.0 {
        delay.min(u64::MAX as f64) as u64
    } else {
        0
    }
}

fn js_timer_id(value: &JsValue) -> Option<TimerId> {
    let number = value.to_number();
    if number.is_finite() && number > 0.0 {
        Some(TimerId(number as u64))
    } else {
        None
    }
}

fn js_raf_id(value: &JsValue) -> Option<RafId> {
    let number = value.to_number();
    if number.is_finite() && number > 0.0 {
        Some(RafId(number as u64))
    } else {
        None
    }
}
