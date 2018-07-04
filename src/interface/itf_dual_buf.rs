use interface::interface::ItfVertInfo;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferUsage;
use std::sync::{Arc,Weak};
use vulkano;
use std::collections::BTreeMap;
use Engine;
use super::bin::Bin;
use parking_lot::RwLock;
use vulkano::sampler::Sampler;
use misc::BTreeMapExtras;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::buffer::DeviceLocalBuffer;
use parking_lot::Mutex;
use std::time::Instant;
use std::sync::atomic::{self,AtomicUsize};
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;
use std::sync::Barrier;

const DEFAULT_BUFFER_LEN: usize = 10000000;
const VERT_SIZE: usize = ::std::mem::size_of::<ItfVertInfo>();
const UPDATE_INTERVAL: u32 = 10;

struct BinData {
	atlas: usize,
	pos: usize,
	len: usize,
}

struct BufferData {
	buffer: Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
	force_update: bool,
	version: Instant,
	freespaces: Vec<(usize, usize)>,
	max_allocated: usize,
	bin_data: BTreeMap<u64, Vec<BinData>>,
}

impl BufferData {
	fn new(engine: &Arc<Engine>) -> Self {
		let buffer = unsafe {
			DeviceLocalBuffer::raw(
				engine.device(),
				DEFAULT_BUFFER_LEN * VERT_SIZE,
				BufferUsage::all(),
				vec![engine.graphics_queue().family()]
			).unwrap()
		};
		
		BufferData {
			buffer,
			force_update: true,
			version: Instant::now(),
			freespaces: vec![(0, DEFAULT_BUFFER_LEN)],
			max_allocated: 0,
			bin_data: BTreeMap::new(),
		}
	}
}

pub struct ItfDualBuffer {
	engine: Arc<Engine>,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	resized: Mutex<bool>,
	win_size: Mutex<[f32; 2]>,
	buffer0: Mutex<BufferData>,
	buffer1: Mutex<BufferData>,
	current: Mutex<u8>,
	updating: Mutex<bool>,
	updating_status: Arc<AtomicUsize>,
	wait_frame: Mutex<Option<(Arc<Barrier>, Arc<Barrier>)>>,
}

