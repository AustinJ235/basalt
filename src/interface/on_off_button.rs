use crate::input::MouseButton;
use crate::interface::bin::{self, Bin, BinPosition, BinStyle, KeepAlive};
use crate::interface::hook::BinHookFn;
use crate::Basalt;
use ilmenite::ImtHoriAlign;
use parking_lot::Mutex;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;

impl KeepAlive for Arc<OnOffButton> {}

pub struct OnOffButton {
	pub container: Arc<Bin>,
	theme: OnOffButtonTheme,
	enabled: AtomicBool,
	on: Arc<Bin>,
	off: Arc<Bin>,
	on_change_fns: Mutex<Vec<Arc<dyn Fn(bool) + Send + Sync>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnOffButtonTheme {
	/// Color of the container when off
	pub color1: bin::Color,
	/// Color of the container when on
	pub color2: bin::Color,
	/// Color of the inner slidy bit
	pub color3: bin::Color,
	/// Color of the off text color
	pub color4: bin::Color,
	/// Color of the on text color
	pub color5: bin::Color,
}

impl Default for OnOffButtonTheme {
	fn default() -> Self {
		OnOffButtonTheme {
			color1: bin::Color::srgb_hex("ff0000d0"),
			color2: bin::Color::srgb_hex("00ff00d0"),
			color3: bin::Color::srgb_hex("000000f0"),
			color4: bin::Color::srgb_hex("ffffffff"),
			color5: bin::Color::srgb_hex("ffffffff"),
		}
	}
}

impl OnOffButton {
	pub fn new(
		basalt: Arc<Basalt>,
		theme: OnOffButtonTheme,
		parent: Option<Arc<Bin>>,
	) -> Arc<Self> {
		let mut bins = basalt.interface_ref().new_bins(3);
		let container = bins.pop().unwrap();
		let on = bins.pop().unwrap();
		let off = bins.pop().unwrap();
		container.add_child(on.clone());
		container.add_child(off.clone());

		if let Some(parent) = parent.as_ref() {
			parent.add_child(container.clone());
		}

		container.style_update(BinStyle {
			position: Some(match parent.is_some() {
				true => BinPosition::Parent,
				false => BinPosition::Window,
			}),
			pos_from_t: Some(0.0),
			pos_from_l: Some(0.0),
			width: Some(60.0),
			height: Some(24.0),
			border_radius_tl: Some(3.0),
			border_radius_bl: Some(3.0),
			border_radius_tr: Some(3.0),
			border_radius_br: Some(3.0),
			back_color: Some(theme.color1.clone()),
			..BinStyle::default()
		});

		off.style_update(BinStyle {
			position: Some(BinPosition::Parent),
			pos_from_t: Some(2.0),
			pos_from_l: Some(2.0),
			pos_from_b: Some(2.0),
			width: Some(28.0),
			pad_t: Some(5.0),
			text: String::from("Off"),
			text_color: Some(theme.color4.clone()),
			text_height: Some(12.0),
			text_hori_align: Some(ImtHoriAlign::Center),
			..BinStyle::default()
		});

		on.style_update(BinStyle {
			position: Some(BinPosition::Parent),
			pos_from_t: Some(2.0),
			pos_from_r: Some(2.0),
			pos_from_b: Some(2.0),
			width: Some(28.0),
			border_radius_tl: Some(3.0),
			border_radius_bl: Some(3.0),
			border_radius_tr: Some(3.0),
			border_radius_br: Some(3.0),
			back_color: Some(theme.color3.clone()),
			..BinStyle::default()
		});

		let ret = Arc::new(OnOffButton {
			container,
			theme,
			enabled: AtomicBool::new(false),
			on,
			off,
			on_change_fns: Mutex::new(Vec::new()),
		});

		let button = ret.clone();

		let on_press_fn: BinHookFn = Arc::new(move |_, _| {
			button.toggle();
		});

		ret.on.on_mouse_press(MouseButton::Left, on_press_fn.clone());
		ret.off.on_mouse_press(MouseButton::Left, on_press_fn.clone());
		ret.container.on_mouse_press(MouseButton::Left, on_press_fn.clone());
		ret
	}

	pub fn toggle(&self) -> bool {
		let cur = self.enabled.load(atomic::Ordering::Relaxed);
		self.set(!cur);
		!cur
	}

	pub fn is_on(&self) -> bool {
		self.enabled.load(atomic::Ordering::Relaxed)
	}

	pub fn on_change(&self, func: Arc<dyn Fn(bool) + Send + Sync>) {
		self.on_change_fns.lock().push(func);
	}

	pub fn set(&self, on: bool) {
		self.enabled.store(on, atomic::Ordering::Relaxed);

		if !on {
			self.container.style_update(BinStyle {
				back_color: Some(self.theme.color1.clone()),
				..self.container.style_copy()
			});

			self.on.style_update(BinStyle {
				position: Some(BinPosition::Parent),
				pos_from_t: Some(2.0),
				pos_from_r: Some(2.0),
				pos_from_b: Some(2.0),
				width: Some(28.0),
				border_radius_tl: Some(3.0),
				border_radius_bl: Some(3.0),
				border_radius_tr: Some(3.0),
				border_radius_br: Some(3.0),
				back_color: Some(self.theme.color3.clone()),
				..BinStyle::default()
			});

			self.off.style_update(BinStyle {
				position: Some(BinPosition::Parent),
				pos_from_t: Some(2.0),
				pos_from_l: Some(2.0),
				pos_from_b: Some(2.0),
				width: Some(28.0),
				pad_t: Some(5.0),
				text: String::from("Off"),
				text_color: Some(self.theme.color4.clone()),
				text_height: Some(12.0),
				text_hori_align: Some(ImtHoriAlign::Center),
				..BinStyle::default()
			});
		} else {
			self.container.style_update(BinStyle {
				back_color: Some(self.theme.color2.clone()),
				..self.container.style_copy()
			});

			self.on.style_update(BinStyle {
				position: Some(BinPosition::Parent),
				pos_from_t: Some(2.0),
				pos_from_r: Some(2.0),
				pos_from_b: Some(2.0),
				width: Some(28.0),
				pad_t: Some(5.0),
				text: String::from("On"),
				text_color: Some(self.theme.color5.clone()),
				text_height: Some(12.0),
				text_hori_align: Some(ImtHoriAlign::Center),
				..BinStyle::default()
			});

			self.off.style_update(BinStyle {
				position: Some(BinPosition::Parent),
				pos_from_t: Some(2.0),
				pos_from_l: Some(2.0),
				pos_from_b: Some(2.0),
				width: Some(28.0),
				border_radius_tl: Some(3.0),
				border_radius_bl: Some(3.0),
				border_radius_tr: Some(3.0),
				border_radius_br: Some(3.0),
				back_color: Some(self.theme.color3.clone()),
				..BinStyle::default()
			});
		}

		for func in self.on_change_fns.lock().iter() {
			func(on);
		}
	}
}
