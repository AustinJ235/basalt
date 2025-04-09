use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use parking_lot::Mutex;

use crate::input::{InputHookCtrl, MouseButton};
use crate::interface::UnitValue::Pixels;
use crate::interface::{Bin, BinStyle, Color, Position, TextAttrs, TextBody, TextHoriAlign};
use crate::window::Window;

/// ***Obsolete:** This is retained in a semi-working/untested state until widgets are implemented.*
pub struct OnOffButton {
    pub container: Arc<Bin>,
    theme: OnOffButtonTheme,
    enabled: AtomicBool,
    on: Arc<Bin>,
    off: Arc<Bin>,
    on_change_fns: Mutex<Vec<Box<dyn FnMut(bool) + Send + 'static>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnOffButtonTheme {
    /// Color of the container when off
    pub color1: Color,
    /// Color of the container when on
    pub color2: Color,
    /// Color of the inner slidy bit
    pub color3: Color,
    /// Color of the off text color
    pub color4: Color,
    /// Color of the on text color
    pub color5: Color,
}

impl Default for OnOffButtonTheme {
    fn default() -> Self {
        OnOffButtonTheme {
            color1: Color::shex("ff0000d0"),
            color2: Color::shex("00ff00d0"),
            color3: Color::shex("000000f0"),
            color4: Color::shex("ffffffff"),
            color5: Color::shex("ffffffff"),
        }
    }
}

impl OnOffButton {
    pub fn new(
        window: Arc<Window>,
        theme: OnOffButtonTheme,
        parent: Option<Arc<Bin>>,
    ) -> Arc<Self> {
        let mut bins = window.new_bins(3);
        let container = bins.pop().unwrap();
        let on = bins.pop().unwrap();
        let off = bins.pop().unwrap();
        container.add_child(on.clone());
        container.add_child(off.clone());

        if let Some(parent) = parent.as_ref() {
            parent.add_child(container.clone());
        }

        container
            .style_update(BinStyle {
                position: Position::Relative,
                pos_from_t: Pixels(0.0),
                pos_from_l: Pixels(0.0),
                width: Pixels(60.0),
                height: Pixels(24.0),
                border_radius_tl: 3.0,
                border_radius_bl: 3.0,
                border_radius_tr: 3.0,
                border_radius_br: 3.0,
                back_color: theme.color1,
                ..BinStyle::default()
            })
            .expect_valid();

        off.style_update(BinStyle {
            position: Position::Relative,
            pos_from_t: Pixels(2.0),
            pos_from_l: Pixels(2.0),
            pos_from_b: Pixels(2.0),
            width: Pixels(28.0),
            padding_t: Pixels(5.0),
            text: TextBody {
                spans: vec!["Off".into()],
                hori_align: TextHoriAlign::Center,
                base_attrs: TextAttrs {
                    color: theme.color4,
                    height: Pixels(12.0),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..BinStyle::default()
        })
        .expect_valid();

        on.style_update(BinStyle {
            position: Position::Relative,
            pos_from_t: Pixels(2.0),
            pos_from_r: Pixels(2.0),
            pos_from_b: Pixels(2.0),
            width: Pixels(28.0),
            border_radius_tl: 3.0,
            border_radius_bl: 3.0,
            border_radius_tr: 3.0,
            border_radius_br: 3.0,
            back_color: theme.color3,
            ..BinStyle::default()
        })
        .expect_valid();

        let ret = Arc::new(OnOffButton {
            container,
            theme,
            enabled: AtomicBool::new(false),
            on,
            off,
            on_change_fns: Mutex::new(Vec::new()),
        });

        let button_wk = Arc::downgrade(&ret);

        ret.on.on_press(MouseButton::Left, move |_, _, _| {
            match button_wk.upgrade() {
                Some(button) => {
                    button.toggle();
                    Default::default()
                },
                None => InputHookCtrl::Remove,
            }
        });

        let button_wk = Arc::downgrade(&ret);

        ret.off.on_press(MouseButton::Left, move |_, _, _| {
            match button_wk.upgrade() {
                Some(button) => {
                    button.toggle();
                    Default::default()
                },
                None => InputHookCtrl::Remove,
            }
        });

        let button_wk = Arc::downgrade(&ret);

        ret.container.on_press(MouseButton::Left, move |_, _, _| {
            match button_wk.upgrade() {
                Some(button) => {
                    button.toggle();
                    Default::default()
                },
                None => InputHookCtrl::Remove,
            }
        });

        ret
    }

    pub fn toggle(&self) -> bool {
        let cur = self.enabled.load(atomic::Ordering::Relaxed);
        self.set(!cur);
        !cur
    }

    pub fn is_on(&self) -> bool {
        self.enabled.load(atomic::Ordering::Relaxed)
    }

    pub fn on_change<F: FnMut(bool) + Send + 'static>(&self, func: F) {
        self.on_change_fns.lock().push(Box::new(func));
    }

    pub fn set(&self, on: bool) {
        self.enabled.store(on, atomic::Ordering::Relaxed);

        if !on {
            self.container
                .style_update(BinStyle {
                    back_color: self.theme.color1,
                    ..self.container.style_copy()
                })
                .expect_valid();

            self.on
                .style_update(BinStyle {
                    position: Position::Relative,
                    pos_from_t: Pixels(2.0),
                    pos_from_r: Pixels(2.0),
                    pos_from_b: Pixels(2.0),
                    width: Pixels(28.0),
                    border_radius_tl: 3.0,
                    border_radius_bl: 3.0,
                    border_radius_tr: 3.0,
                    border_radius_br: 3.0,
                    back_color: self.theme.color3,
                    ..BinStyle::default()
                })
                .expect_valid();

            self.off
                .style_update(BinStyle {
                    position: Position::Relative,
                    pos_from_t: Pixels(2.0),
                    pos_from_l: Pixels(2.0),
                    pos_from_b: Pixels(2.0),
                    width: Pixels(28.0),
                    padding_t: Pixels(5.0),
                    text: TextBody {
                        spans: vec!["Off".into()],
                        hori_align: TextHoriAlign::Center,
                        base_attrs: TextAttrs {
                            color: self.theme.color4,
                            height: Pixels(12.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..BinStyle::default()
                })
                .expect_valid();
        } else {
            self.container
                .style_update(BinStyle {
                    back_color: self.theme.color2,
                    ..self.container.style_copy()
                })
                .expect_valid();

            self.on
                .style_update(BinStyle {
                    position: Position::Relative,
                    pos_from_t: Pixels(2.0),
                    pos_from_r: Pixels(2.0),
                    pos_from_b: Pixels(2.0),
                    width: Pixels(28.0),
                    padding_t: Pixels(5.0),
                    text: TextBody {
                        spans: vec!["On".into()],
                        hori_align: TextHoriAlign::Center,
                        base_attrs: TextAttrs {
                            color: self.theme.color5,
                            height: Pixels(12.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..BinStyle::default()
                })
                .expect_valid();

            self.off
                .style_update(BinStyle {
                    position: Position::Relative,
                    pos_from_t: Pixels(2.0),
                    pos_from_l: Pixels(2.0),
                    pos_from_b: Pixels(2.0),
                    width: Pixels(28.0),
                    border_radius_tl: 3.0,
                    border_radius_bl: 3.0,
                    border_radius_tr: 3.0,
                    border_radius_br: 3.0,
                    back_color: self.theme.color3,
                    ..BinStyle::default()
                })
                .expect_valid();
        }

        for func in self.on_change_fns.lock().iter_mut() {
            func(on);
        }
    }
}
