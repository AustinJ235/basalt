pub mod qwerty;
pub use self::qwerty::*;

use crate::interface::hook::BinHookEvent;
use crate::interface::Interface;
use crate::{BasaltWindow, BstEvent, BstEventSend, BstWinEv};
use crossbeam::channel::{self, Sender};
use crossbeam::sync::{Parker, Unparker};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{self, AtomicUsize};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub type InputHookID = u64;

/// Control what should happen after execution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InputHookCtrl {
	/// Remove hook after execution
	Remove,
	#[default]
	/// Retain hook after execution
	Retain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
	Left,
	Right,
	Middle,
	Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOrMouseButton {
	Key(Qwerty),
	MouseButton(MouseButton),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputHook {
	/// Press is called once when all keys and mouse buttons are active.
	Press {
		global: bool,
		keys: Vec<Qwerty>,
		mouse_buttons: Vec<MouseButton>,
	},
	/// Hold is called while the key and mouse buttons are called. Nothing will
	/// be called until the initial delay period has elapsed. After that it will
	/// be called every time interval has elapsed. `accel` is not implemnted at
	/// this time.
	Hold {
		global: bool,
		keys: Vec<Qwerty>,
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
		keys: Vec<Qwerty>,
		mouse_buttons: Vec<MouseButton>,
	},
	/// Like a normal key press. ``Qwerty`` is converted into a ``Character`` with
	/// modifiers in consideration.
	Character,
	/// Called when the mouse enters the window.
	MouseEnter,
	/// Called when the mouse leaves the window.
	MouseLeave,
	/// Called when the mouse moves within the window.
	MouseMove,
	/// Called when the mouse motion is recieved. This is not a window event, but
	/// rather a device event. Do not use this in combination with MouseMove as the
	/// data units may differ. Example use would be for game camera.
	MouseMotion,
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
					active: false,
					mouse_x: 0.0,
					mouse_y: 0.0,
					first_call: Instant::now(),
					last_call: Instant::now(),
					is_first_call: true,
					initial_delay: *initial_delay,
					initial_delay_elapsed: false,
					interval: *interval,
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

			InputHook::Character =>
				InputHookData::Character {
					character: Character::Value(' '),
				},

			InputHook::MouseEnter =>
				InputHookData::MouseEnter {
					mouse_x: 0.0,
					mouse_y: 0.0,
				},

			InputHook::MouseLeave =>
				InputHookData::MouseLeave {
					mouse_x: 0.0,
					mouse_y: 0.0,
				},

			InputHook::MouseMove =>
				InputHookData::MouseMove {
					mouse_x: 0.0,
					mouse_y: 0.0,
					mouse_dx: 0.0,
					mouse_dy: 0.0,
				},

			InputHook::MouseMotion =>
				InputHookData::MouseMotion {
					x: 0.0,
					y: 0.0,
				},

			InputHook::MouseScroll =>
				InputHookData::MouseScroll {
					scroll_amt: 0.0,
					mouse_x: 0.0,
					mouse_y: 0.0,
				},

			InputHook::WindowFocused => InputHookData::WindowFocused,
			InputHook::WindowLostFocus => InputHookData::WindowLostFocus,

			InputHook::AnyMouseOrKeyPress {
				global,
			} =>
				InputHookData::AnyMouseOrKeyPress {
					global: *global,
					either: KeyOrMouseButton::Key(Qwerty::Space),
				},

			InputHook::AnyMousePress {
				global,
			} =>
				InputHookData::AnyMousePress {
					global: *global,
					button: MouseButton::Left,
				},

			InputHook::AnyKeyPress {
				global,
			} =>
				InputHookData::AnyKeyPress {
					global: *global,
					key: Qwerty::Space,
				},

			InputHook::AnyMouseOrKeyRelease {
				global,
			} =>
				InputHookData::AnyMouseOrKeyRelease {
					global: *global,
					either: KeyOrMouseButton::Key(Qwerty::Space),
				},

			InputHook::AnyMouseRelease {
				global,
			} =>
				InputHookData::AnyMouseRelease {
					global: *global,
					button: MouseButton::Left,
				},

			InputHook::AnyKeyRelease {
				global,
			} =>
				InputHookData::AnyKeyRelease {
					global: *global,
					key: Qwerty::Space,
				},
		}
	}

	pub fn ty(&self) -> InputHookTy {
		match self {
			InputHook::Press {
				..
			} => InputHookTy::Press,
			InputHook::Hold {
				..
			} => InputHookTy::Hold,
			InputHook::Release {
				..
			} => InputHookTy::Release,
			InputHook::Character => InputHookTy::Character,
			InputHook::MouseEnter => InputHookTy::MouseEnter,
			InputHook::MouseLeave => InputHookTy::MouseLeave,
			InputHook::MouseMove => InputHookTy::MouseMove,
			InputHook::MouseMotion {
				..
			} => InputHookTy::MouseMotion,
			InputHook::MouseScroll => InputHookTy::MouseScroll,
			InputHook::WindowFocused => InputHookTy::WindowFocused,
			InputHook::WindowLostFocus => InputHookTy::WindowLostFocus,
			InputHook::AnyMouseOrKeyPress {
				..
			} => InputHookTy::AnyMouseOrKeyPress,
			InputHook::AnyMousePress {
				..
			} => InputHookTy::AnyMousePress,
			InputHook::AnyKeyPress {
				..
			} => InputHookTy::AnyKeyPress,
			InputHook::AnyMouseOrKeyRelease {
				..
			} => InputHookTy::AnyMouseOrKeyRelease,
			InputHook::AnyMouseRelease {
				..
			} => InputHookTy::AnyMouseRelease,
			InputHook::AnyKeyRelease {
				..
			} => InputHookTy::AnyKeyRelease,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputHookTy {
	Press,
	Hold,
	Release,
	Character,
	MouseEnter,
	MouseLeave,
	MouseMove,
	MouseMotion,
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

#[derive(Debug, Clone)]
pub enum InputHookData {
	Press {
		global: bool,
		mouse_x: f32,
		mouse_y: f32,
		key_active: HashMap<Qwerty, bool>,
		mouse_active: HashMap<MouseButton, bool>,
	},
	Hold {
		global: bool,
		active: bool,
		mouse_x: f32,
		mouse_y: f32,
		first_call: Instant,
		last_call: Instant,
		is_first_call: bool,
		initial_delay: Duration,
		initial_delay_elapsed: bool,
		interval: Duration,
		accel: f32,
		key_active: HashMap<Qwerty, bool>,
		mouse_active: HashMap<MouseButton, bool>,
	},
	Release {
		global: bool,
		pressed: bool,
		key_active: HashMap<Qwerty, bool>,
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
	MouseMotion {
		x: f32,
		y: f32,
	},
	MouseScroll {
		mouse_x: f32,
		mouse_y: f32,
		scroll_amt: f32,
	},
	WindowFocused,
	WindowLostFocus,
	AnyMouseOrKeyPress {
		global: bool,
		either: KeyOrMouseButton,
	},
	AnyMousePress {
		global: bool,
		button: MouseButton,
	},
	AnyKeyPress {
		global: bool,
		key: Qwerty,
	},
	AnyMouseOrKeyRelease {
		global: bool,
		either: KeyOrMouseButton,
	},
	AnyMouseRelease {
		global: bool,
		button: MouseButton,
	},
	AnyKeyRelease {
		global: bool,
		key: Qwerty,
	},
}

impl InputHookData {
	pub fn ty(&self) -> InputHookTy {
		match self {
			InputHookData::Press {
				..
			} => InputHookTy::Press,
			InputHookData::Hold {
				..
			} => InputHookTy::Hold,
			InputHookData::Release {
				..
			} => InputHookTy::Release,
			InputHookData::Character {
				..
			} => InputHookTy::Character,
			InputHookData::MouseEnter {
				..
			} => InputHookTy::MouseEnter,
			InputHookData::MouseLeave {
				..
			} => InputHookTy::MouseLeave,
			InputHookData::MouseMove {
				..
			} => InputHookTy::MouseMove,
			InputHookData::MouseMotion {
				..
			} => InputHookTy::MouseMotion,
			InputHookData::MouseScroll {
				..
			} => InputHookTy::MouseScroll,
			InputHookData::WindowFocused => InputHookTy::WindowFocused,
			InputHookData::WindowLostFocus => InputHookTy::WindowLostFocus,
			InputHookData::AnyMouseOrKeyPress {
				..
			} => InputHookTy::AnyMouseOrKeyPress,
			InputHookData::AnyMousePress {
				..
			} => InputHookTy::AnyMousePress,
			InputHookData::AnyKeyPress {
				..
			} => InputHookTy::AnyKeyPress,
			InputHookData::AnyMouseOrKeyRelease {
				..
			} => InputHookTy::AnyMouseOrKeyRelease,
			InputHookData::AnyMouseRelease {
				..
			} => InputHookTy::AnyMouseRelease,
			InputHookData::AnyKeyRelease {
				..
			} => InputHookTy::AnyKeyRelease,
		}
	}

	pub fn cond_met(&self) -> bool {
		match self {
			InputHookData::Press {
				key_active,
				mouse_active,
				..
			} => {
				for v in key_active.values() {
					if !v {
						return false;
					}
				}

				for v in mouse_active.values() {
					if !v {
						return false;
					}
				}
			},

			InputHookData::Hold {
				key_active,
				mouse_active,
				..
			} => {
				for v in key_active.values() {
					if !v {
						return false;
					}
				}

				for v in mouse_active.values() {
					if !v {
						return false;
					}
				}
			},

			InputHookData::Release {
				key_active,
				mouse_active,
				..
			} => {
				for v in key_active.values() {
					if !v {
						return false;
					}
				}

				for v in mouse_active.values() {
					if !v {
						return false;
					}
				}
			},

			_ => (),
		}

		true
	}
}

pub enum Event {
	KeyPress(Qwerty),
	KeyRelease(Qwerty),
	MousePress(MouseButton),
	MouseRelease(MouseButton),
	MouseMotion(f32, f32),
	MousePosition(f32, f32),
	MouseScroll(f32),
	MouseEnter,
	MouseLeave,
	WindowResize(u32, u32),
	WindowScale(f32),
	WindowRedraw,
	WindowFocused,
	WindowLostFocus,
	AddHook(
		InputHookID,
		InputHook,
		Box<dyn FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>,
	),
	DelHook(InputHookID),
	FullscreenExclusive(bool),
}

pub struct Input {
	event_send: Sender<Event>,
	hook_id_count: AtomicUsize,
	unparker: Unparker,
}

impl Input {
	pub fn send_event(&self, event: Event) {
		self.event_send.send(event).unwrap();
		self.unparker.unpark();
	}

	pub fn add_hook<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		hook: InputHook,
		func: F,
	) -> InputHookID {
		let id = self.hook_id_count.fetch_add(1, atomic::Ordering::SeqCst) as u64;
		self.event_send.send(Event::AddHook(id, hook, Box::new(func))).unwrap();
		id
	}

	pub fn remove_hook(&self, id: InputHookID) {
		self.event_send.send(Event::DelHook(id)).unwrap();
	}

	#[inline]
	pub fn on_key_press<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		key: Qwerty,
		func: F,
	) -> InputHookID {
		self.on_key_combo_press(vec![key], func)
	}

	#[inline]
	pub fn on_key_hold<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		key: Qwerty,
		init: Duration,
		int: Duration,
		func: F,
	) -> InputHookID {
		self.on_key_combo_hold(vec![key], init, int, func)
	}

	#[inline]
	pub fn on_key_release<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		key: Qwerty,
		func: F,
	) -> InputHookID {
		self.on_key_combo_release(vec![key], func)
	}

	#[inline]
	pub fn on_key_combo_press<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		combo: Vec<Qwerty>,
		func: F,
	) -> InputHookID {
		self.add_hook(
			InputHook::Press {
				global: false,
				keys: combo,
				mouse_buttons: Vec::new(),
			},
			func,
		)
	}

	#[inline]
	pub fn on_key_combo_hold<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		combo: Vec<Qwerty>,
		init: Duration,
		int: Duration,
		func: F,
	) -> InputHookID {
		self.add_hook(
			InputHook::Hold {
				global: false,
				keys: combo,
				mouse_buttons: Vec::new(),
				initial_delay: init,
				interval: int,
				accel: 0.0,
			},
			func,
		)
	}

	#[inline]
	pub fn on_key_combo_release<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		combo: Vec<Qwerty>,
		func: F,
	) -> InputHookID {
		self.add_hook(
			InputHook::Release {
				global: false,
				keys: combo,
				mouse_buttons: Vec::new(),
			},
			func,
		)
	}

	#[inline]
	pub fn on_mouse_press<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		button: MouseButton,
		func: F,
	) -> InputHookID {
		self.add_hook(
			InputHook::Press {
				global: false,
				keys: Vec::new(),
				mouse_buttons: vec![button],
			},
			func,
		)
	}

	#[inline]
	pub fn on_mouse_hold<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		button: MouseButton,
		init: Duration,
		int: Duration,
		func: F,
	) -> InputHookID {
		self.add_hook(
			InputHook::Hold {
				global: false,
				keys: Vec::new(),
				mouse_buttons: vec![button],
				initial_delay: init,
				interval: int,
				accel: 0.0,
			},
			func,
		)
	}

	#[inline]
	pub fn on_mouse_release<F: FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>(
		&self,
		button: MouseButton,
		func: F,
	) -> InputHookID {
		self.add_hook(
			InputHook::Release {
				global: false,
				keys: Vec::new(),
				mouse_buttons: vec![button],
			},
			func,
		)
	}

	pub(crate) fn new(
		window: Arc<dyn BasaltWindow>,
		interface: Arc<Interface>,
		bst_event_send: BstEventSend,
	) -> Arc<Self> {
		let (event_send, event_recv) = channel::unbounded();
		let parker = Parker::new();
		let unparker = parker.unparker().clone();

		let input = Arc::new(Input {
			event_send,
			hook_id_count: AtomicUsize::new(0),
			unparker,
		});

		let window_cp = window.clone();
		let interface_cp = interface.clone();

		input.add_hook(
			InputHook::AnyKeyPress {
				global: false,
			},
			move |data| {
				if let InputHookData::AnyKeyPress {
					key,
					..
				} = data
				{
					if !window_cp.cursor_captured() {
						interface_cp.hman().send_event(BinHookEvent::KeyPress(*key));
					}
				}

				InputHookCtrl::Retain
			},
		);

		let window_cp = window.clone();
		let interface_cp = interface.clone();

		input.add_hook(
			InputHook::AnyKeyRelease {
				global: false,
			},
			move |data| {
				if let InputHookData::AnyKeyRelease {
					key,
					..
				} = data
				{
					if !window_cp.cursor_captured() {
						interface_cp.hman().send_event(BinHookEvent::KeyRelease(*key));
					}
				}

				InputHookCtrl::Retain
			},
		);

		let window_cp = window.clone();
		let interface_cp = interface.clone();

		input.add_hook(
			InputHook::AnyMousePress {
				global: false,
			},
			move |data| {
				if let InputHookData::AnyMousePress {
					button,
					..
				} = data
				{
					if !window_cp.cursor_captured() {
						interface_cp.hman().send_event(BinHookEvent::MousePress(*button));
					}
				}

				InputHookCtrl::Retain
			},
		);

		let window_cp = window.clone();
		let interface_cp = interface.clone();

		input.add_hook(
			InputHook::AnyMouseRelease {
				global: false,
			},
			move |data| {
				if let InputHookData::AnyMouseRelease {
					button,
					..
				} = data
				{
					if !window_cp.cursor_captured() {
						interface_cp.hman().send_event(BinHookEvent::MouseRelease(*button));
					}
				}

				InputHookCtrl::Retain
			},
		);

		let window_cp = window.clone();
		let interface_cp = interface.clone();

		input.add_hook(InputHook::MouseMove, move |data| {
			if let InputHookData::MouseMove {
				mouse_x,
				mouse_y,
				mouse_dx,
				mouse_dy,
			} = data
			{
				if !window_cp.cursor_captured() {
					interface_cp
						.hman()
						.send_event(BinHookEvent::MousePosition(*mouse_x, *mouse_y));
					interface_cp
						.hman()
						.send_event(BinHookEvent::MouseDelta(*mouse_dx, *mouse_dy));
				}
			}

			InputHookCtrl::Retain
		});

		let window_cp = window.clone();
		let interface_cp = interface.clone();

		input.add_hook(InputHook::MouseScroll, move |data| {
			if let InputHookData::MouseScroll {
				scroll_amt,
				..
			} = data
			{
				if !window_cp.cursor_captured() {
					interface_cp.hman().send_event(BinHookEvent::Scroll(*scroll_amt));
				}
			}

			InputHookCtrl::Retain
		});

		thread::spawn(move || {
			let mut key_state: HashMap<Qwerty, bool> = HashMap::new();
			let mut mouse_state = HashMap::new();
			let mut global_key_state = HashMap::new();
			let mut global_mouse_state = HashMap::new();
			let mut mouse_pos_x = 0.0;
			let mut mouse_pos_y = 0.0;
			let mut window_focused = true;
			let mut mouse_inside = true;
			let mut hook_map: BTreeMap<
				InputHookID,
				(
					InputHookData,
					Box<dyn FnMut(&InputHookData) -> InputHookCtrl + Send + 'static>,
				),
			> = BTreeMap::new();

			loop {
				let mut mouse_motion_x = 0.0;
				let mut mouse_motion_y = 0.0;
				let mut mouse_motion = false;
				let mut mouse_moved = false;
				let mut m_scroll_amt = 0.0;
				let mut scrolled = false;
				let mut events = Vec::new();

				while let Ok(event) = event_recv.try_recv() {
					match event {
						Event::AddHook(id, hook_data, func) => {
							hook_map.insert(id, (hook_data.into_data(), func));
						},
						Event::DelHook(id) => {
							hook_map.remove(&id);
						},
						event => {
							events.push(event);
						},
					}
				}

				let mut window_focus_lost = false;
				let mut remove_hook_ids: Vec<InputHookID> = Vec::new();

				events.retain(|e| {
					match e {
						Event::MouseEnter => {
							for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
								if hook_data.ty() == InputHookTy::MouseEnter {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
							}

							mouse_inside = true;
							false
						},
						Event::MouseLeave => {
							for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
								if hook_data.ty() == InputHookTy::MouseLeave {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
							}

							mouse_inside = false;
							false
						},
						Event::WindowResize(w, h) => {
							bst_event_send.send(BstEvent::BstWinEv(BstWinEv::Resized(*w, *h)));
							false
						},
						Event::WindowRedraw => {
							bst_event_send.send(BstEvent::BstWinEv(BstWinEv::RedrawRequest));
							false
						},
						Event::WindowScale(scale) => {
							interface.set_window_scale(*scale);
							false
						},
						Event::FullscreenExclusive(ex) => {
							bst_event_send
								.send(BstEvent::BstWinEv(BstWinEv::FullScreenExclusive(*ex)));
							false
						},
						Event::WindowFocused => {
							window_focused = true;

							for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
								if hook_data.ty() == InputHookTy::WindowFocused {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
							}

							false
						},
						Event::WindowLostFocus => {
							window_focused = false;
							window_focus_lost = true;

							for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
								if hook_data.ty() == InputHookTy::WindowLostFocus {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
							}

							false
						},
						_ => true,
					}
				});

				if window_focus_lost {
					for (k, v) in key_state.iter_mut() {
						if *v {
							*v = false;
							events.push(Event::KeyRelease(*k));
						}
					}
				}

				for e in events {
					match e {
						Event::KeyPress(k) => {
							if window_focused {
								for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
									let mut call = false;

									if let InputHookData::Character {
										character,
									} = hook_data
									{
										let shift = *key_state
											.entry(Qwerty::LShift)
											.or_insert(false) || *key_state
											.entry(Qwerty::RShift)
											.or_insert(false);
										*character = match k.into_char(shift) {
											Some(some) => some,
											None => continue,
										};

										call = true;
									}

									if call {
										if let InputHookCtrl::Remove = hook_func(hook_data) {
											remove_hook_ids.push(*hook_id);
										}
									}
								}
							}

							let global_entry = global_key_state.entry(k).or_insert(false);
							let entry = key_state.entry(k).or_insert(false);
							let global_reject = *global_entry;

							if *entry && *global_entry {
								continue;
							}

							*global_entry = true;

							if window_focused {
								*entry = true;
							}

							for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
								let mut call = false;

								match hook_data.ty() {
									InputHookTy::AnyMouseOrKeyPress =>
										if let InputHookData::AnyMouseOrKeyPress {
											global,
											either,
										} = hook_data
										{
											if (*global && !global_reject)
												|| (!*global && window_focused)
											{
												*either = KeyOrMouseButton::Key(k);
												call = true;
											}
										},
									InputHookTy::AnyKeyPress =>
										if let InputHookData::AnyKeyPress {
											global,
											key,
										} = hook_data
										{
											if (*global && !global_reject)
												|| (!*global && window_focused)
											{
												*key = k;
												call = true;
											}
										},
									_ => (),
								}

								if call {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
							}
						},

						Event::KeyRelease(k) => {
							if !window_focus_lost {
								let entry = global_key_state.entry(k).or_insert(true);

								if !*entry {
									continue;
								}

								*entry = false;
							}

							if window_focused {
								*key_state.entry(k).or_insert(false) = false;
							}

							for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
								let mut call = false;

								match hook_data.ty() {
									InputHookTy::AnyMouseOrKeyRelease =>
										if let InputHookData::AnyMouseOrKeyRelease {
											global,
											either,
										} = hook_data
										{
											if (*global && !window_focus_lost)
												|| (!*global
													&& (window_focused || window_focus_lost))
											{
												*either = KeyOrMouseButton::Key(k);
												call = true;
											}
										},
									InputHookTy::AnyKeyRelease =>
										if let InputHookData::AnyKeyRelease {
											global,
											key,
										} = hook_data
										{
											if (*global && !window_focus_lost)
												|| (!*global
													&& (window_focused || window_focus_lost))
											{
												*key = k;
												call = true;
											}
										},
									_ => (),
								}

								if call {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
							}
						},

						Event::MousePress(b) => {
							*global_mouse_state.entry(b).or_insert(true) = true;

							if window_focused {
								*mouse_state.entry(b).or_insert(true) = true;
							}

							for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
								let mut call = false;

								match hook_data.ty() {
									InputHookTy::AnyMouseOrKeyPress =>
										if let InputHookData::AnyMouseOrKeyPress {
											global,
											either,
										} = hook_data
										{
											if *global || window_focused {
												*either = KeyOrMouseButton::MouseButton(b);
												call = true;
											}
										},
									InputHookTy::AnyMousePress =>
										if let InputHookData::AnyMousePress {
											global,
											button,
										} = hook_data
										{
											if *global || window_focused {
												*button = b;
												call = true;
											}
										},
									_ => (),
								}

								if call {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
							}
						},

						Event::MouseRelease(b) => {
							*global_mouse_state.entry(b).or_insert(false) = false;

							if window_focused {
								*mouse_state.entry(b).or_insert(false) = false;
							}

							for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
								let mut call = false;

								match hook_data.ty() {
									InputHookTy::AnyMouseOrKeyRelease =>
										if let InputHookData::AnyMouseOrKeyRelease {
											global,
											either,
										} = hook_data
										{
											if *global || window_focused {
												*either = KeyOrMouseButton::MouseButton(b);
												call = true;
											}
										},
									InputHookTy::AnyMouseRelease =>
										if let InputHookData::AnyMouseRelease {
											global,
											button,
										} = hook_data
										{
											if *global || window_focused {
												*button = b;
												call = true;
											}
										},
									_ => (),
								}

								if call {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
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
							m_scroll_amt += v;
							scrolled = true;
						},

						_ => unreachable!(),
					}
				}

				for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
					let mut call = false;

					match hook_data {
						InputHookData::MouseMotion {
							x,
							y,
						} =>
							if mouse_motion && window_focused {
								*x = mouse_motion_x;
								*y = mouse_motion_y;
								call = true;
							},

						InputHookData::MouseMove {
							mouse_x,
							mouse_y,
							mouse_dx,
							mouse_dy,
						} =>
							if mouse_moved && window_focused {
								*mouse_dx = *mouse_x - mouse_pos_x;
								*mouse_x = mouse_pos_x;
								*mouse_dy = *mouse_y - mouse_pos_y;
								*mouse_y = mouse_pos_y;
								call = true;
							},

						InputHookData::MouseScroll {
							scroll_amt,
							mouse_x,
							mouse_y,
						} =>
							if scrolled && mouse_inside {
								*scroll_amt = m_scroll_amt;
								call = true;
								*mouse_x = mouse_pos_x;
								*mouse_y = mouse_pos_y;
							},

						_ => (),
					}

					if call {
						if let InputHookCtrl::Remove = hook_func(hook_data) {
							remove_hook_ids.push(*hook_id);
						}
					}
				}

				for (hook_id, (hook_data, hook_func)) in hook_map.iter_mut() {
					match hook_data.ty() {
						InputHookTy::Press => {
							let mut cond_change = false;

							if let InputHookData::Press {
								global,
								mouse_x,
								mouse_y,
								key_active,
								mouse_active,
							} = hook_data
							{
								for (key, val) in key_active.iter_mut() {
									let v = if *global {
										global_key_state.entry(*key).or_insert(false)
									} else {
										key_state.entry(*key).or_insert(false)
									};

									if *v != *val {
										*val = *v;
										cond_change = true;
									}
								}

								for (button, val) in mouse_active.iter_mut() {
									let b = if *global {
										global_mouse_state.entry(*button).or_insert(false)
									} else {
										mouse_state.entry(*button).or_insert(false)
									};

									if *b != *val {
										*val = *b;
										cond_change = true;
									}
								}

								if cond_change {
									*mouse_x = mouse_pos_x;
									*mouse_y = mouse_pos_y;
								}
							}

							if cond_change && hook_data.cond_met() {
								if let InputHookCtrl::Remove = hook_func(hook_data) {
									remove_hook_ids.push(*hook_id);
								}
							}
						},

						InputHookTy::Release => {
							let mut cond_change = false;

							if let InputHookData::Release {
								global,
								key_active,
								mouse_active,
								..
							} = hook_data
							{
								for (key, val) in key_active.iter_mut() {
									let v = if *global {
										global_key_state.entry(*key).or_insert(false)
									} else {
										key_state.entry(*key).or_insert(false)
									};

									if *v != *val {
										*val = *v;
										cond_change = true;
									}
								}

								for (button, val) in mouse_active.iter_mut() {
									let b = if *global {
										global_mouse_state.entry(*button).or_insert(false)
									} else {
										mouse_state.entry(*button).or_insert(false)
									};

									if *b != *val {
										*val = *b;
										cond_change = true;
									}
								}
							}

							if cond_change {
								let cond_met = hook_data.cond_met();
								let mut call = false;

								if let InputHookData::Release {
									pressed,
									..
								} = hook_data
								{
									if cond_met {
										if !*pressed {
											*pressed = true;
										}
									} else if *pressed {
										*pressed = false;
										call = true;
									}
								}

								if call {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}
								}
							}
						},

						InputHookTy::Hold => {
							let mut hook_act = false;
							let mut cond_change = false;

							if let InputHookData::Hold {
								global,
								active,
								key_active,
								mouse_active,
								..
							} = hook_data
							{
								hook_act = *active;

								for (key, val) in key_active.iter_mut() {
									let v = if *global {
										global_key_state.entry(*key).or_insert(false)
									} else {
										key_state.entry(*key).or_insert(false)
									};

									if *v != *val {
										*val = *v;
										cond_change = true;
									}
								}

								for (button, val) in mouse_active.iter_mut() {
									let b = if *global {
										global_mouse_state.entry(*button).or_insert(false)
									} else {
										mouse_state.entry(*button).or_insert(false)
									};

									if *b != *val {
										*val = *b;
										cond_change = true;
									}
								}
							}

							if cond_change {
								let cond_met = hook_data.cond_met();

								if !hook_act && cond_met {
									if let InputHookData::Hold {
										active,
										is_first_call,
										first_call,
										..
									} = hook_data
									{
										hook_act = true;
										*active = true;
										*is_first_call = true;
										*first_call = Instant::now();
									}
								}

								if hook_act && !cond_met {
									if let InputHookData::Hold {
										active,
										..
									} = hook_data
									{
										*active = false;
										hook_act = false;
									}
								}
							}

							if hook_act {
								let mut call = false;

								if let InputHookData::Hold {
									is_first_call,
									first_call,
									last_call,
									initial_delay,
									mouse_x,
									mouse_y,
									interval,
									..
								} = hook_data
								{
									if *is_first_call {
										if first_call.elapsed() >= *initial_delay {
											*first_call = Instant::now();
											*last_call = Instant::now();
											*mouse_x = mouse_pos_x;
											*mouse_y = mouse_pos_y;
											call = true;
										}
									} else if last_call.elapsed() >= *interval {
										*mouse_x = mouse_pos_x;
										*mouse_y = mouse_pos_y;
										call = true;
									}
								}

								if call {
									if let InputHookCtrl::Remove = hook_func(hook_data) {
										remove_hook_ids.push(*hook_id);
									}

									if let InputHookData::Hold {
										is_first_call,
										last_call,
										..
									} = hook_data
									{
										*is_first_call = false;
										*last_call = Instant::now();
									}
								}
							}
						},

						_ => (),
					}
				}

				for hook_id in remove_hook_ids {
					hook_map.remove(&hook_id);
				}

				parker.park_timeout(Duration::from_micros(4167));
			}
		});

		input
	}
}
