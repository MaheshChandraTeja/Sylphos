use std::rc::Rc;

use crate::{
    install_transfer_globals, install_web_api_globals, install_worker_globals, EventLoopConfig,
    ResearchTransferHost, ResearchWebApiHost, ResearchWorkerHost, ScheduledVm, VmConfig,
    WebApiHost, WebApiResponse,
};

#[test]
fn worker_web_api_transfer_stack_can_be_installed_together_on_main_vm() {
    let web = Rc::new(ResearchWebApiHost::new("https://worker.local/main"));
    web.register_route(
        "/api/data",
        WebApiResponse::text("https://worker.local/api/data", "payload"),
    );

    let transfer = Rc::new(ResearchTransferHost::default());
    let workers = Rc::new(ResearchWorkerHost::default());

    let mut scheduled = ScheduledVm::with_config(VmConfig::default(), EventLoopConfig::default());
    install_web_api_globals(&mut scheduled.vm, scheduled.event_loop.clone(), web.clone());
    install_transfer_globals(&mut scheduled.vm, transfer.clone());
    install_worker_globals(&mut scheduled.vm, scheduled.event_loop.clone(), workers.clone());

    scheduled
        .execute_script(
            r#"
            const buffer = new ArrayBuffer(4);
            const bytes = new Uint8Array(buffer);
            bytes[0] = 99;

            fetch("/api/data").then(function (response) {
                response.text().then(function (text) {
                    console.log(text, buffer.byteLength, bytes[0]);
                });
            });
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["payload 4 99"]);
    assert_eq!(web.metrics().fetch_calls, 1);
    assert_eq!(transfer.metrics().buffers_created, 1);
}
