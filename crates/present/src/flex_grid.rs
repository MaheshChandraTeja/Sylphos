#![allow(clippy::too_many_lines)]
#![doc = "Flexbox/Grid Lite layout engine for Sylphos Present."]

use std::collections::BTreeMap;

/// Stable layout node id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayoutNodeId(pub u64);

/// 2D size in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    #[must_use]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0)
    }

    #[must_use]
    pub fn sanitized(self) -> Self {
        Self {
            width: sanitize_dimension(self.width),
            height: sanitize_dimension(self.height),
        }
    }
}

/// 2D point in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Rectangle in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    #[must_use]
    pub const fn size(self) -> Size {
        Size::new(self.width, self.height)
    }

    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    #[must_use]
    pub fn inset(self, edges: EdgeSizes) -> Self {
        Self {
            x: self.x + edges.left,
            y: self.y + edges.top,
            width: (self.width - edges.horizontal()).max(0.0),
            height: (self.height - edges.vertical()).max(0.0),
        }
    }

    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self {
            x: left,
            y: top,
            width: (right - left).max(0.0),
            height: (bottom - top).max(0.0),
        }
    }
}

/// Box edge sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSizes {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Default for EdgeSizes {
    fn default() -> Self {
        Self::zero()
    }
}

impl EdgeSizes {
    #[must_use]
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self { top, right, bottom, left }
    }

    #[must_use]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }

    #[must_use]
    pub const fn all(value: f32) -> Self {
        Self::new(value, value, value, value)
    }

    #[must_use]
    pub fn horizontal(self) -> f32 {
        self.left + self.right
    }

    #[must_use]
    pub fn vertical(self) -> f32 {
        self.top + self.bottom
    }

    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            top: self.top + other.top,
            right: self.right + other.right,
            bottom: self.bottom + other.bottom,
            left: self.left + other.left,
        }
    }
}

/// CSS length-ish value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Auto,
    Px(f32),
    Percent(f32),
    Fr(f32),
    MinContent,
    MaxContent,
}

impl Default for Length {
    fn default() -> Self {
        Self::Auto
    }
}

/// Box sizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}

impl Default for BoxSizing {
    fn default() -> Self {
        Self::ContentBox
    }
}

/// Display type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayKind {
    None,
    Block,
    Flex,
    Grid,
}

impl Default for DisplayKind {
    fn default() -> Self {
        Self::Block
    }
}

/// Flex direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl Default for FlexDirection {
    fn default() -> Self {
        Self::Row
    }
}

/// Flex wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
}

impl Default for FlexWrap {
    fn default() -> Self {
        Self::NoWrap
    }
}

/// Justify-content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JustifyContent {
    FlexStart,
    Center,
    FlexEnd,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

impl Default for JustifyContent {
    fn default() -> Self {
        Self::FlexStart
    }
}

/// Align-items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignItems {
    Stretch,
    FlexStart,
    Center,
    FlexEnd,
}

impl Default for AlignItems {
    fn default() -> Self {
        Self::Stretch
    }
}

/// Flex container style.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexContainerStyle {
    pub direction: FlexDirection,
    pub wrap: FlexWrap,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub gap: f32,
}

impl Default for FlexContainerStyle {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Row,
            wrap: FlexWrap::NoWrap,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            gap: 0.0,
        }
    }
}

/// Flex item style.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexItemStyle {
    pub grow: f32,
    pub shrink: f32,
    pub basis: Length,
    pub align_self: Option<AlignItems>,
    pub order: i32,
}

impl Default for FlexItemStyle {
    fn default() -> Self {
        Self {
            grow: 0.0,
            shrink: 1.0,
            basis: Length::Auto,
            align_self: None,
            order: 0,
        }
    }
}

/// Grid track size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridTrackSize {
    Px(f32),
    Percent(f32),
    Fr(f32),
    Auto,
}

impl Default for GridTrackSize {
    fn default() -> Self {
        Self::Auto
    }
}

/// Grid auto-flow direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridAutoFlow {
    Row,
    Column,
}

impl Default for GridAutoFlow {
    fn default() -> Self {
        Self::Row
    }
}

