#![doc = "Promise-lite host implementation for SylJS."]

use crate::{
    event_loop::JsEventLoop, JsFunction, JsNativeFunction, JsObject, JsObjectKind, JsRuntimeError,
    JsValue,
};
use std::{cell::RefCell, rc::Rc};

/// Promise state used for diagnostics and future VM evolution.
#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    /// Promise is pending.
    Pending,

    /// Promise fulfilled with value.
    Fulfilled(JsValue),

    /// Promise rejected with reason.
    Rejected(JsValue),
}

/// Promise inspection snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct PromiseInspection {
    /// Promise state.
    pub state: PromiseState,
}

/// Creates the global `Promise` constructor object.
#[must_use]
pub fn create_promise_constructor(event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    let constructor = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));

    let resolve_loop = event_loop.clone();
    let resolve: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        let value = args.first().cloned().unwrap_or(JsValue::Undefined);
        Ok(create_resolved_promise_value(resolve_loop.clone(), value))
    });

    let reject_loop = event_loop;
    let reject: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        let reason = args.first().cloned().unwrap_or(JsValue::Undefined);
        Ok(create_rejected_promise_value(reject_loop.clone(), reason))
    });

    constructor.set_property(
        "resolve",
        JsValue::function(JsFunction::Native {
            name: "Promise.resolve".to_owned(),
            function: resolve,
        }),
    );
    constructor.set_property(
        "reject",
        JsValue::function(JsFunction::Native {
            name: "Promise.reject".to_owned(),
            function: reject,
        }),
    );

    constructor
}

/// Creates an already fulfilled Promise-lite value.
#[must_use]
pub fn create_resolved_promise_value(
    event_loop: Rc<RefCell<JsEventLoop>>,
    value: JsValue,
) -> JsValue {
    create_settled_promise(event_loop, PromiseState::Fulfilled(value))
}

/// Creates an already rejected Promise-lite value.
#[must_use]
pub fn create_rejected_promise_value(
    event_loop: Rc<RefCell<JsEventLoop>>,
    reason: JsValue,
) -> JsValue {
    create_settled_promise(event_loop, PromiseState::Rejected(reason))
}

fn create_settled_promise(event_loop: Rc<RefCell<JsEventLoop>>, state: PromiseState) -> JsValue {
    let object = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));

    let then_state = state.clone();
    let then_object = object.clone();
    let then_loop = event_loop.clone();
    let then_fn: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        match &then_state {
            PromiseState::Fulfilled(value) => {
                if let Some(callback) = args.first().cloned() {
                    if callback.as_function().is_some() {
                        then_loop
                            .borrow_mut()
                            .queue_promise_reaction(callback, vec![value.clone()]);
                    }
                }
            }
            PromiseState::Rejected(_) | PromiseState::Pending => {}
        }
        Ok(then_object.clone())
    });

    let catch_state = state.clone();
    let catch_object = object.clone();
    let catch_loop = event_loop.clone();
    let catch_fn: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        match &catch_state {
            PromiseState::Rejected(reason) => {
                if let Some(callback) = args.first().cloned() {
                    if callback.as_function().is_some() {
                        catch_loop
                            .borrow_mut()
                            .queue_promise_reaction(callback, vec![reason.clone()]);
                    }
                }
            }
            PromiseState::Fulfilled(_) | PromiseState::Pending => {}
        }
        Ok(catch_object.clone())
    });

    let finally_object = object.clone();
    let finally_loop = event_loop;
    let finally_fn: JsNativeFunction = Rc::new(move |_vm, _this, args| {
        if let Some(callback) = args.first().cloned() {
            if callback.as_function().is_some() {
                finally_loop
                    .borrow_mut()
                    .queue_promise_reaction(callback, Vec::new());
            }
        }
        Ok(finally_object.clone())
    });

    object.set_property(
        "then",
        JsValue::function(JsFunction::Native {
            name: "Promise.then".to_owned(),
            function: then_fn,
        }),
    );
    object.set_property(
        "catch",
        JsValue::function(JsFunction::Native {
            name: "Promise.catch".to_owned(),
            function: catch_fn,
        }),
    );
    object.set_property(
        "finally",
        JsValue::function(JsFunction::Native {
            name: "Promise.finally".to_owned(),
            function: finally_fn,
        }),
    );

    object.set_property(
        "__syljsPromiseState",
        JsValue::String(
            match state {
                PromiseState::Pending => "pending",
                PromiseState::Fulfilled(_) => "fulfilled",
                PromiseState::Rejected(_) => "rejected",
            }
            .to_owned(),
        ),
    );

    object
}

/// Returns a small promise inspection object.
pub fn inspect_promise(value: &JsValue) -> Result<PromiseInspection, JsRuntimeError> {
    let state = value.get_property("__syljsPromiseState").to_js_string();

    match state.as_str() {
        "fulfilled" => Ok(PromiseInspection {
            state: PromiseState::Fulfilled(JsValue::Undefined),
        }),
        "rejected" => Ok(PromiseInspection {
            state: PromiseState::Rejected(JsValue::Undefined),
        }),
        "pending" => Ok(PromiseInspection {
            state: PromiseState::Pending,
        }),
        _ => Err(JsRuntimeError::new("value is not a SylJS promise")),
    }
}
