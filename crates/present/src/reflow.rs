#![doc = "Incremental reflow and dirty-region calculation for Sylphos."]
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use crate::{
    build_paint_plan_from_layout, layout_document, measure_line_height, measure_text_width,
    DirtyFlags, InvalidationSet, LayoutTree, PaintCommand, PaintPlan, RenderDocument,
};

const MAX_DIRTY_REGIONS: usize = 32;
const COMMAND_DIFF_FULL_REPAINT_THRESHOLD: usize = 96;

/// Rectangle in viewport-local paint coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirtyRect {
    /// Left coordinate.
    pub x: f32,

    /// Top coordinate.
    pub y: f32,

    /// Width.
    pub width: f32,

    /// Height.
    pub height: f32,
}

impl DirtyRect {
    /// Creates a sanitized dirty rectangle.
    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Option<Self> {
        if !x.is_finite() || !y.is_finite() || !width.is_finite() || !height.is_finite() {
            return None;
        }

        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        Some(Self {
            x,
            y,
            width,
            height,
        })
    }

    /// Returns a full-viewport rectangle.
    #[must_use]
    pub fn full(width: f32, height: f32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }

    /// Returns the right edge.
    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// Returns the bottom edge.
    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Returns the union of two rectangles.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self.right().max(other.right());
        let y2 = self.bottom().max(other.bottom());

        Self {
            x: x1,
            y: y1,
            width: (x2 - x1).max(1.0),
            height: (y2 - y1).max(1.0),
        }
    }

    /// Returns the rectangle translated by an offset.
    #[must_use]
    pub fn translate(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: self.width,
            height: self.height,
        }
    }

    /// Returns this rectangle clipped to a viewport.
    #[must_use]
    pub fn clipped_to_viewport(self, width: f32, height: f32) -> Option<Self> {
        let x1 = self.x.max(0.0);
        let y1 = self.y.max(0.0);
        let x2 = self.right().min(width.max(1.0));
        let y2 = self.bottom().min(height.max(1.0));

        Self::new(x1, y1, x2 - x1, y2 - y1)
    }
}

/// A bounded set of dirty regions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DirtyRegionSet {
    regions: Vec<DirtyRect>,
    full_repaint: bool,
}

impl DirtyRegionSet {
    /// Creates an empty set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            regions: Vec::new(),
            full_repaint: false,
        }
    }

    /// Creates a full-viewport dirty set.
    #[must_use]
    pub fn full(width: f32, height: f32) -> Self {
        Self {
            regions: vec![DirtyRect::full(width, height)],
            full_repaint: true,
        }
    }

    /// Adds one dirty rectangle.
    pub fn add(&mut self, rect: DirtyRect) {
        if self.full_repaint {
            return;
        }

        self.regions.push(rect);

        if self.regions.len() > MAX_DIRTY_REGIONS {
            self.coalesce_all();
        }
    }

    /// Adds all rectangles from another set.
    pub fn merge(&mut self, other: &Self) {
        if other.full_repaint {
            self.regions.clone_from(&other.regions);
            self.full_repaint = true;
            return;
        }

        for region in &other.regions {
            self.add(*region);
        }
    }

    /// Returns true if no dirty work exists.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Returns true if this update should be treated as a full repaint.
    #[must_use]
    pub const fn is_full_repaint(&self) -> bool {
        self.full_repaint
    }

    /// Returns dirty regions.
    #[must_use]
    pub fn regions(&self) -> &[DirtyRect] {
        &self.regions
    }

    /// Translates all regions.
    #[must_use]
    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            regions: self
                .regions
                .iter()
                .map(|region| region.translate(dx, dy))
                .collect(),
            full_repaint: self.full_repaint,
        }
    }

    fn coalesce_all(&mut self) {
        let Some(first) = self.regions.first().copied() else {
            return;
        };

        let union = self
            .regions
            .iter()
            .skip(1)
            .copied()
            .fold(first, DirtyRect::union);

        self.regions.clear();
        self.regions.push(union);
    }
}

