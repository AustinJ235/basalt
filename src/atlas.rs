use std::path::{PathBuf,Path};
use std::collections::{BTreeMap,HashMap};
use super::texture;
use std::sync::Arc;
use vulkano::device::{self,Device};
use vulkano::image::immutable::ImmutableImage;
use vulkano::image::traits::ImageViewAccess;
use vulkano;
use vulkano::sampler::Sampler;
use parking_lot::{RwLock,Mutex};
use super::interface::font::Font;
use super::interface::interface::ItfVertInfo;
use misc::BTreeMapExtras;
use Limits;
use interface::TextWrap;
use vulkano::image::StorageImage;
use vulkano::image::Dimensions as VkDimensions;
use vulkano::image::ImageUsage as VkImageUsage;
use vulkano::buffer::BufferUsage as VkBufferUsage;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::command_buffer::AutoCommandBuffer;

const A_IMG_PADDING: u32 = 3;

#[allow(dead_code)]
pub struct Atlas {
	images: RwLock<BTreeMap<usize, Mutex<AtlasImage>>>,
	image_i: Mutex<usize>,
	null_img: Mutex<Option<Arc<ImageViewAccess + Send + Sync>>>,
	fonts: RwLock<BTreeMap<u32, Mutex<Font>>>,
	limits: Arc<Limits>,
}

impl Atlas {
	pub(crate) fn new(limits: Arc<Limits>) -> Self {
		Atlas {
			images: RwLock::new(BTreeMap::new()),
			image_i: Mutex::new(1),
			null_img: Mutex::new(None),
			fonts: RwLock::new(BTreeMap::new()),
			limits: limits,
		}
	}
	
