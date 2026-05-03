#![allow(clippy::too_many_lines)]
#![doc = "Web API host bindings for SylJS: fetch, XHR, storage, cookies, history, URL, and location."]

use crate::{
    create_rejected_promise_value, create_resolved_promise_value, JsEventLoop, JsFunction,
    JsHostObject, JsNativeFunction, JsObject, JsObjectKind, JsRuntimeError, JsValue, Vm,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Shared Web API host pointer.
pub type SharedWebApiHost = Rc<dyn WebApiHost>;

/// Web storage area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StorageArea {
    /// Persistent origin storage.
    Local,

    /// Per-tab/session storage.
    Session,
}

/// Web API metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebApiMetrics {
    /// Fetch calls.
    pub fetch_calls: u64,

    /// XHR sends.
    pub xhr_sends: u64,

    /// Storage reads.
    pub storage_reads: u64,

    /// Storage writes.
    pub storage_writes: u64,

    /// Storage removals.
    pub storage_removals: u64,

    /// Cookie reads.
    pub cookie_reads: u64,

    /// Cookie writes.
    pub cookie_writes: u64,

    /// History pushes.
    pub history_pushes: u64,

    /// History replaces.
    pub history_replaces: u64,

    /// Location navigations.
    pub location_navigations: u64,

    /// URL objects created.
    pub urls_created: u64,

    /// URLSearchParams objects created.
    pub url_search_params_created: u64,
}

/// Fetch record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRecord {
    /// URL.
    pub url: String,

    /// Method.
    pub method: String,
}

/// XHR record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XhrRecord {
    /// Method.
    pub method: String,

    /// URL.
    pub url: String,

    /// Status.
    pub status: u16,
}

/// Cookie record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieRecord {
    /// Raw cookie assignment.
    pub raw: String,
}

/// History record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRecord {
    /// API: pushState or replaceState.
    pub kind: String,

    /// URL.
    pub url: String,
}

/// Deterministic response returned by WebApiHost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebApiResponse {
    /// Final URL.
    pub url: String,

    /// HTTP-like status code.
    pub status: u16,

    /// Status text.
    pub status_text: String,

    /// Body text.
    pub body: String,

    /// Headers.
    pub headers: BTreeMap<String, String>,
}

impl WebApiResponse {
    /// Creates a 200 text response.
    #[must_use]
    pub fn text(url: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            status: 200,
            status_text: "OK".to_owned(),
            body: body.into(),
            headers: BTreeMap::new(),
        }
    }

    /// Returns success flag.
    #[must_use]
    pub const fn ok(&self) -> bool {
        self.status >= 200 && self.status < 300
    }
}

/// Host abstraction for Web APIs.
pub trait WebApiHost {
    /// Current origin.
    fn origin(&self) -> String;

    /// Current location href.
    fn location_href(&self) -> String;

    /// Sets/replaces location.
    fn set_location_href(&self, href: String);

    /// Resolves a URL against current location.
    fn resolve_url(&self, input: &str) -> String;

    /// Performs deterministic fetch.
    fn fetch(&self, url: &str, method: &str) -> Result<WebApiResponse, JsRuntimeError>;

    /// Gets storage value.
    fn storage_get(&self, area: StorageArea, key: &str) -> Option<String>;

    /// Sets storage value.
    fn storage_set(&self, area: StorageArea, key: &str, value: String);

    /// Removes storage value.
    fn storage_remove(&self, area: StorageArea, key: &str);

    /// Clears storage area.
    fn storage_clear(&self, area: StorageArea);

    /// Returns cookie string visible to document.cookie.
    fn cookie_string(&self) -> String;

    /// Assigns a cookie string.
    fn set_cookie(&self, raw: &str);

    /// Pushes history state.
    fn history_push(&self, url: String);

    /// Replaces history state.
    fn history_replace(&self, url: String);

    /// Returns metrics.
    fn metrics(&self) -> WebApiMetrics;
}

