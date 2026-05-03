#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::or_fun_call,
    clippy::suboptimal_flops,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
#![doc = "Font shaping and text measurement upgrade for Sylphos Present."]
#![doc = ""]
#![doc = "This module provides deterministic font fallback, glyph planning,"]
#![doc = "line breaking, text measurement, baseline metrics, whitespace handling,"]
#![doc = "overflow clipping, ellipsis support, and an atlas-friendly glyph run."]
#![doc = "It is intentionally dependency-light so it can land safely before"]
#![doc = "full HarfBuzz/fontkit integration. A browser engine needs stepping"]
#![doc = "stones, not a ceremonial dive into typography lava."]

use std::collections::{BTreeMap, BTreeSet};

/// Stable font face id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontFaceId(pub u64);

/// Stable glyph id inside a font face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphId(pub u32);

/// Font style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontStyle {
    /// Normal style.
    Normal,

    /// Italic style.
    Italic,

    /// Oblique style.
    Oblique,
}

impl Default for FontStyle {
    fn default() -> Self {
        Self::Normal
    }
}

/// Font stretch, represented as CSS percentage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontStretch(pub f32);

impl Default for FontStretch {
    fn default() -> Self {
        Self(100.0)
    }
}

/// CSS font weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontWeight(pub u16);

impl Default for FontWeight {
    fn default() -> Self {
        Self(400)
    }
}

impl FontWeight {
    /// Normal weight.
    pub const NORMAL: Self = Self(400);

    /// Bold weight.
    pub const BOLD: Self = Self(700);

    /// Clamped CSS weight.
    #[must_use]
    pub fn sanitized(self) -> Self {
        Self(self.0.clamp(1, 1000))
    }

    /// Distance to another weight.
    #[must_use]
    pub fn distance(self, other: Self) -> u16 {
        self.0.abs_diff(other.0)
    }
}

/// Font descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct FontDescriptor {
    /// Family name.
    pub family: String,

    /// Weight.
    pub weight: FontWeight,

    /// Style.
    pub style: FontStyle,

    /// Stretch.
    pub stretch: FontStretch,

    /// Whether this face is monospace.
    pub monospace: bool,

    /// Whether this face is emoji-capable.
    pub emoji: bool,
}

impl FontDescriptor {
    /// Creates a normal face descriptor.
    #[must_use]
    pub fn normal(family: impl Into<String>) -> Self {
        Self {
            family: family.into(),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
            stretch: FontStretch::default(),
            monospace: false,
            emoji: false,
        }
    }
}

/// Font metrics in font units normalized to em.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    /// Ascender as fraction of font size.
    pub ascent: f32,

    /// Descender as positive fraction of font size.
    pub descent: f32,

    /// Line gap as fraction of font size.
    pub line_gap: f32,

    /// x-height as fraction of font size.
    pub x_height: f32,

    /// Cap height as fraction of font size.
    pub cap_height: f32,

    /// Average advance as fraction of font size.
    pub average_advance: f32,

    /// Space advance as fraction of font size.
    pub space_advance: f32,
}

impl Default for FontMetrics {
    fn default() -> Self {
        Self {
            ascent: 0.80,
            descent: 0.20,
            line_gap: 0.10,
            x_height: 0.52,
            cap_height: 0.70,
            average_advance: 0.54,
            space_advance: 0.28,
        }
    }
}

impl FontMetrics {
    /// Line height at font size.
    #[must_use]
    pub fn natural_line_height(self, font_size: f32) -> f32 {
        sanitize_px(font_size) * (self.ascent + self.descent + self.line_gap)
    }

    /// Baseline offset from top.
    #[must_use]
    pub fn baseline(self, font_size: f32) -> f32 {
        sanitize_px(font_size) * self.ascent
    }
}

/// Registered font face.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFace {
    /// Face id.
    pub id: FontFaceId,

    /// Descriptor.
    pub descriptor: FontDescriptor,

    /// Metrics.
    pub metrics: FontMetrics,

    /// Unicode coverage ranges inclusive.
    pub coverage: Vec<(u32, u32)>,

    /// Optional source label.
    pub source: Option<String>,
}

impl FontFace {
    /// Returns whether the face supports a character.
    #[must_use]
    pub fn supports(&self, ch: char) -> bool {
        let cp = u32::from(ch);
        self.coverage
            .iter()
            .any(|(start, end)| cp >= *start && cp <= *end)
    }
}

/// Font matching request.
#[derive(Debug, Clone, PartialEq)]
pub struct FontRequest {
    /// Preferred families in CSS order.
    pub families: Vec<String>,

    /// Requested weight.
    pub weight: FontWeight,

    /// Requested style.
    pub style: FontStyle,

    /// Requested stretch.
    pub stretch: FontStretch,
}

impl Default for FontRequest {
    fn default() -> Self {
        Self {
            families: vec!["system-ui".to_owned(), "sans-serif".to_owned()],
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
            stretch: FontStretch::default(),
        }
    }
}

