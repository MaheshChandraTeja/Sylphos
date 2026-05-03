#![allow(clippy::too_many_lines)]
#![doc = "Canvas 2D simulation and command recording for SylJS."]

use crate::{
    create_element_object, create_inline_style_object,
    dom::{DomNodeRef, SharedDomHost},
    event_loop::JsEventLoop,
    JsFunction, JsHostObject, JsValue, Vm,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Shared Canvas host pointer.
pub type SharedCanvasHost = Rc<dyn CanvasHost>;

/// Stable canvas element id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanvasElementId(pub u64);

/// Stable 2D context id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanvasContextId(pub u64);

/// Canvas image-data descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasImageData {
    /// Width in pixels.
    pub width: u32,

    /// Height in pixels.
    pub height: u32,

    /// Backing byte length.
    pub byte_length: usize,
}

/// Canvas text metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasTextMetrics {
    /// Measured width.
    pub width: f64,

    /// Approximate actual bounding-box ascent.
    pub actual_bounding_box_ascent: f64,

    /// Approximate actual bounding-box descent.
    pub actual_bounding_box_descent: f64,
}

/// Gradient color stop.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasGradientStop {
    /// Offset in `[0, 1]`.
    pub offset: f64,

    /// CSS color string.
    pub color: String,
}

/// Canvas command record.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasCommand {
    /// `save()`.
    Save { context: CanvasContextId },

    /// `restore()`.
    Restore { context: CanvasContextId },

    /// `beginPath()`.
    BeginPath { context: CanvasContextId },

    /// `closePath()`.
    ClosePath { context: CanvasContextId },

    /// `moveTo(x, y)`.
    MoveTo {
        context: CanvasContextId,
        x: f64,
        y: f64,
    },

    /// `lineTo(x, y)`.
    LineTo {
        context: CanvasContextId,
        x: f64,
        y: f64,
    },

    /// `rect(x, y, w, h)`.
    Rect {
        context: CanvasContextId,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },

    /// `arc(...)`.
    Arc {
        context: CanvasContextId,
        x: f64,
        y: f64,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },

    /// `fill()`.
    Fill {
        context: CanvasContextId,
        fill_style: String,
    },

    /// `stroke()`.
    Stroke {
        context: CanvasContextId,
        stroke_style: String,
        line_width: f64,
    },

    /// `clearRect(...)`.
    ClearRect {
        context: CanvasContextId,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },

    /// `fillRect(...)`.
    FillRect {
        context: CanvasContextId,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        fill_style: String,
    },

    /// `strokeRect(...)`.
    StrokeRect {
        context: CanvasContextId,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        stroke_style: String,
        line_width: f64,
    },

    /// `fillText(...)`.
    FillText {
        context: CanvasContextId,
        text: String,
        x: f64,
        y: f64,
        font: String,
        fill_style: String,
    },

    /// `strokeText(...)`.
    StrokeText {
        context: CanvasContextId,
        text: String,
        x: f64,
        y: f64,
        font: String,
        stroke_style: String,
    },

    /// `drawImage(...)`.
    DrawImage {
        context: CanvasContextId,
        source: String,
        x: f64,
        y: f64,
        width: Option<f64>,
        height: Option<f64>,
    },

    /// `putImageData(...)`.
    PutImageData {
        context: CanvasContextId,
        width: u32,
        height: u32,
        x: f64,
        y: f64,
    },

    /// `getImageData(...)`.
    GetImageData {
        context: CanvasContextId,
        x: f64,
        y: f64,
        width: u32,
        height: u32,
    },

    /// Transform command.
    Transform {
        context: CanvasContextId,
        kind: String,
        values: Vec<f64>,
    },
}

/// Canvas element snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasElementSnapshot {
    /// Canvas id.
    pub id: CanvasElementId,

    /// Optional DOM node.
    pub dom_node: Option<DomNodeRef>,

    /// Width.
    pub width: u32,

    /// Height.
    pub height: u32,

    /// Contexts.
    pub contexts: Vec<CanvasContextId>,
}

/// Canvas context snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasContextSnapshot {
    /// Context id.
    pub id: CanvasContextId,

    /// Parent canvas.
    pub canvas: CanvasElementId,

    /// Current state properties.
    pub state: BTreeMap<String, String>,

    /// Commands.
    pub commands: Vec<CanvasCommand>,
}

/// Canvas metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanvasMetrics {
    /// Canvas elements created.
    pub canvas_elements_created: u64,

    /// Contexts created.
    pub contexts_created: u64,

    /// getContext calls.
    pub get_context_calls: u64,

    /// State reads.
    pub state_reads: u64,

    /// State writes.
    pub state_writes: u64,

    /// Draw/path commands.
    pub commands_recorded: u64,

    /// Image-data allocations.
    pub image_data_allocations: u64,

    /// Text measurements.
    pub text_measurements: u64,

    /// Data URL exports.
    pub data_url_exports: u64,

    /// Context clears.
    pub command_clears: u64,
}