/// Installs fetch, XHR, storage, cookie, history, location, URL, URLSearchParams, navigator.
pub fn install_web_api_globals(
    vm: &mut Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    host: SharedWebApiHost,
) {
    vm.define_global(
        "fetch",
        create_fetch_function(host.clone(), event_loop.clone()),
    );
    vm.define_global("XMLHttpRequest", create_xhr_constructor(host.clone()));
    vm.define_global(
        "localStorage",
        create_storage_object(host.clone(), StorageArea::Local),
    );
    vm.define_global(
        "sessionStorage",
        create_storage_object(host.clone(), StorageArea::Session),
    );
    vm.define_global("history", create_history_object(host.clone()));
    vm.define_global("location", create_location_object(host.clone()));
    vm.define_global("URL", create_url_constructor(host.clone()));
    vm.define_global(
        "URLSearchParams",
        create_url_search_params_constructor(host.clone()),
    );
    vm.define_global("navigator", create_navigator_object());

    // If DOM bindings already installed a document/window, attach cookie/location.
    let document = vm.get_name("document");
    if !matches!(document, JsValue::Undefined | JsValue::Null) {
        document.set_property("cookie", JsValue::String(host.cookie_string()));
    }

    let window = vm.get_name("window");
    if !matches!(window, JsValue::Undefined | JsValue::Null) {
        window.set_property("location", create_location_object(host));
    }
}

fn create_fetch_function(host: SharedWebApiHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "fetch".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let url = args.first().map_or_else(String::new, JsValue::to_js_string);
            let resolved = host.resolve_url(&url);

            match host.fetch(&resolved, "GET") {
                Ok(response) => {
                    let response_object = create_response_object(event_loop.clone(), response);
                    Ok(create_resolved_promise_value(
                        event_loop.clone(),
                        response_object,
                    ))
                }
                Err(error) => Ok(create_rejected_promise_value(
                    event_loop.clone(),
                    JsValue::String(error.to_string()),
                )),
            }
        }),
    })
}

fn create_response_object(
    event_loop: Rc<RefCell<JsEventLoop>>,
    response: WebApiResponse,
) -> JsValue {
    let body_for_text = response.body.clone();
    let body_for_json = response.body.clone();
    let ok = response.ok();

    let object = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
    object.set_property("url", JsValue::String(response.url));
    object.set_property("status", JsValue::Number(f64::from(response.status)));
    object.set_property("statusText", JsValue::String(response.status_text));
    object.set_property("ok", JsValue::Boolean(ok));

    let text_loop = event_loop.clone();
    let text_fn: JsNativeFunction = Rc::new(move |_vm, _this, _args| {
        Ok(create_resolved_promise_value(
            text_loop.clone(),
            JsValue::String(body_for_text.clone()),
        ))
    });

    let json_loop = event_loop;
    let json_fn: JsNativeFunction = Rc::new(move |_vm, _this, _args| {
        let parsed = parse_json_lite(&body_for_json)
            .unwrap_or_else(|| JsValue::String(body_for_json.clone()));
        Ok(create_resolved_promise_value(json_loop.clone(), parsed))
    });

    object.set_property(
        "text",
        JsValue::function(JsFunction::Native {
            name: "Response.text".to_owned(),
            function: text_fn,
        }),
    );
    object.set_property(
        "json",
        JsValue::function(JsFunction::Native {
            name: "Response.json".to_owned(),
            function: json_fn,
        }),
    );

    object
}

fn create_xhr_constructor(host: SharedWebApiHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "XMLHttpRequest".to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(create_xhr_object(host.clone()))),
    })
}

