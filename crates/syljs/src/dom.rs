#![allow(clippy::too_many_lines)]
#![doc = "DOM host bindings for SylJS."]

use crate::{
    cssom::{create_inline_style_object, ResearchCssomHost, SharedCssomHost},
    event_loop::JsEventLoop,
    JsFunction, JsHostObject, JsNativeFunction, JsObject, JsObjectKind, JsRuntimeError, JsValue,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

/// Shared DOM host pointer.
pub type SharedDomHost = Rc<dyn DomHost>;

/// Stable DOM node reference exposed to SylJS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DomNodeRef(pub u64);

/// DOM node type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomNodeType {
    /// Document/root node.
    Document,

    /// Element node.
    Element,

    /// Text node.
    Text,
}

/// Snapshot of a DOM node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomNodeSnapshot {
    /// Node id.
    pub id: DomNodeRef,

    /// Parent id.
    pub parent: Option<DomNodeRef>,

    /// Children.
    pub children: Vec<DomNodeRef>,

    /// Node type.
    pub node_type: DomNodeType,

    /// Tag name.
    pub tag_name: String,

    /// Text content.
    pub text: String,

    /// Attributes.
    pub attributes: BTreeMap<String, String>,
}

/// Event listener record for diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct DomListenerRecord {
    /// Target node.
    pub target: DomNodeRef,

    /// Event type.
    pub event_type: String,
}

/// Event dispatch record for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomEventRecord {
    /// Target node.
    pub target: DomNodeRef,

    /// Event type.
    pub event_type: String,
}

/// DOM binding metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomBindingMetrics {
    /// Query calls.
    pub queries: u64,

    /// Node creation calls.
    pub nodes_created: u64,

    /// Text mutation calls.
    pub text_mutations: u64,

    /// Attribute mutations.
    pub attribute_mutations: u64,

    /// Value mutations.
    pub value_mutations: u64,

    /// Append/remove mutations.
    pub structure_mutations: u64,

    /// Event listeners added.
    pub listeners_added: u64,

    /// Events dispatched.
    pub events_dispatched: u64,
}

/// Browser DOM host abstraction.
pub trait DomHost {
    /// Document title.
    fn title(&self) -> String;

    /// Sets document title.
    fn set_title(&self, title: String);

    /// Returns document/body root node.
    fn body(&self) -> DomNodeRef;

    /// Returns a node snapshot.
    fn node_snapshot(&self, id: DomNodeRef) -> Option<DomNodeSnapshot>;

    /// Finds first matching selector globally.
    fn query_selector(&self, selector: &str) -> Option<DomNodeRef>;

    /// Finds first matching selector under a root.
    fn query_selector_within(&self, root: DomNodeRef, selector: &str) -> Option<DomNodeRef>;

    /// Finds element by id.
    fn get_element_by_id(&self, id: &str) -> Option<DomNodeRef>;

    /// Creates an element.
    fn create_element(&self, tag: &str) -> DomNodeRef;

    /// Creates a text node.
    fn create_text_node(&self, text: &str) -> DomNodeRef;

    /// Appends child to parent.
    fn append_child(&self, parent: DomNodeRef, child: DomNodeRef) -> bool;

    /// Removes a node.
    fn remove_node(&self, node: DomNodeRef) -> bool;

    /// Sets text content.
    fn set_text_content(&self, node: DomNodeRef, value: String) -> bool;

    /// Returns text content.
    fn text_content(&self, node: DomNodeRef) -> String;

    /// Sets form/control value.
    fn set_value(&self, node: DomNodeRef, value: String) -> bool;

    /// Returns form/control value.
    fn value(&self, node: DomNodeRef) -> String;

    /// Sets attribute.
    fn set_attribute(&self, node: DomNodeRef, name: &str, value: String) -> bool;

    /// Gets attribute.
    fn get_attribute(&self, node: DomNodeRef, name: &str) -> Option<String>;

    /// Removes attribute.
    fn remove_attribute(&self, node: DomNodeRef, name: &str) -> bool;

    /// Adds an event listener callback.
    fn add_event_listener(&self, node: DomNodeRef, event_type: &str, callback: JsValue);

