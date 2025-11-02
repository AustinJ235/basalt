use std::cell::RefCell;
use std::f32::consts::PI;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::time::Duration;

use parking_lot::ReentrantMutex;

use crate::image::ImageKey;
use crate::input::{InputHookCtrl, MouseButton};
use crate::interface::UnitValue::{
    PctOfHeight, PctOfHeightOffset, PctOfWidth, PctOfWidthOffset, Percent, Pixels,
};
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::button::{BtnHookColors, button_hooks};
use crate::interface::widgets::{Container, Theme, WidgetPlacement};
use crate::interface::{Bin, BinID, BinStyle, BinVertex, Color, Position, StyleUpdateBatch};
use crate::ulps_eq;

/// Determintes the orientation and axis of the [`ScrollBar`].
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    /// The [`ScrollBar`] will control the x-axis and be oriented horizontally.
    X,
    /// The [`ScrollBar`] will control the y-axis and be oriented vertically.
    ///
    /// **Note**: This is the default.
    #[default]
    Y,
}

struct ScrollBarProperties {
    axis: ScrollAxis,
    smooth: bool,
    step: f32,
    accel: bool,
    accel_pow: f32,
    max_accel_mult: f32,
    animation_duration: Duration,
}

/// Builder for [`ScrollBar`]
pub struct ScrollBarBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    properties: ScrollBarProperties,
    target: Arc<Bin>,
    placement_op: Option<WidgetPlacement>,
    on_create_scroll_to_op: Option<f32>,
}

