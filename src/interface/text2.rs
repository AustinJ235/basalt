use bindings::harfbuzz::*;
use freetype::freetype::*;
use interface::interface::ItfVertInfo;
use std::ptr;
use std::ffi::CString;
use std::sync::{Arc,Barrier};
use Engine;
use atlas;
use interface::WrapTy;
use std::collections::BTreeMap;
use interface::TextAlign;
use atlas::CoordsInfo;
use std::sync::atomic::{self,AtomicPtr};
use parking_lot::Mutex;
use crossbeam::queue::MsQueue;
use std::collections::HashMap;
use std::thread;

pub struct Text {
	queue: MsQueue<Request>,
	fonts: Mutex<HashMap<FontKey, Font>>,
	render_thread: Mutex<Option<thread::JoinHandle<()>>>,
}

struct Font {
	ft_library: AtomicPtr<FT_LibraryRec_>,
	ft_face: AtomicPtr<FT_FaceRec_>,
	hb_font: AtomicPtr<hb_font_t>,
	hb_buffer: AtomicPtr<hb_buffer_t>,
	start_y: f32,
	line_height: f32,
	glyphs: BTreeMap<u64, Glyph>,
}

impl Font {
	fn new(size: u32) -> Result<Self, String> {
		unsafe {
			let mut ft_library = ptr::null_mut();

			match FT_Init_FreeType(&mut ft_library) {
				0 => (),
				e => return Err(format!("FT_Init_FreeType: error {}", e))
			}
			
			let mut ft_face = ptr::null_mut();
			let bytes = include_bytes!("ABeeZee-Regular.ttf");
			//let bytes = include_bytes!("/usr/share/fonts/TTF/Amiri-Regular.ttf");
			//let bytes = include_bytes!("/usr/share/fonts/TTF/SourceSansPro-Regular.ttf");
			
			match FT_New_Memory_Face(ft_library, bytes.as_ptr(), (bytes.len() as i32).into(), 0, &mut ft_face) {
				0 => (),
				e => return Err(format!("FT_New_Memory_Face: error {}", e))
			}
			
			match FT_Set_Pixel_Sizes(ft_face, 0, size.into()) {
				0 => (),
				e => return Err(format!("FT_Set_Pixel_Sizes: error {}", e))
			}
			
			let hb_font = hb_ft_font_create_referenced(ft_face);
			let hb_buffer = hb_buffer_create();
			let ctext = CString::new("Tg").unwrap();
			hb_buffer_add_utf8(hb_buffer, ctext.as_ptr(), -1, 0, -1);
			hb_buffer_guess_segment_properties (hb_buffer);
			hb_shape(hb_font, hb_buffer, ptr::null_mut(), 0);
			let len = hb_buffer_get_length(hb_buffer) as usize;
			let info = ::std::slice::from_raw_parts(hb_buffer_get_glyph_infos(hb_buffer, ptr::null_mut()), len);
			let pos = std::slice::from_raw_parts(hb_buffer_get_glyph_positions(hb_buffer, ptr::null_mut()), len);
			assert!(len == 2);
			
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
			let start_y = -top;
			let line_height = (bottom-top) + (size as f32 / 6.0).ceil();
			hb_buffer_reset(hb_buffer);
			
			Ok(Font {
				ft_library: AtomicPtr::new(ft_library),
				ft_face: AtomicPtr::new(ft_face),
				hb_font: AtomicPtr::new(hb_font),
				hb_buffer: AtomicPtr::new(hb_buffer),
				start_y,
				line_height,
				glyphs: BTreeMap::new()
			})
		}
	}
}

impl Drop for Font {
	fn drop(&mut self) {
		unsafe {
			let ft_library = self.ft_library.swap(ptr::null_mut(), atomic::Ordering::Relaxed);
			let ft_face = self.ft_face.swap(ptr::null_mut(), atomic::Ordering::Relaxed);
			let hb_font = self.hb_font.swap(ptr::null_mut(), atomic::Ordering::Relaxed);
			let hb_buffer = self.hb_buffer.swap(ptr::null_mut(), atomic::Ordering::Relaxed);
			hb_buffer_destroy(hb_buffer);
	  		hb_font_destroy(hb_font);
			FT_Done_Face(ft_face);
			FT_Done_Library(ft_library);
		}
	}
}
		
#[derive(PartialEq,Eq,Hash,Clone)]
struct FontKey {
	size: u32,
}

struct Request {
	text: String,
	family: String,
	size: u32,
	color: (f32, f32, f32, f32),
	wrap: WrapTy,
	align: TextAlign,
	resbar: Arc<Barrier>,
	res: Arc<Mutex<Result<BTreeMap<usize, Vec<ItfVertInfo>>, String>>>,
}

struct Glyph {
	w: f32,
	h: f32,
	hbx: f32,
	hby: f32,
	coords: CoordsInfo,
}

