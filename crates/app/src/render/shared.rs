use crate::{browser::ChromeSnapshot, render::DecodedImageStore};
use present::RenderDocument;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub(crate) struct SharedPaintState {
    inner: Arc<Mutex<PaintState>>,
}

#[derive(Debug, Clone, Default)]
struct PaintState {
    revision: u64,
    document: Option<RenderDocument>,
    images: DecodedImageStore,
    chrome: ChromeSnapshot,
}

#[derive(Debug, Clone)]
pub(crate) struct PaintSnapshot {
    pub revision: u64,
    pub document: Option<RenderDocument>,
    pub images: DecodedImageStore,
    pub chrome: ChromeSnapshot,
}

impl SharedPaintState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_document(&self, document: RenderDocument) -> bool {
        match self.inner.lock() {
            Ok(mut state) => {
                state.revision = state.revision.wrapping_add(1);
                state.document = Some(document);
                state.images = DecodedImageStore::default();
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn set_images(&self, images: DecodedImageStore) -> bool {
        match self.inner.lock() {
            Ok(mut state) => {
                state.revision = state.revision.wrapping_add(1);
                state.images = images;
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn clear_images(&self) -> bool {
        self.set_images(DecodedImageStore::default())
    }

    pub(crate) fn set_chrome(&self, chrome: ChromeSnapshot) -> bool {
        match self.inner.lock() {
            Ok(mut state) => {
                state.revision = state.revision.wrapping_add(1);
                state.chrome = chrome;
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn snapshot(&self) -> Option<PaintSnapshot> {
        self.inner.lock().ok().map(|state| PaintSnapshot {
            revision: state.revision,
            document: state.document.clone(),
            images: state.images.clone(),
            chrome: state.chrome.clone(),
        })
    }
}