fn create_xhr_object(host: SharedWebApiHost) -> JsValue {
    let state = Rc::new(RefCell::new(XhrState::default()));
    let object = JsValue::host_object(
        Rc::new(XhrHostObject {
            host: host.clone(),
            state: state.clone(),
        }),
        "[object XMLHttpRequest]",
    );

    object.set_property("readyState", JsValue::Number(0.0));
    object.set_property("status", JsValue::Number(0.0));
    object.set_property("responseText", JsValue::String(String::new()));
    object.set_property("onload", JsValue::Null);
    object.set_property("onerror", JsValue::Null);

    object
}

#[derive(Debug, Clone)]
struct XhrState {
    method: String,
    url: String,
}

impl Default for XhrState {
    fn default() -> Self {
        Self {
            method: "GET".to_owned(),
            url: String::new(),
        }
    }
}

#[derive(Clone)]
struct XhrHostObject {
    host: SharedWebApiHost,
    state: Rc<RefCell<XhrState>>,
}

impl JsHostObject for XhrHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "open" => Some(xhr_open(self.state.clone())),
            "send" => Some(xhr_send(self.host.clone(), self.state.clone())),
            "setRequestHeader" => Some(native_noop("XMLHttpRequest.setRequestHeader")),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn xhr_open(state: Rc<RefCell<XhrState>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "XMLHttpRequest.open".to_owned(),
        function: Rc::new(move |_vm, this, args| {
            let method = args
                .first()
                .map_or_else(|| "GET".to_owned(), JsValue::to_js_string);
            let url = args.get(1).map_or_else(String::new, JsValue::to_js_string);

            *state.borrow_mut() = XhrState { method, url };

            this.set_property("readyState", JsValue::Number(1.0));
            Ok(JsValue::Undefined)
        }),
    })
}

fn xhr_send(host: SharedWebApiHost, state: Rc<RefCell<XhrState>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "XMLHttpRequest.send".to_owned(),
        function: Rc::new(move |vm, this, _args| {
            let state = state.borrow().clone();
            let url = host.resolve_url(&state.url);

            match host.fetch(&url, &state.method) {
                Ok(response) => {
                    this.set_property("readyState", JsValue::Number(4.0));
                    this.set_property("status", JsValue::Number(f64::from(response.status)));
                    this.set_property("responseText", JsValue::String(response.body));
                    let onload = this.get_property("onload");
                    if onload.as_function().is_some() {
                        let _ = vm.call_function(onload, this.clone(), Vec::new())?;
                    }
                }
                Err(error) => {
                    this.set_property("readyState", JsValue::Number(4.0));
                    this.set_property("status", JsValue::Number(0.0));
                    this.set_property("responseText", JsValue::String(error.to_string()));
                    let onerror = this.get_property("onerror");
                    if onerror.as_function().is_some() {
                        let _ = vm.call_function(onerror, this.clone(), Vec::new())?;
                    }
                }
            }

            Ok(JsValue::Undefined)
        }),
    })
}

fn create_storage_object(host: SharedWebApiHost, area: StorageArea) -> JsValue {
    JsValue::host_object(
        Rc::new(StorageHostObject { host, area }),
        match area {
            StorageArea::Local => "[object Storage:localStorage]",
            StorageArea::Session => "[object Storage:sessionStorage]",
        },
    )
}

#[derive(Clone)]
struct StorageHostObject {
    host: SharedWebApiHost,
    area: StorageArea,
}

impl JsHostObject for StorageHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "getItem" => Some(storage_get_item(self.host.clone(), self.area)),
            "setItem" => Some(storage_set_item(self.host.clone(), self.area)),
            "removeItem" => Some(storage_remove_item(self.host.clone(), self.area)),
            "clear" => Some(storage_clear(self.host.clone(), self.area)),
            _ => self.host.storage_get(self.area, key).map(JsValue::String),
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        self.host.storage_set(self.area, key, value.to_js_string());
        true
    }
}

fn storage_get_item(host: SharedWebApiHost, area: StorageArea) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Storage.getItem".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let key = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(host
                .storage_get(area, &key)
                .map_or(JsValue::Null, JsValue::String))
        }),
    })
}

