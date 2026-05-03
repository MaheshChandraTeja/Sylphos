#![doc = "Tiny CSS parser used by the Sylphos presentation layer."]

use crate::box_model::{BoxStyleLite, DisplayLite, EdgeSizes};
use crate::selector::{
    parse_color_value, parse_css_rules_lite, parse_font_size, parse_px, parse_viewport_fraction,
    CssDeclarationLite,
};
use crate::StyleSheetLite;

const DEFAULT_FONT_SIZE: f32 = 16.0;

/// Parses a tiny CSS subset into the legacy global `StyleSheetLite` slots.
///
/// Module 18 keeps this function for compatibility with the existing layout pipeline, but it now
/// relies on the selector-aware parser. Only plain tag selectors update global slots; class, id,
/// and descendant selectors are retained separately as `StyleRuleLite` and applied per render block.
#[must_use]
pub fn parse_css_lite(source: &str) -> StyleSheetLite {
    let mut sheet = StyleSheetLite::default();

    for rule in parse_css_rules_lite(source, 0) {
        for selector in &rule.selectors {
            let Some(tag) = selector.simple_tag_name() else {
                continue;
            };
            apply_declarations_to_simple_tag(tag, &rule.declarations, &mut sheet);
        }
    }

    sheet
}

fn apply_declarations_to_simple_tag(
    tag: &str,
    declarations: &[CssDeclarationLite],
    sheet: &mut StyleSheetLite,
) {
    match tag {
        "body" => apply_body_declarations(declarations, sheet),
        "p" => apply_paragraph_declarations(declarations, sheet),
        "a" => apply_link_declarations(declarations, sheet),
        "img" => apply_box_declarations(declarations, &mut sheet.box_model.image),
        "form" => apply_box_declarations(declarations, &mut sheet.box_model.form),
        "input" => apply_box_declarations(declarations, &mut sheet.box_model.input),
        "button" => apply_box_declarations(declarations, &mut sheet.box_model.button),
        "textarea" => apply_box_declarations(declarations, &mut sheet.box_model.textarea),
        "div" | "section" | "article" | "main" | "nav" | "header" | "footer" | "aside" => {
            apply_box_declarations(declarations, &mut sheet.box_model.div);
        }
        selector => {
            if let Some(level) = heading_level(selector) {
                apply_heading_declarations(level, declarations, sheet);
            }
        }
    }
}

fn apply_body_declarations(declarations: &[CssDeclarationLite], sheet: &mut StyleSheetLite) {
    for declaration in declarations {
        match declaration.property.as_str() {
            "background" | "background-color" => {
                if let Some(color) = parse_color_value(&declaration.value) {
                    sheet.body_background = Some(color);
                    sheet.box_model.body.background = Some(color);
                }
            }
            "color" => {
                if let Some(color) = parse_color_value(&declaration.value) {
                    sheet.body_color = Some(color);
                }
            }
            "font-size" => {
                if let Some(size) = parse_font_size(&declaration.value, DEFAULT_FONT_SIZE) {
                    sheet.body_font_size = Some(size);
                }
            }
            "width" => {
                if let Some(fraction) = parse_viewport_fraction(&declaration.value, "vw") {
                    sheet.content_width_fraction = Some(fraction);
                }
            }
            "margin" => apply_body_margin(&declaration.value, sheet),
            "margin-top" => {
                if let Some(px) = parse_px(&declaration.value) {
                    sheet.margin_top_px = Some(px);
                } else if let Some(fraction) = parse_viewport_fraction(&declaration.value, "vh") {
                    sheet.margin_top_fraction = Some(fraction);
                }
            }
            "margin-left" => {
                if declaration.value.trim().eq_ignore_ascii_case("auto") {
                    sheet.center_horizontally = true;
                } else if let Some(px) = parse_px(&declaration.value) {
                    sheet.margin_left_px = Some(px);
                }
            }
            "margin-right" => {
                if declaration.value.trim().eq_ignore_ascii_case("auto") {
                    sheet.center_horizontally = true;
                }
            }
            _ => {}
        }
    }

    apply_box_declarations(declarations, &mut sheet.box_model.body);
}

fn apply_paragraph_declarations(declarations: &[CssDeclarationLite], sheet: &mut StyleSheetLite) {
    let base = sheet.body_font_size.unwrap_or(DEFAULT_FONT_SIZE);

    for declaration in declarations {
        if declaration.property == "font-size" {
            if let Some(size) = parse_font_size(&declaration.value, base) {
                sheet.paragraph_size = Some(size);
            }
        }
    }

    apply_box_declarations(declarations, &mut sheet.box_model.paragraph);
}

fn apply_link_declarations(declarations: &[CssDeclarationLite], sheet: &mut StyleSheetLite) {
    for declaration in declarations {
        match declaration.property.as_str() {
            "color" => {
                if let Some(color) = parse_color_value(&declaration.value) {
                    sheet.link_color = Some(color);
                }
            }
            "font-size" => {
                if let Some(size) = parse_font_size(&declaration.value, DEFAULT_FONT_SIZE) {
                    sheet.paragraph_size = Some(size);
                }
            }
            _ => {}
        }
    }

    apply_box_declarations(declarations, &mut sheet.box_model.link);
}

