#![doc = "Viewport-aware layout engine for the Sylphos presentation layer."]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use crate::box_model::{BoxStyleLite, ComputedBoxStyle, EdgeSizes};
use crate::selector::ComputedNodeStyle;
use crate::{
    Color, ComputedPaintStyle, FormBlock, FormControl, FormControlKind, InlineFragment,
    RenderBlock, RenderDocument,
};

const GLYPH_WIDTH_RATIO: f32 = 0.56;
const LINE_HEIGHT_RATIO: f32 = 1.35;
const IMAGE_PLACEHOLDER_LIGHT: Color = Color::rgba(0.78, 0.80, 0.84, 1.0);
const IMAGE_PLACEHOLDER_DARK: Color = Color::rgba(0.18, 0.20, 0.26, 1.0);
const IMAGE_LABEL_INSET: f32 = 12.0;
const MIN_IMAGE_HEIGHT: f32 = 72.0;
const MAX_IMAGE_HEIGHT: f32 = 180.0;

const FORM_GAP: f32 = 10.0;
const CONTROL_HEIGHT: f32 = 38.0;
const TEXTAREA_HEIGHT: f32 = 88.0;
const BUTTON_WIDTH: f32 = 170.0;
const CONTROL_TEXT_INSET_X: f32 = 12.0;
const CONTROL_TEXT_INSET_Y: f32 = 9.0;
const CONTROL_BG: Color = Color::rgba(0.965, 0.970, 0.985, 1.0);
const CONTROL_BG_FOCUSED: Color = Color::rgba(0.925, 0.945, 1.0, 1.0);
const CONTROL_BG_DISABLED: Color = Color::rgba(0.82, 0.83, 0.86, 1.0);
const BUTTON_BG: Color = Color::rgba(0.20, 0.32, 0.58, 1.0);
const CONTROL_TEXT: Color = Color::rgba(0.10, 0.11, 0.13, 1.0);
const CONTROL_TEXT_MUTED: Color = Color::rgba(0.45, 0.47, 0.52, 1.0);
const BUTTON_TEXT: Color = Color::rgba(0.98, 0.99, 1.0, 1.0);

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

    /// Returns an inset rectangle.
    #[must_use]
    pub fn inset(self, edges: EdgeSizes) -> Self {
        Self::new(
            self.x + edges.left,
            self.y + edges.top,
            (self.width - edges.horizontal()).max(1.0),
            (self.height - edges.vertical()).max(1.0),
        )
    }
}

/// One laid-out block box.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBox {
    /// Block type.
    pub kind: LayoutBoxKind,

    /// Border-box rectangle for the block.
    pub rect: LayoutRect,

    /// Optional block background.
    pub background: Option<Color>,

    /// Optional border descriptor.
    pub border: Option<LayoutBorder>,

    /// Additional margin after this box.
    pub margin_after: f32,

    /// Text runs inside this block.
    pub text_runs: Vec<LayoutTextRun>,
}

/// Border information for a layout box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutBorder {
    /// Border widths.
    pub widths: EdgeSizes,

    /// Border color.
    pub color: Color,
}

