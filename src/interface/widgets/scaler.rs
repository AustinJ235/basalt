use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use parking_lot::ReentrantMutex;

use crate::input::{MouseButton, Qwerty, WindowState};
use crate::interface::UnitValue::{
    PctOfHeight, PctOfHeightOffset, PctOfWidth, PctOfWidthOffset, Percent, Pixels,
};
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Container, Theme, WidgetPlacement};
use crate::interface::{Bin, BinStyle, Position};

/// Builder for [`Scaler`]
pub struct ScalerBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    plmt_is_default: bool,
    on_change: Vec<Box<dyn FnMut(&Arc<Scaler>, f32) + Send + 'static>>,
}

/// An error than can occur from [`ScalerBuilder::build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScalerError {
    /// Value provided by [`ScalerBuilder::max_value`] is greater than the value provided by
    /// [`ScalerBuilder::min_value`].
    MaxLessThanMin,
    /// Value provided by [`ScalerBuilder::set_value`] is not in range specified by
    /// [`ScalerBuilder::min_value`] and [`ScalerBuilder::max_value`].
    SetValNotInRange,
}

/// Determines how the value of [`Scaler`] is rounded when it is modified.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalerRound {
    /// The value is not rounded and left as is.
    ///
    /// **Note**: This is the default.
    #[default]
    None,
    /// The value is rounded to increments of the small step provided by
    /// [`ScalerBuilder::small_step`].
    Step,
    /// The value is rounded to the nearest whole number.
    Int,
}

/// The orientation of the [`Scaler`].
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalerOrientation {
    /// Display the [`Scaler`] horizonatally.
    ///
    /// This is the the default.
    #[default]
    Horizontal,
    /// Display the [`Scaler`] vertically.
    Vertical,
}

struct Properties {
    min: f32,
    max: f32,
    val: f32,
    small_step: f32,
    medium_step: f32,
    large_step: f32,
    round: ScalerRound,
    orientation: ScalerOrientation,
    placement: WidgetPlacement,
}

impl Properties {
    fn new(placement: WidgetPlacement) -> Self {
        Self {
            min: 0.0,
            max: 0.0,
            val: 0.0,
            small_step: 1.0,
            medium_step: 1.0,
            large_step: 1.0,
            round: Default::default(),
            orientation: Default::default(),
            placement,
        }
    }
}

