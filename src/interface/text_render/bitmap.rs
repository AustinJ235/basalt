pub use super::font::{BstFont,BstFontWeight};
pub use super::glyph::{BstGlyph,BstGlyphRaw,BstGlyphPos,BstGlyphGeo,BstGlyphPoint};
pub use super::error::{BstTextError,BstTextErrorSrc,BstTextErrorTy};
pub use super::script::{BstTextScript,BstTextLang};
use crate::atlas::{Coords,Image,ImageType,ImageDims,ImageData,SubImageCacheID};
use ordered_float::OrderedFloat;
use std::sync::Arc;
use crate::Basalt;

#[derive(Clone,Debug,PartialEq)]
pub struct BstGlyphBitmap {
	pub glyph_raw: Arc<BstGlyphRaw>,
	pub width: u32,
	pub height: u32,
	pub bearing_x: f32,
	pub bearing_y: f32,
	pub data: Vec<Vec<f32>>,
	pub coords: Coords,
}

impl BstGlyphBitmap {
	pub fn new(glyph_raw: Arc<BstGlyphRaw>) -> BstGlyphBitmap {
		let bearing_x = glyph_raw.min_x.ceil();
		let bearing_y = 0.0;
		let width = (glyph_raw.max_x.ceil() - glyph_raw.min_x.ceil()) as u32;
		let height = (glyph_raw.max_y.ceil() - glyph_raw.min_y.ceil()) as u32;
		
		dbg!(width, height);
		
		let mut data = Vec::with_capacity(width as usize);
		data.resize_with(width as usize, || {
			let mut col = Vec::with_capacity(height as usize);
			col.resize(height as usize, 0.0);
			col
		});
		
		BstGlyphBitmap {
			width,
			height,
			bearing_x,
			bearing_y,
			data,
			glyph_raw,
			coords: Coords::none(),
		}
	}
	
	pub fn atlas_cache_id(&self) -> SubImageCacheID {
		SubImageCacheID::BstGlyph(
			self.glyph_raw.font.atlas_iden(),
			OrderedFloat::from(self.glyph_raw.font_height),
			self.glyph_raw.index
		)
	}
	
	pub fn create_atlas_image(&mut self, basalt: &Arc<Basalt>) -> Result<(), BstTextError> {
		if self.width == 0 || self.height == 0 {
			return Ok(());
		}
	
		let data_len = (self.width * self.height) as usize;
		let mut data = Vec::with_capacity(data_len);
		data.resize(data_len, 0_u8);
		
		for x in 0..(self.width as usize) {
			for y in 0..(self.height as usize) {
				data[(self.width as usize * (self.height as usize - 1 - y)) + x] =
					(self.data[x][y] * u8::max_value() as f32).round() as u8;
			}
		}
		
		let atlas_image = Image::new(
			ImageType::LMono,
			ImageDims {
				w: self.width,
				h: self.height
			},
			ImageData::D8(data)
		).map_err(|e| BstTextError::src_and_ty(BstTextErrorSrc::Bitmap, BstTextErrorTy::Other(e)))?;
		
		self.coords = basalt.atlas_ref().load_image(self.atlas_cache_id(), atlas_image)
			.map_err(|e| BstTextError::src_and_ty(BstTextErrorSrc::Bitmap, BstTextErrorTy::Other(e)))?;
			
		Ok(())
	}

	pub fn fill(&mut self) {
		
	}
	
	pub fn draw_outline(&mut self) -> Result<(), BstTextError> {
		let glyph_raw = self.glyph_raw.clone();
		
		for geometry in &glyph_raw.geometry {
			self.draw_geometry(geometry)?;
		}
		
		Ok(())
	}
	
	pub fn draw_geometry(&mut self, geo: &BstGlyphGeo) -> Result<(), BstTextError> {
		match geo {
			&BstGlyphGeo::Line(ref points) => self.draw_line(&points[0], &points[1]),
			&BstGlyphGeo::Curve(ref points) => self.draw_curve(&points[0], &points[1], &points[2])
		}
	}
	
	pub fn draw_line(
		&mut self,
		point_a: &BstGlyphPoint,
		point_b: &BstGlyphPoint
	) -> Result<(), BstTextError> {
		let diff_x = point_b.x - point_a.x;
		let diff_y = point_b.y - point_a.y;
		let steps = (diff_x.powi(2) + diff_y.powi(2)).sqrt().ceil() as usize;
		
		for s in 0..=steps {
			let x = ((point_a.x + ((diff_x / steps as f32) * s as f32)) - self.glyph_raw.min_x).floor() as usize;
			let y = ((point_a.y + ((diff_y / steps as f32) * s as f32)) - self.glyph_raw.min_y).floor() as usize;
			
			if let Some(v) = self.data.get_mut(x).and_then(|v| v.get_mut(y)) {
				*v = 1.0;
			}
		}
		
		Ok(())
	}
	
	pub fn draw_curve(
		&mut self,
		point_a: &BstGlyphPoint,
		point_b: &BstGlyphPoint,
		point_c: &BstGlyphPoint
	) -> Result<(), BstTextError> {
		let steps = 3_usize;
		let mut last = point_a.clone();
		
		for s in 0..=steps {
			let t = s as f32 / steps as f32;
			let next = point_a.lerp(t, point_b).lerp(t, &point_b.lerp(t, point_c));
			self.draw_line(&last, &next);
			last = next;
		}
		
		Ok(())
	}
}
