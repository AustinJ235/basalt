use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use basalt::input::{MouseButton, Qwerty};
use basalt::interface::UnitValue::Pixels;
use basalt::interface::{BinStyle, Color, TextAttrs, TextBody, TextCursor, TextSelection};
use basalt::render::{Renderer, RendererError};
use basalt::window::WindowOptions;
use basalt::{Basalt, BasaltOptions};
use parking_lot::Mutex;

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
                    spans: vec![
                        "Enter Text Here\n\n...".into(),
                        basalt::interface::TextSpan {
                            attrs: TextAttrs {
                                height: Pixels(20.0),
                                ..Default::default()
                            },
                            .."\nAnother Span Here".into()
                        },
                    ],
                    base_attrs: TextAttrs {
                        height: Pixels(16.0),
                        color: Color::shex("101010"),
                        ..Default::default()
                    },
                    cursor_color: Color::shex("101010"),
                    ..Default::default()
                },
                ..Default::default()
            })
            .expect_valid();

        struct State {
            cursor_op: Option<TextCursor>,
        }

        let state = Arc::new(Mutex::new(State {
            cursor_op: None,
        }));

        let selecting = Arc::new(AtomicBool::new(false));

        let cb_state = state.clone();
        let cb_selecting = selecting.clone();

        text_area.on_press(MouseButton::Left, move |target, window, _| {
            let text_area = target.into_bin().unwrap();
            let text_cursor_op = text_area.get_text_cursor(window.cursor_pos());

            text_area.style_modify(|style| {
                style.text_body.cursor = text_cursor_op;
                style.text_body.selection = None;
            });

            if text_cursor_op.is_some() {
                cb_selecting.store(true, atomic::Ordering::Relaxed);
            }

            cb_state.lock().cursor_op = text_cursor_op;
            Default::default()
        });

        let cb_selecting = selecting.clone();

        text_area.on_release(MouseButton::Left, move |_, _, _| {
            cb_selecting.store(false, atomic::Ordering::Relaxed);
            Default::default()
        });

        let cb_state = state.clone();
        let cb_selecting = selecting.clone();

        text_area.on_cursor(move |target, window, _| {
            if cb_selecting.load(atomic::Ordering::Relaxed) {
                let text_area = target.into_bin().unwrap();
                let state = cb_state.lock();

                if state.cursor_op.is_none() {
                    return Default::default();
                }

                let start = state.cursor_op.unwrap();
                let end_op = text_area.get_text_cursor(window.cursor_pos());

                text_area.style_modify(|style| {
                    match end_op {
                        Some(end) => {
                            style.text_body.selection = if end == start {
                                None
                            } else if end < start {
                                Some(TextSelection {
                                    start: end,
                                    end: start,
                                })
                            } else {
                                Some(TextSelection {
                                    start,
                                    end,
                                })
                            };
                        },
                        None => {
                            style.text_body.selection = None;
                        },
                    }

                    if let Some(selection) = style.text_body.selection {
                        println!(
                            "Selected: \"{}\"",
                            style.text_body.selection_value(selection).unwrap()
                        );
                    }
                });
            }

            Default::default()
        });

        let cb_state = state.clone();

        text_area.on_press(Qwerty::ArrowLeft, move |target, _, _| {
            let mut state = cb_state.lock();
            let text_area = target.into_bin().unwrap();

            text_area.style_modify(|style| {
                if state.cursor_op.is_none() {
                    return;
                }

                let curr_cursor = state.cursor_op.unwrap();
                let prev_cursor = style
                    .text_body
                    .cursor_prev(curr_cursor)
                    .unwrap_or(curr_cursor);
                style.text_body.cursor = Some(prev_cursor);
                state.cursor_op = Some(prev_cursor);
                println!("{:?}", prev_cursor);
            });

            Default::default()
        });

        let cb_state = state.clone();

        text_area.on_press(Qwerty::ArrowRight, move |target, _, _| {
            let mut state = cb_state.lock();
            let text_area = target.into_bin().unwrap();

            text_area.style_modify(|style| {
                if state.cursor_op.is_none() {
                    return;
                }

                let curr_cursor = state.cursor_op.unwrap();
                let next_cursor = style
                    .text_body
                    .cursor_next(curr_cursor)
                    .unwrap_or(curr_cursor);
                style.text_body.cursor = Some(next_cursor);
                state.cursor_op = Some(next_cursor);
                println!("{:?}", next_cursor);
            });

            Default::default()
        });

        let cb_state = state;

        text_area.on_character(move |target, _, mut c| {
            let mut state = cb_state.lock();
            let text_area = target.into_bin().unwrap();

            text_area.style_modify(|style| {
                if c.is_backspace() {
                    if state.cursor_op.is_none() {
                        return;
                    }

                    state.cursor_op = style.text_body.cursor_delete(state.cursor_op.unwrap());
                    style.text_body.cursor = state.cursor_op;
                } else {
                    if c.0 == '\r' {
                        c.0 = '\n';
                    }

                    if state.cursor_op.is_none() {
                        state.cursor_op = Some(style.text_body.push(*c));
                        style.text_body.cursor = state.cursor_op;
                    } else {
                        state.cursor_op =
                            style.text_body.cursor_insert(state.cursor_op.unwrap(), *c);
                        style.text_body.cursor = state.cursor_op;
                    }
                }
            });

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
