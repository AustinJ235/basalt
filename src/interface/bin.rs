use std::sync::atomic::{self,AtomicBool};
use super::interface::ItfVertInfo;
use interface::interface::scale_verts;
use parking_lot::{RwLock,Mutex};
use std::sync::{Weak,Arc};
use Engine;
use super::super::mouse;
use vulkano;
use vulkano::image::traits::ImageViewAccess;
use super::super::atlas;
use super::super::keyboard::CharType;
use std::thread;
use std::time::Duration;
pub use interface::TextWrap;
use std::sync::Barrier;
use atlas::CoordsInfo;
use vulkano::image::immutable::ImmutableImage;
use std::time::Instant;
use keyboard::{self,Qwery};
use std::collections::BTreeMap;
use misc;
//use interface::text;
use interface::TextAlign;
use interface::WrapTy;
use interface::hook::{BinHook,BinHookID,BinHookFn};

type OnLeftMousePress = Arc<Fn() + Send + Sync>;

pub trait KeepAlive { }
impl KeepAlive for Arc<Bin> {}
impl KeepAlive for Bin {}
impl<T: KeepAlive> KeepAlive for Vec<T> {}

pub type HookFn = Arc<Fn(EventInfo) + Send + Sync>;

#[allow(dead_code)]
pub struct Hook {
	pub(crate) requires_focus: bool,
	pub(crate) mouse_press: Vec<mouse::Button>,
	pub(crate) mouse_hold: Vec<(mouse::Button, Repeat)>,
	pub(crate) mouse_release: Vec<mouse::Button>,
	pub(crate) mouse_move: bool,
	pub(crate) mouse_enter: bool,
	pub(crate) mouse_leave: bool,
	pub(crate) mouse_scroll: bool,
	pub(crate) key_press: Vec<Vec<Qwery>>,
	pub(crate) key_hold: Vec<(Vec<Qwery>, Repeat)>,
	pub(crate) key_release: Vec<Vec<Qwery>>,
	pub(crate) char_press: bool,
	pub(crate) func: Option<HookFn>,
	pub(crate) func_spawn: bool,
	pub(crate) lost_focus: bool,
	pub(crate) on_focus: bool,
}

impl Hook {
	fn add_to_engine(&mut self, engine: &Arc<Engine>, bin: &Bin) -> Vec<Hook> {
		if self.func.is_none() {
			return Vec::new();
		}
	
		if !self.key_press.is_empty() || !self.key_hold.is_empty() || !self.key_release.is_empty() || self.char_press {
			let mut new_hooks = Vec::new();
			
			let focused = Arc::new(Mutex::new(false));
			let focused_cp = focused.clone();
			new_hooks.push(Hook::new().on_focus().func(Arc::new(move |_| { *focused_cp.lock() = true; })));
			let focused_cp = focused.clone();
			new_hooks.push(Hook::new().lost_focus().func(Arc::new(move |_| { *focused_cp.lock() = false; })));
			let mut keyboard_hooks = bin.kb_hook_ids.lock();
			
			if self.char_press {
				let func = self.func.as_ref().unwrap().clone();
				let focused = focused.clone();
				
				keyboard_hooks.push(engine.keyboard_ref().on_char_press(Arc::new(move |keyboard::CallInfo {
					char_ty,
					..
				}| {
					if !*focused.lock() { return; }
					let mut info = EventInfo::other();
					info.trigger = HookTrigger::CharPress;
					info.char_ty = char_ty;
					func(info);
				})));
			}
			
			if self.key_press.is_empty() {
				let func = self.func.as_ref().unwrap().clone();
				let focused = focused.clone();
				
				keyboard_hooks.push(engine.keyboard_ref().on_press(self.key_press.clone(), Arc::new(move |keyboard::CallInfo {
					combos,
					..
				}| {
					if !*focused.lock() { return; }
					let mut info = EventInfo::other();
					info.trigger = HookTrigger::KeyPress;
					info.key_combos = combos;
					func(info);
				})));
			}
			
			if self.key_hold.is_empty() {
				for (combo, repeat) in &self.key_hold {
					let millis = repeat.rate.as_millis() as u64;
					let func = self.func.as_ref().unwrap().clone();
					let focused = focused.clone();
					
					keyboard_hooks.push(engine.keyboard_ref().on_hold(vec![combo.clone()], millis, Arc::new(move |keyboard::CallInfo {
						combos,
						..
					}| {
						if !*focused.lock() { return; }
						let mut info = EventInfo::other();
						info.trigger = HookTrigger::KeyHold;
						info.key_combos = combos;
						func(info);
					})));
				}
			}
			
			if self.key_release.is_empty() {
				let func = self.func.as_ref().unwrap().clone();
				let focused = focused.clone();
				
				keyboard_hooks.push(engine.keyboard_ref().on_release(self.key_release.clone(), Arc::new(move |keyboard::CallInfo {
					combos,
					..
				}| {
					if !*focused.lock() { return; }
					let mut info = EventInfo::other();
					info.trigger = HookTrigger::KeyHold;
					info.key_combos = combos;
					func(info);
				})));
			}
			
			new_hooks
		} else {
			return Vec::new();
		}
	}

	pub fn new() -> Self {
		Hook {
			requires_focus: true,
			mouse_press: Vec::new(),
			mouse_hold: Vec::new(),
			mouse_release: Vec::new(),
			mouse_move: false,
			mouse_enter: false,
			mouse_leave: false,
			mouse_scroll: false,
			key_press: Vec::new(),
			key_hold: Vec::new(),
			key_release: Vec::new(),
			char_press: false,
			func: None,
			func_spawn: false,
			lost_focus: false,
			on_focus: false,
		}
	}
	
	pub(crate) fn run(&self, info: EventInfo) {
		if let Some(func) = self.func.clone() {
			if self.func_spawn {
				thread::spawn(move || {
					func(info);
				});
			} else {
				func(info);
			}
		}
	}
	
	pub fn key_press(mut self, key: Qwery) -> Self {
		self.key_press.push(vec![key]);
		self
	} pub fn key_hold(mut self, key: Qwery) -> Self {
		self.key_hold.push((vec![key], Repeat::basic()));
		self
	} pub fn key_release(mut self, key: Qwery) -> Self {
		self.key_release.push(vec![key]);
		self
	} pub fn char_press(mut self) -> Self {
		self.char_press = true;
		self
	} pub fn func(mut self, func: HookFn) -> Self {
		self.func = Some(func);
		self
	} pub fn spawn(mut self) -> Self {
		self.func_spawn = true;
		self
	} pub fn mouse_press(mut self, button: mouse::Button) -> Self {
		self.mouse_press.push(button);
		self
	} pub fn mouse_hold(mut self, button: mouse::Button) -> Self {
		self.mouse_hold.push((button, Repeat::basic()));
		self
	} pub fn mouse_release(mut self, button: mouse::Button) -> Self {
		self.mouse_release.push(button);
		self
	} pub fn mouse_move(mut self) -> Self {
		self.mouse_move = true;
		self
	} pub fn mouse_enter(mut self) -> Self {
		self.mouse_enter = true;
		self
	} pub fn mouse_leave(mut self) -> Self {
		self.mouse_leave = true;
		self
	} pub fn mouse_scroll(mut self) -> Self {
		self.mouse_scroll = true;
		self
	} pub fn no_focus(mut self) -> Self {
		self.requires_focus = false;
		self
	} pub fn on_focus(mut self) -> Self {
		self.on_focus = true;
		self
	} pub fn lost_focus(mut self) -> Self {
		self.lost_focus = true;
		self
	}
		
}

