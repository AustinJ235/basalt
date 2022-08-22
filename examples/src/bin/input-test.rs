use basalt::input::MouseButton;
use basalt::input_v2::InputHookCtrl;
use basalt::interface::bin::{self, BinPosition, BinStyle};
use basalt::{Basalt, BstOptions};

fn main() {
	Basalt::initialize(
		BstOptions::default().window_size(300, 300).title("Basalt").app_loop(),
		Box::new(move |basalt_res| {
			let basalt = basalt_res.unwrap();
			let background = basalt.interface_ref().new_bin();

			background.style_update(BinStyle {
				pos_from_t: Some(0.0),
				pos_from_b: Some(0.0),
				pos_from_l: Some(0.0),
				pos_from_r: Some(0.0),
				back_color: Some(bin::Color::srgb_hex("f0f0f0")),
				..BinStyle::default()
			});

			let button = basalt.interface_ref().new_bin();
			background.add_child(button.clone());

			button.style_update(BinStyle {
				position: Some(BinPosition::Parent),
				pos_from_t: Some(75.0),
				pos_from_l: Some(75.0),
				width: Some(75.0),
				height: Some(30.0),
				back_color: Some(bin::Color::srgb_hex("c0c0c0")),
				border_size_t: Some(1.0),
				border_size_b: Some(1.0),
				border_size_l: Some(1.0),
				border_size_r: Some(1.0),
				border_color_t: Some(bin::Color::srgb_hex("707070")),
				border_color_b: Some(bin::Color::srgb_hex("707070")),
				border_color_l: Some(bin::Color::srgb_hex("707070")),
				border_color_r: Some(bin::Color::srgb_hex("707070")),
				text: String::from("Button"),
				text_height: Some(14.0),
				pad_t: Some(10.0),
				pad_l: Some(10.0),
				text_color: Some(bin::Color::srgb_hex("303030")),
				..BinStyle::default()
			});

			basalt
				.input_ref_v2()
				.hook()
				.bin(&button)
				.on_press()
				.key(MouseButton::Left)
				.weight(10)
				.call(move |_, _, _| {
					println!("Press Left");
					InputHookCtrl::RetainNoPass
				})
				.finish()
				.unwrap()
				.submit()
				.unwrap();

			basalt
				.input_ref_v2()
				.hook()
				.bin(&button)
				.on_release()
				.key(MouseButton::Left)
				.weight(11)
				.call(move |_, _, _| {
					println!("Release Left");
					InputHookCtrl::Retain
				})
				.finish()
				.unwrap()
				.submit()
				.unwrap();

			basalt
				.input_ref_v2()
				.hook()
				.bin(&button)
				.on_release()
				.key(MouseButton::Left)
				.call(move |_, _, _| {
					println!("Uncallable");
					Default::default()
				})
				.finish()
				.unwrap()
				.submit()
				.unwrap();

			basalt
				.input_ref_v2()
				.hook()
				.bin(&button)
				.on_release()
				.key(MouseButton::Right)
				.call(move |_, _, _| {
					println!("Release Right");
					Default::default()
				})
				.finish()
				.unwrap()
				.submit()
				.unwrap();

			basalt
				.input_ref_v2()
				.hook()
				.bin(&button)
				.on_press()
				.key(MouseButton::Right)
				.call(move |_, _, _| {
					println!("Press Right");
					Default::default()
				})
				.finish()
				.unwrap()
				.submit()
				.unwrap();
			
			basalt
				.input_ref_v2()
				.hook()
				.bin(&button)
				.on_focus()
				.call(move |_, _| {
					println!("Focus");
					Default::default()
				})
				.finish()
				.unwrap()
				.submit()
				.unwrap();
			
			basalt
				.input_ref_v2()
				.hook()
				.bin(&button)
				.on_focus_lost()
				.call(move |_, _| {
					println!("Focus Lost");
					Default::default()
				})
				.finish()
				.unwrap()
				.submit()
				.unwrap();

			basalt.wait_for_exit().unwrap();
		}),
	);
}
