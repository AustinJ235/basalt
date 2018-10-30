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
use std::cmp::Ordering;
use vulkano::buffer::BufferAccess;

const DEFAULT_BUFFER_LEN: usize = 78643; // ~3 MB
const MAX_BUFFER_LEN: usize = 13421773; // ~512 MB
const VERT_SIZE: usize = ::std::mem::size_of::<ItfVertInfo>();
const UPDATE_INTERVAL: u32 = 15;
const BUF_RESIZE_THRESHOLD: u64 = 3;
const UPDATE_BENCH: bool = false;

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Debug)]
struct ImageID {
	ty: u8,
	id: usize,
}

impl Ord for ImageID {
    fn cmp(&self, other: &ImageID) -> Ordering {
    	match self.ty.cmp(&other.ty) {
    		Ordering::Equal => self.id.cmp(&other.id),
    		Ordering::Less =>  Ordering::Less,
    		Ordering::Greater => Ordering::Greater
    	}
    }
}

struct BinData {
	pos: usize,
	len: usize,
}

struct ImageData {
	max_draw: usize,
	buffer: Arc<DeviceLocalBuffer<[ItfVertInfo]>>,
	buffer_len: usize,
	recreate_wanted: Option<Instant>,
	freespaces: Vec<(usize, usize)>,
	bin_data: BTreeMap<u64, Vec<BinData>>,
	custom_img: Option<Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>>,
}

impl ImageData {
	fn new(engine: &Arc<Engine>, custom_img: Option<Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>>) -> Self {
		let buffer = unsafe {
			DeviceLocalBuffer::raw(
				engine.device(),
				DEFAULT_BUFFER_LEN * VERT_SIZE,
				BufferUsage::all(),
				vec![engine.graphics_queue().family()]
			).unwrap()
		};
		
		ImageData {
			max_draw: 0,
			buffer,
			buffer_len: DEFAULT_BUFFER_LEN,
			recreate_wanted: None,
			freespaces: vec![(0, MAX_BUFFER_LEN)],
			bin_data: BTreeMap::new(),
			custom_img,
		}
	}
}

struct BufferData {
	force_update: bool,
	version: Instant,
	image_data: BTreeMap<ImageID, ImageData>,
}

