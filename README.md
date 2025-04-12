Basalt is a window/ui framework for building desktop applications or providing a ui a top other applications. In the backend vulkano which is a safe rust wrapper around vulkan. Basalt provides window creation, advance input handling, and along with the ui itself. The UI is based on the idea of a Bin. A Bin can have borders, backgrounds, and text and is the the fundamental element for building any ui widget/element. Currently the amount of provided widgets/elements is limited.

The project is very much a work in progress and is what I work on the side. Some issues exists, but nothing preventing you from creating a full-fledged app!

```rust
use basalt::input::MouseButton;
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
```