/// Text transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextTransform {
    /// none.
    None,

    /// uppercase.
    Uppercase,

    /// lowercase.
    Lowercase,

    /// capitalize.
    Capitalize,
}

impl Default for TextTransform {
    fn default() -> Self {
        Self::None
    }
}

/// White-space behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WhiteSpace {
    /// normal.
    Normal,

    /// nowrap.
    NoWrap,

    /// pre.
    Pre,

    /// pre-wrap.
    PreWrap,

    /// pre-line.
    PreLine,
}

impl Default for WhiteSpace {
    fn default() -> Self {
        Self::Normal
    }
}

/// Text overflow behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOverflow {
    /// Clip overflowing text.
    Clip,

    /// Append ellipsis when possible.
    Ellipsis,
}

impl Default for TextOverflow {
    fn default() -> Self {
        Self::Clip
    }
}

/// Text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    /// start.
    Start,

    /// left.
    Left,

    /// center.
    Center,

    /// right.
    Right,

    /// end.
    End,
}

impl Default for TextAlign {
    fn default() -> Self {
        Self::Start
    }
}

/// Text style.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Font request.
    pub font: FontRequest,

    /// Font size px.
    pub font_size: f32,

    /// Explicit line height px, if any.
    pub line_height: Option<f32>,

    /// Letter spacing px.
    pub letter_spacing: f32,

    /// Word spacing px.
    pub word_spacing: f32,

    /// White-space.
    pub white_space: WhiteSpace,

    /// Overflow.
    pub overflow: TextOverflow,

    /// Transform.
    pub transform: TextTransform,

    /// Align.
    pub align: TextAlign,

    /// Maximum lines.
    pub max_lines: Option<usize>,

    /// Text direction.
    pub direction: TextDirection,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: FontRequest::default(),
            font_size: 16.0,
            line_height: None,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            white_space: WhiteSpace::Normal,
            overflow: TextOverflow::Clip,
            transform: TextTransform::None,
            align: TextAlign::Start,
            max_lines: None,
            direction: TextDirection::Ltr,
        }
    }
}

/// Text direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    /// Left-to-right.
    Ltr,

    /// Right-to-left.
    Rtl,
}

impl Default for TextDirection {
    fn default() -> Self {
        Self::Ltr
    }
}

/// Script classification for a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScriptClass {
    /// Latin.
    Latin,

    /// Common punctuation/digits/space.
    Common,

    /// CJK.
    Cjk,

    /// Arabic/Hebrew-ish RTL.
    Rtl,

    /// Emoji.
    Emoji,

    /// Unknown fallback.
    Unknown,
}

/// Glyph cluster.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphCluster {
    /// Original byte range.
    pub byte_range: std::ops::Range<usize>,

    /// Text content.
    pub text: String,

    /// Chosen font face.
    pub font: FontFaceId,

    /// Glyph id.
    pub glyph_id: GlyphId,

    /// Advance px.
    pub advance: f32,

    /// X offset px.
    pub x_offset: f32,

    /// Y offset px.
    pub y_offset: f32,

    /// Script class.
    pub script: ScriptClass,

    /// Whether this cluster is whitespace.
    pub whitespace: bool,

    /// Whether this cluster is a hard line break.
    pub hard_break: bool,
}

/// Glyph run.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    /// Font face.
    pub font: FontFaceId,

    /// Glyph clusters.
    pub clusters: Vec<GlyphCluster>,

    /// Total advance.
    pub advance: f32,

    /// Direction.
    pub direction: TextDirection,
}

/// Shaped text.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedText {
    /// Transformed text.
    pub text: String,

    /// Glyph runs.
    pub runs: Vec<GlyphRun>,

    /// Total advance.
    pub advance: f32,

    /// Font size.
    pub font_size: f32,

    /// Natural line height.
    pub line_height: f32,

    /// Baseline.
    pub baseline: f32,
}

/// Line break opportunity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakOpportunity {
    /// No break.
    None,

    /// Soft break.
    Soft,

    /// Hard break.
    Hard,
}

/// Text line.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    /// Line index.
    pub index: usize,

    /// Text content.
    pub text: String,

    /// Glyph clusters.
    pub clusters: Vec<GlyphCluster>,

    /// X offset after alignment.
    pub x: f32,

    /// Y offset.
    pub y: f32,

    /// Width.
    pub width: f32,

    /// Height.
    pub height: f32,

    /// Baseline offset.
    pub baseline: f32,

    /// Whether line was ellipsized.
    pub ellipsized: bool,
}

/// Text layout result.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    /// Lines.
    pub lines: Vec<TextLine>,

    /// Overall width.
    pub width: f32,

    /// Overall height.
    pub height: f32,

    /// Natural unwrapped width.
    pub natural_width: f32,

    /// Baseline of first line.
    pub first_baseline: f32,

    /// Whether text overflowed.
    pub overflowed: bool,

    /// Metrics.
    pub metrics: TextMetrics,
}

/// Text measurement result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMeasure {
    /// Width.
    pub width: f32,

    /// Height.
    pub height: f32,

    /// Baseline.
    pub baseline: f32,

    /// Ascent.
    pub ascent: f32,

    /// Descent.
    pub descent: f32,

    /// Line gap.
    pub line_gap: f32,
}

