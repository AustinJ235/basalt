use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferUsage;
use std::sync::{Arc,Weak};
use vulkano;
use std::collections::BTreeMap;
use Engine;
use super::bin::Bin;
use parking_lot::RwLock;
use super::super::atlas::Atlas;
use vulkano::sampler::Sampler;
use misc::BTreeMapExtras;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::command_buffer::AutoCommandBuffer;
use vulkano::buffer::DeviceLocalBuffer;

pub struct Interface {
	bin_i: u64,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	engine: Arc<Engine>,
	atlas: Arc<Atlas>,
	buffers: BTreeMap<usize, ItfBuffer>,
	custom_bufs: BTreeMap<u64, Vec<(
		Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
		Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>,
		Arc<Sampler>
	)>>,
}

impl_vertex!(ItfVertInfo, position, coords, color, ty);
#[derive(Clone)]
#[repr(C)]
pub(crate) struct ItfVertInfo {
	pub position: (f32, f32, f32),
	pub coords: (f32, f32),
	pub color: (f32, f32, f32, f32),
	pub ty: i32
}

impl Default for ItfVertInfo {
	fn default() -> Self {
		ItfVertInfo {
			position: (0.0, 0.0, 0.0),
			coords: (0.0, 0.0),
			color: (0.0, 0.0, 0.0, 0.0),
			ty: 0,
		}
	}
}

pub(crate) fn scale_verts(win_size: &[f32; 2], verts: &mut Vec<ItfVertInfo>) {
	for vert in verts {
		vert.position.0 += win_size[0] / -2.0;
		vert.position.0 /= win_size[0] / 2.0;
		vert.position.1 += win_size[1] / -2.0;
		vert.position.1 /= win_size[1] / 2.0;
	}
}

struct ItfBuffer {
	atlas_i: usize,
	dev_buf: Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
	in_buf: Vec<(u64, usize, usize)>,
	free: Vec<(usize, usize)>,
	to_rm: Vec<(usize, usize)>,
	to_add: Vec<(usize, Vec<ItfVertInfo>)>,
}

impl ItfBuffer {
	fn new(engine: &Arc<Engine>, atlas_i: usize) -> Self {
		let len = 10000000;
		
		let dev_buf = unsafe {
			DeviceLocalBuffer::raw(
				engine.device(),
				len * ::std::mem::size_of::<ItfVertInfo>(),
				BufferUsage::all(),
				vec![engine.graphics_queue().family()]
			).unwrap()
		};
		
		ItfBuffer {
			atlas_i,
			dev_buf,
			in_buf: Vec::new(),
			free: vec![(0, len)],
			to_rm: Vec::new(),
			to_add: Vec::new(),
		}
	}
	
	fn find_free(&self, len: usize) -> Option<(usize, usize)> {
		let mut ops = Vec::new();
	
		for (free_pos, free_len) in &self.free {
			if *free_len >= len {
				ops.push((free_len-len, free_pos, free_len));
			}
		}
		
		ops.sort_by_key(|k| k.0);
		ops.pop().map(|v| (*v.1, *v.2))
	}
	
	fn add(&mut self, id: u64, pos: usize, data: Vec<ItfVertInfo>) -> Result<(), ()> {
		let mut free_i_op = None;
	
		for (free_i, (free_pos, free_len)) in self.free.iter().enumerate() {
			if pos >= *free_pos && pos + data.len() <= *free_pos + free_len {
				free_i_op = Some(free_i);
				break;
			}
		}
		
		if free_i_op.is_none() {
			return Err(());
		}
		
		let (free_pos, free_len) = self.free.swap_remove(free_i_op.unwrap());
		
		if free_len > data.len() {
			self.free.push((free_pos+data.len(), free_len-data.len()));
		}
		
		self.in_buf.push((id, free_pos, data.len()));
		self.to_add.push((free_pos, data));
		Ok(())
	}
	
	fn remove(&mut self, id: u64) {
		let mut append_free = Vec::new();
	
		self.in_buf.retain(|(in_id, pos, len)| {
			if *in_id == id {
				append_free.push((*pos, *len));
				false
			} else {
				true
			}
		});
		
		for (pos, len) in append_free {
			self.to_rm.push((pos, len));
			self.free.push((pos, len));
		}
	}
	