impl<'a, C> ScrollBarBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>, target: Arc<Bin>) -> Self {
        Self {
            placement_op: builder.placement.take(),
            widget: builder,
            properties: ScrollBarProperties {
                axis: ScrollAxis::Y,
                smooth: true,
                step: 50.0,
                accel: true,
                accel_pow: 1.2,
                max_accel_mult: 4.0,
                animation_duration: Duration::from_millis(100),
            },
            target,
            on_create_scroll_to_op: None,
        }
    }

    /// Set the amount the target container should be scrolled initially.
    ///
    /// **Note**: If not set this defaults the current scroll amount defined by the target container.
    pub fn scroll(mut self, scroll: f32) -> Self {
        self.on_create_scroll_to_op = Some(scroll);
        self
    }

    /// Set the axis.
    ///
    /// See [`ScrollAxis`] docs for more information.
    ///
    /// **Note**: If not set this defaults to [`ScrollAxis::Y`].
    pub fn axis(mut self, axis: ScrollAxis) -> Self {
        self.properties.axis = axis;
        self
    }

    /// Set if smooth scroll is enabled.
    ///
    /// **Note**: If not set this defaults to `true`.
    pub fn smooth(mut self, smooth: bool) -> Self {
        self.properties.smooth = smooth;
        self
    }

    /// Set the step size per input event.
    ///
    /// **Note**: If not set this defaults to `50.0`.
    pub fn step(mut self, step: f32) -> Self {
        self.properties.step = step;
        self
    }

    /// Set if scroll acceleration is enabled.
    ///
    /// Acceleration behavior is defined by: step size, acceleration power, max acceleration
    /// multiplier and animation duration.
    ///
    /// Acceleration is applied when there is pending scroll events in the animation queue. The
    /// pending scroll amount is divided by step size and raised to the power of acceleration power.
    /// This value is then used as a multiplier on the new step size. The multiplier is capped by the
    /// max acceleration multiplier.
    ///
    /// **Notes**:
    /// - If not set this defaults to `true`.
    /// - Smooth scroll will be enabled if acceleration is enabled.
    pub fn accel(mut self, accel: bool) -> Self {
        self.properties.accel = accel;
        self
    }

    /// Set the acceleration power.
    ///
    /// **Notes**:
    /// - If not set this defaults to `1.2`.
    /// - Has no effect if acceleration is not enabled.
    pub fn accel_pow(mut self, accel_pow: f32) -> Self {
        self.properties.accel_pow = accel_pow;
        self
    }

    /// Set the max acceleration multiplier.
    ///
    /// **Notes**:
    /// - If not set this defaults to `4.0`.
    /// - Has no effect if acceleration is not enabled.
    pub fn max_accel_mult(mut self, max_accel_mult: f32) -> Self {
        self.properties.max_accel_mult = max_accel_mult;
        self
    }

    /// Set the duration of animations.
    ///
    /// **Notes**:
    /// - If not set this defaults to 100 ms.
    /// - Has no effect if smooth scroll or acceleration is not enabled.
    pub fn animation_duration(mut self, animation_duration: Duration) -> Self {
        self.properties.animation_duration = animation_duration;
        self
    }

    /// Finish building the [`ScrollBar`].
    pub fn build(self) -> Arc<ScrollBar> {
        let container = self.widget.container.create_bin();
        let mut bins = container.create_bins(3);
        let upright = bins.next().unwrap();
        let downleft = bins.next().unwrap();
        let confine = bins.next().unwrap();
        let bar = confine.create_bin();
        drop(bins);

        let scroll = self.on_create_scroll_to_op.unwrap_or_else(|| {
            self.target.style_inspect(|style| {
                match self.properties.axis {
                    ScrollAxis::X => style.scroll_x,
                    ScrollAxis::Y => style.scroll_y,
                }
            })
        });

        let placement = self.placement_op.unwrap_or_else(|| {
            ScrollBar::default_placement(&self.widget.theme, self.properties.axis)
        });

        let scroll_bar = Arc::new(ScrollBar {
            theme: self.widget.theme,
            properties: self.properties,
            target: self.target,
            container,
            upright,
            downleft,
            confine,
            bar,
            state: ReentrantMutex::new(State {
                target: RefCell::new(TargetState {
                    overflow: scroll,
                    scroll,
                    size: 0.0,
                }),
                smooth: RefCell::new(SmoothState {
                    run: false,
                    start: 0.0,
                    target: 0.0,
                    time: 0.0,
                }),
                drag: RefCell::new(DragState {
                    cursor_start: 0.0,
                    scroll_start: 0.0,
                    scroll_per_px: 0.0,
                }),
                placement: RefCell::new(placement),
            }),
        });

        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        scroll_bar.target.on_update(move |_, _| {
            if let Some(scroll_bar) = scroll_bar_wk.upgrade() {
                scroll_bar.refresh();
            }
        });

        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        scroll_bar.target.on_children_added(move |_, _| {
            if let Some(scroll_bar) = scroll_bar_wk.upgrade() {
                scroll_bar.refresh();
            }
        });

        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        scroll_bar.target.on_children_removed(move |_, _| {
            if let Some(scroll_bar) = scroll_bar_wk.upgrade() {
                scroll_bar.refresh();
            }
        });

        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        scroll_bar
            .target
            .basalt_ref()
            .input_ref()
            .hook()
            .bin(&scroll_bar.target)
            .on_scroll()
            .upper_blocks(true)
            .call(move |_, _, scroll_y, scroll_x| {
                let scroll_bar = match scroll_bar_wk.upgrade() {
                    Some(some) => some,
                    None => return InputHookCtrl::Remove,
                };

                match scroll_bar.properties.axis {
                    ScrollAxis::X => {
                        if scroll_x != 0.0 {
                            scroll_bar.scroll(scroll_x * scroll_bar.properties.step);
                        }
                    },
                    ScrollAxis::Y => {
                        if scroll_y != 0.0 {
                            scroll_bar.scroll(scroll_y * scroll_bar.properties.step);
                        }
                    },
                }

                Default::default()
            })
            .finish()
            .unwrap();

        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        scroll_bar
            .container
            .basalt_ref()
            .input_ref()
            .hook()
            .bin(&scroll_bar.container)
            .on_scroll()
            .upper_blocks(true)
            .call(move |_, _, scroll_y, scroll_x| {
                let scroll_bar = match scroll_bar_wk.upgrade() {
                    Some(some) => some,
                    None => return InputHookCtrl::Remove,
                };

                match scroll_bar.properties.axis {
                    ScrollAxis::X => {
                        if scroll_x != 0.0 {
                            scroll_bar.scroll(scroll_x * scroll_bar.properties.step);
                        }
                    },
                    ScrollAxis::Y => {
                        if scroll_y != 0.0 {
                            scroll_bar.scroll(scroll_y * scroll_bar.properties.step);
                        }
                    },
                }

                Default::default()
            })
            .finish()
            .unwrap();

        let bar_held = Arc::new(AtomicBool::new(false));
        let scroll_bar_wk = Arc::downgrade(&scroll_bar);
        let cb_bar_held = bar_held.clone();

        scroll_bar
            .bar
            .on_press(MouseButton::Left, move |_, w_state, _| {
                let scroll_bar = match scroll_bar_wk.upgrade() {
                    Some(some) => some,
                    None => return InputHookCtrl::Remove,
                };

                let [cursor_x, cursor_y] = w_state.cursor_pos();

                let cursor_start = match scroll_bar.properties.axis {
                    ScrollAxis::X => cursor_x,
                    ScrollAxis::Y => cursor_y,
                };

                let state = scroll_bar.state.lock();
                state.smooth.borrow_mut().run = false;

                let mut drag_state = state.drag.borrow_mut();
                drag_state.cursor_start = cursor_start;
                drag_state.scroll_start = state.target.borrow().scroll;

                cb_bar_held.store(true, atomic::Ordering::SeqCst);
                Default::default()
            });

        let cb_bar_held = bar_held.clone();

        scroll_bar
            .bar
            .on_release(MouseButton::Left, move |_, _, _| {
                cb_bar_held.store(false, atomic::Ordering::SeqCst);
                Default::default()
            });

        let cb_bar_held = bar_held;
        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        scroll_bar
            .container
            .attach_input_hook(scroll_bar.container.window().unwrap().on_cursor(
                move |_, w_state, _| {
                    let scroll_bar = match scroll_bar_wk.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    if cb_bar_held.load(atomic::Ordering::SeqCst) {
                        let [cursor_x, cursor_y] = w_state.cursor_pos();
                        let state = scroll_bar.state.lock();

                        let jump_to = {
                            let drag_state = state.drag.borrow_mut();

                            let delta = match scroll_bar.properties.axis {
                                ScrollAxis::X => cursor_x - drag_state.cursor_start,
                                ScrollAxis::Y => cursor_y - drag_state.cursor_start,
                            };

                            drag_state.scroll_start + (delta * drag_state.scroll_per_px)
                        };

                        scroll_bar.jump_to(jump_to);
                    }

                    Default::default()
                },
            ));

        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        scroll_bar
            .confine
            .on_press(MouseButton::Left, move |_, w_state, _| {
                let scroll_bar = match scroll_bar_wk.upgrade() {
                    Some(some) => some,
                    None => return InputHookCtrl::Remove,
                };

                let [cursor_x, cursor_y] = w_state.cursor_pos();
                let bar_bpu = scroll_bar.bar.post_update();
                let state = scroll_bar.state.lock();

                let delta = match scroll_bar.properties.axis {
                    ScrollAxis::X => {
                        cursor_x - (((bar_bpu.tri[0] - bar_bpu.tli[0]) / 2.0) + bar_bpu.tli[0])
                    },
                    ScrollAxis::Y => {
                        cursor_y - (((bar_bpu.bli[1] - bar_bpu.tli[1]) / 2.0) + bar_bpu.tli[1])
                    },
                };

                let scroll_to =
                    state.target.borrow().scroll + (delta * state.drag.borrow().scroll_per_px);

                scroll_bar.scroll_to(scroll_to);
                Default::default()
            });

        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        button_hooks(
            &scroll_bar.upright,
            BtnHookColors {
                vert_clr: Some(scroll_bar.theme.colors.border1),
                h_vert_clr: Some(scroll_bar.theme.colors.border3),
                p_vert_clr: Some(scroll_bar.theme.colors.border2),
                ..Default::default()
            },
            move |_| {
                if let Some(scroll_bar) = scroll_bar_wk.upgrade() {
                    scroll_bar.scroll(-scroll_bar.properties.step);
                }
            },
        );

        let scroll_bar_wk = Arc::downgrade(&scroll_bar);

        button_hooks(
            &scroll_bar.downleft,
            BtnHookColors {
                vert_clr: Some(scroll_bar.theme.colors.border1),
                h_vert_clr: Some(scroll_bar.theme.colors.border3),
                p_vert_clr: Some(scroll_bar.theme.colors.border2),
                ..Default::default()
            },
            move |_| {
                if let Some(scroll_bar) = scroll_bar_wk.upgrade() {
                    scroll_bar.scroll(scroll_bar.properties.step);
                }
            },
        );

        scroll_bar.style_update(true, None);
        scroll_bar
    }
}

