//! Multi-tab document/session state.

use crate::render::DecodedImageStore;
use present::RenderDocument;

use super::navigation::{NavigationController, ABOUT_BLANK};

/// Stable tab identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TabId(u64);

impl TabId {
    #[must_use]
    pub(crate) const fn value(self) -> u64 {
        self.0
    }
}

/// Navigation target emitted by a tab action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavigationTarget {
    pub tab_id: TabId,
    pub url: String,
}

/// Renderer/chrome snapshot for one tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabSnapshot {
    pub id: TabId,
    pub title: String,
    pub url: String,
    pub active: bool,
    pub loading: bool,
    pub can_close: bool,
}

/// Stored rendered page data for a tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TabContentSnapshot {
    pub document: Option<RenderDocument>,
    pub images: DecodedImageStore,
}

/// Owns all open browser tabs.
#[derive(Debug, Clone)]
pub(crate) struct TabManager {
    tabs: Vec<BrowserTab>,
    active_index: usize,
    next_id: u64,
}

#[derive(Debug, Clone)]
struct BrowserTab {
    id: TabId,
    navigation: NavigationController,
    title: Option<String>,
    loading: bool,
    pending_navigation_id: Option<u64>,
    content: TabContentSnapshot,
}

impl TabManager {
    /// Creates a tab manager with one initial tab.
    #[must_use]
    pub(crate) fn new(initial_url: String) -> Self {
        Self {
            tabs: vec![BrowserTab::new(TabId(1), initial_url)],
            active_index: 0,
            next_id: 2,
        }
    }

    /// Returns the active tab id.
    #[must_use]
    pub(crate) fn active_id(&self) -> TabId {
        self.tabs
            .get(self.active_index)
            .map_or(TabId(0), |tab| tab.id)
    }

    /// Returns the active tab URL.
    #[must_use]
    pub(crate) fn current_url(&self) -> &str {
        self.active_tab()
            .map_or(ABOUT_BLANK, |tab| tab.navigation.current_url())
    }

    /// Returns whether active tab can go back.
    #[must_use]
    pub(crate) fn can_go_back(&self) -> bool {
        self.active_tab()
            .is_some_and(|tab| tab.navigation.can_go_back())
    }

    /// Returns whether active tab can go forward.
    #[must_use]
    pub(crate) fn can_go_forward(&self) -> bool {
        self.active_tab()
            .is_some_and(|tab| tab.navigation.can_go_forward())
    }

    /// Navigates the active tab to a URL.
    pub(crate) fn navigate_active_to(&mut self, url: String) -> NavigationTarget {
        let Some(tab) = self.active_tab_mut() else {
            return NavigationTarget {
                tab_id: TabId(0),
                url,
            };
        };

        let target_url = tab.navigation.navigate_to(url);
        NavigationTarget {
            tab_id: tab.id,
            url: target_url,
        }
    }

    /// Returns a reload target for the active tab.
    #[must_use]
    pub(crate) fn reload_active(&self) -> NavigationTarget {
        let Some(tab) = self.active_tab() else {
            return NavigationTarget {
                tab_id: TabId(0),
                url: ABOUT_BLANK.to_owned(),
            };
        };

        NavigationTarget {
            tab_id: tab.id,
            url: tab.navigation.reload_url(),
        }
    }

    /// Goes backward in the active tab.
    pub(crate) fn go_back_active(&mut self) -> Option<NavigationTarget> {
        let tab = self.active_tab_mut()?;
        let url = tab.navigation.go_back()?;
        Some(NavigationTarget {
            tab_id: tab.id,
            url,
        })
    }

    /// Goes forward in the active tab.
    pub(crate) fn go_forward_active(&mut self) -> Option<NavigationTarget> {
        let tab = self.active_tab_mut()?;
        let url = tab.navigation.go_forward()?;
        Some(NavigationTarget {
            tab_id: tab.id,
            url,
        })
    }

    /// Creates and activates a blank tab.
    pub(crate) fn new_blank_tab(&mut self) -> TabId {
        let id = TabId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.tabs.push(BrowserTab::new(id, ABOUT_BLANK.to_owned()));
        self.active_index = self.tabs.len().saturating_sub(1);
        id
    }