    /// Returns event listeners.
    fn event_listeners(&self, node: DomNodeRef, event_type: &str) -> Vec<JsValue>;

    /// Records event dispatch.
    fn record_event_dispatch(&self, node: DomNodeRef, event_type: &str);

    /// Returns CSSOM host backing `element.style`.
    fn cssom_host(&self) -> SharedCssomHost;

    /// Returns metrics.
    fn metrics(&self) -> DomBindingMetrics;
}

/// Installs `window`, `document`, DOM constructors, and basic event bindings.
pub fn install_dom_globals(
    vm: &mut crate::Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    host: SharedDomHost,
) {
    let document = create_document_object(host.clone(), event_loop.clone());
    let window = create_window_object(host.clone(), document.clone(), event_loop.clone());

    vm.define_global("document", document.clone());
    vm.define_global("window", window.clone());
    vm.define_global("self", window);
    vm.define_global("Event", create_event_constructor());
    vm.define_global("MouseEvent", create_event_constructor());
    vm.define_global("KeyboardEvent", create_event_constructor());
    vm.define_global(
        "HTMLElement",
        JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host)))),
    );
}

/// Creates a script-visible document object.
#[must_use]
pub fn create_document_object(
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(DocumentHostObject { host, event_loop }),
        "[object Document]",
    );
    object.set_property("nodeType", JsValue::Number(9.0));
    object
}

/// Creates a script-visible element object.
#[must_use]
pub fn create_element_object(
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    node: DomNodeRef,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(ElementHostObject {
            host,
            event_loop,
            node,
        }),
        "[object HTMLElement]",
    );
    object.set_property("__syljsDomNodeId", JsValue::Number(node.0 as f64));
    object.set_property("nodeType", JsValue::Number(1.0));
    object
}

fn create_text_node_object(
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    node: DomNodeRef,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(ElementHostObject {
            host,
            event_loop,
            node,
        }),
        "[object Text]",
    );
    object.set_property("__syljsDomNodeId", JsValue::Number(node.0 as f64));
    object.set_property("nodeType", JsValue::Number(3.0));
    object
}

fn create_window_object(
    host: SharedDomHost,
    document: JsValue,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    let window = JsValue::host_object(
        Rc::new(WindowHostObject {
            host,
            event_loop,
            document: document.clone(),
        }),
        "[object Window]",
    );
    window.set_property("document", document);
    window.set_property("window", window.clone());
    window
}

fn create_event_constructor() -> JsValue {
    let constructor: JsNativeFunction = Rc::new(|_vm, _this, args| {
        let event_type = args
            .first()
            .map_or_else(|| "event".to_owned(), JsValue::to_js_string);

        Ok(create_event_object(event_type))
    });

    JsValue::function(JsFunction::Native {
        name: "Event".to_owned(),
        function: constructor,
    })
}

fn create_event_object(event_type: String) -> JsValue {
    let event = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
    event.set_property("type", JsValue::String(event_type));
    event.set_property("defaultPrevented", JsValue::Boolean(false));

    let prevent_target = event.clone();
    let prevent_default: JsNativeFunction = Rc::new(move |_vm, _this, _args| {
        prevent_target.set_property("defaultPrevented", JsValue::Boolean(true));
        Ok(JsValue::Undefined)
    });

    event.set_property(
        "preventDefault",
        JsValue::function(JsFunction::Native {
            name: "Event.preventDefault".to_owned(),
            function: prevent_default,
        }),
    );

    event
}

#[derive(Clone)]
struct WindowHostObject {
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    document: JsValue,
}

impl JsHostObject for WindowHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "document" => Some(self.document.clone()),
            "window" | "self" => None,
            "innerWidth" => Some(JsValue::Number(1024.0)),
            "innerHeight" => Some(JsValue::Number(768.0)),
            "location" => Some(create_location_object()),
            "addEventListener" => Some(native_window_add_event_listener()),
            "dispatchEvent" => Some(native_window_dispatch_event(self.event_loop.clone())),
            _ => {
                let _ = &self.host;
                None
            }
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

#[derive(Clone)]
struct DocumentHostObject {
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
}