/// Scroll bar widget
pub struct ScrollBar {
    theme: Theme,
    properties: ScrollBarProperties,
    target: Arc<Bin>, // TODO: Should this be Weak?
    container: Arc<Bin>,
    upright: Arc<Bin>,
    downleft: Arc<Bin>,
    confine: Arc<Bin>,
    bar: Arc<Bin>,
    state: ReentrantMutex<State>,
}

struct State {
    target: RefCell<TargetState>,
    smooth: RefCell<SmoothState>,
    drag: RefCell<DragState>,
    placement: RefCell<WidgetPlacement>,
}

struct TargetState {
    overflow: f32,
    scroll: f32,
    size: f32,
}

struct SmoothState {
    run: bool,
    start: f32,
    target: f32,
    time: f32,
}

struct DragState {
    cursor_start: f32,
    scroll_start: f32,
    scroll_per_px: f32,
}

impl ScrollBar {
    /// Scroll an amount of pixels.
    ///
    /// **Notes**:
    /// - This may be effected by acceleration.
    /// - If smooth scroll or acceleration are both disabled this uses [`ScrollBar::jump`].
    pub fn scroll(self: &Arc<Self>, amt: f32) {
        let state = self.state.lock();

        if !self.properties.accel && !self.properties.smooth {
            self.scroll_no_anim(amt);
            return;
        }

        let target_state = state.target.borrow();
        let mut smooth_state = state.smooth.borrow_mut();

        smooth_state.target = if !smooth_state.run {
            target_state.scroll + amt
        } else {
            let direction_changes = !ulps_eq(
                (smooth_state.target - target_state.scroll).signum(),
                amt.signum(),
                4,
            );

            if self.properties.accel {
                if direction_changes {
                    target_state.scroll + amt
                } else {
                    smooth_state.target
                        + (((smooth_state.target - target_state.scroll).abs()
                            / self.properties.step)
                            .max(1.0)
                            .powf(self.properties.accel_pow)
                            .clamp(1.0, self.properties.max_accel_mult)
                            * amt)
                }
            } else {
                if direction_changes {
                    target_state.scroll + amt
                } else {
                    smooth_state.target + amt
                }
            }
        };

        if ulps_eq(smooth_state.target, target_state.scroll, 4) {
            return;
        }

        if !smooth_state.run {
            smooth_state.run = true;
            self.run_smooth_scroll();
        }

        smooth_state.start = target_state.scroll;
        smooth_state.time = 0.0;
    }