fn apply_heading_declarations(
    level: usize,
    declarations: &[CssDeclarationLite],
    sheet: &mut StyleSheetLite,
) {
    let base = sheet.body_font_size.unwrap_or(DEFAULT_FONT_SIZE);

    for declaration in declarations {
        if declaration.property == "font-size" {
            if let Some(size) = parse_font_size(&declaration.value, base) {
                if let Some(slot) = sheet.heading_sizes.get_mut(level.saturating_sub(1)) {
                    *slot = Some(size);
                }
            }
        }
    }

    if let Some(slot) = sheet.box_model.headings.get_mut(level.saturating_sub(1)) {
        apply_box_declarations(declarations, slot);
    }
}

fn apply_box_declarations(declarations: &[CssDeclarationLite], target: &mut BoxStyleLite) {
    for declaration in declarations {
        match declaration.property.as_str() {
            "background" | "background-color" => {
                if let Some(color) = parse_color_value(&declaration.value) {
                    target.background = Some(color);
                }
            }
            "display" => {
                if let Some(display) = parse_display(&declaration.value) {
                    target.display = Some(display);
                }
            }
            "margin" => {
                if let Some(edges) = parse_edges(&declaration.value) {
                    target.margin = Some(edges);
                }
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                if let Some(px) = parse_px(&declaration.value) {
                    let mut edges = target.margin.unwrap_or_else(EdgeSizes::zero);
                    apply_edge(&mut edges, &declaration.property, px);
                    target.margin = Some(edges);
                }
            }
            "padding" => {
                if let Some(edges) = parse_edges(&declaration.value) {
                    target.padding = Some(edges);
                }
            }
            "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
                if let Some(px) = parse_px(&declaration.value) {
                    let mut edges = target.padding.unwrap_or_else(EdgeSizes::zero);
                    apply_edge(&mut edges, &declaration.property, px);
                    target.padding = Some(edges);
                }
            }
            "border" => {
                if let Some(width) = first_px_token(&declaration.value) {
                    target.border_width = Some(EdgeSizes::all(width));
                }
                if let Some(color) = parse_color_value(&declaration.value) {
                    target.border_color = Some(color);
                }
            }
            "border-width" => {
                if let Some(edges) = parse_edges(&declaration.value) {
                    target.border_width = Some(edges);
                }
            }
            "border-color" => {
                if let Some(color) = parse_color_value(&declaration.value) {
                    target.border_color = Some(color);
                }
            }
            _ => {}
        }
    }
}

fn apply_body_margin(value: &str, sheet: &mut StyleSheetLite) {
    let parts = value.split_whitespace().collect::<Vec<_>>();

    if parts.is_empty() {
        return;
    }

    if let Some(px) = parse_px(parts[0]) {
        sheet.margin_top_px = Some(px);
    } else if let Some(fraction) = parse_viewport_fraction(parts[0], "vh") {
        sheet.margin_top_fraction = Some(fraction);
    }

    match parts.as_slice() {
        [_top, horizontal] => apply_horizontal_margin_token(horizontal, sheet),
        [_top, right, _bottom, left] => {
            if right.eq_ignore_ascii_case("auto") && left.eq_ignore_ascii_case("auto") {
                sheet.center_horizontally = true;
            } else {
                apply_horizontal_margin_token(left, sheet);
            }
        }
        _ => {}
    }
}

fn apply_horizontal_margin_token(token: &str, sheet: &mut StyleSheetLite) {
    if token.eq_ignore_ascii_case("auto") {
        sheet.center_horizontally = true;
    } else if let Some(px) = parse_px(token) {
        sheet.margin_left_px = Some(px);
    }
}

fn parse_display(value: &str) -> Option<DisplayLite> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(DisplayLite::None),
        "inline" | "inline-block" => Some(DisplayLite::Inline),
        "block" | "flex" | "grid" | "table" | "list-item" => Some(DisplayLite::Block),
        _ => None,
    }
}

fn parse_edges(value: &str) -> Option<EdgeSizes> {
    let values = value
        .split_whitespace()
        .filter_map(parse_px)
        .collect::<Vec<_>>();

    match values.as_slice() {
        [all] => Some(EdgeSizes::all(*all)),
        [vertical, horizontal] => Some(EdgeSizes::new(
            *vertical,
            *horizontal,
            *vertical,
            *horizontal,
        )),
        [top, horizontal, bottom] => Some(EdgeSizes::new(*top, *horizontal, *bottom, *horizontal)),
        [top, right, bottom, left, ..] => Some(EdgeSizes::new(*top, *right, *bottom, *left)),
        _ => None,
    }
}

fn first_px_token(value: &str) -> Option<f32> {
    value.split_whitespace().find_map(parse_px)
}

fn apply_edge(edges: &mut EdgeSizes, property: &str, value: f32) {
    match property {
        "margin-top" | "padding-top" => edges.top = value,
        "margin-right" | "padding-right" => edges.right = value,
        "margin-bottom" | "padding-bottom" => edges.bottom = value,
        "margin-left" | "padding-left" => edges.left = value,
        _ => {}
    }
}

fn heading_level(selector: &str) -> Option<usize> {
    let bytes = selector.as_bytes();

    if bytes.len() == 2 && bytes[0].eq_ignore_ascii_case(&b'h') && (b'1'..=b'6').contains(&bytes[1])
    {
        Some(usize::from(bytes[1] - b'0'))
    } else {
        None
    }
}
