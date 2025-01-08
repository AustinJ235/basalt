use std::sync::Arc;

mod vk {
    pub use vulkano::format::FormatFeatures;
    pub use vulkano::image::{Image, ImageType};
    pub use vulkano_taskgraph::Id;
}

use crate::image_cache::ImageCacheKey;
use crate::interface::{Bin, Color};
use crate::NonExhaustive;

/// Position of a `Bin`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BinPosition {
    /// Position will be done from the window's dimensions
    #[default]
    Window,
    /// Position will be done from the parent's dimensions
    Parent,
    /// Position will be done from the parent's dimensions
    /// and other siblings the same type.
    Floating,
}

/// How floating children `Bin` are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChildFloatMode {
    #[default]
    Row,
    Column,
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

/// Weight of a font
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontWeight {
    Thin,
    ExtraLight,
    Light,
    #[default]
    Normal,
    Medium,
    Semibold,
    Bold,
    Extrabold,
    Black,
}

impl From<FontWeight> for cosmic_text::Weight {
    fn from(weight: FontWeight) -> Self {
        Self(match weight {
            FontWeight::Thin => 100,
            FontWeight::ExtraLight => 200,
            FontWeight::Light => 300,
            FontWeight::Normal => 400,
            FontWeight::Medium => 500,
            FontWeight::Semibold => 600,
            FontWeight::Bold => 700,
            FontWeight::Extrabold => 800,
            FontWeight::Black => 900,
        })
    }
}

/// Stretch of a font
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    #[default]
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

impl From<FontStretch> for cosmic_text::Stretch {
    fn from(stretch: FontStretch) -> Self {
        match stretch {
            FontStretch::UltraCondensed => Self::UltraCondensed,
            FontStretch::ExtraCondensed => Self::ExtraCondensed,
            FontStretch::Condensed => Self::Condensed,
            FontStretch::SemiCondensed => Self::SemiCondensed,
            FontStretch::Normal => Self::Normal,
            FontStretch::SemiExpanded => Self::SemiExpanded,
            FontStretch::Expanded => Self::Expanded,
            FontStretch::ExtraExpanded => Self::ExtraExpanded,
            FontStretch::UltraExpanded => Self::UltraExpanded,
        }
    }
}

/// Style of a font
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

impl From<FontStyle> for cosmic_text::Style {
    fn from(style: FontStyle) -> Self {
        match style {
            FontStyle::Normal => Self::Normal,
            FontStyle::Italic => Self::Italic,
            FontStyle::Oblique => Self::Oblique,
        }
    }
}

