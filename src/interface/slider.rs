use std::sync::Arc;
use super::bin::{KeepAlive,Bin,BinInner,PositionTy,Color};
use mouse;
use std::sync::atomic::{self,AtomicBool};
use parking_lot::Mutex;
use super::super::keyboard;
use Engine;
use interface::TextWrap;
use std::thread;
use super::interface::Interface;

impl KeepAlive for Slider {}

pub struct Slider {
	pub engine: Arc<Engine>,
	pub container: Arc<Bin>,
	pub slidy_bit: Arc<Bin>,
	pub input_box: Arc<Bin>,
	pub slide_back: Arc<Bin>,
	data: Mutex<Data>,
	on_change: Mutex<Vec<Arc<Fn(f32) + Send + Sync>>>,
	hooks: Mutex<Hooks>,
}

#[derive(Default)]
struct Hooks {
	ms: Vec<u64>,
	kb: Vec<u64>,
}

struct Data {
	min: f32,
	max: f32,
	at: f32,
	step: f32,
	method: Method,
}

impl Data {
	fn apply_method(&mut self) {
		match self.method {
			Method::Float => return,
			Method::RoundToStep => {
				self.at -= self.min;
				self.at /= self.step;
				self.at = f32::round(self.at);
				self.at *= self.step;
				self.at += self.min;
			}, Method::RoundToInt => {
				self.at = f32::round(self.at);
			}
		} if self.at > self.max {
			self.at = self.max;
		} else if self.at < self.min {
			self.at = self.min;
		}
	}
}

pub enum Method {
	Float,
	RoundToStep,
	RoundToInt,
}

impl Drop for Slider {
	fn drop(&mut self) {
		let mut hooks = self.hooks.lock();
		
		for id in hooks.ms.split_off(0) {
			self.engine.mouse().delete_hook(id);
		}
		
		for id in hooks.kb.split_off(0) {
			self.engine.keyboard().delete_hook(id);
		}
	}
}

impl Slider {
	pub fn set_min_max(&self, min: f32, max: f32) {
		let mut data = self.data.lock();
		data.min = min;
		data.max = max;
	}
	
	pub fn min_max(&self) -> (f32, f32) {
		let data = self.data.lock();
		(data.min, data.max)
	}
	
	pub fn at(&self) -> f32 {
		self.data.lock().at
	} pub fn set_step_size(&self, size: f32) {
		self.data.lock().step = size;
	} pub fn on_change(&self, func: Arc<Fn(f32) + Send + Sync>) {
		self.on_change.lock().push(func);
	} pub fn set_method(&self, method: Method) {
		self.data.lock().method = method;
	}

