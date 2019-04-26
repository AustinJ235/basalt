pub mod winit;
pub mod x11;
pub mod qwery;
pub use self::qwery::*;

use std::time::Duration;
use std::thread;
use std::sync::Arc;
use std::time::Instant;
use Engine;
use crossbeam::channel::{self,Sender};
use std::collections::{BTreeMap,HashMap};
use std::sync::atomic::{self,AtomicUsize};
use EngineEvent;

pub type InputHookID = u64;
pub type InputHookFn = Arc<Fn(&InputHookData) -> InputHookRes + Send + Sync>;

#[derive(Debug,Clone,PartialEq,Eq)]
pub enum InputHookRes {
	Remove,
	Success,
	Warning(String),
	Error(String),
}

#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub enum MouseButton {
	Left,
	Right,
	Middle,
}

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum KeyOrMouseButton {
	Key(Qwery),
	MouseButton(MouseButton),
}

#[derive(Debug,Clone,PartialEq)]
pub enum InputHook {
	/// Press is called once when all keys and mouse buttons are active.
	Press {
		global: bool,
		keys: Vec<Qwery>,
		mouse_buttons: Vec<MouseButton>,
	},
	/// Hold is called while the key and mouse buttons are called. Nothing will
	/// be called until the initial delay period has elapsed. After that it will
	/// be called every time interval has elapsed. `accel` is not implemnted at
	/// this time.
	Hold {
		global: bool,
		keys: Vec<Qwery>,
		mouse_buttons: Vec<MouseButton>,
		initial_delay: Duration,
		interval: Duration,
		accel: f32,
	},
	/// Release is called when the all keys/buttons have been set to active and
	/// then anyone of them has been release. Release is also called when the
	/// window loses focus.
	Release {
		global: bool,
		keys: Vec<Qwery>,
		mouse_buttons: Vec<MouseButton>,
	},
	/// Like a normal key press. ``Qwery`` is converted into a ``Character`` with
	/// modifiers in consideration.
	Character,
	/// Called when the mouse enters the window.
	MouseEnter,
	/// Called when the mouse leaves the window.
	MouseLeave,
	/// Called when the mouse moves within the window.
	MouseMove,
	/// Called when the mouse is over the window.
	MouseScroll,
	/// Called when the window gains focus.
	WindowFocused,
	/// Called when the window loses focus.
	WindowLostFocus,
	/// Called on any mouse button or key press.
	AnyMouseOrKeyPress {
		global: bool,
	},
	/// Called on any mouse button press.
	AnyMousePress {
		global: bool,
	},
	/// Called on any key press.
	AnyKeyPress {
		global: bool,
	},
	/// Called on any mouse button or key release. Also called when the window
	/// loses focus with all the keys and buttons that were currently held before.
	AnyMouseOrKeyRelease {
		global: bool,
	},
	/// Called on any mouse button release. Also called when the window loses
	/// focus with all the mouse buttons that were currently held before.
	AnyMouseRelease {
		global: bool,
	},
	/// Called on any key release. Also called when the window loses
	/// focus with all the keys that were currently held before.
	AnyKeyRelease {
		global: bool,
	},
}

