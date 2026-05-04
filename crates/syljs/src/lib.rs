#![deny(unsafe_code)]
#![allow(
    clippy::all,
    clippy::expect_used,
    clippy::nursery,
    clippy::pedantic,
    clippy::unwrap_used
)]
#![doc = "SylJS frontend, bytecode VM, event loop, DOM, CSSOM, Web APIs, media, Canvas 2D, Worker simulation, and benchmark harness."]
#![doc = ""]
#![doc = "SylJS intentionally implements a JavaScript subset suitable for"]
#![doc = "browser-engine research: DOM mutation scripts, control flow, functions,"]
#![doc = "calls, member access, bytecode execution, runtime values, timers,"]
#![doc = "microtasks, Promise-lite, host DOM bindings, Web APIs, dynamic CSSOM,"]
#![doc = "media simulation, Canvas 2D command recording, Worker-lite scheduling,"]
#![doc = "and synthetic YouTube-like benchmark workloads."]

/// Abstract syntax tree definitions.
pub mod ast;

/// Benchmark harness and research metrics.
pub mod benchmark;

/// Bytecode instruction definitions.
pub mod bytecode;

/// Canvas 2D simulation and command recording.
pub mod canvas;

/// Cache API and Service Worker simulation.
pub mod cache_api;

/// AST-to-bytecode compiler.
pub mod compiler;

/// CSSOM host bindings and dynamic style mutation runtime.
pub mod cssom;

/// Source diagnostics.
pub mod diagnostic;

/// JavaScript DOM host bindings.
pub mod dom;

/// JavaScript event loop, timers, tasks, microtasks, and RAF.
pub mod event_loop;

/// HTTP request/response semantics.
pub mod http;

/// Style/layout/paint invalidation bridge.
pub mod invalidation;

/// Lexer implementation.
pub mod lexer;

/// MediaSource, video/audio element, and source-buffer simulation.
pub mod media;

/// Parser implementation.
pub mod parser;

/// Promise-lite host implementation.
pub mod promise;

/// Source span utilities.
pub mod span;

/// Token definitions.
pub mod token;

/// Runtime values and heap objects.
pub mod value;

/// Script loading and execution pipeline.
pub mod script_pipeline;

/// Browser security policy and origin checks.
pub mod security;

/// Synthetic Google/GitHub/Wikipedia site compatibility harness.
pub mod site_compat;

/// Transferable ArrayBuffer and typed-array-lite runtime.
pub mod transfer;

/// AST visitor and statistics helpers.
pub mod visit;

/// Bytecode virtual machine.
pub mod vm;

/// Web API host bindings.
pub mod webapi;

/// Worker-lite host bindings.
pub mod worker;

