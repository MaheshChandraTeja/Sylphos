#![allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::module_name_repetitions,
    clippy::too_many_arguments
)]

use present::{Color, SvgPaintCommand, SvgPaintPlan, SvgPoint, SvgViewBox};

use super::mesh::{CanvasSize, Mesh, Rect};

const CIRCLE_SEGMENTS: usize = 28;

#[derive(Debug, Clone, Copy)]
struct MappedPoint {
    x: f32,
    y: f32,
}

impl MappedPoint {
    fn xy(self) -> [f32; 2] {
        [self.x, self.y]
    }
}

#[derive(Debug, Clone, Copy)]
struct SvgTransform {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

pub(crate) fn append_svg_icon(
    mesh: &mut Mesh,
    plan: &SvgPaintPlan,
    target: Rect,
    canvas: CanvasSize,
) {
    if plan.is_empty() || target.width <= 0.0 || target.height <= 0.0 {
        return;
    }

    let transform = SvgTransform::new(plan.viewport, target);

    for command in &plan.commands {
        match command {
            SvgPaintCommand::Rect {
                x,
                y,
                width,
                height,
                fill,
            } => append_rect(mesh, transform, *x, *y, *width, *height, *fill, canvas),
            SvgPaintCommand::Circle { cx, cy, r, fill } => {
                append_circle(mesh, transform, *cx, *cy, *r, *fill, canvas);
            }
            SvgPaintCommand::FillPath { points, fill } => {
                append_filled_path(mesh, transform, points, *fill, canvas);
            }
            SvgPaintCommand::StrokeLine {
                x1,
                y1,
                x2,
                y2,
                width,
                color,
            } => append_stroked_line(
                mesh,
                transform.map(SvgPoint::new(*x1, *y1)),
                transform.map(SvgPoint::new(*x2, *y2)),
                transform.stroke_width(*width),
                *color,
                canvas,
            ),
            SvgPaintCommand::StrokePolyline {
                points,
                width,
                color,
                closed,
            } => append_stroked_polyline(
                mesh,
                transform,
                points,
                transform.stroke_width(*width),
                *color,
                *closed,
                canvas,
            ),
        }
    }
}

impl SvgTransform {
    fn new(viewbox: SvgViewBox, target: Rect) -> Self {
        let scale_x = target.width / viewbox.width.max(1.0);
        let scale_y = target.height / viewbox.height.max(1.0);
        let scale = scale_x.min(scale_y).max(0.0);
        let fitted_width = viewbox.width * scale;
        let fitted_height = viewbox.height * scale;
        let offset_x = target.x + ((target.width - fitted_width) * 0.5) - (viewbox.min_x * scale);
        let offset_y = target.y + ((target.height - fitted_height) * 0.5) - (viewbox.min_y * scale);

        Self {
            scale,
            offset_x,
            offset_y,
        }
    }

    fn map(self, point: SvgPoint) -> MappedPoint {
        MappedPoint {
            x: point.x.mul_add(self.scale, self.offset_x),
            y: point.y.mul_add(self.scale, self.offset_y),
        }
    }

    fn map_dimension(self, value: f32) -> f32 {
        value.max(0.0) * self.scale
    }

    fn stroke_width(self, value: f32) -> f32 {
        self.map_dimension(value).max(1.0)
    }
}

fn append_rect(
    mesh: &mut Mesh,
    transform: SvgTransform,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fill: Color,
    canvas: CanvasSize,
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }

    let origin = transform.map(SvgPoint::new(x, y));
    mesh.push_rect(
        Rect {
            x: origin.x,
            y: origin.y,
            width: transform.map_dimension(width),
            height: transform.map_dimension(height),
        },
        fill,
        canvas,
    );
}

fn append_circle(
    mesh: &mut Mesh,
    transform: SvgTransform,
    cx: f32,
    cy: f32,
    r: f32,
    fill: Color,
    canvas: CanvasSize,
) {
    if r <= 0.0 {
        return;
    }

    let center = transform.map(SvgPoint::new(cx, cy));
    let radius = transform.map_dimension(r);
    let mut previous = circle_point(center, radius, CIRCLE_SEGMENTS - 1);

    for index in 0..CIRCLE_SEGMENTS {
        let next = circle_point(center, radius, index);
        mesh.push_solid_triangle(center.xy(), previous.xy(), next.xy(), fill, canvas);
        previous = next;
    }
}

fn append_filled_path(
    mesh: &mut Mesh,
    transform: SvgTransform,
    points: &[SvgPoint],
    fill: Color,
    canvas: CanvasSize,
) {
    if points.len() < 3 {
        return;
    }

    let mapped = points
        .iter()
        .map(|point| transform.map(*point))
        .collect::<Vec<_>>();
    let Some(first) = mapped.first().copied() else {
        return;
    };

    for index in 1..mapped.len().saturating_sub(1) {
        let Some(left) = mapped.get(index).copied() else {
            continue;
        };
        let Some(right) = mapped.get(index + 1).copied() else {
            continue;
        };
        mesh.push_solid_triangle(first.xy(), left.xy(), right.xy(), fill, canvas);
    }
}

fn append_stroked_polyline(
    mesh: &mut Mesh,
    transform: SvgTransform,
    points: &[SvgPoint],
    width: f32,
    color: Color,
    closed: bool,
    canvas: CanvasSize,
) {
    if points.len() < 2 {
        return;
    }

    let mapped = points
        .iter()
        .map(|point| transform.map(*point))
        .collect::<Vec<_>>();
    for pair in mapped.windows(2) {
        if let [left, right] = pair {
            append_stroked_line(mesh, *left, *right, width, color, canvas);
        }
    }

    if closed {
        if let (Some(first), Some(last)) = (mapped.first().copied(), mapped.last().copied()) {
            append_stroked_line(mesh, last, first, width, color, canvas);
        }
    }
}

fn append_stroked_line(
    mesh: &mut Mesh,
    start: MappedPoint,
    end: MappedPoint,
    width: f32,
    color: Color,
    canvas: CanvasSize,
) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = dx.hypot(dy);
    if length <= f32::EPSILON {
        let half = width * 0.5;
        mesh.push_rect(
            Rect {
                x: start.x - half,
                y: start.y - half,
                width,
                height: width,
            },
            color,
            canvas,
        );
        return;
    }

    let nx = -(dy / length) * (width * 0.5);
    let ny = (dx / length) * (width * 0.5);

    let a = MappedPoint {
        x: start.x + nx,
        y: start.y + ny,
    };
    let b = MappedPoint {
        x: start.x - nx,
        y: start.y - ny,
    };
    let c = MappedPoint {
        x: end.x - nx,
        y: end.y - ny,
    };
    let d = MappedPoint {
        x: end.x + nx,
        y: end.y + ny,
    };

    mesh.push_solid_triangle(a.xy(), b.xy(), c.xy(), color, canvas);
    mesh.push_solid_triangle(a.xy(), c.xy(), d.xy(), color, canvas);
}

fn circle_point(center: MappedPoint, radius: f32, index: usize) -> MappedPoint {
    let angle = (index as f32 / CIRCLE_SEGMENTS as f32) * std::f32::consts::TAU;
    MappedPoint {
        x: center.x + angle.cos() * radius,
        y: center.y + angle.sin() * radius,
    }
}
