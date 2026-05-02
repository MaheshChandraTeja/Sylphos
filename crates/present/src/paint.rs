#![doc = "Deterministic paint plan generation from viewport layout."]

use crate::{layout_document, Color, LayoutBoxKind, LayoutTree, RenderDocument};

/// Deterministic paint plan for the renderer.
///
/// The paint plan is a small, stable intermediate representation. The `app`
/// crate converts these commands into GPU mesh data.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintPlan {
    /// Background color selected for the document.
    pub background: Color,

    /// Ordered paint commands.
    pub commands: Vec<PaintCommand>,
}

/// Primitive paint operation.
#[derive(Debug, Clone, PartialEq)]
pub enum PaintCommand {
    /// Filled rectangle command.
    Rect {
        /// Left position in logical pixels.
        x: f32,

        /// Top position in logical pixels.
        y: f32,

        /// Rectangle width in logical pixels.
        width: f32,

        /// Rectangle height in logical pixels.
        height: f32,

        /// Fill color.
        color: Color,
    },

    /// Text placeholder command.
    TextPlaceholder {
        /// Left position in logical pixels.
        x: f32,

        /// Top position in logical pixels.
        y: f32,

        /// Text to display.
        text: String,

        /// Text size in logical pixels.
        size: f32,

        /// Text color.
        color: Color,
    },

    /// Image command.
    ///
    /// This command identifies a source image and a rectangle where that image
    /// should be drawn. The app renderer owns runtime fetching, decoding, and
    /// texture upload. If the image is unavailable, the renderer draws the
    /// supplied background as a deterministic placeholder.
    Image {
        /// Left position in logical pixels.
        x: f32,

        /// Top position in logical pixels.
        y: f32,

        /// Image box width in logical pixels.
        width: f32,

        /// Image box height in logical pixels.
        height: f32,

        /// Optional image source URL or path as extracted from HTML.
        src: Option<String>,

        /// Optional alternate text.
        alt: Option<String>,

        /// Placeholder/background color.
        background: Color,
    },
}

/// Builds a deterministic, style-aware paint plan from a render document.
#[must_use]
pub fn build_paint_plan(doc: &RenderDocument, width: f32, height: f32) -> PaintPlan {
    let layout = layout_document(doc, width, height);
    build_paint_plan_from_layout(&layout)
}

/// Converts a layout tree into a paint plan.
#[must_use]
pub fn build_paint_plan_from_layout(layout: &LayoutTree) -> PaintPlan {
    let mut commands = Vec::with_capacity(
        layout
            .boxes
            .iter()
            .map(|layout_box| layout_box.text_runs.len().saturating_add(1))
            .sum::<usize>()
            .saturating_add(1),
    );

    commands.push(PaintCommand::Rect {
        x: 0.0,
        y: 0.0,
        width: layout.viewport.width,
        height: layout.viewport.height,
        color: layout.background,
    });

    for layout_box in &layout.boxes {
        if let LayoutBoxKind::Image { src, alt } = &layout_box.kind {
            commands.push(PaintCommand::Image {
                x: layout_box.rect.x,
                y: layout_box.rect.y,
                width: layout_box.rect.width,
                height: layout_box.rect.height,
                src: src.clone(),
                alt: alt.clone(),
                background: layout_box
                    .background
                    .unwrap_or_else(|| placeholder_from_background(layout.background)),
            });
            continue;
        }

        if let Some(color) = layout_box.background {
            commands.push(PaintCommand::Rect {
                x: layout_box.rect.x,
                y: layout_box.rect.y,
                width: layout_box.rect.width,
                height: layout_box.rect.height,
                color,
            });
        }

        for run in &layout_box.text_runs {
            if run.text.is_empty() {
                continue;
            }

            commands.push(PaintCommand::TextPlaceholder {
                x: run.x,
                y: run.y,
                text: run.text.clone(),
                size: run.size,
                color: run.color,
            });
        }
    }

    PaintPlan {
        background: layout.background,
        commands,
    }
}

fn placeholder_from_background(background: Color) -> Color {
    if background.luminance() > 0.45 {
        Color::rgba(0.78, 0.80, 0.84, 1.0)
    } else {
        Color::rgba(0.18, 0.20, 0.26, 1.0)
    }
}