	pub fn new(engine: Arc<Engine>, interface_op: Option<&mut Interface>, parent_: Option<Arc<Bin>>) -> Arc<Slider> {
		let mut bins: Vec<_> = match interface_op {
			Some(mut itf) => { (0..4).into_iter().map(|_| itf.new_bin()).collect() },
			None => {
				let itf_ = engine.interface();
				let mut itf = itf_.lock();
				(0..4).into_iter().map(|_| itf.new_bin()).collect()
			}
		}; bins.reverse();
	
		let slider = Arc::new(Slider {
			engine: engine.clone(),
			container: bins.pop().unwrap(),
			slide_back: bins.pop().unwrap(),
			slidy_bit: bins.pop().unwrap(),
			input_box: bins.pop().unwrap(),
			data: Mutex::new(Data {
				min: 0.0,
				max: 100.0,
				at: 0.0,
				step: 10.0,
				method: Method::Float,
			}), on_change: Mutex::new(Vec::new()),
			hooks: Mutex::new(Hooks::default()),
		});
		
		if let Some(parent) = parent_ {
			parent.add_child(slider.container.clone());
		}
		
		slider.container.add_child(slider.slidy_bit.clone());
		slider.container.add_child(slider.input_box.clone());
		slider.container.add_child(slider.slide_back.clone());
		
		slider.container.inner_update(BinInner {
			position_t: Some(PositionTy::FromParent),
			.. BinInner::default()
		});
		
		slider.slidy_bit.inner_update(BinInner {
			position_t: Some(PositionTy::FromParent),
			add_z_index: Some(1),
			pos_from_l: Some(30.0),
			pos_from_t: Some(10.0),
			pos_from_b: Some(10.0),
			width: Some(10.0),
			border_size_t: Some(1.0),
			border_size_b: Some(1.0),
			border_size_l: Some(1.0),
			border_size_r: Some(1.0),
			border_color_t: Some(Color::from_hex("808080")),
			border_color_b: Some(Color::from_hex("808080")),
			border_color_l: Some(Color::from_hex("808080")),
			border_color_r: Some(Color::from_hex("808080")),
			back_color: Some(Color::from_hex("f8f8f8")),
			.. BinInner::default()
		});
		
		slider.input_box.inner_update(BinInner {
			position_t: Some(PositionTy::FromParent),
			pos_from_t: Some(1.0),
			pos_from_b: Some(1.0),
			pos_from_r: Some(0.0),
			pad_l: Some(5.0),
			text_size: Some(14),
			width: Some(60.0),
			border_size_t: Some(1.0),
			border_size_b: Some(1.0),
			border_size_l: Some(1.0),
			border_size_r: Some(1.0),
			border_color_t: Some(Color::from_hex("808080")),
			border_color_b: Some(Color::from_hex("808080")),
			border_color_l: Some(Color::from_hex("808080")),
			border_color_r: Some(Color::from_hex("808080")),
			back_color: Some(Color::from_hex("f8f8f8")),
			text_wrap: Some(TextWrap::None),
			.. BinInner::default()
		});
		
		slider.slide_back.inner_update(BinInner {
			position_t: Some(PositionTy::FromParent),
			pos_from_t: Some(13.0),
			pos_from_b: Some(13.0),
			pos_from_l: Some(0.0),
			pos_from_r: Some(70.0),
			border_size_t: Some(1.0),
			border_size_b: Some(1.0),
			border_size_l: Some(1.0),
			border_size_r: Some(1.0),
			border_color_t: Some(Color::from_hex("f8f8f8")),
			border_color_b: Some(Color::from_hex("f8f8f8")),
			border_color_l: Some(Color::from_hex("f8f8f8")),
			border_color_r: Some(Color::from_hex("f8f8f8")),
			back_color: Some(Color::from_hex("808080")),
			.. BinInner::default()
		});
		
		let _slider = Arc::downgrade(&slider);
		
		slider.slide_back.on_update(Arc::new(move || {
			let _slider = match _slider.upgrade() {
				Some(some) => some,
				None => return
			};
		
			_slider.force_update(None);
		}));
		
		let sliding = Arc::new(AtomicBool::new(false));
		let focused = Arc::new(AtomicBool::new(false));
		let _slider = Arc::downgrade(&slider);
		let _sliding = sliding.clone();
		let _focused = focused.clone();
		
		{
			let mut hooks = slider.hooks.lock();

			hooks.ms.push(engine.mouse().on_press(mouse::Button::Left, Arc::new(move |_, info| {
				let _slider = match _slider.upgrade() {
					Some(some) => some,
					None => return
				};
			
				if _slider.slidy_bit.mouse_inside(info.window_x, info.window_y) {
					_sliding.store(true, atomic::Ordering::Relaxed);
				} if _slider.container.mouse_inside(info.window_x, info.window_y) {
					_focused.store(true, atomic::Ordering::Relaxed);
				} else {
					_focused.store(false, atomic::Ordering::Relaxed);
				}
			})));
		
			let _slider = Arc::downgrade(&slider);
		
			hooks.ms.push(engine.mouse().on_scroll(Arc::new(move |_, x, y, amt| {
				let _slider = match _slider.upgrade() {
					Some(some) => some,
					None => return
				};
			
				if _slider.container.mouse_inside(x, y) {
					if amt > 0.0 {
						_slider.increment();
					} else {
						_slider.decrement();
					}
				}
			})));
			
			let _focused = focused.clone();
			let _slider = Arc::downgrade(&slider);
		
			hooks.kb.push(engine.keyboard().on_press(vec![vec![keyboard::Qwery::ArrowRight]], Arc::new(move |_| {
				let _slider = match _slider.upgrade() {
					Some(some) => some,
					None => return
				};
			
				if _focused.load(atomic::Ordering::Relaxed) {
					_slider.increment();
				}
			})));
		
			let _focused = focused.clone();
			let _slider = Arc::downgrade(&slider);
		
			hooks.kb.push(engine.keyboard().on_press(vec![vec![keyboard::Qwery::ArrowLeft]], Arc::new(move |_| {
				let _slider = match _slider.upgrade() {
					Some(some) => some,
					None => return
				};
			
				if _focused.load(atomic::Ordering::Relaxed) {
					_slider.decrement();
				}
			})));
		
			let _focused = focused.clone();
			let _slider = Arc::downgrade(&slider);
		
			hooks.kb.push(engine.keyboard().on_press_and_hold(vec![vec![keyboard::Qwery::ArrowRight]], 150, Arc::new(move |_| {
				let _slider = match _slider.upgrade() {
					Some(some) => some,
					None => return
				};
			
				if _focused.load(atomic::Ordering::Relaxed) {
					_slider.increment();
				}
			})));
		
			let _focused = focused.clone();
			let _slider = Arc::downgrade(&slider);
		
			hooks.kb.push(engine.keyboard().on_press_and_hold(vec![vec![keyboard::Qwery::ArrowLeft]], 150, Arc::new(move |_| {
				let _slider = match _slider.upgrade() {
					Some(some) => some,
					None => return
				};
			
				if _focused.load(atomic::Ordering::Relaxed) {
					_slider.decrement();
				}
			})));
		
			let _sliding = sliding.clone();
			let _slider = Arc::downgrade(&slider);
		
			hooks.ms.push(engine.mouse().on_move(Arc::new(move |_, _, _, mouse_x, _| {
				let _slider = match _slider.upgrade() {
					Some(some) => some,
					None => return
				};
			
				if _sliding.load(atomic::Ordering::Relaxed) {
					let back_bps = _slider.slide_back.box_points();
					let back_width = back_bps.tro[0] - back_bps.tlo[0];
					let sbit_inner = _slider.slidy_bit.inner_copy();
					let sbit_width = sbit_inner.width.unwrap_or(0.0);
					let sbit_bordl = sbit_inner.border_size_l.unwrap_or(0.0);
					let sbit_bordr = sbit_inner.border_size_r.unwrap_or(0.0);
					let mut from_l = mouse_x - back_bps.tlo[0] - (sbit_width / 2.0);
					let max_from_l = back_width - sbit_width - sbit_bordl - sbit_bordr;
				
					if from_l < 0.0 {
						from_l = 0.0;
					} else if from_l > max_from_l {
						from_l = max_from_l;
					}
				
					let mut percent = from_l / max_from_l;
					let mut data = _slider.data.lock();
					data.at = ((data.max - data.min) * percent) + data.min;
					data.apply_method();
					percent = (data.at - data.min) / (data.max - data.min);
					from_l = max_from_l * percent;
				
					_slider.slidy_bit.inner_update(BinInner {
						pos_from_l: Some(from_l),
						.. sbit_inner
					});
				
					_slider.input_box.inner_update(BinInner {
						text: format!("{}", data.at),
						.. _slider.input_box.inner_copy()
					});
				
					let funcs = _slider.on_change.lock().clone();
					let at_copy = data.at.clone();
	
					thread::spawn(move || {
						for func in funcs {
							func(at_copy);
						}
					});
				}
			})));
		
			let _sliding = sliding.clone();
		
			hooks.ms.push(engine.mouse().on_release(mouse::Button::Left, Arc::new(move |_| {
				_sliding.store(false, atomic::Ordering::Relaxed);
			})));
		}
		
		slider
	}
	
