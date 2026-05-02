//! Browser shell modules: navigation, tabs, persistent history, chrome rendering, cache, image loading, and interaction.

mod cache;
mod chrome;
mod history;
mod images;
mod navigation;
mod tabs;

pub(crate) use cache::{CachePolicy, CacheSource, CacheStore};
pub(crate) use chrome::{
    build_chrome_paint_plan, BrowserChrome, ChromeAction, ChromeSnapshot, TOOLBAR_HEIGHT,
};
pub(crate) use history::BrowserHistory;
pub(crate) use images::{fetch_decode_images, resolve_document_image_sources};
pub(crate) use navigation::{normalize_user_url, resolve_link_url};
pub(crate) use tabs::{NavigationTarget, TabId, TabManager, TabSnapshot};
