use std::sync::atomic::{self,AtomicBool};
use super::interface::ItfVertInfo;
use interface::interface::scale_verts;
use parking_lot::{RwLock,Mutex};
use std::sync::{Weak,Arc};
use Basalt;
use vulkano;
use vulkano::image::traits::ImageViewAccess;
use super::super::atlas;
use std::thread;
use std::time::Duration;
use std::sync::Barrier;
use vulkano::image::immutable::ImmutableImage;
use std::time::Instant;
use misc;
use interface::hook::{BinHook,BinHookID,BinHookFn,BinHookData};
use std::f32::consts::PI;
use input::*;
use ilmenite::*;

pub trait KeepAlive { }
impl KeepAlive for Arc<Bin> {}
impl KeepAlive for Bin {}
impl<T: KeepAlive> KeepAlive for Vec<T> {}

#[derive(Default,Clone,Debug,PartialEq)]
pub struct BinVert {
	pub position: (f32, f32, i16),
	pub color: Color,
}

#[derive(Default,Clone,Debug)]
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
	pub pos_from_t_pct: Option<f32>,
	pub pos_from_b_pct: Option<f32>,
	pub pos_from_l_pct: Option<f32>,
	pub pos_from_r_pct: Option<f32>,
	pub pos_from_l_offset: Option<f32>,
	pub pos_from_t_offset: Option<f32>,
	// Size
	pub width: Option<f32>,
	pub width_pct: Option<f32>,
	pub height: Option<f32>,
	pub height_pct: Option<f32>,
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
	pub border_radius_tl: Option<f32>,
	pub border_radius_tr: Option<f32>,
	pub border_radius_bl: Option<f32>,
	pub border_radius_br: Option<f32>,
	// Background
	pub back_color: Option<Color>,
	pub back_image: Option<String>,
	pub back_image_url: Option<String>,
	pub back_image_atlas: Option<atlas::Coords>,
	pub back_srgb_yuv: Option<bool>,
	pub back_image_effect: Option<ImageEffect>,
	// Text
	pub text: String,
	pub text_color: Option<Color>,
	pub text_height: Option<f32>,
	pub text_wrap: Option<ImtTextWrap>,
	pub text_vert_align: Option<ImtVertAlign>,
	pub text_hori_align: Option<ImtHoriAlign>,
	pub custom_verts: Vec<BinVert>,
}

#[derive(Clone,Debug)]
pub enum ImageEffect {
	BackColorAdd,
	BackColorBehind,
	BackColorSubtract,
	BackColorMultiply,
	BackColorDivide,
	Invert,
}

impl ImageEffect {
	pub fn vert_type(&self) -> i32 {
		match self {
			&ImageEffect::BackColorAdd => 102,
			&ImageEffect::BackColorBehind => 103,
			&ImageEffect::BackColorSubtract => 104,
			&ImageEffect::BackColorMultiply => 105,
			&ImageEffect::BackColorDivide => 106,
			&ImageEffect::Invert => 107,
		}
	}
}

struct ImageInfo {
	image: Option<Arc<dyn ImageViewAccess + Send + Sync>>,
	coords: atlas::Coords,
}

pub struct Bin {
	initial: Mutex<bool>,
	style: Mutex<BinStyle>,
	update: AtomicBool,
	verts: Mutex<Vec<(Vec<ItfVertInfo>, Option<Arc<dyn vulkano::image::traits::ImageViewAccess + Send + Sync>>, u64)>>,
	id: u64,
	basalt: Arc<Basalt>,
	parent: Mutex<Option<Weak<Bin>>>,
	children: Mutex<Vec<Weak<Bin>>>,
	back_image: Mutex<Option<ImageInfo>>,
	post_update: RwLock<PostUpdate>,
	on_update: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
	on_update_once: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
	input_hook_ids: Mutex<Vec<InputHookID>>,
	keep_alive: Mutex<Vec<Arc<dyn KeepAlive + Send + Sync>>>,
	last_update: Mutex<Instant>,
	hook_ids: Mutex<Vec<BinHookID>>,
	used_by_basalt: AtomicBool,
}

