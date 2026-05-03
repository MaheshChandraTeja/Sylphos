#![allow(clippy::too_many_lines)]
#![doc = "Transferable ArrayBuffer and typed-array-lite runtime for SylJS."]

use crate::{
    JsFunction, JsHostObject, JsNativeFunction, JsObject, JsObjectKind, JsRuntimeError, JsValue, Vm,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Shared transfer host pointer.
pub type SharedTransferHost = Rc<dyn TransferHost>;

/// Stable ArrayBuffer id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArrayBufferId(pub u64);

/// Typed array kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedArrayKind {
    /// Uint8Array.
    Uint8,

    /// Uint8ClampedArray.
    Uint8Clamped,

    /// Int32Array.
    Int32,

    /// Float64Array.
    Float64,
}

impl TypedArrayKind {
    /// Bytes per element.
    #[must_use]
    pub const fn bytes_per_element(self) -> usize {
        match self {
            Self::Uint8 | Self::Uint8Clamped => 1,
            Self::Int32 => 4,
            Self::Float64 => 8,
        }
    }

    fn constructor_name(self) -> &'static str {
        match self {
            Self::Uint8 => "Uint8Array",
            Self::Uint8Clamped => "Uint8ClampedArray",
            Self::Int32 => "Int32Array",
            Self::Float64 => "Float64Array",
        }
    }
}

/// Transfer mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferMode {
    /// Structured clone.
    Clone,

    /// Transfer ownership and detach original.
    Transfer,
}

/// ArrayBuffer snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayBufferSnapshot {
    /// Buffer id.
    pub id: ArrayBufferId,

    /// Current byte length. Detached buffers have 0.
    pub byte_length: usize,

    /// Detached flag.
    pub detached: bool,

    /// Total allocated bytes before detach.
    pub allocated_bytes: usize,
}

/// Typed array snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedArraySnapshot {
    /// Array kind.
    pub kind: TypedArrayKind,

    /// Backing buffer id.
    pub buffer: ArrayBufferId,

    /// Byte offset.
    pub byte_offset: usize,

    /// Length in elements.
    pub length: usize,

    /// Byte length.
    pub byte_length: usize,

    /// Detached flag from backing buffer.
    pub detached: bool,
}

/// Transfer operation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRecord {
    /// Source buffer.
    pub source: ArrayBufferId,

    /// Destination buffer, when cloned/transferred.
    pub destination: Option<ArrayBufferId>,

    /// Mode.
    pub mode: TransferMode,

    /// Bytes moved/cloned.
    pub bytes: usize,
}

/// Transfer metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransferMetrics {
    /// ArrayBuffers created.
    pub buffers_created: u64,

    /// Typed array views created.
    pub typed_arrays_created: u64,

    /// Total bytes allocated.
    pub bytes_allocated: u64,

    /// Buffers cloned.
    pub buffers_cloned: u64,

    /// Buffers transferred.
    pub buffers_transferred: u64,

    /// Buffers detached.
    pub buffers_detached: u64,

    /// Byte reads.
    pub byte_reads: u64,

    /// Byte writes.
    pub byte_writes: u64,

    /// Structured clone operations.
    pub structured_clones: u64,
}

/// Transfer host abstraction.
pub trait TransferHost {
    /// Creates a new ArrayBuffer.
    fn create_buffer(&self, byte_length: usize) -> ArrayBufferId;

    /// Returns a snapshot.
    fn buffer_snapshot(&self, id: ArrayBufferId) -> Option<ArrayBufferSnapshot>;

    /// Reads a byte.
    fn read_byte(&self, id: ArrayBufferId, offset: usize) -> Option<u8>;

    /// Writes a byte.
    fn write_byte(&self, id: ArrayBufferId, offset: usize, value: u8) -> bool;

    /// Clones buffer into a new buffer.
    fn clone_buffer(&self, id: ArrayBufferId) -> Option<ArrayBufferId>;

    /// Transfers buffer into a new buffer and detaches original.
    fn transfer_buffer(&self, id: ArrayBufferId) -> Option<ArrayBufferId>;