/// Semantic kind of a layout box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutBoxKind {
    /// Heading box.
    Heading {
        /// HTML heading rank, where `1` is the highest-level heading.
        level: u8,
    },

    /// Paragraph box.
    Paragraph,

    /// Link box.
    Link {
        /// Target URL from the link, when one was provided.
        href: Option<String>,
    },

    /// Image placeholder box.
    Image {
        /// Image source URL, when one was provided.
        src: Option<String>,

        /// Alternative text, when one was provided.
        alt: Option<String>,
    },

    /// Paragraph-like mixed inline-flow box.
    InlineFlow,

    /// Form control box.
    FormControl {
        /// Parent form id.
        form_id: u64,

        /// Control id.
        control_id: u64,

        /// Control kind.
        kind: FormControlKind,

        /// Optional control name.
        name: Option<String>,
    },

    /// Generic fallback box.
    Generic {
        /// Source HTML tag represented by this fallback box.
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

    /// Optional link target carried by inline-flow runs.
    pub href: Option<String>,
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

    for (block_index, block) in doc.blocks.iter().enumerate() {
        let node_style = doc.computed_style_for_block(block_index);
        let block_boxes = layout_block(block, cursor_y, &style, viewport, node_style);
        if block_boxes.is_empty() {
            continue;
        }

        for layout_box in block_boxes {
            overflow_y = overflow_y.max(layout_box.rect.bottom() + layout_box.margin_after);

            if layout_box.rect.y > viewport.height {
                clipped = true;
                break;
            }

            cursor_y =
                cursor_y.max(layout_box.rect.bottom() + layout_box.margin_after + style.line_gap);
            clipped |= cursor_y > viewport.height;
            boxes.push(layout_box);
        }

        if clipped && cursor_y > viewport.height {
            break;
        }
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
    node_style: Option<&ComputedNodeStyle>,
) -> Vec<LayoutBox> {
    match block {
        RenderBlock::Heading { level, text } => {
            let size = node_style
                .and_then(|node_style| node_style.font_size)
                .unwrap_or_else(|| heading_size(*level, style));
            let color = node_style
                .and_then(|node_style| node_style.text_color)
                .unwrap_or(style.text_color);
            let box_style =
                apply_node_box_override(style.heading_box(*level), node_style, style.text_color);
            text_box(
                LayoutBoxKind::Heading { level: *level },
                text,
                style,
                cursor_y,
                size,
                color,
                box_style,
            )
            .into_iter()
            .collect()
        }
        RenderBlock::Paragraph { text } => text_box(
            LayoutBoxKind::Paragraph,
            text,
            style,
            cursor_y,
            node_style
                .and_then(|node_style| node_style.font_size)
                .unwrap_or(style.paragraph_size),
            node_style
                .and_then(|node_style| node_style.text_color)
                .unwrap_or(style.text_color),
            apply_node_box_override(style.box_model.paragraph, node_style, style.text_color),
        )
        .into_iter()
        .collect(),
        RenderBlock::Link { text, href } => {
            let display_text = link_display_text(text, href.as_deref());
            text_box(
                LayoutBoxKind::Link { href: href.clone() },
                &display_text,
                style,
                cursor_y,
                node_style
                    .and_then(|node_style| node_style.font_size)
                    .unwrap_or(style.paragraph_size),
                node_style
                    .and_then(|node_style| node_style.text_color)
                    .unwrap_or(style.link_color),
                apply_node_box_override(style.box_model.link, node_style, style.text_color),
            )
            .into_iter()
            .collect()
        }
        RenderBlock::InlineFlow { fragments } => {
            layout_inline_flow(fragments, cursor_y, style, node_style)
                .into_iter()
                .collect()
        }
        RenderBlock::Image { alt, src } => {
            let box_style =
                apply_node_box_override(style.box_model.image, node_style, style.text_color);
            if box_style.is_display_none() {
                Vec::new()
            } else {
                vec![image_box(
                    alt.clone(),
                    src.clone(),
                    cursor_y,
                    style,
                    viewport,
                    box_style,
                )]
            }
        }
        RenderBlock::Form(form) => layout_form(form, cursor_y, style, node_style),
        RenderBlock::Generic { tag, text } => {
            let display_text = format!("<{tag}> {text}");
            let box_style =
                apply_node_box_override(style.box_model.div, node_style, style.text_color);
            text_box(
                LayoutBoxKind::Generic { tag: tag.clone() },
                &display_text,
                style,
                cursor_y,
                node_style
                    .and_then(|node_style| node_style.font_size)
                    .unwrap_or(style.generic_size),
                node_style
                    .and_then(|node_style| node_style.text_color)
                    .unwrap_or(style.text_color),
                box_style,
            )
            .into_iter()
            .collect()
        }
    }
}

fn apply_node_box_override(
    mut base: ComputedBoxStyle,
    node_style: Option<&ComputedNodeStyle>,
    fallback_border_color: Color,
) -> ComputedBoxStyle {
    let Some(node_style) = node_style else {
        return base;
    };

    let override_style: BoxStyleLite = node_style.box_style;

    if let Some(margin) = override_style.margin {
        base.margin = margin.sanitized();
    }
    if let Some(padding) = override_style.padding {
        base.padding = padding.sanitized();
    }
    if let Some(border_width) = override_style.border_width {
        base.border_width = border_width.sanitized();
    }
    if let Some(border_color) = override_style.border_color {
        base.border_color = Some(border_color);
    } else if !base.border_width.is_zero() && base.border_color.is_none() {
        base.border_color = Some(fallback_border_color);
    }
    if let Some(background) = override_style.background {
        base.background = Some(background);
    }
    if let Some(display) = override_style.display {
        base.display = display;
    }

    if base.border_width.is_zero() {
        base.border_color = None;
    }

    base
}

impl ComputedPaintStyle {
    fn heading_box(&self, level: u8) -> ComputedBoxStyle {
        let index = usize::from(level.saturating_sub(1)).min(self.box_model.headings.len() - 1);
        self.box_model.headings[index]
    }
}

fn text_box(
    kind: LayoutBoxKind,
    text: &str,
    style: &ComputedPaintStyle,
    cursor_y: f32,
    size: f32,
    color: Color,
    box_style: ComputedBoxStyle,
) -> Option<LayoutBox> {
    if text.trim().is_empty()
        || style.content_width <= 0.0
        || size <= 0.0
        || box_style.is_display_none()
    {
        return None;
    }

    let metrics = BoxMetrics::new(style.content_x, cursor_y, style.content_width, box_style);
    let lines = wrap_text_to_width(text, metrics.inner_rect.width, size);
    let line_height = measure_line_height(size);
    let content_height = (lines.len().max(1) as f32) * line_height;
    let rect = metrics.outer_rect_with_content_height(content_height);
    let inner = rect.inset(box_style.border_width).inset(box_style.padding);

    let text_runs = lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| LayoutTextRun {
            x: inner.x,
            y: (index as f32).mul_add(line_height, inner.y),
            text: line,
            size,
            color,
            href: None,
        })
        .collect::<Vec<_>>();

    Some(LayoutBox {
        kind,
        rect,
        background: box_style.background,
        border: layout_border(box_style),
        margin_after: box_style.margin.bottom,
        text_runs,
    })
}