pub enum HookTrigger {
	KeyPress,
	KeyHold,
	KeyRelease,
	CharPress,
	MousePress,
	MouseHold,
	MouseRelease,
	MouseEnter,
	MouseLeave,
	MouseMove,
	MouseScroll,
	Focus,
	LostFocus,
	Other
}

pub struct EventInfo {
	pub trigger: HookTrigger,
	pub mouse_btts: Vec<mouse::Button>,
	pub key_combos: Vec<Vec<Qwery>>,
	pub scroll_amt: f32,
	pub mouse_dx: f32,
	pub mouse_dy: f32,
	pub mouse_x: f32,
	pub mouse_y: f32,
	pub char_ty: Option<keyboard::CharType>,
}

impl EventInfo {
	pub(crate) fn other() -> Self {
		EventInfo {
			trigger: HookTrigger::Other,
			mouse_btts: Vec::new(),
			key_combos: Vec::new(),
			scroll_amt: 0.0,
			mouse_dx: 0.0,
			mouse_dy: 0.0,
			mouse_x: 0.0,
			mouse_y: 0.0,
			char_ty: None,
		}
	}
}

#[allow(dead_code)]
pub struct Repeat {
	once: bool,
	initial: Duration,
	rate: Duration,
	count: usize,
	accel_fn: Option<Box<Fn(usize, u32) -> usize + Send + Sync>>,
}

impl Repeat {
	pub fn basic() -> Self {
		Repeat {
			once: false,
			initial: Duration::from_millis(200),
			rate: Duration::from_millis(50),
			count: 0,
			accel_fn: None,
		}
	}
}

#[derive(Default,Clone,Debug,PartialEq)]
pub struct BinVert {
	pub position: (f32, f32, i16),
	pub color: Color,
}

#[derive(Default,Clone)]
pub struct BinStyle {
	pub position_t: Option<PositionTy>,
	pub z_index: Option<i16>,
	pub add_z_index: Option<i16>,
	pub hidden: Option<bool>,
	pub opacity: Option<f32>,
	pub pass_events: Option<bool>,
	// Position from Edges
	pub pos_from_t: Option<f32>,
	pub pos_from_b: Option<f32>,
	pub pos_from_l: Option<f32>,
	pub pos_from_r: Option<f32>,
	// Size
	pub width: Option<f32>,
	pub height: Option<f32>,
	// Margin
	pub margin_t: Option<f32>, //|
	pub margin_b: Option<f32>, //| Not Implemented
	pub margin_l: Option<f32>, //|
	pub margin_r: Option<f32>, //|
	// Padding
	pub pad_t: Option<f32>, //|
	pub pad_b: Option<f32>, //| Text Only
	pub pad_l: Option<f32>, //|
	pub pad_r: Option<f32>, //|
	// Scrolling
	pub scroll_y: Option<f32>,
	pub scroll_x: Option<f32>, // Not Implemented
	pub overflow_y: Option<bool>,
	pub overflow_x: Option<bool>, // Not Implemented
	// Border
	pub border_size_t: Option<f32>,
	pub border_size_b: Option<f32>,
	pub border_size_l: Option<f32>,
	pub border_size_r: Option<f32>,
	pub border_color_t: Option<Color>,
	pub border_color_b: Option<Color>,
	pub border_color_l: Option<Color>,
	pub border_color_r: Option<Color>,
	// Background
	pub back_color: Option<Color>,
	pub back_image: Option<String>,
	pub back_srgb_yuv: Option<bool>,
	pub back_image_mode: Option<ImageMode>, // Not Implemented
	// Text
	pub text: String,
	pub text_size: Option<u32>,
	pub text_color: Option<Color>,
	pub text_wrap: Option<TextWrap>,
	pub text_align: Option<TextAlign>,
	// Custom Verts
	pub custom_verts: Vec<BinVert>,
}

struct ImageInfo {
	image: Option<Arc<ImageViewAccess + Send + Sync>>,
	coords: CoordsInfo,
}

pub struct Bin {
	initial: Mutex<bool>,
	style: Mutex<BinStyle>,
	update: AtomicBool,
	verts: Mutex<Vec<(Vec<ItfVertInfo>, Option<Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>>, usize)>>,
	id: u64,
	engine: Arc<Engine>,
	parent: Mutex<Option<Weak<Bin>>>,
	children: Mutex<Vec<Weak<Bin>>>,
	back_image: Mutex<Option<ImageInfo>>,
	post_update: RwLock<PostUpdate>,
	on_left_mouse_press: Mutex<Vec<OnLeftMousePress>>,
	on_update: Mutex<Vec<Arc<Fn() + Send + Sync>>>,
	on_update_once: Mutex<Vec<Arc<Fn() + Send + Sync>>>,
	kb_hook_ids: Mutex<Vec<u64>>,
	ms_hook_ids: Mutex<Vec<u64>>,
	keep_alive: Mutex<Vec<Arc<KeepAlive + Send + Sync>>>,
	last_update: Mutex<Instant>,
	pub(crate) hooks: Mutex<BTreeMap<u64, Hook>>,
	hook_counter: Mutex<u64>,
}

#[derive(Clone,Default)]
pub struct PostUpdate {
	pub tlo: [f32; 2],
	pub tli: [f32; 2],
	pub blo: [f32; 2],
	pub bli: [f32; 2],
	pub tro: [f32; 2],
	pub tri: [f32; 2],
	pub bro: [f32; 2],
	pub bri: [f32; 2],
	pub z_index: i16,
	pub pre_bound_min_y: f32,
	pub pre_bound_max_y: f32,
	pub text_overflow_y: f32,
}

#[derive(Default)]
struct DragStart {
	mouse_x: f32,
	mouse_y: f32,
	position_t: Option<f32>,
	position_b: Option<f32>,
	position_l: Option<f32>,
	position_r: Option<f32>,
}

impl Drop for Bin {
	fn drop(&mut self) {
		for hook in self.kb_hook_ids.lock().split_off(0) {
			self.engine.keyboard().delete_hook(hook);
		}
		
		for hook in self.ms_hook_ids.lock().split_off(0) {
			self.engine.mouse().delete_hook(hook);
		}
	}
}