impl JsHostObject for DocumentHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "title" => Some(JsValue::String(self.host.title())),
            "body" => Some(create_element_object(
                self.host.clone(),
                self.event_loop.clone(),
                self.host.body(),
            )),
            "documentElement" => Some(create_element_object(
                self.host.clone(),
                self.event_loop.clone(),
                self.host.body(),
            )),
            "querySelector" => Some(native_query_selector(
                self.host.clone(),
                self.event_loop.clone(),
                None,
            )),
            "querySelectorAll" => Some(native_query_selector_all(
                self.host.clone(),
                self.event_loop.clone(),
                None,
            )),
            "getElementById" => Some(native_get_element_by_id(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            "createElement" => Some(native_create_element(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            "createTextNode" => Some(native_create_text_node(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            "addEventListener" => Some(native_add_event_listener(
                self.host.clone(),
                self.event_loop.clone(),
                self.host.body(),
            )),
            "dispatchEvent" => Some(native_dispatch_event(
                self.host.clone(),
                self.event_loop.clone(),
                self.host.body(),
            )),
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        match key {
            "title" => {
                self.host.set_title(value.to_js_string());
                true
            }
            _ => false,
        }
    }
}

#[derive(Clone)]
struct ElementHostObject {
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    node: DomNodeRef,
}

impl JsHostObject for ElementHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        let snapshot = self.host.node_snapshot(self.node);

        match key {
            "style" => Some(create_inline_style_object(
                self.host.cssom_host(),
                self.node,
            )),
            "tagName" | "nodeName" => Some(JsValue::String(
                snapshot
                    .as_ref()
                    .map_or_else(String::new, |node| node.tag_name.to_ascii_uppercase()),
            )),
            "localName" => Some(JsValue::String(
                snapshot
                    .as_ref()
                    .map_or_else(String::new, |node| node.tag_name.to_ascii_lowercase()),
            )),
            "id" => Some(JsValue::String(
                self.host.get_attribute(self.node, "id").unwrap_or_default(),
            )),
            "className" => Some(JsValue::String(
                self.host
                    .get_attribute(self.node, "class")
                    .unwrap_or_default(),
            )),
            "textContent" | "innerText" | "innerHTML" => {
                Some(JsValue::String(self.host.text_content(self.node)))
            }
            "value" => Some(JsValue::String(self.host.value(self.node))),
            "href" => Some(JsValue::String(
                self.host
                    .get_attribute(self.node, "href")
                    .unwrap_or_default(),
            )),
            "src" => Some(JsValue::String(
                self.host
                    .get_attribute(self.node, "src")
                    .unwrap_or_default(),
            )),
            "name" => Some(JsValue::String(
                self.host
                    .get_attribute(self.node, "name")
                    .unwrap_or_default(),
            )),
            "children" => Some(create_children_array(
                self.host.clone(),
                self.event_loop.clone(),
                snapshot
                    .as_ref()
                    .map_or_else(Vec::new, |node| node.children.clone()),
            )),
            "classList" => Some(create_class_list_object(self.host.clone(), self.node)),
            "setAttribute" => Some(native_set_attribute(self.host.clone(), self.node)),
            "getAttribute" => Some(native_get_attribute(self.host.clone(), self.node)),
            "removeAttribute" => Some(native_remove_attribute(self.host.clone(), self.node)),
            "appendChild" => Some(native_append_child(
                self.host.clone(),
                self.event_loop.clone(),
                self.node,
            )),
            "remove" => Some(native_remove(self.host.clone(), self.node)),
            "querySelector" => Some(native_query_selector(
                self.host.clone(),
                self.event_loop.clone(),
                Some(self.node),
            )),
            "querySelectorAll" => Some(native_query_selector_all(
                self.host.clone(),
                self.event_loop.clone(),
                Some(self.node),
            )),
            "addEventListener" => Some(native_add_event_listener(
                self.host.clone(),
                self.event_loop.clone(),
                self.node,
            )),
            "dispatchEvent" => Some(native_dispatch_event(
                self.host.clone(),
                self.event_loop.clone(),
                self.node,
            )),
            "focus" => Some(native_noop("HTMLElement.focus")),
            "blur" => Some(native_noop("HTMLElement.blur")),
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        match key {
            "id" => self
                .host
                .set_attribute(self.node, "id", value.to_js_string()),
            "className" => self
                .host
                .set_attribute(self.node, "class", value.to_js_string()),
            "textContent" | "innerText" | "innerHTML" => {
                self.host.set_text_content(self.node, value.to_js_string())
            }
            "value" => self.host.set_value(self.node, value.to_js_string()),
            "href" => self
                .host
                .set_attribute(self.node, "href", value.to_js_string()),
            "src" => self
                .host
                .set_attribute(self.node, "src", value.to_js_string()),
            "name" => self
                .host
                .set_attribute(self.node, "name", value.to_js_string()),
            _ => false,
        }
    }
}

fn native_query_selector(
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    root: Option<DomNodeRef>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "querySelector".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let selector = args.first().map_or_else(String::new, JsValue::to_js_string);
            let found = root.map_or_else(
                || host.query_selector(&selector),
                |root| host.query_selector_within(root, &selector),
            );

            Ok(found.map_or(JsValue::Null, |node| {
                create_element_object(host.clone(), event_loop.clone(), node)
            }))
        }),
    })
}

