use crossbeam::channel::{self, Sender};
use std::collections::HashMap;
use std::sync::atomic::{self, AtomicU64};
use std::thread;
use std::time::{Duration, Instant};

/// An ID of a `Interval` hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntvlHookID(u64);

/// Controls what happens after the hook method is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IntvlHookCtrl {
	/// Continue the hook.
	#[default]
	Continue,
	/// Pause the hook.
	///
	/// Interval::resume(ID) to resume hook operation.
	Pause,
	/// Remove the hook.
	Remove,
}

struct IntvlHook {
	every: Duration,
	last: Option<Instant>,
	delay: Option<Duration>,
	delay_start: Option<Instant>,
	paused: bool,
	method: Box<dyn FnMut(Option<Duration>) -> IntvlHookCtrl + Send + 'static>,
}

enum IntvlEvent {
	Add(IntvlHookID, IntvlHook),
	Pause(IntvlHookID),
	Resume(IntvlHookID),
	Remove(IntvlHookID),
}

/// A system for running things on an interval.
pub struct Interval {
	current_id: AtomicU64,
	event_send: Sender<IntvlEvent>,
}

impl Interval {
	pub fn new() -> Self {
		let (event_send, event_recv) = channel::unbounded();

		let intvl = Self {
			current_id: AtomicU64::new(0),
			event_send,
		};

		thread::spawn(move || {
			let mut hooks: HashMap<IntvlHookID, IntvlHook> = HashMap::new();

			#[cfg(target_os = "windows")]
			unsafe {
				timeBeginPeriod(1);
			}

			loop {
				while let Ok(event) = event_recv.try_recv() {
					match event {
						IntvlEvent::Add(id, hook) => {
							hooks.insert(id, hook);
						},
						IntvlEvent::Remove(id) => {
							hooks.remove(&id);
						},
						IntvlEvent::Resume(id) =>
							if let Some(hook) = hooks.get_mut(&id) {
								hook.paused = false;
							},
						IntvlEvent::Pause(id) =>
							if let Some(hook) = hooks.get_mut(&id) {
								hook.paused = true;
								hook.last = None;
								hook.delay_start = None;
							},
					}
				}

				let mut remove_hooks = Vec::new();

				for (hook_id, hook) in hooks.iter_mut() {
					if !hook.paused {
						if let Some(delay) = &hook.delay {
							if hook.delay_start.is_none() {
								hook.delay_start = Some(Instant::now());
								continue;
							}

							if hook.delay_start.as_ref().unwrap().elapsed() < *delay {
								continue;
							}
						}

						if hook.last.is_none() {
							hook.last = Some(Instant::now());
						} else if hook.last.as_ref().unwrap().elapsed() < hook.every {
							continue;
						} else {
							hook.last = Some(Instant::now());
						}

						match (hook.method)(hook.last.as_ref().map(|last| last.elapsed())) {
							IntvlHookCtrl::Continue => (),
							IntvlHookCtrl::Pause => {
								hook.paused = true;
								hook.last = None;
								hook.delay_start = None;
							},
							IntvlHookCtrl::Remove => {
								remove_hooks.push(*hook_id);
							},
						}
					}
				}

				for hook_id in remove_hooks {
					hooks.remove(&hook_id);
				}

				// On Windows this will be 1.48 ms
				thread::sleep(Duration::from_micros(1000));
			}
		});

		intvl
	}

	fn add_hook(&self, hook: IntvlHook) -> IntvlHookID {
		let id = IntvlHookID(self.current_id.fetch_add(1, atomic::Ordering::SeqCst));
		self.event_send.send(IntvlEvent::Add(id, hook)).unwrap();
		id
	}

	/// Call the method at provided internval.
	///
	/// Method signature is `fn(last_call: Option<Duration>) -> IntvlHookCtrl`. `last_call` is
    /// the last time the hook was *continuously* called. This means this value will be `None`
    /// when the hook is first called or after resuming.
	pub fn do_every<F: FnMut(Option<Duration>) -> IntvlHookCtrl + Send + 'static>(
		&self,
		every: Duration,
		method: F,
	) -> IntvlHookID {
		self.add_hook(IntvlHook {
			every,
			last: None,
			delay: None,
			delay_start: None,
			paused: false,
			method: Box::new(method),
		})
	}

	/// Same as `do_every` but with a delay.
	///
	/// Delay is will be used after adding or resuming.
	pub fn do_every_delay<F: FnMut(Option<Duration>) -> IntvlHookCtrl + Send + 'static>(
		&self,
		every: Duration,
		delay: Duration,
		method: F,
	) -> IntvlHookID {
		self.add_hook(IntvlHook {
			every,
			last: None,
			delay: Some(delay),
			delay_start: None,
			paused: false,
			method: Box::new(method),
		})
	}

	/// Pause a hook.
	///
	/// # Notes
	/// - If hook doesn't exist this does nothing.
	pub fn pause(&self, id: IntvlHookID) {
		self.event_send.send(IntvlEvent::Pause(id)).unwrap();
	}

	/// Resume a hook.
	///
	/// # Notes
	/// - If hook doesn't exist this does nothing.
	pub fn resume(&self, id: IntvlHookID) {
		self.event_send.send(IntvlEvent::Resume(id)).unwrap();
	}

	/// Remove a hook.
	///
	/// # Notes
	/// - If hook doesn't exist this does nothing.
	pub fn remove(&self, id: IntvlHookID) {
		self.event_send.send(IntvlEvent::Remove(id)).unwrap();
	}
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
extern "stdcall" {
	fn timeBeginPeriod(uPeriod: u32) -> u32;
}