/// Canvas host abstraction.
pub trait CanvasHost {
    /// Creates a canvas.
    fn create_canvas(&self, dom_node: Option<DomNodeRef>) -> CanvasElementId;

    /// Binds DOM node to canvas.
    fn bind_dom_node(&self, node: DomNodeRef) -> CanvasElementId;

    /// Returns canvas id for DOM node.
    fn canvas_for_dom_node(&self, node: DomNodeRef) -> Option<CanvasElementId>;

    /// Canvas snapshot.
    fn canvas_snapshot(&self, canvas: CanvasElementId) -> Option<CanvasElementSnapshot>;

    /// Sets canvas dimensions.
    fn set_canvas_size(&self, canvas: CanvasElementId, width: u32, height: u32);

    /// Gets or creates 2D context.
    fn context_2d(&self, canvas: CanvasElementId) -> CanvasContextId;

    /// Context snapshot.
    fn context_snapshot(&self, context: CanvasContextId) -> Option<CanvasContextSnapshot>;

    /// Reads context state.
    fn state_get(&self, context: CanvasContextId, property: &str) -> String;

    /// Writes context state.
    fn state_set(&self, context: CanvasContextId, property: &str, value: String);

    /// Records command.
    fn record_command(&self, command: CanvasCommand);

    /// Clears context command list.
    fn clear_commands(&self, context: CanvasContextId);

    /// Measures text.
    fn measure_text(&self, context: CanvasContextId, text: &str) -> CanvasTextMetrics;

    /// Creates image data descriptor.
    fn create_image_data(&self, width: u32, height: u32) -> CanvasImageData;

    /// Exports data URL.
    fn to_data_url(&self, canvas: CanvasElementId, mime: &str) -> String;

    /// Metrics.
    fn metrics(&self) -> CanvasMetrics;

    /// All commands.
    fn commands(&self) -> Vec<CanvasCommand>;
}

/// Installs Canvas globals and patches `document.createElement("canvas")` when a DOM host exists.
pub fn install_canvas_globals(
    vm: &mut Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    canvas: SharedCanvasHost,
    dom: Option<SharedDomHost>,
) {
    vm.define_global(
        "HTMLCanvasElement",
        create_canvas_element_constructor(canvas.clone(), event_loop.clone(), dom.clone()),
    );
    vm.define_global("ImageData", create_image_data_constructor(canvas.clone()));
    vm.define_global(
        "__sylphosCanvasMetrics",
        create_canvas_metrics_function(canvas.clone()),
    );

    if let Some(dom) = dom {
        let document = vm.get_name("document");
        if !matches!(document, JsValue::Undefined | JsValue::Null) {
            document.set_property(
                "createElement",
                create_canvas_aware_create_element(dom, canvas, event_loop),
            );
        }
    }
}

fn create_canvas_aware_create_element(
    dom: SharedDomHost,
    canvas: SharedCanvasHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "document.createElement".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let tag = args
                .first()
                .map_or_else(|| "div".to_owned(), JsValue::to_js_string);
            let tag = tag.to_ascii_lowercase();

            if tag == "canvas" {
                let node = dom.create_element("canvas");
                let canvas_id = canvas.bind_dom_node(node);
                Ok(create_canvas_element_object(
                    canvas.clone(),
                    event_loop.clone(),
                    Some(dom.clone()),
                    Some(node),
                    canvas_id,
                ))
            } else {
                let node = dom.create_element(&tag);
                Ok(create_element_object(dom.clone(), event_loop.clone(), node))
            }
        }),
    })
}

fn create_canvas_element_constructor(
    canvas: SharedCanvasHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    dom: Option<SharedDomHost>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLCanvasElement".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let (node, id) = if let Some(dom) = &dom {
                let node = dom.create_element("canvas");
                (Some(node), canvas.bind_dom_node(node))
            } else {
                (None, canvas.create_canvas(None))
            };

            Ok(create_canvas_element_object(
                canvas.clone(),
                event_loop.clone(),
                dom.clone(),
                node,
                id,
            ))
        }),
    })
}

fn create_canvas_element_object(
    canvas: SharedCanvasHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    dom: Option<SharedDomHost>,
    dom_node: Option<DomNodeRef>,
    canvas_id: CanvasElementId,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(CanvasElementHost {
            canvas,
            event_loop,
            dom,
            dom_node,
            canvas_id,
        }),
        "[object HTMLCanvasElement]",
    );
    object.set_property(
        "__syljsCanvasElementId",
        JsValue::Number(canvas_id.0 as f64),
    );
    if let Some(node) = dom_node {
        object.set_property("__syljsDomNodeId", JsValue::Number(node.0 as f64));
    }
    object
}

