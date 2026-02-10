use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use parking_lot::ReentrantMutex;

use crate::input::MouseButton;
use crate::interface::UnitValue::Pixels;
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Container, Theme, WidgetPlacement};
use crate::interface::{
    Bin, BinStyle, Position, TextAttrs, TextBody, TextHoriAlign, TextVertAlign, TextWrap,
};

/// Builder for [`ToggleButton`]
pub struct ToggleButtonBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    on_change: Vec<Box<dyn FnMut(&Arc<ToggleButton>, bool) + Send + 'static>>,
}

struct Properties {
    disabled_text: String,
    enabled_text: String,
    enabled: bool,
    placement: WidgetPlacement,
}

impl Properties {
    fn new(placement: WidgetPlacement) -> Self {
        Self {
            disabled_text: String::new(),
            enabled_text: String::new(),
            enabled: false,
            placement,
        }
    }
}

impl<'a, C> ToggleButtonBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| ToggleButton::default_placement(&builder.theme)),
            ),
            widget: builder,
            on_change: Vec::new(),
        }
    }

    /// Set the text to be displayed when disabled.
    ///
    /// **Note**: When this isn't used the disabled text will be empty.
    pub fn disabled_text<T>(mut self, text: T) -> Self
    where
        T: Into<String>,
    {
        self.props.disabled_text = text.into();
        self
    }

    /// Set the text to be displayed when enabled.
    ///
    /// **Note**: When this isn't used the enabled text will be empty.
    pub fn enabled_text<T>(mut self, text: T) -> Self
    where
        T: Into<String>,
    {
        self.props.enabled_text = text.into();
        self
    }

    /// Set the initial enabled state.
    ///
    /// **Note**: When this isn't used the initial value will be `false`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.props.enabled = enabled;
        self
    }

    /// Add a callback to be called when the [`ToggleButton`]'s value changed.
    ///
    /// **Note**: When changing the value within the callback, no callbacks will be called with
    ///  the updated value.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: FnMut(&Arc<ToggleButton>, bool) + Send + 'static,
    {
        self.on_change.push(Box::new(on_change));
        self
    }

    /// Finish building the [`ToggleButton`].
    pub fn build(self) -> Arc<ToggleButton> {
        let container = self.widget.container.create_bin();
        let enabled = self.props.enabled;

        let toggle_button = Arc::new(ToggleButton {
            theme: self.widget.theme,
            props: self.props,
            container,
            state: ReentrantMutex::new(State {
                enabled: RefCell::new(enabled),
                on_change: RefCell::new(self.on_change),
            }),
        });

        let cursor_inside = Arc::new(AtomicBool::new(false));
        let button_pressed = Arc::new(AtomicBool::new(false));

        let cb_toggle_button = toggle_button.clone();
        let cb_cursor_inside = cursor_inside.clone();
        let cb_button_pressed = button_pressed.clone();

        toggle_button.container.on_enter(move |_, _| {
            cb_cursor_inside.store(true, atomic::Ordering::SeqCst);

            if !cb_button_pressed.load(atomic::Ordering::SeqCst) && !cb_toggle_button.get() {
                let mut style = cb_toggle_button.container.style_copy();
                style.back_color = cb_toggle_button.theme.colors.accent1;
                style.text_body.base_attrs.color = cb_toggle_button.theme.colors.text1b;

                cb_toggle_button
                    .container
                    .style_update(style)
                    .expect_valid();
            }

            Default::default()
        });

        let cb_toggle_button = toggle_button.clone();
        let cb_cursor_inside = cursor_inside.clone();
        let cb_button_pressed = button_pressed.clone();

        toggle_button.container.on_leave(move |_, _| {
            cb_cursor_inside.store(false, atomic::Ordering::SeqCst);

            if !cb_button_pressed.load(atomic::Ordering::SeqCst) && !cb_toggle_button.get() {
                let mut style = cb_toggle_button.container.style_copy();
                style.back_color = cb_toggle_button.theme.colors.back3;
                style.text_body.base_attrs.color = cb_toggle_button.theme.colors.text1a;

                cb_toggle_button
                    .container
                    .style_update(style)
                    .expect_valid();
            }

            Default::default()
        });

        let cb_toggle_button = toggle_button.clone();
        let cb_button_pressed = button_pressed.clone();

        toggle_button
            .container
            .on_press(MouseButton::Left, move |_, _, _| {
                cb_button_pressed.store(true, atomic::Ordering::SeqCst);
                cb_toggle_button.toggle();
                Default::default()
            });

        let cb_toggle_button = toggle_button.clone();
        let cb_cursor_inside = cursor_inside;
        let cb_button_pressed = button_pressed;

        toggle_button
            .container
            .on_release(MouseButton::Left, move |_, _, _| {
                cb_button_pressed.store(false, atomic::Ordering::SeqCst);

                if !cb_toggle_button.get() {
                    let mut style = cb_toggle_button.container.style_copy();

                    if cb_cursor_inside.load(atomic::Ordering::SeqCst) {
                        style.back_color = cb_toggle_button.theme.colors.accent1;
                        style.text_body.base_attrs.color = cb_toggle_button.theme.colors.text1b;
                    } else {
                        style.back_color = cb_toggle_button.theme.colors.back3;
                        style.text_body.base_attrs.color = cb_toggle_button.theme.colors.text1a;
                    }

                    cb_toggle_button
                        .container
                        .style_update(style)
                        .expect_valid();
                }

                Default::default()
            });

        toggle_button.style_update();
        toggle_button
    }
}