/// Grid container style.
#[derive(Debug, Clone, PartialEq)]
pub struct GridContainerStyle {
    pub template_columns: Vec<GridTrackSize>,
    pub template_rows: Vec<GridTrackSize>,
    pub auto_rows: GridTrackSize,
    pub auto_columns: GridTrackSize,
    pub column_gap: f32,
    pub row_gap: f32,
    pub auto_flow: GridAutoFlow,
}

impl Default for GridContainerStyle {
    fn default() -> Self {
        Self {
            template_columns: Vec::new(),
            template_rows: Vec::new(),
            auto_rows: GridTrackSize::Auto,
            auto_columns: GridTrackSize::Auto,
            column_gap: 0.0,
            row_gap: 0.0,
            auto_flow: GridAutoFlow::Row,
        }
    }
}

/// Grid item placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridItemStyle {
    pub column: Option<usize>,
    pub row: Option<usize>,
    pub column_span: usize,
    pub row_span: usize,
}

impl Default for GridItemStyle {
    fn default() -> Self {
        Self {
            column: None,
            row: None,
            column_span: 1,
            row_span: 1,
        }
    }
}

/// Layout style.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutStyle {
    pub display: DisplayKind,
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub max_width: Length,
    pub min_height: Length,
    pub max_height: Length,
    pub margin: EdgeSizes,
    pub padding: EdgeSizes,
    pub border: EdgeSizes,
    pub box_sizing: BoxSizing,
    pub flex_container: FlexContainerStyle,
    pub flex_item: FlexItemStyle,
    pub grid_container: GridContainerStyle,
    pub grid_item: GridItemStyle,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            display: DisplayKind::Block,
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            min_height: Length::Auto,
            max_height: Length::Auto,
            margin: EdgeSizes::zero(),
            padding: EdgeSizes::zero(),
            border: EdgeSizes::zero(),
            box_sizing: BoxSizing::ContentBox,
            flex_container: FlexContainerStyle::default(),
            flex_item: FlexItemStyle::default(),
            grid_container: GridContainerStyle::default(),
            grid_item: GridItemStyle::default(),
        }
    }
}

/// Intrinsic content size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntrinsicSize {
    pub min_content_width: f32,
    pub max_content_width: f32,
    pub preferred_height: f32,
}

impl Default for IntrinsicSize {
    fn default() -> Self {
        Self {
            min_content_width: 0.0,
            max_content_width: 0.0,
            preferred_height: 0.0,
        }
    }
}

/// Layout input node.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutNode {
    pub id: LayoutNodeId,
    pub debug_name: String,
    pub style: LayoutStyle,
    pub intrinsic: IntrinsicSize,
    pub children: Vec<LayoutNode>,
}

impl LayoutNode {
    #[must_use]
    pub fn new(id: u64) -> Self {
        Self {
            id: LayoutNodeId(id),
            debug_name: format!("node-{id}"),
            style: LayoutStyle::default(),
            intrinsic: IntrinsicSize::default(),
            children: Vec::new(),
        }
    }

    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.debug_name = name.into();
        self
    }

    #[must_use]
    pub fn with_style(mut self, style: LayoutStyle) -> Self {
        self.style = style;
        self
    }

    #[must_use]
    pub const fn with_intrinsic(mut self, intrinsic: IntrinsicSize) -> Self {
        self.intrinsic = intrinsic;
        self
    }

    #[must_use]
    pub fn with_children(mut self, children: Vec<LayoutNode>) -> Self {
        self.children = children;
        self
    }
}

/// Box kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutBoxKind {
    Block,
    FlexContainer,
    FlexItem,
    GridContainer,
    GridItem,
    Hidden,
}

/// Computed layout box.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBox {
    pub id: LayoutNodeId,
    pub parent: Option<LayoutNodeId>,
    pub debug_name: String,
    pub kind: LayoutBoxKind,
    pub margin_box: Rect,
    pub border_box: Rect,
    pub padding_box: Rect,
    pub content_box: Rect,
    pub children: Vec<LayoutNodeId>,
    pub flex_line: Option<usize>,
    pub grid_area: Option<(usize, usize, usize, usize)>,
}

