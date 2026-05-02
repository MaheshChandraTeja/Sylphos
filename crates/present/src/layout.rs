#![doc = "Viewport-aware layout engine for the Sylphos presentation layer."]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use crate::{Color, ComputedPaintStyle, RenderBlock, RenderDocument};

const GLYPH_WIDTH_RATIO: f32 = 0.56;
const LINE_HEIGHT_RATIO: f32 = 1.35;
const IMAGE_PLACEHOLDER_LIGHT: Color = Color::rgba(0.78, 0.80, 0.84, 1.0);
const IMAGE_PLACEHOLDER_DARK: Color = Color::rgba(0.18, 0.20, 0.26, 1.0);
const IMAGE_LABEL_INSET: f32 = 12.0;
const MIN_IMAGE_HEIGHT: f32 = 72.0;
const MAX_IMAGE_HEIGHT: f32 = 180.0;

/// Physical viewport used by the layout engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Viewport width in logical pixels.
    pub width: f32,

    /// Viewport height in logical pixels.
    pub height: f32,
}

impl Viewport {
    /// Creates a sanitized viewport.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width: sanitize_dimension(width),
            height: sanitize_dimension(height),
        }
    }
}

/// Laid-out document tree.
///
/// This is still intentionally small. It is not a browser layout tree, but it
/// gives Sylphos a real viewport-aware intermediate representation between
/// `RenderDocument` and `PaintPlan`.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutTree {
    /// Page background resolved from CSS-lite or theme color.
    pub background: Color,

    /// Viewport used for this layout pass.
    pub viewport: Viewport,

    /// Resolved content rectangle.
    pub content_rect: LayoutRect,

    /// Ordered block boxes.
    pub boxes: Vec<LayoutBox>,

    /// Final content bottom before clipping.
    pub overflow_y: f32,

    /// Whether content extends beyond the viewport height.
    pub clipped: bool,
}

/// Rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutRect {
    /// Left coordinate.
    pub x: f32,

    /// Top coordinate.
    pub y: f32,

    /// Width.
    pub width: f32,

    /// Height.
    pub height: f32,
}

impl LayoutRect {
    /// Creates a rectangle with sanitized dimensions.
    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x: sanitize_coordinate(x),
            y: sanitize_coordinate(y),
            width: sanitize_dimension(width),
            height: sanitize_dimension(height),
        }
    }

    /// Returns the bottom edge.
    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}

/// One laid-out block box.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBox {
    /// Block type.
    pub kind: LayoutBoxKind,

    /// Border/content rectangle for the block.
    pub rect: LayoutRect,

    /// Optional block background, currently used by image placeholders.
    pub background: Option<Color>,

    /// Text runs inside this block.
    pub text_runs: Vec<LayoutTextRun>,
}

/// Semantic kind of a layout box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutBoxKind {
    /// Heading box.
    Heading {
        /// Heading level.
        level: u8,
    },

    /// Paragraph box.
    Paragraph,

    /// Link box.
    Link {
        /// Optional link target.
        href: Option<String>,
    },

    /// Image placeholder box.
    Image {
        /// Optional image source.
        src: Option<String>,

        /// Optional image alt text.
        alt: Option<String>,
    },

    /// Generic fallback box.
    Generic {
        /// Source tag name.
        tag: String,
    },
}

/// Positioned text run.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutTextRun {
    /// Left coordinate.
    pub x: f32,

    /// Top coordinate.
    pub y: f32,

    /// Text content.
    pub text: String,

    /// Text size.
    pub size: f32,

    /// Text color.
    pub color: Color,
}