impl Bin {
	pub(crate) fn new(id: u64, engine: Arc<Engine>) -> Arc<Self> {
		Arc::new(Bin {
			initial: Mutex::new(true),
			style: Mutex::new(BinStyle::default()),
			update: AtomicBool::new(false),
			verts: Mutex::new(Vec::new()),
			id: id,
			engine: engine.clone(),
			parent: Mutex::new(None),
			children: Mutex::new(Vec::new()),
			back_image: Mutex::new(None),
			post_update: RwLock::new(PostUpdate::default()),
			on_left_mouse_press: Mutex::new(Vec::new()),
			on_update: Mutex::new(Vec::new()),
			on_update_once: Mutex::new(Vec::new()),
			kb_hook_ids: Mutex::new(Vec::new()),
			ms_hook_ids: Mutex::new(Vec::new()),
			keep_alive: Mutex::new(Vec::new()),
			last_update: Mutex::new(Instant::now()),
			hooks: Mutex::new(BTreeMap::new()),
			hook_counter: Mutex::new(0),
		})
	}
	
	pub fn ancestors(&self) -> Vec<Arc<Bin>> {
		let mut out = Vec::new();
		let mut check_wk_op = self.parent.lock().clone();
		
		while let Some(check_wk) = check_wk_op.take() {
			if let Some(check) = check_wk.upgrade() {
				out.push(check.clone());
				check_wk_op = check.parent.lock().clone();
			}
		}
		
		out
	}
	
	pub fn add_hook_raw(self: &Arc<Self>, hook: BinHook, func: BinHookFn) -> BinHookID {
		self.engine.interface_ref().hook_manager.add_hook(self.clone(), hook, func)
	}
	
