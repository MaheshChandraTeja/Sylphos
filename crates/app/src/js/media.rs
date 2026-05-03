#![allow(dead_code)]

//! Media, Canvas, Worker, and YouTube-compatibility host layer.
//!
//! Module 27 does not pretend that a full Chromium media stack appears by
//! magic. It creates the host boundary needed by a real JavaScript runtime:
//! media element state, canvas command capture, worker script loading, MediaSource
//! readiness, capability queries, and YouTube boot-signal diagnostics. The
//! current intrinsic executor captures common script patterns and routes them
//! through this module so the rest of the engine can observe and schedule work
//! deterministically.

use crate::browser::{ResourceRequest, ResourceScheduler};
use anyhow::{Context, Result};
use std::{collections::BTreeMap, path::PathBuf};
use tracing::{debug, warn};
use url::Url;

const MAX_MEDIA_EFFECTS_PER_SCRIPT: usize = 192;
const MAX_WORKERS_PER_DOCUMENT: usize = 16;
const MAX_WORKER_SCRIPT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CANVAS_COMMANDS_PER_SURFACE: usize = 4096;

/// Captured media/canvas/worker host effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MediaCanvasWorkerEffect {
    /// `<video>` / `<audio>` or `new Audio(...)` was observed.
    MediaElementCreated {
        kind: MediaElementKind,
        src: Option<String>,
    },

    /// A media `src` assignment was observed.
    MediaSourceAssigned {
        selector: Option<String>,
        src: String,
    },

    /// `new MediaSource()` or MSE-ish API usage was observed.
    MediaSourceObjectCreated,

    /// `mediaSource.addSourceBuffer(...)` was observed.
    SourceBufferAdded { mime: String },

    /// `video.canPlayType(...)` / `audio.canPlayType(...)` was observed.
    CanPlayType { mime: String },

    /// Playback control was observed.
    MediaControl {
        selector: Option<String>,
        action: MediaControlAction,
    },

    /// Canvas construction or query was observed.
    CanvasCreated { selector: Option<String> },

    /// `canvas.getContext(...)` was observed.
    CanvasContextRequested {
        selector: Option<String>,
        context: CanvasContextKind,
    },

    /// A 2D canvas drawing command was observed.
    CanvasCommand {
        selector: Option<String>,
        command: CanvasCommandLite,
    },

    /// `canvas.toDataURL()` was observed.
    CanvasSnapshotRequested { selector: Option<String> },

    /// `new Worker(...)` was observed.
    WorkerCreated { url: String },

    /// `worker.postMessage(...)` was observed.
    WorkerPostMessage { payload_preview: String },

    /// `worker.terminate()` was observed.
    WorkerTerminate,

    /// `performance.now()` was observed.
    PerformanceNow,

    /// YouTube-specific boot signal was observed.
    YoutubeSignal { name: String },
}

/// Media element kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaElementKind {
    Audio,
    Video,
}

impl MediaElementKind {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }
}

/// Media control action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaControlAction {
    Play,
    Pause,
    Load,
    Seek,
    SetVolume,
    SetMuted,
}

impl MediaControlAction {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Load => "load",
            Self::Seek => "seek",
            Self::SetVolume => "set-volume",
            Self::SetMuted => "set-muted",
        }
    }
}

/// Canvas context kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanvasContextKind {
    TwoD,
    WebGl,
    WebGl2,
    BitmapRenderer,
    Unknown,
}

impl CanvasContextKind {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TwoD => "2d",
            Self::WebGl => "webgl",
            Self::WebGl2 => "webgl2",
            Self::BitmapRenderer => "bitmaprenderer",
            Self::Unknown => "unknown",
        }
    }
}

/// Simplified captured canvas command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CanvasCommandLite {
    FillRect,
    ClearRect,
    StrokeRect,
    DrawImage,
    FillText,
    StrokeText,
    BeginPath,
    MoveTo,
    LineTo,
    Arc,
    Stroke,
    Fill,
    PutImageData,
    Unknown(String),
}

impl CanvasCommandLite {
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::FillRect => "fillRect",
            Self::ClearRect => "clearRect",
            Self::StrokeRect => "strokeRect",
            Self::DrawImage => "drawImage",
            Self::FillText => "fillText",
            Self::StrokeText => "strokeText",
            Self::BeginPath => "beginPath",
            Self::MoveTo => "moveTo",
            Self::LineTo => "lineTo",
            Self::Arc => "arc",
            Self::Stroke => "stroke",
            Self::Fill => "fill",
            Self::PutImageData => "putImageData",
            Self::Unknown(name) => name.as_str(),
        }
    }
}

