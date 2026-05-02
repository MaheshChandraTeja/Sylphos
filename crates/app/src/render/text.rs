#![allow(clippy::cast_precision_loss)]

use present::Color;

use super::{
    font_atlas::{quantize_size, FontAtlas},
    mesh::{CanvasSize, Mesh, Rect, TexRect},
};

const FALLBACK_SPACE_ADVANCE_RATIO: f32 = 0.33;
const BASELINE_RATIO: f32 = 0.82;
const LINE_HEIGHT_RATIO: f32 = 1.35;

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_text(
    mesh: &mut Mesh,
    atlas: &FontAtlas,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    color: Color,
    canvas: CanvasSize,
) {
    if text.is_empty() || size <= 0.0 || canvas.width <= 0.0 || canvas.height <= 0.0 {
        return;
    }

    let size_px = quantize_size(size) as f32;
    let mut cursor_x = x;
    let mut cursor_y = y;
    let line_height = (size_px * LINE_HEIGHT_RATIO).max(size_px + 2.0);
    let baseline_offset = size_px * BASELINE_RATIO;

    for ch in text.chars() {
        if ch == '\n' {
            cursor_x = x;
            cursor_y += line_height;
            continue;
        }

        if ch == '\t' {
            cursor_x += size_px * FALLBACK_SPACE_ADVANCE_RATIO * 4.0;
            continue;
        }

        let Some(glyph) = atlas.glyph(ch, size_px) else {
            cursor_x += size_px * FALLBACK_SPACE_ADVANCE_RATIO;
            continue;
        };

        if glyph.width > 0.0 && glyph.height > 0.0 {
            let draw_x = cursor_x + glyph.xmin;
            let baseline_y = cursor_y + baseline_offset;
            let draw_y = baseline_y - glyph.height - glyph.ymin;

            mesh.push_font_rect(
                Rect {
                    x: draw_x,
                    y: draw_y,
                    width: glyph.width,
                    height: glyph.height,
                },
                TexRect {
                    u0: glyph.uv_min[0],
                    v0: glyph.uv_min[1],
                    u1: glyph.uv_max[0],
                    v1: glyph.uv_max[1],
                },
                color,
                canvas,
            );
        }

        cursor_x += glyph
            .advance_width
            .max(size_px * FALLBACK_SPACE_ADVANCE_RATIO);

        if cursor_y > canvas.height {
            break;
        }
    }
}
