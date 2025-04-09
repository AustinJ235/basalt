use std::sync::Arc;

mod vko {
    pub use vulkano::format::FormatFeatures;
    pub use vulkano::image::ImageType;
}

use crate::NonExhaustive;
use crate::image::ImageKey;
use crate::interface::{Bin, Color};

/// A unit with a corresponding value.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum UnitValue {
    /// Value is not defined.
    #[default]
    Undefined,
    /// Value is to be interpreted as pixels.
    Pixels(f32),
    /// Value is to be interpreted as a percent.
    Percent(f32),
    /// Value is to be interpreted as a percent with a pixel offset.
    PctOffsetPx(f32, f32),
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
            Self::Percent(pct) => Self::PctOffsetPx(pct, offset_px),
            Self::PctOffsetPx(pct, opx) => Self::PctOffsetPx(pct, opx + offset_px),
        }
    }

    /// Convert into pixels given an extent.
    ///
    /// **Note**: If [`Undefined`][`UnitValue::Undefined`] this returns [`None`].
    pub fn into_pixels(self, extent: f32) -> Option<f32> {
        match self {
            Self::Undefined => None,
            Self::Pixels(px) => Some(px),
            Self::Percent(pct) => Some(extent * (pct / 100.0)),
            Self::PctOffsetPx(pct, off_px) => Some((extent * (pct / 100.0)) + off_px),
        }
    }
}

/// Position type
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

/// Z-Index behavior
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZIndex {
    /// Z-index will be determinted automatically.
    #[default]
    Auto,
    /// Z-index will be set to a specific value.
    Fixed(i16),
    /// Z-index will be offset from the automatic value.
    Offset(i16),
}

/// Determintes order of floating targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FloatWeight {
    /// Float weight will be determinted by creation order.
    #[default]
    Auto,
    /// Float weight will be fixed.
    Fixed(i16),
}
/// How floating children `Bin` are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChildFloatMode {
    #[default]
    /// `Bin`'s will be placed left to right then down.
    Row,
    /// `Bin`'s will be placed top to bottom then right.
    Column,
}

/// How visiblity is determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// Inheirt visibility of the parent.
    ///
    /// **Note**: If there is no parent this will be [`Show`][`Visibility::Show`].
    #[default]
    Inheirt,
    /// Set the visibility to hidden.
    ///
    /// **Note**: This ignores the parent's visibility.
    Hide,
    /// Set the visibility to shown.
    ///
    /// **Note**: This ignores the parent's visibility.
    Show,
}

/// How opacity is determinted.
///
/// Opacity is a value between `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Opacity {
    /// Inheirt the opacity of the parent.
    ///
    /// **Note**: If there is no parent this will be [`Fixed(1.0)`][`Visibility::Fixed`].
    #[default]
    Inheirt,
    /// Set the opacity to a fixed value.
    ///
    /// **Note*: This ignores the parent's opacity.
    Fixed(f32),
    /// Multiply the parent's opacity by the provided value.
    Multiply(f32),
}

/// Set the region of the background image to use.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BackImageRegion {
    pub offset: [UnitValue; 2],
    pub extent: [UnitValue; 2],
}

/// Text wrap method used
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextWrap {
    Shift,
    #[default]
    Normal,
    None,
}

/// Text horizonal alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextHoriAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Text vertical alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextVertAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineSpacing {
    HeightMult(f32),
    HeightMultAdd(f32, f32),
}

