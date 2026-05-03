#![allow(clippy::too_many_lines)]
#![doc = "MediaSource, video/audio element, and source-buffer simulation for SylJS."]

use crate::{
    create_element_object, create_inline_style_object, create_resolved_promise_value,
    dom::{DomNodeRef, SharedDomHost},
    event_loop::JsEventLoop,
    JsFunction, JsHostObject, JsObject, JsObjectKind, JsRuntimeError, JsValue, Vm,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

/// Shared media host pointer.
pub type SharedMediaHost = Rc<dyn MediaHost>;

/// Media element id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaElementId(pub u64);

/// MediaSource id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaSourceId(pub u64);

/// SourceBuffer id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceBufferId(pub u64);

/// Media element kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaElementKind {
    /// `<video>`.
    Video,

    /// `<audio>`.
    Audio,
}

impl MediaElementKind {
    fn tag_name(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }
}

/// HTMLMediaElement readyState.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaReadyState {
    /// HAVE_NOTHING.
    HaveNothing = 0,

    /// HAVE_METADATA.
    HaveMetadata = 1,

    /// HAVE_CURRENT_DATA.
    HaveCurrentData = 2,

    /// HAVE_FUTURE_DATA.
    HaveFutureData = 3,

    /// HAVE_ENOUGH_DATA.
    HaveEnoughData = 4,
}

/// HTMLMediaElement networkState.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaNetworkState {
    /// NETWORK_EMPTY.
    Empty = 0,

    /// NETWORK_IDLE.
    Idle = 1,

    /// NETWORK_LOADING.
    Loading = 2,

    /// NETWORK_NO_SOURCE.
    NoSource = 3,
}

/// MediaSource ready state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSourceState {
    /// `closed`.
    Closed,

    /// `open`.
    Open,

    /// `ended`.
    Ended,
}

impl MediaSourceState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
            Self::Ended => "ended",
        }
    }
}

/// SourceBuffer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBufferState {
    /// Idle.
    Idle,

    /// Updating.
    Updating,

    /// Removed.
    Removed,
}

/// Buffered range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BufferedRange {
    /// Start seconds.
    pub start: f64,

    /// End seconds.
    pub end: f64,
}

/// Media element snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaElementSnapshot {
    /// Element id.
    pub id: MediaElementId,

    /// Kind.
    pub kind: MediaElementKind,

    /// Optional DOM node.
    pub dom_node: Option<DomNodeRef>,

    /// Source URL.
    pub src: String,

    /// Attached media source.
    pub media_source: Option<MediaSourceId>,

    /// Current time.
    pub current_time: f64,

    /// Duration.
    pub duration: f64,

    /// Paused flag.
    pub paused: bool,

    /// Muted flag.
    pub muted: bool,

    /// Volume.
    pub volume: f64,

    /// Ready state.
    pub ready_state: MediaReadyState,

    /// Network state.
    pub network_state: MediaNetworkState,

    /// Buffered ranges.
    pub buffered: Vec<BufferedRange>,

    /// Video width.
    pub video_width: u32,

    /// Video height.
    pub video_height: u32,
}

/// MediaSource snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaSourceSnapshot {
    /// Source id.
    pub id: MediaSourceId,

    /// Ready state.
    pub ready_state: MediaSourceState,

    /// Duration.
    pub duration: f64,

    /// Buffers.
    pub source_buffers: Vec<SourceBufferId>,
}

/// SourceBuffer snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceBufferSnapshot {
    /// Buffer id.
    pub id: SourceBufferId,

    /// Parent source.
    pub source: MediaSourceId,

    /// MIME type.
    pub mime_type: String,

    /// State.
    pub state: SourceBufferState,

    /// Buffered ranges.
    pub buffered: Vec<BufferedRange>,

    /// Bytes appended.
    pub bytes_appended: usize,

    /// Segment count.
    pub segments: usize,

    /// Timestamp offset.
    pub timestamp_offset: f64,
}

/// Media event record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaEventRecord {
    /// Optional element id.
    pub element: Option<MediaElementId>,

    /// Optional source id.
    pub source: Option<MediaSourceId>,

    /// Optional source buffer id.
    pub buffer: Option<SourceBufferId>,

    /// Event type.
    pub event_type: String,
}

/// Media segment append record.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaSegmentRecord {
    /// SourceBuffer id.
    pub buffer: SourceBufferId,

    /// Bytes appended.
    pub bytes: usize,

    /// Start time.
    pub start: f64,

    /// End time.
    pub end: f64,
}

/// Media time update.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaTimeUpdate {
    /// Element id.
    pub element: MediaElementId,

    /// New current time.
    pub current_time: f64,
}

/// Media metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaMetrics {
    /// Media elements created.
    pub media_elements_created: u64,

    /// MediaSource objects created.
    pub media_sources_created: u64,

    /// SourceBuffer objects created.
    pub source_buffers_created: u64,

    /// `src` assignments.
    pub src_assignments: u64,

    /// Object URLs created.
    pub object_urls_created: u64,

    /// Object URLs revoked.
    pub object_urls_revoked: u64,

    /// play calls.
    pub play_calls: u64,

    /// pause calls.
    pub pause_calls: u64,

    /// load calls.
    pub load_calls: u64,

    /// seek/currentTime changes.
    pub seeks: u64,

    /// canPlayType checks.
    pub can_play_type_checks: u64,

    /// MediaSource attachments.
    pub media_source_attachments: u64,

    /// Buffer appends.
    pub buffer_appends: u64,

    /// Buffer removes.
    pub buffer_removes: u64,

    /// Media events queued/recorded.
    pub events: u64,
}

/// Media host abstraction.
pub trait MediaHost {
    /// Creates a media element.
    fn create_media_element(&self, kind: MediaElementKind) -> MediaElementId;

    /// Binds a DOM node to a media element id.
    fn bind_dom_node(&self, kind: MediaElementKind, node: DomNodeRef) -> MediaElementId;

    /// Returns media element for DOM node.
    fn element_for_dom_node(&self, node: DomNodeRef) -> Option<MediaElementId>;

    /// Returns element snapshot.
    fn element_snapshot(&self, id: MediaElementId) -> Option<MediaElementSnapshot>;

