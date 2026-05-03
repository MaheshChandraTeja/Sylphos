#![allow(clippy::too_many_lines)]
#![doc = "Style/layout/paint invalidation bridge for SylJS."]
#![doc = ""]
#![doc = "Module 40 consumes ScriptPipeline reflow requests, CSSOM mutations,"]
#![doc = "and DOM snapshots, then produces coalesced style/layout/paint plans"]
#![doc = "that the app renderer can use to rebuild layout trees and PaintPlans."]

use crate::{
    script_pipeline::{
        DirtyFlag, DirtyFlags, ReflowRequest, ScriptDescriptor, ScriptExecutionFailure,
        ScriptPipelineHooks,
    },
    CssStyleMutation, DomNodeRef, DomNodeSnapshot, DomNodeType, StyleInvalidationKind,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeSet, VecDeque},
    fmt,
    rc::Rc,
};

/// Shared invalidation engine pointer.
pub type SharedInvalidationEngine = Rc<RefCell<InvalidationEngine>>;

/// Invalidation source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InvalidationSource {
    /// DOM tree mutation.
    Dom,

    /// CSSOM or inline style mutation.
    Cssom,

    /// Script pipeline lifecycle/reflow request.
    ScriptPipeline,

    /// Resource load affected layout/paint.
    Resource,

    /// Viewport resize.
    Viewport,

    /// Form/control state changed.
    Form,

    /// Media/canvas visual state changed.
    VisualRuntime,
}

/// Invalidation priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InvalidationPriority {
    /// Can be delayed.
    Low,

    /// Normal invalidation.
    Normal,

    /// Needed before next paint.
    High,

    /// Must be flushed immediately.
    Critical,
}

impl Default for InvalidationPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Invalidation scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InvalidationScope {
    /// Whole document.
    Document,

    /// Viewport-level.
    Viewport,

    /// Subtree rooted at a DOM node.
    Subtree(DomNodeRef),

    /// Single node.
    Node(DomNodeRef),

    /// Unknown or detached scope.
    Unknown,
}

/// Normalized invalidation node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InvalidationNode {
    /// DOM node id.
    pub dom_node: DomNodeRef,
}

/// Invalidation rectangle in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InvalidationRect {
    /// X coordinate.
    pub x: f32,

    /// Y coordinate.
    pub y: f32,

    /// Width.
    pub width: f32,

    /// Height.
    pub height: f32,
}

impl InvalidationRect {
    /// Creates a rectangle.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Full viewport marker.
    #[must_use]
    pub const fn viewport() -> Self {
        Self::new(0.0, 0.0, f32::MAX, f32::MAX)
    }

    /// Returns true if rectangle is full viewport marker.
    #[must_use]
    pub fn is_viewport(&self) -> bool {
        self.width == f32::MAX && self.height == f32::MAX
    }

    /// Unions two rectangles.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if self.is_viewport() || other.is_viewport() {
            return Self::viewport();
        }

        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);

        Self {
            x: left,
            y: top,
            width: (right - left).max(0.0),
            height: (bottom - top).max(0.0),
        }
    }
}

/// Style invalidation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StyleInvalidationKindLite {
    /// No style work.
    None,

    /// Recompute inline style only.
    Inline,

    /// Re-match selectors for subtree.
    SelectorMatch,

    /// Full cascade recomputation.
    Cascade,
}

/// Layout invalidation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LayoutInvalidationKind {
    /// No layout work.
    None,

    /// Measure text or intrinsic size.
    IntrinsicSize,

    /// Reflow a node.
    Node,

    /// Reflow subtree.
    Subtree,

    /// Rebuild layout tree.
    Tree,

    /// Full viewport layout.
    Viewport,
}

/// Paint invalidation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PaintInvalidationKind {
    /// No paint work.
    None,

    /// Repaint a node.
    Node,

    /// Repaint stacking context/subtree.
    Subtree,

    /// Rebuild PaintPlan.
    PaintPlan,

    /// Full viewport repaint.
    Viewport,
}