/// Layout metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutMetrics {
    pub nodes_visited: u64,
    pub boxes_emitted: u64,
    pub block_contexts: u64,
    pub flex_contexts: u64,
    pub grid_contexts: u64,
    pub flex_items: u64,
    pub grid_items: u64,
    pub hidden_nodes: u64,
    pub layout_passes: u64,
}

/// Layout result.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutResult {
    pub viewport: Size,
    pub root: LayoutNodeId,
    pub boxes: Vec<LayoutBox>,
    pub by_id: BTreeMap<LayoutNodeId, usize>,
    pub document_bounds: Rect,
    pub metrics: LayoutMetrics,
}

impl LayoutResult {
    #[must_use]
    pub fn box_for(&self, id: LayoutNodeId) -> Option<&LayoutBox> {
        self.by_id.get(&id).and_then(|index| self.boxes.get(*index))
    }
}

/// Layout engine configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexGridLayoutConfig {
    pub default_auto_row_height: f32,
    pub default_auto_column_width: f32,
    pub allow_flex_shrink_below_min_content: bool,
    pub dense_grid_auto_placement: bool,
    pub max_depth: usize,
}

impl Default for FlexGridLayoutConfig {
    fn default() -> Self {
        Self {
            default_auto_row_height: 40.0,
            default_auto_column_width: 120.0,
            allow_flex_shrink_below_min_content: false,
            dense_grid_auto_placement: false,
            max_depth: 512,
        }
    }
}

/// Flex/Grid layout engine.
#[derive(Debug)]
pub struct FlexGridLayoutEngine {
    config: FlexGridLayoutConfig,
    boxes: Vec<LayoutBox>,
    by_id: BTreeMap<LayoutNodeId, usize>,
    metrics: LayoutMetrics,
}

impl Default for FlexGridLayoutEngine {
    fn default() -> Self {
        Self::new(FlexGridLayoutConfig::default())
    }
}

impl FlexGridLayoutEngine {
    #[must_use]
    pub fn new(config: FlexGridLayoutConfig) -> Self {
        Self {
            config,
            boxes: Vec::new(),
            by_id: BTreeMap::new(),
            metrics: LayoutMetrics::default(),
        }
    }

    #[must_use]
    pub fn layout(mut self, root: &LayoutNode, viewport: Size) -> LayoutResult {
        self.metrics.layout_passes = self.metrics.layout_passes.saturating_add(1);
        let viewport = viewport.sanitized();
        let containing = Rect::new(0.0, 0.0, viewport.width, viewport.height);
        let root_box = self.layout_node(root, None, containing, None, 0, ChildRole::Normal);

        let document_bounds = self
            .boxes
            .iter()
            .fold(root_box.margin_box, |acc, item| acc.union(item.margin_box));

        LayoutResult {
            viewport,
            root: root.id,
            boxes: self.boxes,
            by_id: self.by_id,
            document_bounds,
            metrics: self.metrics,
        }
    }

