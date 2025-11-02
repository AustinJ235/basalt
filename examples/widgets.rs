use std::sync::Arc;
use std::time::Duration;

use basalt::input::Qwerty;
use basalt::interface::BinStyle;
use basalt::interface::UnitValue::Pixels;
use basalt::interface::widgets::{RadioButtonGroup, Theme, Container, WidgetPlacement};
use basalt::interval::IntvlHookCtrl;
use basalt::render::{MSAA, Renderer, RendererError};
use basalt::{Basalt, BasaltOptions};

fn main() {
    Basalt::initialize(BasaltOptions::default(), move |basalt_res| {
        let basalt = basalt_res.unwrap();

        let window = basalt
            .window_manager_ref()
            .create()
            .title("widgets")
            .size([717, 332])
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
            println!(
                "Fullscreen: {:?}",
                window.toggle_full_screen(true, Default::default())
            );
            Default::default()
        });

        let frame = window
            .create_widget()
            .with_placement(WidgetPlacement {
                pos_from_t: Pixels(0.0),
                pos_from_b: Pixels(0.0),
                pos_from_l: Pixels(0.0),
                pos_from_r: Pixels(0.0),
                ..Default::default()
            })
            .frame()
            .build();

        let _button = frame.create_widget().button().text("Button").build();

        let _spin_button = frame
            .create_widget()
            .spin_button()
            .max_value(100)
            .medium_step(5)
            .large_step(10)
            .build()
            .unwrap();

        let _toggle_button = frame
            .create_widget()
            .toggle_button()
            .enabled_text("On")
            .disabled_text("Off")
            .build();

        let _switch_button = frame.create_widget().switch_button().build();

        let _scaler = frame
            .create_widget()
            .scaler()
            .max_value(100.0)
            .small_step(1.0)
            .medium_step(5.0)
            .large_step(10.0)
            .build()
            .unwrap();

        // Progress Bar

        let progress_bar = frame.create_widget().progress_bar().set_pct(100.0).build();

        progress_bar.on_press(|progress_bar, pct| {
            progress_bar.set_pct(pct);
        });

        let wk_progress_bar = Arc::downgrade(&progress_bar);
        let mut progress = 0.0;

        let hook_id =
            basalt
                .interval_ref()
                .do_every(Duration::from_millis(10), None, move |elapsed_op| {
                    if let Some(elapsed) = elapsed_op {
                        progress += elapsed.as_millis() as f32 / 20.0;

                        if progress > 100.0 {
                            progress = 0.0;
                        }

                        match wk_progress_bar.upgrade() {
                            Some(progress_bar) => {
                                progress_bar.set_pct(progress);
                                IntvlHookCtrl::Continue
                            },
                            None => IntvlHookCtrl::Remove,
                        }
                    } else {
                        IntvlHookCtrl::Continue
                    }
                });

        basalt.interval_ref().start(hook_id);

        // Radio Buttons

        #[derive(PartialEq, Debug)]
        enum RadioValue {
            A,
            B,
            C,
        }

        let radio_group = RadioButtonGroup::new();

        let _radio_a = frame
            .create_widget()
            .radio_button(RadioValue::A)
            .group(&radio_group)
            .build();

        let _radio_b = frame
            .create_widget()
            .radio_button(RadioValue::B)
            .group(&radio_group)
            .build();

        let _radio_c = frame
            .create_widget()
            .radio_button(RadioValue::C)
            .group(&radio_group)
            .build();

        radio_group.on_change(move |radio_op| {
            println!("radio value: {:?}", radio_op.map(|radio| radio.value_ref()));
        });

        // Check Boxes

        let _check_a = frame.create_widget().check_box(()).build();
        let _check_b = frame.create_widget().check_box(()).build();
        let _check_c = frame.create_widget().check_box(()).build();

        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum Options {
            A,
            B,
            C,
            D,
            E,
        }

        let _select = frame
            .create_widget()
            .select::<Options>()
            .add_option(Options::A, "Option A")
            .add_option(Options::B, "Option B")
            .add_option(Options::C, "Option C")
            .add_option(Options::D, "Option D")
            .add_option(Options::E, "Option E")
            .no_selection_label("Select an option...")
            .on_select(|_, selected| {
                println!("{:?}", selected);
            })
            .build();

        let _text_entry = frame
            .create_widget()
            .text_entry()
            .with_text("Enter text here...")
            .build();

        let _text_area = frame
            .create_widget()
            .text_editor()
            .with_text(
                "Lorem ipsum dolor sit amet consectetur adipiscing elit. Quisque faucibus ex \
                 sapien vitae pellentesque sem placerat. In id cursus mi pretium tellus duis \
                 convallis. Tempus leo eu aenean sed diam urna tempor. Pulvinar vivamus fringilla \
                 lacus nec metus bibendum egestas. Iaculis massa nisl malesuada lacinia integer \
                 nunc posuere. Ut hendrerit semper vel class aptent taciti sociosqu. Ad litora \
                 torquent per conubia nostra inceptos himenaeos.",
            )
            .build();

        let _code_editor = frame
            .create_widget()
            .code_editor()
            .with_text(
                r#"fn main() {
    println!("Hello World!");
}
"#,
            )
            .build();

        let mut renderer = Renderer::new(window).unwrap();
        renderer.interface_only().msaa(MSAA::X8);

        match renderer.run() {
            Ok(_) | Err(RendererError::Closed) => (),
            Err(e) => {
                println!("{:?}", e);
            },
        }

        basalt.exit();
    });
}