    /// Detaches a buffer.
    fn detach_buffer(&self, id: ArrayBufferId) -> bool;

    /// Structured-clones a JS value.
    fn structured_clone(&self, value: JsValue, transfer_list: &[JsValue]) -> JsValue;

    /// Metrics.
    fn metrics(&self) -> TransferMetrics;

    /// Transfer records.
    fn records(&self) -> Vec<TransferRecord>;
}

/// Installs ArrayBuffer and typed-array-lite constructors.
pub fn install_transfer_globals(vm: &mut Vm, host: SharedTransferHost) {
    vm.define_global("ArrayBuffer", create_array_buffer_constructor(host.clone()));
    vm.define_global(
        "Uint8Array",
        create_typed_array_constructor(host.clone(), TypedArrayKind::Uint8),
    );
    vm.define_global(
        "Uint8ClampedArray",
        create_typed_array_constructor(host.clone(), TypedArrayKind::Uint8Clamped),
    );
    vm.define_global(
        "Int32Array",
        create_typed_array_constructor(host.clone(), TypedArrayKind::Int32),
    );
    vm.define_global(
        "Float64Array",
        create_typed_array_constructor(host.clone(), TypedArrayKind::Float64),
    );
    vm.define_global(
        "structuredClone",
        create_structured_clone_function(host.clone()),
    );
    vm.define_global(
        "__sylphosTransferMetrics",
        create_transfer_metrics_function(host),
    );
}

/// Creates a script-visible ArrayBuffer object.
#[must_use]
pub fn create_array_buffer_object(host: SharedTransferHost, id: ArrayBufferId) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(ArrayBufferHost { host, id }),
        "[object ArrayBuffer]",
    );
    object.set_property("__syljsArrayBufferId", JsValue::Number(id.0 as f64));
    object
}

/// Reads ArrayBuffer id from JS value.
#[must_use]
pub fn array_buffer_id_from_value(value: &JsValue) -> Option<ArrayBufferId> {
    let number = value.get_property("__syljsArrayBufferId").to_number();
    (number.is_finite() && number > 0.0).then_some(ArrayBufferId(number as u64))
}

/// Reads transfer list from JS value.
#[must_use]
pub fn transfer_list_from_value(value: Option<&JsValue>) -> Vec<JsValue> {
    let Some(value) = value else {
        return Vec::new();
    };

    let length = value.get_property("length").to_number().max(0.0) as usize;
    (0..length)
        .filter_map(|index| {
            let item = value.get_property(&index.to_string());
            (!matches!(item, JsValue::Undefined | JsValue::Null)).then_some(item)
        })
        .collect()
}

fn create_array_buffer_constructor(host: SharedTransferHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "ArrayBuffer".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let byte_length = args.first().map_or(0.0, JsValue::to_number).max(0.0) as usize;
            let id = host.create_buffer(byte_length);
            Ok(create_array_buffer_object(host.clone(), id))
        }),
    })
}

#[derive(Clone)]
struct ArrayBufferHost {
    host: SharedTransferHost,
    id: ArrayBufferId,
}

impl JsHostObject for ArrayBufferHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "byteLength" => Some(JsValue::Number(
                self.host
                    .buffer_snapshot(self.id)
                    .map_or(0.0, |snapshot| snapshot.byte_length as f64),
            )),
            "slice" => Some(array_buffer_slice(self.host.clone(), self.id)),
            "detached" | "__sylphosDetached" => Some(JsValue::Boolean(
                self.host
                    .buffer_snapshot(self.id)
                    .is_some_and(|snapshot| snapshot.detached),
            )),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn array_buffer_slice(host: SharedTransferHost, id: ArrayBufferId) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "ArrayBuffer.slice".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let start = args.first().map_or(0.0, JsValue::to_number).max(0.0) as usize;
            let end = args
                .get(1)
                .map(|value| value.to_number().max(start as f64) as usize);

            let Some(snapshot) = host.buffer_snapshot(id) else {
                return Ok(JsValue::Null);
            };

            let end = end.unwrap_or(snapshot.byte_length).min(snapshot.byte_length);
            let length = end.saturating_sub(start);
            let new_id = host.create_buffer(length);

            for index in 0..length {
                if let Some(byte) = host.read_byte(id, start + index) {
                    host.write_byte(new_id, index, byte);
                }
            }

            Ok(create_array_buffer_object(host.clone(), new_id))
        }),
    })
}