impl InputHook {
	pub fn into_data(&self) -> InputHookData {
		match self {
			InputHook::Press {
				global,
				keys,
				mouse_buttons,
			} => {
				let mut key_active = HashMap::new();
				let mut mouse_active = HashMap::new();
				
				for key in keys {
					key_active.insert(*key, false);
				}
				
				for button in mouse_buttons {
					mouse_active.insert(*button, false);
				}
			
				InputHookData::Press {
					global: *global,
					mouse_x: 0.0,
					mouse_y: 0.0,
					key_active,
					mouse_active,
				}
			},
			
			InputHook::Hold {
				global,
				keys,
				mouse_buttons,
				initial_delay,
				interval,
				accel,
			} => {
				let mut key_active = HashMap::new();
				let mut mouse_active = HashMap::new();
				
				for key in keys {
					key_active.insert(*key, false);
				}
				
				for button in mouse_buttons {
					mouse_active.insert(*button, false);
				}
				
				InputHookData::Hold {
					global: *global,
					mouse_x: 0.0,
					mouse_y: 0.0,
					first_call: Instant::now(),
					last_call: Instant::now(),
					is_first_call: true,
					initial_delay: initial_delay.clone(),
					initial_delay_wait: true,
					initial_delay_elapsed: false,
					interval: interval.clone(),
					accel: *accel,
					key_active,
					mouse_active,
				}
			},
			
			InputHook::Release {
				global,
				keys,
				mouse_buttons,
			} => {
				let mut key_active = HashMap::new();
				let mut mouse_active = HashMap::new();
				
				for key in keys {
					key_active.insert(*key, false);
				}
				
				for button in mouse_buttons {
					mouse_active.insert(*button, false);
				}
				
				InputHookData::Release {
					global: *global,
					pressed: false,
					key_active,
					mouse_active,
				}
			},
				
			InputHook::Character => {
				InputHookData::Character {
					character: Character::Value(' '),
				}
			},
			
			InputHook::MouseEnter => {
				InputHookData::MouseEnter {
					mouse_x: 0.0,
					mouse_y: 0.0,
				}
			},
			
			InputHook::MouseLeave => {
				InputHookData::MouseLeave {
					mouse_x: 0.0,
					mouse_y: 0.0,
				}
			},
			
			InputHook::MouseMove => {
				InputHookData::MouseMove {
					mouse_x: 0.0,
					mouse_y: 0.0,
					mouse_dx: 0.0,
					mouse_dy: 0.0,
				}
			},
			
			InputHook::MouseScroll => {
				InputHookData::MouseScroll {
					scroll_amt: 0.0,
				}
			},
			
			InputHook::WindowFocused => InputHookData::WindowFocused,
			InputHook::WindowLostFocus => InputHookData::WindowLostFocus,
			
			InputHook::AnyMouseOrKeyPress {
				global,
			} => {
				InputHookData::AnyMouseOrKeyPress {
					global: *global,
					either: KeyOrMouseButton::Key(Qwery::Space),
				}
			}
			
			InputHook::AnyMousePress {
				global,
			} => {
				InputHookData::AnyMousePress {
					global: *global,
					button: MouseButton::Left,
				}
			},
			
			InputHook::AnyKeyPress {
				global,
			} => {
				InputHookData::AnyKeyPress {
					global: *global,
					key: Qwery::Space,
				}
			},
			
			InputHook::AnyMouseOrKeyRelease {
				global,
			} => {
				InputHookData::AnyMouseOrKeyRelease {
					global: *global,
					either: KeyOrMouseButton::Key(Qwery::Space),
				}
			},
			
			InputHook::AnyMouseRelease {
				global,
			} => {
				InputHookData::AnyMouseRelease {
					global: *global,
					button: MouseButton::Left,
				}
			},
			
			InputHook::AnyKeyRelease {
				global,
			} => {
				InputHookData::AnyKeyRelease {
					global: *global,
					key: Qwery::Space,
				}
			},
		}
	}
	
	pub fn ty(&self) -> InputHookTy {
		match self {
			InputHook::Press { .. } => InputHookTy::Press,
			InputHook::Hold { .. } => InputHookTy::Hold,
			InputHook::Release { .. } => InputHookTy::Release,
			InputHook::Character => InputHookTy::Character,
			InputHook::MouseEnter => InputHookTy::MouseEnter,
			InputHook::MouseLeave => InputHookTy::MouseLeave,
			InputHook::MouseMove => InputHookTy::MouseMove,
			InputHook::MouseScroll => InputHookTy::MouseScroll,
			InputHook::WindowFocused => InputHookTy::WindowFocused,
			InputHook::WindowLostFocus => InputHookTy::WindowLostFocus,
			InputHook::AnyMouseOrKeyPress { .. } => InputHookTy::AnyMouseOrKeyPress,
			InputHook::AnyMousePress { .. } => InputHookTy::AnyMousePress,
			InputHook::AnyKeyPress { .. } => InputHookTy::AnyKeyPress,
			InputHook::AnyMouseOrKeyRelease { .. } => InputHookTy::AnyMouseOrKeyRelease,
			InputHook::AnyMouseRelease { .. } => InputHookTy::AnyMouseRelease,
			InputHook::AnyKeyRelease { .. } => InputHookTy::AnyKeyRelease,
		}
	}
}

#[derive(Debug,Clone,PartialEq,Eq)]
pub enum InputHookTy {
	Press,
	Hold,
	Release,
	Character,
	MouseEnter,
	MouseLeave,
	MouseMove,
	MouseScroll,
	WindowFocused,
	WindowLostFocus,
	AnyMouseOrKeyPress,
	AnyMousePress,
	AnyKeyPress,
	AnyMouseOrKeyRelease,
	AnyMouseRelease,
	AnyKeyRelease,
}

