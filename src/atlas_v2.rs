#![allow(warnings)]

use vulkano::sampler::MipmapMode;
use vulkano::sampler::UnnormalizedSamplerAddressMode;
use std::sync::atomic::{self,AtomicU64,AtomicBool};
use vulkano::sampler::BorderColor;
use std::path::PathBuf;
use std::collections::{BTreeMap,HashMap};
use std::sync::Arc;
use vulkano::sampler::Sampler;
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
	pub mipmap_mode: MipmapMode,
	pub u_addr_mode: UnnormalizedSamplerAddressMode,
	pub v_addr_mode: UnnormalizedSamplerAddressMode,
}

impl Default for SamplerDesc {
	fn default() -> Self {
		SamplerDesc {
			mipmap_mode: MipmapMode::Linear,
			u_addr_mode: UnnormalizedSamplerAddressMode::ClampToEdge,
			v_addr_mode: UnnormalizedSamplerAddressMode::ClampToEdge,
		}
	}
}

impl SamplerDesc {
	pub fn linear_clamp_edge() -> Self {
		Self::default()
	}

	pub fn nearest_clamp_edge() -> Self {
		SamplerDesc {
			mipmap_mode: MipmapMode::Nearest,
			u_addr_mode: UnnormalizedSamplerAddressMode::ClampToEdge,
			v_addr_mode: UnnormalizedSamplerAddressMode::ClampToEdge,
		}
	}
	
	pub fn linear_clamp_border(border_color: BorderColor) -> Self {
		SamplerDesc {
			mipmap_mode: MipmapMode::Linear,
			u_addr_mode: UnnormalizedSamplerAddressMode::ClampToBorder(border_color),
			v_addr_mode: UnnormalizedSamplerAddressMode::ClampToBorder(border_color),
		}
	}
	
	pub fn nearest_clamp_border(border_color: BorderColor) -> Self {
		SamplerDesc {
			mipmap_mode: MipmapMode::Nearest,
			u_addr_mode: UnnormalizedSamplerAddressMode::ClampToBorder(border_color),
			v_addr_mode: UnnormalizedSamplerAddressMode::ClampToBorder(border_color),
		}
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

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub enum DataType {
	LRGBA,
	LRGB,
	LMono,
	SRGBA,
	SRGB,
	YUV,
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
	pub upload: bool,
	pub notify: Option<Arc<Barrier>>,
}

struct Image {
	limits: Arc<Limits>,
	images: Vec<(Arc<ImageViewAccess + Send + Sync>, Vec<Arc<AtomicBool>>)>,
	current: Option<usize>,
	sub_images: BTreeMap<SubImageID, SubImage>,
	sub_images_in: Vec<Vec<SubImageID>>,
	grid: Vec<Vec<Option<SubImageID>>>,
	grid_uw: usize,
}

const UNIT_SIZE: u32 = 32;
const UNIT_PADDING: u32 = 1;

impl Image {
	fn image_render_data(&mut self, image_id: SubImageID) -> Option<(TmpImageViewAccess, SamplerDesc, Coords)> {
		let image_i = self.current.as_ref()?;
		
		if self.sub_images_in[*image_i].contains(&image_id) {
			let sub_image = self.sub_images.get(&image_id)?;
			let (tmp_img, abool) = TmpImageViewAccess::new_abool(self.images[*image_i].0.clone());
			self.images[*image_i].1.push(abool);
			Some((tmp_img, sub_image.sampler_desc.clone(), sub_image.coords.clone()))
		} else {
			None
		}
	}

	fn new(limits: Arc<Limits>) -> Self {
		let grid_uw = (limits.max_image_dimension_2d as f32 / (UNIT_SIZE + (UNIT_PADDING * 2)) as f32).floor() as usize;
		let mut grid = Vec::with_capacity(grid_uw as usize);
		grid.resize_with(grid_uw as usize, || {
			let mut out = Vec::with_capacity(grid_uw as usize);
			out.resize(grid_uw as usize, None);
			out
		});
	
		Image {
			limits,
			images: Vec::new(),
			current: None,
			sub_images: BTreeMap::new(),
			sub_images_in: Vec::new(),
			grid, grid_uw
		}
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
	limits: Arc<Limits>,
}

impl Atlas {
	pub(crate) fn new(limits: Arc<Limits>) -> Arc<Self> {
		let (upload_queue_s, upload_queue_r) = channel::unbounded();
	
		let atlas = Arc::new(Atlas {
			sub_image_counter: AtomicU64::new(0),
			atlas_image_counter: AtomicU64::new(0),
			images: Mutex::new(BTreeMap::new()),
			cached_images: Mutex::new(HashMap::new()),
			sampler_cache: Mutex::new(HashMap::new()),
			upload_queue: upload_queue_s,
			limits,
		});
		let atlas_ret = atlas.clone();
		
		thread::spawn(move || {
			let mut iter_start = Instant::now();
			
			loop {
				while let Ok((
					cache_id, data_ty, sampler_desc,
					width, height, img_data, result, barrier
				)) = upload_queue_r.try_recv() {
				
				
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