	pub fn on_key_press(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
		self.engine.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Press {
			keys: vec![key],
			mouse_buttons: Vec::new(),
		}, func)
	}
	
	pub fn on_key_release(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
		self.engine.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Release {
			keys: vec![key],
			mouse_buttons: Vec::new(),
		}, func)
	}
	
	pub fn on_key_hold(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
		self.engine.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Hold {
			keys: vec![key],
			mouse_buttons: Vec::new(),
			initial_delay: Duration::from_millis(1000),
			interval: Duration::from_millis(100),
			accel: 1.0,
		}, func)
	}
	
	pub fn on_mouse_press(self: &Arc<Self>, button: mouse::Button, func: BinHookFn) -> BinHookID {
		self.engine.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Press {
			keys: Vec::new(),
			mouse_buttons: vec![button],
		}, func)
	}
	
	pub fn on_mouse_release(self: &Arc<Self>, button: mouse::Button, func: BinHookFn) -> BinHookID {
		self.engine.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Release {
			keys: Vec::new(),
			mouse_buttons: vec![button],
		}, func)
	}
	
	pub fn on_mouse_hold(self: &Arc<Self>, button: mouse::Button, func: BinHookFn) -> BinHookID {
		self.engine.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Hold {
			keys: Vec::new(),
			mouse_buttons: vec![button],
			initial_delay: Duration::from_millis(1000),
			interval: Duration::from_millis(100),
			accel: 1.0,
		}, func)
	}
	
	pub fn add_hook(&self, mut hook: Hook) -> Vec<u64> {
		let mut counter = self.hook_counter.lock();
		let mut new_hooks = hook.add_to_engine(&self.engine, self);
		new_hooks.push(hook);
		let mut ids = Vec::new();
		let mut hooks = self.hooks.lock();
		
		for hook in new_hooks {
			let id = *counter;
			*counter += 1;
			ids.push(id);
			hooks.insert(id, hook);
		}
		
		ids
	}
	
	pub fn last_update(&self) -> Instant {
		self.last_update.lock().clone()
	}
	
	pub fn add_child(self: &Arc<Self>, child: Arc<Bin>) {
		*child.parent.lock() = Some(Arc::downgrade(self));
		self.children.lock().push(Arc::downgrade(&child));
	}
	
	pub fn add_children(self: &Arc<Self>, children: Vec<Arc<Bin>>) {
		for child in children {
			*child.parent.lock() = Some(Arc::downgrade(self));
			self.children.lock().push(Arc::downgrade(&child));
		}
	}
	
	pub fn keep_alive(&self, thing: Arc<KeepAlive + Send + Sync>) {
		self.keep_alive.lock().push(thing);
	}
	
	pub(crate) fn call_left_mouse_press(&self) {
		for func in &*self.on_left_mouse_press.lock() {
			func();
		}
	}
	
	pub fn engine(&self) -> Arc<Engine> {
		self.engine.clone()
	}
	
	pub fn engine_ref(&self) -> &Arc<Engine> {
		&self.engine
	}
	
	pub fn take_children(&self) -> Vec<Arc<Bin>> {
		self.children.lock().split_off(0).into_iter().filter_map(|child_wk| {
			match child_wk.upgrade() {
				Some(child) => {
					*child.parent.lock() = None;
					Some(child)
				}, None => None
			}
		}).collect()
	}
	
	pub fn children(&self) -> Vec<Arc<Bin>> {
		let mut out = Vec::new();
		for child in &*self.children.lock() {
			if let Some(some) = child.upgrade() {
				out.push(some);
			}
		} out
	}
	
	pub fn children_recursive(self: &Arc<Bin>) -> Vec<Arc<Bin>> {
		let mut out = Vec::new();
		let mut to_check = vec![self.clone()];
		
		while to_check.len() > 0 {
			let child = to_check.pop().unwrap();
			to_check.append(&mut child.children());
			out.push(child);
		}
		
		out
	}
	
	pub fn parent(&self) -> Option<Arc<Bin>> {
		match self.parent.lock().clone() {
			Some(some) => some.upgrade(),
			None => None
		}
	}
	
	pub fn add_select_events(self: &Arc<Self>) {
		let parent = Arc::downgrade(self);
		let show_children = AtomicBool::new(false);
		
		self.style_update(BinStyle {
			overflow_y: Some(true),
			.. self.style_copy()
		});
	
		self.engine.mouse().on_press(mouse::Button::Left, Arc::new(move |_, info| {
			let parent = match parent.upgrade() {
				Some(some) => some,
				None => return
			};
		
			if !show_children.load(atomic::Ordering::Relaxed) {
				if parent.mouse_inside(info.window_x, info.window_y) {
					show_children.store(true, atomic::Ordering::Relaxed);
				
					for child in parent.children() {
						child.hidden(Some(false));
					}
				}
			} else {
				let children = parent.children();
				
				for child in &children {
					if child.mouse_inside(info.window_x, info.window_y) {
						parent.style_update(BinStyle {
							text: child.style_copy().text,
							.. parent.style_copy()
						});
						
						break;
					}
				}
				
				show_children.store(false, atomic::Ordering::Relaxed);
			
				for child in &children {
					child.hidden(Some(true));
				}
			}
		}));
	}
	
	pub fn new_select_child<S: Into<String>>(self: &Arc<Self>, text: S) -> Arc<Bin> {
		let child = self.engine.interface_ref().new_bin();
		let mut children = self.children.lock();
		let style = self.style_copy();
		let text = text.into();
		let bps = self.post_update.read().clone();
		let mut child_height = bps.bli[1] - bps.tli[1];
		let has_parent = self.parent.lock().is_some();
		let border_size_b = style.border_size_b.unwrap_or(0.0);
		
		if child_height == 0.0 {
			child_height = match style.position_t.unwrap_or(PositionTy::FromWindow) {
				PositionTy::FromParent => match has_parent {
					true => self.pos_size_tlwh(None).3,
					false => {
						println!("UI Bin Warning! ID: {}, created a new select child \
							with a height of zero because parent has height of zero!", self.id
						); child_height
					}
				}, _ => {
					println!("UI Bin Warning! ID: {}, created a new select child with \
						a height of zero because parent has height of zero!", self.id
					); child_height
				}
			};
		}
		
		let back_color = match style.back_color {
			Some(color) => {
				let mut color = Color {
					r: color.r * 1.1,
					g: color.g * 1.1,
					b: color.b * 1.1,
					a: color.a
				};
				
				color.clamp();
				Some(color)
			}, None => None
		};
		
		let child_style = BinStyle {
			position_t: Some(PositionTy::FromParent),
			hidden: Some(true),
			pos_from_t: Some((child_height + border_size_b) * (children.len()+1) as f32),
			pos_from_l: Some(0.0),
			pos_from_r: Some(0.0),
			height: Some(child_height),
			pad_t: style.pad_t,
			pad_b: style.pad_b,
			pad_l: style.pad_l,
			pad_r: style.pad_r,
			back_color: back_color,
			text: text,
			text_size: style.text_size,
			text_color: style.text_color,
			border_size_t: None,
			border_size_b: style.border_size_b,
			border_size_l: style.border_size_l,
			border_size_r: style.border_size_r,
			border_color_t: style.border_color_t,
			border_color_b: style.border_color_b,
			border_color_l: style.border_color_l,
			border_color_r: style.border_color_r,
			.. BinStyle::default()
		};
		
		child.style_update(child_style);
		children.push(Arc::downgrade(&child));
		*child.parent.lock() = Some(Arc::downgrade(self));
		child
	}
	
	pub fn add_drag_events(self: &Arc<Self>) {
		let bin = Arc::downgrade(self);
		let mouse = self.engine.mouse();
		let drag = Arc::new(AtomicBool::new(false));
		let start = Arc::new(Mutex::new(DragStart::default()));
		
		let _bin = bin.clone();
		let _drag = drag.clone();
		let _start = start.clone();
		
		self.ms_hook_ids.lock().push(mouse.on_press(mouse::Button::Middle, Arc::new(move |engine, info| {
			let bin = match _bin.upgrade() {
				Some(some) => some,
				None => return
			};
			
			if !engine.mouse_captured() && !bin.is_hidden(None) && bin.mouse_inside(info.window_x, info.window_y) {
				let style = bin.style_copy();
				*_start.lock() = DragStart {
					mouse_x: info.window_x,
					mouse_y: info.window_y,
					position_t: style.pos_from_t,
					position_b: style.pos_from_b,
					position_l: style.pos_from_l,
					position_r: style.pos_from_r,
				}; _drag.store(true, atomic::Ordering::Relaxed);
			}
		})));
		
		let _bin = bin.clone();
		let _drag = drag.clone();
		let _start = start.clone();
		
		self.ms_hook_ids.lock().push(mouse.on_move(Arc::new(move |_, _, _, mouse_x, mouse_y| {
			let bin = match _bin.upgrade() {
				Some(some) => some,
				None => return
			};
			
			if _drag.load(atomic::Ordering::Relaxed) {
				let start = _start.lock();
				let diff_x = mouse_x - start.mouse_x;
				let diff_y = mouse_y - start.mouse_y;
				
				let t = match start.position_t {
					Some(from_t) => Some(from_t + diff_y),
					None => None
				}; let b = match start.position_b {
					Some(from_b) => Some(from_b - diff_y),
					None => None
				}; let l = match start.position_l {
					Some(from_l) => Some(from_l + diff_x),
					None => None
				}; let r = match start.position_r {
					Some(from_r) => Some(from_r - diff_x),
					None => None
				};
				
				bin.style_update(BinStyle {
					pos_from_t: t,
					pos_from_b: b,
					pos_from_l: l,
					pos_from_r: r,
					.. bin.style_copy()
				});
			}
		})));
		
		self.ms_hook_ids.lock().push(mouse.on_release(mouse::Button::Middle, Arc::new(move |_| {
			drag.store(false, atomic::Ordering::Relaxed);
		})));
	}
	
	pub fn add_enter_text_events(self: &Arc<Self>) {
		let bin_wk = Arc::downgrade(self);
		
		self.add_hook(Hook::new().char_press().func(Arc::new(move |EventInfo {
			char_ty,
			..
		}| {
			let bin = match bin_wk.upgrade() {
				Some(some) => some,
				None => return
			};
			
			let mut style = bin.style_copy();
			
			match char_ty.unwrap() {
				CharType::Backspace => { style.text.pop(); },
				CharType::Letter(c) => { style.text.push(c); }
			}
			
			bin.style_update(style);
		})));
	}
	
	pub fn add_button_fade_events(self: &Arc<Self>) {
		let bin = Arc::downgrade(self);
		let mouse = self.engine.mouse();
		let focused = Arc::new(AtomicBool::new(false));
		let _focused = focused.clone();
		let previous = Arc::new(Mutex::new(None));
		let _previous = previous.clone();
		
		self.ms_hook_ids.lock().push(mouse.on_press(mouse::Button::Left, Arc::new(move |_, info| {
			let bin = match bin.upgrade() {
				Some(some) => some,
				None => return
			};
			
			if bin.mouse_inside(info.window_x, info.window_y) {
				if !_focused.swap(true, atomic::Ordering::Relaxed) {
					let mut copy = bin.style_copy();
					*_previous.lock() = copy.opacity;
					copy.opacity = Some(0.5);
					bin.style_update(copy);
					bin.update_children();
				}
			}
		})));
		
		let bin = Arc::downgrade(self);
		
		self.ms_hook_ids.lock().push(mouse.on_release(mouse::Button::Left, Arc::new(move |_| {
			let bin = match bin.upgrade() {
				Some(some) => some,
				None => return
			};
			
			if focused.swap(false, atomic::Ordering::Relaxed) {
				let mut copy = bin.style_copy();
				copy.opacity = *previous.lock();
				bin.style_update(copy);
				bin.update_children();
			}
		})));
	}
	
	pub fn fade_out(self: &Arc<Self>, millis: u64) {
		let bin = self.clone();
		let start_opacity = self.style_copy().opacity.unwrap_or(1.0);
		let steps = (millis/10) as i64;
		let step_size = start_opacity / steps as f32;
		let mut step_i = 0;
	
		thread::spawn(move || {
			loop {
				if step_i > steps {
					break;
				}
				
				let opacity = start_opacity - (step_i as f32 * step_size);
				let mut copy = bin.style_copy();
				copy.opacity = Some(opacity);
				
				if step_i == steps {
					copy.hidden = Some(true);
				}
				
				bin.style_update(copy);
				bin.update_children();
				step_i += 1;
				thread::sleep(Duration::from_millis(10));
			}
		});
	}
	
	pub fn fade_in(self: &Arc<Self>, millis: u64, target: f32) {
		let bin = self.clone();
		let start_opacity = bin.style_copy().opacity.unwrap_or(1.0);
		let steps = (millis/10) as i64;
		let step_size = (target-start_opacity) / steps as f32;
		let mut step_i = 0;
	
		thread::spawn(move || {
			loop {
				if step_i > steps {
					break;
				}
				
				let opacity = (step_i as f32 * step_size) + start_opacity;
				let mut copy = bin.style_copy();
				copy.opacity = Some(opacity);
				copy.hidden = Some(false);
				bin.style_update(copy);
				bin.update_children();
				step_i += 1;
				thread::sleep(Duration::from_millis(10));
			}
		});
	}
	
	pub fn calc_overflow(self: &Arc<Bin>) -> f32 {
		let mut min_y = 0.0;
		let mut max_y = 0.0;
		
		for child in self.children() {
			let post = child.post_update.read();
			
			if post.pre_bound_min_y < min_y {
				min_y = post.pre_bound_min_y;
			}
			
			if post.pre_bound_max_y > max_y {
				max_y = post.pre_bound_max_y;
			}
		}
		
		let style = self.style.lock();
		let pad_t = style.pad_t.clone().unwrap_or(0.0);
		let pad_b = style.pad_b.clone().unwrap_or(0.0);
		let content_height = max_y - min_y + pad_b + pad_t;
		let self_post = self.post_update.read();
		let height = self_post.bli[1] - self_post.tli[1];
		
		if content_height > height {
			content_height - height
		} else {
			0.0
		}
	}
	
	pub fn on_update(&self, func: Arc<Fn() + Send + Sync>) {
		self.on_update.lock().push(func);
	}
	
	pub fn on_update_once(&self, func: Arc<Fn() + Send + Sync>) {
		self.on_update_once.lock().push(func);
	}
	
	pub fn wait_for_update(&self) {
		let barrier = Arc::new(Barrier::new(2));
		let barrier_copy = barrier.clone();
		
		self.on_update_once(Arc::new(move || {
			barrier_copy.wait();
		}));
		
		barrier.wait();
	}
	
	pub fn post_update(&self) -> PostUpdate {
		self.post_update.read().clone()
	}
	
	pub fn id(&self) -> u64 {
		self.id
	}
	
	#[deprecated]
	pub fn on_left_mouse_press(&self, func: OnLeftMousePress) {
		self.add_hook(Hook::new().mouse_press(mouse::Button::Left).func(Arc::new(move |_| {
			func()
		})));
		//self.on_left_mouse_press.lock().push(func);
	}
	
	pub fn mouse_inside(&self, mouse_x: f32, mouse_y: f32) -> bool {
		if self.is_hidden(None) {
			return false;
		}
		
		let post = self.post_update.read();
		
		if
			mouse_x >= post.tlo[0] && mouse_x <= post.tro[0] &&
			mouse_y >= post.tlo[1] && mouse_y <= post.blo[1]
		{
			return true;
		}
		
		false
	}

	fn pos_size_tlwh(&self, win_size_: Option<[f32; 2]>) -> (f32, f32, f32, f32) {
		let win_size = win_size_.unwrap_or([0.0, 0.0]);
		let style = self.style_copy();
		let (par_t, par_b, par_l, par_r) = match style.position_t.unwrap_or(PositionTy::FromWindow) {
			PositionTy::FromWindow => (0.0, win_size[1], 0.0, win_size[0]),
			PositionTy::FromParent => match self.parent() {
				Some(ref parent) => {
					let (top, left, width, height) = parent.pos_size_tlwh(win_size_);
					(top, top+height, left, left+width)
				}, None => (0.0, win_size[1], 0.0, win_size[0])
			}
		}; let from_t = match style.pos_from_t {
			Some(from_t) => par_t+from_t,
			None => match style.pos_from_b {
				Some(from_b) => match style.height {
					Some(height) => par_b - from_b - height,
					None => {
						println!("UI Bin Warning! ID: {}, Unable to get position \
							from top, position from bottom is specified \
							but no height was provied.", self.id
						); 0.0
					}
				}, None => {
					println!("UI Bin Warning! ID: {}, Unable to get position \
						from top, position from bottom is non specified.", self.id
					); 0.0
				}
			}
		}; let from_l = match style.pos_from_l {
			Some(from_l) => from_l+par_l,
			None => match style.pos_from_r {
				Some(from_r) => match style.width {
					Some(width) => par_r - from_r - width,
					None => {
						println!("UI Bin Warning! ID: {}, Unable to get position \
							from left, position from right is specified \
							but no width was provided.", self.id
						); 0.0
					}
				}, None => {
					println!("UI Bin Warning! ID: {}, Unable to get position from\
						left, position from right is not specified.", self.id
					); 0.0
				}
			}
		}; let width = {
			if style.pos_from_l.is_some() && style.pos_from_r.is_some() {
				par_r - style.pos_from_r.unwrap() - from_l
			} else {
				match style.width {
					Some(some) => some,
					None => {
						println!("UI Bin Warning! ID: {}, Unable to get width. Width \
							must be provided or both position from left and right \
							must be provided.", self.id
						); 0.0
					}
				}
			}
		}; let height = {
			if style.pos_from_t.is_some() && style.pos_from_b.is_some() {
				par_b - style.pos_from_b.unwrap() - from_t
			} else {
				match style.height {
					Some(some) => some,
					None => {
						println!("UI Bin Warning! ID: {}, Unable to get height. Height \
							must be provied or both position from top and bottom \
							must be provied.", self.id
						); 0.0
					}
				}
			}
		}; 
		
		(from_t, from_l, width, height)
	}
	
	pub fn visible(&self) -> bool {
		!self.is_hidden(None)
	}
	
	fn is_hidden(&self, style_: Option<&BinStyle>) -> bool {
		match match style_ {
			Some(style) => match style.hidden {
				Some(hide) => hide,
				None => false
			}, None => match self.style_copy().hidden {
				Some(hide) => hide,
				None => false
			}
		} {
			true => true,
			false => match self.parent() {
				Some(parent) => parent.is_hidden(None),
				None => false
			}
		}
	}
	
	pub(crate) fn verts_cp(&self) -> Vec<(Vec<ItfVertInfo>, Option<Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>>, usize)> {
		self.verts.lock().clone()
	}
	
	pub(crate) fn wants_update(&self) -> bool {
		self.update.load(atomic::Ordering::Relaxed)
	}
	
	pub(crate) fn do_update(self: &Arc<Self>, win_size: [f32; 2], scale: f32) {
		if *self.initial.lock() { return; }
		self.update.store(false, atomic::Ordering::Relaxed);
		let style = self.style_copy();
		let scaled_win_size = [win_size[0] / scale, win_size[1] / scale];
		
		if self.is_hidden(Some(&style)) {
			*self.verts.lock() = Vec::new();
			*self.last_update.lock() = Instant::now();
			return;
		}
		
		let ancestor_data: Vec<(Arc<Bin>, BinStyle, f32, f32, f32, f32)> = self.ancestors().into_iter().map(|bin| {
			let (top, left, width, height) = bin.pos_size_tlwh(Some(scaled_win_size));
			(
				bin.clone(),
				bin.style_copy(),
				top, left, width, height
			)
		}).collect();
	
		let (top, left, width, height) = self.pos_size_tlwh(Some(scaled_win_size));
		let border_size_t = style.border_size_t.unwrap_or(0.0);
		let border_size_b = style.border_size_b.unwrap_or(0.0);
		let border_size_l = style.border_size_l.unwrap_or(0.0);
		let border_size_r = style.border_size_r.unwrap_or(0.0);
		let mut border_color_t = style.border_color_t.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
		let mut border_color_b = style.border_color_b.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
		let mut border_color_l = style.border_color_l.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
		let mut border_color_r = style.border_color_r.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
		let mut back_color = style.back_color.unwrap_or(Color { r: 0.0, b: 0.0, g: 0.0, a: 0.0 });
		let text = style.text;
		let text_size = style.text_size.unwrap_or(10);
		let mut text_color = style.text_color.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 });
		let text_align = style.text_align.unwrap_or(TextAlign::Left);
		let pad_t = style.pad_t.unwrap_or(0.0);
		let pad_b = style.pad_b.unwrap_or(0.0);
		let pad_l = style.pad_l.unwrap_or(0.0);
		let pad_r = style.pad_r.unwrap_or(0.0);
		
		// -- z-index calc ------------------------------------------------------------- //
		
		let z_index = match style.z_index.as_ref() {
			Some(some) => *some,
			None => {
				let mut z_index_op = None;
				let mut checked = 0;
			
				for (_, check_style, _, _, _, _) in &ancestor_data {
					match check_style.z_index.as_ref() {
						Some(some) => {
							z_index_op = Some(*some + checked + 1);
							break;
						}, None => {
							checked += 1;
						}
					}
				}
				
				z_index_op.unwrap_or(ancestor_data.len() as i16)
			}
		} + style.add_z_index.clone().unwrap_or(0);
		
		// -- create post update ------------------------------------------------------- //
		
		let mut bps = PostUpdate {
			tlo: [left-border_size_l, top-border_size_t],
			tli: [left, top],
			blo: [left-border_size_l, top+height+border_size_b],
			bli: [left, top+height],
			tro: [left+width+border_size_r, top-border_size_t],
			tri: [left+width, top],
			bro: [left+width+border_size_r, top+height+border_size_b],
			bri: [left+width, top+height],
			z_index: z_index,
			pre_bound_min_y: 0.0,
			pre_bound_max_y: 0.0,
			text_overflow_y: 0.0,
		};
		
		// -- Background Image --------------------------------------------------------- //
		
		let (back_img, back_coords) = match &*self.back_image.lock() {
			&Some(ref img_info) => match &img_info.image {
				&Some(ref img) => (Some(img.clone()), img_info.coords.clone()),
				&None => (None, img_info.coords.clone())
			}, &None => match style.back_image {
				Some(path) => match self.engine.atlas_ref().coords_with_path(&path) {
					Ok(coords) => (None, coords),
					Err(e) => {
						println!("UI Bin Warning! ID: {}, failed to load image into atlas {}: {}", self.id, path, e);
						(None, atlas::CoordsInfo::none())
					}
				}, None => (None, atlas::CoordsInfo::none())
			}
		};
		
		// -- Opacity ------------------------------------------------------------------ //
		
		let mut opacity = style.opacity.unwrap_or(1.0);
		
		for (_, check_style, _, _, _, _) in &ancestor_data {
			opacity *= check_style.opacity.clone().unwrap_or(1.0);
		}
		
		if opacity != 1.0 {
			border_color_t.a *= opacity;
			border_color_b.a *= opacity;
			border_color_l.a *= opacity;
			border_color_r.a *= opacity;
			text_color.a *= opacity;
			back_color.a *= opacity;
		}
		
		// ----------------------------------------------------------------------------- //
		
		let base_z = ((-1 * z_index) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
		let content_z = ((-1 * (z_index + 1)) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
		let mut verts = Vec::with_capacity(54);
		
		if border_color_t.a > 0.0 && border_size_t > 0.0 {
			// Top Border
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tro[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tlo[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tro[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
		} if border_color_b.a > 0.0 && border_size_b > 0.0 {
			// Bottom Border
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.blo[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.blo[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bro[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
		} if border_color_l.a > 0.0 && border_size_l > 0.0 {
			// Left Border
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tlo[0], bps.tli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.blo[0], bps.bli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.blo[0], bps.bli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
		} if border_color_r.a > 0.0 && border_size_r > 0.0 {
			// Right Border
			verts.push(ItfVertInfo { position: (bps.tro[0], bps.tri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tro[0], bps.tri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bro[0], bps.bri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
		} if border_color_t.a > 0.0 && border_size_t > 0.0 && border_color_l.a > 0.0 && border_size_l > 0.0 {
			// Top Left Border Corner (Color of Left)
			verts.push(ItfVertInfo { position: (bps.tlo[0], bps.tlo[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tlo[0], bps.tli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			// Top Left Border Corner (Color of Top)
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tlo[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tlo[0], bps.tlo[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
		} if border_color_t.a > 0.0 && border_size_t > 0.0 && border_color_r.a > 0.0 && border_size_r > 0.0 {
			// Top Right Border Corner (Color of Right)
			verts.push(ItfVertInfo { position: (bps.tro[0], bps.tro[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tro[0], bps.tri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			// Top Right Border Corner (Color of Top)
			verts.push(ItfVertInfo { position: (bps.tro[0], bps.tro[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tro[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], 0.0), coords: (0.0, 0.0), color: border_color_t.as_tuple(), ty: 0 });
		} if border_color_b.a > 0.0 && border_size_b > 0.0 && border_color_l.a > 0.0 && border_size_l > 0.0 {
			// Bottom Left Border Corner (Color of Left)
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.blo[0], bps.bli[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.blo[0], bps.blo[1], 0.0), coords: (0.0, 0.0), color: border_color_l.as_tuple(), ty: 0 });
			// Bottom Left Border Corner (Color of Bottom)
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.blo[0], bps.blo[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.blo[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
		} if border_color_b.a > 0.0 && border_size_b > 0.0 && border_color_r.a > 0.0 && border_size_r > 0.0 {
			// Bottom Right Border Corner (Color of Right)
			verts.push(ItfVertInfo { position: (bps.bro[0], bps.bri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bro[0], bps.bro[1], 0.0), coords: (0.0, 0.0), color: border_color_r.as_tuple(), ty: 0 });
			// Bottom Right Border Corner (Color of Bottom)
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bro[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
			verts.push(ItfVertInfo { position: (bps.bro[0], bps.bro[1], 0.0), coords: (0.0, 0.0), color: border_color_b.as_tuple(), ty: 0 });
		} if back_color.a > 0.0 || back_coords.atlas_i != 0 || back_img.is_some() {
			// Background
			let ty = {
				if back_coords.atlas_i != 0 || back_img.is_some() {
					if style.back_srgb_yuv.unwrap_or(false) {
						3
					} else {
						2
					}
				} else {
					0
				}
			};
			
			let z = match ty {
				2 | 3 => content_z,
				_ => base_z
			};
			
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], z), coords: back_coords.f32_top_right(), color: back_color.as_tuple(), ty: ty });
			verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], z), coords: back_coords.f32_top_left(), color: back_color.as_tuple(), ty: ty });
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], z), coords: back_coords.f32_bottom_left(), color: back_color.as_tuple(), ty: ty });
			verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], z), coords: back_coords.f32_top_right(), color: back_color.as_tuple(), ty: ty });
			verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], z), coords: back_coords.f32_bottom_left(), color: back_color.as_tuple(), ty: ty });
			verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], z), coords: back_coords.f32_bottom_right(), color: back_color.as_tuple(), ty: ty });
		}
		
		for BinVert { mut position, color } in style.custom_verts {
			let z = if position.2 == 0 {
				content_z
			} else {
				((-1 * (z_index + position.2)) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32
			};
			
			verts.push(ItfVertInfo { position: (bps.tli[0] + position.0, bps.tli[1] + position.1, z), coords: (0.0, 0.0), color: color.as_tuple(), ty: 0 });
		}
		
		let mut vert_data = vec![
			(verts, back_img, back_coords.atlas_i),
		];
		
		for &mut (ref mut verts, _, _) in &mut vert_data {
			for vert in verts {
				if vert.position.2 == 0.0 {
					vert.position.2 = base_z;
				}
			}
		}
		
		match self.engine.interface_ref().text_ref().render_text(
			text, "default",
			(text_size as f32 * scale).ceil() as u32,
			text_color.as_tuple(),
			WrapTy::Normal(
				(bps.tri[0] - bps.tli[0] - pad_l - pad_r) * scale,
				(bps.bri[1] - bps.tli[1] - pad_t - pad_b) * scale,
			),
			text_align
		) {
			Ok(text_verts) => {
				//bps.text_overflow_y = ofy;
				
				for (atlas_i, mut verts) in text_verts {
					for vert in &mut verts {
						vert.position.0 /= scale;
						vert.position.1 /= scale;
						vert.position.0 += bps.tli[0] + pad_l;
						vert.position.1 += bps.tli[1] + pad_t;
						vert.position.2 = content_z;
					}
					
					vert_data.push((verts, None, atlas_i));
				}
			}, Err(e) => {
				println!("Failed to render text: {}", e);
			}
		}
		
		// -- Get current content height before overflow checks ------------------------ //
		
		for (verts, _, _) in &mut vert_data {
			for vert in verts {
				if vert.position.1 < bps.pre_bound_min_y {
					bps.pre_bound_min_y = vert.position.1;
				}
				
				if vert.position.1 > bps.pre_bound_max_y {
					bps.pre_bound_max_y = vert.position.1;
				}
			}
		}
		
		// -- Make sure that the verts are within the boundries of all ancestors. ------ //
		// TODO: Implement horizonal checks
		
		let mut cut_amt;
		let mut cut_percent;
		let mut pos_min_y;
		let mut pos_max_y;
		let mut coords_min_y;
		let mut coords_max_y;
		let mut tri_h;
		let mut img_h;
		
		for (_check_bin, check_style, check_pft, _check_pfl, _check_w, check_h) in &ancestor_data {
			let scroll_y = check_style.scroll_y.clone().unwrap_or(0.0);
			let overflow_y = check_style.overflow_y.clone().unwrap_or(false);
			let check_b = *check_pft + *check_h;
			
			if !overflow_y {	
				let bps_check_y: Vec<&mut f32> = vec![
					&mut bps.tli[1], &mut bps.tri[1],
					&mut bps.bli[1], &mut bps.bri[1],
					&mut bps.tlo[1], &mut bps.tro[1],
					&mut bps.blo[1], &mut bps.bro[1]
				];
				
				for y in bps_check_y {
					*y -= scroll_y;
				
					if *y < *check_pft {
						*y = *check_pft;
					} else if *y > check_b {
						*y = check_b;
					}
				}
			}
			
			for (verts, _, _) in &mut vert_data {
				let mut rm_tris: Vec<usize> = Vec::new();
			
				for (tri_i, tri) in verts.chunks_mut(3).enumerate() {
					tri[0].position.1 -= scroll_y;
					tri[1].position.1 -= scroll_y;
					tri[2].position.1 -= scroll_y;
					
					if !overflow_y {
						if
							(
								tri[0].position.1 < *check_pft &&
								tri[1].position.1 < *check_pft &&
								tri[2].position.1 < *check_pft
							) || (
								tri[0].position.1 > check_b &&
								tri[1].position.1 > check_b &&
								tri[2].position.1 > check_b
							)
						{
							rm_tris.push(tri_i);
						} else {
							pos_min_y = misc::partial_ord_min3(tri[0].position.1, tri[1].position.1, tri[2].position.1);
							pos_max_y = misc::partial_ord_max3(tri[0].position.1, tri[1].position.1, tri[2].position.1);
							coords_min_y = misc::partial_ord_min3(tri[0].coords.1, tri[1].coords.1, tri[2].coords.1);
							coords_max_y = misc::partial_ord_max3(tri[0].coords.1, tri[1].coords.1, tri[2].coords.1);
							tri_h = pos_max_y - pos_min_y;
							img_h = coords_max_y - coords_min_y;
							
							for vert in tri {
								if vert.position.1 < *check_pft {
									cut_amt = check_pft - vert.position.1;
									cut_percent = cut_amt / tri_h;
									vert.coords.1 += cut_percent * img_h;
									vert.position.1 += cut_amt;
								} else if vert.position.1 > check_b {
									cut_amt = vert.position.1 - check_b;
									cut_percent = cut_amt / tri_h;
									vert.coords.1 -= cut_percent * img_h;
									vert.position.1 -= cut_amt;
								}
							}
						}
					}
				}
				
				for tri_i in rm_tris.into_iter().rev() {
					for i in (0..3).into_iter().rev() {
						verts.swap_remove((tri_i * 3) + i);
					}
				}
			}
		}
		
		/*if bps.pre_bound_max_y - bps.pre_bound_min_y > bps.bli[1] - bps.tli[1] {
			println!("{} {}", bps.pre_bound_min_y, bps.pre_bound_max_y);
		}*/
		
		// ----------------------------------------------------------------------------- //
		
		for &mut (ref mut verts, _, _) in &mut vert_data {
			scale_verts(&[win_size[0], win_size[1]], scale, verts);
		}
		
		*self.verts.lock() = vert_data;
		*self.post_update.write() = bps;
		*self.last_update.lock() = Instant::now();
		
		let mut funcs = self.on_update.lock().clone();
		funcs.append(&mut self.on_update_once.lock().split_off(0));

		for func in funcs {
			func();
		}
	}
	
	pub fn style_copy(&self) -> BinStyle {
		self.style.lock().clone()
	}
	
	pub fn style_update(&self, copy: BinStyle) {
		self.update.store(true, atomic::Ordering::Relaxed);
		*self.style.lock() = copy;
		*self.initial.lock() = false;
	}
	
	pub fn update_children(&self) {
		let mut list = self.children();
		let mut i = 0;
		
		loop {
			if i >= list.len() {
				break;
			}
			
			list[i].update.store(true, atomic::Ordering::Relaxed);
			let mut childs_children = list[i].children();
			list.append(&mut childs_children);
			i += 1;
		}
	}
	
	pub fn hidden(&self, to: Option<bool>) {
		let mut copy = self.style_copy();
		copy.hidden = to;
		self.style_update(copy);
		self.update_children();
	}
	
	pub fn set_raw_back_img(&self, img: Arc<ImageViewAccess + Send + Sync>) {
		let mut coords = CoordsInfo::none();
		coords.w = 1;
		coords.h = 1;
	
		*self.back_image.lock() = Some(ImageInfo {
			image: Some(img),
			coords: coords,
		});
		
		self.update.store(true, atomic::Ordering::Relaxed);
	}
	
	pub fn set_raw_img_yuv_422(&self, width: u32, height: u32, data: Vec<u8>) -> Result<(), String> {
		use vulkano::sync::GpuFuture;
		
		let mut back_image = self.back_image.lock();
	
		let (img, future) = ImmutableImage::from_iter(
			data.into_iter(),
			vulkano::image::Dimensions::Dim2d {
				width: width,
				height: height + (height / 2),
			}, vulkano::format::Format::R8Unorm,
			self.engine.transfer_queue()
		).unwrap();
		
		let fence = future.then_signal_fence_and_flush().unwrap();
		fence.wait(None).unwrap();
		
		let mut coords = CoordsInfo::none();
		coords.w = 1;
		coords.h = 1;
		
		*back_image = Some(ImageInfo {
			image: Some(img),
			coords: coords,
		});
		
		self.update.store(true, atomic::Ordering::Relaxed);
		Ok(())
	}	
	
	pub fn separate_raw_image(&self, width: u32, height: u32, data: Vec<u8>) -> Result<(), String> {
		let img = ImmutableImage::from_iter(
			data.into_iter(),
			vulkano::image::Dimensions::Dim2d {
				width: width,
				height: height,
			}, vulkano::format::Format::R8G8B8A8Unorm,
			self.engine.graphics_queue()
		).unwrap().0;
		
		let mut coords = CoordsInfo::none();
		coords.w = 1;
		coords.h = 1;
		
		*self.back_image.lock() = Some(ImageInfo {
			image: Some(img),
			coords: coords,
		});
		
		self.update.store(true, atomic::Ordering::Relaxed);
		
		Ok(())
	}
	
	pub fn set_raw_back_data(&self, width: u32, height: u32, data: Vec<u8>) -> Result<(), String> {
		self.engine.atlas_ref().remove_raw(self.id);
	
		let coords = match self.engine.atlas_ref().load_raw(self.id, data, width, height) {
			Ok(ok) => ok,
			Err(e) => return Err(e)
		};
		
		*self.back_image.lock() = Some(ImageInfo {
			image: None,
			coords: coords
		});
		
		self.update.store(true, atomic::Ordering::Relaxed);
		Ok(())
	}
	
	pub fn remove_raw_back_img(&self) {
		*self.back_image.lock() = None;
		self.update.store(true, atomic::Ordering::Relaxed);
	}
}

