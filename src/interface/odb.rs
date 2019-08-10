use parking_lot::{Mutex,RwLock};
use vulkano::buffer::DeviceLocalBuffer;
use interface::interface::ItfVertInfo;
use std::time::Instant;
use interface::bin::Bin;
use std::sync::{Arc,Weak};
use std::collections::BTreeMap;
use vulkano::sampler::Sampler;
use vulkano::image::traits::ImageViewAccess;
use vulkano::buffer::BufferSlice;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use Basalt;
use vulkano::buffer::BufferUsage;
use std::thread;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;
use vulkano::buffer::BufferAccess;
use decorum::R32;
use atlas;
use std::collections::HashMap;
use crossbeam::sync::{Parker,Unparker};

const VERT_SIZE: usize = ::std::mem::size_of::<ItfVertInfo>();

pub struct OrderedDualBuffer {
	basalt: Arc<Basalt>,
	active: Mutex<Buffer>,
	inactive: Mutex<Buffer>,
	atlas_draw: Mutex<Option<Arc<HashMap<atlas::AtlasImageID, Arc<dyn ImageViewAccess + Send + Sync>>>>>,
	draw_sets: Mutex<Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<dyn ImageViewAccess + Send + Sync>, Arc<Sampler>,
	)>>,
	park: Mutex<Parker>,
	unpark: Unparker,
}

struct Buffer {
	basalt: Arc<Basalt>,
	chunks: Vec<Chunk>,
	bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	buffer_op: Option<Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
	draw_sets: Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		atlas::AtlasImageID, Option<Arc<dyn ImageViewAccess + Send + Sync>>,
	)>,
	resize: bool,
	win_size: [f32; 2],
	scale: f32,
}

enum ChunkData {
	Local(Vec<ItfVertInfo>),
	InBuf(usize, usize),
}

struct Chunk {
	z_index: R32,
	atlas_id: u64,
	bin_id: u64,
	version: Instant,
	data: ChunkData,
	image_op: Option<Arc<dyn ImageViewAccess + Send + Sync>>,
}

