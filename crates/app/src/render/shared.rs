use crate::{browser::ChromeSnapshot, render::DecodedImageStore};
use present::{DirtyFlags, InvalidationSet, RenderDocument};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub(crate) struct SharedPaintState {
    inner: Arc<Mutex<PaintState>>,
}

#[derive(Debug, Clone, Default)]
struct PaintState {
    revision: u64,
    document_revision: u64,
    image_revision: u64,
    chrome_revision: u64,
    document: Option<RenderDocument>,
    images: DecodedImageStore,
    chrome: ChromeSnapshot,
    invalidation: Option<InvalidationSet>,
}

#[derive(Debug, Clone)]
pub(crate) struct PaintSnapshot {
    pub revision: u64,
    pub document_revision: u64,
    pub image_revision: u64,
    pub chrome_revision: u64,
    pub document: Option<RenderDocument>,
    pub images: DecodedImageStore,
    pub chrome: ChromeSnapshot,
    pub invalidation: Option<InvalidationSet>,
}

impl SharedPaintState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_document(&self, document: RenderDocument) -> bool {
        let mut invalidation = InvalidationSet::default();
        invalidation.mark(
            present::DomNodeId::from_raw_for_tests(1),
            DirtyFlags::structure(),
        );
        self.set_document_with_invalidation(document, invalidation)
    }

    pub(crate) fn set_document_with_invalidation(
        &self,
        document: RenderDocument,
        invalidation: InvalidationSet,
    ) -> bool {
        match self.inner.lock() {
            Ok(mut state) => {
                state.revision = state.revision.wrapping_add(1);
                state.document_revision = state.document_revision.wrapping_add(1);
                state.document = Some(document);
                state.images = DecodedImageStore::default();
                state.image_revision = state.image_revision.wrapping_add(1);
                state.invalidation = Some(invalidation);
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn set_images(&self, images: DecodedImageStore) -> bool {
        match self.inner.lock() {
            Ok(mut state) => {
                state.revision = state.revision.wrapping_add(1);
                state.image_revision = state.image_revision.wrapping_add(1);
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
                state.chrome_revision = state.chrome_revision.wrapping_add(1);
                state.chrome = chrome;
                true
            }
            Err(_) => false,
        }
    }

    pub(crate) fn snapshot(&self) -> Option<PaintSnapshot> {
        self.inner.lock().ok().map(|state| PaintSnapshot {
            revision: state.revision,
            document_revision: state.document_revision,
            image_revision: state.image_revision,
            chrome_revision: state.chrome_revision,
            document: state.document.clone(),
            images: state.images.clone(),
            chrome: state.chrome.clone(),
            invalidation: state.invalidation.clone(),
        })
    }
}