fn create_typed_array_constructor(host: SharedTransferHost, kind: TypedArrayKind) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: kind.constructor_name().to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let first = args.first().cloned().unwrap_or(JsValue::Number(0.0));

            let (buffer, byte_offset, length) = if let Some(existing) = array_buffer_id_from_value(&first) {
                let byte_offset = args.get(1).map_or(0.0, JsValue::to_number).max(0.0) as usize;
                let snapshot = host.buffer_snapshot(existing).ok_or_else(|| {
                    JsRuntimeError::new("typed array received unknown ArrayBuffer")
                })?;
                let max_len = snapshot
                    .byte_length
                    .saturating_sub(byte_offset)
                    / kind.bytes_per_element();
                let length = args
                    .get(2)
                    .map(|value| value.to_number().max(0.0) as usize)
                    .unwrap_or(max_len)
                    .min(max_len);
                (existing, byte_offset, length)
            } else {
                let length = first.to_number().max(0.0) as usize;
                let buffer = host.create_buffer(length.saturating_mul(kind.bytes_per_element()));
                (buffer, 0, length)
            };

            Ok(create_typed_array_object(
                host.clone(),
                kind,
                buffer,
                byte_offset,
                length,
            ))
        }),
    })
}

fn create_typed_array_object(
    host: SharedTransferHost,
    kind: TypedArrayKind,
    buffer: ArrayBufferId,
    byte_offset: usize,
    length: usize,
) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(TypedArrayHost {
            host,
            kind,
            buffer,
            byte_offset,
            length,
        }),
        match kind {
            TypedArrayKind::Uint8 => "[object Uint8Array]",
            TypedArrayKind::Uint8Clamped => "[object Uint8ClampedArray]",
            TypedArrayKind::Int32 => "[object Int32Array]",
            TypedArrayKind::Float64 => "[object Float64Array]",
        },
    );
    object.set_property("__syljsArrayBufferId", JsValue::Number(buffer.0 as f64));
    object
}

#[derive(Clone)]
struct TypedArrayHost {
    host: SharedTransferHost,
    kind: TypedArrayKind,
    buffer: ArrayBufferId,
    byte_offset: usize,
    length: usize,
}

impl JsHostObject for TypedArrayHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "buffer" => Some(create_array_buffer_object(self.host.clone(), self.buffer)),
            "byteOffset" => Some(JsValue::Number(self.byte_offset as f64)),
            "byteLength" => Some(JsValue::Number(
                self.length.saturating_mul(self.kind.bytes_per_element()) as f64,
            )),
            "length" => Some(JsValue::Number(self.length as f64)),
            "BYTES_PER_ELEMENT" => Some(JsValue::Number(self.kind.bytes_per_element() as f64)),
            "set" => Some(typed_array_set(self.clone())),
            "slice" => Some(typed_array_slice(self.clone())),
            "subarray" => Some(typed_array_subarray(self.clone())),
            index if index.parse::<usize>().is_ok() => {
                let index = index.parse::<usize>().ok()?;
                Some(JsValue::Number(self.read_numeric(index)))
            }
            _ => None,
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        let Ok(index) = key.parse::<usize>() else {
            return false;
        };

        if index >= self.length {
            return false;
        }

        self.write_numeric(index, value.to_number());
        true
    }
}

impl TypedArrayHost {
    fn snapshot(&self) -> TypedArraySnapshot {
        let byte_length = self.length.saturating_mul(self.kind.bytes_per_element());
        TypedArraySnapshot {
            kind: self.kind,
            buffer: self.buffer,
            byte_offset: self.byte_offset,
            length: self.length,
            byte_length,
            detached: self
                .host
                .buffer_snapshot(self.buffer)
                .is_some_and(|snapshot| snapshot.detached),
        }
    }

