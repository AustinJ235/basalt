//! System for running things on an interval.

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
	/// Interval::start(ID) to start hook operation again.
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
	Start(IntvlHookID),
	Remove(IntvlHookID),
}

/// The main struct for the interval system.
///
/// Accessed via `basalt.interval_ref()` or `basalt.interval()`.
pub struct Interval {
	current_id: AtomicU64,
	event_send: Sender<IntvlEvent>,
}

impl Interval {
	pub(crate) fn new() -> Self {
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
						IntvlEvent::Start(id) =>
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

						let elapsed = if hook.last.is_none() {
							let elapsed = hook.last.take().map(|last| last.elapsed());
							hook.last = Some(Instant::now());
							elapsed
						} else if hook.last.as_ref().unwrap().elapsed() < hook.every {
							continue;
						} else {
							let elapsed = hook.last.take().map(|last| last.elapsed());
							hook.last = Some(Instant::now());
							elapsed
						};

						match (hook.method)(elapsed) {
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
				thread::sleep(Duration::from_millis(1));
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
	/// Takes a `Fn(last_call: Option<Duration>) -> IntvlHookCtrl`.
	/// - `last_call`: Duration since the last method was called.
	/// - `delay`: is the duration that has to elapsed after `Interval::start(...)` before
	/// the hook method is called.
	/// - `IntvlHookCtrl`: controls how the hook is handled after the method is called.
	///
	/// # Notes
	/// - Hooks are paused to begin with. They must be started with `Interval::start(...)`.
	/// - `last_call` will only be `Some` if the method is called continuously. Returning
	/// `InputHookCtrl::Pause` or using `Interval::pause(...)` will cause the next call to
	/// be `None`.
	pub fn do_every<F: FnMut(Option<Duration>) -> IntvlHookCtrl + Send + 'static>(
		&self,
		every: Duration,
		delay: Option<Duration>,
		method: F,
	) -> IntvlHookID {
		self.add_hook(IntvlHook {
			every,
			last: None,
			delay,
			delay_start: None,
			paused: true,
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

	/// Start a hook.
	///
	/// # Notes
	/// - If hook doesn't exist this does nothing.
	pub fn start(&self, id: IntvlHookID) {
		self.event_send.send(IntvlEvent::Start(id)).unwrap();
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