fn image_box(
    alt: Option<String>,
    src: Option<String>,
    cursor_y: f32,
    style: &ComputedPaintStyle,
    viewport: Viewport,
    box_style: ComputedBoxStyle,
) -> LayoutBox {
    let placeholder_height = (style.paragraph_size * 6.0).clamp(MIN_IMAGE_HEIGHT, MAX_IMAGE_HEIGHT);
    let default_background = if style.background.luminance() > 0.45 {
        IMAGE_PLACEHOLDER_LIGHT
    } else {
        IMAGE_PLACEHOLDER_DARK
    };
    let metrics = BoxMetrics::new(style.content_x, cursor_y, style.content_width, box_style);
    let rect = metrics.outer_rect_with_content_height(placeholder_height);
    let inner = rect.inset(box_style.border_width).inset(box_style.padding);

    let label = image_display_text(alt.as_deref(), src.as_deref());
    let label_width = IMAGE_LABEL_INSET.mul_add(-2.0, inner.width).max(1.0);
    let label_lines = wrap_text_to_width(&label, label_width, style.generic_size);
    let line_height = measure_line_height(style.generic_size);
    let label_x = inner.x + IMAGE_LABEL_INSET;
    let label_y = (inner.y + IMAGE_LABEL_INSET).min(viewport.height);

    let text_runs = label_lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| LayoutTextRun {
            x: label_x,
            y: (index as f32).mul_add(line_height, label_y),
            text: line,
            size: style.generic_size,
            color: style.text_color,
            href: None,
        })
        .collect::<Vec<_>>();

    LayoutBox {
        kind: LayoutBoxKind::Image { src, alt },
        rect,
        background: box_style.background.or(Some(default_background)),
        border: layout_border(box_style),
        margin_after: box_style.margin.bottom,
        text_runs,
    }
}

