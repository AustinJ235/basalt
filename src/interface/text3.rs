#![allow(warnings)]

use bindings::harfbuzz::*;
use freetype::freetype::*;
use std::sync::Arc;
use crossbeam::channel::{self,Sender,Receiver};
use std::collections::{BTreeMap,HashMap};
use parking_lot::{RwLock,Mutex};
use std::sync::atomic::{self,AtomicPtr,AtomicUsize};
use crossbeam::queue::MsQueue;
use atlas::CoordsInfo;
use interface::TextAlign;
use interface::WrapTy;
use interface::interface::ItfVertInfo;
use std::ptr;
use std::ffi::CString;
use Engine;
use atlas;

pub struct Text {
	engine: Arc<Engine>,
	ft_faces: RwLock<BTreeMap<u32, (Arc<AtomicPtr<FT_LibraryRec_>>, Arc<AtomicPtr<FT_FaceRec_>>)>>,
	hb_fonts: RwLock<BTreeMap<u32, Arc<AtomicPtr<hb_font_t>>>>,
	size_infos: RwLock<BTreeMap<u32, SizeInfo>>,
	hb_free_bufs: MsQueue<AtomicPtr<hb_buffer_t>>,
	glyphs: RwLock<BTreeMap<u32, BTreeMap<u64, Arc<Glyph>>>>,
}

#[derive(Clone)]
struct SizeInfo {
	start_y: f32,
	line_height: f32,
}

struct Glyph {
	w: f32,
	h: f32,
	hbx: f32,
	hby: f32,
	coords: CoordsInfo,
}

impl Text {
	pub(crate) fn new(engine: Arc<Engine>) -> Arc<Self> {
		Arc::new(Text {
			engine,
			ft_faces: RwLock::new(BTreeMap::new()),
			hb_fonts: RwLock::new(BTreeMap::new()),
			size_infos: RwLock::new(BTreeMap::new()),
			hb_free_bufs: MsQueue::new(),
			glyphs: RwLock::new(BTreeMap::new()),
		})
	}

