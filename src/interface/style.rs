/// A unit with a corresponding value.
///
/// **Default**: [`Undefined`](`UnitValue::Undefined`)
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum UnitValue {
    /// Value is not defined.
    #[default]
    Undefined,
    /// Value is pixels.
    Pixels(f32),
    /// Value is a percent.
    Percent(f32),
    /// Value is a percent offset by pixels.
    ///
    /// `PctOffset(PERCENT, OFFSET_PIXELS)`
    PctOffset(f32, f32),
    /// Value is a percent of the width.
    PctOfWidth(f32),
    /// Value is a percent of the height.
    PctOfHeight(f32),
    /// Value is a percent of the width offset by pixels.
    ///
    /// `PctOfWidthOffset(PERCENT_OF_WIDTH, OFFSET_PIXELS)`
    PctOfWidthOffset(f32, f32),
    /// Value is a percent of the height offset by pixels.
    ///
    /// `PctOfWidthOffset(PERCENT_OF_HEIGHT, OFFSET_PIXELS)`
    PctOfHeightOffset(f32, f32),
}

impl UnitValue {
    /// Returns `true` if `Self` is not [`Undefined`](`UnitValue::Undefined`).
    pub fn is_defined(&self) -> bool {
        *self != Self::Undefined
    }

    /// Apply a pixel offset.
    ///
    /// **Note**: If [`Undefined`][`UnitValue::Undefined`] this returns [`Undefined`][`UnitValue::Undefined`].
    pub fn offset_pixels(self, offset_px: f32) -> Self {
        match self {
            Self::Undefined => Self::Undefined,
            Self::Pixels(px) => Self::Pixels(px + offset_px),
            Self::Percent(pct) => Self::PctOffset(pct, offset_px),
            Self::PctOffset(pct, off) => Self::PctOffset(pct, off + offset_px),
            Self::PctOfWidth(pct) => Self::PctOfWidthOffset(pct, offset_px),
            Self::PctOfHeight(pct) => Self::PctOfHeightOffset(pct, offset_px),
            Self::PctOfWidthOffset(pct, off) => Self::PctOfWidthOffset(pct, off + offset_px),
            Self::PctOfHeightOffset(pct, off) => Self::PctOfHeightOffset(pct, off + offset_px),
        }
    }

    /// Convert into width as pixels given an extent.
    ///
    /// **Note**: If [`Undefined`][`UnitValue::Undefined`] this returns [`None`].
    pub fn px_width(self, extent: [f32; 2]) -> Option<f32> {
        match self {
            Self::Undefined => None,
            Self::Pixels(px) => Some(px),
            Self::Percent(pct) => Some(extent[0] * (pct / 100.0)),
            Self::PctOffset(pct, off) => Some((extent[0] * (pct / 100.0)) + off),
            Self::PctOfWidth(pct) => Some(extent[0] * (pct / 100.0)),
            Self::PctOfHeight(pct) => Some(extent[1] * (pct / 100.0)),
            Self::PctOfWidthOffset(pct, off) => Some((extent[0] * (pct / 100.0)) + off),
            Self::PctOfHeightOffset(pct, off) => Some((extent[1] * (pct / 100.0)) + off),
        }
    }

    /// Convert into height as pixels given an extent.
    ///
    /// **Note**: If [`Undefined`][`UnitValue::Undefined`] this returns [`None`].
    pub fn px_height(self, extent: [f32; 2]) -> Option<f32> {
        match self {
            Self::Undefined => None,
            Self::Pixels(px) => Some(px),
            Self::Percent(pct) => Some(extent[1] * (pct / 100.0)),
            Self::PctOffset(pct, off) => Some((extent[1] * (pct / 100.0)) + off),
            Self::PctOfWidth(pct) => Some(extent[0] * (pct / 100.0)),
            Self::PctOfHeight(pct) => Some(extent[1] * (pct / 100.0)),
            Self::PctOfWidthOffset(pct, off) => Some((extent[0] * (pct / 100.0)) + off),
            Self::PctOfHeightOffset(pct, off) => Some((extent[1] * (pct / 100.0)) + off),
        }
    }
}

/// Position type
///
/// **Default**: [`Relative`](`Position::Relative`)
///
/// See [`BinStyle`](struct.BinStyle.html#position--size) for more information.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Position {
    /// Position will be within the parent.
    #[default]
    Relative,
    /// Position will float within the parent.
    Floating,
    /// Position will anchor to the parent.
    Anchor,
}

