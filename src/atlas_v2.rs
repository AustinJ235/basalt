#![allow(warnings)]

use vulkano::sampler::MipmapMode;
use vulkano::sampler::UnnormalizedSamplerAddressMode;
use std::sync::atomic::{self,AtomicU64,AtomicBool};
use vulkano::sampler::BorderColor;
use std::path::PathBuf;
use std::collections::{BTreeMap,HashMap};
use std::sync::Arc;
use vulkano::sampler::{self,Sampler};
use parking_lot::Mutex;
use tmp_image_access::TmpImageViewAccess;
use vulkano::image::traits::ImageViewAccess;
use crossbeam::channel::{self,Sender};
use std::sync::Barrier;
use std::time::{Instant,Duration};
use std::thread;
use Limits;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use image;
use image::GenericImageView;
use std::fs::File;
use std::io::Read;
use Engine;
use vulkano::image::StorageImage;
use vulkano::image::Dimensions as VkDimensions;
use vulkano::image::ImageUsage as VkImageUsage;
use vulkano::buffer::BufferUsage as VkBufferUsage;
use vulkano::format::Format as VkFormat;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferAccess;
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;

const UNIT_SIZE: u32 = 32;
const UNIT_PADDING: u32 = 1;

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Debug,Hash)]
pub struct SubImageID(u64);

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Debug,Hash)]
pub struct AtlasImageID(u64);

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub enum SubImageCacheID {
	Path(PathBuf),
	Url(String),
	Glyph(u32, u64),
	None
}

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub struct SamplerDesc {
	pub mag_filter: sampler::Filter,
	pub min_filter: sampler::Filter,
}

impl Default for SamplerDesc {
	fn default() -> Self {
		SamplerDesc {
			mag_filter: sampler::Filter::Linear,
			min_filter: sampler::Filter::Nearest,
		}
	}
}

impl SamplerDesc {
	fn create_sampler(&self, engine: &Arc<Engine>) -> Arc<Sampler> {
		Sampler::new(
			engine.device(),
			self.mag_filter.clone(),
			self.min_filter.clone(),
			sampler::MipmapMode::Nearest,
			sampler::SamplerAddressMode::Repeat,
			sampler::SamplerAddressMode::Repeat,
			sampler::SamplerAddressMode::Repeat,
			1.0, 1.0, 0.0, 100.0
		).unwrap()
	}
}

#[derive(Clone,Copy,PartialEq,Eq,Debug)]
pub struct Coords {
	pub image_id: AtlasImageID,
	pub sub_image_id: SubImageID,
	pub x: u32,
	pub y: u32,
	pub w: u32,
	pub h: u32,
}

