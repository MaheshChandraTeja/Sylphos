#![allow(clippy::float_cmp, clippy::needless_raw_string_hashes)]

use crate::{
    build_paint_plan, collect_form_control_hit_regions, collect_link_hit_regions,
    edit_form_control, extract_render_document, extract_style_sources, extract_stylesheet_links,
    focus_form_control, form_submission_pairs, hit_test_form_control, hit_test_link,
    layout_document, parse_css_lite, Color, DisplayLite, FormControlKind, FormTextEdit,
    LayoutBoxKind, PaintCommand, RenderBlock, StyleSourceLite,
};

fn parse_document(source: &str) -> html_mvp::Document {
    match html_mvp::parse(source) {
        Ok(document) => document,
        Err(error) => panic!("test HTML failed to parse: {error}"),
    }
}

fn assert_color_close(actual: Color, expected: Color) {
    const EPSILON: f32 = 0.000_01;
    assert!((actual.r - expected.r).abs() < EPSILON, "red mismatch");
    assert!((actual.g - expected.g).abs() < EPSILON, "green mismatch");
    assert!((actual.b - expected.b).abs() < EPSILON, "blue mismatch");
    assert!((actual.a - expected.a).abs() < EPSILON, "alpha mismatch");
}

#[test]
fn parses_short_and_long_css_hex_color() {
    let Some(short) = Color::from_css_hex("#369") else {
        panic!("expected valid #rgb color");
    };
    let Some(long) = Color::from_css_hex("#336699") else {
        panic!("expected valid #rrggbb color");
    };

    let expected = Color::rgba(
        f32::from(0x33_u8) / 255.0,
        f32::from(0x66_u8) / 255.0,
        f32::from(0x99_u8) / 255.0,
        1.0,
    );

    assert_color_close(short, expected);
    assert_color_close(long, expected);
    assert!(Color::from_css_hex("rgb(1,2,3)").is_none());
}

