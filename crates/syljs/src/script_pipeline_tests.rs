use std::rc::Rc;

use crate::{
    install_cssom_globals, install_dom_globals, run_research_script_pipeline, AsyncSchedulingPolicy,
    DirtyFlag, PipelineDocumentPhase, ResearchCssomHost, ResearchDom, ResearchReflowHooks,
    ResearchScriptResourceLoader, ScheduledVm, ScriptDescriptor, ScriptLoadMode,
    ScriptPipelineConfig,
};

#[test]
fn parser_blocking_script_executes_immediately_and_requests_reflow() {
    let loader = Rc::new(ResearchScriptResourceLoader::default());
    let hooks = Rc::new(ResearchReflowHooks::default());
    let cssom = Rc::new(ResearchCssomHost::default());
    let dom = Rc::new(ResearchDom::with_cssom("Before", cssom.clone()));

    let mut scheduled = ScheduledVm::default();
    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom,
    );

    let run = run_research_script_pipeline(
        &mut scheduled,
        vec![ScriptDescriptor::inline(
            1,
            r#"
            document.title = "After";
            const node = document.createElement("div");
            node.id = "created";
            document.body.appendChild(node);
            console.log(document.currentScript.id, document.title);
            "#,
        )],
        loader,
        hooks.clone(),
        ScriptPipelineConfig::default(),
    )
    .expect("run");

    assert_eq!(dom.title(), "After");
    assert!(dom.get_element_by_id("created").is_some());
    assert_eq!(run.phase, PipelineDocumentPhase::Complete);
    assert_eq!(run.metrics.parser_blocking_scripts, 1);
    assert_eq!(run.metrics.current_script_sets, 1);
    assert_eq!(run.metrics.current_script_clears, 1);
    assert_eq!(hooks.before_script_ids(), vec![1]);
    assert_eq!(hooks.after_script_ids(), vec![1]);
    assert!(hooks
        .reflow_requests()
        .iter()
        .any(|request| request.dirty.contains(DirtyFlag::Layout)));
    assert!(run
        .last_summary
        .as_ref()
        .unwrap()
        .console
        .iter()
        .any(|line| line.contains("syljs-script-1 After")));
}

#[test]
fn defer_scripts_wait_until_after_parser_blocking_scripts() {
    let loader = Rc::new(ResearchScriptResourceLoader::default());
    loader.register_script(
        "/defer.js",
        r#"
        console.log("defer", document.title);
        document.title = document.title + "-defer";
        "#,
    );

    let hooks = Rc::new(ResearchReflowHooks::default());
    let cssom = Rc::new(ResearchCssomHost::default());
    let dom = Rc::new(ResearchDom::with_cssom("Initial", cssom.clone()));
    let mut scheduled = ScheduledVm::default();

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom,
    );

    let run = run_research_script_pipeline(
        &mut scheduled,
        vec![
            ScriptDescriptor::inline(1, r#"document.title = "parser"; console.log("parser");"#),
            ScriptDescriptor::external(2, "/defer.js", ScriptLoadMode::Defer),
        ],
        loader,
        hooks,
        ScriptPipelineConfig::default(),
    )
    .expect("run");

    assert_eq!(dom.title(), "parser-defer");
    assert_eq!(run.metrics.parser_blocking_scripts, 1);
    assert_eq!(run.metrics.defer_scripts_queued, 1);
    assert_eq!(run.metrics.defer_scripts_executed, 1);

    let console = &run.last_summary.as_ref().unwrap().console;
    assert_eq!(console, &vec!["parser".to_owned(), "defer parser".to_owned()]);
}

#[test]
fn async_policy_after_dom_content_loaded_runs_async_late() {
    let loader = Rc::new(ResearchScriptResourceLoader::default());
    loader.register_script(
        "/async.js",
        r#"
        document.title = document.title + "-async";
        console.log("async", document.title);
        "#,
    );

    let hooks = Rc::new(ResearchReflowHooks::default());
    let cssom = Rc::new(ResearchCssomHost::default());
    let dom = Rc::new(ResearchDom::with_cssom("start", cssom.clone()));
    let mut scheduled = ScheduledVm::default();

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom,
    );

    let mut config = ScriptPipelineConfig::default();
    config.async_policy = AsyncSchedulingPolicy::AfterDomContentLoaded;

    let run = run_research_script_pipeline(
        &mut scheduled,
        vec![
            ScriptDescriptor::external(1, "/async.js", ScriptLoadMode::Async),
            ScriptDescriptor::inline(2, r#"document.title = "parser"; console.log("parser");"#),
        ],
        loader,
        hooks.clone(),
        config,
    )
    .expect("run");

    assert_eq!(dom.title(), "parser-async");
    assert_eq!(run.metrics.async_scripts_queued, 1);
    assert_eq!(run.metrics.async_scripts_executed, 1);
    assert_eq!(hooks.dom_content_loaded_count(), 1);
    assert_eq!(hooks.load_count(), 1);
}

#[test]
fn missing_external_script_records_failure_when_continue_on_error() {
    let loader = Rc::new(ResearchScriptResourceLoader::default());
    let hooks = Rc::new(ResearchReflowHooks::default());
    let mut scheduled = ScheduledVm::default();

    let run = run_research_script_pipeline(
        &mut scheduled,
        vec![
            ScriptDescriptor::external(1, "/missing.js", ScriptLoadMode::ParserBlocking),
            ScriptDescriptor::inline(2, r#"console.log("still-runs");"#),
        ],
        loader,
        hooks.clone(),
        ScriptPipelineConfig::default(),
    )
    .expect("run");

    assert_eq!(run.metrics.script_failures, 1);
    assert_eq!(run.metrics.scripts_executed, 1);
    assert_eq!(run.failures.len(), 1);
    assert_eq!(hooks.failures().len(), 1);
    assert_eq!(run.last_summary.as_ref().unwrap().console, vec!["still-runs"]);
}

#[test]
fn stop_on_error_returns_error() {
    let loader = Rc::new(ResearchScriptResourceLoader::default());
    let hooks = Rc::new(ResearchReflowHooks::default());
    let mut scheduled = ScheduledVm::default();

    let mut config = ScriptPipelineConfig::default();
    config.continue_on_error = false;

    let result = run_research_script_pipeline(
        &mut scheduled,
        vec![ScriptDescriptor::external(
            1,
            "/missing.js",
            ScriptLoadMode::ParserBlocking,
        )],
        loader,
        hooks,
        config,
    );

    assert!(result.is_err());
}