	pub(crate) fn text_verts<S: AsRef<str>>(&self, size: f32, from: [f32; 2], to_: Option<[f32; 2]>, mode: TextWrap, color: (f32, f32, f32, f32), text: S) -> Result<(BTreeMap<usize, Vec<ItfVertInfo>>, f32), String> {
		let mut verts_map: BTreeMap<usize, Vec<ItfVertInfo>> = BTreeMap::new();
		let mut max_ht = None;
		let use_size = f32::ceil(size) as u32;
			
		match self.fonts.read().get(&use_size) {
			Some(some) => max_ht = Some(some.lock().max_ht()),
			None => ()
		}
		
		if max_ht.is_none() {
			let font = match Font::new(use_size) {
				Ok(ok) => ok,
				Err(e) => return Err(format!("Failed to create font: {}", e))
			};
			
			max_ht = Some(font.max_ht());
			self.fonts.write().insert(use_size, Mutex::new(font));
		}
		
		let max_ht = max_ht.unwrap() as f32;
		let mut y_off = from[1];
		let mut x_off = from[0];
		let mut max_y = 0.0;
		
		for c in text.as_ref().chars() {
			let code = c as u64;
			
			if code == 10 {
				y_off += max_ht;
				x_off = from[0];
				continue;
			}
			
			let coords = match self.coords_for_glyph(use_size, code) {
				Ok(ok) => ok,
				Err(e) => {
					println!("UI Error while creating text verts: {}", e);
					continue;
				}
			};
			
			let w = coords.w as f32;
			let h = coords.h as f32;
			let bx = coords.glyph_props.as_ref().unwrap().bx as f32;
			let by = coords.glyph_props.as_ref().unwrap().by as f32;
			let adv = coords.glyph_props.as_ref().unwrap().adv as f32;
			
			let mut sx = x_off + bx;
			let mut sy = y_off + (max_ht - by);
			let mut cut_y = 0.0;
			let mut cut_x = 0.0;
			
			// Reached right boundry so line break
			if to_.is_some() && sx + w > to_.as_ref().unwrap()[0] {
				match mode {
					TextWrap::NewLine => {
						y_off += max_ht;
						x_off = from[0];
						sx = x_off + bx;
						sy = y_off + (max_ht - by);
					}, TextWrap::None => {
						cut_x = (sx + w) - to_.as_ref().unwrap()[0];
						
						if cut_x > w {
							continue;
						}
					}, TextWrap::Shift => {
						let shift_amt = (sx + w) - to_.as_ref().unwrap()[0];
						x_off -= shift_amt;
						sx -= shift_amt;
						
						for (_, verts) in &mut verts_map {
							for vert in verts {
								vert.position.0 -= shift_amt;
							}
						}
					}
				}		
			}
			
			if sy + h > max_y {
				max_y = sy + h;
			}
			
			// Reached bottom boundry so cutoff letters
			if to_.is_some() && sy + h > to_.as_ref().unwrap()[1] {
				cut_y = (sy + h) - to_.as_ref().unwrap()[1];
				
				if cut_y > h {
					continue;
				}
			}
			
			let tl = (sx, sy, 0.0);
			let mut tr = (sx+w, sy, 0.0);
			let mut bl = (sx, sy+h, 0.0);
			let mut br = (sx+w, sy+h, 0.0);
			let ctl = coords.f32_top_left();
			let mut ctr = coords.f32_top_right();
			let mut cbl = coords.f32_bottom_left();
			let mut cbr = coords.f32_bottom_right();
			
			bl.1 -= cut_y;
			br.1 -= cut_y;
			cbl.1 -= cut_y;
			cbr.1 -= cut_y;
			
			tr.0 -= cut_x;
			br.0 -= cut_x;
			ctr.0 -= cut_x;
			cbr.0 -= cut_x;
			
			let verts = verts_map.get_mut_or_else(&coords.atlas_i, || { Vec::new() });
			verts.push(ItfVertInfo { position: tr, coords: ctr, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: tl, coords: ctl, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: bl, coords: cbl, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: tr, coords: ctr, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: bl, coords: cbl, color: color, ty: 1 });
			verts.push(ItfVertInfo { position: br, coords: cbr, color: color, ty: 1 });
			
			x_off += adv;
		}
		
		match mode {
			TextWrap::Shift => {
				for (_, verts) in &mut verts_map {
					let mut remove = Vec::new();
					
					for g in 0..(verts.len()/6) {
						let cut = from[0] - verts[(g*6)+1].position.0;
						
						if cut < 0.0 {
							continue;
						}
						
						let width = verts[g*6].position.0 - verts[(g*6)+1].position.0;

						if cut > width {
							for v in 0..6 {
								remove.push(g*v);
								continue;
							}
						}
						
						verts[(g*6)+1].position.0 += cut;
						verts[(g*6)+2].position.0 += cut;
						verts[(g*6)+4].position.0 += cut;
						verts[(g*6)+1].coords.0 += cut;
						verts[(g*6)+2].coords.0 += cut;
						verts[(g*6)+4].coords.0 += cut;
					}
					
					for i in remove.into_iter().rev() {
						verts.remove(i);
					}
				}
			}, _ => ()
		}

		Ok((verts_map, max_y))
	}
	
	pub fn coords_for_glyph(&self, size: u32, code: u64) -> Result<CoordsInfo, String> {
		let mut glyph_info = None;
		let mut create_font = false;

		match self.fonts.read().get(&size) {
			Some(font_) => match font_.lock().get_glyph(code) {
				Ok(ok) => glyph_info = Some(ok),
				Err(e) => return Err(format!("Failed to load glyph: {}", e))
			}, None => create_font = true
		}
		
		if create_font {
			let mut font = match Font::new(size) {
				Ok(ok) => ok,
				Err(e) => return Err(format!("Failed to load font: {}", e))
			}; match font.get_glyph(code) {
				Ok(ok) => glyph_info = Some(ok),
				Err(e) => return Err(format!("Failed to laod glyph: {}", e))
			} self.fonts.write().insert(size, Mutex::new(font));
		}
		
		let glyph_info = glyph_info.unwrap();
		let mut data = Vec::new();
		
		for val in glyph_info.bitmap {
			data.push(0);
			data.push(0);
			data.push(0);
			data.push(val);
		}
		
		for (atlas_i, image_) in &*self.images.read() {
			let mut image = image_.lock();
			
			if image.will_fit(glyph_info.width as u32, glyph_info.height as u32) {
				return match image.load_raw(&ImageKey::Glyph(size, code), data, glyph_info.width as u32, glyph_info.height as u32) {
					Ok(mut coords) => {
						coords.atlas_i = *atlas_i;
						coords.glyph_props = Some(GlyphProps {
							bx: glyph_info.bearing_x,
							by: glyph_info.bearing_y,
							adv: glyph_info.advance
						}); Ok(coords)
					}, Err((_, e)) => Err(format!("Failed to load glyph data into atlas: {}", e))
				}
			}
		}
		
		let mut new_image = AtlasImage::new(&self.limits);
		let mut image_i = self.image_i.lock();
		
		let coords = match new_image.load_raw(&ImageKey::Glyph(size, code), data, glyph_info.width as u32, glyph_info.height as u32) {
			Ok(mut coords) => {
				coords.atlas_i = *image_i;
				coords.glyph_props = Some(GlyphProps {
					bx: glyph_info.bearing_x,
					by: glyph_info.bearing_y,
					adv: glyph_info.advance
				}); coords
			}, Err((_, e)) => return Err(format!("Failed to laod glyph data into atlas: {}", e))
		};
		
		self.images.write().insert(*image_i, Mutex::new(new_image));
		*image_i += 1;
		Ok(coords)
	}
	