    /// Sets element source.
    fn set_src(&self, id: MediaElementId, src: String);

    /// Loads element.
    fn load(&self, id: MediaElementId);

    /// Plays element.
    fn play(&self, id: MediaElementId);

    /// Pauses element.
    fn pause(&self, id: MediaElementId);

    /// Seeks element.
    fn set_current_time(&self, id: MediaElementId, current_time: f64);

    /// Sets volume.
    fn set_volume(&self, id: MediaElementId, volume: f64);

    /// Sets muted.
    fn set_muted(&self, id: MediaElementId, muted: bool);

    /// Returns canPlayType result.
    fn can_play_type(&self, mime: &str) -> String;

    /// Creates MediaSource.
    fn create_media_source(&self) -> MediaSourceId;

    /// Returns source snapshot.
    fn media_source_snapshot(&self, id: MediaSourceId) -> Option<MediaSourceSnapshot>;

    /// Adds source buffer.
    fn add_source_buffer(&self, source: MediaSourceId, mime: String) -> SourceBufferId;

    /// Returns buffer snapshot.
    fn source_buffer_snapshot(&self, id: SourceBufferId) -> Option<SourceBufferSnapshot>;

    /// Appends bytes/chunk to SourceBuffer.
    fn append_buffer(&self, id: SourceBufferId, bytes: usize);

    /// Removes buffer range.
    fn remove_buffer(&self, id: SourceBufferId, start: f64, end: f64);

    /// Aborts SourceBuffer update.
    fn abort_buffer(&self, id: SourceBufferId);

    /// Ends media source stream.
    fn end_of_stream(&self, source: MediaSourceId);

    /// Creates object URL for MediaSource.
    fn create_object_url(&self, source: MediaSourceId) -> String;

    /// Revokes object URL.
    fn revoke_object_url(&self, url: &str) -> bool;

    /// Resolves object URL to MediaSource.
    fn media_source_for_object_url(&self, url: &str) -> Option<MediaSourceId>;

    /// Attaches MediaSource to media element.
    fn attach_media_source(&self, element: MediaElementId, source: MediaSourceId);

    /// Adds media event listener.
    fn add_event_listener(&self, element: MediaElementId, event_type: &str, callback: JsValue);

    /// Returns media event listeners.
    fn event_listeners(&self, element: MediaElementId, event_type: &str) -> Vec<JsValue>;

    /// Records event.
    fn record_event(&self, record: MediaEventRecord);

    /// Returns metrics.
    fn metrics(&self) -> MediaMetrics;

    /// Returns event records.
    fn events(&self) -> Vec<MediaEventRecord>;

    /// Returns segment records.
    fn segments(&self) -> Vec<MediaSegmentRecord>;
}

/// Installs MediaSource/video/audio globals and, when a DOM host is supplied,
/// patches `document.createElement` so `video` and `audio` return media-aware elements.
pub fn install_media_globals(
    vm: &mut Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    media: SharedMediaHost,
    dom: Option<SharedDomHost>,
) {
    vm.define_global(
        "MediaSource",
        create_media_source_constructor(media.clone()),
    );
    vm.define_global(
        "HTMLVideoElement",
        create_media_element_constructor(
            media.clone(),
            event_loop.clone(),
            dom.clone(),
            MediaElementKind::Video,
        ),
    );
    vm.define_global(
        "HTMLAudioElement",
        create_media_element_constructor(
            media.clone(),
            event_loop.clone(),
            dom.clone(),
            MediaElementKind::Audio,
        ),
    );

    let url_global = vm.get_name("URL");
    if !matches!(url_global, JsValue::Undefined | JsValue::Null) {
        url_global.set_property("createObjectURL", create_object_url_function(media.clone()));
        url_global.set_property("revokeObjectURL", revoke_object_url_function(media.clone()));
    } else {
        let url_object = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
        url_object.set_property("createObjectURL", create_object_url_function(media.clone()));
        url_object.set_property("revokeObjectURL", revoke_object_url_function(media.clone()));
        vm.define_global("URL", url_object);
    }

    if let Some(dom) = dom {
        let document = vm.get_name("document");
        if !matches!(document, JsValue::Undefined | JsValue::Null) {
            document.set_property(
                "createElement",
                create_media_aware_create_element(dom, media, event_loop),
            );
        }
    }
}

fn create_media_aware_create_element(
    dom: SharedDomHost,
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "document.createElement".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let tag = args
                .first()
                .map_or_else(|| "div".to_owned(), JsValue::to_js_string);
            let tag = tag.to_ascii_lowercase();

            match tag.as_str() {
                "video" => {
                    let node = dom.create_element("video");
                    let id = media.bind_dom_node(MediaElementKind::Video, node);
                    Ok(create_media_element_object(
                        media.clone(),
                        event_loop.clone(),
                        Some(dom.clone()),
                        Some(node),
                        id,
                    ))
                }
                "audio" => {
                    let node = dom.create_element("audio");
                    let id = media.bind_dom_node(MediaElementKind::Audio, node);
                    Ok(create_media_element_object(
                        media.clone(),
                        event_loop.clone(),
                        Some(dom.clone()),
                        Some(node),
                        id,
                    ))
                }
                _ => {
                    let node = dom.create_element(&tag);
                    Ok(create_element_object(dom.clone(), event_loop.clone(), node))
                }
            }
        }),
    })
}

fn create_media_element_constructor(
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    dom: Option<SharedDomHost>,
    kind: MediaElementKind,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: match kind {
            MediaElementKind::Video => "HTMLVideoElement",
            MediaElementKind::Audio => "HTMLAudioElement",
        }
        .to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let (node, id) = if let Some(dom) = &dom {
                let node = dom.create_element(kind.tag_name());
                (Some(node), media.bind_dom_node(kind, node))
            } else {
                (None, media.create_media_element(kind))
            };

            Ok(create_media_element_object(
                media.clone(),
                event_loop.clone(),
                dom.clone(),
                node,
                id,
            ))
        }),
    })
}