    fn layout_node(
        &mut self,
        node: &LayoutNode,
        parent: Option<LayoutNodeId>,
        containing: Rect,
        forced_size: Option<Size>,
        depth: usize,
        role: ChildRole,
    ) -> LayoutBox {
        self.metrics.nodes_visited = self.metrics.nodes_visited.saturating_add(1);

        if depth > self.config.max_depth || node.style.display == DisplayKind::None {
            self.metrics.hidden_nodes = self.metrics.hidden_nodes.saturating_add(1);
            return LayoutBox {
                id: node.id,
                parent,
                debug_name: node.debug_name.clone(),
                kind: LayoutBoxKind::Hidden,
                margin_box: Rect::new(containing.x, containing.y, 0.0, 0.0),
                border_box: Rect::new(containing.x, containing.y, 0.0, 0.0),
                padding_box: Rect::new(containing.x, containing.y, 0.0, 0.0),
                content_box: Rect::new(containing.x, containing.y, 0.0, 0.0),
                children: Vec::new(),
                flex_line: None,
                grid_area: None,
            };
        }

        let outer_edges = node.style.margin.plus(node.style.border).plus(node.style.padding);
        let available_content_width = (containing.width - outer_edges.horizontal()).max(0.0);

        let mut content_width = forced_size
            .map(|size| size.width)
            .or_else(|| resolve_length(node.style.width, containing.width, node.intrinsic.max_content_width))
            .unwrap_or(available_content_width);

        let mut content_height = forced_size
            .map(|size| size.height)
            .or_else(|| resolve_length(node.style.height, containing.height, node.intrinsic.preferred_height))
            .unwrap_or(0.0);

        content_width = apply_min_max(
            content_width,
            node.style.min_width,
            node.style.max_width,
            containing.width,
            node.intrinsic.min_content_width,
            node.intrinsic.max_content_width,
        );

        content_height = apply_min_max(
            content_height,
            node.style.min_height,
            node.style.max_height,
            containing.height,
            node.intrinsic.preferred_height,
            node.intrinsic.preferred_height,
        );

        if node.style.box_sizing == BoxSizing::BorderBox {
            content_width = (content_width - node.style.padding.horizontal() - node.style.border.horizontal()).max(0.0);
            content_height = (content_height - node.style.padding.vertical() - node.style.border.vertical()).max(0.0);
        }

        let margin_box_x = containing.x + node.style.margin.left;
        let margin_box_y = containing.y + node.style.margin.top;

        let border_box = Rect::new(
            margin_box_x,
            margin_box_y,
            content_width + node.style.padding.horizontal() + node.style.border.horizontal(),
            content_height + node.style.padding.vertical() + node.style.border.vertical(),
        );
        let padding_box = border_box.inset(node.style.border);
        let mut content_box = padding_box.inset(node.style.padding);

        let kind = match (node.style.display, role) {
            (DisplayKind::Flex, _) => LayoutBoxKind::FlexContainer,
            (DisplayKind::Grid, _) => LayoutBoxKind::GridContainer,
            (_, ChildRole::FlexItem { .. }) => LayoutBoxKind::FlexItem,
            (_, ChildRole::GridItem { .. }) => LayoutBoxKind::GridItem,
            _ => LayoutBoxKind::Block,
        };

        let mut child_ids = Vec::new();
        let mut flex_line = None;
        let mut grid_area = None;

        match role {
            ChildRole::FlexItem { line } => flex_line = Some(line),
            ChildRole::GridItem { row, column, row_span, column_span } => {
                grid_area = Some((row, column, row_span, column_span));
            }
            ChildRole::Normal => {}
        }

        let children_bounds = match node.style.display {
            DisplayKind::None => None,
            DisplayKind::Block => {
                self.metrics.block_contexts = self.metrics.block_contexts.saturating_add(1);
                self.layout_block_children(node, content_box, depth + 1, &mut child_ids)
            }
            DisplayKind::Flex => {
                self.metrics.flex_contexts = self.metrics.flex_contexts.saturating_add(1);
                self.layout_flex_children(node, content_box, depth + 1, &mut child_ids)
            }
            DisplayKind::Grid => {
                self.metrics.grid_contexts = self.metrics.grid_contexts.saturating_add(1);
                self.layout_grid_children(node, content_box, depth + 1, &mut child_ids)
            }
        };

        if forced_size.is_none() && matches!(node.style.height, Length::Auto) {
            if let Some(bounds) = children_bounds {
                content_height = (bounds.bottom() - content_box.y).max(node.intrinsic.preferred_height);
            } else {
                content_height = node.intrinsic.preferred_height;
            }

            content_height = apply_min_max(
                content_height,
                node.style.min_height,
                node.style.max_height,
                containing.height,
                node.intrinsic.preferred_height,
                node.intrinsic.preferred_height,
            );

            content_box.height = content_height;
        }

        let final_padding_box = Rect::new(
            padding_box.x,
            padding_box.y,
            content_box.width + node.style.padding.horizontal(),
            content_box.height + node.style.padding.vertical(),
        );
        let final_border_box = Rect::new(
            border_box.x,
            border_box.y,
            final_padding_box.width + node.style.border.horizontal(),
            final_padding_box.height + node.style.border.vertical(),
        );
        let final_margin_box = Rect::new(
            containing.x,
            containing.y,
            final_border_box.width + node.style.margin.horizontal(),
            final_border_box.height + node.style.margin.vertical(),
        );

        self.emit_box(LayoutBox {
            id: node.id,
            parent,
            debug_name: node.debug_name.clone(),
            kind,
            margin_box: final_margin_box,
            border_box: final_border_box,
            padding_box: final_padding_box,
            content_box,
            children: child_ids,
            flex_line,
            grid_area,
        })
    }