fn layout_inline_flow(
    fragments: &[InlineFragment],
    cursor_y: f32,
    style: &ComputedPaintStyle,
    node_style: Option<&ComputedNodeStyle>,
) -> Option<LayoutBox> {
    if fragments.is_empty() {
        return None;
    }

    let size = node_style
        .and_then(|node_style| node_style.font_size)
        .unwrap_or(style.paragraph_size);
    let base_color = node_style
        .and_then(|node_style| node_style.text_color)
        .unwrap_or(style.text_color);
    let box_style =
        apply_node_box_override(style.box_model.paragraph, node_style, style.text_color);
    if box_style.is_display_none() {
        return None;
    }

    let metrics = BoxMetrics::new(style.content_x, cursor_y, style.content_width, box_style);
    let line_height = measure_line_height(size);
    let max_width = metrics.inner_rect.width.max(1.0);
    let mut runs = Vec::new();
    let mut x = metrics.inner_rect.x;
    let mut y = metrics.inner_rect.y;
    let mut line_has_content = false;
    let space_width = measure_text_width(" ", size);

    for fragment in fragments {
        let href = fragment.href().map(ToOwned::to_owned);
        let color = if href.is_some() {
            style.link_color
        } else {
            base_color
        };
        let words = fragment
            .text_content()
            .split_whitespace()
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>();

        for word in words {
            let word_width = measure_text_width(word, size);
            let prefix = if line_has_content { space_width } else { 0.0 };

            if line_has_content && x + prefix + word_width > metrics.inner_rect.x + max_width {
                x = metrics.inner_rect.x;
                y += line_height;
                line_has_content = false;
            }

            if word_width > max_width {
                let chunks = split_word_to_width(word, max_width, size);
                for chunk in chunks {
                    let chunk_width = measure_text_width(&chunk, size);
                    if line_has_content
                        && x + space_width + chunk_width > metrics.inner_rect.x + max_width
                    {
                        x = metrics.inner_rect.x;
                        y += line_height;
                        line_has_content = false;
                    }
                    let run_x = if line_has_content { x + space_width } else { x };
                    runs.push(LayoutTextRun {
                        x: run_x,
                        y,
                        text: chunk,
                        size,
                        color,
                        href: href.clone(),
                    });
                    x = run_x + chunk_width;
                    line_has_content = true;
                }
                continue;
            }

            let run_x = if line_has_content { x + space_width } else { x };
            runs.push(LayoutTextRun {
                x: run_x,
                y,
                text: word.to_owned(),
                size,
                color,
                href: href.clone(),
            });
            x = run_x + word_width;
            line_has_content = true;
        }
    }

    if runs.is_empty() {
        return None;
    }

    let line_count = ((y - metrics.inner_rect.y) / line_height).floor() + 1.0;
    let content_height = line_count.max(1.0) * line_height;
    let rect = metrics.outer_rect_with_content_height(content_height);

    Some(LayoutBox {
        kind: LayoutBoxKind::InlineFlow,
        rect,
        background: box_style.background,
        border: layout_border(box_style),
        margin_after: box_style.margin.bottom,
        text_runs: runs,
    })
}

