Basalt is a window/ui framework for building desktop applications or providing a ui a top other applications. In the backend vulkano which is a safe rust wrapper around vulkan. Basalt provides window creation, advance input handling, and along with the ui itself. The UI is based on the idea of a Bin. A Bin can have borders, backgrounds, and text and is the the fundamental element for building any ui widget/element. Currently the amount of provided widgets/elements is limited.

The project is very much a work in progress and is what I work on the side. Some issues exists, but nothing preventing you from creating a full-fledged app!

```rust
use basalt::input::MouseButton;
use basalt::interface::{BinPosition, BinStyle, Color};
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

        let background = window.new_bin();

        background
            .style_update(BinStyle {
                pos_from_t: Some(0.0),
                pos_from_b: Some(0.0),
                pos_from_l: Some(0.0),
                pos_from_r: Some(0.0),
                back_color: Some(Color::shex("f0f0f0")),
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
                back_color: Some(Color::shex("c0c0c0")),
                border_size_t: Some(1.0),
                border_size_b: Some(1.0),
                border_size_l: Some(1.0),
                border_size_r: Some(1.0),
                border_color_t: Some(Color::shex("707070")),
                border_color_b: Some(Color::shex("707070")),
                border_color_l: Some(Color::shex("707070")),
                border_color_r: Some(Color::shex("707070")),
                text: String::from("Button"),
                text_height: Some(16.0),
                pad_t: Some(7.0),
                pad_l: Some(8.0),
                text_color: Some(Color::shex("303030")),
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

```
