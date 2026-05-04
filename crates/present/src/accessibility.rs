#![doc = "Accessibility tree, ARIA-lite semantics, and keyboard focus navigation for Sylphos."]
#![allow(
    clippy::missing_const_for_fn,
    clippy::module_name_repetitions,
    clippy::struct_excessive_bools,
    clippy::too_many_lines
)]

use crate::{
    collect_form_control_hit_regions, collect_link_hit_regions, focus_form_control,
    focused_form_control, form_control_by_id, layout_document, measure_line_height,
    measure_text_width, ElementSignature, FormControl, FormControlKind, LayoutBoxKind, LayoutRect,
    RenderBlock, RenderDocument,
};
use std::collections::BTreeSet;

/// Stable id assigned to one accessibility node for a single tree build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccessibleNodeId(pub u64);

/// Coarse accessible role used by the Sylphos accessibility tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AccessibleRole {
    /// Root document node.
    Document,
    /// Generic section/group.
    Group,
    /// Heading node with a level.
    Heading,
    /// Paragraph text block.
    Paragraph,
    /// Plain static text.
    Text,
    /// Hyperlink.
    Link,
    /// Image.
    Image,
    /// Text input.
    TextBox,
    /// Search input.
    SearchBox,
    /// Password input.
    PasswordTextBox,
    /// Multi-line text input.
    TextArea,
    /// Button or submit control.
    Button,
    /// Form group.
    Form,
    /// List container.
    List,
    /// List item.
    ListItem,
    /// Table.
    Table,
    /// Row.
    Row,
    /// Cell.
    Cell,
    /// Navigation landmark.
    Navigation,
    /// Main landmark.
    Main,
    /// Complementary/asides.
    Complementary,
    /// Banner/header landmark.
    Banner,
    /// Contentinfo/footer landmark.
    ContentInfo,
    /// User-supplied ARIA role not otherwise modeled.
    Custom,
}

impl AccessibleRole {
    /// Returns a stable diagnostics label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Group => "group",
            Self::Heading => "heading",
            Self::Paragraph => "paragraph",
            Self::Text => "text",
            Self::Link => "link",
            Self::Image => "image",
            Self::TextBox => "textbox",
            Self::SearchBox => "searchbox",
            Self::PasswordTextBox => "password",
            Self::TextArea => "textarea",
            Self::Button => "button",
            Self::Form => "form",
            Self::List => "list",
            Self::ListItem => "listitem",
            Self::Table => "table",
            Self::Row => "row",
            Self::Cell => "cell",
            Self::Navigation => "navigation",
            Self::Main => "main",
            Self::Complementary => "complementary",
            Self::Banner => "banner",
            Self::ContentInfo => "contentinfo",
            Self::Custom => "custom",
        }
    }
}

/// Runtime accessibility state exposed to focus navigation and diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessibleState {
    /// Node is keyboard focusable.
    pub focusable: bool,
    /// Node is currently focused in the presentation model.
    pub focused: bool,
    /// Node is disabled.
    pub disabled: bool,
    /// Node is hidden from assistive traversal.
    pub hidden: bool,
    /// Node is editable.
    pub editable: bool,
}

/// Target represented by a keyboard focus stop.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyboardFocusTarget {
    /// Form control target.
    FormControl {
        /// Parent form id.
        form_id: u64,
        /// Control id.
        control_id: u64,
    },
    /// Link target.
    Link {
        /// Link href.
        href: String,
        /// Link text.
        text: String,
    },
    /// Non-control focusable target.
    Element {
        /// Node id from the latest accessibility tree.
        node_id: AccessibleNodeId,
        /// Display name.
        name: String,
    },
}

impl KeyboardFocusTarget {
    /// Returns a compact diagnostics string.
    #[must_use]
    pub fn compact(&self) -> String {
        match self {
            Self::FormControl {
                form_id,
                control_id,
            } => format!("form-control:{form_id}:{control_id}"),
            Self::Link { href, text } => format!("link:{text}->{href}"),
            Self::Element { node_id, name } => format!("element:{}:{name}", node_id.0),
        }
    }
}