    /// Scroll to a certain amount of pixels.
    ///
    /// **Note**: If smooth scroll or acceleration are both disabled this uses [`ScrollBar::jump_to`].
    pub fn scroll_to(self: &Arc<Self>, to: f32) {
        let state = self.state.lock();

        if !self.properties.accel && !self.properties.smooth {
            self.jump_to(to);
            return;
        }

        let target_state = state.target.borrow();
        let mut smooth_state = state.smooth.borrow_mut();

        if ulps_eq(target_state.scroll, to, 4) {
            smooth_state.run = false;
            return;
        }

        if !ulps_eq(smooth_state.target, to, 4) {
            if !smooth_state.run {
                smooth_state.run = true;
                self.run_smooth_scroll();
            }

            smooth_state.start = target_state.scroll;
            smooth_state.target = to;
            smooth_state.time = 0.0;
        }
    }

    /// Scroll to the minimum.
    ///
    /// If [`ScrollAxis`] is `Y` this it the top. If `X` then the left.
    ///
    /// **Note**: If smooth scroll or acceleration are both disabled this uses [`ScrollBar::jump_to_min`].
    pub fn scroll_to_min(self: &Arc<Self>) {
        self.scroll_to(0.0);
    }

    /// Scroll to the maximum.
    ///
    /// If [`ScrollAxis`] is `Y` this it the bottom. If `X` then the right.
    ///
    /// **Note**: If smooth scroll or acceleration are both disabled this uses [`ScrollBar::jump_to_max`].
    pub fn scroll_to_max(self: &Arc<Self>) {
        let state = self.state.lock();
        let max = state.target.borrow().overflow;
        self.scroll_to(max);
    }

    fn scroll_no_anim(&self, amt: f32) {
        let state = self.state.lock();
        let mut update = self.check_target_state();

        {
            let mut target_state = state.target.borrow_mut();

            if amt.is_sign_negative() {
                if !ulps_eq(target_state.scroll, 0.0, 4) {
                    if target_state.scroll + amt < 0.0 {
                        target_state.scroll = 0.0;
                    } else {
                        target_state.scroll += amt;
                    }

                    update = true;
                }
            } else {
                if !ulps_eq(target_state.scroll, target_state.overflow, 4) {
                    if target_state.scroll + amt > target_state.overflow {
                        target_state.scroll = target_state.overflow;
                    } else {
                        target_state.scroll += amt;
                    }

                    update = true;
                }
            }
        }

        if update {
            self.update();
        }
    }

    /// Jump an amount of pixels.
    ///
    /// **Note**: This is the same as [`ScrollBar::scroll`] but does not animate or accelerate.
    pub fn jump(&self, amt: f32) {
        let state = self.state.lock();
        state.smooth.borrow_mut().run = false;
        self.scroll_no_anim(amt);
    }

    /// Jump to a certain amount of pixels.
    ///
    /// **Note**: This is the same as [`ScrollBar::scroll_to`] but does not animate.
    pub fn jump_to(&self, to: f32) {
        self.jump_to_inner(to, true);
    }

    fn jump_to_inner(&self, to: f32, cancel_smooth: bool) {
        let state = self.state.lock();
        let mut update = self.check_target_state();

        {
            let mut target_state = state.target.borrow_mut();

            if cancel_smooth {
                state.smooth.borrow_mut().run = false;
            }

            if to > target_state.overflow {
                if !ulps_eq(target_state.scroll, target_state.overflow, 4) {
                    target_state.scroll = target_state.overflow;
                    update = true;
                }
            } else if to < 0.0 {
                if !ulps_eq(target_state.scroll, 0.0, 4) {
                    target_state.scroll = 0.0;
                    update = true;
                }
            } else {
                if !ulps_eq(target_state.scroll, to, 4) {
                    target_state.scroll = to;
                    update = true;
                }
            }
        }

        if update {
            self.update();
        }
    }