/// Text metrics counters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextMetrics {
    /// Shape calls.
    pub shape_calls: u64,

    /// Measure calls.
    pub measure_calls: u64,

    /// Layout calls.
    pub layout_calls: u64,

    /// Font fallback decisions.
    pub fallback_decisions: u64,

    /// Glyphs emitted.
    pub glyphs_emitted: u64,

    /// Lines emitted.
    pub lines_emitted: u64,

    /// Soft wraps.
    pub soft_wraps: u64,

    /// Hard wraps.
    pub hard_wraps: u64,

    /// Ellipsis applications.
    pub ellipsis_applications: u64,

    /// Cache hits.
    pub cache_hits: u64,

    /// Cache misses.
    pub cache_misses: u64,
}

/// Glyph atlas key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphAtlasKey {
    /// Font face.
    pub font: FontFaceId,

    /// Glyph id.
    pub glyph: GlyphId,

    /// Quantized font size.
    pub size_px_x64: u32,
}

/// Glyph atlas request.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphAtlasRequest {
    /// Key.
    pub key: GlyphAtlasKey,

    /// Estimated width.
    pub estimated_width: f32,

    /// Estimated height.
    pub estimated_height: f32,
}

/// Font database.
#[derive(Debug, Clone)]
pub struct FontDatabase {
    next_id: u64,
    faces: BTreeMap<FontFaceId, FontFace>,
    family_index: BTreeMap<String, Vec<FontFaceId>>,
    generic_families: BTreeMap<String, Vec<String>>,
}

impl Default for FontDatabase {
    fn default() -> Self {
        let mut db = Self {
            next_id: 1,
            faces: BTreeMap::new(),
            family_index: BTreeMap::new(),
            generic_families: BTreeMap::new(),
        };

        db.install_default_fonts();
        db
    }
}