fn create_media_element_object(
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    dom: Option<SharedDomHost>,
    dom_node: Option<DomNodeRef>,
    element: MediaElementId,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(MediaElementHost {
            media,
            event_loop,
            dom,
            dom_node,
            element,
        }),
        "[object HTMLMediaElement]",
    );
    object.set_property("__syljsMediaElementId", JsValue::Number(element.0 as f64));
    if let Some(node) = dom_node {
        object.set_property("__syljsDomNodeId", JsValue::Number(node.0 as f64));
    }
    object
}

#[derive(Clone)]
struct MediaElementHost {
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    dom: Option<SharedDomHost>,
    dom_node: Option<DomNodeRef>,
    element: MediaElementId,
}

impl JsHostObject for MediaElementHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        let snapshot = self.media.element_snapshot(self.element)?;

        match key {
            "tagName" | "nodeName" => Some(JsValue::String(
                snapshot.kind.tag_name().to_ascii_uppercase(),
            )),
            "localName" => Some(JsValue::String(snapshot.kind.tag_name().to_owned())),
            "style" => self
                .dom
                .as_ref()
                .zip(self.dom_node)
                .map(|(dom, node)| create_inline_style_object(dom.cssom_host(), node)),
            "id" => self.dom_get_attribute("id"),
            "className" => self.dom_get_attribute("class"),
            "textContent" | "innerText" | "innerHTML" => Some(JsValue::String(
                self.dom
                    .as_ref()
                    .zip(self.dom_node)
                    .map_or_else(String::new, |(dom, node)| dom.text_content(node)),
            )),
            "src" | "currentSrc" => Some(JsValue::String(snapshot.src)),
            "paused" => Some(JsValue::Boolean(snapshot.paused)),
            "muted" => Some(JsValue::Boolean(snapshot.muted)),
            "volume" => Some(JsValue::Number(snapshot.volume)),
            "currentTime" => Some(JsValue::Number(snapshot.current_time)),
            "duration" => Some(JsValue::Number(snapshot.duration)),
            "readyState" => Some(JsValue::Number(snapshot.ready_state as u8 as f64)),
            "networkState" => Some(JsValue::Number(snapshot.network_state as u8 as f64)),
            "buffered" => Some(create_time_ranges_object(snapshot.buffered)),
            "videoWidth" => Some(JsValue::Number(snapshot.video_width as f64)),
            "videoHeight" => Some(JsValue::Number(snapshot.video_height as f64)),
            "HAVE_NOTHING" => Some(JsValue::Number(MediaReadyState::HaveNothing as u8 as f64)),
            "HAVE_METADATA" => Some(JsValue::Number(MediaReadyState::HaveMetadata as u8 as f64)),
            "HAVE_CURRENT_DATA" => Some(JsValue::Number(
                MediaReadyState::HaveCurrentData as u8 as f64,
            )),
            "HAVE_FUTURE_DATA" => Some(JsValue::Number(
                MediaReadyState::HaveFutureData as u8 as f64,
            )),
            "HAVE_ENOUGH_DATA" => Some(JsValue::Number(
                MediaReadyState::HaveEnoughData as u8 as f64,
            )),
            "NETWORK_EMPTY" => Some(JsValue::Number(MediaNetworkState::Empty as u8 as f64)),
            "NETWORK_IDLE" => Some(JsValue::Number(MediaNetworkState::Idle as u8 as f64)),
            "NETWORK_LOADING" => Some(JsValue::Number(MediaNetworkState::Loading as u8 as f64)),
            "NETWORK_NO_SOURCE" => Some(JsValue::Number(MediaNetworkState::NoSource as u8 as f64)),
            "play" => Some(media_play(
                self.media.clone(),
                self.event_loop.clone(),
                self.element,
            )),
            "pause" => Some(media_pause(
                self.media.clone(),
                self.event_loop.clone(),
                self.element,
            )),
            "load" => Some(media_load(
                self.media.clone(),
                self.event_loop.clone(),
                self.element,
            )),
            "canPlayType" => Some(media_can_play_type(self.media.clone())),
            "addEventListener" => Some(media_add_event_listener(self.media.clone(), self.element)),
            "dispatchEvent" => Some(media_dispatch_event(
                self.media.clone(),
                self.event_loop.clone(),
                self.element,
            )),
            "setAttribute" => self
                .dom_node
                .map(|node| dom_set_attribute(self.dom.clone(), node)),
            "getAttribute" => self
                .dom_node
                .map(|node| dom_get_attribute(self.dom.clone(), node)),
            "removeAttribute" => self
                .dom_node
                .map(|node| dom_remove_attribute(self.dom.clone(), node)),
            "appendChild" => self
                .dom_node
                .map(|node| dom_append_child(self.dom.clone(), node)),
            "remove" => self.dom_node.map(|node| dom_remove(self.dom.clone(), node)),
            "focus" | "blur" => Some(native_noop("HTMLMediaElement.focusOrBlur")),
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        match key {
            "src" | "currentSrc" => {
                let src = value.to_js_string();
                self.media.set_src(self.element, src.clone());
                if self.media.media_source_for_object_url(&src).is_some() {
                    queue_media_event(
                        self.media.clone(),
                        self.event_loop.clone(),
                        self.element,
                        "loadedmetadata",
                    );
                }
                self.dom_set_attribute("src", src);
                true
            }
            "currentTime" => {
                self.media
                    .set_current_time(self.element, value.to_number().max(0.0));
                queue_media_event(
                    self.media.clone(),
                    self.event_loop.clone(),
                    self.element,
                    "timeupdate",
                );
                true
            }
            "volume" => {
                self.media
                    .set_volume(self.element, value.to_number().clamp(0.0, 1.0));
                true
            }
            "muted" => {
                self.media.set_muted(self.element, value.is_truthy());
                true
            }
            "id" => self.dom_set_attribute("id", value.to_js_string()),
            "className" => self.dom_set_attribute("class", value.to_js_string()),
            "controls" | "autoplay" | "loop" => self.dom_set_attribute(key, value.to_js_string()),
            "textContent" | "innerText" | "innerHTML" => {
                if let Some((dom, node)) = self.dom.as_ref().zip(self.dom_node) {
                    dom.set_text_content(node, value.to_js_string())
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

impl MediaElementHost {
    fn dom_get_attribute(&self, name: &str) -> Option<JsValue> {
        self.dom
            .as_ref()
            .zip(self.dom_node)
            .map(|(dom, node)| JsValue::String(dom.get_attribute(node, name).unwrap_or_default()))
    }

    fn dom_set_attribute(&self, name: &str, value: String) -> bool {
        self.dom
            .as_ref()
            .zip(self.dom_node)
            .is_some_and(|(dom, node)| dom.set_attribute(node, name, value))
    }
}

fn media_play(
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    element: MediaElementId,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLMediaElement.play".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            media.play(element);
            queue_media_event(media.clone(), event_loop.clone(), element, "play");
            queue_media_event(media.clone(), event_loop.clone(), element, "playing");
            Ok(create_resolved_promise_value(
                event_loop.clone(),
                JsValue::Undefined,
            ))
        }),
    })
}

