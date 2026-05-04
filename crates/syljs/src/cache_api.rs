#![allow(clippy::too_many_lines)]
#![doc = "Service Worker and Cache API simulation for SylJS."]
#![doc = ""]
#![doc = "Module 44 provides a deterministic, dependency-light Service Worker"]
#![doc = "registration model plus CacheStorage/Cache/Request/Response host objects."]
#![doc = "It is deliberately conservative: it models lifecycle, scope matching,"]
#![doc = "cache mutation, fetch interception, and research metrics without trying"]
#![doc = "to pretend a tiny teaching VM is Chromium wearing a fake moustache."]

use crate::{
    create_rejected_promise_value, create_resolved_promise_value, JsEventLoop, JsFunction,
    JsHostObject, JsObject, JsObjectKind, JsRuntimeError, JsValue, Vm, WebApiResponse,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

const DEFAULT_CACHE_NAME: &str = "default";
const DEFAULT_METHOD: &str = "GET";
const MAX_CACHE_NAME_LEN: usize = 128;
const MAX_CACHE_ENTRIES_PER_CACHE: usize = 4096;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Shared Cache API host pointer.
pub type SharedCacheApiHost = Rc<dyn CacheApiHost>;

/// Shared Service Worker host pointer.
pub type SharedServiceWorkerHost = Rc<dyn ServiceWorkerHost>;

/// Stable Service Worker registration id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceWorkerRegistrationId(pub u64);

/// Cache API request key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CacheApiRequest {
    /// Absolute or caller-normalized URL.
    pub url: String,

    /// HTTP method. Cache API primarily supports GET matching, but we track the
    /// method so tests can verify that unsafe methods are not silently confused.
    pub method: String,
}

impl CacheApiRequest {
    /// Creates a sanitized request key.
    #[must_use]
    pub fn new(url: impl Into<String>, method: impl Into<String>) -> Self {
        let url = normalize_url_string(&url.into());
        let method = normalize_method(&method.into());
        Self { url, method }
    }

    /// Creates a GET request.
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self::new(url, DEFAULT_METHOD)
    }

    /// Returns true when this request is GET-like and cacheable by default.
    #[must_use]
    pub fn is_cacheable_get(&self) -> bool {
        self.method.eq_ignore_ascii_case(DEFAULT_METHOD)
    }
}

/// Cache API response payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheApiResponse {
    /// Final URL.
    pub url: String,

    /// HTTP-like status.
    pub status: u16,

    /// Status text.
    pub status_text: String,

    /// Body text.
    pub body: String,

    /// Headers, normalized to lowercase names.
    pub headers: BTreeMap<String, String>,
}

impl CacheApiResponse {
    /// Creates a text response.
    #[must_use]
    pub fn text(url: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            url: normalize_url_string(&url.into()),
            status: 200,
            status_text: "OK".to_owned(),
            body: body.into(),
            headers: BTreeMap::new(),
        }
    }

    /// Creates a response from the existing Web API response model.
    #[must_use]
    pub fn from_web_api(response: WebApiResponse) -> Self {
        Self {
            url: normalize_url_string(&response.url),
            status: response.status,
            status_text: response.status_text,
            body: response.body,
            headers: normalize_headers(response.headers),
        }
    }

    /// Converts into the existing Web API response model.
    #[must_use]
    pub fn into_web_api(self) -> WebApiResponse {
        WebApiResponse {
            url: self.url,
            status: self.status,
            status_text: self.status_text,
            body: self.body,
            headers: self.headers,
        }
    }

    /// Returns success flag.
    #[must_use]
    pub const fn ok(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// Approximate stored size.
    #[must_use]
    pub fn size_bytes(&self) -> usize {
        self.body.len().saturating_add(
            self.headers
                .iter()
                .map(|(key, value)| key.len().saturating_add(value.len()).saturating_add(4))
                .sum::<usize>(),
        )
    }
}

/// One Cache API entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheApiEntry {
    /// Request key.
    pub request: CacheApiRequest,

    /// Stored response.
    pub response: CacheApiResponse,

    /// Logical insertion revision.
    pub revision: u64,

    /// Unix timestamp millis when inserted.
    pub inserted_at_ms: u128,
}

/// Cache snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSnapshot {
    /// Cache name.
    pub name: String,

    /// Entries in deterministic URL order.
    pub entries: Vec<CacheApiEntry>,
}

/// Service Worker lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceWorkerState {
    /// Installing.
    Installing,

    /// Installed/waiting.
    Installed,

    /// Activating.
    Activating,

    /// Active and eligible to control matching clients.
    Activated,

    /// Redundant or unregistered.
    Redundant,
}

impl ServiceWorkerState {
    /// JavaScript-facing state string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Installing => "installing",
            Self::Installed => "installed",
            Self::Activating => "activating",
            Self::Activated => "activated",
            Self::Redundant => "redundant",
        }
    }
}

/// Static analysis summary for a Service Worker script.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceWorkerScriptAnalysis {
    /// Whether an install listener was detected.
    pub has_install_listener: bool,

    /// Whether an activate listener was detected.
    pub has_activate_listener: bool,

    /// Whether a fetch listener was detected.
    pub has_fetch_listener: bool,

    /// Cache names mentioned through `caches.open(...)`.
    pub cache_names: Vec<String>,

    /// URLs discovered in `cache.addAll([...])` or `cache.add(...)`.
    pub precache_urls: Vec<String>,

    /// Whether `skipWaiting()` was called.
    pub skip_waiting: bool,

    /// Whether `clients.claim()` was called.
    pub clients_claim: bool,

    /// Whether a cache-first pattern was detected.
    pub cache_first_fetch: bool,
}

/// Service Worker registration snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceWorkerRegistration {
    /// Registration id.
    pub id: ServiceWorkerRegistrationId,

    /// Script URL.
    pub script_url: String,

    /// Registration scope.
    pub scope: String,

    /// Lifecycle state.
    pub state: ServiceWorkerState,

    /// Parsed script capabilities.
    pub analysis: ServiceWorkerScriptAnalysis,

    /// Whether the worker currently controls matching pages.
    pub controls_clients: bool,

    /// Last error, if any.
    pub last_error: Option<String>,
}

