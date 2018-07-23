#![allow(warnings)]

use std::path::{Path,PathBuf};
use std::sync::Arc;
use Engine;
use interface::interface::ItfVertInfo;
use parking_lot::{RwLock,Mutex};
use std::collections::{HashMap,BTreeMap};
use std::sync::atomic::{self,AtomicPtr};
use freetype_sys::*;
use misc::{HashMapExtras,BTreeMapExtras};
use std::rc::Rc;
use std::ptr;
use std::fs::File;
use std::io::Read;
use atlas;
use interface::bin;

pub(crate) struct Text {
	engine: Arc<Engine>,
	font_srcs: RwLock<HashMap<String, PathBuf>>,
	fonts: RwLock<HashMap<String, BTreeMap<u32, Arc<Font>>>>,
}

pub enum WrapTy {
	ShiftX(f32),
	ShiftY(f32),
	Normal(f32, f32),
	None
}

impl Text {
	pub fn new(engine: Arc<Engine>) -> Arc<Self> {
		Arc::new(Text {
			engine,
			font_srcs: RwLock::new(HashMap::new()),
			fonts: RwLock::new(HashMap::new()),
		})
	}

	pub fn add_font<P: AsRef<Path>, F: AsRef<str>>(&self, path: P, family: F) -> Result<(), String> {
		self.font_srcs.write().insert(String::from(family.as_ref()), path.as_ref().to_owned());
		Ok(())
	}
	
