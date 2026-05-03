#![doc = "CSS-lite box-model structures for Sylphos presentation layout."]

use crate::Color;

/// Four-sided CSS edge values in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EdgeSizes {
    /// Top edge.
    pub top: f32,

    /// Right edge.
    pub right: f32,

    /// Bottom edge.
    pub bottom: f32,

    /// Left edge.
    pub left: f32,
}

impl EdgeSizes {
    /// Creates four-sided edge sizes.
    #[must_use]
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Creates equal edge sizes on all sides.
    #[must_use]
    pub const fn all(value: f32) -> Self {
        Self::new(value, value, value, value)
    }

    /// Returns zero edge sizes.
    #[must_use]
    pub const fn zero() -> Self {
        Self::all(0.0)
    }

    /// Returns left + right.
    #[must_use]
    pub fn horizontal(self) -> f32 {
        self.left + self.right
    }

    /// Returns top + bottom.
    #[must_use]
    pub fn vertical(self) -> f32 {
        self.top + self.bottom
    }

    /// Returns true when all sides are effectively zero.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.top.abs() < f32::EPSILON
            && self.right.abs() < f32::EPSILON
            && self.bottom.abs() < f32::EPSILON
            && self.left.abs() < f32::EPSILON
    }

    /// Returns a sanitized copy with finite, non-negative values.
    #[must_use]
    pub fn sanitized(self) -> Self {
        Self {
            top: sanitize_edge(self.top),
            right: sanitize_edge(self.right),
            bottom: sanitize_edge(self.bottom),
            left: sanitize_edge(self.left),
        }
    }
}

/// Tiny subset of CSS `display` used by Sylphos layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayLite {
    /// Element contributes a block box.
    Block,

    /// Element contributes inline-ish text but is still blockified by the current renderer.
    Inline,

    /// Element is omitted from layout.
    None,
}

impl Default for DisplayLite {
    fn default() -> Self {
        Self::Block
    }
}

/// Optional box styling extracted from a CSS-lite selector.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BoxStyleLite {
    /// Optional margin edges.
    pub margin: Option<EdgeSizes>,

    /// Optional padding edges.
    pub padding: Option<EdgeSizes>,

    /// Optional border widths.
    pub border_width: Option<EdgeSizes>,

    /// Optional border color.
    pub border_color: Option<Color>,

    /// Optional background color for the box.
    pub background: Option<Color>,

    /// Optional display mode.
    pub display: Option<DisplayLite>,
}

impl BoxStyleLite {
    /// Returns `true` when the style has no supported properties.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.margin.is_none()
            && self.padding.is_none()
            && self.border_width.is_none()
            && self.border_color.is_none()
            && self.background.is_none()
            && self.display.is_none()
    }

    /// Merges another optional style into this one using later-rule-wins behavior.
    pub fn merge_from(&mut self, other: Self) {
        if other.margin.is_some() {
            self.margin = other.margin;
        }
        if other.padding.is_some() {
            self.padding = other.padding;
        }
        if other.border_width.is_some() {
            self.border_width = other.border_width;
        }
        if other.border_color.is_some() {
            self.border_color = other.border_color;
        }
        if other.background.is_some() {
            self.background = other.background;
        }
        if other.display.is_some() {
            self.display = other.display;
        }
    }

    /// Computes concrete box values.
    #[must_use]
    pub fn compute(self, fallback_border_color: Color) -> ComputedBoxStyle {
        let border_width = self
            .border_width
            .unwrap_or_else(EdgeSizes::zero)
            .sanitized();
        let border_color = if border_width.is_zero() {
            None
        } else {
            Some(self.border_color.unwrap_or(fallback_border_color))
        };

        ComputedBoxStyle {
            margin: self.margin.unwrap_or_else(EdgeSizes::zero).sanitized(),
            padding: self.padding.unwrap_or_else(EdgeSizes::zero).sanitized(),
            border_width,
            border_color,
            background: self.background,
            display: self.display.unwrap_or_default(),
        }
    }
}

