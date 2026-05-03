//! DOM/style to Present Flex/Grid adapter.

use present::flex_grid::{
    AlignItems, DisplayKind, EdgeSizes, FlexContainerStyle, GridContainerStyle, GridTrackSize,
    IntrinsicSize, JustifyContent, LayoutNode, LayoutNodeId, LayoutStyle, Length,
};

/// Minimal style payload from app/computed-style layer.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AppComputedStyleLite {
    pub display: String,
    pub width_px: Option<f32>,
    pub height_px: Option<f32>,
    pub margin: EdgeSizes,
    pub padding: EdgeSizes,
    pub gap_px: Option<f32>,
    pub grid_columns_fr: Vec<f32>,
}

/// Minimal DOM payload from app layer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AppDomLayoutNode {
    pub id: u64,
    pub name: String,
    pub style: AppComputedStyleLite,
    pub intrinsic_width: f32,
    pub intrinsic_height: f32,
    pub children: Vec<AppDomLayoutNode>,
}

/// Converts app DOM payload to Present layout tree.
pub(crate) fn app_dom_to_flex_grid_node(input: &AppDomLayoutNode) -> LayoutNode {
    let style = style_from_app_computed(&input.style);

    LayoutNode {
        id: LayoutNodeId(input.id),
        debug_name: input.name.clone(),
        style,
        intrinsic: IntrinsicSize {
            min_content_width: input.intrinsic_width * 0.5,
            max_content_width: input.intrinsic_width,
            preferred_height: input.intrinsic_height,
        },
        children: input
            .children
            .iter()
            .map(app_dom_to_flex_grid_node)
            .collect(),
    }
}

fn style_from_app_computed(input: &AppComputedStyleLite) -> LayoutStyle {
    let display = match input.display.as_str() {
        "none" => DisplayKind::None,
        "flex" | "inline-flex" => DisplayKind::Flex,
        "grid" | "inline-grid" => DisplayKind::Grid,
        _ => DisplayKind::Block,
    };

    let mut style = LayoutStyle {
        display,
        width: input.width_px.map_or(Length::Auto, Length::Px),
        height: input.height_px.map_or(Length::Auto, Length::Px),
        margin: input.margin,
        padding: input.padding,
        ..LayoutStyle::default()
    };

    if display == DisplayKind::Flex {
        style.flex_container = FlexContainerStyle {
            gap: input.gap_px.unwrap_or(0.0),
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            ..FlexContainerStyle::default()
        };
    }

    if display == DisplayKind::Grid {
        style.grid_container = GridContainerStyle {
            template_columns: if input.grid_columns_fr.is_empty() {
                vec![GridTrackSize::Fr(1.0)]
            } else {
                input
                    .grid_columns_fr
                    .iter()
                    .copied()
                    .map(GridTrackSize::Fr)
                    .collect()
            },
            column_gap: input.gap_px.unwrap_or(0.0),
            row_gap: input.gap_px.unwrap_or(0.0),
            ..GridContainerStyle::default()
        };
    }

    style
}