/// Style of a `Bin`
#[derive(Clone)]
pub struct BinStyle {
    /// Determines the positioning type
    pub position: Option<BinPosition>,
    /// Overrides the z-index automatically calculated.
    pub z_index: Option<i16>,
    /// Offsets the z-index automatically calculated.
    pub add_z_index: Option<i16>,
    /// How children of this `Bin` float.
    pub child_float_mode: Option<ChildFloatMode>,
    /// The floating weight of this `Bin`.
    ///
    /// Lesser values will be left-most and greator values right-most in `ChildFloatMode::Row`.
    /// Likewise with `ChildFloatMode::Column` lesser is top-most and greator is bottom-most.
    ///
    /// ***Note:** When setting the weight explicitly, all other silbings's weights should be set
    /// to ensure that they are displayed as intended.*
    pub float_weight: Option<i16>,
    /// Determines if the `Bin` is hidden.
    /// - `None`: Inherited from the parent `Bin`.
    /// - `Some(true)`: Always hidden.
    /// - `Some(false)`: Always visible even when the parent is hidden.
    pub hidden: Option<bool>,
    /// Set the opacity of the bin's content.
    pub opacity: Option<f32>,
    // Position from Edges
    pub pos_from_t: Option<f32>,
    pub pos_from_b: Option<f32>,
    pub pos_from_l: Option<f32>,
    pub pos_from_r: Option<f32>,
    pub pos_from_t_pct: Option<f32>,
    pub pos_from_b_pct: Option<f32>,
    pub pos_from_l_pct: Option<f32>,
    pub pos_from_r_pct: Option<f32>,
    pub pos_from_l_offset: Option<f32>,
    pub pos_from_t_offset: Option<f32>,
    pub pos_from_r_offset: Option<f32>,
    pub pos_from_b_offset: Option<f32>,
    // Size
    pub width: Option<f32>,
    pub width_pct: Option<f32>,
    /// Used in conjunction with `width_pct` to provide additional flexibility
    pub width_offset: Option<f32>,
    pub height: Option<f32>,
    pub height_pct: Option<f32>,
    /// Used in conjunction with `height_pct` to provide additional flexibility
    pub height_offset: Option<f32>,
    pub margin_t: Option<f32>,
    pub margin_b: Option<f32>,
    pub margin_l: Option<f32>,
    pub margin_r: Option<f32>,
    // Padding
    pub pad_t: Option<f32>,
    pub pad_b: Option<f32>,
    pub pad_l: Option<f32>,
    pub pad_r: Option<f32>,
    // Scrolling
    pub scroll_y: Option<f32>,
    pub scroll_x: Option<f32>,
    pub overflow_y: Option<bool>,
    pub overflow_x: Option<bool>,
    // Border
    pub border_size_t: Option<f32>,
    pub border_size_b: Option<f32>,
    pub border_size_l: Option<f32>,
    pub border_size_r: Option<f32>,
    pub border_color_t: Option<Color>,
    pub border_color_b: Option<Color>,
    pub border_color_l: Option<Color>,
    pub border_color_r: Option<Color>,
    pub border_radius_tl: Option<f32>,
    pub border_radius_tr: Option<f32>,
    pub border_radius_bl: Option<f32>,
    pub border_radius_br: Option<f32>,
    // Background
    pub back_color: Option<Color>,
    pub back_image: Option<ImageCacheKey>,
    pub back_image_vk: Option<vk::Id<vk::Image>>,
    pub back_image_coords: Option<[f32; 4]>,
    pub back_image_effect: Option<ImageEffect>,
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
            position: None,
            z_index: None,
            add_z_index: None,
            child_float_mode: None,
            float_weight: None,
            hidden: None,
            opacity: None,
            pos_from_t: None,
            pos_from_b: None,
            pos_from_l: None,
            pos_from_r: None,
            pos_from_t_pct: None,
            pos_from_b_pct: None,
            pos_from_l_pct: None,
            pos_from_r_pct: None,
            pos_from_l_offset: None,
            pos_from_t_offset: None,
            pos_from_r_offset: None,
            pos_from_b_offset: None,
            width: None,
            width_pct: None,
            width_offset: None,
            height: None,
            height_pct: None,
            height_offset: None,
            margin_t: None,
            margin_b: None,
            margin_l: None,
            margin_r: None,
            pad_t: None,
            pad_b: None,
            pad_l: None,
            pad_r: None,
            scroll_y: None,
            scroll_x: None,
            overflow_y: None,
            overflow_x: None,
            border_size_t: None,
            border_size_b: None,
            border_size_l: None,
            border_size_r: None,
            border_color_t: None,
            border_color_b: None,
            border_color_l: None,
            border_color_r: None,
            border_radius_tl: None,
            border_radius_tr: None,
            border_radius_bl: None,
            border_radius_br: None,
            back_color: None,
            back_image: None,
            back_image_vk: None,
            back_image_coords: None,
            back_image_effect: None,
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
        if $style.$field.is_some() {
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
        let has_parent = bin.hrchy.load().parent.is_some();

        match self.position.unwrap_or(BinPosition::Window) {
            BinPosition::Window | BinPosition::Parent => {
                useless_field!(self, float_weight, "float_weight", validation);

                if self.pos_from_t.is_some() && self.pos_from_t_pct.is_some() {
                    validation.error(
                        BinStyleErrorType::ConflictingFields,
                        "Both 'pos_from_t' and 'pos_from_t_pct' are set.",
                    );
                }

                if self.pos_from_b.is_some() && self.pos_from_b_pct.is_some() {
                    validation.error(
                        BinStyleErrorType::ConflictingFields,
                        "Both 'pos_from_b' and 'pos_from_b_pct' are set.",
                    );
                }

                if self.pos_from_l.is_some() && self.pos_from_l_pct.is_some() {
                    validation.error(
                        BinStyleErrorType::ConflictingFields,
                        "Both 'pos_from_l' and 'pos_from_l_pct' are set.",
                    );
                }

                if self.pos_from_r.is_some() && self.pos_from_r_pct.is_some() {
                    validation.error(
                        BinStyleErrorType::ConflictingFields,
                        "Both 'pos_from_r' and 'pos_from_r_pct' are set.",
                    );
                }

                if self.width.is_some() && self.width_pct.is_some() {
                    validation.error(
                        BinStyleErrorType::ConflictingFields,
                        "Both 'width' and 'width_pct' are set.",
                    );
                }

                if self.height.is_some() && self.height_pct.is_some() {
                    validation.error(
                        BinStyleErrorType::ConflictingFields,
                        "Both 'height' and 'height_pct' are set.",
                    );
                }

                if validation.errors.is_empty() {
                    let pft = self.pos_from_t.is_some() || self.pos_from_t_pct.is_some();
                    let pfb = self.pos_from_b.is_some() || self.pos_from_b_pct.is_some();
                    let pfl = self.pos_from_l.is_some() || self.pos_from_l_pct.is_some();
                    let pfr = self.pos_from_r.is_some() || self.pos_from_r_pct.is_some();
                    let width = self.width.is_some() || self.width_pct.is_some();
                    let height = self.height.is_some() || self.height_pct.is_some();

                    match (pft, pfb, height) {
                        (true, true, true) => {
                            let pft_field = if self.pos_from_t.is_some() {
                                "pos_from_t"
                            } else {
                                "pos_from_t_pct"
                            };

                            let pfb_field = if self.pos_from_b.is_some() {
                                "pos_from_b"
                            } else {
                                "pos_from_b_pct"
                            };

                            let height_field = if self.height.is_some() {
                                "height"
                            } else {
                                "height_pct"
                            };

                            validation.error(
                                BinStyleErrorType::TooManyConstraints,
                                format!(
                                    "'{}', '{}' & '{}' are all defined. Only two can be defined.",
                                    pft_field, pfb_field, height_field,
                                ),
                            );
                        },
                        (true, false, false) => {
                            let pft_field = if self.pos_from_t.is_some() {
                                "pos_from_t"
                            } else {
                                "pos_from_t_pct"
                            };

                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'{}' is defined, but one of `pos_from_b`, `pos_from_b_pct`, \
                                     `height` or `height_pct` must also be defined.",
                                    pft_field,
                                ),
                            );
                        },
                        (false, true, false) => {
                            let pfb_field = if self.pos_from_b.is_some() {
                                "pos_from_b"
                            } else {
                                "pos_from_b_pct"
                            };

                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'{}' is defined, but one of `pos_from_t`, `pos_from_t_pct`, \
                                     `height` or `height_pct` must also be defined.",
                                    pfb_field,
                                ),
                            );
                        },
                        (false, false, true) => {
                            let height_field = if self.height.is_some() {
                                "height"
                            } else {
                                "height_pct"
                            };

                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'{}' is defined, but one of `pos_from_t`, `pos_from_t_pct`, \
                                     `pos_from_b` or `pos_from_b_pct` must also be defined.",
                                    height_field,
                                ),
                            );
                        },
                        _ => (),
                    }

                    match (pfl, pfr, width) {
                        (true, true, true) => {
                            let pfl_field = if self.pos_from_l.is_some() {
                                "pos_from_l"
                            } else {
                                "pos_from_l_pct"
                            };

                            let pfr_field = if self.pos_from_r.is_some() {
                                "pos_from_r"
                            } else {
                                "pos_from_r_pct"
                            };

                            let width_field = if self.width.is_some() {
                                "width"
                            } else {
                                "width_pct"
                            };

                            validation.error(
                                BinStyleErrorType::TooManyConstraints,
                                format!(
                                    "'{}', '{}' & '{}' are all defined. Only two can be defined.",
                                    pfl_field, pfr_field, width_field,
                                ),
                            );
                        },
                        (true, false, false) => {
                            let pfl_field = if self.pos_from_t.is_some() {
                                "pos_from_l"
                            } else {
                                "pos_from_l_pct"
                            };

                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'{}' is defined, but one of `pos_from_r`, `pos_from_r_pct`, \
                                     `width` or `width_pct` must also be defined.",
                                    pfl_field,
                                ),
                            );
                        },
                        (false, true, false) => {
                            let pfr_field = if self.pos_from_t.is_some() {
                                "pos_from_r"
                            } else {
                                "pos_from_r_pct"
                            };

                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'{}' is defined, but one of `pos_from_l`, `pos_from_l_pct`, \
                                     `width` or `width_pct` must also be defined.",
                                    pfr_field,
                                ),
                            );
                        },
                        (false, false, true) => {
                            let width_field = if self.pos_from_t.is_some() {
                                "width"
                            } else {
                                "width_pct"
                            };

                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                format!(
                                    "'{}' is defined, but one of `pos_from_l`, `pos_from_l_pct`, \
                                     `pos_from_r` or `pos_from_r_pct` must also be defined.",
                                    width_field,
                                ),
                            );
                        },
                        _ => (),
                    }
                }
            },
            BinPosition::Floating => {
                useless_field!(self, pos_from_t, "pos_from_t", validation);
                useless_field!(self, pos_from_b, "pos_from_b", validation);
                useless_field!(self, pos_from_l, "pos_from_l", validation);
                useless_field!(self, pos_from_r, "pos_from_r", validation);
                useless_field!(self, pos_from_t_pct, "pos_from_t_pct", validation);
                useless_field!(self, pos_from_b_pct, "pos_from_b_pct", validation);
                useless_field!(self, pos_from_l_pct, "pos_from_l_pct", validation);
                useless_field!(self, pos_from_r_pct, "pos_from_r_pct", validation);
                useless_field!(self, pos_from_t_offset, "pos_from_t_offset", validation);
                useless_field!(self, pos_from_t_offset, "pos_from_b_offset", validation);
                useless_field!(self, pos_from_t_offset, "pos_from_l_offset", validation);
                useless_field!(self, pos_from_t_offset, "pos_from_r_offset", validation);

                if !has_parent {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "Floating Bin's must have a parent.",
                    );
                }

                if self.width.is_none() && self.width_pct.is_none() {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "'width' or 'width_pct' must be defined.",
                    );
                }

                if self.height.is_none() && self.height_pct.is_none() {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "'height' or 'height_pct' must be defined.",
                    );
                }
            },
        }

        if self.back_image.is_some() && self.back_image_vk.is_some() {
            validation.error(
                BinStyleErrorType::ConflictingFields,
                "Both 'back_image' and 'back_image_vk' are set.",
            );
        }

        if let Some(image_id) = self.back_image_vk {
            match bin.basalt.device_resources_ref().image(image_id) {
                Ok(image_state) => {
                    let image = image_state.image();

                    if image.image_type() != vk::ImageType::Dim2d {
                        validation.error(
                            BinStyleErrorType::InvalidImage,
                            "Image provided with 'back_image_vk' isn't a 2d.",
                        );
                    }

                    if image.array_layers() != 1 {
                        validation.error(
                            BinStyleErrorType::InvalidImage,
                            "Image provided with 'back_image_vk' must not have array layers.",
                        );
                    }

                    if image.mip_levels() != 1 {
                        validation.error(
                            BinStyleErrorType::InvalidImage,
                            "Image provided with 'back_image_vk' must not have multiple mip \
                             levels.",
                        );
                    }

                    if !image.format_features().contains(
                        vk::FormatFeatures::TRANSFER_DST
                            | vk::FormatFeatures::TRANSFER_SRC
                            | vk::FormatFeatures::SAMPLED_IMAGE
                            | vk::FormatFeatures::SAMPLED_IMAGE_FILTER_LINEAR,
                    ) {
                        validation.error(
                            BinStyleErrorType::InvalidImage,
                            "Image provided with 'back_image_vk' must have a format that \
                             supports, 'TRANSFER_DST`, `TRANSFER_SRC`, `SAMPLED_IMAGE`, & \
                             `SAMPLED_IMAGE_FILTER_LINEAR`.",
                        );
                    }
                },
                Err(_) => {
                    validation.error(
                        BinStyleErrorType::InvalidImage,
                        "Image provided with 'back_image_vk' isn't valid.",
                    );
                },
            };
        }

        if let Some(image_cache_key) = self.back_image.as_ref() {
            if matches!(image_cache_key, ImageCacheKey::Glyph(..)) {
                validation.error(
                    BinStyleErrorType::InvalidImage,
                    "'ImageCacheKey' provided with 'back_image' must not be \
                     'ImageCacheKey::Glyph'. 'ImageCacheKey::User' should be used instead.",
                );
            }

            if matches!(image_cache_key, ImageCacheKey::User(..))
                && bin
                    .basalt
                    .image_cache_ref()
                    .obtain_image_info(image_cache_key.clone())
                    .is_none()
            {
                validation.error(
                    BinStyleErrorType::InvalidImage,
                    "'ImageCacheKey::User' provided with 'back_image' must be preloaded into the \
                     `ImageCache`.",
                );
            }
        }

        validation
    }
}

/// Effect used on the background image of a `Bin`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageEffect {
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
