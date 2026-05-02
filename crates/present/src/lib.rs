#![deny(unsafe_code)]
#![doc = "Presentation-layer extraction, CSS-lite styling, layout, hit-testing, and paint planning for Sylphos."]
#![doc = ""]
#![doc = "This crate converts the parsed `html_mvp` DOM into a lightweight"]
#![doc = "render document and then into a deterministic paint plan. It deliberately"]
#![doc = "supports only a tiny, safe CSS subset. It does not implement JavaScript,"]
#![doc = "full CSS cascade, image loading, font shaping, or browser layout."]

/// Color parsing and color conversion helpers.
pub mod color;

/// Minimal CSS parser for the presentation layer.
pub mod css_lite;

/// Lightweight presentation document types.
pub mod document;

/// DOM extraction logic.
pub mod extract;

/// Hit-testing helpers for links and future interactive page content.
pub mod hit;

/// Viewport-aware layout engine.
pub mod layout;

/// Deterministic paint planning.
pub mod paint;

/// Style-sheet and computed paint-style types.
pub mod style;

pub use color::Color;
pub use css_lite::parse_css_lite;
pub use document::{RenderBlock, RenderDocument, RenderText};
pub use extract::{extract_render_document, extract_style_sheet, extract_theme_color};
pub use hit::{collect_link_hit_regions, hit_test_link, LinkHitRegion, LinkHitResult};
pub use layout::{
    layout_document, measure_line_height, measure_text_width, wrap_text_to_width, LayoutBox,
    LayoutBoxKind, LayoutRect, LayoutTextRun, LayoutTree, Viewport,
};
pub use paint::{build_paint_plan, build_paint_plan_from_layout, PaintCommand, PaintPlan};
pub use style::{ComputedPaintStyle, StyleSheetLite};

#[cfg(test)]
mod tests;
