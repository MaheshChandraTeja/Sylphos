#![allow(clippy::cast_precision_loss)]

use anyhow::{Context, Result};
use clap::Parser;
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::Instant,
};
use tracing::{error, info, warn};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::ModifiersState,
    window::{CursorIcon, Window, WindowId},
};

mod browser;
mod js;
mod pipeline;
mod present_adapter;
mod render;
mod state;

use browser::{
    fetch_decode_images_with_scheduler, load_and_apply_stylesheets_with_scheduler,
    normalize_user_url, resolve_document_image_sources, resolve_link_url, BrowserChrome,
    BrowserHistory, CachePolicy, CacheSource, CacheStore, ChromeAction, NavigationTarget,
    PageDomController, PageFormAction, PageFormController, ResourceRequest, ResourceScheduler,
    ResourceSummary, StylesheetLoadSummary, TabId, TabManager, TOOLBAR_HEIGHT,
};
use js::{
    execute_document_scripts, MediaCanvasWorkerHost, ScriptExecutionSummary, ServiceWorkerHost,
    WebPlatformHost,
};
use render::{DecodedImageStore, SharedPaintState};

#[derive(Debug, Parser)]
#[command(
    name = "sylphos",
    about = "Sylphos Fetch → Parse → Present browser shell"
)]
struct Cli {
    #[arg(long, default_value = "https://example.com")]
    url: String,

    #[arg(long)]
    disable_cache: bool,

    #[arg(long)]
    clear_cache: bool,

    #[arg(long)]
    cache_dir: Option<PathBuf>,
}

#[derive(Debug)]
struct PipelineMessage {
    tab_id: TabId,
    navigation_id: u64,
    url: String,
    result: PipelineResult,
}

#[derive(Debug)]
enum PipelineResult {
    Loaded(Box<LoadedPage>),
    Failed { message: String, elapsed_ms: u128 },
}

struct App {
    clear_rgba: Arc<Mutex<[f32; 4]>>,
    paint_state: SharedPaintState,
    window: Option<Arc<Window>>,
    renderer: Option<state::State>,
    rt: tokio::runtime::Runtime,
    tabs: TabManager,
    history: BrowserHistory,
    chrome: BrowserChrome,
    page_forms: PageFormController,
    page_dom: PageDomController,
    pipeline_tx: mpsc::Sender<PipelineMessage>,
    pipeline_rx: mpsc::Receiver<PipelineMessage>,
    active_navigation_id: u64,
    resources: ResourceScheduler,
    modifiers: ModifiersState,
    cursor_x: f32,
    cursor_y: f32,
    has_started_pipeline: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let mut window_attributes =
            Window::default_attributes().with_title("Sylphos — DOM Bindings + Event Loop");

        if let Some(icon) = load_window_icon() {
            window_attributes = window_attributes.with_window_icon(Some(icon));
        }

