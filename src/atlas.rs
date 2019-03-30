use Engine;
use tmp_image_access::TmpImageViewAccess;
use std::sync::Arc;
use std::thread;
use std::time::{Duration,Instant};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs::File;
use std::io::Read;
use std::sync::atomic::{self,AtomicBool};
use crossbeam::channel::{self,Sender,Receiver};
use parking_lot::{Mutex,Condvar};
use image;
use image::GenericImageView;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;
use vulkano::image::StorageImage;
use vulkano::image::Dimensions as VkDimensions;
use vulkano::image::ImageDimensions as VkImgDimensions;
use vulkano::image::ImageUsage as VkImageUsage;
use vulkano::buffer::BufferUsage as VkBufferUsage;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferAccess;
use vulkano::sampler::Sampler;
use vulkano::image::ImageViewAccess;
use vulkano::image::ImageAccess;
use vulkano::image::immutable::ImmutableImage;

#[inline]
fn index3d(a: usize, b: usize, c: usize, _aw: usize, bw: usize, cw: usize) -> usize {
	(a * bw * cw) + (b * cw) + c
}

const ITER_DURATION: Duration = Duration::from_millis(1);
const CELL_WIDTH: u32 = 32;
const CELL_PAD: u32 = 4;

const ALT_UPDATE: bool = false;

pub type AtlasImageID = u64;
pub type SubImageID = u64;

#[derive(Debug,Clone,PartialEq,Eq,Hash)]
pub enum SubImageCacheID {
	Path(PathBuf),
	Url(String),
	Glyph(u32, u64),
	None
}

#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct Coords {
	pub img_id: AtlasImageID,
	pub sub_img_id: SubImageID,
	pub x: u32,
	pub y: u32,
	pub w: u32,
	pub h: u32
}

impl Coords {
	pub fn none() -> Self {
		Coords {
			img_id: 0,
			sub_img_id: 0,
			x: 0,
			y: 0,
			w: 0,
			h: 0,
		}
	}
	
	pub fn top_left(&self) -> (f32, f32) {
		(self.x as f32, self.y as f32)
	}
	
	pub fn top_right(&self) -> (f32, f32) {
		((self.x + self.w) as f32, self.y as f32)
	}
	
	pub fn bottom_left(&self) -> (f32, f32) {
		(self.x as f32, (self.y + self.h) as f32)
	}
	
	pub fn bottom_right(&self) -> (f32, f32) {
		((self.x + self.w) as f32, (self.y + self.h) as f32)
	}
}

#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct ImageDims {
	pub w: u32,
	pub h: u32
}

#[derive(Debug,Clone)]
pub enum ImageData {
	D8(Vec<u8>),
	#[doc(hidden)]
    __Nonexhaustive,
}

#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub enum ImageType {
	LRGBA,
	LRGB,
	LMono,
	SRGBA,
	SRGB,
	YUV444,
}

impl ImageType {
	pub fn components(&self) -> usize {
		match self {
			&ImageType::LRGBA => 4,
			&ImageType::LRGB => 3,
			&ImageType::LMono => 1,
			&ImageType::SRGBA => 4,
			&ImageType::SRGB => 3,
			&ImageType::YUV444 => 3,
		}
	}
}

pub struct Image {
	ty: ImageType,
	dims: ImageDims,
	data: ImageData,
}

impl Image {
	pub fn new(ty: ImageType, dims: ImageDims, mut data: ImageData) -> Result<Image, String> {
		let expected_len = dims.w as usize * dims.h as usize * ty.components();
		
		if expected_len == 0 {
			return Err(format!("Image can't be empty"));
		}
		
		match &mut data {
			&mut ImageData::D8(ref mut d) => if d.len() > expected_len {
				d.truncate(expected_len);
			} else if d.len() < expected_len {
				return Err(format!("Data length doesn't match the provided dimensions."));
			},
			_ => unreachable!()
		}
	
		Ok(Image {
			ty, dims, data
		})
	}
	
