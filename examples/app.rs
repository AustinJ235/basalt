use basalt::input::{MouseButton, Qwerty};
use basalt::interface::UnitValue::Pixels;
use basalt::interface::{BinStyle, Color, TextAttrs, TextBody};
use basalt::render::{Renderer, RendererError};
use basalt::{Basalt, BasaltOptions};

fn main() {
    Basalt::initialize(BasaltOptions::default(), move |basalt_res| {
        let basalt = basalt_res.unwrap();

        let window = basalt
            .window_manager_ref()
            .create()
            .title("app")
            .size([400, 400])
            .build()
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

        window.on_press(Qwerty::F11, move |target, _, _| {
            let window = target.into_window().unwrap();
            println!("Fullscreen: {:?}", window.toggle_fullscreen());
            Default::default()
        });

        let background = window.new_bin();

        background
            .style_update(BinStyle {
                pos_from_t: Pixels(0.0),
                pos_from_b: Pixels(0.0),
                pos_from_l: Pixels(0.0),
                pos_from_r: Pixels(0.0),
                back_color: Color::shex("f0f0f0"),
                ..Default::default()
            })
            .expect_valid();

        let button = window.new_bin();
        background.add_child(button.clone());

        button
            .style_update(BinStyle {
                pos_from_t: Pixels(75.0),
                pos_from_l: Pixels(75.0),
                width: Pixels(75.0),
                height: Pixels(32.0),
                padding_t: Pixels(8.0),
                padding_l: Pixels(8.0),
                back_color: Color::shex("c0c0c0"),
                border_size_t: Pixels(1.0),
                border_size_b: Pixels(1.0),
                border_size_l: Pixels(1.0),
                border_size_r: Pixels(1.0),
                border_color_t: Color::shex("707070"),
                border_color_b: Color::shex("707070"),
                border_color_l: Color::shex("707070"),
                border_color_r: Color::shex("707070"),
                text_body: TextBody {
                    base_attrs: TextAttrs {
                        height: Pixels(16.0),
                        color: Color::shex("303030"),
                        ..Default::default()
                    },
                    ..TextBody::from("Button")
                },
                ..Default::default()
            })
            .expect_valid();

        button.on_press(MouseButton::Left, move |_, window, local| {
            println!("{:?} {:?}", window, local);
            Default::default()
        });

        let mut renderer = Renderer::new(window).unwrap();
        renderer.interface_only();

        match renderer.run() {
            Ok(_) | Err(RendererError::Closed) => (),
            Err(e) => {
                println!("{:?}", e);
            },
        }

        basalt.exit();
    });
}