#[derive(Clone)]
struct CanvasElementHost {
    canvas: SharedCanvasHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    dom: Option<SharedDomHost>,
    dom_node: Option<DomNodeRef>,
    canvas_id: CanvasElementId,
}

impl JsHostObject for CanvasElementHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        let snapshot = self.canvas.canvas_snapshot(self.canvas_id)?;

        match key {
            "tagName" | "nodeName" => Some(JsValue::String("CANVAS".to_owned())),
            "localName" => Some(JsValue::String("canvas".to_owned())),
            "width" => Some(JsValue::Number(snapshot.width as f64)),
            "height" => Some(JsValue::Number(snapshot.height as f64)),
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
            "getContext" => Some(canvas_get_context(self.canvas.clone(), self.canvas_id)),
            "toDataURL" => Some(canvas_to_data_url(self.canvas.clone(), self.canvas_id)),
            "toBlob" => Some(canvas_to_blob(
                self.canvas.clone(),
                self.event_loop.clone(),
                self.canvas_id,
            )),
            "transferControlToOffscreen" => Some(canvas_transfer_offscreen(
                self.canvas.clone(),
                self.canvas_id,
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
            "addEventListener" | "removeEventListener" | "dispatchEvent" => {
                Some(native_noop("HTMLCanvasElement.event"))
            }
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        match key {
            "width" => {
                let snapshot = self.canvas.canvas_snapshot(self.canvas_id);
                let height = snapshot.map_or(150, |snapshot| snapshot.height);
                self.canvas.set_canvas_size(
                    self.canvas_id,
                    value.to_number().max(0.0) as u32,
                    height,
                );
                self.dom_set_attribute("width", value.to_js_string());
                true
            }
            "height" => {
                let snapshot = self.canvas.canvas_snapshot(self.canvas_id);
                let width = snapshot.map_or(300, |snapshot| snapshot.width);
                self.canvas.set_canvas_size(
                    self.canvas_id,
                    width,
                    value.to_number().max(0.0) as u32,
                );
                self.dom_set_attribute("height", value.to_js_string());
                true
            }
            "id" => self.dom_set_attribute("id", value.to_js_string()),
            "className" => self.dom_set_attribute("class", value.to_js_string()),
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

impl CanvasElementHost {
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

fn canvas_get_context(canvas: SharedCanvasHost, canvas_id: CanvasElementId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLCanvasElement.getContext".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let context_type = args
                .first()
                .map_or_else(|| "2d".to_owned(), JsValue::to_js_string);
            if context_type != "2d" {
                return Ok(JsValue::Null);
            }
            let context = canvas.context_2d(canvas_id);
            Ok(create_canvas_context_object(canvas.clone(), context))
        }),
    })
}

fn canvas_to_data_url(canvas: SharedCanvasHost, canvas_id: CanvasElementId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLCanvasElement.toDataURL".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let mime = args
                .first()
                .map_or_else(|| "image/png".to_owned(), JsValue::to_js_string);
            Ok(JsValue::String(canvas.to_data_url(canvas_id, &mime)))
        }),
    })
}

fn canvas_to_blob(
    canvas: SharedCanvasHost,
    event_loop: Rc<RefCell<JsEventLoop>>,
    canvas_id: CanvasElementId,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLCanvasElement.toBlob".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            if callback.as_function().is_some() {
                let blob = JsValue::object();
                blob.set_property("type", JsValue::String("image/png".to_owned()));
                blob.set_property(
                    "dataURL",
                    JsValue::String(canvas.to_data_url(canvas_id, "image/png")),
                );
                event_loop.borrow_mut().queue_task(callback, vec![blob]);
            }
            Ok(JsValue::Undefined)
        }),
    })
}

fn canvas_transfer_offscreen(canvas: SharedCanvasHost, canvas_id: CanvasElementId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "HTMLCanvasElement.transferControlToOffscreen".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            Ok(create_canvas_element_object(
                canvas.clone(),
                Rc::new(RefCell::new(JsEventLoop::default())),
                None,
                None,
                canvas_id,
            ))
        }),
    })
}

fn create_canvas_context_object(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(CanvasContext2dHost { canvas, context }),
        "[object CanvasRenderingContext2D]",
    );
    object.set_property("__syljsCanvasContextId", JsValue::Number(context.0 as f64));
    object
}

#[derive(Clone)]
struct CanvasContext2dHost {
    canvas: SharedCanvasHost,
    context: CanvasContextId,
}