	fn to_lrgba(self) -> Self {
		if let ImageData::D8(data) = self.data {
			let mut lrgba = Vec::with_capacity(data.len() / self.ty.components() * 4);
		
			match self.ty {
				ImageType::LRGBA => lrgba = data,
				ImageType::LRGB => {
					for v in data {
						lrgba.push(v);
						
						if lrgba.len() % 4 == 2 {
							lrgba.push(255);
						}
					}
				},
				ImageType::LMono => {
					for v in data {
						lrgba.push(v);
						lrgba.push(v);
						lrgba.push(v);
						lrgba.push(255);
					}
				},
				ImageType::SRGBA => {
					for v in data {
						let mut v = ((v as f32 + (0.055 * 255.0)) / 1.055).powf(2.4).round();
						
						if v > 255.0 {
							v = 255.0;
						} else if v < 0.0 {
							v = 0.0;
						}
						
						lrgba.push(v as u8);
					}
				}, ImageType::SRGB => {
					for v in data {
						let mut v = ((v as f32 + (0.055 * 255.0)) / 1.055).powf(2.4).round();
						
						if v > 255.0 {
							v = 255.0;
						} else if v < 0.0 {
							v = 0.0;
						}
						
						lrgba.push(v as u8);
						
						if lrgba.len() % 4 == 2 {
							lrgba.push(255);
						}
					}
				}, ImageType::YUV444 => {
					for chunk in data.chunks_exact(3) {
						let mut components = [
							chunk[0] as f32 + (1.402 * (chunk[2] as f32 - 128.0)),
							chunk[0] as f32 + (0.344 * (chunk[1] as f32 - 128.0))
								- (0.714 * (chunk[2] as f32 - 128.0)),
							chunk[0] as f32 + (1.772 * (chunk[1] as f32 - 128.0))
						];
						
						for v in &mut components {
							*v = ((*v + (0.055 * 255.0)) / 1.055).powf(2.4).round();
						
							if *v > 255.0 {
								*v = 255.0;
							} else if *v < 0.0 {
								*v = 0.0;
							}
						}
						
						for v in &components {
							lrgba.push(*v as u8);
						}
						
						lrgba.push(255);
					}
				}
			}
			
			Image {
				ty: ImageType::LRGBA,
				dims: self.dims,
				data: ImageData::D8(lrgba)
			}
		} else {
			unreachable!()
		}
	}
}

enum Command {
	Upload(Upload),
	CacheIDLookup(CacheIDLookup),
	Delete(SubImageID),
	DeleteCache(SubImageCacheID),
}
	
struct CacheIDLookup {
	result: Arc<Mutex<Option<Option<Coords>>>>,
	condvar: Arc<Condvar>,
	cache_id: SubImageCacheID,
}

impl CacheIDLookup {
	fn some(&self, some: Coords) {
		let mut result = self.result.lock();
		*result = Some(Some(some));
		self.condvar.notify_one();
	}
	
	fn none(&self) {
		let mut result = self.result.lock();
		*result = Some(None);
		self.condvar.notify_one();
	}
}

struct Upload {
	result: Arc<Mutex<Option<Result<Coords, String>>>>,
	condvar: Arc<Condvar>,
	cache_id: SubImageCacheID,
	image: Image,
}

impl Upload {
	fn ok(&self, ok: Coords) {
		let mut result = self.result.lock();
		*result = Some(Ok(ok));
		self.condvar.notify_one();
	}
	
	fn err(&self, err: String) {
		let mut result = self.result.lock();
		*result = Some(Err(err));
		self.condvar.notify_one();
	}
}

pub struct AtlasDraw {
	pub images: HashMap<AtlasImageID, Arc<TmpImageViewAccess>>,
}

pub struct Atlas {
	engine: Arc<Engine>,
	cmd_queue: Sender<Command>,
	draw_recv: Receiver<Arc<AtlasDraw>>,
	empty_image: Arc<ImageViewAccess + Send + Sync>,
	default_sampler: Arc<Sampler>,
}

