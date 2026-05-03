#![doc = "Script-ready event primitives for the Sylphos mutable DOM model."]

use crate::{DirtyFlags, DomNodeId, DomRuntime, FormControlKind, FormMethod};

/// DOM event kind supported by the current script-ready hook layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomEventKind {
    /// Mouse/pointer click.
    Click,

    /// Focus moved to a node/control.
    Focus,

    /// Focus left a node/control.
    Blur,

    /// Keyboard keydown event.
    KeyDown,

    /// Before-input hook, useful before text mutation.
    BeforeInput,

    /// Text/value changed.
    Input,

    /// Committed value changed.
    Change,

    /// Form submit event.
    Submit,
}

/// DOM event payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomEventPayload {
    /// No payload.
    None,

    /// Text payload for input-like events.
    Text(String),

    /// Keyboard key description.
    Key(String),

    /// Link activation target.
    Link {
        /// Link destination.
        href: String,

        /// Visible link text.
        text: String,
    },

    /// Form-control activation or mutation.
    FormControl {
        /// Parent form id, when the control belongs to a form.
        form_id: Option<u64>,

        /// Activated control id.
        control_id: u64,

        /// Activated control kind.
        kind: FormControlKind,

        /// Optional control name.
        name: Option<String>,

        /// Current control value.
        value: String,
    },

    /// Form submit payload.
    FormSubmit {
        /// Submitted form id.
        form_id: u64,

        /// Submission method.
        method: FormMethod,

        /// Optional submission action URL.
        action: Option<String>,
    },
}

/// Script-ready event object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomEvent {
    /// Event id assigned by the runtime.
    pub id: u64,

    /// Event kind.
    pub kind: DomEventKind,

    /// Target node id.
    pub target: DomNodeId,

    /// Whether this event should bubble in a future DOM tree dispatcher.
    pub bubbles: bool,

    /// Whether default action can be prevented.
    pub cancelable: bool,

    /// Whether default action has been prevented.
    pub default_prevented: bool,

    /// Event payload.
    pub payload: DomEventPayload,
}

impl DomEvent {
    /// Creates a new event with id `0`; the runtime assigns the real id.
    #[must_use]
    pub const fn new(kind: DomEventKind, target: DomNodeId, payload: DomEventPayload) -> Self {
        Self {
            id: 0,
            kind,
            target,
            bubbles: true,
            cancelable: true,
            default_prevented: false,
            payload,
        }
    }

    /// Marks this event as non-cancelable.
    #[must_use]
    pub const fn non_cancelable(mut self) -> Self {
        self.cancelable = false;
        self
    }

    /// Prevents default action when the event is cancelable.
    pub fn prevent_default(&mut self) {
        if self.cancelable {
            self.default_prevented = true;
        }
    }
}

/// Default browser action associated with an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultAction {
    /// Navigate to a URL.
    Navigate(String),

    /// Focus a form control.
    FocusControl(u64),

    /// Blur all form controls.
    BlurControls,

    /// Submit a form.
    SubmitForm {
        /// Form id.
        form_id: u64,

        /// Submit control id, if any.
        submit_control_id: Option<u64>,
    },

    /// Mutate text in a control.
    MutateTextControl {
        /// Control id.
        control_id: u64,

        /// New value or inserted text depending on the app layer.
        value: String,
    },
}

/// Result of event dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDispatchResult {
    /// Event after dispatch bookkeeping.
    pub event: DomEvent,

    /// Default action inferred for the app shell.
    pub default_action: Option<DefaultAction>,

    /// Dirty work produced by this dispatch.
    pub dirty: DirtyFlags,
}

/// Queued script hook for a future JavaScript runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptHook {
    /// Monotonic hook id.
    pub id: u64,

    /// Event kind that generated this hook.
    pub event_kind: DomEventKind,

    /// Target node.
    pub target: DomNodeId,

    /// Human-readable diagnostic message.
    pub description: String,
}

/// FIFO queue of script hooks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptHookQueue {
    next_id: u64,
    hooks: Vec<ScriptHook>,
}

impl ScriptHookQueue {
    /// Adds one hook.
    pub fn push(
        &mut self,
        event_kind: DomEventKind,
        target: DomNodeId,
        description: impl Into<String>,
    ) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.hooks.push(ScriptHook {
            id,
            event_kind,
            target,
            description: description.into(),
        });
    }

    /// Returns queued hooks.
    #[must_use]
    pub fn hooks(&self) -> &[ScriptHook] {
        &self.hooks
    }

    /// Drains queued hooks.
    #[must_use]
    pub fn drain(&mut self) -> Vec<ScriptHook> {
        std::mem::take(&mut self.hooks)
    }
}

/// Dispatches an event through the current script-ready runtime.
///
/// This is not JavaScript execution. It records the event, queues a future script
/// hook, computes a conservative default action, and marks the right dirty work.
pub fn dispatch_dom_event(runtime: &mut DomRuntime, event: DomEvent) -> EventDispatchResult {
    runtime.dispatch_event(event)
}
