#[cfg(test)]
pub mod test;

pub use ilmenite::font::{BstFont,BstFontWeight};
pub use ilmenite::glyph::{BstGlyph,BstGlyphRaw,BstGlyphPos,BstGlyphGeo};
pub use ilmenite::error::{BstTextError,BstTextErrorSrc,BstTextErrorTy};
pub use ilmenite::script::{BstTextScript,BstTextLang};
pub use ilmenite::parse::parse_and_shape;
pub use ilmenite::bitmap::BstGlyphBitmap;
pub use ilmenite::bitmap_cache::BstGlyphBitmapCache;

use crate::interface::bin::{Bin,BinStyle,PositionTy};
use crate::Basalt;
use std::sync::Arc;
use std::collections::BTreeMap;
use atlas::{ImageDims,Coords,ImageData,SubImageCacheID,ImageType,Image};
use ordered_float::OrderedFloat;

pub struct BasaltText {
	pub container: Arc<Bin>,
	pub bitmap_cache: BstGlyphBitmapCache,
	pub glyph_data: Vec<BstGlyphData>,
}

pub struct BstGlyphData {
	pub glyph: BstGlyph,
	pub bin: Arc<Bin>,
}

pub fn create_basalt_text<T: AsRef<str>>(basalt: &Arc<Basalt>, text: T, script: BstTextScript, lang: BstTextLang) -> Result<BasaltText, BstTextError> {
	let glyphs = parse_and_shape(text, script, lang)?;
	let mut bins = basalt.interface_ref().new_bins(glyphs.len() + 1);
	let container = bins.pop().unwrap();
	let height = glyphs.first().unwrap().glyph_raw.font.ascender - glyphs.first().unwrap().glyph_raw.font.descender;
	
	container.style_update(BinStyle {
		position_t: Some(PositionTy::FromParent),
		pos_from_t: Some(0.0),
		pos_from_l: Some(0.0),
		pos_from_r: Some(0.0),
		height: Some(height),
		overflow_y: Some(true),
		.. BinStyle::default()
	});
	
	let mut bitmap_cache = BstGlyphBitmapCache::new(basalt.device(), basalt.graphics_queue());
	let mut atlas_coords = BTreeMap::new();
	let mut glyph_data: Vec<BstGlyphData> = Vec::new();
	
	for glyph in glyphs {
		let bitmap = bitmap_cache.bitmap_for_glyph(&glyph)?;
		let bin = bins.pop().unwrap();
		container.add_child(bin.clone());
		
		let coords = atlas_coords.entry(glyph.glyph_raw.index).or_insert_with(|| {
			create_atlas_image(basalt, &bitmap).unwrap()
		}).clone();
		
		bin.style_update(BinStyle {
			position_t: Some(PositionTy::FromParent),
			pos_from_l: Some((glyph.position.x + bitmap.bearing_x).floor()),
			pos_from_t: Some((glyph.position.y + bitmap.bearing_y).ceil()),
			width: Some(bitmap.width as f32),
			height: Some(bitmap.height as f32),
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
		bitmap_cache,
		glyph_data
	})
}

pub fn atlas_cache_id(glyph: &BstGlyphRaw) -> SubImageCacheID {
	SubImageCacheID::BstGlyph(
		glyph.font.atlas_iden(),
		OrderedFloat::from(glyph.font_height),
		glyph.index
	)
}

pub fn create_atlas_image(basalt: &Arc<Basalt>, bitmap: &BstGlyphBitmap) -> Result<Coords, BstTextError> {
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
	).map_err(|e| BstTextError::src_and_ty(BstTextErrorSrc::Bitmap, BstTextErrorTy::Other(e)))?;
	
	Ok(basalt.atlas_ref().load_image(atlas_cache_id(&bitmap.glyph_raw), atlas_image)
		.map_err(|e| BstTextError::src_and_ty(BstTextErrorSrc::Bitmap, BstTextErrorTy::Other(e)))?)
}