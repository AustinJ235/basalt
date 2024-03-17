use basalt::input::{MouseButton, Qwerty};
use basalt::interface::bin;
use basalt::interface::bin::{BinPosition, BinStyle};
use basalt::render::Renderer;
use basalt::window::WindowOptions;
use basalt::{Basalt, BasaltOptions};

fn main() {
    Basalt::initialize(BasaltOptions::default(), move |basalt_res| {
        let basalt = basalt_res.unwrap();

        let window = basalt
            .window_manager_ref()
            .create(WindowOptions {
                title: String::from("app"),
                inner_size: Some([400; 2]),
                ..WindowOptions::default()
            })
            .unwrap();

        window.on_press(Qwerty::F8, move |target, _, _| {
            let window = target.into_window().unwrap();
            println!("VSync: {:?}", window.toggle_renderer_vsync());
            Default::default()
        });

        window.on_press(Qwerty::F9, move |target, _, _| {
            let window = target.into_window().unwrap();
            println!("MSAA: {:?}", window.decr_renderer_msaa());
            Default::default()
        });

        window.on_press(Qwerty::F10, move |target, _, _| {
            let window = target.into_window().unwrap();
            println!("MSAA: {:?}", window.incr_renderer_msaa());
            Default::default()
        });

        let background = window.new_bin();

        background
            .style_update(BinStyle {
                pos_from_t: Some(0.0),
                pos_from_b: Some(0.0),
                pos_from_l: Some(0.0),
                pos_from_r: Some(0.0),
                back_color: Some(bin::Color::srgb_hex("f0f0f0")),
                ..BinStyle::default()
            })
            .expect_valid();

        let button = window.new_bin();
        background.add_child(button.clone());

        button
            .style_update(BinStyle {
                position: Some(BinPosition::Parent),
                pos_from_t: Some(75.0),
                pos_from_l: Some(75.0),
                width: Some(75.0),
                height: Some(32.0),
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
                text_height: Some(16.0),
                pad_t: Some(7.0),
                pad_l: Some(8.0),
                text_color: Some(bin::Color::srgb_hex("303030")),
                ..BinStyle::default()
            })
            .expect_valid();

        button.on_press(MouseButton::Left, move |_, window, local| {
            println!("{:?} {:?}", window, local);
            Default::default()
        });

        Renderer::new(window)
            .unwrap()
            .with_interface_only()
            .run()
            .unwrap();
        basalt.exit();
    });
}
