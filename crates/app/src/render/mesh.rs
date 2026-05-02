#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use present::{Color, PaintCommand, PaintPlan};
use tracing::warn;

use super::{
    font_atlas::FontAtlas,
    image_atlas::{DecodedImageStore, ImageAtlas, ImageAtlasEntry},
    text,
};

const VERTEX_FLOATS: usize = 9;
const VERTEX_SIZE_BYTES: wgpu::BufferAddress =
    (VERTEX_FLOATS * std::mem::size_of::<f32>()) as wgpu::BufferAddress;

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x4,
    2 => Float32x2,
    3 => Float32
];

const TEXTURE_KIND_SOLID: f32 = 0.0;
const TEXTURE_KIND_FONT: f32 = 1.0;
const TEXTURE_KIND_IMAGE: f32 = 2.0;

#[derive(Debug, Clone)]
pub(crate) struct DrawMesh {
    pub vertices: Vec<Vertex>,
    pub font_atlas: FontAtlas,
    pub image_atlas: ImageAtlas,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Vertex {
    x: f32,
    y: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    u: f32,
    v: f32,
    texture_kind: f32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Mesh {
    pub vertices: Vec<Vertex>,
}

impl Mesh {
    pub(crate) fn push_rect(&mut self, rect: Rect, color: Color, canvas: CanvasSize) {
        let Some(clipped) = clip_rect(rect, canvas) else {
            return;
        };

        let top_left = Vertex::solid(clipped.left, clipped.top, color, canvas);
        let bottom_left = Vertex::solid(clipped.left, clipped.bottom, color, canvas);
        let bottom_right = Vertex::solid(clipped.right, clipped.bottom, color, canvas);
        let top_right = Vertex::solid(clipped.right, clipped.top, color, canvas);

        self.vertices.extend_from_slice(&[
            top_left,
            bottom_left,
            bottom_right,
            top_left,
            bottom_right,
            top_right,
        ]);
    }

    pub(crate) fn push_font_rect(
        &mut self,
        rect: Rect,
        tex: TexRect,
        color: Color,
        canvas: CanvasSize,
    ) {
        self.push_textured_rect(rect, tex, color, TEXTURE_KIND_FONT, canvas);
    }

    pub(crate) fn push_image_rect(
        &mut self,
        rect: Rect,
        tex: TexRect,
        color: Color,
        canvas: CanvasSize,
    ) {
        self.push_textured_rect(rect, tex, color, TEXTURE_KIND_IMAGE, canvas);
    }

    fn push_textured_rect(
        &mut self,
        rect: Rect,
        tex: TexRect,
        color: Color,
        texture_kind: f32,
        canvas: CanvasSize,
    ) {
        let Some(clipped) = clip_textured_rect(rect, tex, canvas) else {
            return;
        };

        let top_left = Vertex::textured(
            clipped.left,
            clipped.top,
            color,
            clipped.u0,
            clipped.v0,
            texture_kind,
            canvas,
        );
        let bottom_left = Vertex::textured(
            clipped.left,
            clipped.bottom,
            color,
            clipped.u0,
            clipped.v1,
            texture_kind,
            canvas,
        );
        let bottom_right = Vertex::textured(
            clipped.right,
            clipped.bottom,
            color,
            clipped.u1,
            clipped.v1,
            texture_kind,
            canvas,
        );
        let top_right = Vertex::textured(
            clipped.right,
            clipped.top,
            color,
            clipped.u1,
            clipped.v0,
            texture_kind,
            canvas,
        );

        self.vertices.extend_from_slice(&[
            top_left,
            bottom_left,
            bottom_right,
            top_left,
            bottom_right,
            top_right,
        ]);
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CanvasSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TexRect {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

#[derive(Debug, Clone, Copy)]
struct ClippedRect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

#[derive(Debug, Clone, Copy)]
struct ClippedTexturedRect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
}

impl Vertex {
    fn solid(x: f32, y: f32, color: Color, canvas: CanvasSize) -> Self {
        Self::new(x, y, color, 0.0, 0.0, TEXTURE_KIND_SOLID, canvas)
    }

    fn textured(
        x: f32,
        y: f32,
        color: Color,
        u: f32,
        v: f32,
        texture_kind: f32,
        canvas: CanvasSize,
    ) -> Self {
        Self::new(x, y, color, u, v, texture_kind, canvas)
    }

    fn new(
        x: f32,
        y: f32,
        color: Color,
        u: f32,
        v: f32,
        texture_kind: f32,
        canvas: CanvasSize,
    ) -> Self {
        let safe_width = canvas.width.max(1.0);
        let safe_height = canvas.height.max(1.0);
        let ndc_x = (x / safe_width).mul_add(2.0, -1.0);
        let ndc_y = 1.0 - ((y / safe_height) * 2.0);

        Self {
            x: ndc_x,
            y: ndc_y,
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
            u,
            v,
            texture_kind,
        }
    }
}

pub(crate) fn vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: VERTEX_SIZE_BYTES,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    }
}

pub(crate) fn encode_vertices(vertices: &[Vertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(
        vertices
            .len()
            .saturating_mul(VERTEX_FLOATS)
            .saturating_mul(4),
    );

    for vertex in vertices {
        for value in [
            vertex.x,
            vertex.y,
            vertex.r,
            vertex.g,
            vertex.b,
            vertex.a,
            vertex.u,
            vertex.v,
            vertex.texture_kind,
        ] {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }

    bytes
}

pub(crate) fn build_draw_mesh_from_plan(
    plan: &PaintPlan,
    width: f32,
    height: f32,
    images: &DecodedImageStore,
) -> DrawMesh {
    let font_atlas = match FontAtlas::build_for_plan(plan) {
        Ok(atlas) => atlas,
        Err(error) => {
            warn!(error = %error, "failed to build font atlas; text will be skipped for this frame");
            FontAtlas::empty()
        }
    };

    let image_atlas = ImageAtlas::build(images);
    let canvas = CanvasSize {
        width: width.max(1.0),
        height: height.max(1.0),
    };

    let mut mesh = Mesh::default();

    for command in &plan.commands {
        match command {
            PaintCommand::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                mesh.push_rect(
                    Rect {
                        x: *x,
                        y: *y,
                        width: *width,
                        height: *height,
                    },
                    *color,
                    canvas,
                );
            }
            PaintCommand::TextPlaceholder {
                x,
                y,
                text,
                size,
                color,
            } => {
                text::append_text(&mut mesh, &font_atlas, text, *x, *y, *size, *color, canvas);
            }
            PaintCommand::Image {
                x,
                y,
                width,
                height,
                src,
                background,
                ..
            } => {
                append_image_command(
                    &mut mesh,
                    &image_atlas,
                    src.as_deref(),
                    Rect {
                        x: *x,
                        y: *y,
                        width: *width,
                        height: *height,
                    },
                    *background,
                    canvas,
                );
            }
        }
    }

    DrawMesh {
        vertices: mesh.vertices,
        font_atlas,
        image_atlas,
    }
}

fn append_image_command(
    mesh: &mut Mesh,
    atlas: &ImageAtlas,
    src: Option<&str>,
    rect: Rect,
    background: Color,
    canvas: CanvasSize,
) {
    mesh.push_rect(rect, background, canvas);

    let Some(src) = src else {
        return;
    };

    let Some(entry) = atlas.image(src) else {
        return;
    };

    let fitted = fit_image_rect(rect, entry);

    mesh.push_image_rect(
        fitted,
        TexRect {
            u0: entry.uv_min[0],
            v0: entry.uv_min[1],
            u1: entry.uv_max[0],
            v1: entry.uv_max[1],
        },
        Color::white(),
        canvas,
    );
}

fn fit_image_rect(rect: Rect, image: ImageAtlasEntry) -> Rect {
    if image.width <= 0.0 || image.height <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return rect;
    }

    let scale = (rect.width / image.width).min(rect.height / image.height);
    let fitted_width = image.width * scale;
    let fitted_height = image.height * scale;

    Rect {
        x: rect.x + ((rect.width - fitted_width) * 0.5),
        y: rect.y + ((rect.height - fitted_height) * 0.5),
        width: fitted_width,
        height: fitted_height,
    }
}

fn clip_rect(rect: Rect, canvas: CanvasSize) -> Option<ClippedRect> {
    if rect.width <= 0.0 || rect.height <= 0.0 || canvas.width <= 0.0 || canvas.height <= 0.0 {
        return None;
    }

    let left = rect.x.max(0.0);
    let top = rect.y.max(0.0);
    let right = (rect.x + rect.width).min(canvas.width).max(left);
    let bottom = (rect.y + rect.height).min(canvas.height).max(top);

    (right > left && bottom > top).then_some(ClippedRect {
        left,
        top,
        right,
        bottom,
    })
}

fn clip_textured_rect(rect: Rect, tex: TexRect, canvas: CanvasSize) -> Option<ClippedTexturedRect> {
    let clipped = clip_rect(rect, canvas)?;

    let original_right = rect.x + rect.width;
    let original_bottom = rect.y + rect.height;

    if original_right <= rect.x || original_bottom <= rect.y {
        return None;
    }

    let left_ratio = ((clipped.left - rect.x) / rect.width).clamp(0.0, 1.0);
    let right_ratio = ((clipped.right - rect.x) / rect.width).clamp(0.0, 1.0);
    let top_ratio = ((clipped.top - rect.y) / rect.height).clamp(0.0, 1.0);
    let bottom_ratio = ((clipped.bottom - rect.y) / rect.height).clamp(0.0, 1.0);

    Some(ClippedTexturedRect {
        left: clipped.left,
        top: clipped.top,
        right: clipped.right,
        bottom: clipped.bottom,
        u0: lerp(tex.u0, tex.u1, left_ratio),
        u1: lerp(tex.u0, tex.u1, right_ratio),
        v0: lerp(tex.v0, tex.v1, top_ratio),
        v1: lerp(tex.v0, tex.v1, bottom_ratio),
    })
}

fn lerp(left: f32, right: f32, amount: f32) -> f32 {
    left.mul_add(1.0 - amount, right * amount)
}