/// Renderer rebuild hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RebuildHint {
    /// No rebuild needed.
    None,

    /// Style recomputation only.
    StyleOnly,

    /// Layout tree update.
    LayoutTree,

    /// PaintPlan regeneration.
    PaintPlan,

    /// Full render pipeline.
    FullPipeline,
}

impl Default for RebuildHint {
    fn default() -> Self {
        Self::None
    }
}

/// Invalidation impact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidationImpact {
    /// Style kind.
    pub style: StyleInvalidationKindLite,

    /// Layout kind.
    pub layout: LayoutInvalidationKind,

    /// Paint kind.
    pub paint: PaintInvalidationKind,

    /// Rebuild hint.
    pub rebuild: RebuildHint,
}

impl InvalidationImpact {
    /// No-op impact.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            style: StyleInvalidationKindLite::None,
            layout: LayoutInvalidationKind::None,
            paint: PaintInvalidationKind::None,
            rebuild: RebuildHint::None,
        }
    }

    /// Paint only.
    #[must_use]
    pub const fn paint() -> Self {
        Self {
            style: StyleInvalidationKindLite::None,
            layout: LayoutInvalidationKind::None,
            paint: PaintInvalidationKind::Node,
            rebuild: RebuildHint::PaintPlan,
        }
    }

    /// Style and paint.
    #[must_use]
    pub const fn style_paint() -> Self {
        Self {
            style: StyleInvalidationKindLite::Inline,
            layout: LayoutInvalidationKind::None,
            paint: PaintInvalidationKind::Node,
            rebuild: RebuildHint::PaintPlan,
        }
    }

    /// Layout and paint.
    #[must_use]
    pub const fn layout_paint() -> Self {
        Self {
            style: StyleInvalidationKindLite::Inline,
            layout: LayoutInvalidationKind::Node,
            paint: PaintInvalidationKind::Subtree,
            rebuild: RebuildHint::LayoutTree,
        }
    }

    /// Full document pipeline.
    #[must_use]
    pub const fn full_pipeline() -> Self {
        Self {
            style: StyleInvalidationKindLite::Cascade,
            layout: LayoutInvalidationKind::Viewport,
            paint: PaintInvalidationKind::Viewport,
            rebuild: RebuildHint::FullPipeline,
        }
    }

    /// Merges two impacts.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        Self {
            style: self.style.max(other.style),
            layout: self.layout.max(other.layout),
            paint: self.paint.max(other.paint),
            rebuild: self.rebuild.max(other.rebuild),
        }
    }
}

/// Invalidation reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidationReason {
    /// Reason code.
    pub code: String,

    /// Human-readable detail.
    pub detail: String,
}

impl InvalidationReason {
    /// Creates a reason.
    #[must_use]
    pub fn new(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            detail: detail.into(),
        }
    }
}

/// Invalidation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidationInput {
    /// Source.
    pub source: InvalidationSource,

    /// Priority.
    pub priority: InvalidationPriority,

    /// Scope.
    pub scope: InvalidationScope,

    /// Impact.
    pub impact: InvalidationImpact,

    /// Paint rect, if known.
    pub rect: Option<InvalidationRect>,

    /// Reason.
    pub reason: InvalidationReason,
}

/// Coalesced invalidation batch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InvalidationBatch {
    /// Inputs in arrival order.
    pub inputs: Vec<InvalidationInput>,

    /// Coalesced scope.
    pub scope: Option<InvalidationScope>,

    /// Coalesced impact.
    pub impact: Option<InvalidationImpact>,

    /// Coalesced paint rect.
    pub rect: Option<InvalidationRect>,

    /// Highest priority.
    pub priority: Option<InvalidationPriority>,

    /// Reason codes.
    pub reasons: Vec<String>,
}