	pub fn remove_raw(&self, raw_id: u64) {
		let key = ImageKey::RawId(raw_id);
		
		for (_, image_mu) in  &*self.images.read() {
			image_mu.lock().remove(&key);
		}
	}
	
	pub fn load_raw_with_key(&self, key: &ImageKey, mut data: Vec<u8>, width: u32, height: u32) -> Result<CoordsInfo, String> {
		for (i, image_mu) in &*self.images.read() {
			match image_mu.lock().load_raw(key, data, width, height) {
				Ok(mut coords) => {
					coords.atlas_i = *i;
					return Ok(coords);
				}, Err((ret_data, _)) => {
					data = ret_data;
				}
			}
		}
		
		let mut new_image = AtlasImage::new(&self.limits);
		let mut image_i = self.image_i.lock();
		
		let coords = match new_image.load_raw(key, data, width, height) {
			Ok(mut coords) => {
				coords.atlas_i = *image_i;
				coords
			}, Err((_, e)) => return Err(e)
		};
		
		self.images.write().insert(*image_i, Mutex::new(new_image));
		*image_i += 1;
		Ok(coords)
	}
	
	pub fn load_raw(&self, raw_id: u64, data: Vec<u8>, width: u32, height: u32) -> Result<CoordsInfo, String> {
		self.load_raw_with_key(&ImageKey::RawId(raw_id), data, width, height)
	}
	
	pub fn coords_with_path<P: AsRef<Path>>(&self, path: P) -> Result<CoordsInfo, String> {
		for (i, image_) in &*self.images.read() {
			match image_.lock().coords_with_path(&path) {
				Ok(mut coords) => {
					coords.atlas_i = *i;
					return Ok(coords);
				}, Err(_) => ()
			}
		}
		
		let mut new_image = AtlasImage::new(&self.limits);
		let mut image_i = self.image_i.lock();
		
		let coords = match new_image.coords_with_path(&path) {
			Ok(mut coords) => {
				coords.atlas_i = *image_i;
				coords
			}, Err(e) => return Err(e)
		};

		self.images.write().insert(*image_i, Mutex::new(new_image));
		*image_i += 1;
		Ok(coords)
	}
	
	pub(crate) fn update(&self, device: Arc<Device>, queue: Arc<device::Queue>)
		-> Vec<AutoCommandBuffer<vulkano::command_buffer::pool::standard::StandardCommandPoolAlloc>>
	{
		let mut out = Vec::new();
		
		for (i, image_) in &*self.images.read() {
			if let Some(cmd_buf) = image_.lock().update(*i, device.clone(), queue.clone()) {
				out.push(cmd_buf);
			}
		}
		
		out
	}
	
	pub fn null_img(&self, queue: Arc<device::Queue>) -> Arc<ImageViewAccess + Send + Sync> {
		let mut null_img_ = self.null_img.lock();
		
		if let Some(some) = null_img_.as_ref() {
			return some.clone();
		}
		
		*null_img_ = Some(ImmutableImage::from_iter(
			vec![0,0,0,0].into_iter(),
			vulkano::image::Dimensions::Dim2d {
				width: 1,
				height: 1,
			}, vulkano::format::Format::R8G8B8A8Srgb,
			queue
		).unwrap().0);
		
		null_img_.as_ref().unwrap().clone()
	}

