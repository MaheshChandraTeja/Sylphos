use std::rc::Rc;

use crate::{
    eval_script_with_research_isolated_worker, install_worker_globals, EventLoopConfig,
    ResearchWorkerHost, ScheduledVm, VmConfig, WorkerId, WorkerState,
};

#[test]
fn isolated_worker_executes_registered_script_on_message() {
    let (summary, workers) = eval_script_with_research_isolated_worker(
        r#"
        const worker = new Worker("worker.js");
        worker.onmessage = function (event) {
            console.log("main", event.data);
        };
        worker.postMessage("hello");
        "#,
        &[(
            "worker.js",
            r#"
            self.onmessage = function (event) {
                postMessage("reply:" + event.data);
            };
            "#,
        )],
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["main reply:hello"]);

    let metrics = workers.metrics();
    assert_eq!(metrics.workers_created, 1);
    assert_eq!(metrics.messages_to_worker, 1);
    assert_eq!(metrics.messages_to_main, 1);
    assert!(metrics.worker_vm_instructions > 0);
    assert_eq!(metrics.scripts_loaded, 1);

    let snapshot = workers.snapshot(WorkerId(1)).expect("snapshot");
    assert_eq!(snapshot.state, WorkerState::Active);
    assert_eq!(snapshot.outbound_messages, 1);
    assert_eq!(snapshot.inbound_messages, 1);
}

#[test]
fn import_scripts_loads_helper_into_same_worker_vm() {
    let workers = Rc::new(ResearchWorkerHost::default());
    workers.register_script(
        "helper.js",
        r#"
        function transform(value) {
            return "helper:" + value;
        }
        "#,
    );
    workers.register_script(
        "worker.js",
        r#"
        importScripts("helper.js");

        self.onmessage = function (event) {
            postMessage(transform(event.data));
        };
        "#,
    );

    let mut scheduled = ScheduledVm::default();
    install_worker_globals(&mut scheduled.vm, scheduled.event_loop.clone(), workers.clone());

    scheduled
        .execute_script(
            r#"
            const worker = new Worker("worker.js");
            worker.addEventListener("message", function (event) {
                console.log(event.data);
            });
            worker.postMessage("data");
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["helper:data"]);
    assert_eq!(workers.metrics().import_scripts_calls, 1);
    assert_eq!(workers.metrics().imported_scripts_loaded, 1);

    let snapshot = workers.snapshot(WorkerId(1)).expect("snapshot");
    assert_eq!(snapshot.imported_scripts, vec!["helper.js"]);
}

#[test]
fn worker_local_microtasks_and_timers_are_drained_before_response_returns() {
    let (summary, workers) = eval_script_with_research_isolated_worker(
        r#"
        const worker = new Worker("worker.js");
        worker.onmessage = function (event) {
            console.log(event.data);
        };
        worker.postMessage("x");
        "#,
        &[(
            "worker.js",
            r#"
            self.onmessage = function (event) {
                setTimeout(function () {
                    postMessage("timer:" + event.data);
                }, 5);

                queueMicrotask(function () {
                    postMessage("micro:" + event.data);
                });
            };
            "#,
        )],
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["micro:x", "timer:x"]);
    assert!(workers.metrics().worker_timers_fired >= 1);
    assert!(workers.metrics().worker_microtasks_executed >= 1);
}

#[test]
fn missing_worker_script_uses_backward_compatible_echo_worker() {
    let (summary, workers) = eval_script_with_research_isolated_worker(
        r#"
        const worker = new Worker("missing-worker.js");
        worker.onmessage = function (event) {
            console.log("echo", event.data);
        };
        worker.postMessage("still-compatible");
        "#,
        &[],
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["echo still-compatible"]);
    assert_eq!(workers.metrics().workers_failed, 0);
    assert_eq!(workers.metrics().messages_to_main, 1);
}

#[test]
fn worker_close_terminates_after_message() {
    let (summary, workers) = eval_script_with_research_isolated_worker(
        r#"
        const worker = new Worker("worker.js");
        worker.onmessage = function (event) {
            console.log(event.data);
        };
        worker.postMessage("first");
        worker.postMessage("second");
        "#,
        &[(
            "worker.js",
            r#"
            self.onmessage = function (event) {
                postMessage(event.data);
                close();
            };
            "#,
        )],
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["first"]);

    let snapshot = workers.snapshot(WorkerId(1)).expect("snapshot");
    assert_eq!(snapshot.state, WorkerState::Terminated);
    assert_eq!(workers.metrics().workers_terminated, 1);
    assert_eq!(workers.metrics().messages_to_worker, 1);
}

#[test]
fn import_scripts_missing_script_marks_worker_failed() {
    let (summary, workers) = eval_script_with_research_isolated_worker(
        r#"
        const worker = new Worker("worker.js");
        worker.onmessage = function (event) {
            console.log(event.data);
        };
        worker.postMessage("x");
        "#,
        &[(
            "worker.js",
            r#"
            importScripts("missing-helper.js");
            self.onmessage = function (event) {
                postMessage(event.data);
            };
            "#,
        )],
    )
    .expect("execute");

    assert!(summary.console.is_empty());
    assert_eq!(workers.metrics().workers_failed, 1);

    let snapshot = workers.snapshot(WorkerId(1)).expect("snapshot");
    assert_eq!(snapshot.state, WorkerState::Failed);
    assert!(snapshot
        .last_error
        .unwrap_or_default()
        .contains("importScripts could not load"));
}

#[test]
fn worker_metrics_are_exposed_to_javascript() {
    let (summary, _workers) = eval_script_with_research_isolated_worker(
        r#"
        const worker = new Worker("worker.js");
        worker.onmessage = function (event) {
            const metrics = __sylphosWorkerMetrics();
            console.log(metrics.workersCreated, metrics.messagesToMain, event.data);
        };
        worker.postMessage("ok");
        "#,
        &[(
            "worker.js",
            r#"
            self.onmessage = function (event) {
                postMessage(event.data);
            };
            "#,
        )],
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["1 1 ok"]);
}

#[test]
fn custom_worker_host_config_limits_bad_worker_without_crashing_main() {
    let workers = Rc::new(ResearchWorkerHost::new(
        VmConfig {
            instruction_budget: 2_000,
            max_call_depth: 64,
        },
        EventLoopConfig {
            max_jobs_per_run: 256,
            max_timer_advances: 256,
            auto_advance_time: true,
        },
    ));

    workers.register_script(
        "worker.js",
        r#"
        self.onmessage = function (event) {
            let i = 0;
            while (i < 1000000) {
                i = i + 1;
            }
            postMessage("done");
        };
        "#,
    );

    let mut scheduled = ScheduledVm::default();
    install_worker_globals(&mut scheduled.vm, scheduled.event_loop.clone(), workers.clone());

    scheduled
        .execute_script(
            r#"
            const worker = new Worker("worker.js");
            worker.onmessage = function (event) {
                console.log(event.data);
            };
            worker.postMessage("x");
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("main drain");

    assert!(summary.console.is_empty());
    assert_eq!(workers.snapshot(WorkerId(1)).unwrap().state, WorkerState::Failed);
    assert!(workers.metrics().workers_failed >= 1);
    assert!(!workers.execution_records().is_empty());
}
