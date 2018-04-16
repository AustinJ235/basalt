use std::sync::atomic::{self,AtomicBool};
use super::interface::ItfVertInfo;
use interface::interface::scale_verts;
use parking_lot::{RwLock,Mutex};
use std::sync::{Weak,Arc};
use Engine;
use super::super::mouse;
use vulkano;
use vulkano::image::traits::ImageViewAccess;
use super::super::atlas::{self,Atlas};
use super::super::keyboard::CharType;
use std::thread;
use std::time::Duration;
use interface::TextWrap;
use std::sync::Barrier;
use keyboard::CallInfo;
use atlas::CoordsInfo;
use vulkano::image::immutable::ImmutableImage;

type OnLeftMousePress = Arc<Fn() + Send + Sync>;
pub trait KeepAlive { }
impl KeepAlive for Arc<Bin> {}
impl KeepAlive for Bin {}

#[derive(Default,Clone)]
pub struct BinInner {
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
}

struct ImageInfo {
	image: Option<Arc<ImageViewAccess + Send + Sync>>,
	coords: CoordsInfo,
}

pub struct Bin {
	inner: Mutex<BinInner>,
	atlas: Arc<Atlas>,
	update: AtomicBool,
	verts: Mutex<Vec<(Vec<ItfVertInfo>, Option<Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>>, usize)>>,
	id: u64,
	engine: Arc<Engine>,
	parent: Mutex<Option<Weak<Bin>>>,
	children: Mutex<Vec<Weak<Bin>>>,
	back_image: Mutex<Option<ImageInfo>>,
	box_points: RwLock<BoxPoints>,
	on_left_mouse_press: Mutex<Vec<OnLeftMousePress>>,
	on_update: Mutex<Vec<Arc<Fn() + Send + Sync>>>,
	on_update_once: Mutex<Vec<Arc<Fn() + Send + Sync>>>,
	kb_hook_ids: Mutex<Vec<u64>>,
	ms_hook_ids: Mutex<Vec<u64>>,
	keep_alive: Mutex<Vec<Arc<KeepAlive + Send + Sync>>>,
	yuv_422_img: Mutex<YUV422ImageData>,
}

#[derive(Default)]
struct YUV422ImageData {
	width: u32,
	height: u32,
	image: Option<Arc<ImageViewAccess + Send + Sync>>,
}

#[derive(Clone,Default)]
pub struct BoxPoints {
	pub tlo: [f32; 2],
	pub tli: [f32; 2],
	pub blo: [f32; 2],
	pub bli: [f32; 2],
	pub tro: [f32; 2],
	pub tri: [f32; 2],
	pub bro: [f32; 2],
	pub bri: [f32; 2],
	pub z_index: i16,
	pub text_overflow_y: f32,
}

pub trait ArcBin {
	fn add_child(&self, child: Arc<Bin>);
	fn add_select_events(&self);
	fn new_select_child<S: Into<String>>(&self, text: S) -> Arc<Bin>;
	fn add_drag_events(&self);
	fn add_enter_text_events(&self);
	fn add_button_fade_events(&self);
	fn fade_out(&self, millis: u64);
	fn fade_in(&self, millis: u64, target: f32);
}

impl ArcBin for Arc<Bin> {
	fn add_child(&self, child: Arc<Bin>) {
		*child.parent.lock() = Some(Arc::downgrade(self));
		self.children.lock().push(Arc::downgrade(&child));
	}
	