/// A font family
///
/// **Default**: [`Inheirt`](`FontFamily::Inheirt`)
///
/// To set the interface default see [`set_default_font`](`crate::interface::Interface::set_default_font`).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FontFamily {
    #[default]
    Inheirt,
    Serif,
    SansSerif,
    Cursive,
    Fantasy,
    Monospace,
    Named(String),
}

impl FontFamily {
    pub(crate) fn as_cosmic(&self) -> Option<cosmic_text::Family> {
        match self {
            Self::Inheirt => None,
            Self::Serif => Some(cosmic_text::Family::Serif),
            Self::SansSerif => Some(cosmic_text::Family::SansSerif),
            Self::Cursive => Some(cosmic_text::Family::Cursive),
            Self::Fantasy => Some(cosmic_text::Family::Fantasy),
            Self::Monospace => Some(cosmic_text::Family::Monospace),
            Self::Named(named) => Some(cosmic_text::Family::Name(named.as_str())),
        }
    }

    pub fn named<N>(name: N) -> Self
    where
        N: Into<String>,
    {
        Self::Named(name.into())
    }
}

/// Weight of a font
///
/// **Default**: [`Inheirt`](`FontWeight::Inheirt`)
///
/// To set the interface default see [`set_default_font`](`crate::interface::Interface::set_default_font`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontWeight {
    #[default]
    Inheirt,
    Thin,
    ExtraLight,
    Light,
    Normal,
    Medium,
    Semibold,
    Bold,
    Extrabold,
    Black,
}

impl FontWeight {
    pub(crate) fn into_cosmic(self) -> Option<cosmic_text::Weight> {
        match self {
            Self::Inheirt => None,
            Self::Thin => Some(cosmic_text::Weight(100)),
            Self::ExtraLight => Some(cosmic_text::Weight(200)),
            Self::Light => Some(cosmic_text::Weight(300)),
            Self::Normal => Some(cosmic_text::Weight(400)),
            Self::Medium => Some(cosmic_text::Weight(500)),
            Self::Semibold => Some(cosmic_text::Weight(600)),
            Self::Bold => Some(cosmic_text::Weight(700)),
            Self::Extrabold => Some(cosmic_text::Weight(800)),
            Self::Black => Some(cosmic_text::Weight(900)),
        }
    }
}

/// Stretch of a font
///
/// **Default**: [`Inheirt`](`FontStretch::Inheirt`)
///
/// To set the interface default see [`set_default_font`](`crate::interface::Interface::set_default_font`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStretch {
    #[default]
    Inheirt,
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

impl FontStretch {
    pub(crate) fn into_cosmic(self) -> Option<cosmic_text::Stretch> {
        match self {
            Self::Inheirt => None,
            Self::UltraCondensed => Some(cosmic_text::Stretch::UltraCondensed),
            Self::ExtraCondensed => Some(cosmic_text::Stretch::ExtraCondensed),
            Self::Condensed => Some(cosmic_text::Stretch::Condensed),
            Self::SemiCondensed => Some(cosmic_text::Stretch::SemiCondensed),
            Self::Normal => Some(cosmic_text::Stretch::Normal),
            Self::SemiExpanded => Some(cosmic_text::Stretch::SemiExpanded),
            Self::Expanded => Some(cosmic_text::Stretch::Expanded),
            Self::ExtraExpanded => Some(cosmic_text::Stretch::ExtraExpanded),
            Self::UltraExpanded => Some(cosmic_text::Stretch::UltraExpanded),
        }
    }
}

/// Style of a font
///
/// **Default**: [`Inheirt`](`FontStyle::Inheirt`)
///
/// To set the interface default see [`set_default_font`](`crate::interface::Interface::set_default_font`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStyle {
    #[default]
    Inheirt,
    Normal,
    Italic,
    Oblique,
}

impl FontStyle {
    pub(crate) fn into_cosmic(self) -> Option<cosmic_text::Style> {
        match self {
            Self::Inheirt => None,
            Self::Normal => Some(cosmic_text::Style::Normal),
            Self::Italic => Some(cosmic_text::Style::Italic),
            Self::Oblique => Some(cosmic_text::Style::Oblique),
        }
    }
}
