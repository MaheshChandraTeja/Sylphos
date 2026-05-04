#![allow(clippy::default_trait_access)]

use crate::text::{
    layout_text, measure_text, parse_font_weight, positioned_glyphs, shape_text, FontDatabase,
    FontDescriptor, FontMetrics, FontRequest, FontStyle, FontWeight, ScriptClass, TextAlign,
    TextDirection, TextEngine, TextOverflow, TextStyle, TextTransform, WhiteSpace,
};

#[test]
fn measures_basic_latin_text() {
    let style = TextStyle {
        font_size: 20.0,
        ..TextStyle::default()
    };

    let measure = measure_text("Sylphos", &style);

    assert!(measure.width > 40.0);
    assert!(measure.height > 20.0);
    assert!(measure.baseline > 0.0);
}

#[test]
fn measurement_cache_records_hits() {
    let mut engine = TextEngine::default();
    let style = TextStyle::default();

    let first = engine.measure_text("cache me", &style);
    let second = engine.measure_text("cache me", &style);

    assert_eq!(first, second);
    assert_eq!(engine.metrics().cache_hits, 1);
    assert_eq!(engine.metrics().cache_misses, 1);
}

#[test]
fn font_fallback_selects_cjk_and_emoji_faces() {
    let mut engine = TextEngine::default();
    let shaped = engine.shape_text("A界🙂", &TextStyle::default());

    let scripts = shaped
        .runs
        .iter()
        .flat_map(|run| run.clusters.iter().map(|cluster| cluster.script))
        .collect::<Vec<_>>();

    assert!(scripts.contains(&ScriptClass::Latin));
    assert!(scripts.contains(&ScriptClass::Cjk));
    assert!(scripts.contains(&ScriptClass::Emoji));
    assert!(shaped.runs.len() >= 2);
}

#[test]
fn line_wrapping_splits_text() {
    let mut engine = TextEngine::default();
    let style = TextStyle {
        font_size: 16.0,
        white_space: WhiteSpace::Normal,
        ..TextStyle::default()
    };

    let layout = engine.layout_text(
        "This is a long line that should wrap into multiple lines",
        &style,
        120.0,
    );

    assert!(layout.lines.len() > 1);
    assert!(layout.height > 20.0);
    assert!(engine.metrics().soft_wraps > 0);
}

#[test]
fn nowrap_keeps_single_line() {
    let style = TextStyle {
        white_space: WhiteSpace::NoWrap,
        ..TextStyle::default()
    };

    let layout = layout_text(
        "This line refuses to wrap because CSS said so",
        &style,
        80.0,
    );

    assert_eq!(layout.lines.len(), 1);
    assert!(layout.overflowed);
}

#[test]
fn pre_preserves_hard_line_breaks() {
    let style = TextStyle {
        white_space: WhiteSpace::Pre,
        ..TextStyle::default()
    };

    let layout = layout_text("one\ntwo\nthree", &style, 500.0);

    assert_eq!(layout.lines.len(), 3);
    assert_eq!(layout.lines[0].text, "one");
    assert_eq!(layout.lines[1].text, "two");
}

#[test]
fn ellipsis_is_applied_on_overflow() {
    let style = TextStyle {
        overflow: TextOverflow::Ellipsis,
        white_space: WhiteSpace::NoWrap,
        ..TextStyle::default()
    };

    let layout = layout_text("This will be clipped with an ellipsis", &style, 80.0);

    assert_eq!(layout.lines.len(), 1);
    assert!(layout.lines[0].ellipsized);
    assert!(layout.lines[0].text.ends_with('…'));
}

#[test]
fn text_transform_uppercase_changes_measurement_text() {
    let shaped = shape_text(
        "abc",
        &TextStyle {
            transform: TextTransform::Uppercase,
            ..TextStyle::default()
        },
    );

    assert_eq!(shaped.text, "ABC");
}

#[test]
fn alignment_center_offsets_line() {
    let style = TextStyle {
        align: TextAlign::Center,
        ..TextStyle::default()
    };

    let layout = layout_text("center", &style, 300.0);

    assert!(layout.lines[0].x > 0.0);
}

#[test]
fn rtl_start_alignment_offsets_to_right() {
    let style = TextStyle {
        direction: TextDirection::Rtl,
        align: TextAlign::Start,
        ..TextStyle::default()
    };

    let layout = layout_text("abc", &style, 300.0);

    assert!(layout.lines[0].x > 0.0);
}

#[test]
fn positioned_glyphs_use_line_baseline() {
    let layout = layout_text("abc", &TextStyle::default(), 200.0);
    let glyphs = positioned_glyphs(&layout);

    assert_eq!(glyphs.len(), 3);
    assert!(glyphs[0].y > 0.0);
    assert!(glyphs[1].x > glyphs[0].x);
}

#[test]
fn atlas_requests_are_deduplicated() {
    let mut engine = TextEngine::default();
    let shaped = engine.shape_text("aaa", &TextStyle::default());
    let requests = engine.atlas_requests(&shaped);

    assert_eq!(requests.len(), 1);
}

#[test]
fn custom_font_registration_is_used() {
    let mut db = FontDatabase::empty();
    let face = db.register_face(
        FontDescriptor {
            family: "Test Sans".to_owned(),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
            stretch: Default::default(),
            monospace: false,
            emoji: false,
        },
        FontMetrics {
            average_advance: 1.0,
            ..FontMetrics::default()
        },
        vec![(0x0000, 0x007F)],
        Some("test".to_owned()),
    );
    db.register_generic_family("sans-serif", ["Test Sans"]);

    let mut engine = TextEngine::new(db);
    let shaped = engine.shape_text(
        "abc",
        &TextStyle {
            font: FontRequest {
                families: vec!["Test Sans".to_owned()],
                ..FontRequest::default()
            },
            font_size: 10.0,
            ..TextStyle::default()
        },
    );

    assert_eq!(shaped.runs[0].font, face);
    assert!(shaped.advance >= 30.0);
}

#[test]
fn parses_css_font_weight() {
    assert_eq!(parse_font_weight("bold"), FontWeight::BOLD);
    assert_eq!(parse_font_weight("500"), FontWeight(500));
    assert_eq!(parse_font_weight("nonsense"), FontWeight::NORMAL);
}
