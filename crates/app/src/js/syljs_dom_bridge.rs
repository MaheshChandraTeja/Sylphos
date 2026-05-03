//! App bridge between SylJS DOM bindings and Sylphos presentation DOM runtime.
//!
//! The SylJS crate owns the VM-facing `DomHost` trait. This file adapts that
//! trait to `present::DomRuntime` so browser scripts can mutate page state and
//! then push those mutations back into `RenderDocument` for layout/repaint.

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use present::{DomNode, DomNodeId, DomNodeKind, DomRuntime, RenderBlock, RenderDocument};
use syljs::{
    install_dom_globals, DomBindingMetrics, DomEventRecord, DomHost, DomNodeRef, DomNodeSnapshot,
    DomNodeType, EventLoopConfig, EventLoopRunSummary, JsRuntimeError, JsValue, ProgramKind,
    ScheduledVm, SharedDomHost, VmConfig,
};

/// DOM-bound script input.
#[derive(Debug, Clone)]
pub(crate) struct DomBoundSylJsScript {
    /// Script label, usually source URL or inline index.
    pub label: String,

    /// Script source.
    pub source: String,

    /// Script kind.
    pub kind: ProgramKind,
}

/// Result from executing scripts against a page DOM.
#[derive(Debug, Clone)]
pub(crate) struct DomBoundSylJsResult {
    /// Event-loop and VM run summary.
    pub summary: EventLoopRunSummary,

    /// Script labels that failed before/during execution.
    pub failed_scripts: Vec<String>,

    /// DOM binding metrics.
    pub dom_metrics: DomBindingMetrics,

    /// Updated title, if scripts set `document.title`.
    pub title: Option<String>,
}

/// Executes scripts against a mutable presentation DOM runtime and applies
/// supported mutations back into the render document.
pub(crate) fn execute_dom_bound_syljs_scripts<I>(
    document: &mut RenderDocument,
    scripts: I,
    vm_config: VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<DomBoundSylJsResult, JsRuntimeError>
where
    I: IntoIterator<Item = DomBoundSylJsScript>,
{
    let runtime = Rc::new(RefCell::new(DomRuntime::from_render_document(document)));
    let host = Rc::new(PresentDomHost::new(runtime.clone(), document.title.clone()));

    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);
    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), host.clone());

    let mut failed_scripts = Vec::new();

    for script in scripts {
        let parsed = match script.kind {
            ProgramKind::Script => syljs::parse_script(&script.source),
            ProgramKind::Module => syljs::parse_module(&script.source),
        };

        let result = parsed
            .map_err(JsRuntimeError::from_frontend_error)
            .and_then(|program| syljs::compile_program(&program, Default::default()).map_err(Into::into))
            .and_then(|bytecode| scheduled.vm.execute(&bytecode));

        if let Err(error) = result {
            tracing::warn!(
                label = %script.label,
                error = %error,
                "failed to execute DOM-bound SylJS script"
            );
            failed_scripts.push(script.label);
        }
    }

    let summary = scheduled.run_until_idle()?;
    apply_runtime_to_render_document(&runtime.borrow(), document);

    if let Some(title) = host.title_override.borrow().clone() {
        document.title = Some(title.clone());
    }

    Ok(DomBoundSylJsResult {
        summary,
        failed_scripts,
        dom_metrics: host.metrics.borrow().clone(),
        title: host.title_override.borrow().clone(),
    })
}

/// `present::DomRuntime` adapter for SylJS.
#[derive(Debug)]
pub(crate) struct PresentDomHost {
    runtime: Rc<RefCell<DomRuntime>>,
    title_override: RefCell<Option<String>>,
    metrics: RefCell<DomBindingMetrics>,
    synthetic_nodes: RefCell<BTreeMap<DomNodeRef, SyntheticNode>>,
    next_synthetic_node: RefCell<u64>,
    listeners: RefCell<BTreeMap<(DomNodeRef, String), Vec<JsValue>>>,
    events: RefCell<Vec<DomEventRecord>>,
}