impl JsHostObject for CanvasContext2dHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "canvas" => self.canvas.context_snapshot(self.context).map(|snapshot| {
                let canvas = snapshot.canvas;
                let object = JsValue::object();
                object.set_property("__syljsCanvasElementId", JsValue::Number(canvas.0 as f64));
                object
            }),
            "fillStyle"
            | "strokeStyle"
            | "font"
            | "textAlign"
            | "textBaseline"
            | "lineCap"
            | "lineJoin"
            | "globalCompositeOperation" => {
                Some(JsValue::String(self.canvas.state_get(self.context, key)))
            }
            "lineWidth" | "globalAlpha" | "miterLimit" => Some(JsValue::Number(
                self.canvas
                    .state_get(self.context, key)
                    .parse()
                    .unwrap_or(1.0),
            )),
            "save" => Some(record_no_arg(self.canvas.clone(), self.context, "save")),
            "restore" => Some(record_no_arg(self.canvas.clone(), self.context, "restore")),
            "beginPath" => Some(record_no_arg(
                self.canvas.clone(),
                self.context,
                "beginPath",
            )),
            "closePath" => Some(record_no_arg(
                self.canvas.clone(),
                self.context,
                "closePath",
            )),
            "moveTo" => Some(path_two_arg(self.canvas.clone(), self.context, "moveTo")),
            "lineTo" => Some(path_two_arg(self.canvas.clone(), self.context, "lineTo")),
            "rect" => Some(rect_command(self.canvas.clone(), self.context)),
            "arc" => Some(arc_command(self.canvas.clone(), self.context)),
            "fill" => Some(fill_command(self.canvas.clone(), self.context)),
            "stroke" => Some(stroke_command(self.canvas.clone(), self.context)),
            "clearRect" => Some(clear_rect_command(self.canvas.clone(), self.context)),
            "fillRect" => Some(fill_rect_command(self.canvas.clone(), self.context)),
            "strokeRect" => Some(stroke_rect_command(self.canvas.clone(), self.context)),
            "fillText" => Some(text_command(self.canvas.clone(), self.context, true)),
            "strokeText" => Some(text_command(self.canvas.clone(), self.context, false)),
            "measureText" => Some(measure_text_command(self.canvas.clone(), self.context)),
            "drawImage" => Some(draw_image_command(self.canvas.clone(), self.context)),
            "createImageData" => Some(create_image_data_method(self.canvas.clone())),
            "getImageData" => Some(get_image_data_method(self.canvas.clone(), self.context)),
            "putImageData" => Some(put_image_data_method(self.canvas.clone(), self.context)),
            "translate" => Some(transform_command(
                self.canvas.clone(),
                self.context,
                "translate",
            )),
            "scale" => Some(transform_command(
                self.canvas.clone(),
                self.context,
                "scale",
            )),
            "rotate" => Some(transform_command(
                self.canvas.clone(),
                self.context,
                "rotate",
            )),
            "setTransform" => Some(transform_command(
                self.canvas.clone(),
                self.context,
                "setTransform",
            )),
            "resetTransform" => Some(transform_command(
                self.canvas.clone(),
                self.context,
                "resetTransform",
            )),
            "clearCommands" => Some(clear_commands_method(self.canvas.clone(), self.context)),
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        match key {
            "fillStyle"
            | "strokeStyle"
            | "font"
            | "textAlign"
            | "textBaseline"
            | "lineCap"
            | "lineJoin"
            | "globalCompositeOperation"
            | "lineWidth"
            | "globalAlpha"
            | "miterLimit" => {
                self.canvas
                    .state_set(self.context, key, value.to_js_string());
                true
            }
            _ => false,
        }
    }
}

fn record_no_arg(
    canvas: SharedCanvasHost,
    context: CanvasContextId,
    kind: &'static str,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: format!("CanvasRenderingContext2D.{kind}"),
        function: Rc::new(move |_vm, _this, _args| {
            let command = match kind {
                "save" => CanvasCommand::Save { context },
                "restore" => CanvasCommand::Restore { context },
                "beginPath" => CanvasCommand::BeginPath { context },
                "closePath" => CanvasCommand::ClosePath { context },
                _ => return Ok(JsValue::Undefined),
            };
            canvas.record_command(command);
            Ok(JsValue::Undefined)
        }),
    })
}

fn path_two_arg(canvas: SharedCanvasHost, context: CanvasContextId, kind: &'static str) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: format!("CanvasRenderingContext2D.{kind}"),
        function: Rc::new(move |_vm, _this, args| {
            let x = args.first().map_or(0.0, JsValue::to_number);
            let y = args.get(1).map_or(0.0, JsValue::to_number);
            let command = if kind == "moveTo" {
                CanvasCommand::MoveTo { context, x, y }
            } else {
                CanvasCommand::LineTo { context, x, y }
            };
            canvas.record_command(command);
            Ok(JsValue::Undefined)
        }),
    })
}