#[derive(Debug,Clone)]
pub enum InputHookData {
	Press {
		global: bool,
		mouse_x: f32,
		mouse_y: f32,
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<MouseButton, bool>,
	},
	Hold {
		global: bool,
		mouse_x: f32,
		mouse_y: f32,
		first_call: Instant,
		last_call: Instant,
		is_first_call: bool,
		initial_delay: Duration,
		initial_delay_wait: bool,
		initial_delay_elapsed: bool,
		interval: Duration,
		accel: f32,
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<MouseButton, bool>,
	},
	Release {
		global: bool,
		pressed: bool,
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<MouseButton, bool>,
	},
	Character {
		character: Character,
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
	WindowFocused,
	WindowLostFocus,
	AnyMouseOrKeyPress {
		global: bool,
		either: KeyOrMouseButton
	},
	AnyMousePress {
		global: bool,
		button: MouseButton,
	},
	AnyKeyPress {
		global: bool,
		key: Qwery,
	},
	AnyMouseOrKeyRelease {
		global: bool,
		either: KeyOrMouseButton
	},
	AnyMouseRelease {
		global: bool,
		button: MouseButton,
	},
	AnyKeyRelease {
		global: bool,
		key: Qwery,
	},
}

impl InputHookData {
	pub fn ty(&self) -> InputHookTy {
		match self {
			InputHookData::Press { .. } => InputHookTy::Press,
			InputHookData::Hold { .. } => InputHookTy::Hold,
			InputHookData::Release { .. } => InputHookTy::Release,
			InputHookData::Character { .. } => InputHookTy::Character,
			InputHookData::MouseEnter { .. } => InputHookTy::MouseEnter,
			InputHookData::MouseLeave { .. } => InputHookTy::MouseLeave,
			InputHookData::MouseMove { .. } => InputHookTy::MouseMove,
			InputHookData::MouseScroll { .. } => InputHookTy::MouseScroll,
			InputHookData::WindowFocused => InputHookTy::WindowFocused,
			InputHookData::WindowLostFocus => InputHookTy::WindowLostFocus,
			InputHookData::AnyMouseOrKeyPress { .. } => InputHookTy::AnyMouseOrKeyPress,
			InputHookData::AnyMousePress { .. } => InputHookTy::AnyMousePress,
			InputHookData::AnyKeyPress { .. } => InputHookTy::AnyKeyPress,
			InputHookData::AnyMouseOrKeyRelease { .. } => InputHookTy::AnyMouseOrKeyRelease,
			InputHookData::AnyMouseRelease { .. } => InputHookTy::AnyMouseRelease,
			InputHookData::AnyKeyRelease { .. } => InputHookTy::AnyKeyRelease,
		}
	}
}

pub enum Event {
	KeyPress(Qwery),
	KeyRelease(Qwery),
	MousePress(MouseButton),
	MouseRelease(MouseButton),
	MouseMotion(f32, f32),
	MousePosition(f32, f32),
	MouseScroll(f32),
	MouseEnter,
	MouseLeave,
	WindowResized,
	WindowDPIChange(f32),
	WindowFocused,
	WindowLostFocus,
	AddHook(InputHookID, InputHook, InputHookFn),
	DelHook(InputHookID),
}

pub struct Input {
	engine: Arc<Engine>,
	event_send: Sender<Event>,
	hook_id_count: AtomicUsize,
}

impl Input {
	pub(crate) fn new(engine: Arc<Engine>) -> Arc<Self> {
		let (event_send, event_recv) = channel::unbounded();
	
		let input_ret = Arc::new(Input {
			engine,
			event_send,
			hook_id_count: AtomicUsize::new(0),
		});
		
		let input = input_ret.clone();
		
		thread::spawn(move || {
			let mut key_state = HashMap::new();
			let mut mouse_state = HashMap::new();
			let mut global_key_state = HashMap::new();
			let mut global_mouse_state = HashMap::new();
			let mut mouse_pos_x = 0.0;
			let mut mouse_pos_y = 0.0;
			let mut window_focused = true;
			let mut hook_map: BTreeMap<InputHookID, (InputHookData, InputHookFn)> = BTreeMap::new();
		
			loop {
				let start = Instant::now();
				let mut mouse_motion_x = 0.0;
				let mut mouse_motion_y = 0.0;
				let mut mouse_motion = false;
				let mut mouse_moved = false;
				let mut scroll_amt = 0.0;
				let mut scrolled = false;
				let mut focus_changed = false;
				let mut events = Vec::new();

				while let Ok(event) = event_recv.try_recv() {
					events.push(event);
				}
				
				events.retain(|e| {
					match e {
						Event::AddHook(id, hook_data, func) => {
							hook_map.insert(*id, (hook_data.into_data(), func.clone()));
							false
						},
						Event::DelHook(id) => {
							hook_map.remove(&id);
							false
						},
						_ => true
					}
				});
				
				events.retain(|e| {
					match e {
						Event::MouseEnter => {
							for (_hook_id, (hook_data, hook_func)) in &hook_map {
								if hook_data.ty() == InputHookTy::MouseEnter {
									hook_func(hook_data);
								}	
							}
							
							false
						},
						Event::MouseLeave => {
							for (_hook_id, (hook_data, hook_func)) in &hook_map {
								if hook_data.ty() == InputHookTy::MouseLeave {
									hook_func(hook_data);
								}	
							}
							
							false
						},
						Event::WindowResized => {
							input.engine.send_event(EngineEvent::WindowResized);
							false
						},
						Event::WindowDPIChange(dpi) => {
							input.engine.send_event(EngineEvent::DPIChanged(*dpi));
							false
						}
						Event::WindowFocused => {
							window_focused = true;
							
							for (_hook_id, (hook_data, hook_func)) in &hook_map {
								if hook_data.ty() == InputHookTy::WindowFocused {
									hook_func(hook_data);
								}	
							}
							
							false
						},
						Event::WindowLostFocus => {
							window_focused = false;
							
							for (_hook_id, (hook_data, hook_func)) in &hook_map {
								if hook_data.ty() == InputHookTy::WindowLostFocus {
									hook_func(hook_data);
								}	
							}
							
							false
						},
						_ => true
					}
				});
				
				for e in events {
					match e {
						Event::KeyPress(k) => {
							*global_key_state.entry(k).or_insert(true) = true;
							
							if window_focused {
								*key_state.entry(k).or_insert(true) = true;
							}
							
							for (_hook_id, (ref mut hook_data, hook_func)) in &mut hook_map {
								let mut call = false;
								
								match hook_data.ty() {
									InputHookTy::AnyMouseOrKeyPress
										=> if let InputHookData::AnyMouseOrKeyPress {
											global,
											either
										} = hook_data {
											if *global || window_focused {
												*either = KeyOrMouseButton::Key(k.clone());
												call = true;
											}
										},
									InputHookTy::AnyKeyPress
										=> if let InputHookData::AnyKeyPress {
											global,
											key
										} = hook_data {
											if *global || window_focused {
												*key = k.clone();
												call = true;
											}
										},
									_ => (),
								}
								
								if call {
									hook_func(hook_data);
								}
							}
						},
						Event::KeyRelease(k) => {
							*global_key_state.entry(k).or_insert(false) = false;
							
							if window_focused {
								*key_state.entry(k).or_insert(false) = false;
							}
							
							for (_hook_id, (ref mut hook_data, hook_func)) in &mut hook_map {
								let mut call = false;
								
								match hook_data.ty() {
									InputHookTy::AnyMouseOrKeyRelease
										=> if let InputHookData::AnyMouseOrKeyRelease {
											global,
											either
										} = hook_data {
											if *global || window_focused {
												*either = KeyOrMouseButton::Key(k.clone());
												call = true;
											}
										},
									InputHookTy::AnyKeyPress
										=> if let InputHookData::AnyKeyRelease {
											global,
											key
										} = hook_data {
											if *global || window_focused {
												*key = k.clone();
												call = true;
											}
										},
									_ => (),
								}
								
								if call {
									hook_func(hook_data);
								}
							}
						},
						Event::MousePress(b) => {
							*global_mouse_state.entry(b).or_insert(true) = true;
							
							if window_focused {
								*mouse_state.entry(b).or_insert(true) = true;
							}
						},
						Event::MouseRelease(b) => {
							*global_mouse_state.entry(b).or_insert(false) = false;
							
							if window_focused {
								*mouse_state.entry(b).or_insert(false) = false;
							}
						},
						Event::MouseMotion(x, y) => {
							mouse_motion_x += x;
							mouse_motion_y += y;
							mouse_motion = true;
						},
						Event::MousePosition(x, y) => {
							mouse_pos_x = x;
							mouse_pos_y = y;
							mouse_moved = true;
						},
						Event::MouseScroll(v) => {
							scroll_amt += v;
							scrolled = true;
						},
						_ => unreachable!()
					}
				}
				
				for (hook_id, hook) in hook_map.iter_mut() {
				
				}
				
				if start.elapsed() > Duration::from_millis(10) {
					continue;
				}
				
				thread::sleep(Duration::from_millis(10) - start.elapsed());
			}
		});
		
		input_ret
	}
	
	pub fn send_event(&self, event: Event) {
		self.event_send.send(event).unwrap();
	}
	
	pub fn add_hook(&self, hook: InputHook, func: InputHookFn) -> InputHookID {
		let id = self.hook_id_count.fetch_add(1, atomic::Ordering::SeqCst) as u64;
		self.event_send.send(Event::AddHook(id, hook, func)).unwrap();
		id
	}
}