/// Why the incremental engine chose a particular reflow mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflowReason {
    /// First layout for a document.
    Initial,

    /// Viewport dimensions changed.
    ViewportChanged,

    /// Style or structure dirtiness requires full layout.
    StructuralInvalidation,

    /// Layout dirtiness requires layout rebuild.
    LayoutInvalidation,

    /// Paint-only invalidation can reuse the previous layout tree.
    PaintOnlyInvalidation,

    /// No invalidation existed, but revision changed externally.
    ExternalRevision,
}

/// Reflow mode used for the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflowMode {
    /// Built layout and paint from scratch.
    Full,

    /// Reused layout and rebuilt paint.
    PaintOnly,

    /// Reused previous paint plan unchanged.
    Reused,
}

/// Input for one incremental reflow pass.
#[derive(Debug, Clone, Copy)]
pub struct ReflowRequest<'a> {
    /// Document to render.
    pub document: &'a RenderDocument,

    /// Page viewport width.
    pub width: f32,

    /// Page viewport height.
    pub height: f32,

    /// Optional mutation-driven invalidation set.
    pub invalidation: Option<&'a InvalidationSet>,

    /// Force full reflow, usually after resize or navigation.
    pub force_full: bool,
}

/// Output of a reflow pass.
#[derive(Debug, Clone)]
pub struct ReflowOutput {
    /// Paint plan for the current document.
    pub paint_plan: PaintPlan,

    /// Dirty regions comparing previous and current plans.
    pub dirty_regions: DirtyRegionSet,

    /// Chosen reflow mode.
    pub mode: ReflowMode,

    /// Reason for the chosen mode.
    pub reason: ReflowReason,

    /// Reflow generation.
    pub generation: u64,

    /// Number of commands in the previous plan.
    pub previous_command_count: usize,

    /// Number of commands in the current plan.
    pub current_command_count: usize,
}

/// Stateful incremental reflow engine for one page viewport.
#[derive(Debug, Clone, Default)]
pub struct IncrementalReflowEngine {
    generation: u64,
    last_width: Option<f32>,
    last_height: Option<f32>,
    last_document_fingerprint: Option<u64>,
    last_layout: Option<LayoutTree>,
    last_paint_plan: Option<PaintPlan>,
}

impl IncrementalReflowEngine {
    /// Creates a new empty reflow engine.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            generation: 0,
            last_width: None,
            last_height: None,
            last_document_fingerprint: None,
            last_layout: None,
            last_paint_plan: None,
        }
    }

    /// Clears cached layout and paint state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Executes an incremental reflow pass.
    #[must_use]
    pub fn update(&mut self, request: ReflowRequest<'_>) -> ReflowOutput {
        let width = sanitize_dimension(request.width);
        let height = sanitize_dimension(request.height);
        let fingerprint = document_fingerprint(request.document);
        let flags = request
            .invalidation
            .map_or_else(DirtyFlags::none, InvalidationSet::flags);

        let viewport_changed =
            self.last_width
                .zip(self.last_height)
                .map_or(true, |(last_w, last_h)| {
                    (last_w - width).abs() > f32::EPSILON || (last_h - height).abs() > f32::EPSILON
                });
        let document_changed = self.last_document_fingerprint != Some(fingerprint);

        let reason = if self.last_paint_plan.is_none() || request.force_full {
            ReflowReason::Initial
        } else if viewport_changed {
            ReflowReason::ViewportChanged
        } else if flags.style || flags.accessibility || flags.layout && document_changed {
            ReflowReason::StructuralInvalidation
        } else if flags.layout {
            ReflowReason::LayoutInvalidation
        } else if flags.paint || flags.hit_test {
            ReflowReason::PaintOnlyInvalidation
        } else if document_changed {
            ReflowReason::ExternalRevision
        } else {
            return self.reuse_previous();
        };

        let previous_plan = self.last_paint_plan.clone();
        let previous_command_count = previous_plan.as_ref().map_or(0, |plan| plan.commands.len());

        let (layout, paint_plan, mode) = if matches!(reason, ReflowReason::PaintOnlyInvalidation)
            && self.last_layout.is_some()
            && !document_changed
        {
            let Some(layout) = self.last_layout.clone() else {
                return self.reuse_previous();
            };
            let paint_plan = build_paint_plan_from_layout(&layout);
            (layout, paint_plan, ReflowMode::PaintOnly)
        } else {
            let layout = layout_document(request.document, width, height);
            let paint_plan = build_paint_plan_from_layout(&layout);
            (layout, paint_plan, ReflowMode::Full)
        };

        let dirty_regions =
            dirty_regions_between(previous_plan.as_ref(), &paint_plan, width, height)
                .unwrap_or_else(|| DirtyRegionSet::full(width, height));

        self.generation = self.generation.wrapping_add(1);
        self.last_width = Some(width);
        self.last_height = Some(height);
        self.last_document_fingerprint = Some(fingerprint);
        self.last_layout = Some(layout);
        self.last_paint_plan = Some(paint_plan.clone());

        ReflowOutput {
            paint_plan,
            dirty_regions,
            mode,
            reason,
            generation: self.generation,
            previous_command_count,
            current_command_count: self
                .last_paint_plan
                .as_ref()
                .map_or(0, |plan| plan.commands.len()),
        }
    }

    fn reuse_previous(&self) -> ReflowOutput {
        let paint_plan = self.last_paint_plan.clone().unwrap_or_else(|| PaintPlan {
            background: crate::Color::rgba(0.95, 0.95, 0.94, 1.0),
            commands: Vec::new(),
        });

        let count = paint_plan.commands.len();

        ReflowOutput {
            paint_plan,
            dirty_regions: DirtyRegionSet::new(),
            mode: ReflowMode::Reused,
            reason: ReflowReason::ExternalRevision,
            generation: self.generation,
            previous_command_count: count,
            current_command_count: count,
        }
    }
}