fn storage_set_item(host: SharedWebApiHost, area: StorageArea) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Storage.setItem".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let key = args.first().map_or_else(String::new, JsValue::to_js_string);
            let value = args.get(1).map_or_else(String::new, JsValue::to_js_string);
            host.storage_set(area, &key, value);
            Ok(JsValue::Undefined)
        }),
    })
}

fn storage_remove_item(host: SharedWebApiHost, area: StorageArea) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Storage.removeItem".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let key = args.first().map_or_else(String::new, JsValue::to_js_string);
            host.storage_remove(area, &key);
            Ok(JsValue::Undefined)
        }),
    })
}

fn storage_clear(host: SharedWebApiHost, area: StorageArea) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Storage.clear".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            host.storage_clear(area);
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_history_object(host: SharedWebApiHost) -> JsValue {
    JsValue::host_object(Rc::new(HistoryHostObject { host }), "[object History]")
}

#[derive(Clone)]
struct HistoryHostObject {
    host: SharedWebApiHost,
}

impl JsHostObject for HistoryHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "pushState" => Some(history_push_state(self.host.clone())),
            "replaceState" => Some(history_replace_state(self.host.clone())),
            "length" => Some(JsValue::Number(1.0)),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn history_push_state(host: SharedWebApiHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "history.pushState".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let url = args
                .get(2)
                .map_or_else(|| host.location_href(), JsValue::to_js_string);
            let resolved = host.resolve_url(&url);
            host.history_push(resolved);
            Ok(JsValue::Undefined)
        }),
    })
}

fn history_replace_state(host: SharedWebApiHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "history.replaceState".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let url = args
                .get(2)
                .map_or_else(|| host.location_href(), JsValue::to_js_string);
            let resolved = host.resolve_url(&url);
            host.history_replace(resolved);
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_location_object(host: SharedWebApiHost) -> JsValue {
    JsValue::host_object(Rc::new(LocationHostObject { host }), "[object Location]")
}

#[derive(Clone)]
struct LocationHostObject {
    host: SharedWebApiHost,
}

impl JsHostObject for LocationHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "href" => Some(JsValue::String(self.host.location_href())),
            "origin" => Some(JsValue::String(self.host.origin())),
            "assign" => Some(location_assign(self.host.clone())),
            "replace" => Some(location_replace(self.host.clone())),
            "reload" => Some(native_noop("location.reload")),
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        if key == "href" {
            let url = self.host.resolve_url(&value.to_js_string());
            self.host.set_location_href(url);
            true
        } else {
            false
        }
    }
}

fn location_assign(host: SharedWebApiHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "location.assign".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let url = args.first().map_or_else(String::new, JsValue::to_js_string);
            let resolved = host.resolve_url(&url);
            host.set_location_href(resolved);
            Ok(JsValue::Undefined)
        }),
    })
}

fn location_replace(host: SharedWebApiHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "location.replace".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let url = args.first().map_or_else(String::new, JsValue::to_js_string);
            let resolved = host.resolve_url(&url);
            host.history_replace(resolved.clone());
            host.set_location_href(resolved);
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_url_constructor(host: SharedWebApiHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "URL".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let input = args.first().map_or_else(String::new, JsValue::to_js_string);
            let resolved = host.resolve_url(&input);
            Ok(create_url_object(resolved))
        }),
    })
}

fn create_url_object(href: String) -> JsValue {
    let object = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
    object.set_property("href", JsValue::String(href.clone()));
    object.set_property("toString", native_return_string("URL.toString", href));
    object
}

fn create_url_search_params_constructor(_host: SharedWebApiHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "URLSearchParams".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let raw = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(create_url_search_params_object(raw))
        }),
    })
}

fn create_url_search_params_object(raw: String) -> JsValue {
    let params = parse_query_params(raw.trim_start_matches('?'));
    let object = JsValue::host_object(
        Rc::new(UrlSearchParamsHostObject {
            params: RefCell::new(params),
        }),
        "[object URLSearchParams]",
    );
    object
}

