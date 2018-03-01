use freetype_sys::*;
use std::collections::BTreeMap;
use std::ptr;
use std::sync::atomic::AtomicPtr;
use parking_lot::RwLock;

#[derive(Clone)]
pub struct GlyphInfo {
	pub bitmap: Vec<u8>,
	pub bearing_x: i32,
	pub bearing_y: i32,
	pub width: i32,
	pub height: i32,
	pub advance: i32,
}

impl GlyphInfo {
	pub fn from(ft_glyph_slot: &FT_GlyphSlotRec) -> Self {
		let buf_size = (ft_glyph_slot.bitmap.width * ft_glyph_slot.bitmap.rows) as usize;
		let mut buffer: Vec<u8> = Vec::with_capacity(buf_size);
		
		for i in 0..buf_size {
			unsafe { buffer.push(*(ft_glyph_slot.bitmap.buffer).offset(i as isize)) };
		}
	
		GlyphInfo {
			bitmap: buffer,
			bearing_x: f32::round(ft_glyph_slot.metrics.horiBearingX as f32 / 64_f32) as i32,
			bearing_y: f32::round(ft_glyph_slot.metrics.horiBearingY as f32 / 64_f32) as i32,
			width: ft_glyph_slot.bitmap.width,
			height: ft_glyph_slot.bitmap.rows,
			advance: f32::floor(ft_glyph_slot.metrics.horiAdvance as f32 / 64_f32) as i32,
		}
	}
}

#[allow(dead_code)]
pub struct Font {
	ft_lib: AtomicPtr<FT_LibraryRec>,
	ft_face: AtomicPtr<FT_FaceRec>,
	glyphs: RwLock<BTreeMap<u64, GlyphInfo>>,
	max_ht: i32,
}

impl Font {
	pub fn new(size: u32) -> Result<Self, String> {
		let mut library: FT_Library = ptr::null_mut();
		let mut result = unsafe { FT_Init_FreeType(&mut library) };
		
		if result > 0 {
			return Err(format!("Failed to init freetype, error id: {}", result));
		}
		
		let mut face: FT_Face = ptr::null_mut();
		let bytes = include_bytes!("ABeeZee-Regular.ttf");
		result = {
			#[cfg(target_os = "windows")]
			unsafe { FT_New_Memory_Face(library, bytes.as_ptr(), bytes.len() as i32, 0, &mut face) }
			#[cfg(not(target_os = "windows"))]
			unsafe { FT_New_Memory_Face(library, bytes.as_ptr(), bytes.len() as i64, 0, &mut face) }
		};
		
		if result > 0 {
			return Err(format!("Failed create new face, error id: {}", result));
		}
		
		if unsafe { FT_Set_Pixel_Sizes(face, 0, size) } > 0 {
			return Err(format!("failed to set pixel sizes, error id: {}", result));
		}
		
		let max_ht = f32::floor(unsafe { (*face).height } as f32 / 96.0) as i32 + (size/2) as i32;
	
		Ok(Font {
			ft_lib: AtomicPtr::new(library),
			ft_face: AtomicPtr::new(face),
			glyphs: RwLock::new(BTreeMap::new()),
			max_ht: max_ht,
		})
	}
	
	pub fn max_ht(&self) -> i32 {
		self.max_ht
	}
	
	pub fn load_glyph(&mut self, char_code: u64) -> Result<(), String> {
		if self.glyphs.read().get(&char_code).is_some() {
			return Ok(())
		}
	
		let glyph_i = {
			#[cfg(target_os = "windows")]
			unsafe { FT_Get_Char_Index(*self.ft_face.get_mut(), char_code as u32) }
			#[cfg(not(target_os = "windows"))]
			unsafe { FT_Get_Char_Index(*self.ft_face.get_mut(), char_code) }
		};
		
		let mut result = unsafe { FT_Load_Glyph(*self.ft_face.get_mut(), glyph_i, FT_LOAD_DEFAULT) };
		
		if result > 0 {
			return Err(format!("Failed to load glyph, error id: {}", result));
		}
		
		result = unsafe { FT_Render_Glyph((**self.ft_face.get_mut()).glyph, FT_RENDER_MODE_NORMAL) };
		
		if result > 0 {
			return Err(format!("Failed to render glyph, error id: {}", result));
		}
		
		let info = unsafe { GlyphInfo::from(&*(**self.ft_face.get_mut()).glyph) };
		self.glyphs.write().insert(char_code, info);
		Ok(())
	}
	
	pub fn get_glyph(&mut self, char_code: u64) -> Result<GlyphInfo, String> {
		if let Err(e) = self.load_glyph(char_code) {
			return Err(e);
		} Ok(self.glyphs.read().get(&char_code).unwrap().clone())
	}
}

