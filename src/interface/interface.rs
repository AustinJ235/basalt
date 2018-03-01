use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferUsage;
use std::sync::{Arc,Weak};
use vulkano::device::{self,Device};
use vulkano;
use std::collections::BTreeMap;
use Engine;
use super::bin::Bin;
use parking_lot::RwLock;
use super::super::atlas::Atlas;
use vulkano::sampler::Sampler;
use misc::BTreeMapExtras;

pub struct Interface {
	bin_i: u64,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	engine: Arc<Engine>,
	atlas: Arc<Atlas>,
}

impl_vertex!(ItfVertInfo, position, coords, color, ty);
#[derive(Clone)]
pub(crate) struct ItfVertInfo {
	pub position: (f32, f32, f32),
	pub coords: (f32, f32),
	pub color: (f32, f32, f32, f32),
	pub ty: i32
}

pub(crate) fn scale_verts(win_size: &[f32; 2], verts: &mut Vec<ItfVertInfo>) {
	for vert in verts {
		vert.position.0 += win_size[0] / -2.0;
		vert.position.0 /= win_size[0] / 2.0;
		vert.position.1 += win_size[1] / -2.0;
		vert.position.1 /= win_size[1] / 2.0;
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

	pub(crate) fn draw_bufs(&mut self, device: Arc<Device>, queue: Arc<device::Queue>, win_size: [usize; 2], resized: bool)
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
	}
}

