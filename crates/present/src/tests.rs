#![allow(clippy::float_cmp, clippy::needless_raw_string_hashes)]

use crate::{
    build_paint_plan, collect_link_hit_regions, extract_render_document, extract_style_sheet,
    extract_theme_color, hit_test_link, layout_document, parse_css_lite, Color, LayoutBoxKind,
    PaintCommand, RenderBlock,
};

fn parse_document(source: &str) -> html_mvp::Document {
    match html_mvp::parse(source) {
        Ok(document) => document,
        Err(error) => panic!("test HTML failed to parse: {error}"),
    }
}

fn assert_color_close(actual: Color, expected: Color) {
    const EPSILON: f32 = 0.000_01;

    assert!(
        (actual.r - expected.r).abs() < EPSILON,
        "red mismatch: actual={} expected={}",
        actual.r,
        expected.r
    );
    assert!(
        (actual.g - expected.g).abs() < EPSILON,
        "green mismatch: actual={} expected={}",
        actual.g,
        expected.g
    );
    assert!(
        (actual.b - expected.b).abs() < EPSILON,
        "blue mismatch: actual={} expected={}",
        actual.b,
        expected.b
    );
    assert!(
        (actual.a - expected.a).abs() < EPSILON,
        "alpha mismatch: actual={} expected={}",
        actual.a,
        expected.a
    );
}

#[test]
fn parses_short_css_hex_color() {
    let Some(color) = Color::from_css_hex("#369") else {
        panic!("expected valid #rgb color");
    };

    assert_color_close(
        color,
        Color::rgba(
            f32::from(0x33_u8) / 255.0,
            f32::from(0x66_u8) / 255.0,
            f32::from(0x99_u8) / 255.0,
            1.0,
        ),
    );
}

#[test]
fn parses_long_css_hex_color() {
    let Some(color) = Color::from_css_hex("#336699") else {
        panic!("expected valid #rrggbb color");
    };

    assert_color_close(
        color,
        Color::rgba(
            f32::from(0x33_u8) / 255.0,
            f32::from(0x66_u8) / 255.0,
            f32::from(0x99_u8) / 255.0,
            1.0,
        ),
    );
}

#[test]
fn rejects_unsupported_color_formats() {
    assert!(Color::from_css_hex("336699").is_none());
    assert!(Color::from_css_hex("#12").is_none());
    assert!(Color::from_css_hex("#12345").is_none());
    assert!(Color::from_css_hex("rgb(1, 2, 3)").is_none());
    assert!(Color::from_css_hex("red").is_none());
}

#[test]
fn css_lite_parses_example_com_style() {
    let css = r#"
        body{background:#eee;width:60vw;margin:15vh auto;font-family:system-ui,sans-serif}
        h1{font-size:1.5em}
        div{opacity:0.8}
        a:link,a:visited{color:#348}
    "#;

    let sheet = parse_css_lite(css);

    let Some(background) = sheet.body_background else {
        panic!("expected body background");
    };
    let Some(link_color) = sheet.link_color else {
        panic!("expected link color");
    };
    let Some(h1_size) = sheet.heading_sizes[0] else {
        panic!("expected h1 font size");
    };
    let Some(width_fraction) = sheet.content_width_fraction else {
        panic!("expected content width fraction");
    };
    let Some(margin_top_fraction) = sheet.margin_top_fraction else {
        panic!("expected margin top fraction");
    };

    assert_color_close(
        background,
        Color::rgba(0.933_333_34, 0.933_333_34, 0.933_333_34, 1.0),
    );
    assert_color_close(
        link_color,
        Color::rgba(0.2, 0.266_666_68, 0.533_333_36, 1.0),
    );
    assert!((h1_size - 24.0).abs() < 0.000_01);
    assert!((width_fraction - 0.60).abs() < 0.000_01);
    assert!((margin_top_fraction - 0.15).abs() < 0.000_01);
    assert!(sheet.center_horizontally);
}

#[test]
fn extracts_theme_color() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <meta name="theme-color" content="#336699">
            </head>
        </html>
        "##,
    );

    let Some(color) = extract_theme_color(&document) else {
        panic!("expected theme color");
    };

    assert_color_close(
        color,
        Color::rgba(
            f32::from(0x33_u8) / 255.0,
            f32::from(0x66_u8) / 255.0,
            f32::from(0x99_u8) / 255.0,
            1.0,
        ),
    );
}

#[test]
fn extracts_inline_style_sheet() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>
                    body { background: #eee; width: 60vw; margin: 15vh auto; }
                    a:link, a:visited { color: #348; }
                </style>
            </head>
        </html>
        "##,
    );

    let sheet = extract_style_sheet(&document);

    assert!(sheet.body_background.is_some());
    assert!(sheet.link_color.is_some());
    assert!(sheet.center_horizontally);
}

