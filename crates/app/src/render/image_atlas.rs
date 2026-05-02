#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

//! Runtime image atlas for decoded page images.
//!
//! This is intentionally simple: decoded images are packed into one RGBA atlas
//! per paint-state revision. It keeps the renderer single-pipeline and avoids a
//! bind-group circus for every `<img>` tag. Circus technology remains reserved
//! for CSS, regrettably.

use std::collections::BTreeMap;

const ATLAS_WIDTH: usize = 2048;
const ATLAS_PADDING: usize = 2;
const EMPTY_ATLAS_WIDTH: u32 = 1;
const EMPTY_ATLAS_HEIGHT: u32 = 1;

/// Decoded RGBA image owned by the app runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedImage {
    /// Fully resolved URL used as the image key.
    pub url: String,

    /// Pixel width.
    pub width: u32,

    /// Pixel height.
    pub height: u32,

    /// RGBA8 pixels in row-major order.
    pub rgba: Vec<u8>,
}

/// Collection of decoded page images.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DecodedImageStore {
    images: BTreeMap<String, DecodedImage>,
}

impl DecodedImageStore {
    pub(crate) fn insert(&mut self, image: DecodedImage) {
        self.images.insert(image.url.clone(), image);
    }

    pub(crate) fn len(&self) -> usize {
        self.images.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    fn values(&self) -> impl Iterator<Item = &DecodedImage> {
        self.images.values()
    }
}

/// Packed image atlas sampled by the WGPU fragment shader.
#[derive(Debug, Clone)]
pub(crate) struct ImageAtlas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    entries: BTreeMap<String, ImageAtlasEntry>,
}

/// Atlas placement for one decoded image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ImageAtlasEntry {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy)]
struct Placement {
    x: usize,
    y: usize,
}

impl ImageAtlas {
    pub(crate) fn empty() -> Self {
        Self {
            width: EMPTY_ATLAS_WIDTH,
            height: EMPTY_ATLAS_HEIGHT,
            pixels: vec![255, 255, 255, 255],
            entries: BTreeMap::new(),
        }
    }

    pub(crate) fn build(store: &DecodedImageStore) -> Self {
        if store.is_empty() {
            return Self::empty();
        }

        let images = store.values().collect::<Vec<_>>();
        let placements = pack_images(&images);
        let atlas_width = ATLAS_WIDTH;
        let atlas_height = placements
            .height
            .max(usize::try_from(EMPTY_ATLAS_HEIGHT).unwrap_or(1));

        let mut pixels = vec![0_u8; atlas_width.saturating_mul(atlas_height).saturating_mul(4)];
        let mut entries = BTreeMap::new();

        for (image, placement) in images.iter().zip(placements.items.iter()) {
            blit_image(&mut pixels, atlas_width, atlas_height, image, *placement);

            let x0 = placement.x as f32 / atlas_width as f32;
            let y0 = placement.y as f32 / atlas_height as f32;
            let x1 = placement.x.saturating_add(image.width as usize) as f32 / atlas_width as f32;
            let y1 = placement.y.saturating_add(image.height as usize) as f32 / atlas_height as f32;

            entries.insert(
                image.url.clone(),
                ImageAtlasEntry {
                    uv_min: [x0, y0],
                    uv_max: [x1, y1],
                    width: image.width as f32,
                    height: image.height as f32,
                },
            );
        }

        Self {
            width: atlas_width as u32,
            height: atlas_height as u32,
            pixels,
            entries,
        }
    }

    pub(crate) fn image(&self, url: &str) -> Option<ImageAtlasEntry> {
        self.entries.get(url).copied()
    }
}

struct PackedImages {
    items: Vec<Placement>,
    height: usize,
}

fn pack_images(images: &[&DecodedImage]) -> PackedImages {
    let mut x = ATLAS_PADDING;
    let mut y = ATLAS_PADDING;
    let mut row_height = 0usize;
    let mut placements = Vec::with_capacity(images.len());

    for image in images {
        let width = image.width as usize;
        let height = image.height as usize;

        if width == 0 || height == 0 {
            placements.push(Placement { x: 0, y: 0 });
            continue;
        }

        if x.saturating_add(width).saturating_add(ATLAS_PADDING) > ATLAS_WIDTH {
            x = ATLAS_PADDING;
            y = y.saturating_add(row_height).saturating_add(ATLAS_PADDING);
            row_height = 0;
        }

        placements.push(Placement { x, y });
        x = x.saturating_add(width).saturating_add(ATLAS_PADDING);
        row_height = row_height.max(height.saturating_add(ATLAS_PADDING));
    }

    PackedImages {
        items: placements,
        height: y.saturating_add(row_height).saturating_add(ATLAS_PADDING),
    }
}

fn blit_image(
    pixels: &mut [u8],
    atlas_width: usize,
    atlas_height: usize,
    image: &DecodedImage,
    placement: Placement,
) {
    let image_width = image.width as usize;
    let image_height = image.height as usize;

    for row in 0..image_height {
        for column in 0..image_width {
            let target_x = placement.x.saturating_add(column);
            let target_y = placement.y.saturating_add(row);

            if target_x >= atlas_width || target_y >= atlas_height {
                continue;
            }

            let source_index = row
                .saturating_mul(image_width)
                .saturating_add(column)
                .saturating_mul(4);
            let target_index = target_y
                .saturating_mul(atlas_width)
                .saturating_add(target_x)
                .saturating_mul(4);

            if source_index.saturating_add(4) > image.rgba.len()
                || target_index.saturating_add(4) > pixels.len()
            {
                continue;
            }

            pixels[target_index..target_index + 4]
                .copy_from_slice(&image.rgba[source_index..source_index + 4]);
        }
    }
}