    fn layout_block_children(
        &mut self,
        node: &LayoutNode,
        content_box: Rect,
        depth: usize,
        child_ids: &mut Vec<LayoutNodeId>,
    ) -> Option<Rect> {
        let mut cursor_y = content_box.y;
        let mut bounds = None;

        for child in &node.children {
            if child.style.display == DisplayKind::None {
                continue;
            }

            let child_containing = Rect::new(content_box.x, cursor_y, content_box.width, content_box.height);
            let child_box = self.layout_node(child, Some(node.id), child_containing, None, depth, ChildRole::Normal);
            cursor_y = child_box.margin_box.bottom();
            child_ids.push(child.id);
            bounds = Some(bounds.map_or(child_box.margin_box, |existing: Rect| existing.union(child_box.margin_box)));
        }

        bounds
    }

    fn layout_flex_children(
        &mut self,
        node: &LayoutNode,
        content_box: Rect,
        depth: usize,
        child_ids: &mut Vec<LayoutNodeId>,
    ) -> Option<Rect> {
        let is_row = matches!(
            node.style.flex_container.direction,
            FlexDirection::Row | FlexDirection::RowReverse
        );
        let reverse = matches!(
            node.style.flex_container.direction,
            FlexDirection::RowReverse | FlexDirection::ColumnReverse
        );

        let mut items = node
            .children
            .iter()
            .filter(|child| child.style.display != DisplayKind::None)
            .collect::<Vec<_>>();

        items.sort_by_key(|child| child.style.flex_item.order);

        if reverse {
            items.reverse();
        }

        let available_main = if is_row { content_box.width } else { content_box.height };
        let available_cross = if is_row { content_box.height } else { content_box.width };
        let gap = node.style.flex_container.gap.max(0.0);
        let item_count = items.len();

        if item_count == 0 {
            return None;
        }

        let bases = items
            .iter()
            .map(|child| flex_basis(child, available_main, is_row))
            .collect::<Vec<_>>();

        let total_basis = bases.iter().copied().sum::<f32>();
        let total_gap = gap * item_count.saturating_sub(1) as f32;
        let free_space = available_main - total_basis - total_gap;

        let total_grow = items
            .iter()
            .map(|child| child.style.flex_item.grow.max(0.0))
            .sum::<f32>();

        let total_shrink = items
            .iter()
            .enumerate()
            .map(|(index, child)| child.style.flex_item.shrink.max(0.0) * bases[index])
            .sum::<f32>();

        let mut final_main_sizes = Vec::with_capacity(item_count);

        for (index, child) in items.iter().enumerate() {
            let mut main = bases[index];

            if free_space > 0.0 && total_grow > 0.0 {
                main += free_space * (child.style.flex_item.grow.max(0.0) / total_grow);
            } else if free_space < 0.0 && total_shrink > 0.0 {
                let shrink_factor = child.style.flex_item.shrink.max(0.0) * bases[index] / total_shrink;
                main += free_space * shrink_factor;
            }

            if !self.config.allow_flex_shrink_below_min_content {
                let min_content = if is_row {
                    child.intrinsic.min_content_width
                } else {
                    child.intrinsic.preferred_height
                };
                main = main.max(min_content);
            }

            final_main_sizes.push(main.max(0.0));
        }

        let used_main = final_main_sizes.iter().copied().sum::<f32>() + total_gap;
        let remaining = (available_main - used_main).max(0.0);
        let (start_offset, effective_gap) =
            justify_offsets(node.style.flex_container.justify_content, remaining, gap, item_count);

        let mut main_cursor = start_offset;
        let mut bounds = None;

        for (index, child) in items.iter().enumerate() {
            let main_size = final_main_sizes[index];
            let align = child
                .style
                .flex_item
                .align_self
                .unwrap_or(node.style.flex_container.align_items);

            let cross_size = flex_cross_size(child, available_cross, align, is_row);
            let cross_offset = align_offset(align, available_cross, cross_size);

            let child_rect = if is_row {
                Rect::new(
                    content_box.x + main_cursor,
                    content_box.y + cross_offset,
                    main_size,
                    cross_size,
                )
            } else {
                Rect::new(
                    content_box.x + cross_offset,
                    content_box.y + main_cursor,
                    cross_size,
                    main_size,
                )
            };

            self.metrics.flex_items = self.metrics.flex_items.saturating_add(1);
            let child_box = self.layout_node(
                child,
                Some(node.id),
                child_rect,
                Some(child_rect.size()),
                depth,
                ChildRole::FlexItem { line: 0 },
            );

            child_ids.push(child.id);
            main_cursor += main_size + effective_gap;
            bounds = Some(bounds.map_or(child_box.margin_box, |existing: Rect| existing.union(child_box.margin_box)));
        }

        bounds
    }

