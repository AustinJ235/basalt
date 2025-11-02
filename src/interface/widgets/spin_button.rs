use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::ReentrantMutex;

use crate::image::ImageKey;
use crate::input::{InputHookCtrl, Qwerty, WindowState};
use crate::interface::UnitValue::{PctOfHeight, PctOfHeightOffset, Percent, Pixels};
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::button::{BtnHookColors, button_hooks};
use crate::interface::widgets::{Container, Theme, WidgetPlacement, text_hooks};
use crate::interface::{
    Bin, BinPostUpdate, BinStyle, BinVertex, Color, Position, TextAttrs, TextBody, TextHoriAlign,
    TextVertAlign, TextWrap, ZIndex,
};

/// Builder for [`SpinButton`]
pub struct SpinButtonBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    on_change: Vec<Box<dyn FnMut(&Arc<SpinButton>, i32) + Send + 'static>>,
}

/// An error than can occur from [`SpinButtonBuilder::build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpinButtonError {
    /// Value provided by [`SpinButtonBuilder::max_value`] is greater than the value provided by
    /// [`SpinButtonBuilder::min_value`].
    MaxLessThanMin,
    /// Value provided by [`SpinButtonBuilder::set_value`] is not in range specified by
    /// [`SpinButtonBuilder::min_value`] and [`SpinButtonBuilder::max_value`].
    SetValNotInRange,
}

struct Properties {
    min: i32,
    max: i32,
    val: i32,
    small_step: i32,
    medium_step: i32,
    large_step: i32,
    placement: WidgetPlacement,
}

impl Properties {
    fn new(placement: WidgetPlacement) -> Self {
        Self {
            min: 0,
            max: 0,
            val: 0,
            small_step: 1,
            medium_step: 1,
            large_step: 1,
            placement,
        }
    }
}

