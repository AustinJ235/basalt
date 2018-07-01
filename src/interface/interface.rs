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
use std::sync::atomic::AtomicPtr;
use vulkano::command_buffer::AutoCommandBuffer;

pub struct Interface {
	bin_i: u64,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	engine: Arc<Engine>,
	atlas: Arc<Atlas>,
	buffers: BTreeMap<usize, ItfBuffer>,
	custom_bufs: BTreeMap<u64, Vec<(
		Arc<CpuAccessibleBuffer<[ItfVertInfo]>>,
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
	buf: Arc<CpuAccessibleBuffer<[ItfVertInfo]>>,
	in_buf: Vec<(u64, usize, usize)>,
	free: Vec<(usize, usize)>,
	to_rm: Vec<(usize, usize)>,
	to_add: Vec<(usize, Vec<ItfVertInfo>)>,
}

impl ItfBuffer {
	fn new(engine: &Arc<Engine>, atlas_i: usize) -> Self {
		let len = 1000000;	
		let mut empty = Vec::with_capacity(len);
		empty.resize(len, ItfVertInfo::default());
		
		let buf = CpuAccessibleBuffer::from_iter(
			engine.device(), BufferUsage::all(),
			empty.into_iter()
		).unwrap();
		
		ItfBuffer {
			atlas_i,
			buf,
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
		let mut timer = ::timer::Timer::new();
		timer.start("vert update");
		
		let mut bins_updated = 0;
	
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
						let buf = CpuAccessibleBuffer::from_iter(
							self.engine.device(), BufferUsage::all(),
							verts.into_iter()
						).unwrap();
						
						bin_custom_bufs.push((buf, custom_img.unwrap(), sampler));
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
				
				bins_updated += 1;
			}
		}
		
		timer.start("cmd create");
		
		let mut cmd_rm = 0;
		let mut cmd_add = 0;
		
		let mut cmd_buf = AutoCommandBufferBuilder::new(self.engine.device(), self.engine.graphics_queue_ref().family()).unwrap();
		let mut zero = Vec::new();
		
		for (_, mut buffer) in &mut self.buffers {
			for (pos, len) in buffer.to_rm.split_off(0) {
				zero.resize(len, ItfVertInfo::default());
				
				cmd_buf = cmd_buf.update_buffer_naughty(
					buffer.buf.clone(),
					unsafe { AtomicPtr::new(::std::mem::transmute(zero.as_ptr())) },
					pos * ::std::mem::size_of::<ItfVertInfo>(),
					len * ::std::mem::size_of::<ItfVertInfo>()
				).unwrap();
				
				cmd_rm += 1;
			}
			
			for (pos, data) in buffer.to_add.split_off(0) {
				cmd_buf = cmd_buf.update_buffer_naughty(
					buffer.buf.clone(),
					unsafe { AtomicPtr::new(::std::mem::transmute(data.as_ptr())) },
					pos * ::std::mem::size_of::<ItfVertInfo>(),
					data.len() * ::std::mem::size_of::<ItfVertInfo>()
				).unwrap();
				
				cmd_add += 1;
			}
		}

		let mut cmds = vec![cmd_buf.build().unwrap()];
		timer.start("atlas update");
		cmds.append(&mut self.atlas.update(self.engine.device(), self.engine.graphics_queue()));
		//println!("{}, bins_updated: {}, remove cmds: {}, add cmds: {}", timer.display(), bins_updated, cmd_rm, cmd_add);
		cmds
	}
	
	pub(crate) fn draw_bufs(&mut self)
		-> Vec<(
			Arc<CpuAccessibleBuffer<[ItfVertInfo]>>,
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
				buffer.buf.clone(),
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
	
	/*pub(crate) fn draw_bufs_old(&mut self, device: Arc<Device>, queue: Arc<device::Queue>, win_size: [usize; 2], resized: bool)
		-> Vec<(
			Arc<CpuAccessibleBuffer<[ItfVertInfo]>>,
			Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>,
			Arc<Sampler>,
		)>
	{
		let mut vert_map = BTreeMap::new();
		let mut out = Vec::new();
		let mut custom_out = Vec::new();
		let mut update_count = 0;
		
		for bin in self.bins_with_clean() {
			let (vert_data, updated) = bin.verts([win_size[0] as f32, win_size[1] as f32], resized);
			
			if updated {
				update_count += 1;
			}
		
			for (mut verts, img_, atlas_i) in vert_data {
				if img_.is_none() {
					let append_to = vert_map.get_mut_or_else(&atlas_i, || { Vec::new() });
					append_to.append(&mut verts);
				} else {
					let sampler = Sampler::simple_repeat_linear_no_mipmap(device.clone());
					let img = img_.unwrap();
					let buf = CpuAccessibleBuffer::from_iter(
						device.clone(), BufferUsage::all(),
						verts.into_iter()
					).unwrap();
					
					custom_out.push((buf, img, sampler));
				}
			}
		}
		
		if let Some(_) = option_env!("BIN_UP_COUNT") {
			if update_count > 0 {
				println!("Interface Updated {} Bins", update_count);
			}
		}
		
		for (atlas_i, verts) in vert_map {
			let (img, sampler) = {
				if atlas_i == 0 {
					(self.atlas.null_img(queue.clone()), Sampler::simple_repeat_linear_no_mipmap(device.clone()))
				} else {
					match self.engine.atlas().image_and_sampler(atlas_i) {
						Some(some) => some,
						None => (self.atlas.null_img(queue.clone()), Sampler::simple_repeat_linear_no_mipmap(device.clone()))
					}
				}
			}; let buf = CpuAccessibleBuffer::from_iter(
				device.clone(), BufferUsage::all(),
				verts.into_iter()
			).unwrap();
			
			out.push((buf, img, sampler));
		}
		
		out.append(&mut custom_out);
		
		//self.atlas.update(device.clone(), queue.clone());
		out
	}*/
}

