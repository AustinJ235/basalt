Basalt is a window/ui framework for building desktop applications or providing a ui a top other applications. In the backend vulkano which is a safe rust wrapper around vulkan. Basalt provides window creation, advance input handling, and along with the ui itself. The UI is based on the idea of a Bin. A Bin can have borders, backgrounds, and text and is the the fundamental element for building any ui widget/element. Currently the amount of provided widgets/elements is limited.

The project is very much a work in progress and is what I work on the side. Some issues exists, but nothing preventing you from creating a full-fledged app!

```rust
use basalt::input::MouseButton;
use basalt::interface::bin::{self, BinPosition, BinStyle};
use basalt::{Basalt, BstOptions};

fn main() {
    Basalt::initialize(
        BstOptions::default()
            .window_size(300, 300)
            .title("Basalt")
            .app_loop(),
        Box::new(move |basalt_res| {
            let basalt = basalt_res.unwrap();
            let background = basalt.interface_ref().new_bin();

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

            let button = basalt.interface_ref().new_bin();
            background.add_child(button.clone());

            button
                .style_update(BinStyle {
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
                })
                .expect_valid();

            button.on_press(MouseButton::Left, move |_, window, local| {
                println!("{:?} {:?}", window, local);
                Default::default()
            });

            basalt.wait_for_exit().unwrap();
        }),
    );
}
```
