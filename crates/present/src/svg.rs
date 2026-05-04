#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::collapsible_else_if,
    clippy::cognitive_complexity,
    clippy::derivable_impls,
    clippy::field_reassign_with_default,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::module_name_repetitions,
    clippy::needless_raw_string_hashes,
    clippy::suboptimal_flops,
    clippy::too_many_lines
)]
#![doc = "SVG and icon rendering-lite primitives for Sylphos Present."]

use std::collections::BTreeMap;

use crate::Color;

/// Default icon viewport size used by common SVG icon packs.
pub const DEFAULT_ICON_SIZE: f32 = 24.0;

/// SVG document parsed into a deterministic, renderer-friendly subset.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgDocument {
    /// SVG viewport or viewBox.
    pub viewport: SvgViewBox,

    /// Intrinsic width, when supplied.
    pub intrinsic_width: Option<f32>,

    /// Intrinsic height, when supplied.
    pub intrinsic_height: Option<f32>,

    /// Optional accessible title.
    pub title: Option<String>,

    /// Ordered drawable nodes.
    pub nodes: Vec<SvgNode>,

    /// Non-fatal parser counters.
    pub diagnostics: SvgDiagnostics,
}

impl Default for SvgDocument {
    fn default() -> Self {
        Self {
            viewport: SvgViewBox::default(),
            intrinsic_width: None,
            intrinsic_height: None,
            title: None,
            nodes: Vec::new(),
            diagnostics: SvgDiagnostics::default(),
        }
    }
}

impl SvgDocument {
    /// Returns whether the document has at least one drawable node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Builds a paint plan using `current_color` for `currentColor` SVG paints.
    #[must_use]
    pub fn paint_plan(&self, current_color: Color) -> SvgPaintPlan {
        build_svg_paint_plan(self, current_color)
    }
}

/// SVG viewBox.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgViewBox {
    /// Minimum x coordinate.
    pub min_x: f32,

    /// Minimum y coordinate.
    pub min_y: f32,

    /// Width.
    pub width: f32,

    /// Height.
    pub height: f32,
}

impl Default for SvgViewBox {
    fn default() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            width: DEFAULT_ICON_SIZE,
            height: DEFAULT_ICON_SIZE,
        }
    }
}

impl SvgViewBox {
    /// Creates a sanitized viewBox.
    #[must_use]
    pub fn new(min_x: f32, min_y: f32, width: f32, height: f32) -> Self {
        Self {
            min_x: sanitize_coordinate(min_x),
            min_y: sanitize_coordinate(min_y),
            width: sanitize_dimension(width).max(1.0),
            height: sanitize_dimension(height).max(1.0),
        }
    }
}

/// Non-fatal SVG parser diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SvgDiagnostics {
    /// Number of parsed elements.
    pub elements_seen: u32,

    /// Number of drawable elements emitted.
    pub drawable_nodes: u32,

    /// Number of elements skipped because the lite renderer does not support them.
    pub unsupported_elements: u32,

    /// Number of malformed attributes or path segments skipped.
    pub malformed_items: u32,
}

impl SvgDiagnostics {
    fn saw_element(&mut self) {
        self.elements_seen = self.elements_seen.saturating_add(1);
    }

    fn saw_drawable(&mut self) {
        self.drawable_nodes = self.drawable_nodes.saturating_add(1);
    }

    fn saw_unsupported(&mut self) {
        self.unsupported_elements = self.unsupported_elements.saturating_add(1);
    }

    fn saw_malformed(&mut self) {
        self.malformed_items = self.malformed_items.saturating_add(1);
    }
}

/// Supported SVG node types.
#[derive(Debug, Clone, PartialEq)]
pub enum SvgNode {
    /// Rectangle.
    Rect(SvgRect),

    /// Circle.
    Circle(SvgCircle),

    /// Line.
    Line(SvgLine),

    /// Polyline.
    Polyline(SvgPolyline),

    /// Polygon.
    Polygon(SvgPolyline),

    /// Path.
    Path(SvgPath),
}

/// Common SVG paint values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgPaint {
    /// Fill color.
    pub fill: Option<SvgColor>,

    /// Stroke color.
    pub stroke: Option<SvgColor>,

    /// Stroke width in viewBox units.
    pub stroke_width: f32,

    /// Overall opacity.
    pub opacity: f32,
}

impl Default for SvgPaint {
    fn default() -> Self {
        Self {
            fill: Some(SvgColor::Color(Color::black())),
            stroke: None,
            stroke_width: 1.0,
            opacity: 1.0,
        }
    }
}

/// SVG color value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgColor {
    /// Concrete color.
    Color(Color),

    /// CSS currentColor placeholder.
    CurrentColor,
}

impl SvgColor {
    /// Resolves the color.
    #[must_use]
    pub fn resolve(self, current_color: Color, opacity: f32) -> Color {
        let mut color = match self {
            Self::Color(color) => color,
            Self::CurrentColor => current_color,
        };
        color.a = (color.a * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        color
    }
}

/// Rectangle node.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgRect {
    /// Left coordinate.
    pub x: f32,

