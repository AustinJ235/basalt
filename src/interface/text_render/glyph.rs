use super::font::BstFont;
use std::sync::Arc;

#[derive(Clone,Debug,PartialEq)]
pub struct BstGlyph {
	pub glyph_raw: Arc<BstGlyphRaw>,
	pub position: BstGlyphPos,
}

#[derive(Clone,Debug,PartialEq)]
pub struct BstGlyphRaw {
	pub font: Arc<BstFont>,
	pub index: u16,
	pub min_x: f32,
	pub min_y: f32,
	pub max_x: f32,
	pub max_y: f32,
	pub geometry: Vec<BstGlyphGeo>,
	pub font_height: f32,
}

impl BstGlyphRaw {
	pub fn empty(font: Arc<BstFont>) -> Self {
		BstGlyphRaw {
			font,
			index: 0,
			min_x: 0.0,
			min_y: 0.0,
			max_x: 0.0,
			max_y: 0.0,
			geometry: Vec::new(),
			font_height: 16.0,
		}
	}
}

#[derive(Clone,Debug,PartialEq)]
pub enum BstGlyphGeo {
	Line([BstGlyphPoint; 2]),
	Curve([BstGlyphPoint; 3]),
}

#[derive(Clone,Debug,PartialEq)]
pub struct BstGlyphPos {
	pub x: f32,
	pub y: f32,
}

#[derive(Clone,Debug,PartialEq)]
pub struct BstGlyphPoint {
	pub x: f32,
	pub y: f32,
}

impl BstGlyphPoint {
	pub fn lerp(&self, t: f32, other: &Self) -> Self {
		BstGlyphPoint {
			x: self.x + ((other.x - self.x) * t),
			y: self.y + ((other.y - self.y) * t),
		}
	}
	
	pub fn dist(&self, other: &Self) -> f32 {
		((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
	}
}