fn media_pause(
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    element: MediaElementId,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLMediaElement.pause".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            media.pause(element);
            queue_media_event(media.clone(), event_loop.clone(), element, "pause");
            Ok(JsValue::Undefined)
        }),
    })
}

fn media_load(
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    element: MediaElementId,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLMediaElement.load".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            media.load(element);
            queue_media_event(media.clone(), event_loop.clone(), element, "loadstart");
            queue_media_event(media.clone(), event_loop.clone(), element, "loadedmetadata");
            Ok(JsValue::Undefined)
        }),
    })
}

fn media_can_play_type(media: SharedMediaHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLMediaElement.canPlayType".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let mime = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(JsValue::String(media.can_play_type(&mime)))
        }),
    })
}

fn media_add_event_listener(media: SharedMediaHost, element: MediaElementId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLMediaElement.addEventListener".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let event_type = args.first().map_or_else(String::new, JsValue::to_js_string);
            let Some(callback) = args.get(1).cloned() else {
                return Ok(JsValue::Undefined);
            };

            if callback.as_function().is_none() {
                return Err(JsRuntimeError::new("media event listener is not callable"));
            }

            media.add_event_listener(element, &event_type, callback);
            Ok(JsValue::Undefined)
        }),
    })
}

fn media_dispatch_event(
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    element: MediaElementId,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLMediaElement.dispatchEvent".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let event = args
                .first()
                .cloned()
                .unwrap_or_else(|| create_event_object("event"));
            let event_type = event.get_property("type").to_js_string();
            queue_media_event(media.clone(), event_loop.clone(), element, &event_type);
            Ok(JsValue::Boolean(true))
        }),
    })
}

fn queue_media_event(
    media: SharedMediaHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    element: MediaElementId,
    event_type: &str,
) {
    media.record_event(MediaEventRecord {
        element: Some(element),
        source: None,
        buffer: None,
        event_type: event_type.to_owned(),
    });

    let event = create_event_object(event_type);
    for callback in media.event_listeners(element, event_type) {
        event_loop
            .borrow_mut()
            .queue_task(callback, vec![event.clone()]);
    }
}

fn create_event_object(event_type: &str) -> JsValue {
    let object = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
    object.set_property("type", JsValue::String(event_type.to_owned()));
    object
}

fn dom_set_attribute(dom: Option<SharedDomHost>, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.setAttribute".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            if let Some(dom) = &dom {
                let name = args.first().map_or_else(String::new, JsValue::to_js_string);
                let value = args.get(1).map_or_else(String::new, JsValue::to_js_string);
                dom.set_attribute(node, &name, value);
            }
            Ok(JsValue::Undefined)
        }),
    })
}

fn dom_get_attribute(dom: Option<SharedDomHost>, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.getAttribute".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let Some(dom) = &dom else {
                return Ok(JsValue::Null);
            };
            let name = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(dom
                .get_attribute(node, &name)
                .map_or(JsValue::Null, JsValue::String))
        }),
    })
}

fn dom_remove_attribute(dom: Option<SharedDomHost>, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.removeAttribute".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            if let Some(dom) = &dom {
                let name = args.first().map_or_else(String::new, JsValue::to_js_string);
                dom.remove_attribute(node, &name);
            }
            Ok(JsValue::Undefined)
        }),
    })
}

fn dom_append_child(dom: Option<SharedDomHost>, parent: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.appendChild".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let Some(dom) = &dom else {
                return Ok(JsValue::Null);
            };
            let Some(child) = args.first().and_then(JsValue::dom_node_id).map(DomNodeRef) else {
                return Ok(JsValue::Null);
            };
            dom.append_child(parent, child);
            Ok(args.first().cloned().unwrap_or(JsValue::Null))
        }),
    })
}

fn dom_remove(dom: Option<SharedDomHost>, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "Element.remove".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            if let Some(dom) = &dom {
                dom.remove_node(node);
            }
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_media_source_constructor(media: SharedMediaHost) -> JsValue {
    let constructor = JsValue::function(JsFunction::Native {
        name: "MediaSource".to_owned(),
        function: Rc::new({
            let media = media.clone();
            move |_vm, _this, _args| {
                let id = media.create_media_source();
                Ok(create_media_source_object(media.clone(), id))
            }
        }),
    });

    constructor.set_property("isTypeSupported", media_source_is_type_supported(media));

    constructor
}

fn media_source_is_type_supported(media: SharedMediaHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "MediaSource.isTypeSupported".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let mime = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(JsValue::Boolean(!media.can_play_type(&mime).is_empty()))
        }),
    })
}

fn create_media_source_object(media: SharedMediaHost, source: MediaSourceId) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(MediaSourceHost { media, source }),
        "[object MediaSource]",
    );
    object.set_property("__syljsMediaSourceId", JsValue::Number(source.0 as f64));
    object
}

#[derive(Clone)]
struct MediaSourceHost {
    media: SharedMediaHost,
    source: MediaSourceId,
}

