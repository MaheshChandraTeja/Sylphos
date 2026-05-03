//! Page form interaction and GET submission support.

use anyhow::{anyhow, bail, Context, Result};
use present::{
    edit_focused_form_control, focus_form_control, focused_form_control, form_by_id,
    form_id_for_control, form_submission_pairs, FormControlHitResult, FormControlKind, FormMethod,
    FormTextEdit, RenderDocument,
};
use url::Url;
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Result of a page-form interaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PageFormAction {
    /// Nothing changed.
    None,

    /// The render document was mutated and should be repainted.
    Mutated,

    /// Navigate to the generated form submission URL.
    Submit(String),

    /// Show a user-facing error.
    Error(String),
}

/// Page-local form interaction state.
#[derive(Debug, Clone, Default)]
pub(crate) struct PageFormController {
    focused_control_id: Option<u64>,
}

impl PageFormController {
    /// Clears active page-form focus.
    pub(crate) fn clear_focus(&mut self) {
        self.focused_control_id = None;
    }

    /// Returns true if a page form control is focused.
    #[must_use]
    pub(crate) fn has_focus(&self) -> bool {
        self.focused_control_id.is_some()
    }

    /// Handles a click on a form control.
    pub(crate) fn handle_click(
        &mut self,
        document: &mut RenderDocument,
        current_url: &str,
        hit: &FormControlHitResult,
    ) -> PageFormAction {
        match hit.kind {
            FormControlKind::Submit => {
                self.focused_control_id = None;
                let _ = focus_form_control(document, None);
                self.submit(document, current_url, hit.form_id, Some(hit.control_id))
            }
            FormControlKind::Button => {
                self.focused_control_id = None;
                let _ = focus_form_control(document, None);
                PageFormAction::Mutated
            }
            kind if kind.is_text_editable() => {
                self.focused_control_id = Some(hit.control_id);
                if focus_form_control(document, Some(hit.control_id)) {
                    PageFormAction::Mutated
                } else {
                    PageFormAction::None
                }
            }
            _ => PageFormAction::None,
        }
    }

    /// Handles keyboard input for a focused form control.
    pub(crate) fn handle_key(
        &mut self,
        document: &mut RenderDocument,
        current_url: &str,
        key: &Key,
        modifiers: ModifiersState,
    ) -> PageFormAction {
        if modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
            return PageFormAction::None;
        }

        let Some(control) = focused_form_control(document) else {
            self.focused_control_id = None;
            return PageFormAction::None;
        };

        self.focused_control_id = Some(control.id);

        match key {
            Key::Named(NamedKey::Escape) => {
                self.focused_control_id = None;
                let _ = focus_form_control(document, None);
                PageFormAction::Mutated
            }
            Key::Named(NamedKey::Backspace) => {
                if edit_focused_form_control(document, FormTextEdit::Backspace) {
                    PageFormAction::Mutated
                } else {
                    PageFormAction::None
                }
            }
            Key::Named(NamedKey::Delete) => PageFormAction::None,
            Key::Named(NamedKey::Enter) => {
                if control.kind == FormControlKind::TextArea && modifiers.shift_key() {
                    if edit_focused_form_control(document, FormTextEdit::Insert("\n".to_owned())) {
                        return PageFormAction::Mutated;
                    }
                    return PageFormAction::None;
                }

                let Some(form_id) = form_id_for_control(document, control.id) else {
                    return PageFormAction::None;
                };

                self.submit(document, current_url, form_id, None)
            }
            Key::Character(text) => {
                if text.is_empty() {
                    return PageFormAction::None;
                }

                if edit_focused_form_control(document, FormTextEdit::Insert(text.to_string())) {
                    PageFormAction::Mutated
                } else {
                    PageFormAction::None
                }
            }
            _ => PageFormAction::None,
        }
    }

    fn submit(
        &mut self,
        document: &RenderDocument,
        current_url: &str,
        form_id: u64,
        submit_control_id: Option<u64>,
    ) -> PageFormAction {
        match build_form_submission_url(current_url, document, form_id, submit_control_id) {
            Ok(url) => PageFormAction::Submit(url),
            Err(error) => PageFormAction::Error(error.to_string()),
        }
    }
}

/// Builds a navigation URL for a form submission.
pub(crate) fn build_form_submission_url(
    current_url: &str,
    document: &RenderDocument,
    form_id: u64,
    submit_control_id: Option<u64>,
) -> Result<String> {
    let form = form_by_id(document, form_id).ok_or_else(|| anyhow!("form no longer exists"))?;

    if form.method != FormMethod::Get {
        bail!("POST forms are not supported yet");
    }

    let base = Url::parse(current_url).context("current page URL is invalid")?;
    let action = form.action.as_deref().unwrap_or(current_url);
    let mut target = base.join(action).context("failed to resolve form action")?;
    let pairs = form_submission_pairs(document, form_id, submit_control_id);

    {
        let mut query = target.query_pairs_mut();
        query.clear();
        for pair in pairs {
            query.append_pair(&pair.name, &pair.value);
        }
    }

    Ok(target.to_string())
}
