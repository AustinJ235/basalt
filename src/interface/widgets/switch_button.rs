use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::ReentrantMutex;

use crate::input::MouseButton;
use crate::interface::UnitValue::{PctOfHeight, PctOffset, Percent, Pixels};
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Theme, Container, WidgetPlacement};
use crate::interface::{Bin, BinStyle, Position};

/// Builder for [`SwitchButton`]
pub struct SwitchButtonBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    on_change: Vec<Box<dyn FnMut(&Arc<SwitchButton>, bool) + Send + 'static>>,
}

#[derive(Default)]
struct Properties {
    enabled: bool,
    placement: WidgetPlacement,
}

impl Properties {
    fn new(placement: WidgetPlacement) -> Self {
        Self {
            enabled: false,
            placement,
        }
    }
}

impl<'a, C> SwitchButtonBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| SwitchButton::default_placement(&builder.theme)),
            ),
            widget: builder,
            on_change: Vec::new(),
        }
    }

    /// Set the initial enabled state.
    ///
    /// **Note**: When this isn't used the initial value will be `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.props.enabled = enabled;
        self
    }

    /// Add a callback to be called when the [`SwitchButton`]'s value changed.
    ///
    /// **Note**: When changing the value within the callback, no callbacks will be called with
    ///  the updated value.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: FnMut(&Arc<SwitchButton>, bool) + Send + 'static,
    {
        self.on_change.push(Box::new(on_change));
        self
    }

    /// Finish building the [`SwitchButton`].
    pub fn build(self) -> Arc<SwitchButton> {
        let container = self.widget.container.create_bin();
        let knob = container.create_bin();
        let enabled = self.props.enabled;

        let switch_button = Arc::new(SwitchButton {
            theme: self.widget.theme,
            props: self.props,
            container,
            knob,
            state: ReentrantMutex::new(State {
                enabled: RefCell::new(enabled),
                on_change: RefCell::new(self.on_change),
            }),
        });

        let cb_switch_button = switch_button.clone();

        switch_button
            .container
            .on_press(MouseButton::Left, move |_, _, _| {
                cb_switch_button.toggle();
                Default::default()
            });

        let cb_switch_button = switch_button.clone();

        switch_button
            .knob
            .on_press(MouseButton::Left, move |_, _, _| {
                cb_switch_button.toggle();
                Default::default()
            });

        switch_button.style_update();
        switch_button
    }
}

/// Switch button widget
pub struct SwitchButton {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    knob: Arc<Bin>,
    state: ReentrantMutex<State>,
}

struct State {
    enabled: RefCell<bool>,
    on_change: RefCell<Vec<Box<dyn FnMut(&Arc<SwitchButton>, bool) + Send + 'static>>>,
}

impl SwitchButton {
    /// Set the enabled state.
    pub fn set(self: &Arc<Self>, enabled: bool) {
        let state = self.state.lock();
        *state.enabled.borrow_mut() = enabled;

        if enabled {
            Bin::style_update_batch([
                (
                    &self.container,
                    BinStyle {
                        back_color: self.theme.colors.accent1,
                        ..self.container.style_copy()
                    },
                ),
                (
                    &self.knob,
                    BinStyle {
                        pos_from_r: PctOffset(10.0, -self.theme.border.unwrap_or(0.0)),
                        pos_from_l: Default::default(),
                        ..self.knob.style_copy()
                    },
                ),
            ]);
        } else {
            Bin::style_update_batch([
                (
                    &self.container,
                    BinStyle {
                        back_color: self.theme.colors.back3,
                        ..self.container.style_copy()
                    },
                ),
                (
                    &self.knob,
                    BinStyle {
                        pos_from_l: PctOffset(10.0, -self.theme.border.unwrap_or(0.0)),
                        pos_from_r: Default::default(),
                        ..self.knob.style_copy()
                    },
                ),
            ]);
        }

        if let Ok(mut on_change_cbs) = state.on_change.try_borrow_mut() {
            for on_change in on_change_cbs.iter_mut() {
                on_change(self, enabled);
            }
        }
    }

    /// Toggle the enabled state returning the new enabled state.
    pub fn toggle(self: &Arc<Self>) -> bool {
        let state = self.state.lock();
        let enabled = !*state.enabled.borrow();
        self.set(enabled);
        enabled
    }

    /// Get the current enabled state.
    pub fn get(&self) -> bool {
        *self.state.lock().enabled.borrow()
    }

    /// Add a callback to be called when the [`SwitchButton`]'s value changed.
    ///
    /// **Note**: When changing the value within the callback, no callbacks will be called with
    ///  the updated value.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(&self, on_change: F)
    where
        F: FnMut(&Arc<SwitchButton>, bool) + Send + 'static,
    {
        self.state
            .lock()
            .on_change
            .borrow_mut()
            .push(Box::new(on_change));
    }

    /// Obtain the default [`WidgetPlacement`](`WidgetPlacement`) given a [`Theme`](`Theme`).
    pub fn default_placement(theme: &Theme) -> WidgetPlacement {
        let height = theme.base_size;
        let width = height * 2.0;

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

    fn style_update(&self) {
        let enabled = *self.state.lock().enabled.borrow();

        let mut container_style = BinStyle {
            border_radius_tl: PctOfHeight(50.0),
            border_radius_tr: PctOfHeight(50.0),
            border_radius_bl: PctOfHeight(50.0),
            border_radius_br: PctOfHeight(50.0),
            ..self.props.placement.clone().into_style()
        };

        let mut knob_style = BinStyle {
            pos_from_t: Percent(10.0),
            pos_from_b: Percent(10.0),
            width: PctOfHeight(80.0),
            back_color: self.theme.colors.back1,
            border_radius_tl: PctOfHeight(50.0),
            border_radius_tr: PctOfHeight(50.0),
            border_radius_bl: PctOfHeight(50.0),
            border_radius_br: PctOfHeight(50.0),
            ..Default::default()
        };

        if enabled {
            container_style.back_color = self.theme.colors.accent1;
            knob_style.pos_from_r = PctOffset(10.0, -self.theme.border.unwrap_or(0.0));
        } else {
            container_style.back_color = self.theme.colors.back3;
            knob_style.pos_from_l = PctOffset(10.0, -self.theme.border.unwrap_or(0.0));
        }

        if let Some(border_size) = self.theme.border {
            container_style.border_size_t = Pixels(border_size);
            container_style.border_size_b = Pixels(border_size);
            container_style.border_size_l = Pixels(border_size);
            container_style.border_size_r = Pixels(border_size);
            container_style.border_color_t = self.theme.colors.border1;
            container_style.border_color_b = self.theme.colors.border1;
            container_style.border_color_l = self.theme.colors.border1;
            container_style.border_color_r = self.theme.colors.border1;

            knob_style.border_size_t = Pixels(border_size);
            knob_style.border_size_b = Pixels(border_size);
            knob_style.border_size_l = Pixels(border_size);
            knob_style.border_size_r = Pixels(border_size);
            knob_style.border_color_t = self.theme.colors.border3;
            knob_style.border_color_b = self.theme.colors.border3;
            knob_style.border_color_l = self.theme.colors.border3;
            knob_style.border_color_r = self.theme.colors.border3;
        }

        Bin::style_update_batch([(&self.container, container_style), (&self.knob, knob_style)]);
    }
}