impl Atlas {
	pub fn new(engine: Arc<Engine>) -> Arc<Self> {
		let (cmd_queue, receiver) = channel::unbounded();
		// TODO: Switched this to bounded so if the render loop
		//       freezes up memory doesn't go out of control.
		let (draw_send, draw_recv) = channel::bounded(4);
		
		let default_sampler = Sampler::unnormalized(
			engine.device(),
			vulkano::sampler::Filter::Nearest,
			vulkano::sampler::UnnormalizedSamplerAddressMode::ClampToBorder(vulkano::sampler::BorderColor::IntTransparentBlack),
			vulkano::sampler::UnnormalizedSamplerAddressMode::ClampToBorder(vulkano::sampler::BorderColor::IntTransparentBlack),
		).unwrap();
		
		let empty_image = ImmutableImage::<vulkano::format::R8G8B8A8Unorm>::from_iter(
			vec![255,255,255,255].into_iter(),
			VkDimensions::Dim2d {
				width: 1,
				height: 1
			},
			vulkano::format::R8G8B8A8Unorm,
			engine.transfer_queue.clone()
		).unwrap().0;
		
		let atlas_ret = Arc::new(Atlas {
			engine, cmd_queue, draw_recv,
			default_sampler, empty_image
		});
		
		let atlas = atlas_ret.clone();
		
		thread::spawn(move || {
			let mut iter_start;
			let mut atlas_images: Vec<AtlasImage> = Vec::new();
			let mut sub_img_id_count = 1;
			let mut cached_map = HashMap::new();
			
			loop {
				iter_start = Instant::now();
				let mut cmds = Vec::new();
				
				while let Ok(cmd) = receiver.try_recv() {
					match cmd {
						Command::Upload(upreq) => {
							let mut space_op = None;
							
							for (i, atlas_image) in atlas_images.iter().enumerate() {
								if let Some(region) = atlas_image.find_space_for(&upreq.image.dims) {
									space_op = Some((i+1, region));
									break;
								}
							}
							
							if space_op.is_none() {
								let atlas_image = AtlasImage::new(atlas.engine.clone());
								
								match atlas_image.find_space_for(&upreq.image.dims) {
									Some(region) => {
										space_op = Some((atlas_images.len()+1, region));
									}, None => {
										upreq.err(format!("Image to big to fit in atlas."));
										continue;
									}
								}
								
								atlas_images.push(atlas_image);
							}
							
							let (atlas_image_i, region) = space_op.unwrap();
							let sub_img_id = sub_img_id_count;
							sub_img_id_count += 1;
							let coords = region.coords(atlas_image_i as u64, sub_img_id, &upreq.image.dims);
							
							if upreq.cache_id != SubImageCacheID::None {
								cached_map.insert(upreq.cache_id.clone(), coords);
							}
							
							upreq.ok(coords);
							atlas_images[atlas_image_i-1].insert(&region, sub_img_id, coords, upreq.image);
						}, c => cmds.push(c)
					}
				}
				
				for cmd in cmds {
					match cmd {
						Command::Upload(_) => unreachable!(),
						Command::Delete(_sub_img_id) => (), // TODO: Implement Deletes
						Command::DeleteCache(_sub_img_cache_id) => (),
						Command::CacheIDLookup(clookup) => {
							match cached_map.get(&clookup.cache_id) {
								Some(some) => clookup.some(some.clone()),
								None => clookup.none()
							}
						}
					}
				}
				
				let mut cmd_buf = AutoCommandBufferBuilder::new(
					atlas.engine.device(),
					atlas.engine.transfer_queue_ref().family()
				).unwrap();
				
				for atlas_image in &mut atlas_images {
					cmd_buf = atlas_image.update(cmd_buf);
				}
				
				cmd_buf
					.build().unwrap()
					.execute(atlas.engine.transfer_queue()).unwrap()
					.then_signal_fence_and_flush().unwrap()
					.wait(None).unwrap();
				
				let mut changed = false;
				let mut atlas_draw = AtlasDraw {
					images: HashMap::new(),
				};
					
				for (i, atlas_image) in atlas_images.iter_mut().enumerate() {
					if let Some((c, tmp_img)) = atlas_image.complete_update() {
						if c {
							changed = c;
						}
						
						atlas_draw.images.insert((i+1) as u64, Arc::new(tmp_img));
					}
				}
				
				if changed {
					draw_send.send(Arc::new(atlas_draw)).unwrap();
				}
				
				let elapsed = iter_start.elapsed();
				
				if elapsed > ITER_DURATION {
					continue;
				}
				
				thread::sleep(ITER_DURATION - elapsed);
			
			}
		});
		
		atlas_ret
	}
	
