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

const CELL_SIZE: u32 = 32;
const CELL_PADDING: u32 = 4;

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Debug,Hash)]
pub struct SubImageID(pub u64);

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Debug,Hash)]
pub struct AtlasImageID(pub u64);

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
	pub image: AtlasImageID,
	pub sub_image: SubImageID,
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
	pub notify: Option<Arc<AtomicBool>>,
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
	grid_uw: u32,
	updating_to: Option<usize>,
}

impl Image {
	fn switch(&mut self) {
		if let Some(image_i) = self.updating_to.take() {
			self.current = Some(image_i);
		}
	}

	fn update(&mut self, mut cmd_buf: AutoCommandBufferBuilder)
		-> (AutoCommandBufferBuilder, Vec<Arc<AtomicBool>>)
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
			println!("{:?}:{} Created as {}x{}", self.id, self.images.len(), w, h);
		} else {
			let image_i = *self.updating_to.as_ref().unwrap();
			let (min_w, min_h) = self.minimum_size();
		
			if let VkDimensions::Dim2d { width, height } = self.images[image_i].dimensions() {
				if width < min_w || height < min_h {
					self.images[image_i] = StorageImage::with_usage(
						self.engine.device(),
						VkDimensions::Dim2d {
							width: min_w,
							height: min_h,
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
					
					self.sub_images_in[image_i].clear(); // TODO: Copy old onto new
					println!("{:?}:{} Resized to {}x{}", self.id, self.images.len(), min_w, min_h);
				}
			}
			
		}
		
		let image_i = *self.updating_to.as_ref().unwrap();
		let mut upload_data = Vec::new();
		let mut copy_cmds = Vec::new();
		let mut notifies = Vec::new();
		
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
					notifies.push(notify);
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
		
		(cmd_buf, notifies)
	}
	
	fn current_tmp(&mut self) -> Option<TmpImageViewAccess> {
		let image_i = self.current.as_ref()?;
		let (tmp_img, abool) = TmpImageViewAccess::new_abool(self.images[*image_i].clone());
		self.leases[*image_i].push(abool);
		Some(tmp_img)
	}
	
	fn image_coords(&mut self, image_id: SubImageID) -> Option<Coords> {
		let image_i = self.current.as_ref()?;
		
		if self.sub_images_in[*image_i].contains(&image_id) {
			let sub_image = self.sub_images.get(&image_id)?;
			Some(sub_image.coords.clone())
		} else {
			None
		}
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
		let max_w = engine.limits().max_image_dimension_2d;
		let mut grid_uw = 0;
		
		loop {
			let w = (grid_uw * CELL_SIZE) + (grid_uw * CELL_PADDING) + CELL_PADDING;
			
			if grid_uw > max_w {
				grid_uw -= 1;
				break;
			}
			
			grid_uw += 1;
		}
		
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
		ux: u32, uy: u32,
		uw: u32, uh: u32,
	) {
		for i in ux..(ux+uw) {
			for j in uy..(uy+uh) {
				self.grid[i as usize][j as usize] = Some(sub_image_id);
			}
		}
		
		self.sub_images.insert(sub_image_id, sub_image);
	}
	
	fn space_for(&self, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
		let mut uw = 1;
		
		loop {
			let test_w = (uw * CELL_SIZE) + ((uw - 1) * CELL_PADDING);
			
			if test_w >= w {
				break;
			} else {
				uw += 1;
				
				if uw > self.grid_uw {
					return None;
				}
			}
		}
		
		let mut uh = 1;
		
		loop {
			let test_h = (uh * CELL_SIZE) + ((uh - 1) * CELL_PADDING);
			
			if test_h >= w {
				break;
			} else {
				uh += 1;
				
				if uh > self.grid_uw {
					return None;
				}
			}
		}
		
		for i in 0..self.grid_uw {
			'j : for j in 0..self.grid_uw {
				for k in 0..uw {
					for l in 0..uh {
						if let Some(cell) = self.grid.get((i+k) as usize).and_then(|v| v.get((j+l) as usize)) {
							if cell.is_some() {
								continue 'j;
							}
						}
					}
				}
				
				return Some((i, j, uw, uh));
			}
		}
		
		None
	}
	
	fn minimum_size(&self) -> (u32, u32) {
		/*let mut max_x = 0;
		let mut max_y = 0;
		
		for sub_image in self.sub_images.values() {
			if sub_image.coords.x + sub_image.coords.w > max_x {
				max_x = sub_image.coords.x + sub_image.coords.w;
			}
			
			if sub_image.coords.y + sub_image.coords.h > max_y {
				max_y = sub_image.coords.y + sub_image.coords.h;
			}
		}
		
		(max_x + CELL_PADDING, max_y + CELL_PADDING)*/
		
		let mut max_x = 0;
		let mut max_y = 0;
		
		for i in 0..self.grid_uw {
			for j in 0..self.grid_uw {
				if self.grid[i as usize][j as usize].is_some() {
					if i > max_x {
						max_x = i;
					}
					
					if j > max_y {
						max_y = j;
					}
				}
			}
		}
		
		max_x += 1;
		max_y += 1;
		
		(
			(max_x * CELL_SIZE) + (max_x * CELL_PADDING) + CELL_PADDING,
			(max_y * CELL_SIZE) + (max_y * CELL_PADDING) + CELL_PADDING
		)
	}
}

