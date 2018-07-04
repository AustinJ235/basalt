use std::sync::{Arc,Weak};
use vulkano;
use std::collections::BTreeMap;
use Engine;
use super::bin::Bin;
use parking_lot::RwLock;
use super::super::atlas::Atlas;
use vulkano::sampler::Sampler;
use vulkano::buffer::DeviceLocalBuffer;
use interface::itf_dual_buf::ItfDualBuffer;

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
	dual_buffer: Arc<ItfDualBuffer>,
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
			bin_map: bin_map.clone(),
			atlas: engine.atlas(),
			dual_buffer: ItfDualBuffer::new(engine.clone(), bin_map),
			engine: engine,
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
	
	pub(crate) fn draw_bufs(&mut self, win_size: [u32; 2], resized: bool)
		-> Vec<(
			Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
			Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>,
			Arc<Sampler>,
			Option<(usize, usize)>,
		)>
	{
		self.dual_buffer.draw_data(win_size, resized)
	}
}

