use crate::atlas::{self, AtlasImageID};
use crate::image_view::BstImageView;
use crate::interface::bin::Bin;
use crate::interface::interface::ItfVertInfo;
use crate::{Basalt, BstEvent, BstItfEv};
use crossbeam::queue::SegQueue;
use crossbeam::sync::{Parker, Unparker};
use ordered_float::OrderedFloat;
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::Instant;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::{BufferAccess, BufferSlice, BufferUsage, DeviceLocalBuffer};
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, CommandBufferUsage, PrimaryCommandBuffer,
};
use vulkano::sampler::Sampler;
use vulkano::sync::GpuFuture;

const VERT_SIZE: usize = ::std::mem::size_of::<ItfVertInfo>();

pub struct OrderedDualBuffer {
	active: Mutex<OrderedBuffer>,
	inactive: Mutex<OrderedBuffer>,
	parker: Mutex<Parker>,
	unparker: Unparker,
	switch_req: Mutex<bool>,
	switch_mu: Mutex<bool>,
	switch_cond: Condvar,
	force_up: AtomicBool,
	size_scale: Mutex<([u32; 2], f32)>,
}

impl OrderedDualBuffer {
	pub fn new(basalt: Arc<Basalt>, bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		let parker = Parker::new();
		let unparker = parker.unparker().clone();

		let ret = Arc::new(OrderedDualBuffer {
			active: Mutex::new(OrderedBuffer::new(basalt.clone(), bins.clone())),
			inactive: Mutex::new(OrderedBuffer::new(basalt.clone(), bins)),
			parker: Mutex::new(parker),
			unparker,
			switch_req: Mutex::new(false),
			switch_mu: Mutex::new(false),
			switch_cond: Condvar::new(),
			force_up: AtomicBool::new(true),
			size_scale: Mutex::new(([1920, 1080], basalt.options_ref().scale)),
		});

		let odb = ret.clone();

		thread::spawn(move || {
			loop {
				if odb.force_up.swap(false, atomic::Ordering::SeqCst) {
					let (win_size, scale) = odb.size_scale.lock().clone();
					let mut inactive = odb.inactive.lock();
					inactive.win_size = win_size;
					inactive.scale = scale;
					inactive.update(true);
					drop(inactive);
					*odb.switch_req.lock() = true;
					basalt.send_event(BstEvent::BstItfEv(BstItfEv::ODBUpdate));
					let mut switch_mu = odb.switch_mu.lock();

					while !*switch_mu {
						odb.switch_cond.wait(&mut switch_mu);
					}

					*switch_mu = false;
					drop(switch_mu);
					let mut inactive = odb.inactive.lock();
					inactive.win_size = win_size;
					inactive.scale = scale;
					inactive.update(true);
					drop(inactive);
					*odb.switch_req.lock() = true;
					basalt.send_event(BstEvent::BstItfEv(BstItfEv::ODBUpdate));
					let mut switch_mu = odb.switch_mu.lock();

					while !*switch_mu {
						odb.switch_cond.wait(&mut switch_mu);
					}

					*switch_mu = false;
				} else {
					let mut inactive = odb.inactive.lock();

					if inactive.update(false) {
						drop(inactive);
						*odb.switch_req.lock() = true;
						basalt.send_event(BstEvent::BstItfEv(BstItfEv::ODBUpdate));
						let mut switch_mu = odb.switch_mu.lock();

						while !*switch_mu {
							odb.switch_cond.wait(&mut switch_mu);
						}

						*switch_mu = false;
					}
				}

				odb.parker.lock().park();
			}
		});

		ret
	}

	pub(crate) fn unpark(&self) {
		self.unparker.unpark();
	}

	pub(crate) fn draw_data(
		&self,
		win_size: [u32; 2],
		resize: bool,
		scale: f32,
	) -> Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<BstImageView>,
		Arc<Sampler>,
	)> {
		if resize {
			*self.size_scale.lock() = (win_size, scale);
			self.force_up.store(true, atomic::Ordering::SeqCst);
			self.unpark();
		}

		let mut switch_req = self.switch_req.lock();

		if *switch_req {
			let mut inactive = self.inactive.lock();
			let mut active = self.active.lock();
			::std::mem::swap(&mut *inactive, &mut *active);
			*self.switch_mu.lock() = true;
			self.switch_cond.notify_one();
			*switch_req = false;
		}

		self.active.lock().draw_data()
	}
}