impl Default for LineSpacing {
    fn default() -> Self {
        Self::HeightMult(1.2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineLimit {
    #[default]
    None,
    Fixed(usize),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum FontFamily {
    #[default]
    Inheirt,
    Named(String),
}

/// Weight of a font
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

#[derive(Debug, Clone, PartialEq)]
pub struct TextBody {
    pub spans: Vec<TextSpan>,
    pub line_spacing: LineSpacing,
    pub line_limit: LineLimit,
    pub text_wrap: TextWrap,
    pub vert_align: TextVertAlign,
    pub hori_align: TextHoriAlign,
    pub base_attrs: TextAttrs,
    pub _ne: NonExhaustive,
}

impl Default for TextBody {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            line_spacing: Default::default(),
            line_limit: Default::default(),
            text_wrap: Default::default(),
            vert_align: Default::default(),
            hori_align: Default::default(),
            base_attrs: TextAttrs::default(),
            _ne: NonExhaustive(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    pub text: String,
    pub attrs: TextAttrs,
    pub _ne: NonExhaustive,
}

impl Default for TextSpan {
    fn default() -> Self {
        Self {
            text: String::new(),
            attrs: TextAttrs::default(),
            _ne: NonExhaustive(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextAttrs {
    pub color: Color,
    pub height: UnitValue,
    pub secret: bool,
    pub font_family: FontFamily,
    pub font_weight: FontWeight,
    pub font_stretch: FontStretch,
    pub font_style: FontStyle,
    pub _ne: NonExhaustive,
}

impl Default for TextAttrs {
    fn default() -> Self {
        Self {
            color: Color::black(),
            height: Default::default(),
            secret: false,
            font_family: Default::default(),
            font_weight: Default::default(),
            font_stretch: Default::default(),
            font_style: Default::default(),
            _ne: NonExhaustive(()),
        }
    }
}

/// Style of a `Bin`
#[derive(Clone)]
pub struct BinStyle {
    // Placement
    pub position: Position,
    pub z_index: ZIndex,
    pub pos_from_t: UnitValue,
    pub pos_from_b: UnitValue,
    pub pos_from_l: UnitValue,
    pub pos_from_r: UnitValue,
    pub width: UnitValue,
    pub height: UnitValue,
    // Visiblity & Opacity
    pub visibility: Visibility,
    pub opacity: Opacity,
    // Floating Properties
    pub child_float_mode: ChildFloatMode,
    pub float_weight: FloatWeight,
    // Margin
    pub margin_t: UnitValue,
    pub margin_b: UnitValue,
    pub margin_l: UnitValue,
    pub margin_r: UnitValue,
    // Padding
    pub padding_t: UnitValue,
    pub padding_b: UnitValue,
    pub padding_l: UnitValue,
    pub padding_r: UnitValue,
    // Scroll & Overflow
    pub scroll_y: f32,
    pub scroll_x: f32,
    pub overflow_y: bool,
    pub overflow_x: bool,
    // Border
    pub border_size_t: UnitValue,
    pub border_size_b: UnitValue,
    pub border_size_l: UnitValue,
    pub border_size_r: UnitValue,
    pub border_color_t: Color,
    pub border_color_b: Color,
    pub border_color_l: Color,
    pub border_color_r: Color,
    pub border_radius_tl: f32,
    pub border_radius_tr: f32,
    pub border_radius_bl: f32,
    pub border_radius_br: f32,
    // Background
    pub back_color: Color,
    pub back_image: ImageKey,
    pub back_image_region: BackImageRegion,
    pub back_image_effect: ImageEffect,
    // Text
    pub text: String,
    pub text_color: Option<Color>,
    pub text_height: Option<f32>,
    pub text_secret: Option<bool>,
    pub line_spacing: Option<f32>,
    pub line_limit: Option<usize>,
    pub text_wrap: Option<TextWrap>,
    pub text_vert_align: Option<TextVertAlign>,
    pub text_hori_align: Option<TextHoriAlign>,
    pub font_family: Option<String>,
    pub font_weight: Option<FontWeight>,
    pub font_stretch: Option<FontStretch>,
    pub font_style: Option<FontStyle>,
    // Misc
    pub custom_verts: Vec<BinVert>,
    pub _ne: NonExhaustive,
}

impl Default for BinStyle {
    fn default() -> Self {
        Self {
            position: Default::default(),
            z_index: Default::default(),
            child_float_mode: Default::default(),
            float_weight: Default::default(),
            visibility: Default::default(),
            opacity: Default::default(),
            pos_from_t: Default::default(),
            pos_from_b: Default::default(),
            pos_from_l: Default::default(),
            pos_from_r: Default::default(),
            width: Default::default(),
            height: Default::default(),
            margin_t: Default::default(),
            margin_b: Default::default(),
            margin_l: Default::default(),
            margin_r: Default::default(),
            padding_t: Default::default(),
            padding_b: Default::default(),
            padding_l: Default::default(),
            padding_r: Default::default(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            overflow_y: false,
            overflow_x: false,
            border_size_t: Default::default(),
            border_size_b: Default::default(),
            border_size_l: Default::default(),
            border_size_r: Default::default(),
            border_color_t: Default::default(),
            border_color_b: Default::default(),
            border_color_l: Default::default(),
            border_color_r: Default::default(),
            border_radius_tl: 0.0,
            border_radius_tr: 0.0,
            border_radius_bl: 0.0,
            border_radius_br: 0.0,
            back_color: Default::default(),
            back_image: Default::default(),
            back_image_region: Default::default(),
            back_image_effect: Default::default(),
            text: String::new(),
            text_color: None,
            text_height: None,
            text_secret: None,
            line_spacing: None,
            line_limit: None,
            text_wrap: None,
            text_vert_align: None,
            text_hori_align: None,
            font_family: None,
            font_weight: None,
            font_stretch: None,
            font_style: None,
            custom_verts: Vec::new(),
            _ne: NonExhaustive(()),
        }
    }
}

/// Error produced from an invalid style
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinStyleError {
    pub ty: BinStyleErrorType,
    pub desc: String,
}

impl std::fmt::Display for BinStyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: {}", self.ty, self.desc)
    }
}

/// Type of error produced from an invalid style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BinStyleErrorType {
    /// Two fields are conflicted only one must be set.
    ConflictingFields,
    /// Too many fields are defining an attribute.
    TooManyConstraints,
    /// Not enough fields are defining an attribute.
    NotEnoughConstraints,
    /// Provided Image isn't valid.
    InvalidImage,
}

impl std::fmt::Display for BinStyleErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ConflictingFields => write!(f, "Conflicting Fields"),
            Self::TooManyConstraints => write!(f, "Too Many Constraints"),
            Self::NotEnoughConstraints => write!(f, "Not Enough Constraints"),
            _ => write!(f, "Unknown"),
        }
    }
}

/// Warning produced for a suboptimal style
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinStyleWarn {
    pub ty: BinStyleWarnType,
    pub desc: String,
}

impl std::fmt::Display for BinStyleWarn {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: {}", self.ty, self.desc)
    }
}

/// Type of warning produced for a suboptimal style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BinStyleWarnType {
    /// Field is set, but isn't used or incompatible with other styles.
    UselessField,
}

impl std::fmt::Display for BinStyleWarnType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::UselessField => write!(f, "Useless Field"),
        }
    }
}

