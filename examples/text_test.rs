use basalt::input::{MouseButton, Qwerty};
use basalt::interface::UnitValue::Pixels;
use basalt::interface::{BinStyle, Color, TextAttrs, TextBody};
use basalt::render::{Renderer, RendererError};
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
                pos_from_t: Pixels(0.0),
                pos_from_b: Pixels(0.0),
                pos_from_l: Pixels(0.0),
                pos_from_r: Pixels(0.0),
                back_color: Color::shex("d0d0d0"),
                ..Default::default()
            })
            .expect_valid();

        let text_area = window.new_bin();
        background.add_child(text_area.clone());

        text_area
            .style_update(BinStyle {
                pos_from_t: Pixels(10.0),
                pos_from_l: Pixels(10.0),
                pos_from_r: Pixels(10.0),
                pos_from_b: Pixels(10.0),
                padding_t: Pixels(10.0),
                padding_b: Pixels(10.0),
                padding_l: Pixels(10.0),
                padding_r: Pixels(10.0),
                back_color: Color::shex("f8f8f8"),
                border_size_t: Pixels(1.0),
                border_size_b: Pixels(1.0),
                border_size_l: Pixels(1.0),
                border_size_r: Pixels(1.0),
                border_color_t: Color::shex("707070"),
                border_color_b: Color::shex("707070"),
                border_color_l: Color::shex("707070"),
                border_color_r: Color::shex("707070"),
                border_radius_tl: Pixels(5.0),
                border_radius_tr: Pixels(5.0),
                border_radius_bl: Pixels(5.0),
                border_radius_br: Pixels(5.0),
                text_body: TextBody {
                    base_attrs: TextAttrs {
                        height: Pixels(16.0),
                        color: Color::shex("101010"),
                        ..Default::default()
                    },
                    ..TextBody::from("Enter Text Here...")
                },
                ..Default::default()
            })
            .expect_valid();

        text_area.on_press(MouseButton::Left, move |target, window, _| {
            let text_area = target.into_bin().unwrap();
            println!("{:?}", text_area.get_text_cursor(window.cursor_pos()));
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