    fn read_numeric(&self, index: usize) -> f64 {
        if index >= self.length {
            return f64::NAN;
        }

        let offset = self
            .byte_offset
            .saturating_add(index.saturating_mul(self.kind.bytes_per_element()));

        match self.kind {
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => {
                self.host.read_byte(self.buffer, offset).unwrap_or(0) as f64
            }
            TypedArrayKind::Int32 => {
                let bytes = [
                    self.host.read_byte(self.buffer, offset).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 1).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 2).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 3).unwrap_or(0),
                ];
                i32::from_le_bytes(bytes) as f64
            }
            TypedArrayKind::Float64 => {
                let bytes = [
                    self.host.read_byte(self.buffer, offset).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 1).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 2).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 3).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 4).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 5).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 6).unwrap_or(0),
                    self.host.read_byte(self.buffer, offset + 7).unwrap_or(0),
                ];
                f64::from_le_bytes(bytes)
            }
        }
    }

    fn write_numeric(&self, index: usize, value: f64) {
        let offset = self
            .byte_offset
            .saturating_add(index.saturating_mul(self.kind.bytes_per_element()));

        match self.kind {
            TypedArrayKind::Uint8 => {
                self.host.write_byte(self.buffer, offset, value.clamp(0.0, 255.0) as u8);
            }
            TypedArrayKind::Uint8Clamped => {
                self.host.write_byte(self.buffer, offset, value.round().clamp(0.0, 255.0) as u8);
            }
            TypedArrayKind::Int32 => {
                for (byte_index, byte) in (value as i32).to_le_bytes().iter().copied().enumerate() {
                    self.host.write_byte(self.buffer, offset + byte_index, byte);
                }
            }
            TypedArrayKind::Float64 => {
                for (byte_index, byte) in value.to_le_bytes().iter().copied().enumerate() {
                    self.host.write_byte(self.buffer, offset + byte_index, byte);
                }
            }
        }
    }
}

fn typed_array_set(array: TypedArrayHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: format!("{}.set", array.kind.constructor_name()),
        function: Rc::new(move |_vm, _this, args| {
            let source = args.first().cloned().unwrap_or(JsValue::Undefined);
            let offset = args.get(1).map_or(0.0, JsValue::to_number).max(0.0) as usize;
            let len = source.get_property("length").to_number().max(0.0) as usize;

            for index in 0..len {
                let value = source.get_property(&index.to_string()).to_number();
                if offset + index < array.length {
                    array.write_numeric(offset + index, value);
                }
            }

            Ok(JsValue::Undefined)
        }),
    })
}

fn typed_array_slice(array: TypedArrayHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: format!("{}.slice", array.kind.constructor_name()),
        function: Rc::new(move |_vm, _this, args| {
            let start = args.first().map_or(0.0, JsValue::to_number).max(0.0) as usize;
            let end = args
                .get(1)
                .map(|value| value.to_number().max(start as f64) as usize)
                .unwrap_or(array.length)
                .min(array.length);
            let length = end.saturating_sub(start);
            let new_buffer = array
                .host
                .create_buffer(length.saturating_mul(array.kind.bytes_per_element()));
            let sliced = TypedArrayHost {
                host: array.host.clone(),
                kind: array.kind,
                buffer: new_buffer,
                byte_offset: 0,
                length,
            };

            for index in 0..length {
                sliced.write_numeric(index, array.read_numeric(start + index));
            }

            Ok(create_typed_array_object(
                array.host.clone(),
                array.kind,
                new_buffer,
                0,
                length,
            ))
        }),
    })
}

fn typed_array_subarray(array: TypedArrayHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: format!("{}.subarray", array.kind.constructor_name()),
        function: Rc::new(move |_vm, _this, args| {
            let start = args.first().map_or(0.0, JsValue::to_number).max(0.0) as usize;
            let end = args
                .get(1)
                .map(|value| value.to_number().max(start as f64) as usize)
                .unwrap_or(array.length)
                .min(array.length);
            let length = end.saturating_sub(start);
            Ok(create_typed_array_object(
                array.host.clone(),
                array.kind,
                array.buffer,
                array.byte_offset + start * array.kind.bytes_per_element(),
                length,
            ))
        }),
    })
}