        let window = match event_loop.create_window(window_attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                error!(error = %error, "failed to create window");
                event_loop.exit();
                return;
            }
        };

        self.sync_browser_to_renderer();

        let renderer = match pollster::block_on(state::State::new(
            window.clone(),
            self.clear_rgba.clone(),
            self.paint_state.clone(),
        )) {
            Ok(renderer) => renderer,
            Err(error) => {
                error!(error = %error, "failed to initialize renderer");
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        self.renderer = Some(renderer);

        if !self.has_started_pipeline {
            self.has_started_pipeline = true;
            self.reload_current();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };

        if id != window.id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
                self.update_hover_feedback(&window);
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged {
                mut inner_size_writer,
                ..
            } => {
                let _ = inner_size_writer.request_inner_size(window.inner_size());
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(window.inner_size());
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_x = position.x as f32;
                self.cursor_y = position.y as f32;
                self.update_hover_feedback(&window);
                self.sync_browser_to_renderer();
                self.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let action = self.chrome.handle_click(
                    self.cursor_x,
                    self.cursor_y,
                    window.inner_size().width as f32,
                );

                let chrome_consumed = action != ChromeAction::None;
                self.execute_chrome_action(action);

                if !chrome_consumed && !self.execute_page_form_click(&window) {
                    self.execute_page_click(&window);
                }

                self.update_hover_feedback(&window);
                self.sync_browser_to_renderer();
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if self.execute_page_form_key(&event.logical_key) {
                    self.sync_browser_to_renderer();
                    self.request_redraw();
                    return;
                }

                let action = self.chrome.handle_key(&event.logical_key, self.modifiers);
                self.execute_chrome_action(action);
                self.sync_browser_to_renderer();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    match renderer.render() {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            renderer.resize(window.inner_size());
                            window.request_redraw();
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(error) => error!(error = ?error, "render error"),
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.poll_pipeline_messages();

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl App {
    fn execute_chrome_action(&mut self, action: ChromeAction) {
        match action {
            ChromeAction::None => {}
            ChromeAction::Navigate(input) => self.navigate_from_input(&input),
            ChromeAction::Reload => self.reload_current(),
            ChromeAction::Back => self.go_back(),
            ChromeAction::Forward => self.go_forward(),
            ChromeAction::NewTab => self.new_tab(),
            ChromeAction::CloseTab(tab_id) => self.close_tab(tab_id),
            ChromeAction::SwitchTab(tab_id) => self.switch_tab(tab_id),
            ChromeAction::NextTab => self.next_tab(),
            ChromeAction::PreviousTab => self.previous_tab(),
        }
    }

    fn navigate_from_input(&mut self, input: &str) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        match normalize_user_url(input) {
            Ok(url) => {
                let target = self.tabs.navigate_active_to(url);
                self.begin_navigation(target);
            }
            Err(error) => {
                self.chrome.set_error(error.to_string());
                self.sync_browser_to_renderer();
            }
        }
    }

    fn reload_current(&mut self) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        let target = self.tabs.reload_active();
        self.begin_navigation(target);
    }

    fn go_back(&mut self) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        if let Some(target) = self.tabs.go_back_active() {
            self.begin_navigation(target);
        }
    }

    fn go_forward(&mut self) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        if let Some(target) = self.tabs.go_forward_active() {
            self.begin_navigation(target);
        }
    }

    fn new_tab(&mut self) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        let tab_id = self.tabs.new_blank_tab();
        info!(tab_id = tab_id.value(), "created new tab");
        self.apply_active_tab_to_paint_state();
        self.chrome.set_loaded(self.tabs.current_url());
        self.sync_browser_to_renderer();
    }

    fn close_tab(&mut self, tab_id: TabId) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        if self.tabs.close_tab(tab_id) {
            info!(tab_id = tab_id.value(), "closed tab");
            self.apply_active_tab_to_paint_state();
            self.chrome.set_loaded(self.tabs.current_url());
            self.sync_browser_to_renderer();
        }
    }

    fn switch_tab(&mut self, tab_id: TabId) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        if self.tabs.switch_to(tab_id) {
            info!(tab_id = tab_id.value(), url = %self.tabs.current_url(), "switched tab");
            self.apply_active_tab_to_paint_state();
            self.chrome.set_loaded(self.tabs.current_url());
            self.sync_browser_to_renderer();
        }
    }

    fn next_tab(&mut self) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        if self.tabs.activate_next() {
            self.apply_active_tab_to_paint_state();
            self.chrome.set_loaded(self.tabs.current_url());
            self.sync_browser_to_renderer();
        }
    }

    fn previous_tab(&mut self) {
        self.page_forms.clear_focus();
        self.page_dom.clear();
        if self.tabs.activate_previous() {
            self.apply_active_tab_to_paint_state();
            self.chrome.set_loaded(self.tabs.current_url());
            self.sync_browser_to_renderer();
        }
    }

    fn begin_navigation(&mut self, target: NavigationTarget) {
        self.active_navigation_id = self.active_navigation_id.wrapping_add(1);
        let navigation_id = self.active_navigation_id;
        let url = target.url;
        let tab_id = target.tab_id;

        self.page_dom.clear();

        if url.eq_ignore_ascii_case("about:blank") {
            let _ = self.tabs.mark_loaded(
                tab_id,
                None,
                present::RenderDocument::new(),
                DecodedImageStore::default(),
            );
            self.page_dom.clear();
            self.apply_active_tab_to_paint_state();
            self.chrome.set_loaded("about:blank");
            self.sync_browser_to_renderer();
            return;
        }

        let _ = self.tabs.mark_loading(tab_id, navigation_id, &url);
        self.chrome.set_loading(&url);
        self.sync_browser_to_renderer();

        if tab_id == self.tabs.active_id() {
            self.clear_active_page_surface();
        }

        let resources = self.resources.clone();
        resources.reset_summary();
        let tx = self.pipeline_tx.clone();

        info!(url = %url, navigation_id, tab_id = tab_id.value(), "starting navigation");

        drop(self.rt.spawn(async move {
            let started = Instant::now();
            let result = load_url(&url, resources).await;
            let elapsed_ms = started.elapsed().as_millis();

            let message = match result {
                Ok(page) => PipelineMessage {
                    tab_id,
                    navigation_id,
                    url,
                    result: PipelineResult::Loaded(Box::new(page.with_elapsed(elapsed_ms))),
                },
                Err(error) => PipelineMessage {
                    tab_id,
                    navigation_id,
                    url,
                    result: PipelineResult::Failed {
                        message: error.to_string(),
                        elapsed_ms,
                    },
                },
            };

            let _ = tx.send(message);
        }));
    }

    fn poll_pipeline_messages(&mut self) {
        let messages = self.pipeline_rx.try_iter().collect::<Vec<_>>();

        for message in messages {
            if !self
                .tabs
                .is_pending_navigation(message.tab_id, message.navigation_id)
            {
                info!(
                    navigation_id = message.navigation_id,
                    tab_id = message.tab_id.value(),
                    "ignored stale navigation result"
                );
                continue;
            }

            match message.result {
                PipelineResult::Loaded(page) => {
                    let title = page.title.clone();
                    let _ = self.tabs.mark_loaded(
                        message.tab_id,
                        title.clone(),
                        page.document.clone(),
                        page.images.clone(),
                    );
                    self.history.record_visit(&message.url, title.as_deref());

                    if message.tab_id == self.tabs.active_id() {
                        self.page_forms.clear_focus();
                        self.apply_active_tab_to_paint_state();
                        self.chrome.set_loaded(&message.url);
                    }

                    info!(
                        url = %message.url,
                        title = title.as_deref().unwrap_or(""),
                        bytes = page.bytes,
                        blocks = page.blocks,
                        stylesheets_discovered = page.stylesheets.discovered,
                        stylesheets_loaded = page.stylesheets.loaded,
                        stylesheets_failed = page.stylesheets.failed,
                        stylesheets_skipped = page.stylesheets.skipped,
                        stylesheets_imported = page.stylesheets.imported,
                        stylesheet_bytes = page.stylesheets.bytes,
                        stylesheet_rules = page.stylesheets.rule_count,
                        stylesheet_memory_hits = page.stylesheets.memory_hits,
                        stylesheet_disk_hits = page.stylesheets.disk_hits,
                        stylesheet_network_fetches = page.stylesheets.network_fetches,
                        stylesheet_disabled_fetches = page.stylesheets.disabled_fetches,
                        scripts_discovered = page.scripts.discovered,
                        scripts_executable = page.scripts.executable,
                        scripts_inline_executed = page.scripts.inline_executed,
                        scripts_external_executed = page.scripts.external_executed,
                        scripts_skipped = page.scripts.skipped,
                        scripts_failed = page.scripts.failed,
                        script_bytes = page.scripts.bytes,
                        script_console_messages = page.scripts.console_messages,
                        script_warnings = page.scripts.warnings,
                        script_errors = page.scripts.errors,
                        js_dom_mutations = page.scripts.dom_mutations,
                        js_dom_ignored = page.scripts.dom_ignored,
                        js_event_tasks = page.scripts.event_tasks_executed,
                        js_microtasks = page.scripts.microtasks_executed,
                        js_event_listeners = page.scripts.registered_listeners,
                        js_dispatched_events = page.scripts.dispatched_events,
                        script_memory_hits = page.scripts.memory_hits,
                        script_disk_hits = page.scripts.disk_hits,
                        script_network_fetches = page.scripts.network_fetches,
                        script_disabled_fetches = page.scripts.disabled_fetches,
                        web_api_effects = page.scripts.web_platform.effects,
                        web_api_fetch_calls = page.scripts.web_platform.fetch_calls,
                        web_api_xhr_calls = page.scripts.web_platform.xhr_calls,
                        web_api_network_succeeded = page.scripts.web_platform.network_succeeded,
                        web_api_network_failed = page.scripts.web_platform.network_failed,
                        web_api_response_bytes = page.scripts.web_platform.response_bytes,
                        web_storage_writes = page.scripts.web_platform.storage_writes,
                        web_storage_removes = page.scripts.web_platform.storage_removes,
                        web_cookie_writes = page.scripts.web_platform.cookie_writes,
                        web_history_pushes = page.scripts.web_platform.history_pushes,
                        web_history_replaces = page.scripts.web_platform.history_replaces,
                        web_timers_scheduled = page.scripts.web_platform.timers_scheduled,
                        web_timers_cleared = page.scripts.web_platform.timers_cleared,
                        web_platform_summary = %page.scripts.web_platform.compact(),
                        media_effects = page.scripts.media.effects,
                        media_elements = page.scripts.media.media_elements,
                        media_src_assignments = page.scripts.media.media_src_assignments,
                        media_controls = page.scripts.media.media_controls,
                        media_source_objects = page.scripts.media.media_source_objects,
                        media_source_buffers = page.scripts.media.source_buffers,
                        media_can_play_queries = page.scripts.media.can_play_type_queries,
                        media_can_play_supported = page.scripts.media.can_play_type_supported,
                        canvas_surfaces = page.scripts.media.canvas_surfaces,
                        canvas_contexts = page.scripts.media.canvas_contexts,
                        canvas_commands = page.scripts.media.canvas_commands,
                        workers_created = page.scripts.media.workers_created,
                        workers_loaded = page.scripts.media.workers_loaded,
                        worker_bytes = page.scripts.media.worker_bytes,
                        youtube_signals = page.scripts.media.youtube_signals,
                        media_summary = %page.scripts.media.compact(),
                        service_worker_effects = page.scripts.service_worker.effects,
                        service_worker_registrations = page.scripts.service_worker.registrations,
                        service_worker_script_fetches = page.scripts.service_worker.script_fetches,
                        service_worker_script_failures = page.scripts.service_worker.script_fetch_failures,
                        service_worker_cache_opens = page.scripts.service_worker.cache_opens,
                        service_worker_cache_adds = page.scripts.service_worker.cache_adds,
                        service_worker_cache_hits = page.scripts.service_worker.cache_hits,
                        service_worker_cache_misses = page.scripts.service_worker.cache_misses,
                        service_worker_precache_fetches = page.scripts.service_worker.precache_fetches,
                        service_worker_precache_failures = page.scripts.service_worker.precache_failures,
                        service_worker_precache_bytes = page.scripts.service_worker.precache_bytes,
                        service_worker_summary = %page.scripts.service_worker.compact(),
                        images_discovered = page.images_discovered,
                        images_decoded = page.images_decoded,
                        images_failed = page.images_failed,
                        image_memory_hits = page.image_memory_hits,
                        image_disk_hits = page.image_disk_hits,
                        image_network_fetches = page.image_network_fetches,
                        image_disabled_fetches = page.image_disabled_fetches,
                        page_cache_source = page.page_cache_source.as_str(),
                        resources_total = page.resources.total,
                        resources_succeeded = page.resources.succeeded,
                        resources_failed = page.resources.failed,
                        resources_bytes = page.resources.bytes,
                        resource_network_fetches = page.resources.network_fetches,
                        resource_memory_hits = page.resources.memory_hits,
                        resource_disk_hits = page.resources.disk_hits,
                        resource_summary = %page.resources.compact(),
                        elapsed_ms = page.elapsed_ms,
                        tab_id = message.tab_id.value(),
                        "navigation loaded"
                    );
                }
                PipelineResult::Failed {
                    message: error,
                    elapsed_ms,
                } => {
                    let _ = self.tabs.mark_failed(message.tab_id);

                    if message.tab_id == self.tabs.active_id() {
                        self.chrome.set_error(format!("Navigation failed: {error}"));
                    }

                    warn!(
                        url = %message.url,
                        elapsed_ms,
                        error = %error,
                        tab_id = message.tab_id.value(),
                        "navigation failed"
                    );
                }
            }

            self.sync_browser_to_renderer();
            self.request_redraw();
        }
    }

    fn execute_page_form_click(&mut self, window: &Window) -> bool {
        let Some(hit) = self.hit_test_page_form_control(window) else {
            if self.page_forms.has_focus() {
                if let Some(mut document) = self.tabs.active_document() {
                    let _ = present::focus_form_control(&mut document, None);
                    self.page_forms.clear_focus();
                    self.commit_active_document(document);
                }
            }
            return false;
        };

        let Some(mut document) = self.tabs.active_document() else {
            return false;
        };

        self.page_dom.dispatch_form_control_click(&hit);

        let current_url = self.tabs.current_url().to_owned();
        let action = self
            .page_forms
            .handle_click(&mut document, &current_url, &hit);
        self.page_dom.dispatch_form_document_mutation(&document);
        self.commit_active_document(document);
        self.execute_page_form_action(action);
        true
    }

    fn execute_page_form_key(&mut self, key: &winit::keyboard::Key) -> bool {
        if !self.page_forms.has_focus() {
            return false;
        }

        let Some(mut document) = self.tabs.active_document() else {
            self.page_forms.clear_focus();
            return false;
        };

        let current_url = self.tabs.current_url().to_owned();
        let action = self
            .page_forms
            .handle_key(&mut document, &current_url, key, self.modifiers);

        if action == PageFormAction::None {
            return false;
        }

        self.page_dom.dispatch_form_document_mutation(&document);
        self.commit_active_document(document);
        self.execute_page_form_action(action);
        true
    }

    fn execute_page_form_action(&mut self, action: PageFormAction) {
        match action {
            PageFormAction::None | PageFormAction::Mutated => {}
            PageFormAction::Submit(url) => {
                info!(url = %url, "submitting GET form");
                self.page_forms.clear_focus();
                let target = self.tabs.navigate_active_to(url);
                self.begin_navigation(target);
            }
            PageFormAction::Error(error) => {
                warn!(error = %error, "form action failed");
                self.chrome.set_error(error);
            }
        }
    }

    fn execute_page_click(&mut self, window: &Window) {
        let Some(link) = self.hit_test_page_link(window) else {
            return;
        };

        self.page_dom.dispatch_link_click(&link);

        match resolve_link_url(self.tabs.current_url(), &link.href) {
            Ok(url) => {
                info!(href = %link.href, resolved = %url, text = %link.text, "clicked page link");
                let target = self.tabs.navigate_active_to(url);
                self.begin_navigation(target);
            }
            Err(error) => {
                warn!(href = %link.href, error = %error, "ignored unsupported page link");
                self.chrome.set_error(error.to_string());
            }
        }
    }

    fn update_hover_feedback(&mut self, window: &Window) {
        let width = window.inner_size().width as f32;
        self.chrome
            .set_cursor_position(self.cursor_x, self.cursor_y);

        let chrome_cursor = self.chrome.cursor_icon(width);
        if chrome_cursor != CursorIcon::Default {
            self.chrome.set_hovered_link(None);
            window.set_cursor(chrome_cursor);
            return;
        }

        if let Some(control) = self.hit_test_page_form_control(window) {
            self.chrome.set_hovered_link(control.name.as_deref());
            let cursor = if control.kind.is_text_editable() {
                CursorIcon::Text
            } else {
                CursorIcon::Pointer
            };
            window.set_cursor(cursor);
        } else if let Some(link) = self.hit_test_page_link(window) {
            self.chrome.set_hovered_link(Some(&link.href));
            window.set_cursor(CursorIcon::Pointer);
        } else {
            self.chrome.set_hovered_link(None);
            window.set_cursor(CursorIcon::Default);
        }
    }

    fn hit_test_page_link(&self, window: &Window) -> Option<present::LinkHitResult> {
        if self.cursor_y < TOOLBAR_HEIGHT {
            return None;
        }

        let snapshot = self.paint_state.snapshot()?;
        let document = snapshot.document.as_ref()?;
        let size = window.inner_size();
        let width = size.width as f32;
        let page_height = ((size.height as f32) - TOOLBAR_HEIGHT).max(1.0);
        let page_x = self.cursor_x;
        let page_y = self.cursor_y - TOOLBAR_HEIGHT;

        present::hit_test_link(document, width, page_height, page_x, page_y)
    }

    fn hit_test_page_form_control(&self, window: &Window) -> Option<present::FormControlHitResult> {
        if self.cursor_y < TOOLBAR_HEIGHT {
            return None;
        }

        let snapshot = self.paint_state.snapshot()?;
        let document = snapshot.document.as_ref()?;
        let size = window.inner_size();
        let width = size.width as f32;
        let page_height = ((size.height as f32) - TOOLBAR_HEIGHT).max(1.0);
        let page_x = self.cursor_x;
        let page_y = self.cursor_y - TOOLBAR_HEIGHT;

        present::hit_test_form_control(document, width, page_height, page_x, page_y)
    }

    fn clear_active_page_surface(&self) {
        if !self
            .paint_state
            .set_document(present::RenderDocument::new())
        {
            warn!("paint document state lock is poisoned; renderer document was not cleared");
        }
        if !self.paint_state.clear_images() {
            warn!("paint image state lock is poisoned; old images may remain until next document update");
        }
    }

    fn apply_active_tab_to_paint_state(&mut self) {
        let content = self.tabs.active_content();

        if let Some(document) = content.document {
            self.page_dom
                .install_for_tab(self.tabs.active_id(), &document);
            if !self.paint_state.set_document(document) {
                warn!(
                    "paint document state lock is poisoned; active tab document was not displayed"
                );
            }
        } else {
            self.page_dom.clear();
            if !self
                .paint_state
                .set_document(present::RenderDocument::new())
            {
                warn!("paint document state lock is poisoned; blank tab was not displayed");
            }
        }

        if !self.paint_state.set_images(content.images) {
            warn!("paint image state lock is poisoned; active tab images were not displayed");
        }
    }

    fn commit_active_document(&mut self, document: present::RenderDocument) {
        self.page_dom.sync_from_document(&document);
        let invalidation = self.page_dom.take_invalidation();

        if !self.tabs.set_active_document(document.clone()) {
            warn!("active tab document could not be updated");
            return;
        }

        if !self
            .paint_state
            .set_document_with_invalidation(document, invalidation)
        {
            warn!("paint document state lock is poisoned; incremental document update was not displayed");
        }

        let content = self.tabs.active_content();
        if !self.paint_state.set_images(content.images) {
            warn!("paint image state lock is poisoned; active tab images were not refreshed");
        }
    }

    fn sync_browser_to_renderer(&mut self) {
        self.chrome.set_navigation_state(
            self.tabs.current_url(),
            self.tabs.can_go_back(),
            self.tabs.can_go_forward(),
        );
        self.chrome.set_tabs(self.tabs.snapshots());

        if !self.paint_state.set_chrome(self.chrome.snapshot()) {
            warn!("paint chrome state lock is poisoned; renderer chrome was not updated");
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() -> Result<()> {
    util::init_tracing();

    let cli = Cli::parse();
    let initial_url = normalize_user_url(&cli.url).context("invalid initial URL")?;

    info!(url = %initial_url, "starting Sylphos");

    let cache = create_cache_store(&cli)?;
    let resources = ResourceScheduler::new(cache.clone());
    let history_path = cache.root().join("history").join("history.json");
    let history = BrowserHistory::load(history_path);

    let event_loop = EventLoop::new()?;
    let clear_rgba = Arc::new(Mutex::new([0.10, 0.10, 0.15, 1.0]));
    let paint_state = SharedPaintState::new();
    let (pipeline_tx, pipeline_rx) = mpsc::channel();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()
        .context("failed to create Tokio runtime")?;

    let tabs = TabManager::new(initial_url.clone());
    let mut chrome = BrowserChrome::new(initial_url);
    chrome.set_navigation_state(
        tabs.current_url(),
        tabs.can_go_back(),
        tabs.can_go_forward(),
    );
    chrome.set_tabs(tabs.snapshots());
    let _ = paint_state.set_chrome(chrome.snapshot());

    let mut app = App {
        clear_rgba,
        paint_state,
        window: None,
        renderer: None,
        rt,
        tabs,
        history,
        chrome,
        page_forms: PageFormController::default(),
        page_dom: PageDomController::default(),
        pipeline_tx,
        pipeline_rx,
        active_navigation_id: 0,
        resources,
        modifiers: ModifiersState::default(),
        cursor_x: 0.0,
        cursor_y: 0.0,
        has_started_pipeline: false,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

#[derive(Debug)]
struct LoadedPage {
    title: Option<String>,
    bytes: usize,
    blocks: usize,
    stylesheets: StylesheetLoadSummary,
    scripts: ScriptExecutionSummary,
    images_discovered: usize,
    images_decoded: usize,
    images_failed: usize,
    image_memory_hits: usize,
    image_disk_hits: usize,
    image_network_fetches: usize,
    image_disabled_fetches: usize,
    page_cache_source: CacheSource,
    resources: ResourceSummary,
    elapsed_ms: u128,
    document: present::RenderDocument,
    images: DecodedImageStore,
}

impl LoadedPage {
    fn with_elapsed(mut self, elapsed_ms: u128) -> Self {
        self.elapsed_ms = elapsed_ms;
        self
    }
}

async fn load_url(url: &str, resources: ResourceScheduler) -> Result<LoadedPage> {
    let page_resource = resources
        .fetch_text(ResourceRequest::document(url.to_owned()))
        .await?;
    let page_cache_source = page_resource.source;
    let page_url = page_resource.url;
    let bytes = page_resource.bytes;
    let text = page_resource.text;

    info!(
        url = %page_url,
        bytes,
        cache_source = page_cache_source.as_str(),
        fetch_ms = page_resource.timing.fetch.as_millis(),
        "loaded response body through resource scheduler"
    );

    let document = html_mvp::parse(&text)?;
    let mut render_document = present::extract_render_document(&document);
    let stylesheet_summary =
        load_and_apply_stylesheets_with_scheduler(url, &mut render_document, &resources).await;

    let mut web_platform =
        WebPlatformHost::new(resources.cache().root().join("web-platform"), &page_url);
    let mut media_host =
        MediaCanvasWorkerHost::new(resources.cache().root().join("media-platform"), &page_url);
    let mut service_worker =
        ServiceWorkerHost::new(resources.cache().root().join("service-worker"), &page_url);
    let script_summary = execute_document_scripts(
        &document,
        &mut render_document,
        &resources,
        &page_url,
        None,
        &mut web_platform,
        &mut media_host,
        &mut service_worker,
    )
    .await;

    for requested_url in &script_summary.navigation_requests {
        warn!(
            url = %requested_url,
            "script requested navigation; DOM/browser event loop captured navigation effect for future policy handling"
        );
    }

    resolve_document_image_sources(url, &mut render_document);
    let title = render_document.title.clone();
    let blocks = render_document.blocks.len();

    let image_summary = fetch_decode_images_with_scheduler(url, &render_document, &resources).await;
    let resource_summary = resources.summary();

    let normalized = html_mvp::serialize_document(&document);
    let preview: String = normalized.chars().take(512).collect();

    info!(
        preview = %preview,
        resources = %resource_summary.compact(),
        "normalized html preview"
    );

    Ok(LoadedPage {
        title,
        bytes,
        blocks,
        stylesheets: stylesheet_summary,
        scripts: script_summary,
        images_discovered: image_summary.discovered,
        images_decoded: image_summary.decoded,
        images_failed: image_summary.failed,
        image_memory_hits: image_summary.memory_hits,
        image_disk_hits: image_summary.disk_hits,
        image_network_fetches: image_summary.network_fetches,
        image_disabled_fetches: image_summary.disabled_fetches,
        page_cache_source,
        resources: resource_summary,
        elapsed_ms: 0,
        document: render_document,
        images: image_summary.store,
    })
}

fn create_cache_store(cli: &Cli) -> Result<CacheStore> {
    let policy = if cli.disable_cache {
        CachePolicy::disabled()
    } else {
        CachePolicy::default()
    };

    let cache = if let Some(cache_dir) = &cli.cache_dir {
        CacheStore::with_root(cache_dir.clone(), policy)
    } else if cli.disable_cache {
        CacheStore::disabled()
    } else {
        CacheStore::new(policy)
    };

    if cli.clear_cache {
        cache.clear().context("failed to clear Sylphos cache")?;
    }

    info!(
        enabled = cache.is_enabled(),
        cache_dir = %cache.root_display(),
        clear_cache = cli.clear_cache,
        "cache layer initialized"
    );

    Ok(cache)
}

fn load_window_icon() -> Option<winit::window::Icon> {
    const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.png");
    const MAX_ICON_SIZE: u32 = 256;

    let image = match image::load_from_memory(ICON_BYTES) {
        Ok(image) => image.resize(
            MAX_ICON_SIZE,
            MAX_ICON_SIZE,
            image::imageops::FilterType::Lanczos3,
        ),
        Err(error) => {
            tracing::warn!(error = %error, "failed to load window icon image");
            return None;
        }
    }
    .into_rgba8();

    let (width, height) = image.dimensions();
    let rgba = image.into_raw();

    match winit::window::Icon::from_rgba(rgba, width, height) {
        Ok(icon) => Some(icon),
        Err(error) => {
            tracing::warn!(error = %error, "failed to create window icon");
            None
        }
    }
}
