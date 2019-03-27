#![allow(warnings)]

use vulkano::sampler::MipmapMode;
use vulkano::sampler::UnnormalizedSamplerAddressMode;
use std::sync::atomic::{self,AtomicU64};
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

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Debug,Hash)]
pub struct SubImageID(u64);

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Debug,Hash)]
pub struct AtlasImageID(u64);

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub enum SubImageCacheID {
	Path(PathBuf),
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

pub struct SubImage {
	pub cache_id: SubImageCacheID,
	pub coords: Coords,
	pub data_type: DataType,
	pub data: Data,
	pub sampler_desc: SamplerDesc,
}

pub struct Image {
	images: Vec<(Arc<ImageViewAccess + Send + Sync>, Vec<TmpImageViewAccess>)>,
	current: usize,
	stored: BTreeMap<SubImageID, SubImage>,
}

pub struct ImageLoad {
	atlas: Arc<Atlas>,
	barrier: Arc<Barrier>,
	result: Arc<Mutex<Option<Result<SubImageID, String>>>>,
}

impl ImageLoad {
	pub fn wait(self) -> Result<SubImageID, String> {
		self.barrier.wait();
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
		SamplerDesc, Data,
		Arc<Mutex<Option<Result<SubImageID, String>>>>, Arc<Barrier>
	)>,
}

impl Atlas {
	pub fn new() -> Arc<Self> {
		let (upload_queue_s, upload_queue_r) = channel::unbounded();
	
		let atlas = Arc::new(Atlas {
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
				while let Ok(ok) = upload_queue_r.try_recv() {
				
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
	
	pub fn load_image_from_path(&self, path_buf: PathBuf, sampler_desc: SamplerDesc) -> ImageLoad {
		unimplemented!()
	}
	
	pub fn load_image(self: Arc<Self>, cache_id: SubImageCacheID, ty: DataType, sampler_desc: SamplerDesc, data: Data) -> ImageLoad {
		let result = Arc::new(Mutex::new(None));
		let barrier = Arc::new(Barrier::new(2));
		self.upload_queue.send((cache_id, ty, sampler_desc, data, result.clone(), barrier.clone())).unwrap();
		
		ImageLoad { 
			atlas: self.clone(),
			barrier, result,
		}
	}
	
	pub fn image_coords(&self, id: SubImageID) -> Coords {
		unimplemented!()
	}
	
	pub fn cached_image_coords(&self, cache_id: SubImageCacheID) -> Coords {
		unimplemented!()
	}
}

