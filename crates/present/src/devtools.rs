#![allow(dead_code)]
//! Presentation-layer trace helpers for Module 50.
//!
//! This module intentionally has no serde dependency. The app crate owns JSON
//! export; `present` only summarizes layout/reflow/paint data in stable,
//! copy-pasteable structs. Very rude of architecture to demand boundaries, but
//! here we are.

use crate::{
    LayoutBoxKind, LayoutTree, PaintCommand, PaintPlan, ReflowMode, ReflowOutput, ReflowReason,
};

/// Lightweight paint-plan summary for trace events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaintPlanTraceSummary {
    /// Total commands.
    pub commands: usize,

    /// Filled rectangle commands.
    pub rects: usize,

    /// Text placeholder commands.
    pub text: usize,

    /// Image commands.
    pub images: usize,

    /// Commands added by later modules, such as SVG/icon paint commands.
    pub other: usize,
}

impl PaintPlanTraceSummary {
    /// Builds a summary from a paint plan.
    #[must_use]
    pub fn from_plan(plan: &PaintPlan) -> Self {
        let mut summary = Self {
            commands: plan.commands.len(),
            ..Self::default()
        };

        for command in &plan.commands {
            match command {
                PaintCommand::Rect { .. } => summary.rects = summary.rects.saturating_add(1),
                PaintCommand::TextPlaceholder { .. } => {
                    summary.text = summary.text.saturating_add(1);
                }
                PaintCommand::Image { .. } => summary.images = summary.images.saturating_add(1),
                _ => summary.other = summary.other.saturating_add(1),
            }
        }

        summary
    }
}

/// Lightweight layout-tree summary for trace events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutTraceSummary {
    /// Number of layout boxes.
    pub boxes: usize,

    /// Number of text runs.
    pub text_runs: usize,

    /// Heading boxes.
    pub headings: usize,

    /// Paragraph boxes.
    pub paragraphs: usize,

    /// Link boxes.
    pub links: usize,

    /// Image boxes.
    pub images: usize,

    /// Inline-flow boxes.
    pub inline_flows: usize,

    /// Form-control boxes.
    pub form_controls: usize,

    /// Generic boxes.
    pub generic: usize,

    /// Whether layout clipped due to viewport height.
    pub clipped: bool,

    /// Content overflow Y rounded to logical pixels.
    pub overflow_y: u32,
}

impl LayoutTraceSummary {
    /// Builds a summary from a layout tree.
    #[must_use]
    pub fn from_layout(layout: &LayoutTree) -> Self {
        let mut summary = Self {
            boxes: layout.boxes.len(),
            clipped: layout.clipped,
            overflow_y: saturating_round_u32(layout.overflow_y),
            ..Self::default()
        };

        for layout_box in &layout.boxes {
            summary.text_runs = summary
                .text_runs
                .saturating_add(layout_box.text_runs.len());

            match &layout_box.kind {
                LayoutBoxKind::Heading { .. } => {
                    summary.headings = summary.headings.saturating_add(1);
                }
                LayoutBoxKind::Paragraph => {
                    summary.paragraphs = summary.paragraphs.saturating_add(1);
                }
                LayoutBoxKind::Link { .. } => summary.links = summary.links.saturating_add(1),
                LayoutBoxKind::Image { .. } => summary.images = summary.images.saturating_add(1),
                LayoutBoxKind::InlineFlow => {
                    summary.inline_flows = summary.inline_flows.saturating_add(1);
                }
                LayoutBoxKind::FormControl { .. } => {
                    summary.form_controls = summary.form_controls.saturating_add(1);
                }
                LayoutBoxKind::Generic { .. } => {
                    summary.generic = summary.generic.saturating_add(1);
                }
            }
        }

        summary
    }
}

/// Reflow summary for trace events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflowTraceSummary {
    /// Chosen mode.
    pub mode: String,

    /// Reason.
    pub reason: String,

    /// Generation.
    pub generation: u64,

    /// Previous paint command count.
    pub previous_commands: usize,

    /// Current paint command count.
    pub current_commands: usize,

    /// Dirty region count.
    pub dirty_regions: usize,

    /// Whether the update requested a full repaint.
    pub full_repaint: bool,
}

impl ReflowTraceSummary {
    /// Builds a summary from a reflow output.
    #[must_use]
    pub fn from_output(output: &ReflowOutput) -> Self {
        Self {
            mode: reflow_mode_name(output.mode).to_owned(),
            reason: reflow_reason_name(output.reason).to_owned(),
            generation: output.generation,
            previous_commands: output.previous_command_count,
            current_commands: output.current_command_count,
            dirty_regions: output.dirty_regions.regions().len(),
            full_repaint: output.dirty_regions.is_full_repaint(),
        }
    }
}

/// Builds a paint summary from any plan.
#[must_use]
pub fn summarize_paint_plan(plan: &PaintPlan) -> PaintPlanTraceSummary {
    PaintPlanTraceSummary::from_plan(plan)
}

/// Builds a layout summary from any layout tree.
#[must_use]
pub fn summarize_layout_tree(layout: &LayoutTree) -> LayoutTraceSummary {
    LayoutTraceSummary::from_layout(layout)
}

/// Builds a reflow summary from an incremental reflow output.
#[must_use]
pub fn summarize_reflow_output(output: &ReflowOutput) -> ReflowTraceSummary {
    ReflowTraceSummary::from_output(output)
}

fn reflow_mode_name(mode: ReflowMode) -> &'static str {
    match mode {
        ReflowMode::Full => "full",
        ReflowMode::PaintOnly => "paint-only",
        ReflowMode::Reused => "reused",
    }
}

fn reflow_reason_name(reason: ReflowReason) -> &'static str {
    match reason {
        ReflowReason::Initial => "initial",
        ReflowReason::ViewportChanged => "viewport-changed",
        ReflowReason::StructuralInvalidation => "structural-invalidation",
        ReflowReason::LayoutInvalidation => "layout-invalidation",
        ReflowReason::PaintOnlyInvalidation => "paint-only-invalidation",
        ReflowReason::ExternalRevision => "external-revision",
    }
}

fn saturating_round_u32(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }

    if value >= u32::MAX as f32 {
        return u32::MAX;
    }

    value.round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, PaintCommand, PaintPlan};

    #[test]
    fn summarizes_paint_commands() {
        let plan = PaintPlan {
            background: Color::rgba(1.0, 1.0, 1.0, 1.0),
            commands: vec![
                PaintCommand::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                    color: Color::rgba(0.0, 0.0, 0.0, 1.0),
                },
                PaintCommand::TextPlaceholder {
                    x: 1.0,
                    y: 2.0,
                    text: "hello".to_owned(),
                    size: 16.0,
                    color: Color::rgba(0.0, 0.0, 0.0, 1.0),
                },
            ],
        };

        let summary = PaintPlanTraceSummary::from_plan(&plan);
        assert_eq!(summary.commands, 2);
        assert_eq!(summary.rects, 1);
        assert_eq!(summary.text, 1);
    }
}