/// Cache and Service Worker metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheApiMetrics {
    /// CacheStorage.open calls.
    pub cache_open_calls: u64,

    /// CacheStorage.match calls.
    pub cache_storage_match_calls: u64,

    /// Cache.put calls.
    pub cache_put_calls: u64,

    /// Cache.match calls.
    pub cache_match_calls: u64,

    /// Cache.delete calls.
    pub cache_delete_calls: u64,

    /// Cache.add or addAll synthetic fetches.
    pub cache_add_calls: u64,

    /// Cache hit count.
    pub cache_hits: u64,

    /// Cache miss count.
    pub cache_misses: u64,

    /// Service Worker registrations created or updated.
    pub service_worker_registrations: u64,

    /// Service Worker unregistrations.
    pub service_worker_unregistrations: u64,

    /// Service Worker update calls.
    pub service_worker_updates: u64,

    /// Fetches intercepted by a Service Worker.
    pub fetch_intercepts: u64,

    /// Fetches served from the Cache API.
    pub fetch_cache_hits: u64,

    /// Fetches passed through because no match existed.
    pub fetch_passthroughs: u64,
}

/// Cache API storage abstraction.
pub trait CacheApiHost {
    /// Opens or creates a named cache.
    fn open_cache(&self, name: &str) -> Result<(), JsRuntimeError>;

    /// Deletes a named cache.
    fn delete_cache(&self, name: &str) -> bool;

    /// Returns cache names.
    fn cache_names(&self) -> Vec<String>;

    /// Inserts a response.
    fn put(
        &self,
        cache_name: &str,
        request: CacheApiRequest,
        response: CacheApiResponse,
    ) -> Result<(), JsRuntimeError>;

    /// Matches inside one cache.
    fn match_in_cache(
        &self,
        cache_name: &str,
        request: &CacheApiRequest,
    ) -> Option<CacheApiResponse>;

    /// Matches across all caches in deterministic order.
    fn match_any(&self, request: &CacheApiRequest) -> Option<CacheApiResponse>;

    /// Deletes one cached request.
    fn delete_entry(&self, cache_name: &str, request: &CacheApiRequest) -> bool;

    /// Returns request keys for a cache.
    fn keys(&self, cache_name: &str) -> Vec<CacheApiRequest>;

    /// Returns snapshots.
    fn snapshots(&self) -> Vec<CacheSnapshot>;

    /// Metrics.
    fn metrics(&self) -> CacheApiMetrics;
}

/// Service Worker host abstraction.
pub trait ServiceWorkerHost {
    /// Cache API host used by this Service Worker host.
    fn cache_host(&self) -> SharedCacheApiHost;

    /// Registers or updates a Service Worker.
    fn register_service_worker(
        &self,
        script_url: String,
        scope: Option<String>,
    ) -> Result<ServiceWorkerRegistration, JsRuntimeError>;

    /// Unregisters a Service Worker by scope.
    fn unregister_scope(&self, scope: &str) -> bool;

    /// Updates a registration by scope.
    fn update_scope(
        &self,
        scope: &str,
    ) -> Result<Option<ServiceWorkerRegistration>, JsRuntimeError>;

    /// Returns matching registration for a client URL.
    fn controller_for_url(&self, url: &str) -> Option<ServiceWorkerRegistration>;

    /// Returns registration for scope.
    fn registration_for_scope(&self, scope: &str) -> Option<ServiceWorkerRegistration>;

    /// Returns all registrations.
    fn registrations(&self) -> Vec<ServiceWorkerRegistration>;

    /// Attempts to intercept a fetch.
    fn intercept_fetch(&self, request: &CacheApiRequest) -> Option<CacheApiResponse>;

    /// Metrics.
    fn metrics(&self) -> CacheApiMetrics;
}

#[derive(Debug, Clone, Default)]
struct CacheData {
    entries: BTreeMap<CacheApiRequest, CacheApiEntry>,
}

#[derive(Debug)]
struct ResearchCacheInner {
    caches: BTreeMap<String, CacheData>,
    revision: u64,
    metrics: CacheApiMetrics,
}

/// Deterministic in-memory CacheStorage implementation.
#[derive(Debug)]
pub struct ResearchCacheStorage {
    inner: RefCell<ResearchCacheInner>,
}

impl Default for ResearchCacheStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchCacheStorage {
    /// Creates empty CacheStorage.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(ResearchCacheInner {
                caches: BTreeMap::new(),
                revision: 0,
                metrics: CacheApiMetrics::default(),
            }),
        }
    }

    /// Convenience helper for tests and app bridges.
    pub fn put_text(
        &self,
        cache_name: &str,
        url: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<(), JsRuntimeError> {
        let request = CacheApiRequest::get(url.into());
        let response = CacheApiResponse::text(request.url.clone(), body.into());
        self.put(cache_name, request, response)
    }

    fn sanitize_cache_name(name: &str) -> Result<String, JsRuntimeError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Ok(DEFAULT_CACHE_NAME.to_owned());
        }
        if trimmed.len() > MAX_CACHE_NAME_LEN {
            return Err(JsRuntimeError::new(format!(
                "cache name exceeds {MAX_CACHE_NAME_LEN} bytes"
            )));
        }
        if trimmed
            .chars()
            .any(|ch| ch == '\0' || ch == '/' || ch == '\\')
        {
            return Err(JsRuntimeError::new(
                "cache name contains invalid path characters",
            ));
        }
        Ok(trimmed.to_owned())
    }

    fn bump_revision(inner: &mut ResearchCacheInner) -> u64 {
        inner.revision = inner.revision.saturating_add(1);
        inner.revision
    }
}

impl CacheApiHost for ResearchCacheStorage {
    fn open_cache(&self, name: &str) -> Result<(), JsRuntimeError> {
        let name = Self::sanitize_cache_name(name)?;
        let mut inner = self.inner.borrow_mut();
        inner.metrics.cache_open_calls = inner.metrics.cache_open_calls.saturating_add(1);
        inner.caches.entry(name).or_default();
        Ok(())
    }

    fn delete_cache(&self, name: &str) -> bool {
        let Ok(name) = Self::sanitize_cache_name(name) else {
            return false;
        };
        self.inner.borrow_mut().caches.remove(&name).is_some()
    }