impl Coords {
	pub fn none() -> Self {
		Coords {
			image: AtlasImageID(0),
			sub_image: SubImageID(0),
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

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub enum DataType {
	LRGBA,
	LRGB,
	LMono,
	SRGBA,
	SRGB,
	YUV444,
}

pub enum Data {
	D8(Vec<u8>),
	D10(Vec<u16>),
	D12(Vec<u16>),
	D16(Vec<u16>),
}

struct SubImage {
	pub cache_id: SubImageCacheID,
	pub coords: Coords,
	pub data_type: DataType,
	pub data: Data,
	pub sampler_desc: SamplerDesc,
	pub notify: Option<Arc<Barrier>>,
}

struct Image {
	id: AtlasImageID,
	engine: Arc<Engine>,
	images: Vec<Arc<StorageImage<vulkano::format::Format>>>,
	leases: Vec<Vec<Arc<AtomicBool>>>,
	current: Option<usize>,
	sub_images: BTreeMap<SubImageID, SubImage>,
	sub_images_in: Vec<Vec<SubImageID>>,
	grid: Vec<Vec<Option<SubImageID>>>,
	grid_uw: usize,
	updating_to: Option<usize>,
}

impl Image {
	fn switch(&mut self) {
		if let Some(image_i) = self.updating_to.take() {
			self.current = Some(image_i);
		}
	}

	fn update(&mut self, mut cmd_buf: AutoCommandBufferBuilder)
		-> (AutoCommandBufferBuilder, Vec<Arc<Barrier>>)
	{
		for (i, leases) in self.leases.iter_mut().enumerate() {
			leases.retain(|v| !v.load(atomic::Ordering::Relaxed));
			
			if
				self.updating_to.is_none()
				&& leases.is_empty()
				&& (self.current.is_none() || *self.current.as_ref().unwrap() != i)
			{
				self.updating_to = Some(i);
			}
		}
		
		if self.updating_to.is_none() {
			let (w, h) = self.minimum_size();
			let image = StorageImage::with_usage(
				self.engine.device(),
				VkDimensions::Dim2d {
					width: w,
					height: h,
				},
				VkFormat::R8G8B8A8Unorm,
				VkImageUsage {
					transfer_source: true,
					transfer_destination: true,
					sampled: true,
					color_attachment: true,
					.. VkImageUsage::none()
				},
				vec![self.engine.graphics_queue_ref().family()]
			).unwrap();
			
			self.images.push(image);
			self.leases.push(Vec::new());
			self.sub_images_in.push(Vec::new());
			self.updating_to = Some(self.images.len() - 1);
		}
		
		let image_i = *self.updating_to.as_ref().unwrap();
		let mut upload_data = Vec::new();
		let mut copy_cmds = Vec::new();
		let mut notify_barriers = Vec::new();
		
		for (sub_image_id, sub_image) in self.sub_images.iter_mut() {
			if !self.sub_images_in[image_i].contains(sub_image_id) {
				if let Data::D8(data) = &sub_image.data {			
					let mut lrgba = match &sub_image.data_type {
						&DataType::LRGBA => data.clone(),
						&DataType::LRGB => {
							let mut lrgba = Vec::with_capacity(data.len() / 3 * 4);
							
							for chunk in data.chunks_exact(3) {
								lrgba.extend_from_slice(chunk);
								lrgba.push(255);
							}
							
							lrgba
						},
						&DataType::LMono => {
							let mut lrgba = Vec::with_capacity(data.len() * 4);
							
							for v in data {
								lrgba.push(*v);
								lrgba.push(*v);
								lrgba.push(*v);
								lrgba.push(255);
							}
							
							lrgba
						},
						&DataType::SRGBA => {
							let mut lrgba = Vec::with_capacity(data.len());
							
							for v in data.iter() {
								let mut v = ((*v as f32 + (0.055 * 255.0)) / 1.055).powf(2.4).round();
								
								if v > 255.0 {
									v = 255.0;
								} else if v < 0.0 {
									v = 0.0;
								}
								
								lrgba.push(v as u8);
							}
							
							lrgba
						},
						&DataType::SRGB => {
							let mut lrgba = Vec::with_capacity(data.len() / 3 * 4);
							
							for chunk in data.chunks_exact(3) {
								for v in chunk {
									let mut v = ((*v as f32 + (0.055 * 255.0)) / 1.055).powf(2.4).round();
									
									if v > 255.0 {
										v = 255.0;
									} else if v < 0.0 {
										v = 0.0;
									}
									
									lrgba.push(v as u8);
								}	
									
								lrgba.push(255);
							}
							
							lrgba
						},
						&DataType::YUV444 => {
							let mut lrgba = Vec::with_capacity(data.len() / 3 * 4);
							
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
							
							lrgba
						}
					};
					
					let data_s = upload_data.len();
					upload_data.append(&mut lrgba);
					let data_e = upload_data.len();
					
					copy_cmds.push((
						data_s, data_e,
						sub_image.coords.x, sub_image.coords.y,
						sub_image.coords.w, sub_image.coords.h
					));
				} else {
					println!("Only 8 bit depth images are supported at this time.");
				}
				
				if let Some(notify) = sub_image.notify.take() {
					notify_barriers.push(notify);
				}
			}
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
				self.images[image_i].clone(),
				[x, y, 0],
				[w, h, 1],
				0, 1, 0
			).unwrap();
		}
		
		(cmd_buf, notify_barriers)
	}

	fn image_render_data(&mut self, image_id: SubImageID) -> Option<(TmpImageViewAccess, SamplerDesc, Coords)> {
		let image_i = self.current.as_ref()?;
		
		if self.sub_images_in[*image_i].contains(&image_id) {
			let sub_image = self.sub_images.get(&image_id)?;
			let (tmp_img, abool) = TmpImageViewAccess::new_abool(self.images[*image_i].clone());
			self.leases[*image_i].push(abool);
			Some((tmp_img, sub_image.sampler_desc.clone(), sub_image.coords.clone()))
		} else {
			None
		}
	}

	fn new(engine: Arc<Engine>, id: AtlasImageID) -> Self {
		let grid_uw = (engine.limits().max_image_dimension_2d as f32 / (UNIT_SIZE + (UNIT_PADDING * 2)) as f32).floor() as usize;
		let mut grid = Vec::with_capacity(grid_uw as usize);
		grid.resize_with(grid_uw as usize, || {
			let mut out = Vec::with_capacity(grid_uw as usize);
			out.resize(grid_uw as usize, None);
			out
		});
	
		Image {
			engine, id,
			images: Vec::new(),
			leases: Vec::new(),
			current: None,
			sub_images: BTreeMap::new(),
			sub_images_in: Vec::new(),
			updating_to: None,
			grid, grid_uw
		}
	}
	
	fn insert_sub_image(
		&mut self,
		sub_image_id: SubImageID,
		sub_image: SubImage,
		ux: usize, uy: usize,
		uw: usize, uh: usize,
	) {
		for i in ux..(ux+uw) {
			for j in uy..(uy+uh) {
				self.grid[i][j] = Some(sub_image_id);
			}
		}
		
		self.sub_images.insert(sub_image_id, sub_image);
	}
	
	fn space_for(&self, w: u32, h: u32) -> Option<(usize, usize)> {
		let uw = (w as f32 / UNIT_SIZE as f32).ceil() as usize;
		let uh = (h as f32 / UNIT_SIZE as f32).ceil() as usize;
		let mut i = 0;
		let mut j = 0;
		
		'find: loop {
			for k in 0..uw {
				for l in 0..uh {
					if self.grid.get(i+k).and_then(|v| v.get(j+l)).and_then(|v| if v.is_some() {
							None
						} else {
							Some(())
						}
					).is_some() {
						i += 1;
						
						if i >= self.grid_uw {
							j += 1;
							i = 0;
						}
						
						if j >= self.grid_uw {
							return None;
						}
							
						continue 'find;
					}
				}
			}
			
			return Some((i, j));
		}
		
		unreachable!()
	}
	
	fn minimum_size(&self) -> (u32, u32) {
		let mut max_x = 0;
		let mut max_y = 0;
		
		for i in 0..self.grid_uw {
			for j in 0..self.grid_uw {
				if self.grid[i][j].is_some() {
					if i > max_x {
						max_x = i;
					}
					
					if j > max_y {
						max_y = i;
					}
				}
			}
		}
		
		(
			(max_x as f32 * 34.0).ceil() as u32,
			(max_y as f32 * 34.0).ceil() as u32,
		)
	}
}

pub struct ImageLoad {
	atlas: Arc<Atlas>,
	barrier: Arc<Barrier>,
	result: Arc<Mutex<Option<Result<SubImageID, String>>>>,
}

impl ImageLoad {
	pub fn wait(self) -> Result<SubImageID, String> {
		self.barrier.wait();use Limits;
		self.result.lock().take().unwrap()
	}
	