pub struct OrderedBuffer {
	basalt: Arc<Basalt>,
	bins: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	contains: BTreeMap<u64, BinState>,
	devbuf: Option<Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
	draw: Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		atlas::AtlasImageID,
		Option<Arc<BstImageView>>,
	)>,
	draw_data: Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<BstImageView>,
		Arc<Sampler>,
	)>,
	draw_version: Option<Instant>,
	win_size: [u32; 2],
	scale: f32,
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
	z: OrderedFloat<f32>,
	data: Option<Vec<ItfVertInfo>>,
	image_op: Option<Arc<BstImageView>>,
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
			draw_data: Vec::new(),
			draw_version: None,
			win_size: [1920, 1080],
			scale: 1.0,
		}
	}

	fn update_draw_data(&mut self, force_up: bool) {
		if let Some((version, image_views)) = self.basalt.atlas_ref().image_views() {
			if self.draw_version.is_some()
				&& *self.draw_version.as_ref().unwrap() == version
				&& !force_up
			{
				return;
			}

			self.draw_version = Some(version);
			self.draw_data = Vec::new();

			for (buf, atlas_id, image_op) in &self.draw {
				let image: Arc<BstImageView> = match atlas_id {
					&0 => self.basalt.atlas_ref().empty_image(),
					&::std::u64::MAX =>
						match image_op {
							&Some(ref some) => some.clone(),
							&None => self.basalt.atlas_ref().empty_image(),
						},
					img_id =>
						match image_views.get(img_id) {
							Some(some) => some.clone(),
							None => self.basalt.atlas_ref().empty_image(),
						},
				};

				let sampler = self.basalt.atlas_ref().default_sampler();
				self.draw_data.push((buf.clone(), image, sampler));
			}
		} else {
			self.draw_data = Vec::new();
			self.draw_version = None;
		}
	}

	fn draw_data(
		&mut self,
	) -> Vec<(
		BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<BstImageView>,
		Arc<Sampler>,
	)> {
		self.update_draw_data(false);
		self.draw_data.clone()
	}

	fn update(&mut self, force_all: bool) -> bool {
		// -- Create List of Alive Bins -------------------- //

		let mut alive_bins = BTreeMap::new();

		for bin_wk in self.bins.read().values() {
			if let Some(bin) = bin_wk.upgrade() {
				alive_bins.insert(bin.id(), bin);
			}
		}

		// -- Update bins ---------------------------------- //

		let win_size = [self.win_size[0] as f32, self.win_size[1] as f32];
		let scale = self.scale;
		let mut to_update = Vec::new();

		for (_, bin) in &alive_bins {
			if bin.wants_update() || force_all {
				to_update.push(bin.clone());
			}
		}

		if !to_update.is_empty() {
			let threads = crate::num_cpus::get();
			let queue = Arc::new(SegQueue::new());

			for bin in to_update {
				queue.push(bin);
			}

			let mut handles = Vec::new();

			for _ in 0..threads {
				let queue = queue.clone();

				handles.push(thread::spawn(move || {
					while let Some(bin) = queue.pop() {
						bin.do_update(win_size, scale);
					}
				}));
			}

			for handle in handles {
				handle.join().unwrap();
			}
		}

		// -- Create List of Dead Bins --------------------- //

		let mut dead_bin_ids = Vec::new();

		for bin_id in self.contains.keys() {
			if !alive_bins.contains_key(bin_id) {
				dead_bin_ids.push(*bin_id);
			}
		}

		for bin_id in &dead_bin_ids {
			self.contains.remove(bin_id);
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
				if force_all
					|| match self.contains.get(bin_id) {
						Some(state) => state.version != bin.last_update(),
						None => true,
					} {
					bin_ids_want_up.push(*bin_id);
				}
			}
		}

		if bin_ids_want_up.is_empty() && dead_bin_ids.is_empty() {
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
			let mut sorted: BTreeMap<
				OrderedFloat<f32>,
				HashMap<
					String,
					(String, Option<Arc<BstImageView>>, AtlasImageID, Vec<ItfVertInfo>),
				>,
			> = BTreeMap::new();
			let bin = alive_bins.get(bin_id).unwrap().clone();
			let version = bin.last_update();

			for (verts, image_op, atlas_id) in bin.verts_cp() {
				let image_key = match image_op.as_ref() {
					Some(image) => format!("{:p}", Arc::into_raw(image.clone())),
					None => format!("atlas_{}", atlas_id),
				};

				for vert in verts {
					sorted
						.entry(OrderedFloat::from(vert.position.2))
						.or_insert_with(|| HashMap::new())
						.entry(image_key.clone())
						.or_insert_with(|| {
							(image_key.clone(), image_op.clone(), atlas_id, Vec::new())
						})
						.3
						.push(vert);
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
				chunks,
			}));
		}

		// -- Update contains with new states -------------- //

		for (bin_id, state) in new_states {
			self.contains.insert(*bin_id, state);
		}

		// -- Create sorted list of chunks ----------------- //

		let mut sorted: BTreeMap<OrderedFloat<f32>, HashMap<String, Vec<&mut BufferChunk>>> =
			BTreeMap::new();

		for state in self.contains.values_mut() {
			for chunk in &mut state.chunks {
				sorted
					.entry(chunk.z)
					.or_insert_with(|| HashMap::new())
					.entry(chunk.image_key.clone())
					.or_insert_with(|| Vec::new())
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
				..BufferUsage::none()
			},
			false,
			upload_data.into_iter(),
		)
		.unwrap();

		let old_buf = self.devbuf.clone();
		let new_buf = unsafe {
			DeviceLocalBuffer::raw(
				self.basalt.device(),
				device_len * VERT_SIZE,
				BufferUsage {
					transfer_source: true,
					transfer_destination: true,
					vertex_buffer: true,
					..BufferUsage::none()
				},
				vec![self.basalt.graphics_queue().family()],
			)
			.unwrap()
		};

		let mut cmdbuf = AutoCommandBufferBuilder::primary(
			self.basalt.device(),
			self.basalt.transfer_queue_ref().family(),
			CommandBufferUsage::OneTimeSubmit,
		)
		.unwrap();

		for (si, di, len) in copy_regions {
			cmdbuf
				.copy_buffer(
					old_buf
						.as_ref()
						.unwrap()
						.clone()
						.into_buffer_slice()
						.slice(si..(si + len))
						.unwrap(),
					new_buf.clone().into_buffer_slice().slice(di..(di + len)).unwrap(),
				)
				.unwrap();
		}

		for (si, di, len) in upload_regions {
			cmdbuf
				.copy_buffer(
					local_buf.clone().into_buffer_slice().slice(si..(si + len)).unwrap(),
					new_buf.clone().into_buffer_slice().slice(di..(di + len)).unwrap(),
				)
				.unwrap();
		}

		drop(
			cmdbuf
				.build()
				.unwrap()
				.execute(self.basalt.transfer_queue())
				.unwrap()
				.then_signal_semaphore_and_flush()
				.unwrap()
				.cleanup_finished(),
		);
		self.devbuf = Some(new_buf.clone());

		// -- Create draw list ----------------------------- //

		let mut draw: Vec<(
			BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
			atlas::AtlasImageID,
			Option<Arc<BstImageView>>,
		)> = Vec::new();

		for (_, image_mapped) in sorted.iter_mut().rev() {
			for (_, chunks) in &mut *image_mapped {
				let mut ranges = Vec::new();

				chunks.sort_by_key(|k| k.index);
				let mut start = None;
				let mut len = 0;

				for &mut &mut ref mut chunk in chunks.iter_mut() {
					if start.is_none() {
						start = Some((*chunk).index);
						len += chunk.len;
					} else if *start.as_ref().unwrap() + len == chunk.index {
						len += chunk.len;
					} else {
						ranges.push((start.take().unwrap(), len));
						len = 0;
					}
				}

				if let Some(start) = start {
					ranges.push((start, len));
				}

				for (start, len) in ranges {
					let first = chunks.first_mut().unwrap();
					let image_op = first.image_op.clone();
					let atlas_id = first.atlas_id;

					draw.push((
						new_buf
							.clone()
							.into_buffer_slice()
							.slice(start..(start + len))
							.unwrap(),
						atlas_id,
						image_op,
					));
				}
			}
		}

		self.draw = draw;
		self.update_draw_data(true);
		true
	}
}