#[test]
fn extracts_title() {
    let document = parse_document(
        r#"
        <html>
            <head>
                <title>Sylphos Test Page</title>
            </head>
        </html>
        "#,
    );

    let render_document = extract_render_document(&document);

    assert_eq!(render_document.title, Some("Sylphos Test Page".to_owned()));
}

#[test]
fn extracts_heading_and_paragraph() {
    let document = parse_document(
        r#"
        <body>
            <h1>Hello</h1>
            <p>This is Sylphos.</p>
        </body>
        "#,
    );

    let render_document = extract_render_document(&document);

    match render_document.blocks.as_slice() {
        [RenderBlock::Heading { level, text }, RenderBlock::Paragraph { text: paragraph }] => {
            assert_eq!(*level, 1);
            assert_eq!(text, "Hello");
            assert_eq!(paragraph, "This is Sylphos.");
        }
        other => panic!("unexpected render blocks: {other:?}"),
    }
}

#[test]
fn extracts_standalone_link_from_paragraph() {
    let document = parse_document(
        r#"
        <body>
            <p><a href="https://kairais.com">Kairais</a></p>
        </body>
        "#,
    );

    let render_document = extract_render_document(&document);

    match render_document.blocks.as_slice() {
        [RenderBlock::Link { text, href }] => {
            assert_eq!(text, "Kairais");
            assert_eq!(href.as_deref(), Some("https://kairais.com"));
        }
        other => panic!("unexpected render blocks: {other:?}"),
    }
}

#[test]
fn extracts_image_placeholder_data() {
    let document = parse_document(
        r#"
        <body>
            <img src="/hero.png" alt="Hero">
        </body>
        "#,
    );

    let render_document = extract_render_document(&document);

    match render_document.blocks.as_slice() {
        [RenderBlock::Image { alt, src }] => {
            assert_eq!(alt.as_deref(), Some("Hero"));
            assert_eq!(src.as_deref(), Some("/hero.png"));
        }
        other => panic!("unexpected render blocks: {other:?}"),
    }
}

#[test]
fn ignores_script_style_and_head_content_for_visible_blocks() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <title>Visible Title</title>
                <style>body { color: red; }</style>
                <script>destroyAllHumans()</script>
            </head>
            <body>
                <p>Visible paragraph.</p>
                <script>invisible()</script>
                <style>.x {}</style>
            </body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);

    assert_eq!(render_document.title, Some("Visible Title".to_owned()));
    assert_eq!(render_document.blocks.len(), 1);

    match render_document.blocks.as_slice() {
        [RenderBlock::Paragraph { text }] => {
            assert_eq!(text, "Visible paragraph.");
        }
        other => panic!("unexpected render blocks: {other:?}"),
    }
}

#[test]
fn extracts_legacy_container_text_without_tag_noise() {
    let document = parse_document(
        r#"
        <body>
            <center>&#71;&#111;&#111;&#103;&#108;&#101;&nbsp;<font>Search</font></center>
            <form><input name="q"><input type="submit" value="Google Search"></form>
        </body>
        "#,
    );

    let render_document = extract_render_document(&document);

    match render_document.blocks.as_slice() {
        [RenderBlock::Paragraph { text }] => {
            assert_eq!(text, "Google Search");
        }
        other => panic!("unexpected render blocks: {other:?}"),
    }
}