/// Builder for [`Scaler`].
impl<'a, C> ScalerBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            plmt_is_default: builder.placement.is_none(),
            props: Properties::new(
                builder.placement.take().unwrap_or_else(|| {
                    Scaler::default_placement(&builder.theme, Default::default())
                }),
            ),
            widget: builder,
            on_change: Vec::new(),
        }
    }

    /// Specify the minimum value.
    ///
    /// **Note**: When this isn't used the minimum value will be `0.0`.
    pub fn min_value(mut self, min: f32) -> Self {
        self.props.min = min;
        self
    }

    /// Specify the maximum value.
    ///
    /// **Note**: When this isn't used the maxium value will be `0.0`.
    pub fn max_value(mut self, max: f32) -> Self {
        self.props.max = max;
        self
    }

    /// Set the initial value.
    ///
    /// **Note**: When this isn't used the initial value will be `0.0`.
    pub fn set_value(mut self, val: f32) -> Self {
        self.props.val = val;
        self
    }

    /// Set the value of a small step.
    ///
    /// **Notes**:
    /// - This is when no modifier keys are used.
    /// - When this isn't used the small step will be `1.0`.
    pub fn small_step(mut self, step: f32) -> Self {
        self.props.small_step = step;
        self
    }

    /// Set the value of a medium step.
    ///
    /// **Notes**:
    /// - This when either [`Qwerty::LCtrl`](crate::input::Qwerty::LCtrl) or
    /// [`Qwerty::RCtrl`](crate::input::Qwerty::RCtrl) is used.
    /// - Dragging the knob with the mouse will not be effected by this value.
    /// - When this isn't used the medium step will be `1.0`.
    pub fn medium_step(mut self, step: f32) -> Self {
        self.props.medium_step = step;
        self
    }

    /// Set the value of a large step.
    ///
    /// **Notes**:
    /// - This when either [`Qwerty::LShift`](crate::input::Qwerty::LShift) or
    /// [`Qwerty::RShift`](crate::input::Qwerty::RShift) is used.
    /// - Dragging the knob with the mouse will not be effected by this value.
    /// - When this isn't used the large step will be `1.0`.
    pub fn large_step(mut self, step: f32) -> Self {
        self.props.large_step = step;
        self
    }

    /// Set how the value is rounded after being modified.
    ///
    /// See documentation of [`ScalerRound`] for more information.
    pub fn round(mut self, round: ScalerRound) -> Self {
        self.props.round = round;
        self
    }

    /// Set the orientation of the [`Scaler`].
    ///
    /// **Note**: When this isn't used the [`ScalerOrientation`] will be
    /// [`Horizontal`](ScalerOrientation::Horizontal).
    pub fn orientation(mut self, orientation: ScalerOrientation) -> Self {
        if self.plmt_is_default {
            self.props.placement = Scaler::default_placement(&self.widget.theme, orientation);
        }

        self.props.orientation = orientation;
        self
    }

    /// Add a callback to be called when the [`Scaler`]'s value changed.
    ///
    /// **Note**: When changing the value within the callback, no callbacks will be called with
    ///  the updated value.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: FnMut(&Arc<Scaler>, f32) + Send + 'static,
    {
        self.on_change.push(Box::new(on_change));
        self
    }

    /// Finish building the [`Scaler`].
    pub fn build(self) -> Result<Arc<Scaler>, ScalerError> {
        if self.props.max < self.props.min {
            return Err(ScalerError::MaxLessThanMin);
        }

        if self.props.val < self.props.min || self.props.val > self.props.max {
            return Err(ScalerError::SetValNotInRange);
        }

        let container = self.widget.container.create_bin();
        let mut bins = container.create_bins(2);
        let track = bins.next().unwrap();
        let confine = bins.next().unwrap();
        let knob = confine.create_bin();
        drop(bins);

        let val = self.props.val;

        let scaler = Arc::new(Scaler {
            theme: self.widget.theme,
            props: self.props,
            container,
            track,
            confine,
            knob,
            state: ReentrantMutex::new(State {
                val: RefCell::new(val),
                on_change: RefCell::new(self.on_change),
            }),
        });

        let cb_scaler = scaler.clone();

        scaler.container.on_scroll(move |_, w_state, amt, _| {
            let step = cb_scaler.step_size(w_state) * -amt;
            cb_scaler.increment(step);
            Default::default()
        });

        let knob_held = Arc::new(AtomicBool::new(false));
        let mut window_hook_ids = Vec::new();

        let cb_knob_held = knob_held.clone();

        scaler.knob.on_press(MouseButton::Left, move |_, _, _| {
            cb_knob_held.store(true, atomic::Ordering::SeqCst);
            Default::default()
        });

        let cb_knob_held = knob_held.clone();

        scaler.knob.on_release(MouseButton::Left, move |_, _, _| {
            cb_knob_held.store(false, atomic::Ordering::SeqCst);
            Default::default()
        });

        let window = scaler.container.window().unwrap();
        let cb_scaler = scaler.clone();
        let cb_knob_held = knob_held.clone();

        window_hook_ids.push(window.on_cursor(move |_, w_state, _| {
            if cb_knob_held.load(atomic::Ordering::SeqCst) {
                let [cursor_x, cursor_y] = w_state.cursor_pos();
                let track_bpu = cb_scaler.track.post_update();
                let knob_bpu = cb_scaler.knob.post_update();

                match cb_scaler.props.orientation {
                    ScalerOrientation::Horizontal => {
                        let knob_width_1_2 = (knob_bpu.tri[0] - knob_bpu.tli[0]) / 2.0;
                        let cursor_x_min = track_bpu.tli[0] + knob_width_1_2;
                        let cursor_x_max = track_bpu.tri[0] - knob_width_1_2;
                        let pct =
                            ((cursor_x - cursor_x_min) / (cursor_x_max - cursor_x_min)) * 100.0;
                        cb_scaler.set_pct(pct.clamp(0.0, 100.0));
                    },
                    ScalerOrientation::Vertical => {
                        let knob_height_1_2 = (knob_bpu.bli[0] - knob_bpu.tli[0]) / 2.0;
                        let cursor_y_min = track_bpu.tli[1] + knob_height_1_2;
                        let cursor_y_max = track_bpu.bli[1] - knob_height_1_2;
                        let pct = 100.0
                            - (((cursor_y - cursor_y_min) / (cursor_y_max - cursor_y_min)) * 100.0);
                        cb_scaler.set_pct(pct.clamp(0.0, 100.0));
                    },
                }
            }

            Default::default()
        }));

        let focused = Arc::new(AtomicBool::new(false));

        let widget_bin_ids = [
            scaler.container.id(),
            scaler.track.id(),
            scaler.confine.id(),
            scaler.knob.id(),
        ];

        for bin in [
            &scaler.container,
            &scaler.track,
            &scaler.confine,
            &scaler.knob,
        ] {
            let cb_focused = focused.clone();

            bin.on_focus(move |_, _| {
                cb_focused.store(true, atomic::Ordering::SeqCst);
                Default::default()
            });

            let cb_focused = focused.clone();

            bin.on_focus_lost(move |_, w_state| {
                if let Some(focused_bin_id) = w_state.focused_bin_id() {
                    if !widget_bin_ids.contains(&focused_bin_id) {
                        cb_focused.store(false, atomic::Ordering::SeqCst);
                    }
                }

                Default::default()
            });
        }

        let cb_scaler = scaler.clone();
        let cb_focused = focused.clone();

        window_hook_ids.push(window.on_press(Qwerty::ArrowUp, move |_, w_state, _| {
            if cb_focused.load(atomic::Ordering::SeqCst) {
                let step = cb_scaler.step_size(w_state);
                cb_scaler.increment(step);
            }

            Default::default()
        }));

        let cb_scaler = scaler.clone();
        let cb_focused = focused.clone();

        window_hook_ids.push(window.on_press(Qwerty::ArrowRight, move |_, w_state, _| {
            if cb_focused.load(atomic::Ordering::SeqCst) {
                let step = cb_scaler.step_size(w_state);
                cb_scaler.increment(step);
            }

            Default::default()
        }));

        let cb_scaler = scaler.clone();
        let cb_focused = focused.clone();

        window_hook_ids.push(window.on_press(Qwerty::ArrowDown, move |_, w_state, _| {
            if cb_focused.load(atomic::Ordering::SeqCst) {
                let step = cb_scaler.step_size(w_state);
                cb_scaler.decrement(step);
            }

            Default::default()
        }));

        let cb_scaler = scaler.clone();
        let cb_focused = focused.clone();

        window_hook_ids.push(window.on_press(Qwerty::ArrowLeft, move |_, w_state, _| {
            if cb_focused.load(atomic::Ordering::SeqCst) {
                let step = cb_scaler.step_size(w_state);
                cb_scaler.decrement(step);
            }

            Default::default()
        }));

        for window_hook_id in window_hook_ids {
            scaler.container.attach_input_hook(window_hook_id);
        }

        scaler.style_update();
        Ok(scaler)
    }
}