impl<'a, C> SpinButtonBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| SpinButton::default_placement(&builder.theme)),
            ),
            widget: builder,
            on_change: Vec::new(),
        }
    }

    /// Specify the minimum value.
    ///
    /// **Note**: When this isn't used the minimum value will be `0`.
    pub fn min_value(mut self, min: i32) -> Self {
        self.props.min = min;
        self
    }

    /// Specify the maximum value.
    ///
    /// **Note**: When this isn't used the maxium value will be `0`.
    pub fn max_value(mut self, max: i32) -> Self {
        self.props.max = max;
        self
    }

    /// Set the initial value.
    ///
    /// **Note**: When this isn't used the initial value will be `0`.
    pub fn set_value(mut self, val: i32) -> Self {
        self.props.val = val;
        self
    }

    /// Set the value of a small step.
    ///
    /// **Notes**:
    /// - This is when no modifier keys are used.
    /// - When this isn't used the small step will be `1`.
    pub fn small_step(mut self, step: i32) -> Self {
        self.props.small_step = step;
        self
    }

    /// Set the value of a medium step.
    ///
    /// **Notes**:
    /// - This when either [`Qwerty::LCtrl`](crate::input::Qwerty::LCtrl) or
    /// [`Qwerty::RCtrl`](crate::input::Qwerty::RCtrl) is used.
    /// - Dragging the knob with the mouse will not be effected by this value.
    /// - When this isn't used the medium step will be `1`.
    pub fn medium_step(mut self, step: i32) -> Self {
        self.props.medium_step = step;
        self
    }

    /// Set the value of a large step.
    ///
    /// **Notes**:
    /// - This when either [`Qwerty::LShift`](crate::input::Qwerty::LShift) or
    /// [`Qwerty::RShift`](crate::input::Qwerty::RShift) is used.
    /// - Dragging the knob with the mouse will not be effected by this value.
    /// - When this isn't used the large step will be `1`.
    pub fn large_step(mut self, step: i32) -> Self {
        self.props.large_step = step;
        self
    }

    /// Add a callback to be called when the [`SpinButton`]'s value changed.
    ///
    /// **Note**: When changing the value within the callback, no callbacks will be called with
    ///  the updated value.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: FnMut(&Arc<SpinButton>, i32) + Send + 'static,
    {
        self.on_change.push(Box::new(on_change));
        self
    }

    /// Finish building the [`SpinButton`].
    pub fn build(self) -> Result<Arc<SpinButton>, SpinButtonError> {
        if self.props.max < self.props.min {
            return Err(SpinButtonError::MaxLessThanMin);
        }

        if self.props.val < self.props.min || self.props.val > self.props.max {
            return Err(SpinButtonError::SetValNotInRange);
        }

        let container = self.widget.container.create_bin();
        let mut bins = container.create_bins(3);
        let entry = bins.next().unwrap();
        let sub_button = bins.next().unwrap();
        let add_button = bins.next().unwrap();
        drop(bins);
        let initial_val = self.props.val;

        let spin_button = Arc::new(SpinButton {
            theme: self.widget.theme,
            props: self.props,
            container,
            entry,
            sub_button,
            add_button,
            state: ReentrantMutex::new(State {
                val: RefCell::new(initial_val),
                on_change: RefCell::new(self.on_change),
            }),
        });

        let cb_spin_button = spin_button.clone();

        button_hooks(
            &spin_button.sub_button,
            BtnHookColors {
                back_clr: Some(spin_button.theme.colors.back3),
                vert_clr: Some(spin_button.theme.colors.border2),
                h_back_clr: Some(spin_button.theme.colors.accent1),
                h_vert_clr: Some(spin_button.theme.colors.back2),
                p_back_clr: Some(spin_button.theme.colors.accent2),
                p_vert_clr: Some(spin_button.theme.colors.back2),
                ..Default::default()
            },
            move |w_state| {
                let step = cb_spin_button.step_size(w_state);
                cb_spin_button.decrement(step);
            },
        );

        let cb_spin_button = spin_button.clone();

        button_hooks(
            &spin_button.add_button,
            BtnHookColors {
                back_clr: Some(spin_button.theme.colors.back3),
                vert_clr: Some(spin_button.theme.colors.border2),
                h_back_clr: Some(spin_button.theme.colors.accent1),
                h_vert_clr: Some(spin_button.theme.colors.back2),
                p_back_clr: Some(spin_button.theme.colors.accent2),
                p_vert_clr: Some(spin_button.theme.colors.back2),
                ..Default::default()
            },
            move |w_state| {
                let step = cb_spin_button.step_size(w_state);
                cb_spin_button.increment(step);
            },
        );

        let spin_button_wk = Arc::downgrade(&spin_button);

        text_hooks::create(
            text_hooks::Properties::ENTRY,
            spin_button.entry.clone(),
            spin_button.theme.clone(),
            Some(Arc::new(move |updated| {
                let text_hooks::Updated {
                    cursor: _,
                    cursor_bounds,
                    body_line_count: _,
                    cursor_line_col: _,
                    editor_bpu,
                } = updated;

                if let Some(cursor_bounds) = cursor_bounds
                    && let Some(spin_button) = spin_button_wk.upgrade()
                {
                    spin_button.check_cursor_in_view(editor_bpu, cursor_bounds);
                }
            })),
            None,
        );

        let spin_button_wk = Arc::downgrade(&spin_button);

        spin_button
            .entry
            .basalt_ref()
            .input_ref()
            .hook()
            .bin(&spin_button.entry)
            .on_character()
            .weight(1)
            .call(move |_, _, c| {
                let spin_button = match spin_button_wk.upgrade() {
                    Some(some) => some,
                    None => return InputHookCtrl::Remove,
                };

                let val = match c.0 {
                    '\r' | '\n' => {
                        spin_button
                            .entry
                            .style_inspect(|style| style.text_body.spans[0].text.parse::<i32>())
                            .unwrap_or(*spin_button.state.lock().val.borrow())
                    },
                    '\u{1b}' => *spin_button.state.lock().val.borrow(),
                    _ => return Default::default(),
                };

                let window_id = match spin_button.entry.window() {
                    Some(window) => window.id(),
                    None => return Default::default(),
                };

                spin_button
                    .entry
                    .basalt_ref()
                    .input_ref()
                    .clear_bin_focus(window_id);

                spin_button.set(val);
                InputHookCtrl::RetainNoPass
            })
            .finish()
            .unwrap();

        let cb_spin_button = spin_button.clone();

        spin_button.entry.on_focus(move |_, _| {
            let border_size = cb_spin_button.theme.border.unwrap_or(1.0);

            cb_spin_button.entry.style_modify(|style| {
                style.border_size_t = Pixels(border_size);
                style.border_size_b = Pixels(border_size);
                style.border_size_l = Pixels(border_size);
                style.border_size_r = Pixels(border_size);
            });

            Default::default()
        });

        let cb_spin_button = spin_button.clone();

        spin_button.entry.on_focus_lost(move |_, _| {
            cb_spin_button.entry.style_modify(|style| {
                style.border_size_t = Default::default();
                style.border_size_b = Default::default();
                style.border_size_l = Default::default();
                style.border_size_r = Default::default();
                style.scroll_x = 0.0;
                style.text_body.spans =
                    vec![format!("{}", *cb_spin_button.state.lock().val.borrow()).into()];
            });

            Default::default()
        });

        spin_button.style_update();
        Ok(spin_button)
    }
}