/// One keyboard tab stop in document order.
#[derive(Debug, Clone, PartialEq)]
pub struct TabStop {
    /// Accessibility node id.
    pub node_id: AccessibleNodeId,
    /// Role.
    pub role: AccessibleRole,
    /// Accessible name.
    pub name: String,
    /// Page-local rectangle when known.
    pub rect: Option<LayoutRect>,
    /// Sequential document order.
    pub order: usize,
    /// Positive tab index. Sylphos currently defaults to source order.
    pub tab_index: i32,
    /// Focus target.
    pub target: KeyboardFocusTarget,
}

/// One node in the accessibility tree.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibleNode {
    /// Stable id within this tree build.
    pub id: AccessibleNodeId,
    /// Parent node id.
    pub parent: Option<AccessibleNodeId>,
    /// Child node ids.
    pub children: Vec<AccessibleNodeId>,
    /// Accessible role.
    pub role: AccessibleRole,
    /// Raw role attribute, when present and not otherwise mapped.
    pub role_override: Option<String>,
    /// Accessible name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Current value for form controls.
    pub value: Option<String>,
    /// Heading level for heading nodes.
    pub level: Option<u8>,
    /// Href for links.
    pub href: Option<String>,
    /// Form id for form controls.
    pub form_id: Option<u64>,
    /// Control id for form controls.
    pub control_id: Option<u64>,
    /// Page-local bounds.
    pub rect: Option<LayoutRect>,
    /// Runtime state.
    pub state: AccessibleState,
    /// Source order.
    pub source_order: usize,
}

impl AccessibleNode {
    /// Returns true when this node can be reached by keyboard focus traversal.
    #[must_use]
    pub fn is_keyboard_focusable(&self) -> bool {
        self.state.focusable && !self.state.disabled && !self.state.hidden
    }

    fn tab_stop(&self) -> Option<TabStop> {
        if !self.is_keyboard_focusable() {
            return None;
        }

        let target = match (self.form_id, self.control_id, self.href.as_ref()) {
            (Some(form_id), Some(control_id), _) => KeyboardFocusTarget::FormControl {
                form_id,
                control_id,
            },
            (_, _, Some(href)) => KeyboardFocusTarget::Link {
                href: href.clone(),
                text: self.name.clone(),
            },
            _ => KeyboardFocusTarget::Element {
                node_id: self.id,
                name: self.name.clone(),
            },
        };

        Some(TabStop {
            node_id: self.id,
            role: self.role,
            name: self.name.clone(),
            rect: self.rect,
            order: self.source_order,
            tab_index: 0,
            target,
        })
    }
}

/// Accessibility tree generated for a render document and viewport.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityTree {
    /// Root node id.
    pub root: AccessibleNodeId,
    /// Flat node storage in source order.
    pub nodes: Vec<AccessibleNode>,
    /// Build metrics.
    pub metrics: AccessibilityMetrics,
}

impl AccessibilityTree {
    /// Returns a node by id.
    #[must_use]
    pub fn node(&self, id: AccessibleNodeId) -> Option<&AccessibleNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Returns keyboard focusable tab stops in effective tab order.
    #[must_use]
    pub fn tab_order(&self) -> Vec<TabStop> {
        let mut stops = self
            .nodes
            .iter()
            .filter_map(AccessibleNode::tab_stop)
            .collect::<Vec<_>>();

        stops.sort_by(|left, right| {
            positive_tab_rank(left.tab_index)
                .cmp(&positive_tab_rank(right.tab_index))
                .then_with(|| left.order.cmp(&right.order))
        });

        stops
    }

    /// Returns the currently focused node, if any.
    #[must_use]
    pub fn focused_node(&self) -> Option<&AccessibleNode> {
        self.nodes.iter().find(|node| node.state.focused)
    }

    /// Produces a concise tree dump useful in tests and debug overlays.
    #[must_use]
    pub fn debug_lines(&self) -> Vec<String> {
        self.nodes
            .iter()
            .map(|node| {
                let hidden = if node.state.hidden { " hidden" } else { "" };
                let focus = if node.state.focused { " focused" } else { "" };
                format!(
                    "{}:{} `{}`{}{}",
                    node.id.0,
                    node.role.as_str(),
                    node.name,
                    hidden,
                    focus
                )
            })
            .collect()
    }
}

