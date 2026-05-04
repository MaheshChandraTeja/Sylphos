#![allow(clippy::expect_used)]

use crate::{
    build_accessibility_tree, focus_target_control, move_accessibility_focus, AccessibleRole,
    FocusNavigationDirection, FormBlock, FormControl, FormControlKind, FormMethod,
    KeyboardFocusTarget, RenderBlock, RenderDocument,
};

fn document_with_focusables() -> RenderDocument {
    let mut doc = RenderDocument::new();
    doc.title = Some("A11y test".to_owned());
    doc.blocks.push(RenderBlock::Heading {
        level: 1,
        text: "Search page".to_owned(),
    });
    doc.blocks.push(RenderBlock::Link {
        text: "Repository".to_owned(),
        href: Some("/repo".to_owned()),
    });
    doc.blocks.push(RenderBlock::Form(FormBlock {
        id: 10,
        action: Some("/search".to_owned()),
        method: FormMethod::Get,
        controls: vec![
            FormControl {
                id: 11,
                kind: FormControlKind::Search,
                name: Some("q".to_owned()),
                value: String::new(),
                placeholder: Some("Search".to_owned()),
                label: Some("Search query".to_owned()),
                disabled: false,
                focused: false,
            },
            FormControl {
                id: 12,
                kind: FormControlKind::Submit,
                name: Some("submit".to_owned()),
                value: "Go".to_owned(),
                placeholder: None,
                label: Some("Go".to_owned()),
                disabled: false,
                focused: false,
            },
        ],
    }));
    doc
}

#[test]
fn builds_accessibility_tree_with_tab_order() {
    let doc = document_with_focusables();
    let tree = build_accessibility_tree(&doc, 1024.0, 720.0);

    assert_eq!(tree.root.0, 1);
    assert!(tree.metrics.nodes >= 4);
    assert!(tree.metrics.headings >= 1);
    assert!(tree.metrics.links >= 1);
    assert!(tree.metrics.form_controls >= 2);

    let stops = tree.tab_order();
    assert_eq!(stops.len(), 3);
    assert!(matches!(stops[0].target, KeyboardFocusTarget::Link { .. }));
    assert!(matches!(
        stops[1].target,
        KeyboardFocusTarget::FormControl { control_id: 11, .. }
    ));
}

#[test]
fn forward_focus_moves_to_first_tab_stop() {
    let mut doc = document_with_focusables();
    let result = move_accessibility_focus(
        &mut doc,
        1024.0,
        720.0,
        None,
        FocusNavigationDirection::Forward,
    );

    assert!(matches!(
        result.current,
        Some(KeyboardFocusTarget::Link { .. })
    ));
    assert!(result.tree.tab_order().len() >= 3);
}

#[test]
fn focus_can_reach_form_control_and_mutate_document() {
    let mut doc = document_with_focusables();
    let current = Some(KeyboardFocusTarget::Link {
        href: "/repo".to_owned(),
        text: "Repository".to_owned(),
    });

    let result = move_accessibility_focus(
        &mut doc,
        1024.0,
        720.0,
        current,
        FocusNavigationDirection::Forward,
    );

    assert!(matches!(
        result.current,
        Some(KeyboardFocusTarget::FormControl { control_id: 11, .. })
    ));
    assert!(result.document_mutated);

    let target = result.current.as_ref().expect("target");
    let control = focus_target_control(&doc, target).expect("control");
    assert_eq!(control.id, 11);
    assert!(control.focused);
}

#[test]
fn backward_focus_wraps_to_last_stop() {
    let mut doc = document_with_focusables();
    let result = move_accessibility_focus(
        &mut doc,
        1024.0,
        720.0,
        None,
        FocusNavigationDirection::Backward,
    );

    assert!(matches!(
        result.current,
        Some(KeyboardFocusTarget::FormControl { control_id: 12, .. })
    ));
}

#[test]
fn role_counts_are_reasonable() {
    let doc = document_with_focusables();
    let tree = build_accessibility_tree(&doc, 1024.0, 720.0);

    assert!(tree
        .nodes
        .iter()
        .any(|node| node.role == AccessibleRole::SearchBox && node.name == "Search query"));
    assert!(tree.metrics.compact().contains("focusable="));
}
