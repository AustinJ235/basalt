pub mod button;
pub mod checkbox;
pub mod combo_box;
pub mod context_menu;
pub mod hori_scale;
pub mod hori_scroll_bar;
pub mod image;
pub mod label;
pub mod list_box;
pub mod menu_bar;
pub mod multi_line_entry;
pub mod pager;
pub mod progress_bar;
pub mod radio_button;
pub mod single_line_entry;
pub mod spin_button;
pub mod status;
pub mod switch_button;
pub mod theme;
pub mod toggle_button;
pub mod vert_scale;
pub mod vert_scroll_bar;

pub type WidgetID = u64;

use crate::interface::bin::{Bin, BinChild};
use crate::interface::widget::theme::WidgetTheme;
use std::sync::Arc;

pub trait Widget {
	fn id(&self) -> WidgetID;
	fn set_theme(&self, theme: WidgetTheme);
	fn current_theme(&self) -> WidgetTheme;
	fn container(&self) -> &Arc<Bin>;
	fn contains_bin(&self, bin: &Arc<Bin>) -> bool;
	fn contains_bin_id(&self, id: u64) -> bool;
}

impl<T: Widget> BinChild for T {
	fn bin(&self) -> &Arc<Bin> {
		self.container()
	}
}

impl<T: Widget> BinChild for Arc<T> {
	fn bin(&self) -> &Arc<Bin> {
		(*self).container()
	}
}