#[derive(Clone)]
struct UrlSearchParamsHostObject {
    params: RefCell<BTreeMap<String, String>>,
}

impl JsHostObject for UrlSearchParamsHostObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "get" => Some(url_params_get(self.params.clone())),
            "set" => Some(url_params_set(self.params.clone())),
            "toString" => Some(url_params_to_string(self.params.clone())),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn url_params_get(params: RefCell<BTreeMap<String, String>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "URLSearchParams.get".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let key = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(params
                .borrow()
                .get(&key)
                .cloned()
                .map_or(JsValue::Null, JsValue::String))
        }),
    })
}

fn url_params_set(params: RefCell<BTreeMap<String, String>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "URLSearchParams.set".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let key = args.first().map_or_else(String::new, JsValue::to_js_string);
            let value = args.get(1).map_or_else(String::new, JsValue::to_js_string);
            params.borrow_mut().insert(key, value);
            Ok(JsValue::Undefined)
        }),
    })
}

fn url_params_to_string(params: RefCell<BTreeMap<String, String>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "URLSearchParams.toString".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            Ok(JsValue::String(serialize_query_params(&params.borrow())))
        }),
    })
}

fn create_navigator_object() -> JsValue {
    let navigator = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
    navigator.set_property(
        "userAgent",
        JsValue::String("Sylphos/SylJS ResearchBrowser".to_owned()),
    );
    navigator.set_property("hardwareConcurrency", JsValue::Number(4.0));
    navigator.set_property("language", JsValue::String("en-US".to_owned()));
    navigator.set_property("onLine", JsValue::Boolean(true));
    navigator
}

fn native_noop(name: &'static str) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Undefined)),
    })
}

fn native_return_string(name: &'static str, value: String) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::String(value.clone()))),
    })
}

fn parse_json_lite(source: &str) -> Option<JsValue> {
    let trimmed = source.trim();

    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return Some(JsValue::String(trimmed[1..trimmed.len() - 1].to_owned()));
    }

    if trimmed == "true" {
        return Some(JsValue::Boolean(true));
    }

    if trimmed == "false" {
        return Some(JsValue::Boolean(false));
    }

    if trimmed == "null" {
        return Some(JsValue::Null);
    }

    if let Ok(number) = trimmed.parse::<f64>() {
        return Some(JsValue::Number(number));
    }

    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        let object = JsValue::object();
        let inner = &trimmed[1..trimmed.len() - 1];

        for entry in inner.split(',') {
            let Some((key, value)) = entry.split_once(':') else {
                continue;
            };

            let key = key.trim().trim_matches('"').trim_matches('\'');
            let value = value.trim().trim_matches('"').trim_matches('\'');
            object.set_property(key, JsValue::String(value.to_owned()));
        }

        return Some(object);
    }

    None
}

fn parse_query_params(raw: &str) -> BTreeMap<String, String> {
    raw.split('&')
        .filter_map(|entry| {
            if entry.is_empty() {
                return None;
            }
            let (key, value) = entry.split_once('=').unwrap_or((entry, ""));
            Some((percent_decode_lite(key), percent_decode_lite(value)))
        })
        .collect()
}