/// Accessibility build metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessibilityMetrics {
    /// Total nodes emitted.
    pub nodes: usize,
    /// Keyboard focusable nodes.
    pub focusable: usize,
    /// Link nodes.
    pub links: usize,
    /// Form control nodes.
    pub form_controls: usize,
    /// Image nodes.
    pub images: usize,
    /// Heading nodes.
    pub headings: usize,
    /// Hidden nodes.
    pub hidden: usize,
    /// Nodes with ARIA-derived names.
    pub aria_named: usize,
}

impl AccessibilityMetrics {
    /// Returns a compact diagnostics string.
    #[must_use]
    pub fn compact(self) -> String {
        format!(
            "nodes={} focusable={} links={} controls={} images={} headings={} hidden={} aria_named={}",
            self.nodes,
            self.focusable,
            self.links,
            self.form_controls,
            self.images,
            self.headings,
            self.hidden,
            self.aria_named
        )
    }
}

/// Build options for accessibility tree generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessibilityBuildOptions {
    /// Include hidden nodes in the tree for diagnostics.
    pub include_hidden: bool,
    /// Include static text nodes.
    pub include_static_text: bool,
}

impl Default for AccessibilityBuildOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            include_static_text: true,
        }
    }
}

/// Focus traversal direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusNavigationDirection {
    /// Move forward, as with Tab.
    Forward,
    /// Move backward, as with Shift+Tab.
    Backward,
}

/// Result of a keyboard focus move.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusMoveResult {
    /// Tree used for the move.
    pub tree: AccessibilityTree,
    /// Previous focused target, if known.
    pub previous: Option<KeyboardFocusTarget>,
    /// Newly focused target.
    pub current: Option<KeyboardFocusTarget>,
    /// Whether the render document changed.
    pub document_mutated: bool,
}

/// Builds the default accessibility tree for a document.
#[must_use]
pub fn build_accessibility_tree(
    document: &RenderDocument,
    width: f32,
    height: f32,
) -> AccessibilityTree {
    build_accessibility_tree_with_options(
        document,
        width,
        height,
        AccessibilityBuildOptions::default(),
    )
}