impl InvalidationBatch {
    /// Returns true if no inputs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }
}

/// Final invalidation plan consumed by renderer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InvalidationPlan {
    /// Inputs processed into the plan.
    pub input_count: usize,

    /// Nodes needing style recalc.
    pub style_nodes: Vec<DomNodeRef>,

    /// Nodes needing layout.
    pub layout_nodes: Vec<DomNodeRef>,

    /// Nodes needing paint.
    pub paint_nodes: Vec<DomNodeRef>,

    /// Whether entire document must be restyled.
    pub restyle_document: bool,

    /// Whether entire layout tree must be rebuilt.
    pub rebuild_layout_tree: bool,

    /// Whether PaintPlan must be regenerated.
    pub rebuild_paint_plan: bool,

    /// Whether full viewport paint is required.
    pub full_viewport_paint: bool,

    /// Dirty rects.
    pub dirty_rects: Vec<InvalidationRect>,

    /// Highest priority.
    pub priority: InvalidationPriority,

    /// Highest rebuild hint.
    pub rebuild_hint: RebuildHint,

    /// Reason summary.
    pub reasons: Vec<String>,
}

/// Invalidation engine metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidationMetrics {
    /// Inputs submitted.
    pub inputs_submitted: u64,

    /// Inputs coalesced.
    pub inputs_coalesced: u64,

    /// Plans produced.
    pub plans_produced: u64,

    /// Reflow requests consumed.
    pub reflow_requests_consumed: u64,

    /// CSSOM mutations consumed.
    pub cssom_mutations_consumed: u64,

    /// DOM snapshots consumed.
    pub dom_snapshots_consumed: u64,

    /// Full pipeline plans.
    pub full_pipeline_plans: u64,

    /// Layout tree rebuild plans.
    pub layout_tree_rebuild_plans: u64,

    /// PaintPlan rebuild plans.
    pub paint_plan_rebuild_plans: u64,

    /// Viewport paint plans.
    pub viewport_paint_plans: u64,
}

/// Invalidation engine event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InvalidationEvent {
    /// Input submitted.
    InputSubmitted(InvalidationInput),

    /// Batch coalesced.
    BatchCoalesced(InvalidationBatch),

    /// Plan produced.
    PlanProduced(InvalidationPlan),

    /// Queue cleared.
    QueueCleared,
}

/// Invalidation engine config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidationConfig {
    /// Coalesce document-level inputs aggressively.
    pub coalesce_document_scope: bool,

    /// Convert unknown scope into document scope.
    pub unknown_scope_is_document: bool,

    /// Always rebuild PaintPlan after layout work.
    pub paint_after_layout: bool,

    /// Always restyle before layout tree rebuild.
    pub restyle_before_layout: bool,

    /// Maximum retained events.
    pub max_events: usize,
}

impl Default for InvalidationConfig {
    fn default() -> Self {
        Self {
            coalesce_document_scope: true,
            unknown_scope_is_document: true,
            paint_after_layout: true,
            restyle_before_layout: true,
            max_events: 512,
        }
    }
}

/// Invalidation coalescer.
#[derive(Debug, Clone)]
pub struct InvalidationCoalescer {
    config: InvalidationConfig,
}

impl InvalidationCoalescer {
    /// Creates a coalescer.
    #[must_use]
    pub const fn new(config: InvalidationConfig) -> Self {
        Self { config }
    }

