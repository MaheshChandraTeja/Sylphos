#![doc = "Minimal form model and mutation helpers for page-local form controls."]

/// HTTP form method supported by Sylphos forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMethod {
    /// GET form submission. Supported by the browser shell.
    Get,

    /// POST form submission. Parsed and represented, but not submitted yet.
    Post,
}

impl Default for FormMethod {
    fn default() -> Self {
        Self::Get
    }
}

impl FormMethod {
    /// Parses an HTML method value.
    #[must_use]
    pub fn from_attr(value: Option<&str>) -> Self {
        match value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "post" => Self::Post,
            _ => Self::Get,
        }
    }
}

/// Minimal form control kind supported by Module 15.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormControlKind {
    /// `<input type="text">` and unknown textual inputs.
    Text,

    /// `<input type="search">`.
    Search,

    /// `<input type="password">`.
    Password,

    /// `<input type="hidden">`.
    Hidden,

    /// `<input type="submit">`.
    Submit,

    /// `<button>` or `<input type="button">`.
    Button,

    /// `<textarea>`.
    TextArea,
}

impl FormControlKind {
    /// Parses an HTML input/button type value.
    #[must_use]
    pub fn from_input_type(value: Option<&str>) -> Self {
        match value.unwrap_or("text").trim().to_ascii_lowercase().as_str() {
            "search" => Self::Search,
            "password" => Self::Password,
            "hidden" => Self::Hidden,
            "submit" => Self::Submit,
            "button" | "reset" => Self::Button,
            _ => Self::Text,
        }
    }

    /// Returns true for controls that accept typed text.
    #[must_use]
    pub const fn is_text_editable(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Search | Self::Password | Self::TextArea
        )
    }

    /// Returns true for controls that submit/activate a form.
    #[must_use]
    pub const fn is_submit_like(self) -> bool {
        matches!(self, Self::Submit | Self::Button)
    }

    /// Returns true for controls that should be visible in page layout.
    #[must_use]
    pub const fn is_visible(self) -> bool {
        !matches!(self, Self::Hidden)
    }
}

/// One extracted form control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormControl {
    /// Stable control id assigned during extraction.
    pub id: u64,

    /// Control kind.
    pub kind: FormControlKind,

    /// Optional HTML `name`.
    pub name: Option<String>,

    /// Current value.
    pub value: String,

    /// Placeholder text.
    pub placeholder: Option<String>,

    /// Human label text, when available.
    pub label: Option<String>,

    /// Disabled controls do not accept focus or submit values.
    pub disabled: bool,

    /// Focus marker owned by the presentation document.
    pub focused: bool,
}

impl FormControl {
    /// Returns true when this control can receive keyboard focus.
    #[must_use]
    pub const fn can_focus(&self) -> bool {
        self.kind.is_text_editable() && !self.disabled
    }

    /// Returns display text for rendering.
    #[must_use]
    pub fn display_text(&self) -> String {
        if self.kind == FormControlKind::Password && !self.value.is_empty() {
            return "•".repeat(self.value.chars().count());
        }

        if !self.value.is_empty() {
            return self.value.clone();
        }

        if let Some(label) = &self.label {
            if self.kind.is_submit_like() && !label.trim().is_empty() {
                return label.clone();
            }
        }

        self.placeholder.clone().unwrap_or_default()
    }
}

/// Extracted form block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormBlock {
    /// Stable form id assigned during extraction.
    pub id: u64,

    /// Optional form `action` attribute.
    pub action: Option<String>,

    /// Form submission method.
    pub method: FormMethod,

    /// Controls in source order.
    pub controls: Vec<FormControl>,
}

impl FormBlock {
    /// Returns true when there are no controls.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.controls.is_empty()
    }
}

/// A value pair produced by form submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormDataPair {
    /// Field name.
    pub name: String,

    /// Field value.
    pub value: String,
}

/// Text mutation command for form controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormTextEdit {
    /// Insert text at the end of the current value.
    Insert(String),

    /// Remove the final scalar value.
    Backspace,

    /// Replace with a complete value.
    Set(String),

    /// Clear the current value.
    Clear,
}

