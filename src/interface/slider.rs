use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::bin::{Bin, BinPosition, BinStyle, Color, KeepAlive, TextWrap};
use crate::input::{InputHookCtrl, InputHookID, MouseButton, Qwerty};
use crate::Basalt;

impl KeepAlive for Slider {}

pub struct Slider {
    pub basalt: Arc<Basalt>,
    pub container: Arc<Bin>,
    pub slidy_bit: Arc<Bin>,
    pub input_box: Arc<Bin>,
    pub slide_back: Arc<Bin>,
    data: Mutex<Data>,
    on_change: Mutex<Vec<Box<dyn FnMut(f32) + Send + 'static>>>,
    hooks: Mutex<Vec<InputHookID>>,
}

struct Data {
    min: f32,
    max: f32,
    at: f32,
    step: f32,
    method: Method,
}

impl Data {
    fn apply_method(&mut self) {
        match self.method {
            Method::Float => return,
            Method::RoundToStep => {
                self.at -= self.min;
                self.at /= self.step;
                self.at = f32::round(self.at);
                self.at *= self.step;
                self.at += self.min;
            },
            Method::RoundToInt => {
                self.at = f32::round(self.at);
            },
        }
        if self.at > self.max {
            self.at = self.max;
        } else if self.at < self.min {
            self.at = self.min;
        }
    }
}

pub enum Method {
    Float,
    RoundToStep,
    RoundToInt,
}

impl Drop for Slider {
    fn drop(&mut self) {
        let mut hooks = self.hooks.lock();

        for id in hooks.split_off(0) {
            self.basalt.input_ref().remove_hook(id);
        }
    }
}

impl Slider {
    pub fn set_min_max(&self, min: f32, max: f32) {
        let mut data = self.data.lock();
        data.min = min;
        data.max = max;
    }

    pub fn min_max(&self) -> (f32, f32) {
        let data = self.data.lock();
        (data.min, data.max)
    }

    pub fn at(&self) -> f32 {
        self.data.lock().at
    }

    pub fn set_step_size(&self, size: f32) {
        self.data.lock().step = size;
    }