    /// Coalesces inputs.
    #[must_use]
    pub fn coalesce(&self, inputs: &[InvalidationInput]) -> InvalidationBatch {
        let mut batch = InvalidationBatch {
            inputs: inputs.to_vec(),
            ..InvalidationBatch::default()
        };

        let mut impact = InvalidationImpact::none();
        let mut priority = InvalidationPriority::Low;
        let mut scope = None;
        let mut rect = None;
        let mut reasons = BTreeSet::new();

        for input in inputs {
            impact = impact.merge(input.impact.clone());
            priority = priority.max(input.priority);
            reasons.insert(input.reason.code.clone());

            scope = Some(
                match (scope.clone(), normalize_scope(&self.config, &input.scope)) {
                    (None, new_scope) => new_scope,
                    (Some(existing), new_scope) => merge_scope(&self.config, existing, new_scope),
                },
            );

            rect = match (rect, input.rect) {
                (None, new_rect) => new_rect,
                (Some(existing), Some(new_rect)) => Some(existing.union(new_rect)),
                (Some(existing), None) => Some(existing),
            };
        }

        batch.impact = Some(impact);
        batch.priority = Some(priority);
        batch.scope = scope;
        batch.rect = rect;
        batch.reasons = reasons.into_iter().collect();
        batch
    }
}

/// Invalidation engine.
#[derive(Debug)]
pub struct InvalidationEngine {
    config: InvalidationConfig,
    queue: VecDeque<InvalidationInput>,
    metrics: InvalidationMetrics,
    events: VecDeque<InvalidationEvent>,
}

impl Default for InvalidationEngine {
    fn default() -> Self {
        Self::new(InvalidationConfig::default())
    }
}

impl InvalidationEngine {
    /// Creates a new engine.
    #[must_use]
    pub fn new(config: InvalidationConfig) -> Self {
        Self {
            config,
            queue: VecDeque::new(),
            metrics: InvalidationMetrics::default(),
            events: VecDeque::new(),
        }
    }

    /// Submits invalidation input.
    pub fn submit(&mut self, input: InvalidationInput) {
        self.metrics.inputs_submitted = self.metrics.inputs_submitted.saturating_add(1);
        self.push_event(InvalidationEvent::InputSubmitted(input.clone()));
        self.queue.push_back(input);
    }

    /// Submits many inputs.
    pub fn submit_many(&mut self, inputs: impl IntoIterator<Item = InvalidationInput>) {
        for input in inputs {
            self.submit(input);
        }
    }

