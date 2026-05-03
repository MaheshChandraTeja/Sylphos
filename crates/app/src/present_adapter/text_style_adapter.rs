//! App computed-style to Present text-style adapter.
//!
//! Keep this adapter between CSSOM/computed-style and the text engine. Otherwise
//! every renderer file will learn CSS parsing through osmosis, and we are not
//! building a haunted apprenticeship program.

use present::text::{
    parse_font_style, parse_font_weight, parse_text_align, parse_text_overflow,
    parse_text_transform, parse_white_space, FontRequest, TextDirection, TextStyle,
};

/// Minimal app-side computed style payload for text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AppComputedTextStyle {
    /// CSS font-family list.
    pub font_family: Vec<String>,

    /// CSS font-size px.
    pub font_size_px: f32,

    /// CSS line-height px.
    pub line_height_px: Option<f32>,

    /// CSS font-weight.
    pub font_weight: String,

    /// CSS font-style.
    pub font_style: String,

    /// Letter spacing px.
    pub letter_spacing_px: f32,

    /// Word spacing px.
    pub word_spacing_px: f32,

    /// white-space.
    pub white_space: String,

    /// text-overflow.
    pub text_overflow: String,

    /// text-transform.
    pub text_transform: String,

    /// text-align.
    pub text_align: String,

    /// direction.
    pub direction: String,

    /// max lines.
    pub max_lines: Option<usize>,
}

impl Default for AppComputedTextStyle {
    fn default() -> Self {
        Self {
            font_family: vec!["system-ui".to_owned(), "sans-serif".to_owned()],
            font_size_px: 16.0,
            line_height_px: None,
            font_weight: "normal".to_owned(),
            font_style: "normal".to_owned(),
            letter_spacing_px: 0.0,
            word_spacing_px: 0.0,
            white_space: "normal".to_owned(),
            text_overflow: "clip".to_owned(),
            text_transform: "none".to_owned(),
            text_align: "start".to_owned(),
            direction: "ltr".to_owned(),
            max_lines: None,
        }
    }
}

/// Converts app computed style to Present text style.
pub(crate) fn app_computed_text_style_to_present(input: &AppComputedTextStyle) -> TextStyle {
    let weight = parse_font_weight(&input.font_weight);
    let style = parse_font_style(&input.font_style);

    TextStyle {
        font: FontRequest {
            families: if input.font_family.is_empty() {
                vec!["system-ui".to_owned(), "sans-serif".to_owned()]
            } else {
                input.font_family.clone()
            },
            weight,
            style,
            ..FontRequest::default()
        },
        font_size: input.font_size_px,
        line_height: input.line_height_px,
        letter_spacing: input.letter_spacing_px,
        word_spacing: input.word_spacing_px,
        white_space: parse_white_space(&input.white_space),
        overflow: parse_text_overflow(&input.text_overflow),
        transform: parse_text_transform(&input.text_transform),
        align: parse_text_align(&input.text_align),
        max_lines: input.max_lines,
        direction: if input.direction.eq_ignore_ascii_case("rtl") {
            TextDirection::Rtl
        } else {
            TextDirection::Ltr
        },
    }
}