#[derive(Clone,Debug)]
pub enum PositionTy {
	FromWindow,
	FromParent,
}

#[derive(Clone,Debug,PartialEq,Default)]
pub struct Color {
	pub r: f32,
	pub g: f32,
	pub b: f32,
	pub a: f32,
}

impl Color {
	pub fn as_tuple(&self) -> (f32, f32, f32, f32) {
		(self.r, self.g, self.b, self.a)
	}
	
	fn ffh(mut c1: u8, mut c2: u8) -> f32 {
		if c1 >= 97 && c1 <= 102 {
			c1 -= 87;
		} else if c1 >= 65 && c1 <= 70 {
			c1 -= 65;
		} else if c1 >= 48 && c1 <= 57 {
			c1 -= 48;
		} else {
			c1 = 0;
		} if c2 >= 97 && c2 <= 102 {
			c2 -= 87;
		} else if c2 >= 65 && c2 <= 70 {
			c2 -= 65;
		} else if c2 >= 48 && c2 <= 57 {
			c2 -= 48;
		} else {
			c2 = 0;
		} ((c1 * 16) + c2) as f32 / 255.0
	}
	
	pub fn clamp(&mut self) {
		if self.r > 1.0 {
			self.r = 1.0;
		} else if self.r < 0.0 {
			self.r = 0.0;
		} if self.g > 1.0 {
			self.g = 1.0;
		} else if self.g < 0.0 {
			self.g = 0.0;
		} if self.b > 1.0 {
			self.b = 1.0;
		} else if self.b < 0.0 {
			self.b = 0.0;
		} if self.a > 1.0 {
			self.a = 1.0;
		} else if self.a < 0.0 {
			self.a = 0.0;
		}
	}
	
