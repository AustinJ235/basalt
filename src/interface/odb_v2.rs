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
use atlas::{self,AtlasImageID};
use std::collections::HashMap;
use crossbeam::sync::{Parker,Unparker};

const VERT_SIZE: usize = ::std::mem::size_of::<ItfVertInfo>();

pub struct OrderedDualBuffer {
	basalt: Arc<Basalt>,
}

pub struct OrderedBuffer {
	basalt: Arc<Basalt>,
	bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	contains: BTreeMap<u64, BinState>,
	devbuf: Option<Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
	draw: Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		atlas::AtlasImageID, Option<Arc<dyn ImageViewAccess + Send + Sync>>
	)>,
}

#[derive(Clone)]
pub struct BinState {
	version: Instant,
	chunks: Vec<BufferChunk>,
}

#[derive(Clone)]
pub struct BufferChunk {
	index: usize,
	len: usize,
	z: R32,
	data: Option<Vec<ItfVertInfo>>,
	image_op: Option<Arc<dyn ImageViewAccess + Send + Sync>>,
	atlas_id: u64,
	image_key: String,
}

impl OrderedBuffer {
	fn new(basalt: Arc<Basalt>, bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Self {
		OrderedBuffer {
			bins,
			basalt,
			contains: BTreeMap::new(),
			devbuf: None,
			draw: Vec::new(),
		}
	}
	
	fn update(&mut self, force_all: bool) -> bool {
		// -- Create List of Alive Bins -------------------- //
		
		let mut alive_bins = BTreeMap::new();
		
		for bin_wk in self.bins.read().values() {
			if let Some(bin) = bin_wk.upgrade() {
				alive_bins.insert(bin.id(), bin);
			}
		}
		
		// -- Create List of Dead Bins --------------------- //
		
		let mut dead_bin_ids = Vec::new();
		
		for bin_id in self.contains.keys() {
			if !alive_bins.contains_key(bin_id) {
				dead_bin_ids.push(*bin_id);
			}
		}
		
		// -- Create List of New Bins ---------------------- //
		
		let mut new_bin_ids = Vec::new();
		
		for bin_id in alive_bins.keys() {
			if !self.contains.contains_key(bin_id) {
				new_bin_ids.push(*bin_id);
			}
		}
		
		// -- Create List of bins that want an update ------ //
		
		let mut bin_ids_want_up = Vec::new();
		bin_ids_want_up.append(&mut new_bin_ids);
		
		for (bin_id, bin) in &alive_bins {
			if !new_bin_ids.contains(bin_id) {
				if force_all || bin.last_update() != self.contains.get(bin_id).unwrap().version {
					bin_ids_want_up.push(*bin_id);
				}
			}
		}
		
		if bin_ids_want_up.is_empty() {
			return false;
		}
		
		// -- Create list of bin states to preserve -------- //
		
		let mut preserve_states = Vec::new();
		
		for (bin_id, bin_state) in &self.contains {
			if !bin_ids_want_up.contains(bin_id) && !dead_bin_ids.contains(bin_id) {
				preserve_states.push((*bin_id, bin_state.clone()));
			}
		}
		
		// -- Create list of new bin states ---------------- //
		
		let mut new_states = Vec::new();
		
		for bin_id in &bin_ids_want_up {
			let mut sorted: BTreeMap<R32, HashMap<String, (String, Option<Arc<ImageViewAccess + Send + Sync>>, AtlasImageID, Vec<ItfVertInfo>)>> = BTreeMap::new();
			let bin = alive_bins.get(bin_id).unwrap().clone();
			let version = bin.last_update();
			
			for (verts, image_op, atlas_id) in bin.verts_cp() {
				let image_key = match image_op.as_ref() {
					Some(image) => format!("{:p}", Arc::into_raw(image.clone())),
					None => format!("atlas_{}", atlas_id)
				};
			
				for vert in verts {
					sorted
						.entry(R32::from(vert.position.2)).or_insert_with(|| HashMap::new())
						.entry(image_key.clone()).or_insert_with(|| (image_key.clone(), image_op.clone(), atlas_id, Vec::new()))
						.3.push(vert);
				}
			}
			
			let mut chunks = Vec::new();
			
			for (z, img_map) in sorted {
				for (_, (image_key, image_op, atlas_id, data)) in img_map {
					chunks.push(BufferChunk {
						index: 0,
						len: 0,
						z,
						data: Some(data),
						image_op,
						atlas_id,
						image_key,
					});
				}
			}
			
			new_states.push((bin_id, BinState {
				version,
				chunks
			}));
		}
		
		// -- Update contains with new states -------------- //
		
		for (bin_id, state) in new_states {
			self.contains.insert(*bin_id, state);
		}
		
		// -- Create sorted list of chunks ----------------- //
		
		let mut sorted: BTreeMap<R32, HashMap<String, Vec<&mut BufferChunk>>> = BTreeMap::new();
		
		for state in self.contains.values_mut() {
			for chunk in &mut state.chunks {
				sorted
					.entry(chunk.z).or_insert_with(|| HashMap::new())
					.entry(chunk.image_key.clone()).or_insert_with(|| Vec::new())
					.push(chunk);
			}
		}
		
		// -- Create transfer commands --------------------- //
		
		let mut upload_regions: Vec<(usize, usize, usize)> = Vec::new();
		let mut copy_regions: Vec<(usize, usize, usize)> = Vec::new();
		let mut upload_data = Vec::new();
		let mut device_len = 0;
		
		for (_, image_mapped) in &mut sorted {
			for (_, chunks) in &mut *image_mapped {
				for &mut &mut ref mut chunk in chunks.iter_mut() {
					if let Some(mut data) = chunk.data.take() {
						let ui = upload_data.len();
						let len = data.len();
						let di = device_len;
						upload_regions.push((ui, di, len));
						device_len += len;
						upload_data.append(&mut data);
						chunk.index = di;
						chunk.len = len;
					} else if chunk.len != 0 {
						copy_regions.push((chunk.index, device_len, chunk.len));
						chunk.index = device_len;
						device_len += chunk.len;
					}
				}
			}
		}
		
		// -- Transfer data -------------------------------- //
		
		let local_buf = CpuAccessibleBuffer::from_iter(
			self.basalt.device(),
			BufferUsage {
				transfer_source: true,
				.. BufferUsage::none()
			},
			upload_data.into_iter()
		).unwrap();
		
		let old_buf = self.devbuf.clone();
		let new_buf = unsafe {
			DeviceLocalBuffer::raw(
				self.basalt.device(),
				device_len * VERT_SIZE,
				BufferUsage {
					transfer_source: true,
					transfer_destination: true,
					vertex_buffer: true,
					.. BufferUsage::none()
				},
				vec![self.basalt.graphics_queue().family()]
			).unwrap()
		};
		
		let mut cmdbuf = AutoCommandBufferBuilder::new(
			self.basalt.device(),
			self.basalt.transfer_queue_ref().family()
		).unwrap();
		
		for (si, di, len) in upload_regions {
			cmdbuf = cmdbuf.copy_buffer(
				old_buf.as_ref().unwrap().clone().into_buffer_slice().slice(si..(si+len)).unwrap(),
				new_buf.clone().into_buffer_slice().slice(di..(di+len)).unwrap()
			).unwrap();
		}
		
		for (si, di, len) in copy_regions {
			cmdbuf = cmdbuf.copy_buffer(
				local_buf.clone().into_buffer_slice().slice(si..(si+len)).unwrap(),
				new_buf.clone().into_buffer_slice().slice(di..(di+len)).unwrap()
			).unwrap();
		}
		
		drop(cmdbuf
			.build().unwrap()
			.execute(self.basalt.transfer_queue()).unwrap()
			.then_signal_semaphore_and_flush().unwrap()
			.cleanup_finished());
		self.devbuf = new_buf;
		
		// -- Create draw list ----------------------------- //
		
		// TODO: This
		
		true
	}
}

impl OrderedDualBuffer {
	pub fn new(basalt: Arc<Basalt>, bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		unimplemented!()
	}
	
	pub(crate) fn unpark(&self) {
		unimplemented!()
	}
	
	pub(crate) fn draw_data(&self, win_size: [u32; 2], resize: bool, scale: f32) -> Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<dyn ImageViewAccess + Send + Sync>,
		Arc<Sampler>,
	)> {
		unimplemented!()
	}
}