impl FontDatabase {
    /// Creates an empty database.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            next_id: 1,
            faces: BTreeMap::new(),
            family_index: BTreeMap::new(),
            generic_families: BTreeMap::new(),
        }
    }

    /// Registers a face.
    pub fn register_face(
        &mut self,
        descriptor: FontDescriptor,
        metrics: FontMetrics,
        coverage: Vec<(u32, u32)>,
        source: Option<String>,
    ) -> FontFaceId {
        let id = FontFaceId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);

        let family_key = normalize_family(&descriptor.family);
        let face = FontFace {
            id,
            descriptor,
            metrics,
            coverage,
            source,
        };

        self.faces.insert(id, face);
        self.family_index.entry(family_key).or_default().push(id);
        id
    }

    /// Registers generic family aliases.
    pub fn register_generic_family(
        &mut self,
        generic: impl Into<String>,
        families: impl IntoIterator<Item = impl Into<String>>,
    ) {
        self.generic_families.insert(
            normalize_family(&generic.into()),
            families.into_iter().map(Into::into).collect(),
        );
    }

    /// Returns a face by id.
    #[must_use]
    pub fn face(&self, id: FontFaceId) -> Option<&FontFace> {
        self.faces.get(&id)
    }

    /// Matches a font face for a character.
    #[must_use]
    pub fn match_face(&self, request: &FontRequest, ch: char) -> Option<FontFaceId> {
        let mut candidate_families = Vec::new();

        for family in &request.families {
            let normalized = normalize_family(family);
            if let Some(expanded) = self.generic_families.get(&normalized) {
                candidate_families.extend(expanded.iter().cloned());
            }
            candidate_families.push(family.clone());
        }

        let mut best = None;
        let mut best_score = u32::MAX;

        for family in candidate_families {
            let key = normalize_family(&family);
            let Some(ids) = self.family_index.get(&key) else {
                continue;
            };

            for id in ids {
                let Some(face) = self.faces.get(id) else {
                    continue;
                };

                if !face.supports(ch) {
                    continue;
                }

                let style_penalty = if face.descriptor.style == request.style {
                    0
                } else {
                    100
                };
                let weight_penalty = u32::from(face.descriptor.weight.distance(request.weight));
                let stretch_penalty = (face.descriptor.stretch.0 - request.stretch.0).abs() as u32;
                let score = style_penalty + weight_penalty + stretch_penalty;

                if score < best_score {
                    best_score = score;
                    best = Some(*id);
                }
            }
        }

        best.or_else(|| self.first_supporting_face(ch))
    }

    /// Returns first face supporting char.
    #[must_use]
    pub fn first_supporting_face(&self, ch: char) -> Option<FontFaceId> {
        self.faces
            .iter()
            .find_map(|(id, face)| face.supports(ch).then_some(*id))
    }

    /// Returns default face.
    #[must_use]
    pub fn default_face(&self) -> Option<FontFaceId> {
        self.faces.keys().next().copied()
    }

    fn install_default_fonts(&mut self) {
        let latin = vec![(0x0000, 0x00FF), (0x0100, 0x024F), (0x2000, 0x206F)];
        let cjk = vec![(0x3000, 0x30FF), (0x3400, 0x9FFF), (0xFF00, 0xFFEF)];
        let rtl = vec![(0x0590, 0x08FF), (0xFB1D, 0xFEFC)];
        let emoji = vec![(0x1F000, 0x1FAFF), (0x2600, 0x27BF)];

        self.register_face(
            FontDescriptor {
                family: "Sylphos Sans".to_owned(),
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
                stretch: FontStretch::default(),
                monospace: false,
                emoji: false,
            },
            FontMetrics::default(),
            latin.clone(),
            Some("builtin:sans".to_owned()),
        );

        self.register_face(
            FontDescriptor {
                family: "Sylphos Sans".to_owned(),
                weight: FontWeight::BOLD,
                style: FontStyle::Normal,
                stretch: FontStretch::default(),
                monospace: false,
                emoji: false,
            },
            FontMetrics {
                average_advance: 0.57,
                ..FontMetrics::default()
            },
            latin.clone(),
            Some("builtin:sans-bold".to_owned()),
        );

        self.register_face(
            FontDescriptor {
                family: "Sylphos Mono".to_owned(),
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
                stretch: FontStretch::default(),
                monospace: true,
                emoji: false,
            },
            FontMetrics {
                average_advance: 0.61,
                space_advance: 0.61,
                ..FontMetrics::default()
            },
            latin,
            Some("builtin:mono".to_owned()),
        );

        self.register_face(
            FontDescriptor {
                family: "Sylphos CJK".to_owned(),
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
                stretch: FontStretch::default(),
                monospace: false,
                emoji: false,
            },
            FontMetrics {
                average_advance: 1.0,
                space_advance: 0.5,
                ascent: 0.88,
                descent: 0.18,
                line_gap: 0.04,
                ..FontMetrics::default()
            },
            cjk,
            Some("builtin:cjk".to_owned()),
        );

        self.register_face(
            FontDescriptor {
                family: "Sylphos RTL".to_owned(),
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
                stretch: FontStretch::default(),
                monospace: false,
                emoji: false,
            },
            FontMetrics {
                average_advance: 0.58,
                ascent: 0.86,
                descent: 0.22,
                ..FontMetrics::default()
            },
            rtl,
            Some("builtin:rtl".to_owned()),
        );

        self.register_face(
            FontDescriptor {
                family: "Sylphos Emoji".to_owned(),
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
                stretch: FontStretch::default(),
                monospace: false,
                emoji: true,
            },
            FontMetrics {
                average_advance: 1.0,
                space_advance: 0.5,
                ascent: 0.90,
                descent: 0.10,
                line_gap: 0.0,
                ..FontMetrics::default()
            },
            emoji,
            Some("builtin:emoji".to_owned()),
        );

        self.register_generic_family("system-ui", ["Sylphos Sans"]);
        self.register_generic_family("sans-serif", ["Sylphos Sans"]);
        self.register_generic_family("serif", ["Sylphos Sans"]);
        self.register_generic_family("monospace", ["Sylphos Mono"]);
        self.register_generic_family("emoji", ["Sylphos Emoji"]);
    }
}

/// Text shaper and measurer.
#[derive(Debug, Clone)]
pub struct TextEngine {
    fonts: FontDatabase,
    metrics: TextMetrics,
    measure_cache: BTreeMap<TextCacheKey, TextMeasure>,
    max_cache_entries: usize,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new(FontDatabase::default())
    }
}

impl TextEngine {
    /// Creates a text engine.
    #[must_use]
    pub fn new(fonts: FontDatabase) -> Self {
        Self {
            fonts,
            metrics: TextMetrics::default(),
            measure_cache: BTreeMap::new(),
            max_cache_entries: 2048,
        }
    }

    /// Font database.
    #[must_use]
    pub fn fonts(&self) -> &FontDatabase {
        &self.fonts
    }

    /// Mutable font database.
    pub fn fonts_mut(&mut self) -> &mut FontDatabase {
        &mut self.fonts
    }

    /// Metrics.
    #[must_use]
    pub fn metrics(&self) -> TextMetrics {
        self.metrics.clone()
    }

