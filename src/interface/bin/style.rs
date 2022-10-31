use std::sync::Arc;

use ilmenite::{ImtHoriAlign, ImtTextWrap, ImtVertAlign, ImtWeight};

use crate::atlas::{AtlasCacheCtrl, AtlasCoords};
use crate::image_view::BstImageView;
use crate::interface::Interface;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinPosition {
    /// Position will be done from the window's dimensions
    Window,
    /// Position will be done from the parent's dimensions
    Parent,
    /// Position will be done from the parent's dimensions
    /// and other siblings the same type.
    Floating,
}

impl Default for BinPosition {
    fn default() -> Self {
        BinPosition::Window
    }
}

#[derive(Default, Clone)]
pub struct BinStyle {
    /// Determines the positioning type
    pub position: Option<BinPosition>,
    /// Overrides the z-index automatically calculated.
    pub z_index: Option<i16>,
    /// Offsets the z-index automatically calculated.
    pub add_z_index: Option<i16>,
    /// Hides the bin, with None set parent will decide
    /// the visiblity, setting this explictely will ignore
    /// the parents visiblity.
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
    pub back_image: Option<String>,
    pub back_image_url: Option<String>,
    pub back_image_atlas: Option<AtlasCoords>,
    pub back_image_raw: Option<Arc<BstImageView>>,
    pub back_image_raw_coords: Option<AtlasCoords>,
    pub back_image_cache: Option<AtlasCacheCtrl>,
    pub back_image_effect: Option<ImageEffect>,
    // Text
    pub text: String,
    pub text_color: Option<Color>,
    pub text_height: Option<f32>,
    pub text_secret: Option<bool>,
    pub line_spacing: Option<f32>,
    pub line_limit: Option<usize>,
    pub text_wrap: Option<ImtTextWrap>,
    pub text_vert_align: Option<ImtVertAlign>,
    pub text_hori_align: Option<ImtHoriAlign>,
    pub font_family: Option<String>,
    pub font_weight: Option<ImtWeight>,
    pub custom_verts: Vec<BinVert>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinStyleErrorType {
    /// Two fields are conflicted only one must be set.
    ConflictingFields,
    /// Too many fields are defining an attribute.
    TooManyConstraints,
    /// Not enough fields are defining an attribute.
    NotEnoughConstraints,
    /// Requested font family & weight are not available.
    MissingFont,
}

impl std::fmt::Display for BinStyleErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ConflictingFields => write!(f, "Conflicting Fields"),
            Self::TooManyConstraints => write!(f, "Too Many Constraints"),
            Self::NotEnoughConstraints => write!(f, "Not Enough Constraints"),
            Self::MissingFont => write!(f, "Missing Font"),
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A struct representing errors and warnings returned by `style_update`.
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
    /// # Notes
    /// - This method should only be called once. As it move the errors out.
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
    /// # Notes
    /// - This method should only be called once. As it move the warnings out.
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
    pub(crate) fn validate(
        &self,
        interface: &Arc<Interface>,
        has_parent: bool,
    ) -> BinStyleValidation {
        let mut validation = BinStyleValidation::new();

        match self.position.unwrap_or(BinPosition::Window) {
            BinPosition::Window | BinPosition::Parent => {
                useless_field!(self, margin_t, "margin_t", validation);
                useless_field!(self, margin_b, "margin_b", validation);
                useless_field!(self, margin_l, "margin_l", validation);
                useless_field!(self, margin_r, "margin_r", validation);

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

        let mut back_image_defined = Vec::new();

        if self.back_image.is_some() {
            back_image_defined.push("back_image");
        }

        if self.back_image_url.is_some() {
            back_image_defined.push("back_image_url");
        }

        if self.back_image_atlas.is_some() {
            back_image_defined.push("back_image_atlas");
        }

        if self.back_image_raw.is_some() {
            back_image_defined.push("back_image_raw");
        }

        match back_image_defined.len() {
            0 => {
                useless_field!(
                    self,
                    back_image_raw_coords,
                    "back_image_raw_coords",
                    validation
                );

                useless_field!(self, back_image_cache, "back_image_cache", validation);
                useless_field!(self, back_image_effect, "back_image_effect", validation);
            },
            1 => {
                let back_color_has_effect = match self.back_image_effect {
                    Some(ImageEffect::Invert) | None => false,
                    Some(_) => true,
                };

                if !back_color_has_effect {
                    useless_field!(self, back_color, "back_color", validation);
                }

                if self.back_image_raw.is_none() {
                    useless_field!(
                        self,
                        back_image_raw_coords,
                        "back_image_raw_coords",
                        validation
                    );
                }

                if self.back_image_raw.is_some() || self.back_image_atlas.is_some() {
                    useless_field!(self, back_image_cache, "back_image_cache", validation);
                }
            },
            _ => {
                let mut fields = String::new();

                for (i, field) in back_image_defined.iter().enumerate() {
                    if i == 0 {
                        fields = format!("'{}'", field);
                    }
                    if i == back_image_defined.len() - 1 {
                        fields = format!("{} & '{}'", fields, field);
                    } else {
                        fields = format!("{}, '{}'", fields, field);
                    }
                }

                validation.error(
                    BinStyleErrorType::TooManyConstraints,
                    format!("{} are all defined. Only one can be defined.", fields),
                );
            },
        }

        if !self.text.is_empty() {
            match self.font_family.as_ref() {
                Some(font_family) => {
                    match self.font_weight {
                        Some(font_weight) => {
                            if !interface.has_font(font_family, self.font_weight.unwrap()) {
                                validation.error(
                                    BinStyleErrorType::MissingFont,
                                    format!(
                                        "Font family '{}' with weight of {:?} has not been loaded.",
                                        font_family, font_weight,
                                    ),
                                );
                            }
                        },
                        None => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                "When 'font_family' is defined, 'font_weight' must also be \
                                 defined.",
                            );
                        },
                    }
                },
                None => {
                    match interface.default_font() {
                        Some((default_family, _)) => {
                            if let Some(font_weight) = self.font_weight {
                                if !interface.has_font(&default_family, self.font_weight.unwrap()) {
                                    validation.error(
                                        BinStyleErrorType::MissingFont,
                                        format!(
                                            "Font family '{}'(default) with weight of {:?} has \
                                             not been loaded.",
                                            default_family, font_weight,
                                        ),
                                    );
                                }
                            }
                        },
                        None => {
                            validation.error(
                                BinStyleErrorType::MissingFont,
                                "No default font has been set.",
                            );
                        },
                    }
                },
            }
        }

        validation
    }
}

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