/// Capture output from scanning one script.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MediaCanvasWorkerCapture {
    pub effects: Vec<MediaCanvasWorkerEffect>,
    pub warnings: Vec<String>,
}

/// Host summary accumulated during one document script pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MediaCanvasWorkerSummary {
    pub effects: usize,
    pub media_elements: usize,
    pub media_src_assignments: usize,
    pub media_controls: usize,
    pub media_source_objects: usize,
    pub source_buffers: usize,
    pub can_play_type_queries: usize,
    pub can_play_type_supported: usize,
    pub canvas_surfaces: usize,
    pub canvas_contexts: usize,
    pub canvas_commands: usize,
    pub canvas_snapshots: usize,
    pub workers_created: usize,
    pub workers_loaded: usize,
    pub workers_failed: usize,
    pub worker_bytes: usize,
    pub worker_messages: usize,
    pub workers_terminated: usize,
    pub performance_queries: usize,
    pub youtube_signals: usize,
    pub warnings: usize,
    pub errors: usize,
}

impl MediaCanvasWorkerSummary {
    /// Merges another summary into this one.
    pub(crate) fn merge_from(&mut self, other: Self) {
        self.effects = self.effects.saturating_add(other.effects);
        self.media_elements = self.media_elements.saturating_add(other.media_elements);
        self.media_src_assignments = self
            .media_src_assignments
            .saturating_add(other.media_src_assignments);
        self.media_controls = self.media_controls.saturating_add(other.media_controls);
        self.media_source_objects = self
            .media_source_objects
            .saturating_add(other.media_source_objects);
        self.source_buffers = self.source_buffers.saturating_add(other.source_buffers);
        self.can_play_type_queries = self
            .can_play_type_queries
            .saturating_add(other.can_play_type_queries);
        self.can_play_type_supported = self
            .can_play_type_supported
            .saturating_add(other.can_play_type_supported);
        self.canvas_surfaces = self.canvas_surfaces.saturating_add(other.canvas_surfaces);
        self.canvas_contexts = self.canvas_contexts.saturating_add(other.canvas_contexts);
        self.canvas_commands = self.canvas_commands.saturating_add(other.canvas_commands);
        self.canvas_snapshots = self.canvas_snapshots.saturating_add(other.canvas_snapshots);
        self.workers_created = self.workers_created.saturating_add(other.workers_created);
        self.workers_loaded = self.workers_loaded.saturating_add(other.workers_loaded);
        self.workers_failed = self.workers_failed.saturating_add(other.workers_failed);
        self.worker_bytes = self.worker_bytes.saturating_add(other.worker_bytes);
        self.worker_messages = self.worker_messages.saturating_add(other.worker_messages);
        self.workers_terminated = self
            .workers_terminated
            .saturating_add(other.workers_terminated);
        self.performance_queries = self
            .performance_queries
            .saturating_add(other.performance_queries);
        self.youtube_signals = self.youtube_signals.saturating_add(other.youtube_signals);
        self.warnings = self.warnings.saturating_add(other.warnings);
        self.errors = self.errors.saturating_add(other.errors);
    }

    /// Compact diagnostics string for logs.
    #[must_use]
    pub(crate) fn compact(&self) -> String {
        format!(
            "effects={} media={} mse={} can_play={} canvas={} canvas_cmd={} workers={} workers_loaded={} worker_bytes={} yt={}",
            self.effects,
            self.media_elements,
            self.media_source_objects,
            self.can_play_type_queries,
            self.canvas_surfaces,
            self.canvas_commands,
            self.workers_created,
            self.workers_loaded,
            self.worker_bytes,
            self.youtube_signals,
        )
    }
}