	pub fn on_ready(self, func: Arc<Fn(Arc<Atlas>, Result<SubImageID, String>) + Send + Sync>) {
		thread::spawn(move || {
			self.barrier.wait();
			func(self.atlas, self.result.lock().take().unwrap());
		});
	}
}

pub struct Atlas {
	engine: Arc<Engine>,
	sub_image_counter: AtomicU64,
	atlas_image_counter: AtomicU64,
	images: Mutex<BTreeMap<AtlasImageID, Image>>,
	cached_images: Mutex<HashMap<SubImageCacheID, SubImageID>>,
	sampler_cache: Mutex<HashMap<SamplerDesc, Arc<Sampler>>>,
	upload_queue: Sender<(
		SubImageCacheID, DataType,
		SamplerDesc, u32, u32, Data,
		Arc<Mutex<Option<Result<SubImageID, String>>>>, Arc<Barrier>
	)>,
}

impl Atlas {
	pub(crate) fn new(engine: Arc<Engine>) -> Arc<Self> {
		let (upload_queue_s, upload_queue_r) = channel::unbounded();
		let atlas = Arc::new(Atlas {
			engine,
			sub_image_counter: AtomicU64::new(0),
			atlas_image_counter: AtomicU64::new(0),
			images: Mutex::new(BTreeMap::new()),
			cached_images: Mutex::new(HashMap::new()),
			sampler_cache: Mutex::new(HashMap::new()),
			upload_queue: upload_queue_s,
		});
		
		let atlas_ret = atlas.clone();
		
		thread::spawn(move || {
			let mut iter_start = Instant::now();
			
			loop {
				{
					let mut images = atlas.images.lock();
					let mut cached_images = atlas.cached_images.lock();
					let mut sampler_cache = atlas.sampler_cache.lock();
				
					while let Ok((
						cache_id, data_ty, sampler_desc,
						width, height, img_data, result, barrier
					)) = upload_queue_r.try_recv() {
						let err = |e| {
							*result.lock() = Some(Err(e));
							barrier.wait();
						};
					
						let sub_image_id = SubImageID(atlas.atlas_image_counter.load(atomic::Ordering::Relaxed));
					
						if !sampler_cache.contains_key(&sampler_desc) {
							let sampler = sampler_desc.create_sampler(&atlas.engine);
							sampler_cache.insert(sampler_desc.clone(), sampler);
						}
						
						let mut use_image = None;
						
						for (image_id, image) in &mut *images {
							if let Some((ux, uy)) = image.space_for(width, height) {
								use_image = Some((*image_id, ux, uy));
							}	
						}
						
						if use_image.is_none() {
							let image_id = AtlasImageID(atlas.atlas_image_counter.fetch_add(1, atomic::Ordering::Relaxed));
							let image = Image::new(atlas.engine.clone(), image_id);
							
							let (ux, uy) = match image.space_for(width, height) {
								Some(some) => some,
								None => {
									atlas.atlas_image_counter.fetch_sub(1, atomic::Ordering::Relaxed);
									err(format!("No space for image."));
									continue;
								}
							};
							
							images.insert(image_id, image);
							use_image = Some((image_id, ux, uy));
						}
						
						let (image_id, ux, uy) = use_image.unwrap();
						*result.lock() = Some(Ok(sub_image_id));
						
						let uw = (width as f32 / UNIT_SIZE as f32).ceil() as usize;
						let uh = (height as f32 / UNIT_SIZE as f32).ceil() as usize;
		
						let coords = Coords {
							image: image_id,
							sub_image: sub_image_id,
							x: (uw as u32 * (UNIT_SIZE + (UNIT_PADDING * 2))) + UNIT_PADDING,
							y: (uw as u32 * (UNIT_SIZE + (UNIT_PADDING * 2))) + UNIT_PADDING,
							w: width,
							h: height
						};
						
						let sub_image = SubImage {
							coords, sampler_desc,
							cache_id: cache_id.clone(),
							data: img_data,
							data_type: data_ty,
							notify: Some(barrier.clone()),
						};
						
						images.get_mut(&image_id).unwrap().insert_sub_image(
							sub_image_id,
							sub_image,
							ux, uy, uw, uh
						);	
						
						if cache_id != SubImageCacheID::None {
							cached_images.insert(cache_id, sub_image_id);
						}
					}
					
					let mut cmd_buf = AutoCommandBufferBuilder::new(atlas.engine.device(), atlas.engine.transfer_queue_ref().family()).unwrap();
					let mut notify = Vec::new();
					
					for image in images.values_mut() {
						let (new_cmd_buf, mut notify_barriers) = image.update(cmd_buf);
						cmd_buf = new_cmd_buf;
						notify.append(&mut notify_barriers);
					}
					
					let fence = cmd_buf
						.build().unwrap()
						.execute(atlas.engine.transfer_queue()).unwrap()
						.then_signal_fence_and_flush().unwrap()
						.wait(None).unwrap();
					
					for image in images.values_mut() {
						image.switch();
					}
					
					drop(images);
					drop(cached_images);
					drop(sampler_cache);
					
					for barrier in notify {
						barrier.wait();
					}
				}
			
				if iter_start.elapsed()	> Duration::from_millis(10) {
					continue;
				}
				
				thread::sleep(Duration::from_millis(10) - iter_start.elapsed());
				iter_start = Instant::now();
			}
		});
		
		atlas_ret
	}
	