/// Scaler widget
pub struct Scaler {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    track: Arc<Bin>,
    confine: Arc<Bin>,
    knob: Arc<Bin>,
    state: ReentrantMutex<State>,
}

struct State {
    val: RefCell<f32>,
    on_change: RefCell<Vec<Box<dyn FnMut(&Arc<Scaler>, f32) + Send + 'static>>>,
}

impl Scaler {
    fn step_size(&self, w_state: &WindowState) -> f32 {
        if w_state.is_key_pressed(Qwerty::LCtrl) || w_state.is_key_pressed(Qwerty::RCtrl) {
            self.props.medium_step
        } else if w_state.is_key_pressed(Qwerty::LShift) || w_state.is_key_pressed(Qwerty::RShift) {
            self.props.large_step
        } else {
            self.props.small_step
        }
    }

    fn set_pct(self: &Arc<Self>, pct: f32) {
        self.set(((self.props.max - self.props.min) * (pct / 100.0)) + self.props.min);
    }

    /// Set the value to the provided valued.
    ///
    /// **Notes**:
    /// - This will be effected by rounding provided by [`ScalerBuilder::round`].
    /// - This value will be clamped to values provided by [`ScalerBuilder::min_value`]
    /// and [`ScalerBuilder::max_value`].
    pub fn set(self: &Arc<Self>, mut val: f32) {
        val = match self.props.round {
            ScalerRound::None => val,
            ScalerRound::Int => val.round(),
            ScalerRound::Step => (val / self.props.small_step).round() * self.props.small_step,
        }
        .clamp(self.props.min, self.props.max);

        let pct = ((val - self.props.min) / (self.props.max - self.props.min)) * 100.0;
        let mut knob_style = self.knob.style_copy();

        match self.props.orientation {
            ScalerOrientation::Horizontal => {
                knob_style.pos_from_l = Percent(pct);
            },
            ScalerOrientation::Vertical => {
                knob_style.pos_from_b = Percent(pct);
            },
        }

        self.knob.style_update(knob_style).expect_valid();
        let state = self.state.lock();
        *state.val.borrow_mut() = val;

        if let Ok(mut on_change_cbs) = state.on_change.try_borrow_mut() {
            for on_change in on_change_cbs.iter_mut() {
                on_change(self, val);
            }
        }
    }

