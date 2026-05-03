#![doc = "Runtime values, objects, functions, host objects, and errors for SylJS."]

use crate::{BytecodeFunction, SylJsError};
use std::{cell::RefCell, collections::BTreeMap, fmt, rc::Rc};

/// Result returned by native host functions.
pub type NativeResult = Result<JsValue, JsRuntimeError>;

/// Native function signature.
pub type JsNativeFunction = Rc<dyn Fn(&mut crate::Vm, JsValue, Vec<JsValue>) -> NativeResult>;

/// Dynamic host-object hook.
///
/// SylJS uses this to expose browser objects such as `document`, `Element`,
/// `classList`, and `Event` without baking DOM logic into the VM. The VM only
/// knows about property get/set and function calls. The browser bindings own the
/// glorious swamp of web behavior, as nature maliciously intended.
pub trait JsHostObject {
    /// Returns a dynamic property, if the host object handles it.
    fn get_property(&self, key: &str) -> Option<JsValue>;

    /// Sets a dynamic property.
    ///
    /// Return `true` when the property was handled by the host object.
    fn set_property(&self, key: &str, value: JsValue) -> bool;
}

/// JavaScript runtime value.
#[derive(Clone)]
pub enum JsValue {
    /// `undefined`
    Undefined,

    /// `null`
    Null,

    /// Boolean.
    Boolean(bool),

    /// Number.
    Number(f64),

    /// String.
    String(String),

    /// Object/function/array/host object.
    Object(Rc<RefCell<JsObject>>),
}

impl fmt::Debug for JsValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undefined => formatter.write_str("undefined"),
            Self::Null => formatter.write_str("null"),
            Self::Boolean(value) => write!(formatter, "{value}"),
            Self::Number(value) => write!(formatter, "{value}"),
            Self::String(value) => write!(formatter, "{value:?}"),
            Self::Object(object) => {
                let object = object.borrow();
                write!(formatter, "[object {:?}]", object.kind)
            }
        }
    }
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Undefined, Self::Undefined) | (Self::Null, Self::Null) => true,
            (Self::Boolean(left), Self::Boolean(right)) => left == right,
            (Self::Number(left), Self::Number(right)) => left == right,
            (Self::String(left), Self::String(right)) => left == right,
            (Self::Object(left), Self::Object(right)) => Rc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl JsValue {
    /// Returns JavaScript truthiness.
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Boolean(value) => *value,
            Self::Number(value) => *value != 0.0 && !value.is_nan(),
            Self::String(value) => !value.is_empty(),
            Self::Object(_) => true,
        }
    }

    /// Converts to number using a small JavaScript-compatible subset.
    #[must_use]
    pub fn to_number(&self) -> f64 {
        match self {
            Self::Undefined => f64::NAN,
            Self::Null => 0.0,
            Self::Boolean(value) => {
                if *value {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Number(value) => *value,
            Self::String(value) => value.trim().parse::<f64>().unwrap_or(f64::NAN),
            Self::Object(_) => f64::NAN,
        }
    }

    /// Converts to string using a small JavaScript-compatible subset.
    #[must_use]
    pub fn to_js_string(&self) -> String {
        match self {
            Self::Undefined => "undefined".to_owned(),
            Self::Null => "null".to_owned(),
            Self::Boolean(value) => value.to_string(),
            Self::Number(value) => {
                if value.fract() == 0.0 && value.is_finite() {
                    format!("{value:.0}")
                } else {
                    value.to_string()
                }
            }
            Self::String(value) => value.clone(),
            Self::Object(object) => object.borrow().display_name(),
        }
    }

    /// Creates a plain object.
    #[must_use]
    pub fn object() -> Self {
        Self::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Object))))
    }

    /// Creates a host-backed object.
    #[must_use]
    pub fn host_object(host: Rc<dyn JsHostObject>, display_name: impl Into<String>) -> Self {
        Self::Object(Rc::new(RefCell::new(JsObject::host(host, display_name))))
    }

    /// Creates an array object.
    #[must_use]
    pub fn array(items: Vec<JsValue>) -> Self {
        let object = Rc::new(RefCell::new(JsObject::new(JsObjectKind::Array)));
        let len = items.len();

        for (index, item) in items.into_iter().enumerate() {
            object.borrow_mut().set(index.to_string(), item);
        }

        object
            .borrow_mut()
            .set("length", JsValue::Number(len as f64));
        Self::Object(object)
    }

    /// Creates a function object.
    #[must_use]
    pub fn function(function: JsFunction) -> Self {
        Self::Object(Rc::new(RefCell::new(JsObject::function(function))))
    }

    /// Returns property from value.
    #[must_use]
    pub fn get_property(&self, key: &str) -> JsValue {
        match self {
            Self::Object(object) => object.borrow().get(key),
            Self::String(value) if key == "length" => JsValue::Number(value.chars().count() as f64),
            _ => JsValue::Undefined,
        }
    }

    /// Sets property on value. Non-object targets are ignored.
    pub fn set_property(&self, key: impl Into<String>, value: JsValue) {
        if let Self::Object(object) = self {
            object.borrow_mut().set(key, value);
        }
    }

    /// Returns function kind if this value is callable.
    #[must_use]
    pub fn as_function(&self) -> Option<JsFunction> {
        let Self::Object(object) = self else {
            return None;
        };

        object.borrow().function.clone()
    }

    /// Extracts internal numeric DOM node id when present.
    #[must_use]
    pub fn dom_node_id(&self) -> Option<u64> {
        let value = self.get_property("__syljsDomNodeId");
        let number = value.to_number();

        if number.is_finite() && number > 0.0 {
            Some(number as u64)
        } else {
            None
        }
    }
}

