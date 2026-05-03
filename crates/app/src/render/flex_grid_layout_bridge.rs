//! App bridge for Module 41 Flexbox/Grid Lite layout.

use present::flex_grid::{
    layout_flex_grid_tree_with_config, FlexGridLayoutConfig, LayoutNode, LayoutNodeId,
    LayoutResult, Rect, Size,
};

/// App-level layout request.
#[derive(Debug, Clone)]
pub(crate) struct AppFlexGridLayoutRequest {
    pub root: LayoutNode,
    pub viewport: Size,
    pub config: FlexGridLayoutConfig,
}

impl AppFlexGridLayoutRequest {
    pub(crate) fn new(root: LayoutNode, viewport: Size) -> Self {
        Self {
            root,
            viewport,
            config: FlexGridLayoutConfig::default(),
        }
    }
}

/// App-level layout response.
#[derive(Debug, Clone)]
pub(crate) struct AppFlexGridLayoutResponse {
    pub result: LayoutResult,
    pub dirty_bounds: Rect,
    pub requires_paint_plan: bool,
}

/// Runs Flex/Grid layout.
pub(crate) fn run_app_flex_grid_layout(
    request: AppFlexGridLayoutRequest,
) -> AppFlexGridLayoutResponse {
    let result = layout_flex_grid_tree_with_config(
        &request.root,
        request.viewport,
        request.config,
    );

    AppFlexGridLayoutResponse {
        dirty_bounds: result.document_bounds,
        requires_paint_plan: true,
        result,
    }
}

/// Returns whether a node exists in layout output.
pub(crate) fn layout_contains_node(result: &LayoutResult, id: LayoutNodeId) -> bool {
    result.box_for(id).is_some()
}

/// Converts Module 40 invalidation plan into layout decision.
pub(crate) fn should_run_flex_grid_layout(plan: &syljs::InvalidationPlan) -> bool {
    plan.rebuild_layout_tree
        || !plan.layout_nodes.is_empty()
        || plan.rebuild_hint >= syljs::RebuildHint::LayoutTree
        || plan.full_viewport_paint
}

/// Runs layout only when Module 40 says layout is dirty.
pub(crate) fn run_layout_after_invalidation(
    plan: &syljs::InvalidationPlan,
    request: AppFlexGridLayoutRequest,
) -> Option<AppFlexGridLayoutResponse> {
    should_run_flex_grid_layout(plan).then(|| run_app_flex_grid_layout(request))
}