	pub fn is_cached(&self, cache_id: SubImageCacheID) -> bool {
		self.cached_images.lock().contains_key(&cache_id)
	}
	
	pub fn load_image_from_url<U: AsRef<str>>(self: &Arc<Self>, url: U, sampler_desc: SamplerDesc) -> ImageLoad {
		let cache_id = SubImageCacheID::Url(url.as_ref().to_string());
		
		if let Some(sub_image_id) = self.cached_images.lock().get(&cache_id).clone() {
			return ImageLoad {
				atlas: self.clone(),
				barrier: Arc::new(Barrier::new(1)),
				result: Arc::new(Mutex::new(Some(Ok(*sub_image_id))))
			};
		}
		
		let result_ret = Arc::new(Mutex::new(None));
		let barrier_ret = Arc::new(Barrier::new(2));
		let result = result_ret.clone();
		let barrier = barrier_ret.clone();
		let atlas = self.clone();
		let url = url.as_ref().to_string();
		
		thread::spawn(move || {
			let err = |e| {
				*result.lock() = Some(Err(e));
				barrier.wait();
			};
			
			let bytes = match zhttp::client::get_bytes(&url) {
				Ok(ok) => ok,
				Err(e) => return err(format!("Failed to retreive url data: {}", e))
			};
		
			let format = match image::guess_format(bytes.as_slice()) {
				Ok(ok) => ok,
				Err(e) => return err(format!("Failed to guess image type for data: {}", e))
			};
			
			let (width, height, mut data) = match image::load_from_memory(bytes.as_slice()) {
				Ok(image) => (image.width(), image.height(), image.to_rgba().into_vec()),
				Err(e) => return err(format!("Failed to read image: {}", e))
			};
			
			if match format {
				image::ImageFormat::JPEG => true,
				_ => false
			} {
				for mut v in &mut data {
					*v = f32::round(f32::powf(((*v as f32 / 255.0) + 0.055) / 1.055, 2.4) * 255.0) as u8;
				}
			}
			
			atlas.upload_queue.send((
				cache_id, DataType::SRGBA, sampler_desc,
				width, height, Data::D8(data),
				result.clone(), barrier.clone()
			)).unwrap();
		});
		
		ImageLoad {
			atlas: self.clone(),
			barrier: barrier_ret,
			result: result_ret
		}
	}
	
