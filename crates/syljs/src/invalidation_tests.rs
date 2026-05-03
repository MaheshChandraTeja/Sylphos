use std::{cell::RefCell, rc::Rc};

use crate::{
    collect_cssom_mutation_invalidations, collect_dom_snapshot_invalidations,
    run_research_script_pipeline, CssStyleMutation, DirtyFlag, DirtyFlags, DomNodeRef,
    DomNodeSnapshot, DomNodeType, InvalidationEngine, InvalidationImpact, InvalidationInput,
    InvalidationPriority, InvalidationReason, InvalidationScope, InvalidationSource,
    LayoutInvalidationKind, PaintInvalidationKind, RebuildHint, ResearchInvalidationHooks,
    ResearchScriptResourceLoader, ReflowRequest, ScheduledVm, ScriptDescriptor,
    ScriptPipelineConfig, StyleInvalidationKind, StyleInvalidationKindLite,
};

#[test]
fn reflow_request_becomes_full_pipeline_plan() {
    let mut dirty = DirtyFlags::default();
    dirty.insert(DirtyFlag::Dom);
    dirty.insert(DirtyFlag::Style);
    dirty.insert(DirtyFlag::Layout);
    dirty.insert(DirtyFlag::Paint);

    let request = ReflowRequest {
        script_id: Some(7),
        label: "script:7".to_owned(),
        dirty,
        reason: "script-complete".to_owned(),
    };

    let mut engine = InvalidationEngine::default();
    engine.consume_reflow_request(&request);
    let plan = engine.flush_plan();

    assert_eq!(plan.input_count, 1);
    assert!(plan.restyle_document);
    assert!(plan.rebuild_layout_tree);
    assert!(plan.rebuild_paint_plan);
    assert!(plan.full_viewport_paint);
    assert_eq!(plan.rebuild_hint, RebuildHint::FullPipeline);
    assert_eq!(engine.metrics().reflow_requests_consumed, 1);
}

#[test]
fn cssom_paint_mutation_becomes_paint_plan_rebuild() {
    let mutations = vec![CssStyleMutation {
        node: Some(DomNodeRef(10)),
        property: "color".to_owned(),
        value: Some("red".to_owned()),
        invalidation: StyleInvalidationKind::Paint,
    }];

    let inputs = collect_cssom_mutation_invalidations(&mutations);

    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].scope, InvalidationScope::Node(DomNodeRef(10)));
    assert_eq!(inputs[0].impact.paint, PaintInvalidationKind::Node);

    let mut engine = InvalidationEngine::default();
    engine.submit_many(inputs);
    let plan = engine.flush_plan();

    assert_eq!(plan.paint_nodes, vec![DomNodeRef(10)]);
    assert!(plan.rebuild_paint_plan);
    assert!(!plan.rebuild_layout_tree);
}

#[test]
fn cssom_layout_mutation_becomes_layout_plan() {
    let mutations = vec![CssStyleMutation {
        node: Some(DomNodeRef(11)),
        property: "width".to_owned(),
        value: Some("500px".to_owned()),
        invalidation: StyleInvalidationKind::Layout,
    }];

    let mut engine = InvalidationEngine::default();
    engine.consume_cssom_mutations(&mutations);
    let plan = engine.flush_plan();

    assert_eq!(plan.layout_nodes, vec![DomNodeRef(11)]);
    assert_eq!(plan.priority, InvalidationPriority::High);
    assert!(plan.rebuild_paint_plan);
    assert_eq!(engine.metrics().cssom_mutations_consumed, 1);
}

#[test]
fn selector_recalc_mutation_upgrades_to_layout_tree() {
    let mutations = vec![CssStyleMutation {
        node: Some(DomNodeRef(12)),
        property: "class".to_owned(),
        value: Some("active".to_owned()),
        invalidation: StyleInvalidationKind::StyleRecalc,
    }];

    let mut engine = InvalidationEngine::default();
    engine.consume_cssom_mutations(&mutations);
    let plan = engine.flush_plan();

    assert_eq!(plan.style_nodes, vec![DomNodeRef(12)]);
    assert_eq!(plan.layout_nodes, vec![DomNodeRef(12)]);
    assert!(plan.rebuild_layout_tree);
    assert!(plan.rebuild_paint_plan);
}

#[test]
fn dom_snapshots_create_subtree_invalidations() {
    let snapshots = vec![DomNodeSnapshot {
        id: DomNodeRef(1),
        parent: None,
        children: vec![DomNodeRef(2)],
        node_type: DomNodeType::Element,
        tag_name: "div".to_owned(),
        text: String::new(),
        attributes: Default::default(),
    }];

    let inputs = collect_dom_snapshot_invalidations(&snapshots);

    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].scope, InvalidationScope::Subtree(DomNodeRef(1)));
    assert_eq!(inputs[0].impact.style, StyleInvalidationKindLite::SelectorMatch);
}

#[test]
fn engine_coalesces_multiple_node_inputs_into_document_when_unrelated() {
    let mut engine = InvalidationEngine::default();

    engine.submit(InvalidationInput {
        source: InvalidationSource::Cssom,
        priority: InvalidationPriority::Normal,
        scope: InvalidationScope::Node(DomNodeRef(1)),
        impact: InvalidationImpact::paint(),
        rect: None,
        reason: InvalidationReason::new("a", "a"),
    });

    engine.submit(InvalidationInput {
        source: InvalidationSource::Cssom,
        priority: InvalidationPriority::High,
        scope: InvalidationScope::Node(DomNodeRef(2)),
        impact: InvalidationImpact::layout_paint(),
        rect: None,
        reason: InvalidationReason::new("b", "b"),
    });

    let plan = engine.flush_plan();

    assert_eq!(plan.priority, InvalidationPriority::High);
    assert!(plan.rebuild_layout_tree);
    assert!(plan.full_viewport_paint);
    assert!(plan.reasons.contains(&"a".to_owned()));
    assert!(plan.reasons.contains(&"b".to_owned()));
}

#[test]
fn invalidation_hooks_receive_script_pipeline_reflow_requests() {
    let loader = Rc::new(ResearchScriptResourceLoader::default());
    let engine = Rc::new(RefCell::new(InvalidationEngine::default()));
    let hooks = Rc::new(ResearchInvalidationHooks::new(engine.clone()));
    let mut scheduled = ScheduledVm::default();

    let run = run_research_script_pipeline(
        &mut scheduled,
        vec![ScriptDescriptor::inline(1, r#"console.log("ok");"#)],
        loader,
        hooks,
        ScriptPipelineConfig::default(),
    )
    .expect("pipeline");

    assert_eq!(run.metrics.scripts_executed, 1);
    assert!(engine.borrow().queued_len() >= 1);

    let plan = engine.borrow_mut().flush_plan();
    assert!(plan.rebuild_paint_plan);
    assert!(plan.rebuild_layout_tree || plan.full_viewport_paint);
}