fn split_word_to_width(word: &str, max_width: f32, size: f32) -> Vec<String> {
    let max_chars = estimate_max_chars(max_width, size).max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut count = 0usize;

    for ch in word.chars() {
        if count >= max_chars {
            chunks.push(std::mem::take(&mut current));
            count = 0;
        }
        current.push(ch);
        count += 1;
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn layout_form(
    form: &FormBlock,
    cursor_y: f32,
    style: &ComputedPaintStyle,
    node_style: Option<&ComputedNodeStyle>,
) -> Vec<LayoutBox> {
    let form_style = apply_node_box_override(style.box_model.form, node_style, style.text_color);
    if form_style.is_display_none() {
        return Vec::new();
    }

    let metrics = BoxMetrics::new(style.content_x, cursor_y, style.content_width, form_style);
    let mut boxes = Vec::new();
    let mut y = metrics.inner_rect.y;

    for control in &form.controls {
        if !control.kind.is_visible() {
            continue;
        }

        let layout_box = layout_control(
            form.id,
            control,
            y,
            style,
            metrics.inner_rect.x,
            metrics.inner_rect.width,
        );
        y = layout_box.rect.bottom() + FORM_GAP;
        boxes.push(layout_box);
    }

    if let Some(last) = boxes.last_mut() {
        last.margin_after += form_style.margin.bottom;
    }

    boxes
}

fn layout_control(
    form_id: u64,
    control: &FormControl,
    y: f32,
    style: &ComputedPaintStyle,
    base_x: f32,
    base_width: f32,
) -> LayoutBox {
    let is_button = control.kind.is_submit_like();
    let control_style = control_box_style(control, style);
    let height = if control.kind == FormControlKind::TextArea {
        TEXTAREA_HEIGHT
    } else {
        CONTROL_HEIGHT
    };
    let width = if is_button {
        BUTTON_WIDTH.min(base_width)
    } else {
        base_width
    };

    let metrics = BoxMetrics::new(base_x, y, width, control_style);
    let rect = metrics.outer_rect_with_content_height(height);
    let inner = rect
        .inset(control_style.border_width)
        .inset(control_style.padding);
    let background = control_style
        .background
        .unwrap_or_else(|| control_background(control));
    let text = control.display_text();
    let color = if control.value.is_empty() && control.placeholder.is_some() {
        CONTROL_TEXT_MUTED
    } else if is_button {
        BUTTON_TEXT
    } else {
        CONTROL_TEXT
    };
    let size = style.paragraph_size.max(12.0);
    let available_text_width = CONTROL_TEXT_INSET_X.mul_add(-2.0, inner.width).max(1.0);
    let display_text = if text.is_empty() && !is_button {
        control.name.clone().unwrap_or_default()
    } else {
        text
    };
    let mut runs = Vec::new();

    if !display_text.is_empty() {
        let lines = if control.kind == FormControlKind::TextArea {
            wrap_text_to_width(&display_text, available_text_width, size)
        } else {
            vec![truncate_to_width(&display_text, available_text_width, size)]
        };

        let line_height = measure_line_height(size);
        for (index, line) in lines.into_iter().enumerate() {
            let run_y = (index as f32).mul_add(line_height, inner.y + CONTROL_TEXT_INSET_Y);
            if run_y + line_height > inner.y + height {
                break;
            }
            runs.push(LayoutTextRun {
                x: inner.x + CONTROL_TEXT_INSET_X,
                y: run_y,
                text: line,
                size,
                color,
                href: None,
            });
        }
    }

    if control.focused && control.can_focus() {
        let cursor_x = inner.x
            + CONTROL_TEXT_INSET_X
            + measure_text_width(
                &truncate_to_width(&control.display_text(), available_text_width, size),
                size,
            )
            .min(available_text_width - 4.0);
        runs.push(LayoutTextRun {
            x: cursor_x,
            y: inner.y + CONTROL_TEXT_INSET_Y,
            text: "|".to_owned(),
            size,
            color: CONTROL_TEXT,
            href: None,
        });
    }

    LayoutBox {
        kind: LayoutBoxKind::FormControl {
            form_id,
            control_id: control.id,
            kind: control.kind,
            name: control.name.clone(),
        },
        rect,
        background: Some(background),
        border: layout_border(control_style),
        margin_after: control_style.margin.bottom,
        text_runs: runs,
    }
}

const fn control_box_style(control: &FormControl, style: &ComputedPaintStyle) -> ComputedBoxStyle {
    match control.kind {
        FormControlKind::TextArea => style.box_model.textarea,
        kind if kind.is_submit_like() => style.box_model.button,
        _ => style.box_model.input,
    }
}

const fn control_background(control: &FormControl) -> Color {
    if control.disabled {
        CONTROL_BG_DISABLED
    } else if control.kind.is_submit_like() {
        BUTTON_BG
    } else if control.focused {
        CONTROL_BG_FOCUSED
    } else {
        CONTROL_BG
    }
}

#[derive(Debug, Clone, Copy)]
struct BoxMetrics {
    inner_rect: LayoutRect,
    outer_x: f32,
    outer_y: f32,
    outer_width: f32,
    chrome_vertical: f32,
}

impl BoxMetrics {
    fn new(content_x: f32, cursor_y: f32, content_width: f32, style: ComputedBoxStyle) -> Self {
        let margin = style.margin;
        let padding = style.padding;
        let border = style.border_width;
        let outer_x = content_x + margin.left;
        let outer_y = cursor_y + margin.top;
        let outer_width = (content_width - margin.horizontal()).max(1.0);
        let inner_width = (outer_width - border.horizontal() - padding.horizontal()).max(1.0);
        let inner_rect = LayoutRect::new(
            outer_x + border.left + padding.left,
            outer_y + border.top + padding.top,
            inner_width,
            1.0,
        );

        Self {
            inner_rect,
            outer_x,
            outer_y,
            outer_width,
            chrome_vertical: border.vertical() + padding.vertical(),
        }
    }

    fn outer_rect_with_content_height(self, content_height: f32) -> LayoutRect {
        LayoutRect::new(
            self.outer_x,
            self.outer_y,
            self.outer_width,
            (content_height + self.chrome_vertical).max(1.0),
        )
    }
}

fn layout_border(style: ComputedBoxStyle) -> Option<LayoutBorder> {
    style.border_color.map(|color| LayoutBorder {
        widths: style.border_width,
        color,
    })
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

fn truncate_to_width(text: &str, max_width: f32, size: f32) -> String {
    let max_chars = estimate_max_chars(max_width, size);
    if max_chars == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    if max_chars <= 3 {
        return text.chars().take(max_chars).collect();
    }
    let keep = max_chars - 3;
    let skip = count - keep;
    let start = text
        .char_indices()
        .nth(skip)
        .map_or(text.len(), |(index, _)| index);
    format!("...{}", &text[start..])
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
