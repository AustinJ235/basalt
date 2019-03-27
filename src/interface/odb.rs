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
use Engine;
use vulkano::buffer::BufferUsage;
use std::thread;
use std::time::Duration;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;
use vulkano::buffer::BufferAccess;
use decorum::R32;
use atlas;

const VERT_SIZE: usize = ::std::mem::size_of::<ItfVertInfo>();

pub struct OrderedDualBuffer {
	active: Mutex<Buffer>,
	inactive: Mutex<Buffer>,
}

struct Buffer {
	engine: Arc<Engine>,
	chunks: Vec<Chunk>,
	bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	buffer_op: Option<Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
	draw_sets: Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<ImageViewAccess + Send + Sync>,
		Arc<Sampler>
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
	atlas_id: atlas::AtlasImageID,
	bin_id: u64,
	version: Instant,
	data: ChunkData,
	image_op: Option<Arc<ImageViewAccess + Send + Sync>>,
}

impl Buffer {
	fn new(engine: Arc<Engine>, bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Self {
		Buffer {
			bins,
			chunks: Vec::new(),
			buffer_op: None,
			draw_sets: Vec::new(),
			engine,
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
			let mut data_mapped: BTreeMap<R32, BTreeMap<atlas::AtlasImageID, Vec<(Vec<ItfVertInfo>, Option<_>)>>> = BTreeMap::new();
			
			for (data, image_op, mut atlas_id) in bin.verts_cp() {
				debug_assert!(atlas_id != atlas::AtlasImageID(u64::max_value()));
				let mut tri = Vec::new();
				
				if image_op.is_some() {
					atlas_id = atlas::AtlasImageID(u64::max_value());
				}
				
				for vert in data.into_iter() {
					tri.push(vert);
					
					if tri.len() < 3 {
						continue;
					}
					
					let mut entry_point = data_mapped
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
				self.engine.device(),
				dst_len * VERT_SIZE,
				BufferUsage::all(),
				vec![self.engine.graphics_queue().family()]
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
				self.engine.device(), BufferUsage::all(),
				src_data.into_iter()
			).unwrap())	
		};
		
		let mut cmd_builder = AutoCommandBufferBuilder::new(
			self.engine.device(),
			self.engine.transfer_queue_ref().family()
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
		let fence = cmd_buf.execute(self.engine.transfer_queue()).unwrap().then_signal_fence_and_flush().unwrap();
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
					let (image, sampler) = match &self.chunks[c_i-1].image_op {
						Some(some) => (some.clone(), self.engine.atlas_ref().default_sampler()),
						None => match self.chunks[c_i-1].atlas_id {
							atlas::AtlasImageID(18446744073709551615) => panic!("invalid atlas id: reserved for custom images!"),
							atlas::AtlasImageID(0) => (
								self.engine.atlas_ref().empty_image(),
								self.engine.atlas_ref().default_sampler()
							),
							atlas_id => match self.engine.atlas_ref().atlas_image(atlas_id) {
								Some(some) => (
									Arc::new(some) as Arc<_>,
									self.engine.atlas_ref().default_sampler()
								),
								None => (
									self.engine.atlas_ref().empty_image(),
									self.engine.atlas_ref().default_sampler()
								)
							}
						}
					};
					
					if c_i == self.chunks.len() - 1 {
						cur_pos += data_len;
					}
					
					self.draw_sets.push((
						dst_buf.clone().into_buffer_slice().slice(start..cur_pos).unwrap(),
						image, sampler
					));
					
					start = cur_pos;
					last_atlas = self.chunks[c_i].atlas_id;
				}
				
				cur_pos += data_len;
			}
		}
		
		fence.wait(None).unwrap();
		true
	}
}

impl OrderedDualBuffer {
	pub fn new(engine: Arc<Engine>, bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		let odb = Arc::new(OrderedDualBuffer {
			active: Mutex::new(Buffer::new(engine.clone(), bins.clone())),
			inactive: Mutex::new(Buffer::new(engine.clone(), bins)),
		});
		let odb_ret = odb.clone();
		
		thread::spawn(move || {
			loop {
				let start = Instant::now();
				let mut inactive = odb.inactive.lock();
				
				if inactive.update() {
					let mut active = odb.active.lock();
					::std::mem::swap(&mut *active, &mut *inactive);
				}
				
				let elapsed = start.elapsed();
				
				if elapsed < Duration::from_millis(5) {
					thread::sleep(Duration::from_millis(5) - elapsed);
				}
			}
		});
		
		odb_ret
	}
	
	pub(crate) fn draw_data(&self, win_size: [u32; 2], resize: bool, scale: f32) -> Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<ImageViewAccess + Send + Sync>,
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
				active.draw_sets.clone()
			}, false => self.active.lock().draw_sets.clone()
		}
	}
}
