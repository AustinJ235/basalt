use std::sync::{Arc,Weak};
use std::collections::BTreeMap;
use Basalt;
use super::bin::Bin;
use parking_lot::{Mutex,RwLock};
use interface::odb::OrderedDualBuffer;
use interface::hook::HookManager;

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

pub(crate) fn scale_verts(win_size: &[f32; 2], scale: f32, verts: &mut Vec<ItfVertInfo>) {
	for vert in verts {
		vert.position.0 *= scale;
		vert.position.1 *= scale;
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

pub(crate) enum ItfEvent {
	MSAAChanged,
	ScaleChanged,
}

pub struct Interface {
	basalt: Arc<Basalt>,
	bin_i: Mutex<u64>,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	scale: Mutex<f32>,
	msaa: Mutex<u32>,
	pub(crate) odb: Arc<OrderedDualBuffer>,
	pub(crate) itf_events: Mutex<Vec<ItfEvent>>,
	pub(crate) hook_manager: Arc<HookManager>,
}

impl Interface {
	pub(crate) fn scale(&self) -> f32 {
		*self.scale.lock()
	}

	pub(crate) fn set_scale(&self, to: f32) {
		*self.scale.lock() = to;
		self.itf_events.lock().push(ItfEvent::ScaleChanged);
	}
	
	pub fn msaa(&self) -> u32 {
		*self.msaa.lock()
	}
	
	pub fn set_msaa(&self, amt: u32) -> Result<(), String> {
		let amt = match amt {
			1 => 1,
			2 => 2,
			4 => 4,
			8 => 8,
			a => return Err(format!("Invalid MSAA amount {}X", a))
		};
		
		*self.msaa.lock() = amt;
		self.itf_events.lock().push(ItfEvent::MSAAChanged);
		Ok(())
	}
	
	pub fn increase_msaa(&self) {
		let mut msaa = self.msaa.lock();
		
		*msaa = match *msaa {
			1 => 2,
			2 => 4,
			4 => 8,
			8 => 8,
			_ => panic!("Invalid MSAA level set!")
		};
		
		self.itf_events.lock().push(ItfEvent::MSAAChanged);
	}
	
	pub fn decrease_msaa(&self) {
		let mut msaa = self.msaa.lock();
		
		*msaa = match *msaa {
			1 => 1,
			2 => 1,
			4 => 2,
			8 => 4,
			_ => panic!("Invalid MSAA level set!")
		};
		
		self.itf_events.lock().push(ItfEvent::MSAAChanged);
	}
	
	pub(crate) fn new(basalt: Arc<Basalt>) -> Arc<Self> {
		let bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>> = Arc::new(RwLock::new(BTreeMap::new()));
		
		Arc::new(Interface {
			odb: OrderedDualBuffer::new(basalt.clone(), bin_map.clone()),
			bin_i: Mutex::new(0),
			bin_map: bin_map,
			scale: Mutex::new(1.0),
			msaa: Mutex::new(4),
			itf_events: Mutex::new(Vec::new()),
			hook_manager: HookManager::new(basalt.clone()),
			basalt,
		})
	}
	
	pub fn get_bin_id_atop(&self, mut x: f32, mut y: f32) -> Option<u64> {
		let scale = self.scale();
		x /= scale;
		y /= scale;
	
		let bins: Vec<Arc<Bin>> = self.bin_map.read().iter().filter_map(|(_, b)| b.upgrade()).collect();
		let mut inside = Vec::new();
		
		for bin in bins {
			if bin.mouse_inside(x, y) {
				if !bin.style_copy().pass_events.unwrap_or(false) {
					let z = bin.post_update().z_index;
					inside.push((z, bin));
				}
			}
		}
		
		inside.sort_by_key(|&(z, _)| z);
		inside.pop().map(|v| v.1.id())
	}
	
	pub fn get_bin_atop(&self, mut x: f32, mut y: f32) -> Option<Arc<Bin>> {
		let scale = self.scale();
		x /= scale;
		y /= scale;
		
		let bins: Vec<Arc<Bin>> = self.bin_map.read().iter().filter_map(|(_, b)| b.upgrade()).collect();
		let mut inside = Vec::new();
		
		for bin in bins {
			if bin.mouse_inside(x, y) {
				if !bin.style_copy().pass_events.unwrap_or(false) {
					let z = bin.post_update().z_index;
					inside.push((z, bin));
				}
			}
		}
		
		inside.sort_by_key(|&(z, _)| z);
		inside.pop().map(|v| v.1)
	}
	
	fn bins(&self) -> Vec<Arc<Bin>> {
		self.bin_map.read().iter().filter_map(|(_, b)| b.upgrade()).collect()
	}
	
	pub fn new_bins(&self, amt: usize) -> Vec<Arc<Bin>> {
		let mut out = Vec::with_capacity(amt);
		let mut bin_i = self.bin_i.lock();
		let mut bin_map = self.bin_map.write();
		
		for _ in 0..amt {
			let id = *bin_i;
			*bin_i += 1;
			let bin = Bin::new(id.clone(), self.basalt.clone());
			bin_map.insert(id, Arc::downgrade(&bin));
			out.push(bin);
		}
		
		out
	}
	
	pub fn new_bin(&self) -> Arc<Bin> {
		self.new_bins(1).pop().unwrap()
	}
	
	pub fn get_bin(&self, id: u64) -> Option<Arc<Bin>> {
		match self.bin_map.read().get(&id) {
			Some(some) => some.upgrade(),
			None => None
		}
	}
	
	pub fn mouse_inside(&self, mut mouse_x: f32, mut mouse_y: f32) -> bool {
		let scale = self.scale();
		mouse_x /= scale;
		mouse_y /= scale;
	
		for bin in self.bins() {
			if bin.mouse_inside(mouse_x, mouse_y) {
				return true;
			}
		} false
	}
}