impl BufferData {
	fn new() -> Self {
		BufferData {
			force_update: true,
			version: Instant::now(),
			image_data: BTreeMap::new(),
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
	basic_sampler: Arc<Sampler>,
}

impl ItfDualBuffer {
	pub fn new(engine: Arc<Engine>, bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		let itfdualbuffer = Arc::new(ItfDualBuffer {
			bin_map,
			resized: Mutex::new(true),
			win_size: Mutex::new([1920.0, 1080.0]),
			buffer0: Mutex::new(BufferData::new()),
			buffer1: Mutex::new(BufferData::new()),
			basic_sampler: Sampler::simple_repeat_linear_no_mipmap(engine.device()),
			engine,
			current: Mutex::new(0),
			updating: Mutex::new(false),
			updating_status: Arc::new(AtomicUsize::new(0)),
			wait_frame: Mutex::new(None),
		});
		
		let idb = itfdualbuffer.clone();
		
		::std::thread::spawn(move || {
			let mut last_inst = Instant::now();
			let mut avg_history = ::std::collections::VecDeque::new();
		
			'main_loop: loop {
				let elapsed = last_inst.elapsed();
				
				if elapsed.as_secs() == 0 {
					let millis = elapsed.subsec_millis();
					
					if millis < UPDATE_INTERVAL {
						::std::thread::sleep(::std::time::Duration::from_millis((UPDATE_INTERVAL-millis) as u64));
					} 
				}
				
				last_inst = Instant::now();
				
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
				
				{
					let start = Instant::now();
					let mut ordered = Vec::with_capacity(bins.len());
					let mut update_groups = Vec::new();
					
					for bin in &bins {
						if bin.parent().is_none() {
							ordered.push(bin.clone());
						}
					}
					
					if !ordered.is_empty() {		
						update_groups.push((0, ordered.len()));
						let mut group_i = 0;
						
						loop {
							let (start, end) = update_groups[group_i].clone();
							let mut to_add = Vec::new();
							
							for bin in &ordered[start..end] {
								to_add.append(&mut bin.children());
							}
							
							if to_add.is_empty() {
								break;
							}
							
							ordered.append(&mut to_add);
							update_groups.push((end, ordered.len()));
							group_i += 1;
						}
					}
					
					#[derive(Clone)]
					enum Job {
						Barrier(Arc<Barrier>),
						Bin(Arc<Bin>),
					}
					
					let thread_count: usize = 6;
					let mut thread_i = 0;
					let mut thread_jobs: Vec<Vec<Job>> = Vec::with_capacity(thread_count);
					thread_jobs.resize(thread_count, Vec::new());
					let update_groups_len = update_groups.len();
					let mut update_count = 0;

					for (i, (start, end)) in update_groups.into_iter().enumerate() {
						for bin in &ordered[start..end] {
							if !bin.wants_update() && !resized {
								continue;
							}
							
							thread_jobs[thread_i].push(Job::Bin(bin.clone()));
							thread_i += 1;
							update_count += 1;
							
							if thread_i >= thread_count {
								thread_i = 0;
							}
						}
						
						if i != update_groups_len - 1 {
							let barrier = Arc::new(Barrier::new(thread_count));
							
							for jobs in &mut thread_jobs {
								jobs.push(Job::Barrier(barrier.clone()));
							}
						}
					}
					
					let mut handles = Vec::new();
					let scale = idb.engine.interface_ref().scale();
					
					for jobs in thread_jobs {
						handles.push(::std::thread::spawn(move || {
								for job in jobs {
									match job {
										Job::Barrier(barrier) => { barrier.wait(); },
										Job::Bin(bin) => bin.do_update(win_size, scale)
									}
								}
						}));
					}
					
					for handle in handles {
						handle.join().unwrap();
					}
					
					if UPDATE_BENCH && update_count > 300 {
						let avg = (start.elapsed().subsec_micros() as f32 / 1000.0) / update_count as f32;
						avg_history.push_back(avg);
						
						if avg_history.len() > 1000 {
							avg_history.pop_front();
						}
						
						let print_avg: f32 = avg_history.iter().sum::<f32>() / avg_history.len() as f32;
						println!("{:.1} ms", print_avg * 352.0);
					}
				}
				
				let update_inst = Instant::now();
				
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
				
				let BufferData {
					force_update,
					version,
					image_data,
					..
				} = &mut *match *idb.current.lock() {
					0 => &idb.buffer1,
					1 => &idb.buffer0,
					_ => unreachable!()
				}.lock();
				
				let bin_ids: Vec<u64> = bins.iter().map(|b| b.id()).collect();
				let mut update_bins_ids = Vec::new();
				let mut update_verts = Vec::new();
			
				for bin in bins {
					if bin.last_update() > *version {
						update_bins_ids.push(bin.id());
						update_verts.push((bin.id(), bin.verts_cp()));
					}
				}
				
				for (bin_id, vert_data) in &update_verts {
					for (_, custom_img_op, atlas) in vert_data {
						let image_id = match custom_img_op.is_some() {
							true => ImageID { ty: 1, id: *bin_id as usize },
							false => ImageID { ty: 0, id: *atlas }
						};
						
						if !image_data.contains_key(&image_id) {
							image_data.insert(image_id, ImageData::new(&idb.engine, None));
						}
					}
				}
				
				enum CopySrc {
					Buffer(Arc<DeviceLocalBuffer<[ItfVertInfo]>>),
					Data(Vec<ItfVertInfo>),
				}
				
				let mut ready_for_cp: Vec<(CopySrc, Arc<DeviceLocalBuffer<[ItfVertInfo]>>, Vec<(usize, usize, usize)>)> = Vec::new();
				let mut remove_images = Vec::new();
				
				for (image_id, ImageData {
					buffer,
					buffer_len,
					recreate_wanted,
					freespaces,
					bin_data,
					max_draw,
					custom_img,
					..
				}) in image_data.iter_mut() {
					let rm_ids: Vec<u64> = bin_data.keys().cloned().filter(|k| !bin_ids.contains(&k)).collect();
					let mut tmp_data = Vec::new();
					let mut regions = Vec::new();
					
					for bin_id in update_bins_ids.iter().chain(rm_ids.iter()) {
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
					
					for (bin_id, vert_data) in &update_verts {
						for (verts, custom_img_op, atlas) in vert_data {
							match custom_img_op {
								&Some(ref img) => if image_id.ty != 1 || image_id.id != *bin_id as usize {
									continue;
								} else {
									*custom_img = Some(img.clone());
								}, None => if image_id.ty != 0 || image_id.id != *atlas {
									continue;
								}
							}
							
							let mut free_ops = Vec::new();
							
							for (free_i, (_, free_len)) in freespaces.iter().enumerate() {
								if *free_len >= verts.len() {
									free_ops.push((free_len-verts.len(), free_i));
								}
							}
							
							free_ops.sort_by_key(|k| k.0);
							
							if free_ops.is_empty() {
								println!("{:?} Buffer out of freespace!", image_id);
							}
							
							let (pos, free_len) = freespaces.swap_remove(free_ops[0].1);
							
							if free_len > verts.len() {
								freespaces.push((pos + verts.len(), free_len - verts.len()));
							}
							
							let datas = bin_data.get_mut_or_else(&bin_id, || { Vec::new() });
							
							datas.push(BinData {
								pos,
								len: verts.len()
							});
							
							regions.push((tmp_data.len() * VERT_SIZE, pos * VERT_SIZE, verts.len() * VERT_SIZE));
							tmp_data.append(&mut verts.clone());
						}
					}
					
					if bin_data.is_empty() && tmp_data.is_empty() {
						remove_images.push(*image_id);
						continue;
					}
					
					if !tmp_data.is_empty() {
						*max_draw = 0;
						//let mut used = 0;
					
						for (_, bin_data_v) in bin_data.iter() {
							for bin_data in bin_data_v { 
								let max = bin_data.pos + bin_data.len;
								//used += bin_data.len;
							
								if max > *max_draw {
									*max_draw = max;
								}
							}
						}
						
						//let frag_percent = (1.0 - (used as f32 / *max_draw as f32)) * 100.0;
						//let used_percent = (used as f64 / *buffer_len as f64) * 100.0;
						
						if let Some((reason, required)) = if *max_draw > *buffer_len {
							Some((format!("too small"), true))
						} /*else if frag_percent > 30.0 {
							Some((format!("{:.1}% fragmented", frag_percent), false))
						} else if used_percent < 50.0 {
							Some((format!("is only {:.1}% used", used_percent), false))
						}*/ else {
							None
						} {
							if required || match recreate_wanted {
								Some(some) => if some.elapsed().as_secs() >= BUF_RESIZE_THRESHOLD {
									true
								} else {
									false
								}, None => {
									*recreate_wanted = Some(Instant::now());
									false
								}
							} {
								let new_len = f64::floor(*max_draw as f64 * 1.5) as usize;
								println!("Recreating {:?} because the buffer is {}. Old size {} bytes, new size {} bytes",
									image_id, reason, *buffer_len * VERT_SIZE, new_len * VERT_SIZE);
								
								let new_buffer = unsafe {
									DeviceLocalBuffer::raw(
										idb.engine.device(),
										new_len * VERT_SIZE,
										BufferUsage::all(),
										vec![idb.engine.graphics_queue().family()]
									).unwrap()
								};
								
								ready_for_cp.push((CopySrc::Buffer(buffer.clone()), new_buffer.clone(), vec![(0, 0, *buffer_len * VERT_SIZE)]));
								*buffer = new_buffer;
								*buffer_len = new_len;
								*recreate_wanted = None;
							}
						} else {
							*recreate_wanted = None;
						}
						
						ready_for_cp.push((CopySrc::Data(tmp_data), buffer.clone(), regions));
					}
				}
				
				for image_id in remove_images {
					image_data.remove(&image_id);
				}
				
				if !ready_for_cp.is_empty() {
					*version = update_inst;
					*force_update = false;
					idb.updating_status.store(1, atomic::Ordering::Relaxed);
					*idb.updating.lock() = true;
					let engine = idb.engine.clone();
					let updating_status = idb.updating_status.clone();
						
					::std::thread::spawn(move || {
						let mut cmd_buf = AutoCommandBufferBuilder::new(engine.device(), engine.transfer_queue_ref().family()).unwrap();
						
						for (src, dst_buf, regions) in ready_for_cp {
							match src {
								CopySrc::Buffer(src_buf) => {
									for (mut s, mut e, mut l) in regions {
										s /= VERT_SIZE;
										e /= VERT_SIZE;
										l /= VERT_SIZE;
										if l == 0 { continue; }
										cmd_buf = cmd_buf.copy_buffer(src_buf.clone().into_buffer_slice().slice(s..(s+l)).unwrap(), dst_buf.clone().into_buffer_slice().slice(e..(e+l)).unwrap()).unwrap();
									}
								}, CopySrc::Data(data) => {
									let src_buf = CpuAccessibleBuffer::from_iter(
										engine.device(), BufferUsage::all(),
										data.into_iter()
									).unwrap();
									
									for (mut s, mut e, mut l) in regions {
										s /= VERT_SIZE;
										e /= VERT_SIZE;
										l /= VERT_SIZE;
										if l == 0 { continue; }
										cmd_buf = cmd_buf.copy_buffer(src_buf.clone().into_buffer_slice().slice(s..(s+l)).unwrap(), dst_buf.clone().into_buffer_slice().slice(e..(e+l)).unwrap()).unwrap();
									}
								}
							}
						}
						
						let cmd_buf = cmd_buf.build().unwrap();
						let fence = cmd_buf.execute(engine.transfer_queue()).unwrap().then_signal_fence_and_flush().unwrap();
						fence.wait(None).unwrap();
						updating_status.store(0, atomic::Ordering::Relaxed);
					});
				}
			}
		});
		
		itfdualbuffer
	}
	
	pub(crate) fn draw_data(&self, win_size: [u32; 2], resized: bool) -> Vec<(
		vulkano::buffer::BufferSlice<[ItfVertInfo], Arc<DeviceLocalBuffer<[ItfVertInfo]>>>,
		Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>,
		Arc<Sampler>,
	)> {
		if resized {
			*self.win_size.lock() = [win_size[0] as f32, win_size[1] as f32];
			*self.resized.lock() = true;
		}
		
		if let Some((barrier1, barrier2)) = self.wait_frame.lock().take() {
			barrier1.wait();
			barrier2.wait();
		}
		
		let buffer = match *self.current.lock() {
			0 => &self.buffer0,
			1 => &self.buffer1,
			_ => unreachable!()
		}.lock();
		
		let mut out = Vec::new();
		
		for (image_id, ImageData {
			buffer,
			max_draw,
			custom_img,
			..
		}) in buffer.image_data.iter() {
			let (image, sampler) = match image_id.ty {
				0 => match self.engine.atlas_ref().image_and_sampler(image_id.id) {
					Some(some) => some,
					None => (self.engine.atlas_ref().null_img(self.engine.transfer_queue()), self.basic_sampler.clone())
				},
				
				1 => (match custom_img {
					&Some(ref img) => img.clone(),
					None => self.engine.atlas_ref().null_img(self.engine.transfer_queue())
				}, self.basic_sampler.clone()),
				
				_ => unreachable!()
			};
			
			out.push((
				buffer.clone().into_buffer_slice().slice(0..*max_draw).unwrap(),
				image,
				sampler,
			));
		}
		
		out
	}
}