	pub fn image_and_sampler(&self, id: usize) -> Option<(Arc<ImageViewAccess + Send + Sync>, Arc<Sampler>)> {
		match self.images.read().get(&id) {
			Some(some) => some.lock().image_and_sampler(),
			None => None
		}
	}
}

struct AtlasImage {
	freespaces: Vec<FreeSpace>,
	stored: HashMap<ImageKey, ImageInfo>,
	image: Option<Arc<StorageImage<vulkano::format::Format>>>,
	sampler: Option<Arc<Sampler>>,
	update: bool,
	max_img_w: u32,
	max_img_h: u32,
}

impl AtlasImage {
	fn new(limits: &Arc<Limits>) -> AtlasImage {
		AtlasImage {
			freespaces: vec![
				FreeSpace {
					x: A_IMG_PADDING,
					y: A_IMG_PADDING,
					w: limits.max_image_dimension_2d - (A_IMG_PADDING * 2),
					h: limits.max_image_dimension_2d - (A_IMG_PADDING * 2)
				}
			], stored: HashMap::new(),
			image: None,
			sampler: None,
			update: false,
			max_img_w: limits.max_image_dimension_2d,
			max_img_h: limits.max_image_dimension_2d,
		}
	}
	
	fn remove(&mut self, key: &ImageKey) {
		if let Some(ImageInfo {
			x,
			y,
			w,
			h,
			..
		}) = self.stored.remove(key) {
			self.freespaces.push(FreeSpace {
				x: x,
				y: y,
				w: w,
				h: h
			});
			
			self.update = true;
		}
	}
	
	fn image_and_sampler(&self) -> Option<(Arc<ImageViewAccess + Send + Sync>, Arc<Sampler>)> {
		if self.image.is_none() || self.sampler.is_none() {
			None
		} else {
			Some((self.image.as_ref().unwrap().clone(), self.sampler.as_ref().unwrap().clone()))
		}
	}
	
	fn will_fit(&self, w: u32, h: u32) -> bool {
		if w <= self.max_img_w && h <= self.max_img_h {
			true
		} else {
			false
		}
	}
	
	fn update_max_img_dims(&mut self) {
		for fs in &self.freespaces {
			if fs.w > self.max_img_w {
				self.max_img_w = fs.w;
			} else if fs.h > self.max_img_h {
				self.max_img_h = fs.h;
			}
		}
	}
	
	fn get_free_space(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
		let mut options = Vec::new();
		let mut i = 0_usize;
		
		loop {
			if i >= self.freespaces.len() {
				break;
			} if self.freespaces[i].w >= width && self.freespaces[i].h >= height {
				let freespace_area = self.freespaces[i].w * self.freespaces[i].h;
				options.push((i, freespace_area));
			} i += 1;
		}
		
		if options.is_empty() {
			return None;
		}
		
		options.sort_unstable_by_key(|v| v.1);
		options.reverse();
		
		let (split_i, split_area) = options.pop().unwrap();
		let fs = self.freespaces.swap_remove(split_i);
		self.update_max_img_dims();
		
		if split_area == width * height {
			return Some((fs.x, fs.y));
		}
		
		if fs.w > width + A_IMG_PADDING {
			self.freespaces.push(FreeSpace {
				x: fs.x + width + A_IMG_PADDING,
				y: fs.y,
				w: fs.w - width - A_IMG_PADDING,
				h: height
			});
		}
		
		if fs.h > height + A_IMG_PADDING {
			self.freespaces.push(FreeSpace {
				x: fs.x,
				y: fs.y + height + A_IMG_PADDING,
				w: fs.w,
				h: fs.h - height - A_IMG_PADDING,
			});
		}
		
		Some((fs.x, fs.y))
	}
	