impl JsHostObject for MediaSourceHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        let snapshot = self.media.media_source_snapshot(self.source)?;

        match key {
            "readyState" => Some(JsValue::String(snapshot.ready_state.as_str().to_owned())),
            "duration" => Some(JsValue::Number(snapshot.duration)),
            "sourceBuffers" | "activeSourceBuffers" => Some(create_source_buffer_list_object(
                self.media.clone(),
                snapshot.source_buffers,
            )),
            "addSourceBuffer" => Some(media_source_add_source_buffer(
                self.media.clone(),
                self.source,
            )),
            "endOfStream" => Some(media_source_end_of_stream(self.media.clone(), self.source)),
            "addEventListener" => Some(native_noop("MediaSource.addEventListener")),
            "removeEventListener" => Some(native_noop("MediaSource.removeEventListener")),
            "dispatchEvent" => Some(native_return_bool("MediaSource.dispatchEvent", true)),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn media_source_add_source_buffer(media: SharedMediaHost, source: MediaSourceId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "MediaSource.addSourceBuffer".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let mime = args.first().map_or_else(String::new, JsValue::to_js_string);
            if media.can_play_type(&mime).is_empty() {
                return Err(JsRuntimeError::new(format!(
                    "unsupported SourceBuffer MIME type `{mime}`"
                )));
            }
            let id = media.add_source_buffer(source, mime);
            Ok(create_source_buffer_object(media.clone(), id))
        }),
    })
}

fn media_source_end_of_stream(media: SharedMediaHost, source: MediaSourceId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "MediaSource.endOfStream".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            media.end_of_stream(source);
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_source_buffer_list_object(
    media: SharedMediaHost,
    buffers: Vec<SourceBufferId>,
) -> JsValue {
    JsValue::host_object(
        Rc::new(SourceBufferListHost { media, buffers }),
        "[object SourceBufferList]",
    )
}

#[derive(Clone)]
struct SourceBufferListHost {
    media: SharedMediaHost,
    buffers: Vec<SourceBufferId>,
}

impl JsHostObject for SourceBufferListHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        if key == "length" {
            return Some(JsValue::Number(self.buffers.len() as f64));
        }

        if key == "item" {
            return Some(source_buffer_list_item(
                self.media.clone(),
                self.buffers.clone(),
            ));
        }

        if let Ok(index) = key.parse::<usize>() {
            return self
                .buffers
                .get(index)
                .copied()
                .map(|id| create_source_buffer_object(self.media.clone(), id));
        }

        None
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn source_buffer_list_item(media: SharedMediaHost, buffers: Vec<SourceBufferId>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "SourceBufferList.item".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let index = args
                .first()
                .map_or(0, |value| value.to_number().max(0.0) as usize);
            Ok(buffers.get(index).copied().map_or(JsValue::Null, |id| {
                create_source_buffer_object(media.clone(), id)
            }))
        }),
    })
}

fn create_source_buffer_object(media: SharedMediaHost, buffer: SourceBufferId) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(SourceBufferHost { media, buffer }),
        "[object SourceBuffer]",
    );
    object.set_property("__syljsSourceBufferId", JsValue::Number(buffer.0 as f64));
    object
}

#[derive(Clone)]
struct SourceBufferHost {
    media: SharedMediaHost,
    buffer: SourceBufferId,
}

impl JsHostObject for SourceBufferHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        let snapshot = self.media.source_buffer_snapshot(self.buffer)?;

        match key {
            "updating" => Some(JsValue::Boolean(
                snapshot.state == SourceBufferState::Updating,
            )),
            "mode" => Some(JsValue::String("segments".to_owned())),
            "timestampOffset" => Some(JsValue::Number(snapshot.timestamp_offset)),
            "buffered" => Some(create_time_ranges_object(snapshot.buffered)),
            "appendBuffer" => Some(source_buffer_append(self.media.clone(), self.buffer)),
            "remove" => Some(source_buffer_remove(self.media.clone(), self.buffer)),
            "abort" => Some(source_buffer_abort(self.media.clone(), self.buffer)),
            "addEventListener" => Some(native_noop("SourceBuffer.addEventListener")),
            "removeEventListener" => Some(native_noop("SourceBuffer.removeEventListener")),
            "dispatchEvent" => Some(native_return_bool("SourceBuffer.dispatchEvent", true)),
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        match key {
            "timestampOffset" => {
                let _ = value;
                true
            }
            "mode" => true,
            _ => false,
        }
    }
}

fn source_buffer_append(media: SharedMediaHost, buffer: SourceBufferId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "SourceBuffer.appendBuffer".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let bytes = args.first().map_or(0, estimate_buffer_len);
            media.append_buffer(buffer, bytes);
            Ok(JsValue::Undefined)
        }),
    })
}

fn source_buffer_remove(media: SharedMediaHost, buffer: SourceBufferId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "SourceBuffer.remove".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let start = args.first().map_or(0.0, JsValue::to_number).max(0.0);
            let end = args.get(1).map_or(start, JsValue::to_number).max(start);
            media.remove_buffer(buffer, start, end);
            Ok(JsValue::Undefined)
        }),
    })
}

fn source_buffer_abort(media: SharedMediaHost, buffer: SourceBufferId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "SourceBuffer.abort".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            media.abort_buffer(buffer);
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_object_url_function(media: SharedMediaHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "URL.createObjectURL".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let Some(source) = args.first().and_then(media_source_id_from_value) else {
                return Err(JsRuntimeError::new(
                    "URL.createObjectURL currently supports MediaSource objects",
                ));
            };

            Ok(JsValue::String(media.create_object_url(source)))
        }),
    })
}

fn revoke_object_url_function(media: SharedMediaHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "URL.revokeObjectURL".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let url = args.first().map_or_else(String::new, JsValue::to_js_string);
            Ok(JsValue::Boolean(media.revoke_object_url(&url)))
        }),
    })
}

fn media_source_id_from_value(value: &JsValue) -> Option<MediaSourceId> {
    let number = value.get_property("__syljsMediaSourceId").to_number();
    (number.is_finite() && number > 0.0).then_some(MediaSourceId(number as u64))
}

fn create_time_ranges_object(ranges: Vec<BufferedRange>) -> JsValue {
    JsValue::host_object(Rc::new(TimeRangesHost { ranges }), "[object TimeRanges]")
}

#[derive(Clone)]
struct TimeRangesHost {
    ranges: Vec<BufferedRange>,
}