    /// Jump to the minimum.
    ///
    /// If [`ScrollAxis`] is `Y` this it the top. If `X` then the left.
    ///
    /// **Note**: This is the same as [`ScrollBar::scroll_to_min`] but does not animate.
    pub fn jump_to_min(&self) {
        let state = self.state.lock();
        let mut update = self.check_target_state();

        {
            let mut target_state = state.target.borrow_mut();
            state.smooth.borrow_mut().run = false;

            if !ulps_eq(target_state.scroll, 0.0, 4) {
                target_state.scroll = 0.0;
                update = true;
            }
        }

        if update {
            self.update();
        }
    }

    /// Jump to the minimum.
    ///
    /// If [`ScrollAxis`] is `Y` this it the bottom. If `X` then the right.
    ///
    /// **Note**: This is the same as [`ScrollBar::scroll_to_max`] but does not animate.
    pub fn jump_to_max(&self) {
        let state = self.state.lock();
        let mut update = self.check_target_state();

        {
            let mut target_state = state.target.borrow_mut();
            state.smooth.borrow_mut().run = false;

            if !ulps_eq(target_state.scroll, target_state.overflow, 4) {
                target_state.scroll = target_state.overflow;
                update = true;
            }
        }

        if update {
            self.update();
        }
    }

    /// Recheck the state and update if needed.
    ///
    /// **Note**: This may need to be called in certain cases.
    pub fn refresh(&self) {
        let _state = self.state.lock();

        if self.check_target_state() {
            self.update();
        }
    }

    /// The inner size of the target on the axis that is controlled.
    pub fn target_size(&self) -> f32 {
        let target_bpu = self.target.post_update();

        match self.properties.axis {
            ScrollAxis::X => target_bpu.tri[0] - target_bpu.tli[0],
            ScrollAxis::Y => target_bpu.bli[1] - target_bpu.tli[1],
        }
    }

    /// The amount of overflow of the target on the axis that is controlled.
    pub fn target_overflow(&self) -> f32 {
        match self.properties.axis {
            ScrollAxis::X => self.target.calc_hori_overflow(),
            ScrollAxis::Y => self.target.calc_vert_overflow(),
        }
    }

    /// The current amount the target is scrolled.
    pub fn current_scroll(&self) -> f32 {
        self.state.lock().target.borrow().scroll
    }

    /// The amount the target will be scrolled after animations.
    ///
    /// This will be the same value as [`ScrollBar::current_scroll`] when:
    /// - Smooth scroll and acceleration are disabled.
    /// - There is no animiation currently happening.
    pub fn target_scroll(&self) -> f32 {
        let state = self.state.lock();
        let smooth_state = state.smooth.borrow();

        if smooth_state.run {
            return smooth_state.target;
        }

        state.target.borrow().scroll
    }

    // TODO: Public?
    pub(crate) fn size(theme: &Theme) -> f32 {
        (theme.base_size / 1.5) + theme.border.unwrap_or(0.0)
    }

    pub(crate) fn has_bin_id(&self, bin_id: BinID) -> bool {
        bin_id == self.container.id()
            || bin_id == self.upright.id()
            || bin_id == self.downleft.id()
            || bin_id == self.confine.id()
            || bin_id == self.bar.id()
    }

    fn check_target_state(&self) -> bool {
        let target_overflow = self.target_overflow();
        let target_size = self.target_size();
        let state = self.state.try_lock_for(Duration::from_secs(5)).unwrap();
        let mut target_state = state.target.borrow_mut();
        let mut update = false;

        if !ulps_eq(target_state.overflow, target_overflow, 4) {
            target_state.overflow = target_overflow;
            update = true;
        }

        if target_overflow < target_state.scroll {
            target_state.scroll = target_overflow;
            update = true;
        }

        if !ulps_eq(target_state.size, target_size, 4) {
            target_state.size = target_size;
            update = true;
        }

        update
    }

    fn run_smooth_scroll(self: &Arc<Self>) {
        if let Some(window) = self.container.window() {
            let scroll_bar = self.clone();
            let animation_duration = self.properties.animation_duration.as_micros() as f32 / 1000.0;

            window.renderer_on_frame(move |elapsed_op| {
                let state = scroll_bar.state.lock();
                let mut smooth_state = state.smooth.borrow_mut();

                if !smooth_state.run {
                    return false;
                }

                if let Some(elapsed) = elapsed_op {
                    smooth_state.time += elapsed.as_micros() as f32 / 1000.0;
                }

                let delta = smooth_state.target - smooth_state.start;
                let linear_t = (smooth_state.time / animation_duration).clamp(0.0, 1.0);
                let smooth_t = (((linear_t + 1.5) * PI).sin() + 1.0) / 2.0;
                scroll_bar.jump_to_inner(smooth_state.start + (delta * smooth_t), false);
                smooth_state.run = smooth_state.time < animation_duration;
                smooth_state.run
            });
        }
    }