    fn cache_names(&self) -> Vec<String> {
        self.inner.borrow().caches.keys().cloned().collect()
    }

    fn put(
        &self,
        cache_name: &str,
        request: CacheApiRequest,
        response: CacheApiResponse,
    ) -> Result<(), JsRuntimeError> {
        if !request.is_cacheable_get() {
            return Err(JsRuntimeError::new(
                "Cache API simulation only stores GET requests",
            ));
        }
        if response.size_bytes() > MAX_RESPONSE_BYTES {
            return Err(JsRuntimeError::new(format!(
                "cached response exceeds {MAX_RESPONSE_BYTES} bytes"
            )));
        }

        let name = Self::sanitize_cache_name(cache_name)?;
        let mut inner = self.inner.borrow_mut();
        inner.metrics.cache_put_calls = inner.metrics.cache_put_calls.saturating_add(1);
        let revision = Self::bump_revision(&mut inner);
        let cache = inner.caches.entry(name).or_default();

        if cache.entries.len() >= MAX_CACHE_ENTRIES_PER_CACHE
            && !cache.entries.contains_key(&request)
        {
            let Some(first_key) = cache.entries.keys().next().cloned() else {
                return Ok(());
            };
            cache.entries.remove(&first_key);
        }

        cache.entries.insert(
            request.clone(),
            CacheApiEntry {
                request,
                response,
                revision,
                inserted_at_ms: now_ms(),
            },
        );
        Ok(())
    }

    fn match_in_cache(
        &self,
        cache_name: &str,
        request: &CacheApiRequest,
    ) -> Option<CacheApiResponse> {
        let name = Self::sanitize_cache_name(cache_name).ok()?;
        let mut inner = self.inner.borrow_mut();
        inner.metrics.cache_match_calls = inner.metrics.cache_match_calls.saturating_add(1);
        let result = inner
            .caches
            .get(&name)
            .and_then(|cache| cache.entries.get(request))
            .map(|entry| entry.response.clone());
        if result.is_some() {
            inner.metrics.cache_hits = inner.metrics.cache_hits.saturating_add(1);
        } else {
            inner.metrics.cache_misses = inner.metrics.cache_misses.saturating_add(1);
        }
        result
    }

    fn match_any(&self, request: &CacheApiRequest) -> Option<CacheApiResponse> {
        let mut inner = self.inner.borrow_mut();
        inner.metrics.cache_storage_match_calls =
            inner.metrics.cache_storage_match_calls.saturating_add(1);
        let result = inner.caches.values().find_map(|cache| {
            cache
                .entries
                .get(request)
                .map(|entry| entry.response.clone())
        });
        if result.is_some() {
            inner.metrics.cache_hits = inner.metrics.cache_hits.saturating_add(1);
        } else {
            inner.metrics.cache_misses = inner.metrics.cache_misses.saturating_add(1);
        }
        result
    }

    fn delete_entry(&self, cache_name: &str, request: &CacheApiRequest) -> bool {
        let Ok(name) = Self::sanitize_cache_name(cache_name) else {
            return false;
        };
        let mut inner = self.inner.borrow_mut();
        inner.metrics.cache_delete_calls = inner.metrics.cache_delete_calls.saturating_add(1);
        inner
            .caches
            .get_mut(&name)
            .and_then(|cache| cache.entries.remove(request))
            .is_some()
    }

    fn keys(&self, cache_name: &str) -> Vec<CacheApiRequest> {
        let Ok(name) = Self::sanitize_cache_name(cache_name) else {
            return Vec::new();
        };
        self.inner
            .borrow()
            .caches
            .get(&name)
            .map_or_else(Vec::new, |cache| cache.entries.keys().cloned().collect())
    }

    fn snapshots(&self) -> Vec<CacheSnapshot> {
        self.inner
            .borrow()
            .caches
            .iter()
            .map(|(name, cache)| CacheSnapshot {
                name: name.clone(),
                entries: cache.entries.values().cloned().collect(),
            })
            .collect()
    }

    fn metrics(&self) -> CacheApiMetrics {
        self.inner.borrow().metrics.clone()
    }
}

/// Deterministic Service Worker host backed by `ResearchCacheStorage`.
#[derive(Debug)]
pub struct ResearchServiceWorkerHost {
    cache: Rc<ResearchCacheStorage>,
    scripts: RefCell<BTreeMap<String, String>>,
    inner: RefCell<ResearchServiceWorkerInner>,
}

#[derive(Debug)]
struct ResearchServiceWorkerInner {
    next_registration: u64,
    registrations: BTreeMap<String, ServiceWorkerRegistration>,
    metrics: CacheApiMetrics,
}

impl Default for ResearchServiceWorkerHost {
    fn default() -> Self {
        Self::new(Rc::new(ResearchCacheStorage::new()))
    }
}

impl ResearchServiceWorkerHost {
    /// Creates a host using an existing cache storage instance.
    #[must_use]
    pub fn new(cache: Rc<ResearchCacheStorage>) -> Self {
        Self {
            cache,
            scripts: RefCell::new(BTreeMap::new()),
            inner: RefCell::new(ResearchServiceWorkerInner {
                next_registration: 1,
                registrations: BTreeMap::new(),
                metrics: CacheApiMetrics::default(),
            }),
        }
    }

    /// Registers deterministic script source for a URL.
    pub fn register_script(&self, url: impl Into<String>, source: impl Into<String>) {
        self.scripts
            .borrow_mut()
            .insert(normalize_url_string(&url.into()), source.into());
    }

    /// Returns cache storage.
    #[must_use]
    pub fn cache_storage(&self) -> Rc<ResearchCacheStorage> {
        self.cache.clone()
    }

    fn load_script(&self, script_url: &str) -> Option<String> {
        let normalized = normalize_url_string(script_url);
        self.scripts.borrow().get(&normalized).cloned()
    }

