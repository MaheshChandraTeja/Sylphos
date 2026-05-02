#![doc = "Tiny CSS parser used by the Sylphos presentation layer."]

use crate::{Color, StyleSheetLite};

const DEFAULT_FONT_SIZE: f32 = 16.0;

/// Parses a tiny, safe CSS subset into a `StyleSheetLite`.
///
/// Supported selectors:
///
/// - `body`
/// - `p`
/// - `a`, `a:link`, `a:visited`
/// - `h1` through `h6`
///
/// Supported declarations:
///
/// - `background`, `background-color`
/// - `color`
/// - `font-size`
/// - `width` with `vw`
/// - `margin`, `margin-top`, `margin-left`
///
/// Everything else is intentionally ignored.
#[must_use]
pub fn parse_css_lite(source: &str) -> StyleSheetLite {
    let css = strip_comments(source);
    let mut sheet = StyleSheetLite::default();
    let mut cursor = 0usize;

    while let Some(open_rel) = css[cursor..].find('{') {
        let open = cursor + open_rel;
        let selector_text = css[cursor..open].trim();

        let Some(close_rel) = css[open + 1..].find('}') else {
            break;
        };

        let close = open + 1 + close_rel;
        let declaration_text = &css[open + 1..close];

        apply_rule(selector_text, declaration_text, &mut sheet);
        cursor = close + 1;
    }

    sheet
}

fn apply_rule(selector_text: &str, declaration_text: &str, sheet: &mut StyleSheetLite) {
    let declarations = parse_declarations(declaration_text);

    for selector in selector_text.split(',').map(normalize_selector) {
        if selector.is_empty() {
            continue;
        }

        apply_declarations_to_selector(&selector, &declarations, sheet);
    }
}

fn apply_declarations_to_selector(
    selector: &str,
    declarations: &[(String, String)],
    sheet: &mut StyleSheetLite,
) {
    match selector {
        "body" => apply_body_declarations(declarations, sheet),
        "p" => apply_paragraph_declarations(declarations, sheet),
        "a" => apply_link_declarations(declarations, sheet),
        selector => {
            if let Some(level) = heading_level(selector) {
                apply_heading_declarations(level, declarations, sheet);
            }
        }
    }
}

fn apply_body_declarations(declarations: &[(String, String)], sheet: &mut StyleSheetLite) {
    for (property, value) in declarations {
        match property.as_str() {
            "background" | "background-color" => {
                if let Some(color) = parse_color_value(value) {
                    sheet.body_background = Some(color);
                }
            }
            "color" => {
                if let Some(color) = parse_color_value(value) {
                    sheet.body_color = Some(color);
                }
            }
            "font-size" => {
                if let Some(size) = parse_font_size(value, DEFAULT_FONT_SIZE) {
                    sheet.body_font_size = Some(size);
                }
            }
            "width" => {
                if let Some(fraction) = parse_viewport_fraction(value, "vw") {
                    sheet.content_width_fraction = Some(fraction);
                }
            }
            "margin" => apply_margin(value, sheet),
            "margin-top" => {
                if let Some(px) = parse_px(value) {
                    sheet.margin_top_px = Some(px);
                } else if let Some(fraction) = parse_viewport_fraction(value, "vh") {
                    sheet.margin_top_fraction = Some(fraction);
                }
            }
            "margin-left" => {
                if value.trim().eq_ignore_ascii_case("auto") {
                    sheet.center_horizontally = true;
                } else if let Some(px) = parse_px(value) {
                    sheet.margin_left_px = Some(px);
                }
            }
            "margin-right" => {
                if value.trim().eq_ignore_ascii_case("auto") {
                    sheet.center_horizontally = true;
                }
            }
            _ => {}
        }
    }
}

fn apply_paragraph_declarations(declarations: &[(String, String)], sheet: &mut StyleSheetLite) {
    let base = sheet.body_font_size.unwrap_or(DEFAULT_FONT_SIZE);

    for (property, value) in declarations {
        if property == "font-size" {
            if let Some(size) = parse_font_size(value, base) {
                sheet.paragraph_size = Some(size);
            }
        }
    }
}