	pub fn load_image_from_path(self: &Arc<Self>, path_buf: PathBuf, sampler_desc: SamplerDesc) -> ImageLoad {
		let cache_id = SubImageCacheID::Path(path_buf.clone());
		
		if let Some(sub_image_id) = self.cached_images.lock().get(&cache_id).clone() {
			return ImageLoad {
				atlas: self.clone(),
				barrier: Arc::new(Barrier::new(1)),
				result: Arc::new(Mutex::new(Some(Ok(*sub_image_id))))
			};
		}
		
		let result_ret = Arc::new(Mutex::new(None));
		let barrier_ret = Arc::new(Barrier::new(2));
		let result = result_ret.clone();
		let barrier = barrier_ret.clone();
		let atlas = self.clone();
		
		thread::spawn(move || {
			let err = |e| {
				*result.lock() = Some(Err(e));
				barrier.wait();
			};
			
			let mut handle = match File::open(path_buf) {
				Ok(ok) => ok,
				Err(e) => return err(format!("Failed to open file: {}", e))
			};
			
			let mut bytes = Vec::new();
			
			if let Err(e) = handle.read_to_end(&mut bytes) {
				return err(format!("Failed to read file: {}", e));
			}
		
			let format = match image::guess_format(bytes.as_slice()) {
				Ok(ok) => ok,
				Err(e) => return err(format!("Failed to guess image type for data: {}", e))
			};
			
			let (width, height, mut data) = match image::load_from_memory(bytes.as_slice()) {
				Ok(image) => (image.width(), image.height(), image.to_rgba().into_vec()),
				Err(e) => return err(format!("Failed to read image: {}", e))
			};
			
			if match format {
				image::ImageFormat::JPEG => true,
				_ => false
			} {
				for mut v in &mut data {
					*v = f32::round(f32::powf(((*v as f32 / 255.0) + 0.055) / 1.055, 2.4) * 255.0) as u8;
				}
			}
			
			atlas.upload_queue.send((
				cache_id, DataType::SRGBA, sampler_desc,
				width, height, Data::D8(data),
				result.clone(), barrier.clone()
			)).unwrap();
		
		});
		
		ImageLoad {
			atlas: self.clone(),
			barrier: barrier_ret,
			result: result_ret
		}
	}
	
	pub fn load_image(
		self: Arc<Self>, cache_id: SubImageCacheID,
		ty: DataType, sampler_desc: SamplerDesc,
		width: u32, height: u32, data: Data
	) -> ImageLoad {
	
		let result = Arc::new(Mutex::new(None));
		let barrier = Arc::new(Barrier::new(2));
		
		self.upload_queue.send((
			cache_id, ty, sampler_desc,
			width, height, data,
			result.clone(), barrier.clone()
		)).unwrap();
		
		ImageLoad { 
			atlas: self.clone(),
			barrier, result,
		}
	}
	
	pub fn cached_image_id(&self, cache_id: SubImageCacheID) -> Option<SubImageID> {
		self.cached_images.lock().get(&cache_id).cloned()
	}
	
	pub fn image_render_data(&self, image_id: SubImageID) -> Option<(TmpImageViewAccess, Arc<Sampler>, Coords)> {
		for image in self.images.lock().values_mut() {
			if let Some((tmp_img, sampler_desc, coords)) = image.image_render_data(image_id) {
				let sampler = self.sampler_cache.lock().get(&sampler_desc).cloned()?;
				return Some((tmp_img, sampler.clone(), coords));
			}
		} None
	}
}

