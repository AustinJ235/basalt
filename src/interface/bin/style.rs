use crate::atlas;
use crate::image_view::BstImageView;
use ilmenite::{ImtHoriAlign, ImtTextWrap, ImtVertAlign};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
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
	/// If set to true bin hook events will be passed to
	/// children instead of this bin.
	pub pass_events: Option<bool>,
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
	pub scroll_x: Option<f32>, // Not Implemented
	pub overflow_y: Option<bool>,
	pub overflow_x: Option<bool>, // Not Implemented
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
	pub back_image_atlas: Option<atlas::Coords>,
	pub back_image_raw: Option<Arc<BstImageView>>,
	pub back_image_raw_coords: Option<atlas::Coords>,
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
	pub custom_verts: Vec<BinVert>,
}

impl BinStyle {
	pub fn is_floating_compatible(&self) -> Result<(), String> {
		if self.position != Some(BinPosition::Floating) {
			Err(format!("'position' must be 'BinPosition::Floating'."))
		} else if self.pos_from_t.is_some() {
			Err(format!("'pos_from_t' is not allowed or not implemented."))
		} else if self.pos_from_b.is_some() {
			Err(format!("'pos_from_b' is not allowed or not implemented."))
		} else if self.pos_from_l.is_some() {
			Err(format!("'pos_from_l' is not allowed or not implemented."))
		} else if self.pos_from_r.is_some() {
			Err(format!("'pos_from_r' is not allowed or not implemented."))
		} else if self.pos_from_t_pct.is_some() {
			Err(format!("'pos_from_t_pct' is not allowed or not implemented."))
		} else if self.pos_from_b_pct.is_some() {
			Err(format!("'pos_from_b_pct' is not allowed or not implemented."))
		} else if self.pos_from_l_pct.is_some() {
			Err(format!("'pos_from_l_pct' is not allowed or not implemented."))
		} else if self.pos_from_r_pct.is_some() {
			Err(format!("'pos_from_r_pct' is not allowed or not implemented."))
		} else if self.pos_from_l_offset.is_some() {
			Err(format!("'pos_from_l_offset' is not allowed or not implemented."))
		} else if self.pos_from_t_offset.is_some() {
			Err(format!("'pos_from_t_offset' is not allowed or not implemented."))
		} else if self.pos_from_r_offset.is_some() {
			Err(format!("'pos_from_r_offset' is not allowed or not implemented."))
		} else if self.pos_from_b_offset.is_some() {
			Err(format!("'pos_from_b_offset' is not allowed or not implemented."))
		} else if self.width.is_none() && self.width_pct.is_none() {
			Err(format!("'width' or 'width_pct' must be set."))
		} else if self.height.is_none() && self.height_pct.is_none() {
			Err(format!("'height' or 'height_pct' must be set."))
		} else {
			Ok(())
		}
	}
}

#[derive(Clone, Debug)]
pub enum ImageEffect {
	BackColorAdd,
	BackColorBehind,
	BackColorSubtract,
	BackColorMultiply,
	BackColorDivide,
	Invert,
}

impl ImageEffect {
	pub fn vert_type(&self) -> i32 {
		match self {
			&ImageEffect::BackColorAdd => 102,
			&ImageEffect::BackColorBehind => 103,
			&ImageEffect::BackColorSubtract => 104,
			&ImageEffect::BackColorMultiply => 105,
			&ImageEffect::BackColorDivide => 106,
			&ImageEffect::Invert => 107,
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
	pub fn as_tuple(&self) -> (f32, f32, f32, f32) {
		(self.r, self.g, self.b, self.a)
	}

	fn ffh(mut c1: u8, mut c2: u8) -> f32 {
		if c1 >= 97 && c1 <= 102 {
			c1 -= 87;
		} else if c1 >= 65 && c1 <= 70 {
			c1 -= 65;
		} else if c1 >= 48 && c1 <= 57 {
			c1 = c1.checked_sub(48).unwrap();
		} else {
			c1 = 0;
		}
		if c2 >= 97 && c2 <= 102 {
			c2 -= 87;
		} else if c2 >= 65 && c2 <= 70 {
			c2 -= 65;
		} else if c2 >= 48 && c2 <= 57 {
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
			Some(c1) =>
				match iter.next() {
					Some(c2) => Self::ffh(c1, c2),
					None =>
						return Color {
							r: red,
							g: green,
							b: blue,
							a: alpha,
						},
				},
			None =>
				return Color {
					r: red,
					g: green,
					b: blue,
					a: alpha,
				},
		};
		green = match iter.next() {
			Some(c1) =>
				match iter.next() {
					Some(c2) => Self::ffh(c1, c2),
					None =>
						return Color {
							r: red,
							g: green,
							b: blue,
							a: alpha,
						},
				},
			None =>
				return Color {
					r: red,
					g: green,
					b: blue,
					a: alpha,
				},
		};
		blue = match iter.next() {
			Some(c1) =>
				match iter.next() {
					Some(c2) => Self::ffh(c1, c2),
					None =>
						return Color {
							r: red,
							g: green,
							b: blue,
							a: alpha,
						},
				},
			None =>
				return Color {
					r: red,
					g: green,
					b: blue,
					a: alpha,
				},
		};
		alpha = match iter.next() {
			Some(c1) =>
				match iter.next() {
					Some(c2) => Self::ffh(c1, c2),
					None =>
						return Color {
							r: red,
							g: green,
							b: blue,
							a: alpha,
						},
				},
			None =>
				return Color {
					r: red,
					g: green,
					b: blue,
					a: alpha,
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