/// Builds a viewport-aware layout tree from a render document.
#[must_use]
pub fn layout_document(doc: &RenderDocument, width: f32, height: f32) -> LayoutTree {
    let viewport = Viewport::new(width, height);
    let style = doc
        .style_sheet
        .compute(doc.theme_color, viewport.width, viewport.height);

    let content_rect = LayoutRect::new(
        style.content_x,
        style.content_y,
        style
            .content_width
            .min((viewport.width - style.content_x).max(1.0)),
        (viewport.height - style.content_y).max(0.0),
    );

    let mut boxes = Vec::with_capacity(doc.blocks.len());
    let mut cursor_y = style.content_y;
    let mut overflow_y = cursor_y;
    let mut clipped = false;

    for block in &doc.blocks {
        let Some(layout_box) = layout_block(block, cursor_y, &style, viewport) else {
            continue;
        };

        overflow_y = layout_box.rect.bottom();

        if layout_box.rect.y > viewport.height {
            clipped = true;
            break;
        }

        cursor_y = layout_box.rect.bottom() + style.line_gap;
        clipped |= cursor_y > viewport.height;
        boxes.push(layout_box);
    }

    LayoutTree {
        background: style.background,
        viewport,
        content_rect,
        boxes,
        overflow_y,
        clipped,
    }
}

fn layout_block(
    block: &RenderBlock,
    cursor_y: f32,
    style: &ComputedPaintStyle,
    viewport: Viewport,
) -> Option<LayoutBox> {
    match block {
        RenderBlock::Heading { level, text } => {
            let size = heading_size(*level, style);
            text_box(
                LayoutBoxKind::Heading { level: *level },
                text,
                style.content_x,
                cursor_y,
                style.content_width,
                size,
                style.text_color,
            )
        }
        RenderBlock::Paragraph { text } => text_box(
            LayoutBoxKind::Paragraph,
            text,
            style.content_x,
            cursor_y,
            style.content_width,
            style.paragraph_size,
            style.text_color,
        ),
        RenderBlock::Link { text, href } => {
            let display_text = link_display_text(text, href.as_deref());
            text_box(
                LayoutBoxKind::Link { href: href.clone() },
                &display_text,
                style.content_x,
                cursor_y,
                style.content_width,
                style.paragraph_size,
                style.link_color,
            )
        }
        RenderBlock::Image { alt, src } => Some(image_box(
            alt.clone(),
            src.clone(),
            cursor_y,
            style,
            viewport,
        )),
        RenderBlock::Generic { tag, text } => {
            let display_text = format!("<{tag}> {text}");
            text_box(
                LayoutBoxKind::Generic { tag: tag.clone() },
                &display_text,
                style.content_x,
                cursor_y,
                style.content_width,
                style.generic_size,
                style.text_color,
            )
        }
    }
}

fn text_box(
    kind: LayoutBoxKind,
    text: &str,
    x: f32,
    y: f32,
    width: f32,
    size: f32,
    color: Color,
) -> Option<LayoutBox> {
    if text.trim().is_empty() || width <= 0.0 || size <= 0.0 {
        return None;
    }

    let lines = wrap_text_to_width(text, width, size);
    let line_height = measure_line_height(size);
    let height = (lines.len().max(1) as f32) * line_height;

    let text_runs = lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| LayoutTextRun {
            x,
            y: (index as f32).mul_add(line_height, y),
            text: line,
            size,
            color,
        })
        .collect::<Vec<_>>();

    Some(LayoutBox {
        kind,
        rect: LayoutRect::new(x, y, width, height),
        background: None,
        text_runs,
    })
}

fn image_box(
    alt: Option<String>,
    src: Option<String>,
    cursor_y: f32,
    style: &ComputedPaintStyle,
    viewport: Viewport,
) -> LayoutBox {
    let placeholder_height = (style.paragraph_size * 6.0).clamp(MIN_IMAGE_HEIGHT, MAX_IMAGE_HEIGHT);
    let background = if style.background.luminance() > 0.45 {
        IMAGE_PLACEHOLDER_LIGHT
    } else {
        IMAGE_PLACEHOLDER_DARK
    };

    let label = image_display_text(alt.as_deref(), src.as_deref());
    let label_width = IMAGE_LABEL_INSET
        .mul_add(-2.0, style.content_width)
        .max(1.0);
    let label_lines = wrap_text_to_width(&label, label_width, style.generic_size);
    let line_height = measure_line_height(style.generic_size);
    let label_x = style.content_x + IMAGE_LABEL_INSET;
    let label_y = (cursor_y + IMAGE_LABEL_INSET).min(viewport.height);

    let text_runs = label_lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| LayoutTextRun {
            x: label_x,
            y: (index as f32).mul_add(line_height, label_y),
            text: line,
            size: style.generic_size,
            color: style.text_color,
        })
        .collect::<Vec<_>>();

    LayoutBox {
        kind: LayoutBoxKind::Image { src, alt },
        rect: LayoutRect::new(
            style.content_x,
            cursor_y,
            style.content_width,
            placeholder_height,
        ),
        background: Some(background),
        text_runs,
    }
}