/// Builds an accessibility tree for a document and viewport.
#[must_use]
pub fn build_accessibility_tree_with_options(
    document: &RenderDocument,
    width: f32,
    height: f32,
    options: AccessibilityBuildOptions,
) -> AccessibilityTree {
    let layout = layout_document(document, width, height);
    let link_regions = collect_link_hit_regions(document, width, height);
    let control_regions = collect_form_control_hit_regions(document, width, height);
    let mut builder = AccessibilityTreeBuilder::new(document.title.clone());

    for (block_index, block) in document.blocks.iter().enumerate() {
        let signature = document.block_elements.get(block_index);
        if is_aria_hidden(signature) && !options.include_hidden {
            builder.metrics.hidden = builder.metrics.hidden.saturating_add(1);
            continue;
        }

        match block {
            RenderBlock::Heading { level, text } => {
                let rect = nth_box_rect(
                    &layout.boxes,
                    |kind| matches!(kind, LayoutBoxKind::Heading { level: candidate } if candidate == level),
                );
                builder.push_node(
                    NodeDraft::from_block(
                        AccessibleRole::Heading,
                        accessible_name(signature, text),
                        rect,
                        block_index,
                        signature,
                    )
                    .level(*level),
                );
            }
            RenderBlock::Paragraph { text } => {
                if options.include_static_text {
                    builder.push_node(NodeDraft::from_block(
                        role_from_signature(signature, AccessibleRole::Paragraph),
                        accessible_name(signature, text),
                        nth_box_rect(&layout.boxes, |kind| {
                            matches!(kind, LayoutBoxKind::Paragraph)
                        }),
                        block_index,
                        signature,
                    ));
                }
            }
            RenderBlock::Generic { tag, text } => {
                if options.include_static_text {
                    builder.push_node(NodeDraft::from_block(
                        role_from_tag_or_signature(tag, signature),
                        accessible_name(signature, text),
                        nth_box_rect(&layout.boxes, |kind| {
                            matches!(kind, LayoutBoxKind::Generic { tag: candidate } if candidate == tag)
                        }),
                        block_index,
                        signature,
                    ));
                }
            }
            RenderBlock::Image { alt, src } => {
                let fallback = alt
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| src.clone())
                    .unwrap_or_else(|| "Image".to_owned());
                builder.push_node(
                    NodeDraft::from_block(
                        role_from_signature(signature, AccessibleRole::Image),
                        accessible_name(signature, &fallback),
                        nth_box_rect(&layout.boxes, |kind| {
                            matches!(kind, LayoutBoxKind::Image { .. })
                        }),
                        block_index,
                        signature,
                    )
                    .description(src.clone()),
                );
            }
            RenderBlock::Link { text, href } => {
                let name = if text.trim().is_empty() {
                    href.clone().unwrap_or_default()
                } else {
                    text.clone()
                };
                let rect = link_regions
                    .iter()
                    .find(|region| href.as_ref().is_some_and(|value| value == &region.href))
                    .map(|region| region.rect);
                builder.push_node(
                    NodeDraft::from_block(
                        role_from_signature(signature, AccessibleRole::Link),
                        accessible_name(signature, &name),
                        rect,
                        block_index,
                        signature,
                    )
                    .href(href.clone())
                    .focusable(href.as_ref().is_some_and(|value| !value.trim().is_empty())),
                );
            }
            RenderBlock::InlineFlow { .. } => {
                let mut emitted_links = BTreeSet::new();
                for region in &link_regions {
                    let key = (region.href.clone(), region.text.clone());
                    if emitted_links.insert(key) {
                        builder.push_node(
                            NodeDraft::from_block(
                                AccessibleRole::Link,
                                accessible_name(signature, &region.text),
                                Some(region.rect),
                                block_index,
                                signature,
                            )
                            .href(Some(region.href.clone()))
                            .focusable(true),
                        );
                    }
                }
            }
            RenderBlock::Form(form) => {
                builder.push_node(NodeDraft::from_block(
                    role_from_signature(signature, AccessibleRole::Form),
                    accessible_name(signature, "Form"),
                    None,
                    block_index,
                    signature,
                ));

                for control in &form.controls {
                    if !control.kind.is_visible() {
                        continue;
                    }
                    let rect = control_regions
                        .iter()
                        .find(|region| region.control_id == control.id)
                        .map(|region| region.rect);
                    builder.push_node(control_node_draft(
                        form.id,
                        control,
                        rect,
                        block_index,
                        signature,
                    ));
                }
            }
        }
    }

    builder.finish()
}

/// Moves keyboard focus through the current accessibility tree.
#[must_use]
pub fn move_accessibility_focus(
    document: &mut RenderDocument,
    width: f32,
    height: f32,
    current: Option<KeyboardFocusTarget>,
    direction: FocusNavigationDirection,
) -> FocusMoveResult {
    let before = focused_target_from_document(document);
    let tree = build_accessibility_tree(document, width, height);
    let stops = tree.tab_order();

    if stops.is_empty() {
        let mutated = focus_form_control(document, None);
        return FocusMoveResult {
            tree,
            previous: before,
            current: None,
            document_mutated: mutated,
        };
    }

    let active = current.or_else(|| before.clone());
    let active_index = active
        .as_ref()
        .and_then(|target| stops.iter().position(|stop| &stop.target == target));

    let next_index = match (direction, active_index) {
        (FocusNavigationDirection::Forward, Some(index)) => (index + 1) % stops.len(),
        (FocusNavigationDirection::Backward, Some(0) | None) => stops.len() - 1,
        (FocusNavigationDirection::Backward, Some(index)) => index - 1,
        (FocusNavigationDirection::Forward, None) => 0,
    };

    let selected = stops[next_index].target.clone();
    let mutated = match selected {
        KeyboardFocusTarget::FormControl { control_id, .. } => {
            focus_form_control(document, Some(control_id))
        }
        _ => focus_form_control(document, None),
    };

    let updated_tree = build_accessibility_tree(document, width, height);

    FocusMoveResult {
        tree: updated_tree,
        previous: before,
        current: Some(selected),
        document_mutated: mutated,
    }
}