/// Validation result produced from updating a `BinStyle`
///
/// To remove the `#[must_use]` attribute, enable the `style_validation_debug_on_drop` feature.
/// This feature will call the `debug` method automatically if no other method was used.
#[cfg_attr(not(feature = "style_validation_debug_on_drop"), must_use)]
pub struct BinStyleValidation {
    errors: Vec<BinStyleError>,
    warnings: Vec<BinStyleWarn>,
    location: Option<String>,
    used: bool,
}

impl BinStyleValidation {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            location: None,
            used: false,
        }
    }

    #[track_caller]
    fn error<D: Into<String>>(&mut self, ty: BinStyleErrorType, desc: D) {
        self.errors.push(BinStyleError {
            ty,
            desc: desc.into(),
        });

        if self.location.is_none() {
            self.location = Some(format!("{}", std::panic::Location::caller()));
        }
    }

    #[track_caller]
    fn warning<D: Into<String>>(&mut self, ty: BinStyleWarnType, desc: D) {
        self.warnings.push(BinStyleWarn {
            ty,
            desc: desc.into(),
        });

        if self.location.is_none() {
            self.location = Some(format!("{}", std::panic::Location::caller()));
        }
    }

    /// Expect `BinStyle` provided to `style_update()` is valid panicking if that is not the case.
    pub fn expect_valid(mut self) {
        self.used = true;

        if !self.errors.is_empty() {
            let mut panic_msg = format!(
                "BinStyleValidation-Error {{ caller: {},",
                self.location.take().unwrap()
            );
            let error_count = self.errors.len();

            if error_count == 1 {
                panic_msg = format!(
                    "{} error: Error {{ ty: {}, desc: {} }} }}",
                    panic_msg, self.errors[0].ty, self.errors[0].desc
                );
            } else {
                for (i, error) in self.errors.iter().enumerate() {
                    if i == 0 {
                        panic_msg = format!(
                            "{} errors: [ Error {{ ty: {}, desc: {} }},",
                            panic_msg, error.ty, error.desc
                        );
                    } else if i == error_count - 1 {
                        panic_msg = format!(
                            "{} Error {{ ty: {}, desc: {} }} ] }}",
                            panic_msg, error.ty, error.desc
                        );
                    } else {
                        panic_msg = format!(
                            "{} Error {{ ty: {}, desc: {} }},",
                            panic_msg, error.ty, error.desc
                        );
                    }
                }
            }

            panic!("{}", panic_msg);
        }
    }

    /// Does the same thing as `expect_valid`, but in the case of no errors, it'll print pretty warnings to the terminal.
    pub fn expect_valid_debug_warn(mut self) {
        self.used = true;

        if self.errors.is_empty() {
            if !self.warnings.is_empty() {
                let mut msg = format!(
                    "BinStyleValidation-Warn: {}:\n",
                    self.location.take().unwrap()
                );

                for warning in &self.warnings {
                    msg = format!("{}  {}: {}\n", msg, warning.ty, warning.desc);
                }

                msg.pop();
                println!("{}", msg);
            }
        } else {
            self.expect_valid();
        }
    }

    /// Returns `true` if errors are present.
    pub fn errors_present(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Return an `Iterator` of `BinStyleError`
    ///
    /// ***Note:** This method should only be called once. As it move the errors out.*
    pub fn errors(&mut self) -> impl Iterator<Item = BinStyleError> {
        self.used = true;
        self.errors.split_off(0).into_iter()
    }

    /// Returns `true` if warnings are present.
    pub fn warnings_present(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Return an `Iterator` of `BinStyleWarn`
    ///
    /// ***Note:** This method should only be called once. As it move the warnings out.*
    pub fn warnings(&mut self) -> impl Iterator<Item = BinStyleWarn> {
        self.used = true;
        self.warnings.split_off(0).into_iter()
    }

    /// Acknowlege the style update may have not be successful and just print pretty errors/warnings to the terminal.
    pub fn debug(mut self) {
        self.debug_impl();
    }

    fn debug_impl(&mut self) {
        if self.used {
            return;
        }

        self.used = true;

        if !self.errors.is_empty() || !self.warnings.is_empty() {
            let mut msg = format!("BinStyleValidation: {}:\n", self.location.take().unwrap());

            for error in &self.errors {
                msg = format!("{}  Error {}: {}\n", msg, error.ty, error.desc);
            }

            for warning in &self.warnings {
                msg = format!("{}  Warning {}: {}\n", msg, warning.ty, warning.desc);
            }

            msg.pop();
            println!("{}", msg);
        }
    }
}

#[cfg(feature = "style_validation_debug_on_drop")]
impl Drop for BinStyleValidation {
    fn drop(&mut self) {
        self.debug_impl();
    }
}

macro_rules! useless_field {
    ($style:ident, $field:ident, $name:literal, $validation:ident) => {
        if $style.$field.is_defined() {
            $validation.warning(
                BinStyleWarnType::UselessField,
                format!("'{}' is defined, but is ignored.", $name),
            );
        }
    };
}

impl BinStyle {
    #[track_caller]
    pub(crate) fn validate(&self, bin: &Arc<Bin>) -> BinStyleValidation {
        let mut validation = BinStyleValidation::new();
        let has_parent = bin.hrchy.read().parent.is_some();

        match self.position {
            Position::Relative => {
                if self.float_weight != FloatWeight::Auto {
                    validation.warning(
                        BinStyleWarnType::UselessField,
                        "'float_weight' is `Fixed`, but is ignored.",
                    );
                }

                if validation.errors.is_empty() {
                    let pft = self.pos_from_t.is_defined();
                    let pfb = self.pos_from_b.is_defined();
                    let pfl = self.pos_from_l.is_defined();
                    let pfr = self.pos_from_r.is_defined();
                    let width = self.width.is_defined();
                    let height = self.height.is_defined();

                    match (pft, pfb, height) {
                        (true, true, true) => {
                            validation.error(
                                BinStyleErrorType::TooManyConstraints,
                                format!(
                                    "'pos_from_t', 'pos_from_b' & 'height' are defined, but only \
                                     two can be defined.",
                                ),
                            );
                        },
                        (true, false, false) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'pos_from_t' is defined, but `pos_from_b` or `height` must \
                                     also be defined.",
                                ),
                            );
                        },
                        (false, true, false) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'pos_from_b' is defined, but `pos_from_t` or `height` must \
                                     also be defined.",
                                ),
                            );
                        },
                        (false, false, true) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'height' is defined, but `pos_from_t` or `pos_from_b` must \
                                     also be defined.",
                                ),
                            );
                        },
                        _ => (),
                    }

                    match (pfl, pfr, width) {
                        (true, true, true) => {
                            validation.error(
                                BinStyleErrorType::TooManyConstraints,
                                format!(
                                    "'pos_from_t', 'pos_from_r' & 'width' are defined, but only \
                                     two can be defined.",
                                ),
                            );
                        },
                        (true, false, false) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'pos_from_l' is defined, but `pos_from_r` or `width` must \
                                     also be defined.",
                                ),
                            );
                        },
                        (false, true, false) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'pos_from_r' is defined, but `pos_from_l` or `width` must \
                                     also be defined.",
                                ),
                            );
                        },
                        (false, false, true) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'width' is defined, but `pos_from_l` or `pos_from_r` must \
                                     also be defined.",
                                ),
                            );
                        },
                        _ => (),
                    }
                }
            },
            Position::Floating => {
                useless_field!(self, pos_from_t, "pos_from_t", validation);
                useless_field!(self, pos_from_b, "pos_from_b", validation);
                useless_field!(self, pos_from_l, "pos_from_l", validation);
                useless_field!(self, pos_from_r, "pos_from_r", validation);

                if !has_parent {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "Floating Bin's must have a parent.",
                    );
                }

                if !self.width.is_defined() {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "'width' must be defined.",
                    );
                }

                if !self.height.is_defined() {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "'height' must be defined.",
                    );
                }
            },
            Position::Anchor => todo!(),
        }

        if !self.back_image.is_invalid() {
            if let Some(image_id) = self.back_image.as_vulkano_id() {
                match bin.basalt.device_resources_ref().image(image_id) {
                    Ok(image_state) => {
                        let image = image_state.image();

                        if image.image_type() != vko::ImageType::Dim2d {
                            validation.error(
                                BinStyleErrorType::InvalidImage,
                                "'ImageKey::vulkano_id' provided with 'back_image' must be 2d.",
                            );
                        }

                        if image.array_layers() != 1 {
                            validation.error(
                                BinStyleErrorType::InvalidImage,
                                "'ImageKey::vulkano_id' provided with 'back_image' must not have \
                                 array layers.",
                            );
                        }

                        if image.mip_levels() != 1 {
                            validation.error(
                                BinStyleErrorType::InvalidImage,
                                "'ImageKey::vulkano_id' provided with 'back_image' must not have \
                                 mip levels.",
                            );
                        }

                        if !image.format_features().contains(
                            vko::FormatFeatures::TRANSFER_DST
                                | vko::FormatFeatures::TRANSFER_SRC
                                | vko::FormatFeatures::SAMPLED_IMAGE
                                | vko::FormatFeatures::SAMPLED_IMAGE_FILTER_LINEAR,
                        ) {
                            validation.error(
                                BinStyleErrorType::InvalidImage,
                                "'ImageKey::vulkano_id' provided with 'back_image' must have a \
                                 format that supports, 'TRANSFER_DST`, `TRANSFER_SRC`, \
                                 `SAMPLED_IMAGE`, & `SAMPLED_IMAGE_FILTER_LINEAR`.",
                            );
                        }
                    },
                    Err(_) => {
                        validation.error(
                            BinStyleErrorType::InvalidImage,
                            "'ImageKey::vulkano_id' provided with 'back_image' must be created \
                             from 'Basalt::device_resources_ref()'.",
                        );
                    },
                };
            } else if self.back_image.is_image_cache() {
                if self.back_image.is_glyph() {
                    validation.error(
                        BinStyleErrorType::InvalidImage,
                        "'ImageKey::glyph' provided with 'back_image' can not be used.",
                    );
                } else if self.back_image.is_any_user()
                    && bin
                        .basalt
                        .image_cache_ref()
                        .obtain_image_info(&self.back_image)
                        .is_none()
                {
                    validation.error(
                        BinStyleErrorType::InvalidImage,
                        "'ImageKey::user' provided with 'back_image' must be preloaded into the \
                         `ImageCache`.",
                    );
                }
            } else {
                validation.error(
                    BinStyleErrorType::InvalidImage,
                    "'ImageKey' provided with 'back_image' must be valid.",
                );
            }
        }

        validation
    }
}

/// Effect used on the background image of a `Bin`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageEffect {
    #[default]
    None,
    BackColorAdd,
    BackColorBehind,
    BackColorSubtract,
    BackColorMultiply,
    BackColorDivide,
    GlyphWithColor,
    Invert,
}

impl ImageEffect {
    pub(crate) fn vert_type(&self) -> i32 {
        match *self {
            ImageEffect::None => 100,
            ImageEffect::BackColorAdd => 102,
            ImageEffect::BackColorBehind => 103,
            ImageEffect::BackColorSubtract => 104,
            ImageEffect::BackColorMultiply => 105,
            ImageEffect::BackColorDivide => 106,
            ImageEffect::Invert => 107,
            ImageEffect::GlyphWithColor => 108,
        }
    }
}

/// Custom vertex for `Bin`
///
/// Used for `BinStyle.custom_verts`
#[derive(Default, Clone, Debug, PartialEq)]
pub struct BinVert {
    pub position: (f32, f32, i16),
    pub color: Color,
}
