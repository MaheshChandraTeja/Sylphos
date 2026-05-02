#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use anyhow::{anyhow, Context, Result};
use fontdue::{Font, FontSettings, Metrics};
use present::{PaintCommand, PaintPlan};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const ATLAS_WIDTH: usize = 1024;
const ATLAS_PADDING: usize = 2;
const MIN_FONT_SIZE: u32 = 8;
const MAX_FONT_SIZE: u32 = 96;
const EMPTY_ATLAS_WIDTH: u32 = 256;
const EMPTY_ATLAS_HEIGHT: u32 = 1;
const EMPTY_ATLAS_PIXEL: u8 = 255;

#[derive(Debug, Clone)]
pub(crate) struct FontAtlas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    glyphs: BTreeMap<GlyphKey, AtlasGlyph>,
    pub font_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AtlasGlyph {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub width: f32,
    pub height: f32,
    pub advance_width: f32,
    pub xmin: f32,
    pub ymin: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GlyphKey {
    ch: char,
    size_px: u32,
}

struct LoadedFont {
    font: Font,
    name: String,
}

struct GlyphBitmap {
    key: GlyphKey,
    metrics: Metrics,
    bitmap: Vec<u8>,
}

impl FontAtlas {
    pub(crate) fn empty() -> Self {
        Self {
            width: EMPTY_ATLAS_WIDTH,
            height: EMPTY_ATLAS_HEIGHT,
            pixels: vec![EMPTY_ATLAS_PIXEL; (EMPTY_ATLAS_WIDTH * EMPTY_ATLAS_HEIGHT) as usize],
            glyphs: BTreeMap::new(),
            font_name: "empty".to_owned(),
        }
    }

    pub(crate) fn build_for_plan(plan: &PaintPlan) -> Result<Self> {
        let glyph_keys = collect_glyph_keys(plan);

        if glyph_keys.is_empty() {
            return Ok(Self::empty());
        }

        let loaded_font = load_system_font()?;
        Self::build_from_font(loaded_font, &glyph_keys)
    }

    pub(crate) fn glyph(&self, ch: char, size: f32) -> Option<AtlasGlyph> {
        let size_px = quantize_size(size);
        let key = GlyphKey { ch, size_px };

        self.glyphs.get(&key).copied().or_else(|| {
            let fallback = GlyphKey { ch: '?', size_px };
            self.glyphs.get(&fallback).copied()
        })
    }

    fn build_from_font(loaded_font: LoadedFont, glyph_keys: &BTreeSet<GlyphKey>) -> Result<Self> {
        let mut bitmaps = Vec::with_capacity(glyph_keys.len());
        let mut glyphs = BTreeMap::new();

        for key in glyph_keys {
            let px = key.size_px as f32;
            let (metrics, bitmap) = loaded_font.font.rasterize(key.ch, px);

            if metrics.width == 0 || metrics.height == 0 || bitmap.is_empty() {
                glyphs.insert(*key, atlas_glyph_for_empty_bitmap(&metrics));
                continue;
            }

            bitmaps.push(GlyphBitmap {
                key: *key,
                metrics,
                bitmap,
            });
        }

        bitmaps.sort_by(|left, right| {
            right
                .metrics
                .height
                .cmp(&left.metrics.height)
                .then_with(|| right.metrics.width.cmp(&left.metrics.width))
        });

        let placements = pack_glyphs(&bitmaps);
        let atlas_width = ATLAS_WIDTH;
        let atlas_height = placements.height.max(EMPTY_ATLAS_HEIGHT as usize);
        let mut pixels = vec![0_u8; atlas_width.saturating_mul(atlas_height)];

        for (bitmap, placement) in bitmaps.iter().zip(placements.items.iter()) {
            blit_glyph(
                &mut pixels,
                atlas_width,
                atlas_height,
                placement.x,
                placement.y,
                &bitmap.metrics,
                &bitmap.bitmap,
            );

            glyphs.insert(
                bitmap.key,
                atlas_glyph_for_placed_bitmap(
                    &bitmap.metrics,
                    placement.x,
                    placement.y,
                    atlas_width,
                    atlas_height,
                ),
            );
        }

        let width = u32::try_from(atlas_width).context("font atlas width overflow")?;
        let height = u32::try_from(atlas_height).context("font atlas height overflow")?;

        Ok(Self {
            width,
            height,
            pixels,
            glyphs,
            font_name: loaded_font.name,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct GlyphPlacement {
    x: usize,
    y: usize,
}

struct PackedGlyphs {
    items: Vec<GlyphPlacement>,
    height: usize,
}

fn pack_glyphs(bitmaps: &[GlyphBitmap]) -> PackedGlyphs {
    let mut x = ATLAS_PADDING;
    let mut y = ATLAS_PADDING;
    let mut row_height = 0usize;
    let mut placements = Vec::with_capacity(bitmaps.len());

    for bitmap in bitmaps {
        let width = bitmap.metrics.width.saturating_add(ATLAS_PADDING);
        let height = bitmap.metrics.height.saturating_add(ATLAS_PADDING);

        if x.saturating_add(width).saturating_add(ATLAS_PADDING) > ATLAS_WIDTH {
            x = ATLAS_PADDING;
            y = y.saturating_add(row_height).saturating_add(ATLAS_PADDING);
            row_height = 0;
        }

        placements.push(GlyphPlacement { x, y });
        x = x.saturating_add(width).saturating_add(ATLAS_PADDING);
        row_height = row_height.max(height);
    }

    PackedGlyphs {
        items: placements,
        height: y.saturating_add(row_height).saturating_add(ATLAS_PADDING),
    }
}

fn blit_glyph(
    pixels: &mut [u8],
    atlas_width: usize,
    atlas_height: usize,
    x: usize,
    y: usize,
    metrics: &Metrics,
    bitmap: &[u8],
) {
    for row in 0..metrics.height {
        for column in 0..metrics.width {
            let source_index = row.saturating_mul(metrics.width).saturating_add(column);
            let target_x = x.saturating_add(column);
            let target_y = y.saturating_add(row);

            if target_x >= atlas_width || target_y >= atlas_height || source_index >= bitmap.len() {
                continue;
            }

            let target_index = target_y
                .saturating_mul(atlas_width)
                .saturating_add(target_x);

            if let Some(target) = pixels.get_mut(target_index) {
                *target = bitmap[source_index];
            }
        }
    }
}

fn atlas_glyph_for_empty_bitmap(metrics: &Metrics) -> AtlasGlyph {
    AtlasGlyph {
        uv_min: [0.0, 0.0],
        uv_max: [0.0, 0.0],
        width: 0.0,
        height: 0.0,
        advance_width: metrics.advance_width.max(1.0),
        xmin: metrics.xmin as f32,
        ymin: metrics.ymin as f32,
    }
}

fn atlas_glyph_for_placed_bitmap(
    metrics: &Metrics,
    x: usize,
    y: usize,
    atlas_width: usize,
    atlas_height: usize,
) -> AtlasGlyph {
    let left = x as f32 / atlas_width as f32;
    let top = y as f32 / atlas_height as f32;
    let right = x.saturating_add(metrics.width) as f32 / atlas_width as f32;
    let bottom = y.saturating_add(metrics.height) as f32 / atlas_height as f32;

    AtlasGlyph {
        uv_min: [left, top],
        uv_max: [right, bottom],
        width: metrics.width as f32,
        height: metrics.height as f32,
        advance_width: metrics.advance_width.max(1.0),
        xmin: metrics.xmin as f32,
        ymin: metrics.ymin as f32,
    }
}

fn collect_glyph_keys(plan: &PaintPlan) -> BTreeSet<GlyphKey> {
    let mut keys = BTreeSet::new();

    for command in &plan.commands {
        let PaintCommand::TextPlaceholder { text, size, .. } = command else {
            continue;
        };

        let size_px = quantize_size(*size);
        keys.insert(GlyphKey { ch: '?', size_px });
        keys.insert(GlyphKey { ch: ' ', size_px });

        for ch in text.chars() {
            if ch.is_control() {
                continue;
            }

            keys.insert(GlyphKey { ch, size_px });
        }
    }

    keys
}

pub(crate) fn quantize_size(size: f32) -> u32 {
    if !size.is_finite() {
        return 16;
    }

    let rounded = size
        .round()
        .clamp(MIN_FONT_SIZE as f32, MAX_FONT_SIZE as f32);
    rounded as u32
}

fn load_system_font() -> Result<LoadedFont> {
    let candidates = system_font_candidates();

    for path in candidates {
        if !path.exists() {
            continue;
        }

        let bytes =
            fs::read(&path).with_context(|| format!("failed to read font `{}`", path.display()))?;

        match Font::from_bytes(bytes, FontSettings::default()) {
            Ok(font) => {
                return Ok(LoadedFont {
                    font,
                    name: path.display().to_string(),
                });
            }
            Err(_) => continue,
        }
    }

    Err(anyhow!(
        "no usable system TrueType/OpenType font found for Sylphos font atlas"
    ))
}

fn system_font_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if cfg!(target_os = "windows") {
        push_existing_like(&mut candidates, r"C:\Windows\Fonts\segoeui.ttf");
        push_existing_like(&mut candidates, r"C:\Windows\Fonts\arial.ttf");
        push_existing_like(&mut candidates, r"C:\Windows\Fonts\calibri.ttf");
        push_existing_like(&mut candidates, r"C:\Windows\Fonts\tahoma.ttf");
    }

    if cfg!(target_os = "macos") {
        push_existing_like(&mut candidates, "/System/Library/Fonts/SFNS.ttf");
        push_existing_like(
            &mut candidates,
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        );
        push_existing_like(&mut candidates, "/Library/Fonts/Arial.ttf");
    }

    if cfg!(target_os = "linux") {
        push_existing_like(
            &mut candidates,
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        );
        push_existing_like(
            &mut candidates,
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        );
        push_existing_like(
            &mut candidates,
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        );
        push_existing_like(
            &mut candidates,
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        );
    }

    candidates
}

fn push_existing_like(candidates: &mut Vec<PathBuf>, path: impl AsRef<Path>) {
    candidates.push(path.as_ref().to_path_buf());
}
