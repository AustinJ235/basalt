#[derive(Clone,Debug,PartialEq)]
pub struct BstFont {
	pub name: String,
	pub weight: BstFontWeight,
	pub default_dpi: f32,
	pub default_pixel_height: f32,
	pub ascender: f32,
	pub descender: f32,
	pub line_gap: f32,
}

impl BstFont {
	pub fn atlas_iden(&self) -> String {
		format!("{}-{}", self.name, self.weight.as_string())
	}
}

#[derive(Clone,Debug,PartialEq)]
pub enum BstFontWeight {
	Thin,
	Light,
	Regular,
	Medium,
	Bold,
	Black,
}

impl BstFontWeight {
	pub fn as_string(&self) -> String {
		match self {
			&BstFontWeight::Thin => String::from("Thin"),
			&BstFontWeight::Light => String::from("Light"),
			&BstFontWeight::Regular => String::from("Regular"),
			&BstFontWeight::Medium => String::from("Medium"),
			&BstFontWeight::Bold => String::from("Bold"),
			&BstFontWeight::Black => String::from("Black"),
		}
	}
}
