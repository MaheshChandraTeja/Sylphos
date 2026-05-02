//! Browser chrome state and paint-plan generation.

use present::{Color, PaintCommand, PaintPlan};
use winit::{
    keyboard::{Key, ModifiersState, NamedKey},
    window::CursorIcon,
};

use crate::browser::{TabId, TabSnapshot};

/// Fixed browser chrome height in logical pixels.
pub(crate) const TOOLBAR_HEIGHT: f32 = TAB_STRIP_HEIGHT + NAV_BAR_HEIGHT;

const TAB_STRIP_HEIGHT: f32 = 38.0;
const NAV_BAR_HEIGHT: f32 = 64.0;
const NAV_TOP: f32 = TAB_STRIP_HEIGHT;
const BUTTON_SIZE: f32 = 34.0;
const BUTTON_GAP: f32 = 8.0;
const TOOLBAR_PADDING_X: f32 = 12.0;
const TOOLBAR_PADDING_Y: f32 = 11.0;
const URL_BAR_HEIGHT: f32 = 38.0;
const URL_BAR_LEFT: f32 = TOOLBAR_PADDING_X + ((BUTTON_SIZE + BUTTON_GAP) * 4.0) + 8.0;
const URL_BAR_RIGHT_PADDING: f32 = 18.0;
const TEXT_SIZE: f32 = 16.0;
const STATUS_SIZE: f32 = 12.0;
const TAB_HEIGHT: f32 = 30.0;
const TAB_Y: f32 = 5.0;
const TAB_LEFT: f32 = 8.0;
const TAB_GAP: f32 = 4.0;
const TAB_MIN_WIDTH: f32 = 110.0;
const TAB_MAX_WIDTH: f32 = 220.0;
const NEW_TAB_WIDTH: f32 = 34.0;

const CHROME_BG: Color = Color::rgba(0.145, 0.145, 0.155, 1.0);
const CHROME_BORDER: Color = Color::rgba(0.24, 0.24, 0.27, 1.0);
const TAB_BG: Color = Color::rgba(0.18, 0.18, 0.205, 1.0);
const TAB_BG_ACTIVE: Color = Color::rgba(0.24, 0.24, 0.275, 1.0);
const TAB_BG_HOVER: Color = Color::rgba(0.29, 0.29, 0.33, 1.0);
const BUTTON_BG: Color = Color::rgba(0.21, 0.21, 0.235, 1.0);
const BUTTON_BG_DISABLED: Color = Color::rgba(0.16, 0.16, 0.175, 1.0);
const BUTTON_BG_HOVER: Color = Color::rgba(0.28, 0.28, 0.32, 1.0);
const URL_BG: Color = Color::rgba(0.08, 0.08, 0.09, 1.0);
const URL_BG_FOCUSED: Color = Color::rgba(0.055, 0.065, 0.08, 1.0);
const URL_BORDER: Color = Color::rgba(0.33, 0.35, 0.40, 1.0);
const URL_BORDER_FOCUSED: Color = Color::rgba(0.42, 0.56, 0.90, 1.0);
const TEXT: Color = Color::rgba(0.91, 0.92, 0.96, 1.0);
const TEXT_MUTED: Color = Color::rgba(0.58, 0.59, 0.64, 1.0);
const ERROR_TEXT: Color = Color::rgba(1.0, 0.42, 0.42, 1.0);
const LOADING_TEXT: Color = Color::rgba(0.60, 0.75, 1.0, 1.0);

/// High-level UI actions emitted by the browser chrome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChromeAction {
    None,
    Navigate(String),
    Reload,
    Back,
    Forward,
    NewTab,
    CloseTab(TabId),
    SwitchTab(TabId),
    NextTab,
    PreviousTab,
}

