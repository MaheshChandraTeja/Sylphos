#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

//! Async image discovery, fetching, decoding, and downscaling.
//!
//! This module intentionally stays outside the presentation crate. `present`
//! knows that an image exists and where it should be placed; the app owns the
//! network/decode/cache runtime work needed to turn that source into pixels.

use anyhow::{anyhow, bail, Context, Result};
use image::{imageops::FilterType, GenericImageView};
use present::{RenderBlock, RenderDocument};
use std::collections::BTreeSet;
use tracing::{debug, warn};

use crate::{
    browser::{resolve_link_url, CacheSource, CacheStore},
    render::{DecodedImage, DecodedImageStore},
};

const MAX_IMAGES_PER_PAGE: usize = 24;
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_DECODED_DIMENSION: u32 = 1280;

/// Summary of an image loading pass.
#[derive(Debug, Clone)]
pub(crate) struct ImageLoadSummary {
    /// Unique image URLs discovered in the document after URL resolution.
    pub discovered: usize,

    /// Images successfully fetched and decoded.
    pub decoded: usize,

    /// Images skipped due to limits or unsupported data.
    pub failed: usize,

    /// Images served from the in-process memory cache.
    pub memory_hits: usize,

    /// Images served from persistent disk cache.
    pub disk_hits: usize,

    /// Images fetched from network and written into cache when possible.
    pub network_fetches: usize,

    /// Images fetched from network because cache was disabled.
    pub disabled_fetches: usize,

    /// Decoded image store passed to the renderer.
    pub store: DecodedImageStore,
}

/// Discovers, fetches, decodes, and downscales document images.
///
/// The first version is deliberately conservative: sequential fetches, hard
/// image-count cap, hard byte cap, and max-dimension downscaling. Browsers that
/// eagerly fetch the entire internet are how laptops become space heaters.
pub(crate) fn resolve_document_image_sources(base_url: &str, document: &mut RenderDocument) {
    for block in &mut document.blocks {
        let RenderBlock::Image { src: Some(src), .. } = block else {
            continue;
        };

        match resolve_link_url(base_url, src) {
            Ok(resolved) => *src = resolved,
            Err(error) => {
                debug!(src = %src, error = %error, "kept unresolved image source");
            }
        }
    }
}

pub(crate) async fn fetch_decode_images(
    base_url: &str,
    document: &RenderDocument,
    cache: &CacheStore,
) -> ImageLoadSummary {
    let urls = collect_image_urls(base_url, document);
    let discovered = urls.len();
    let mut store = DecodedImageStore::default();
    let mut failed = 0usize;
    let mut memory_hits = 0usize;
    let mut disk_hits = 0usize;
    let mut network_fetches = 0usize;
    let mut disabled_fetches = 0usize;

    for url in urls.into_iter().take(MAX_IMAGES_PER_PAGE) {
        match fetch_decode_image(&url, cache).await {
            Ok((image, source)) => {
                match source {
                    CacheSource::Memory => memory_hits = memory_hits.saturating_add(1),
                    CacheSource::Disk => disk_hits = disk_hits.saturating_add(1),
                    CacheSource::Network => network_fetches = network_fetches.saturating_add(1),
                    CacheSource::Disabled => disabled_fetches = disabled_fetches.saturating_add(1),
                }

                debug!(
                    url = %url,
                    width = image.width,
                    height = image.height,
                    cache_source = source.as_str(),
                    "decoded page image"
                );
                store.insert(image);
            }
            Err(error) => {
                failed = failed.saturating_add(1);
                warn!(url = %url, error = %error, "failed to load page image");
            }
        }
    }

    if discovered > MAX_IMAGES_PER_PAGE {
        failed = failed.saturating_add(discovered - MAX_IMAGES_PER_PAGE);
    }

    ImageLoadSummary {
        discovered,
        decoded: store.len(),
        failed,
        memory_hits,
        disk_hits,
        network_fetches,
        disabled_fetches,
        store,
    }
}

fn collect_image_urls(base_url: &str, document: &RenderDocument) -> Vec<String> {
    let mut urls = BTreeSet::new();

    for block in &document.blocks {
        let RenderBlock::Image { src: Some(src), .. } = block else {
            continue;
        };

        match resolve_link_url(base_url, src) {
            Ok(url) => {
                urls.insert(url);
            }
            Err(error) => {
                debug!(src = %src, error = %error, "skipped unresolved image source");
            }
        }
    }

    urls.into_iter().collect()
}

async fn fetch_decode_image(url: &str, cache: &CacheStore) -> Result<(DecodedImage, CacheSource)> {
    let cached = cache.get_or_fetch_bytes(url, MAX_IMAGE_BYTES).await?;
    let image = decode_image_bytes(&cached.url, &cached.bytes)?;
    Ok((image, cached.source))
}

fn decode_image_bytes(url: &str, bytes: &[u8]) -> Result<DecodedImage> {
    if bytes.is_empty() {
        bail!("image response body was empty");
    }

    let decoded = image::load_from_memory(bytes)
        .with_context(|| format!("failed to decode image bytes from `{url}`"))?;

    let (source_width, source_height) = decoded.dimensions();

    if source_width == 0 || source_height == 0 {
        return Err(anyhow!("decoded image has zero dimensions"));
    }

    let rgba = decoded.to_rgba8();
    let scaled = downscale_if_needed(rgba, source_width, source_height);
    let (width, height) = scaled.dimensions();

    Ok(DecodedImage {
        url: url.to_owned(),
        width,
        height,
        rgba: scaled.into_raw(),
    })
}

fn downscale_if_needed(
    image: image::RgbaImage,
    source_width: u32,
    source_height: u32,
) -> image::RgbaImage {
    let max_dimension = source_width.max(source_height);

    if max_dimension <= MAX_DECODED_DIMENSION {
        return image;
    }

    let scale = MAX_DECODED_DIMENSION as f32 / max_dimension as f32;
    let width = ((source_width as f32 * scale).round() as u32).max(1);
    let height = ((source_height as f32 * scale).round() as u32).max(1);

    image::imageops::resize(&image, width, height, FilterType::Triangle)
}