	fn max(&self) -> usize {
		let mut max = 0;
	
		for (_, pos, len) in &self.in_buf {
			let bin_max = pos + len;
			
			if bin_max > max {
				max = bin_max;
			}
		}
		
		max
	}
}

impl Interface {
	pub(crate) fn new(engine: Arc<Engine>) -> Self {
		let bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>> = Arc::new(RwLock::new(BTreeMap::new()));
		let bin_map_cp = bin_map.clone();
		
		engine.mouse_ref().on_any_press(Arc::new(move |_, info| {
			let bins: Vec<Arc<Bin>> = bin_map_cp.read().iter().filter_map(|(_, b)| b.upgrade()).collect();
			let mut inside = Vec::new();
			
			for bin in bins {
				if bin.mouse_inside(info.window_x, info.window_y) {
					if !bin.inner_copy().pass_events.unwrap_or(false) {
						let z = bin.box_points().z_index;
						inside.push((z, bin));
					}
				}
			}
			
			inside.sort_by_key(|&(z, _)| z);
			
			if let Some((_, bin)) = inside.pop() {
				bin.call_left_mouse_press();
			}
		}));
		
		Interface {
			bin_i: 0,
			bin_map: bin_map,
			atlas: engine.atlas(),
			engine: engine,
			buffers: BTreeMap::new(),
			custom_bufs: BTreeMap::new(),
		}
	}
	
	pub fn get_bin_id_atop(&self, x: f32, y: f32) -> Option<u64> {
		let bins: Vec<Arc<Bin>> = self.bin_map.read().iter().filter_map(|(_, b)| b.upgrade()).collect();
		let mut inside = Vec::new();
		
		for bin in bins {
			if bin.mouse_inside(x, y) {
				if !bin.inner_copy().pass_events.unwrap_or(false) {
					let z = bin.box_points().z_index;
					inside.push((z, bin));
				}
			}
		}
		
		inside.sort_by_key(|&(z, _)| z);
		inside.pop().map(|v| v.1.id())
	}
	
	fn bins(&self) -> Vec<Arc<Bin>> {
		self.bin_map.read().iter().filter_map(|(_, b)| b.upgrade()).collect()
	}
	
	fn bins_with_clean(&mut self) -> Vec<Arc<Bin>> {
		let mut delete = Vec::new();
		let mut bin_map = self.bin_map.write();
		
		let out = bin_map.iter().filter_map(|(id, b)| {
			match b.upgrade() {
				Some(some) => Some(some),
				None => {
					delete.push(id.clone());
					None
				}
			}
		}).collect();
		
		for id in delete {
			bin_map.remove(&id);
		}
		
		out
	}
	
	pub fn new_bins(&mut self, amt: usize) -> Vec<Arc<Bin>> {
		let mut out = Vec::with_capacity(amt);
		for _ in 0..amt {
			let id = self.bin_i;
			self.bin_i += 1;
			let bin = Bin::new(id.clone(), self.engine.clone(), self.atlas.clone());
			self.bin_map.write().insert(id, Arc::downgrade(&bin));
			out.push(bin);
		} out
	}
	
	pub fn new_bin(&mut self) -> Arc<Bin> {
		let id = self.bin_i;
		self.bin_i += 1;
		let bin = Bin::new(id.clone(), self.engine.clone(), self.atlas.clone());
		self.bin_map.write().insert(id, Arc::downgrade(&bin));
		bin
	}
	
	pub fn get_bin(&self, id: u64) -> Option<Arc<Bin>> {
		match self.bin_map.read().get(&id) {
			Some(some) => some.upgrade(),
			None => None
		}
	}
	
	pub fn mouse_inside(&self, mouse_x: f32, mouse_y: f32) -> bool {
		for bin in self.bins() {
			if bin.mouse_inside(mouse_x, mouse_y) {
				return true;
			}
		} false
	}
	
