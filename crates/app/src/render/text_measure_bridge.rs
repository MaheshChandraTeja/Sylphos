//! App bridge for Module 42 text measurement.
//!
//! The renderer should use this instead of guessing text width with vibes and
//! regret. Guessing was cute for ten modules. Now it is how layouts get cursed.

use present::text::{
    layout_text, measure_text, positioned_glyphs, shape_text, FontDatabase, GlyphAtlasRequest,
    PositionedGlyph, ShapedText, TextEngine, TextLayout, TextMeasure, TextMetrics, TextStyle,
};

/// App-owned text runtime.
#[derive(Debug, Clone)]
pub(crate) struct AppTextRuntime {
    engine: TextEngine,
}

impl Default for AppTextRuntime {
    fn default() -> Self {
        Self {
            engine: TextEngine::default(),
        }
    }
}

impl AppTextRuntime {
    /// Creates a runtime with default fonts.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Creates a runtime with explicit font DB.
    pub(crate) fn with_fonts(fonts: FontDatabase) -> Self {
        Self {
            engine: TextEngine::new(fonts),
        }
    }

    /// Measures unwrapped text.
    pub(crate) fn measure(&mut self, text: &str, style: &TextStyle) -> TextMeasure {
        self.engine.measure_text(text, style)
    }

    /// Shapes text into glyph runs.
    pub(crate) fn shape(&mut self, text: &str, style: &TextStyle) -> ShapedText {
        self.engine.shape_text(text, style)
    }

    /// Lays out text into lines.
    pub(crate) fn layout(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextLayout {
        self.engine.layout_text(text, style, max_width)
    }

    /// Collects atlas requests from shaped text.
    pub(crate) fn atlas_requests(&self, shaped: &ShapedText) -> Vec<GlyphAtlasRequest> {
        self.engine.atlas_requests(shaped)
    }

    /// Runtime metrics.
    pub(crate) fn metrics(&self) -> TextMetrics {
        self.engine.metrics()
    }
}

/// Convenience measurement for stateless call sites.
pub(crate) fn app_measure_text(text: &str, style: &TextStyle) -> TextMeasure {
    measure_text(text, style)
}

/// Convenience layout for stateless call sites.
pub(crate) fn app_layout_text(text: &str, style: &TextStyle, max_width: f32) -> TextLayout {
    layout_text(text, style, max_width)
}

/// Convenience shaping for stateless call sites.
pub(crate) fn app_shape_text(text: &str, style: &TextStyle) -> ShapedText {
    shape_text(text, style)
}

/// Converts a layout into positioned glyphs for PaintPlan generation.
pub(crate) fn app_positioned_glyphs(layout: &TextLayout) -> Vec<PositionedGlyph> {
    positioned_glyphs(layout)
}
