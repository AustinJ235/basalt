use std::sync::Arc;
use std::thread::{self,JoinHandle};
use std::sync::atomic::{self,AtomicBool,AtomicUsize};
use crossbeam::sync::{Parker,Unparker};
use crossbeam::queue::SegQueue;
use parking_lot::Mutex;
use super::Bin;
use std::collections::BTreeMap;

pub struct BinUpdateState {
	scale: f32,
	win_size: [f32; 2],
}

struct Job {
	parent: Option<Arc<Bin>>,
	preceding: Vec<Arc<Bin>>,
	dep_of: Option<Arc<Job>>,
	dep_total: usize,
	dep_done: AtomicUsize,
	bin: Arc<Bin>,
	state: BinUpdateState,
}

pub struct BinUpdatePool {
	num_workers: usize,
	exit: Arc<AtomicBool>,
	thread_handles: Vec<JoinHandle<()>>,
	queue: Arc<SegQueue<Arc<Job>>>,
	unparkers: Vec<Unparker>,
	threads_on_standby: Arc<AtomicUsize>,
	all_on_standby_parker: Mutex<Parker>,
}

impl Drop for BinUpdatePool {
	fn drop(&mut self) {
		self.exit.store(true, atomic::Ordering::SeqCst);
		self.unparkers.iter().for_each(|unparker| unparker.unpark());
		
		for handle in self.thread_handles.split_off(0) {
			handle.join().unwrap();
		}
	}
}

impl BinUpdatePool {
	pub fn wait_for_standby(&self) {
		let parker = self.all_on_standby_parker.lock();
	
		loop {
			if self.threads_on_standby.load(atomic::Ordering::SeqCst) != self.num_workers {
				parker.park();
			} else {
				break;
			}
		}
	}
	
	pub fn queue_update(
		&self,
		mut update: Vec<Arc<Bin>>,
		scale: f32,
		win_size: [f32; 2]
	) {
		// If a bin is floating and it is being updated all other children of the
		// bins parent need to updated also.
	
		let mut add_bins: Vec<Arc<Bin>> = Vec::new();
		
		for bin in update.iter() {
			if bin.is_floating() {
				if let Some(parent) = bin.parent() {
					for child in parent.children() {
						if update.iter().find(|b| b.id() == child.id()).is_none()
							&& add_bins.iter().find(|b| b.id() == child.id()).is_none()
						{
							add_bins.push(child);
						}
					}
				}
			}
		}
		
		update.append(&mut add_bins);
		
		// Map bins into jobs with parent and preceding set, deps will be handled later
	
		let mut jobs: BTreeMap<_, _> = update.into_iter().map(|bin| {
			let parent = bin.parent();
			let preceding = if bin.is_floating() {
				parent.as_ref().map(|parent| {
					let mut children = parent.children();
					let i = children.iter().position(|child| child.id() == bin.id()).unwrap();
					children.truncate(i+1);
					children
				}).unwrap_or(Vec::new())
			} else {
				Vec::new()
			};
			
			let dep_total = 0;
			let dep_done = AtomicUsize::new(0);
			let state = BinUpdateState {
				scale,
				win_size,
			};
			
			(bin.id(), Job {
				parent,
				preceding,
				dep_of: None,
				dep_total,
				dep_done,
				bin,
				state,
			})
		}).collect();
		
		// Find what what jobs need deps added and dep_of added
		
		let mut add_dep_to: Vec<(u64, u64)> = Vec::new();
		
		for (bin_id, job) in jobs.iter() {
			if let Some(parent) = job.parent.as_ref() {
				if jobs.contains_key(&parent.id()) {
					add_dep_to.push((*bin_id, parent.id()));
				}
			}
			
			for preceding in job.preceding.iter() {
				add_dep_to.push((*bin_id, preceding.id()));
			}
		}
		
		// Adjust dep_total's while jobs are mutable
		
		for &(_, ref dep_of_id) in add_dep_to.iter() {
			jobs.get_mut(dep_of_id).unwrap().dep_total += 1;
		}
		
		// Make jobs into Arcs so we can set dep_of
		
		let jobs: BTreeMap<_, _> = jobs.into_iter().map(|(bin_id, job)| (bin_id, Arc::new(job))).collect();
		
		// Now set the dep_of, this is extremly safe...
		
		for (dep_id, dep_of_id) in add_dep_to {
			let dep_of = jobs.get(&dep_of_id).unwrap().clone();
			let dep = jobs.get(&dep_id).unwrap().clone();
			unsafe { *::std::mem::transmute::<_, *mut _>(&dep.dep_of as *const _) = Some(dep_of); }
		}
		
		// Finally add the jobs to the queue
		
		for (_, job) in jobs.into_iter() {
			if job.dep_total == 0 {
				self.queue.push(job);
			}
		}
		
		self.unparkers.iter().for_each(|unparker| unparker.unpark());
	}