	pub fn render_text<T: AsRef<str>, F: AsRef<str>>(&self, text: T, family: F, size: f32, color: (f32, f32, f32, f32), wrap_ty: WrapTy) -> Result<(BTreeMap<usize, Vec<ItfVertInfo>>, f32), String> {
		if text.as_ref().len() < 1 {
			return Ok((BTreeMap::new(), 0.0));
		}
	
		let mut add_font_op = None;
		let size = size.floor() as u32;
		
		let font = match match self.fonts.read().get(family.as_ref()) {
			Some(size_map) => match size_map.get(&size) {
				Some(some) => Some(some.clone()),
				None => None
			}, None => None
		} {
			Some(some) => some,
			None => match self.font_srcs.read().get(family.as_ref()) {
				Some(src) => match Font::new(self.engine.clone(), src, size) {
					Ok(font) => {
						add_font_op = Some(font.clone());
						font
					}, Err(e) => return Err(format!("Failed to add font family '{}' of size {}: {}", family.as_ref(), size, e))
				}, None => return Err(format!("Family '{}' does not have a source.", family.as_ref()))
			}
		};
		
		if let Some(add_font) = add_font_op {
			self.fonts.write().get_mut_or_else(&String::from(family.as_ref()), || { BTreeMap::new() }).insert(size, add_font);
		}
		
		let max_w = match &wrap_ty {
			&WrapTy::ShiftX(ref w) => *w,
			&WrapTy::ShiftY(_) => 0.0,
			&WrapTy::Normal(ref w, _) => *w,
			&WrapTy::None => 0.0
		}.floor() as i32;
		
		let max_y = match &wrap_ty {
			&WrapTy::ShiftX(_) => 0.0,
			&WrapTy::ShiftY(ref h) => *h,
			&WrapTy::Normal(_, ref h) => *h,
			&WrapTy::None => 0.0
		}.floor() as i32;
		
		let mut cur_x_off = 0;
		let mut cur_y_off = 0;
		let mut word: Vec<Arc<CharInfo>> = Vec::new();
		let mut char_i = 0;
		let chars: Vec<char> = text.as_ref().chars().collect();
		let mut vert_map: BTreeMap<usize, Vec<ItfVertInfo>> = BTreeMap::new();
		let mut space_op = None;
		let mut flush = false;
		
		loop {
			if char_i >= chars.len() {
				if word.is_empty() {
					break;
				}
				
				flush = true;
			}
			
			let c = if flush {
				chars[0]
			} else {
				chars[char_i]
			};
			
			let mut wrapped = false;
			
			if !flush && c == ' ' && space_op.is_none() {
				space_op = Some(match font.get_char_info(' ') {
					Ok(ok) => ok,
					Err(e) => return Err(format!("Failed to get char info for ' ': {}", e))
				});
			}
			
			if flush || c == ' ' || c == '\n' {
				let word_w: i32 = word.iter().map(|c| c.adv).sum();
				
				if max_w != 0 && cur_x_off + word_w > max_w {
					cur_x_off = 0;
					cur_y_off += font.max_ht;
					wrapped = true;
				}
				
				if !wrapped && c == ' ' {
					let space = space_op.as_ref().unwrap();
					
					if max_w != 0 && word_w + space.adv < max_w {
						word.push(space.clone());
					}
				}
				
				for wc in word {
					let tl = (
						(cur_x_off + wc.bx) as f32,
						(cur_y_off + (font.max_ht - wc.by)) as f32,
						0.0
					); let tr = (
						tl.0 + wc.w as f32,
						tl.1, 0.0
					); let bl = (
						tl.0,
						tl.1 + wc.h as f32, 0.0,
					); let br = (
						tr.0,
						bl.1, 0.0
					);
					
					let ctl = wc.coords.f32_top_left();
					let ctr = wc.coords.f32_top_right();
					let cbl = wc.coords.f32_bottom_left();
					let cbr = wc.coords.f32_bottom_right();
					
					let verts = vert_map.get_mut_or_else(&wc.coords.atlas_i, || { Vec::new() });
					verts.push(ItfVertInfo { position: tr, coords: ctr, color: color, ty: 1 });
					verts.push(ItfVertInfo { position: tl, coords: ctl, color: color, ty: 1 });
					verts.push(ItfVertInfo { position: bl, coords: cbl, color: color, ty: 1 });
					verts.push(ItfVertInfo { position: tr, coords: ctr, color: color, ty: 1 });
					verts.push(ItfVertInfo { position: bl, coords: cbl, color: color, ty: 1 });
					verts.push(ItfVertInfo { position: br, coords: cbr, color: color, ty: 1 });
					
					cur_x_off += wc.adv;
				}
				
				word = Vec::new();
			}
			
			if !flush {
				if c == '\n' {
					if !wrapped {
						cur_x_off = 0;
						cur_y_off += font.max_ht;
					}
				} else if c != ' ' {
					word.push(match font.get_char_info(c) {
						Ok(ok) => ok,
						Err(e) => return Err(format!("Failed to get char info for '{}': {}", c, e))
					});
				}
			} else {
				break;
			}
			
			char_i += 1;
		}
		
		Ok((vert_map, (cur_y_off + font.max_ht) as f32))
	}
}

struct FTPointers {
	lib: AtomicPtr<FT_LibraryRec>,
	face: AtomicPtr<FT_FaceRec>,
}

pub struct Font {
	engine: Arc<Engine>,
	ft_ptrs: Mutex<FTPointers>,
	size: u32,
	max_ht: i32,
	chars: RwLock<BTreeMap<char, Arc<CharInfo>>>,
}

