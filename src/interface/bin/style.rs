use crate::atlas;
use ilmenite::{ImtHoriAlign, ImtTextWrap, ImtVertAlign};
use std::sync::Arc;
use vulkano::image::ImageViewAccess;

#[derive(Clone, Debug, PartialEq)]
pub enum BinPosition {
	Window,
	Parent,
	Floating,
}

impl Default for BinPosition {
	fn default() -> Self {
		BinPosition::Window
	}
}

#[derive(Default, Clone)]
pub struct BinStyle {
	pub position: Option<BinPosition>,
	pub z_index: Option<i16>,
	pub add_z_index: Option<i16>,
	pub hidden: Option<bool>,
	pub opacity: Option<f32>,
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
	pub height: Option<f32>,
	pub height_pct: Option<f32>,
	// Margin
	pub margin_t: Option<f32>, //|
	pub margin_b: Option<f32>, //| Not Implemented
	pub margin_l: Option<f32>, //|
	pub margin_r: Option<f32>, //|
	// Padding
	pub pad_t: Option<f32>, //|
	pub pad_b: Option<f32>, //| Text Only
	pub pad_l: Option<f32>, //|
	pub pad_r: Option<f32>, //|
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
	pub back_image_raw: Option<Arc<dyn ImageViewAccess + Send + Sync>>,
	pub back_image_raw_coords: Option<atlas::Coords>,
	pub back_srgb_yuv: Option<bool>,
	pub back_image_effect: Option<ImageEffect>,
	// Text
	pub text: String,
	pub text_color: Option<Color>,
	pub text_height: Option<f32>,
	pub line_spacing: Option<f32>,
	pub line_limit: Option<usize>,
	pub text_wrap: Option<ImtTextWrap>,
	pub text_vert_align: Option<ImtVertAlign>,
	pub text_hori_align: Option<ImtHoriAlign>,
	pub custom_verts: Vec<BinVert>,
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

	pub fn srgb_hex(code: &str) -> Self {
		let mut color = Self::from_hex(code);
		color.r = f32::powf((color.r + 0.055) / 1.055, 2.4);
		color.g = f32::powf((color.g + 0.055) / 1.055, 2.4);
		color.b = f32::powf((color.b + 0.055) / 1.055, 2.4);
		color.a = f32::powf((color.a + 0.055) / 1.055, 2.4);
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