    fn analyze_and_precache(
        &self,
        script_url: &str,
        scope: &str,
    ) -> Result<ServiceWorkerScriptAnalysis, JsRuntimeError> {
        let source = self.load_script(script_url).unwrap_or_default();
        let analysis = analyze_service_worker_script(&source);
        let cache_name = analysis
            .cache_names
            .first()
            .cloned()
            .unwrap_or_else(|| DEFAULT_CACHE_NAME.to_owned());

        self.cache.open_cache(&cache_name)?;
        for url in &analysis.precache_urls {
            let resolved = resolve_scope_url(scope, url);
            let body = format!("service-worker-precache:{resolved}");
            self.cache.put_text(&cache_name, resolved, body)?;
            let mut inner = self.inner.borrow_mut();
            inner.metrics.cache_add_calls = inner.metrics.cache_add_calls.saturating_add(1);
        }

        Ok(analysis)
    }
}

impl ServiceWorkerHost for ResearchServiceWorkerHost {
    fn cache_host(&self) -> SharedCacheApiHost {
        self.cache.clone()
    }

    fn register_service_worker(
        &self,
        script_url: String,
        scope: Option<String>,
    ) -> Result<ServiceWorkerRegistration, JsRuntimeError> {
        let script_url = normalize_url_string(&script_url);
        let scope = scope
            .map(|value| normalize_url_string(&value))
            .unwrap_or_else(|| infer_scope_from_script_url(&script_url));
        let analysis = self.analyze_and_precache(&script_url, &scope)?;

        let mut inner = self.inner.borrow_mut();
        inner.metrics.service_worker_registrations =
            inner.metrics.service_worker_registrations.saturating_add(1);
        let id = if let Some(registration) = inner.registrations.get(&scope) {
            registration.id
        } else {
            let id = ServiceWorkerRegistrationId(inner.next_registration);
            inner.next_registration = inner.next_registration.saturating_add(1);
            id
        };

        let controls_clients =
            analysis.clients_claim || analysis.skip_waiting || analysis.has_fetch_listener;
        let registration = ServiceWorkerRegistration {
            id,
            script_url,
            scope: scope.clone(),
            state: ServiceWorkerState::Activated,
            analysis,
            controls_clients,
            last_error: None,
        };
        inner.registrations.insert(scope, registration.clone());
        Ok(registration)
    }

    fn unregister_scope(&self, scope: &str) -> bool {
        let scope = normalize_url_string(scope);
        let mut inner = self.inner.borrow_mut();
        let removed = inner.registrations.remove(&scope).is_some();
        if removed {
            inner.metrics.service_worker_unregistrations = inner
                .metrics
                .service_worker_unregistrations
                .saturating_add(1);
        }
        removed
    }

    fn update_scope(
        &self,
        scope: &str,
    ) -> Result<Option<ServiceWorkerRegistration>, JsRuntimeError> {
        let scope = normalize_url_string(scope);
        let Some(existing) = self.inner.borrow().registrations.get(&scope).cloned() else {
            return Ok(None);
        };

        {
            let mut inner = self.inner.borrow_mut();
            inner.metrics.service_worker_updates =
                inner.metrics.service_worker_updates.saturating_add(1);
        }
        self.register_service_worker(existing.script_url, Some(scope))
            .map(Some)
    }

    fn controller_for_url(&self, url: &str) -> Option<ServiceWorkerRegistration> {
        let url = normalize_url_string(url);
        self.inner
            .borrow()
            .registrations
            .values()
            .filter(|registration| {
                registration.state == ServiceWorkerState::Activated
                    && registration.controls_clients
                    && url.starts_with(&registration.scope)
            })
            .max_by_key(|registration| registration.scope.len())
            .cloned()
    }

    fn registration_for_scope(&self, scope: &str) -> Option<ServiceWorkerRegistration> {
        self.inner
            .borrow()
            .registrations
            .get(&normalize_url_string(scope))
            .cloned()
    }

    fn registrations(&self) -> Vec<ServiceWorkerRegistration> {
        self.inner
            .borrow()
            .registrations
            .values()
            .cloned()
            .collect()
    }

    fn intercept_fetch(&self, request: &CacheApiRequest) -> Option<CacheApiResponse> {
        let controller = self.controller_for_url(&request.url)?;
        if !controller.analysis.has_fetch_listener && !controller.analysis.cache_first_fetch {
            return None;
        }

        let mut inner = self.inner.borrow_mut();
        inner.metrics.fetch_intercepts = inner.metrics.fetch_intercepts.saturating_add(1);
        drop(inner);

        let response = self.cache.match_any(request);
        let mut inner = self.inner.borrow_mut();
        if response.is_some() {
            inner.metrics.fetch_cache_hits = inner.metrics.fetch_cache_hits.saturating_add(1);
        } else {
            inner.metrics.fetch_passthroughs = inner.metrics.fetch_passthroughs.saturating_add(1);
        }
        response
    }

    fn metrics(&self) -> CacheApiMetrics {
        merge_metrics(self.cache.metrics(), self.inner.borrow().metrics.clone())
    }
}

/// Installs global `caches`, `Request`, and `Response` constructors.
pub fn install_cache_api_globals(
    vm: &mut Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    host: SharedCacheApiHost,
) {
    vm.define_global(
        "caches",
        create_cache_storage_object(host.clone(), event_loop.clone()),
    );
    vm.define_global("Request", create_request_constructor());
    vm.define_global("Response", create_response_constructor(event_loop));
    vm.define_global("__sylphosCacheMetrics", create_cache_metrics_function(host));
}

/// Installs Cache API plus `navigator.serviceWorker`.
pub fn install_service_worker_globals(
    vm: &mut Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    host: SharedServiceWorkerHost,
) {
    install_cache_api_globals(vm, event_loop.clone(), host.cache_host());

    let navigator = match vm.get_name("navigator") {
        JsValue::Undefined | JsValue::Null => {
            let object =
                JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Object))));
            vm.define_global("navigator", object.clone());
            object
        }
        value => value,
    };

    navigator.set_property(
        "serviceWorker",
        create_service_worker_container_object(host.clone(), event_loop),
    );
    vm.define_global(
        "__sylphosServiceWorkerMetrics",
        create_service_worker_metrics_function(host),
    );
}

fn create_cache_storage_object(
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    JsValue::host_object(
        Rc::new(CacheStorageObject { host, event_loop }),
        "[object CacheStorage]",
    )
}

#[derive(Clone)]
struct CacheStorageObject {
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
}