	pub fn empty_image(&self) -> Arc<ImageViewAccess + Send + Sync> {
		self.empty_image.clone()
	}
	
	pub fn default_sampler(&self) -> Arc<Sampler> {
		self.default_sampler.clone()
	}
	
	pub fn atlas_draw(&self) -> &Receiver<Arc<AtlasDraw>> {
		&self.draw_recv
	}
	
	pub fn delete_sub_image(&self, sub_img_id: SubImageID) {
		self.cmd_queue.send(Command::Delete(sub_img_id)).unwrap();
	}
	
	pub fn delete_sub_cache_image(&self, sub_img_cache_id: SubImageCacheID) {
		self.cmd_queue.send(Command::DeleteCache(sub_img_cache_id)).unwrap();
	}
	
	pub fn cache_coords(&self, cache_id: SubImageCacheID) -> Option<Coords> {
		let result = Arc::new(Mutex::new(None));
		let condvar = Arc::new(Condvar::new());
		
		let lookup = CacheIDLookup {
			result: result.clone(),
			condvar: condvar.clone(),
			cache_id,
		};
		
		self.cmd_queue.send(Command::CacheIDLookup(lookup)).unwrap();
		let mut result = result.lock();
		
		while result.is_none() {
			condvar.wait(&mut result);
		}
		
		result.take().unwrap()
	}
	
	pub fn load_image(&self, cache_id: SubImageCacheID, mut image: Image) -> Result<Coords, String> {
		image = image.to_lrgba();
		let result = Arc::new(Mutex::new(None));
		let condvar = Arc::new(Condvar::new());
		
		let req = Upload {
			result: result.clone(),
			condvar: condvar.clone(),
			cache_id, image
		};
		
		self.cmd_queue.send(Command::Upload(req)).unwrap();
		let mut result = result.lock();
		
		while result.is_none() {
			condvar.wait(&mut result);
		}
		
		result.take().unwrap()
	}
	
	pub fn load_image_from_bytes(&self, cache_id: SubImageCacheID, bytes: Vec<u8>) -> Result<Coords, String> {
		let format = match image::guess_format(bytes.as_slice()) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to guess image type for data: {}", e))
		};
		
		let (w, h, data) = match image::load_from_memory(bytes.as_slice()) {
			Ok(image) => (image.width(), image.height(), image.to_rgba().into_vec()),
			Err(e) => return Err(format!("Failed to read image: {}", e))
		};
		
		let image_type = match format {
			image::ImageFormat::JPEG => ImageType::SRGBA,
			_ => ImageType::LRGBA
		};
		
		let image = Image::new(image_type, ImageDims { w, h }, ImageData::D8(data))
			.map_err(|e| format!("Invalid Image: {}", e))?;
		self.load_image(cache_id, image)
	}
	
	pub fn load_image_from_path<P: Into<PathBuf>>(&self, path: P) -> Result<Coords, String> {
		let path_buf = path.into();
		let cache_id = SubImageCacheID::Path(path_buf.clone());
		
		if let Some(coords) = self.cache_coords(cache_id.clone()) {
			return Ok(coords);
		}
		
		let mut handle = match File::open(path_buf) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to open file: {}", e))
		};
			
		let mut bytes = Vec::new();
		
		if let Err(e) = handle.read_to_end(&mut bytes) {
			return Err(format!("Failed to read file: {}", e));
		}
		
		self.load_image_from_bytes(cache_id, bytes)
	}
	
	pub fn load_image_from_url<U: AsRef<str>>(self: &Arc<Self>, url: U) -> Result<Coords, String> {
		let cache_id = SubImageCacheID::Url(url.as_ref().to_string());
		
		if let Some(coords) = self.cache_coords(cache_id.clone()) {
			return Ok(coords);
		}
		
		let bytes = match zhttp::client::get_bytes(&url) {
			Ok(ok) => ok,
			Err(e) => return Err(format!("Failed to retreive url data: {}", e))
		};
		
		self.load_image_from_bytes(cache_id, bytes)
	}
}

