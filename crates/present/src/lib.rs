#![deny(unsafe_code)]
#![doc = "Presentation-layer extraction, CSS, layout, mutation, reflow, and paint planning for Sylphos."]

/// Box-model primitives.
pub mod box_model;
/// Color parsing and color conversion helpers.
pub mod color;
/// Minimal CSS parser for the presentation layer.
pub mod css_lite;
/// CSSOM-lite support.
pub mod cssom;
/// Lightweight presentation document types.
pub mod document;
/// Dynamic style/layout invalidation helpers.
pub mod dynamic_style;
/// DOM event primitives.
pub mod events;
/// DOM extraction logic.
pub mod extract;
/// Form-control presentation primitives.
pub mod forms;
/// Hit-testing helpers.
pub mod hit;
/// Inline-flow primitives.
pub mod inline;
/// Invalidation primitives.
pub mod invalidation;
/// Viewport-aware layout engine.
pub mod layout;
/// Mutable DOM model.
pub mod mutation;
#[cfg(test)]
mod mutation_tests;
/// Deterministic paint planning.
pub mod paint;
/// Incremental reflow and dirty-region paint helpers.
pub mod reflow;
#[cfg(test)]
mod reflow_tests;
/// Selector matching.
pub mod selector;
/// CSS-lite style-sheet and computed paint-style types.
pub mod style;
/// Styled tree support.
pub mod styled;
/// External stylesheet metadata.
pub mod stylesheet;

pub use box_model::{
    BoxModelStylesLite, BoxStyleLite, ComputedBoxModelStyles, ComputedBoxStyle, DisplayLite,
    EdgeSizes,
};
pub use color::Color;
pub use css_lite::parse_css_lite;
pub use cssom::{
    apply_cssom_to_render_document, CssDeclarationLite, CssPropertyName, CssRuleLite,
    CssStyleSheetLite, CssomEngine, CssomInvalidation, CssomMutation,
};
pub use document::{RenderBlock, RenderDocument, RenderText};
pub use dynamic_style::{
    cssom_invalidation_to_dirty_flags, cssom_invalidation_to_set, DynamicLayoutState,
    DynamicStyleUpdate,
};
pub use events::{
    dispatch_dom_event, DefaultAction, DomEvent, DomEventKind, DomEventPayload,
    EventDispatchResult, ScriptHook, ScriptHookQueue,
};
pub use extract::{
    extract_render_document, extract_style_sheet, extract_style_sources, extract_stylesheet_links,
    extract_theme_color,
};
pub use forms::{
    edit_focused_form_control, edit_form_control, focus_form_control, focused_form_control,
    form_by_id, form_control_by_id, form_id_for_control, form_submission_pairs, FormBlock,
    FormControl, FormControlKind, FormDataPair, FormMethod, FormTextEdit,
};
pub use hit::{
    collect_form_control_hit_regions, collect_link_hit_regions, hit_test_form_control,
    hit_test_link, FormControlHitRegion, FormControlHitResult, LinkHitRegion, LinkHitResult,
};
pub use inline::InlineFragment;
pub use invalidation::{DirtyFlags, InvalidationSet};
pub use layout::{
    layout_document, measure_line_height, measure_text_width, wrap_text_to_width, LayoutBox,
    LayoutBoxKind, LayoutRect, LayoutTextRun, LayoutTree, Viewport,
};
pub use mutation::{DomMutation, DomMutationKind, DomNode, DomNodeId, DomNodeKind, DomRuntime};
pub use paint::{build_paint_plan, build_paint_plan_from_layout, PaintCommand, PaintPlan};
pub use reflow::{
    command_bounds, dirty_regions_between, DirtyRect, DirtyRegionSet, IncrementalReflowEngine,
    ReflowMode, ReflowOutput, ReflowReason, ReflowRequest,
};
pub use selector::{
    compute_node_styles, parse_color_value, parse_css_rules_lite, parse_font_size, parse_px,
    parse_viewport_fraction, AncestorSignature, ComputedNodeStyle, ElementSignature, SelectorLite,
    Specificity, StyleRuleLite,
};
pub use style::{ComputedPaintStyle, StyleSheetLite};
pub use styled::{compute_styled_document, StyledDocument, StyledNode};
pub use stylesheet::{StyleSourceLite, StylesheetLink};

#[cfg(test)]
mod tests;
