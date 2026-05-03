use std::rc::Rc;

use crate::{
    eval_script_with_research_canvas_worker, install_canvas_globals, install_cssom_globals,
    install_dom_globals, install_worker_globals, CanvasCommand, CanvasHost, DomHost,
    EventLoopConfig, ResearchCanvasHost, ResearchCssomHost, ResearchDom, ResearchWorkerHost,
    ScheduledVm, VmConfig, WorkerHost, WorkerState,
};

#[test]
fn canvas_2d_records_fill_rect_text_and_data_url() {
    let (summary, dom, _cssom, canvas, _workers) = eval_script_with_research_canvas_worker(
        r##"
        const c = document.createElement("canvas");
        c.id = "main";
        c.width = 640;
        c.height = 360;
        document.body.appendChild(c);

        const ctx = c.getContext("2d");
        ctx.fillStyle = "#ff0000";
        ctx.font = "20px sans-serif";
        ctx.fillRect(10, 20, 30, 40);
        ctx.fillText("Sylphos", 50, 60);

        const m = ctx.measureText("Sylphos");
        console.log(c.width, c.height, ctx.fillStyle);
        console.log(m.width);
        console.log(c.toDataURL("image/png"));
        "##,
    )
    .expect("execute");

    assert_eq!(summary.console[0], "640 360 #ff0000");
    assert!(summary.console[1].parse::<f64>().unwrap_or_default() > 0.0);
    assert!(summary.console[2].starts_with("data:image/png;sylphos-canvas"));
    assert!(dom.get_element_by_id("main").is_some());

    let metrics = canvas.metrics();
    assert_eq!(metrics.canvas_elements_created, 1);
    assert_eq!(metrics.contexts_created, 1);
    assert!(metrics.commands_recorded >= 2);
    assert_eq!(metrics.data_url_exports, 1);

    let commands = canvas.commands();
    assert!(commands
        .iter()
        .any(|cmd| matches!(cmd, CanvasCommand::FillRect { .. })));
    assert!(commands
        .iter()
        .any(|cmd| matches!(cmd, CanvasCommand::FillText { .. })));
}

#[test]
fn canvas_2d_supports_path_image_data_and_transforms() {
    let (summary, _dom, _cssom, canvas, _workers) = eval_script_with_research_canvas_worker(
        r#"
        const c = document.createElement("canvas");
        const ctx = c.getContext("2d");

        ctx.save();
        ctx.translate(10, 20);
        ctx.scale(2, 2);
        ctx.beginPath();
        ctx.moveTo(0, 0);
        ctx.lineTo(10, 10);
        ctx.arc(10, 10, 5, 0, 3.14);
        ctx.stroke();

        const data = ctx.createImageData(4, 4);
        ctx.putImageData(data, 1, 2);
        const read = ctx.getImageData(0, 0, 2, 2);

        console.log(data.width, data.height, data.data.length);
        console.log(read.width, read.height);
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console[0], "4 4 64");
    assert_eq!(summary.console[1], "2 2");

    let metrics = canvas.metrics();
    assert!(metrics.commands_recorded >= 7);
    assert!(metrics.image_data_allocations >= 2);
}

#[test]
fn canvas_to_blob_queues_callback() {
    let (summary, _dom, _cssom, canvas, _workers) = eval_script_with_research_canvas_worker(
        r#"
        const c = document.createElement("canvas");
        const ctx = c.getContext("2d");
        ctx.fillRect(1, 2, 3, 4);
        c.toBlob(function (blob) {
            console.log(blob.type, blob.dataURL);
        });
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console.len(), 1);
    assert!(summary.console[0].starts_with("image/png data:image/png;sylphos-canvas"));
    assert_eq!(canvas.metrics().data_url_exports, 1);
}

#[test]
fn worker_lite_echoes_messages_through_event_loop() {
    let (summary, _dom, _cssom, _canvas, workers) = eval_script_with_research_canvas_worker(
        r#"
        const worker = new Worker("echo-worker.js");
        worker.addEventListener("message", function (event) {
            console.log("worker", event.data);
        });
        worker.postMessage("hello");
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["worker hello"]);

    let metrics = workers.metrics();
    assert_eq!(metrics.workers_created, 1);
    assert_eq!(metrics.messages_to_worker, 1);
    assert_eq!(metrics.messages_to_main, 1);
    assert_eq!(metrics.listeners_added, 1);
    assert_eq!(workers.messages().len(), 2);
}

#[test]
fn worker_onmessage_and_terminate_work() {
    let (summary, _dom, _cssom, _canvas, workers) = eval_script_with_research_canvas_worker(
        r#"
        const worker = new Worker("echo-worker.js");
        worker.onmessage = function (event) {
            console.log("reply", event.data);
        };
        worker.postMessage("first");
        worker.terminate();
        worker.postMessage("second");
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["reply first"]);
    assert_eq!(workers.metrics().workers_terminated, 1);

    let snapshot = workers
        .snapshot(crate::WorkerId(1))
        .expect("worker snapshot");
    assert_eq!(snapshot.state, WorkerState::Terminated);
    assert_eq!(snapshot.outbound_messages, 1);
}

#[test]
fn canvas_worker_installs_cleanly_with_existing_runtime_layers() {
    let cssom = Rc::new(ResearchCssomHost::default());
    let dom = Rc::new(ResearchDom::with_cssom("Canvas", cssom.clone()));
    let canvas = Rc::new(ResearchCanvasHost::default());
    let workers = Rc::new(ResearchWorkerHost::default());

    let mut scheduled = ScheduledVm::with_config(VmConfig::default(), EventLoopConfig::default());

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom.clone(),
    );
    install_canvas_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        canvas.clone(),
        Some(dom.clone()),
    );
    install_worker_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        workers.clone(),
    );

    scheduled
        .execute_script(
            r#"
            const c = document.createElement("canvas");
            c.style.width = "320px";
            const ctx = c.getContext("2d");
            ctx.fillRect(0, 0, 10, 10);

            const worker = new Worker("echo.js");
            worker.onmessage = function (event) {
                console.log("ok", event.data, getComputedStyle(c).width);
            };
            worker.postMessage("canvas");
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["ok canvas 320px"]);
    assert_eq!(canvas.metrics().canvas_elements_created, 1);
    assert_eq!(workers.metrics().workers_created, 1);
}