fn native_query_selector_all(
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    root: Option<DomNodeRef>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "querySelectorAll".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let selector = args.first().map_or_else(String::new, JsValue::to_js_string);
            let found = root.map_or_else(
                || host.query_selector(&selector),
                |root| host.query_selector_within(root, &selector),
            );

            let values = found
                .map(|node| {
                    vec![create_element_object(
                        host.clone(),
                        event_loop.clone(),
                        node,
                    )]
                })
                .unwrap_or_default();

            Ok(JsValue::array(values))
        }),
    })
}

fn native_get_element_by_id(host: SharedDomHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "document.getElementById".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let id = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(host.get_element_by_id(&id).map_or(JsValue::Null, |node| {
                create_element_object(host.clone(), event_loop.clone(), node)
            }))
        }),
    })
}

fn native_create_element(host: SharedDomHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "document.createElement".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let tag = args
                .first()
                .map_or_else(|| "div".to_owned(), JsValue::to_js_string);
            let node = host.create_element(&tag);
            Ok(create_element_object(
                host.clone(),
                event_loop.clone(),
                node,
            ))
        }),
    })
}

fn native_create_text_node(host: SharedDomHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "document.createTextNode".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let text = args.first().map_or_else(String::new, JsValue::to_js_string);
            let node = host.create_text_node(&text);
            Ok(create_text_node_object(
                host.clone(),
                event_loop.clone(),
                node,
            ))
        }),
    })
}

fn native_set_attribute(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.setAttribute".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let name = args.first().map_or_else(String::new, JsValue::to_js_string);
            let value = args.get(1).map_or_else(String::new, JsValue::to_js_string);
            host.set_attribute(node, &name, value);
            Ok(JsValue::Undefined)
        }),
    })
}

fn native_get_attribute(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.getAttribute".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let name = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(host
                .get_attribute(node, &name)
                .map_or(JsValue::Null, JsValue::String))
        }),
    })
}

fn native_remove_attribute(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.removeAttribute".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let name = args.first().map_or_else(String::new, JsValue::to_js_string);
            host.remove_attribute(node, &name);
            Ok(JsValue::Undefined)
        }),
    })
}

fn native_append_child(
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    parent: DomNodeRef,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.appendChild".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let child = args.first().and_then(JsValue::dom_node_id).map(DomNodeRef);

            let Some(child) = child else {
                return Ok(JsValue::Null);
            };

            host.append_child(parent, child);
            Ok(create_element_object(
                host.clone(),
                event_loop.clone(),
                child,
            ))
        }),
    })
}

fn native_remove(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.remove".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            host.remove_node(node);
            Ok(JsValue::Undefined)
        }),
    })
}

fn native_add_event_listener(
    host: SharedDomHost,
    _event_loop: Rc<RefCell<JsEventLoop>>,
    node: DomNodeRef,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "addEventListener".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let event_type = args.first().map_or_else(String::new, JsValue::to_js_string);
            let Some(callback) = args.get(1).cloned() else {
                return Ok(JsValue::Undefined);
            };

            if callback.as_function().is_none() {
                return Err(JsRuntimeError::new("event listener is not callable"));
            }

            host.add_event_listener(node, &event_type, callback);
            Ok(JsValue::Undefined)
        }),
    })
}