#[must_use]
fn heading_size(level: u8, style: &ComputedPaintStyle) -> f32 {
    let index =
        usize::from(level.saturating_sub(1)).min(style.heading_sizes.len().saturating_sub(1));
    style.heading_sizes[index].max(8.0)
}

#[must_use]
fn link_display_text(text: &str, href: Option<&str>) -> String {
    if !text.trim().is_empty() {
        return text.trim().to_owned();
    }

    href.unwrap_or_default().trim().to_owned()
}

#[must_use]
fn image_display_text(alt: Option<&str>, src: Option<&str>) -> String {
    match (alt, src) {
        (Some(label), Some(path)) => format!("Image: {label} [{path}]"),
        (Some(label), None) => format!("Image: {label}"),
        (None, Some(path)) => format!("Image: {path}"),
        (None, None) => "Image".to_owned(),
    }
}

/// Wraps text into deterministic viewport-aware lines.
#[must_use]
pub fn wrap_text_to_width(text: &str, max_width: f32, size: f32) -> Vec<String> {
    let max_chars = estimate_max_chars(max_width, size);

    if max_chars == 0 {
        return vec![text.to_owned()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for word in text.split_whitespace() {
        let word_len = word.chars().count();

        if word_len > max_chars {
            flush_line(&mut lines, &mut current, &mut current_len);
            append_long_word_lines(word, max_chars, &mut lines, &mut current, &mut current_len);
            continue;
        }

        if current_len == 0 {
            current.push_str(word);
            current_len = word_len;
        } else if current_len + 1 + word_len <= max_chars {
            current.push(' ');
            current.push_str(word);
            current_len += 1 + word_len;
        } else {
            flush_line(&mut lines, &mut current, &mut current_len);
            current.push_str(word);
            current_len = word_len;
        }
    }

    flush_line(&mut lines, &mut current, &mut current_len);

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn append_long_word_lines(
    word: &str,
    max_chars: usize,
    lines: &mut Vec<String>,
    current: &mut String,
    current_len: &mut usize,
) {
    for ch in word.chars() {
        if *current_len >= max_chars {
            flush_line(lines, current, current_len);
        }

        current.push(ch);
        *current_len += 1;
    }
}

fn flush_line(lines: &mut Vec<String>, current: &mut String, current_len: &mut usize) {
    if current.is_empty() {
        return;
    }

    lines.push(std::mem::take(current));
    *current_len = 0;
}

/// Estimates rendered text width for the current font-atlas renderer.
///
/// The app crate performs true glyph rasterization, while the present crate
/// keeps layout deterministic and dependency-light by using a stable average
/// sans-serif advance estimate.
#[must_use]
pub fn measure_text_width(text: &str, size: f32) -> f32 {
    let glyph_advance = (size * GLYPH_WIDTH_RATIO).max(1.0);
    text.chars().count() as f32 * glyph_advance
}

/// Returns the deterministic line height used by layout.
#[must_use]
pub fn measure_line_height(size: f32) -> f32 {
    (size * LINE_HEIGHT_RATIO).max(1.0)
}

#[must_use]
fn estimate_max_chars(max_width: f32, size: f32) -> usize {
    let advance = (size * GLYPH_WIDTH_RATIO).max(1.0);
    let chars = (max_width / advance).floor();

    if chars.is_finite() && chars > 0.0 {
        chars as usize
    } else {
        0
    }
}

#[must_use]
fn sanitize_dimension(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}

#[must_use]
fn sanitize_coordinate(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}
