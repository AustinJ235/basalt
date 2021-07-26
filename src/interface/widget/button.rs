use crate::interface::bin::Bin;
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
		let id = bst.interface_ref().next_widget_id();
		let container = bst.interface_ref().new_bin();

		let widget = Arc::new(Button {
			id,
			theme: Mutex::new(bst.interface_ref().current_widget_theme()),
			container,
		});

		bst.interface_ref().register_widget(widget.clone());
		widget
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

	fn contains_bin(&self, bin: &Arc<Bin>) -> bool {
		self.container.id() == bin.id()
	}

	fn contains_bin_id(&self, id: u64) -> bool {
		self.container.id() == id
	}
}
