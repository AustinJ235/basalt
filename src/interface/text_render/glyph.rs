use super::bitmap::*;

#[derive(Debug)]
pub struct BasaltGlyph {
	pub x: i32,
	pub y: i32,
	pub bounds_min: [i16; 2],
	pub bounds_max: [i16; 2],
	pub geometry: Vec<Geometry>,
	pub units_per_pixel: f32,
}

impl BasaltGlyph {
	pub fn bitmap(&self, scale: f32) -> Result<GlyphBitmap, String> {
		let width = (self.bounds_max[0] - self.bounds_min[0]) as u32;
		let height = (self.bounds_max[1] - self.bounds_min[1]) as u32;
		let mut data = Vec::with_capacity((width * height) as usize);
		data.resize((width * height) as usize, 0);
		
		let mut bitmap = GlyphBitmap {
			width,
			height,
			data
		};
		
		self.geometry.iter().for_each(|g| match g {
			&Geometry::Line(p) => bitmap.draw_line(self, &p),
			&Geometry::Curve(p) => bitmap.draw_curve(self, &p)
		});
		
		Ok(bitmap)
	}
}

#[derive(Debug)]
pub enum Geometry {
	Line([f32; 4]),
	Curve([f32; 6]),
}