    /// Top coordinate.
    pub y: f32,

    /// Width.
    pub width: f32,

    /// Height.
    pub height: f32,

    /// Corner radius x.
    pub rx: f32,

    /// Corner radius y.
    pub ry: f32,

    /// Paint.
    pub paint: SvgPaint,
}

/// Circle node.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgCircle {
    /// Center x.
    pub cx: f32,

    /// Center y.
    pub cy: f32,

    /// Radius.
    pub r: f32,

    /// Paint.
    pub paint: SvgPaint,
}

/// Line node.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgLine {
    /// Start x.
    pub x1: f32,

    /// Start y.
    pub y1: f32,

    /// End x.
    pub x2: f32,

    /// End y.
    pub y2: f32,

    /// Paint.
    pub paint: SvgPaint,
}

/// Polyline or polygon node.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgPolyline {
    /// Points.
    pub points: Vec<SvgPoint>,

    /// Paint.
    pub paint: SvgPaint,
}

/// Path node.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgPath {
    /// Path segments.
    pub segments: Vec<SvgPathSegment>,

    /// Paint.
    pub paint: SvgPaint,
}

/// Point in SVG coordinate space.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SvgPoint {
    /// x coordinate.
    pub x: f32,

    /// y coordinate.
    pub y: f32,
}

impl SvgPoint {
    /// Creates a point.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Supported path segment subset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgPathSegment {
    /// Move current point.
    MoveTo(SvgPoint),

    /// Straight line to point.
    LineTo(SvgPoint),

    /// Close subpath.
    Close,
}

/// Renderer-ready SVG paint plan.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgPaintPlan {
    /// Source viewport.
    pub viewport: SvgViewBox,

    /// Ordered paint commands.
    pub commands: Vec<SvgPaintCommand>,

    /// Diagnostics copied from parsing.
    pub diagnostics: SvgDiagnostics,
}

impl SvgPaintPlan {
    /// Returns whether the plan has no drawable commands.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// SVG paint command subset consumed by the app renderer.
#[derive(Debug, Clone, PartialEq)]
pub enum SvgPaintCommand {
    /// Filled rectangle.
    Rect {
        /// Left coordinate.
        x: f32,

        /// Top coordinate.
        y: f32,

        /// Width.
        width: f32,

        /// Height.
        height: f32,

        /// Fill color.
        fill: Color,
    },

    /// Filled circle.
    Circle {
        /// Center x.
        cx: f32,

        /// Center y.
        cy: f32,

        /// Radius.
        r: f32,

        /// Fill color.
        fill: Color,
    },

    /// Filled polygon/path.
    FillPath {
        /// Ordered path points.
        points: Vec<SvgPoint>,

        /// Fill color.
        fill: Color,
    },

    /// Stroked line.
    StrokeLine {
        /// Start x.
        x1: f32,

        /// Start y.
        y1: f32,

        /// End x.
        x2: f32,

        /// End y.
        y2: f32,

        /// Stroke width.
        width: f32,

        /// Stroke color.
        color: Color,
    },

    /// Stroked polyline/path.
    StrokePolyline {
        /// Ordered points.
        points: Vec<SvgPoint>,

        /// Stroke width.
        width: f32,

        /// Stroke color.
        color: Color,

        /// Whether final point joins first point.
        closed: bool,
    },
}

/// Small in-memory SVG icon registry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IconRegistry {
    icons: BTreeMap<String, SvgDocument>,
}

impl IconRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a pre-parsed icon.
    pub fn insert(&mut self, name: impl Into<String>, icon: SvgDocument) -> Option<SvgDocument> {
        self.icons.insert(normalize_icon_name(&name.into()), icon)
    }

    /// Parses and inserts an SVG icon.
    pub fn insert_svg(&mut self, name: impl Into<String>, svg: &str) -> Option<SvgDocument> {
        self.insert(name, parse_svg_lite(svg))
    }

    /// Returns a parsed icon.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SvgDocument> {
        self.icons.get(&normalize_icon_name(name))
    }

    /// Builds a paint plan for an icon.
    #[must_use]
    pub fn paint_plan(&self, name: &str, current_color: Color) -> Option<SvgPaintPlan> {
        self.get(name)
            .map(|document| document.paint_plan(current_color))
    }

    /// Returns registered icon names.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.icons.keys().map(String::as_str).collect()
    }
}