#[derive(Default, Clone, Debug, PartialEq)]
pub struct BinVert {
    pub position: (f32, f32, i16),
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub fn as_array(&self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    fn ffh(mut c1: u8, mut c2: u8) -> f32 {
        if (97..=102).contains(&c1) {
            c1 -= 87;
        } else if (65..=70).contains(&c1) {
            c1 -= 65;
        } else if (48..=57).contains(&c1) {
            c1 = c1.checked_sub(48).unwrap();
        } else {
            c1 = 0;
        }

        if (97..=102).contains(&c2) {
            c2 -= 87;
        } else if (65..=70).contains(&c2) {
            c2 -= 65;
        } else if (48..=57).contains(&c2) {
            c2 = c2.checked_sub(48).unwrap();
        } else {
            c2 = 0;
        }
        ((c1 * 16) + c2) as f32 / 255.0
    }

    pub fn clamp(&mut self) {
        if self.r > 1.0 {
            self.r = 1.0;
        } else if self.r < 0.0 {
            self.r = 0.0;
        }
        if self.g > 1.0 {
            self.g = 1.0;
        } else if self.g < 0.0 {
            self.g = 0.0;
        }
        if self.b > 1.0 {
            self.b = 1.0;
        } else if self.b < 0.0 {
            self.b = 0.0;
        }
        if self.a > 1.0 {
            self.a = 1.0;
        } else if self.a < 0.0 {
            self.a = 0.0;
        }
    }

    pub fn to_linear(&mut self) {
        self.r = f32::powf((self.r + 0.055) / 1.055, 2.4);
        self.g = f32::powf((self.g + 0.055) / 1.055, 2.4);
        self.b = f32::powf((self.b + 0.055) / 1.055, 2.4);
        self.a = f32::powf((self.a + 0.055) / 1.055, 2.4);
    }

    pub fn to_nonlinear(&mut self) {
        self.r = (self.r.powf(1.0 / 2.4) * 1.055) - 0.055;
        self.g = (self.g.powf(1.0 / 2.4) * 1.055) - 0.055;
        self.b = (self.b.powf(1.0 / 2.4) * 1.055) - 0.055;
        self.a = (self.a.powf(1.0 / 2.4) * 1.055) - 0.055;
    }

    pub fn srgb_hex(code: &str) -> Self {
        let mut color = Self::from_hex(code);
        color.to_linear();
        color
    }

    pub fn from_hex(code: &str) -> Self {
        let mut iter = code.bytes();
        let mut red = 0.0;
        let mut green = 0.0;
        let mut blue = 0.0;
        let mut alpha = 1.0;

        red = match iter.next() {
            Some(c1) => {
                match iter.next() {
                    Some(c2) => Self::ffh(c1, c2),
                    None => {
                        return Color {
                            r: red,
                            g: green,
                            b: blue,
                            a: alpha,
                        }
                    },
                }
            },
            None => {
                return Color {
                    r: red,
                    g: green,
                    b: blue,
                    a: alpha,
                }
            },
        };
        green = match iter.next() {
            Some(c1) => {
                match iter.next() {
                    Some(c2) => Self::ffh(c1, c2),
                    None => {
                        return Color {
                            r: red,
                            g: green,
                            b: blue,
                            a: alpha,
                        }
                    },
                }
            },
            None => {
                return Color {
                    r: red,
                    g: green,
                    b: blue,
                    a: alpha,
                }
            },
        };
        blue = match iter.next() {
            Some(c1) => {
                match iter.next() {
                    Some(c2) => Self::ffh(c1, c2),
                    None => {
                        return Color {
                            r: red,
                            g: green,
                            b: blue,
                            a: alpha,
                        }
                    },
                }
            },
            None => {
                return Color {
                    r: red,
                    g: green,
                    b: blue,
                    a: alpha,
                }
            },
        };
        alpha = match iter.next() {
            Some(c1) => {
                match iter.next() {
                    Some(c2) => Self::ffh(c1, c2),
                    None => {
                        return Color {
                            r: red,
                            g: green,
                            b: blue,
                            a: alpha,
                        }
                    },
                }
            },
            None => {
                return Color {
                    r: red,
                    g: green,
                    b: blue,
                    a: alpha,
                }
            },
        };

        Color {
            r: red,
            g: green,
            b: blue,
            a: alpha,
        }
    }
}