	fn load_raw(&mut self, key: &ImageKey, data: Vec<u8>, width: u32, height: u32) -> Result<CoordsInfo, (Vec<u8>, String)> {
		if let Some(image_info) = self.stored.get(key) {
			return Ok(image_info.coords_info());
		}
		
		let mut opaque = true;
		
		for chunk in data.chunks(4) {
			if chunk[3] != 255 {
				opaque = false;
				break;
			}
		}
		
		let (atlas_x, atlas_y) = match self.get_free_space(width, height) {
			Some(some) => some,
			None => return Err((data, format!("No room left in atlas for this imge.")))
		}; let image_info = ImageInfo {
			update: true,
			data: data,
			opaque: opaque,
			x: atlas_x,
			y: atlas_y,
			w: width,
			h: height
		};
		
		self.stored.insert(key.clone(), image_info.clone());
		self.update = true;
		Ok(image_info.coords_info())
	}
	
	fn coords_with_path<P: AsRef<Path>>(&mut self, path: P) -> Result<CoordsInfo, String> {
		let store_key = ImageKey::Path(path.as_ref().to_path_buf());
		
		if let Some(image_info) = self.stored.get(&store_key) {
			return Ok(image_info.coords_info());
		}
		
		let load_res = match texture::load_image(&path) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to load image: {}", e))
		}; let (atlas_x, atlas_y) = match self.get_free_space(load_res.width, load_res.height) {
			Some(some) => some,
			None => return Err(format!("No room left in atlas for this image."))
		}; let mut opaque = true;
		
		for chunk in load_res.data.chunks(4) {
			if chunk[3] != 255 {
				opaque = false;
				break;
			}
		}
		
		let image_info = ImageInfo {
			update: true,
			data: load_res.data,
			opaque: opaque,
			x: atlas_x,
			y: atlas_y,
			w: load_res.width,
			h: load_res.height
		};
		
		self.stored.insert(store_key, image_info.clone());
		self.update = true;
		Ok(image_info.coords_info())
	}

	fn update(&mut self, atlas_i: usize, device: Arc<Device>, queue: Arc<device::Queue>)
		-> Option<AutoCommandBuffer<vulkano::command_buffer::pool::standard::StandardCommandPoolAlloc>>
	{
		if !self.update {
			return None;
		}

		let mut need_w = 0;
		let mut need_h = 0;
		
		for (_, info) in &self.stored {
			if info.x + info.w > need_w {
				need_w = info.x + info.w;
			} if info.y + info.h > need_h {
				need_h = info.y + info.h;
			}
		}
		
		let mut cmd_buf = AutoCommandBufferBuilder::new(device.clone(), queue.family()).unwrap();
		let mut cmds = 0;
		
		if match self.image.as_mut() {
			Some(cur_img) => if let VkDimensions::Dim2d { width, height } = cur_img.dimensions() {
				if width < need_w || height < need_h {
					true
				} else {
					false
				}
			} else {
				unreachable!()
			}, None => true
		} {
			let mut new_w = f32::floor(need_w as f32 * 1.5) as u32;
			let mut new_h = f32::floor(need_h as f32 * 1.5) as u32;
			
			if new_w > self.max_img_w {
				new_w = self.max_img_w;
			} if new_h > self.max_img_h {
				new_h = self.max_img_h;
			}
		
			let new_img = StorageImage::with_usage(
				device.clone(),
				vulkano::image::Dimensions::Dim2d {
					width: new_w,
					height: new_h,
				},
				vulkano::format::Format::R8G8B8A8Unorm,
				VkImageUsage {
					transfer_source: true,
					transfer_destination: true,
					sampled: true,
					color_attachment: true,
					.. VkImageUsage::none()
				},
				vec![queue.family()]
			).unwrap();
			
			let zeros_len = (new_w * new_h * 4) as usize;
			let mut zeros = Vec::with_capacity(zeros_len);
			zeros.resize(zeros_len, 0_u8);
			
			let tmp_buf = CpuAccessibleBuffer::from_iter(
				device.clone(),
				VkBufferUsage {
					transfer_source: true,
					.. VkBufferUsage::none()
				},
				zeros.into_iter()
			).unwrap();
			
			cmd_buf = cmd_buf.copy_buffer_to_image(tmp_buf, new_img.clone()).unwrap();
			
			if let Some(old_img) = self.image.take() {
				let (old_w, old_h) = match old_img.dimensions() {
					VkDimensions::Dim2d { width, height } => (width, height),
					_ => unreachable!()
				};
			
				cmd_buf = cmd_buf.copy_image(
					old_img, [0, 0, 0], 0, 0,
					new_img.clone(), [0, 0, 0], 0, 0,
					[old_w, old_h, 1], 1
				).unwrap();
			}
			
			self.image = Some(new_img);
			cmds += 1;
			println!("AtlasImage({}) resized to {}x{}", atlas_i, new_w, new_h);
		}
		
		let mut tmp_data = Vec::new();
		let mut copy_cmds = Vec::new();
		let mut updated = Vec::new();
		
		for (key, mut info) in &mut self.stored {
			if info.update {
				updated.push(key.clone());
				let offset = tmp_data.len();
				tmp_data.append(&mut info.data.clone());
				copy_cmds.push((offset, info.x, info.y, info.w, info.h));
			}
		}
		
		let tmp_buf = CpuAccessibleBuffer::from_iter(
			device.clone(),
			VkBufferUsage {
				transfer_source: true,
				.. VkBufferUsage::none()
			},
			tmp_data.into_iter()
		).unwrap();
		
		for (buffer_offset, x, y, w, h) in copy_cmds {
			cmd_buf = cmd_buf.copy_buffer_to_image_advance(
				tmp_buf.clone(),
				self.image.as_ref().unwrap().clone(),
				buffer_offset, w, h,
				[x, y, 0],
				[w, h, 1],
				0, 1, 0
			).unwrap();
			cmds += 1;
		}
		
		if self.sampler.is_none() {
			self.sampler = Some(Sampler::unnormalized(
				device,
				vulkano::sampler::Filter::Nearest,
				vulkano::sampler::UnnormalizedSamplerAddressMode::ClampToBorder(
					vulkano::sampler::BorderColor::FloatTransparentBlack
				), vulkano::sampler::UnnormalizedSamplerAddressMode::ClampToBorder(
					vulkano::sampler::BorderColor::FloatTransparentBlack
				)
			).unwrap());
		}
		
		if cmds > 0 {
			for key in updated {
				if let Some(info) = self.stored.get_mut(&key) {
					//info.update = false;
				}
			}
		
			self.update = false;
			Some(cmd_buf.build().unwrap())
		} else {
			None
		}
	}
}