    fn layout_grid_children(
        &mut self,
        node: &LayoutNode,
        content_box: Rect,
        depth: usize,
        child_ids: &mut Vec<LayoutNodeId>,
    ) -> Option<Rect> {
        let visible_children = node
            .children
            .iter()
            .filter(|child| child.style.display != DisplayKind::None)
            .collect::<Vec<_>>();

        if visible_children.is_empty() {
            return None;
        }

        let column_count = node.style.grid_container.template_columns.len().max(1);
        let explicit_row_count = node.style.grid_container.template_rows.len();
        let minimum_rows = div_ceil(visible_children.len(), column_count).max(1);
        let row_count = explicit_row_count.max(minimum_rows);

        let columns = resolve_grid_tracks(
            &node.style.grid_container.template_columns,
            column_count,
            node.style.grid_container.auto_columns,
            content_box.width,
            node.style.grid_container.column_gap,
            self.config.default_auto_column_width,
        );

        let rows = resolve_grid_tracks(
            &node.style.grid_container.template_rows,
            row_count,
            node.style.grid_container.auto_rows,
            content_box.height,
            node.style.grid_container.row_gap,
            self.config.default_auto_row_height,
        );

        let mut occupied = BTreeMap::<(usize, usize), LayoutNodeId>::new();
        let mut auto_cursor = 0usize;
        let mut bounds = None;

        for child in visible_children {
            let row_span = child.style.grid_item.row_span.max(1);
            let column_span = child.style.grid_item.column_span.max(1);

            let (row, column) = if let (Some(row), Some(column)) = (child.style.grid_item.row, child.style.grid_item.column) {
                (row.saturating_sub(1), column.saturating_sub(1))
            } else {
                let placement = next_grid_position(
                    &occupied,
                    auto_cursor,
                    row_count,
                    column_count,
                    row_span,
                    column_span,
                    node.style.grid_container.auto_flow,
                );
                auto_cursor = placement.0 * column_count + placement.1 + 1;
                placement
            };

            for y in row..row.saturating_add(row_span) {
                for x in column..column.saturating_add(column_span) {
                    occupied.insert((y, x), child.id);
                }
            }

            let x = content_box.x + track_offset(&columns, column, node.style.grid_container.column_gap);
            let y = content_box.y + track_offset(&rows, row, node.style.grid_container.row_gap);
            let width = span_size(&columns, column, column_span, node.style.grid_container.column_gap);
            let height = span_size(&rows, row, row_span, node.style.grid_container.row_gap);

            let child_rect = Rect::new(x, y, width, height);
            self.metrics.grid_items = self.metrics.grid_items.saturating_add(1);
            let child_box = self.layout_node(
                child,
                Some(node.id),
                child_rect,
                Some(child_rect.size()),
                depth,
                ChildRole::GridItem {
                    row,
                    column,
                    row_span,
                    column_span,
                },
            );

            child_ids.push(child.id);
            bounds = Some(bounds.map_or(child_box.margin_box, |existing: Rect| existing.union(child_box.margin_box)));
        }

        bounds
    }