struct Region {
	x: usize,
	y: usize,
	w: usize,
	h: usize
}

impl Region {
	fn coords(&self, img_id: AtlasImageID, sub_img_id: SubImageID, dims: &ImageDims) -> Coords {
		Coords {
			img_id, sub_img_id,
			x: (self.x as u32 * CELL_WIDTH) + (self.x.checked_sub(1).unwrap_or(0) as u32 * CELL_PAD),
			y: (self.y as u32 * CELL_WIDTH) + (self.y.checked_sub(1).unwrap_or(0) as u32 * CELL_PAD),
			w: dims.w,
			h: dims.h
		}
	}
}

struct SubImage {
	coords: Coords,
	img: Image,
}

struct AtlasImage {
	engine: Arc<Engine>,
	active: Option<usize>,
	update: Option<usize>,
	sto_imgs: Vec<Arc<ImageAccess + Send + Sync>>,
	sto_imgs_view: Vec<Arc<ImageViewAccess + Send + Sync>>,
	sub_imgs: HashMap<SubImageID, SubImage>,
	sto_leases: Vec<Vec<Arc<AtomicBool>>>,
	con_sub_img: Vec<Vec<SubImageID>>,
	alloc_cell_w: usize,
	alloc: Vec<Vec<Option<SubImageID>>>,
}

impl AtlasImage {
	fn new(engine: Arc<Engine>) -> Self {
		let max_img_w = engine.limits().max_image_dimension_2d as f32 + CELL_PAD as f32;
		let alloc_cell_w = (max_img_w / (CELL_WIDTH + CELL_PAD) as f32).floor() as usize;
		let mut alloc = Vec::with_capacity(alloc_cell_w);
		alloc.resize_with(alloc_cell_w, || {
			let mut out = Vec::with_capacity(alloc_cell_w);
			out.resize(alloc_cell_w, None);
			out
		});
	
		AtlasImage {
			engine, alloc, alloc_cell_w,
			active: None,
			update: None,
			sto_imgs: Vec::new(),
			sto_imgs_view: Vec::new(),
			sto_leases: Vec::new(),
			sub_imgs: HashMap::new(),
			con_sub_img: Vec::new(),
		}
	}
	
	fn complete_update(&mut self) -> Option<(bool, TmpImageViewAccess)> {
		let (changed, img_i) = match self.update.take() {
			Some(img_i) => {
				self.active = Some(img_i);
				(true, img_i)
			}, None => (false, *self.active.as_ref()?)
		};
	
		let (tmp_img, abool) = TmpImageViewAccess::new_abool(self.sto_imgs_view[img_i].clone());
		self.sto_leases[img_i].push(abool);
		Some((changed, tmp_img))
	}
	