/// Mutable browser chrome state owned by the app event loop.
#[derive(Debug, Clone)]
pub(crate) struct BrowserChrome {
    input_text: String,
    committed_url: String,
    focused: bool,
    select_all_on_type: bool,
    loading: bool,
    can_go_back: bool,
    can_go_forward: bool,
    status_text: Option<String>,
    error_text: Option<String>,
    cursor_x: f32,
    cursor_y: f32,
    tabs: Vec<TabSnapshot>,
}

impl BrowserChrome {
    /// Creates chrome state for an initial URL.
    pub(crate) fn new(initial_url: String) -> Self {
        Self {
            input_text: initial_url.clone(),
            committed_url: initial_url,
            focused: false,
            select_all_on_type: false,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            status_text: None,
            error_text: None,
            cursor_x: 0.0,
            cursor_y: 0.0,
            tabs: Vec::new(),
        }
    }

    /// Updates tab snapshots for rendering and hit testing.
    pub(crate) fn set_tabs(&mut self, tabs: Vec<TabSnapshot>) {
        self.tabs = tabs;
    }

    /// Updates navigation affordances after a history mutation.
    pub(crate) fn set_navigation_state(
        &mut self,
        current_url: &str,
        can_go_back: bool,
        can_go_forward: bool,
    ) {
        self.committed_url = current_url.to_owned();
        if !self.focused {
            self.input_text = current_url.to_owned();
        }
        self.can_go_back = can_go_back;
        self.can_go_forward = can_go_forward;
    }

    /// Marks the UI as loading a URL.
    pub(crate) fn set_loading(&mut self, url: &str) {
        self.loading = true;
        self.error_text = None;
        self.status_text = Some(format!("Loading {url}"));
        self.committed_url = url.to_owned();
        if !self.focused {
            self.input_text = url.to_owned();
        }
    }

    /// Marks the UI as successfully loaded.
    pub(crate) fn set_loaded(&mut self, url: &str) {
        self.loading = false;
        self.error_text = None;
        self.status_text = Some("Ready".to_owned());
        self.committed_url = url.to_owned();
        if !self.focused {
            self.input_text = url.to_owned();
        }
    }

    /// Marks the UI as failed.
    pub(crate) fn set_error(&mut self, message: impl Into<String>) {
        self.loading = false;
        self.error_text = Some(message.into());
        self.status_text = None;
    }

    /// Shows a transient link target in the toolbar status strip.
    pub(crate) fn set_hovered_link(&mut self, href: Option<&str>) {
        if self.loading || self.error_text.is_some() {
            return;
        }

        self.status_text = href
            .map(ToOwned::to_owned)
            .or_else(|| Some("Ready".to_owned()));
    }

    /// Updates cursor position for hit testing and hover feedback.
    pub(crate) fn set_cursor_position(&mut self, x: f32, y: f32) {
        self.cursor_x = x;
        self.cursor_y = y;
    }

    /// Returns the cursor icon requested by the chrome.
    pub(crate) fn cursor_icon(&self, width: f32) -> CursorIcon {
        if hit_test_url_bar(self.cursor_x, self.cursor_y, width) {
            CursorIcon::Text
        } else if hit_test_button(self.cursor_x, self.cursor_y).is_some()
            || hit_test_tab_action(self.cursor_x, self.cursor_y, width, &self.tabs).is_some()
        {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }
    }

    /// Handles a mouse click.
    pub(crate) fn handle_click(&mut self, x: f32, y: f32, width: f32) -> ChromeAction {
        if let Some(action) = hit_test_tab_action(x, y, width, &self.tabs) {
            self.focused = false;
            self.select_all_on_type = false;
            return action;
        }

        if hit_test_url_bar(x, y, width) {
            self.focused = true;
            self.select_all_on_type = true;
            self.input_text = self.committed_url.clone();
            return ChromeAction::None;
        }

        self.focused = false;
        self.select_all_on_type = false;

        match hit_test_button(x, y) {
            Some(ChromeButton::Back) if self.can_go_back => ChromeAction::Back,
            Some(ChromeButton::Forward) if self.can_go_forward => ChromeAction::Forward,
            Some(ChromeButton::Reload) => ChromeAction::Reload,
            Some(ChromeButton::NewTab) => ChromeAction::NewTab,
            _ => ChromeAction::None,
        }
    }