impl Font {
	pub fn new<P: AsRef<Path>>(engine: Arc<Engine>, path: P, size: u32) -> Result<Arc<Self>, String> {
		let mut library: FT_Library = ptr::null_mut();
		let mut result = unsafe { FT_Init_FreeType(&mut library) };
		
		if result > 0 {
			return Err(format!("Failed to init freetype, freetype error id: {}", result));
		}
		
		let mut face: FT_Face = ptr::null_mut();
		let bytes = match File::open(path.as_ref()) {
			Ok(mut handle) => {
				let mut bytes = Vec::new();
				
				if let Err(e) = handle.read_to_end(&mut bytes) {
					return Err(format!("Failed to read source for font from {}: {}", path.as_ref().display(), e));
				}
				
				bytes
			}, Err(e) => return Err(format!("Failed to read source for font from {}: {}", path.as_ref().display(), e))
		};
		
		result = {
			#[cfg(target_os = "windows")]
			unsafe { FT_New_Memory_Face(library, bytes.as_ptr(), bytes.len() as i32, 0, &mut face) }
			#[cfg(not(target_os = "windows"))]
			unsafe { FT_New_Memory_Face(library, bytes.as_ptr(), bytes.len() as i64, 0, &mut face) }
		};
		
		if result > 0 {
			return Err(format!("Failed create new face, freetype error id: {}", result));
		}
		
		if unsafe { FT_Set_Pixel_Sizes(face, 0, size) } > 0 {
			return Err(format!("failed to set pixel sizes, freetype error id: {}", result));
		}
		
		let max_ht = f32::floor(unsafe { (*face).height } as f32 / 96.0) as i32 + (size/2) as i32;
	
		Ok(Arc::new(Font {
			engine,
			size,
			ft_ptrs: Mutex::new(FTPointers {
				lib: AtomicPtr::new(library),
				face: AtomicPtr::new(face),
			}),
			max_ht,
			chars: RwLock::new(BTreeMap::new()),
		}))
	}
	
	pub fn get_char_info(&self, c: char) -> Result<Arc<CharInfo>, String> {
		unsafe {
			if let Some(info) = self.chars.read().get(&c) {
				return Ok(info.clone());
			}
			
			let mut ptrs = self.ft_ptrs.lock();
			let ft_lib = *ptrs.lib.get_mut();
			let ft_face = *ptrs.face.get_mut();
			
			let glyph_i = {
				#[cfg(target_os = "windows")]
				{ FT_Get_Char_Index(ft_face, c as u32) }
				#[cfg(not(target_os = "windows"))]
				{ FT_Get_Char_Index(ft_face, c as u64) }
			};
			
			let mut result = FT_Load_Glyph(ft_face, glyph_i, FT_LOAD_DEFAULT);
			
			if result > 0 {
				return Err(format!("Failed to load glyph, freetype error id: {}", result));
			}
			
			result = FT_Render_Glyph((*ft_face).glyph, FT_RENDER_MODE_NORMAL);
			
			if result > 0 {
				return Err(format!("Failed to render glyph, freetype error id: {}", result));
			}
			
			let ft_glyph_slot = &*(*ft_face).glyph;
			let buf_size = (ft_glyph_slot.bitmap.width * ft_glyph_slot.bitmap.rows) as usize;
			let mut buffer: Vec<u8> = Vec::with_capacity(buf_size * 4);
			
			for i in 0..buf_size {
				buffer.push(0);
				buffer.push(0);
				buffer.push(0);
				buffer.push(*(ft_glyph_slot.bitmap.buffer).offset(i as isize));
			}
			
			let coords = match self.engine.atlas_ref().load_raw_with_key(
				&atlas::ImageKey::Glyph(self.size, c as u64),
				buffer,
				ft_glyph_slot.bitmap.width as u32,
				ft_glyph_slot.bitmap.rows as u32
			) {
				Ok(ok) => ok,
				Err(e) => return Err(format!("Failed to load glyph into atlas: {}", e))
			};
			
			let char_info = Arc::new(CharInfo {
				bx: f32::round(ft_glyph_slot.metrics.horiBearingX as f32 / 64_f32) as i32,
				by: f32::round(ft_glyph_slot.metrics.horiBearingY as f32 / 64_f32) as i32,
				w: ft_glyph_slot.bitmap.width,
				h: ft_glyph_slot.bitmap.rows,
				adv: f32::floor(ft_glyph_slot.metrics.horiAdvance as f32 / 64_f32) as i32,
				coords: coords,
			});
			
			self.chars.write().insert(c, char_info.clone());
			Ok(char_info)
		}
	}
}

#[derive(Clone,Debug)]
pub struct CharInfo {
	bx: i32,
	by: i32,
	w: i32,
	h: i32,
	adv: i32,
	coords: atlas::CoordsInfo,
}

