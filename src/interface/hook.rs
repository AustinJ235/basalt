#![allow(warnings)]

use keyboard::{self,Qwery};
use mouse;
use std::time::Instant;
use std::time::Duration;
use std::sync::Arc;
use std::collections::BTreeMap;
use std::collections::HashMap;
use parking_lot::Mutex;
use interface::bin::Bin;
use parking_lot::RwLock;
use Engine;
use std::sync::Weak;
use crossbeam::queue::MsQueue;

pub type BinHookFn = Arc<Fn(Arc<Bin>, &BinHook) + Send + Sync>;

#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord,Hash)]
pub struct BinHookID(u64);

pub enum BinHook {
	Press {
		keys: Vec<Qwery>,
		mouse: Vec<mouse::Button>,
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<mouse::Button, bool>,
	},
	
	Hold {
		keys: Vec<Qwery>,
		mouse: Vec<mouse::Button>,
		first_call: Instant,
		last_call: Instant,
		is_first_call: bool,
		initial_delay: Duration,
		initial_delay_wait: bool,
		initial_delay_elapsed: bool,
		interval: Duration,
		accel: bool,
		accel_rate: f32,
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<mouse::Button, bool>,
	},
	
	Release {
		keys: Vec<Qwery>,
		mouse: Vec<mouse::Button>,
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<mouse::Button, bool>,
	},
	
	Character {
		char_ty: keyboard::CharType,
	},
	
	MouseEnter {
		mouse_x: f32,
		mouse_y: f32,
	},
	
	MouseLeave {
		mouse_x: f32,
		mouse_y: f32,
	},
	
	MouseMove {
		mouse_x: f32,
		mouse_y: f32,
		mouse_dx: f32,
		mouse_dy: f32,
	},
	
	MouseScroll {
		scroll_amt: f32,
	},
	
	Focused,
	LostFocus,
}

pub(crate) enum InputEvent {	
	MousePress(mouse::Button),
	MouseRelease(mouse::Button),
	KeyPress(Qwery),
	KeyRelease(Qwery),
	MousePosition(f32, f32),
	MouseDelta(f32, f32),
	Scroll(f32),
}

impl BinHook {
	fn is_active(&self) -> bool {
		match self {
			BinHook::Press {
				key_active,
				mouse_active,
				..
			} => {
				for (_, v) in key_active {
					if *v == false {
						return false;
					}
				}
				
				for (_, v) in mouse_active {
					if *v == false {
						return false;
					}
				}
			},
			
			BinHook::Release {
				key_active,
				mouse_active,
				..
			} => {
				for (_, v) in key_active {
					if *v == true {
						return false;
					}
				}
				
				for (_, v) in mouse_active {
					if *v == true {
						return false;
					}
				}
			},
			
			BinHook::Hold {
				key_active,
				mouse_active,
				..
			} => {
				for (_, v) in key_active {
					if *v == false {
						return false;
					}
				}
				
				for (_, v) in mouse_active {
					if *v == false {
						return false;
					}
				}
			},
			
			_ => ()
		}
		
		true
	}
}

pub(crate) struct HookManager {
	focused: Mutex<Option<u64>>,
	hooks: Mutex<BTreeMap<BinHookID, (Arc<Bin>, BinHook, BinHookFn)>>,
	current_id: Mutex<u64>,
	engine: Arc<Engine>,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	events: MsQueue<InputEvent>,
}

impl HookManager {
	pub fn send_event(&self, event: InputEvent) {
		self.events.push(event);
	}
	
	pub fn add_hook(&self, bin: Arc<Bin>, hook: BinHook, func: BinHookFn) -> BinHookID {
		let mut current_id = self.current_id.lock();
		let id = BinHookID(*current_id);
		*current_id += 1;
		drop(current_id);
		self.hooks.lock().insert(id, (bin, hook, func));
		id
	}

