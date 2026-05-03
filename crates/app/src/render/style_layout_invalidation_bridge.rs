//! App bridge for Module 40 style/layout invalidation.
//!
//! This file intentionally depends only on `syljs`, not on the internal shape
//! of your current `present` crate. It gives the app one stable adapter layer:
//! feed script/CSS/DOM invalidations in, get a renderer action plan out.
//! Miraculous, really. A software seam. Rare endangered species.

use std::{cell::RefCell, rc::Rc};

use syljs::{
    apply_reflow_request_to_invalidation_engine, collect_cssom_mutation_invalidations,
    collect_dom_snapshot_invalidations, CssStyleMutation, DomNodeSnapshot, InvalidationEngine,
    InvalidationInput, InvalidationMetrics, InvalidationPlan, ReflowRequest,
};

/// Renderer action derived from an invalidation plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RendererInvalidationAction {
    /// Nothing to do.
    None,

    /// Recompute styles only.
    RecomputeStyle,

    /// Rebuild or update layout tree.
    RebuildLayout,

    /// Regenerate PaintPlan.
    RegeneratePaintPlan,

    /// Run the full style/layout/paint pipeline.
    FullRenderPipeline,
}

/// App-facing invalidation bridge.
#[derive(Debug, Clone)]
pub(crate) struct StyleLayoutInvalidationBridge {
    engine: Rc<RefCell<InvalidationEngine>>,
}

impl Default for StyleLayoutInvalidationBridge {
    fn default() -> Self {
        Self {
            engine: Rc::new(RefCell::new(InvalidationEngine::default())),
        }
    }
}

impl StyleLayoutInvalidationBridge {
    /// Creates a bridge from a shared engine.
    pub(crate) fn new(engine: Rc<RefCell<InvalidationEngine>>) -> Self {
        Self { engine }
    }

    /// Shared engine.
    pub(crate) fn engine(&self) -> Rc<RefCell<InvalidationEngine>> {
        self.engine.clone()
    }

    /// Submits a script-pipeline reflow request.
    pub(crate) fn submit_reflow_request(&self, request: &ReflowRequest) {
        apply_reflow_request_to_invalidation_engine(&mut self.engine.borrow_mut(), request);
    }

    /// Submits CSSOM mutations.
    pub(crate) fn submit_cssom_mutations(&self, mutations: &[CssStyleMutation]) {
        let inputs = collect_cssom_mutation_invalidations(mutations);
        self.engine.borrow_mut().submit_many(inputs);
    }

    /// Submits DOM snapshots.
    pub(crate) fn submit_dom_snapshots(&self, snapshots: &[DomNodeSnapshot]) {
        let inputs = collect_dom_snapshot_invalidations(snapshots);
        self.engine.borrow_mut().submit_many(inputs);
    }

    /// Submits raw invalidation inputs.
    pub(crate) fn submit_inputs(&self, inputs: impl IntoIterator<Item = InvalidationInput>) {
        self.engine.borrow_mut().submit_many(inputs);
    }

    /// Flushes into a renderer plan.
    pub(crate) fn flush_renderer_plan(&self) -> RendererInvalidationPlan {
        let plan = self.engine.borrow_mut().flush_plan();
        RendererInvalidationPlan::from_invalidation_plan(plan)
    }

    /// Metrics.
    pub(crate) fn metrics(&self) -> InvalidationMetrics {
        self.engine.borrow().metrics()
    }
}

/// App-facing renderer invalidation plan.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RendererInvalidationPlan {
    /// Low-level SylJS plan.
    pub raw: InvalidationPlan,

    /// Renderer action.
    pub action: RendererInvalidationAction,

    /// Whether to rematch style.
    pub rematch_style: bool,

    /// Whether to rebuild layout tree.
    pub rebuild_layout_tree: bool,

    /// Whether to regenerate paint plan.
    pub regenerate_paint_plan: bool,

    /// Whether to repaint full viewport.
    pub repaint_full_viewport: bool,
}

impl RendererInvalidationPlan {
    /// Builds renderer plan from SylJS invalidation plan.
    pub(crate) fn from_invalidation_plan(raw: InvalidationPlan) -> Self {
        let action = if raw.rebuild_hint == syljs::RebuildHint::FullPipeline {
            RendererInvalidationAction::FullRenderPipeline
        } else if raw.rebuild_layout_tree {
            RendererInvalidationAction::RebuildLayout
        } else if raw.rebuild_paint_plan {
            RendererInvalidationAction::RegeneratePaintPlan
        } else if raw.restyle_document || !raw.style_nodes.is_empty() {
            RendererInvalidationAction::RecomputeStyle
        } else {
            RendererInvalidationAction::None
        };

        Self {
            rematch_style: raw.restyle_document || !raw.style_nodes.is_empty(),
            rebuild_layout_tree: raw.rebuild_layout_tree || !raw.layout_nodes.is_empty(),
            regenerate_paint_plan: raw.rebuild_paint_plan || !raw.paint_nodes.is_empty(),
            repaint_full_viewport: raw.full_viewport_paint,
            raw,
            action,
        }
    }
}

/// Suggested integration point after JS execution.
///
/// Call this after Module 39 script pipeline completes, after CSSOM mutation
/// collection, or after DOM mutation batches. If `action` is:
///
/// - `RecomputeStyle`: rebuild computed style tree.
/// - `RebuildLayout`: run layout tree update, then PaintPlan.
/// - `RegeneratePaintPlan`: rebuild PaintPlan only.
/// - `FullRenderPipeline`: style + layout + PaintPlan.
pub(crate) fn describe_renderer_action(action: RendererInvalidationAction) -> &'static str {
    match action {
        RendererInvalidationAction::None => "none",
        RendererInvalidationAction::RecomputeStyle => "recompute-style",
        RendererInvalidationAction::RebuildLayout => "rebuild-layout",
        RendererInvalidationAction::RegeneratePaintPlan => "regenerate-paint-plan",
        RendererInvalidationAction::FullRenderPipeline => "full-render-pipeline",
    }
}