#[derive(Debug, Clone)]
struct SyntheticNode {
    id: DomNodeRef,
    parent: Option<DomNodeRef>,
    children: Vec<DomNodeRef>,
    node_type: DomNodeType,
    tag_name: String,
    text: String,
    attributes: BTreeMap<String, String>,
}

impl PresentDomHost {
    /// Creates a DOM host adapter.
    pub(crate) fn new(runtime: Rc<RefCell<DomRuntime>>, title: Option<String>) -> Self {
        Self {
            runtime,
            title_override: RefCell::new(title),
            metrics: RefCell::new(DomBindingMetrics::default()),
            synthetic_nodes: RefCell::new(BTreeMap::new()),
            next_synthetic_node: RefCell::new(1_000_000),
            listeners: RefCell::new(BTreeMap::new()),
            events: RefCell::new(Vec::new()),
        }
    }

    fn alloc_synthetic(&self, node_type: DomNodeType, tag: impl Into<String>, text: String) -> DomNodeRef {
        let mut next = self.next_synthetic_node.borrow_mut();
        let id = DomNodeRef(*next);
        *next = next.saturating_add(1);
        self.synthetic_nodes.borrow_mut().insert(
            id,
            SyntheticNode {
                id,
                parent: None,
                children: Vec::new(),
                node_type,
                tag_name: tag.into().to_ascii_lowercase(),
                text,
                attributes: BTreeMap::new(),
            },
        );
        self.metrics.borrow_mut().nodes_created =
            self.metrics.borrow().nodes_created.saturating_add(1);
        id
    }

    fn runtime_id(id: DomNodeRef) -> Option<DomNodeId> {
        if id.0 >= 1_000_000 {
            None
        } else {
            Some(DomNodeId(id.0))
        }
    }

    fn snapshot_runtime_node(node: &DomNode) -> DomNodeSnapshot {
        DomNodeSnapshot {
            id: DomNodeRef(node.id.value()),
            parent: node.parent.map(|parent| DomNodeRef(parent.value())),
            children: node
                .children
                .iter()
                .map(|child| DomNodeRef(child.value()))
                .collect(),
            node_type: if node.kind == DomNodeKind::Text {
                DomNodeType::Text
            } else {
                DomNodeType::Element
            },
            tag_name: node.tag.clone(),
            text: node.text.clone(),
            attributes: node.attributes.clone(),
        }
    }

    fn snapshot_synthetic(node: &SyntheticNode) -> DomNodeSnapshot {
        DomNodeSnapshot {
            id: node.id,
            parent: node.parent,
            children: node.children.clone(),
            node_type: node.node_type,
            tag_name: node.tag_name.clone(),
            text: node.text.clone(),
            attributes: node.attributes.clone(),
        }
    }

    fn matches(snapshot: &DomNodeSnapshot, selector: &str) -> bool {
        let selector = selector.trim();

        if selector == "body" {
            return snapshot.parent.is_none() || snapshot.tag_name == "#root";
        }

        if let Some(id) = selector.strip_prefix('#') {
            return snapshot.attributes.get("id").map(String::as_str) == Some(id);
        }

        if let Some(class) = selector.strip_prefix('.') {
            return snapshot
                .attributes
                .get("class")
                .is_some_and(|value| value.split_whitespace().any(|entry| entry == class));
        }

        snapshot.tag_name.eq_ignore_ascii_case(selector)
    }
}

impl DomHost for PresentDomHost {
    fn title(&self) -> String {
        self.title_override.borrow().clone().unwrap_or_default()
    }

    fn set_title(&self, title: String) {
        *self.title_override.borrow_mut() = Some(title);
        self.metrics.borrow_mut().attribute_mutations =
            self.metrics.borrow().attribute_mutations.saturating_add(1);
    }

    fn body(&self) -> DomNodeRef {
        DomNodeRef(self.runtime.borrow().root_id().value())
    }