/// Builds the built-in icon set used for smoke tests and default UI chrome.
#[must_use]
pub fn builtin_icon_registry() -> IconRegistry {
    let mut registry = IconRegistry::new();
    let _ = registry.insert_svg(
        "check",
        r##"<svg viewBox="0 0 24 24"><path fill="currentColor" d="M9 16.2 L4.8 12 L3.4 13.4 L9 19 L21 7 L19.6 5.6 Z"/></svg>"##,
    );
    let _ = registry.insert_svg(
        "close",
        r##"<svg viewBox="0 0 24 24"><path fill="currentColor" d="M6 5 L5 6 L11 12 L5 18 L6 19 L12 13 L18 19 L19 18 L13 12 L19 6 L18 5 L12 11 Z"/></svg>"##,
    );
    let _ = registry.insert_svg(
        "warning",
        r##"<svg viewBox="0 0 24 24"><path fill="currentColor" d="M12 3 L22 21 L2 21 Z"/><rect x="11" y="9" width="2" height="6" fill="#ffffff"/><rect x="11" y="17" width="2" height="2" fill="#ffffff"/></svg>"##,
    );
    let _ = registry.insert_svg(
        "info",
        r##"<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="currentColor"/><rect x="11" y="10" width="2" height="7" fill="#ffffff"/><rect x="11" y="6" width="2" height="2" fill="#ffffff"/></svg>"##,
    );
    registry
}

/// Parses an SVG document into the supported lite subset.
#[must_use]
pub fn parse_svg_lite(input: &str) -> SvgDocument {
    let mut document = SvgDocument::default();
    document.title = extract_title(input);

    for tag in SvgTagIterator::new(input) {
        if tag.closing {
            continue;
        }

        document.diagnostics.saw_element();

        match tag.name.as_str() {
            "svg" => apply_svg_root(&mut document, &tag.attrs),
            "rect" => push_rect(&mut document, &tag.attrs),
            "circle" => push_circle(&mut document, &tag.attrs),
            "line" => push_line(&mut document, &tag.attrs),
            "polyline" => push_polyline(&mut document, &tag.attrs, false),
            "polygon" => push_polyline(&mut document, &tag.attrs, true),
            "path" => push_path(&mut document, &tag.attrs),
            "title" | "desc" | "g" | "defs" | "symbol" | "use" => {}
            _ => document.diagnostics.saw_unsupported(),
        }
    }

    document
}

/// Builds a deterministic paint plan from a parsed SVG document.
#[must_use]
pub fn build_svg_paint_plan(document: &SvgDocument, current_color: Color) -> SvgPaintPlan {
    let mut commands = Vec::new();

    for node in &document.nodes {
        match node {
            SvgNode::Rect(rect) => {
                if rect.width <= 0.0 || rect.height <= 0.0 {
                    continue;
                }
                if let Some(fill) = rect.paint.fill {
                    commands.push(SvgPaintCommand::Rect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                        fill: fill.resolve(current_color, rect.paint.opacity),
                    });
                }
                append_rect_stroke(&mut commands, rect, current_color);
            }
            SvgNode::Circle(circle) => {
                if circle.r <= 0.0 {
                    continue;
                }
                if let Some(fill) = circle.paint.fill {
                    commands.push(SvgPaintCommand::Circle {
                        cx: circle.cx,
                        cy: circle.cy,
                        r: circle.r,
                        fill: fill.resolve(current_color, circle.paint.opacity),
                    });
                }
                append_circle_stroke(&mut commands, circle, current_color);
            }
            SvgNode::Line(line) => {
                if let Some(stroke) = line.paint.stroke {
                    commands.push(SvgPaintCommand::StrokeLine {
                        x1: line.x1,
                        y1: line.y1,
                        x2: line.x2,
                        y2: line.y2,
                        width: line.paint.stroke_width.max(0.25),
                        color: stroke.resolve(current_color, line.paint.opacity),
                    });
                }
            }
            SvgNode::Polyline(polyline) => {
                append_polyline_commands(&mut commands, polyline, false, current_color);
            }
            SvgNode::Polygon(polyline) => {
                append_polyline_commands(&mut commands, polyline, true, current_color);
            }
            SvgNode::Path(path) => {
                append_path_commands(&mut commands, path, current_color);
            }
        }
    }

    SvgPaintPlan {
        viewport: document.viewport,
        commands,
        diagnostics: document.diagnostics,
    }
}

fn append_rect_stroke(commands: &mut Vec<SvgPaintCommand>, rect: &SvgRect, current_color: Color) {
    let Some(stroke) = rect.paint.stroke else {
        return;
    };
    let color = stroke.resolve(current_color, rect.paint.opacity);
    let width = rect.paint.stroke_width.max(0.25);
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    commands.push(SvgPaintCommand::StrokePolyline {
        points: vec![
            SvgPoint::new(rect.x, rect.y),
            SvgPoint::new(right, rect.y),
            SvgPoint::new(right, bottom),
            SvgPoint::new(rect.x, bottom),
        ],
        width,
        color,
        closed: true,
    });
}

fn append_circle_stroke(
    commands: &mut Vec<SvgPaintCommand>,
    circle: &SvgCircle,
    current_color: Color,
) {
    let Some(stroke) = circle.paint.stroke else {
        return;
    };
    let color = stroke.resolve(current_color, circle.paint.opacity);
    let points = circle_points(circle.cx, circle.cy, circle.r, 24);
    commands.push(SvgPaintCommand::StrokePolyline {
        points,
        width: circle.paint.stroke_width.max(0.25),
        color,
        closed: true,
    });
}

