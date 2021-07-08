use super::bin::{Bin, BinPosition, BinStyle, Color, KeepAlive};
use crate::input::*;
use crate::Basalt;
use ilmenite::ImtTextWrap;
use parking_lot::Mutex;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

impl KeepAlive for Slider {}

pub struct Slider {
	pub basalt: Arc<Basalt>,
	pub container: Arc<Bin>,
	pub slidy_bit: Arc<Bin>,
	pub input_box: Arc<Bin>,
	pub slide_back: Arc<Bin>,
	data: Mutex<Data>,
	on_change: Mutex<Vec<Arc<dyn Fn(f32) + Send + Sync>>>,
	hooks: Mutex<Vec<InputHookID>>,
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
			},
			Method::RoundToInt => {
				self.at = f32::round(self.at);
			},
		}
		if self.at > self.max {
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

		for id in hooks.split_off(0) {
			self.basalt.input_ref().remove_hook(id);
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
	}

	pub fn set_step_size(&self, size: f32) {
		self.data.lock().step = size;
	}

	pub fn on_change(&self, func: Arc<dyn Fn(f32) + Send + Sync>) {
		self.on_change.lock().push(func);
	}

	pub fn set_method(&self, method: Method) {
		self.data.lock().method = method;
	}

	pub fn new(basalt: Arc<Basalt>, parent_op: Option<Arc<Bin>>) -> Arc<Slider> {
		let mut bins = basalt.interface_ref().new_bins(4);
		let slider = Arc::new(Slider {
			basalt: basalt.clone(),
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
			}),
			on_change: Mutex::new(Vec::new()),
			hooks: Mutex::new(Vec::new()),
		});

		if let Some(parent) = parent_op {
			parent.add_child(slider.container.clone());
		}

		slider.slide_back.add_child(slider.slidy_bit.clone());
		slider.container.add_child(slider.input_box.clone());
		slider.container.add_child(slider.slide_back.clone());

		slider.container.style_update(BinStyle {
			position: Some(BinPosition::Parent),
			..BinStyle::default()
		});

		slider.slidy_bit.style_update(BinStyle {
			position: Some(BinPosition::Parent),
			add_z_index: Some(100),
			pos_from_l: Some(30.0),
			pos_from_t: Some(-3.0),
			pos_from_b: Some(-3.0),
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
			..BinStyle::default()
		});

		slider.input_box.style_update(BinStyle {
			position: Some(BinPosition::Parent),
			pos_from_t: Some(1.0),
			pos_from_b: Some(1.0),
			pos_from_r: Some(0.0),
			pad_l: Some(5.0),
			text_height: Some(14.0),
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
			text_wrap: Some(ImtTextWrap::None),
			..BinStyle::default()
		});

		slider.slide_back.style_update(BinStyle {
			position: Some(BinPosition::Parent),
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
			overflow_y: Some(true),
			overflow_x: Some(true),
			..BinStyle::default()
		});

		let _slider = Arc::downgrade(&slider);

		slider.slide_back.on_update(Arc::new(move || {
			let _slider = match _slider.upgrade() {
				Some(some) => some,
				None => return,
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

			hooks.push(basalt.input_ref().on_mouse_press(
				MouseButton::Left,
				Arc::new(move |data| {
					if let InputHookData::Press {
						mouse_x,
						mouse_y,
						..
					} = data
					{
						let _slider = match _slider.upgrade() {
							Some(some) => some,
							None => return InputHookRes::Remove,
						};

						if _slider.slidy_bit.mouse_inside(*mouse_x, *mouse_y) {
							_sliding.store(true, atomic::Ordering::Relaxed);
						}
						if _slider.container.mouse_inside(*mouse_x, *mouse_y) {
							_focused.store(true, atomic::Ordering::Relaxed);
						} else {
							_focused.store(false, atomic::Ordering::Relaxed);
						}
					}

					InputHookRes::Success
				}),
			));

			let _slider = Arc::downgrade(&slider);

			hooks.push(basalt.input_ref().add_hook(
				InputHook::MouseScroll,
				Arc::new(move |data| {
					if let InputHookData::MouseScroll {
						mouse_x,
						mouse_y,
						scroll_amt,
					} = data
					{
						let _slider = match _slider.upgrade() {
							Some(some) => some,
							None => return InputHookRes::Remove,
						};

						if _slider.container.mouse_inside(*mouse_x, *mouse_y) {
							if *scroll_amt > 0.0 {
								_slider.increment();
							} else {
								_slider.decrement();
							}
						}
					}

					InputHookRes::Success
				}),
			));

			let _focused = focused.clone();
			let _slider = Arc::downgrade(&slider);

			hooks.push(basalt.input_ref().on_key_press(
				Qwery::ArrowRight,
				Arc::new(move |_| {
					let _slider = match _slider.upgrade() {
						Some(some) => some,
						None => return InputHookRes::Remove,
					};

					if _focused.load(atomic::Ordering::Relaxed) {
						_slider.increment();
					}

					InputHookRes::Success
				}),
			));

			let _focused = focused.clone();
			let _slider = Arc::downgrade(&slider);

			hooks.push(basalt.input_ref().on_key_press(
				Qwery::ArrowLeft,
				Arc::new(move |_| {
					let _slider = match _slider.upgrade() {
						Some(some) => some,
						None => return InputHookRes::Remove,
					};

					if _focused.load(atomic::Ordering::Relaxed) {
						_slider.decrement();
					}

					InputHookRes::Success
				}),
			));

			let _focused = focused.clone();
			let _slider = Arc::downgrade(&slider);

			hooks.push(basalt.input_ref().on_key_hold(
				Qwery::ArrowRight,
				Duration::from_millis(300),
				Duration::from_millis(150),
				Arc::new(move |_| {
					let _slider = match _slider.upgrade() {
						Some(some) => some,
						None => return InputHookRes::Remove,
					};

					if _focused.load(atomic::Ordering::Relaxed) {
						_slider.increment();
					}

					InputHookRes::Success
				}),
			));

			let _focused = focused.clone();
			let _slider = Arc::downgrade(&slider);

			hooks.push(basalt.input_ref().on_key_hold(
				Qwery::ArrowLeft,
				Duration::from_millis(300),
				Duration::from_millis(150),
				Arc::new(move |_| {
					let _slider = match _slider.upgrade() {
						Some(some) => some,
						None => return InputHookRes::Remove,
					};

					if _focused.load(atomic::Ordering::Relaxed) {
						_slider.decrement();
					}

					InputHookRes::Success
				}),
			));

			let _sliding = sliding.clone();
			let _slider = Arc::downgrade(&slider);

			hooks.push(basalt.input_ref().add_hook(
				InputHook::MouseMove,
				Arc::new(move |data| {
					if let InputHookData::MouseMove {
						mouse_x,
						..
					} = data
					{
						let _slider = match _slider.upgrade() {
							Some(some) => some,
							None => return InputHookRes::Remove,
						};

						if _sliding.load(atomic::Ordering::Relaxed) {
							let back_bps = _slider.slide_back.post_update();
							let back_width = back_bps.tro[0] - back_bps.tlo[0];
							let sbit_style = _slider.slidy_bit.style_copy();
							let sbit_width = sbit_style.width.unwrap_or(0.0);
							let sbit_bordl = sbit_style.border_size_l.unwrap_or(0.0);
							let sbit_bordr = sbit_style.border_size_r.unwrap_or(0.0);
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

							_slider.slidy_bit.style_update(BinStyle {
								pos_from_l: Some(from_l),
								..sbit_style
							});

							_slider.input_box.style_update(BinStyle {
								text: format!("{}", data.at),
								.._slider.input_box.style_copy()
							});

							let funcs = _slider.on_change.lock().clone();
							let at_copy = data.at.clone();

							thread::spawn(move || {
								for func in funcs {
									func(at_copy);
								}
							});
						}
					}

					InputHookRes::Success
				}),
			));

			let _sliding = sliding.clone();

			hooks.push(basalt.input_ref().on_mouse_release(
				MouseButton::Left,
				Arc::new(move |_| {
					_sliding.store(false, atomic::Ordering::Relaxed);
					InputHookRes::Success
				}),
			));
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
			},
		};

		let back_bps = self.slide_back.post_update();
		let back_width = back_bps.tro[0] - back_bps.tlo[0];
		let sbit_style = self.slidy_bit.style_copy();
		let sbit_width = sbit_style.width.unwrap_or(0.0);
		let sbit_bordl = sbit_style.border_size_l.unwrap_or(0.0);
		let sbit_bordr = sbit_style.border_size_r.unwrap_or(0.0);
		let max_from_l = back_width - sbit_bordl - sbit_bordr - sbit_width;
		let set_from_l = max_from_l * percent;

		self.slidy_bit.style_update(BinStyle {
			pos_from_l: Some(set_from_l),
			..sbit_style
		});

		self.input_box.style_update(BinStyle {
			text: format!("{}", at),
			..self.input_box.style_copy()
		});

		let funcs = self.on_change.lock().clone();
		let at_copy = at.clone();

		thread::spawn(move || {
			for func in funcs {
				func(at_copy);
			}
		});
	}
}