	pub fn srgb_hex(code: &str) -> Self {
		let mut color = Self::from_hex(code);
		color.r = f32::powf((color.r + 0.055) / 1.055, 2.4);
		color.g = f32::powf((color.g + 0.055) / 1.055, 2.4);
		color.b = f32::powf((color.b + 0.055) / 1.055, 2.4);
		color.a = f32::powf((color.a + 0.055) / 1.055, 2.4);
		color
	}	
	
	pub fn from_hex(code: &str) -> Self {
		let mut iter = code.bytes();
		let mut red = 0.0;
		let mut green = 0.0;
		let mut blue = 0.0;
		let mut alpha = 1.0;
		
		red = match iter.next() {
			Some(c1) => match iter.next() {
				Some(c2) => {
					Self::ffh(c1, c2)
				}, None => return Color { r: red, g: green, b: blue, a: alpha }
			}, None => return Color { r: red, g: green, b: blue, a: alpha }
		}; green = match iter.next() {
			Some(c1) => match iter.next() {
				Some(c2) => {
					Self::ffh(c1, c2)
				}, None => return Color { r: red, g: green, b: blue, a: alpha }
			}, None => return Color { r: red, g: green, b: blue, a: alpha }
		}; blue = match iter.next() {
			Some(c1) => match iter.next() {
				Some(c2) => {
					Self::ffh(c1, c2)
				}, None => return Color { r: red, g: green, b: blue, a: alpha }
			}, None => return Color { r: red, g: green, b: blue, a: alpha }
		}; alpha = match iter.next() {
			Some(c1) => match iter.next() {
				Some(c2) => {
					Self::ffh(c1, c2)
				}, None => return Color { r: red, g: green, b: blue, a: alpha }
			}, None => return Color { r: red, g: green, b: blue, a: alpha }
		};
		
		Color { r: red, g: green, b: blue, a: alpha }
	}	
}

#[derive(Clone)]
pub enum ImageMode {

}