	fn update_alternative(&mut self, cmd_buf: AutoCommandBufferBuilder) -> AutoCommandBufferBuilder {
		self.update = None;
		
		for i in 0..self.sto_imgs.len() {
			self.sto_leases[i].retain(|v| v.load(atomic::Ordering::Relaxed));
			
			if self.update.is_none() && self.sto_leases[i].is_empty() {
				self.update = Some(i);
				break;
			}
		}
		
		if self.update.is_none() && self.sto_imgs.len() >= 3 {
			return cmd_buf;
		}
		
		let (img_i, mut update) = match self.update.as_ref() {
			Some(img_i) => (*img_i, false),
			None => (self.sto_imgs.len(), true)
		};
		
		if !update {
			for sub_img_id in self.sub_imgs.keys() {
				if !self.con_sub_img[img_i].contains(sub_img_id) {
					self.con_sub_img[img_i].push(*sub_img_id);
					update = true;
				}
			} if !update {
				self.update = None;
				return cmd_buf;
			}
		}
		
		let (img_w, img_h) = self.minium_size();
		let dst_len = (img_w * img_h) as usize * 4;
		let mut img_data = Vec::with_capacity(dst_len);
		img_data.resize(dst_len, 0_u8);
		
		for sub_img in self.sub_imgs.values() {
			if let ImageData::D8(sub_img_data) = &sub_img.img.data {
				for x in 0..(sub_img.coords.w as usize) {
					for y in 0..(sub_img.coords.y as usize) {
						for c in 0..4 {
							img_data.get_mut(index3d((sub_img.coords.y as usize) + y, (sub_img.coords.x as usize) + x, c, img_h as usize, img_w as usize, 4)).and_then(|d|
								sub_img_data.get(index3d(y, x, c, sub_img.coords.h as usize, sub_img.coords.w as usize, 4)).and_then(|s| {
									*d = *s;
									Some(())
								})
							);
						}
					}
				}
			} else {
				unreachable!()
			}
		}
		
		let image = ImmutableImage::<vulkano::format::R8G8B8A8Unorm>::from_iter(
			img_data.into_iter(),
			VkDimensions::Dim2d {
				width: img_w,
				height: img_h
			},
			vulkano::format::R8G8B8A8Unorm,
			self.engine.transfer_queue.clone()
		).unwrap().0;
			
		if img_i < self.sto_imgs.len() {
			self.sto_imgs[img_i] = image.clone();
			self.sto_imgs_view[img_i] = image.clone();
			println!("{} created as {}x{}", img_i, img_w, img_h);
		} else {
			self.sto_imgs.push(image.clone());
			self.sto_imgs_view.push(image.clone());
			self.sto_leases.push(Vec::new());
			self.con_sub_img.push(self.sub_imgs.keys().cloned().collect());
			println!("{} created as {}x{}", img_i, img_w, img_h);
		}
		
		cmd_buf
	}
	
	// https://github.com/vulkano-rs/vulkano/issues/1190
	fn update(&mut self, mut cmd_buf: AutoCommandBufferBuilder) -> AutoCommandBufferBuilder {
		if ALT_UPDATE {
			return self.update_alternative(cmd_buf);
		}
	
		self.update = None;
		let mut found_op = None;
		let (min_img_w, min_img_h) = self.minium_size();
		let mut resize = false;
	
		for (i, sto_img) in self.sto_imgs.iter().enumerate() {
			self.sto_leases[i].retain(|v| v.load(atomic::Ordering::Relaxed));
			
			if found_op.is_none() && self.sto_leases[i].is_empty() {
				if let VkImgDimensions::Dim2d { width, height, .. } = sto_img.dimensions() {
					self.update = Some(i);
					found_op = Some((i, sto_img.clone()));
					resize = width < min_img_w || height < min_img_h;
				} else {
					unreachable!()
				}		
			}
		}
		
		if found_op.is_none() && self.sto_imgs.len() > 3 {
			return cmd_buf;
		}
		
		if found_op.is_none() || resize {
			let img_i = match found_op.as_ref() {
				Some((img_i, _)) => *img_i,
				None => self.sto_imgs.len()
			};
			
			let image = StorageImage::<vulkano::format::R8G8B8A8Unorm>::with_usage(
				self.engine.device(),
				VkDimensions::Dim2d {
					width: min_img_w,
					height: min_img_h,
				},
				vulkano::format::R8G8B8A8Unorm,
				VkImageUsage {
					transfer_source: true,
					transfer_destination: true,
					sampled: true,
					.. VkImageUsage::none()
				},
				vec![self.engine.transfer_queue_ref().family()]
			).unwrap();
			
			let zero_len = (min_img_w * min_img_h * 4) as usize;
			let mut zeros = Vec::with_capacity(zero_len);
			zeros.resize(zero_len, 0);
			
			let upload_buf = CpuAccessibleBuffer::from_iter(
				self.engine.device(),
				VkBufferUsage {
					transfer_source: true,
					.. VkBufferUsage::none()
				},
				zeros.into_iter()
			).unwrap();
			
			cmd_buf = cmd_buf.copy_buffer_to_image(upload_buf, image.clone()).unwrap();
			
			if img_i < self.sto_imgs.len() {
				self.sto_imgs[img_i] = image.clone();
				self.sto_imgs_view[img_i] = image.clone();
				self.con_sub_img[img_i].clear();
				self.sto_leases[img_i].clear();
				found_op = Some((img_i, image));
				println!("{} resized to {}x{}", img_i, min_img_w, min_img_h);
			} else {
				self.sto_imgs.push(image.clone());
				self.sto_imgs_view.push(image.clone());
				self.con_sub_img.push(Vec::new());
				self.sto_leases.push(Vec::new());
				found_op = Some((img_i, image));
				self.update = Some(img_i);
				println!("{} created as {}x{}", img_i, min_img_w, min_img_h);
			}
		}
		
		let (img_i, sto_img) = found_op.unwrap();
		let mut upload_data = Vec::new();
		let mut copy_cmds = Vec::new();
		
		for (sub_img_id, sub_img) in &self.sub_imgs {
			if !self.con_sub_img[img_i].contains(sub_img_id) {
				if let ImageData::D8(sub_img_data) = &sub_img.img.data {
					assert!(ImageType::LRGBA == sub_img.img.ty);
					assert!(!sub_img_data.is_empty());
					
					let s = upload_data.len();
					upload_data.extend_from_slice(&sub_img_data);
					
					copy_cmds.push((
						s, upload_data.len(),
						sub_img.coords.x, sub_img.coords.y,
						sub_img.coords.w, sub_img.coords.h
					));
					
					self.con_sub_img[img_i].push(*sub_img_id);
				} else {
					unreachable!()
				}
			}
		}
		
		if copy_cmds.is_empty() {
			self.update = None;
			return cmd_buf;
		}
		
		let upload_buf = CpuAccessibleBuffer::from_iter(
			self.engine.device(),
			VkBufferUsage {
				transfer_source: true,
				.. VkBufferUsage::none()
			},
			upload_data.into_iter()
		).unwrap();
		
		for (s, e, x, y, w, h) in copy_cmds {
			cmd_buf = cmd_buf.copy_buffer_to_image_dimensions(
				upload_buf.clone().into_buffer_slice().slice(s..e).unwrap(),
				sto_img.clone(),
				[x, y, 0],
				[w, h, 0],
				0, 1, 0
			).unwrap();
		}
		
		cmd_buf
	}
	