impl Text {
	pub(crate) fn render_text<T: Into<String>, F: Into<String>>(
		&self, text: T, family: F, size: u32, color: (f32, f32, f32, f32),
		wrap: WrapTy, align: TextAlign,
	) -> Result<BTreeMap<usize, Vec<ItfVertInfo>>, String> {
		let resbar = Arc::new(Barrier::new(2));
		let resbar_cp = resbar.clone();
		let res = Arc::new(Mutex::new(Err(format!("Checked result too early."))));
		let res_cp = res.clone();
	
		let request = Request {
			text: text.into(),
			family: family.into(),
			size, color, wrap,
			align, resbar, res
		};
		
		self.queue.push(request);
		let mut result = Err(format!("Result has been taken."));
		resbar_cp.wait();
		::std::mem::swap(&mut *res_cp.lock(), &mut result);
		result
	}

	pub fn new(engine: Arc<Engine>) -> Arc<Self> {
		let text_ret = Arc::new(Text {
			queue: MsQueue::new(),
			fonts: Mutex::new(HashMap::new()),
			render_thread: Mutex::new(None),
		});
		
		let text = text_ret.clone();
		*text_ret.render_thread.lock() = Some(thread::spawn(move || unsafe {
			let mut fonts = text.fonts.lock();
		
			'request: loop {
				let request = text.queue.pop();
				let font_key = FontKey {
					size: request.size,
				};
				
				if !fonts.contains_key(&font_key) {
					fonts.insert(font_key.clone(), match Font::new(request.size) {
						Ok(ok) => ok,
						Err(e) => {
							*request.res.lock() = Err(e);
							request.resbar.wait();
							continue;
						}
					});
				}
				
				let font = fonts.get_mut(&font_key).unwrap();
				let ft_face = font.ft_face.load(atomic::Ordering::Relaxed);
				let hb_font = font.hb_font.load(atomic::Ordering::Relaxed);
				let hb_buffer = font.hb_buffer.load(atomic::Ordering::Relaxed);
				let clines: Vec<CString> = request.text.lines().map(|v| CString::new(v).unwrap()).collect();
				let mut current_x = 0.0;
				let mut current_y = font.start_y;
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
						match FT_Load_Glyph(ft_face, info[i].codepoint.into(), FT_LOAD_DEFAULT as i32) {
							0 => (),
							e => {
								*request.res.lock() = Err(format!("FT_Load_Glyph: error {}", e));
								request.resbar.wait();
								continue;
							}
						}
						
						if !font.glyphs.contains_key(&(info[i].codepoint as u64)) {
							let mut glyph = *(*ft_face).glyph;
							
							match FT_Render_Glyph(&mut glyph, FT_Render_Mode::FT_RENDER_MODE_NORMAL) {
								0 => (),
								e => {
									*request.res.lock() = Err(format!("FT_Render_Glyph: error {}", e));
									request.resbar.wait();
									continue 'request;
								}
							}
							
							let bitmap = glyph.bitmap;
							let w = bitmap.width as usize;
							let h = bitmap.rows as usize;
							
							if w == 0 || h == 0 {
								font.glyphs.insert(info[i].codepoint as u64, Glyph {
									w: 0.0,
									h: 0.0,
									hbx: 0.0,
									hby: 0.0,
									coords: CoordsInfo::none()
								});
							} else {
								let mut image_data = Vec::with_capacity(w * h * 4);
						
								for i in 0..((w*h) as isize) {
									image_data.push(0);
									image_data.push(0);
									image_data.push(0);
									image_data.push(*bitmap.buffer.offset(i));
								}
								
								let coords = match engine.atlas_ref().load_raw_with_key(
									&atlas::ImageKey::Glyph(request.size, info[i].codepoint as u64),
									image_data, w as u32, h as u32
								) {
									Ok(ok) => ok,
									Err(e) => {
										*request.res.lock() = Err(format!("Atlas::load_raw_with_key: Error {}", e));
										request.resbar.wait();
										continue 'request;
									}
								};
								
								font.glyphs.insert(info[i].codepoint as u64, Glyph {
									w: glyph.metrics.width as f32 / 64.0,
									h: glyph.metrics.height as f32 / 64.0,
									hbx: glyph.metrics.horiBearingX as f32 / 64.0,
									hby: glyph.metrics.horiBearingY as f32 / 64.0,
									coords,
								});
							}
						}
						
						let glyph_info = font.glyphs.get(&(info[i].codepoint as u64)).unwrap();
						
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
							verts.push(ItfVertInfo { position: tr, coords: ctr, color: request.color, ty: 1 });
							verts.push(ItfVertInfo { position: tl, coords: ctl, color: request.color, ty: 1 });
							verts.push(ItfVertInfo { position: bl, coords: cbl, color: request.color, ty: 1 });
							verts.push(ItfVertInfo { position: tr, coords: ctr, color: request.color, ty: 1 });
							verts.push(ItfVertInfo { position: bl, coords: cbl, color: request.color, ty: 1 });
							verts.push(ItfVertInfo { position: br, coords: cbr, color: request.color, ty: 1 });
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
				
				match request.wrap {
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
								let xoffset = match request.align {
									TextAlign::Left => -start,
									TextAlign::Center => ((w - lwidth) / 2.0) - start,
									TextAlign::Right => (w - lwidth) - start,
								};
								let yoffset = line_i as f32 * font.line_height;
							
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
				
				*request.res.lock() = Ok(vert_map);
				request.resbar.wait();
			}
		}));
		
		text_ret
	}
}