/// Returns the currently focused target from form-control state.
#[must_use]
pub fn focused_target_from_document(document: &RenderDocument) -> Option<KeyboardFocusTarget> {
    let control = focused_form_control(document)?;
    let form_id = crate::form_id_for_control(document, control.id)?;
    Some(KeyboardFocusTarget::FormControl {
        form_id,
        control_id: control.id,
    })
}

struct AccessibilityTreeBuilder {
    nodes: Vec<AccessibleNode>,
    metrics: AccessibilityMetrics,
    root: AccessibleNodeId,
    next_id: u64,
}

impl AccessibilityTreeBuilder {
    fn new(title: Option<String>) -> Self {
        let root = AccessibleNodeId(1);
        let name = title.unwrap_or_else(|| "Document".to_owned());
        let root_node = AccessibleNode {
            id: root,
            parent: None,
            children: Vec::new(),
            role: AccessibleRole::Document,
            role_override: None,
            name,
            description: None,
            value: None,
            level: None,
            href: None,
            form_id: None,
            control_id: None,
            rect: None,
            state: AccessibleState::default(),
            source_order: 0,
        };

        Self {
            nodes: vec![root_node],
            metrics: AccessibilityMetrics::default(),
            root,
            next_id: 2,
        }
    }

    fn push_node(&mut self, draft: NodeDraft) {
        if draft.state.hidden {
            self.metrics.hidden = self.metrics.hidden.saturating_add(1);
        }

        let id = AccessibleNodeId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);

        if let Some(root) = self.nodes.first_mut() {
            root.children.push(id);
        }

        let node = AccessibleNode {
            id,
            parent: Some(self.root),
            children: Vec::new(),
            role: draft.role,
            role_override: draft.role_override,
            name: collapse_ws(&draft.name),
            description: draft.description.map(|value| collapse_ws(&value)),
            value: draft.value,
            level: draft.level,
            href: draft.href,
            form_id: draft.form_id,
            control_id: draft.control_id,
            rect: draft.rect,
            state: draft.state,
            source_order: draft.source_order,
        };

        self.record_metrics(&node, draft.aria_named);
        self.nodes.push(node);
    }

    fn record_metrics(&mut self, node: &AccessibleNode, aria_named: bool) {
        self.metrics.nodes = self.metrics.nodes.saturating_add(1);
        if node.is_keyboard_focusable() {
            self.metrics.focusable = self.metrics.focusable.saturating_add(1);
        }
        if node.role == AccessibleRole::Link {
            self.metrics.links = self.metrics.links.saturating_add(1);
        }
        if node.control_id.is_some() {
            self.metrics.form_controls = self.metrics.form_controls.saturating_add(1);
        }
        if node.role == AccessibleRole::Image {
            self.metrics.images = self.metrics.images.saturating_add(1);
        }
        if node.role == AccessibleRole::Heading {
            self.metrics.headings = self.metrics.headings.saturating_add(1);
        }
        if aria_named {
            self.metrics.aria_named = self.metrics.aria_named.saturating_add(1);
        }
    }

    fn finish(mut self) -> AccessibilityTree {
        self.metrics.nodes = self.nodes.len();
        AccessibilityTree {
            root: self.root,
            nodes: self.nodes,
            metrics: self.metrics,
        }
    }
}

#[derive(Debug, Clone)]
struct NodeDraft {
    role: AccessibleRole,
    role_override: Option<String>,
    name: String,
    description: Option<String>,
    value: Option<String>,
    level: Option<u8>,
    href: Option<String>,
    form_id: Option<u64>,
    control_id: Option<u64>,
    rect: Option<LayoutRect>,
    state: AccessibleState,
    source_order: usize,
    aria_named: bool,
}

impl NodeDraft {
    fn from_block(
        fallback_role: AccessibleRole,
        name: AccessibleName,
        rect: Option<LayoutRect>,
        source_order: usize,
        signature: Option<&ElementSignature>,
    ) -> Self {
        let (role, override_role) = role_and_override(signature, fallback_role);
        let hidden = is_aria_hidden(signature);
        Self {
            role,
            role_override: override_role,
            name: name.value,
            description: aria_attr(signature, "aria-description"),
            value: None,
            level: None,
            href: None,
            form_id: None,
            control_id: None,
            rect,
            state: AccessibleState {
                hidden,
                ..AccessibleState::default()
            },
            source_order,
            aria_named: name.from_aria,
        }
    }

