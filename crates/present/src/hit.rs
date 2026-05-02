#![doc = "Hit-testing helpers for presentation-layer interaction."]

use crate::{
    layout_document, measure_line_height, measure_text_width, LayoutBoxKind, LayoutRect,
    RenderDocument,
};

/// A clickable link region generated from the viewport layout tree.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkHitRegion {
    /// Link target exactly as extracted from the source HTML.
    pub href: String,

    /// Human-readable link text for status display and diagnostics.
    pub text: String,

    /// Clickable rectangle in page-local logical pixels.
    pub rect: LayoutRect,
}

/// Result returned from a link hit-test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkHitResult {
    /// Link target exactly as extracted from the source HTML.
    pub href: String,

    /// Human-readable link text.
    pub text: String,
}

/// Builds clickable link regions for a document at the given viewport size.
///
/// Coordinates are page-local. If the app has browser chrome above the page,
/// subtract the chrome height before calling [`hit_test_link`].
#[must_use]
pub fn collect_link_hit_regions(
    doc: &RenderDocument,
    width: f32,
    height: f32,
) -> Vec<LinkHitRegion> {
    let layout = layout_document(doc, width, height);
    let mut regions = Vec::new();

    for layout_box in &layout.boxes {
        let LayoutBoxKind::Link { href: Some(href) } = &layout_box.kind else {
            continue;
        };

        if href.trim().is_empty() {
            continue;
        }

        for run in &layout_box.text_runs {
            if run.text.trim().is_empty() {
                continue;
            }

            let rect = LayoutRect::new(
                run.x,
                run.y,
                measure_text_width(&run.text, run.size).max(run.size * 0.5),
                measure_line_height(run.size),
            );

            regions.push(LinkHitRegion {
                href: href.clone(),
                text: run.text.clone(),
                rect,
            });
        }
    }

    regions
}

/// Hit-tests links in page-local coordinates.
#[must_use]
pub fn hit_test_link(
    doc: &RenderDocument,
    width: f32,
    height: f32,
    x: f32,
    y: f32,
) -> Option<LinkHitResult> {
    collect_link_hit_regions(doc, width, height)
        .into_iter()
        .find(|region| contains(region.rect, x, y))
        .map(|region| LinkHitResult {
            href: region.href,
            text: region.text,
        })
}

fn contains(rect: LayoutRect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}
