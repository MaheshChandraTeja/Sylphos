#![doc = "Computed style tree for render-block level presentation nodes."]

use crate::selector::{compute_node_styles, ComputedNodeStyle, ElementSignature, StyleRuleLite};
use crate::{RenderBlock, RenderDocument};

/// One node in the computed style tree.
#[derive(Debug, Clone, PartialEq)]
pub struct StyledNode {
    /// Index into `RenderDocument::blocks`.
    pub block_index: usize,

    /// Semantic block associated with this styled node.
    pub block: RenderBlock,

    /// Element metadata used for selector matching.
    pub element: ElementSignature,

    /// Selector-computed style overrides for this block.
    pub computed_style: ComputedNodeStyle,
}

/// Block-level computed style tree.
///
/// This is intentionally not a full browser DOM style tree yet. It is the next
/// safe step: every render block receives selector-aware computed style data,
/// preserving enough element metadata for class, id, tag, and descendant rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyledDocument {
    /// Styled nodes in render-block order.
    pub nodes: Vec<StyledNode>,

    /// Number of CSS rules considered during the last compute pass.
    pub rule_count: usize,
}

impl StyledDocument {
    /// Returns the computed style for a render-block index.
    #[must_use]
    pub fn style_for_block(&self, block_index: usize) -> Option<&ComputedNodeStyle> {
        self.nodes
            .iter()
            .find(|node| node.block_index == block_index)
            .map(|node| &node.computed_style)
    }
}

/// Computes a style tree for the current render document.
#[must_use]
pub fn compute_styled_document(
    blocks: &[RenderBlock],
    elements: &[ElementSignature],
    rules: &[StyleRuleLite],
) -> StyledDocument {
    let computed_styles = compute_node_styles(elements, rules);
    let nodes = blocks
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, block)| StyledNode {
            block_index: index,
            block,
            element: elements
                .get(index)
                .cloned()
                .unwrap_or_else(|| ElementSignature::synthetic("div", &[])),
            computed_style: computed_styles.get(index).cloned().unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    StyledDocument {
        nodes,
        rule_count: rules.len(),
    }
}

/// Recomputes the style tree stored on a render document.
pub fn recompute_document_style_tree(document: &mut RenderDocument) {
    document.style_tree = compute_styled_document(
        &document.blocks,
        &document.block_elements,
        &document.style_rules,
    );
}