fn create_structured_clone_function(host: SharedTransferHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "structuredClone".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let value = args.first().cloned().unwrap_or(JsValue::Undefined);
            let transfer_list = if let Some(options) = args.get(1) {
                transfer_list_from_value(Some(&options.get_property("transfer")))
            } else {
                Vec::new()
            };

            Ok(host.structured_clone(value, &transfer_list))
        }),
    })
}

fn create_transfer_metrics_function(host: SharedTransferHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosTransferMetrics".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let metrics = host.metrics();
            let object = JsValue::object();
            object.set_property("buffersCreated", JsValue::Number(metrics.buffers_created as f64));
            object.set_property(
                "typedArraysCreated",
                JsValue::Number(metrics.typed_arrays_created as f64),
            );
            object.set_property("bytesAllocated", JsValue::Number(metrics.bytes_allocated as f64));
            object.set_property("buffersCloned", JsValue::Number(metrics.buffers_cloned as f64));
            object.set_property(
                "buffersTransferred",
                JsValue::Number(metrics.buffers_transferred as f64),
            );
            object.set_property("buffersDetached", JsValue::Number(metrics.buffers_detached as f64));
            object.set_property("byteReads", JsValue::Number(metrics.byte_reads as f64));
            object.set_property("byteWrites", JsValue::Number(metrics.byte_writes as f64));
            Ok(object)
        }),
    })
}

/// Deterministic in-memory transfer host.
#[derive(Debug, Default)]
pub struct ResearchTransferHost {
    inner: RefCell<TransferInner>,
}

#[derive(Debug, Default)]
struct TransferInner {
    next_buffer: u64,
    buffers: BTreeMap<ArrayBufferId, BufferState>,
    metrics: TransferMetrics,
    records: Vec<TransferRecord>,
}

#[derive(Debug, Clone)]
struct BufferState {
    bytes: Vec<u8>,
    allocated_bytes: usize,
    detached: bool,
}

impl ResearchTransferHost {
    fn copy_buffer(&self, id: ArrayBufferId, mode: TransferMode) -> Option<ArrayBufferId> {
        let mut inner = self.inner.borrow_mut();
        let source = inner.buffers.get(&id)?.clone();
        if source.detached {
            return None;
        }

        let new_id = ArrayBufferId(inner.next_buffer.max(1));
        inner.next_buffer = new_id.0.saturating_add(1);
        inner.buffers.insert(
            new_id,
            BufferState {
                bytes: source.bytes.clone(),
                allocated_bytes: source.bytes.len(),
                detached: false,
            },
        );

        let bytes = source.bytes.len();
        inner.metrics.buffers_created = inner.metrics.buffers_created.saturating_add(1);
        inner.metrics.bytes_allocated = inner.metrics.bytes_allocated.saturating_add(bytes as u64);

        match mode {
            TransferMode::Clone => {
                inner.metrics.buffers_cloned = inner.metrics.buffers_cloned.saturating_add(1);
            }
            TransferMode::Transfer => {
                inner.metrics.buffers_transferred =
                    inner.metrics.buffers_transferred.saturating_add(1);
                if let Some(source_buffer) = inner.buffers.get_mut(&id) {
                    source_buffer.bytes.clear();
                    source_buffer.detached = true;
                    inner.metrics.buffers_detached =
                        inner.metrics.buffers_detached.saturating_add(1);
                }
            }
        }

        inner.records.push(TransferRecord {
            source: id,
            destination: Some(new_id),
            mode,
            bytes,
        });

        Some(new_id)
    }
}