    /// Switches to a tab by id.
    pub(crate) fn switch_to(&mut self, id: TabId) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        self.active_index = index;
        true
    }

    /// Activates the next tab, wrapping around.
    pub(crate) fn activate_next(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        self.active_index = (self.active_index + 1) % self.tabs.len();
        true
    }

    /// Activates the previous tab, wrapping around.
    pub(crate) fn activate_previous(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        self.active_index = if self.active_index == 0 {
            self.tabs.len().saturating_sub(1)
        } else {
            self.active_index.saturating_sub(1)
        };
        true
    }

    /// Closes a tab. The last tab is replaced with a fresh blank tab.
    pub(crate) fn close_tab(&mut self, id: TabId) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };

        if self.tabs.len() == 1 {
            self.tabs[0] = BrowserTab::new(id, ABOUT_BLANK.to_owned());
            self.active_index = 0;
            return true;
        }

        self.tabs.remove(index);

        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len().saturating_sub(1);
        } else if index < self.active_index {
            self.active_index = self.active_index.saturating_sub(1);
        }

        true
    }

    /// Marks a tab as loading a navigation id.
    pub(crate) fn mark_loading(&mut self, tab_id: TabId, navigation_id: u64, url: &str) -> bool {
        let Some(tab) = self.tab_mut(tab_id) else {
            return false;
        };

        tab.loading = true;
        tab.pending_navigation_id = Some(navigation_id);
        tab.title = None;
        tab.content = TabContentSnapshot::default();
        let _ = tab.navigation.navigate_to(url.to_owned());
        true
    }

    /// Returns true when a pipeline result still belongs to the tab's latest navigation.
    #[must_use]
    pub(crate) fn is_pending_navigation(&self, tab_id: TabId, navigation_id: u64) -> bool {
        self.tab(tab_id)
            .is_some_and(|tab| tab.pending_navigation_id == Some(navigation_id))
    }

    /// Marks a tab as successfully loaded.
    pub(crate) fn mark_loaded(
        &mut self,
        tab_id: TabId,
        title: Option<String>,
        document: RenderDocument,
        images: DecodedImageStore,
    ) -> bool {
        let Some(tab) = self.tab_mut(tab_id) else {
            return false;
        };

        tab.loading = false;
        tab.pending_navigation_id = None;
        tab.title = title;
        tab.content = TabContentSnapshot {
            document: Some(document),
            images,
        };
        true
    }

    /// Marks a tab as failed.
    pub(crate) fn mark_failed(&mut self, tab_id: TabId) -> bool {
        let Some(tab) = self.tab_mut(tab_id) else {
            return false;
        };

        tab.loading = false;
        tab.pending_navigation_id = None;
        true
    }

    /// Returns active tab content.
    #[must_use]
    pub(crate) fn active_content(&self) -> TabContentSnapshot {
        self.active_tab()
            .map_or_else(TabContentSnapshot::default, |tab| tab.content.clone())
    }

    /// Returns immutable tab snapshots for chrome rendering.
    #[must_use]
    pub(crate) fn snapshots(&self) -> Vec<TabSnapshot> {
        let can_close = self.tabs.len() > 1;
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| TabSnapshot {
                id: tab.id,
                title: tab.display_title(),
                url: tab.navigation.current_url().to_owned(),
                active: index == self.active_index,
                loading: tab.loading,
                can_close,
            })
            .collect()
    }

    fn active_tab(&self) -> Option<&BrowserTab> {
        self.tabs.get(self.active_index)
    }

    fn active_tab_mut(&mut self) -> Option<&mut BrowserTab> {
        self.tabs.get_mut(self.active_index)
    }

    fn tab(&self, id: TabId) -> Option<&BrowserTab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    fn tab_mut(&mut self, id: TabId) -> Option<&mut BrowserTab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }
}

impl BrowserTab {
    fn new(id: TabId, initial_url: String) -> Self {
        Self {
            id,
            navigation: NavigationController::new(initial_url),
            title: None,
            loading: false,
            pending_navigation_id: None,
            content: TabContentSnapshot::default(),
        }
    }

    fn display_title(&self) -> String {
        if self.loading {
            return "Loading...".to_owned();
        }

        if let Some(title) = &self.title {
            if !title.trim().is_empty() {
                return title.clone();
            }
        }

        let url = self.navigation.current_url();
        if url == ABOUT_BLANK {
            "New Tab".to_owned()
        } else {
            url.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_switches_and_closes_tabs() {
        let mut tabs = TabManager::new("https://example.com/".to_owned());
        let first = tabs.active_id();
        let second = tabs.new_blank_tab();

        assert_ne!(first, second);
        assert_eq!(tabs.active_id(), second);
        assert!(tabs.switch_to(first));
        assert_eq!(tabs.active_id(), first);
        assert!(tabs.close_tab(first));
        assert_eq!(tabs.active_id(), second);
    }
}