impl JsHostObject for CacheStorageObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "open" => Some(cache_storage_open(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            "match" => Some(cache_storage_match(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            "delete" => Some(cache_storage_delete(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            "keys" => Some(cache_storage_keys(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn cache_storage_open(host: SharedCacheApiHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CacheStorage.open".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let name = args
                .first()
                .map_or_else(|| DEFAULT_CACHE_NAME.to_owned(), JsValue::to_js_string);
            match host.open_cache(&name) {
                Ok(()) => Ok(create_resolved_promise_value(
                    event_loop.clone(),
                    create_cache_object(host.clone(), event_loop.clone(), name),
                )),
                Err(error) => Ok(create_rejected_promise_value(
                    event_loop.clone(),
                    JsValue::String(error.to_string()),
                )),
            }
        }),
    })
}

fn cache_storage_match(host: SharedCacheApiHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CacheStorage.match".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let request = args.first().map_or_else(
                || CacheApiRequest::get(String::new()),
                request_from_js_value,
            );
            let value = host
                .match_any(&request)
                .map_or(JsValue::Undefined, |response| {
                    create_response_object(event_loop.clone(), response)
                });
            Ok(create_resolved_promise_value(event_loop.clone(), value))
        }),
    })
}

fn cache_storage_delete(host: SharedCacheApiHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CacheStorage.delete".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let name = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(create_resolved_promise_value(
                event_loop.clone(),
                JsValue::Boolean(host.delete_cache(&name)),
            ))
        }),
    })
}

fn cache_storage_keys(host: SharedCacheApiHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CacheStorage.keys".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let keys = host
                .cache_names()
                .into_iter()
                .map(JsValue::String)
                .collect();
            Ok(create_resolved_promise_value(
                event_loop.clone(),
                JsValue::array(keys),
            ))
        }),
    })
}

fn create_cache_object(
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    name: String,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(CacheObject {
            host,
            event_loop,
            name: name.clone(),
        }),
        "[object Cache]",
    );
    object.set_property("name", JsValue::String(name));
    object
}

#[derive(Clone)]
struct CacheObject {
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    name: String,
}

impl JsHostObject for CacheObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "put" => Some(cache_put(
                self.host.clone(),
                self.event_loop.clone(),
                self.name.clone(),
            )),
            "match" => Some(cache_match(
                self.host.clone(),
                self.event_loop.clone(),
                self.name.clone(),
            )),
            "delete" => Some(cache_delete(
                self.host.clone(),
                self.event_loop.clone(),
                self.name.clone(),
            )),
            "keys" => Some(cache_keys(
                self.host.clone(),
                self.event_loop.clone(),
                self.name.clone(),
            )),
            "add" => Some(cache_add(
                self.host.clone(),
                self.event_loop.clone(),
                self.name.clone(),
            )),
            "addAll" => Some(cache_add_all(
                self.host.clone(),
                self.event_loop.clone(),
                self.name.clone(),
            )),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn cache_put(
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    name: String,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Cache.put".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let request = args.first().map_or_else(
                || CacheApiRequest::get(String::new()),
                request_from_js_value,
            );
            let response = args.get(1).map_or_else(
                || CacheApiResponse::text(request.url.clone(), String::new()),
                response_from_js_value,
            );
            match host.put(&name, request, response) {
                Ok(()) => Ok(create_resolved_promise_value(
                    event_loop.clone(),
                    JsValue::Undefined,
                )),
                Err(error) => Ok(create_rejected_promise_value(
                    event_loop.clone(),
                    JsValue::String(error.to_string()),
                )),
            }
        }),
    })
}

fn cache_match(
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    name: String,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Cache.match".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let request = args.first().map_or_else(
                || CacheApiRequest::get(String::new()),
                request_from_js_value,
            );
            let value = host
                .match_in_cache(&name, &request)
                .map_or(JsValue::Undefined, |response| {
                    create_response_object(event_loop.clone(), response)
                });
            Ok(create_resolved_promise_value(event_loop.clone(), value))
        }),
    })
}

fn cache_delete(
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    name: String,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Cache.delete".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let request = args.first().map_or_else(
                || CacheApiRequest::get(String::new()),
                request_from_js_value,
            );
            Ok(create_resolved_promise_value(
                event_loop.clone(),
                JsValue::Boolean(host.delete_entry(&name, &request)),
            ))
        }),
    })
}

fn cache_keys(
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    name: String,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Cache.keys".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let keys = host
                .keys(&name)
                .into_iter()
                .map(create_request_object)
                .collect();
            Ok(create_resolved_promise_value(
                event_loop.clone(),
                JsValue::array(keys),
            ))
        }),
    })
}

fn cache_add(
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    name: String,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Cache.add".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let request = args.first().map_or_else(
                || CacheApiRequest::get(String::new()),
                request_from_js_value,
            );
            let response = CacheApiResponse::text(
                request.url.clone(),
                format!("synthetic-cache-add:{}", request.url),
            );
            let result = host.put(&name, request, response);
            Ok(promise_from_result(event_loop.clone(), result))
        }),
    })
}

fn cache_add_all(
    host: SharedCacheApiHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    name: String,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Cache.addAll".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let requests = args
                .first()
                .map_or_else(Vec::new, request_array_from_js_value);
            let mut error = None;
            for request in requests {
                let response = CacheApiResponse::text(
                    request.url.clone(),
                    format!("synthetic-cache-add:{}", request.url),
                );
                if let Err(err) = host.put(&name, request, response) {
                    error = Some(err);
                    break;
                }
            }
            match error {
                Some(err) => Ok(create_rejected_promise_value(
                    event_loop.clone(),
                    JsValue::String(err.to_string()),
                )),
                None => Ok(create_resolved_promise_value(
                    event_loop.clone(),
                    JsValue::Undefined,
                )),
            }
        }),
    })
}

fn create_service_worker_container_object(
    host: SharedServiceWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    JsValue::host_object(
        Rc::new(ServiceWorkerContainerObject { host, event_loop }),
        "[object ServiceWorkerContainer]",
    )
}

#[derive(Clone)]
struct ServiceWorkerContainerObject {
    host: SharedServiceWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
}