    fn level(mut self, level: u8) -> Self {
        self.level = Some(level.clamp(1, 6));
        self
    }

    fn href(mut self, href: Option<String>) -> Self {
        self.href = href;
        self
    }

    fn description(mut self, description: Option<String>) -> Self {
        self.description = self.description.or(description);
        self
    }

    fn focusable(mut self, focusable: bool) -> Self {
        self.state.focusable = focusable;
        self
    }
}

#[derive(Debug, Clone)]
struct AccessibleName {
    value: String,
    from_aria: bool,
}

fn control_node_draft(
    form_id: u64,
    control: &FormControl,
    rect: Option<LayoutRect>,
    source_order: usize,
    signature: Option<&ElementSignature>,
) -> NodeDraft {
    let fallback = control_accessible_name(control);
    let name = accessible_name(signature, &fallback);
    let role = role_from_form_control(control.kind);
    let mut draft = NodeDraft::from_block(role, name, rect, source_order, signature);
    draft.form_id = Some(form_id);
    draft.control_id = Some(control.id);
    draft.value = Some(control.value.clone());
    draft.state.focusable = control.can_focus() || control.kind.is_submit_like();
    draft.state.focused = control.focused;
    draft.state.disabled = control.disabled;
    draft.state.editable = control.kind.is_text_editable();
    draft
}

fn role_from_form_control(kind: FormControlKind) -> AccessibleRole {
    match kind {
        FormControlKind::Search => AccessibleRole::SearchBox,
        FormControlKind::Password => AccessibleRole::PasswordTextBox,
        FormControlKind::TextArea => AccessibleRole::TextArea,
        FormControlKind::Submit | FormControlKind::Button => AccessibleRole::Button,
        FormControlKind::Text | FormControlKind::Hidden => AccessibleRole::TextBox,
    }
}

fn control_accessible_name(control: &FormControl) -> String {
    control
        .label
        .clone()
        .or_else(|| control.name.clone())
        .or_else(|| control.placeholder.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| match control.kind {
            FormControlKind::Search => "Search".to_owned(),
            FormControlKind::Password => "Password".to_owned(),
            FormControlKind::Submit => "Submit".to_owned(),
            FormControlKind::Button => "Button".to_owned(),
            FormControlKind::TextArea => "Text area".to_owned(),
            FormControlKind::Text | FormControlKind::Hidden => "Text field".to_owned(),
        })
}

fn role_from_tag_or_signature(tag: &str, signature: Option<&ElementSignature>) -> AccessibleRole {
    role_from_signature(signature, role_from_tag(tag))
}

fn role_from_signature(
    signature: Option<&ElementSignature>,
    fallback: AccessibleRole,
) -> AccessibleRole {
    role_and_override(signature, fallback).0
}

fn role_and_override(
    signature: Option<&ElementSignature>,
    fallback: AccessibleRole,
) -> (AccessibleRole, Option<String>) {
    let Some(role) = aria_attr(signature, "role") else {
        return (fallback, None);
    };

    let normalized = role.trim().to_ascii_lowercase();
    let mapped = match normalized.as_str() {
        "button" => AccessibleRole::Button,
        "link" => AccessibleRole::Link,
        "heading" => AccessibleRole::Heading,
        "img" | "image" => AccessibleRole::Image,
        "textbox" => AccessibleRole::TextBox,
        "searchbox" => AccessibleRole::SearchBox,
        "form" => AccessibleRole::Form,
        "list" => AccessibleRole::List,
        "listitem" => AccessibleRole::ListItem,
        "table" => AccessibleRole::Table,
        "row" => AccessibleRole::Row,
        "cell" | "gridcell" => AccessibleRole::Cell,
        "navigation" => AccessibleRole::Navigation,
        "main" => AccessibleRole::Main,
        "complementary" => AccessibleRole::Complementary,
        "banner" => AccessibleRole::Banner,
        "contentinfo" => AccessibleRole::ContentInfo,
        "presentation" | "none" => AccessibleRole::Group,
        _ => AccessibleRole::Custom,
    };

    (mapped, Some(normalized))
}