pub use ast::{
    AssignOp, BinaryOp, BindingPattern, Expr, ExprKind, FunctionDecl, FunctionParam, Literal,
    MemberProperty, ObjectProperty, Program, ProgramKind, Stmt, StmtKind, UnaryOp, VarDecl,
    VarDeclKind,
};
pub use benchmark::{
    build_youtube_like_script, run_youtube_like_benchmark, BenchmarkAggregateMetrics,
    BenchmarkConfig, BenchmarkConsoleRecord, BenchmarkProfile, BenchmarkRun, BenchmarkScenario,
    BenchmarkScriptSummary,
};
pub use bytecode::{BytecodeFunction, Constant, Instruction};
pub use cache_api::*;
pub use canvas::{
    install_canvas_globals, CanvasCommand, CanvasContextId, CanvasContextSnapshot, CanvasElementId,
    CanvasElementSnapshot, CanvasGradientStop, CanvasHost, CanvasImageData, CanvasMetrics,
    CanvasTextMetrics, ResearchCanvasHost, SharedCanvasHost,
};
pub use compiler::{compile_program, CompileError, CompileOptions};
pub use cssom::{
    create_computed_style_object, create_inline_style_object, install_cssom_globals, CssRuleRecord,
    CssStyleMutation, CssStyleSheetRecord, CssomHost, CssomMetrics, ResearchCssomHost,
    SharedCssomHost, StyleInvalidationKind,
};
pub use diagnostic::{Diagnostic, DiagnosticKind, SylJsError};
pub use dom::{
    create_document_object, create_element_object, install_dom_globals, DomBindingMetrics,
    DomEventRecord, DomHost, DomListenerRecord, DomNodeRef, DomNodeSnapshot, DomNodeType,
    ResearchDom, SharedDomHost,
};
pub use event_loop::{
    install_event_loop_globals, EventLoopConfig, EventLoopMetrics, EventLoopRunSummary,
    JsEventLoop, QueuedJob, QueuedJobKind, RafId, ScheduledVm, TaskId, TimerId,
};
pub use http::*;
pub use invalidation::{
    apply_reflow_request_to_invalidation_engine, collect_cssom_mutation_invalidations,
    collect_dom_snapshot_invalidations, InvalidationBatch, InvalidationCoalescer,
    InvalidationConfig, InvalidationEngine, InvalidationEvent, InvalidationImpact,
    InvalidationInput, InvalidationMetrics, InvalidationNode, InvalidationPlan,
    InvalidationPriority, InvalidationReason, InvalidationRect, InvalidationScope,
    InvalidationSource, LayoutInvalidationKind, PaintInvalidationKind, RebuildHint,
    ResearchInvalidationHooks, SharedInvalidationEngine, StyleInvalidationKindLite,
};
pub use lexer::Lexer;
pub use media::{
    install_media_globals, BufferedRange, MediaElementId, MediaElementKind, MediaElementSnapshot,
    MediaEventRecord, MediaHost, MediaMetrics, MediaNetworkState, MediaReadyState,
    MediaSegmentRecord, MediaSourceId, MediaSourceSnapshot, MediaSourceState, MediaTimeUpdate,
    ResearchMediaHost, SharedMediaHost, SourceBufferId, SourceBufferSnapshot, SourceBufferState,
};
pub use parser::{parse_module, parse_script, Parser};
pub use promise::{
    create_promise_constructor, create_rejected_promise_value, create_resolved_promise_value,
    PromiseInspection, PromiseState,
};
pub use script_pipeline::{
    run_research_script_pipeline, AsyncSchedulingPolicy, DirtyFlag, DirtyFlags,
    PipelineDocumentPhase, PipelineEvent, PipelineScriptRun, ReflowRequest, ResearchReflowHooks,
    ResearchScriptResourceLoader, ScriptDescriptor, ScriptExecutionFailure, ScriptKind,
    ScriptLoadMode, ScriptPipelineConfig, ScriptPipelineHooks, ScriptPipelineMetrics,
    ScriptPipelineRun, ScriptResourceLoader, ScriptSource, SharedScriptPipelineHooks,
    SharedScriptResourceLoader, WebApiScriptResourceLoader,
};
pub use security::*;
pub use site_compat::{
    build_site_compatibility_script, run_site_compatibility_harness, SiteCompatibilityAggregate,
    SiteCompatibilityHarnessConfig, SiteCompatibilityHarnessRun, SiteCompatibilityMetrics,
    SiteCompatibilityProfile, SiteCompatibilityScore, SiteCompatibilitySignal,
    SiteCompatibilitySuite, SiteCompatibilitySuiteRun,
};
pub use span::{SourceId, Span};
pub use token::{Keyword, Token, TokenKind};
pub use transfer::{
    array_buffer_id_from_value, create_array_buffer_object, install_transfer_globals,
    transfer_list_from_value, ArrayBufferId, ArrayBufferSnapshot, ResearchTransferHost,
    SharedTransferHost, TransferHost, TransferMetrics, TransferMode, TransferRecord,
    TypedArrayKind, TypedArraySnapshot,
};
pub use value::{
    JsFunction, JsHostObject, JsNativeFunction, JsObject, JsObjectKind, JsRuntimeError, JsValue,
    NativeResult,
};
pub use visit::{AstStats, AstVisitor};
pub use vm::{ExecutionOutcome, Vm, VmConfig, VmMetrics};
pub use webapi::{
    install_web_api_globals, CookieRecord, FetchRecord, HistoryRecord, ResearchWebApiHost,
    SharedWebApiHost, StorageArea, WebApiHost, WebApiMetrics, WebApiResponse, XhrRecord,
};
pub use worker::{
    install_worker_globals, MessageRecord, ResearchWorkerHost, SharedWorkerHost, WorkerEventRecord,
    WorkerHost, WorkerId, WorkerMetrics, WorkerSnapshot, WorkerState,
};

/// Parses source text as a classic script.
pub fn parse(source: &str) -> Result<Program, SylJsError> {
    parse_script(source)
}

/// Parses, compiles, and executes a classic script in a fresh VM.
pub fn eval_script(source: &str) -> Result<ExecutionOutcome, JsRuntimeError> {
    let program = parse_script(source).map_err(JsRuntimeError::from_frontend_error)?;
    let function = compile_program(&program, CompileOptions::default())?;
    let mut vm = Vm::default();
    vm.execute(&function)
}