#[derive(Clone,Default,Debug)]
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
}

impl Drop for Bin {
	fn drop(&mut self) {
		for hook in self.input_hook_ids.lock().split_off(0) {
			self.basalt.input_ref().remove_hook(hook);
		}
		
		self.basalt.interface_ref().hook_manager.remove_hooks(self.hook_ids.lock().split_off(0));
	}
}

impl Bin {
	pub(crate) fn new(id: u64, basalt: Arc<Basalt>) -> Arc<Self> {
		Arc::new(Bin {
			initial: Mutex::new(true),
			style: Mutex::new(BinStyle::default()),
			update: AtomicBool::new(false),
			verts: Mutex::new(Vec::new()),
			id: id,
			basalt: basalt.clone(),
			parent: Mutex::new(None),
			children: Mutex::new(Vec::new()),
			back_image: Mutex::new(None),
			post_update: RwLock::new(PostUpdate::default()),
			on_update: Mutex::new(Vec::new()),
			on_update_once: Mutex::new(Vec::new()),
			input_hook_ids: Mutex::new(Vec::new()),
			keep_alive: Mutex::new(Vec::new()),
			last_update: Mutex::new(Instant::now()),
			hook_ids: Mutex::new(Vec::new()),
			used_by_basalt: AtomicBool::new(false),
		})
	}
	
	pub fn basalt_use(&self) {
		self.used_by_basalt.store(true, atomic::Ordering::Relaxed);
	}
	
	pub fn attach_input_hook(&self, id: InputHookID) {
		self.input_hook_ids.lock().push(id);
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
		let id = self.basalt.interface_ref().hook_manager.add_hook(self.clone(), hook, func);
		self.hook_ids.lock().push(id);
		id
	}
	
	pub fn remove_hook(self: &Arc<Self>, hook_id: BinHookID) {
		self.basalt.interface_ref().hook_manager.remove_hook(hook_id);
		let mut hook_ids = self.hook_ids.lock();
		
		for i in 0..hook_ids.len() {
			if hook_ids[i] == hook_id {
				hook_ids.swap_remove(i);
				break;
			}
		}
	}
	