fn serialize_query_params(params: &BTreeMap<String, String>) -> String {
    params
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                percent_encode_lite(key),
                percent_encode_lite(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_decode_lite(input: &str) -> String {
    input.replace('+', " ")
}

fn percent_encode_lite(input: &str) -> String {
    input.replace(' ', "+")
}

/// Deterministic research Web API host.
#[derive(Debug)]
pub struct ResearchWebApiHost {
    origin: String,
    location: RefCell<String>,
    routes: RefCell<BTreeMap<String, WebApiResponse>>,
    local_storage: RefCell<BTreeMap<String, String>>,
    session_storage: RefCell<BTreeMap<String, String>>,
    cookies: RefCell<BTreeMap<String, String>>,
    fetches: RefCell<Vec<FetchRecord>>,
    xhrs: RefCell<Vec<XhrRecord>>,
    cookie_records: RefCell<Vec<CookieRecord>>,
    history: RefCell<Vec<HistoryRecord>>,
    metrics: RefCell<WebApiMetrics>,
}

impl Default for ResearchWebApiHost {
    fn default() -> Self {
        Self::new("https://sylphos.local/")
    }
}

impl ResearchWebApiHost {
    /// Creates a research host.
    #[must_use]
    pub fn new(location: impl Into<String>) -> Self {
        let location = location.into();
        let origin = origin_from_url(&location);

        Self {
            origin,
            location: RefCell::new(location),
            routes: RefCell::new(BTreeMap::new()),
            local_storage: RefCell::new(BTreeMap::new()),
            session_storage: RefCell::new(BTreeMap::new()),
            cookies: RefCell::new(BTreeMap::new()),
            fetches: RefCell::new(Vec::new()),
            xhrs: RefCell::new(Vec::new()),
            cookie_records: RefCell::new(Vec::new()),
            history: RefCell::new(Vec::new()),
            metrics: RefCell::new(WebApiMetrics::default()),
        }
    }

    /// Registers deterministic route response.
    pub fn register_route(&self, url: impl Into<String>, response: WebApiResponse) {
        let url = self.resolve_url(&url.into());
        self.routes.borrow_mut().insert(url, response);
    }

    /// Fetch records.
    #[must_use]
    pub fn fetch_records(&self) -> Vec<FetchRecord> {
        self.fetches.borrow().clone()
    }

    /// XHR records.
    #[must_use]
    pub fn xhr_records(&self) -> Vec<XhrRecord> {
        self.xhrs.borrow().clone()
    }

    /// History records.
    #[must_use]
    pub fn history_records(&self) -> Vec<HistoryRecord> {
        self.history.borrow().clone()
    }

    /// Cookie records.
    #[must_use]
    pub fn cookie_records(&self) -> Vec<CookieRecord> {
        self.cookie_records.borrow().clone()
    }

    fn bump_metrics(&self, update: impl FnOnce(&mut WebApiMetrics)) {
        update(&mut self.metrics.borrow_mut());
    }
}

impl WebApiHost for ResearchWebApiHost {
    fn origin(&self) -> String {
        self.origin.clone()
    }

    fn location_href(&self) -> String {
        self.location.borrow().clone()
    }

    fn set_location_href(&self, href: String) {
        *self.location.borrow_mut() = self.resolve_url(&href);
        self.bump_metrics(|metrics| {
            metrics.location_navigations = metrics.location_navigations.saturating_add(1);
        });
    }

    fn resolve_url(&self, input: &str) -> String {
        resolve_url_lite(&self.location.borrow(), input)
    }

    fn fetch(&self, url: &str, method: &str) -> Result<WebApiResponse, JsRuntimeError> {
        let resolved = self.resolve_url(url);
        self.fetches.borrow_mut().push(FetchRecord {
            url: resolved.clone(),
            method: method.to_ascii_uppercase(),
        });
        self.bump_metrics(|metrics| {
            metrics.fetch_calls = metrics.fetch_calls.saturating_add(1);
        });

        let response = self
            .routes
            .borrow()
            .get(&resolved)
            .cloned()
            .unwrap_or_else(|| {
                WebApiResponse::text(resolved.clone(), format!("response:{resolved}"))
            });

        if method.eq_ignore_ascii_case("GET") {
            Ok(response)
        } else {
            Ok(WebApiResponse {
                status: 405,
                status_text: "Method Not Allowed".to_owned(),
                ..response
            })
        }
    }

    fn storage_get(&self, area: StorageArea, key: &str) -> Option<String> {
        self.bump_metrics(|metrics| {
            metrics.storage_reads = metrics.storage_reads.saturating_add(1);
        });
        match area {
            StorageArea::Local => self.local_storage.borrow().get(key).cloned(),
            StorageArea::Session => self.session_storage.borrow().get(key).cloned(),
        }
    }

    fn storage_set(&self, area: StorageArea, key: &str, value: String) {
        self.bump_metrics(|metrics| {
            metrics.storage_writes = metrics.storage_writes.saturating_add(1);
        });
        match area {
            StorageArea::Local => self
                .local_storage
                .borrow_mut()
                .insert(key.to_owned(), value),
            StorageArea::Session => self
                .session_storage
                .borrow_mut()
                .insert(key.to_owned(), value),
        };
    }

    fn storage_remove(&self, area: StorageArea, key: &str) {
        self.bump_metrics(|metrics| {
            metrics.storage_removals = metrics.storage_removals.saturating_add(1);
        });
        match area {
            StorageArea::Local => self.local_storage.borrow_mut().remove(key),
            StorageArea::Session => self.session_storage.borrow_mut().remove(key),
        };
    }

    fn storage_clear(&self, area: StorageArea) {
        self.bump_metrics(|metrics| {
            metrics.storage_removals = metrics.storage_removals.saturating_add(1);
        });
        match area {
            StorageArea::Local => self.local_storage.borrow_mut().clear(),
            StorageArea::Session => self.session_storage.borrow_mut().clear(),
        }
    }

    fn cookie_string(&self) -> String {
        self.bump_metrics(|metrics| {
            metrics.cookie_reads = metrics.cookie_reads.saturating_add(1);
        });
        self.cookies
            .borrow()
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn set_cookie(&self, raw: &str) {
        self.cookie_records.borrow_mut().push(CookieRecord {
            raw: raw.to_owned(),
        });
        self.bump_metrics(|metrics| {
            metrics.cookie_writes = metrics.cookie_writes.saturating_add(1);
        });

        let first = raw.split(';').next().unwrap_or_default();
        if let Some((name, value)) = first.split_once('=') {
            self.cookies
                .borrow_mut()
                .insert(name.trim().to_owned(), value.trim().to_owned());
        }
    }

    fn history_push(&self, url: String) {
        self.history.borrow_mut().push(HistoryRecord {
            kind: "pushState".to_owned(),
            url: url.clone(),
        });
        *self.location.borrow_mut() = url;
        self.bump_metrics(|metrics| {
            metrics.history_pushes = metrics.history_pushes.saturating_add(1);
        });
    }

    fn history_replace(&self, url: String) {
        self.history.borrow_mut().push(HistoryRecord {
            kind: "replaceState".to_owned(),
            url: url.clone(),
        });
        *self.location.borrow_mut() = url;
        self.bump_metrics(|metrics| {
            metrics.history_replaces = metrics.history_replaces.saturating_add(1);
        });
    }

    fn metrics(&self) -> WebApiMetrics {
        self.metrics.borrow().clone()
    }
}

fn resolve_url_lite(base: &str, input: &str) -> String {
    let input = input.trim();

    if input.starts_with("http://") || input.starts_with("https://") || input.starts_with("about:")
    {
        return input.to_owned();
    }

    if input.starts_with("//") {
        return format!("https:{input}");
    }

    let origin = origin_from_url(base);

    if input.starts_with('/') {
        return format!("{origin}{input}");
    }

    let mut prefix = base.to_owned();
    if !prefix.ends_with('/') {
        if let Some(index) = prefix.rfind('/') {
            prefix.truncate(index + 1);
        } else {
            prefix.push('/');
        }
    }

    format!("{prefix}{input}")
}

fn origin_from_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "about:blank".to_owned();
    };

    let host = rest.split('/').next().unwrap_or_default();
    format!("{scheme}://{host}")
}