/// Toggle button widget
pub struct ToggleButton {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    state: ReentrantMutex<State>,
}

struct State {
    enabled: RefCell<bool>,
    on_change: RefCell<Vec<Box<dyn FnMut(&Arc<ToggleButton>, bool) + Send + 'static>>>,
}

impl ToggleButton {
    /// Set the enabled state.
    pub fn set(self: &Arc<Self>, enabled: bool) {
        let state = self.state.lock();
        *state.enabled.borrow_mut() = enabled;

        let mut style = self.container.style_copy();
        style.back_color = self.theme.colors.accent2;
        style.text_body.base_attrs.color = self.theme.colors.text1b;

        style.text_body.spans[0].text = if enabled {
            self.props.enabled_text.clone()
        } else {
            self.props.disabled_text.clone()
        };

        self.container.style_update(style).expect_valid();

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

    /// Add a callback to be called when the [`ToggleButton`]'s value changed.
    ///
    /// **Note**: When changing the value within the callback, no callbacks will be called with
    ///  the updated value.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(&self, on_change: F)
    where
        F: FnMut(&Arc<ToggleButton>, bool) + Send + 'static,
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
        let mut container_style = BinStyle {
            text_body: TextBody {
                spans: vec![Default::default()],
                hori_align: TextHoriAlign::Center,
                vert_align: TextVertAlign::Center,
                text_wrap: TextWrap::None,
                base_attrs: TextAttrs {
                    height: Pixels(self.theme.text_height),
                    font_family: self.theme.font_family.clone(),
                    font_weight: self.theme.font_weight,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..self.props.placement.clone().into_style()
        };

        if *self.state.lock().enabled.borrow() {
            container_style.back_color = self.theme.colors.accent2;
            container_style.text_body.base_attrs.color = self.theme.colors.text1b;
            container_style.text_body.spans[0].text = self.props.enabled_text.clone();
        } else {
            container_style.back_color = self.theme.colors.back3;
            container_style.text_body.base_attrs.color = self.theme.colors.text1a;
            container_style.text_body.spans[0].text = self.props.disabled_text.clone();
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
        }

        if let Some(border_radius) = self.theme.roundness {
            container_style.border_radius_tl = Pixels(border_radius);
            container_style.border_radius_tr = Pixels(border_radius);
            container_style.border_radius_bl = Pixels(border_radius);
            container_style.border_radius_br = Pixels(border_radius);
        }

        self.container.style_update(container_style).expect_valid();
    }
}