    /// Get the current value.
    pub fn val(&self) -> f32 {
        *self.state.lock().val.borrow()
    }

    /// Increment the value by the provided amount.
    ///
    /// **Notes**:
    /// - The resulting value will be effected by rounding provided by [`ScalerBuilder::round`].
    /// - The resulting value will be clamped to values provided by [`ScalerBuilder::min_value`]
    /// and [`ScalerBuilder::max_value`].
    pub fn increment(self: &Arc<Self>, amt: f32) {
        let state = self.state.lock();
        let val = *state.val.borrow() + amt;
        self.set(val);
    }

    /// Decrement the value by the provided amount.
    ///
    /// **Notes**:
    /// - The resulting value will be effected by rounding provided by [`ScalerBuilder::round`].
    /// - The resulting value will be clamped to values provided by [`ScalerBuilder::min_value`]
    /// and [`ScalerBuilder::max_value`].
    pub fn decrement(self: &Arc<Self>, amt: f32) {
        let state = self.state.lock();
        let val = *state.val.borrow() - amt;
        self.set(val);
    }

    /// Add a callback to be called when the [`Scaler`]'s value changed.
    ///
    /// **Note**: When changing the value within the callback, no callbacks will be called with
    ///  the updated value.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(&self, on_change: F)
    where
        F: FnMut(&Arc<Scaler>, f32) + Send + 'static,
    {
        self.state
            .lock()
            .on_change
            .borrow_mut()
            .push(Box::new(on_change));
    }

    /// Obtain the default [`WidgetPlacement`](`WidgetPlacement`) given a [`Theme`](`Theme`) and
    /// the [`ScalerOrientation`](`ScalerOrientation`).
    pub fn default_placement(theme: &Theme, orientation: ScalerOrientation) -> WidgetPlacement {
        match orientation {
            ScalerOrientation::Horizontal => {
                WidgetPlacement {
                    position: Position::Floating,
                    margin_t: Pixels(theme.spacing),
                    margin_b: Pixels(theme.spacing),
                    margin_l: Pixels(theme.spacing),
                    margin_r: Pixels(theme.spacing),
                    width: Pixels(theme.base_size * 4.0),
                    height: Pixels(theme.base_size),
                    ..Default::default()
                }
            },
            ScalerOrientation::Vertical => {
                WidgetPlacement {
                    position: Position::Floating,
                    margin_t: Pixels(theme.spacing),
                    margin_b: Pixels(theme.spacing),
                    margin_l: Pixels(theme.spacing),
                    margin_r: Pixels(theme.spacing),
                    width: Pixels(theme.base_size),
                    height: Pixels(theme.base_size * 4.0),
                    ..Default::default()
                }
            },
        }
    }

