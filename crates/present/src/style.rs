#![doc = "CSS-lite style-sheet and computed paint-style types."]

use crate::box_model::{BoxModelStylesLite, ComputedBoxModelStyles};
use crate::Color;

const DEFAULT_DARK_BACKGROUND: Color = Color::rgba(0.10, 0.10, 0.15, 1.0);
const DEFAULT_LINK_DARK: Color = Color::rgba(0.18, 0.28, 0.53, 1.0);
const DEFAULT_LINK_LIGHT: Color = Color::rgba(0.62, 0.76, 1.0, 1.0);

/// Minimal style sheet extracted from inline `<style>` blocks.
///
/// This is intentionally not a general CSS object model. It tracks only the
/// values Sylphos can currently use to build a better paint plan.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StyleSheetLite {
    /// `body { background: ... }` or `body { background-color: ... }`.
    pub body_background: Option<Color>,

    /// `body { color: ... }`.
    pub body_color: Option<Color>,

    /// `a`, `a:link`, or `a:visited` color.
    pub link_color: Option<Color>,

    /// Optional heading sizes for `h1` through `h6`.
    pub heading_sizes: [Option<f32>; 6],

    /// Optional paragraph font size.
    pub paragraph_size: Option<f32>,

    /// Optional root font size from `body { font-size: ... }`.
    pub body_font_size: Option<f32>,

    /// Optional body width fraction from values such as `60vw`.
    pub content_width_fraction: Option<f32>,

    /// Optional top margin fraction from values such as `15vh`.
    pub margin_top_fraction: Option<f32>,

    /// Optional left margin in pixels.
    pub margin_left_px: Option<f32>,

    /// Optional top margin in pixels.
    pub margin_top_px: Option<f32>,

    /// Whether horizontal auto-centering was requested with values like `margin: 15vh auto`.
    pub center_horizontally: bool,

    /// CSS box-model values for supported tag selectors.
    pub box_model: BoxModelStylesLite,
}

impl StyleSheetLite {
    /// Returns `true` when no supported style has been extracted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.body_background.is_none()
            && self.body_color.is_none()
            && self.link_color.is_none()
            && self.heading_sizes.iter().all(Option::is_none)
            && self.paragraph_size.is_none()
            && self.body_font_size.is_none()
            && self.content_width_fraction.is_none()
            && self.margin_top_fraction.is_none()
            && self.margin_left_px.is_none()
            && self.margin_top_px.is_none()
            && !self.center_horizontally
            && self.box_model.is_empty()
    }

    /// Merges another style sheet into this one.
    ///
    /// Later CSS rules win, so `Some` values in `other` replace existing values.
    pub fn merge_from(&mut self, other: Self) {
        if other.body_background.is_some() {
            self.body_background = other.body_background;
        }
        if other.body_color.is_some() {
            self.body_color = other.body_color;
        }
        if other.link_color.is_some() {
            self.link_color = other.link_color;
        }
        if other.paragraph_size.is_some() {
            self.paragraph_size = other.paragraph_size;
        }
        if other.body_font_size.is_some() {
            self.body_font_size = other.body_font_size;
        }
        if other.content_width_fraction.is_some() {
            self.content_width_fraction = other.content_width_fraction;
        }
        if other.margin_top_fraction.is_some() {
            self.margin_top_fraction = other.margin_top_fraction;
        }
        if other.margin_left_px.is_some() {
            self.margin_left_px = other.margin_left_px;
        }
        if other.margin_top_px.is_some() {
            self.margin_top_px = other.margin_top_px;
        }
        if other.center_horizontally {
            self.center_horizontally = true;
        }

        for (target, source) in self.heading_sizes.iter_mut().zip(other.heading_sizes) {
            if source.is_some() {
                *target = source;
            }
        }

        self.box_model.merge_from(other.box_model);
    }

    /// Computes concrete paint-time style values for a viewport.
    #[must_use]
    pub fn compute(
        &self,
        theme_color: Option<Color>,
        viewport_width: f32,
        viewport_height: f32,
    ) -> ComputedPaintStyle {
        let background = self
            .body_background
            .or(self.box_model.body.background)
            .or(theme_color)
            .unwrap_or(DEFAULT_DARK_BACKGROUND);

        let text_color = self
            .body_color
            .unwrap_or_else(|| background.readable_foreground());

        let default_link = if background.luminance() > 0.45 {
            DEFAULT_LINK_DARK
        } else {
            DEFAULT_LINK_LIGHT
        };

        let link_color = self.link_color.unwrap_or(default_link);
        let body_size = self.body_font_size.unwrap_or(16.0).max(8.0);

        let heading_sizes = [
            self.heading_sizes[0].unwrap_or(body_size * 1.75),
            self.heading_sizes[1].unwrap_or(body_size * 1.50),
            self.heading_sizes[2].unwrap_or(body_size * 1.30),
            self.heading_sizes[3].unwrap_or(body_size * 1.15),
            self.heading_sizes[4].unwrap_or(body_size * 1.05),
            self.heading_sizes[5].unwrap_or(body_size),
        ];

        let paragraph_size = self.paragraph_size.unwrap_or(body_size).max(8.0);
        let computed_box_model = self.box_model.compute(text_color);
        let body_padding = computed_box_model.body.padding;
        let body_border = computed_box_model.body.border_width;

        let content_width = self.content_width_fraction.map_or_else(
            || (viewport_width - 64.0).max(240.0),
            |fraction| viewport_width * fraction.clamp(0.05, 1.0),
        );

        let margin_top = self
            .margin_top_px
            .or_else(|| {
                self.margin_top_fraction
                    .map(|fraction| viewport_height * fraction)
            })
            .unwrap_or(32.0)
            .max(0.0)
            + computed_box_model.body.margin.top
            + body_border.top
            + body_padding.top;

        let margin_left = if self.center_horizontally {
            ((viewport_width - content_width) / 2.0).max(24.0)
        } else {
            self.margin_left_px.unwrap_or(32.0).max(0.0) + computed_box_model.body.margin.left
        } + body_border.left
            + body_padding.left;

        let content_width =
            (content_width - body_padding.horizontal() - body_border.horizontal()).max(80.0);

        ComputedPaintStyle {
            background,
            text_color,
            link_color,
            heading_sizes,
            paragraph_size,
            generic_size: (paragraph_size * 0.90).max(8.0),
            content_x: margin_left,
            content_y: margin_top,
            content_width,
            line_gap: (paragraph_size * 0.75).max(12.0),
            box_model: computed_box_model,
        }
    }
}

/// Concrete style values used by `build_paint_plan`.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedPaintStyle {
    /// Resolved page background.
    pub background: Color,

    /// Resolved default text color.
    pub text_color: Color,

    /// Resolved link color.
    pub link_color: Color,

    /// Resolved heading sizes for `h1` through `h6`.
    pub heading_sizes: [f32; 6],

    /// Resolved paragraph size.
    pub paragraph_size: f32,

    /// Resolved generic/fallback text size.
    pub generic_size: f32,

    /// Content start x position.
    pub content_x: f32,

    /// Content start y position.
    pub content_y: f32,

    /// Content width used for simple text wrapping.
    pub content_width: f32,

    /// Vertical gap between blocks.
    pub line_gap: f32,

    /// Computed box-model styles for supported selectors.
    pub box_model: ComputedBoxModelStyles,
}
