use crate::interface::bin::{Bin, BinStyle};
use crate::interface::widget::theme::WidgetTheme;
use crate::interface::widget::{Widget, WidgetID};
use crate::Basalt;
use parking_lot::Mutex;
use std::ops::Deref;
use std::sync::Arc;

pub struct Button {
	id: WidgetID,
	theme: Mutex<WidgetTheme>,
	container: Arc<Bin>,
}

impl Button {
	pub fn new(bst: Arc<Basalt>) -> Arc<Self> {
		let theme = bst.interface_ref().current_widget_theme();
		let id = bst.interface_ref().next_widget_id();
		let container = bst.interface_ref().new_bin();

		container.internal_mark_widget();
		container.internal_style_update(BinStyle {
			border_size_t: Some(theme.dim_border1.clone()),
			border_size_b: Some(theme.dim_border1.clone()),
			border_size_l: Some(theme.dim_border1.clone()),
			border_size_r: Some(theme.dim_border1.clone()),
			border_color_t: Some(theme.color_border1.clone()),
			border_color_b: Some(theme.color_border1.clone()),
			border_color_l: Some(theme.color_border1.clone()),
			border_color_r: Some(theme.color_border1.clone()),
			back_color: Some(theme.color_back1.clone()),
			text_color: Some(theme.color_text1.clone()),
			text_height: Some(theme.dim_text1.clone()),
			..BinStyle::default()
		});

		let widget = Arc::new(Button {
			id,
			theme: Mutex::new(theme),
			container,
		});

		bst.interface_ref().register_widget(widget.clone());
		widget
	}

	pub fn set_text<T: Into<String>>(&self, text: T) {
		self.container.internal_style_update(BinStyle {
			text: text.into(),
			..self.container.style_copy()
		});
	}
}

impl Deref for Button {
	type Target = Arc<Bin>;

	fn deref(&self) -> &Self::Target {
		&self.container
	}
}

impl Widget for Button {
	fn id(&self) -> WidgetID {
		self.id
	}

	fn set_theme(&self, theme: WidgetTheme) {
		*self.theme.lock() = theme;
	}

	fn current_theme(&self) -> WidgetTheme {
		self.theme.lock().clone()
	}

	fn container(&self) -> &Arc<Bin> {
		&self.container
	}

	fn contains_bin(&self, bin: &Arc<Bin>) -> bool {
		self.container.id() == bin.id()
	}

	fn contains_bin_id(&self, id: u64) -> bool {
		self.container.id() == id
	}
}
