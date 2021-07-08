use crate::input::*;
use crate::interface::bin::{self, Bin, BinPosition, BinStyle, BinVert};
use crate::interface::hook::*;
use crate::Basalt;
use parking_lot::Mutex;
use std::sync::Arc;

pub struct ScrollBarStyle {
	pub border_color: bin::Color,
	pub arrow_color: bin::Color,
	pub bar_color: bin::Color,
	pub back_color: bin::Color,
}

impl Default for ScrollBarStyle {
	fn default() -> Self {
		ScrollBarStyle {
			back_color: bin::Color::srgb_hex("35353c"),
			bar_color: bin::Color::srgb_hex("f0f0f0"),
			arrow_color: bin::Color::srgb_hex("f0f0f0"),
			border_color: bin::Color::srgb_hex("222227"),
		}
	}
}

pub struct ScrollBar {
	pub back: Arc<Bin>,
	pub up: Arc<Bin>,
	pub down: Arc<Bin>,
	pub bar: Arc<Bin>,
	scroll: Arc<Bin>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScrollTo {
	Same,
	Top,
	Bottom,
	Percent(f32),
	Amount(f32),
	Set(f32),
}

impl ScrollBar {
	pub fn new(
		basalt: Arc<Basalt>,
		style: Option<ScrollBarStyle>,
		parent: Option<Arc<Bin>>,
		scroll: Arc<Bin>,
	) -> Arc<Self> {
		let style = style.unwrap_or_default();
		let mut bins = basalt.interface_ref().new_bins(4);
		let back = bins.pop().unwrap();
		let up = bins.pop().unwrap();
		let down = bins.pop().unwrap();
		let bar = bins.pop().unwrap();
		let position = match parent {
			Some(parent) => {
				parent.add_child(back.clone());
				BinPosition::Parent
			},
			None => BinPosition::Window,
		};

		back.add_child(up.clone());
		back.add_child(down.clone());
		back.add_child(bar.clone());

		back.style_update(BinStyle {
			position: Some(position),
			pos_from_t: Some(0.0),
			pos_from_b: Some(0.0),
			pos_from_r: Some(0.0),
			width: Some(15.0),
			back_color: Some(style.back_color),
			border_size_l: Some(1.0),
			border_color_l: Some(style.border_color),
			..BinStyle::default()
		});

		up.style_update(BinStyle {
			position: Some(BinPosition::Parent),
			pos_from_t: Some(0.0),
			pos_from_l: Some(0.0),
			pos_from_r: Some(0.0),
			height: Some(13.0),
			custom_verts: vec![
				BinVert {
					position: (7.5, 4.0, 0),
					color: style.arrow_color.clone(),
				},
				BinVert {
					position: (4.0, 9.0, 0),
					color: style.arrow_color.clone(),
				},
				BinVert {
					position: (11.0, 9.0, 0),
					color: style.arrow_color.clone(),
				},
			],
			..BinStyle::default()
		});

		down.style_update(BinStyle {
			position: Some(BinPosition::Parent),
			pos_from_b: Some(0.0),
			pos_from_l: Some(0.0),
			pos_from_r: Some(0.0),
			height: Some(13.0),
			custom_verts: vec![
				BinVert {
					position: (11.0, 4.0, 0),
					color: style.arrow_color.clone(),
				},
				BinVert {
					position: (4.0, 4.0, 0),
					color: style.arrow_color.clone(),
				},
				BinVert {
					position: (7.5, 9.0, 0),
					color: style.arrow_color,
				},
			],
			..BinStyle::default()
		});

		bar.style_update(BinStyle {
			position: Some(BinPosition::Parent),
			pos_from_t: Some(15.0),
			pos_from_b: Some(15.0),
			pos_from_l: Some(2.0),
			pos_from_r: Some(2.0),
			back_color: Some(style.bar_color),
			..BinStyle::default()
		});

		let sb = Arc::new(ScrollBar {
			back,
			up,
			down,
			bar,
			scroll,
		});

		let sb_wk = Arc::downgrade(&sb);
		let drag_data: Arc<Mutex<Option<(f32, f32)>>> = Arc::new(Mutex::new(None));
		let drag_data_cp = drag_data.clone();

		sb.bar.on_mouse_press(
			MouseButton::Left,
			Arc::new(move |_, hook_data| {
				if let BinHookData::Press {
					mouse_y,
					..
				} = hook_data
				{
					let sb = match sb_wk.upgrade() {
						Some(some) => some,
						None => return,
					};

					let scroll_y = sb.scroll.style_copy().scroll_y.unwrap_or(0.0);
					*drag_data_cp.lock() = Some((*mouse_y, scroll_y));
				}
			}),
		);

		let drag_data_cp = drag_data.clone();

		sb.bar.on_mouse_release(
			MouseButton::Left,
			Arc::new(move |_, _| {
				*drag_data_cp.lock() = None;
			}),
		);

		let drag_data_cp = drag_data.clone();
		let sb_wk = Arc::downgrade(&sb);

		sb.bar.attach_input_hook(basalt.input_ref().add_hook(
			InputHook::MouseMove,
			Arc::new(move |data| {
				if let InputHookData::MouseMove {
					mouse_y,
					..
				} = data
				{
					let drag_data_op = drag_data_cp.lock();
					let drag_data = match drag_data_op.as_ref() {
						Some(some) => some,
						None => return InputHookRes::Success,
					};

					let sb = match sb_wk.upgrade() {
						Some(some) => some,
						None => return InputHookRes::Remove,
					};

					let overflow = sb.scroll.calc_overflow();
					let up_post = sb.up.post_update();
					let down_post = sb.down.post_update();
					let max_bar_h = down_post.tlo[1] - up_post.blo[1];
					let mut bar_sp = overflow / 10.0;
					let mut bar_h = max_bar_h - bar_sp;

					if bar_h < 3.0 {
						bar_h = 3.0;
						bar_sp = max_bar_h - bar_h;
					}

					let bar_inc = overflow / bar_sp;
					sb.update(ScrollTo::Set(drag_data.1 + ((mouse_y - drag_data.0) * bar_inc)));
				}

				InputHookRes::Success
			}),
		));

		let sb_wk = Arc::downgrade(&sb);

		sb.scroll.on_update(Arc::new(move || {
			if let Some(sb) = sb_wk.upgrade() {
				sb.back.force_update();
				let sb_wk = Arc::downgrade(&sb);

				sb.back.on_update_once(Arc::new(move || {
					if let Some(sb) = sb_wk.upgrade() {
						sb.update(ScrollTo::Same);
					}
				}));
			}
		}));

		let sb_wk = Arc::downgrade(&sb);

		sb.back.on_update(Arc::new(move || {
			match sb_wk.upgrade() {
				Some(sb) => sb.update(ScrollTo::Same),
				None => (),
			}
		}));

		let sb_wk = Arc::downgrade(&sb);

		sb.up.on_mouse_press(
			MouseButton::Left,
			Arc::new(move |_, _| {
				match sb_wk.upgrade() {
					Some(sb) => sb.update(ScrollTo::Amount(-10.0)),
					None => (),
				}
			}),
		);

		let sb_wk = Arc::downgrade(&sb);

		sb.down.on_mouse_press(
			MouseButton::Left,
			Arc::new(move |_, _| {
				match sb_wk.upgrade() {
					Some(sb) => sb.update(ScrollTo::Amount(10.0)),
					None => (),
				}
			}),
		);

		let sb_wk = Arc::downgrade(&sb);

		sb.back.add_hook_raw(
			BinHook::MouseScroll,
			Arc::new(move |_, data| {
				if let BinHookData::MouseScroll {
					scroll_amt,
					..
				} = data
				{
					match sb_wk.upgrade() {
						Some(sb) => sb.update(ScrollTo::Amount(*scroll_amt)),
						None => (),
					}
				}
			}),
		);

		let sb_wk = Arc::downgrade(&sb);

		sb.scroll.add_hook_raw(
			BinHook::MouseScroll,
			Arc::new(move |_, data| {
				if let BinHookData::MouseScroll {
					scroll_amt,
					..
				} = data
				{
					match sb_wk.upgrade() {
						Some(sb) => sb.update(ScrollTo::Amount(*scroll_amt)),
						None => (),
					}
				}
			}),
		);

		sb
	}