fn native_dispatch_event(
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    node: DomNodeRef,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "dispatchEvent".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let event = args
                .first()
                .cloned()
                .unwrap_or_else(|| create_event_object("event".to_owned()));
            let event_type = event.get_property("type").to_js_string();

            host.record_event_dispatch(node, &event_type);

            for callback in host.event_listeners(node, &event_type) {
                event_loop
                    .borrow_mut()
                    .queue_task(callback, vec![event.clone()]);
            }

            Ok(JsValue::Boolean(!matches!(
                event.get_property("defaultPrevented"),
                JsValue::Boolean(true)
            )))
        }),
    })
}

fn native_window_add_event_listener() -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "window.addEventListener".to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Undefined)),
    })
}

fn native_window_dispatch_event(event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "window.dispatchEvent".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let event = args
                .first()
                .cloned()
                .unwrap_or_else(|| create_event_object("event".to_owned()));
            let _ = &event_loop;
            Ok(JsValue::Boolean(!matches!(
                event.get_property("defaultPrevented"),
                JsValue::Boolean(true)
            )))
        }),
    })
}

fn native_noop(name: &'static str) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Undefined)),
    })
}

fn create_location_object() -> JsValue {
    let object = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
    object.set_property("href", JsValue::String("about:blank".to_owned()));
    object
}

fn create_children_array(
    host: SharedDomHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    children: Vec<DomNodeRef>,
) -> JsValue {
    let values = children
        .into_iter()
        .map(|child| create_element_object(host.clone(), event_loop.clone(), child))
        .collect::<Vec<_>>();
    JsValue::array(values)
}

fn create_class_list_object(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::host_object(
        Rc::new(ClassListHostObject { host, node }),
        "[object DOMTokenList]",
    )
}

#[derive(Clone)]
struct ClassListHostObject {
    host: SharedDomHost,
    node: DomNodeRef,
}

impl JsHostObject for ClassListHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "add" => Some(class_list_add(self.host.clone(), self.node)),
            "remove" => Some(class_list_remove(self.host.clone(), self.node)),
            "toggle" => Some(class_list_toggle(self.host.clone(), self.node)),
            "contains" => Some(class_list_contains(self.host.clone(), self.node)),
            "value" => Some(JsValue::String(
                self.host
                    .get_attribute(self.node, "class")
                    .unwrap_or_default(),
            )),
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        if key == "value" {
            return self
                .host
                .set_attribute(self.node, "class", value.to_js_string());
        }
        false
    }
}

fn class_list_add(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "classList.add".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            mutate_class_list(&host, node, |classes| {
                for arg in args {
                    let value = arg.to_js_string();
                    if !value.is_empty() {
                        classes.insert(value);
                    }
                }
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn class_list_remove(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "classList.remove".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            mutate_class_list(&host, node, |classes| {
                for arg in args {
                    classes.remove(&arg.to_js_string());
                }
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn class_list_toggle(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "classList.toggle".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let token = args.first().map_or_else(String::new, JsValue::to_js_string);
            let mut now_present = false;

            mutate_class_list(&host, node, |classes| {
                if classes.contains(&token) {
                    classes.remove(&token);
                } else if !token.is_empty() {
                    classes.insert(token.clone());
                    now_present = true;
                }
            });

            Ok(JsValue::Boolean(now_present))
        }),
    })
}

fn class_list_contains(host: SharedDomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "classList.contains".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let token = args.first().map_or_else(String::new, JsValue::to_js_string);
            let classes = class_set(&host.get_attribute(node, "class").unwrap_or_default());
            Ok(JsValue::Boolean(classes.contains(&token)))
        }),
    })
}

fn mutate_class_list(
    host: &SharedDomHost,
    node: DomNodeRef,
    mutate: impl FnOnce(&mut BTreeSet<String>),
) {
    let mut classes = class_set(&host.get_attribute(node, "class").unwrap_or_default());
    mutate(&mut classes);
    let value = classes.into_iter().collect::<Vec<_>>().join(" ");
    host.set_attribute(node, "class", value);
}