impl TransferHost for ResearchTransferHost {
    fn create_buffer(&self, byte_length: usize) -> ArrayBufferId {
        let mut inner = self.inner.borrow_mut();
        let id = ArrayBufferId(inner.next_buffer.max(1));
        inner.next_buffer = id.0.saturating_add(1);
        inner.buffers.insert(
            id,
            BufferState {
                bytes: vec![0; byte_length],
                allocated_bytes: byte_length,
                detached: false,
            },
        );
        inner.metrics.buffers_created = inner.metrics.buffers_created.saturating_add(1);
        inner.metrics.bytes_allocated =
            inner.metrics.bytes_allocated.saturating_add(byte_length as u64);
        id
    }

    fn buffer_snapshot(&self, id: ArrayBufferId) -> Option<ArrayBufferSnapshot> {
        self.inner.borrow().buffers.get(&id).map(|buffer| ArrayBufferSnapshot {
            id,
            byte_length: if buffer.detached { 0 } else { buffer.bytes.len() },
            detached: buffer.detached,
            allocated_bytes: buffer.allocated_bytes,
        })
    }

    fn read_byte(&self, id: ArrayBufferId, offset: usize) -> Option<u8> {
        let mut inner = self.inner.borrow_mut();
        inner.metrics.byte_reads = inner.metrics.byte_reads.saturating_add(1);
        let buffer = inner.buffers.get(&id)?;
        (!buffer.detached).then(|| buffer.bytes.get(offset).copied()).flatten()
    }

    fn write_byte(&self, id: ArrayBufferId, offset: usize, value: u8) -> bool {
        let mut inner = self.inner.borrow_mut();
        inner.metrics.byte_writes = inner.metrics.byte_writes.saturating_add(1);
        let Some(buffer) = inner.buffers.get_mut(&id) else {
            return false;
        };

        if buffer.detached || offset >= buffer.bytes.len() {
            return false;
        }

        buffer.bytes[offset] = value;
        true
    }

    fn clone_buffer(&self, id: ArrayBufferId) -> Option<ArrayBufferId> {
        self.copy_buffer(id, TransferMode::Clone)
    }

    fn transfer_buffer(&self, id: ArrayBufferId) -> Option<ArrayBufferId> {
        self.copy_buffer(id, TransferMode::Transfer)
    }

    fn detach_buffer(&self, id: ArrayBufferId) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(buffer) = inner.buffers.get_mut(&id) else {
            return false;
        };

        if buffer.detached {
            return false;
        }

        let bytes = buffer.bytes.len();
        buffer.bytes.clear();
        buffer.detached = true;
        inner.metrics.buffers_detached = inner.metrics.buffers_detached.saturating_add(1);
        inner.records.push(TransferRecord {
            source: id,
            destination: None,
            mode: TransferMode::Transfer,
            bytes,
        });
        true
    }

    fn structured_clone(&self, value: JsValue, transfer_list: &[JsValue]) -> JsValue {
        self.inner.borrow_mut().metrics.structured_clones =
            self.inner.borrow().metrics.structured_clones.saturating_add(1);

        for transferable in transfer_list {
            if let Some(id) = array_buffer_id_from_value(transferable) {
                if let Some(new_id) = self.transfer_buffer(id) {
                    if array_buffer_id_from_value(&value) == Some(id) {
                        return create_array_buffer_object(Rc::new(self.clone()), new_id);
                    }
                }
            }
        }

        if let Some(id) = array_buffer_id_from_value(&value) {
            return self
                .clone_buffer(id)
                .map_or(JsValue::Null, |new_id| create_array_buffer_object(Rc::new(self.clone()), new_id));
        }

        value
    }

    fn metrics(&self) -> TransferMetrics {
        self.inner.borrow().metrics.clone()
    }

    fn records(&self) -> Vec<TransferRecord> {
        self.inner.borrow().records.clone()
    }
}

impl Clone for ResearchTransferHost {
    fn clone(&self) -> Self {
        Self {
            inner: RefCell::new(TransferInner {
                next_buffer: self.inner.borrow().next_buffer,
                buffers: self.inner.borrow().buffers.clone(),
                metrics: self.inner.borrow().metrics.clone(),
                records: self.inner.borrow().records.clone(),
            }),
        }
    }
}