fn apply_link_declarations(declarations: &[(String, String)], sheet: &mut StyleSheetLite) {
    for (property, value) in declarations {
        if property == "color" {
            if let Some(color) = parse_color_value(value) {
                sheet.link_color = Some(color);
            }
        }
    }
}

fn apply_heading_declarations(
    level: usize,
    declarations: &[(String, String)],
    sheet: &mut StyleSheetLite,
) {
    let base = sheet.body_font_size.unwrap_or(DEFAULT_FONT_SIZE);

    for (property, value) in declarations {
        if property == "font-size" {
            if let Some(size) = parse_font_size(value, base) {
                if let Some(slot) = sheet.heading_sizes.get_mut(level.saturating_sub(1)) {
                    *slot = Some(size);
                }
            }
        }
    }
}

fn apply_margin(value: &str, sheet: &mut StyleSheetLite) {
    let parts: Vec<&str> = value.split_whitespace().collect();

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

fn parse_declarations(source: &str) -> Vec<(String, String)> {
    source
        .split(';')
        .filter_map(|entry| {
            let (property, value) = entry.split_once(':')?;
            let property = property.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();

            if property.is_empty() || value.is_empty() {
                None
            } else {
                Some((property, value))
            }
        })
        .collect()
}

fn normalize_selector(selector: &str) -> String {
    let simple = selector
        .split_whitespace()
        .next_back()
        .unwrap_or_default()
        .trim();

    let without_pseudo = simple.split_once(':').map_or(simple, |(before, _)| before);
    let without_class = without_pseudo
        .split_once('.')
        .map_or(without_pseudo, |(before, _)| before);
    let without_id = without_class
        .split_once('#')
        .map_or(without_class, |(before, _)| before);

    without_id.to_ascii_lowercase()
}

fn heading_level(selector: &str) -> Option<usize> {
    let bytes = selector.as_bytes();

    if bytes.len() == 2 && bytes[0].eq_ignore_ascii_case(&b'h') && (b'1'..=b'6').contains(&bytes[1])
    {
        return Some(usize::from(bytes[1] - b'0'));
    }

    None
}

fn parse_color_value(value: &str) -> Option<Color> {
    value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ')' | '('))
        .find_map(|token| {
            let cleaned = token.trim_matches(|ch: char| matches!(ch, ';' | ','));
            Color::from_css_hex(cleaned)
        })
}

fn parse_font_size(value: &str, base: f32) -> Option<f32> {
    let trimmed = value.trim().to_ascii_lowercase();

    if let Some(px) = parse_px(&trimmed) {
        return Some(px.max(1.0));
    }

    if let Some(raw) = trimmed
        .strip_suffix("rem")
        .or_else(|| trimmed.strip_suffix("em"))
    {
        let factor = raw.trim().parse::<f32>().ok()?;
        return Some((factor * base).max(1.0));
    }

    trimmed.parse::<f32>().ok().map(|size| size.max(1.0))
}

fn parse_px(value: &str) -> Option<f32> {
    let trimmed = value.trim().to_ascii_lowercase();

    if let Some(raw) = trimmed.strip_suffix("px") {
        return raw.trim().parse::<f32>().ok();
    }

    trimmed.parse::<f32>().ok()
}

fn parse_viewport_fraction(value: &str, unit: &str) -> Option<f32> {
    let trimmed = value.trim().to_ascii_lowercase();
    let raw = trimmed.strip_suffix(unit)?;
    let percentage = raw.trim().parse::<f32>().ok()?;

    Some((percentage / 100.0).clamp(0.0, 1.0))
}

fn strip_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;

    loop {
        let Some(start) = rest.find("/*") else {
            output.push_str(rest);
            break;
        };

        output.push_str(&rest[..start]);

        let Some(end_rel) = rest[start + 2..].find("*/") else {
            break;
        };

        rest = &rest[start + 2 + end_rel + 2..];
    }

    output
}