	pub(crate) fn render_text<T: Into<String>, F: Into<String>>(
		&self, text: T, family: F, size: u32, color: (f32, f32, f32, f32),
		wrap: WrapTy, align: TextAlign,
	) -> Result<BTreeMap<usize, Vec<ItfVertInfo>>, String> {
		unsafe {
			let hb_buffer_ap = match self.hb_free_bufs.try_pop() {
				Some(some) => some,
				None => AtomicPtr::new(hb_buffer_create())
			}; let hb_buffer = hb_buffer_ap.load(atomic::Ordering::Relaxed);
			
			let (_, ft_face_ap) = {
				let ft_faces = self.ft_faces.upgradable_read();
				
				if ft_faces.contains_key(&size) {
					ft_faces.get(&size).unwrap().clone()
				} else {
					let mut ft_faces = ft_faces.upgrade();
					let mut ft_library = ptr::null_mut();

					match FT_Init_FreeType(&mut ft_library) {
						0 => (),
						e => return Err(format!("FT_Init_FreeType: error {}", e))
					}
					
					let mut ft_face = ptr::null_mut();
					let bytes = include_bytes!("ABeeZee-Regular.ttf");
					
					match FT_New_Memory_Face(ft_library, bytes.as_ptr(), (bytes.len() as i32).into(), 0, &mut ft_face) {
						0 => (),
						e => return Err(format!("FT_New_Memory_Face: error {}", e))
					}
					
					match FT_Set_Pixel_Sizes(ft_face, 0, size.into()) {
						0 => (),
						e => return Err(format!("FT_Set_Pixel_Sizes: error {}", e))
					}
					
					let ret = (Arc::new(AtomicPtr::new(ft_library)), Arc::new(AtomicPtr::new(ft_face)));
					ft_faces.insert(size, ret.clone());
					println!("add ft_face for size {}", size);
					ret
				}
			}; let ft_face = ft_face_ap.load(atomic::Ordering::Relaxed);
			
			let hb_font_ap = {
				let hb_fonts = self.hb_fonts.upgradable_read();
				
				if hb_fonts.contains_key(&size) {
					hb_fonts.get(&size).unwrap().clone()
				} else {
					let mut hb_fonts = hb_fonts.upgrade();
					let ret = Arc::new(AtomicPtr::new(hb_ft_font_create_referenced(ft_face)));
					hb_fonts.insert(size, ret.clone());
					println!("add hb_font for size {}", size);
					ret
				}
			}; let hb_font = hb_font_ap.load(atomic::Ordering::Relaxed);
			
			let size_info = {
				let size_infos = self.size_infos.upgradable_read();
				
				if size_infos.contains_key(&size) {
					size_infos.get(&size).unwrap().clone()
				} else {
					let mut size_infos = size_infos.upgrade();
					let ctext = CString::new("Tg").unwrap();
					hb_buffer_add_utf8(hb_buffer, ctext.as_ptr(), -1, 0, -1);
					hb_buffer_guess_segment_properties (hb_buffer);
					hb_shape(hb_font, hb_buffer, ptr::null_mut(), 0);
					let len = hb_buffer_get_length(hb_buffer) as usize;
					let info = ::std::slice::from_raw_parts(hb_buffer_get_glyph_infos(hb_buffer, ptr::null_mut()), len);
					let pos = std::slice::from_raw_parts(hb_buffer_get_glyph_positions(hb_buffer, ptr::null_mut()), len);
					
					let t_metrics = match FT_Load_Glyph(ft_face, info[0].codepoint.into(), FT_LOAD_DEFAULT as i32) {
						0 => (*(*ft_face).glyph).metrics.clone(),
						e => return Err(format!("FT_Load_Glyph: error {}", e))
					};
					
					let g_metrics = match FT_Load_Glyph(ft_face, info[1].codepoint.into(), FT_LOAD_DEFAULT as i32) {
						0 => (*(*ft_face).glyph).metrics.clone(),
						e => return Err(format!("FT_Load_Glyph: error {}", e))
					};
					
					let top = (pos[0].y_offset as f32 / 64.0) - (t_metrics.horiBearingY as f32 / 64.0);
					let bottom = (pos[1].y_offset as f32 - g_metrics.horiBearingY as f32 + g_metrics.height as f32) / 64.0;
					hb_buffer_reset(hb_buffer);
					
					let ret = SizeInfo {
						start_y: -top,
						line_height: (bottom-top) + (size as f32 / 6.0).ceil()
					};
					
					size_infos.insert(size, ret.clone());
					println!("add size info {}", size);
					ret
				}
			};
			
			let clines: Vec<CString> = text.into().lines().map(|v| CString::new(v).unwrap()).collect();
			let mut current_x = 0.0;
			let mut current_y = size_info.start_y;
			let mut vert_map = BTreeMap::new();
			let mut lines = Vec::new();
			lines.push(Vec::new());
			lines.last_mut().unwrap().push(Vec::new());
			
			for ctext in clines {
				hb_buffer_add_utf8(hb_buffer, ctext.as_ptr(), -1, 0, -1);
				hb_buffer_guess_segment_properties(hb_buffer);
				hb_shape(hb_font, hb_buffer, ptr::null_mut(), 0);
				
				let len = hb_buffer_get_length(hb_buffer) as usize;
				let info = ::std::slice::from_raw_parts(hb_buffer_get_glyph_infos(hb_buffer, ptr::null_mut()), len);
				let pos = ::std::slice::from_raw_parts(hb_buffer_get_glyph_positions(hb_buffer, ptr::null_mut()), len);
				
				for i in 0..len {
					let glyph_info: Arc<Glyph> = {
						let glyphs = self.glyphs.upgradable_read();
						
						if glyphs.contains_key(&size) && glyphs.get(&size).unwrap().contains_key(&(info[i].codepoint as u64)) {
							glyphs.get(&size).unwrap().get(&(info[i].codepoint as u64)).unwrap().clone()
						} else {
							let mut glyphs_sizes = glyphs.upgrade();
							let mut glyphs = glyphs_sizes.entry(size).or_insert_with(|| BTreeMap::new());
							
							match FT_Load_Glyph(ft_face, info[i].codepoint.into(), FT_LOAD_DEFAULT as i32) {
								0 => (),
								e => return Err(format!("FT_Load_Glyph: error {}", e))
							}
							
							match FT_Render_Glyph((*ft_face).glyph, FT_Render_Mode::FT_RENDER_MODE_NORMAL) {
								0 => (),
								e => return Err(format!("FT_Render_Glyph: error {}", e))
							}
							
							let bitmap = (*(*ft_face).glyph).bitmap;
							let w = bitmap.width as usize;
							let h = bitmap.rows as usize;
							
							if w == 0 || h == 0 {
								let ret = Arc::new(Glyph {
									w: 0.0,
									h: 0.0,
									hbx: 0.0,
									hby: 0.0,
									coords: CoordsInfo::none()
								});
								
								glyphs.insert(info[i].codepoint as u64, ret.clone());
								ret
							} else {
								let mut image_data = Vec::with_capacity(w * h * 4);
						
								for i in 0..((w*h) as isize) {
									image_data.push(0);
									image_data.push(0);
									image_data.push(0);
									image_data.push(*bitmap.buffer.offset(i));
								}
								
								let coords = match self.engine.atlas_ref().load_raw_with_key(
									&atlas::ImageKey::Glyph(size, info[i].codepoint as u64),
									image_data, w as u32, h as u32
								) {
									Ok(ok) => ok,
									Err(e) => return Err(format!("Atlas::load_raw_with_key: Error {}", e))
								};
								
								let ret = Arc::new(Glyph {
									w: (*(*ft_face).glyph).metrics.width as f32 / 64.0,
									h: (*(*ft_face).glyph).metrics.height as f32 / 64.0,
									hbx: (*(*ft_face).glyph).metrics.horiBearingX as f32 / 64.0,
									hby: (*(*ft_face).glyph).metrics.horiBearingY as f32 / 64.0,
									coords,
								});
								
								glyphs.insert(info[i].codepoint as u64, ret.clone());
								println!("added glyph");
								ret
							}
						}
					};
					
					if glyph_info.w == 0.0 || glyph_info.h == 0.0 {
						lines.last_mut().unwrap().push(Vec::new());
					} else {
						let tl = (
							current_x + (pos[i].x_offset as f32 / 64.0) + (glyph_info.hbx),
							current_y + (pos[i].y_offset as f32 / 64.0) - (glyph_info.hby),
							0.0
						);
						
						let tr = (tl.0 + (glyph_info.w), tl.1, 0.0);
						let bl = (tl.0, tl.1 + (glyph_info.h), 0.0);
						let br = (tr.0, bl.1, 0.0);
						
						let ctl = glyph_info.coords.f32_top_left();
						let ctr = glyph_info.coords.f32_top_right();
						let cbl = glyph_info.coords.f32_bottom_left();
						let cbr = glyph_info.coords.f32_bottom_right();
						
						let mut verts = Vec::with_capacity(6);
						verts.push(ItfVertInfo { position: tr, coords: ctr, color: color, ty: 1 });
						verts.push(ItfVertInfo { position: tl, coords: ctl, color: color, ty: 1 });
						verts.push(ItfVertInfo { position: bl, coords: cbl, color: color, ty: 1 });
						verts.push(ItfVertInfo { position: tr, coords: ctr, color: color, ty: 1 });
						verts.push(ItfVertInfo { position: bl, coords: cbl, color: color, ty: 1 });
						verts.push(ItfVertInfo { position: br, coords: cbr, color: color, ty: 1 });
						lines.last_mut().unwrap().last_mut().unwrap().push((glyph_info.coords.atlas_i, verts));
					}
					
					current_x += pos[i].x_advance as f32 / 64.0;
					current_y += pos[i].y_advance as f32 / 64.0;
				}
				
				lines.push(Vec::new());
				lines.last_mut().unwrap().push(Vec::new());
				hb_buffer_clear_contents(hb_buffer);
			}
			
			hb_buffer_reset(hb_buffer);
			self.hb_free_bufs.push(hb_buffer_ap);
			
			match wrap {
				WrapTy::ShiftX(_w) => {
				
				},
				WrapTy::ShiftY(_) => unimplemented!(),
				WrapTy::Normal(w, _h) => {
					let mut cur_line = Vec::new();
					cur_line.push(Vec::new());
					let mut wrapped_lines = Vec::new();
					
					for line in lines {
						let mut start = 0.0;
						let mut end = 0.0;
						let mut last_max_x = 0.0;
						let mut w_len = line.len();
					
						for (w_i, word) in line.into_iter().enumerate() {
							if word.is_empty() {
								continue;
							}
							
							let mut min_x = None;
							let mut max_x = None;
							
							for (_, verts) in &word {
								for vert in verts {
									if max_x.is_none() || vert.position.0 > *max_x.as_ref().unwrap() {
										max_x = Some(vert.position.0);
									}
									
									if min_x.is_none() || vert.position.0 < *min_x.as_ref().unwrap() {
										min_x = Some(vert.position.0);
									}	
								}
							}
							
							let min_x = min_x.unwrap();
							let max_x = max_x.unwrap();
							
							if cur_line.is_empty() {
								start = min_x;
							}
							
							if max_x - start > w && w_i != 0 {
								wrapped_lines.push((cur_line, start, last_max_x));
								cur_line = Vec::new();
								start = min_x;
							} else {
								last_max_x = max_x;
							}
							
							cur_line.push(word);
							
							if w_i == w_len-1 {
								end = max_x;
							}
						}
						
						wrapped_lines.push((cur_line, start, end));
						cur_line = Vec::new();
					}
					
					for (line_i, (words, start, end)) in wrapped_lines.into_iter().enumerate() {
						for word in words {
							let lwidth = end - start;
							let xoffset = match align {
								TextAlign::Left => -start,
								TextAlign::Center => ((w - lwidth) / 2.0) - start,
								TextAlign::Right => (w - lwidth) - start,
							};
							let yoffset = line_i as f32 * size_info.line_height;
						
							for (atlas_i, mut verts) in word {
								for vert in &mut verts {
									vert.position.0 += xoffset;
									vert.position.1 += yoffset;
								}
							
								vert_map.entry(atlas_i).or_insert(Vec::new()).append(&mut verts);
							}
						}
					}
				},
				WrapTy::None => {
					for words in lines {
						for word in words {
							for (atlas_i, mut verts) in word {
								vert_map.entry(atlas_i).or_insert(Vec::new()).append(&mut verts);
							}
						}
					}
				}
			}
			
			Ok(vert_map)
		}
	}
}
