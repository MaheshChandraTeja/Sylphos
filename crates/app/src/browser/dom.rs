//! Page DOM runtime bridge for events, mutations, and future script execution.
//!
//! The app still owns navigation, forms, cache, and rendering. This controller
//! keeps a mutable DOM runtime beside the active render document so clicks,
//! inputs, submits, and future JavaScript hooks have one central place to land.

use present::{
    dispatch_dom_event, DomEvent, DomEventKind, DomEventPayload, DomRuntime, FormControlHitResult,
    InvalidationSet, LinkHitResult, RenderDocument,
};
use tracing::debug;

use crate::browser::TabId;

/// Active-page DOM controller.
#[derive(Debug, Clone, Default)]
pub(crate) struct PageDomController {
    active_tab_id: Option<TabId>,
    runtime: Option<DomRuntime>,
}

impl PageDomController {
    /// Clears the active DOM runtime.
    pub(crate) fn clear(&mut self) {
        self.active_tab_id = None;
        self.runtime = None;
    }

    /// Installs a fresh runtime for the active tab document.
    pub(crate) fn install_for_tab(&mut self, tab_id: TabId, document: &RenderDocument) {
        if self.active_tab_id == Some(tab_id) {
            if let Some(runtime) = self.runtime.as_mut() {
                runtime.sync_from_render_document(document);
                debug!(
                    tab_id = tab_id.value(),
                    mutations = runtime.mutations().len(),
                    "synced existing mutable DOM runtime for active tab"
                );
                return;
            }
        }

        let runtime = DomRuntime::from_render_document(document);
        debug!(
            tab_id = tab_id.value(),
            nodes = runtime.nodes().count(),
            "installed mutable DOM runtime"
        );
        self.active_tab_id = Some(tab_id);
        self.runtime = Some(runtime);
    }

    /// Synchronizes runtime form values/focus from the current render document.
    pub(crate) fn sync_from_document(&mut self, document: &RenderDocument) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.sync_from_render_document(document);
            debug!(
                mutations = runtime.mutations().len(),
                invalidation_generation = runtime.invalidation().generation(),
                "synced render document into mutable DOM runtime"
            );
        }
    }

    /// Dispatches a link click event into the DOM runtime.
    pub(crate) fn dispatch_link_click(&mut self, link: &LinkHitResult) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };

        let target = runtime
            .node_for_link(&link.href, &link.text)
            .unwrap_or_else(|| runtime.root_id());

        let result = dispatch_dom_event(
            runtime,
            DomEvent::new(
                DomEventKind::Click,
                target,
                DomEventPayload::Link {
                    href: link.href.clone(),
                    text: link.text.clone(),
                },
            ),
        );

        debug!(
            href = %link.href,
            default_action = ?result.default_action,
            hooks = runtime.script_hooks().hooks().len(),
            "dispatched link click DOM event"
        );
    }

    /// Dispatches a form-control click event into the DOM runtime.
    pub(crate) fn dispatch_form_control_click(&mut self, hit: &FormControlHitResult) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };

        let target = runtime
            .node_for_control(hit.control_id)
            .unwrap_or_else(|| runtime.root_id());

        let result = dispatch_dom_event(
            runtime,
            DomEvent::new(
                DomEventKind::Click,
                target,
                DomEventPayload::FormControl {
                    form_id: Some(hit.form_id),
                    control_id: hit.control_id,
                    kind: hit.kind,
                    name: hit.name.clone(),
                    value: String::new(),
                },
            ),
        );

        debug!(
            form_id = hit.form_id,
            control_id = hit.control_id,
            kind = ?hit.kind,
            default_action = ?result.default_action,
            hooks = runtime.script_hooks().hooks().len(),
            "dispatched form-control click DOM event"
        );
    }

    /// Dispatches a form input/change event after the render document has mutated.
    pub(crate) fn dispatch_form_document_mutation(&mut self, document: &RenderDocument) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };

        runtime.sync_from_render_document(document);

        let Some(control) = present::focused_form_control(document) else {
            return;
        };

        let target = runtime
            .node_for_control(control.id)
            .unwrap_or_else(|| runtime.root_id());

        let result = dispatch_dom_event(
            runtime,
            DomEvent::new(
                DomEventKind::Input,
                target,
                DomEventPayload::FormControl {
                    form_id: present::form_id_for_control(document, control.id),
                    control_id: control.id,
                    kind: control.kind,
                    name: control.name.clone(),
                    value: control.value.clone(),
                },
            ),
        );

        debug!(
            control_id = control.id,
            dirty = ?result.dirty,
            hooks = runtime.script_hooks().hooks().len(),
            "dispatched form input DOM event"
        );
    }

    /// Returns and clears pending DOM invalidation for incremental reflow.
    pub(crate) fn take_invalidation(&mut self) -> InvalidationSet {
        self.runtime
            .as_mut()
            .map_or_else(InvalidationSet::default, DomRuntime::take_invalidation)
    }
}
