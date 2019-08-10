use std::collections::BTreeMap;
use std::sync::Arc;
use vulkano::buffer::DeviceLocalBuffer;
use std::time::Instant;
use vulkano::image::traits::ImageViewAccess;
use interface::interface::ItfVertInfo;
use std::sync::atomic::AtomicBool;
use interface::bin::Bin;
use std::sync::Weak;
use parking_lot::RwLock;

pub type BinID = u64;

pub const CELL_WIDTH: usize = 20;
pub const ROW_LENGTH: usize = 100;

pub struct DrawOnUpdate {
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	buffers: Vec<BufferData>,
}

pub struct BufferData {
	index: usize,
	version: Instant,
	in_use: Vec<Arc<AtomicBool>>,
	cell_width: usize,
	row_length: usize,
	max_rows: usize,
	own_by_bin: BTreeMap<BinID, (Vec<(usize, usize)>, Instant, Option<Arc<dyn ImageViewAccess + Send + Sync>>)>,
	own_by_pos: BTreeMap<(usize, usize), (BinID, Instant, Option<Arc<dyn ImageViewAccess + Send + Sync>>)>,
	inner: Option<Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
}

impl BufferData {
	pub fn new(index: usize) -> BufferData {
		BufferData {
			index,
			version: Instant::now(),
			in_use: Vec::new(),
			cell_width: 32,
			row_length: 64,
			max_rows: 8192,
			own_by_bin: BTreeMap::new(),
			own_by_pos: BTreeMap::new(),
			inner: None
		}
	}
	
	pub fn update(&self, bins: Vec<Arc<Bin>>) {
		let mut update = Vec::new();
		let mut create = Vec::new();
		let mut delete = Vec::new();
		let mut ids = Vec::new();
	
		for bin in bins {
			ids.push(bin.id());
			
			match self.version_of_bin(*ids.last().unwrap()) {
				Some(some) => if bin.last_update() > some {
					update.push(bin);
				} else {
					()
				}, None => create.push(bin)
			}
		}
		
		for key in self.own_by_bin.keys() {
			if !ids.contains(key) {
				delete.push(key);
			}
		}
		
		
		
		
					
			
		
	}
	
	pub fn version_of_bin(&self, id: BinID) -> Option<Instant> {
		match self.own_by_bin.get(&id) {
			Some((_, version, _)) => Some(version.clone()),
			None => None
		}
	}
}