#[test]
fn layout_uses_example_com_viewport_rules() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>
                    body { background: #eee; width: 60vw; margin: 15vh auto; color: #111; }
                    h1 { font-size: 1.5em; }
                    a:link, a:visited { color: #348; }
                </style>
            </head>
            <body>
                <div>
                    <h1>Example Domain</h1>
                    <p>This domain is for use in documentation examples without needing permission.</p>
                    <p><a href="https://iana.org/domains/example">Learn more</a></p>
                </div>
            </body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let layout = layout_document(&render_document, 1000.0, 800.0);

    assert_color_close(
        layout.background,
        Color::rgba(0.933_333_34, 0.933_333_34, 0.933_333_34, 1.0),
    );
    assert!((layout.content_rect.x - 200.0).abs() < 0.000_01);
    assert!((layout.content_rect.y - 120.0).abs() < 0.000_01);
    assert!((layout.content_rect.width - 600.0).abs() < 0.000_01);
    assert_eq!(layout.boxes.len(), 3);

    match &layout.boxes[0].kind {
        LayoutBoxKind::Heading { level } => assert_eq!(*level, 1),
        other => panic!("expected heading layout box, got {other:?}"),
    }

    assert_eq!(layout.boxes[0].text_runs[0].text, "Example Domain");
    assert!((layout.boxes[0].text_runs[0].size - 24.0).abs() < 0.000_01);
}

#[test]
fn layout_wraps_long_text_into_multiple_runs() {
    let document = parse_document(
        r#"
        <body>
            <p>Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu.</p>
        </body>
        "#,
    );

    let render_document = extract_render_document(&document);
    let layout = layout_document(&render_document, 220.0, 400.0);

    assert_eq!(layout.boxes.len(), 1);
    assert!(layout.boxes[0].text_runs.len() > 1);
}

#[test]
fn paint_plan_is_generated_from_layout_lines() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>
                    body { background: #eee; width: 60vw; margin: 15vh auto; color: #111; }
                    h1 { font-size: 1.5em; }
                    a:link, a:visited { color: #348; }
                </style>
            </head>
            <body>
                <div>
                    <h1>Example Domain</h1>
                    <p>This domain is for use in documentation examples without needing permission.</p>
                    <p><a href="https://iana.org/domains/example">Learn more</a></p>
                </div>
            </body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let plan = build_paint_plan(&render_document, 1000.0, 800.0);

    assert_color_close(
        plan.background,
        Color::rgba(0.933_333_34, 0.933_333_34, 0.933_333_34, 1.0),
    );
    assert!(plan.commands.len() >= 4);

    match &plan.commands[0] {
        PaintCommand::Rect {
            color,
            width,
            height,
            ..
        } => {
            assert_color_close(*color, plan.background);
            assert!((*width - 1000.0).abs() < 0.000_01);
            assert!((*height - 800.0).abs() < 0.000_01);
        }
        other => panic!("expected background rect, got {other:?}"),
    }

    assert!(plan.commands.iter().any(|command| matches!(
        command,
        PaintCommand::TextPlaceholder { text, .. } if text == "Example Domain"
    )));
    assert!(plan.commands.iter().any(|command| matches!(
        command,
        PaintCommand::TextPlaceholder { text, color, .. }
            if text == "Learn more" && *color == Color::rgba(0.2, 0.266_666_68, 0.533_333_36, 1.0)
    )));
}

#[test]
fn layout_changes_when_viewport_changes() {
    let document = parse_document(
        r##"
        <html>
            <head><style>body { width: 60vw; margin: 15vh auto; }</style></head>
            <body><p>Viewport sensitive text.</p></body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let small = layout_document(&render_document, 500.0, 400.0);
    let large = layout_document(&render_document, 1000.0, 800.0);

    assert_ne!(small.content_rect.x, large.content_rect.x);
    assert_ne!(small.content_rect.y, large.content_rect.y);
    assert_ne!(small.content_rect.width, large.content_rect.width);
}

#[test]
fn paint_plan_is_deterministic() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>body { background: #eee; }</style>
            </head>
            <body>
                <h1>Hello</h1>
                <p>This is Sylphos.</p>
                <a href="https://kairais.com">Kairais</a>
            </body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);

    let first = build_paint_plan(&render_document, 800.0, 600.0);
    let second = build_paint_plan(&render_document, 800.0, 600.0);

    assert_eq!(first, second);
    assert!(first.commands.len() >= 4);
}

#[test]
fn link_hit_regions_track_laid_out_links() {
    let document = parse_document(
        r##"
        <html>
            <head><style>body { width: 60vw; margin: 15vh auto; }</style></head>
            <body><p><a href="https://iana.org/domains/example">Learn more</a></p></body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let regions = collect_link_hit_regions(&render_document, 1000.0, 800.0);

    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].href, "https://iana.org/domains/example");
    assert_eq!(regions[0].text, "Learn more");
    assert!(regions[0].rect.width > 0.0);
    assert!(regions[0].rect.height > 0.0);
}

#[test]
fn link_hit_test_returns_matching_link() {
    let document = parse_document(
        r##"
        <html>
            <head><style>body { width: 60vw; margin: 15vh auto; }</style></head>
            <body><p><a href="/domains/example">Learn more</a></p></body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let regions = collect_link_hit_regions(&render_document, 1000.0, 800.0);
    let Some(region) = regions.first() else {
        panic!("expected link hit region");
    };

    let result = hit_test_link(
        &render_document,
        1000.0,
        800.0,
        region.rect.x + 1.0,
        region.rect.y + 1.0,
    );

    let Some(result) = result else {
        panic!("expected hit-test result");
    };

    assert_eq!(result.href, "/domains/example");
    assert_eq!(result.text, "Learn more");
}

#[test]
fn image_blocks_emit_image_paint_commands() {
    let document = parse_document(
        r#"
        <body>
            <img src="/hero.png" alt="Hero">
        </body>
        "#,
    );

    let render_document = extract_render_document(&document);
    let plan = build_paint_plan(&render_document, 800.0, 600.0);

    assert!(plan.commands.iter().any(|command| matches!(
        command,
        PaintCommand::Image {
            src: Some(src),
            alt: Some(alt),
            ..
        } if src == "/hero.png" && alt == "Hero"
    )));
}
