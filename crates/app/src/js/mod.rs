//! JavaScript runtime core, DOM bindings, browser event loop, Web Platform APIs,
//! CSSOM hooks, media/canvas/worker compatibility, service worker/cache API,
//! security policy capture, script discovery, loading, and diagnostics.

mod console;
mod cookies;
pub(crate) mod cssom;
mod dom_bindings;
mod event_loop;
mod history_api;
mod media;
mod runtime;
mod scripts;
mod security_policy;
mod service_worker;
mod storage;
pub(crate) mod syljs_invalidation_bridge;
pub(crate) mod syljs_script_pipeline_bridge;
mod web_platform;

pub(crate) use console::{ConsoleLevel, ConsoleMessage};
pub(crate) use cookies::CookieJar;
pub(crate) use cssom::ScriptCssomEffects;
pub(crate) use dom_bindings::{
    apply_dom_binding_effect, capture_dom_binding_effects, DomBindingEffect,
};
pub(crate) use event_loop::BrowserEventLoop;
pub(crate) use history_api::HistoryApiState;
pub(crate) use media::{
    capture_media_canvas_worker_effects, MediaCanvasWorkerEffect, MediaCanvasWorkerHost,
    MediaCanvasWorkerSummary,
};
pub(crate) use runtime::{JavaScriptRuntime, ScriptExecution, ScriptProgram};
pub(crate) use scripts::{execute_document_scripts, ScriptExecutionSummary};
pub(crate) use service_worker::{
    capture_service_worker_effects, ServiceWorkerHost, ServiceWorkerSummary,
};
pub(crate) use storage::{StorageAreaKind, WebStorage};
pub(crate) use web_platform::{
    capture_web_platform_effects, WebApiEffect, WebPlatformHost, WebPlatformSummary,
};