/// Media/canvas/worker host state for one document runtime.
#[derive(Debug, Clone)]
pub(crate) struct MediaCanvasWorkerHost {
    document_url: String,
    media_elements: Vec<MediaElementState>,
    canvases: BTreeMap<String, CanvasSurfaceState>,
    workers: Vec<WorkerState>,
    _profile_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaElementState {
    id: usize,
    kind: MediaElementKind,
    src: Option<String>,
    controls: Vec<MediaControlAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanvasSurfaceState {
    selector: Option<String>,
    context: Option<CanvasContextKind>,
    commands: Vec<CanvasCommandLite>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkerState {
    url: String,
    loaded: bool,
    bytes: usize,
}

impl MediaCanvasWorkerHost {
    /// Creates a host for one document.
    pub(crate) fn new(root: impl Into<PathBuf>, document_url: &str) -> Self {
        Self {
            document_url: document_url.to_owned(),
            media_elements: Vec::new(),
            canvases: BTreeMap::new(),
            workers: Vec::new(),
            _profile_root: root.into(),
        }
    }

    /// Applies captured effects through browser services.
    pub(crate) async fn apply_effects(
        &mut self,
        effects: &[MediaCanvasWorkerEffect],
        scheduler: &ResourceScheduler,
    ) -> MediaCanvasWorkerSummary {
        let mut summary = MediaCanvasWorkerSummary::default();

        for effect in effects.iter().take(MAX_MEDIA_EFFECTS_PER_SCRIPT) {
            summary.effects = summary.effects.saturating_add(1);

            match effect {
                MediaCanvasWorkerEffect::MediaElementCreated { kind, src } => {
                    summary.media_elements = summary.media_elements.saturating_add(1);
                    self.media_elements.push(MediaElementState {
                        id: self.media_elements.len().saturating_add(1),
                        kind: *kind,
                        src: src.clone(),
                        controls: Vec::new(),
                    });
                }
                MediaCanvasWorkerEffect::MediaSourceAssigned { selector, src } => {
                    summary.media_src_assignments = summary.media_src_assignments.saturating_add(1);
                    let resolved = resolve_against_document(&self.document_url, src)
                        .unwrap_or_else(|_| src.clone());
                    let kind = if selector
                        .as_deref()
                        .is_some_and(|value| value.to_ascii_lowercase().contains("audio"))
                    {
                        MediaElementKind::Audio
                    } else {
                        MediaElementKind::Video
                    };
                    self.media_elements.push(MediaElementState {
                        id: self.media_elements.len().saturating_add(1),
                        kind,
                        src: Some(resolved),
                        controls: Vec::new(),
                    });
                }
                MediaCanvasWorkerEffect::MediaSourceObjectCreated => {
                    summary.media_source_objects = summary.media_source_objects.saturating_add(1);
                }
                MediaCanvasWorkerEffect::SourceBufferAdded { mime } => {
                    summary.source_buffers = summary.source_buffers.saturating_add(1);
                    if media_capability_for_mime(mime) != MediaCapability::Unsupported {
                        summary.can_play_type_supported =
                            summary.can_play_type_supported.saturating_add(1);
                    }
                }
                MediaCanvasWorkerEffect::CanPlayType { mime } => {
                    summary.can_play_type_queries = summary.can_play_type_queries.saturating_add(1);
                    if media_capability_for_mime(mime) != MediaCapability::Unsupported {
                        summary.can_play_type_supported =
                            summary.can_play_type_supported.saturating_add(1);
                    }
                }
                MediaCanvasWorkerEffect::MediaControl { action, .. } => {
                    summary.media_controls = summary.media_controls.saturating_add(1);
                    if let Some(element) = self.media_elements.last_mut() {
                        element.controls.push(*action);
                    }
                }
                MediaCanvasWorkerEffect::CanvasCreated { selector } => {
                    summary.canvas_surfaces = summary.canvas_surfaces.saturating_add(1);
                    let key = canvas_key(selector.as_deref(), self.canvases.len());
                    self.canvases.entry(key).or_insert(CanvasSurfaceState {
                        selector: selector.clone(),
                        context: None,
                        commands: Vec::new(),
                    });
                }
                MediaCanvasWorkerEffect::CanvasContextRequested { selector, context } => {
                    summary.canvas_contexts = summary.canvas_contexts.saturating_add(1);
                    let key = canvas_key(selector.as_deref(), self.canvases.len());
                    let surface = self.canvases.entry(key).or_insert(CanvasSurfaceState {
                        selector: selector.clone(),
                        context: None,
                        commands: Vec::new(),
                    });
                    surface.context = Some(*context);
                    if matches!(
                        context,
                        CanvasContextKind::WebGl | CanvasContextKind::WebGl2
                    ) {
                        summary.warnings = summary.warnings.saturating_add(1);
                        warn!(context = context.as_str(), "WebGL context requested; Module 27 records it but does not implement GPU canvas yet");
                    }
                }
                MediaCanvasWorkerEffect::CanvasCommand { selector, command } => {
                    summary.canvas_commands = summary.canvas_commands.saturating_add(1);
                    let key = canvas_key(selector.as_deref(), self.canvases.len());
                    let surface = self.canvases.entry(key).or_insert(CanvasSurfaceState {
                        selector: selector.clone(),
                        context: Some(CanvasContextKind::TwoD),
                        commands: Vec::new(),
                    });
                    if surface.commands.len() < MAX_CANVAS_COMMANDS_PER_SURFACE {
                        surface.commands.push(command.clone());
                    }
                }
                MediaCanvasWorkerEffect::CanvasSnapshotRequested { .. } => {
                    summary.canvas_snapshots = summary.canvas_snapshots.saturating_add(1);
                }
                MediaCanvasWorkerEffect::WorkerCreated { url } => {
                    summary.workers_created = summary.workers_created.saturating_add(1);
                    if self.workers.len() >= MAX_WORKERS_PER_DOCUMENT {
                        summary.workers_failed = summary.workers_failed.saturating_add(1);
                        summary.warnings = summary.warnings.saturating_add(1);
                        warn!(url = %url, "skipped worker due to per-document worker limit");
                        continue;
                    }

                    match self.load_worker_script(url, scheduler).await {
                        Ok(bytes) => {
                            summary.workers_loaded = summary.workers_loaded.saturating_add(1);
                            summary.worker_bytes = summary.worker_bytes.saturating_add(bytes);
                        }
                        Err(error) => {
                            summary.workers_failed = summary.workers_failed.saturating_add(1);
                            summary.errors = summary.errors.saturating_add(1);
                            warn!(url = %url, error = %error, "failed to load worker script");
                        }
                    }
                }
                MediaCanvasWorkerEffect::WorkerPostMessage { .. } => {
                    summary.worker_messages = summary.worker_messages.saturating_add(1);
                }
                MediaCanvasWorkerEffect::WorkerTerminate => {
                    summary.workers_terminated = summary.workers_terminated.saturating_add(1);
                }
                MediaCanvasWorkerEffect::PerformanceNow => {
                    summary.performance_queries = summary.performance_queries.saturating_add(1);
                }
                MediaCanvasWorkerEffect::YoutubeSignal { name } => {
                    summary.youtube_signals = summary.youtube_signals.saturating_add(1);
                    debug!(signal = %name, "detected YouTube compatibility boot signal");
                }
            }
        }

        if effects.len() > MAX_MEDIA_EFFECTS_PER_SCRIPT {
            summary.warnings = summary.warnings.saturating_add(1);
        }

        summary
    }

    async fn load_worker_script(
        &mut self,
        raw_url: &str,
        scheduler: &ResourceScheduler,
    ) -> Result<usize> {
        let url = resolve_against_document(&self.document_url, raw_url)?;
        let resource = scheduler
            .fetch_text(ResourceRequest::script(url.clone()).max_bytes(MAX_WORKER_SCRIPT_BYTES))
            .await
            .with_context(|| format!("failed to fetch worker `{url}`"))?;
        let bytes = resource.bytes;
        self.workers.push(WorkerState {
            url,
            loaded: true,
            bytes,
        });
        Ok(bytes)
    }
}

/// Captures media/canvas/worker effects from script text.
#[must_use]
pub(crate) fn capture_media_canvas_worker_effects(source: &str) -> MediaCanvasWorkerCapture {
    let mut capture = MediaCanvasWorkerCapture::default();

    capture_media_elements(source, &mut capture);
    capture_media_source(source, &mut capture);
    capture_canvas(source, &mut capture);
    capture_workers(source, &mut capture);
    capture_performance(source, &mut capture);
    capture_youtube_signals(source, &mut capture);

    if capture.effects.len() > MAX_MEDIA_EFFECTS_PER_SCRIPT {
        capture.warnings.push(format!(
            "media/canvas/worker effect count exceeded {}; later effects will be ignored by host",
            MAX_MEDIA_EFFECTS_PER_SCRIPT
        ));
    }

    capture
}

fn capture_media_elements(source: &str, capture: &mut MediaCanvasWorkerCapture) {
    for (needle, kind) in [
        ("document.createElement('video')", MediaElementKind::Video),
        ("document.createElement(\"video\")", MediaElementKind::Video),
        ("document.createElement('audio')", MediaElementKind::Audio),
        ("document.createElement(\"audio\")", MediaElementKind::Audio),
        ("new Audio", MediaElementKind::Audio),
    ] {
        let count = source.match_indices(needle).count();
        for _ in 0..count {
            capture
                .effects
                .push(MediaCanvasWorkerEffect::MediaElementCreated { kind, src: None });
        }
    }

    for src in capture_assignment_values(source, ".src") {
        if looks_like_media_url(&src) {
            capture
                .effects
                .push(MediaCanvasWorkerEffect::MediaSourceAssigned {
                    selector: None,
                    src,
                });
        }
    }

    for (needle, action) in [
        (".play()", MediaControlAction::Play),
        (".pause()", MediaControlAction::Pause),
        (".load()", MediaControlAction::Load),
        (".currentTime", MediaControlAction::Seek),
        (".volume", MediaControlAction::SetVolume),
        (".muted", MediaControlAction::SetMuted),
    ] {
        for _ in source.match_indices(needle) {
            capture.effects.push(MediaCanvasWorkerEffect::MediaControl {
                selector: None,
                action,
            });
        }
    }

    let mut offset = 0usize;
    while let Some(relative) = source[offset..].find("canPlayType") {
        let absolute = offset + relative;
        let after = absolute + "canPlayType".len();
        if let Some((args, end)) = extract_parenthesized(source, after) {
            if let Some(mime) = first_string_literal(args) {
                capture
                    .effects
                    .push(MediaCanvasWorkerEffect::CanPlayType { mime });
            }
            offset = end;
        } else {
            break;
        }
    }
}

fn capture_media_source(source: &str, capture: &mut MediaCanvasWorkerCapture) {
    for needle in ["new MediaSource", "MediaSource.isTypeSupported"] {
        for _ in source.match_indices(needle) {
            capture
                .effects
                .push(MediaCanvasWorkerEffect::MediaSourceObjectCreated);
        }
    }

    let mut offset = 0usize;
    while let Some(relative) = source[offset..].find("addSourceBuffer") {
        let absolute = offset + relative;
        let after = absolute + "addSourceBuffer".len();
        if let Some((args, end)) = extract_parenthesized(source, after) {
            if let Some(mime) = first_string_literal(args) {
                capture
                    .effects
                    .push(MediaCanvasWorkerEffect::SourceBufferAdded { mime });
            }
            offset = end;
        } else {
            break;
        }
    }
}

fn capture_canvas(source: &str, capture: &mut MediaCanvasWorkerCapture) {
    for needle in [
        "document.createElement('canvas')",
        "document.createElement(\"canvas\")",
        "HTMLCanvasElement",
    ] {
        for _ in source.match_indices(needle) {
            capture
                .effects
                .push(MediaCanvasWorkerEffect::CanvasCreated { selector: None });
        }
    }

    let mut offset = 0usize;
    while let Some(relative) = source[offset..].find("getContext") {
        let absolute = offset + relative;
        let after = absolute + "getContext".len();
        if let Some((args, end)) = extract_parenthesized(source, after) {
            let context = first_string_literal(args).map_or(CanvasContextKind::Unknown, |value| {
                classify_canvas_context(&value)
            });
            capture
                .effects
                .push(MediaCanvasWorkerEffect::CanvasContextRequested {
                    selector: None,
                    context,
                });
            offset = end;
        } else {
            break;
        }
    }

    for (needle, command) in [
        (".fillRect(", CanvasCommandLite::FillRect),
        (".clearRect(", CanvasCommandLite::ClearRect),
        (".strokeRect(", CanvasCommandLite::StrokeRect),
        (".drawImage(", CanvasCommandLite::DrawImage),
        (".fillText(", CanvasCommandLite::FillText),
        (".strokeText(", CanvasCommandLite::StrokeText),
        (".beginPath(", CanvasCommandLite::BeginPath),
        (".moveTo(", CanvasCommandLite::MoveTo),
        (".lineTo(", CanvasCommandLite::LineTo),
        (".arc(", CanvasCommandLite::Arc),
        (".stroke(", CanvasCommandLite::Stroke),
        (".fill(", CanvasCommandLite::Fill),
        (".putImageData(", CanvasCommandLite::PutImageData),
    ] {
        for _ in source.match_indices(needle) {
            capture
                .effects
                .push(MediaCanvasWorkerEffect::CanvasCommand {
                    selector: None,
                    command: command.clone(),
                });
        }
    }

    for _ in source.match_indices(".toDataURL(") {
        capture
            .effects
            .push(MediaCanvasWorkerEffect::CanvasSnapshotRequested { selector: None });
    }
}

fn capture_workers(source: &str, capture: &mut MediaCanvasWorkerCapture) {
    let mut offset = 0usize;
    while let Some(relative) = source[offset..].find("new Worker") {
        let absolute = offset + relative;
        let after = absolute + "new Worker".len();
        if let Some((args, end)) = extract_parenthesized(source, after) {
            if let Some(url) = first_string_literal(args) {
                capture
                    .effects
                    .push(MediaCanvasWorkerEffect::WorkerCreated { url });
            }
            offset = end;
        } else {
            break;
        }
    }

    for needle in [".postMessage(", "postMessage("] {
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(needle) {
            let absolute = offset + relative;
            let after = absolute + needle.len() - 1;
            if let Some((args, end)) = extract_parenthesized(source, after) {
                capture
                    .effects
                    .push(MediaCanvasWorkerEffect::WorkerPostMessage {
                        payload_preview: preview(args, 96),
                    });
                offset = end;
            } else {
                break;
            }
        }
    }

    for _ in source.match_indices(".terminate()") {
        capture
            .effects
            .push(MediaCanvasWorkerEffect::WorkerTerminate);
    }
}

fn capture_performance(source: &str, capture: &mut MediaCanvasWorkerCapture) {
    for _ in source.match_indices("performance.now") {
        capture
            .effects
            .push(MediaCanvasWorkerEffect::PerformanceNow);
    }
}

fn capture_youtube_signals(source: &str, capture: &mut MediaCanvasWorkerCapture) {
    for signal in [
        "ytInitialData",
        "ytInitialPlayerResponse",
        "ytcfg.set",
        "ytd-app",
        "adaptiveFormats",
        "streamingData",
        "playerResponse",
        "base.js",
        "www-player",
        "innertube",
    ] {
        if source.contains(signal) {
            capture
                .effects
                .push(MediaCanvasWorkerEffect::YoutubeSignal {
                    name: signal.to_owned(),
                });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaCapability {
    Probably,
    Maybe,
    Unsupported,
}

fn media_capability_for_mime(mime: &str) -> MediaCapability {
    let normalized = mime.to_ascii_lowercase();

    if normalized.contains("video/mp4")
        || normalized.contains("avc1")
        || normalized.contains("audio/mp4")
        || normalized.contains("mp4a")
    {
        return MediaCapability::Probably;
    }

    if normalized.contains("video/webm")
        || normalized.contains("vp9")
        || normalized.contains("vp09")
        || normalized.contains("opus")
        || normalized.contains("audio/webm")
    {
        return MediaCapability::Maybe;
    }

    MediaCapability::Unsupported
}

fn classify_canvas_context(value: &str) -> CanvasContextKind {
    match value.trim().to_ascii_lowercase().as_str() {
        "2d" => CanvasContextKind::TwoD,
        "webgl" | "experimental-webgl" => CanvasContextKind::WebGl,
        "webgl2" => CanvasContextKind::WebGl2,
        "bitmaprenderer" => CanvasContextKind::BitmapRenderer,
        _ => CanvasContextKind::Unknown,
    }
}

fn resolve_against_document(base_url: &str, value: &str) -> Result<String> {
    let base =
        Url::parse(base_url).with_context(|| format!("invalid document URL `{base_url}`"))?;
    let resolved = base
        .join(value.trim())
        .with_context(|| format!("invalid resource URL `{value}`"))?;

    match resolved.scheme() {
        "http" | "https" => Ok(resolved.to_string()),
        scheme => anyhow::bail!("unsupported resource scheme `{scheme}`"),
    }
}

fn canvas_key(selector: Option<&str>, fallback_index: usize) -> String {
    selector
        .filter(|value| !value.trim().is_empty())
        .map_or_else(|| format!("canvas:{fallback_index}"), ToOwned::to_owned)
}

fn looks_like_media_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains(".mp4")
        || lower.contains(".webm")
        || lower.contains(".m3u8")
        || lower.contains("videoplayback")
        || lower.contains("mime=video")
        || lower.contains("mime=audio")
}

fn capture_assignment_values(source: &str, property_needle: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;

    while let Some(relative) = source[offset..].find(property_needle) {
        let absolute = offset + relative + property_needle.len();
        let Some(eq_rel) = source[absolute..].find('=') else {
            break;
        };
        let start = absolute + eq_rel + 1;
        let Some((value, end)) = read_string_literal_at_or_after(source, start) else {
            offset = start;
            continue;
        };
        values.push(value);
        offset = end;
    }

    values
}

fn extract_parenthesized(source: &str, after_function_name: usize) -> Option<(&str, usize)> {
    let open = source[after_function_name..].find('(')? + after_function_name;
    let mut depth = 0i32;
    let mut in_quote: Option<char> = None;
    let mut escaped = false;

    for (relative, ch) in source[open..].char_indices() {
        let absolute = open + relative;

        if let Some(quote) = in_quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => in_quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&source[open + 1..absolute], absolute + 1));
                }
            }
            _ => {}
        }
    }