/// Fixed selector slots for the CSS-lite box model.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BoxModelStylesLite {
    /// Body box style.
    pub body: BoxStyleLite,

    /// Paragraph box style.
    pub paragraph: BoxStyleLite,

    /// Link box style.
    pub link: BoxStyleLite,

    /// Image box style.
    pub image: BoxStyleLite,

    /// Form container style.
    pub form: BoxStyleLite,

    /// Text input style.
    pub input: BoxStyleLite,

    /// Button style.
    pub button: BoxStyleLite,

    /// Textarea style.
    pub textarea: BoxStyleLite,

    /// Div/generic container style.
    pub div: BoxStyleLite,

    /// Heading box styles for h1 through h6.
    pub headings: [BoxStyleLite; 6],
}

impl BoxModelStylesLite {
    /// Returns `true` when no selector has supported box-model data.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.body.is_empty()
            && self.paragraph.is_empty()
            && self.link.is_empty()
            && self.image.is_empty()
            && self.form.is_empty()
            && self.input.is_empty()
            && self.button.is_empty()
            && self.textarea.is_empty()
            && self.div.is_empty()
            && self.headings.iter().all(|style| style.is_empty())
    }

    /// Merges another box-model sheet into this one.
    pub fn merge_from(&mut self, other: Self) {
        self.body.merge_from(other.body);
        self.paragraph.merge_from(other.paragraph);
        self.link.merge_from(other.link);
        self.image.merge_from(other.image);
        self.form.merge_from(other.form);
        self.input.merge_from(other.input);
        self.button.merge_from(other.button);
        self.textarea.merge_from(other.textarea);
        self.div.merge_from(other.div);

        for (target, source) in self.headings.iter_mut().zip(other.headings) {
            target.merge_from(source);
        }
    }

    /// Computes concrete box styles.
    #[must_use]
    pub fn compute(self, fallback_border_color: Color) -> ComputedBoxModelStyles {
        ComputedBoxModelStyles {
            body: self.body.compute(fallback_border_color),
            paragraph: self.paragraph.compute(fallback_border_color),
            link: self.link.compute(fallback_border_color),
            image: self.image.compute(fallback_border_color),
            form: self.form.compute(fallback_border_color),
            input: self.input.compute(fallback_border_color),
            button: self.button.compute(fallback_border_color),
            textarea: self.textarea.compute(fallback_border_color),
            div: self.div.compute(fallback_border_color),
            headings: self
                .headings
                .map(|style| style.compute(fallback_border_color)),
        }
    }
}

/// Concrete box style used by layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputedBoxStyle {
    /// Concrete margin edges.
    pub margin: EdgeSizes,

    /// Concrete padding edges.
    pub padding: EdgeSizes,

    /// Concrete border widths.
    pub border_width: EdgeSizes,

    /// Concrete border color when any border side is visible.
    pub border_color: Option<Color>,

    /// Concrete background color when set.
    pub background: Option<Color>,

    /// Concrete display mode.
    pub display: DisplayLite,
}

impl Default for ComputedBoxStyle {
    fn default() -> Self {
        Self {
            margin: EdgeSizes::zero(),
            padding: EdgeSizes::zero(),
            border_width: EdgeSizes::zero(),
            border_color: None,
            background: None,
            display: DisplayLite::Block,
        }
    }
}

impl ComputedBoxStyle {
    /// Returns true when this style should not be laid out.
    #[must_use]
    pub const fn is_display_none(self) -> bool {
        matches!(self.display, DisplayLite::None)
    }
}

/// Computed box-model selector slots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputedBoxModelStyles {
    /// Body style.
    pub body: ComputedBoxStyle,

    /// Paragraph style.
    pub paragraph: ComputedBoxStyle,

    /// Link style.
    pub link: ComputedBoxStyle,

    /// Image style.
    pub image: ComputedBoxStyle,

    /// Form container style.
    pub form: ComputedBoxStyle,

    /// Text input style.
    pub input: ComputedBoxStyle,

    /// Button style.
    pub button: ComputedBoxStyle,

    /// Textarea style.
    pub textarea: ComputedBoxStyle,

    /// Div/generic container style.
    pub div: ComputedBoxStyle,

    /// Heading styles.
    pub headings: [ComputedBoxStyle; 6],
}

impl Default for ComputedBoxModelStyles {
    fn default() -> Self {
        let default = ComputedBoxStyle::default();
        Self {
            body: default,
            paragraph: default,
            link: default,
            image: default,
            form: default,
            input: default,
            button: default,
            textarea: default,
            div: default,
            headings: [default; 6],
        }
    }
}

fn sanitize_edge(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}