fn append_polyline_commands(
    commands: &mut Vec<SvgPaintCommand>,
    polyline: &SvgPolyline,
    closed: bool,
    current_color: Color,
) {
    if polyline.points.len() < 2 {
        return;
    }

    if closed && polyline.points.len() >= 3 {
        if let Some(fill) = polyline.paint.fill {
            commands.push(SvgPaintCommand::FillPath {
                points: polyline.points.clone(),
                fill: fill.resolve(current_color, polyline.paint.opacity),
            });
        }
    }

    if let Some(stroke) = polyline.paint.stroke {
        commands.push(SvgPaintCommand::StrokePolyline {
            points: polyline.points.clone(),
            width: polyline.paint.stroke_width.max(0.25),
            color: stroke.resolve(current_color, polyline.paint.opacity),
            closed,
        });
    }
}

fn append_path_commands(commands: &mut Vec<SvgPaintCommand>, path: &SvgPath, current_color: Color) {
    let subpaths = path_subpaths(&path.segments);

    for subpath in subpaths {
        if subpath.points.len() >= 3 {
            if let Some(fill) = path.paint.fill {
                commands.push(SvgPaintCommand::FillPath {
                    points: subpath.points.clone(),
                    fill: fill.resolve(current_color, path.paint.opacity),
                });
            }
        }

        if subpath.points.len() >= 2 {
            if let Some(stroke) = path.paint.stroke {
                commands.push(SvgPaintCommand::StrokePolyline {
                    points: subpath.points,
                    width: path.paint.stroke_width.max(0.25),
                    color: stroke.resolve(current_color, path.paint.opacity),
                    closed: subpath.closed,
                });
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct SvgSubpath {
    points: Vec<SvgPoint>,
    closed: bool,
}

fn path_subpaths(segments: &[SvgPathSegment]) -> Vec<SvgSubpath> {
    let mut subpaths = Vec::new();
    let mut current = Vec::<SvgPoint>::new();
    let mut closed = false;

    for segment in segments {
        match *segment {
            SvgPathSegment::MoveTo(point) => {
                if !current.is_empty() {
                    subpaths.push(SvgSubpath {
                        points: std::mem::take(&mut current),
                        closed,
                    });
                }
                current.push(point);
                closed = false;
            }
            SvgPathSegment::LineTo(point) => current.push(point),
            SvgPathSegment::Close => closed = true,
        }
    }

    if !current.is_empty() {
        subpaths.push(SvgSubpath {
            points: current,
            closed,
        });
    }

    subpaths
}

fn apply_svg_root(document: &mut SvgDocument, attrs: &[SvgAttr]) {
    document.intrinsic_width = attr_px(attrs, "width");
    document.intrinsic_height = attr_px(attrs, "height");

    if let Some(viewbox) = attr_value(attrs, "viewBox").or_else(|| attr_value(attrs, "viewbox")) {
        if let Some(parsed) = parse_viewbox(viewbox) {
            document.viewport = parsed;
            return;
        }
        document.diagnostics.saw_malformed();
    }

    if let (Some(width), Some(height)) = (document.intrinsic_width, document.intrinsic_height) {
        document.viewport = SvgViewBox::new(0.0, 0.0, width, height);
    }
}

fn push_rect(document: &mut SvgDocument, attrs: &[SvgAttr]) {
    let width = attr_px(attrs, "width").unwrap_or(0.0);
    let height = attr_px(attrs, "height").unwrap_or(0.0);

    if width <= 0.0 || height <= 0.0 {
        document.diagnostics.saw_malformed();
        return;
    }

    document.nodes.push(SvgNode::Rect(SvgRect {
        x: attr_px(attrs, "x").unwrap_or(0.0),
        y: attr_px(attrs, "y").unwrap_or(0.0),
        width,
        height,
        rx: attr_px(attrs, "rx").unwrap_or(0.0),
        ry: attr_px(attrs, "ry").unwrap_or(0.0),
        paint: parse_paint(attrs),
    }));
    document.diagnostics.saw_drawable();
}

fn push_circle(document: &mut SvgDocument, attrs: &[SvgAttr]) {
    let r = attr_px(attrs, "r").unwrap_or(0.0);

    if r <= 0.0 {
        document.diagnostics.saw_malformed();
        return;
    }

    document.nodes.push(SvgNode::Circle(SvgCircle {
        cx: attr_px(attrs, "cx").unwrap_or(0.0),
        cy: attr_px(attrs, "cy").unwrap_or(0.0),
        r,
        paint: parse_paint(attrs),
    }));
    document.diagnostics.saw_drawable();
}

fn push_line(document: &mut SvgDocument, attrs: &[SvgAttr]) {
    document.nodes.push(SvgNode::Line(SvgLine {
        x1: attr_px(attrs, "x1").unwrap_or(0.0),
        y1: attr_px(attrs, "y1").unwrap_or(0.0),
        x2: attr_px(attrs, "x2").unwrap_or(0.0),
        y2: attr_px(attrs, "y2").unwrap_or(0.0),
        paint: parse_paint(attrs),
    }));
    document.diagnostics.saw_drawable();
}

fn push_polyline(document: &mut SvgDocument, attrs: &[SvgAttr], polygon: bool) {
    let Some(points) = attr_value(attrs, "points").and_then(parse_points) else {
        document.diagnostics.saw_malformed();
        return;
    };

    if points.len() < 2 {
        document.diagnostics.saw_malformed();
        return;
    }

    let node = SvgPolyline {
        points,
        paint: parse_paint(attrs),
    };

    if polygon {
        document.nodes.push(SvgNode::Polygon(node));
    } else {
        document.nodes.push(SvgNode::Polyline(node));
    }

    document.diagnostics.saw_drawable();
}

fn push_path(document: &mut SvgDocument, attrs: &[SvgAttr]) {
    let Some(data) = attr_value(attrs, "d") else {
        document.diagnostics.saw_malformed();
        return;
    };

    let parsed = parse_path_internal(data);
    if parsed.segments.is_empty() {
        document.diagnostics.saw_malformed();
        return;
    }

    document.nodes.push(SvgNode::Path(SvgPath {
        segments: parsed.segments,
        paint: parse_paint(attrs),
    }));

    for _ in 0..parsed.malformed_items {
        document.diagnostics.saw_malformed();
    }

    document.diagnostics.saw_drawable();
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedPath {
    segments: Vec<SvgPathSegment>,
    malformed_items: u32,
}

/// Parses SVG path data into the lite line-based subset.
///
/// Curve and arc commands are reduced to their end points. This keeps icons
/// visible and deterministic before full Bézier tessellation lands.
#[must_use]
pub fn parse_svg_path_lite(data: &str) -> Vec<SvgPathSegment> {
    parse_path_internal(data).segments
}

fn parse_path_internal(data: &str) -> ParsedPath {
    let tokens = tokenize_path(data);
    let mut index = 0usize;
    let mut command = None::<char>;
    let mut current = SvgPoint::default();
    let mut subpath_start = SvgPoint::default();
    let mut segments = Vec::<SvgPathSegment>::new();
    let mut malformed_items = 0u32;

    while index < tokens.len() {
        if let Some(path_command) = token_command(tokens.get(index)) {
            command = Some(path_command);
            index = index.saturating_add(1);
        }

        let Some(active) = command else {
            malformed_items = malformed_items.saturating_add(1);
            index = index.saturating_add(1);
            continue;
        };

        match active {
            'M' | 'm' => {
                let mut first = true;
                while let Some((x, y)) = read_pair(&tokens, &mut index) {
                    let point = resolve_point(active.is_lowercase(), current, x, y);
                    if first {
                        segments.push(SvgPathSegment::MoveTo(point));
                        subpath_start = point;
                        first = false;
                    } else {
                        segments.push(SvgPathSegment::LineTo(point));
                    }
                    current = point;
                    if next_is_command(&tokens, index) {
                        break;
                    }
                }
                command = Some(if active == 'm' { 'l' } else { 'L' });
            }
            'L' | 'l' => {
                if !read_line_pairs(active, &tokens, &mut index, &mut current, &mut segments) {
                    malformed_items = malformed_items.saturating_add(1);
                    index = index.saturating_add(1);
                }
            }
            'H' | 'h' => {
                if !read_horizontal(active, &tokens, &mut index, &mut current, &mut segments) {
                    malformed_items = malformed_items.saturating_add(1);
                    index = index.saturating_add(1);
                }
            }
            'V' | 'v' => {
                if !read_vertical(active, &tokens, &mut index, &mut current, &mut segments) {
                    malformed_items = malformed_items.saturating_add(1);
                    index = index.saturating_add(1);
                }
            }
            'C' | 'c' => {
                if !read_endpoint_groups(
                    active,
                    &tokens,
                    &mut index,
                    6,
                    4,
                    &mut current,
                    &mut segments,
                ) {
                    malformed_items = malformed_items.saturating_add(1);
                    index = index.saturating_add(1);
                }
            }
            'S' | 's' | 'Q' | 'q' => {
                if !read_endpoint_groups(
                    active,
                    &tokens,
                    &mut index,
                    4,
                    2,
                    &mut current,
                    &mut segments,
                ) {
                    malformed_items = malformed_items.saturating_add(1);
                    index = index.saturating_add(1);
                }
            }
            'T' | 't' => {
                if !read_line_pairs(active, &tokens, &mut index, &mut current, &mut segments) {
                    malformed_items = malformed_items.saturating_add(1);
                    index = index.saturating_add(1);
                }
            }
            'A' | 'a' => {
                if !read_endpoint_groups(
                    active,
                    &tokens,
                    &mut index,
                    7,
                    5,
                    &mut current,
                    &mut segments,
                ) {
                    malformed_items = malformed_items.saturating_add(1);
                    index = index.saturating_add(1);
                }
            }
            'Z' | 'z' => {
                segments.push(SvgPathSegment::Close);
                current = subpath_start;
                command = None;
            }
            _ => {
                malformed_items = malformed_items.saturating_add(1);
                index = index.saturating_add(1);
            }
        }
    }

    ParsedPath {
        segments,
        malformed_items,
    }
}

fn read_line_pairs(
    command: char,
    tokens: &[PathToken],
    index: &mut usize,
    current: &mut SvgPoint,
    segments: &mut Vec<SvgPathSegment>,
) -> bool {
    let mut any = false;
    while let Some((x, y)) = read_pair(tokens, index) {
        let point = resolve_point(command.is_lowercase(), *current, x, y);
        segments.push(SvgPathSegment::LineTo(point));
        *current = point;
        any = true;
        if next_is_command(tokens, *index) {
            break;
        }
    }
    any
}

fn read_horizontal(
    command: char,
    tokens: &[PathToken],
    index: &mut usize,
    current: &mut SvgPoint,
    segments: &mut Vec<SvgPathSegment>,
) -> bool {
    let mut any = false;
    while let Some(x) = read_number(tokens, index) {
        let next_x = if command.is_lowercase() {
            current.x + x
        } else {
            x
        };
        let point = SvgPoint::new(next_x, current.y);
        segments.push(SvgPathSegment::LineTo(point));
        *current = point;
        any = true;
        if next_is_command(tokens, *index) {
            break;
        }
    }
    any
}

fn read_vertical(
    command: char,
    tokens: &[PathToken],
    index: &mut usize,
    current: &mut SvgPoint,
    segments: &mut Vec<SvgPathSegment>,
) -> bool {
    let mut any = false;
    while let Some(y) = read_number(tokens, index) {
        let next_y = if command.is_lowercase() {
            current.y + y
        } else {
            y
        };
        let point = SvgPoint::new(current.x, next_y);
        segments.push(SvgPathSegment::LineTo(point));
        *current = point;
        any = true;
        if next_is_command(tokens, *index) {
            break;
        }
    }
    any
}

fn read_endpoint_groups(
    command: char,
    tokens: &[PathToken],
    index: &mut usize,
    group_size: usize,
    endpoint_offset: usize,
    current: &mut SvgPoint,
    segments: &mut Vec<SvgPathSegment>,
) -> bool {
    let mut any = false;

    while count_numbers_until_command(tokens, *index) >= group_size {
        let mut group = Vec::with_capacity(group_size);
        for _ in 0..group_size {
            if let Some(value) = read_number(tokens, index) {
                group.push(value);
            }
        }

        if group.len() != group_size {
            break;
        }

        let x = group[endpoint_offset];
        let y = group[endpoint_offset + 1];
        let point = resolve_point(command.is_lowercase(), *current, x, y);
        segments.push(SvgPathSegment::LineTo(point));
        *current = point;
        any = true;

        if next_is_command(tokens, *index) {
            break;
        }
    }

    any
}

fn read_pair(tokens: &[PathToken], index: &mut usize) -> Option<(f32, f32)> {
    let x = read_number(tokens, index)?;
    let y = read_number(tokens, index)?;
    Some((x, y))
}

fn read_number(tokens: &[PathToken], index: &mut usize) -> Option<f32> {
    let value = match tokens.get(*index)? {
        PathToken::Number(value) => *value,
        PathToken::Command(_) => return None,
    };
    *index = (*index).saturating_add(1);
    Some(value)
}

fn token_command(token: Option<&PathToken>) -> Option<char> {
    match token {
        Some(PathToken::Command(command)) => Some(*command),
        _ => None,
    }
}

fn next_is_command(tokens: &[PathToken], index: usize) -> bool {
    matches!(tokens.get(index), Some(PathToken::Command(_)))
}

fn count_numbers_until_command(tokens: &[PathToken], mut index: usize) -> usize {
    let mut count = 0usize;
    while let Some(token) = tokens.get(index) {
        match token {
            PathToken::Number(_) => count = count.saturating_add(1),
            PathToken::Command(_) => break,
        }
        index = index.saturating_add(1);
    }
    count
}

fn resolve_point(relative: bool, current: SvgPoint, x: f32, y: f32) -> SvgPoint {
    if relative {
        SvgPoint::new(current.x + x, current.y + y)
    } else {
        SvgPoint::new(x, y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PathToken {
    Command(char),
    Number(f32),
}

fn tokenize_path(data: &str) -> Vec<PathToken> {
    let chars = data.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    let mut tokens = Vec::new();

    while let Some(ch) = chars.get(index).copied() {
        if ch.is_ascii_whitespace() || ch == ',' {
            index = index.saturating_add(1);
            continue;
        }

        if ch.is_ascii_alphabetic() {
            tokens.push(PathToken::Command(ch));
            index = index.saturating_add(1);
            continue;
        }

        let start = index;
        index = index.saturating_add(1);
        while let Some(next) = chars.get(index).copied() {
            if next.is_ascii_alphabetic() || next == ',' || next.is_ascii_whitespace() {
                break;
            }
            if (next == '-' || next == '+')
                && !is_exponent_marker(chars.get(index.saturating_sub(1)))
            {
                break;
            }
            index = index.saturating_add(1);
        }

        let value = chars[start..index].iter().collect::<String>();
        if let Ok(number) = value.parse::<f32>() {
            if number.is_finite() {
                tokens.push(PathToken::Number(number));
            }
        }
    }

    tokens
}

fn is_exponent_marker(ch: Option<&char>) -> bool {
    matches!(ch, Some(value) if *value == 'e' || *value == 'E')
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SvgTag {
    name: String,
    attrs: Vec<SvgAttr>,
    closing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SvgAttr {
    name: String,
    value: String,
}

struct SvgTagIterator<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> SvgTagIterator<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }
}

impl Iterator for SvgTagIterator<'_> {
    type Item = SvgTag;

    fn next(&mut self) -> Option<Self::Item> {
        let input = &self.input[self.cursor..];
        let open_offset = input.find('<')?;
        let after_open = self.cursor + open_offset + 1;
        let close_offset = self.input[after_open..].find('>')?;
        let close = after_open + close_offset;
        self.cursor = close.saturating_add(1);

        let raw = self.input[after_open..close].trim();
        if raw.is_empty() || raw.starts_with('!') || raw.starts_with('?') {
            return self.next();
        }

        let closing = raw.starts_with('/');
        let content = raw.trim_start_matches('/').trim_end_matches('/').trim();
        let mut chars = content.char_indices();
        let mut name_end = content.len();
        for (index, ch) in chars.by_ref() {
            if ch.is_ascii_whitespace() {
                name_end = index;
                break;
            }
        }

        let name = content[..name_end].trim().to_ascii_lowercase();
        if name.is_empty() {
            return self.next();
        }

        let attr_source = content.get(name_end..).unwrap_or_default();
        Some(SvgTag {
            name,
            attrs: parse_attributes(attr_source),
            closing,
        })
    }
}

fn parse_attributes(source: &str) -> Vec<SvgAttr> {
    let chars = source.chars().collect::<Vec<_>>();
    let mut attrs = Vec::new();
    let mut index = 0usize;

    while index < chars.len() {
        while matches!(chars.get(index), Some(ch) if ch.is_ascii_whitespace() || *ch == '/') {
            index = index.saturating_add(1);
        }

        let name_start = index;
        while matches!(chars.get(index), Some(ch) if !ch.is_ascii_whitespace() && *ch != '=' && *ch != '/')
        {
            index = index.saturating_add(1);
        }
        if index == name_start {
            break;
        }

        let name = chars[name_start..index]
            .iter()
            .collect::<String>()
            .trim()
            .to_owned();

        while matches!(chars.get(index), Some(ch) if ch.is_ascii_whitespace()) {
            index = index.saturating_add(1);
        }

        if chars.get(index).copied() != Some('=') {
            attrs.push(SvgAttr {
                name,
                value: String::new(),
            });
            continue;
        }
        index = index.saturating_add(1);

        while matches!(chars.get(index), Some(ch) if ch.is_ascii_whitespace()) {
            index = index.saturating_add(1);
        }

        let value = if matches!(chars.get(index).copied(), Some('"' | '\'')) {
            let quote = chars[index];
            index = index.saturating_add(1);
            let value_start = index;
            while matches!(chars.get(index), Some(ch) if *ch != quote) {
                index = index.saturating_add(1);
            }
            let value = chars[value_start..index].iter().collect::<String>();
            if matches!(chars.get(index), Some(ch) if *ch == quote) {
                index = index.saturating_add(1);
            }
            value
        } else {
            let value_start = index;
            while matches!(chars.get(index), Some(ch) if !ch.is_ascii_whitespace() && *ch != '/') {
                index = index.saturating_add(1);
            }
            chars[value_start..index].iter().collect::<String>()
        };

        attrs.push(SvgAttr { name, value });
    }

    attrs
}

fn parse_paint(attrs: &[SvgAttr]) -> SvgPaint {
    let mut paint = SvgPaint::default();

    if let Some(fill) = attr_style_value(attrs, "fill") {
        paint.fill = parse_svg_color(fill);
    }
    if let Some(stroke) = attr_style_value(attrs, "stroke") {
        paint.stroke = parse_svg_color(stroke);
    }
    if let Some(width) = attr_style_value(attrs, "stroke-width").and_then(parse_svg_number) {
        paint.stroke_width = width.max(0.0);
    }

    let opacity = attr_style_value(attrs, "opacity")
        .and_then(parse_svg_number)
        .unwrap_or(1.0);
    let fill_opacity = attr_style_value(attrs, "fill-opacity")
        .and_then(parse_svg_number)
        .unwrap_or(1.0);
    let stroke_opacity = attr_style_value(attrs, "stroke-opacity")
        .and_then(parse_svg_number)
        .unwrap_or(1.0);

    paint.opacity = opacity.clamp(0.0, 1.0);

    if paint.fill.is_some() && fill_opacity < 1.0 {
        paint.opacity = (paint.opacity * fill_opacity).clamp(0.0, 1.0);
    }
    if paint.stroke.is_some() && stroke_opacity < 1.0 && paint.fill.is_none() {
        paint.opacity = (paint.opacity * stroke_opacity).clamp(0.0, 1.0);
    }

    paint
}

fn parse_svg_color(value: &str) -> Option<SvgColor> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("transparent") {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("currentColor") {
        return Some(SvgColor::CurrentColor);
    }
    if let Some(color) = Color::from_css_hex(trimmed) {
        return Some(SvgColor::Color(color));
    }
    named_color(trimmed).map(SvgColor::Color)
}

fn named_color(value: &str) -> Option<Color> {
    match value.trim().to_ascii_lowercase().as_str() {
        "black" => Some(Color::black()),
        "white" => Some(Color::white()),
        "red" => Some(Color::rgba(1.0, 0.0, 0.0, 1.0)),
        "green" => Some(Color::rgba(0.0, 0.50, 0.0, 1.0)),
        "blue" => Some(Color::rgba(0.0, 0.0, 1.0, 1.0)),
        "gray" | "grey" => Some(Color::rgba(0.50, 0.50, 0.50, 1.0)),
        "yellow" => Some(Color::rgba(1.0, 1.0, 0.0, 1.0)),
        "orange" => Some(Color::rgba(1.0, 0.65, 0.0, 1.0)),
        "purple" => Some(Color::rgba(0.50, 0.0, 0.50, 1.0)),
        _ => None,
    }
}

fn attr_value<'a>(attrs: &'a [SvgAttr], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attr| attr.name.eq_ignore_ascii_case(name))
        .map(|attr| attr.value.as_str())
}

fn attr_style_value<'a>(attrs: &'a [SvgAttr], property: &str) -> Option<&'a str> {
    let style_value =
        attr_value(attrs, "style").and_then(|style| style_property_value(style, property));
    style_value.or_else(|| attr_value(attrs, property))
}

fn style_property_value<'a>(style: &'a str, property: &str) -> Option<&'a str> {
    for declaration in style.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case(property) {
            return Some(value.trim());
        }
    }
    None
}