fn rect_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.rect".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            canvas.record_command(CanvasCommand::Rect {
                context,
                x: args.first().map_or(0.0, JsValue::to_number),
                y: args.get(1).map_or(0.0, JsValue::to_number),
                width: args.get(2).map_or(0.0, JsValue::to_number),
                height: args.get(3).map_or(0.0, JsValue::to_number),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn arc_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.arc".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            canvas.record_command(CanvasCommand::Arc {
                context,
                x: args.first().map_or(0.0, JsValue::to_number),
                y: args.get(1).map_or(0.0, JsValue::to_number),
                radius: args.get(2).map_or(0.0, JsValue::to_number),
                start_angle: args.get(3).map_or(0.0, JsValue::to_number),
                end_angle: args.get(4).map_or(0.0, JsValue::to_number),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn fill_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.fill".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            canvas.record_command(CanvasCommand::Fill {
                context,
                fill_style: canvas.state_get(context, "fillStyle"),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn stroke_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.stroke".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            canvas.record_command(CanvasCommand::Stroke {
                context,
                stroke_style: canvas.state_get(context, "strokeStyle"),
                line_width: canvas
                    .state_get(context, "lineWidth")
                    .parse()
                    .unwrap_or(1.0),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn clear_rect_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.clearRect".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            canvas.record_command(CanvasCommand::ClearRect {
                context,
                x: args.first().map_or(0.0, JsValue::to_number),
                y: args.get(1).map_or(0.0, JsValue::to_number),
                width: args.get(2).map_or(0.0, JsValue::to_number),
                height: args.get(3).map_or(0.0, JsValue::to_number),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn fill_rect_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.fillRect".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            canvas.record_command(CanvasCommand::FillRect {
                context,
                x: args.first().map_or(0.0, JsValue::to_number),
                y: args.get(1).map_or(0.0, JsValue::to_number),
                width: args.get(2).map_or(0.0, JsValue::to_number),
                height: args.get(3).map_or(0.0, JsValue::to_number),
                fill_style: canvas.state_get(context, "fillStyle"),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn stroke_rect_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.strokeRect".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            canvas.record_command(CanvasCommand::StrokeRect {
                context,
                x: args.first().map_or(0.0, JsValue::to_number),
                y: args.get(1).map_or(0.0, JsValue::to_number),
                width: args.get(2).map_or(0.0, JsValue::to_number),
                height: args.get(3).map_or(0.0, JsValue::to_number),
                stroke_style: canvas.state_get(context, "strokeStyle"),
                line_width: canvas
                    .state_get(context, "lineWidth")
                    .parse()
                    .unwrap_or(1.0),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn text_command(canvas: SharedCanvasHost, context: CanvasContextId, fill: bool) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: if fill {
            "CanvasRenderingContext2D.fillText"
        } else {
            "CanvasRenderingContext2D.strokeText"
        }
        .to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let text = args.first().map_or_else(String::new, JsValue::to_js_string);
            let x = args.get(1).map_or(0.0, JsValue::to_number);
            let y = args.get(2).map_or(0.0, JsValue::to_number);

            if fill {
                canvas.record_command(CanvasCommand::FillText {
                    context,
                    text,
                    x,
                    y,
                    font: canvas.state_get(context, "font"),
                    fill_style: canvas.state_get(context, "fillStyle"),
                });
            } else {
                canvas.record_command(CanvasCommand::StrokeText {
                    context,
                    text,
                    x,
                    y,
                    font: canvas.state_get(context, "font"),
                    stroke_style: canvas.state_get(context, "strokeStyle"),
                });
            }

            Ok(JsValue::Undefined)
        }),
    })
}

fn measure_text_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.measureText".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let text = args.first().map_or_else(String::new, JsValue::to_js_string);
            let metrics = canvas.measure_text(context, &text);
            let object = JsValue::object();
            object.set_property("width", JsValue::Number(metrics.width));
            object.set_property(
                "actualBoundingBoxAscent",
                JsValue::Number(metrics.actual_bounding_box_ascent),
            );
            object.set_property(
                "actualBoundingBoxDescent",
                JsValue::Number(metrics.actual_bounding_box_descent),
            );
            Ok(object)
        }),
    })
}

