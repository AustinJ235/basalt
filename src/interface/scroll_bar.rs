use std::sync::Arc;

use parking_lot::Mutex;

use crate::input::{InputHookCtrl, MouseButton};
use crate::interface::{Bin, BinPosition, BinStyle, BinVert, Color};
use crate::window::Window;

pub struct ScrollBarStyle {
    pub border_color: Color,
    pub arrow_color: Color,
    pub bar_color: Color,
    pub back_color: Color,
}

impl Default for ScrollBarStyle {
    fn default() -> Self {
        ScrollBarStyle {
            back_color: Color::shex("35353c"),
            bar_color: Color::shex("f0f0f0"),
            arrow_color: Color::shex("f0f0f0"),
            border_color: Color::shex("222227"),
        }
    }
}

pub struct ScrollBar {
    pub back: Arc<Bin>,
    pub up: Arc<Bin>,
    pub down: Arc<Bin>,
    pub bar: Arc<Bin>,
    scroll: Arc<Bin>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScrollTo {
    Same,
    Top,
    Bottom,
    Percent(f32),
    Amount(f32),
    Set(f32),
}

impl ScrollBar {
    /// # Notes
    /// - Panics if parent bin is not associated to the window provided.
    pub fn new(
        window: Arc<Window>,
        style: Option<ScrollBarStyle>,
        parent: Option<Arc<Bin>>,
        scroll: Arc<Bin>,
    ) -> Arc<Self> {
        if let Some(parent) = parent.as_ref() {
            match parent.window() {
                Some(parent_window) => {
                    if window != parent_window {
                        panic!("parent bin is not associated to the window provided");
                    }
                },
                None => {
                    panic!("parent bin is not associated to a window");
                },
            }
        }

        let style = style.unwrap_or_default();
        let mut bins = window.new_bins(4);
        let back = bins.pop().unwrap();
        let up = bins.pop().unwrap();
        let down = bins.pop().unwrap();
        let bar = bins.pop().unwrap();
        let position = match parent {
            Some(parent) => {
                parent.add_child(back.clone());
                BinPosition::Parent
            },
            None => BinPosition::Window,
        };

        back.add_child(up.clone());
        back.add_child(down.clone());
        back.add_child(bar.clone());

        back.style_update(BinStyle {
            position: Some(position),
            pos_from_t: Some(0.0),
            pos_from_b: Some(0.0),
            pos_from_r: Some(0.0),
            width: Some(15.0),
            back_color: Some(style.back_color),
            border_size_l: Some(1.0),
            border_color_l: Some(style.border_color),
            ..BinStyle::default()
        })
        .expect_valid();

        up.style_update(BinStyle {
            position: Some(BinPosition::Parent),
            pos_from_t: Some(0.0),
            pos_from_l: Some(0.0),
            pos_from_r: Some(0.0),
            height: Some(13.0),
            custom_verts: vec![
                BinVert {
                    position: (7.5, 4.0, 0),
                    color: style.arrow_color,
                },
                BinVert {
                    position: (4.0, 9.0, 0),
                    color: style.arrow_color,
                },
                BinVert {
                    position: (11.0, 9.0, 0),
                    color: style.arrow_color,
                },
            ],
            ..BinStyle::default()
        })
        .expect_valid();

        down.style_update(BinStyle {
            position: Some(BinPosition::Parent),
            pos_from_b: Some(0.0),
            pos_from_l: Some(0.0),
            pos_from_r: Some(0.0),
            height: Some(13.0),
            custom_verts: vec![
                BinVert {
                    position: (11.0, 4.0, 0),
                    color: style.arrow_color,
                },
                BinVert {
                    position: (4.0, 4.0, 0),
                    color: style.arrow_color,
                },
                BinVert {
                    position: (7.5, 9.0, 0),
                    color: style.arrow_color,
                },
            ],
            ..BinStyle::default()
        })
        .expect_valid();

        bar.style_update(BinStyle {
            position: Some(BinPosition::Parent),
            pos_from_t: Some(15.0),
            pos_from_b: Some(15.0),
            pos_from_l: Some(2.0),
            pos_from_r: Some(2.0),
            back_color: Some(style.bar_color),
            ..BinStyle::default()
        })
        .expect_valid();

        let sb = Arc::new(ScrollBar {
            back,
            up,
            down,
            bar,
            scroll,
        });

        let sb_wk = Arc::downgrade(&sb);
        let drag_data: Arc<Mutex<Option<(f32, f32)>>> = Arc::new(Mutex::new(None));
        let drag_data_cp = drag_data.clone();

        sb.bar.on_press(MouseButton::Left, move |_, window, _| {
            let sb = match sb_wk.upgrade() {
                Some(some) => some,
                None => return InputHookCtrl::Remove,
            };

            let [_, mouse_y] = window.cursor_pos();
            let scroll_y = sb.scroll.style_copy().scroll_y.unwrap_or(0.0);
            *drag_data_cp.lock() = Some((mouse_y, scroll_y));
            Default::default()
        });

        let drag_data_cp = drag_data.clone();

        sb.bar.on_release(MouseButton::Left, move |_, _, _| {
            *drag_data_cp.lock() = None;
            Default::default()
        });

        let sb_wk = Arc::downgrade(&sb);

        sb.bar.attach_input_hook(
            window
                .basalt_ref()
                .input_ref()
                .hook()
                .window(&window)
                .on_cursor()
                .call(move |_, window, _| {
                    let sb = match sb_wk.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    let [_, mouse_y] = window.cursor_pos();
                    let drag_data_op = drag_data.lock();

                    let drag_data = match drag_data_op.as_ref() {
                        Some(some) => some,
                        None => return Default::default(),
                    };

                    let overflow = sb.scroll.calc_vert_overflow();
                    let up_post = sb.up.post_update();
                    let down_post = sb.down.post_update();
                    let max_bar_h = down_post.tlo[1] - up_post.blo[1];
                    let mut bar_sp = overflow / 10.0;
                    let mut bar_h = max_bar_h - bar_sp;

                    if bar_h < 3.0 {
                        bar_h = 3.0;
                        bar_sp = max_bar_h - bar_h;
                    }

                    let bar_inc = overflow / bar_sp;
                    sb.update(ScrollTo::Set(
                        drag_data.1 + ((mouse_y - drag_data.0) * bar_inc),
                    ));
                    Default::default()
                })
                .finish()
                .unwrap(),
        );

        let sb_wk = Arc::downgrade(&sb);

        sb.scroll.on_update(move |_, _| {
            if let Some(sb) = sb_wk.upgrade() {
                sb.back.trigger_update();
                let sb_wk = Arc::downgrade(&sb);

                sb.back.on_update_once(move |_, _| {
                    if let Some(sb) = sb_wk.upgrade() {
                        sb.update(ScrollTo::Same);
                    }
                });
            }
        });

        let sb_wk = Arc::downgrade(&sb);

        sb.scroll.on_children_added(move |_, _| {
            if let Some(sb) = sb_wk.upgrade() {
                sb.back.trigger_update();
                let sb_wk = Arc::downgrade(&sb);

                sb.back.on_update_once(move |_, _| {
                    if let Some(sb) = sb_wk.upgrade() {
                        sb.update(ScrollTo::Same);
                    }
                });
            }
        });

        let sb_wk = Arc::downgrade(&sb);

        sb.scroll.on_children_removed(move |_, _| {
            if let Some(sb) = sb_wk.upgrade() {
                sb.back.trigger_update();
                let sb_wk = Arc::downgrade(&sb);

                sb.back.on_update_once(move |_, _| {
                    if let Some(sb) = sb_wk.upgrade() {
                        sb.update(ScrollTo::Same);
                    }
                });
            }
        });

        let sb_wk = Arc::downgrade(&sb);

        sb.back.on_update(move |_, _| {
            if let Some(sb) = sb_wk.upgrade() {
                sb.update(ScrollTo::Same);
            }
        });

        let sb_wk = Arc::downgrade(&sb);

        sb.up.on_press(MouseButton::Left, move |_, _, _| {
            match sb_wk.upgrade() {
                Some(sb) => {
                    sb.update(ScrollTo::Amount(-10.0));
                    Default::default()
                },
                None => InputHookCtrl::Remove,
            }
        });

        let sb_wk = Arc::downgrade(&sb);

        sb.down.on_press(MouseButton::Left, move |_, _, _| {
            match sb_wk.upgrade() {
                Some(sb) => {
                    sb.update(ScrollTo::Amount(10.0));
                    Default::default()
                },
                None => InputHookCtrl::Remove,
            }
        });

        let sb_wk = Arc::downgrade(&sb);

        sb.back.attach_input_hook(
            window
                .basalt_ref()
                .input_ref()
                .hook()
                .bin(&sb.back)
                .on_scroll()
                .enable_smooth(true)
                .call(move |_, _, mut amt, _| {
                    amt = amt.round();

                    if amt == 0.0 {
                        return Default::default();
                    }

                    match sb_wk.upgrade() {
                        Some(sb) => {
                            sb.update(ScrollTo::Amount(amt));
                            Default::default()
                        },
                        None => InputHookCtrl::Remove,
                    }
                })
                .finish()
                .unwrap(),
        );

        let sb_wk = Arc::downgrade(&sb);

        sb.scroll.attach_input_hook(
            window
                .basalt_ref()
                .input_ref()
                .hook()
                .bin(&sb.scroll)
                .on_scroll()
                .enable_smooth(true)
                .upper_blocks(true)
                .call(move |_, _, mut amt, _| {
                    amt = amt.round();

                    if amt == 0.0 {
                        return Default::default();
                    }

                    match sb_wk.upgrade() {
                        Some(sb) => {
                            sb.update(ScrollTo::Amount(amt));
                            Default::default()
                        },
                        None => crate::input::InputHookCtrl::Remove,
                    }
                })
                .finish()
                .unwrap(),
        );

        sb
    }