	pub fn update(&self, amount: ScrollTo) {
		let mut scroll_y = self.scroll.style_copy().scroll_y.unwrap_or(0.0);
		let overflow = self.scroll.calc_overflow();

		if match amount {
			ScrollTo::Same => false,
			ScrollTo::Top =>
				if scroll_y == 0.0 {
					false
				} else {
					scroll_y = 0.0;
					true
				},
			ScrollTo::Bottom =>
				if scroll_y == overflow {
					false
				} else {
					scroll_y = overflow;
					true
				},
			ScrollTo::Percent(p) =>
				if p.is_sign_positive() {
					if scroll_y == overflow {
						false
					} else {
						let amt = overflow * p;

						if scroll_y + amt > overflow {
							scroll_y = overflow;
						} else {
							scroll_y += amt;
						}

						true
					}
				} else {
					if scroll_y == 0.0 {
						false
					} else {
						let amt = overflow * p;

						if scroll_y + amt < 0.0 {
							scroll_y = 0.0;
						} else {
							scroll_y += amt;
						}

						true
					}
				},
			ScrollTo::Amount(amt) =>
				if amt.is_sign_positive() {
					if scroll_y == overflow {
						false
					} else {
						if scroll_y + amt > overflow {
							scroll_y = overflow;
						} else {
							scroll_y += amt;
						}

						true
					}
				} else {
					if scroll_y == 0.0 {
						false
					} else {
						if scroll_y + amt < 0.0 {
							scroll_y = 0.0;
						} else {
							scroll_y += amt;
						}

						true
					}
				},
			ScrollTo::Set(to) =>
				if to < 0.0 {
					if scroll_y == 0.0 {
						false
					} else {
						scroll_y = 0.0;
						true
					}
				} else if to > overflow {
					if scroll_y == overflow {
						false
					} else {
						scroll_y = overflow;
						true
					}
				} else {
					scroll_y = to;
					true
				},
		} {
			self.scroll.style_update(BinStyle {
				scroll_y: Some(scroll_y),
				..self.scroll.style_copy()
			});

			self.scroll.update_children();
		}

		let up_post = self.up.post_update();
		let down_post = self.down.post_update();
		let max_bar_h = down_post.tlo[1] - up_post.blo[1];

		if max_bar_h < 3.0 {
			// println!("Scroll bar less than minimum height.");
		}

		let mut bar_sp = overflow / 10.0;
		let mut bar_h = max_bar_h - bar_sp;

		if bar_h < 3.0 {
			bar_h = 3.0;
			bar_sp = max_bar_h - bar_h;
		}

		let bar_inc = overflow / bar_sp;
		let bar_pos = scroll_y / bar_inc;

		self.bar.style_update(BinStyle {
			pos_from_t: Some(bar_pos + up_post.blo[1] - up_post.tlo[1]),
			pos_from_b: None,
			height: Some(bar_h),
			..self.bar.style_copy()
		});
	}
}