fn draw_image_command(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.drawImage".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let source = args
                .first()
                .map_or_else(|| "[source]".to_owned(), JsValue::to_js_string);
            canvas.record_command(CanvasCommand::DrawImage {
                context,
                source,
                x: args.get(1).map_or(0.0, JsValue::to_number),
                y: args.get(2).map_or(0.0, JsValue::to_number),
                width: args.get(3).map(JsValue::to_number),
                height: args.get(4).map(JsValue::to_number),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_image_data_method(canvas: SharedCanvasHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.createImageData".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let width = args.first().map_or(0.0, JsValue::to_number).max(0.0) as u32;
            let height = args.get(1).map_or(0.0, JsValue::to_number).max(0.0) as u32;
            Ok(create_image_data_object(
                canvas.create_image_data(width, height),
            ))
        }),
    })
}

fn get_image_data_method(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.getImageData".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let width = args.get(2).map_or(0.0, JsValue::to_number).max(0.0) as u32;
            let height = args.get(3).map_or(0.0, JsValue::to_number).max(0.0) as u32;
            canvas.record_command(CanvasCommand::GetImageData {
                context,
                x: args.first().map_or(0.0, JsValue::to_number),
                y: args.get(1).map_or(0.0, JsValue::to_number),
                width,
                height,
            });
            Ok(create_image_data_object(
                canvas.create_image_data(width, height),
            ))
        }),
    })
}

fn put_image_data_method(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.putImageData".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let data = args.first().cloned().unwrap_or(JsValue::Undefined);
            let width = data.get_property("width").to_number().max(0.0) as u32;
            let height = data.get_property("height").to_number().max(0.0) as u32;
            canvas.record_command(CanvasCommand::PutImageData {
                context,
                width,
                height,
                x: args.get(1).map_or(0.0, JsValue::to_number),
                y: args.get(2).map_or(0.0, JsValue::to_number),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn transform_command(
    canvas: SharedCanvasHost,
    context: CanvasContextId,
    kind: &'static str,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: format!("CanvasRenderingContext2D.{kind}"),
        function: Rc::new(move |_vm, _this, args| {
            canvas.record_command(CanvasCommand::Transform {
                context,
                kind: kind.to_owned(),
                values: args.iter().map(JsValue::to_number).collect(),
            });
            Ok(JsValue::Undefined)
        }),
    })
}

fn clear_commands_method(canvas: SharedCanvasHost, context: CanvasContextId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CanvasRenderingContext2D.clearCommands".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            canvas.clear_commands(context);
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_image_data_constructor(canvas: SharedCanvasHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "ImageData".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let width = args.first().map_or(0.0, JsValue::to_number).max(0.0) as u32;
            let height = args.get(1).map_or(0.0, JsValue::to_number).max(0.0) as u32;
            Ok(create_image_data_object(
                canvas.create_image_data(width, height),
            ))
        }),
    })
}

fn create_image_data_object(data: CanvasImageData) -> JsValue {
    let object = JsValue::object();
    object.set_property("width", JsValue::Number(data.width as f64));
    object.set_property("height", JsValue::Number(data.height as f64));
    object.set_property("data", create_u8_clamped_array(data.byte_length));
    object
}

fn create_u8_clamped_array(length: usize) -> JsValue {
    let array = JsValue::array(Vec::new());
    array.set_property("length", JsValue::Number(length as f64));
    array.set_property("byteLength", JsValue::Number(length as f64));
    array
}

fn create_canvas_metrics_function(canvas: SharedCanvasHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosCanvasMetrics".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let metrics = canvas.metrics();
            let object = JsValue::object();
            object.set_property(
                "canvasElementsCreated",
                JsValue::Number(metrics.canvas_elements_created as f64),
            );
            object.set_property(
                "contextsCreated",
                JsValue::Number(metrics.contexts_created as f64),
            );
            object.set_property(
                "commandsRecorded",
                JsValue::Number(metrics.commands_recorded as f64),
            );
            object.set_property("stateWrites", JsValue::Number(metrics.state_writes as f64));
            object.set_property(
                "dataUrlExports",
                JsValue::Number(metrics.data_url_exports as f64),
            );
            Ok(object)
        }),
    })
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

fn native_noop(name: &'static str) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: name.to_owned(),
        function: Rc::new(move |_vm, _this, _args| Ok(JsValue::Undefined)),
    })
}

/// Deterministic research Canvas host.
#[derive(Debug)]
pub struct ResearchCanvasHost {
    inner: RefCell<ResearchCanvasInner>,
}

#[derive(Debug)]
struct ResearchCanvasInner {
    next_canvas: u64,
    next_context: u64,
    canvases: BTreeMap<CanvasElementId, CanvasElementState>,
    dom_bindings: BTreeMap<DomNodeRef, CanvasElementId>,
    contexts: BTreeMap<CanvasContextId, CanvasContextState>,
    metrics: CanvasMetrics,
}