	pub fn on_key_press(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
		let id = self.basalt.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Press {
			keys: vec![key],
			mouse_buttons: Vec::new(),
		}, func);
		self.hook_ids.lock().push(id);
		id
	}
	
	pub fn on_key_release(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
		let id = self.basalt.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Release {
			keys: vec![key],
			mouse_buttons: Vec::new(),
		}, func);
		self.hook_ids.lock().push(id);
		id
	}
	
	pub fn on_key_hold(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
		let id = self.basalt.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Hold {
			keys: vec![key],
			mouse_buttons: Vec::new(),
			initial_delay: Duration::from_millis(1000),
			interval: Duration::from_millis(100),
			accel: 1.0,
		}, func);
		self.hook_ids.lock().push(id);
		id
	}
	
	pub fn on_mouse_press(self: &Arc<Self>, button: MouseButton, func: BinHookFn) -> BinHookID {
		let id = self.basalt.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Press {
			keys: Vec::new(),
			mouse_buttons: vec![button],
		}, func);
		self.hook_ids.lock().push(id);
		id
	}
	
	pub fn on_mouse_release(self: &Arc<Self>, button: MouseButton, func: BinHookFn) -> BinHookID {
		let id = self.basalt.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Release {
			keys: Vec::new(),
			mouse_buttons: vec![button],
		}, func);
		self.hook_ids.lock().push(id);
		id
	}
	
	pub fn on_mouse_hold(self: &Arc<Self>, button: MouseButton, func: BinHookFn) -> BinHookID {
		let id = self.basalt.interface_ref().hook_manager.add_hook(self.clone(), BinHook::Hold {
			keys: Vec::new(),
			mouse_buttons: vec![button],
			initial_delay: Duration::from_millis(1000),
			interval: Duration::from_millis(100),
			accel: 1.0,
		}, func);
		self.hook_ids.lock().push(id);
		id
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
	
	pub fn keep_alive(&self, thing: Arc<dyn KeepAlive + Send + Sync>) {
		self.keep_alive.lock().push(thing);
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
	
	pub fn add_drag_events(self: &Arc<Self>, target_op: Option<Arc<Bin>>) {
		#[derive(Default)]
		struct Data {
			target: Weak<Bin>,
			mouse_x: f32,
			mouse_y: f32,
			pos_from_t: Option<f32>,
			pos_from_b: Option<f32>,
			pos_from_l: Option<f32>,
			pos_from_r: Option<f32>,
		}
		
		let data = Arc::new(Mutex::new(None));
		let target_wk = target_op.map(|v| Arc::downgrade(&v)).unwrap_or(Arc::downgrade(self));
		let data_cp = data.clone();
		
		self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_press(MouseButton::Middle, Arc::new(move |data| {
			if let InputHookData::Press {
				mouse_x,
				mouse_y,
				..
			} = data {
				let style = match target_wk.upgrade() {
					Some(bin) => bin.style_copy(),
					None => return InputHookRes::Remove
				};
				
				*data_cp.lock() = Some(Data {
					target: target_wk.clone(),
					mouse_x: *mouse_x,
					mouse_y: *mouse_y,
					pos_from_t: style.pos_from_t,
					pos_from_b: style.pos_from_b,
					pos_from_l: style.pos_from_l,
					pos_from_r: style.pos_from_r,
				});
			}
			
			InputHookRes::Success
		})));
		
		let data_cp = data.clone();
		
		self.input_hook_ids.lock().push(self.basalt.input_ref().add_hook(InputHook::MouseMove, Arc::new(move |data| {
			if let InputHookData::MouseMove {
				mouse_x,
				mouse_y,
				..
			} = data {
				let mut data_op = data_cp.lock();
				let data = match &mut *data_op {
					Some(some) => some,
					None => return InputHookRes::Success
				};
				
				let target = match data.target.upgrade() {
					Some(some) => some,
					None => return InputHookRes::Remove
				};
				
				let dx = mouse_x - data.mouse_x;
				let dy = mouse_y - data.mouse_y;
				
				target.style_update(BinStyle {
					pos_from_t: data.pos_from_t.as_ref().map(|v| *v + dy),
					pos_from_b: data.pos_from_b.as_ref().map(|v| *v - dy),
					pos_from_l: data.pos_from_l.as_ref().map(|v| *v + dx),
					pos_from_r: data.pos_from_r.as_ref().map(|v| *v - dx),
					.. target.style_copy()
				});
				
				target.update_children();
			}
			
			InputHookRes::Success
		})));
		
		let data_cp = data.clone();
		
		self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_release(MouseButton::Middle, Arc::new(move |_| {
			*data_cp.lock() = None;
			InputHookRes::Success
		})));
	}
	
	pub fn add_enter_text_events(self: &Arc<Self>) {
		self.add_hook_raw(BinHook::Character, Arc::new(move |bin, data| {
			if let BinHookData::Character {
				char_ty,
				..
			} = data {
				let mut style = bin.style_copy();
				
				match char_ty {
					Character::Backspace => { style.text.pop(); },
					Character::Value(c) => { style.text.push(*c); }
				}
				
				bin.style_update(style);
			}
		}));
	}
	
	// TODO: Use Bin Hooks
	pub fn add_button_fade_events(self: &Arc<Self>) {
		let bin = Arc::downgrade(self);
		let focused = Arc::new(AtomicBool::new(false));
		let _focused = focused.clone();
		let previous = Arc::new(Mutex::new(None));
		let _previous = previous.clone();
		
		self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_press(MouseButton::Left, Arc::new(move |data| {
			if let InputHookData::Press {
				mouse_x,
				mouse_y,
				..
			} = data {
				let bin = match bin.upgrade() {
					Some(some) => some,
					None => return InputHookRes::Remove
				};
				
				if bin.mouse_inside(*mouse_x, *mouse_y) {
					if !_focused.swap(true, atomic::Ordering::Relaxed) {
						let mut copy = bin.style_copy();
						*_previous.lock() = copy.opacity;
						copy.opacity = Some(0.5);
						bin.style_update(copy);
						bin.update_children();
					}
				}
			}
			
			InputHookRes::Success
		})));
		
		let bin = Arc::downgrade(self);
		
		self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_release(MouseButton::Left, Arc::new(move |_| {
			let bin = match bin.upgrade() {
				Some(some) => some,
				None => return InputHookRes::Remove
			};
			
			if focused.swap(false, atomic::Ordering::Relaxed) {
				let mut copy = bin.style_copy();
				copy.opacity = *previous.lock();
				bin.style_update(copy);
				bin.update_children();
			}
			
			InputHookRes::Success
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
		
		// For some reason tli[1] doesn't need to be subtracted
		let height = self_post.bli[1];// - self_post.tli[1]; 
		
		if content_height > height {
			content_height - height
		} else {
			0.0
		}
	}
	
	pub fn on_update(&self, func: Arc<dyn Fn() + Send + Sync>) {
		self.on_update.lock().push(func);
	}
	
	pub fn on_update_once(&self, func: Arc<dyn Fn() + Send + Sync>) {
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
		}; let pos_from_t = match style.pos_from_t {
			Some(some) => Some(some),
			None => match style.pos_from_t_pct {
				Some(some) => Some((some / 100.0) * (par_b - par_t)),
				None => None
			}
		}; let pos_from_b = match style.pos_from_b {
			Some(some) => Some(some),
			None => match style.pos_from_b_pct {
				Some(some) => Some((some / 100.0) * (par_b - par_t)),
				None => None
			}
		}; let pos_from_l = match style.pos_from_l {
			Some(some) => Some(some),
			None => match style.pos_from_l_pct {
				Some(some) => Some((some / 100.0) * (par_r - par_l)),
				None => None
			}
		}; let pos_from_r = match style.pos_from_r {
			Some(some) => Some(some),
			None => match style.pos_from_r_pct {
				Some(some) => Some((some / 100.0) * (par_r - par_l)),
				None => None
			}
		};
		
		let from_t = match pos_from_t {
			Some(from_t) => par_t+from_t,
			None => match pos_from_b {
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
		} + style.pos_from_t_offset.unwrap_or(0.0);
		
		let from_l = match pos_from_l {
			Some(from_l) => from_l+par_l,
			None => match pos_from_r {
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
		} + style.pos_from_l_offset.unwrap_or(0.0);
		
		let width = {
			if pos_from_l.is_some() && pos_from_r.is_some() {
				par_r - pos_from_r.unwrap() - from_l
			} else {
				match style.width {
					Some(some) => some,
					None => match style.width_pct {
						Some(some) => (some / 100.0) * (par_r - par_l),
						None => {
							println!("UI Bin Warning! ID: {}, Unable to get width. Width \
								must be provided or both position from left and right \
								must be provided.", self.id
							); 0.0
						}
					}
				}
			}
		}; let height = {
			if pos_from_t.is_some() && pos_from_b.is_some() {
				par_b - pos_from_b.unwrap() - from_t
			} else {
				match style.height {
					Some(some) => some,
					None => match style.height_pct {
						Some(some) => (some / 100.0) * (par_b - par_t),
						None => {
							println!("UI Bin Warning! ID: {}, Unable to get height. Height \
								must be provied or both position from top and bottom \
								must be provied.", self.id
							); 0.0
						}
					}
				}
			}
		}; 
		
		(from_t, from_l, width, height)
	}
	
	pub fn visible(&self) -> bool {
		!self.is_hidden(None)
	}
	
	pub fn toggle_hidden(&self) {
		let mut style = self.style_copy();
		style.hidden = Some(!style.hidden.unwrap_or(false));
		self.style_update(style);
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
	
	pub(crate) fn verts_cp(&self) -> Vec<(Vec<ItfVertInfo>, Option<Arc<dyn vulkano::image::traits::ImageViewAccess + Send + Sync>>, u64)> {
		self.verts.lock().clone()
	}
	
	pub(crate) fn wants_update(&self) -> bool {
		self.update.load(atomic::Ordering::SeqCst)
	}
	
	pub(crate) fn do_update(self: &Arc<Self>, win_size: [f32; 2], scale: f32) {
		if *self.initial.lock() { return; }
		self.update.store(false, atomic::Ordering::SeqCst);
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
		let pad_t = style.pad_t.unwrap_or(0.0);
		let pad_b = style.pad_b.unwrap_or(0.0);
		let pad_l = style.pad_l.unwrap_or(0.0);
		let pad_r = style.pad_r.unwrap_or(0.0);
		
		// -- z-index calc ------------------------------------------------------------- //
		
		let mut z_index = match style.z_index.as_ref() {
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
		
		if self.used_by_basalt.load(atomic::Ordering::Relaxed) {
			z_index += ::std::i16::MAX - 100;
		} else if z_index >= ::std::i16::MAX - 100 {
			println!("Max z-index of {} reached!", ::std::i16::MAX - 101);
			z_index = ::std::i16::MAX - 101;
		}
		
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
		};
		
		// -- Background Image --------------------------------------------------------- //
		
		let (back_img, back_coords) = match &*self.back_image.lock() {
			&Some(ref img_info) => match &img_info.image {
				&Some(ref img) => (Some(img.clone()), img_info.coords.clone()),
				&None => (None, img_info.coords.clone())
			}, &None => match style.back_image {
				Some(path) => match self.basalt.atlas_ref().load_image_from_path(&path) {
					Ok(coords) => (None, coords),
					Err(e) => {
						println!("UI Bin Warning! ID: {}, failed to load image into atlas {}: {}", self.id, path, e);
						(None, atlas::Coords::none())
					}
				}, None => match style.back_image_url {
					Some(url) => match self.basalt.atlas_ref().load_image_from_url(&url) {
						Ok(coords) => (None, coords),
						Err(e) => {
							println!("UI Bin Warning! ID: {}, failed to load image into atlas {}: {}", self.id, url, e);
							(None, atlas::Coords::none())
						}
					}, None => match style.back_image_atlas {
						Some(coords) => (None, coords),
						None => (None, atlas::Coords::none()),
					}
				}
			}
		};
		
		let back_img_vert_ty = match style.back_srgb_yuv {
			Some(some) => match some {
				true => 101,
				false => match style.back_image_effect {
					Some(some) => some.vert_type(),
					None => 100
				}
			}, None => match style.back_image_effect {
				Some(some) => some.vert_type(),
				None => 100
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
			back_color.a *= opacity;
		}
		
		// ----------------------------------------------------------------------------- //
		
		let base_z = ((-1 * z_index) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
		let content_z = ((-1 * (z_index + 1)) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
		let mut verts = Vec::with_capacity(54);
		
		let border_radius_tl = style.border_radius_tl.unwrap_or(0.0);
		let border_radius_tr = style.border_radius_tr.unwrap_or(0.0);
		let border_radius_bl = style.border_radius_bl.unwrap_or(0.0);
		let border_radius_br = style.border_radius_br.unwrap_or(0.0);
		
		if border_radius_tl != 0.0 || border_radius_tr != 0.0 || border_radius_bl != 0.0 || border_radius_br != 0.0 {
			let border_radius_tmax = if border_radius_tl > border_radius_tr {
				border_radius_tl
			} else {
				border_radius_tr
			};
			
			let border_radius_bmax = if border_radius_bl > border_radius_br {
				border_radius_bl
			} else {
				border_radius_br
			};
			
			if back_color.a > 0.0 || back_coords.img_id != 0 || back_img.is_some() {
				let mut back_verts = Vec::new();
				
				if border_radius_tl != 0.0 || border_radius_tr != 0.0 {
					back_verts.push((bps.tri[0] - border_radius_tr, bps.tri[1]));
					back_verts.push((bps.tli[0] + border_radius_tl, bps.tli[1]));
					back_verts.push((bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tmax));
					back_verts.push((bps.tri[0] - border_radius_tr, bps.tri[1]));
					back_verts.push((bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tmax));
					back_verts.push((bps.tri[0] - border_radius_tr, bps.tri[1] + border_radius_tmax));
					
					if border_radius_tl > border_radius_tr {
						back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tr));
						back_verts.push((bps.tri[0] - border_radius_tr, bps.tri[1] + border_radius_tr));
						back_verts.push((bps.tri[0] - border_radius_tr, bps.tri[1] + border_radius_tmax));
						back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tr));
						back_verts.push((bps.tri[0] - border_radius_tr, bps.tri[1] + border_radius_tmax));
						back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tmax));
					} else if border_radius_tr > border_radius_tl {
						back_verts.push((bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tl));
						back_verts.push((bps.tli[0], bps.tli[1] + border_radius_tl));
						back_verts.push((bps.tli[0], bps.tli[1] + border_radius_tmax));
						back_verts.push((bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tl));
						back_verts.push((bps.tli[0], bps.tli[1] + border_radius_tmax));
						back_verts.push((bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tmax));
					}
				}
				
				if border_radius_bl != 0.0 || border_radius_br != 0.0 {
					back_verts.push((bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_bmax));
					back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1] - border_radius_bmax));
					back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1]));
					back_verts.push((bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_bmax));
					back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1]));
					back_verts.push((bps.bri[0] - border_radius_br, bps.bri[1]));
					
					if border_radius_bl > border_radius_br {
						back_verts.push((bps.bri[0], bps.bri[1] - border_radius_bmax));
						back_verts.push((bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_bmax));
						back_verts.push((bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_br));
						back_verts.push((bps.bri[0], bps.bri[1] - border_radius_bmax));
						back_verts.push((bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_br));
						back_verts.push((bps.bri[0], bps.bri[1] - border_radius_br));
					} else if border_radius_br > border_radius_bl {
						back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1] - border_radius_bmax));
						back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bmax));
						back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bl));
						back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1] - border_radius_bmax));
						back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bl));
						back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1] - border_radius_bl));
					}
				}
				
				if border_radius_tl != 0.0 {
					let triangles = border_radius_tl.ceil() as usize * 2;
					let step_size = (0.5 * PI) / triangles as f32;
					let base = PI / 2.0;
					let mut points = Vec::new();
					
					for i in 0..(triangles+1) {
						points.push((
							(bps.tli[0] + border_radius_tl) + (border_radius_tl * f32::cos(base + (step_size * i as f32))),
							(bps.tli[1] + border_radius_tl) - (border_radius_tl * f32::sin(base + (step_size * i as f32))),
						));
					}
					
					for i in 0..triangles {
						back_verts.push(points[i].clone());
						back_verts.push(points[i+1].clone());
						back_verts.push((bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tl));
					}
				}
				
				if border_radius_tr != 0.0 {
					let triangles = border_radius_tr.ceil() as usize * 2;
					let step_size = (0.5 * PI) / triangles as f32;
					let base = 0.0;
					let mut points = Vec::new();
					
					for i in 0..(triangles+1) {
						points.push((
							(bps.tri[0] - border_radius_tr) + (border_radius_tr * f32::cos(base + (step_size * i as f32))),
							(bps.tri[1] + border_radius_tr) - (border_radius_tr * f32::sin(base + (step_size * i as f32))),
						));
					}
					
					for i in 0..triangles {
						back_verts.push(points[i].clone());
						back_verts.push(points[i+1].clone());
						back_verts.push((bps.tri[0] - border_radius_tl, bps.tri[1] + border_radius_tr));
					}
				}
				
				if border_radius_bl != 0.0 {
					let triangles = border_radius_bl.ceil() as usize * 2;
					let step_size = (0.5 * PI) / triangles as f32;
					let base = PI;
					let mut points = Vec::new();
					
					for i in 0..(triangles+1) {
						points.push((
							(bps.bli[0] + border_radius_bl) + (border_radius_bl * f32::cos(base + (step_size * i as f32))),
							(bps.bli[1] - border_radius_bl) - (border_radius_bl * f32::sin(base + (step_size * i as f32))),
						));
					}
					
					for i in 0..triangles {
						back_verts.push(points[i].clone());
						back_verts.push(points[i+1].clone());
						back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1] - border_radius_bl));
					}
				}
				
				if border_radius_br != 0.0 {
					let triangles = border_radius_br.ceil() as usize * 2;
					let step_size = (0.5 * PI) / triangles as f32;
					let base = PI * 1.5;
					let mut points = Vec::new();
					
					for i in 0..(triangles+1) {
						points.push((
							(bps.bri[0] - border_radius_br) + (border_radius_br * f32::cos(base + (step_size * i as f32))),
							(bps.bri[1] - border_radius_br) - (border_radius_br * f32::sin(base + (step_size * i as f32))),
						));
					}
					
					for i in 0..triangles {
						back_verts.push(points[i].clone());
						back_verts.push(points[i+1].clone());
						back_verts.push((bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_br));
					}
				}
				
				back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tmax));
				back_verts.push((bps.tli[0], bps.tli[1] + border_radius_tmax));
				back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bmax));
				back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tmax));
				back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bmax));
				back_verts.push((bps.bri[0], bps.bri[1] - border_radius_bmax));
				
				let ty = if back_coords.img_id != 0 || back_img.is_some() {
					back_img_vert_ty
				} else {
					0
				};
				
				for (x, y) in back_verts {
					let coords_x = (((x - bps.tli[0]) / (bps.tri[0] - bps.tli[0])) * back_coords.w as f32) + back_coords.x as f32;
					let coords_y = (((y - bps.tli[1]) / (bps.bli[1] - bps.tli[1])) * back_coords.h as f32) + back_coords.y as f32;
					verts.push(ItfVertInfo { position: (x, y, base_z), coords: (coords_x, coords_y), color: back_color.as_tuple(), ty: ty });
				}
			}
		} else {
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
			} if back_color.a > 0.0 || back_coords.img_id != 0 || back_img.is_some() {	
				let ty = if back_coords.img_id != 0 || back_img.is_some() {
					back_img_vert_ty
				} else {
					0
				};
				
				verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], base_z), coords: back_coords.top_right(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.tli[0], bps.tli[1], base_z), coords: back_coords.top_left(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], base_z), coords: back_coords.bottom_left(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.tri[0], bps.tri[1], base_z), coords: back_coords.top_right(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.bli[0], bps.bli[1], base_z), coords: back_coords.bottom_left(), color: back_color.as_tuple(), ty: ty });
				verts.push(ItfVertInfo { position: (bps.bri[0], bps.bri[1], base_z), coords: back_coords.bottom_right(), color: back_color.as_tuple(), ty: ty });
			}
		}
		
		for BinVert { position, color } in style.custom_verts {
			let z = if position.2 == 0 {
				content_z
			} else {
				((-1 * (z_index + position.2)) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32
			};
			
			verts.push(ItfVertInfo { position: (bps.tli[0] + position.0, bps.tli[1] + position.1, z), coords: (0.0, 0.0), color: color.as_tuple(), ty: 0 });
		}
		
		let mut vert_data = vec![
			(verts, back_img, back_coords.img_id),
		];
		
		for &mut (ref mut verts, _, _) in &mut vert_data {
			for vert in verts {
				if vert.position.2 == 0.0 {
					vert.position.2 = base_z;
				}
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
	
	pub fn force_update(&self) {
		self.update.store(true, atomic::Ordering::SeqCst);
		self.basalt.interface_ref().odb.unpark();
	}
	
	pub fn style_copy(&self) -> BinStyle {
		self.style.lock().clone()
	}
	
	pub fn style_update(&self, copy: BinStyle) {
		*self.style.lock() = copy;
		*self.initial.lock() = false;
		self.update.store(true, atomic::Ordering::SeqCst);
		self.basalt.interface_ref().odb.unpark();
	}
	
	pub fn update_children(&self) {
		let mut list = self.children();
		let mut i = 0;
		
		loop {
			if i >= list.len() {
				break;
			}
			
			list[i].update.store(true, atomic::Ordering::SeqCst);
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
	
	pub fn set_raw_back_img(&self, img: Arc<dyn ImageViewAccess + Send + Sync>) {
		let mut coords = atlas::Coords::none();
		coords.w = 1;
		coords.h = 1;
	
		*self.back_image.lock() = Some(ImageInfo {
			image: Some(img),
			coords: coords,
		});
		
		self.update.store(true, atomic::Ordering::SeqCst);
		self.basalt.interface_ref().odb.unpark();
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
			self.basalt.transfer_queue()
		).unwrap();
		
		let fence = future.then_signal_fence_and_flush().unwrap();
		fence.wait(None).unwrap();
		
		let mut coords = atlas::Coords::none();
		coords.w = 1;
		coords.h = 1;
		
		*back_image = Some(ImageInfo {
			image: Some(img),
			coords: coords,
		});
		
		self.update.store(true, atomic::Ordering::SeqCst);
		self.basalt.interface_ref().odb.unpark();
		Ok(())
	}	
	
	pub fn separate_raw_image(&self, width: u32, height: u32, data: Vec<u8>) -> Result<(), String> {
		let img = ImmutableImage::from_iter(
			data.into_iter(),
			vulkano::image::Dimensions::Dim2d {
				width: width,
				height: height,
			}, vulkano::format::Format::R8G8B8A8Unorm,
			self.basalt.graphics_queue()
		).unwrap().0;
		
		let mut coords = atlas::Coords::none();
		coords.w = 1;
		coords.h = 1;
		
		*self.back_image.lock() = Some(ImageInfo {
			image: Some(img),
			coords: coords,
		});
		
		self.update.store(true, atomic::Ordering::SeqCst);
		self.basalt.interface_ref().odb.unpark();
		Ok(())
	}
	
	/*pub fn set_raw_back_data(&self, width: u32, height: u32, data: Vec<u8>) -> Result<(), String> {
		self.basalt.atlas_ref().remove_raw(self.id);
	
		let coords = match self.basalt.atlas_ref().load_raw(self.id, data, width, height) {
			Ok(ok) => ok,
			Err(e) => return Err(e)
		};
		
		*self.back_image.lock() = Some(ImageInfo {
			image: None,
			coords: coords
		});
		
		self.update.store(true, atomic::Ordering::SeqCst);
		self.basalt.interface_ref().odb.unpark();
		Ok(())
	}*/
	
	pub fn remove_raw_back_img(&self) {
		*self.back_image.lock() = None;
		self.update.store(true, atomic::Ordering::SeqCst);
		self.basalt.interface_ref().odb.unpark();
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
			c1 = c1.checked_sub(48).unwrap();
		} else {
			c1 = 0;
		} if c2 >= 97 && c2 <= 102 {
			c2 -= 87;
		} else if c2 >= 65 && c2 <= 70 {
			c2 -= 65;
		} else if c2 >= 48 && c2 <= 57 {
			c2 = c2.checked_sub(48).unwrap();
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