    pub fn on_change<F: FnMut(f32) + Send + 'static>(&self, func: F) {
        self.on_change.lock().push(Box::new(func));
    }

    pub fn set_method(&self, method: Method) {
        self.data.lock().method = method;
    }

    pub fn new(basalt: Arc<Basalt>, parent_op: Option<Arc<Bin>>) -> Arc<Slider> {
        let mut bins = basalt.interface_ref().new_bins(4);
        let slider = Arc::new(Slider {
            basalt: basalt.clone(),
            container: bins.pop().unwrap(),
            slide_back: bins.pop().unwrap(),
            slidy_bit: bins.pop().unwrap(),
            input_box: bins.pop().unwrap(),
            data: Mutex::new(Data {
                min: 0.0,
                max: 100.0,
                at: 0.0,
                step: 10.0,
                method: Method::Float,
            }),
            on_change: Mutex::new(Vec::new()),
            hooks: Mutex::new(Vec::new()),
        });

        if let Some(parent) = parent_op {
            parent.add_child(slider.container.clone());
        }

        slider.slide_back.add_child(slider.slidy_bit.clone());
        slider.container.add_child(slider.input_box.clone());
        slider.container.add_child(slider.slide_back.clone());

        slider
            .container
            .style_update(BinStyle {
                position: Some(BinPosition::Parent),
                ..BinStyle::default()
            })
            .debug(); // TODO:

        slider
            .slidy_bit
            .style_update(BinStyle {
                position: Some(BinPosition::Parent),
                add_z_index: Some(100),
                pos_from_l: Some(30.0),
                pos_from_t: Some(-3.0),
                pos_from_b: Some(-3.0),
                width: Some(10.0),
                border_size_t: Some(1.0),
                border_size_b: Some(1.0),
                border_size_l: Some(1.0),
                border_size_r: Some(1.0),
                border_color_t: Some(Color::from_hex("808080")),
                border_color_b: Some(Color::from_hex("808080")),
                border_color_l: Some(Color::from_hex("808080")),
                border_color_r: Some(Color::from_hex("808080")),
                back_color: Some(Color::from_hex("f8f8f8")),
                ..BinStyle::default()
            })
            .expect_valid();

        slider
            .input_box
            .style_update(BinStyle {
                position: Some(BinPosition::Parent),
                pos_from_t: Some(1.0),
                pos_from_b: Some(1.0),
                pos_from_r: Some(0.0),
                pad_l: Some(5.0),
                text_height: Some(14.0),
                width: Some(60.0),
                border_size_t: Some(1.0),
                border_size_b: Some(1.0),
                border_size_l: Some(1.0),
                border_size_r: Some(1.0),
                border_color_t: Some(Color::from_hex("808080")),
                border_color_b: Some(Color::from_hex("808080")),
                border_color_l: Some(Color::from_hex("808080")),
                border_color_r: Some(Color::from_hex("808080")),
                back_color: Some(Color::from_hex("f8f8f8")),
                text_wrap: Some(TextWrap::None),
                ..BinStyle::default()
            })
            .expect_valid();

        slider
            .slide_back
            .style_update(BinStyle {
                position: Some(BinPosition::Parent),
                pos_from_t: Some(13.0),
                pos_from_b: Some(13.0),
                pos_from_l: Some(0.0),
                pos_from_r: Some(70.0),
                border_size_t: Some(1.0),
                border_size_b: Some(1.0),
                border_size_l: Some(1.0),
                border_size_r: Some(1.0),
                border_color_t: Some(Color::from_hex("f8f8f8")),
                border_color_b: Some(Color::from_hex("f8f8f8")),
                border_color_l: Some(Color::from_hex("f8f8f8")),
                border_color_r: Some(Color::from_hex("f8f8f8")),
                back_color: Some(Color::from_hex("808080")),
                overflow_y: Some(true),
                overflow_x: Some(true),
                ..BinStyle::default()
            })
            .expect_valid();

        let slider_cp = Arc::downgrade(&slider);

        slider.slide_back.on_update(move |_, _| {
            let slider_cp = match slider_cp.upgrade() {
                Some(some) => some,
                None => return,
            };

            slider_cp.force_update(None);
        });

        let mut hooks = slider.hooks.lock();
        let sliding = Arc::new(AtomicBool::new(false));
        let focused = Arc::new(AtomicBool::new(false));
        let slider_wk = Arc::downgrade(&slider);
        let sliding_cp = sliding.clone();
        let focused_cp = focused.clone();

        hooks.push(
            basalt
                .input_ref()
                .hook()
                .window(&basalt.window())
                .on_press()
                .keys(MouseButton::Left)
                .call(move |_, window, _| {
                    let slider = match slider_wk.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    let [mouse_x, mouse_y] = window.cursor_pos();

                    if slider.slidy_bit.mouse_inside(mouse_x, mouse_y) {
                        sliding_cp.store(true, atomic::Ordering::SeqCst);
                    }

                    if slider.container.mouse_inside(mouse_x, mouse_y) {
                        focused_cp.store(true, atomic::Ordering::SeqCst);
                    } else {
                        focused_cp.store(false, atomic::Ordering::SeqCst);
                    }

                    Default::default()
                })
                .finish()
                .unwrap(),
        );

        let sliding_cp = sliding.clone();

        hooks.push(
            basalt
                .input_ref()
                .hook()
                .window(&basalt.window())
                .on_release()
                .keys(MouseButton::Left)
                .call(move |_, _, _| {
                    sliding_cp.store(false, atomic::Ordering::SeqCst);
                    Default::default()
                })
                .finish()
                .unwrap(),
        );

        let slider_wk = Arc::downgrade(&slider);

        hooks.push(
            basalt
                .input_ref()
                .hook()
                .window(&basalt.window())
                .on_scroll()
                .call(move |_, window, scroll_amt, _| {
                    let slider = match slider_wk.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    let [mouse_x, mouse_y] = window.cursor_pos();

                    if slider.container.mouse_inside(mouse_x, mouse_y) {
                        if scroll_amt > 0.0 {
                            slider.increment();
                        } else {
                            slider.decrement();
                        }
                    }

                    Default::default()
                })
                .finish()
                .unwrap(),
        );

        let focused_cp = focused.clone();
        let slider_wk = Arc::downgrade(&slider);

        hooks.push(
            basalt
                .input_ref()
                .hook()
                .window(&basalt.window())
                .on_hold()
                .keys(Qwerty::ArrowRight)
                .interval(Duration::from_millis(150))
                .call(move |_, _, _| {
                    let slider = match slider_wk.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    if focused_cp.load(atomic::Ordering::SeqCst) {
                        slider.increment();
                    }

                    Default::default()
                })
                .finish()
                .unwrap(),
        );

        let slider_wk = Arc::downgrade(&slider);

        hooks.push(
            basalt
                .input_ref()
                .hook()
                .window(&basalt.window())
                .on_hold()
                .keys(Qwerty::ArrowLeft)
                .interval(Duration::from_millis(150))
                .call(move |_, _, _| {
                    let slider = match slider_wk.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    if focused.load(atomic::Ordering::SeqCst) {
                        slider.decrement();
                    }

                    Default::default()
                })
                .finish()
                .unwrap(),
        );

        let slider_wk = Arc::downgrade(&slider);

        hooks.push(
            basalt
                .input_ref()
                .hook()
                .window(&basalt.window())
                .on_cursor()
                .call(move |_, window, _| {
                    let slider = match slider_wk.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    if sliding.load(atomic::Ordering::SeqCst) {
                        let [mouse_x, _] = window.cursor_pos();
                        let back_bps = slider.slide_back.post_update();
                        let back_width = back_bps.tro[0] - back_bps.tlo[0];
                        let sbit_style = slider.slidy_bit.style_copy();
                        let sbit_width = sbit_style.width.unwrap_or(0.0);
                        let sbit_bordl = sbit_style.border_size_l.unwrap_or(0.0);
                        let sbit_bordr = sbit_style.border_size_r.unwrap_or(0.0);
                        let mut from_l = mouse_x - back_bps.tlo[0] - (sbit_width / 2.0);
                        let max_from_l = back_width - sbit_width - sbit_bordl - sbit_bordr;

                        if from_l < 0.0 {
                            from_l = 0.0;
                        } else if from_l > max_from_l {
                            from_l = max_from_l;
                        }

                        let mut percent = from_l / max_from_l;
                        let mut data = slider.data.lock();
                        data.at = ((data.max - data.min) * percent) + data.min;
                        data.apply_method();
                        percent = (data.at - data.min) / (data.max - data.min);
                        from_l = max_from_l * percent;

                        slider
                            .slidy_bit
                            .style_update(BinStyle {
                                pos_from_l: Some(from_l),
                                ..sbit_style
                            })
                            .expect_valid();

                        slider
                            .input_box
                            .style_update(BinStyle {
                                text: format!("{}", data.at),
                                ..slider.input_box.style_copy()
                            })
                            .expect_valid();

                        for func in slider.on_change.lock().iter_mut() {
                            func(data.at);
                        }
                    }

                    Default::default()
                })
                .finish()
                .unwrap(),
        );

        drop(hooks);
        slider
    }

    pub fn set(&self, val: f32) {
        let mut data = self.data.lock();
        data.at = val;

        if data.at > data.max {
            data.at = data.max;
        } else if data.at < data.min {
            data.at = data.min;
        }

        self.force_update(Some(&mut *data));
    }

    pub fn increment(&self) {
        let mut data = self.data.lock();
        data.at += data.step;

        if data.at > data.max {
            data.at = data.max;
        }

        self.force_update(Some(&mut *data));
    }

    pub fn decrement(&self) {
        let mut data = self.data.lock();
        data.at -= data.step;

        if data.at < data.min {
            data.at = data.min;
        }

        self.force_update(Some(&mut *data));
    }

    fn force_update(&self, data: Option<&mut Data>) {
        let (percent, at, changed) = match data {
            Some(data) => ((data.at - data.min) / (data.max - data.min), data.at, true),
            None => {
                let data = self.data.lock();
                ((data.at - data.min) / (data.max - data.min), data.at, false)
            },
        };

        let back_bps = self.slide_back.post_update();
        let back_width = back_bps.tro[0] - back_bps.tlo[0];
        let sbit_style = self.slidy_bit.style_copy();
        let sbit_width = sbit_style.width.unwrap_or(0.0);
        let sbit_bordl = sbit_style.border_size_l.unwrap_or(0.0);
        let sbit_bordr = sbit_style.border_size_r.unwrap_or(0.0);
        let max_from_l = back_width - sbit_bordl - sbit_bordr - sbit_width;
        let set_from_l = max_from_l * percent;

        self.slidy_bit
            .style_update(BinStyle {
                pos_from_l: Some(set_from_l),
                ..sbit_style
            })
            .expect_valid();

        self.input_box
            .style_update(BinStyle {
                text: format!("{}", at),
                ..self.input_box.style_copy()
            })
            .expect_valid();

        if changed {
            for func in self.on_change.lock().iter_mut() {
                func(at);
            }
        }
    }
}