use crate::{RenderBlock, RenderDocument};

/// Clears all form-control focus flags and optionally focuses one control.
#[must_use]
pub fn focus_form_control(document: &mut RenderDocument, control_id: Option<u64>) -> bool {
    let mut found = control_id.is_none();

    for form in forms_mut(document) {
        for control in &mut form.controls {
            control.focused = control_id == Some(control.id) && control.can_focus();
            found |= control.focused;
        }
    }

    found
}

/// Returns the focused form control, if any.
#[must_use]
pub fn focused_form_control(document: &RenderDocument) -> Option<FormControl> {
    forms(document)
        .flat_map(|form| form.controls.iter())
        .find(|control| control.focused)
        .cloned()
}

/// Returns the form id containing a control.
#[must_use]
pub fn form_id_for_control(document: &RenderDocument, control_id: u64) -> Option<u64> {
    forms(document).find_map(|form| {
        form.controls
            .iter()
            .any(|control| control.id == control_id)
            .then_some(form.id)
    })
}

/// Returns a cloned form control by id.
#[must_use]
pub fn form_control_by_id(document: &RenderDocument, control_id: u64) -> Option<FormControl> {
    forms(document)
        .flat_map(|form| form.controls.iter())
        .find(|control| control.id == control_id)
        .cloned()
}

/// Applies a text edit to one control.
#[must_use]
pub fn edit_form_control(
    document: &mut RenderDocument,
    control_id: u64,
    edit: FormTextEdit,
) -> bool {
    let Some(control) = forms_mut(document)
        .flat_map(|form| form.controls.iter_mut())
        .find(|control| control.id == control_id)
    else {
        return false;
    };

    if !control.can_focus() {
        return false;
    }

    match edit {
        FormTextEdit::Insert(text) => {
            let filtered = text
                .chars()
                .filter(|ch| {
                    !ch.is_control() || (*ch == '\n' && control.kind == FormControlKind::TextArea)
                })
                .collect::<String>();
            control.value.push_str(&filtered);
        }
        FormTextEdit::Backspace => {
            control.value.pop();
        }
        FormTextEdit::Set(value) => {
            control.value = value;
        }
        FormTextEdit::Clear => {
            control.value.clear();
        }
    }

    true
}

/// Applies a text edit to the currently focused control.
#[must_use]
pub fn edit_focused_form_control(document: &mut RenderDocument, edit: FormTextEdit) -> bool {
    let Some(control) = focused_form_control(document) else {
        return false;
    };

    edit_form_control(document, control.id, edit)
}

/// Builds successful GET/POST submission pairs for a form.
///
/// Disabled controls and unnamed controls are excluded. Submit/button controls
/// are excluded unless they match `submit_control_id`.
#[must_use]
pub fn form_submission_pairs(
    document: &RenderDocument,
    form_id: u64,
    submit_control_id: Option<u64>,
) -> Vec<FormDataPair> {
    let Some(form) = forms(document).find(|form| form.id == form_id) else {
        return Vec::new();
    };

    form.controls
        .iter()
        .filter(|control| !control.disabled)
        .filter_map(|control| {
            let name = control.name.as_ref()?.trim();
            if name.is_empty() {
                return None;
            }

            if control.kind.is_submit_like() && Some(control.id) != submit_control_id {
                return None;
            }

            Some(FormDataPair {
                name: name.to_owned(),
                value: control.value.clone(),
            })
        })
        .collect()
}

/// Returns a cloned form block by id.
#[must_use]
pub fn form_by_id(document: &RenderDocument, form_id: u64) -> Option<FormBlock> {
    forms(document).find(|form| form.id == form_id).cloned()
}

fn forms(document: &RenderDocument) -> impl Iterator<Item = &FormBlock> {
    document.blocks.iter().filter_map(|block| match block {
        RenderBlock::Form(form) => Some(form),
        _ => None,
    })
}

fn forms_mut(document: &mut RenderDocument) -> impl Iterator<Item = &mut FormBlock> {
    document.blocks.iter_mut().filter_map(|block| match block {
        RenderBlock::Form(form) => Some(form),
        _ => None,
    })
}
