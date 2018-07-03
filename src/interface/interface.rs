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
use parking_lot::Mutex;
use std::time::Instant;
use std::sync::atomic::{self,AtomicUsize};
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;

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

#[allow(dead_code)]
struct BinBufferData {
	atlas_i: usize,
	pos: usize,
	len: usize,
}

pub struct Interface {
	bin_i: u64,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	engine: Arc<Engine>,
	atlas: Arc<Atlas>,
	update_active: Mutex<bool>,
	active_updates: Arc<AtomicUsize>,
	buf0: Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
	buf1: Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
	buf0_force_update: bool,
	buf1_force_update: bool,
	buf0_ver: Instant,
	buf1_ver: Instant,
	buf0_freespace: Vec<(usize, usize)>,
	buf1_freespace: Vec<(usize, usize)>,
	buf0_bin_buf_data: BTreeMap<u64, Vec<BinBufferData>>,
	buf1_bin_buf_data: BTreeMap<u64, Vec<BinBufferData>>,
	cur_buf: u8,
	buf0_draw: usize,
	buf1_draw: usize,
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
		
		let len = 10000000;
		
		let buf0 = unsafe {
			DeviceLocalBuffer::raw(
				engine.device(),
				len * ::std::mem::size_of::<ItfVertInfo>(),
				BufferUsage::all(),
				vec![engine.graphics_queue().family()]
			).unwrap()
		};
		
		let buf1 = unsafe {
			DeviceLocalBuffer::raw(
				engine.device(),
				len * ::std::mem::size_of::<ItfVertInfo>(),
				BufferUsage::all(),
				vec![engine.graphics_queue().family()]
			).unwrap()
		};			
		
