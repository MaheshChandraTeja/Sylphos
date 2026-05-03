use crate::events::dispatch_dom_event;
use crate::{
    DomEvent, DomEventKind, DomEventPayload, DomRuntime, FormBlock, FormControl, FormControlKind,
    FormMethod, RenderBlock, RenderDocument,
};

fn document_with_form() -> RenderDocument {
    let mut doc = RenderDocument::new();
    doc.blocks.push(RenderBlock::Form(FormBlock {
        id: 7,
        action: Some("/search".to_owned()),
        method: FormMethod::Get,
        controls: vec![FormControl {
            id: 11,
            kind: FormControlKind::Search,
            name: Some("q".to_owned()),
            value: String::new(),
            placeholder: Some("Search".to_owned()),
            label: None,
            disabled: false,
            focused: false,
        }],
    }));
    doc
}

#[test]
fn dom_runtime_builds_nodes_from_render_document() {
    let mut doc = RenderDocument::new();
    doc.blocks.push(RenderBlock::Heading {
        level: 1,
        text: "Hello".to_owned(),
    });
    doc.blocks.push(RenderBlock::Link {
        text: "More".to_owned(),
        href: Some("/more".to_owned()),
    });

    let runtime = DomRuntime::from_render_document(&doc);
    assert!(runtime.nodes().count() >= 3);
    assert!(runtime.node_for_link("/more", "More").is_some());
}

#[test]
fn dom_runtime_tracks_form_value_and_focus() {
    let mut doc = document_with_form();
    let mut runtime = DomRuntime::from_render_document(&doc);

    assert!(runtime.focus_control(Some(11)));
    assert!(runtime.set_form_value(11, "sylphos"));
    runtime.apply_to_render_document(&mut doc);

    let RenderBlock::Form(form) = &doc.blocks[0] else {
        panic!("expected form block");
    };
    assert_eq!(form.controls[0].value, "sylphos");
    assert!(form.controls[0].focused);
    assert!(runtime.invalidation().is_dirty());
}

#[test]
fn event_dispatch_queues_script_hook_and_default_action() {
    let doc = document_with_form();
    let mut runtime = DomRuntime::from_render_document(&doc);
    let Some(node_id) = runtime.node_for_control(11) else {
        panic!("expected control node");
    };

    let result = dispatch_dom_event(
        &mut runtime,
        DomEvent::new(
            DomEventKind::Click,
            node_id,
            DomEventPayload::FormControl {
                form_id: Some(7),
                control_id: 11,
                kind: FormControlKind::Search,
                name: Some("q".to_owned()),
                value: String::new(),
            },
        ),
    );

    assert!(result.default_action.is_some());
    assert_eq!(runtime.script_hooks().hooks().len(), 1);
}