fn class_set(value: &str) -> BTreeSet<String> {
    value
        .split_whitespace()
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Deterministic in-memory DOM used by SylJS tests and paper benchmarks.
pub struct ResearchDom {
    inner: RefCell<ResearchDomInner>,
    metrics: RefCell<DomBindingMetrics>,
    cssom: SharedCssomHost,
}

impl std::fmt::Debug for ResearchDom {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResearchDom")
            .field("inner", &self.inner)
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct ResearchDomInner {
    title: String,
    nodes: BTreeMap<DomNodeRef, ResearchNode>,
    next_node: u64,
    body: DomNodeRef,
    listeners: BTreeMap<(DomNodeRef, String), Vec<JsValue>>,
    events: Vec<DomEventRecord>,
}

#[derive(Debug, Clone)]
struct ResearchNode {
    id: DomNodeRef,
    parent: Option<DomNodeRef>,
    children: Vec<DomNodeRef>,
    node_type: DomNodeType,
    tag_name: String,
    text: String,
    attributes: BTreeMap<String, String>,
}

impl ResearchDom {
    /// Creates a new document with a body root.
    #[must_use]
    pub fn new_document(title: impl Into<String>) -> Self {
        let body = DomNodeRef(1);
        let mut nodes = BTreeMap::new();
        nodes.insert(
            body,
            ResearchNode {
                id: body,
                parent: None,
                children: Vec::new(),
                node_type: DomNodeType::Element,
                tag_name: "body".to_owned(),
                text: String::new(),
                attributes: BTreeMap::new(),
            },
        );

        Self {
            inner: RefCell::new(ResearchDomInner {
                title: title.into(),
                nodes,
                next_node: 2,
                body,
                listeners: BTreeMap::new(),
                events: Vec::new(),
            }),
            metrics: RefCell::new(DomBindingMetrics::default()),
            cssom: Rc::new(ResearchCssomHost::new()),
        }
    }

    /// Creates document using a caller-provided CSSOM host.
    #[must_use]
    pub fn with_cssom(title: impl Into<String>, cssom: SharedCssomHost) -> Self {
        let mut dom = Self::new_document(title);
        dom.cssom = cssom;
        dom
    }

    /// Returns all dispatched event records.
    #[must_use]
    pub fn events(&self) -> Vec<DomEventRecord> {
        self.inner.borrow().events.clone()
    }

    /// Returns all node snapshots.
    #[must_use]
    pub fn all_nodes(&self) -> Vec<DomNodeSnapshot> {
        self.inner
            .borrow()
            .nodes
            .values()
            .map(ResearchNode::snapshot)
            .collect()
    }

    fn alloc_node(
        &self,
        node_type: DomNodeType,
        tag: impl Into<String>,
        text: String,
    ) -> DomNodeRef {
        let mut inner = self.inner.borrow_mut();
        let id = DomNodeRef(inner.next_node);
        inner.next_node = inner.next_node.saturating_add(1);
        inner.nodes.insert(
            id,
            ResearchNode {
                id,
                parent: None,
                children: Vec::new(),
                node_type,
                tag_name: tag.into().to_ascii_lowercase(),
                text,
                attributes: BTreeMap::new(),
            },
        );
        self.bump_metrics(|metrics| {
            metrics.nodes_created = metrics.nodes_created.saturating_add(1);
        });
        id
    }

    fn bump_metrics(&self, update: impl FnOnce(&mut DomBindingMetrics)) {
        update(&mut self.metrics.borrow_mut());
    }
}

impl Default for ResearchDom {
    fn default() -> Self {
        Self::new_document("Sylphos")
    }
}

impl DomHost for ResearchDom {
    fn title(&self) -> String {
        self.inner.borrow().title.clone()
    }

    fn set_title(&self, title: String) {
        self.inner.borrow_mut().title = title;
        self.bump_metrics(|metrics| {
            metrics.attribute_mutations = metrics.attribute_mutations.saturating_add(1);
        });
    }

    fn body(&self) -> DomNodeRef {
        self.inner.borrow().body
    }

    fn node_snapshot(&self, id: DomNodeRef) -> Option<DomNodeSnapshot> {
        self.inner
            .borrow()
            .nodes
            .get(&id)
            .map(ResearchNode::snapshot)
    }

    fn query_selector(&self, selector: &str) -> Option<DomNodeRef> {
        self.bump_metrics(|metrics| {
            metrics.queries = metrics.queries.saturating_add(1);
        });
        self.inner
            .borrow()
            .nodes
            .values()
            .find(|node| matches_selector(node, selector))
            .map(|node| node.id)
    }

    fn query_selector_within(&self, root: DomNodeRef, selector: &str) -> Option<DomNodeRef> {
        self.bump_metrics(|metrics| {
            metrics.queries = metrics.queries.saturating_add(1);
        });
        let inner = self.inner.borrow();
        let mut stack = vec![root];

        while let Some(id) = stack.pop() {
            let Some(node) = inner.nodes.get(&id) else {
                continue;
            };

            if id != root && matches_selector(node, selector) {
                return Some(id);
            }

            for child in node.children.iter().rev() {
                stack.push(*child);
            }
        }

        None
    }

    fn get_element_by_id(&self, id: &str) -> Option<DomNodeRef> {
        self.bump_metrics(|metrics| {
            metrics.queries = metrics.queries.saturating_add(1);
        });
        self.inner.borrow().nodes.values().find_map(|node| {
            (node.attributes.get("id").map(String::as_str) == Some(id)).then_some(node.id)
        })
    }

    fn create_element(&self, tag: &str) -> DomNodeRef {
        self.alloc_node(DomNodeType::Element, tag, String::new())
    }

    fn create_text_node(&self, text: &str) -> DomNodeRef {
        self.alloc_node(DomNodeType::Text, "#text", text.to_owned())
    }

    fn append_child(&self, parent: DomNodeRef, child: DomNodeRef) -> bool {
        let mut inner = self.inner.borrow_mut();

        if !inner.nodes.contains_key(&parent) || !inner.nodes.contains_key(&child) {
            return false;
        }

        if let Some(old_parent) = inner.nodes.get(&child).and_then(|node| node.parent) {
            if let Some(old_parent_node) = inner.nodes.get_mut(&old_parent) {
                old_parent_node.children.retain(|id| *id != child);
            }
        }

        if let Some(child_node) = inner.nodes.get_mut(&child) {
            child_node.parent = Some(parent);
        }

        if let Some(parent_node) = inner.nodes.get_mut(&parent) {
            if !parent_node.children.contains(&child) {
                parent_node.children.push(child);
            }
        }

        self.bump_metrics(|metrics| {
            metrics.structure_mutations = metrics.structure_mutations.saturating_add(1);
        });
        true
    }

    fn remove_node(&self, node: DomNodeRef) -> bool {
        let mut inner = self.inner.borrow_mut();

        if node == inner.body {
            return false;
        }

        let Some(removed) = inner.nodes.remove(&node) else {
            return false;
        };

        if let Some(parent) = removed.parent {
            if let Some(parent_node) = inner.nodes.get_mut(&parent) {
                parent_node.children.retain(|id| *id != node);
            }
        }

        for child in removed.children {
            if let Some(child_node) = inner.nodes.get_mut(&child) {
                child_node.parent = None;
            }
        }

        self.bump_metrics(|metrics| {
            metrics.structure_mutations = metrics.structure_mutations.saturating_add(1);
        });
        true
    }

    fn set_text_content(&self, node: DomNodeRef, value: String) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(node) = inner.nodes.get_mut(&node) else {
            return false;
        };
        node.text = value;
        self.bump_metrics(|metrics| {
            metrics.text_mutations = metrics.text_mutations.saturating_add(1);
        });
        true
    }

    fn text_content(&self, node: DomNodeRef) -> String {
        let inner = self.inner.borrow();
        collect_text(&inner.nodes, node)
    }

    fn set_value(&self, node: DomNodeRef, value: String) -> bool {
        let ok = self.set_attribute(node, "value", value.clone());
        if ok {
            if let Some(node) = self.inner.borrow_mut().nodes.get_mut(&node) {
                node.text = value;
            }
            self.bump_metrics(|metrics| {
                metrics.value_mutations = metrics.value_mutations.saturating_add(1);
            });
        }
        ok
    }

    fn value(&self, node: DomNodeRef) -> String {
        self.get_attribute(node, "value")
            .unwrap_or_else(|| self.text_content(node))
    }

    fn set_attribute(&self, node: DomNodeRef, name: &str, value: String) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(node) = inner.nodes.get_mut(&node) else {
            return false;
        };

        node.attributes.insert(name.to_ascii_lowercase(), value);
        self.bump_metrics(|metrics| {
            metrics.attribute_mutations = metrics.attribute_mutations.saturating_add(1);
        });
        true
    }

    fn get_attribute(&self, node: DomNodeRef, name: &str) -> Option<String> {
        self.inner
            .borrow()
            .nodes
            .get(&node)
            .and_then(|node| node.attributes.get(&name.to_ascii_lowercase()).cloned())
    }

    fn remove_attribute(&self, node: DomNodeRef, name: &str) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(node) = inner.nodes.get_mut(&node) else {
            return false;
        };

        let removed = node.attributes.remove(&name.to_ascii_lowercase()).is_some();
        if removed {
            self.bump_metrics(|metrics| {
                metrics.attribute_mutations = metrics.attribute_mutations.saturating_add(1);
            });
        }
        removed
    }

    fn add_event_listener(&self, node: DomNodeRef, event_type: &str, callback: JsValue) {
        self.inner
            .borrow_mut()
            .listeners
            .entry((node, event_type.to_owned()))
            .or_default()
            .push(callback);
        self.bump_metrics(|metrics| {
            metrics.listeners_added = metrics.listeners_added.saturating_add(1);
        });
    }

    fn event_listeners(&self, node: DomNodeRef, event_type: &str) -> Vec<JsValue> {
        self.inner
            .borrow()
            .listeners
            .get(&(node, event_type.to_owned()))
            .cloned()
            .unwrap_or_default()
    }

    fn record_event_dispatch(&self, node: DomNodeRef, event_type: &str) {
        self.inner.borrow_mut().events.push(DomEventRecord {
            target: node,
            event_type: event_type.to_owned(),
        });
        self.bump_metrics(|metrics| {
            metrics.events_dispatched = metrics.events_dispatched.saturating_add(1);
        });
    }

    fn cssom_host(&self) -> SharedCssomHost {
        self.cssom.clone()
    }

    fn metrics(&self) -> DomBindingMetrics {
        self.metrics.borrow().clone()
    }
}