/// Computes dirty regions by comparing two paint plans.
#[must_use]
pub fn dirty_regions_between(
    previous: Option<&PaintPlan>,
    current: &PaintPlan,
    width: f32,
    height: f32,
) -> Option<DirtyRegionSet> {
    let Some(previous) = previous else {
        return Some(DirtyRegionSet::full(width, height));
    };

    if previous.background != current.background {
        return Some(DirtyRegionSet::full(width, height));
    }

    let max_len = previous.commands.len().max(current.commands.len());

    if max_len > COMMAND_DIFF_FULL_REPAINT_THRESHOLD
        && previous.commands.len().abs_diff(current.commands.len()) > 8
    {
        return Some(DirtyRegionSet::full(width, height));
    }

    let mut dirty = DirtyRegionSet::new();

    for index in 0..max_len {
        let old = previous.commands.get(index);
        let new = current.commands.get(index);

        if old == new {
            continue;
        }

        if let Some(rect) = old.and_then(command_bounds) {
            if let Some(clipped) = rect.clipped_to_viewport(width, height) {
                dirty.add(clipped);
            }
        }

        if let Some(rect) = new.and_then(command_bounds) {
            if let Some(clipped) = rect.clipped_to_viewport(width, height) {
                dirty.add(clipped);
            }
        }
    }

    if dirty.is_empty() {
        None
    } else {
        Some(dirty)
    }
}

/// Returns the approximate bounding rectangle for one paint command.
#[must_use]
pub fn command_bounds(command: &PaintCommand) -> Option<DirtyRect> {
    match command {
        PaintCommand::Rect {
            x,
            y,
            width,
            height,
            ..
        }
        | PaintCommand::Image {
            x,
            y,
            width,
            height,
            ..
        } => DirtyRect::new(*x, *y, *width, *height),
        PaintCommand::TextPlaceholder {
            x, y, text, size, ..
        } => DirtyRect::new(
            *x,
            *y,
            measure_text_width(text, *size).max(*size),
            measure_line_height(*size),
        ),
    }
}

fn document_fingerprint(document: &RenderDocument) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    hash = hash_str(hash, document.title.as_deref().unwrap_or_default());
    hash = hash_u64(hash, document.blocks.len() as u64);
    hash = hash_str(hash, &format!("{:?}", document.theme_color));
    hash = hash_str(hash, &format!("{:?}", document.style_sheet));

    for block in &document.blocks {
        hash = hash_str(hash, &format!("{block:?}"));
    }

    hash
}

fn hash_str(mut hash: u64, value: &str) -> u64 {
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn hash_u64(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn sanitize_dimension(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}