	pub fn new(num_workers: usize) -> Self {
		let exit = Arc::new(AtomicBool::new(false));
		let mut thread_handles = Vec::with_capacity(num_workers);
		let all_on_standby_parker = Parker::new();
		let all_on_standby_unparker = all_on_standby_parker.unparker().clone();
		let all_on_standby_parker = Mutex::new(all_on_standby_parker);
		let threads_on_standby = Arc::new(AtomicUsize::new(12));
		let queue: Arc<SegQueue<Arc<Job>>> = Arc::new(SegQueue::new());
		let mut parkers = Vec::with_capacity(num_workers);
		parkers.resize_with(num_workers, || Parker::new());
		
		let unparkers: Vec<_> = parkers
			.iter()
			.map(|parker| parker.unparker().clone())
			.collect();
		
		for (i, parker) in parkers.into_iter().enumerate() {
			let exit = exit.clone();
			let queue = queue.clone();
			let unparkers: Vec<_> = unparkers
				.clone()
				.into_iter()
				.enumerate()
				.filter_map(|(j, unparker)| {
					if j != i {
						Some(unparker)
					} else {
						None
					}
				})
				.collect();
			let threads_on_standby = threads_on_standby.clone();
			let all_on_standby_unparker = all_on_standby_unparker.clone();
			
			thread_handles.push(thread::spawn(move || {
				let mut no_work_available = true;
				
				loop {
					if exit.load(atomic::Ordering::SeqCst) {
						if !no_work_available {
							threads_on_standby.fetch_sub(1, atomic::Ordering::SeqCst);
						} return;
					}
					
					let mut first_iter = true;
					
					while let Ok(job) = queue.pop() {
						if first_iter {
							first_iter = false;
						}
						
						if no_work_available {
							no_work_available = false;
							threads_on_standby.fetch_sub(1, atomic::Ordering::SeqCst);
						}
						
						job.bin.do_update(job.state.win_size, job.state.scale);
						let mut added_to_queue = false;
						
						for glyph_bin in job.bin.update_text(job.state.scale) {
							added_to_queue = true;
							let state = BinUpdateState {
								scale: job.state.scale,
								win_size: job.state.win_size,
							};
						
							queue.push(Arc::new(Job {
								parent: Some(job.bin.clone()),
								preceding: Vec::new(),
								dep_of: None,
								dep_total: 0,
								dep_done: AtomicUsize::new(0),
								bin: glyph_bin,
								state
							}));
						}
						
						if let Some(dep_of) = job.dep_of.as_ref() {
							let dep_done = dep_of.dep_done.fetch_add(1, atomic::Ordering::SeqCst) + 1;
							
							if dep_done == dep_of.dep_total {
								queue.push(dep_of.clone());
								added_to_queue = true;
							}
						}
						
						if added_to_queue {
							unparkers.iter().for_each(|unparker| unparker.unpark());
						}
					}
					
					if !no_work_available {
						threads_on_standby.fetch_add(1, atomic::Ordering::SeqCst);
						no_work_available = true;
						all_on_standby_unparker.unpark();
					}
					
					parker.park();
				}
			}));
		}
		
		BinUpdatePool {
			num_workers,
			exit,
			thread_handles,
			queue,
			unparkers,
			threads_on_standby,
			all_on_standby_parker,
		}
	}
}
