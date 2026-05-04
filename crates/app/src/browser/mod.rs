//! Browser shell modules: navigation, tabs, persistent history, chrome rendering, cache, image loading, resources, stylesheets, DOM, CSSOM, accessibility, and interaction.

mod accessibility;
mod cache;
mod chrome;
mod cssom_bridge;
mod dom;
mod forms;
mod history;
mod http;
mod images;
mod navigation;
mod resources;
mod security;
mod stylesheets;
mod tabs;

pub(crate) use cache::{CacheBucket, CachePolicy, CacheSource, CacheStore};
pub(crate) use chrome::{
    build_chrome_paint_plan, BrowserChrome, ChromeAction, ChromeSnapshot, TOOLBAR_HEIGHT,
};
pub(crate) use dom::PageDomController;
pub(crate) use forms::{PageFormAction, PageFormController};
pub(crate) use history::BrowserHistory;
pub(crate) use images::{fetch_decode_images_with_scheduler, resolve_document_image_sources};
pub(crate) use navigation::{normalize_user_url, resolve_link_url};
pub(crate) use resources::{ResourceKind, ResourceRequest, ResourceScheduler, ResourceSummary};
pub(crate) use stylesheets::{load_and_apply_stylesheets_with_scheduler, StylesheetLoadSummary};
pub(crate) use tabs::{NavigationTarget, TabId, TabManager, TabSnapshot};