    fn update(&self) {
        let state = self.state.lock();
        let target_state = state.target.borrow();
        let confine_bpu = self.confine.post_update();

        let confine_size = match self.properties.axis {
            ScrollAxis::X => confine_bpu.tri[0] - confine_bpu.tli[0],
            ScrollAxis::Y => confine_bpu.bli[1] - confine_bpu.tli[1],
        };

        let confine_sec_size = match self.properties.axis {
            ScrollAxis::X => confine_bpu.bli[1] - confine_bpu.tli[1],
            ScrollAxis::Y => confine_bpu.tri[0] - confine_bpu.tli[0],
        };

        let [scroll_per_px, bar_size_pct, bar_offset_pct] =
            if target_state.overflow > 0.0 && target_state.size > 0.0 {
                let overflow_ratio = target_state.overflow / target_state.size;
                let max_space_size = confine_size - confine_sec_size;

                let space_size = ((-1.0 / ((0.25 * overflow_ratio) + 0.5) + 2.0)
                    * (max_space_size / 2.0))
                    .clamp(0.0, max_space_size);

                let scroll_per_px = target_state.overflow / space_size;
                let bar_size_pct = ((confine_size - space_size) / confine_size) * 100.0;
                let bar_offset_pct = ((target_state.scroll / scroll_per_px) / confine_size) * 100.0;

                [scroll_per_px, bar_size_pct, bar_offset_pct]
            } else {
                [0.0, 100.0, 0.0]
            };

        state.drag.borrow_mut().scroll_per_px = scroll_per_px;

        let mut bar_style = self.bar.style_copy();
        let mut target_style = self.target.style_copy();
        let mut target_style_update = false;

        match self.properties.axis {
            ScrollAxis::X => {
                if !ulps_eq(target_style.scroll_x, target_state.scroll, 4) {
                    target_style.scroll_x = target_state.scroll;
                    target_style_update = true;
                }

                bar_style.pos_from_l = Percent(bar_offset_pct);
                bar_style.width = Percent(bar_size_pct);
            },
            ScrollAxis::Y => {
                if !ulps_eq(target_style.scroll_y, target_state.scroll, 4) {
                    target_style.scroll_y = target_state.scroll;
                    target_style_update = true;
                }

                bar_style.pos_from_t = Percent(bar_offset_pct);
                bar_style.height = Percent(bar_size_pct);
            },
        }

        if target_style_update {
            Bin::style_update_batch([(&self.target, target_style), (&self.bar, bar_style)]);
        } else {
            self.bar.style_update(bar_style).expect_valid();
        }
    }

    pub fn update_placement(&self, placement: WidgetPlacement) {
        let state = self.state.lock();
        *state.placement.borrow_mut() = placement;
        self.style_update(false, None);
    }