    /// Handles a keyboard event and returns a navigation action when needed.
    pub(crate) fn handle_key(&mut self, key: &Key, modifiers: ModifiersState) -> ChromeAction {
        if modifiers.control_key() && is_character_key(key, "l") {
            self.focus_url_bar();
            return ChromeAction::None;
        }

        if modifiers.control_key() && is_character_key(key, "r") {
            return ChromeAction::Reload;
        }

        if modifiers.control_key() && is_character_key(key, "t") {
            return ChromeAction::NewTab;
        }

        if modifiers.control_key() && is_character_key(key, "w") {
            if let Some(active) = self.tabs.iter().find(|tab| tab.active) {
                return ChromeAction::CloseTab(active.id);
            }
        }

        if modifiers.control_key()
            && !modifiers.shift_key()
            && matches!(key, Key::Named(NamedKey::Tab))
        {
            return ChromeAction::NextTab;
        }

        if modifiers.control_key()
            && modifiers.shift_key()
            && matches!(key, Key::Named(NamedKey::Tab))
        {
            return ChromeAction::PreviousTab;
        }

        if modifiers.alt_key() && matches!(key, Key::Named(NamedKey::ArrowLeft)) {
            return ChromeAction::Back;
        }

        if modifiers.alt_key() && matches!(key, Key::Named(NamedKey::ArrowRight)) {
            return ChromeAction::Forward;
        }

        if !self.focused {
            return ChromeAction::None;
        }

        match key {
            Key::Named(NamedKey::Enter) => {
                self.focused = false;
                self.select_all_on_type = false;
                ChromeAction::Navigate(self.input_text.clone())
            }
            Key::Named(NamedKey::Escape) => {
                self.input_text = self.committed_url.clone();
                self.focused = false;
                self.select_all_on_type = false;
                ChromeAction::None
            }
            Key::Named(NamedKey::Backspace) => {
                if self.select_all_on_type {
                    self.input_text.clear();
                    self.select_all_on_type = false;
                } else {
                    self.input_text.pop();
                }
                ChromeAction::None
            }
            Key::Named(NamedKey::Delete) => {
                if self.select_all_on_type {
                    self.input_text.clear();
                    self.select_all_on_type = false;
                }
                ChromeAction::None
            }
            Key::Character(text)
                if !modifiers.control_key() && !modifiers.alt_key() && !modifiers.super_key() =>
            {
                self.insert_text(text.as_ref());
                ChromeAction::None
            }
            _ => ChromeAction::None,
        }
    }

    /// Returns an immutable snapshot for the renderer.
    pub(crate) fn snapshot(&self) -> ChromeSnapshot {
        ChromeSnapshot {
            input_text: self.input_text.clone(),
            focused: self.focused,
            loading: self.loading,
            can_go_back: self.can_go_back,
            can_go_forward: self.can_go_forward,
            status_text: self.status_text.clone(),
            error_text: self.error_text.clone(),
            cursor_x: self.cursor_x,
            cursor_y: self.cursor_y,
            tabs: self.tabs.clone(),
        }
    }

    fn focus_url_bar(&mut self) {
        self.focused = true;
        self.select_all_on_type = true;
        self.input_text = self.committed_url.clone();
    }

    fn insert_text(&mut self, text: &str) {
        let filtered = text
            .chars()
            .filter(|ch| !ch.is_control() && *ch != '\n' && *ch != '\r')
            .collect::<String>();

        if filtered.is_empty() {
            return;
        }

        if self.select_all_on_type {
            self.input_text.clear();
            self.select_all_on_type = false;
        }

        self.input_text.push_str(&filtered);
    }
}