impl ResearchNode {
    fn snapshot(&self) -> DomNodeSnapshot {
        DomNodeSnapshot {
            id: self.id,
            parent: self.parent,
            children: self.children.clone(),
            node_type: self.node_type,
            tag_name: self.tag_name.clone(),
            text: self.text.clone(),
            attributes: self.attributes.clone(),
        }
    }
}

fn matches_selector(node: &ResearchNode, selector: &str) -> bool {
    let selector = selector.trim();

    if selector.is_empty() {
        return false;
    }

    if selector == "body" {
        return node.tag_name == "body";
    }

    if let Some(id) = selector.strip_prefix('#') {
        return node.attributes.get("id").map(String::as_str) == Some(id);
    }

    if let Some(class) = selector.strip_prefix('.') {
        return node
            .attributes
            .get("class")
            .is_some_and(|value| value.split_whitespace().any(|entry| entry == class));
    }

    if let Some((tag, class)) = selector.split_once('.') {
        return node.tag_name == tag.to_ascii_lowercase()
            && node
                .attributes
                .get("class")
                .is_some_and(|value| value.split_whitespace().any(|entry| entry == class));
    }

    if let Some((tag, id)) = selector.split_once('#') {
        return node.tag_name == tag.to_ascii_lowercase()
            && node.attributes.get("id").map(String::as_str) == Some(id);
    }

    node.tag_name == selector.to_ascii_lowercase()
}

fn collect_text(nodes: &BTreeMap<DomNodeRef, ResearchNode>, node: DomNodeRef) -> String {
    let Some(node) = nodes.get(&node) else {
        return String::new();
    };

    if node.children.is_empty() {
        return node.text.clone();
    }

    let mut output = node.text.clone();
    for child in &node.children {
        output.push_str(&collect_text(nodes, *child));
    }
    output
}