    fn node_snapshot(&self, id: DomNodeRef) -> Option<DomNodeSnapshot> {
        if let Some(runtime_id) = Self::runtime_id(id) {
            return self
                .runtime
                .borrow()
                .node(runtime_id)
                .map(Self::snapshot_runtime_node);
        }

        self.synthetic_nodes
            .borrow()
            .get(&id)
            .map(Self::snapshot_synthetic)
    }

    fn query_selector(&self, selector: &str) -> Option<DomNodeRef> {
        self.metrics.borrow_mut().queries = self.metrics.borrow().queries.saturating_add(1);

        for node in self.runtime.borrow().nodes() {
            let snapshot = Self::snapshot_runtime_node(node);
            if Self::matches(&snapshot, selector) {
                return Some(snapshot.id);
            }
        }

        for node in self.synthetic_nodes.borrow().values() {
            let snapshot = Self::snapshot_synthetic(node);
            if Self::matches(&snapshot, selector) {
                return Some(snapshot.id);
            }
        }

        None
    }

    fn query_selector_within(&self, _root: DomNodeRef, selector: &str) -> Option<DomNodeRef> {
        self.query_selector(selector)
    }

    fn get_element_by_id(&self, id: &str) -> Option<DomNodeRef> {
        self.query_selector(&format!("#{id}"))
    }

    fn create_element(&self, tag: &str) -> DomNodeRef {
        self.alloc_synthetic(DomNodeType::Element, tag, String::new())
    }

    fn create_text_node(&self, text: &str) -> DomNodeRef {
        self.alloc_synthetic(DomNodeType::Text, "#text", text.to_owned())
    }

    fn append_child(&self, parent: DomNodeRef, child: DomNodeRef) -> bool {
        if let Some(runtime_parent) = Self::runtime_id(parent) {
            if Self::runtime_id(child).is_none() {
                let text = self.text_content(child);
                if !text.is_empty() {
                    let created = self
                        .runtime
                        .borrow_mut()
                        .append_text_child(runtime_parent, text)
                        .is_some();
                    if created {
                        self.metrics.borrow_mut().structure_mutations =
                            self.metrics.borrow().structure_mutations.saturating_add(1);
                    }
                    return created;
                }
            }
        }

        let mut synthetic = self.synthetic_nodes.borrow_mut();

        if !synthetic.contains_key(&child) {
            return false;
        }

        if let Some(parent_node) = synthetic.get_mut(&parent) {
            if !parent_node.children.contains(&child) {
                parent_node.children.push(child);
            }
        }

        if let Some(child_node) = synthetic.get_mut(&child) {
            child_node.parent = Some(parent);
        }

        self.metrics.borrow_mut().structure_mutations =
            self.metrics.borrow().structure_mutations.saturating_add(1);
        true
    }

    fn remove_node(&self, node: DomNodeRef) -> bool {
        if let Some(runtime_id) = Self::runtime_id(node) {
            return self
                .runtime
                .borrow_mut()
                .set_attribute(runtime_id, "hidden", "true");
        }

        self.synthetic_nodes.borrow_mut().remove(&node).is_some()
    }

    fn set_text_content(&self, node: DomNodeRef, value: String) -> bool {
        if let Some(runtime_id) = Self::runtime_id(node) {
            let ok = self.runtime.borrow_mut().set_text(runtime_id, value);
            if ok {
                self.metrics.borrow_mut().text_mutations =
                    self.metrics.borrow().text_mutations.saturating_add(1);
            }
            return ok;
        }

        if let Some(node) = self.synthetic_nodes.borrow_mut().get_mut(&node) {
            node.text = value;
            self.metrics.borrow_mut().text_mutations =
                self.metrics.borrow().text_mutations.saturating_add(1);
            return true;
        }

        false
    }

    fn text_content(&self, node: DomNodeRef) -> String {
        self.node_snapshot(node).map_or_else(String::new, |node| node.text)
    }

