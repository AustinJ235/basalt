use Engine;

use std::sync::Arc;
use std::thread;
use std::time::{Duration,Instant};
use std::collections::HashMap;
use std::path::PathBuf;

use crossbeam::channel::{self,Sender};
use parking_lot::{Mutex,Condvar};

use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;
use vulkano::image::StorageImage;
use vulkano::image::Dimensions as VkDimensions;
use vulkano::image::ImageUsage as VkImageUsage;
use vulkano::buffer::BufferUsage as VkBufferUsage;
use vulkano::format::Format as VkFormat;

const ITER_DURATION: Duration = Duration::from_millis(25);
const CELL_WIDTH: u32 = 32;
const CELL_PAD: u32 = 4;

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
	img_id: AtlasImageID,
	sub_img_id: SubImageID,
	x: u32,
	y: u32,
	w: u32,
	h: u32
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
}

#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct ImageDims {
	pub w: u32,
	pub h: u32
}

#[derive(Debug,Clone)]
pub enum ImageData {
	D8(Vec<u8>),
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

pub struct Image {
	ty: ImageType,
	dims: ImageDims,
	data: ImageData,
}

impl Image {
	pub fn new(ty: ImageType, dims: ImageDims, data: ImageData) -> Result<Image, String> {
		Ok(Image {
			ty, dims, data
		})
	}
	
	fn to_lrgba(self) -> Self {
		unimplemented!()
	}
}

struct UploadReq {
	result: Mutex<Result<Coords, String>>,
	condvar: Condvar,
	image: Image,
}

impl UploadReq {
	fn ok(&self, ok: Coords) {
		let mut result = self.result.lock();
		*result = Ok(ok);
		self.condvar.notify_one();
	}
	
	fn err(&self, err: String) {
		let mut result = self.result.lock();
		*result = Err(err);
		self.condvar.notify_one();
	}
}

pub struct Atlas {
	engine: Arc<Engine>,
	upload_queue: Sender<UploadReq>,
}

impl Atlas {
	pub fn new(engine: Arc<Engine>) -> Arc<Self> {
		let (upload_queue, receiver) = channel::unbounded();
		let atlas_ret = Arc::new(Atlas {
			engine, upload_queue,
		});
		let atlas = atlas_ret.clone();
		
		thread::spawn(move || {
			let mut iter_start;
			let mut atlas_images: Vec<AtlasImage> = Vec::new();
			let mut sub_img_id_counter = 1;
			
			loop {
				iter_start = Instant::now();
			
				while let Ok(upreq) = receiver.try_recv() {
					let mut space_op = None;
					
					for (i, atlas_image) in atlas_images.iter().enumerate() {
						if let Some(region) = atlas_image.find_space_for(&upreq.image.dims) {
							space_op = Some((i, region));
							break;
						}
					}
					
					if space_op.is_none() {
						let atlas_image = AtlasImage::new(&atlas.engine);
						
						match atlas_image.find_space_for(&upreq.image.dims) {
							Some(region) => {
								space_op = Some((atlas_images.len(), region));
							}, None => {
								upreq.err(format!("Image to big to fit in atlas."));
								continue;
							}
						}
						
						atlas_images.push(atlas_image);
					}
					
					let (atlas_image_i, region) = space_op.unwrap();
					let sub_img_id = sub_img_id_counter;
					sub_img_id_counter += 1;
					let coords = region.coords(atlas_image_i as u64, sub_img_id, &upreq.image.dims);
					
					upreq.ok(coords);
					
					atlas_images[atlas_image_i].insert(&region, sub_img_id, coords, upreq.image);
				}
				
				let mut cmd_buf = AutoCommandBufferBuilder::new(
					atlas.engine.device(),
					atlas.engine.transfer_queue_ref().family()
				).unwrap();
				
				for atlas_image in &mut atlas_images {
					atlas_image.update(&mut cmd_buf);
				}
				
				cmd_buf
					.build().unwrap()
					.execute(atlas.engine.transfer_queue()).unwrap()
					.then_signal_fence_and_flush().unwrap()
					.wait(None).unwrap();
				
				let elapsed = iter_start.elapsed();
				
				if elapsed > ITER_DURATION {
					continue;
				}
				
				thread::sleep(ITER_DURATION - elapsed);
			
			}
		});
		
		atlas_ret
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
			y: (self.y as u32 * CELL_WIDTH) + (self.x.checked_sub(1).unwrap_or(0) as u32 * CELL_PAD),
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
	active: Option<usize>,
	update: Option<usize>,
	sto_imgs: Vec<Arc<StorageImage<VkFormat>>>,
	sub_imgs: HashMap<SubImageID, SubImage>,
	con_sub_img: Vec<Vec<SubImageID>>,
	alloc_cell_w: usize,
	alloc: Vec<Vec<Option<SubImageID>>>,
}

impl AtlasImage {
	pub fn new(engine: &Arc<Engine>) -> Self {
		let max_img_w = engine.limits().max_image_dimension_2d as f32;
		let alloc_cell_w = (max_img_w / (CELL_WIDTH + CELL_PAD) as f32).floor() as usize; // TODO: Could eek a bit more space out of this
		let mut alloc = Vec::with_capacity(alloc_cell_w);
		alloc.resize_with(alloc_cell_w, || {
			let mut out = Vec::with_capacity(alloc_cell_w);
			out.resize(alloc_cell_w, None);
			out
		});
	
		AtlasImage {
			alloc, alloc_cell_w,
			active: None,
			update: None,
			sto_imgs: Vec::new(),
			sub_imgs: HashMap::new(),
			con_sub_img: Vec::new(),
		}
	}

	pub fn find_space_for(&self, dims: &ImageDims) -> Option<Region> {
		let w = (dims.w as f32 / CELL_WIDTH as f32).ceil() as usize; // TODO: Include padding in available space
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
	
	pub fn insert(&mut self, region: &Region, sub_img_id: SubImageID, coords: Coords, img: Image) {
		for x in region.x..(region.x+region.w) {
			for y in region.y..(region.y+region.h) {
				self.alloc[x][y] = Some(sub_img_id);
			}
		}
		
		self.sub_imgs.insert(sub_img_id, SubImage { coords, img });
	}
	
	pub fn update(&mut self, cmd_buf: &mut AutoCommandBufferBuilder) {
		unimplemented!()
	}
}

