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
    pub back_srgb_yuv: Option<bool>,
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
    /// Field is set, but isn't used or incompatible with other styles.
    WarnUselessField,
}

impl std::fmt::Display for BinStyleErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ConflictingFields => write!(f, "Conflicting Fields"),
            Self::TooManyConstraints => write!(f, "Too Many Constraints"),
            Self::NotEnoughConstraints => write!(f, "Not Enough Constraints"),
            Self::MissingFont => write!(f, "Missing Font"),
            Self::WarnUselessField => write!(f, "Warning Useless Field"),
        }
    }
}

macro_rules! useless_field {
    ($style:ident, $field:ident, $name:literal, $warnings:ident) => {
        if $style.$field.is_some() {
            $warnings.push(BinStyleError {
                ty: BinStyleErrorType::WarnUselessField,
                desc: format!("'{}' is defined, but is ignored.", $name),
            });
        }
    };
}

impl BinStyle {
    pub(crate) fn validate(
        &self,
        interface: &Arc<Interface>,
        has_parent: bool,
    ) -> Result<Vec<BinStyleError>, Vec<BinStyleError>> {
        let mut errors: Vec<BinStyleError> = Vec::new();
        let mut warnings: Vec<BinStyleError> = Vec::new();

        match self.position.unwrap_or(BinPosition::Window) {
            BinPosition::Window | BinPosition::Parent => {
                useless_field!(self, margin_t, "margin_t", warnings);
                useless_field!(self, margin_b, "margin_b", warnings);
                useless_field!(self, margin_l, "margin_l", warnings);
                useless_field!(self, margin_r, "margin_r", warnings);

                if self.pos_from_t.is_some() && self.pos_from_t_pct.is_some() {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::ConflictingFields,
                        desc: "Both 'pos_from_t' and 'pos_from_t_pct' are set.".to_string(),
                    });
                }

                if self.pos_from_b.is_some() && self.pos_from_b_pct.is_some() {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::ConflictingFields,
                        desc: "Both 'pos_from_b' and 'pos_from_b_pct' are set.".to_string(),
                    });
                }

                if self.pos_from_l.is_some() && self.pos_from_l_pct.is_some() {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::ConflictingFields,
                        desc: "Both 'pos_from_l' and 'pos_from_l_pct' are set.".to_string(),
                    });
                }

                if self.pos_from_r.is_some() && self.pos_from_r_pct.is_some() {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::ConflictingFields,
                        desc: "Both 'pos_from_r' and 'pos_from_r_pct' are set.".to_string(),
                    });
                }

                if self.width.is_some() && self.width_pct.is_some() {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::ConflictingFields,
                        desc: "Both 'width' and 'width_pct' are set.".to_string(),
                    });
                }

                if self.height.is_some() && self.height_pct.is_some() {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::ConflictingFields,
                        desc: "Both 'height' and 'height_pct' are set.".to_string(),
                    });
                }

                if errors.is_empty() {
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

                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::TooManyConstraints,
                                desc: format!(
                                    "'{}', '{}' & '{}' are all defined. Only two can be defined.",
                                    pft_field, pfb_field, height_field,
                                ),
                            });
                        },
                        (true, false, false) => {
                            let pft_field = if self.pos_from_t.is_some() {
                                "pos_from_t"
                            } else {
                                "pos_from_t_pct"
                            };

                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::NotEnoughConstraints,
                                desc: format!(
                                    "'{}' is defined, but one of `pos_from_b`, `pos_from_b_pct`, \
                                     `height` or `height_pct` must also be defined.",
                                    pft_field,
                                ),
                            });
                        },
                        (false, true, false) => {
                            let pfb_field = if self.pos_from_b.is_some() {
                                "pos_from_b"
                            } else {
                                "pos_from_b_pct"
                            };

                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::NotEnoughConstraints,
                                desc: format!(
                                    "'{}' is defined, but one of `pos_from_t`, `pos_from_t_pct`, \
                                     `height` or `height_pct` must also be defined.",
                                    pfb_field,
                                ),
                            });
                        },
                        (false, false, true) => {
                            let height_field = if self.height.is_some() {
                                "height"
                            } else {
                                "height_pct"
                            };

                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::NotEnoughConstraints,
                                desc: format!(
                                    "'{}' is defined, but one of `pos_from_t`, `pos_from_t_pct`, \
                                     `pos_from_b` or `pos_from_b_pct` must also be defined.",
                                    height_field,
                                ),
                            });
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

                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::TooManyConstraints,
                                desc: format!(
                                    "'{}', '{}' & '{}' are all defined. Only two can be defined.",
                                    pfl_field, pfr_field, width_field,
                                ),
                            });
                        },
                        (true, false, false) => {
                            let pfl_field = if self.pos_from_t.is_some() {
                                "pos_from_l"
                            } else {
                                "pos_from_l_pct"
                            };

                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::NotEnoughConstraints,
                                desc: format!(
                                    "'{}' is defined, but one of `pos_from_r`, `pos_from_r_pct`, \
                                     `width` or `width_pct` must also be defined.",
                                    pfl_field,
                                ),
                            });
                        },
                        (false, true, false) => {
                            let pfr_field = if self.pos_from_t.is_some() {
                                "pos_from_r"
                            } else {
                                "pos_from_r_pct"
                            };

                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::NotEnoughConstraints,
                                desc: format!(
                                    "'{}' is defined, but one of `pos_from_l`, `pos_from_l_pct`, \
                                     `width` or `width_pct` must also be defined.",
                                    pfr_field,
                                ),
                            });
                        },
                        (false, false, true) => {
                            let width_field = if self.pos_from_t.is_some() {
                                "width"
                            } else {
                                "width_pct"
                            };

                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::NotEnoughConstraints,
                                desc: format!(
                                    "'{}' is defined, but one of `pos_from_l`, `pos_from_l_pct`, \
                                     `pos_from_r` or `pos_from_r_pct` must also be defined.",
                                    width_field,
                                ),
                            });
                        },
                        _ => (),
                    }
                }
            },
            BinPosition::Floating => {
                useless_field!(self, pos_from_t, "pos_from_t", warnings);
                useless_field!(self, pos_from_b, "pos_from_b", warnings);
                useless_field!(self, pos_from_l, "pos_from_l", warnings);
                useless_field!(self, pos_from_r, "pos_from_r", warnings);
                useless_field!(self, pos_from_t_pct, "pos_from_t_pct", warnings);
                useless_field!(self, pos_from_b_pct, "pos_from_b_pct", warnings);
                useless_field!(self, pos_from_l_pct, "pos_from_l_pct", warnings);
                useless_field!(self, pos_from_r_pct, "pos_from_r_pct", warnings);
                useless_field!(self, pos_from_t_offset, "pos_from_t_offset", warnings);
                useless_field!(self, pos_from_t_offset, "pos_from_b_offset", warnings);
                useless_field!(self, pos_from_t_offset, "pos_from_l_offset", warnings);
                useless_field!(self, pos_from_t_offset, "pos_from_r_offset", warnings);

                if !has_parent {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::NotEnoughConstraints,
                        desc: "Floating Bin's must have a parent.".to_string(),
                    });
                }

                if self.width.is_none() || self.width_pct.is_none() {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::NotEnoughConstraints,
                        desc: "'width' or 'width_pct' must be defined.".to_string(),
                    });
                }

                if self.height.is_none() || self.height_pct.is_none() {
                    errors.push(BinStyleError {
                        ty: BinStyleErrorType::NotEnoughConstraints,
                        desc: "'height' or 'height_pct' must be defined.".to_string(),
                    });
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

        if self.back_image_url.is_some() {
            back_image_defined.push("back_image_url");
        }

        if back_image_defined.len() > 1 {
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

            errors.push(BinStyleError {
                ty: BinStyleErrorType::TooManyConstraints,
                desc: format!("{} are all defined. Only one can be defined.", fields),
            });
        } else if back_image_defined.len() == 1 {
            let back_color_has_effect = match self.back_image_effect {
                Some(ImageEffect::Invert) | None => false,
                Some(_) => true,
            };

            if !back_color_has_effect {
                useless_field!(self, back_color, "back_color", warnings);
            }

            if self.back_image_raw.is_none() {
                useless_field!(
                    self,
                    back_image_raw_coords,
                    "back_image_raw_coords",
                    warnings
                );
            }

            if self.back_image_raw.is_some() || self.back_image_atlas.is_some() {
                useless_field!(self, back_image_cache, "back_image_cache", warnings);
            }
        } else {
            useless_field!(
                self,
                back_image_raw_coords,
                "back_image_raw_coords",
                warnings
            );
            useless_field!(self, back_image_cache, "back_image_cache", warnings);
            useless_field!(self, back_srgb_yuv, "back_srgb_yuv", warnings);
            useless_field!(self, back_image_effect, "back_image_effect", warnings);
        }

        if self.text.len() > 0 {
            match self.font_family.as_ref() {
                Some(font_family) => {
                    match self.font_weight {
                        Some(font_weight) => {
                            if !interface.has_font(font_family, self.font_weight.unwrap()) {
                                errors.push(BinStyleError {
                                    ty: BinStyleErrorType::MissingFont,
                                    desc: format!(
                                        "Font family '{}' with weight of {:?} has not been loaded.",
                                        font_family, font_weight,
                                    ),
                                });
                            }
                        },
                        None => {
                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::NotEnoughConstraints,
                                desc: "When 'font_family' is defined, 'font_weight' must also be \
                                       defined."
                                    .to_string(),
                            });
                        },
                    }
                },
                None => {
                    match interface.default_font() {
                        Some((default_family, _)) => {
                            if let Some(font_weight) = self.font_weight {
                                if !interface.has_font(&default_family, self.font_weight.unwrap()) {
                                    errors.push(BinStyleError {
                                        ty: BinStyleErrorType::MissingFont,
                                        desc: format!(
                                            "Font family '{}'(default) with weight of {:?} has \
                                             not been loaded.",
                                            default_family, font_weight,
                                        ),
                                    });
                                }
                            }
                        },
                        None => {
                            errors.push(BinStyleError {
                                ty: BinStyleErrorType::MissingFont,
                                desc: "No default font has been set.".to_string(),
                            });
                        },
                    }
                },
            }
        }

        if errors.is_empty() {
            Ok(warnings)
        } else {
            errors.append(&mut warnings);
            Err(errors)
        }
    }
}

