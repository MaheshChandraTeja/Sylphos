//! Browser-page CSSOM bridge.
#![allow(dead_code)]

use present::{
    apply_cssom_to_render_document, cssom_invalidation_to_set, CssomEngine, CssomInvalidation,
    CssomMutation, DynamicLayoutState, RenderDocument,
};

use crate::js::ScriptCssomEffects;

/// Per-page CSSOM state owned by the browser/app layer.
#[derive(Debug, Clone, Default)]
pub(crate) struct PageCssomController {
    engine: CssomEngine,
    dynamic_layout: DynamicLayoutState,
    last_invalidation: CssomInvalidation,
}

/// Diagnostic summary for script-driven CSSOM work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CssomBridgeSummary {
    pub(crate) mutations_applied: usize,
    pub(crate) computed_style_queries: usize,
    pub(crate) style_sheet_reads: usize,
    pub(crate) class_mutations: usize,
    pub(crate) style_revision: u64,
    pub(crate) layout_revision: u64,
    pub(crate) paint_revision: u64,
}

impl CssomBridgeSummary {
    #[must_use]
    pub(crate) fn as_log_string(&self) -> String {
        format!(
            "mutations={} computedStyle={} styleSheets={} classMutations={} revs={}/{}/{}",
            self.mutations_applied,
            self.computed_style_queries,
            self.style_sheet_reads,
            self.class_mutations,
            self.style_revision,
            self.layout_revision,
            self.paint_revision,
        )
    }
}

impl PageCssomController {
    /// Creates a new controller.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Builds a CSSOM controller from discovered CSS source strings.
    #[must_use]
    pub(crate) fn from_sources(sources: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            engine: CssomEngine::from_sources(sources),
            dynamic_layout: DynamicLayoutState::default(),
            last_invalidation: CssomInvalidation::full(),
        }
    }

    /// Applies script-detected CSSOM effects to the render document.
    pub(crate) fn apply_script_effects(
        &mut self,
        effects: ScriptCssomEffects,
        document: &mut RenderDocument,
        viewport_width: f32,
        viewport_height: f32,
    ) -> CssomBridgeSummary {
        let mutation_count = effects.mutations.len();
        let invalidation = self.engine.apply_mutations(effects.mutations);
        self.dynamic_layout.apply_cssom_invalidation(invalidation);
        self.last_invalidation = invalidation;
        let _dirty = cssom_invalidation_to_set(invalidation, viewport_width, viewport_height);
        let _ = apply_cssom_to_render_document(document, &self.engine);

        CssomBridgeSummary {
            mutations_applied: mutation_count,
            computed_style_queries: effects.computed_style_queries,
            style_sheet_reads: effects.style_sheet_reads,
            class_mutations: effects.class_mutations,
            style_revision: self.dynamic_layout.style_revision,
            layout_revision: self.dynamic_layout.layout_revision,
            paint_revision: self.dynamic_layout.paint_revision,
        }
    }

    /// Applies the current CSSOM state to a document.
    pub(crate) fn apply_to_document(&self, document: &mut RenderDocument) {
        self.engine.apply_to_document(document);
    }

    /// Applies host-generated mutations.
    pub(crate) fn apply_host_mutations(
        &mut self,
        mutations: impl IntoIterator<Item = CssomMutation>,
        document: &mut RenderDocument,
    ) -> CssomInvalidation {
        let invalidation = self.engine.apply_mutations(mutations);
        self.dynamic_layout.apply_cssom_invalidation(invalidation);
        self.engine.apply_to_document(document);
        self.last_invalidation = invalidation;
        invalidation
    }
}