    fn style_update(self: &Arc<Self>) {
        let border_size = self.theme.border.unwrap_or(0.0);

        let pct = ((*self.state.lock().val.borrow() - self.props.min)
            / (self.props.max - self.props.min))
            * 100.0;

        let container_style = self.props.placement.clone().into_style();

        let mut track_style = BinStyle {
            back_color: self.theme.colors.back3,
            ..Default::default()
        };

        let mut confine_style = BinStyle::default();

        let mut knob_style = BinStyle {
            position: Position::Anchor,
            back_color: self.theme.colors.accent1,
            border_radius_tl: PctOfWidth(50.0),
            border_radius_tr: PctOfWidth(50.0),
            border_radius_bl: PctOfWidth(50.0),
            border_radius_br: PctOfWidth(50.0),
            ..Default::default()
        };

        match self.props.orientation {
            ScalerOrientation::Horizontal => {
                track_style.pos_from_t = Percent(25.0);
                track_style.pos_from_b = Percent(25.0);
                track_style.pos_from_l = Pixels(border_size);
                track_style.pos_from_r = Pixels(border_size);
                track_style.border_radius_tl = PctOfHeight(50.0);
                track_style.border_radius_tr = PctOfHeight(50.0);
                track_style.border_radius_bl = PctOfHeight(50.0);
                track_style.border_radius_br = PctOfHeight(50.0);

                confine_style.pos_from_t = Pixels(0.0);
                confine_style.pos_from_b = Pixels(0.0);
                confine_style.pos_from_l = Pixels(border_size);
                confine_style.pos_from_r = PctOfHeightOffset(100.0, -border_size);

                knob_style.pos_from_t = Pixels(border_size);
                knob_style.pos_from_b = Pixels(border_size);
                knob_style.pos_from_l = Percent(pct);
                knob_style.width = PctOfHeightOffset(100.0, -2.0 * border_size);
            },
            ScalerOrientation::Vertical => {
                track_style.pos_from_t = Pixels(border_size);
                track_style.pos_from_b = Pixels(border_size);
                track_style.pos_from_l = Percent(25.0);
                track_style.pos_from_r = Percent(25.0);
                track_style.border_radius_tl = PctOfWidth(50.0);
                track_style.border_radius_tr = PctOfWidth(50.0);
                track_style.border_radius_bl = PctOfWidth(50.0);
                track_style.border_radius_br = PctOfWidth(50.0);

                confine_style.pos_from_t = PctOfWidthOffset(100.0, -border_size);
                confine_style.pos_from_b = Pixels(border_size);
                confine_style.pos_from_l = Pixels(0.0);
                confine_style.pos_from_r = Pixels(0.0);

                knob_style.pos_from_l = Pixels(border_size);
                knob_style.pos_from_r = Pixels(border_size);
                knob_style.pos_from_b = Percent(pct);
                knob_style.height = PctOfWidthOffset(100.0, -2.0 * border_size);
            },
        }

        if let Some(border_size) = self.theme.border {
            track_style.border_size_t = Pixels(border_size);
            track_style.border_size_b = Pixels(border_size);
            track_style.border_size_l = Pixels(border_size);
            track_style.border_size_r = Pixels(border_size);
            track_style.border_color_t = self.theme.colors.border3;
            track_style.border_color_b = self.theme.colors.border3;
            track_style.border_color_l = self.theme.colors.border3;
            track_style.border_color_r = self.theme.colors.border3;
            knob_style.border_size_t = Pixels(border_size);
            knob_style.border_size_b = Pixels(border_size);
            knob_style.border_size_l = Pixels(border_size);
            knob_style.border_size_r = Pixels(border_size);
            knob_style.border_color_t = self.theme.colors.border3;
            knob_style.border_color_b = self.theme.colors.border3;
            knob_style.border_color_l = self.theme.colors.border3;
            knob_style.border_color_r = self.theme.colors.border3;
        }

        Bin::style_update_batch([
            (&self.container, container_style),
            (&self.track, track_style),
            (&self.confine, confine_style),
            (&self.knob, knob_style),
        ]);
    }
}