impl ItfDualBuffer {
	pub fn new(engine: Arc<Engine>, bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		let itfdualbuffer = Arc::new(ItfDualBuffer {
			bin_map,
			resized: Mutex::new(true),
			win_size: Mutex::new([1920.0, 1080.0]),
			buffer0: Mutex::new(BufferData::new(&engine)),
			buffer1: Mutex::new(BufferData::new(&engine)),
			engine,
			current: Mutex::new(0),
			updating: Mutex::new(false),
			updating_status: Arc::new(AtomicUsize::new(0)),
			wait_frame: Mutex::new(None),
		});
		
		let idb = itfdualbuffer.clone();
		
		::std::thread::spawn(move || {
			let mut last_inst = Instant::now();
		
			loop {
				let elapsed = last_inst.elapsed();
				
				if elapsed.as_secs() == 0 {
					let millis = elapsed.subsec_millis();
					
					if millis < UPDATE_INTERVAL {
						::std::thread::sleep(::std::time::Duration::from_millis((UPDATE_INTERVAL-millis) as u64));
					} 
				}
				
				last_inst = Instant::now();
			
				// -- Get Bins ----------------------------- //
				
				let bins = {
					let mut bin_map = idb.bin_map.write();
					let mut rm_ids = Vec::new();
					let mut bins = Vec::with_capacity(bin_map.len());
					
					for (bin_id, bin_wk) in &*bin_map {
						match bin_wk.upgrade() {
							Some(bin) => bins.push(bin),
							None => rm_ids.push(*bin_id)
						}
					}
					
					for id in rm_ids {
						bin_map.remove(&id);
					}
					
					bins
				};
				
				// -- Update Bins -------------------------- //
				
				let resized = {
					let mut resized = idb.resized.lock();
					
					if *resized {
						*resized = false;
						true
					} else {
						false
					}
				};
				
				let win_size = idb.win_size.lock().clone();
				
				for bin in &bins {
					bin.do_update(win_size, resized);
				}
				
				let update_inst = Instant::now();
				
				// -- Swap Buffers ------------------------- //
				
				if *idb.updating.lock() {
					if idb.updating_status.load(atomic::Ordering::Relaxed) == 0 {
						*idb.updating.lock() = false;
						let barrier1 = Arc::new(Barrier::new(2));
						let barrier2 = Arc::new(Barrier::new(2));
						*idb.wait_frame.lock() = Some((barrier1.clone(), barrier2.clone()));
						
						barrier1.wait();
						
						let mut current = idb.current.lock();
						*current = match *current {
							0 => 1,
							1 => 0,
							_ => unreachable!()
						};
						
						barrier2.wait();	
					} else {
						continue;
					}
				}
				
				// -- Buffer Update ------------------------ //
				
				let BufferData {
					buffer,
					force_update,
					version,
					freespaces,
					max_allocated,
					bin_data
				} = &mut *match *idb.current.lock() {
					0 => &idb.buffer1,
					1 => &idb.buffer0,
					_ => unreachable!()
				}.lock();
				
				let bin_ids: Vec<u64> = bins.iter().map(|b| b.id()).collect();
				let rm_ids: Vec<u64> = bin_data.keys().cloned().filter(|k| !bin_ids.contains(&k)).collect();
				let mut update_bins_ids = Vec::new();
				let mut update_bins = Vec::new();
			
				for bin in bins {
					if bin.last_update() > *version {
						update_bins_ids.push(bin.id());
						update_bins.push(bin);
					}
				}
				
				if !update_bins.is_empty() {
					let mut tmp_data = Vec::new();
					let mut regions = Vec::new();
					
					for bin_id in update_bins_ids.into_iter().chain(rm_ids) {
						if let Some(datas) = bin_data.remove(&bin_id) {
							for BinData { pos, len, .. } in datas {
								if tmp_data.len() < len {
									tmp_data.resize(len, ItfVertInfo::default());
								}
								
								regions.push((0 * VERT_SIZE, pos * VERT_SIZE, len * VERT_SIZE));
								freespaces.push((pos, len));
							}
						}
					}
					
					freespaces.sort_by_key(|k| k.0);
					let mut free_i = 0;
					let mut rm_free = Vec::new();
					
					loop {
						if free_i >= freespaces.len() - 1 {
							break;
						}
						
						let mut free_j = free_i + 1;
						let mut jump = 1;
						
						loop {
							if free_j >= freespaces.len() {
								break;
							}
							
							if freespaces[free_j].0 == freespaces[free_i].0 + freespaces[free_i].1 {
								freespaces[free_i].1 += freespaces[free_j].1;
								rm_free.push(free_j);
								free_j += 1;
								jump += 1;
								continue;
							}
							
							break;
						}
						
						free_i += jump;
					}
					
					for free_i in rm_free.into_iter().rev() {
						freespaces.swap_remove(free_i);
					}
					
					for bin in update_bins {
						for (mut verts, custom_img_op, atlas) in bin.verts_cp() {
							if custom_img_op.is_some() {
								println!("Custom Image!");
								continue;
							}
							
							let mut free_ops = Vec::new();
							
							for (free_i, (_, free_len)) in freespaces.iter().enumerate() {
								if *free_len >= verts.len() {
									free_ops.push((free_len-verts.len(), free_i));
								}
							}
							
							free_ops.sort_by_key(|k| k.0);
							
							if free_ops.is_empty() {
								panic!("Buffer out of freespace!");
							}
							
							let (pos, free_len) = freespaces.swap_remove(free_ops[0].1);
							
							if free_len > verts.len() {
								freespaces.push((pos + verts.len(), free_len - verts.len()));
							}
							
							let datas = bin_data.get_mut_or_else(&bin.id(), || { Vec::new() });
							
							datas.push(BinData {
								atlas,
								pos,
								len: verts.len()
							});
							
							if pos+verts.len() > *max_allocated {
								*max_allocated = pos+verts.len();
							}
							
							regions.push((tmp_data.len() * VERT_SIZE, pos * VERT_SIZE, verts.len() * VERT_SIZE));
							tmp_data.append(&mut verts);
						}
					}
					
					if !tmp_data.is_empty() {
						*version = update_inst;
						*force_update = false;
						idb.updating_status.store(1, atomic::Ordering::Relaxed);
						*idb.updating.lock() = true;
						let buffer = buffer.clone();
						let engine = idb.engine.clone();
						let updating_status = idb.updating_status.clone();
						
						::std::thread::spawn(move || {
							let mut cmd_buf = AutoCommandBufferBuilder::new(engine.device(), engine.transfer_queue_ref().family()).unwrap();
							let tmp_buf = CpuAccessibleBuffer::from_iter(
								engine.device(), BufferUsage::all(),
								tmp_data.into_iter()
							).unwrap();
							
							cmd_buf = cmd_buf.copy_buffer_with_regions(tmp_buf, buffer, regions.into_iter()).unwrap();
							let cmd_buf = cmd_buf.build().unwrap();
							let fence = cmd_buf.execute(engine.transfer_queue()).unwrap().then_signal_fence_and_flush().unwrap();
							fence.wait(None).unwrap();
							updating_status.store(0, atomic::Ordering::Relaxed);
						});
					}
				}
			}
		});
		
		itfdualbuffer
	}
	
	pub(crate) fn draw_data(&self, win_size: [u32; 2], resized: bool) -> Vec<(
		Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
		Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>,
		Arc<Sampler>,
		Option<(usize, usize)>,
	)> {
		if resized {
			*self.win_size.lock() = [win_size[0] as f32, win_size[1] as f32];
			*self.resized.lock() = true;
		}
		
		if let Some((barrier1, barrier2)) = self.wait_frame.lock().take() {
			barrier1.wait();
			barrier2.wait();
		}
	
		let(image, sampler) = match self.engine.atlas_ref().image_and_sampler(1) {
			Some(some) => some,
			None => (self.engine.atlas_ref().null_img(self.engine.transfer_queue()), Sampler::simple_repeat_linear_no_mipmap(self.engine.device()))
		};
	
		let buffer = match *self.current.lock() {
			0 => &self.buffer0,
			1 => &self.buffer1,
			_ => unreachable!()
		}.lock();
		
		let buf = buffer.buffer.clone();
		let draw_max = buffer.max_allocated;
		
		vec![(
			buf,
			image,
			sampler,
			Some((0, draw_max))
		)]
	}
}

