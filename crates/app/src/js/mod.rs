//! JavaScript runtime core, DOM bindings, browser event loop, Web Platform APIs,
//! CSSOM hooks, media/canvas/worker compatibility, script discovery, loading,
//! and diagnostics.
//!
//! Module 27 extends the bounded intrinsic runtime surface with media element,
//! canvas, worker, MediaSource, and YouTube boot-signal host hooks. It is still
//! not a full V8 runtime, but it gives the browser engine stable service
//! boundaries for the platform APIs YouTube-class applications expect.

mod console;
mod cookies;
pub(crate) mod cssom;
mod dom_bindings;
mod event_loop;
mod history_api;
mod media;
mod runtime;
mod scripts;
mod storage;
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
pub(crate) use storage::{StorageAreaKind, WebStorage};
pub(crate) use web_platform::{
    capture_web_platform_effects, WebApiEffect, WebPlatformHost, WebPlatformSummary,
};