impl Buffer {
	fn new(basalt: Arc<Basalt>, bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Self {
		Buffer {
			bins,
			chunks: Vec::new(),
			buffer_op: None,
			draw_sets: Vec::new(),
			basalt,
			resize: false,
			win_size: [1920.0, 1080.0],
			scale: 1.0,
		}
	}
	
	fn update(&mut self) -> bool {
		let mut new_chunks = Vec::new();
		let mut up_bin_ids = Vec::new();
		let mut alive_bins = Vec::new();
		let bins: Vec<_> = self.bins.read().iter().filter_map(|(id, w)| w.upgrade().map(|v| (*id, v))).collect();
		let win_size = self.win_size.clone();
		let scale = self.scale.clone();
		//let start = Instant::now();
		
		::misc::do_work(
			bins.iter().filter_map(|(_, b)| match b.wants_update() || self.resize {
				true => Some(b.clone()),
				false => None
			}).collect(),
			Arc::new(move |bin| {
				bin.do_update(win_size, scale);
			})
		);
		
		//println!("{}", start.elapsed().as_micros() as f32 / 1000.0);
		
		for (id, bin) in bins {
			alive_bins.push(id);
		
			let mut prexisting_chunks: Vec<_> = self.chunks.iter().filter(|c| c.bin_id == id).collect();
			let latest_version = bin.last_update();
			
			if !prexisting_chunks.is_empty() {
				prexisting_chunks.sort_by_key(|c| c.version);
				
				if prexisting_chunks.first().unwrap().version >= latest_version {
					continue;
				}
			}
			
			up_bin_ids.push(id);
			let mut data_mapped: BTreeMap<R32, BTreeMap<u64, Vec<(Vec<ItfVertInfo>, Option<_>)>>> = BTreeMap::new();
			
			for (data, image_op, mut atlas_id) in bin.verts_cp() {
				debug_assert!(atlas_id != u64::max_value());
				let mut tri = Vec::new();
				
				if image_op.is_some() {
					atlas_id = u64::max_value();
				}
				
				for vert in data.into_iter() {
					tri.push(vert);
					
					if tri.len() < 3 {
						continue;
					}
					
					let entry_point = data_mapped
						.entry(R32::from(-1.0 * tri[0].position.2)).or_insert(BTreeMap::new())
						.entry(atlas_id).or_insert(Vec::new());
						
					if entry_point.is_empty() {
						entry_point.push((tri.split_off(0), image_op.clone()));
					} else {
						let mut found = false;
						
						for (d, iop) in &mut *entry_point {
							if
								(iop.is_none() && image_op.is_none())
								|| (
									iop.is_some() && image_op.is_some()
									&& Arc::ptr_eq(iop.as_ref().unwrap(), image_op.as_ref().unwrap())
								)
							{
								d.append(&mut tri);
								found = true;
								break;
							}
						}
						
						if !found {
							entry_point.push((tri.split_off(0), image_op.clone()));
						}
					}
				}
			}
			
			for (z_index, atlas_mapped) in data_mapped {
				for (atlas_id, data_image) in atlas_mapped {
					for (data, image_op) in data_image {
						new_chunks.push(Chunk {
							z_index,
							atlas_id,
							bin_id: bin.id(),
							version: latest_version.clone(),
							data: ChunkData::Local(data),
							image_op,
						});
					}
				}
			}
		}
		
		self.resize = false;
		let start_len = self.chunks.len();
		
		for i in (0..self.chunks.len()).rev() {
			if
				up_bin_ids.contains(&self.chunks[i].bin_id)
				|| !alive_bins.contains(&self.chunks[i].bin_id)
			{
				self.chunks.swap_remove(i);
			}
		}
		
		if start_len == self.chunks.len() && new_chunks.is_empty() {
			return false;
		}
		
		self.chunks.append(&mut new_chunks);
		
		if self.chunks.is_empty() {
			return if self.buffer_op.is_some() {
				self.buffer_op = None;
				true
			} else {
				false
			};
		}
		
		self.chunks.sort_by_key(|c| c.z_index);
		let mut cur_z = self.chunks[0].z_index;
		let mut cur_z_s = 0;
		
		for c_i in 1..self.chunks.len() {
			if self.chunks[c_i].z_index != cur_z || c_i == self.chunks.len() - 1 {
				self.chunks.as_mut_slice()[cur_z_s..c_i].sort_by_key(|c| c.atlas_id);
				cur_z = self.chunks[c_i].z_index;
				cur_z_s = c_i;
			}
		}
		
		let dst_len: usize = self.chunks.iter().map(|c| match &c.data {
			ChunkData::Local(d) => d.len(),
			ChunkData::InBuf(_, l) => *l
		}).sum();
		
		let src_len: usize = self.chunks.iter().map(|c| match &c.data {
			ChunkData::Local(d) => d.len(),
			ChunkData::InBuf(_, _) => 0
		}).sum();
		
		let dst_buf = unsafe {
			DeviceLocalBuffer::raw(
				self.basalt.device(),
				dst_len * VERT_SIZE,
				BufferUsage::all(),
				vec![self.basalt.graphics_queue().family()]
			).unwrap()
		};
		
		let old_buf_op = self.buffer_op.take();
		self.buffer_op = Some(dst_buf.clone());
		
		let mut src_data = Vec::with_capacity(src_len);
		let mut copy_from_src: Vec<(usize, usize, usize)> = Vec::new();
		let mut copy_from_dev: Vec<(usize, usize, usize)> = Vec::new();
		let mut cur_pos = 0;
		
		for chunk in &mut self.chunks {
			let (p, l) = match &mut chunk.data {
				ChunkData::Local(ref mut d) => {
					let src = src_data.len();
					let len = d.len();
					src_data.append(d);
					copy_from_src.push((src, cur_pos, len));
					(cur_pos, len)
				}, ChunkData::InBuf(p, l) => {
					copy_from_dev.push((*p, cur_pos, *l));
					(cur_pos, *l)
				}
			};
			
			chunk.data = ChunkData::InBuf(p, l);
			cur_pos += l;
		}
		
		let src_buf_op = if src_len == 0 {
			None
		} else {
			Some(CpuAccessibleBuffer::from_iter(
				self.basalt.device(), BufferUsage::all(),
				src_data.into_iter()
			).unwrap())	
		};
		
		let mut cmd_builder = AutoCommandBufferBuilder::new(
			self.basalt.device(),
			self.basalt.transfer_queue_ref().family()
		).unwrap();
		
		for (src, dst, len) in copy_from_src {
			cmd_builder = cmd_builder.copy_buffer(
				src_buf_op.clone().unwrap().into_buffer_slice().slice(src..(src+len)).unwrap(),
				dst_buf.clone().into_buffer_slice().slice(dst..(dst+len)).unwrap()
			).unwrap();
		}
		
		if old_buf_op.is_none() && !copy_from_dev.is_empty() {
			panic!("Unable to copy from device buffer. Old buffer doesn't exist!");
		}
		
		for (src, dst, len) in copy_from_dev {
			cmd_builder = cmd_builder.copy_buffer(
				old_buf_op.clone().unwrap().into_buffer_slice().slice(src..(src+len)).unwrap(),
				dst_buf.clone().into_buffer_slice().slice(dst..(dst+len)).unwrap()
			).unwrap();
		}
		
		let cmd_buf = cmd_builder.build().unwrap();
		let future = cmd_buf.execute(self.basalt.transfer_queue()).unwrap();
		self.draw_sets = Vec::new();
		
		if !self.chunks.is_empty() {
			let mut last_atlas = self.chunks[0].atlas_id;
			let mut cur_pos = match &self.chunks[0].data {
				ChunkData::Local(_) => unreachable!(),
				ChunkData::InBuf(_, l) => *l
			};
			let mut start = 0;
			
			for c_i in 1..self.chunks.len() {
				let data_len = match &self.chunks[c_i].data {
					ChunkData::Local(_) => unreachable!(),
					ChunkData::InBuf(_, l) => *l
				};
			
				if
					self.chunks[c_i].atlas_id != last_atlas
					|| self.chunks[c_i].image_op.is_some()
					|| c_i == self.chunks.len() - 1
				{
					if c_i == self.chunks.len() - 1 {
						cur_pos += data_len;
					}
					
					self.draw_sets.push((
						dst_buf.clone().into_buffer_slice().slice(start..cur_pos).unwrap(),
						self.chunks[c_i-1].atlas_id, self.chunks[c_i-1].image_op.clone()
					));
					
					start = cur_pos;
					last_atlas = self.chunks[c_i].atlas_id;
				}
				
				cur_pos += data_len;
			}
		}
		
		let mut future = future.then_signal_semaphore_and_flush().unwrap();
		future.cleanup_finished();
		drop(future);
		true
	}
}

impl OrderedDualBuffer {
	pub fn new(basalt: Arc<Basalt>, bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		let park = Parker::new();
		let unpark = park.unparker().clone();
		
		let odb = Arc::new(OrderedDualBuffer {
			basalt: basalt.clone(),
			active: Mutex::new(Buffer::new(basalt.clone(), bins.clone())),
			inactive: Mutex::new(Buffer::new(basalt.clone(), bins)),
			atlas_draw: Mutex::new(None),
			draw_sets: Mutex::new(Vec::new()),
			park: Mutex::new(park),
			unpark,
		});
		let odb_ret = odb.clone();
		
		thread::spawn(move || {
			let mut force_update = false;
			let mut update_draw = false;
			let mut image_views_ver = Instant::now();
			
			loop {
				if force_update {
					let mut inactive = odb.inactive.lock();
					inactive.resize = true;
					inactive.update();
					
					let mut active = odb.active.lock();
					::std::mem::swap(&mut *active, &mut *inactive);
					drop(active);
					
					inactive.resize = true;
					inactive.update();
					
					let mut active = odb.active.lock();
					::std::mem::swap(&mut *active, &mut *inactive);
					force_update = false;
					update_draw = true;
					continue;
				}
				
				let mut inactive = odb.inactive.lock();
				
				if inactive.update()  {
					let mut active = odb.active.lock();
					::std::mem::swap(&mut *active, &mut *inactive);
					update_draw = true;
				}
				
				drop(inactive);
				let mut draw_op = odb.atlas_draw.lock();
				
				if let Some((version, image_views)) = odb.basalt.atlas_ref().image_views() {
					if version != image_views_ver {
						*draw_op = Some(image_views);
						image_views_ver = version;
						force_update = true;
						continue;
					}
				}
				
				if update_draw {
					let mut draw_sets = Vec::new();
				
					if let Some(draw) = draw_op.as_ref() {	
						for (buf, atlas_img_id, image_op) in &odb.active.lock().draw_sets {
							let img: Arc<dyn ImageViewAccess + Send + Sync> = match atlas_img_id {
								&0 => odb.basalt.atlas_ref().empty_image(),
								&::std::u64::MAX => match image_op {
									&Some(ref some) => some.clone(),
									&None => odb.basalt.atlas_ref().empty_image()
								}, img_id => match draw.get(img_id) {
									Some(some) => some.clone(),
									None => odb.basalt.atlas_ref().empty_image()
								}
							};
							
							let sampler = odb.basalt.atlas_ref().default_sampler();
							draw_sets.push((buf.clone(), img, sampler));
						}
					}
					
					*odb.draw_sets.lock() = draw_sets;
					update_draw = false;
				}
				
				odb.park.lock().park();
			}
		});
		
		odb_ret
	}
	
	pub(crate) fn unpark(&self) {
		self.unpark.unpark()
	}
	
	pub(crate) fn draw_data(&self, win_size: [u32; 2], resize: bool, scale: f32) -> Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<dyn ImageViewAccess + Send + Sync>,
		Arc<Sampler>,
	)> {
		match resize {
			true => {
				let win_size = [win_size[0] as f32, win_size[1] as f32];
				let mut inactive = self.inactive.lock();
				let mut active = self.active.lock();
				active.resize = resize;
				active.win_size = win_size.clone();
				active.scale = scale;
				inactive.resize = resize;
				inactive.win_size = win_size;
				inactive.scale = scale;
				self.unpark.unpark();
			}, false => ()
		}
		
		self.draw_sets.lock().clone()
	}
}