fn attr_px(attrs: &[SvgAttr], name: &str) -> Option<f32> {
    attr_value(attrs, name).and_then(parse_svg_number)
}

fn parse_svg_number(value: &str) -> Option<f32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let number = trimmed
        .trim_end_matches("px")
        .trim_end_matches('%')
        .trim()
        .parse::<f32>()
        .ok()?;
    number.is_finite().then_some(number)
}

fn parse_viewbox(value: &str) -> Option<SvgViewBox> {
    let values = parse_number_list(value);
    if values.len() != 4 {
        return None;
    }
    Some(SvgViewBox::new(values[0], values[1], values[2], values[3]))
}

fn parse_points(value: &str) -> Option<Vec<SvgPoint>> {
    let numbers = parse_number_list(value);
    if numbers.len() < 4 || numbers.len() % 2 != 0 {
        return None;
    }

    let mut points = Vec::with_capacity(numbers.len() / 2);
    let mut index = 0usize;
    while index + 1 < numbers.len() {
        points.push(SvgPoint::new(numbers[index], numbers[index + 1]));
        index = index.saturating_add(2);
    }
    Some(points)
}

fn parse_number_list(value: &str) -> Vec<f32> {
    value
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .filter_map(parse_svg_number)
        .collect()
}

fn extract_title(input: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let content_start = input[start..]
        .find('>')?
        .saturating_add(start)
        .saturating_add(1);
    let content_end = lower[content_start..]
        .find("</title>")?
        .saturating_add(content_start);
    let title = decode_xml_entities(input[content_start..content_end].trim());
    (!title.is_empty()).then_some(title)
}

fn decode_xml_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn circle_points(cx: f32, cy: f32, r: f32, count: usize) -> Vec<SvgPoint> {
    let safe_count = count.max(8);
    let mut points = Vec::with_capacity(safe_count);
    for index in 0..safe_count {
        let angle = (index as f32 / safe_count as f32) * std::f32::consts::TAU;
        points.push(SvgPoint::new(cx + angle.cos() * r, cy + angle.sin() * r));
    }
    points
}

fn normalize_icon_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('_', "-")
}

fn sanitize_coordinate(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn sanitize_dimension(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        DEFAULT_ICON_SIZE
    }
}
