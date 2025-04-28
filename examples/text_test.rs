use std::sync::Arc;

use basalt::input::{MouseButton, Qwerty};
use basalt::interface::UnitValue::Pixels;
use basalt::interface::{BinStyle, Color, TextAttrs, TextBody, TextCursor, TextCursorAffinity};
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

        struct State {
            cursor_op: Option<TextCursor>,
        }

        let state = Arc::new(Mutex::new(State {
            cursor_op: None,
        }));

        let cb_state = state.clone();

        text_area.on_press(MouseButton::Left, move |target, window, _| {
            let text_area = target.into_bin().unwrap();
            let text_cursor_op = text_area.get_text_cursor(window.cursor_pos());
            println!("{:?}", text_cursor_op);
            cb_state.lock().cursor_op = text_cursor_op;
            Default::default()
        });

        let cb_state = state;

        text_area.on_character(move |target, _, c| {
            let mut state = cb_state.lock();
            let text_area = target.into_bin().unwrap();

            text_area.style_modify(|style| {
                if c.is_backspace() {
                    if state.cursor_op.is_none() {
                        return;
                    }

                    let cursor = state.cursor_op.as_mut().unwrap();
                    let text = &mut style.text_body.spans[cursor.span].text;

                    let rm_i = match cursor.affinity {
                        TextCursorAffinity::Before => {
                            if cursor.byte_s == 0 {
                                return;
                            }

                            let mut rm_i = 0;

                            for i in (0..cursor.byte_s).rev() {
                                if text.is_char_boundary(i) {
                                    rm_i = i;
                                    break;
                                }
                            }

                            rm_i
                        },
                        TextCursorAffinity::After => cursor.byte_s,
                    };

                    text.remove(rm_i);
                    let mut utf8_len = 1;

                    for i in (0..rm_i).rev() {
                        if text.is_char_boundary(i) {
                            break;
                        } else {
                            utf8_len += 1;
                        }
                    }

                    if rm_i < utf8_len {
                        if !text.is_empty() {
                            let first_c = text.chars().next().unwrap();
                            cursor.byte_s = 0;
                            cursor.byte_e = first_c.len_utf8();
                            cursor.affinity = TextCursorAffinity::Before;
                        } else {
                            state.cursor_op = None;
                        }
                    } else {
                        cursor.byte_s = cursor.byte_s - utf8_len;
                        cursor.byte_e = cursor.byte_s + utf8_len;
                        cursor.affinity = TextCursorAffinity::After;
                    }
                } else {
                    if state.cursor_op.is_none() {
                        if style.text_body.spans.is_empty() {
                            style.text_body.spans.push(Default::default());
                        }

                        let span = style.text_body.spans.last_mut().unwrap();
                        let byte_s = span.text.len();
                        let byte_e = byte_s + c.0.len_utf8();
                        span.text.push(*c);

                        state.cursor_op = Some(TextCursor {
                            span: style.text_body.spans.len() - 1,
                            byte_s,
                            byte_e,
                            affinity: TextCursorAffinity::After,
                        });

                        return;
                    }

                    let cursor = state.cursor_op.as_mut().unwrap();

                    match cursor.affinity {
                        TextCursorAffinity::Before => {
                            style.text_body.spans[cursor.span]
                                .text
                                .insert(cursor.byte_s, *c);

                            cursor.byte_e = cursor.byte_s + c.0.len_utf8();
                            cursor.affinity = TextCursorAffinity::After;
                        },
                        TextCursorAffinity::After => {
                            style.text_body.spans[cursor.span]
                                .text
                                .insert(cursor.byte_e, *c);

                            cursor.byte_s = cursor.byte_e;
                            cursor.byte_e = cursor.byte_s + c.0.len_utf8();
                        },
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
