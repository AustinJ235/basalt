use std::sync::Arc;
use std::time::Duration;

use basalt::interface::BinStyle;
use basalt::interface::UnitValue::Pixels;
use basalt::interface::widgets::{RadioButtonGroup, Theme, WidgetContainer};
use basalt::interval::IntvlHookCtrl;
use basalt::render::{MSAA, Renderer, RendererError};
use basalt::window::WindowOptions;
use basalt::{Basalt, BasaltOptions};

fn main() {
    Basalt::initialize(BasaltOptions::default(), move |basalt_res| {
        let basalt = basalt_res.unwrap();
        let theme = Theme::default();

        /*.create(WindowOptions {
            title: String::from("basalt-widgets"),
            inner_size: Some([720, 360]),
            ..WindowOptions::default()
        })
        .unwrap();*/

        let background = window.new_bin();

        background
            .style_update(BinStyle {
                pos_from_t: Pixels(0.0),
                pos_from_b: Pixels(0.0),
                pos_from_l: Pixels(0.0),
                pos_from_r: Pixels(0.0),
                back_color: theme.colors.back1,
                ..Default::default()
            })
            .expect_valid();

        let _button = background.create_widget().button().text("Button").build();

        let _spin_button = background
            .create_widget()
            .spin_button()
            .max_value(100)
            .medium_step(5)
            .large_step(10)
            .build()
            .unwrap();

        let _toggle_button = background
            .create_widget()
            .toggle_button()
            .enabled_text("On")
            .disabled_text("Off")
            .build();

        let _switch_button = background.create_widget().switch_button().build();

        let _scaler = background
            .create_widget()
            .scaler()
            .max_value(100.0)
            .small_step(1.0)
            .medium_step(5.0)
            .large_step(10.0)
            .build()
            .unwrap();

        // Progress Bar

        let progress_bar = background
            .create_widget()
            .progress_bar()
            .set_pct(100.0)
            .build();

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

        let _radio_a = background
            .create_widget()
            .radio_button(RadioValue::A)
            .group(&radio_group)
            .build();

        let _radio_b = background
            .create_widget()
            .radio_button(RadioValue::B)
            .group(&radio_group)
            .build();

        let _radio_c = background
            .create_widget()
            .radio_button(RadioValue::C)
            .group(&radio_group)
            .build();

        radio_group.on_change(move |radio_op| {
            println!("radio value: {:?}", radio_op.map(|radio| radio.value_ref()));
        });

        // Check Boxes

        let _check_a = background.create_widget().check_box(()).build();
        let _check_b = background.create_widget().check_box(()).build();
        let _check_c = background.create_widget().check_box(()).build();

        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum Options {
            A,
            B,
            C,
            D,
            E,
        }

        let _select = background
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

        let _text_entry = background
            .create_widget()
            .text_entry()
            .with_text("Enter text here...")
            .build();

        let _text_area = background
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

        let _code_editor = background
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
