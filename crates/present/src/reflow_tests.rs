use crate::reflow::{dirty_regions_between, ReflowMode, ReflowReason, ReflowRequest};
use crate::{
    extract_render_document, DirtyFlags, IncrementalReflowEngine, InvalidationSet, PaintCommand,
    PaintPlan,
};

fn parse_document(source: &str) -> crate::RenderDocument {
    let document = match html_mvp::parse(source) {
        Ok(document) => document,
        Err(error) => panic!("test HTML parses: {error}"),
    };
    extract_render_document(&document)
}

#[test]
fn initial_reflow_is_full_and_marks_viewport_dirty() {
    let document = parse_document("<body><h1>Hello</h1><p>World</p></body>");
    let mut engine = IncrementalReflowEngine::new();

    let output = engine.update(ReflowRequest {
        document: &document,
        width: 800.0,
        height: 600.0,
        invalidation: None,
        force_full: false,
    });

    assert_eq!(output.mode, ReflowMode::Full);
    assert_eq!(output.reason, ReflowReason::Initial);
    assert!(output.dirty_regions.is_full_repaint());
    assert!(!output.paint_plan.commands.is_empty());
}

#[test]
fn unchanged_document_reuses_previous_paint_plan() {
    let document = parse_document("<body><p>Stable text</p></body>");
    let mut engine = IncrementalReflowEngine::new();

    let first = engine.update(ReflowRequest {
        document: &document,
        width: 640.0,
        height: 480.0,
        invalidation: None,
        force_full: false,
    });
    let second = engine.update(ReflowRequest {
        document: &document,
        width: 640.0,
        height: 480.0,
        invalidation: None,
        force_full: false,
    });

    assert_eq!(second.mode, ReflowMode::Reused);
    assert!(second.dirty_regions.is_empty());
    assert_eq!(first.paint_plan, second.paint_plan);
}

#[test]
fn text_invalidation_rebuilds_layout() {
    let document = parse_document("<body><p>Changed by form typing</p></body>");
    let mut engine = IncrementalReflowEngine::new();
    let _ = engine.update(ReflowRequest {
        document: &document,
        width: 640.0,
        height: 480.0,
        invalidation: None,
        force_full: false,
    });

    let mut invalidation = InvalidationSet::default();
    invalidation.mark(crate::DomNodeId::from_raw_for_tests(7), DirtyFlags::text());

    let output = engine.update(ReflowRequest {
        document: &document,
        width: 640.0,
        height: 480.0,
        invalidation: Some(&invalidation),
        force_full: false,
    });

    assert!(matches!(
        output.mode,
        ReflowMode::Full | ReflowMode::PaintOnly
    ));
    assert!(!output.paint_plan.commands.is_empty());
}

#[test]
fn command_diff_returns_precise_dirty_regions() {
    let previous = PaintPlan {
        background: crate::Color::white(),
        commands: vec![PaintCommand::TextPlaceholder {
            x: 10.0,
            y: 10.0,
            text: "Old".to_owned(),
            size: 16.0,
            color: crate::Color::black(),
        }],
    };
    let current = PaintPlan {
        background: crate::Color::white(),
        commands: vec![PaintCommand::TextPlaceholder {
            x: 10.0,
            y: 10.0,
            text: "New".to_owned(),
            size: 16.0,
            color: crate::Color::black(),
        }],
    };

    let Some(dirty) = dirty_regions_between(Some(&previous), &current, 800.0, 600.0) else {
        panic!("changed text creates dirty region");
    };

    assert!(!dirty.is_full_repaint());
    assert!(!dirty.regions().is_empty());
}