    pub fn update(&self, amount: ScrollTo) {
        let mut scroll_y = self.scroll.style_copy().scroll_y.unwrap_or(0.0);
        let overflow = self.scroll.calc_vert_overflow();

        if match amount {
            ScrollTo::Same => {
                if scroll_y > overflow {
                    scroll_y = overflow;
                    true
                } else {
                    false
                }
            },
            ScrollTo::Top => {
                if scroll_y == 0.0 {
                    false
                } else {
                    scroll_y = 0.0;
                    true
                }
            },
            ScrollTo::Bottom => {
                if scroll_y == overflow {
                    false
                } else {
                    scroll_y = overflow;
                    true
                }
            },
            ScrollTo::Percent(p) => {
                if p.is_sign_positive() {
                    if scroll_y == overflow {
                        false
                    } else {
                        let amt = overflow * p;

                        if scroll_y + amt > overflow {
                            scroll_y = overflow;
                        } else {
                            scroll_y += amt;
                        }

                        true
                    }
                } else if scroll_y == 0.0 {
                    false
                } else {
                    let amt = overflow * p;

                    if scroll_y + amt < 0.0 {
                        scroll_y = 0.0;
                    } else {
                        scroll_y += amt;
                    }

                    true
                }
            },
            ScrollTo::Amount(amt) => {
                if amt.is_sign_positive() {
                    if scroll_y == overflow {
                        false
                    } else {
                        if scroll_y + amt > overflow {
                            scroll_y = overflow;
                        } else {
                            scroll_y += amt;
                        }

                        true
                    }
                } else if scroll_y == 0.0 {
                    false
                } else {
                    if scroll_y + amt < 0.0 {
                        scroll_y = 0.0;
                    } else {
                        scroll_y += amt;
                    }

                    true
                }
            },
            ScrollTo::Set(to) => {
                if to < 0.0 {
                    if scroll_y == 0.0 {
                        false
                    } else {
                        scroll_y = 0.0;
                        true
                    }
                } else if to > overflow {
                    if scroll_y == overflow {
                        false
                    } else {
                        scroll_y = overflow;
                        true
                    }
                } else {
                    scroll_y = to;
                    true
                }
            },
        } {
            self.scroll
                .style_update(BinStyle {
                    scroll_y: Some(scroll_y),
                    ..self.scroll.style_copy()
                })
                .expect_valid();
        }

        let up_post = self.up.post_update();
        let down_post = self.down.post_update();
        let max_bar_h = down_post.tlo[1] - up_post.blo[1];

        if max_bar_h < 3.0 {
            // println!("Scroll bar less than minimum height.");
        }

        let mut bar_sp = overflow / 10.0;
        let mut bar_h = max_bar_h - bar_sp;

        if bar_h < 3.0 {
            bar_h = 3.0;
            bar_sp = max_bar_h - bar_h;
        }

        let bar_inc = overflow / bar_sp;
        let bar_pos = scroll_y / bar_inc;

        self.bar
            .style_update(BinStyle {
                pos_from_t: Some(bar_pos + up_post.blo[1] - up_post.tlo[1]),
                pos_from_b: None,
                height: Some(bar_h),
                ..self.bar.style_copy()
            })
            .expect_valid();
    }
}