	pub fn set(&self, val: f32) {
		let mut data = self.data.lock();
		data.at = val;
		
		if data.at > data.max {
			data.at = data.max;
		} else if data.at < data.min {
			data.at = data.min;
		}
		
		self.force_update(Some(&mut *data));
	}
	
	pub fn increment(&self) {
		let mut data = self.data.lock();
		data.at += data.step;
		
		if data.at > data.max {
			data.at = data.max;
		}
		
		self.force_update(Some(&mut *data));
	}
	
	pub fn decrement(&self) {
		let mut data = self.data.lock();
		data.at -= data.step;
		
		if data.at < data.min {
			data.at = data.min;
		}
		
		self.force_update(Some(&mut *data));
	}

	fn force_update(&self, data: Option<&mut Data>) {	
		let (percent, at) = match data {
			Some(data) => ((data.at - data.min) / (data.max - data.min), data.at),
			None => {
				let data = self.data.lock();
				((data.at - data.min) / (data.max - data.min), data.at)
			}
		};
		
		let back_bps = self.slide_back.box_points();
		let back_width = back_bps.tro[0] - back_bps.tlo[0];
		let sbit_inner = self.slidy_bit.inner_copy();
		let sbit_width = sbit_inner.width.unwrap_or(0.0);
		let sbit_bordl = sbit_inner.border_size_l.unwrap_or(0.0);
		let sbit_bordr = sbit_inner.border_size_r.unwrap_or(0.0);
		let max_from_l = back_width - sbit_bordl - sbit_bordr - sbit_width;
		let set_from_l = max_from_l * percent;
		
		self.slidy_bit.inner_update(BinInner {
			pos_from_l: Some(set_from_l),
			.. sbit_inner
		});
		
		self.input_box.set_text(format!("{}", at));
		let funcs = self.on_change.lock().clone();
		let at_copy = at.clone();
		
		thread::spawn(move || {
			for func in funcs {
				func(at_copy);
			}
		});
	}
}