    /// Returns queued input count.
    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.queue.len()
    }

    /// Clears queue.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.push_event(InvalidationEvent::QueueCleared);
    }

    /// Produces a plan and clears queue.
    pub fn flush_plan(&mut self) -> InvalidationPlan {
        let inputs = self.queue.drain(..).collect::<Vec<_>>();
        let coalescer = InvalidationCoalescer::new(self.config.clone());
        let batch = coalescer.coalesce(&inputs);

        self.metrics.inputs_coalesced = self
            .metrics
            .inputs_coalesced
            .saturating_add(batch.inputs.len() as u64);
        self.push_event(InvalidationEvent::BatchCoalesced(batch.clone()));

        let plan = self.plan_from_batch(batch);
        self.metrics.plans_produced = self.metrics.plans_produced.saturating_add(1);

        if plan.rebuild_hint == RebuildHint::FullPipeline {
            self.metrics.full_pipeline_plans = self.metrics.full_pipeline_plans.saturating_add(1);
        }
        if plan.rebuild_layout_tree {
            self.metrics.layout_tree_rebuild_plans =
                self.metrics.layout_tree_rebuild_plans.saturating_add(1);
        }
        if plan.rebuild_paint_plan {
            self.metrics.paint_plan_rebuild_plans =
                self.metrics.paint_plan_rebuild_plans.saturating_add(1);
        }
        if plan.full_viewport_paint {
            self.metrics.viewport_paint_plans = self.metrics.viewport_paint_plans.saturating_add(1);
        }

        self.push_event(InvalidationEvent::PlanProduced(plan.clone()));
        plan
    }

    /// Consumes a script pipeline reflow request.
    pub fn consume_reflow_request(&mut self, request: &ReflowRequest) {
        self.metrics.reflow_requests_consumed =
            self.metrics.reflow_requests_consumed.saturating_add(1);
        self.submit(input_from_reflow_request(request));
    }

    /// Consumes CSSOM mutations.
    pub fn consume_cssom_mutations(&mut self, mutations: &[CssStyleMutation]) {
        self.metrics.cssom_mutations_consumed = self
            .metrics
            .cssom_mutations_consumed
            .saturating_add(mutations.len() as u64);
        self.submit_many(collect_cssom_mutation_invalidations(mutations));
    }

    /// Consumes DOM snapshots conservatively.
    pub fn consume_dom_snapshots(&mut self, snapshots: &[DomNodeSnapshot]) {
        self.metrics.dom_snapshots_consumed = self
            .metrics
            .dom_snapshots_consumed
            .saturating_add(snapshots.len() as u64);
        self.submit_many(collect_dom_snapshot_invalidations(snapshots));
    }

    /// Metrics snapshot.
    #[must_use]
    pub fn metrics(&self) -> InvalidationMetrics {
        self.metrics.clone()
    }

    /// Events snapshot.
    #[must_use]
    pub fn events(&self) -> Vec<InvalidationEvent> {
        self.events.iter().cloned().collect()
    }

    fn plan_from_batch(&self, batch: InvalidationBatch) -> InvalidationPlan {
        if batch.is_empty() {
            return InvalidationPlan {
                priority: InvalidationPriority::Low,
                rebuild_hint: RebuildHint::None,
                ..InvalidationPlan::default()
            };
        }

        let impact = batch
            .impact
            .clone()
            .unwrap_or_else(InvalidationImpact::none);
        let priority = batch.priority.unwrap_or(InvalidationPriority::Normal);
        let scope = batch.scope.clone().unwrap_or(InvalidationScope::Document);

        let mut style_nodes = BTreeSet::new();
        let mut layout_nodes = BTreeSet::new();
        let mut paint_nodes = BTreeSet::new();

        let restyle_document = matches!(
            scope,
            InvalidationScope::Document | InvalidationScope::Viewport
        ) || impact.style >= StyleInvalidationKindLite::Cascade;

        let mut rebuild_layout_tree = matches!(
            impact.layout,
            LayoutInvalidationKind::Tree | LayoutInvalidationKind::Viewport
        ) || matches!(
            impact.rebuild,
            RebuildHint::LayoutTree | RebuildHint::FullPipeline
        ) || matches!(
            scope,
            InvalidationScope::Document | InvalidationScope::Viewport
        ) && impact.layout != LayoutInvalidationKind::None;

        if self.config.restyle_before_layout && rebuild_layout_tree {
            // Whole-tree layout rebuild requires a fresh style pass.
            // Not glamorous, but neither is CSS.
        }

        let mut rebuild_paint_plan = impact.paint >= PaintInvalidationKind::PaintPlan
            || impact.layout != LayoutInvalidationKind::None
            || impact.rebuild >= RebuildHint::PaintPlan;

        if self.config.paint_after_layout && impact.layout != LayoutInvalidationKind::None {
            rebuild_paint_plan = true;
        }

        let full_viewport_paint = matches!(
            scope,
            InvalidationScope::Document | InvalidationScope::Viewport
        ) || impact.paint == PaintInvalidationKind::Viewport
            || impact.rebuild == RebuildHint::FullPipeline;

        match scope {
            InvalidationScope::Node(node) => {
                if impact.style != StyleInvalidationKindLite::None {
                    style_nodes.insert(node);
                }
                if impact.layout != LayoutInvalidationKind::None {
                    layout_nodes.insert(node);
                }
                if impact.paint != PaintInvalidationKind::None {
                    paint_nodes.insert(node);
                }
            }
            InvalidationScope::Subtree(node) => {
                if impact.style != StyleInvalidationKindLite::None {
                    style_nodes.insert(node);
                }
                if impact.layout != LayoutInvalidationKind::None {
                    layout_nodes.insert(node);
                }
                if impact.paint != PaintInvalidationKind::None {
                    paint_nodes.insert(node);
                }
                if impact.layout >= LayoutInvalidationKind::Subtree {
                    rebuild_layout_tree = true;
                }
            }
            InvalidationScope::Document
            | InvalidationScope::Viewport
            | InvalidationScope::Unknown => {
                rebuild_layout_tree |= impact.layout != LayoutInvalidationKind::None;
                rebuild_paint_plan |= impact.paint != PaintInvalidationKind::None;
            }
        }

        let mut dirty_rects = Vec::new();

        if let Some(rect) = batch.rect {
            dirty_rects.push(rect);
        } else if full_viewport_paint {
            dirty_rects.push(InvalidationRect::viewport());
        }

        let rebuild_hint = impact.rebuild.max(if rebuild_layout_tree {
            RebuildHint::LayoutTree
        } else if rebuild_paint_plan {
            RebuildHint::PaintPlan
        } else if restyle_document || !style_nodes.is_empty() {
            RebuildHint::StyleOnly
        } else {
            RebuildHint::None
        });

        InvalidationPlan {
            input_count: batch.inputs.len(),
            style_nodes: style_nodes.into_iter().collect(),
            layout_nodes: layout_nodes.into_iter().collect(),
            paint_nodes: paint_nodes.into_iter().collect(),
            restyle_document,
            rebuild_layout_tree,
            rebuild_paint_plan,
            full_viewport_paint,
            dirty_rects,
            priority,
            rebuild_hint,
            reasons: batch.reasons,
        }
    }

    fn push_event(&mut self, event: InvalidationEvent) {
        self.events.push_back(event);

        while self.events.len() > self.config.max_events {
            self.events.pop_front();
        }
    }
}

