use crate::flex_grid::{
    fr, layout_flex_grid_tree, px, track_px, AlignItems, DisplayKind, EdgeSizes,
    FlexContainerStyle, FlexGridLayoutConfig, FlexGridLayoutEngine, GridContainerStyle,
    GridTrackSize, IntrinsicSize, JustifyContent, LayoutBoxKind, LayoutNode, LayoutNodeId,
    LayoutStyle, Length, Size,
};

#[test]
fn block_layout_stacks_children_vertically() {
    let root_style = LayoutStyle {
        width: px(300.0),
        padding: EdgeSizes::all(10.0),
        ..LayoutStyle::default()
    };

    let child_style = LayoutStyle {
        height: px(40.0),
        ..LayoutStyle::default()
    };

    let root = LayoutNode::new(1).with_style(root_style).with_children(vec![
        LayoutNode::new(2).with_style(child_style.clone()),
        LayoutNode::new(3).with_style(child_style),
    ]);

    let result = layout_flex_grid_tree(&root, Size::new(800.0, 600.0));
    let child_a = result.box_for(LayoutNodeId(2)).expect("child 2");
    let child_b = result.box_for(LayoutNodeId(3)).expect("child 3");

    assert_eq!(child_a.border_box.y, 10.0);
    assert_eq!(child_b.border_box.y, 50.0);
    assert_eq!(result.metrics.block_contexts, 3);
}

#[test]
fn flex_row_distributes_free_space_by_grow() {
    let root_style = LayoutStyle {
        display: DisplayKind::Flex,
        width: px(600.0),
        height: px(100.0),
        flex_container: FlexContainerStyle {
            gap: 10.0,
            align_items: AlignItems::Stretch,
            ..FlexContainerStyle::default()
        },
        ..LayoutStyle::default()
    };

    let mut a_style = LayoutStyle {
        width: px(100.0),
        ..LayoutStyle::default()
    };
    a_style.flex_item.grow = 1.0;

    let mut b_style = LayoutStyle {
        width: px(100.0),
        ..LayoutStyle::default()
    };
    b_style.flex_item.grow = 2.0;

    let root = LayoutNode::new(1).with_style(root_style).with_children(vec![
        LayoutNode::new(2).with_style(a_style),
        LayoutNode::new(3).with_style(b_style),
    ]);

    let result = layout_flex_grid_tree(&root, Size::new(800.0, 600.0));
    let a = result.box_for(LayoutNodeId(2)).expect("a");
    let b = result.box_for(LayoutNodeId(3)).expect("b");

    assert_eq!(a.kind, LayoutBoxKind::FlexItem);
    assert!((a.content_box.width - 230.0).abs() < 0.01);
    assert!((b.content_box.width - 360.0).abs() < 0.01);
    assert!((b.border_box.x - 240.0).abs() < 0.01);
    assert_eq!(result.metrics.flex_contexts, 1);
    assert_eq!(result.metrics.flex_items, 2);
}

#[test]
fn flex_justify_center_offsets_items() {
    let root_style = LayoutStyle {
        display: DisplayKind::Flex,
        width: px(500.0),
        height: px(100.0),
        flex_container: FlexContainerStyle {
            justify_content: JustifyContent::Center,
            gap: 0.0,
            ..FlexContainerStyle::default()
        },
        ..LayoutStyle::default()
    };

    let child_style = LayoutStyle {
        width: px(100.0),
        height: px(20.0),
        ..LayoutStyle::default()
    };

    let root = LayoutNode::new(1).with_style(root_style).with_children(vec![
        LayoutNode::new(2).with_style(child_style.clone()),
        LayoutNode::new(3).with_style(child_style),
    ]);

    let result = layout_flex_grid_tree(&root, Size::new(800.0, 600.0));
    let first = result.box_for(LayoutNodeId(2)).expect("first");

    assert!((first.border_box.x - 150.0).abs() < 0.01);
}

#[test]
fn grid_places_children_in_columns_and_rows() {
    let root_style = LayoutStyle {
        display: DisplayKind::Grid,
        width: px(600.0),
        grid_container: GridContainerStyle {
            template_columns: vec![track_px(100.0), fr(1.0), fr(2.0)],
            template_rows: vec![GridTrackSize::Px(40.0), GridTrackSize::Px(50.0)],
            column_gap: 10.0,
            row_gap: 5.0,
            ..GridContainerStyle::default()
        },
        ..LayoutStyle::default()
    };

    let root = LayoutNode::new(1).with_style(root_style).with_children(vec![
        LayoutNode::new(2),
        LayoutNode::new(3),
        LayoutNode::new(4),
        LayoutNode::new(5),
    ]);

    let result = layout_flex_grid_tree(&root, Size::new(800.0, 600.0));
    let first = result.box_for(LayoutNodeId(2)).expect("first");
    let second = result.box_for(LayoutNodeId(3)).expect("second");
    let fourth = result.box_for(LayoutNodeId(5)).expect("fourth");

    assert_eq!(first.grid_area, Some((0, 0, 1, 1)));
    assert_eq!(second.grid_area, Some((0, 1, 1, 1)));
    assert_eq!(fourth.grid_area, Some((1, 0, 1, 1)));
    assert!((second.border_box.x - 110.0).abs() < 0.01);
    assert_eq!(result.metrics.grid_contexts, 1);
    assert_eq!(result.metrics.grid_items, 4);
}

