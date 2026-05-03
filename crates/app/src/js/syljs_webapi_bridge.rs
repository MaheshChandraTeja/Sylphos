//! App bridge for SylJS Web APIs.
//!
//! The core `syljs` crate exposes a `WebApiHost` trait. This file provides a
//! conservative app-side adapter shape that can be backed by Sylphos cache,
//! resource scheduling, cookies, history, and navigation systems. It is kept
//! additive so your existing browser shell does not get bulldozed by a module
//! zip, because that would be very on-brand for software and still terrible.

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use syljs::{
    install_web_api_globals, CookieRecord, EventLoopConfig, EventLoopRunSummary, FetchRecord,
    HistoryRecord, JsRuntimeError, ProgramKind, ScheduledVm, SharedWebApiHost, StorageArea,
    WebApiHost, WebApiMetrics, WebApiResponse, XhrRecord,
};

/// Script input for Web-API-bound execution.
#[derive(Debug, Clone)]
pub(crate) struct WebApiBoundSylJsScript {
    /// Script label.
    pub label: String,

    /// Script source.
    pub source: String,

    /// Script kind.
    pub kind: ProgramKind,
}

/// Result from running scripts with Web API globals.
#[derive(Debug, Clone)]
pub(crate) struct WebApiBoundSylJsResult {
    /// Event-loop summary.
    pub summary: EventLoopRunSummary,

    /// Failed script labels.
    pub failed_scripts: Vec<String>,

    /// Web API metrics.
    pub web_api_metrics: WebApiMetrics,

    /// Final location.
    pub final_location: String,
}

/// Executes scripts with Web API globals using the provided host.
pub(crate) fn execute_webapi_bound_syljs_scripts<I>(
    scripts: I,
    host: SharedWebApiHost,
    vm_config: syljs::VmConfig,
    event_loop_config: EventLoopConfig,
) -> Result<WebApiBoundSylJsResult, JsRuntimeError>
where
    I: IntoIterator<Item = WebApiBoundSylJsScript>,
{
    let mut scheduled = ScheduledVm::with_config(vm_config, event_loop_config);
    install_web_api_globals(&mut scheduled.vm, scheduled.event_loop.clone(), host.clone());

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
                "failed to execute Web-API-bound SylJS script"
            );
            failed_scripts.push(script.label);
        }
    }

    let summary = scheduled.run_until_idle()?;

    Ok(WebApiBoundSylJsResult {
        summary,
        failed_scripts,
        web_api_metrics: host.metrics(),
        final_location: host.location_href(),
    })
}

/// A deterministic app-local Web API host.
///
/// This can be used immediately for local tests. For production browser runs,
/// wire `fetch` to your ResourceScheduler/CacheStore by implementing WebApiHost
/// on a richer app adapter and passing it into `execute_webapi_bound_syljs_scripts`.
#[derive(Debug)]
pub(crate) struct AppResearchWebApiHost {
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

impl AppResearchWebApiHost {
    /// Creates a host from current page URL.
    pub(crate) fn new(location: impl Into<String>) -> Rc<Self> {
        let location = location.into();
        Rc::new(Self {
            origin: origin_from_url(&location),
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
        })
    }

    /// Registers a deterministic response for tests/local fixtures.
    pub(crate) fn register_route(&self, url: impl Into<String>, response: WebApiResponse) {
        let url = self.resolve_url(&url.into());
        self.routes.borrow_mut().insert(url, response);
    }
}

impl WebApiHost for AppResearchWebApiHost {
    fn origin(&self) -> String {
        self.origin.clone()
    }

    fn location_href(&self) -> String {
        self.location.borrow().clone()
    }

    fn set_location_href(&self, href: String) {
        *self.location.borrow_mut() = self.resolve_url(&href);
        self.metrics.borrow_mut().location_navigations =
            self.metrics.borrow().location_navigations.saturating_add(1);
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
        self.metrics.borrow_mut().fetch_calls =
            self.metrics.borrow().fetch_calls.saturating_add(1);

        Ok(self
            .routes
            .borrow()
            .get(&resolved)
            .cloned()
            .unwrap_or_else(|| WebApiResponse::text(resolved.clone(), format!("response:{resolved}"))))
    }

    fn storage_get(&self, area: StorageArea, key: &str) -> Option<String> {
        self.metrics.borrow_mut().storage_reads =
            self.metrics.borrow().storage_reads.saturating_add(1);
        match area {
            StorageArea::Local => self.local_storage.borrow().get(key).cloned(),
            StorageArea::Session => self.session_storage.borrow().get(key).cloned(),
        }
    }

    fn storage_set(&self, area: StorageArea, key: &str, value: String) {
        self.metrics.borrow_mut().storage_writes =
            self.metrics.borrow().storage_writes.saturating_add(1);
        match area {
            StorageArea::Local => self.local_storage.borrow_mut().insert(key.to_owned(), value),
            StorageArea::Session => self.session_storage.borrow_mut().insert(key.to_owned(), value),
        };
    }

    fn storage_remove(&self, area: StorageArea, key: &str) {
        self.metrics.borrow_mut().storage_removals =
            self.metrics.borrow().storage_removals.saturating_add(1);
        match area {
            StorageArea::Local => self.local_storage.borrow_mut().remove(key),
            StorageArea::Session => self.session_storage.borrow_mut().remove(key),
        };
    }

    fn storage_clear(&self, area: StorageArea) {
        self.metrics.borrow_mut().storage_removals =
            self.metrics.borrow().storage_removals.saturating_add(1);
        match area {
            StorageArea::Local => self.local_storage.borrow_mut().clear(),
            StorageArea::Session => self.session_storage.borrow_mut().clear(),
        }
    }

    fn cookie_string(&self) -> String {
        self.metrics.borrow_mut().cookie_reads =
            self.metrics.borrow().cookie_reads.saturating_add(1);
        self.cookies
            .borrow()
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn set_cookie(&self, raw: &str) {
        self.cookie_records
            .borrow_mut()
            .push(CookieRecord { raw: raw.to_owned() });
        self.metrics.borrow_mut().cookie_writes =
            self.metrics.borrow().cookie_writes.saturating_add(1);

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
        self.metrics.borrow_mut().history_pushes =
            self.metrics.borrow().history_pushes.saturating_add(1);
    }

    fn history_replace(&self, url: String) {
        self.history.borrow_mut().push(HistoryRecord {
            kind: "replaceState".to_owned(),
            url: url.clone(),
        });
        *self.location.borrow_mut() = url;
        self.metrics.borrow_mut().history_replaces =
            self.metrics.borrow().history_replaces.saturating_add(1);
    }

    fn metrics(&self) -> WebApiMetrics {
        self.metrics.borrow().clone()
    }
}

fn resolve_url_lite(base: &str, input: &str) -> String {
    let input = input.trim();

    if input.starts_with("http://") || input.starts_with("https://") || input.starts_with("about:") {
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