    pub fn update_placement_with_batch<'a>(
        &'a self,
        placement: WidgetPlacement,
        batch: &mut StyleUpdateBatch<'a>,
    ) {
        let state = self.state.lock();
        *state.placement.borrow_mut() = placement;
        self.style_update(false, Some(batch));
    }

    /// Obtain the default [`WidgetPlacement`](`WidgetPlacement`) given a [`Theme`](`Theme`) and
    /// the [`ScrollAxis`](`ScrollAxis`).
    pub fn default_placement(theme: &Theme, axis: ScrollAxis) -> WidgetPlacement {
        match axis {
            ScrollAxis::X => {
                WidgetPlacement {
                    pos_from_b: Pixels(0.0),
                    pos_from_l: Pixels(0.0),
                    pos_from_r: Pixels(0.0),
                    height: Pixels((theme.base_size / 1.5).ceil()),
                    ..Default::default()
                }
            },
            ScrollAxis::Y => {
                WidgetPlacement {
                    pos_from_t: Pixels(0.0),
                    pos_from_b: Pixels(0.0),
                    pos_from_r: Pixels(0.0),
                    width: Pixels((theme.base_size / 1.5).ceil()),
                    ..Default::default()
                }
            },
        }
    }

    fn style_update<'a>(
        &'a self,
        initial_update: bool,
        batch_op: Option<&mut StyleUpdateBatch<'a>>,
    ) {
        let state = self.state.lock();
        let placement = state.placement.borrow().clone();
        let spacing = (self.theme.spacing / 10.0).ceil();
        let border_size = self.theme.border.unwrap_or(0.0);

        let mut container_style = BinStyle {
            back_color: self.theme.colors.back2,
            ..placement.clone().into_style()
        };

        let mut upright_style = BinStyle {
            ..Default::default()
        };

        let mut downleft_style = BinStyle {
            ..Default::default()
        };

        let mut confine_style = BinStyle {
            ..Default::default()
        };

        let mut bar_style = BinStyle {
            position: Position::Anchor,
            back_color: self.theme.colors.accent1,
            ..Default::default()
        };

        match self.properties.axis {
            ScrollAxis::X => {
                upright_style.pos_from_t = Pixels(0.0);
                upright_style.pos_from_b = Pixels(0.0);
                upright_style.pos_from_r = Pixels(0.0);
                upright_style.width = PctOfHeight(100.0);
                upright_style.user_vertexes = vec![(
                    ImageKey::INVALID,
                    right_symbol_verts(10.0, self.theme.colors.border1),
                )];

                downleft_style.pos_from_t = Pixels(0.0);
                downleft_style.pos_from_b = Pixels(0.0);
                downleft_style.pos_from_l = Pixels(0.0);
                downleft_style.width = PctOfHeight(100.0);
                downleft_style.user_vertexes = vec![(
                    ImageKey::INVALID,
                    left_symbol_verts(10.0, self.theme.colors.border1),
                )];

                confine_style.pos_from_t = Pixels(spacing);
                confine_style.pos_from_b = Pixels(spacing);
                confine_style.pos_from_l = PctOfHeightOffset(100.0, border_size);
                confine_style.pos_from_r = PctOfHeightOffset(100.0, border_size);

                bar_style.pos_from_t = Pixels(0.0);
                bar_style.pos_from_b = Pixels(0.0);

                if initial_update {
                    bar_style.pos_from_l = Percent(0.0);
                    bar_style.width = Percent(100.0);
                } else {
                    self.bar.style_inspect(|style| {
                        bar_style.pos_from_l = style.pos_from_l;
                        bar_style.width = style.width;
                    });
                }
            },
            ScrollAxis::Y => {
                upright_style.pos_from_t = Pixels(0.0);
                upright_style.pos_from_l = Pixels(0.0);
                upright_style.pos_from_r = Pixels(0.0);
                upright_style.height = PctOfWidth(100.0);
                upright_style.user_vertexes = vec![(
                    ImageKey::INVALID,
                    up_symbol_verts(10.0, self.theme.colors.border1),
                )];

                downleft_style.pos_from_b = Pixels(0.0);
                downleft_style.pos_from_l = Pixels(0.0);
                downleft_style.pos_from_r = Pixels(0.0);
                downleft_style.height = PctOfWidth(100.0);
                downleft_style.user_vertexes = vec![(
                    ImageKey::INVALID,
                    down_symbol_verts(10.0, self.theme.colors.border1),
                )];

                confine_style.pos_from_t = PctOfWidthOffset(100.0, border_size);
                confine_style.pos_from_b = PctOfWidthOffset(100.0, border_size);
                confine_style.pos_from_l = Pixels(spacing);
                confine_style.pos_from_r = Pixels(spacing);

                bar_style.pos_from_l = Pixels(0.0);
                bar_style.pos_from_r = Pixels(0.0);

                if initial_update {
                    bar_style.pos_from_t = Percent(0.0);
                    bar_style.height = Percent(100.0);
                } else {
                    self.bar.style_inspect(|style| {
                        bar_style.pos_from_t = style.pos_from_t;
                        bar_style.height = style.height;
                    });
                }
            },
        }

        if let Some(border_size) = self.theme.border {
            bar_style.border_size_t = Pixels(border_size);
            bar_style.border_size_b = Pixels(border_size);
            bar_style.border_size_l = Pixels(border_size);
            bar_style.border_size_r = Pixels(border_size);
            bar_style.border_color_t = self.theme.colors.border3;
            bar_style.border_color_b = self.theme.colors.border3;
            bar_style.border_color_l = self.theme.colors.border3;
            bar_style.border_color_r = self.theme.colors.border3;

            if !container_style.border_size_t.is_defined() {
                container_style.border_size_t = Pixels(border_size);
                container_style.border_color_t = self.theme.colors.border1;
            }

            if !container_style.border_size_b.is_defined() {
                container_style.border_size_b = Pixels(border_size);
                container_style.border_color_b = self.theme.colors.border1;
            }

            if !container_style.border_size_l.is_defined() {
                container_style.border_size_l = Pixels(border_size);
                container_style.border_color_l = self.theme.colors.border1;
            }

            if !container_style.border_size_r.is_defined() {
                container_style.border_size_r = Pixels(border_size);
                container_style.border_color_r = self.theme.colors.border1;
            }
        }

        if let Some(border_radius) = self.theme.roundness {
            match self.properties.axis {
                ScrollAxis::X => {
                    bar_style.border_radius_tl = PctOfHeight(50.0);
                    bar_style.border_radius_tr = PctOfHeight(50.0);
                    bar_style.border_radius_bl = PctOfHeight(50.0);
                    bar_style.border_radius_br = PctOfHeight(50.0);
                },
                ScrollAxis::Y => {
                    bar_style.border_radius_tl = PctOfWidth(50.0);
                    bar_style.border_radius_tr = PctOfWidth(50.0);
                    bar_style.border_radius_bl = PctOfWidth(50.0);
                    bar_style.border_radius_br = PctOfWidth(50.0);
                },
            }

            // TODO: This is a workaround!
            //       Since Bin's don't take into account border radius when cropping, this tries
            //       to guess the container's border radius, so it still displays correctly.

            let unit_val_is_zero =
                |val: crate::interface::UnitValue| val.px_width([100.0; 2]).unwrap_or(0.0) == 0.0;

            match (
                placement.pos_from_t.is_defined(),
                placement.pos_from_b.is_defined(),
                placement.pos_from_l.is_defined(),
                placement.pos_from_r.is_defined(),
            ) {
                (false, true, true, true) => {
                    if unit_val_is_zero(placement.pos_from_l) {
                        container_style.border_radius_bl = Pixels(border_radius);
                    }

                    if unit_val_is_zero(placement.pos_from_r) {
                        container_style.border_radius_br = Pixels(border_radius);
                    }
                },
                (true, false, true, true) => {
                    if unit_val_is_zero(placement.pos_from_l) {
                        container_style.border_radius_tl = Pixels(border_radius);
                    }

                    if unit_val_is_zero(placement.pos_from_r) {
                        container_style.border_radius_tr = Pixels(border_radius);
                    }
                },
                (true, true, false, true) => {
                    if unit_val_is_zero(placement.pos_from_t) {
                        container_style.border_radius_tr = Pixels(border_radius);
                    }

                    if unit_val_is_zero(placement.pos_from_b) {
                        container_style.border_radius_br = Pixels(border_radius);
                    }
                },
                (true, true, true, false) => {
                    if unit_val_is_zero(placement.pos_from_t) {
                        container_style.border_radius_tl = Pixels(border_radius);
                    }

                    if unit_val_is_zero(placement.pos_from_b) {
                        container_style.border_radius_bl = Pixels(border_radius);
                    }
                },
                _ => (), // TODO: ?
            }
        }

        let updates = [
            (&self.container, container_style),
            (&self.upright, upright_style),
            (&self.downleft, downleft_style),
            (&self.confine, confine_style),
            (&self.bar, bar_style),
        ];

        match batch_op {
            Some(batch) => batch.update_many(updates),
            None => StyleUpdateBatch::from(updates).commit(),
        }
    }
}