    fn emit_box(&mut self, layout_box: LayoutBox) -> LayoutBox {
        self.by_id.insert(layout_box.id, self.boxes.len());
        self.metrics.boxes_emitted = self.metrics.boxes_emitted.saturating_add(1);
        self.boxes.push(layout_box.clone());
        layout_box
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildRole {
    Normal,
    FlexItem {
        line: usize,
    },
    GridItem {
        row: usize,
        column: usize,
        row_span: usize,
        column_span: usize,
    },
}

#[must_use]
pub fn layout_flex_grid_tree(root: &LayoutNode, viewport: Size) -> LayoutResult {
    FlexGridLayoutEngine::default().layout(root, viewport)
}

#[must_use]
pub fn layout_flex_grid_tree_with_config(
    root: &LayoutNode,
    viewport: Size,
    config: FlexGridLayoutConfig,
) -> LayoutResult {
    FlexGridLayoutEngine::new(config).layout(root, viewport)
}

fn sanitize_dimension(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn resolve_length(length: Length, available: f32, intrinsic: f32) -> Option<f32> {
    match length {
        Length::Auto => None,
        Length::Px(value) => Some(sanitize_dimension(value)),
        Length::Percent(value) => Some(sanitize_dimension(available * value)),
        Length::Fr(_) => None,
        Length::MinContent | Length::MaxContent => Some(sanitize_dimension(intrinsic)),
    }
}

fn apply_min_max(
    mut value: f32,
    min: Length,
    max: Length,
    available: f32,
    min_intrinsic: f32,
    max_intrinsic: f32,
) -> f32 {
    if let Some(minimum) = resolve_length(min, available, min_intrinsic) {
        value = value.max(minimum);
    }

    if let Some(maximum) = resolve_length(max, available, max_intrinsic) {
        value = value.min(maximum);
    }

    sanitize_dimension(value)
}

fn flex_basis(child: &LayoutNode, available_main: f32, is_row: bool) -> f32 {
    let basis = child.style.flex_item.basis;

    if let Some(value) = resolve_length(
        basis,
        available_main,
        if is_row {
            child.intrinsic.max_content_width
        } else {
            child.intrinsic.preferred_height
        },
    ) {
        return value;
    }

    let main_length = if is_row {
        child.style.width
    } else {
        child.style.height
    };

    resolve_length(
        main_length,
        available_main,
        if is_row {
            child.intrinsic.max_content_width
        } else {
            child.intrinsic.preferred_height
        },
    )
    .unwrap_or_else(|| {
        if is_row {
            child.intrinsic.max_content_width
        } else {
            child.intrinsic.preferred_height
        }
    })
    .max(0.0)
}

fn flex_cross_size(child: &LayoutNode, available_cross: f32, align: AlignItems, is_row: bool) -> f32 {
    let cross_length = if is_row {
        child.style.height
    } else {
        child.style.width
    };

    resolve_length(
        cross_length,
        available_cross,
        if is_row {
            child.intrinsic.preferred_height
        } else {
            child.intrinsic.max_content_width
        },
    )
    .unwrap_or_else(|| {
        if align == AlignItems::Stretch {
            available_cross
        } else if is_row {
            child.intrinsic.preferred_height
        } else {
            child.intrinsic.max_content_width
        }
    })
    .max(0.0)
}

fn justify_offsets(justify: JustifyContent, remaining: f32, gap: f32, item_count: usize) -> (f32, f32) {
    match justify {
        JustifyContent::FlexStart => (0.0, gap),
        JustifyContent::Center => (remaining / 2.0, gap),
        JustifyContent::FlexEnd => (remaining, gap),
        JustifyContent::SpaceBetween if item_count > 1 => {
            (0.0, gap + remaining / item_count.saturating_sub(1) as f32)
        }
        JustifyContent::SpaceAround if item_count > 0 => {
            let extra = remaining / item_count as f32;
            (extra / 2.0, gap + extra)
        }
        JustifyContent::SpaceEvenly if item_count > 0 => {
            let extra = remaining / (item_count + 1) as f32;
            (extra, gap + extra)
        }
        _ => (0.0, gap),
    }
}

fn align_offset(align: AlignItems, available_cross: f32, cross_size: f32) -> f32 {
    match align {
        AlignItems::Stretch | AlignItems::FlexStart => 0.0,
        AlignItems::Center => (available_cross - cross_size).max(0.0) / 2.0,
        AlignItems::FlexEnd => (available_cross - cross_size).max(0.0),
    }
}

fn resolve_grid_tracks(
    explicit: &[GridTrackSize],
    count: usize,
    auto_track: GridTrackSize,
    available: f32,
    gap: f32,
    auto_fallback: f32,
) -> Vec<f32> {
    let tracks = (0..count)
        .map(|index| explicit.get(index).copied().unwrap_or(auto_track))
        .collect::<Vec<_>>();

    let total_gap = gap.max(0.0) * count.saturating_sub(1) as f32;
    let available_without_gap = (available - total_gap).max(0.0);

    let mut fixed = 0.0;
    let mut total_fr = 0.0;
    let mut auto_count = 0usize;

    for track in &tracks {
        match *track {
            GridTrackSize::Px(value) => fixed += sanitize_dimension(value),
            GridTrackSize::Percent(value) => fixed += sanitize_dimension(available_without_gap * value),
            GridTrackSize::Fr(value) => total_fr += value.max(0.0),
            GridTrackSize::Auto => auto_count = auto_count.saturating_add(1),
        }
    }

    let auto_total = if auto_count > 0 {
        auto_fallback * auto_count as f32
    } else {
        0.0
    };

    let remaining = (available_without_gap - fixed - auto_total).max(0.0);
    let fr_unit = if total_fr > 0.0 { remaining / total_fr } else { 0.0 };

    tracks
        .into_iter()
        .map(|track| match track {
            GridTrackSize::Px(value) => sanitize_dimension(value),
            GridTrackSize::Percent(value) => sanitize_dimension(available_without_gap * value),
            GridTrackSize::Fr(value) => sanitize_dimension(value.max(0.0) * fr_unit),
            GridTrackSize::Auto => auto_fallback,
        })
        .collect()
}

fn track_offset(tracks: &[f32], index: usize, gap: f32) -> f32 {
    tracks.iter().take(index).copied().sum::<f32>() + gap.max(0.0) * index as f32
}

fn span_size(tracks: &[f32], start: usize, span: usize, gap: f32) -> f32 {
    let available_tracks = tracks.iter().skip(start).take(span).copied().sum::<f32>();
    available_tracks + gap.max(0.0) * span.saturating_sub(1) as f32
}

fn next_grid_position(
    occupied: &BTreeMap<(usize, usize), LayoutNodeId>,
    start_cursor: usize,
    row_count: usize,
    column_count: usize,
    row_span: usize,
    column_span: usize,
    auto_flow: GridAutoFlow,
) -> (usize, usize) {
    let cell_count = row_count.saturating_mul(column_count).max(1);

    for offset in 0..cell_count {
        let cursor = start_cursor.saturating_add(offset);
        let (row, column) = match auto_flow {
            GridAutoFlow::Row => (cursor / column_count, cursor % column_count),
            GridAutoFlow::Column => (cursor % row_count, cursor / row_count),
        };

        if row + row_span <= row_count
            && column + column_span <= column_count
            && grid_area_is_free(occupied, row, column, row_span, column_span)
        {
            return (row, column);
        }
    }

    (row_count.saturating_sub(1), column_count.saturating_sub(1))
}

fn grid_area_is_free(
    occupied: &BTreeMap<(usize, usize), LayoutNodeId>,
    row: usize,
    column: usize,
    row_span: usize,
    column_span: usize,
) -> bool {
    for y in row..row.saturating_add(row_span) {
        for x in column..column.saturating_add(column_span) {
            if occupied.contains_key(&(y, x)) {
                return false;
            }
        }
    }

    true
}

fn div_ceil(value: usize, divisor: usize) -> usize {
    if divisor == 0 {
        0
    } else {
        value.saturating_add(divisor - 1) / divisor
    }
}

#[must_use]
pub const fn px(value: f32) -> Length {
    Length::Px(value)
}

#[must_use]
pub const fn percent(value: f32) -> Length {
    Length::Percent(value)
}

#[must_use]
pub const fn fr(value: f32) -> GridTrackSize {
    GridTrackSize::Fr(value)
}

#[must_use]
pub const fn track_px(value: f32) -> GridTrackSize {
    GridTrackSize::Px(value)
}