/// Spin button widget
pub struct SpinButton {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    entry: Arc<Bin>,
    sub_button: Arc<Bin>,
    add_button: Arc<Bin>,
    state: ReentrantMutex<State>,
}

struct State {
    val: RefCell<i32>,
    on_change: RefCell<Vec<Box<dyn FnMut(&Arc<SpinButton>, i32) + Send + 'static>>>,
}

impl SpinButton {
    fn step_size(&self, w_state: &WindowState) -> i32 {
        if w_state.is_key_pressed(Qwerty::LCtrl) || w_state.is_key_pressed(Qwerty::RCtrl) {
            self.props.medium_step
        } else if w_state.is_key_pressed(Qwerty::LShift) || w_state.is_key_pressed(Qwerty::RShift) {
            self.props.large_step
        } else {
            self.props.small_step
        }
    }

    /// Set the value to the provided valued.
    ///
    /// **Note**: This value will be clamped to values provided by [`SpinButtonBuilder::min_value`]
    /// and [`SpinButtonBuilder::max_value`].
    pub fn set(self: &Arc<Self>, val: i32) {
        let state = self.state.lock();
        let val = val.clamp(self.props.min, self.props.max);
        *state.val.borrow_mut() = val;

        self.entry.style_modify(|style| {
            style.text_body.spans = vec![format!("{}", val).into()];
        });

        if let Ok(mut on_change_cbs) = state.on_change.try_borrow_mut() {
            for on_change in on_change_cbs.iter_mut() {
                on_change(self, val);
            }
        }
    }

    /// Get the current value.
    pub fn val(&self) -> i32 {
        *self.state.lock().val.borrow()
    }

    /// Increment the value by the provided amount.
    ///
    /// **Note**: The resulting value will be clamped to values provided by [`SpinButtonBuilder::min_value`]
    /// and [`SpinButtonBuilder::max_value`].
    pub fn increment(self: &Arc<Self>, amt: i32) {
        let state = self.state.lock();

        let val = state
            .val
            .borrow()
            .checked_add(amt)
            .unwrap_or(self.props.max);

        self.set(val);
    }

    /// Decrement the value by the provided amount.
    ///
    /// **Note**: The resulting value will be clamped to values provided by [`SpinButtonBuilder::min_value`]
    /// and [`SpinButtonBuilder::max_value`].
    pub fn decrement(self: &Arc<Self>, amt: i32) {
        let state = self.state.lock();

        let val = state
            .val
            .borrow()
            .checked_sub(amt)
            .unwrap_or(self.props.min);

        self.set(val);
    }

    /// Add a callback to be called when the [`SpinButton`]'s value changed.
    ///
    /// **Note**: When changing the value within the callback, no callbacks will be called with
    ///  the updated value.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(&self, on_change: F)
    where
        F: FnMut(&Arc<SpinButton>, i32) + Send + 'static,
    {
        self.state
            .lock()
            .on_change
            .borrow_mut()
            .push(Box::new(on_change));
    }

    /// Obtain the default [`WidgetPlacement`](`WidgetPlacement`) given a [`Theme`](`Theme`).
    pub fn default_placement(theme: &Theme) -> WidgetPlacement {
        let height = theme.spacing + theme.base_size;
        let width = height * 3.5;

        WidgetPlacement {
            position: Position::Floating,
            margin_t: Pixels(theme.spacing),
            margin_b: Pixels(theme.spacing),
            margin_l: Pixels(theme.spacing),
            margin_r: Pixels(theme.spacing),
            width: Pixels(width),
            height: Pixels(height),
            ..Default::default()
        }
    }

    fn check_cursor_in_view(&self, entry_bpu: &BinPostUpdate, cursor_bounds: [f32; 4]) {
        let view_bounds = entry_bpu.optimal_content_bounds;

        let scroll_x_op = if cursor_bounds[0] < view_bounds[0] {
            Some(cursor_bounds[0] - entry_bpu.content_offset[0] - view_bounds[0])
        } else if cursor_bounds[1] > view_bounds[1] {
            Some(cursor_bounds[1] - entry_bpu.content_offset[0] - view_bounds[1])
        } else {
            None
        };

        if let Some(scroll_x) = scroll_x_op {
            self.entry.style_modify(|style| {
                style.scroll_x = scroll_x;
            });
        }
    }