    None
}

fn first_string_literal(source: &str) -> Option<String> {
    read_string_literal_at_or_after(source, 0).map(|(value, _)| value)
}

fn read_string_literal_at_or_after(source: &str, start: usize) -> Option<(String, usize)> {
    let mut index = start;
    let bytes = source.as_bytes();

    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    while index < bytes.len()
        && bytes[index] != b'\''
        && bytes[index] != b'"'
        && bytes[index] != b'`'
    {
        index += 1;
    }

    if index >= bytes.len() {
        return None;
    }

    let quote = source[index..].chars().next()?;
    let mut output = String::new();
    let mut escaped = false;

    for (relative, ch) in source[index + quote.len_utf8()..].char_indices() {
        let absolute = index + quote.len_utf8() + relative;

        if escaped {
            output.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return Some((output, absolute + quote.len_utf8()));
        } else {
            output.push(ch);
        }
    }

    None
}

fn preview(value: &str, max: usize) -> String {
    let mut result = value.chars().take(max).collect::<String>();
    if value.chars().count() > max {
        result.push_str("...");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_video_canvas_worker_and_youtube_signals() {
        let source = r#"
            const v = document.createElement('video');
            v.src = 'https://example.com/videoplayback?mime=video/mp4';
            v.play();
            const ms = new MediaSource();
            ms.addSourceBuffer('video/mp4; codecs="avc1.42E01E"');
            const c = document.createElement('canvas');
            const ctx = c.getContext('2d');
            ctx.fillRect(0,0,10,10);
            const worker = new Worker('/worker.js');
            worker.postMessage({hello: true});
            performance.now();
            window.ytInitialData = {};
        "#;

        let capture = capture_media_canvas_worker_effects(source);
        assert!(capture.effects.iter().any(|effect| matches!(
            effect,
            MediaCanvasWorkerEffect::MediaElementCreated {
                kind: MediaElementKind::Video,
                ..
            }
        )));
        assert!(capture.effects.iter().any(|effect| matches!(
            effect,
            MediaCanvasWorkerEffect::CanvasContextRequested {
                context: CanvasContextKind::TwoD,
                ..
            }
        )));
        assert!(capture.effects.iter().any(|effect| matches!(
            effect,
            MediaCanvasWorkerEffect::WorkerCreated { url } if url == "/worker.js"
        )));
        assert!(capture.effects.iter().any(|effect| matches!(
            effect,
            MediaCanvasWorkerEffect::YoutubeSignal { name } if name == "ytInitialData"
        )));
    }

    #[test]
    fn recognizes_common_youtube_media_capabilities() {
        assert_eq!(
            media_capability_for_mime("video/mp4; codecs=\"avc1.42E01E\""),
            MediaCapability::Probably
        );
        assert_eq!(
            media_capability_for_mime("video/webm; codecs=\"vp9\""),
            MediaCapability::Maybe
        );
        assert_eq!(
            media_capability_for_mime("application/json"),
            MediaCapability::Unsupported
        );
    }
}