	pub fn new(engine: Arc<Engine>, bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		let hman_ret = Arc::new(HookManager {
			focused: Mutex::new(None),
			hooks: Mutex::new(BTreeMap::new()),
			current_id: Mutex::new(0),
			engine,
			bin_map,
			events: MsQueue::new(),
		});
	
		/*
			Press: Mouse(X), Key(X)
			Hold: Mouse(X), Key(X)
			Release: Mouse(X), Key(X)
			Character(-)
			MouseEnter(-)
			MouseLeave(-)
			MouseMove(-)
			MouseScroll(-)
			Focused(X)
			LostFocus(X)
		*/
		
		let hman = hman_ret.clone();
		
		::std::thread::spawn(move || {
			let mut last_tick = Instant::now();
			let tick_interval = Duration::from_millis(5);
			let mut m_window_x = 0.0;
			let mut m_window_y = 0.0;
			let mut m_delta_x = 0.0;
			let mut m_delta_y = 0.0;
			let mut m_moved = false;
			
			let mut hooks_wf_release = Vec::new();
			
			loop {
				let mut focused = hman.focused.lock();
				let mut hooks = hman.hooks.lock();
				let mut events = Vec::new();
				let mut scroll_amt = 0.0;
			
				while let Some(event) = hman.events.try_pop() {
					events.push(event);
				}
				
				events.retain(|event| match event {
					InputEvent::MousePosition(x, y) => {
						m_window_x = *x;
						m_window_y = *y;
						false
					}, InputEvent::MouseDelta(x, y) => {
						m_delta_x += *x;
						m_delta_y += *y;
						m_moved = true;
						false
					}, InputEvent::Scroll(y) => {
						scroll_amt += y;
						false
					}, _ => {
						true
					}
				});
				
				for event in events {
					match event {
						InputEvent::MousePress(button) => {
							let mut top_bin_op = hman.engine.interface_ref().get_bin_atop(m_window_x, m_window_y);
							
							if top_bin_op.as_ref().map(|v| v.id()) != *focused {
								if let Some(bin_id) = &*focused {
									for (_, (hb, hook, func)) in &mut *hooks {
										if hb.id() == *bin_id {
											match hook {
												BinHook::LostFocus => func(hb.clone(), hook), // Call Lost Focus
												_ => ()
											}
										}
									}
								}
								
								*focused = top_bin_op.map(|v| v.id());
								
								if let Some(bin_id) = &*focused {
									for (_, (hb, hook, func)) in &mut *hooks {
										if hb.id() == *bin_id {
											match hook {
												BinHook::Focused => func(hb.clone(), hook), // Call Focused
												_ => ()
											}
										}
									}
								}
							}
							
							if let Some(bin_id) = &*focused {
								for (hook_id, (hb, hook, func)) in &mut *hooks {
									if hb.id() == *bin_id {
										match hook {
											BinHook::Press {
												mouse_active,
												..
											} => {
												mouse_active.entry(button.clone()).and_modify(|v| *v = true);
											},
											
											_ => ()
										}
										
										match hook {
											BinHook::Press { .. } => if hook.is_active() {
												hooks_wf_release.push(hook_id.clone());
												func(hb.clone(), hook); // Call Press
											}, _ => ()
										}
									}
								}
							}
						},
						
						InputEvent::MouseRelease(button) => {
							hooks_wf_release.retain(|hook_id| {
								if let Some((hb, hook, func)) = hooks.get_mut(&hook_id) {
									match hook {
										BinHook::Release {
											mouse_active,
											..
										} => {
											mouse_active.entry(button.clone()).and_modify(|v| *v = false);
										},
										
										BinHook::Hold {
											is_first_call,
											initial_delay_wait,
											initial_delay_elapsed,
											..
										} => {
											*is_first_call = true;
											*initial_delay_wait = false;
											*initial_delay_elapsed = false;
										},
											
										_ => ()
									}
									
									match hook {
										BinHook::Release { .. } => if hook.is_active() {
											func(hb.clone(), hook); // Call Release
											false
										} else {
											true
										}, _ => true
									}
								} else {
									false
								}
							});
						},
						
						InputEvent::KeyPress(key) => {
							if let Some(bin_id) = &*focused {
								for (hook_id, (hb, hook, func)) in &mut *hooks {
									if hb.id() == *bin_id {
										match hook {
											BinHook::Press {
												key_active,
												..
											} => {
												key_active.entry(key.clone()).and_modify(|v| *v = true);
											},
											
											_ => ()
										}
										
										match hook {
											BinHook::Press { .. } => if hook.is_active() {
												func(hb.clone(), hook); // Call Press
												hooks_wf_release.push(hook_id.clone());
											}, _ => ()
										}
									}
								}
							}
						}
						
						InputEvent::KeyRelease(key) => {
							hooks_wf_release.retain(|hook_id| {
								if let Some((hb, hook, func)) = hooks.get_mut(&hook_id) {
									match hook {
										BinHook::Release {
											key_active,
											..
										} => {
											key_active.entry(key.clone()).and_modify(|v| *v = false);
										},
										
										_ => ()
									}
									
									match hook {
										BinHook::Release { .. } => if hook.is_active() {
											func(hb.clone(), hook); // Call Release
											false
										} else {
											true
										}, _ => true
									}
								} else {
									false
								}
							});
						},
						
						_ => ()
					}
				}
				
				if let Some(bin_id) = &*focused {
					for (_, (hb, hook, func)) in &mut *hooks {
						if hb.id() == *bin_id {
							if let BinHook::Hold { .. } = hook {
								if !hook.is_active() {
									continue;
								}
							}
						
							if match hook {
								BinHook::Hold {
									first_call,
									last_call,
									is_first_call,
									interval,
									initial_delay,
									initial_delay_wait,
									initial_delay_elapsed,
									..
								} => {
									if *is_first_call {
										if *initial_delay_wait {
											if first_call.elapsed() < *initial_delay {
												continue;
											} else {
												*initial_delay_wait = false;
												*initial_delay_elapsed = true;
												*first_call = Instant::now();
												*is_first_call = false;
											}
										} else if !*initial_delay_elapsed {
											*initial_delay_wait = true;
											*first_call = Instant::now();
											continue;
										}
									} else if last_call.elapsed() < *interval {
										continue;
									}
									
									true
								},
								
								_ => false
							} {
								func(hb.clone(), hook); // Call Hold
								
								if let BinHook::Hold { last_call, .. } = &mut *hook {
									*last_call = Instant::now();
								}
							}
						}
					}
				}
				
				let elapsed = last_tick.elapsed();
				
				if elapsed < tick_interval {
					::std::thread::sleep(tick_interval - elapsed);
				}
			}
		});	
			
		hman_ret
	}
}