fn role_from_tag(tag: &str) -> AccessibleRole {
    match tag.to_ascii_lowercase().as_str() {
        "nav" => AccessibleRole::Navigation,
        "main" => AccessibleRole::Main,
        "aside" => AccessibleRole::Complementary,
        "header" => AccessibleRole::Banner,
        "footer" => AccessibleRole::ContentInfo,
        "ul" | "ol" => AccessibleRole::List,
        "li" => AccessibleRole::ListItem,
        "table" => AccessibleRole::Table,
        "tr" => AccessibleRole::Row,
        "td" | "th" => AccessibleRole::Cell,
        "form" => AccessibleRole::Form,
        _ => AccessibleRole::Group,
    }
}

fn accessible_name(signature: Option<&ElementSignature>, fallback: &str) -> AccessibleName {
    if let Some(label) = aria_attr(signature, "aria-label") {
        if !label.trim().is_empty() {
            return AccessibleName {
                value: label,
                from_aria: true,
            };
        }
    }

    if let Some(title) = aria_attr(signature, "title") {
        if !title.trim().is_empty() && fallback.trim().is_empty() {
            return AccessibleName {
                value: title,
                from_aria: true,
            };
        }
    }

    AccessibleName {
        value: fallback.to_owned(),
        from_aria: false,
    }
}

fn is_aria_hidden(signature: Option<&ElementSignature>) -> bool {
    aria_attr(signature, "aria-hidden")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

fn aria_attr(signature: Option<&ElementSignature>, name: &str) -> Option<String> {
    signature?.attrs.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn nth_box_rect(
    boxes: &[crate::LayoutBox],
    predicate: impl Fn(&LayoutBoxKind) -> bool,
) -> Option<LayoutRect> {
    boxes
        .iter()
        .find(|layout_box| predicate(&layout_box.kind))
        .map(|layout_box| layout_box.rect)
}

fn positive_tab_rank(index: i32) -> (bool, i32) {
    if index > 0 {
        (false, index)
    } else {
        (true, 0)
    }
}

fn collapse_ws(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns a rough accessible rectangle for a text node.
#[must_use]
pub fn text_accessible_rect(x: f32, y: f32, text: &str, size: f32) -> LayoutRect {
    LayoutRect::new(
        x,
        y,
        measure_text_width(text, size).max(size * 0.5),
        measure_line_height(size),
    )
}

/// Returns a form-control value for a focus target, if it references one.
#[must_use]
pub fn focus_target_control(
    document: &RenderDocument,
    target: &KeyboardFocusTarget,
) -> Option<FormControl> {
    match target {
        KeyboardFocusTarget::FormControl { control_id, .. } => {
            form_control_by_id(document, *control_id)
        }
        KeyboardFocusTarget::Link { .. } | KeyboardFocusTarget::Element { .. } => None,
    }
}

#[cfg(test)]
mod local_tests {
    use super::*;
    use crate::{FormBlock, FormControl, FormMethod, RenderBlock, RenderDocument};

    #[test]
    fn tree_contains_heading_link_and_control() {
        let mut doc = RenderDocument::new();
        doc.blocks.push(RenderBlock::Heading {
            level: 1,
            text: "Hello".to_owned(),
        });
        doc.blocks.push(RenderBlock::Link {
            text: "Docs".to_owned(),
            href: Some("/docs".to_owned()),
        });
        doc.blocks.push(RenderBlock::Form(FormBlock {
            id: 1,
            action: None,
            method: FormMethod::Get,
            controls: vec![FormControl {
                id: 7,
                kind: FormControlKind::Search,
                name: Some("q".to_owned()),
                value: String::new(),
                placeholder: Some("Search".to_owned()),
                label: None,
                disabled: false,
                focused: false,
            }],
        }));

        let tree = build_accessibility_tree(&doc, 800.0, 600.0);
        assert!(tree
            .nodes
            .iter()
            .any(|node| node.role == AccessibleRole::Heading));
        assert!(tree
            .nodes
            .iter()
            .any(|node| node.role == AccessibleRole::Link));
        assert!(tree
            .nodes
            .iter()
            .any(|node| node.role == AccessibleRole::SearchBox));
        assert_eq!(tree.tab_order().len(), 2);
    }
}