fn normalize_scope(config: &InvalidationConfig, scope: &InvalidationScope) -> InvalidationScope {
    if config.unknown_scope_is_document && matches!(scope, InvalidationScope::Unknown) {
        InvalidationScope::Document
    } else {
        scope.clone()
    }
}

fn merge_scope(
    config: &InvalidationConfig,
    existing: InvalidationScope,
    new_scope: InvalidationScope,
) -> InvalidationScope {
    if existing == new_scope {
        return existing;
    }

    match (&existing, &new_scope) {
        (InvalidationScope::Document, _) | (_, InvalidationScope::Document) => {
            if config.coalesce_document_scope {
                InvalidationScope::Document
            } else {
                existing
            }
        }
        (InvalidationScope::Viewport, _) | (_, InvalidationScope::Viewport) => {
            InvalidationScope::Viewport
        }
        (InvalidationScope::Subtree(a), InvalidationScope::Node(b)) if a == b => existing,
        (InvalidationScope::Node(a), InvalidationScope::Subtree(b)) if a == b => new_scope,
        (InvalidationScope::Node(a), InvalidationScope::Node(b)) if a == b => existing,
        _ => InvalidationScope::Document,
    }
}

/// Converts a ScriptPipeline reflow request to invalidation input.
#[must_use]
pub fn input_from_reflow_request(request: &ReflowRequest) -> InvalidationInput {
    let impact = impact_from_dirty_flags(&request.dirty);
    InvalidationInput {
        source: InvalidationSource::ScriptPipeline,
        priority: InvalidationPriority::High,
        scope: InvalidationScope::Document,
        impact,
        rect: None,
        reason: InvalidationReason::new("script-reflow", request.reason.clone()),
    }
}

/// Applies a reflow request to an engine.
pub fn apply_reflow_request_to_invalidation_engine(
    engine: &mut InvalidationEngine,
    request: &ReflowRequest,
) {
    engine.consume_reflow_request(request);
}