struct FreeSpace {
	x: u32,
	y: u32,
	w: u32,
	h: u32,
}

#[derive(Clone)]
struct ImageInfo {
	update: bool,
	data: Vec<u8>,
	opaque: bool,
	x: u32,
	y: u32,
	w: u32,
	h: u32,
}

#[derive(Clone)]
pub struct CoordsInfo {
	pub x: u32,
	pub y: u32,
	pub w: u32,
	pub h: u32,
	pub opaque: bool,
	pub atlas_i: usize,
	glyph_props: Option<GlyphProps>,
}

#[derive(Clone)]
struct GlyphProps {
	bx: i32,
	by: i32,
	adv: i32
}

impl CoordsInfo {
	pub fn none() -> Self {
		CoordsInfo {
			x: 0,
			y: 0,
			w: 0,
			h: 0,
			opaque: true,
			atlas_i: 0,
			glyph_props: None
		}
	} pub fn f32_top_left(&self) -> (f32, f32) {
		(self.x as f32, self.y as f32)
	} pub fn f32_top_right(&self) -> (f32, f32) {
		((self.x + self.w) as f32, self.y as f32)
	} pub fn f32_bottom_left(&self) -> (f32, f32) {
		(self.x as f32, (self.y + self.h) as f32)
	} pub fn f32_bottom_right(&self) -> (f32, f32) {
		((self.x + self.w) as f32, (self.y + self.h) as f32)
	}
}

impl ::std::fmt::Debug for CoordsInfo {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "CoordsInfo {{ x: {}, y: {}, w: {}, h: {}, opaque: {}, atlas_i: {} }}",
        	self.x, self.y, self.w, self.h, self.opaque, self.atlas_i)
    }
}

impl ImageInfo {
	fn coords_info(&self) -> CoordsInfo {
		CoordsInfo {
			x: self.x,
			y: self.y,
			w: self.w,
			h: self.h,
			opaque: self.opaque,
			atlas_i: 0,
			glyph_props: None
		}
	}
}

#[derive(Clone,PartialEq,Eq,Hash)]
pub enum ImageKey {
	Path(PathBuf),
	Glyph(u32, u64),
	RawId(u64),
}