impl JsHostObject for TimeRangesHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "length" => Some(JsValue::Number(self.ranges.len() as f64)),
            "start" => Some(time_ranges_start(self.ranges.clone())),
            "end" => Some(time_ranges_end(self.ranges.clone())),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn time_ranges_start(ranges: Vec<BufferedRange>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "TimeRanges.start".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let index = args
                .first()
                .map_or(0, |value| value.to_number().max(0.0) as usize);
            Ok(JsValue::Number(
                ranges.get(index).map_or(0.0, |range| range.start),
            ))
        }),
    })
}

fn time_ranges_end(ranges: Vec<BufferedRange>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "TimeRanges.end".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let index = args
                .first()
                .map_or(0, |value| value.to_number().max(0.0) as usize);
            Ok(JsValue::Number(
                ranges.get(index).map_or(0.0, |range| range.end),
            ))
        }),
    })
}

fn native_noop(name: &'static str) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Undefined)),
    })
}

fn native_return_bool(name: &'static str, value: bool) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Boolean(value))),
    })
}

fn estimate_buffer_len(value: &JsValue) -> usize {
    match value {
        JsValue::String(value) => value.len(),
        JsValue::Number(value) => (*value).max(0.0) as usize,
        JsValue::Object(_) => value
            .get_property("byteLength")
            .to_number()
            .max(value.get_property("length").to_number())
            .max(0.0) as usize,
        JsValue::Boolean(_) | JsValue::Null | JsValue::Undefined => 0,
    }
}

/// Deterministic research media host.
#[derive(Debug)]
pub struct ResearchMediaHost {
    inner: RefCell<ResearchMediaInner>,
}

#[derive(Debug)]
struct ResearchMediaInner {
    next_element: u64,
    next_source: u64,
    next_buffer: u64,
    elements: BTreeMap<MediaElementId, MediaElementState>,
    dom_bindings: BTreeMap<DomNodeRef, MediaElementId>,
    sources: BTreeMap<MediaSourceId, MediaSourceStateData>,
    buffers: BTreeMap<SourceBufferId, SourceBufferStateData>,
    object_urls: BTreeMap<String, MediaSourceId>,
    listeners: BTreeMap<(MediaElementId, String), Vec<JsValue>>,
    events: Vec<MediaEventRecord>,
    segments: Vec<MediaSegmentRecord>,
    time_updates: VecDeque<MediaTimeUpdate>,
    metrics: MediaMetrics,
}

#[derive(Debug, Clone)]
struct MediaElementState {
    id: MediaElementId,
    kind: MediaElementKind,
    dom_node: Option<DomNodeRef>,
    src: String,
    media_source: Option<MediaSourceId>,
    current_time: f64,
    duration: f64,
    paused: bool,
    muted: bool,
    volume: f64,
    ready_state: MediaReadyState,
    network_state: MediaNetworkState,
    buffered: Vec<BufferedRange>,
    video_width: u32,
    video_height: u32,
}

#[derive(Debug, Clone)]
struct MediaSourceStateData {
    id: MediaSourceId,
    ready_state: MediaSourceState,
    duration: f64,
    source_buffers: Vec<SourceBufferId>,
}

#[derive(Debug, Clone)]
struct SourceBufferStateData {
    id: SourceBufferId,
    source: MediaSourceId,
    mime_type: String,
    state: SourceBufferState,
    buffered: Vec<BufferedRange>,
    bytes_appended: usize,
    segments: usize,
    timestamp_offset: f64,
}

impl Default for ResearchMediaHost {
    fn default() -> Self {
        Self {
            inner: RefCell::new(ResearchMediaInner {
                next_element: 1,
                next_source: 1,
                next_buffer: 1,
                elements: BTreeMap::new(),
                dom_bindings: BTreeMap::new(),
                sources: BTreeMap::new(),
                buffers: BTreeMap::new(),
                object_urls: BTreeMap::new(),
                listeners: BTreeMap::new(),
                events: Vec::new(),
                segments: Vec::new(),
                time_updates: VecDeque::new(),
                metrics: MediaMetrics::default(),
            }),
        }
    }
}

impl ResearchMediaHost {
    fn alloc_element(
        inner: &mut ResearchMediaInner,
        kind: MediaElementKind,
        dom_node: Option<DomNodeRef>,
    ) -> MediaElementId {
        let id = MediaElementId(inner.next_element);
        inner.next_element = inner.next_element.saturating_add(1);
        inner.elements.insert(
            id,
            MediaElementState {
                id,
                kind,
                dom_node,
                src: String::new(),
                media_source: None,
                current_time: 0.0,
                duration: 0.0,
                paused: true,
                muted: false,
                volume: 1.0,
                ready_state: MediaReadyState::HaveNothing,
                network_state: MediaNetworkState::Empty,
                buffered: Vec::new(),
                video_width: if kind == MediaElementKind::Video {
                    640
                } else {
                    0
                },
                video_height: if kind == MediaElementKind::Video {
                    360
                } else {
                    0
                },
            },
        );

        if let Some(node) = dom_node {
            inner.dom_bindings.insert(node, id);
        }

        inner.metrics.media_elements_created =
            inner.metrics.media_elements_created.saturating_add(1);
        id
    }