#[test]
fn grid_explicit_placement_and_span_work() {
    let root_style = LayoutStyle {
        display: DisplayKind::Grid,
        width: px(400.0),
        grid_container: GridContainerStyle {
            template_columns: vec![track_px(100.0), track_px(100.0), track_px(100.0)],
            template_rows: vec![track_px(40.0), track_px(40.0)],
            column_gap: 10.0,
            row_gap: 10.0,
            ..GridContainerStyle::default()
        },
        ..LayoutStyle::default()
    };

    let mut child_style = LayoutStyle::default();
    child_style.grid_item.column = Some(2);
    child_style.grid_item.row = Some(1);
    child_style.grid_item.column_span = 2;

    let root = LayoutNode::new(1)
        .with_style(root_style)
        .with_children(vec![LayoutNode::new(2).with_style(child_style)]);

    let result = layout_flex_grid_tree(&root, Size::new(800.0, 600.0));
    let item = result.box_for(LayoutNodeId(2)).expect("item");

    assert_eq!(item.grid_area, Some((0, 1, 1, 2)));
    assert!((item.border_box.x - 110.0).abs() < 0.01);
    assert!((item.content_box.width - 210.0).abs() < 0.01);
}

#[test]
fn display_none_skips_children_from_visible_layout() {
    let hidden_style = LayoutStyle {
        display: DisplayKind::None,
        ..LayoutStyle::default()
    };

    let root = LayoutNode::new(1).with_children(vec![
        LayoutNode::new(2).with_style(hidden_style),
        LayoutNode::new(3).with_style(LayoutStyle {
            height: px(10.0),
            ..LayoutStyle::default()
        }),
    ]);

    let result = layout_flex_grid_tree(&root, Size::new(100.0, 100.0));

    assert!(result.box_for(LayoutNodeId(2)).is_none());
    assert!(result.box_for(LayoutNodeId(3)).is_some());
}

#[test]
fn intrinsic_size_used_for_auto_flex_basis() {
    let root_style = LayoutStyle {
        display: DisplayKind::Flex,
        width: px(500.0),
        height: px(100.0),
        ..LayoutStyle::default()
    };

    let child = LayoutNode::new(2).with_intrinsic(IntrinsicSize {
        min_content_width: 80.0,
        max_content_width: 140.0,
        preferred_height: 20.0,
    });

    let root = LayoutNode::new(1)
        .with_style(root_style)
        .with_children(vec![child]);

    let result = layout_flex_grid_tree(&root, Size::new(800.0, 600.0));
    let child_box = result.box_for(LayoutNodeId(2)).expect("child");

    assert!((child_box.content_box.width - 140.0).abs() < 0.01);
    assert!((child_box.content_box.height - 100.0).abs() < 0.01);
}

#[test]
fn max_depth_prevents_unbounded_layout_recursion() {
    let deep = LayoutNode::new(1).with_children(vec![LayoutNode::new(2).with_children(vec![
        LayoutNode::new(3).with_children(vec![LayoutNode::new(4)]),
    ])]);

    let config = FlexGridLayoutConfig {
        max_depth: 1,
        ..FlexGridLayoutConfig::default()
    };

    let result = FlexGridLayoutEngine::new(config).layout(&deep, Size::new(100.0, 100.0));

    assert!(result.metrics.hidden_nodes >= 1);
}

#[test]
fn percentage_width_resolves_against_container() {
    let child_style = LayoutStyle {
        width: Length::Percent(0.5),
        height: px(20.0),
        ..LayoutStyle::default()
    };

    let root = LayoutNode::new(1)
        .with_style(LayoutStyle {
            width: px(400.0),
            ..LayoutStyle::default()
        })
        .with_children(vec![LayoutNode::new(2).with_style(child_style)]);

    let result = layout_flex_grid_tree(&root, Size::new(800.0, 600.0));
    let child = result.box_for(LayoutNodeId(2)).expect("child");

    assert!((child.content_box.width - 200.0).abs() < 0.01);
}