impl From<&str> for JsValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for JsValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<f64> for JsValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<bool> for JsValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

/// Runtime object.
#[derive(Clone)]
pub struct JsObject {
    /// Object kind.
    pub kind: JsObjectKind,

    /// Own properties.
    pub properties: BTreeMap<String, JsValue>,

    /// Optional function payload.
    pub function: Option<JsFunction>,

    /// Optional host property hook.
    pub host: Option<Rc<dyn JsHostObject>>,

    display_name: Option<String>,
}

impl fmt::Debug for JsObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JsObject")
            .field("kind", &self.kind)
            .field("properties", &self.properties.keys().collect::<Vec<_>>())
            .field("function", &self.function.as_ref().map(JsFunction::name))
            .field("host", &self.host.is_some())
            .finish()
    }
}

impl JsObject {
    /// Creates a new object.
    #[must_use]
    pub fn new(kind: JsObjectKind) -> Self {
        Self {
            kind,
            properties: BTreeMap::new(),
            function: None,
            host: None,
            display_name: None,
        }
    }

    /// Creates a host object.
    #[must_use]
    pub fn host(host: Rc<dyn JsHostObject>, display_name: impl Into<String>) -> Self {
        Self {
            kind: JsObjectKind::Host,
            properties: BTreeMap::new(),
            function: None,
            host: Some(host),
            display_name: Some(display_name.into()),
        }
    }

    /// Creates a function object.
    #[must_use]
    pub fn function(function: JsFunction) -> Self {
        Self {
            kind: JsObjectKind::Function,
            properties: BTreeMap::new(),
            function: Some(function),
            host: None,
            display_name: None,
        }
    }

    /// Gets an own or host property.
    #[must_use]
    pub fn get(&self, key: &str) -> JsValue {
        if let Some(value) = self.properties.get(key) {
            return value.clone();
        }

        if let Some(host) = &self.host {
            if let Some(value) = host.get_property(key) {
                return value;
            }
        }

        JsValue::Undefined
    }

    /// Sets an own or host property.
    pub fn set(&mut self, key: impl Into<String>, value: JsValue) {
        let key = key.into();

        if let Some(host) = &self.host {
            if host.set_property(&key, value.clone()) {
                return;
            }
        }

        self.properties.insert(key, value);
    }

    /// Returns display name.
    #[must_use]
    pub fn display_name(&self) -> String {
        if let Some(display_name) = &self.display_name {
            return display_name.clone();
        }

        match self.kind {
            JsObjectKind::Object => "[object Object]".to_owned(),
            JsObjectKind::Array => "[object Array]".to_owned(),
            JsObjectKind::Function => "[function]".to_owned(),
            JsObjectKind::Host => "[object Host]".to_owned(),
        }
    }
}

/// Object kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsObjectKind {
    /// Plain object.
    Object,

    /// Array object.
    Array,

    /// Function object.
    Function,

    /// Host object.
    Host,
}

/// Function payload.
#[derive(Clone)]
pub enum JsFunction {
    /// Bytecode function.
    Bytecode(Rc<BytecodeFunction>),

    /// Native host function.
    Native {
        /// Function name.
        name: String,

        /// Function implementation.
        function: JsNativeFunction,
    },
}

impl fmt::Debug for JsFunction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytecode(function) => formatter
                .debug_struct("Bytecode")
                .field("name", &function.name)
                .finish(),
            Self::Native { name, .. } => formatter
                .debug_struct("Native")
                .field("name", name)
                .finish(),
        }
    }
}

impl PartialEq for JsFunction {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl JsFunction {
    /// Returns function display name.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Bytecode(function) => function
                .name
                .clone()
                .unwrap_or_else(|| "<anonymous>".to_owned()),
            Self::Native { name, .. } => name.clone(),
        }
    }
}

/// Runtime error.
#[derive(Debug, Clone, PartialEq)]
pub struct JsRuntimeError {
    /// Error message.
    pub message: String,
}

impl JsRuntimeError {
    /// Creates a runtime error.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Converts frontend errors into runtime errors for bridge APIs.
    #[must_use]
    pub fn from_frontend_error(error: SylJsError) -> Self {
        Self::new(format!("frontend error: {error}"))
    }
}

impl std::fmt::Display for JsRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for JsRuntimeError {}

impl From<crate::CompileError> for JsRuntimeError {
    fn from(error: crate::CompileError) -> Self {
        Self::new(error.message)
    }
}