    fn refresh_element_buffers(inner: &mut ResearchMediaInner, source: MediaSourceId) {
        let ranges = inner
            .sources
            .get(&source)
            .map(|source| {
                source
                    .source_buffers
                    .iter()
                    .filter_map(|buffer| inner.buffers.get(buffer))
                    .flat_map(|buffer| buffer.buffered.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for element in inner.elements.values_mut() {
            if element.media_source == Some(source) {
                element.buffered = ranges.clone();
                element.duration = ranges.iter().map(|range| range.end).fold(0.0, f64::max);
                if !ranges.is_empty() {
                    element.ready_state = MediaReadyState::HaveEnoughData;
                    element.network_state = MediaNetworkState::Idle;
                }
            }
        }
    }
}

impl MediaHost for ResearchMediaHost {
    fn create_media_element(&self, kind: MediaElementKind) -> MediaElementId {
        Self::alloc_element(&mut self.inner.borrow_mut(), kind, None)
    }

    fn bind_dom_node(&self, kind: MediaElementKind, node: DomNodeRef) -> MediaElementId {
        let mut inner = self.inner.borrow_mut();

        if let Some(id) = inner.dom_bindings.get(&node).copied() {
            return id;
        }

        Self::alloc_element(&mut inner, kind, Some(node))
    }

    fn element_for_dom_node(&self, node: DomNodeRef) -> Option<MediaElementId> {
        self.inner.borrow().dom_bindings.get(&node).copied()
    }

    fn element_snapshot(&self, id: MediaElementId) -> Option<MediaElementSnapshot> {
        self.inner
            .borrow()
            .elements
            .get(&id)
            .map(|element| MediaElementSnapshot {
                id: element.id,
                kind: element.kind,
                dom_node: element.dom_node,
                src: element.src.clone(),
                media_source: element.media_source,
                current_time: element.current_time,
                duration: element.duration,
                paused: element.paused,
                muted: element.muted,
                volume: element.volume,
                ready_state: element.ready_state,
                network_state: element.network_state,
                buffered: element.buffered.clone(),
                video_width: element.video_width,
                video_height: element.video_height,
            })
    }

    fn set_src(&self, id: MediaElementId, src: String) {
        let mut inner = self.inner.borrow_mut();
        let source = inner.object_urls.get(&src).copied();

        if let Some(element) = inner.elements.get_mut(&id) {
            element.src = src;
            element.media_source = source;
            element.network_state = MediaNetworkState::Loading;
            element.ready_state = if source.is_some() {
                MediaReadyState::HaveMetadata
            } else {
                MediaReadyState::HaveCurrentData
            };
            element.duration = if source.is_some() { 0.0 } else { 60.0 };
            if source.is_none() {
                element.buffered = vec![BufferedRange {
                    start: 0.0,
                    end: 60.0,
                }];
            }
        }

        inner.metrics.src_assignments = inner.metrics.src_assignments.saturating_add(1);

        if let Some(source) = source {
            drop(inner);
            self.attach_media_source(id, source);
        }
    }

    fn load(&self, id: MediaElementId) {
        let mut inner = self.inner.borrow_mut();
        if let Some(element) = inner.elements.get_mut(&id) {
            element.network_state = MediaNetworkState::Loading;
            element.ready_state = if element.src.is_empty() {
                MediaReadyState::HaveNothing
            } else {
                MediaReadyState::HaveMetadata
            };
        }
        inner.metrics.load_calls = inner.metrics.load_calls.saturating_add(1);
    }

    fn play(&self, id: MediaElementId) {
        let mut inner = self.inner.borrow_mut();
        if let Some(element) = inner.elements.get_mut(&id) {
            element.paused = false;
            element.ready_state = if element.ready_state == MediaReadyState::HaveNothing {
                MediaReadyState::HaveCurrentData
            } else {
                element.ready_state
            };
            element.network_state = MediaNetworkState::Idle;
        }
        inner.metrics.play_calls = inner.metrics.play_calls.saturating_add(1);
    }

    fn pause(&self, id: MediaElementId) {
        let mut inner = self.inner.borrow_mut();
        if let Some(element) = inner.elements.get_mut(&id) {
            element.paused = true;
        }
        inner.metrics.pause_calls = inner.metrics.pause_calls.saturating_add(1);
    }

    fn set_current_time(&self, id: MediaElementId, current_time: f64) {
        let mut inner = self.inner.borrow_mut();
        if let Some(element) = inner.elements.get_mut(&id) {
            element.current_time = current_time;
            inner.time_updates.push_back(MediaTimeUpdate {
                element: id,
                current_time,
            });
        }
        inner.metrics.seeks = inner.metrics.seeks.saturating_add(1);
    }

    fn set_volume(&self, id: MediaElementId, volume: f64) {
        if let Some(element) = self.inner.borrow_mut().elements.get_mut(&id) {
            element.volume = volume.clamp(0.0, 1.0);
        }
    }

    fn set_muted(&self, id: MediaElementId, muted: bool) {
        if let Some(element) = self.inner.borrow_mut().elements.get_mut(&id) {
            element.muted = muted;
        }
    }

    fn can_play_type(&self, mime: &str) -> String {
        {
            let mut inner = self.inner.borrow_mut();
            inner.metrics.can_play_type_checks =
                inner.metrics.can_play_type_checks.saturating_add(1);
        }

        let mime = mime.to_ascii_lowercase();

        if mime.contains("video/mp4")
            || mime.contains("audio/mp4")
            || mime.contains("video/webm")
            || mime.contains("audio/webm")
            || mime.contains("avc1")
            || mime.contains("mp4a")
            || mime.contains("vp9")
            || mime.contains("opus")
        {
            "probably".to_owned()
        } else if mime.contains("video/") || mime.contains("audio/") {
            "maybe".to_owned()
        } else {
            String::new()
        }
    }

    fn create_media_source(&self) -> MediaSourceId {
        let mut inner = self.inner.borrow_mut();
        let id = MediaSourceId(inner.next_source);
        inner.next_source = inner.next_source.saturating_add(1);
        inner.sources.insert(
            id,
            MediaSourceStateData {
                id,
                ready_state: MediaSourceState::Open,
                duration: 0.0,
                source_buffers: Vec::new(),
            },
        );
        inner.metrics.media_sources_created = inner.metrics.media_sources_created.saturating_add(1);
        id
    }

    fn media_source_snapshot(&self, id: MediaSourceId) -> Option<MediaSourceSnapshot> {
        self.inner
            .borrow()
            .sources
            .get(&id)
            .map(|source| MediaSourceSnapshot {
                id: source.id,
                ready_state: source.ready_state,
                duration: source.duration,
                source_buffers: source.source_buffers.clone(),
            })
    }

    fn add_source_buffer(&self, source: MediaSourceId, mime: String) -> SourceBufferId {
        let mut inner = self.inner.borrow_mut();
        let id = SourceBufferId(inner.next_buffer);
        inner.next_buffer = inner.next_buffer.saturating_add(1);

        inner.buffers.insert(
            id,
            SourceBufferStateData {
                id,
                source,
                mime_type: mime,
                state: SourceBufferState::Idle,
                buffered: Vec::new(),
                bytes_appended: 0,
                segments: 0,
                timestamp_offset: 0.0,
            },
        );

        if let Some(source) = inner.sources.get_mut(&source) {
            source.source_buffers.push(id);
            source.ready_state = MediaSourceState::Open;
        }

        inner.metrics.source_buffers_created =
            inner.metrics.source_buffers_created.saturating_add(1);
        id
    }

    fn source_buffer_snapshot(&self, id: SourceBufferId) -> Option<SourceBufferSnapshot> {
        self.inner
            .borrow()
            .buffers
            .get(&id)
            .map(|buffer| SourceBufferSnapshot {
                id: buffer.id,
                source: buffer.source,
                mime_type: buffer.mime_type.clone(),
                state: buffer.state,
                buffered: buffer.buffered.clone(),
                bytes_appended: buffer.bytes_appended,
                segments: buffer.segments,
                timestamp_offset: buffer.timestamp_offset,
            })
    }

    fn append_buffer(&self, id: SourceBufferId, bytes: usize) {
        let mut inner = self.inner.borrow_mut();
        let mut source_to_refresh = None;
        let mut segment_record = None;

        if let Some(buffer) = inner.buffers.get_mut(&id) {
            buffer.state = SourceBufferState::Updating;
            let start = buffer.buffered.last().map_or(0.0, |range| range.end);
            let duration = (bytes.max(1) as f64 / 1024.0).max(0.25);
            let end = start + duration;

            buffer.buffered.push(BufferedRange { start, end });
            buffer.bytes_appended = buffer.bytes_appended.saturating_add(bytes);
            buffer.segments = buffer.segments.saturating_add(1);
            buffer.state = SourceBufferState::Idle;
            source_to_refresh = Some(buffer.source);
            segment_record = Some(MediaSegmentRecord {
                buffer: id,
                bytes,
                start,
                end,
            });
        }

        if let Some(record) = segment_record {
            inner.segments.push(record);
        }

        inner.metrics.buffer_appends = inner.metrics.buffer_appends.saturating_add(1);

        if let Some(source) = source_to_refresh {
            let duration = inner
                .buffers
                .values()
                .filter(|buffer| buffer.source == source)
                .flat_map(|buffer| buffer.buffered.iter().map(|range| range.end))
                .fold(0.0, f64::max);

            if let Some(source_state) = inner.sources.get_mut(&source) {
                source_state.duration = duration;
            }
            Self::refresh_element_buffers(&mut inner, source);
        }
    }

    fn remove_buffer(&self, id: SourceBufferId, start: f64, end: f64) {
        let mut inner = self.inner.borrow_mut();
        let mut source_to_refresh = None;

        if let Some(buffer) = inner.buffers.get_mut(&id) {
            buffer
                .buffered
                .retain(|range| range.end <= start || range.start >= end);
            source_to_refresh = Some(buffer.source);
        }

        inner.metrics.buffer_removes = inner.metrics.buffer_removes.saturating_add(1);

        if let Some(source) = source_to_refresh {
            Self::refresh_element_buffers(&mut inner, source);
        }
    }

    fn abort_buffer(&self, id: SourceBufferId) {
        if let Some(buffer) = self.inner.borrow_mut().buffers.get_mut(&id) {
            buffer.state = SourceBufferState::Idle;
        }
    }

    fn end_of_stream(&self, source: MediaSourceId) {
        let mut inner = self.inner.borrow_mut();
        if let Some(source) = inner.sources.get_mut(&source) {
            source.ready_state = MediaSourceState::Ended;
        }
    }

    fn create_object_url(&self, source: MediaSourceId) -> String {
        let mut inner = self.inner.borrow_mut();
        let url = format!("sylphos-media-source:{}", source.0);
        inner.object_urls.insert(url.clone(), source);
        inner.metrics.object_urls_created = inner.metrics.object_urls_created.saturating_add(1);
        url
    }

    fn revoke_object_url(&self, url: &str) -> bool {
        let mut inner = self.inner.borrow_mut();
        let removed = inner.object_urls.remove(url).is_some();
        if removed {
            inner.metrics.object_urls_revoked = inner.metrics.object_urls_revoked.saturating_add(1);
        }
        removed
    }

    fn media_source_for_object_url(&self, url: &str) -> Option<MediaSourceId> {
        self.inner.borrow().object_urls.get(url).copied()
    }

    fn attach_media_source(&self, element: MediaElementId, source: MediaSourceId) {
        let mut inner = self.inner.borrow_mut();

        if let Some(element) = inner.elements.get_mut(&element) {
            element.media_source = Some(source);
            element.ready_state = MediaReadyState::HaveMetadata;
            element.network_state = MediaNetworkState::Loading;
        }

        if let Some(source) = inner.sources.get_mut(&source) {
            source.ready_state = MediaSourceState::Open;
        }

        inner.metrics.media_source_attachments =
            inner.metrics.media_source_attachments.saturating_add(1);
        Self::refresh_element_buffers(&mut inner, source);
    }

    fn add_event_listener(&self, element: MediaElementId, event_type: &str, callback: JsValue) {
        self.inner
            .borrow_mut()
            .listeners
            .entry((element, event_type.to_owned()))
            .or_default()
            .push(callback);
    }

    fn event_listeners(&self, element: MediaElementId, event_type: &str) -> Vec<JsValue> {
        self.inner
            .borrow()
            .listeners
            .get(&(element, event_type.to_owned()))
            .cloned()
            .unwrap_or_default()
    }

    fn record_event(&self, record: MediaEventRecord) {
        let mut inner = self.inner.borrow_mut();
        inner.events.push(record);
        inner.metrics.events = inner.metrics.events.saturating_add(1);
    }

    fn metrics(&self) -> MediaMetrics {
        self.inner.borrow().metrics.clone()
    }

    fn events(&self) -> Vec<MediaEventRecord> {
        self.inner.borrow().events.clone()
    }

    fn segments(&self) -> Vec<MediaSegmentRecord> {
        self.inner.borrow().segments.clone()
    }
}