/// Parses, compiles, executes, and drains timers/microtasks in a fresh scheduled VM.
pub fn eval_script_with_event_loop(source: &str) -> Result<EventLoopRunSummary, JsRuntimeError> {
    let mut scheduled = ScheduledVm::default();
    scheduled.execute_script(source)?;
    scheduled.run_until_idle()
}

/// Parses, compiles, executes, and drains a script with an in-memory research DOM.
pub fn eval_script_with_research_dom(
    source: &str,
) -> Result<(EventLoopRunSummary, std::rc::Rc<ResearchDom>), JsRuntimeError> {
    let cssom = std::rc::Rc::new(ResearchCssomHost::default());
    let dom = std::rc::Rc::new(ResearchDom::with_cssom("Sylphos", cssom.clone()));
    let mut scheduled = ScheduledVm::default();
    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom,
    );
    scheduled.execute_script(source)?;
    let summary = scheduled.run_until_idle()?;
    Ok((summary, dom))
}

/// Parses, compiles, executes, and drains a script with deterministic Web APIs.
pub fn eval_script_with_research_webapi(
    source: &str,
) -> Result<(EventLoopRunSummary, std::rc::Rc<ResearchWebApiHost>), JsRuntimeError> {
    let host = std::rc::Rc::new(ResearchWebApiHost::default());
    let mut scheduled = ScheduledVm::default();
    install_web_api_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        host.clone(),
    );
    scheduled.execute_script(source)?;
    let summary = scheduled.run_until_idle()?;
    Ok((summary, host))
}

/// Parses, compiles, executes, and drains a script with research DOM + CSSOM.
pub fn eval_script_with_research_cssom(
    source: &str,
) -> Result<
    (
        EventLoopRunSummary,
        std::rc::Rc<ResearchDom>,
        std::rc::Rc<ResearchCssomHost>,
    ),
    JsRuntimeError,
> {
    let cssom = std::rc::Rc::new(ResearchCssomHost::default());
    let dom = std::rc::Rc::new(ResearchDom::with_cssom("Sylphos", cssom.clone()));
    let mut scheduled = ScheduledVm::default();

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom.clone(),
    );

    scheduled.execute_script(source)?;
    let summary = scheduled.run_until_idle()?;
    Ok((summary, dom, cssom))
}

/// Parses, compiles, executes, and drains a script with DOM/CSSOM/media simulation.
pub fn eval_script_with_research_media(
    source: &str,
) -> Result<
    (
        EventLoopRunSummary,
        std::rc::Rc<ResearchDom>,
        std::rc::Rc<ResearchCssomHost>,
        std::rc::Rc<ResearchMediaHost>,
    ),
    JsRuntimeError,
> {
    let cssom = std::rc::Rc::new(ResearchCssomHost::default());
    let dom = std::rc::Rc::new(ResearchDom::with_cssom("Sylphos", cssom.clone()));
    let media = std::rc::Rc::new(ResearchMediaHost::default());
    let mut scheduled = ScheduledVm::default();

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom.clone(),
    );
    install_media_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        media.clone(),
        Some(dom.clone()),
    );

    scheduled.execute_script(source)?;
    let summary = scheduled.run_until_idle()?;
    Ok((summary, dom, cssom, media))
}

/// Parses, compiles, executes, and drains a script with DOM/CSSOM/Canvas/Worker.
pub fn eval_script_with_research_canvas_worker(
    source: &str,
) -> Result<
    (
        EventLoopRunSummary,
        std::rc::Rc<ResearchDom>,
        std::rc::Rc<ResearchCssomHost>,
        std::rc::Rc<ResearchCanvasHost>,
        std::rc::Rc<ResearchWorkerHost>,
    ),
    JsRuntimeError,
> {
    let cssom = std::rc::Rc::new(ResearchCssomHost::default());
    let dom = std::rc::Rc::new(ResearchDom::with_cssom("Sylphos", cssom.clone()));
    let canvas = std::rc::Rc::new(ResearchCanvasHost::default());
    let workers = std::rc::Rc::new(ResearchWorkerHost::default());
    let mut scheduled = ScheduledVm::default();

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

    scheduled.execute_script(source)?;
    let summary = scheduled.run_until_idle()?;
    Ok((summary, dom, cssom, canvas, workers))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod media_tests;

#[cfg(test)]
mod canvas_worker_tests;

#[cfg(test)]
mod benchmark_tests;

#[cfg(test)]
mod invalidation_tests;

#[cfg(test)]
mod script_pipeline_tests;

#[cfg(test)]
mod transfer_tests;

#[cfg(test)]
mod cache_api_tests;

#[cfg(test)]
mod http_tests;

#[cfg(test)]
mod security_tests;

#[cfg(test)]
mod site_compat_tests;