/// Renderer-owned immutable browser chrome snapshot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChromeSnapshot {
    pub input_text: String,
    pub focused: bool,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub status_text: Option<String>,
    pub error_text: Option<String>,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub tabs: Vec<TabSnapshot>,
}

impl Default for ChromeSnapshot {
    fn default() -> Self {
        Self {
            input_text: String::new(),
            focused: false,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            status_text: None,
            error_text: None,
            cursor_x: 0.0,
            cursor_y: 0.0,
            tabs: Vec::new(),
        }
    }
}

/// Builds a paint plan for the browser chrome.
pub(crate) fn build_chrome_paint_plan(snapshot: &ChromeSnapshot, width: f32) -> PaintPlan {
    let toolbar_width = width.max(1.0);
    let mut commands = Vec::with_capacity(48);

    commands.push(PaintCommand::Rect {
        x: 0.0,
        y: 0.0,
        width: toolbar_width,
        height: TOOLBAR_HEIGHT,
        color: CHROME_BG,
    });

    push_tab_strip(&mut commands, snapshot, toolbar_width);

    commands.push(PaintCommand::Rect {
        x: 0.0,
        y: TOOLBAR_HEIGHT - 1.0,
        width: toolbar_width,
        height: 1.0,
        color: CHROME_BORDER,
    });

    push_button(
        &mut commands,
        ChromeButton::Back,
        "<",
        snapshot.can_go_back,
        snapshot,
    );
    push_button(
        &mut commands,
        ChromeButton::Forward,
        ">",
        snapshot.can_go_forward,
        snapshot,
    );
    push_button(&mut commands, ChromeButton::Reload, "R", true, snapshot);
    push_button(&mut commands, ChromeButton::NewTab, "+", true, snapshot);
    push_url_bar(&mut commands, snapshot, toolbar_width);
    push_status_text(&mut commands, snapshot, toolbar_width);

    PaintPlan {
        background: CHROME_BG,
        commands,
    }
}

fn push_tab_strip(commands: &mut Vec<PaintCommand>, snapshot: &ChromeSnapshot, width: f32) {
    commands.push(PaintCommand::Rect {
        x: 0.0,
        y: 0.0,
        width,
        height: TAB_STRIP_HEIGHT,
        color: Color::rgba(0.105, 0.105, 0.118, 1.0),
    });

    let rects = tab_rects(width, &snapshot.tabs);

    for (tab, rect) in snapshot.tabs.iter().zip(rects.iter()) {
        let hovered = hit_test_rect(snapshot.cursor_x, snapshot.cursor_y, *rect);
        let color = if tab.active {
            TAB_BG_ACTIVE
        } else if hovered {
            TAB_BG_HOVER
        } else {
            TAB_BG
        };

        commands.push(PaintCommand::Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color,
        });

        let label = visible_url_text(&tab.title, rect.width - 42.0, 13.0);
        let mut title = if tab.loading {
            format!("* {label}")
        } else {
            label
        };
        if tab.active {
            title.push('|');
        }

        commands.push(PaintCommand::TextPlaceholder {
            x: rect.x + 10.0,
            y: rect.y + 8.0,
            text: title,
            size: 13.0,
            color: if tab.active { TEXT } else { TEXT_MUTED },
        });

        if tab.can_close {
            let close = close_rect(*rect);
            commands.push(PaintCommand::TextPlaceholder {
                x: close.x + 4.0,
                y: close.y + 3.0,
                text: "x".to_owned(),
                size: 12.0,
                color: TEXT_MUTED,
            });
        }
    }

    let new_rect = new_tab_rect(width, &snapshot.tabs);
    if new_rect.width > 0.0 {
        let hovered = hit_test_rect(snapshot.cursor_x, snapshot.cursor_y, new_rect);
        commands.push(PaintCommand::Rect {
            x: new_rect.x,
            y: new_rect.y,
            width: new_rect.width,
            height: new_rect.height,
            color: if hovered { TAB_BG_HOVER } else { TAB_BG },
        });
        commands.push(PaintCommand::TextPlaceholder {
            x: new_rect.x + 11.0,
            y: new_rect.y + 6.0,
            text: "+".to_owned(),
            size: 16.0,
            color: TEXT,
        });
    }
}

