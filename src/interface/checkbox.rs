use super::bin::{Bin, BinStyle, KeepAlive};
use crate::input::*;
use crate::Basalt;
use parking_lot::Mutex;
use std::sync::Arc;

/// Simple checkbox. Provides a change hook and the ability to get the state.
/// When checked, the inner box is set to being visible and vise versa.

impl KeepAlive for CheckBox {}

pub struct CheckBox {
	pub basalt: Arc<Basalt>,
	pub inner_box: Arc<Bin>,
	pub outer_box: Arc<Bin>,
	checked: Mutex<bool>,
	on_change: Mutex<Vec<Box<dyn FnMut(bool) + Send + 'static>>>,
}

impl CheckBox {
	pub fn is_checked(&self) -> bool {
		*self.checked.lock()
	}

	pub fn set(&self, check: bool) {
		*self.checked.lock() = check;
		self.update(Some(check));
		self.call_on_change(Some(check));
	}

	pub fn check(&self) {
		self.set(true);
	}

	pub fn uncheck(&self) {
		self.set(false);
	}

	pub fn toggle(&self) {
		let mut checked = self.checked.lock();
		*checked = !*checked;
		self.update(Some(*checked));
		self.call_on_change(Some(*checked));
	}

	pub fn on_change<F: FnMut(bool) + Send + 'static>(&self, func: F) {
		self.on_change.lock().push(Box::new(func));
	}

	fn call_on_change(&self, checked_op: Option<bool>) {
		let checked = match checked_op {
			Some(some) => some,
			None => self.is_checked(),
		};

		for func in self.on_change.lock().iter_mut() {
			func(checked);
		}
	}

	fn update(&self, checked_op: Option<bool>) {
		let checked = match checked_op {
			Some(some) => some,
			None => self.is_checked(),
		};

		self.inner_box.style_update(BinStyle {
			hidden: Some(!checked),
			..self.inner_box.style_copy()
		});
	}

	pub fn new(basalt: Arc<Basalt>) -> Arc<Self> {
		let mut bins = basalt.interface_ref().new_bins(2);
		let checkbox = Arc::new(CheckBox {
			basalt,
			inner_box: bins.pop().unwrap(),
			outer_box: bins.pop().unwrap(),
			checked: Mutex::new(false),
			on_change: Mutex::new(Vec::new()),
		});

		checkbox.outer_box.add_child(checkbox.inner_box.clone());
		let checkbox_wk = Arc::downgrade(&checkbox);

		checkbox.outer_box.on_mouse_press(MouseButton::Left, move |_, _| {
			if let Some(checkbox) = checkbox_wk.upgrade() {
				checkbox.toggle();
			}
		});

		checkbox
	}
}