	fn minium_size(&self) -> (u32, u32) {
		let mut min_x = 1;
		let mut min_y = 1;
		
		for sub_img in self.sub_imgs.values() {
			let x = sub_img.coords.x + sub_img.coords.w;
			let y = sub_img.coords.y + sub_img.coords.h;
			
			if x > min_x {
				min_x = x;
			}
			
			if y > min_y {
				min_y = y;
			}
		}
		
		(min_x, min_y)
	}

	fn find_space_for(&self, dims: &ImageDims) -> Option<Region> {
		// TODO: Include padding in available space
		let w = (dims.w as f32 / CELL_WIDTH as f32).ceil() as usize;
		let h = (dims.h as f32 / CELL_WIDTH as f32).ceil() as usize;
		let mut cell_pos = None;
		
		for i in 0..self.alloc_cell_w {
			for j in 0..self.alloc_cell_w {
				let mut fits = true;
			
				for k in 0..w {
					for l in 0..h {
						match self.alloc.get(i+k).and_then(|xarr| xarr.get(j+l)) {
							Some(cell) => if cell.is_some() {
								fits = false;
								break;
							}, None => {
								fits = false;
								break;
							}
						}
					} if !fits {
						break;
					}
				}
				
				if fits {
					cell_pos = Some((i, j));
					break;
				}
			}
			
			if cell_pos.is_some() {
				break;
			}
		}
		
		let (x, y) = cell_pos?;
		Some(Region { x, y, w, h })
	}
	
	fn insert(&mut self, region: &Region, sub_img_id: SubImageID, coords: Coords, img: Image) {
		for x in region.x..(region.x+region.w) {
			for y in region.y..(region.y+region.h) {
				self.alloc[x][y] = Some(sub_img_id);
			}
		}
		
		self.sub_imgs.insert(sub_img_id, SubImage { coords, img });
	}
}