impl JsHostObject for ServiceWorkerContainerObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "register" => Some(sw_register(self.host.clone(), self.event_loop.clone())),
            "getRegistration" => Some(sw_get_registration(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            "getRegistrations" => Some(sw_get_registrations(
                self.host.clone(),
                self.event_loop.clone(),
            )),
            "ready" => Some(create_resolved_promise_value(
                self.event_loop.clone(),
                self.host.registrations().into_iter().next().map_or(
                    JsValue::Null,
                    |registration| {
                        create_registration_object(
                            self.host.clone(),
                            self.event_loop.clone(),
                            registration,
                        )
                    },
                ),
            )),
            "controller" => Some(
                self.host
                    .registrations()
                    .into_iter()
                    .find(|registration| registration.controls_clients)
                    .map_or(JsValue::Null, create_service_worker_object),
            ),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn sw_register(host: SharedServiceWorkerHost, event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "ServiceWorkerContainer.register".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let script_url = args.first().map_or_else(String::new, JsValue::to_js_string);
            let scope = args.get(1).and_then(scope_from_registration_options);
            match host.register_service_worker(script_url, scope) {
                Ok(registration) => Ok(create_resolved_promise_value(
                    event_loop.clone(),
                    create_registration_object(host.clone(), event_loop.clone(), registration),
                )),
                Err(error) => Ok(create_rejected_promise_value(
                    event_loop.clone(),
                    JsValue::String(error.to_string()),
                )),
            }
        }),
    })
}

fn sw_get_registration(
    host: SharedServiceWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "ServiceWorkerContainer.getRegistration".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let scope = args.first().map_or_else(String::new, JsValue::to_js_string);
            let value =
                host.registration_for_scope(&scope)
                    .map_or(JsValue::Undefined, |registration| {
                        create_registration_object(host.clone(), event_loop.clone(), registration)
                    });
            Ok(create_resolved_promise_value(event_loop.clone(), value))
        }),
    })
}

fn sw_get_registrations(
    host: SharedServiceWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "ServiceWorkerContainer.getRegistrations".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let registrations = host
                .registrations()
                .into_iter()
                .map(|registration| {
                    create_registration_object(host.clone(), event_loop.clone(), registration)
                })
                .collect();
            Ok(create_resolved_promise_value(
                event_loop.clone(),
                JsValue::array(registrations),
            ))
        }),
    })
}

fn create_registration_object(
    host: SharedServiceWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    registration: ServiceWorkerRegistration,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(ServiceWorkerRegistrationObject {
            host,
            event_loop,
            scope: registration.scope.clone(),
        }),
        "[object ServiceWorkerRegistration]",
    );
    object.set_property("scope", JsValue::String(registration.scope.clone()));
    object.set_property("active", create_service_worker_object(registration));
    object.set_property("installing", JsValue::Null);
    object.set_property("waiting", JsValue::Null);
    object
}

#[derive(Clone)]
struct ServiceWorkerRegistrationObject {
    host: SharedServiceWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    scope: String,
}

impl JsHostObject for ServiceWorkerRegistrationObject {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "update" => Some(registration_update(
                self.host.clone(),
                self.event_loop.clone(),
                self.scope.clone(),
            )),
            "unregister" => Some(registration_unregister(
                self.host.clone(),
                self.event_loop.clone(),
                self.scope.clone(),
            )),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn registration_update(
    host: SharedServiceWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    scope: String,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "ServiceWorkerRegistration.update".to_owned(),
        function: Rc::new(move |_vm, _this, _args| match host.update_scope(&scope) {
            Ok(Some(registration)) => Ok(create_resolved_promise_value(
                event_loop.clone(),
                create_registration_object(host.clone(), event_loop.clone(), registration),
            )),
            Ok(None) => Ok(create_resolved_promise_value(
                event_loop.clone(),
                JsValue::Undefined,
            )),
            Err(error) => Ok(create_rejected_promise_value(
                event_loop.clone(),
                JsValue::String(error.to_string()),
            )),
        }),
    })
}

fn registration_unregister(
    host: SharedServiceWorkerHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    scope: String,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "ServiceWorkerRegistration.unregister".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            Ok(create_resolved_promise_value(
                event_loop.clone(),
                JsValue::Boolean(host.unregister_scope(&scope)),
            ))
        }),
    })
}

fn create_service_worker_object(registration: ServiceWorkerRegistration) -> JsValue {
    let object = JsValue::object();
    object.set_property("scriptURL", JsValue::String(registration.script_url));
    object.set_property(
        "state",
        JsValue::String(registration.state.as_str().to_owned()),
    );
    object.set_property("onstatechange", JsValue::Null);
    object.set_property("postMessage", native_noop("ServiceWorker.postMessage"));
    object
}

fn create_request_constructor() -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Request".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let request = args.first().map_or_else(
                || CacheApiRequest::get(String::new()),
                request_from_js_value,
            );
            Ok(create_request_object(request))
        }),
    })
}

fn create_request_object(request: CacheApiRequest) -> JsValue {
    let object = JsValue::object();
    object.set_property("url", JsValue::String(request.url));
    object.set_property("method", JsValue::String(request.method));
    object.set_property("clone", native_clone_request());
    object
}

fn native_clone_request() -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Request.clone".to_owned(),
        function: Rc::new(move |_vm, this, _args| Ok(this)),
    })
}

fn create_response_constructor(event_loop: Rc<RefCell<JsEventLoop>>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Response".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let body = args.first().map_or_else(String::new, JsValue::to_js_string);
            let mut response = CacheApiResponse::text("about:synthetic-response", body);
            if let Some(init) = args.get(1) {
                let status = init.get_property("status").to_number();
                if status.is_finite() && status >= 100.0 && status <= 599.0 {
                    response.status = status as u16;
                }
                let status_text = init.get_property("statusText");
                if !matches!(status_text, JsValue::Undefined | JsValue::Null) {
                    response.status_text = status_text.to_js_string();
                }
            }
            Ok(create_response_object(event_loop.clone(), response))
        }),
    })
}