fn impact_from_dirty_flags(flags: &DirtyFlags) -> InvalidationImpact {
    let mut impact = InvalidationImpact::none();

    if flags.contains(DirtyFlag::Dom) {
        impact = impact.merge(InvalidationImpact {
            style: StyleInvalidationKindLite::Cascade,
            layout: LayoutInvalidationKind::Tree,
            paint: PaintInvalidationKind::PaintPlan,
            rebuild: RebuildHint::FullPipeline,
        });
    }

    if flags.contains(DirtyFlag::Style) {
        impact = impact.merge(InvalidationImpact {
            style: StyleInvalidationKindLite::Cascade,
            layout: LayoutInvalidationKind::Subtree,
            paint: PaintInvalidationKind::PaintPlan,
            rebuild: RebuildHint::LayoutTree,
        });
    }

    if flags.contains(DirtyFlag::Layout) {
        impact = impact.merge(InvalidationImpact {
            style: StyleInvalidationKindLite::None,
            layout: LayoutInvalidationKind::Tree,
            paint: PaintInvalidationKind::PaintPlan,
            rebuild: RebuildHint::LayoutTree,
        });
    }

    if flags.contains(DirtyFlag::Paint) {
        impact = impact.merge(InvalidationImpact {
            style: StyleInvalidationKindLite::None,
            layout: LayoutInvalidationKind::None,
            paint: PaintInvalidationKind::PaintPlan,
            rebuild: RebuildHint::PaintPlan,
        });
    }

    if flags.contains(DirtyFlag::Lifecycle) {
        impact = impact.merge(InvalidationImpact::paint());
    }

    impact
}

/// Converts CSSOM mutations into invalidation inputs.
#[must_use]
pub fn collect_cssom_mutation_invalidations(
    mutations: &[CssStyleMutation],
) -> Vec<InvalidationInput> {
    mutations
        .iter()
        .map(|mutation| {
            let impact = match mutation.invalidation {
                StyleInvalidationKind::Paint => InvalidationImpact::style_paint(),
                StyleInvalidationKind::Layout => InvalidationImpact::layout_paint(),
                StyleInvalidationKind::StyleRecalc => InvalidationImpact {
                    style: StyleInvalidationKindLite::SelectorMatch,
                    layout: LayoutInvalidationKind::Subtree,
                    paint: PaintInvalidationKind::PaintPlan,
                    rebuild: RebuildHint::LayoutTree,
                },
            };

            InvalidationInput {
                source: InvalidationSource::Cssom,
                priority: if mutation.invalidation == StyleInvalidationKind::Layout {
                    InvalidationPriority::High
                } else {
                    InvalidationPriority::Normal
                },
                scope: mutation
                    .node
                    .map_or(InvalidationScope::Document, InvalidationScope::Node),
                impact,
                rect: None,
                reason: InvalidationReason::new(
                    "cssom-mutation",
                    format!("{} changed", mutation.property),
                ),
            }
        })
        .collect()
}

/// Converts DOM snapshots into conservative invalidation inputs.
#[must_use]
pub fn collect_dom_snapshot_invalidations(snapshots: &[DomNodeSnapshot]) -> Vec<InvalidationInput> {
    snapshots
        .iter()
        .filter_map(|snapshot| {
            if snapshot.node_type == DomNodeType::Text {
                return Some(InvalidationInput {
                    source: InvalidationSource::Dom,
                    priority: InvalidationPriority::Normal,
                    scope: snapshot.parent.map_or(
                        InvalidationScope::Node(snapshot.id),
                        InvalidationScope::Subtree,
                    ),
                    impact: InvalidationImpact {
                        style: StyleInvalidationKindLite::None,
                        layout: LayoutInvalidationKind::IntrinsicSize,
                        paint: PaintInvalidationKind::Subtree,
                        rebuild: RebuildHint::LayoutTree,
                    },
                    rect: None,
                    reason: InvalidationReason::new("dom-text", "text node changed"),
                });
            }

            if snapshot.tag_name.eq_ignore_ascii_case("script") {
                return None;
            }

            Some(InvalidationInput {
                source: InvalidationSource::Dom,
                priority: InvalidationPriority::Normal,
                scope: InvalidationScope::Subtree(snapshot.id),
                impact: InvalidationImpact {
                    style: StyleInvalidationKindLite::SelectorMatch,
                    layout: LayoutInvalidationKind::Subtree,
                    paint: PaintInvalidationKind::PaintPlan,
                    rebuild: RebuildHint::LayoutTree,
                },
                rect: None,
                reason: InvalidationReason::new(
                    "dom-node",
                    format!("{} node changed", snapshot.tag_name),
                ),
            })
        })
        .collect()
}