    /// Shapes text into glyph runs.
    pub fn shape_text(&mut self, text: &str, style: &TextStyle) -> ShapedText {
        self.metrics.shape_calls = self.metrics.shape_calls.saturating_add(1);

        let transformed = transform_text(text, style.transform);
        let normalized = normalize_whitespace(&transformed, style.white_space);
        let font_size = sanitize_px(style.font_size).max(1.0);
        let fallback_face = self.fonts.default_face().unwrap_or(FontFaceId(0));

        let mut runs = Vec::<GlyphRun>::new();
        let mut current_font = None::<FontFaceId>;
        let mut current_clusters = Vec::<GlyphCluster>::new();
        let mut current_advance = 0.0;
        let mut total_advance = 0.0;

        for (byte_index, ch) in normalized.char_indices() {
            let font = self
                .fonts
                .match_face(&style.font, ch)
                .unwrap_or(fallback_face);

            if current_font.is_some_and(|id| id != font) && !current_clusters.is_empty() {
                runs.push(GlyphRun {
                    font: current_font.unwrap_or(font),
                    clusters: std::mem::take(&mut current_clusters),
                    advance: current_advance,
                    direction: style.direction,
                });
                current_advance = 0.0;
            }

            if current_font != Some(font) {
                self.metrics.fallback_decisions = self.metrics.fallback_decisions.saturating_add(1);
                current_font = Some(font);
            }

            let script = classify_script(ch);
            let face = self.fonts.face(font);
            let metrics = face.map_or(FontMetrics::default(), |face| face.metrics);
            let hard_break = ch == '\n';
            let whitespace = ch.is_whitespace() && !hard_break;
            let advance = if hard_break {
                0.0
            } else {
                glyph_advance(
                    ch,
                    metrics,
                    font_size,
                    style.letter_spacing,
                    style.word_spacing,
                )
            };

            let end = byte_index + ch.len_utf8();

            current_clusters.push(GlyphCluster {
                byte_range: byte_index..end,
                text: ch.to_string(),
                font,
                glyph_id: glyph_id_for_char(ch),
                advance,
                x_offset: 0.0,
                y_offset: 0.0,
                script,
                whitespace,
                hard_break,
            });

            current_advance += advance;
            total_advance += advance;
            self.metrics.glyphs_emitted = self.metrics.glyphs_emitted.saturating_add(1);
        }

        if let Some(font) = current_font {
            if !current_clusters.is_empty() {
                runs.push(GlyphRun {
                    font,
                    clusters: current_clusters,
                    advance: current_advance,
                    direction: style.direction,
                });
            }
        }

        let primary = self
            .fonts
            .match_face(&style.font, 'A')
            .or_else(|| self.fonts.default_face());
        let primary_metrics = primary
            .and_then(|id| self.fonts.face(id))
            .map_or(FontMetrics::default(), |face| face.metrics);
        let line_height = style
            .line_height
            .map(sanitize_px)
            .unwrap_or_else(|| primary_metrics.natural_line_height(font_size));

        ShapedText {
            text: normalized,
            runs,
            advance: total_advance,
            font_size,
            line_height,
            baseline: primary_metrics.baseline(font_size),
        }
    }

    /// Measures text without line wrapping.
    pub fn measure_text(&mut self, text: &str, style: &TextStyle) -> TextMeasure {
        self.metrics.measure_calls = self.metrics.measure_calls.saturating_add(1);

        let key = TextCacheKey::from_text_style(text, style);

        if let Some(measure) = self.measure_cache.get(&key).copied() {
            self.metrics.cache_hits = self.metrics.cache_hits.saturating_add(1);
            return measure;
        }

        self.metrics.cache_misses = self.metrics.cache_misses.saturating_add(1);
        let shaped = self.shape_text(text, style);
        let primary = self
            .fonts
            .match_face(&style.font, 'A')
            .or_else(|| self.fonts.default_face());
        let metrics = primary
            .and_then(|id| self.fonts.face(id))
            .map_or(FontMetrics::default(), |face| face.metrics);

        let measure = TextMeasure {
            width: shaped.advance,
            height: shaped.line_height,
            baseline: shaped.baseline,
            ascent: metrics.ascent * shaped.font_size,
            descent: metrics.descent * shaped.font_size,
            line_gap: metrics.line_gap * shaped.font_size,
        };

        if self.measure_cache.len() >= self.max_cache_entries {
            if let Some(first) = self.measure_cache.keys().next().cloned() {
                self.measure_cache.remove(&first);
            }
        }

        self.measure_cache.insert(key, measure);
        measure
    }