		Interface {
			bin_i: 0,
			bin_map: bin_map,
			atlas: engine.atlas(),
			engine: engine,
			update_active: Mutex::new(false),
			active_updates: Arc::new(AtomicUsize::new(0)),
			buf0,
			buf1,
			buf0_force_update: true,
			buf1_force_update: true,
			buf0_ver: Instant::now(),
			buf1_ver: Instant::now(),
			buf0_freespace: vec![(0, len)],
			buf1_freespace: vec![(0, len)],
			buf0_bin_buf_data: BTreeMap::new(),
			buf1_bin_buf_data: BTreeMap::new(),
			buf0_draw: 0,
			buf1_draw: 0,
			cur_buf: 0,
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
		const VERT_SIZE: usize = ::std::mem::size_of::<ItfVertInfo>();
		let cmds = self.atlas.update(self.engine.device(), self.engine.graphics_queue());
		
		if *self.update_active.lock() {
			if self.active_updates.load(atomic::Ordering::Relaxed) == 0 {
				self.cur_buf = match self.cur_buf {
					0 => 1,
					1 => 0,
					_ => unreachable!()
				};
				
				*self.update_active.lock() = false;
			} else {
				return cmds;
			}
		}
		
		if resized {
			self.buf0_force_update = true;
			self.buf1_force_update = true;
		}
		
		let bins = self.bins_with_clean();
		
		let (
			buf,
			force_update,
			version,
			buf_draw,
			buf_freespace,
			buf_bin_buf_data
		) = match self.cur_buf {
			0 => (
				self.buf1.clone(),
				&mut self.buf1_force_update,
				&mut self.buf1_ver,
				&mut self.buf1_draw,
				&mut self.buf1_freespace,
				&mut self.buf1_bin_buf_data,
			), 1 => (
				self.buf0.clone(),
				&mut self.buf0_force_update,
				&mut self.buf0_ver,
				&mut self.buf0_draw,
				&mut self.buf0_freespace,
				&mut self.buf0_bin_buf_data,
			), _ => unreachable!()
		};
		
		for bin in &bins {
			bin.do_update([win_size[0] as f32, win_size[1] as f32], *force_update);
		}
		
		let update_inst = Instant::now();
		let mut update_bins = Vec::new();
		let bin_ids: Vec<u64> = bins.iter().map(|b| b.id()).collect();
		let rm_ids: Vec<u64> = buf_bin_buf_data.keys().cloned().filter(|k| !bin_ids.contains(&k)).collect();
		
		for bin in bins {
			if bin.last_update() > *version {
				update_bins.push(bin);
			}
		}
		
		if update_bins.is_empty() {
			return cmds;
		}
		
		let mut tmp_data = Vec::new();
		let mut regions = Vec::new();
		
		for bin_id in update_bins.iter().map(|b| b.id()).chain(rm_ids) {
			if let Some(bin_buf_datas) = buf_bin_buf_data.remove(&bin_id) {
				for BinBufferData { pos, len, .. } in bin_buf_datas {
					if tmp_data.len() < len {
						tmp_data.resize(len, ItfVertInfo::default());
					}
					
					regions.push((0 * VERT_SIZE, pos * VERT_SIZE, len * VERT_SIZE));
					buf_freespace.push((pos, len));
				}
			}
		}
		
		buf_freespace.sort_by_key(|k| k.0);
		let mut free_i = 0;
		let mut to_remove = Vec::new();
		
		loop {
			if free_i >= buf_freespace.len() - 1 {
				break;
			}
			
			let mut free_j = free_i + 1;
			let mut jump = 1;
			
			loop {
				if free_j >= buf_freespace.len() {
					break;
				}
				
				if buf_freespace[free_j].0 == buf_freespace[free_i].0 + buf_freespace[free_i].1 {
					buf_freespace[free_i].1 += buf_freespace[free_j].1;
					to_remove.push(free_j);
					free_j += 1;
					jump += 1;
					continue;
				}
				
				break;
			}
			
			free_i += jump;
		}
		
		for free_i in to_remove.into_iter().rev() {
			buf_freespace.swap_remove(free_i);
		}
		
		for bin in update_bins {
			for (mut verts, custom_img_op, atlas_i) in bin.verts_cp() {
				if custom_img_op.is_some() {
					println!("Custom Image!");
					continue;
				}
				
				let mut free_ops = Vec::new();
				
				for (free_i, (_, free_len)) in buf_freespace.iter().enumerate() {
					if *free_len >= verts.len() {
						free_ops.push((free_len-verts.len(), free_i));
					}
				}
				
				free_ops.sort_by_key(|k| k.0);
				
				if free_ops.is_empty() {
					panic!("Buffer out of freespace!");
				}
				
				let (pos, free_len) = buf_freespace.swap_remove(free_ops[0].1);
				
				if free_len > verts.len() {
					buf_freespace.push((pos + verts.len(), free_len - verts.len()));
				}
				
				let bin_buf_datas = buf_bin_buf_data.get_mut_or_else(&bin.id(), || { Vec::new() });
				
				bin_buf_datas.push(BinBufferData {
					atlas_i,
					pos,
					len: verts.len()
				});
				
				if pos+verts.len() > *buf_draw {
					*buf_draw = pos+verts.len();
				}
				
				regions.push((tmp_data.len() * VERT_SIZE, pos * VERT_SIZE, verts.len() * VERT_SIZE));
				tmp_data.append(&mut verts);
			}
		}
		
		if !tmp_data.is_empty() {
			*version = update_inst;
			*force_update = false;
			self.active_updates.store(1, atomic::Ordering::Relaxed);
			*self.update_active.lock() = true;
			
			let engine = self.engine.clone();
			let active_updates = self.active_updates.clone();
		
			::std::thread::spawn(move || {
				let mut cmd_buf = AutoCommandBufferBuilder::new(engine.device(), engine.transfer_queue_ref().family()).unwrap();
			
				let tmp_buf = CpuAccessibleBuffer::from_iter(
					engine.device(), BufferUsage::all(),
					tmp_data.into_iter()
				).unwrap();
				
				cmd_buf = cmd_buf.copy_buffer_with_regions(tmp_buf, buf, regions.into_iter()).unwrap();
				let cmd_buf = cmd_buf.build().unwrap();
				let fence = cmd_buf.execute(engine.transfer_queue()).unwrap().then_signal_fence_and_flush().unwrap();
				fence.wait(None).unwrap();
				active_updates.store(0, atomic::Ordering::Relaxed);
			});
		}
		
		cmds
	}
	
	pub(crate) fn draw_bufs(&mut self)
		-> Vec<(
			Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
			Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>,
			Arc<Sampler>,
			Option<(usize, usize)>,
		)>
	{
		let mut out = Vec::new();
		
		let(image, sampler) = match self.engine.atlas_ref().image_and_sampler(1) {
			Some(some) => some,
			None => (self.engine.atlas_ref().null_img(self.engine.transfer_queue()), Sampler::simple_repeat_linear_no_mipmap(self.engine.device()))
		};
		
		let (buf, buf_draw) = match self.cur_buf {
			0 => (self.buf0.clone(), self.buf0_draw),
			1 => (self.buf1.clone(), self.buf1_draw),
			_ => unreachable!()
		};
		
		out.push((
			buf,
			image,
			sampler,
			Some((0, buf_draw))
		));
		
		out
	}
}