	pub(crate) fn update_buffers(&mut self, win_size: [usize; 2], resized: bool)
		-> Vec<AutoCommandBuffer<vulkano::command_buffer::pool::standard::StandardCommandPoolAlloc>>
	{
		let mut cmd_buf = AutoCommandBufferBuilder::new(self.engine.device(), self.engine.graphics_queue_ref().family()).unwrap();
		const VERT_SIZE: usize = ::std::mem::size_of::<ItfVertInfo>();
		
		for bin in self.bins_with_clean() {
			if let Some(vert_data) = bin.update_verts([win_size[0] as f32, win_size[1] as f32], resized) {
				for (_, mut buffer) in &mut self.buffers {
					buffer.remove(bin.id());
				}
				
				self.custom_bufs.remove(&bin.id());
				let mut bin_custom_bufs = Vec::new();
				
				for (verts, custom_img, atlas_i) in vert_data {
					if custom_img.is_some() {
						let sampler = Sampler::simple_repeat_linear_no_mipmap(self.engine.device());
						let byte_size = verts.len() * VERT_SIZE;
						
						let tmp_buf = CpuAccessibleBuffer::from_iter(
							self.engine.device(), BufferUsage::all(),
							verts.into_iter()
						).unwrap();
						
						let dev_buf = unsafe {
							DeviceLocalBuffer::raw(
								self.engine.device(),
								byte_size,
								BufferUsage::all(),
								vec![self.engine.graphics_queue().family()]
							).unwrap()
						};
						
						cmd_buf = cmd_buf.copy_buffer(tmp_buf, dev_buf.clone()).unwrap();
						bin_custom_bufs.push((dev_buf, custom_img.unwrap(), sampler));
						continue;
					}
					
					let engine_cp = self.engine.clone();
					let mut buffer = self.buffers.get_mut_or_else(&atlas_i, || { ItfBuffer::new(&engine_cp, atlas_i) });
					let (free_pos, _) = buffer.find_free(verts.len()).expect("ItfBuffer doesn't have any freespace left!");
					buffer.add(bin.id(), free_pos, verts).expect("ItfBuffer failed to add must not be enough freespace.");
				}
				
				if !bin_custom_bufs.is_empty() {
					self.custom_bufs.insert(bin.id(), bin_custom_bufs);
				}
			}
		}

		for (_, mut buffer) in &mut self.buffers {
			let mut cpu_buf_data = Vec::new();
			let mut regions = Vec::new();
		
			for (pos, len) in buffer.to_rm.split_off(0) {
				if len > cpu_buf_data.len() {
					cpu_buf_data.resize(len, ItfVertInfo::default());
				}
				
				regions.push((0, pos * VERT_SIZE, len * VERT_SIZE));
			}

			for (pos, mut data) in buffer.to_add.split_off(0) {
				regions.push((cpu_buf_data.len() * VERT_SIZE, pos * VERT_SIZE, data.len() * VERT_SIZE));
				cpu_buf_data.append(&mut data);
			}
			
			let tmp_buf = CpuAccessibleBuffer::from_iter(
				self.engine.device(), BufferUsage::all(),
				cpu_buf_data.into_iter()
			).unwrap();
			
			cmd_buf = cmd_buf.copy_buffer_with_regions(tmp_buf, buffer.dev_buf.clone(), regions.into_iter()).unwrap();
		}

		let mut cmds = vec![cmd_buf.build().unwrap()];
		cmds.append(&mut self.atlas.update(self.engine.device(), self.engine.graphics_queue()));
		cmds
	}
	
	pub(crate) fn draw_bufs(&mut self)
		-> Vec<(
			Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
			Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>,
			Arc<Sampler>,
			Option<usize>
		)>
	{
		let mut out = Vec::new();
	
		for (_, buffer) in &self.buffers {
			let(image, sampler) = match self.engine.atlas_ref().image_and_sampler(buffer.atlas_i) {
				Some(some) => some,
				None => (self.engine.atlas_ref().null_img(self.engine.transfer_queue()), Sampler::simple_repeat_linear_no_mipmap(self.engine.device()))
			};
		
			out.push((
				buffer.dev_buf.clone(),
				image,
				sampler,
				Some(buffer.max())
			));
		}
		
		for (_, verts_data) in &self.custom_bufs {
			for (buf, image, sampler) in verts_data {
				out.push((
					buf.clone(),
					image.clone(),
					sampler.clone(),
					None
				));
			}
		}
		
		out
	}
}