fn create_response_object(
    event_loop: Rc<RefCell<JsEventLoop>>,
    response: CacheApiResponse,
) -> JsValue {
    let object = JsValue::object();
    object.set_property("url", JsValue::String(response.url.clone()));
    object.set_property("status", JsValue::Number(f64::from(response.status)));
    object.set_property("statusText", JsValue::String(response.status_text.clone()));
    object.set_property("ok", JsValue::Boolean(response.ok()));
    object.set_property("__sylphosBody", JsValue::String(response.body.clone()));

    let body_for_text = response.body.clone();
    let text_loop = event_loop.clone();
    object.set_property(
        "text",
        JsValue::function(JsFunction::Native {
            name: "Response.text".to_owned(),
            function: Rc::new(move |_vm, _this, _args| {
                Ok(create_resolved_promise_value(
                    text_loop.clone(),
                    JsValue::String(body_for_text.clone()),
                ))
            }),
        }),
    );

    let body_for_json = response.body.clone();
    let json_loop = event_loop.clone();
    object.set_property(
        "json",
        JsValue::function(JsFunction::Native {
            name: "Response.json".to_owned(),
            function: Rc::new(move |_vm, _this, _args| {
                Ok(create_resolved_promise_value(
                    json_loop.clone(),
                    parse_json_lite(&body_for_json)
                        .unwrap_or_else(|| JsValue::String(body_for_json.clone())),
                ))
            }),
        }),
    );

    let clone_loop = event_loop;
    object.set_property(
        "clone",
        JsValue::function(JsFunction::Native {
            name: "Response.clone".to_owned(),
            function: Rc::new(move |_vm, this, _args| {
                Ok(create_response_object(
                    clone_loop.clone(),
                    response_from_js_value(&this),
                ))
            }),
        }),
    );

    object
}

fn create_cache_metrics_function(host: SharedCacheApiHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosCacheMetrics".to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(metrics_to_object(host.metrics()))),
    })
}

fn create_service_worker_metrics_function(host: SharedServiceWorkerHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosServiceWorkerMetrics".to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(metrics_to_object(host.metrics()))),
    })
}

fn metrics_to_object(metrics: CacheApiMetrics) -> JsValue {
    let object = JsValue::object();
    object.set_property(
        "cacheOpenCalls",
        JsValue::Number(metrics.cache_open_calls as f64),
    );
    object.set_property(
        "cachePutCalls",
        JsValue::Number(metrics.cache_put_calls as f64),
    );
    object.set_property("cacheHits", JsValue::Number(metrics.cache_hits as f64));
    object.set_property("cacheMisses", JsValue::Number(metrics.cache_misses as f64));
    object.set_property(
        "serviceWorkerRegistrations",
        JsValue::Number(metrics.service_worker_registrations as f64),
    );
    object.set_property(
        "fetchInterceptions",
        JsValue::Number(metrics.fetch_intercepts as f64),
    );
    object.set_property(
        "fetchCacheHits",
        JsValue::Number(metrics.fetch_cache_hits as f64),
    );
    object
}

fn native_noop(name: &'static str) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Undefined)),
    })
}

fn promise_from_result(
    event_loop: Rc<RefCell<JsEventLoop>>,
    result: Result<(), JsRuntimeError>,
) -> JsValue {
    match result {
        Ok(()) => create_resolved_promise_value(event_loop, JsValue::Undefined),
        Err(error) => create_rejected_promise_value(event_loop, JsValue::String(error.to_string())),
    }
}

fn request_from_js_value(value: &JsValue) -> CacheApiRequest {
    match value {
        JsValue::String(url) => CacheApiRequest::get(url.clone()),
        JsValue::Object(_) => {
            let url = value.get_property("url").to_js_string();
            let method_value = value.get_property("method");
            let method = if matches!(method_value, JsValue::Undefined | JsValue::Null) {
                DEFAULT_METHOD.to_owned()
            } else {
                method_value.to_js_string()
            };
            CacheApiRequest::new(url, method)
        }
        _ => CacheApiRequest::get(value.to_js_string()),
    }
}

fn response_from_js_value(value: &JsValue) -> CacheApiResponse {
    let url_value = value.get_property("url");
    let url = if matches!(url_value, JsValue::Undefined | JsValue::Null) {
        "about:synthetic-response".to_owned()
    } else {
        url_value.to_js_string()
    };
    let body_value = value.get_property("__sylphosBody");
    let body = if matches!(body_value, JsValue::Undefined | JsValue::Null) {
        value.to_js_string()
    } else {
        body_value.to_js_string()
    };
    let status = value.get_property("status").to_number();
    let status = if status.is_finite() && status >= 100.0 && status <= 599.0 {
        status as u16
    } else {
        200
    };
    let status_text_value = value.get_property("statusText");
    let status_text = if matches!(status_text_value, JsValue::Undefined | JsValue::Null) {
        if status == 200 { "OK" } else { "Synthetic" }.to_owned()
    } else {
        status_text_value.to_js_string()
    };

    CacheApiResponse {
        url,
        status,
        status_text,
        body,
        headers: BTreeMap::new(),
    }
}

fn request_array_from_js_value(value: &JsValue) -> Vec<CacheApiRequest> {
    let length = value.get_property("length").to_number();
    if !length.is_finite() || length <= 0.0 {
        return Vec::new();
    }
    (0..(length as usize).min(256))
        .map(|index| request_from_js_value(&value.get_property(&index.to_string())))
        .collect()
}

fn scope_from_registration_options(value: &JsValue) -> Option<String> {
    let scope = value.get_property("scope");
    match scope {
        JsValue::Undefined | JsValue::Null => None,
        _ => Some(scope.to_js_string()),
    }
}

/// Performs simple static analysis of a Service Worker script.
#[must_use]
pub fn analyze_service_worker_script(source: &str) -> ServiceWorkerScriptAnalysis {
    let clean = strip_line_comments(source);
    let mut analysis = ServiceWorkerScriptAnalysis {
        has_install_listener: has_event_listener(&clean, "install"),
        has_activate_listener: has_event_listener(&clean, "activate"),
        has_fetch_listener: has_event_listener(&clean, "fetch"),
        cache_names: capture_function_first_string_arg(&clean, "caches.open"),
        precache_urls: capture_precache_urls(&clean),
        skip_waiting: clean.contains("skipWaiting()") || clean.contains("self.skipWaiting()"),
        clients_claim: clean.contains("clients.claim()") || clean.contains("self.clients.claim()"),
        cache_first_fetch: clean.contains("caches.match(") && clean.contains("respondWith"),
    };

    dedupe_strings(&mut analysis.cache_names);
    dedupe_strings(&mut analysis.precache_urls);
    analysis
}