    /// Lays out text into lines.
    pub fn layout_text(&mut self, text: &str, style: &TextStyle, max_width: f32) -> TextLayout {
        self.metrics.layout_calls = self.metrics.layout_calls.saturating_add(1);

        let shaped = self.shape_text(text, style);
        let max_width = sanitize_px(max_width);
        let wrap_enabled = !matches!(style.white_space, WhiteSpace::NoWrap | WhiteSpace::Pre);
        let mut lines = Vec::<TextLine>::new();
        let mut current = Vec::<GlyphCluster>::new();
        let mut current_width = 0.0;
        let mut current_text = String::new();
        let mut natural_width = 0.0;
        let mut overflowed = false;
        let mut line_index = 0usize;

        for cluster in shaped
            .runs
            .iter()
            .flat_map(|run| run.clusters.iter())
            .cloned()
        {
            if cluster.hard_break {
                self.metrics.hard_wraps = self.metrics.hard_wraps.saturating_add(1);
                push_line(
                    &mut lines,
                    line_index,
                    &mut current,
                    &mut current_text,
                    current_width,
                    &mut current_width,
                    &shaped,
                    style,
                    max_width,
                    false,
                );
                line_index += 1;
                continue;
            }

            let should_wrap = wrap_enabled
                && max_width > 0.0
                && current_width > 0.0
                && current_width + cluster.advance > max_width
                && can_break_before(&cluster, style.white_space);

            if should_wrap {
                self.metrics.soft_wraps = self.metrics.soft_wraps.saturating_add(1);
                push_line(
                    &mut lines,
                    line_index,
                    &mut current,
                    &mut current_text,
                    current_width,
                    &mut current_width,
                    &shaped,
                    style,
                    max_width,
                    false,
                );
                line_index += 1;
            }

            current_width += cluster.advance;
            current_text.push_str(&cluster.text);
            natural_width = f32::max(natural_width, current_width);
            current.push(cluster);
        }

        if !current.is_empty() || lines.is_empty() {
            push_line(
                &mut lines,
                line_index,
                &mut current,
                &mut current_text,
                current_width,
                &mut current_width,
                &shaped,
                style,
                max_width,
                false,
            );
        }

        if let Some(max_lines) = style.max_lines {
            if lines.len() > max_lines {
                lines.truncate(max_lines);
                overflowed = true;
            }
        }

        if max_width > 0.0 {
            for line in &mut lines {
                if line.width > max_width {
                    overflowed = true;
                    if style.overflow == TextOverflow::Ellipsis {
                        apply_ellipsis(
                            line,
                            style,
                            &mut self.metrics,
                            &self.fonts,
                            max_width,
                            shaped.font_size,
                        );
                    }
                }
                line.x = alignment_offset(style.align, style.direction, max_width, line.width);
            }
        }

        for (index, line) in lines.iter_mut().enumerate() {
            line.index = index;
            line.y = index as f32 * shaped.line_height;
        }

        self.metrics.lines_emitted = self
            .metrics
            .lines_emitted
            .saturating_add(lines.len() as u64);

        let width = if max_width > 0.0 {
            lines
                .iter()
                .map(|line| line.width.min(max_width))
                .fold(0.0, f32::max)
        } else {
            lines.iter().map(|line| line.width).fold(0.0, f32::max)
        };

        let height = lines.len() as f32 * shaped.line_height;

        TextLayout {
            first_baseline: shaped.baseline,
            lines,
            width,
            height,
            natural_width: natural_width.max(shaped.advance),
            overflowed,
            metrics: self.metrics(),
        }
    }

