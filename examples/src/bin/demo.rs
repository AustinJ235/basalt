extern crate basalt;

use basalt::input::MouseButton;
use basalt::interface::bin::{self, BinPosition, BinStyle};
use basalt::interface::widget::button::Button;
use basalt::Basalt;
use std::sync::Arc;

fn main() {
	Basalt::initialize(
		basalt::Options::default()
			.ignore_dpi(true)
			.window_size(300, 300)
			.title("Basalt")
			.app_loop(),
		Box::new(move |basalt_res| {
			let basalt = basalt_res.unwrap();
			let background = basalt.interface_ref().new_bin();

			background.style_update(BinStyle {
				pos_from_t: Some(0.0),
				pos_from_b: Some(0.0),
				pos_from_r: Some(0.0),
				pos_from_l: Some(0.0),
				back_color: Some(bin::Color::srgb_hex("ffffff")),
				..BinStyle::default()
			});

			let button = Button::new(basalt.clone());
			background.add_child(&button);

			button.style_update(BinStyle {
				position: Some(BinPosition::Parent),
				pos_from_t: Some(10.0),
				pos_from_l: Some(10.0),
				width: Some(100.0),
				height: Some(30.0),
				..button.style_copy()
			});

			button.set_text("Button");
			basalt.wait_for_exit().unwrap();
		}),
	);
}
