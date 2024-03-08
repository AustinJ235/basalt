use std::sync::Arc;

use basalt::input::Qwerty;
use basalt::interface::bin;
use basalt::interface::bin::{BinPosition, BinStyle};
use basalt::renderer::AutoMultiWindowRenderer;
use basalt::window::{Window, WindowOptions};
use basalt::{Basalt, BstOptions};

fn main() {
    Basalt::initialize(
        BstOptions::default(),
        Box::new(move |basalt_res| {
            let basalt = basalt_res.unwrap();

            basalt.window_manager_ref().on_open(|window| {
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

                window.on_press(Qwerty::O, move |target, _, _| {
                    let window = target
                        .into_window()
                        .unwrap()
                        .basalt_ref()
                        .window_manager_ref()
                        .create(WindowOptions {
                            title: String::from("app"),
                            inner_size: Some([400; 2]),
                            ..WindowOptions::default()
                        })
                        .unwrap();

                    add_bins_to_window(window);
                    Default::default()
                });
            });

            // Create a scope for the initial window. Keeping a reference to a window before calling
            // AutoMultiWindowRenderer::run will result in the window not properly closing.
            {
                let window = basalt
                    .window_manager_ref()
                    .create(WindowOptions {
                        title: String::from("app"),
                        inner_size: Some([400; 2]),
                        ..WindowOptions::default()
                    })
                    .unwrap();

                add_bins_to_window(window);
            }

            AutoMultiWindowRenderer::new(basalt.clone())
                .run(true)
                .unwrap();
            basalt.exit();
        }),
    );
}

fn add_bins_to_window(window: Arc<Window>) {
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

    let window_id_disp = window.new_bin();
    background.add_child(window_id_disp.clone());

    window_id_disp
        .style_update(BinStyle {
            position: Some(BinPosition::Parent),
            pos_from_t: Some(0.0),
            pos_from_l: Some(0.0),
            pos_from_r: Some(0.0),
            height: Some(32.0),
            back_color: Some(bin::Color::srgb_hex("c0c0c0")),
            border_size_b: Some(1.0),
            border_color_b: Some(bin::Color::srgb_hex("707070")),
            text: format!("{:?}", window.id()),
            text_height: Some(16.0),
            pad_t: Some(7.0),
            pad_l: Some(8.0),
            text_color: Some(bin::Color::srgb_hex("303030")),
            ..BinStyle::default()
        })
        .expect_valid();

    window.keep_alive([background, window_id_disp]);
}