    /// Collects glyph atlas requests for shaped text.
    #[must_use]
    pub fn atlas_requests(&self, shaped: &ShapedText) -> Vec<GlyphAtlasRequest> {
        let mut seen = BTreeSet::new();
        let mut requests = Vec::new();

        for cluster in shaped.runs.iter().flat_map(|run| &run.clusters) {
            if cluster.hard_break || cluster.whitespace {
                continue;
            }

            let key = GlyphAtlasKey {
                font: cluster.font,
                glyph: cluster.glyph_id,
                size_px_x64: (shaped.font_size * 64.0).round().max(1.0) as u32,
            };

            if seen.insert(key) {
                requests.push(GlyphAtlasRequest {
                    key,
                    estimated_width: cluster.advance.max(1.0).ceil(),
                    estimated_height: shaped.line_height.ceil(),
                });
            }
        }

        requests
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TextCacheKey {
    text: String,
    families: Vec<String>,
    weight: u16,
    style: FontStyle,
    font_size_x64: u32,
    line_height_x64: Option<u32>,
    letter_spacing_x64: i32,
    word_spacing_x64: i32,
    white_space: WhiteSpace,
    transform: TextTransform,
}

impl TextCacheKey {
    fn from_text_style(text: &str, style: &TextStyle) -> Self {
        Self {
            text: text.to_owned(),
            families: style.font.families.clone(),
            weight: style.font.weight.sanitized().0,
            style: style.font.style,
            font_size_x64: quantize_px(style.font_size),
            line_height_x64: style.line_height.map(quantize_px),
            letter_spacing_x64: quantize_signed_px(style.letter_spacing),
            word_spacing_x64: quantize_signed_px(style.word_spacing),
            white_space: style.white_space,
            transform: style.transform,
        }
    }
}

/// One paint-positioned glyph.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    /// Glyph cluster.
    pub cluster: GlyphCluster,

    /// X coordinate.
    pub x: f32,

    /// Y coordinate baseline.
    pub y: f32,
}

/// Converts text layout into positioned glyph stream.
#[must_use]
pub fn positioned_glyphs(layout: &TextLayout) -> Vec<PositionedGlyph> {
    let mut output = Vec::new();

    for line in &layout.lines {
        let mut x = line.x;
        for cluster in &line.clusters {
            output.push(PositionedGlyph {
                cluster: cluster.clone(),
                x,
                y: line.y + line.baseline,
            });
            x += cluster.advance;
        }
    }

    output
}

/// Measures text using a temporary default engine.
#[must_use]
pub fn measure_text(text: &str, style: &TextStyle) -> TextMeasure {
    let mut engine = TextEngine::default();
    engine.measure_text(text, style)
}

/// Lays out text using a temporary default engine.
#[must_use]
pub fn layout_text(text: &str, style: &TextStyle, max_width: f32) -> TextLayout {
    let mut engine = TextEngine::default();
    engine.layout_text(text, style, max_width)
}

/// Shapes text using a temporary default engine.
#[must_use]
pub fn shape_text(text: &str, style: &TextStyle) -> ShapedText {
    let mut engine = TextEngine::default();
    engine.shape_text(text, style)
}

fn push_line(
    lines: &mut Vec<TextLine>,
    index: usize,
    current: &mut Vec<GlyphCluster>,
    current_text: &mut String,
    width: f32,
    current_width: &mut f32,
    shaped: &ShapedText,
    _style: &TextStyle,
    _max_width: f32,
    ellipsized: bool,
) {
    let text = current_text.trim_end_matches(' ').to_owned();
    let trimmed_width = trim_trailing_space_width(current, width);

    lines.push(TextLine {
        index,
        text,
        clusters: current.clone(),
        x: 0.0,
        y: index as f32 * shaped.line_height,
        width: trimmed_width,
        height: shaped.line_height,
        baseline: shaped.baseline,
        ellipsized,
    });

    current.clear();
    current_text.clear();
    *current_width = 0.0;
}

fn trim_trailing_space_width(clusters: &[GlyphCluster], width: f32) -> f32 {
    let trailing = clusters
        .iter()
        .rev()
        .take_while(|cluster| cluster.whitespace)
        .map(|cluster| cluster.advance)
        .sum::<f32>();

    (width - trailing).max(0.0)
}

fn apply_ellipsis(
    line: &mut TextLine,
    style: &TextStyle,
    metrics: &mut TextMetrics,
    fonts: &FontDatabase,
    max_width: f32,
    font_size: f32,
) {
    let ellipsis = '…';
    let font = fonts
        .match_face(&style.font, ellipsis)
        .or_else(|| fonts.default_face())
        .unwrap_or(FontFaceId(0));
    let face_metrics = fonts
        .face(font)
        .map_or(FontMetrics::default(), |face| face.metrics);
    let ellipsis_advance = glyph_advance(
        ellipsis,
        face_metrics,
        font_size,
        style.letter_spacing,
        style.word_spacing,
    );

    while !line.clusters.is_empty() && line.width + ellipsis_advance > max_width {
        if let Some(cluster) = line.clusters.pop() {
            line.width = (line.width - cluster.advance).max(0.0);
            let new_len = line.text.len().saturating_sub(cluster.text.len());
            line.text.truncate(new_len);
        }
    }

    line.clusters.push(GlyphCluster {
        byte_range: 0..ellipsis.len_utf8(),
        text: ellipsis.to_string(),
        font,
        glyph_id: glyph_id_for_char(ellipsis),
        advance: ellipsis_advance,
        x_offset: 0.0,
        y_offset: 0.0,
        script: ScriptClass::Common,
        whitespace: false,
        hard_break: false,
    });
    line.text.push(ellipsis);
    line.width = (line.width + ellipsis_advance).min(max_width);
    line.ellipsized = true;
    metrics.ellipsis_applications = metrics.ellipsis_applications.saturating_add(1);
}

fn can_break_before(cluster: &GlyphCluster, white_space: WhiteSpace) -> bool {
    match white_space {
        WhiteSpace::NoWrap | WhiteSpace::Pre => false,
        WhiteSpace::Normal | WhiteSpace::PreWrap | WhiteSpace::PreLine => {
            cluster.whitespace || matches!(cluster.script, ScriptClass::Cjk)
        }
    }
}

fn alignment_offset(
    align: TextAlign,
    direction: TextDirection,
    max_width: f32,
    line_width: f32,
) -> f32 {
    let remaining = (max_width - line_width).max(0.0);

    match (align, direction) {
        (TextAlign::Center, _) => remaining / 2.0,
        (TextAlign::Right | TextAlign::End, TextDirection::Ltr) => remaining,
        (TextAlign::Left | TextAlign::Start, TextDirection::Rtl) => remaining,
        _ => 0.0,
    }
}

fn glyph_advance(
    ch: char,
    metrics: FontMetrics,
    font_size: f32,
    letter_spacing: f32,
    word_spacing: f32,
) -> f32 {
    if ch == '\n' {
        return 0.0;
    }

    let base = if ch == ' ' || ch == '\t' {
        metrics.space_advance * font_size + word_spacing
    } else if is_cjk(ch) || is_emoji(ch) {
        font_size
    } else if ch.is_ascii_punctuation() {
        metrics.average_advance * font_size * 0.75
    } else if ch.is_ascii_digit() {
        metrics.average_advance * font_size * 0.95
    } else if ch.is_ascii_uppercase() {
        metrics.average_advance * font_size * 1.08
    } else {
        metrics.average_advance * font_size
    };

    (base + letter_spacing).max(0.0)
}

fn glyph_id_for_char(ch: char) -> GlyphId {
    GlyphId(u32::from(ch))
}

fn classify_script(ch: char) -> ScriptClass {
    if is_emoji(ch) {
        ScriptClass::Emoji
    } else if is_cjk(ch) {
        ScriptClass::Cjk
    } else if is_rtl(ch) {
        ScriptClass::Rtl
    } else if ch.is_ascii_alphabetic() || ('\u{0100}'..='\u{024F}').contains(&ch) {
        ScriptClass::Latin
    } else if ch.is_ascii() || ch.is_whitespace() || ch.is_ascii_punctuation() {
        ScriptClass::Common
    } else {
        ScriptClass::Unknown
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x3000..=0x30FF | 0x3400..=0x9FFF | 0xFF00..=0xFFEF
    )
}

fn is_rtl(ch: char) -> bool {
    matches!(u32::from(ch), 0x0590..=0x08FF | 0xFB1D..=0xFEFC)
}

fn is_emoji(ch: char) -> bool {
    matches!(u32::from(ch), 0x1F000..=0x1FAFF | 0x2600..=0x27BF)
}

fn normalize_family(input: &str) -> String {
    input
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase()
}

fn transform_text(text: &str, transform: TextTransform) -> String {
    match transform {
        TextTransform::None => text.to_owned(),
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::Capitalize => {
            let mut output = String::new();
            let mut new_word = true;

            for ch in text.chars() {
                if ch.is_whitespace() {
                    new_word = true;
                    output.push(ch);
                } else if new_word {
                    for upper in ch.to_uppercase() {
                        output.push(upper);
                    }
                    new_word = false;
                } else {
                    output.push(ch);
                }
            }

            output
        }
    }
}

fn normalize_whitespace(text: &str, white_space: WhiteSpace) -> String {
    match white_space {
        WhiteSpace::Pre | WhiteSpace::PreWrap => text.to_owned(),
        WhiteSpace::PreLine => collapse_spaces_preserve_newlines(text),
        WhiteSpace::Normal | WhiteSpace::NoWrap => collapse_all_whitespace(text),
    }
}

fn collapse_all_whitespace(text: &str) -> String {
    let mut output = String::new();
    let mut last_space = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_space {
                output.push(' ');
                last_space = true;
            }
        } else {
            output.push(ch);
            last_space = false;
        }
    }

    output.trim().to_owned()
}

