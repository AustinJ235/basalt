#[cfg(test)]
pub mod test;

pub use ilmenite::*;

use crate::interface::bin::{Bin,BinStyle,PositionTy};
use crate::Basalt;
use std::sync::Arc;
use std::collections::BTreeMap;
use atlas::{ImageDims,Coords,ImageData,SubImageCacheID,ImageType,Image};
use ordered_float::OrderedFloat;

pub struct BasaltText {
	pub container: Arc<Bin>,
	pub atlas_coords: BTreeMap<u16, Coords>,
	pub glyph_data: Vec<BstGlyphData>,
}

pub struct BstGlyphData {
	pub glyph: ImtRasteredGlyph,
	pub bin: Arc<Bin>,
}

pub fn create_basalt_text<T: AsRef<str>>(basalt: &Arc<Basalt>, text: T, script: ImtScript, lang: ImtLang) -> Result<BasaltText, ImtError> {
	let mut parser = ImtParser::new(include_bytes!("./ABeeZee-Regular.ttf"))?;
	let shaper = ImtShaper::new()?;
	let mut raster = ImtRaster::new(basalt.device(), basalt.graphics_queue(), ImtRasterOps::default())?;
	
	let parsed_glyphs = parser.retreive_text(text, script, lang)?;
	let shaped_glyphs = shaper.shape_parsed_glyphs(&mut parser, script, lang, ImtShapeOpts::default(), parsed_glyphs)?;
	let rastered_glyphs = raster.raster_shaped_glyphs(&parser, 36.0, shaped_glyphs)?;
	
	let font_props = parser.font_props();
	let line_height = (font_props.ascender * font_props.scaler * 36.0) - (font_props.descender * font_props.scaler * 36.0);
	
	let mut bins = basalt.interface_ref().new_bins(rastered_glyphs.len() + 1);
	let container = bins.pop().unwrap();
	
	container.style_update(BinStyle {
		position_t: Some(PositionTy::FromParent),
		pos_from_t: Some(0.0),
		pos_from_l: Some(0.0),
		pos_from_r: Some(0.0),
		height: Some(line_height),
		overflow_y: Some(true),
		.. BinStyle::default()
	});
	
	let mut atlas_coords = BTreeMap::new();
	let mut glyph_data: Vec<BstGlyphData> = Vec::new();
	
	for glyph in rastered_glyphs {
		let bin = bins.pop().unwrap();
		container.add_child(bin.clone());
		let index = glyph.shaped.parsed.inner.glyph_index.unwrap();
		
		let coords = atlas_coords.entry(index).or_insert_with(|| {
			create_atlas_image(basalt, index, &glyph.bitmap).unwrap()
		}).clone();
		
		let pos_from_l = Some(((glyph.shaped.position.x * font_props.scaler * 36.0) + glyph.bitmap.bearing_x).floor());
		let pos_from_t = Some(((glyph.shaped.position.y * font_props.scaler * 36.0) + glyph.bitmap.bearing_y).floor());
		
		bin.style_update(BinStyle {
			position_t: Some(PositionTy::FromParent),
			pos_from_l,
			pos_from_t,
			width: Some(glyph.bitmap.width as f32),
			height: Some(glyph.bitmap.height as f32),
			back_image_atlas: Some(coords),
			.. BinStyle::default()
		});
		
		glyph_data.push(BstGlyphData {
			glyph,
			bin
		})
	}
	
	Ok(BasaltText {
		container,
		atlas_coords,
		glyph_data
	})
}

pub fn create_atlas_image(basalt: &Arc<Basalt>, index: u16, bitmap: &Arc<ImtGlyphBitmap>) -> Result<Coords, ImtError> {
	if bitmap.width == 0 || bitmap.height == 0 {
		return Ok(Coords::none());
	}

	let data_len = (bitmap.width * bitmap.height) as usize;
	let mut data = Vec::with_capacity(data_len);
	data.resize(data_len, 0_u8);
	
	for x in 0..(bitmap.width as usize) {
		for y in 0..(bitmap.height as usize) {
			data[(bitmap.width as usize * (bitmap.height as usize - 1 - y)) + x] =
				(bitmap.data[x][y] * u8::max_value() as f32).round() as u8;
		}
	}
	
	let atlas_image = Image::new(
		ImageType::LMono,
		ImageDims {
			w: bitmap.width,
			h: bitmap.height
		},
		ImageData::D8(data)
	).map_err(|e| ImtError::src_and_ty(ImtErrorSrc::Bitmap, ImtErrorTy::Other(e)))?;
	
	let cache_id = SubImageCacheID::BstGlyph (
		String::from("ABeeZee Regular"),
		OrderedFloat::from(36.0),
		index
	);
	
	Ok(basalt.atlas_ref().load_image(cache_id, atlas_image)
		.map_err(|e| ImtError::src_and_ty(ImtErrorSrc::Bitmap, ImtErrorTy::Other(e)))?)
}