#[derive(Debug, Clone)]
struct CanvasElementState {
    id: CanvasElementId,
    dom_node: Option<DomNodeRef>,
    width: u32,
    height: u32,
    contexts: Vec<CanvasContextId>,
}

#[derive(Debug, Clone)]
struct CanvasContextState {
    id: CanvasContextId,
    canvas: CanvasElementId,
    state: BTreeMap<String, String>,
    commands: Vec<CanvasCommand>,
}

impl Default for ResearchCanvasHost {
    fn default() -> Self {
        Self {
            inner: RefCell::new(ResearchCanvasInner {
                next_canvas: 1,
                next_context: 1,
                canvases: BTreeMap::new(),
                dom_bindings: BTreeMap::new(),
                contexts: BTreeMap::new(),
                metrics: CanvasMetrics::default(),
            }),
        }
    }
}

impl ResearchCanvasHost {
    fn default_state() -> BTreeMap<String, String> {
        [
            ("fillStyle", "#000000"),
            ("strokeStyle", "#000000"),
            ("font", "10px sans-serif"),
            ("textAlign", "start"),
            ("textBaseline", "alphabetic"),
            ("lineWidth", "1"),
            ("globalAlpha", "1"),
            ("lineCap", "butt"),
            ("lineJoin", "miter"),
            ("miterLimit", "10"),
            ("globalCompositeOperation", "source-over"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
    }

    fn alloc_canvas(
        inner: &mut ResearchCanvasInner,
        dom_node: Option<DomNodeRef>,
    ) -> CanvasElementId {
        let id = CanvasElementId(inner.next_canvas);
        inner.next_canvas = inner.next_canvas.saturating_add(1);
        inner.canvases.insert(
            id,
            CanvasElementState {
                id,
                dom_node,
                width: 300,
                height: 150,
                contexts: Vec::new(),
            },
        );
        if let Some(node) = dom_node {
            inner.dom_bindings.insert(node, id);
        }
        inner.metrics.canvas_elements_created =
            inner.metrics.canvas_elements_created.saturating_add(1);
        id
    }
}

impl CanvasHost for ResearchCanvasHost {
    fn create_canvas(&self, dom_node: Option<DomNodeRef>) -> CanvasElementId {
        Self::alloc_canvas(&mut self.inner.borrow_mut(), dom_node)
    }

    fn bind_dom_node(&self, node: DomNodeRef) -> CanvasElementId {
        let mut inner = self.inner.borrow_mut();
        if let Some(id) = inner.dom_bindings.get(&node).copied() {
            return id;
        }
        Self::alloc_canvas(&mut inner, Some(node))
    }

    fn canvas_for_dom_node(&self, node: DomNodeRef) -> Option<CanvasElementId> {
        self.inner.borrow().dom_bindings.get(&node).copied()
    }

    fn canvas_snapshot(&self, canvas: CanvasElementId) -> Option<CanvasElementSnapshot> {
        self.inner
            .borrow()
            .canvases
            .get(&canvas)
            .map(|canvas| CanvasElementSnapshot {
                id: canvas.id,
                dom_node: canvas.dom_node,
                width: canvas.width,
                height: canvas.height,
                contexts: canvas.contexts.clone(),
            })
    }

    fn set_canvas_size(&self, canvas: CanvasElementId, width: u32, height: u32) {
        let mut inner = self.inner.borrow_mut();
        if let Some(canvas) = inner.canvases.get_mut(&canvas) {
            canvas.width = width;
            canvas.height = height;
            for context in canvas.contexts.clone() {
                if let Some(context) = inner.contexts.get_mut(&context) {
                    context.commands.clear();
                }
            }
        }
    }

    fn context_2d(&self, canvas: CanvasElementId) -> CanvasContextId {
        let mut inner = self.inner.borrow_mut();
        inner.metrics.get_context_calls = inner.metrics.get_context_calls.saturating_add(1);

        if let Some(existing) = inner
            .canvases
            .get(&canvas)
            .and_then(|canvas| canvas.contexts.first())
            .copied()
        {
            return existing;
        }

        let id = CanvasContextId(inner.next_context);
        inner.next_context = inner.next_context.saturating_add(1);
        inner.contexts.insert(
            id,
            CanvasContextState {
                id,
                canvas,
                state: Self::default_state(),
                commands: Vec::new(),
            },
        );

        if let Some(canvas) = inner.canvases.get_mut(&canvas) {
            canvas.contexts.push(id);
        }

        inner.metrics.contexts_created = inner.metrics.contexts_created.saturating_add(1);
        id
    }

    fn context_snapshot(&self, context: CanvasContextId) -> Option<CanvasContextSnapshot> {
        self.inner
            .borrow()
            .contexts
            .get(&context)
            .map(|context| CanvasContextSnapshot {
                id: context.id,
                canvas: context.canvas,
                state: context.state.clone(),
                commands: context.commands.clone(),
            })
    }

    fn state_get(&self, context: CanvasContextId, property: &str) -> String {
        let mut inner = self.inner.borrow_mut();
        inner.metrics.state_reads = inner.metrics.state_reads.saturating_add(1);
        inner
            .contexts
            .get(&context)
            .and_then(|context| context.state.get(property).cloned())
            .unwrap_or_default()
    }

    fn state_set(&self, context: CanvasContextId, property: &str, value: String) {
        let mut inner = self.inner.borrow_mut();
        if let Some(context) = inner.contexts.get_mut(&context) {
            context.state.insert(property.to_owned(), value);
        }
        inner.metrics.state_writes = inner.metrics.state_writes.saturating_add(1);
    }

    fn record_command(&self, command: CanvasCommand) {
        let context_id = match &command {
            CanvasCommand::Save { context }
            | CanvasCommand::Restore { context }
            | CanvasCommand::BeginPath { context }
            | CanvasCommand::ClosePath { context }
            | CanvasCommand::MoveTo { context, .. }
            | CanvasCommand::LineTo { context, .. }
            | CanvasCommand::Rect { context, .. }
            | CanvasCommand::Arc { context, .. }
            | CanvasCommand::Fill { context, .. }
            | CanvasCommand::Stroke { context, .. }
            | CanvasCommand::ClearRect { context, .. }
            | CanvasCommand::FillRect { context, .. }
            | CanvasCommand::StrokeRect { context, .. }
            | CanvasCommand::FillText { context, .. }
            | CanvasCommand::StrokeText { context, .. }
            | CanvasCommand::DrawImage { context, .. }
            | CanvasCommand::PutImageData { context, .. }
            | CanvasCommand::GetImageData { context, .. }
            | CanvasCommand::Transform { context, .. } => *context,
        };

        let mut inner = self.inner.borrow_mut();
        if let Some(context) = inner.contexts.get_mut(&context_id) {
            context.commands.push(command);
        }
        inner.metrics.commands_recorded = inner.metrics.commands_recorded.saturating_add(1);
    }

    fn clear_commands(&self, context: CanvasContextId) {
        let mut inner = self.inner.borrow_mut();
        if let Some(context) = inner.contexts.get_mut(&context) {
            context.commands.clear();
        }
        inner.metrics.command_clears = inner.metrics.command_clears.saturating_add(1);
    }

    fn measure_text(&self, context: CanvasContextId, text: &str) -> CanvasTextMetrics {
        let mut inner = self.inner.borrow_mut();
        inner.metrics.text_measurements = inner.metrics.text_measurements.saturating_add(1);

        let font = inner
            .contexts
            .get(&context)
            .and_then(|context| context.state.get("font"))
            .cloned()
            .unwrap_or_else(|| "10px sans-serif".to_owned());

        let font_size = font
            .split_whitespace()
            .find_map(|part| part.strip_suffix("px"))
            .and_then(|part| part.parse::<f64>().ok())
            .unwrap_or(10.0);

        CanvasTextMetrics {
            width: text.chars().count() as f64 * font_size * 0.55,
            actual_bounding_box_ascent: font_size * 0.8,
            actual_bounding_box_descent: font_size * 0.2,
        }
    }

    fn create_image_data(&self, width: u32, height: u32) -> CanvasImageData {
        let mut inner = self.inner.borrow_mut();
        inner.metrics.image_data_allocations =
            inner.metrics.image_data_allocations.saturating_add(1);
        CanvasImageData {
            width,
            height,
            byte_length: width as usize * height as usize * 4,
        }
    }

    fn to_data_url(&self, canvas: CanvasElementId, mime: &str) -> String {
        let mut inner = self.inner.borrow_mut();
        inner.metrics.data_url_exports = inner.metrics.data_url_exports.saturating_add(1);
        let command_count = inner
            .canvases
            .get(&canvas)
            .map(|canvas| {
                canvas
                    .contexts
                    .iter()
                    .filter_map(|context| inner.contexts.get(context))
                    .map(|context| context.commands.len())
                    .sum::<usize>()
            })
            .unwrap_or(0);
        format!("data:{mime};sylphos-canvas,commands={command_count}")
    }

    fn metrics(&self) -> CanvasMetrics {
        self.inner.borrow().metrics.clone()
    }

    fn commands(&self) -> Vec<CanvasCommand> {
        self.inner
            .borrow()
            .contexts
            .values()
            .flat_map(|context| context.commands.clone())
            .collect()
    }
}