fn push_button(
    commands: &mut Vec<PaintCommand>,
    button: ChromeButton,
    label: &str,
    enabled: bool,
    snapshot: &ChromeSnapshot,
) {
    let rect = button_rect(button);
    let hovered = hit_test_rect(snapshot.cursor_x, snapshot.cursor_y, rect);
    let bg = if !enabled {
        BUTTON_BG_DISABLED
    } else if hovered {
        BUTTON_BG_HOVER
    } else {
        BUTTON_BG
    };
    let text_color = if enabled { TEXT } else { TEXT_MUTED };

    commands.push(PaintCommand::Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        color: bg,
    });

    commands.push(PaintCommand::TextPlaceholder {
        x: rect.x + 11.0,
        y: rect.y + 8.0,
        text: label.to_owned(),
        size: 18.0,
        color: text_color,
    });
}

fn push_url_bar(commands: &mut Vec<PaintCommand>, snapshot: &ChromeSnapshot, width: f32) {
    let rect = url_bar_rect(width);
    let border_color = if snapshot.focused {
        URL_BORDER_FOCUSED
    } else {
        URL_BORDER
    };
    let bg = if snapshot.focused {
        URL_BG_FOCUSED
    } else {
        URL_BG
    };

    commands.push(PaintCommand::Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        color: border_color,
    });

    commands.push(PaintCommand::Rect {
        x: rect.x + 1.0,
        y: rect.y + 1.0,
        width: (rect.width - 2.0).max(1.0),
        height: (rect.height - 2.0).max(1.0),
        color: bg,
    });

    let display = visible_url_text(&snapshot.input_text, rect.width - 28.0, TEXT_SIZE);
    let mut text = display;

    if snapshot.focused {
        text.push('|');
    }

    commands.push(PaintCommand::TextPlaceholder {
        x: rect.x + 12.0,
        y: rect.y + 10.0,
        text,
        size: TEXT_SIZE,
        color: TEXT,
    });
}

fn push_status_text(commands: &mut Vec<PaintCommand>, snapshot: &ChromeSnapshot, width: f32) {
    let status = if let Some(error) = &snapshot.error_text {
        Some((error.as_str(), ERROR_TEXT))
    } else if snapshot.loading {
        snapshot
            .status_text
            .as_deref()
            .map(|status| (status, LOADING_TEXT))
    } else {
        snapshot
            .status_text
            .as_deref()
            .map(|status| (status, TEXT_MUTED))
    };

    let Some((text, color)) = status else {
        return;
    };

    let rect = url_bar_rect(width);
    let display = visible_url_text(text, rect.width - 28.0, STATUS_SIZE);

    commands.push(PaintCommand::TextPlaceholder {
        x: rect.x + 12.0,
        y: TOOLBAR_HEIGHT + 7.0,
        text: display,
        size: STATUS_SIZE,
        color,
    });
}