#[test]
fn css_lite_parses_box_model_properties() {
    let css = r#"
        body { background: #eee; width: 60vw; margin: 15vh auto; padding: 8px 12px; }
        h1 { font-size: 1.5em; margin-bottom: 10px; padding: 4px; border: 2px solid #336699; }
        p { margin: 0 0 12px 0; padding-left: 6px; }
        a:link, a:visited { color: #348; }
        img { display: none; }
        input { padding: 6px 8px; border-width: thin; border-color: #999; }
    "#;

    let sheet = parse_css_lite(css);

    assert!(sheet.body_background.is_some());
    assert!(sheet.link_color.is_some());
    assert!(sheet.heading_sizes[0].is_some());
    assert!(sheet.center_horizontally);
    assert_eq!(sheet.box_model.image.display, Some(DisplayLite::None));
    let Some(body_padding) = sheet.box_model.body.padding else {
        panic!("body padding");
    };
    let Some(h1_border) = sheet.box_model.headings[0].border_width else {
        panic!("h1 border");
    };
    let Some(input_padding) = sheet.box_model.input.padding else {
        panic!("input padding");
    };
    assert_eq!(body_padding.top, 8.0);
    assert_eq!(body_padding.right, 12.0);
    assert_eq!(h1_border.top, 2.0);
    assert_eq!(input_padding.left, 8.0);
}

#[test]
fn extracts_title_style_and_basic_blocks() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <title>Sylphos Test Page</title>
                <meta name="theme-color" content="#336699">
                <style>body { background: #eee; }</style>
            </head>
            <body>
                <h1>Hello</h1>
                <p>This is Sylphos.</p>
                <p><a href="https://kairais.com">Kairais</a></p>
                <img src="/hero.png" alt="Hero">
            </body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);

    assert_eq!(render_document.title, Some("Sylphos Test Page".to_owned()));
    assert!(render_document.theme_color.is_some());
    assert!(render_document.style_sheet.body_background.is_some());
    assert_eq!(render_document.blocks.len(), 4);

    match render_document.blocks.as_slice() {
        [RenderBlock::Heading { level, text }, RenderBlock::Paragraph { text: paragraph }, RenderBlock::Link {
            text: link_text,
            href,
        }, RenderBlock::Image { alt, src }] => {
            assert_eq!(*level, 1);
            assert_eq!(text, "Hello");
            assert_eq!(paragraph, "This is Sylphos.");
            assert_eq!(link_text, "Kairais");
            assert_eq!(href.as_deref(), Some("https://kairais.com"));
            assert_eq!(alt.as_deref(), Some("Hero"));
            assert_eq!(src.as_deref(), Some("/hero.png"));
        }
        other => panic!("unexpected render blocks: {other:?}"),
    }
}

#[test]
fn layout_applies_box_model_to_heading_and_paragraph() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>
                    body { background: #eee; width: 60vw; margin: 15vh auto; }
                    h1 { font-size: 1.5em; margin-bottom: 10px; padding: 4px 8px; border: 2px solid #336699; }
                    p { margin: 0 0 12px 0; padding-left: 6px; }
                </style>
            </head>
            <body>
                <h1>Example Domain</h1>
                <p>This domain is for use in documentation examples.</p>
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
    assert_eq!(layout.boxes.len(), 2);

    let heading = &layout.boxes[0];
    match &heading.kind {
        LayoutBoxKind::Heading { level } => assert_eq!(*level, 1),
        other => panic!("expected heading, got {other:?}"),
    }
    assert!(heading.border.is_some());
    assert_eq!(heading.rect.x, 200.0);
    assert_eq!(heading.text_runs[0].x, 210.0);
    assert_eq!(heading.margin_after, 10.0);

    let paragraph = &layout.boxes[1];
    assert!(matches!(paragraph.kind, LayoutBoxKind::Paragraph));
    assert_eq!(paragraph.text_runs[0].x, 206.0);
    assert_eq!(paragraph.margin_after, 12.0);
}

#[test]
fn display_none_omits_images_from_layout_and_paint() {
    let document = parse_document(
        r##"
        <html>
            <head><style>img { display: none; }</style></head>
            <body><img src="/hero.png" alt="Hero"><p>Visible</p></body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let layout = layout_document(&render_document, 800.0, 600.0);
    let plan = build_paint_plan(&render_document, 800.0, 600.0);

    assert_eq!(layout.boxes.len(), 1);
    assert!(matches!(layout.boxes[0].kind, LayoutBoxKind::Paragraph));
    assert!(!plan
        .commands
        .iter()
        .any(|command| matches!(command, PaintCommand::Image { .. })));
}

#[test]
fn paint_plan_draws_border_before_text() {
    let document = parse_document(
        r##"
        <html>
            <head><style>h1 { border: 2px solid #336699; padding: 4px; }</style></head>
            <body><h1>Hello</h1></body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let plan = build_paint_plan(&render_document, 800.0, 600.0);

    assert!(plan.commands.iter().any(|command| matches!(
        command,
        PaintCommand::Rect { color, .. } if *color == Color::rgba(0.2, 0.4, 0.6, 1.0)
    )));
    assert!(plan.commands.iter().any(|command| matches!(
        command,
        PaintCommand::TextPlaceholder { text, .. } if text == "Hello"
    )));
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

    let result = hit_test_link(
        &render_document,
        1000.0,
        800.0,
        regions[0].rect.x + 1.0,
        regions[0].rect.y + 1.0,
    );

    assert!(result.is_some());
}

#[test]
fn forms_extract_layout_edit_and_submit() {
    let document = parse_document(
        r#"
        <body>
            <form action="/search" method="get">
                <input type="search" name="q" placeholder="Search">
                <input type="submit" value="Go">
            </form>
        </body>
        "#,
    );

    let mut render_document = extract_render_document(&document);
    let regions = collect_form_control_hit_regions(&render_document, 800.0, 600.0);

    assert_eq!(regions.len(), 2);
    let search = regions
        .iter()
        .find(|region| region.kind == FormControlKind::Search);
    let Some(search) = search else {
        panic!("search input region");
    };

    assert!(hit_test_form_control(
        &render_document,
        800.0,
        600.0,
        search.rect.x + 2.0,
        search.rect.y + 2.0,
    )
    .is_some());

    assert!(focus_form_control(
        &mut render_document,
        Some(search.control_id)
    ));
    assert!(edit_form_control(
        &mut render_document,
        search.control_id,
        FormTextEdit::Insert("rust".to_owned())
    ));

    let pairs = form_submission_pairs(&render_document, search.form_id, None);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].name, "q");
    assert_eq!(pairs[0].value, "rust");
}

#[test]
fn paint_plan_is_deterministic() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>
                    body { background: #eee; }
                    p { margin-bottom: 8px; padding: 4px; border: 1px solid #999; }
                </style>
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
fn extracts_external_stylesheet_links_in_source_order() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <link rel="stylesheet" href="/base.css">
                <style>body { background: #eee; }</style>
                <link rel="alternate stylesheet" href="/alternate.css">
                <link rel="stylesheet" media="print" href="/print.css">
                <link rel="stylesheet" media="screen" href="/screen.css">
            </head>
            <body><p>Hello</p></body>
        </html>
        "##,
    );

    let links = extract_stylesheet_links(&document);
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].href, "/base.css");
    assert_eq!(links[1].href, "/print.css");
    assert_eq!(links[2].href, "/screen.css");
    assert!(!links[1].applies_to_screen());
    assert!(links[2].applies_to_screen());

    let sources = extract_style_sources(&document);
    assert_eq!(sources.len(), 4);
    assert!(matches!(sources[0], StyleSourceLite::External(_)));
    assert!(matches!(sources[1], StyleSourceLite::Inline { .. }));
}

#[test]
fn render_document_keeps_stylesheet_sources_for_browser_loader() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <link rel="stylesheet" href="/base.css">
                <style>h1 { font-size: 24px; }</style>
            </head>
            <body><h1>Hello</h1></body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);

    assert_eq!(render_document.external_stylesheets.len(), 1);
    assert_eq!(render_document.external_stylesheets[0].href, "/base.css");
    assert_eq!(render_document.style_sources.len(), 2);
    assert!(render_document.style_sheet.heading_sizes[0].is_some());
}

#[test]
fn selector_matching_applies_class_styles_to_matching_blocks() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>
                    .hero { color: #123456; background: #eeeeee; padding: 10px; }
                </style>
            </head>
            <body>
                <p class="hero">Styled paragraph</p>
            </body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    assert_eq!(render_document.style_tree.rule_count, 1);
    assert_eq!(render_document.style_tree.nodes.len(), 1);
    assert_eq!(
        render_document.block_elements[0].classes,
        vec!["hero".to_owned()]
    );

    let plan = build_paint_plan(&render_document, 800.0, 600.0);

    assert!(plan.commands.iter().any(|command| matches!(
        command,
        PaintCommand::TextPlaceholder { text, color, .. }
            if text == "Styled paragraph" && *color == Color::rgba(
                f32::from(0x12_u8) / 255.0,
                f32::from(0x34_u8) / 255.0,
                f32::from(0x56_u8) / 255.0,
                1.0,
            )
    )));
}

#[test]
fn selector_specificity_prefers_id_over_class() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>
                    .note { color: #111111; }
                    #important { color: #333333; }
                </style>
            </head>
            <body>
                <p id="important" class="note">Specific text</p>
            </body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let plan = build_paint_plan(&render_document, 800.0, 600.0);

    assert!(plan.commands.iter().any(|command| matches!(
        command,
        PaintCommand::TextPlaceholder { text, color, .. }
            if text == "Specific text" && *color == Color::rgba(
                f32::from(0x33_u8) / 255.0,
                f32::from(0x33_u8) / 255.0,
                f32::from(0x33_u8) / 255.0,
                1.0,
            )
    )));
}

#[test]
fn descendant_selector_can_hide_matching_block() {
    let document = parse_document(
        r##"
        <html>
            <head>
                <style>
                    div .gone { display: none; }
                </style>
            </head>
            <body>
                <div><p class="gone">Invisible text</p></div>
                <p>Visible text</p>
            </body>
        </html>
        "##,
    );

    let render_document = extract_render_document(&document);
    let layout = layout_document(&render_document, 800.0, 600.0);

    assert!(layout.boxes.iter().all(|layout_box| {
        layout_box
            .text_runs
            .iter()
            .all(|run| run.text != "Invisible text")
    }));
    assert!(layout.boxes.iter().any(|layout_box| {
        layout_box
            .text_runs
            .iter()
            .any(|run| run.text == "Visible text")
    }));
}

#[test]
fn extracts_mixed_inline_flow_without_losing_links() {
    let document = parse_document(
        r##"
        <body>
            <p>Read the <a href="/guide">guide</a> before continuing.</p>
        </body>
        "##,
    );

    let render_document = extract_render_document(&document);

    match render_document.blocks.as_slice() {
        [RenderBlock::InlineFlow { fragments }] => {
            assert_eq!(
                crate::InlineFragment::plain_text(fragments),
                "Read the guide before continuing."
            );
            assert!(crate::InlineFragment::contains_link(fragments));
        }
        other => panic!("expected inline-flow block, got {other:?}"),
    }
}

#[test]
fn inline_flow_layout_keeps_link_hit_regions_inside_paragraphs() {
    let document = parse_document(
        r##"
        <body>
            <p>Read the <a href="/guide">guide</a> before continuing.</p>
        </body>
        "##,
    );

    let render_document = extract_render_document(&document);
    let regions = collect_link_hit_regions(&render_document, 800.0, 600.0);

    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].href, "/guide");
    assert_eq!(regions[0].text, "guide");

    let hit = hit_test_link(
        &render_document,
        800.0,
        600.0,
        regions[0].rect.x + 1.0,
        regions[0].rect.y + 1.0,
    );

    let Some(hit) = hit else {
        panic!("expected inline link hit result");
    };

    assert_eq!(hit.href, "/guide");
    assert_eq!(hit.text, "guide");
}

#[test]
fn inline_flow_wraps_across_fragments() {
    let document = parse_document(
        r##"
        <body>
            <p>Alpha beta <a href="/gamma">gamma delta epsilon</a> zeta eta theta iota.</p>
        </body>
        "##,
    );

    let render_document = extract_render_document(&document);
    let layout = layout_document(&render_document, 180.0, 600.0);

    assert_eq!(layout.boxes.len(), 1);
    assert!(layout.boxes[0].text_runs.len() > 3);
    assert!(layout.boxes[0]
        .text_runs
        .iter()
        .any(|run| run.href.as_deref() == Some("/gamma")));
}
