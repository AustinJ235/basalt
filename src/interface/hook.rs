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

pub type BinHookFn = Arc<Fn(&BinHook) + Send + Sync>;

#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord,Hash)]
pub struct BinHookID(u64);

pub enum BinHook {
	Press {
		keys: Vec<Qwery>,
		mouse: Vec<mouse::Button>,
		keys_active: HashMap<Vec<Qwery>, bool>,
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
		keys_active: HashMap<Vec<Qwery>, bool>,
		mouse_active: HashMap<mouse::Button, bool>,
	},
	
	Release {
		keys: Vec<Qwery>,
		mouse: Vec<mouse::Button>,
		keys_active: HashMap<Vec<Qwery>, bool>,
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

impl BinHook {
	fn is_active(&self) -> bool {
		unimplemented!()
	}
}

pub(crate) struct HookManager {
	focused: Mutex<Option<u64>>,
	hooks: Mutex<Hooks>,
	engine: Arc<Engine>,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
}

impl HookManager {
	pub fn new(engine: Arc<Engine>, bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		let hman_ret = Arc::new(HookManager {
			focused: Mutex::new(None),
			hooks: Mutex::new(Hooks {
				inner: BTreeMap::new(),
				by_mouse: HashMap::new(),
				by_key_combo: HashMap::new(),
				by_bin: BTreeMap::new(),
				current_id: 0
			}),
			engine,
			bin_map,
		});
	
		/*
			Press: Mouse(X), Key(-)
			Hold: Mouse(X), Key(X)
			Release: Mouse(X), Key(-)
			Character(-)
			MouseEnter(-)
			MouseLeave(-)
			MouseMove(-)
			MouseScroll(-)
			Focused(X)
			LostFocus(X)
		*/
	
		let hman = hman_ret.clone();
		
		hman_ret.engine.mouse_ref().on_any_press(Arc::new(move |_, mouse::PressInfo {
			button,
			window_x,
			window_y,
			..
		}| {
			let mut focused = hman.focused.lock();
			let mut hooks = hman.hooks.lock();
			let mut top_bin_op = hman.engine.interface_ref().get_bin_atop(window_x, window_y);
			
			if
				(focused.is_some() && top_bin_op.is_none())
				|| (
					focused.is_some() && top_bin_op.is_some()
					&& *focused.as_ref().unwrap() != top_bin_op.as_ref().unwrap().id()
				) || (focused.is_none() && top_bin_op.is_some())
			{
				if let Some(bin_id) = &*focused {
					for hook in hooks.by_bin_id(*bin_id) {
						match hook {
							BinHook::LostFocus => (), // Call Lost Focus
							_ => ()
						}
					}
				}
			
				*focused = top_bin_op.map(|v| v.id());
				
				if let Some(bin_id) = &*focused {
					for hook in hooks.by_bin_id(*bin_id) {
						match hook {
							BinHook::Focused => (), // Call Focused
							_ => ()
						}
					}
				}
			}
			
			if let Some(bin_id) = &*focused {
				for hook in hooks.by_bin_id(*bin_id) {
					match hook {
						BinHook::Press {
							mouse_active,
							..
						} => {
							mouse_active.entry(button.clone()).and_modify(|v| *v = true);
						},
						
						_ => ()
					}
					
					if hook.is_active() {
						// Call Press
					}
				}
			}
		}));
		
		let hman = hman_ret.clone();
		
		hman_ret.engine.mouse_ref().on_any_press(Arc::new(move |_, mouse::PressInfo {
			button,
			window_x,
			window_y,
			..
		}| {
			let mut focused = hman.focused.lock();
			let mut hooks = hman.hooks.lock();
			
			if let Some(bin_id) = &*focused {
				for hook in hooks.by_bin_id(*bin_id) {
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
					
					if hook.is_active() {
						// Call Release
					}
				}
			}
		}));
		
		let hman = hman_ret.clone();
		
		::std::thread::spawn(move || {
			let mut last_tick = Instant::now();
			let tick_interval = Duration::from_millis(5);
			
			loop {
				let mut focused = hman.focused.lock();
				let mut hooks = hman.hooks.lock();
				
				if let Some(bin_id) = &*focused {
					for hook in hooks.by_bin_id(*bin_id) {
						if let BinHook::Hold { .. } = hook {
							if !hook.is_active() {
								continue;
							}
						}
					
						match hook {
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
								
								// Call Hold
								
								*last_call = Instant::now();
							},
							
							_ => ()
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

struct Hooks {
	inner: BTreeMap<BinHookID, (u64, BinHook)>,
	by_mouse: HashMap<mouse::Button, BinHookID>,
	by_key_combo: HashMap<Vec<Qwery>, BinHookID>,
	by_bin: BTreeMap<u64, BinHookID>,
	current_id: u64,
}

impl Hooks {
	fn add_hook(&mut self, bin: Arc<Bin>, hook: BinHook) -> BinHookID {
		let id = BinHookID(self.current_id);
		self.current_id += 1;
		
		let (keys, mouse_buttons) = match &hook {
			BinHook::Press { keys, mouse, .. } => (keys.clone(), mouse.clone()),
			BinHook::Hold { keys, mouse, .. } => (keys.clone(), mouse.clone()),
			BinHook::Release { keys, mouse, .. } => (keys.clone(), mouse.clone()),
			_ => (Vec::new(), Vec::new())
		};
		
		self.by_key_combo.insert(keys, id);
		
		for button in mouse_buttons {
			self.by_mouse.insert(button, id);
		}
		
		self.by_bin.insert(bin.id(), id);
		self.inner.insert(id, (bin.id(), hook));
		id
	}
	
	fn by_bin_id(&self, id: u64) -> Vec<&mut BinHook> {
		unimplemented!()
	}
}