fn collapse_spaces_preserve_newlines(text: &str) -> String {
    let mut output = String::new();
    let mut last_space = false;

    for ch in text.chars() {
        match ch {
            '\n' => {
                output.push('\n');
                last_space = false;
            }
            ch if ch.is_whitespace() => {
                if !last_space {
                    output.push(' ');
                    last_space = true;
                }
            }
            _ => {
                output.push(ch);
                last_space = false;
            }
        }
    }

    output
}

fn sanitize_px(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn quantize_px(value: f32) -> u32 {
    (sanitize_px(value) * 64.0).round().max(0.0) as u32
}

fn quantize_signed_px(value: f32) -> i32 {
    (value.clamp(-10000.0, 10000.0) * 64.0).round() as i32
}

/// Parses a CSS font-weight-ish value.
#[must_use]
pub fn parse_font_weight(value: &str) -> FontWeight {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => FontWeight::NORMAL,
        "bold" => FontWeight::BOLD,
        "lighter" => FontWeight(300),
        "bolder" => FontWeight(700),
        other => other
            .parse::<u16>()
            .map(FontWeight)
            .unwrap_or(FontWeight::NORMAL)
            .sanitized(),
    }
}

/// Parses a CSS font-style-ish value.
#[must_use]
pub fn parse_font_style(value: &str) -> FontStyle {
    match value.trim().to_ascii_lowercase().as_str() {
        "italic" => FontStyle::Italic,
        "oblique" => FontStyle::Oblique,
        _ => FontStyle::Normal,
    }
}

/// Parses a CSS white-space value.
#[must_use]
pub fn parse_white_space(value: &str) -> WhiteSpace {
    match value.trim().to_ascii_lowercase().as_str() {
        "nowrap" => WhiteSpace::NoWrap,
        "pre" => WhiteSpace::Pre,
        "pre-wrap" => WhiteSpace::PreWrap,
        "pre-line" => WhiteSpace::PreLine,
        _ => WhiteSpace::Normal,
    }
}

/// Parses a CSS text-overflow value.
#[must_use]
pub fn parse_text_overflow(value: &str) -> TextOverflow {
    match value.trim().to_ascii_lowercase().as_str() {
        "ellipsis" => TextOverflow::Ellipsis,
        _ => TextOverflow::Clip,
    }
}

/// Parses a CSS text-transform value.
#[must_use]
pub fn parse_text_transform(value: &str) -> TextTransform {
    match value.trim().to_ascii_lowercase().as_str() {
        "uppercase" => TextTransform::Uppercase,
        "lowercase" => TextTransform::Lowercase,
        "capitalize" => TextTransform::Capitalize,
        _ => TextTransform::None,
    }
}

/// Parses a CSS text-align value.
#[must_use]
pub fn parse_text_align(value: &str) -> TextAlign {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => TextAlign::Left,
        "center" => TextAlign::Center,
        "right" => TextAlign::Right,
        "end" => TextAlign::End,
        _ => TextAlign::Start,
    }
}