fn visible_url_text(text: &str, width: f32, size: f32) -> String {
    let max_chars = ((width.max(1.0) / (size * 0.56).max(1.0)).floor() as usize).max(4);
    let count = text.chars().count();

    if count <= max_chars {
        return text.to_owned();
    }

    let keep = max_chars.saturating_sub(3);
    let tail = text.chars().rev().take(keep).collect::<Vec<_>>();
    let tail = tail.into_iter().rev().collect::<String>();
    format!("...{tail}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChromeButton {
    Back,
    Forward,
    Reload,
    NewTab,
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

fn button_rect(button: ChromeButton) -> Rect {
    let index = match button {
        ChromeButton::Back => 0.0,
        ChromeButton::Forward => 1.0,
        ChromeButton::Reload => 2.0,
        ChromeButton::NewTab => 3.0,
    };

    Rect {
        x: TOOLBAR_PADDING_X + (index * (BUTTON_SIZE + BUTTON_GAP)),
        y: NAV_TOP + TOOLBAR_PADDING_Y,
        width: BUTTON_SIZE,
        height: BUTTON_SIZE,
    }
}

fn url_bar_rect(width: f32) -> Rect {
    let right = width.max(URL_BAR_LEFT + 100.0) - URL_BAR_RIGHT_PADDING;

    Rect {
        x: URL_BAR_LEFT,
        y: NAV_TOP + TOOLBAR_PADDING_Y,
        width: (right - URL_BAR_LEFT).max(100.0),
        height: URL_BAR_HEIGHT,
    }
}

fn tab_rects(width: f32, tabs: &[TabSnapshot]) -> Vec<Rect> {
    if tabs.is_empty() {
        return Vec::new();
    }

    let usable = (width - TAB_LEFT - NEW_TAB_WIDTH - 24.0).max(TAB_MIN_WIDTH);
    let raw_width = (usable / tabs.len() as f32) - TAB_GAP;
    let tab_width = raw_width.clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);

    tabs.iter()
        .enumerate()
        .map(|(index, _)| Rect {
            x: TAB_LEFT + index as f32 * (tab_width + TAB_GAP),
            y: TAB_Y,
            width: tab_width,
            height: TAB_HEIGHT,
        })
        .take_while(|rect| rect.x + rect.width < width - NEW_TAB_WIDTH - 6.0)
        .collect()
}

fn new_tab_rect(width: f32, tabs: &[TabSnapshot]) -> Rect {
    let rects = tab_rects(width, tabs);
    let x = rects
        .last()
        .map_or(TAB_LEFT, |rect| rect.x + rect.width + TAB_GAP)
        .min(width - NEW_TAB_WIDTH - 6.0);

    if x < TAB_LEFT {
        return Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }

    Rect {
        x,
        y: TAB_Y,
        width: NEW_TAB_WIDTH,
        height: TAB_HEIGHT,
    }
}

fn close_rect(tab_rect: Rect) -> Rect {
    Rect {
        x: tab_rect.x + tab_rect.width - 24.0,
        y: tab_rect.y + 7.0,
        width: 16.0,
        height: 16.0,
    }
}

fn hit_test_button(x: f32, y: f32) -> Option<ChromeButton> {
    [
        ChromeButton::Back,
        ChromeButton::Forward,
        ChromeButton::Reload,
        ChromeButton::NewTab,
    ]
    .into_iter()
    .find(|button| hit_test_rect(x, y, button_rect(*button)))
}

fn hit_test_tab_action(x: f32, y: f32, width: f32, tabs: &[TabSnapshot]) -> Option<ChromeAction> {
    if y > TAB_STRIP_HEIGHT {
        return None;
    }

    let new_rect = new_tab_rect(width, tabs);
    if new_rect.width > 0.0 && hit_test_rect(x, y, new_rect) {
        return Some(ChromeAction::NewTab);
    }

    for (tab, rect) in tabs.iter().zip(tab_rects(width, tabs).iter()) {
        if tab.can_close && hit_test_rect(x, y, close_rect(*rect)) {
            return Some(ChromeAction::CloseTab(tab.id));
        }

        if hit_test_rect(x, y, *rect) {
            return Some(ChromeAction::SwitchTab(tab.id));
        }
    }

    None
}

fn hit_test_url_bar(x: f32, y: f32, width: f32) -> bool {
    hit_test_rect(x, y, url_bar_rect(width))
}

fn hit_test_rect(x: f32, y: f32, rect: Rect) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

fn is_character_key(key: &Key, expected: &str) -> bool {
    match key {
        Key::Character(value) => value.eq_ignore_ascii_case(expected),
        _ => false,
    }
}