/// ScriptPipelineHooks implementation that feeds an invalidation engine.
#[derive(Clone)]
pub struct ResearchInvalidationHooks {
    engine: SharedInvalidationEngine,
    inner: Rc<ResearchInvalidationHooksInner>,
}

#[derive(Debug, Default)]
struct ResearchInvalidationHooksInner {
    before: RefCell<Vec<u64>>,
    after: RefCell<Vec<u64>>,
    failures: RefCell<Vec<ScriptExecutionFailure>>,
}

impl fmt::Debug for ResearchInvalidationHooks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResearchInvalidationHooks")
            .field("before", &self.inner.before.borrow())
            .field("after", &self.inner.after.borrow())
            .field("failures", &self.inner.failures.borrow())
            .finish()
    }
}

impl ResearchInvalidationHooks {
    /// Creates hooks backed by an invalidation engine.
    #[must_use]
    pub fn new(engine: SharedInvalidationEngine) -> Self {
        Self {
            engine,
            inner: Rc::new(ResearchInvalidationHooksInner::default()),
        }
    }

    /// Engine.
    #[must_use]
    pub fn engine(&self) -> SharedInvalidationEngine {
        self.engine.clone()
    }

    /// Before-script ids.
    #[must_use]
    pub fn before_script_ids(&self) -> Vec<u64> {
        self.inner.before.borrow().clone()
    }

    /// After-script ids.
    #[must_use]
    pub fn after_script_ids(&self) -> Vec<u64> {
        self.inner.after.borrow().clone()
    }

    /// Failures.
    #[must_use]
    pub fn failures(&self) -> Vec<ScriptExecutionFailure> {
        self.inner.failures.borrow().clone()
    }
}

impl ScriptPipelineHooks for ResearchInvalidationHooks {
    fn before_script(&self, descriptor: &ScriptDescriptor) {
        self.inner.before.borrow_mut().push(descriptor.id);
    }

    fn after_script(&self, descriptor: &ScriptDescriptor) {
        self.inner.after.borrow_mut().push(descriptor.id);
    }

    fn script_failed(&self, descriptor: &ScriptDescriptor, error: &crate::JsRuntimeError) {
        self.inner
            .failures
            .borrow_mut()
            .push(ScriptExecutionFailure {
                script_id: descriptor.id,
                label: descriptor.label.clone(),
                error: error.to_string(),
            });
    }

    fn request_reflow(&self, request: &ReflowRequest) {
        self.engine.borrow_mut().consume_reflow_request(request);
    }

    fn dom_content_loaded(&self) {
        let mut dirty = DirtyFlags::default();
        dirty.insert(DirtyFlag::Lifecycle);
        self.engine
            .borrow_mut()
            .consume_reflow_request(&ReflowRequest {
                script_id: None,
                label: "DOMContentLoaded".to_owned(),
                dirty,
                reason: "DOMContentLoaded".to_owned(),
            });
    }

    fn load(&self) {
        let mut dirty = DirtyFlags::default();
        dirty.insert(DirtyFlag::Lifecycle);
        self.engine
            .borrow_mut()
            .consume_reflow_request(&ReflowRequest {
                script_id: None,
                label: "load".to_owned(),
                dirty,
                reason: "load".to_owned(),
            });
    }
}
