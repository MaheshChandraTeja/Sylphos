#![allow(dead_code)]

//! App-side accessibility controller for keyboard focus and diagnostics.
//!
//! This module is intentionally thin: the presentation crate owns the actual
//! accessibility tree, names, roles, and tab order. The app controller keeps the
//! latest focus target so keyboard events can move through links and form
//! controls without inventing a second focus model, because apparently one focus
//! model was not already enough danger for civilization.

use present::{
    build_accessibility_tree, move_accessibility_focus, AccessibilityMetrics, AccessibilityTree,
    FocusNavigationDirection, KeyboardFocusTarget, RenderDocument,
};

/// Action requested by activating the current keyboard focus target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccessibilityActivation {
    /// Nothing should happen.
    None,
    /// Navigate to an href.
    Navigate(String),
    /// Activate a form control.
    FormControl { form_id: u64, control_id: u64 },
}

/// Keyboard accessibility state for the active tab.
#[derive(Debug, Clone, Default)]
pub(crate) struct BrowserAccessibilityController {
    focused_target: Option<KeyboardFocusTarget>,
    last_tree: Option<AccessibilityTree>,
    last_metrics: AccessibilityMetrics,
}

impl BrowserAccessibilityController {
    /// Creates an empty controller.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Clears focus state after navigation or tab switch.
    pub(crate) fn clear(&mut self) {
        self.focused_target = None;
        self.last_tree = None;
        self.last_metrics = AccessibilityMetrics::default();
    }

    /// Rebuilds the accessibility tree without moving focus.
    pub(crate) fn refresh(&mut self, document: &RenderDocument, width: f32, height: f32) {
        let tree = build_accessibility_tree(document, width, height);
        self.last_metrics = tree.metrics;
        self.last_tree = Some(tree);
    }

    /// Moves keyboard focus forward/backward through tab order.
    pub(crate) fn move_focus(
        &mut self,
        document: &mut RenderDocument,
        width: f32,
        height: f32,
        reverse: bool,
    ) -> Option<KeyboardFocusTarget> {
        let direction = if reverse {
            FocusNavigationDirection::Backward
        } else {
            FocusNavigationDirection::Forward
        };

        let result = move_accessibility_focus(
            document,
            width,
            height,
            self.focused_target.clone(),
            direction,
        );

        self.focused_target.clone_from(&result.current);
        self.last_metrics = result.tree.metrics;
        self.last_tree = Some(result.tree);
        result.current
    }

    /// Returns the currently focused target.
    #[must_use]
    pub(crate) fn focused_target(&self) -> Option<&KeyboardFocusTarget> {
        self.focused_target.as_ref()
    }

    /// Returns latest metrics.
    #[must_use]
    pub(crate) const fn metrics(&self) -> AccessibilityMetrics {
        self.last_metrics
    }

    /// Returns debug tree lines, if a tree has been built.
    #[must_use]
    pub(crate) fn debug_lines(&self) -> Vec<String> {
        self.last_tree
            .as_ref()
            .map_or_else(Vec::new, AccessibilityTree::debug_lines)
    }

    /// Activates the current focus target.
    #[must_use]
    pub(crate) fn activate_current(&self) -> AccessibilityActivation {
        match self.focused_target.as_ref() {
            Some(KeyboardFocusTarget::Link { href, .. }) if !href.trim().is_empty() => {
                AccessibilityActivation::Navigate(href.clone())
            }
            Some(KeyboardFocusTarget::Link { .. }) => AccessibilityActivation::None,
            Some(KeyboardFocusTarget::FormControl {
                form_id,
                control_id,
            }) => AccessibilityActivation::FormControl {
                form_id: *form_id,
                control_id: *control_id,
            },
            Some(KeyboardFocusTarget::Element { .. }) | None => AccessibilityActivation::None,
        }
    }
}