impl BinStyle {
    pub fn is_floating_compatible(&self) -> Result<(), String> {
        if self.position != Some(BinPosition::Floating) {
            Err(String::from("'position' must be 'BinPosition::Floating'."))
        } else if self.pos_from_t.is_some() {
            Err(String::from(
                "'pos_from_t' is not allowed or not implemented.",
            ))
        } else if self.pos_from_b.is_some() {
            Err(String::from(
                "'pos_from_b' is not allowed or not implemented.",
            ))
        } else if self.pos_from_l.is_some() {
            Err(String::from(
                "'pos_from_l' is not allowed or not implemented.",
            ))
        } else if self.pos_from_r.is_some() {
            Err(String::from(
                "'pos_from_r' is not allowed or not implemented.",
            ))
        } else if self.pos_from_t_pct.is_some() {
            Err(String::from(
                "'pos_from_t_pct' is not allowed or not implemented.",
            ))
        } else if self.pos_from_b_pct.is_some() {
            Err(String::from(
                "'pos_from_b_pct' is not allowed or not implemented.",
            ))
        } else if self.pos_from_l_pct.is_some() {
            Err(String::from(
                "'pos_from_l_pct' is not allowed or not implemented.",
            ))
        } else if self.pos_from_r_pct.is_some() {
            Err(String::from(
                "'pos_from_r_pct' is not allowed or not implemented.",
            ))
        } else if self.pos_from_l_offset.is_some() {
            Err(String::from(
                "'pos_from_l_offset' is not allowed or not implemented.",
            ))
        } else if self.pos_from_t_offset.is_some() {
            Err(String::from(
                "'pos_from_t_offset' is not allowed or not implemented.",
            ))
        } else if self.pos_from_r_offset.is_some() {
            Err(String::from(
                "'pos_from_r_offset' is not allowed or not implemented.",
            ))
        } else if self.pos_from_b_offset.is_some() {
            Err(String::from(
                "'pos_from_b_offset' is not allowed or not implemented.",
            ))
        } else if self.width.is_none() && self.width_pct.is_none() {
            Err(String::from("'width' or 'width_pct' must be set."))
        } else if self.height.is_none() && self.height_pct.is_none() {
            Err(String::from("'height' or 'height_pct' must be set."))
        } else {
            Ok(())
        }
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
