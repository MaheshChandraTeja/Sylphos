#![doc = "Dirty-state tracking for DOM mutations and future incremental engine work."]

use crate::DomNodeId;

/// Fine-grained dirty flags produced by DOM mutations.
///
/// This intentionally avoids an external bitflags dependency. The presentation
/// crate stays dependency-light and the fields remain explicit when logged or
/// inspected in tests.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirtyFlags {
    /// CSS selector matching or computed style may need to be recomputed.
    pub style: bool,

    /// Layout tree may need to be rebuilt.
    pub layout: bool,

    /// Paint plan may need to be rebuilt.
    pub paint: bool,

    /// Hit regions may need to be rebuilt.
    pub hit_test: bool,

    /// Accessibility/event target metadata may need to be refreshed later.
    pub accessibility: bool,
}

impl DirtyFlags {
    /// No dirty work.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            style: false,
            layout: false,
            paint: false,
            hit_test: false,
            accessibility: false,
        }
    }

    /// Dirty state for text/value mutations.
    #[must_use]
    pub const fn text() -> Self {
        Self {
            style: false,
            layout: true,
            paint: true,
            hit_test: true,
            accessibility: true,
        }
    }

    /// Dirty state for attribute mutations.
    #[must_use]
    pub const fn attributes() -> Self {
        Self {
            style: true,
            layout: true,
            paint: true,
            hit_test: true,
            accessibility: true,
        }
    }

    /// Dirty state for structural mutations.
    #[must_use]
    pub const fn structure() -> Self {
        Self {
            style: true,
            layout: true,
            paint: true,
            hit_test: true,
            accessibility: true,
        }
    }

    /// Dirty state for paint-only changes.
    #[must_use]
    pub const fn paint() -> Self {
        Self {
            style: false,
            layout: false,
            paint: true,
            hit_test: false,
            accessibility: false,
        }
    }

    /// Dirty state for focus-only changes.
    #[must_use]
    pub const fn focus() -> Self {
        Self {
            style: false,
            layout: false,
            paint: true,
            hit_test: false,
            accessibility: true,
        }
    }

    /// Returns true when at least one flag is set.
    #[must_use]
    pub const fn any(self) -> bool {
        self.style || self.layout || self.paint || self.hit_test || self.accessibility
    }

    /// Combines another dirty flag set into this one.
    pub fn merge(&mut self, other: Self) {
        self.style |= other.style;
        self.layout |= other.layout;
        self.paint |= other.paint;
        self.hit_test |= other.hit_test;
        self.accessibility |= other.accessibility;
    }
}

/// Accumulated invalidation data for one mutable DOM runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvalidationSet {
    generation: u64,
    flags: DirtyFlags,
    nodes: Vec<DomNodeId>,
}

impl InvalidationSet {
    /// Creates an invalidation set from aggregate flags without node-level detail.
    #[must_use]
    pub fn new(flags: DirtyFlags) -> Self {
        Self {
            generation: u64::from(flags.any()),
            flags,
            nodes: Vec::new(),
        }
    }

    /// Adds dirty work for a node.
    pub fn mark(&mut self, node_id: DomNodeId, flags: DirtyFlags) {
        if !flags.any() {
            return;
        }

        self.generation = self.generation.wrapping_add(1);
        self.flags.merge(flags);

        if !self.nodes.contains(&node_id) {
            self.nodes.push(node_id);
        }
    }

    /// Clears the invalidation set and returns the old value.
    #[must_use]
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    /// Merges another invalidation set into this one.
    pub fn merge_from(&mut self, other: &Self) {
        if !other.flags.any() {
            return;
        }

        self.generation = self.generation.wrapping_add(other.generation.max(1));
        self.flags.merge(other.flags);

        for node in &other.nodes {
            if !self.nodes.contains(node) {
                self.nodes.push(*node);
            }
        }
    }

    /// Clears all dirty state.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Returns true when dirty work exists.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.flags.any()
    }

    /// Current invalidation generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Combined dirty flags.
    #[must_use]
    pub const fn flags(&self) -> DirtyFlags {
        self.flags
    }

    /// Dirty node ids.
    #[must_use]
    pub fn nodes(&self) -> &[DomNodeId] {
        &self.nodes
    }
}