fn has_event_listener(source: &str, event_type: &str) -> bool {
    let single = format!("addEventListener('{event_type}'");
    let double = format!("addEventListener(\"{event_type}\"");
    let self_single = format!("self.{single}");
    let self_double = format!("self.{double}");
    [single, double, self_single, self_double]
        .iter()
        .any(|needle| source.contains(needle))
}

fn capture_precache_urls(source: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for args in capture_method_args(source, "cache", "add") {
        urls.extend(string_literals(&args).into_iter().take(1));
    }
    for args in capture_method_args(source, "cache", "addAll") {
        urls.extend(string_literals(&args));
    }
    urls
}

fn capture_function_first_string_arg(source: &str, name: &str) -> Vec<String> {
    capture_function_args(source, name)
        .into_iter()
        .filter_map(|args| string_literals(&args).first().cloned())
        .collect()
}

fn capture_method_args(source: &str, object_name: &str, method: &str) -> Vec<String> {
    capture_function_args(source, &format!("{object_name}.{method}"))
}

fn capture_function_args(source: &str, name: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cursor = 0usize;
    let needle = format!("{name}(");

    while let Some(index) = source[cursor..].find(&needle) {
        let open = cursor + index + name.len();
        if let Some(value) = extract_parenthesized(source, open) {
            cursor = open.saturating_add(value.len()).saturating_add(2);
            args.push(value);
        } else {
            break;
        }
    }

    args
}

fn extract_parenthesized(source: &str, open_paren_index: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if bytes.get(open_paren_index) != Some(&b'(') {
        return None;
    }

    let mut depth = 0usize;
    let mut in_quote: Option<u8> = None;
    let mut escaped = false;
    let mut start = None;

    for (index, byte) in bytes.iter().enumerate().skip(open_paren_index) {
        if let Some(quote) = in_quote {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == quote {
                in_quote = None;
            }
            continue;
        }

        match *byte {
            b'\'' | b'"' | b'`' => in_quote = Some(*byte),
            b'(' => {
                if depth == 0 {
                    start = Some(index + 1);
                }
                depth = depth.saturating_add(1);
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return start.map(|start| source[start..index].to_owned());
                }
            }
            _ => {}
        }
    }

    None
}

fn string_literals(source: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0usize;

    while index < source.len() {
        if let Some((value, end)) = parse_string_literal_at(source, index) {
            values.push(value);
            index = end;
        } else {
            index += 1;
        }
    }

    values
}

fn parse_string_literal_at(source: &str, start: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let mut index = skip_ws(bytes, start);
    let quote = *bytes.get(index)?;
    if quote != b'\'' && quote != b'"' && quote != b'`' {
        return None;
    }
    index += 1;

    let mut value = String::new();
    let mut escaped = false;

    while let Some(byte) = bytes.get(index) {
        if escaped {
            let ch = match *byte {
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'\\' => '\\',
                b'\'' => '\'',
                b'"' => '"',
                b'`' => '`',
                other => char::from(other),
            };
            value.push(ch);
            escaped = false;
            index += 1;
            continue;
        }

        if *byte == b'\\' {
            escaped = true;
            index += 1;
            continue;
        }

        if *byte == quote {
            return Some((value, index + 1));
        }

        value.push(char::from(*byte));
        index += 1;
    }

    None
}

fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        index += 1;
    }
    index
}

fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(before, _)| before))
        .collect::<Vec<_>>()
        .join("\n")
}

fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn normalize_method(input: &str) -> String {
    let method = input.trim();
    if method.is_empty() {
        DEFAULT_METHOD.to_owned()
    } else {
        method.to_ascii_uppercase()
    }
}

fn normalize_url_string(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.to_owned()
}

fn normalize_headers(headers: BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .into_iter()
        .map(|(key, value)| (key.to_ascii_lowercase(), value))
        .collect()
}

fn infer_scope_from_script_url(script_url: &str) -> String {
    let trimmed = script_url.trim();
    if let Some(index) = trimmed.rfind('/') {
        return trimmed[..=index].to_owned();
    }
    "/".to_owned()
}

fn resolve_scope_url(scope: &str, url: &str) -> String {
    let url = url.trim();
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("/") {
        url.to_owned()
    } else {
        format!("{}{}", scope.trim_end_matches('/'), format!("/{url}"))
    }
}

fn merge_metrics(mut left: CacheApiMetrics, right: CacheApiMetrics) -> CacheApiMetrics {
    left.cache_open_calls = left.cache_open_calls.saturating_add(right.cache_open_calls);
    left.cache_storage_match_calls = left
        .cache_storage_match_calls
        .saturating_add(right.cache_storage_match_calls);
    left.cache_put_calls = left.cache_put_calls.saturating_add(right.cache_put_calls);
    left.cache_match_calls = left
        .cache_match_calls
        .saturating_add(right.cache_match_calls);
    left.cache_delete_calls = left
        .cache_delete_calls
        .saturating_add(right.cache_delete_calls);
    left.cache_add_calls = left.cache_add_calls.saturating_add(right.cache_add_calls);
    left.cache_hits = left.cache_hits.saturating_add(right.cache_hits);
    left.cache_misses = left.cache_misses.saturating_add(right.cache_misses);
    left.service_worker_registrations = left
        .service_worker_registrations
        .saturating_add(right.service_worker_registrations);
    left.service_worker_unregistrations = left
        .service_worker_unregistrations
        .saturating_add(right.service_worker_unregistrations);
    left.service_worker_updates = left
        .service_worker_updates
        .saturating_add(right.service_worker_updates);
    left.fetch_intercepts = left.fetch_intercepts.saturating_add(right.fetch_intercepts);
    left.fetch_cache_hits = left.fetch_cache_hits.saturating_add(right.fetch_cache_hits);
    left.fetch_passthroughs = left
        .fetch_passthroughs
        .saturating_add(right.fetch_passthroughs);
    left
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
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
    None
}
