#![allow(clippy::float_cmp, clippy::needless_raw_string_hashes)]

use crate::{
    build_svg_paint_plan, builtin_icon_registry, parse_svg_lite, parse_svg_path_lite, Color,
    SvgPaintCommand, SvgPathSegment,
};

#[test]
fn parses_basic_svg_rect_circle_and_path() {
    let svg = r##"
        <svg width="32" height="32" viewBox="0 0 32 32">
            <title>Status</title>
            <rect x="2" y="2" width="28" height="28" fill="#112233"/>
            <circle cx="16" cy="16" r="6" fill="currentColor"/>
            <path d="M8 20 L14 26 L26 8 Z" fill="white"/>
        </svg>
    "##;

    let doc = parse_svg_lite(svg);
    assert_eq!(doc.viewport.width, 32.0);
    assert_eq!(doc.viewport.height, 32.0);
    assert_eq!(doc.title.as_deref(), Some("Status"));
    assert_eq!(doc.nodes.len(), 3);

    let plan = build_svg_paint_plan(&doc, Color::rgba(0.4, 0.5, 0.6, 1.0));
    assert_eq!(plan.commands.len(), 3);
    assert!(!plan.is_empty());
}

#[test]
fn parses_path_relative_and_close_commands() {
    let segments = parse_svg_path_lite("M 1 1 l 4 0 v 4 h -4 z");
    assert_eq!(segments.len(), 5);
    assert_eq!(
        segments.first().copied(),
        Some(SvgPathSegment::MoveTo(crate::SvgPoint::new(1.0, 1.0)))
    );
    assert_eq!(segments.last().copied(), Some(SvgPathSegment::Close));
}

#[test]
fn builds_current_color_icon_plan() {
    let registry = builtin_icon_registry();
    let Some(plan) = registry.paint_plan("check", Color::rgba(0.2, 0.3, 0.4, 1.0)) else {
        panic!("built-in icon missing");
    };

    assert!(!plan.commands.is_empty());
    assert!(matches!(plan.commands[0], SvgPaintCommand::FillPath { .. }));
}

#[test]
fn leaves_unknown_elements_as_diagnostics_not_failures() {
    let doc = parse_svg_lite(
        r##"<svg viewBox="0 0 24 24"><banana/><line x1="0" y1="0" x2="4" y2="4" stroke="black"/></svg>"##,
    );
    assert_eq!(doc.diagnostics.unsupported_elements, 1);
    assert_eq!(doc.nodes.len(), 1);
}