	fn add_select_events(&self) {
		let parent = Arc::downgrade(self);
		let show_children = AtomicBool::new(false);
		
		self.inner_update(BinInner {
			overflow_y: Some(true),
			.. self.inner_copy()
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
						parent.set_text(child.inner_copy().text);
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
	
	fn new_select_child<S: Into<String>>(&self, text: S) -> Arc<Bin> {
		let itf_ = self.engine.interface();
		let child = itf_.lock().new_bin();
		let mut children = self.children.lock();
		let inner = self.inner_copy();
		let text = text.into();
		let bps = self.box_points.read().clone();
		let mut child_height = bps.bli[1] - bps.tli[1];
		let has_parent = self.parent.lock().is_some();
		let border_size_b = inner.border_size_b.unwrap_or(0.0);
		
		if child_height == 0.0 {
			child_height = match inner.position_t.unwrap_or(PositionTy::FromWindow) {
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
		
		let back_color = match inner.back_color {
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
		
		let child_inner = BinInner {
			position_t: Some(PositionTy::FromParent),
			hidden: Some(true),
			pos_from_t: Some((child_height + border_size_b) * (children.len()+1) as f32),
			pos_from_l: Some(0.0),
			pos_from_r: Some(0.0),
			height: Some(child_height),
			pad_t: inner.pad_t,
			pad_b: inner.pad_b,
			pad_l: inner.pad_l,
			pad_r: inner.pad_r,
			back_color: back_color,
			text: text,
			text_size: inner.text_size,
			text_color: inner.text_color,
			border_size_t: None,
			border_size_b: inner.border_size_b,
			border_size_l: inner.border_size_l,
			border_size_r: inner.border_size_r,
			border_color_t: inner.border_color_t,
			border_color_b: inner.border_color_b,
			border_color_l: inner.border_color_l,
			border_color_r: inner.border_color_r,
			.. BinInner::default()
		};
		
		child.inner_update(child_inner);
		children.push(Arc::downgrade(&child));
		*child.parent.lock() = Some(Arc::downgrade(self));
		child
	}
	
	fn add_drag_events(&self) {
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
				let inner = bin.inner_copy();
				*_start.lock() = DragStart {
					mouse_x: info.window_x,
					mouse_y: info.window_y,
					position_t: inner.pos_from_t,
					position_b: inner.pos_from_b,
					position_l: inner.pos_from_l,
					position_r: inner.pos_from_r,
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
				
				bin.set_position_all(t, b, l, r);
			}
		})));
		
		self.ms_hook_ids.lock().push(mouse.on_release(mouse::Button::Middle, Arc::new(move |_| {
			drag.store(false, atomic::Ordering::Relaxed);
		})));
	}
	
	fn add_enter_text_events(&self) {
		let bin = Arc::downgrade(self);
		let mouse = self.engine.mouse();
		let keyboard = self.engine.keyboard();
		let focus = Arc::new(AtomicBool::new(false));
		
		let _bin = bin.clone();
		let _focus = focus.clone();
		
		self.ms_hook_ids.lock().push(mouse.on_press(mouse::Button::Left, Arc::new(move |engine, info| {
			let bin = match _bin.upgrade() {
				Some(some) => some,
				None => return
			};
			
			if !engine.mouse_captured() {
				if !_focus.load(atomic::Ordering::Relaxed) {
					if bin.mouse_inside(info.window_x, info.window_y) {
						engine.mouse_capture(false);
						engine.allow_mouse_cap(false);
						_focus.store(true, atomic::Ordering::Relaxed);
					}
				} else {
					if bin.mouse_inside(info.window_x, info.window_y) {
						println!("Already focused you idiot!");
					} else {
						engine.allow_mouse_cap(true);
						_focus.store(false, atomic::Ordering::Relaxed);
					}
				}	
			}
		})));
		
		let _bin = bin.clone();
		let _focus = focus.clone();
		
		self.kb_hook_ids.lock().push(keyboard.on_char_press(Arc::new(move | CallInfo {char_ty, .. } | {
			let bin = match _bin.upgrade() {
				Some(some) => some,
				None => return
			};
			
			if _focus.load(atomic::Ordering::Relaxed) {
				let mut inner = bin.inner_copy();
				
				match char_ty.unwrap() {
					CharType::Backspace => { inner.text.pop(); },
					CharType::Letter(c) => { inner.text.push(c); }
				}
				
				bin.inner_update(inner);
			}
		})));
	}
	
	fn add_button_fade_events(&self) {
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
					let mut copy = bin.inner_copy();
					*_previous.lock() = copy.opacity;
					copy.opacity = Some(0.5);
					bin.inner_update(copy);
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
				let mut copy = bin.inner_copy();
				copy.opacity = *previous.lock();
				bin.inner_update(copy);
				bin.update_children();
			}
		})));
	}
	
	fn fade_out(&self, millis: u64) {
		let bin = self.clone();
		let start_opacity = self.inner_copy().opacity.unwrap_or(1.0);
		let steps = (millis/10) as i64;
		let step_size = start_opacity / steps as f32;
		let mut step_i = 0;
	
		thread::spawn(move || {
			loop {
				if step_i > steps {
					break;
				}
				
				let opacity = start_opacity - (step_i as f32 * step_size);
				let mut copy = bin.inner_copy();
				copy.opacity = Some(opacity);
				
				if step_i == steps {
					copy.hidden = Some(true);
				}
				
				bin.inner_update(copy);
				bin.update_children();
				step_i += 1;
				thread::sleep(Duration::from_millis(10));
			}
		});
	}
	
	fn fade_in(&self, millis: u64, target: f32) {
		let bin = self.clone();
		let start_opacity = bin.inner_copy().opacity.unwrap_or(1.0);
		let steps = (millis/10) as i64;
		let step_size = (target-start_opacity) / steps as f32;
		let mut step_i = 0;
	
		thread::spawn(move || {
			loop {
				if step_i > steps {
					break;
				}
				
				let opacity = (step_i as f32 * step_size) + start_opacity;
				let mut copy = bin.inner_copy();
				copy.opacity = Some(opacity);
				copy.hidden = Some(false);
				bin.inner_update(copy);
				bin.update_children();
				step_i += 1;
				thread::sleep(Duration::from_millis(10));
			}
		});
	}
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
	pub(crate) fn new(id: u64, engine: Arc<Engine>, atlas: Arc<Atlas>) -> Arc<Self> {
		Arc::new(Bin {
			atlas: atlas,
			inner: Mutex::new(BinInner::default()),
			update: AtomicBool::new(false),
			verts: Mutex::new(Vec::new()),
			id: id,
			engine: engine.clone(),
			parent: Mutex::new(None),
			children: Mutex::new(Vec::new()),
			back_image: Mutex::new(None),
			box_points: RwLock::new(BoxPoints::default()),
			on_left_mouse_press: Mutex::new(Vec::new()),
			on_update: Mutex::new(Vec::new()),
			on_update_once: Mutex::new(Vec::new()),
			kb_hook_ids: Mutex::new(Vec::new()),
			ms_hook_ids: Mutex::new(Vec::new()),
			keep_alive: Mutex::new(Vec::new()),
			yuv_422_img: Mutex::new(YUV422ImageData::default()),
		})
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
	
	pub fn calc_overflow(&self) -> f32 {
		let bps = self.box_points.read().clone();
		let pad_b = self.inner_copy().pad_b.unwrap_or(0.0);
		let mut c_max_y = bps.text_overflow_y + pad_b;
		
		for child in self.children() {
			let c_bps = child.box_points();
			
			if c_bps.bli[1] > c_max_y {
				c_max_y = c_bps.bli[1];
			}
		}
		
		c_max_y += pad_b;
		
		if c_max_y < bps.bli[1] {
			0.0
		} else {
			c_max_y - bps.bli[1]
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
	
	pub fn box_points(&self) -> BoxPoints {
		self.box_points.read().clone()
	}
	
	// Useful in cases where it is best for the parent to not be aware of its children
	pub fn set_parent(&self, parent: Option<Arc<Bin>>) {
		*self.parent.lock() = match parent {
			Some(some) => Some(Arc::downgrade(&some)),
			None => None
		};
	}
	
	pub fn id(&self) -> u64 {
		self.id
	}
	
	pub fn on_left_mouse_press(&self, func: OnLeftMousePress) {
		self.on_left_mouse_press.lock().push(func);
	}
	
	pub fn mouse_inside(&self, mouse_x: f32, mouse_y: f32) -> bool {
		let points = self.box_points.read().clone();
		
		let mut to_check_ = self.parent();
		let mut scroll_y = 0.0;
		
		while let Some(to_check) = to_check_ {
			scroll_y += to_check.inner_copy().scroll_y.unwrap_or(0.0);
			to_check_ = to_check.parent();
		}
		
		if self.is_hidden(None) {
			false
		} else if
			(mouse_x as f32) >= points.tlo[0] &&
			(mouse_x as f32) <= points.tro[0] &&
			(mouse_y as f32 + scroll_y) >= points.tlo[1] &&
			(mouse_y as f32 + scroll_y) <= points.blo[1]
		{
			true
		} else {
			false
		}
	}

	fn pos_size_tlwh(&self, win_size_: Option<[f32; 2]>) -> (f32, f32, f32, f32) {
		let win_size = win_size_.unwrap_or([0.0, 0.0]);
		let inner = self.inner_copy();
		let (par_t, par_b, par_l, par_r) = match inner.position_t.unwrap_or(PositionTy::FromWindow) {
			PositionTy::FromWindow => (0.0, win_size[1], 0.0, win_size[0]),
			PositionTy::FromParent => match self.parent() {
				Some(ref parent) => {
					let (top, left, width, height) = parent.pos_size_tlwh(win_size_);
					(top, top+height, left, left+width)
				}, None => (0.0, win_size[1], 0.0, win_size[0])
			}
		}; let from_t = match inner.pos_from_t {
			Some(from_t) => par_t+from_t,
			None => match inner.pos_from_b {
				Some(from_b) => match inner.height {
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
		}; let from_l = match inner.pos_from_l {
			Some(from_l) => from_l+par_l,
			None => match inner.pos_from_r {
				Some(from_r) => match inner.width {
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
			if inner.pos_from_l.is_some() && inner.pos_from_r.is_some() {
				par_r - inner.pos_from_r.unwrap() - from_l
			} else {
				match inner.width {
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
			if inner.pos_from_t.is_some() && inner.pos_from_b.is_some() {
				par_b - inner.pos_from_b.unwrap() - from_t
			} else {
				match inner.height {
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
	
	fn is_hidden(&self, inner_: Option<&BinInner>) -> bool {
		match match inner_ {
			Some(inner) => match inner.hidden {
				Some(hide) => hide,
				None => false
			}, None => match self.inner_copy().hidden {
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
	
	pub(crate) fn verts(&self, win_size: [f32; 2], resized: bool)
		-> (Vec<(Vec<ItfVertInfo>, Option<Arc<vulkano::image::traits::ImageViewAccess + Send + Sync>>, usize)>, bool)
	{
		if self.update.swap(false, atomic::Ordering::Relaxed) || resized {
			let inner = self.inner_copy();
			
			if self.is_hidden(Some(&inner)) {
				*self.verts.lock() = Vec::new();
				return (Vec::new(), true);
			}
		
			let (top, left, width, height) = self.pos_size_tlwh(Some(win_size));
			let border_size_t = inner.border_size_t.unwrap_or(0.0);
			let border_size_b = inner.border_size_b.unwrap_or(0.0);
			let border_size_l = inner.border_size_l.unwrap_or(0.0);
			let border_size_r = inner.border_size_r.unwrap_or(0.0);
			let mut border_color_t = inner.border_color_t.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
			let mut border_color_b = inner.border_color_b.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
			let mut border_color_l = inner.border_color_l.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
			let mut border_color_r = inner.border_color_r.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });

			let text = inner.text;
			let text_size = inner.text_size.unwrap_or(10);
			let mut text_color = inner.text_color.unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 });
			let text_wrap = inner.text_wrap.unwrap_or(TextWrap::NewLine);
			let pad_t = inner.pad_t.unwrap_or(0.0);
			let pad_b = inner.pad_b.unwrap_or(0.0);
			let pad_l = inner.pad_l.unwrap_or(0.0);
			let pad_r = inner.pad_r.unwrap_or(0.0);
			
			let z_index_op = inner.z_index;
			let mut z_index = || -> _ {
				if let Some(index) = z_index_op {
					return index;
				}
			
				let mut hierarchy = Vec::new();
				let mut check = self.parent();
				
				loop {
					match check {
						Some(up) => {
							check = up.parent();
							hierarchy.push(up);
						}, None => break
					}
				}
				
				let mut checked = 0;
				
				for bin in hierarchy.iter() {
					match bin.inner_copy().z_index {
						Some(some) => { return some + checked + 1; },
						None => { checked += 1; }
					}
				}
				
				hierarchy.len() as i16
			}();
			
			z_index += inner.add_z_index.unwrap_or(0);
			
			let mut bps = BoxPoints {
				tlo: [left-border_size_l, top-border_size_t],
				tli: [left, top],
				blo: [left-border_size_l, top+height+border_size_b],
				bli: [left, top+height],
				tro: [left+width+border_size_r, top-border_size_t],
				tri: [left+width, top],
				bro: [left+width+border_size_r, top+height+border_size_b],
				bri: [left+width, top+height],
				z_index: z_index,
				text_overflow_y: 0.0,
			};
			
			let mut verts = Vec::with_capacity(54);
			
			let (back_img, back_coords) = match &*self.back_image.lock() {
				&Some(ref img_info) => match &img_info.image {
					&Some(ref img) => (Some(img.clone()), img_info.coords.clone()),
					&None => (None, img_info.coords.clone())
				}, &None => match inner.back_image {
					Some(path) => match self.atlas.coords_with_path(&path) {
						Ok(coords) => (None, coords),
						Err(e) => {
							println!("UI Bin Warning! ID: {}, failed to load image into atlas {}: {}", self.id, path, e);
							(None, atlas::CoordsInfo::none())
						}
					}, None => (None, atlas::CoordsInfo::none())
				}
			};
			
			let mut back_color = inner.back_color.unwrap_or(Color { r: 0.0, b: 0.0, g: 0.0, a: 0.0 });
			
			let opacity = {
				let mut opacity = inner.opacity.unwrap_or(1.0);
				let mut check = self.parent();
				
				loop {
					if check.is_some() {
						let to_check = check.unwrap();
						opacity *= to_check.inner_copy().opacity.unwrap_or(1.0);
						check = to_check.parent();
					} else {
						break;
					}
				}
				
				opacity
			};
			
			if opacity != 1.0 {
				border_color_t.a *= opacity;
				border_color_b.a *= opacity;
				border_color_l.a *= opacity;
				border_color_r.a *= opacity;
				text_color.a *= opacity;
				back_color.a *= opacity;
			}
			
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
						if inner.back_srgb_yuv.unwrap_or(false) {
							3
						} else {
							2
						}
					} else {
						0
					}
				};
				
				verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], 0.0), coords: back_coords.f32_top_right(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], 0.0), coords: back_coords.f32_top_left(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], 0.0), coords: back_coords.f32_bottom_left(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], 0.0), coords: back_coords.f32_top_right(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], 0.0), coords: back_coords.f32_bottom_left(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], 0.0), coords: back_coords.f32_bottom_right(), color: back_color.as_tuple(), ty: ty });
			}
			
			let mut vert_data = vec![
				(verts, back_img, back_coords.atlas_i),
			];
			
			let base_z = ((-1 * z_index) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
			let text_z = ((-1 * (z_index + 1)) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
			
			for &mut (ref mut verts, _, _) in &mut vert_data {
				for vert in verts {
					vert.position.2 = base_z;
				}
			}
			
			match self.atlas.text_verts(
				text_size as f32,
				[bps.tli[0]+pad_l, bps.tli[1]+pad_t], 
				Some([bps.bri[0]-pad_r, bps.bri[1]-pad_b]),
				text_wrap,
				text_color.as_tuple(),
				text
			) {
				Ok((ok, text_overflow_y)) => {
					bps.text_overflow_y = text_overflow_y;
					
					for (atlas_i, mut verts) in ok {
						for vert in &mut verts {
							vert.position.2 = text_z;
						}
						
						vert_data.push((verts, None, atlas_i));
					}
				}, Err(e) => {
					println!("Failed to get text verts: {}", e);
				}
			}
			
			let mut to_check_ = self.parent();
			let mut overflow_height = 0.0;
			
			while let Some(to_check) = to_check_ {
				let (top, _, _, height) = to_check.pos_size_tlwh(Some(win_size));
				let check_inner = to_check.inner_copy();
				let scroll_y = check_inner.scroll_y.unwrap_or(0.0);
				let overflow_y = check_inner.overflow_y.unwrap_or(false);
				let mut max_cut = 0.0;
				
				for &mut (ref mut verts, _, _) in &mut vert_data {
					for vert in verts {
						vert.position.1 -= scroll_y;
						
						if vert.position.1 > overflow_height {
							overflow_height = vert.position.1;
						}
						
						if !overflow_y {
							if vert.position.1 < top {
								vert.position.1 = top;
							} else if vert.position.1 > top + height {
								if vert.position.1 - top + height > max_cut {
									max_cut = vert.position.1 - top + height;
								}
								
								vert.position.1 = top + height;
							}
						}
					}
				}
				
				to_check_ = to_check.parent();
			}
			
			for &(ref verts, _, _) in &vert_data {
				for vert in verts {
					if vert.position.1 > overflow_height {
						overflow_height = vert.position.1;
					}
				}
			}
			
			for &mut (ref mut verts, _, _) in &mut vert_data {
				scale_verts(&[win_size[0] , win_size[1] ], verts);
			}
			
			*self.verts.lock() = vert_data.clone();
			*self.box_points.write() = bps;
			let mut funcs = self.on_update.lock().clone();
			funcs.append(&mut self.on_update_once.lock().split_off(0));
			
			thread::spawn(move || {
				for func in funcs {
					func();
				}
			});
			
			(vert_data, true)
		} else {
			(self.verts.lock().clone(), false)
		}
	}
	
	pub fn inner_copy(&self) -> BinInner {
		self.inner.lock().clone()
	} pub fn inner_update(&self, copy: BinInner) {
		self.update.store(true, atomic::Ordering::Relaxed);
		*self.inner.lock() = copy;
	}
	
	pub fn set_position_ty(&self, t: Option<PositionTy>) {
		let mut copy = self.inner_copy();
		copy.position_t = t;
		self.inner_update(copy);
		
		for child in &*self.children() {
			child.update.store(true, atomic::Ordering::Relaxed);
		}
	}

	pub fn set_position_all(&self, t: Option<f32>, b: Option<f32>, l: Option<f32>, r: Option<f32>) {
		let mut copy = self.inner_copy();
		copy.pos_from_t = t;
		copy.pos_from_b = b;
		copy.pos_from_l = l;
		copy.pos_from_r = r;
		self.inner_update(copy);
		self.update_children();
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
	
	pub fn get_position_all(&self) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
		let copy = self.inner_copy();
		(copy.pos_from_t, copy.pos_from_b, copy.pos_from_l, copy.pos_from_r)
	} pub fn set_size(&self, w: Option<f32>, h: Option<f32>) {
		let mut copy = self.inner_copy();
		copy.width = w;
		copy.height = h;
		self.inner_update(copy);
	} pub fn get_size(&self) -> (Option<f32>, Option<f32>) {
		let copy = self.inner_copy();
		(copy.width, copy.height)
	} pub fn set_margin_all(&self, t: Option<f32>, b: Option<f32>, l: Option<f32>, r: Option<f32>) {
		let mut copy = self.inner_copy();
		copy.margin_t = t;
		copy.margin_b = b;
		copy.margin_l = l;
		copy.margin_r = r;
		self.inner_update(copy);
	} pub fn get_margin_all(&self) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
		let copy = self.inner_copy();
		(copy.margin_t, copy.margin_b, copy.margin_l, copy.margin_r)
	} pub fn set_padding_all(&self, t: Option<f32>, b: Option<f32>, l: Option<f32>, r: Option<f32>) {
		let mut copy = self.inner_copy();
		copy.pad_t = t;
		copy.pad_b = b;
		copy.pad_l = l;
		copy.pad_r = r;
		self.inner_update(copy);
	} pub fn get_padding_all(&self) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
		let copy = self.inner_copy();
		(copy.pad_t, copy.pad_b, copy.pad_l, copy.pad_r)
	} pub fn set_border_size_all(&self, t: Option<f32>, b: Option<f32>, l: Option<f32>, r: Option<f32>) {
		let mut copy = self.inner_copy();
		copy.border_size_t = t;
		copy.border_size_b = b;
		copy.border_size_l = l;
		copy.border_size_r = r;
		self.inner_update(copy);
	} pub fn get_border_size_all(&self) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
		let copy = self.inner_copy();
		(copy.border_size_t, copy.border_size_b, copy.border_size_l, copy.border_size_r)
	} pub fn set_border_color_all(&self, t: Option<Color>, b: Option<Color>, l: Option<Color>, r: Option<Color>) {
		let mut copy = self.inner_copy();
		copy.border_color_t = t;
		copy.border_color_b = b;
		copy.border_color_l = l;
		copy.border_color_r = r;
		self.inner_update(copy);
	} pub fn get_border_color_all(&self) -> (Option<Color>, Option<Color>, Option<Color>, Option<Color>) {
		let copy = self.inner_copy();
		(copy.border_color_t, copy.border_color_b, copy.border_color_l, copy.border_color_r)
	} pub fn set_back_color(&self, c: Option<Color>) {
		let mut copy = self.inner_copy();
		copy.back_color = c;
		self.inner_update(copy);
	} pub fn get_back_color(&self) -> Option<Color> {
		let copy = self.inner_copy();
		copy.back_color
	} pub fn set_text(&self, text: String) {
		let mut copy = self.inner_copy();
		copy.text = text;
		self.inner_update(copy);
	} pub fn set_text_size(&self, size: Option<u32>) {
		let mut copy = self.inner_copy();
		copy.text_size = size;
		self.inner_update(copy);
	} pub fn set_text_wrap(&self, wrap: Option<TextWrap>) {
		let mut copy = self.inner_copy();
		copy.text_wrap = wrap;
		self.inner_update(copy);
	} pub fn set_text_color(&self, color: Option<Color>) {
		let mut copy = self.inner_copy();
		copy.text_color = color;
		self.inner_update(copy);
	} pub fn set_back_image(&self, image_path: Option<String>) {
		let mut copy = self.inner_copy();
		copy.back_image = image_path;
		self.inner_update(copy);
	}  pub fn set_border_size(&self, t: Option<f32>) {
		self.set_border_size_all(t.clone(), t.clone(), t.clone(), t);
	}  pub fn set_border_color(&self, t: Option<Color>) {
		self.set_border_color_all(t.clone(), t.clone(), t.clone(), t);
	} pub fn hidden(&self, to: Option<bool>) {
		let mut copy = self.inner_copy();
		copy.hidden = to;
		self.inner_update(copy);
		self.update_children();
	}
	
	pub fn set_raw_back_img(&self, img: Arc<ImageViewAccess + Send + Sync>) {
		*self.back_image.lock() = Some(ImageInfo {
			image: Some(img),
			coords: CoordsInfo::none()
		});
		
		self.update.store(true, atomic::Ordering::Relaxed);
	}
	
	pub fn set_raw_img_yuv_422(&self, width: u32, height: u32, data: Vec<u8>) -> Result<(), String> {
		let img = ImmutableImage::from_iter(
			data.into_iter(),
			vulkano::image::Dimensions::Dim2d {
				width: width,
				height: height + (height / 2),
			}, vulkano::format::Format::R8Unorm,
			self.engine.transfer_queue()
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
		self.atlas.remove_raw(self.id);
	
		let coords = match self.atlas.load_raw(self.id, data, width, height) {
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

#[derive(Clone)]
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