    fn style_update(self: &Arc<Self>) {
        let border_size = self.theme.border.unwrap_or(0.0);
        let mut container_style = self.props.placement.clone().into_style();

        let mut entry_style = BinStyle {
            position: Position::Anchor,
            z_index: ZIndex::Offset(1),
            pos_from_t: Pixels(0.0),
            pos_from_l: Pixels(0.0),
            pos_from_b: Pixels(0.0),
            pos_from_r: PctOfHeightOffset(200.0, border_size * 2.0),
            back_color: self.theme.colors.back2,
            border_color_t: self.theme.colors.accent1,
            border_color_b: self.theme.colors.accent1,
            border_color_l: self.theme.colors.accent1,
            border_color_r: self.theme.colors.accent1,
            padding_l: Pixels(self.theme.spacing),
            text_body: TextBody {
                spans: vec![format!("{}", self.props.val).into()],
                hori_align: TextHoriAlign::Left,
                vert_align: TextVertAlign::Center,
                text_wrap: TextWrap::None,
                base_attrs: TextAttrs {
                    height: Pixels(self.theme.text_height),
                    color: self.theme.colors.text1a,
                    font_family: self.theme.font_family.clone(),
                    font_weight: self.theme.font_weight,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let mut sub_button_style = BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_r: PctOfHeightOffset(100.0, border_size),
            pos_from_b: Pixels(0.0),
            width: PctOfHeight(100.0),
            back_color: self.theme.colors.back3,
            user_vertexes: vec![(
                ImageKey::INVALID,
                sub_symbol_verts(
                    self.theme.text_height,
                    self.theme.spacing,
                    self.theme.colors.border2,
                ),
            )],
            ..Default::default()
        };

        let mut add_button_style = BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_r: Pixels(0.0),
            pos_from_b: Pixels(0.0),
            width: PctOfHeight(100.0),
            back_color: self.theme.colors.back3,
            user_vertexes: vec![(
                ImageKey::INVALID,
                add_symbol_verts(
                    self.theme.text_height,
                    self.theme.spacing,
                    self.theme.colors.border2,
                ),
            )],
            ..Default::default()
        };

        if let Some(border_size) = self.theme.border {
            container_style.border_size_t = Pixels(border_size);
            container_style.border_size_b = Pixels(border_size);
            container_style.border_size_l = Pixels(border_size);
            container_style.border_size_r = Pixels(border_size);
            container_style.border_color_t = self.theme.colors.border1;
            container_style.border_color_b = self.theme.colors.border1;
            container_style.border_color_l = self.theme.colors.border1;
            container_style.border_color_r = self.theme.colors.border1;

            sub_button_style.border_size_l = Pixels(border_size);
            sub_button_style.border_color_l = self.theme.colors.border2;

            add_button_style.border_size_l = Pixels(border_size);
            add_button_style.border_color_l = self.theme.colors.border2;
        }

        if let Some(border_radius) = self.theme.roundness {
            container_style.border_radius_tl = Pixels(border_radius);
            container_style.border_radius_tr = Pixels(border_radius);
            container_style.border_radius_bl = Pixels(border_radius);
            container_style.border_radius_br = Pixels(border_radius);

            entry_style.border_radius_tl = Pixels(border_radius);
            entry_style.border_radius_bl = Pixels(border_radius);

            add_button_style.border_radius_tr = Pixels(border_radius);
            add_button_style.border_radius_br = Pixels(border_radius);
        }

        Bin::style_update_batch([
            (&self.container, container_style),
            (&self.entry, entry_style),
            (&self.sub_button, sub_button_style),
            (&self.add_button, add_button_style),
        ]);
    }
}

fn sub_symbol_verts(_target_size: f32, _spacing: f32, color: Color) -> Vec<BinVertex> {
    const PCT_PTS: [[f32; 2]; 4] = [[25.0, 47.0], [75.0, 47.0], [25.0, 53.0], [75.0, 53.0]];

    [1, 0, 2, 1, 2, 3]
        .into_iter()
        .map(|i| {
            BinVertex {
                x: Percent(PCT_PTS[i][0]),
                y: Percent(PCT_PTS[i][1]),
                color,
                ..Default::default()
            }
        })
        .collect()
}

fn add_symbol_verts(_target_size: f32, _spacing: f32, color: Color) -> Vec<BinVertex> {
    const PCT_PTS: [[f32; 2]; 8] = [
        [25.0, 47.0],
        [75.0, 47.0],
        [25.0, 53.0],
        [75.0, 53.0],
        [47.0, 25.0],
        [53.0, 25.0],
        [47.0, 75.0],
        [53.0, 75.0],
    ];

    [1, 0, 2, 1, 2, 3, 5, 4, 6, 5, 6, 7]
        .into_iter()
        .map(|i| {
            BinVertex {
                x: Percent(PCT_PTS[i][0]),
                y: Percent(PCT_PTS[i][1]),
                color,
                ..Default::default()
            }
        })
        .collect()
}
