//! JS/app bridge for Module 40 invalidation integration.

#![allow(dead_code)]

use std::{cell::RefCell, rc::Rc};

use syljs::{
    InvalidationEngine, InvalidationMetrics, InvalidationPlan, ReflowRequest,
    ResearchInvalidationHooks, SharedInvalidationEngine,
};

/// App-owned invalidation runtime.
#[derive(Debug, Clone)]
pub(crate) struct AppInvalidationRuntime {
    engine: SharedInvalidationEngine,
    hooks: Rc<ResearchInvalidationHooks>,
}

impl Default for AppInvalidationRuntime {
    fn default() -> Self {
        let engine = Rc::new(RefCell::new(InvalidationEngine::default()));
        let hooks = Rc::new(ResearchInvalidationHooks::new(engine.clone()));
        Self { engine, hooks }
    }
}

impl AppInvalidationRuntime {
    /// Creates a new invalidation runtime.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Pipeline hooks to pass into Module 39.
    pub(crate) fn hooks(&self) -> Rc<ResearchInvalidationHooks> {
        self.hooks.clone()
    }

    /// Shared engine.
    pub(crate) fn engine(&self) -> SharedInvalidationEngine {
        self.engine.clone()
    }

    /// Submit a reflow request manually.
    pub(crate) fn submit_reflow_request(&self, request: &ReflowRequest) {
        self.engine.borrow_mut().consume_reflow_request(request);
    }

    /// Flush plan.
    pub(crate) fn flush_plan(&self) -> InvalidationPlan {
        self.engine.borrow_mut().flush_plan()
    }

    /// Metrics.
    pub(crate) fn metrics(&self) -> InvalidationMetrics {
        self.engine.borrow().metrics()
    }
}
