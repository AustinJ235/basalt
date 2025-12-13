use std::thread::spawn;

use basalt::interface::UnitValue::Pixels;
use basalt::interface::widgets::Theme;
use basalt::interface::{BinStyle, TextAttrs, TextBody, TextHoriAlign, TextVertAlign};
use basalt::render::{MSAA, Renderer, RendererError};
use basalt::window::WindowBackend;
use basalt::{Basalt, BasaltOptions};

fn main() {
    Basalt::initialize(
        BasaltOptions::default().window_backend(WindowBackend::Wayland),
        move |basalt_res| {
            let basalt = basalt_res.unwrap();
            let mut thrd_handles = Vec::new();

            for monitor in basalt.window_manager_ref().monitors().unwrap() {
                let basalt = basalt.clone();

                thrd_handles.push(spawn(move || {
                    let monitor_name = monitor.name();

                    let window = basalt
                        .window_manager_ref()
                        .create()
                        .wl_layer()
                        .unwrap()
                        .wl_layer_bottom()
                        .unwrap()
                        .wl_layer_anchor(true, false, false, false)
                        .unwrap()
                        .size([monitor.resolution()[0], 40])
                        .wl_layer_exclusive_zone(40)
                        .unwrap()
                        .monitor(monitor)
                        .unwrap()
                        .build()
                        .unwrap();

                    let theme = Theme::default();
                    let background = window.new_bin();

                    background
                        .style_update(BinStyle {
                            pos_from_t: Pixels(0.0),
                            pos_from_b: Pixels(0.0),
                            pos_from_l: Pixels(0.0),
                            pos_from_r: Pixels(0.0),
                            back_color: theme.colors.back1,
                            text_body: TextBody {
                                base_attrs: TextAttrs {
                                    height: Pixels(18.0),
                                    ..Default::default()
                                },
                                hori_align: TextHoriAlign::Center,
                                vert_align: TextVertAlign::Center,
                                ..TextBody::from(format!("Monitor: {monitor_name}"))
                            },
                            ..Default::default()
                        })
                        .expect_valid();

                    let mut renderer = Renderer::new(window).unwrap();
                    renderer.interface_only().msaa(MSAA::X8);

                    match renderer.run() {
                        Ok(_) | Err(RendererError::Closed) => (),
                        Err(e) => {
                            eprintln!("{:?}", e);
                        },
                    }
                }));
            }

            for thrd_handle in thrd_handles {
                thrd_handle.join().unwrap();
            }

            basalt.exit();
        },
    );
}