    fn set_value(&self, node: DomNodeRef, value: String) -> bool {
        if let Some(runtime_id) = Self::runtime_id(node) {
            let control_id = self
                .runtime
                .borrow()
                .node(runtime_id)
                .and_then(|node| node.control_id);
            if let Some(control_id) = control_id {
                let ok = self.runtime.borrow_mut().set_form_value(control_id, value);
                if ok {
                    self.metrics.borrow_mut().value_mutations =
                        self.metrics.borrow().value_mutations.saturating_add(1);
                }
                return ok;
            }
        }

        self.set_attribute(node, "value", value)
    }

    fn value(&self, node: DomNodeRef) -> String {
        self.get_attribute(node, "value")
            .unwrap_or_else(|| self.text_content(node))
    }

    fn set_attribute(&self, node: DomNodeRef, name: &str, value: String) -> bool {
        if let Some(runtime_id) = Self::runtime_id(node) {
            let ok = self
                .runtime
                .borrow_mut()
                .set_attribute(runtime_id, name, value);
            if ok {
                self.metrics.borrow_mut().attribute_mutations =
                    self.metrics.borrow().attribute_mutations.saturating_add(1);
            }
            return ok;
        }

        if let Some(node) = self.synthetic_nodes.borrow_mut().get_mut(&node) {
            node.attributes.insert(name.to_ascii_lowercase(), value);
            self.metrics.borrow_mut().attribute_mutations =
                self.metrics.borrow().attribute_mutations.saturating_add(1);
            return true;
        }

        false
    }

    fn get_attribute(&self, node: DomNodeRef, name: &str) -> Option<String> {
        self.node_snapshot(node)
            .and_then(|node| node.attributes.get(&name.to_ascii_lowercase()).cloned())
    }

    fn remove_attribute(&self, node: DomNodeRef, name: &str) -> bool {
        if let Some(runtime_id) = Self::runtime_id(node) {
            return self.runtime.borrow_mut().remove_attribute(runtime_id, name);
        }

        self.synthetic_nodes
            .borrow_mut()
            .get_mut(&node)
            .is_some_and(|node| node.attributes.remove(&name.to_ascii_lowercase()).is_some())
    }

    fn add_event_listener(&self, node: DomNodeRef, event_type: &str, callback: JsValue) {
        self.listeners
            .borrow_mut()
            .entry((node, event_type.to_owned()))
            .or_default()
            .push(callback);
        self.metrics.borrow_mut().listeners_added =
            self.metrics.borrow().listeners_added.saturating_add(1);
    }

    fn event_listeners(&self, node: DomNodeRef, event_type: &str) -> Vec<JsValue> {
        self.listeners
            .borrow()
            .get(&(node, event_type.to_owned()))
            .cloned()
            .unwrap_or_default()
    }

    fn record_event_dispatch(&self, node: DomNodeRef, event_type: &str) {
        self.events.borrow_mut().push(DomEventRecord {
            target: node,
            event_type: event_type.to_owned(),
        });
        self.metrics.borrow_mut().events_dispatched =
            self.metrics.borrow().events_dispatched.saturating_add(1);
    }

    fn metrics(&self) -> DomBindingMetrics {
        self.metrics.borrow().clone()
    }
}

fn apply_runtime_to_render_document(runtime: &DomRuntime, document: &mut RenderDocument) {
    runtime.apply_to_render_document(document);

    for node in runtime.nodes() {
        let Some(index) = node.block_index else {
            continue;
        };

        let Some(block) = document.blocks.get_mut(index) else {
            continue;
        };

        match block {
            RenderBlock::Heading { text, .. }
            | RenderBlock::Paragraph { text }
            | RenderBlock::Generic { text, .. } => {
                *text = node.text.clone();
            }
            RenderBlock::Link { text, href } => {
                *text = node.text.clone();
                if let Some(value) = node.attributes.get("href") {
                    *href = Some(value.clone());
                }
            }
            RenderBlock::Image { alt, src } => {
                if let Some(value) = node.attributes.get("alt") {
                    *alt = Some(value.clone());
                }
                if let Some(value) = node.attributes.get("src") {
                    *src = Some(value.clone());
                }
            }
            RenderBlock::InlineFlow { .. } | RenderBlock::Form(_) => {}
        }
    }
}
