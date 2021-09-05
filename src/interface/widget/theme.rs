use crate::interface::bin::Color;

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetTheme {
	pub color_back1: Color,
	pub color_border1: Color,
	pub color_text1: Color,
	pub dim_text1: f32,
	pub dim_border1: f32,
}

impl Default for WidgetTheme {
	fn default() -> Self {
		Self {
			color_back1: Color::from_hex("ffffff"),
			color_border1: Color::from_hex("000000"),
			color_text1: Color::from_hex("000000"),
			dim_text1: 14.0,
			dim_border1: 1.0,
		}
	}
}
