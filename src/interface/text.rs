#![allow(warnings)]

use std::path::{Path,PathBuf};
use std::sync::Arc;
use Engine;
use interface::interface::ItfVertInfo;
use parking_lot::RwLock;
use std::collections::{HashMap,BTreeMap};
use std::sync::atomic::{self,AtomicPtr};
use freetype_sys::*;
use misc::HashMapExtras;
use std::rc::Rc;
use std::ptr;
use std::fs::File;
use std::io::Read;

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
	
	pub fn render_text<T: AsRef<str>, F: AsRef<str>>(&self, text: T, family: F, size: f32, wrap_ty: WrapTy) -> Result<Vec<(usize, Vec<ItfVertInfo>)>, String> {
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
		
		let mut to_render = Vec::with_capacity(text.as_ref().len());
		
		for c in text.as_ref().chars() {
			to_render.push(match font.get_char_info(c) {
				Ok(ok) => ok,
				Err(e) => return Err(format!("Failed to get char info for {}, family: {}, size: {}: {}", c, family.as_ref(), size, e))
			});
		}
		
		unimplemented!()
	}
}

pub struct Font {
	engine: Arc<Engine>,
	ft_lib: AtomicPtr<FT_LibraryRec>,
	ft_face: AtomicPtr<FT_FaceRec>,
	max_ht: i32,
	chars: RwLock<BTreeMap<char, Rc<CharInfo>>>,
}

impl Font {
	pub fn new<P: AsRef<Path>>(engine: Arc<Engine>, path: P, size: u32) -> Result<Arc<Self>, String> {
		let mut library: FT_Library = ptr::null_mut();
		let mut result = unsafe { FT_Init_FreeType(&mut library) };
		
		if result > 0 {
			return Err(format!("Failed to init freetype, error id: {}", result));
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
			return Err(format!("Failed create new face, error id: {}", result));
		}
		
		if unsafe { FT_Set_Pixel_Sizes(face, 0, size) } > 0 {
			return Err(format!("failed to set pixel sizes, error id: {}", result));
		}
		
		let max_ht = f32::floor(unsafe { (*face).height } as f32 / 96.0) as i32 + (size/2) as i32;
	
		Ok(Arc::new(Font {
			engine,
			ft_lib: AtomicPtr::new(library),
			ft_face: AtomicPtr::new(face),
			max_ht,
			chars: RwLock::new(BTreeMap::new()),
		}))
	}
	
	pub fn get_char_info(&self, c: char) -> Result<CharInfo, String> {
		unimplemented!()
	}
}

#[derive(Clone)]
pub struct CharInfo {
	bx: i32,
	by: i32,
	w: i32,
	h: i32,
	adv: i32,
	atlas_i: u32,
	atlas_coords: (f32, f32),
	verts: Vec<ItfVertInfo>,
}

