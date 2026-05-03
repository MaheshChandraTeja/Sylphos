//! App-level script pipeline integration.
//!
//! This module exists so the browser shell can talk about page script loading
//! without importing half the JS bridge tree everywhere. A tiny boundary.
//! Revolutionary concept. Software should try it more often.

pub(crate) use crate::js::syljs_script_pipeline_bridge::{
    create_app_reflow_hooks, create_app_script_loader, execute_app_script_pipeline,
    execute_standalone_app_script_pipeline, external_app_script, inline_app_script,
    AppScriptPipelineInput, AppScriptPipelineRequest, AppScriptPipelineResponse,
    AppScriptPipelineSource,
};