fn up_symbol_verts(space_pct: f32, color: Color) -> Vec<BinVertex> {
    symbol_verts(
        color,
        &[
            [50.0, 25.0 + (space_pct / 2.0)],
            [space_pct, 75.0 - (space_pct / 2.0)],
            [100.0 - space_pct, 75.0],
        ],
    )
}

pub(crate) fn down_symbol_verts(space_pct: f32, color: Color) -> Vec<BinVertex> {
    symbol_verts(
        color,
        &[
            [space_pct, 25.0 + (space_pct / 2.0)],
            [100.0 - space_pct, 25.0 + (space_pct / 2.0)],
            [50.0, 75.0 - (space_pct / 2.0)],
        ],
    )
}

fn left_symbol_verts(space_pct: f32, color: Color) -> Vec<BinVertex> {
    symbol_verts(
        color,
        &[
            [75.0 - (space_pct / 2.0), space_pct],
            [25.0 + (space_pct / 2.0), 50.0],
            [75.0 - (space_pct / 2.0), 100.0 - space_pct],
        ],
    )
}

fn right_symbol_verts(space_pct: f32, color: Color) -> Vec<BinVertex> {
    symbol_verts(
        color,
        &[
            [25.0 + (space_pct / 2.0), space_pct],
            [25.0 + (space_pct / 2.0), 100.0 - space_pct],
            [75.0 - (space_pct / 2.0), 50.0],
        ],
    )
}

fn symbol_verts(color: Color, unit_points: &[[f32; 2]; 3]) -> Vec<BinVertex> {
    unit_points
        .into_iter()
        .map(|[x, y]| {
            BinVertex {
                x: Percent(*x),
                y: Percent(*y),
                color,
                ..Default::default()
            }
        })
        .collect()
}