pub struct ImageLoad {
	atlas: Arc<Atlas>,
	ready: Arc<AtomicBool>,
	result: Arc<Mutex<Option<Result<Coords, String>>>>,
}

impl ImageLoad {
	pub fn wait(self) -> Result<Coords, String> {
		loop {
			if self.ready.load(atomic::Ordering::Relaxed) {
				break;
			} else {
				thread::sleep(Duration::from_millis(10));
			}
		}
		
		
		self.result.lock().take().unwrap()
	}
	
	pub fn on_ready(self, func: Arc<Fn(Arc<Atlas>, Result<Coords, String>) + Send + Sync>) {
		thread::spawn(move || {
			loop {
				if self.ready.load(atomic::Ordering::Relaxed) {
					break;
				} else {
					thread::sleep(Duration::from_millis(10));
				}
			}
			
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
		Arc<Mutex<Option<Result<Coords, String>>>>, Arc<AtomicBool>
	)>,
	default_sampler: Arc<Sampler>,
	empty_image: Arc<StorageImage<vulkano::format::Format>>,
}

impl Atlas {
	pub(crate) fn new(engine: Arc<Engine>) -> Arc<Self> {
		let (upload_queue_s, upload_queue_r) = channel::unbounded();
		let default_sampler_desc = SamplerDesc::default();
		let default_sampler = default_sampler_desc.create_sampler(&engine);
		let empty_image = StorageImage::with_usage(
			engine.device(),
			VkDimensions::Dim2d {
				width: 1,
				height: 1,
			},
			VkFormat::R8G8B8A8Unorm,
			VkImageUsage {
				transfer_source: true,
				transfer_destination: true,
				sampled: true,
				color_attachment: true,
				.. VkImageUsage::none()
			},
			vec![engine.graphics_queue_ref().family()]
		).unwrap();
		
		let mut sampler_cache = HashMap::new();
		sampler_cache.insert(default_sampler_desc, default_sampler.clone());
		
		let atlas = Arc::new(Atlas {
			engine,
			sub_image_counter: AtomicU64::new(1),
			atlas_image_counter: AtomicU64::new(1),
			images: Mutex::new(BTreeMap::new()),
			cached_images: Mutex::new(HashMap::new()),
			sampler_cache: Mutex::new(sampler_cache),
			upload_queue: upload_queue_s,
			empty_image,
			default_sampler,
		});
		
		let atlas_ret = atlas.clone();
		
		thread::spawn(move || {
			let mut iter_start;
			
			loop {
				iter_start = Instant::now();
				
				{
					let mut images = atlas.images.lock();
					let mut cached_images = atlas.cached_images.lock();
					let mut sampler_cache = atlas.sampler_cache.lock();
				
					while let Ok((
						cache_id, data_ty, sampler_desc,
						width, height, img_data, result, ready
					)) = upload_queue_r.try_recv() {
						let err = |e| {
							*result.lock() = Some(Err(e));
							ready.store(true, atomic::Ordering::Relaxed);
						};
					
						let sub_image_id = SubImageID(atlas.atlas_image_counter.load(atomic::Ordering::Relaxed));
					
						if !sampler_cache.contains_key(&sampler_desc) {
							let sampler = sampler_desc.create_sampler(&atlas.engine);
							sampler_cache.insert(sampler_desc.clone(), sampler);
						}
						
						let mut use_image = None;
						
						for (image_id, image) in &mut *images {
							if let Some((ux, uy, uw, uh)) = image.space_for(width, height) {
								use_image = Some((*image_id, ux, uy, uw, uh));
							}	
						}
						
						if use_image.is_none() {
							let image_id = AtlasImageID(atlas.atlas_image_counter.fetch_add(1, atomic::Ordering::Relaxed));
							let image = Image::new(atlas.engine.clone(), image_id);
							
							let (ux, uy, uw, uh) = match image.space_for(width, height) {
								Some(some) => some,
								None => {
									atlas.atlas_image_counter.fetch_sub(1, atomic::Ordering::Relaxed);
									err(format!("No space for image."));
									continue;
								}
							};
							
							images.insert(image_id, image);
							println!("{:?} Created", image_id);
							use_image = Some((image_id, ux, uy, uw, uh));
						}
						
						let (image_id, ux, uy, uw, uh) = use_image.unwrap();
						let coords = Coords {
							image: image_id,
							sub_image: sub_image_id,
							x: ((ux * CELL_SIZE) + ((ux + 1) * CELL_PADDING)) as u32,
							y: ((uy * CELL_SIZE) + ((uy + 1) * CELL_PADDING)) as u32,
							w: width,
							h: height
						};
						
						*result.lock() = Some(Ok(coords.clone()));
						
						let sub_image = SubImage {
							coords, sampler_desc,
							cache_id: cache_id.clone(),
							data: img_data,
							data_type: data_ty,
							notify: Some(ready.clone()),
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
					let mut notifies = Vec::new();
					
					for image in images.values_mut() {
						let (new_cmd_buf, mut notify) = image.update(cmd_buf);
						cmd_buf = new_cmd_buf;
						notifies.append(&mut notify);
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
					
					for abool in notifies {
						abool.store(true, atomic::Ordering::Relaxed);
					}
				}
			
				if iter_start.elapsed()	> Duration::from_millis(100) {
					continue;
				}
				
				thread::sleep(Duration::from_millis(100) - iter_start.elapsed());
			}
		});
		
		atlas_ret
	}
	
	pub fn remove(&self, _image_id: SubImageID) {
		println!("TODO: Atlas image remove not implemented!");
	}
	
	pub fn is_cached(&self, cache_id: SubImageCacheID) -> bool {
		self.cached_images.lock().contains_key(&cache_id)
	}
	
	pub fn load_image_from_url<U: AsRef<str>>(self: &Arc<Self>, url: U, sampler_desc: SamplerDesc) -> ImageLoad {
		let cache_id = SubImageCacheID::Url(url.as_ref().to_string());
		
		if let Some(sub_image_id) = self.cached_images.lock().get(&cache_id).clone() {
			let coords = self.image_coords(*sub_image_id).unwrap();
		
			return ImageLoad {
				atlas: self.clone(),
				ready: Arc::new(AtomicBool::new(true)),
				result: Arc::new(Mutex::new(Some(Ok(coords))))
			};
		}
		
		let result_ret = Arc::new(Mutex::new(None));
		let ready_ret = Arc::new(AtomicBool::new(false));
		let result = result_ret.clone();
		let ready = ready_ret.clone();
		let atlas = self.clone();
		let url = url.as_ref().to_string();
		
		thread::spawn(move || {
			let err = |e| {
				*result.lock() = Some(Err(e));
				ready.store(true, atomic::Ordering::Relaxed);
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
				result.clone(), ready.clone()
			)).unwrap();
		});
		
		ImageLoad {
			atlas: self.clone(),
			ready: ready_ret,
			result: result_ret
		}
	}
	
	pub fn load_image_from_path<P: Into<PathBuf>>(self: &Arc<Self>, path_buf: P, sampler_desc: SamplerDesc) -> ImageLoad {
		let path_buf = path_buf.into();
		let cache_id = SubImageCacheID::Path(path_buf.clone());
		
		if let Some(sub_image_id) = self.cached_images.lock().get(&cache_id).clone() {
			let coords = self.image_coords(*sub_image_id).unwrap();
		
			return ImageLoad {
				atlas: self.clone(),
				ready: Arc::new(AtomicBool::new(true)),
				result: Arc::new(Mutex::new(Some(Ok(coords))))
			};
		}
		
		let result_ret = Arc::new(Mutex::new(None));
		let ready_ret = Arc::new(AtomicBool::new(false));
		let result = result_ret.clone();
		let ready = ready_ret.clone();
		let atlas = self.clone();
		
		thread::spawn(move || {
			let err = |e| {
				*result.lock() = Some(Err(e));
				ready.store(true, atomic::Ordering::Relaxed);
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
				result.clone(), ready.clone()
			)).unwrap();
		
		});
		
		ImageLoad {
			atlas: self.clone(),
			ready: ready_ret,
			result: result_ret
		}
	}
	
	pub fn load_image(
		self: &Arc<Self>, cache_id: SubImageCacheID,
		ty: DataType, sampler_desc: SamplerDesc,
		width: u32, height: u32, data: Data
	) -> ImageLoad {
	
		let result = Arc::new(Mutex::new(None));
		let ready = Arc::new(AtomicBool::new(false));
		
		self.upload_queue.send((
			cache_id, ty, sampler_desc,
			width, height, data,
			result.clone(), ready.clone()
		)).unwrap();
		
		ImageLoad { 
			atlas: self.clone(),
			ready, result,
		}
	}
	
	pub fn empty_image(&self) -> Arc<ImageViewAccess + Send + Sync> {
		self.empty_image.clone()
	}
	
	pub fn default_sampler(&self) -> Arc<Sampler> {
		self.default_sampler.clone()
	}
	
	pub fn atlas_image(&self, id: AtlasImageID) -> Option<TmpImageViewAccess> {
		let mut images = self.images.lock();
		images.get_mut(&id)?.current_tmp()
	}
	
	pub fn cached_image_id(&self, cache_id: SubImageCacheID) -> Option<SubImageID> {
		self.cached_images.lock().get(&cache_id).cloned()
	}
	
	pub fn image_coords(&self, image_id: SubImageID) -> Option<Coords> {
		for image in self.images.lock().values_mut() {
			if let Some(coords) = image.image_coords(image_id) {
				return Some(coords)
			}
		} None
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

