use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use parking_lot::ReentrantMutex;

use crate::input::{MouseButton, WindowState};
use crate::interface::UnitValue::Pixels;
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Theme, WidgetContainer, WidgetPlacement};
use crate::interface::{
    Bin, BinStyle, Color, Position, TextAttrs, TextBody, TextHoriAlign, TextVertAlign, TextWrap,
};

/// Builder for [`Button`]
pub struct ButtonBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    on_press: Vec<Box<dyn FnMut(&Arc<Button>) + Send + 'static>>,
}

#[derive(Default)]
struct Properties {
    text: String,
    placement: WidgetPlacement,
}

impl Properties {
    fn new(placement: WidgetPlacement) -> Self {
        Self {
            text: String::new(),
            placement,
        }
    }
}

impl<'a, C> ButtonBuilder<'a, C>
where
    C: WidgetContainer,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| Button::default_placement(&builder.theme)),
            ),
            widget: builder,
            on_press: Vec::new(),
        }
    }

    /// Set the text.
    pub fn text<T>(mut self, text: T) -> Self
    where
        T: Into<String>,
    {
        self.props.text = text.into();
        self
    }

    /// Add a callback to be called when the [`Button`] is pressed.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_press<F>(mut self, on_press: F) -> Self
    where
        F: FnMut(&Arc<Button>) + Send + 'static,
    {
        self.on_press.push(Box::new(on_press));
        self
    }

    /// Finish building the [`Button`].
    pub fn build(self) -> Arc<Button> {
        let window = self
            .widget
            .container
            .container_bin()
            .window()
            .expect("The widget container must have an associated window.");

        let container = window.new_bin();

        self.widget
            .container
            .container_bin()
            .add_child(container.clone());

        let button = Arc::new(Button {
            theme: self.widget.theme,
            props: self.props,
            container,
            state: ReentrantMutex::new(State {
                on_press: RefCell::new(self.on_press),
            }),
        });

        let cb_button = button.clone();

        button_hooks(
            &button.container,
            BtnHookColors {
                text_clr: Some(button.theme.colors.text1a),
                back_clr: Some(button.theme.colors.back3),
                h_text_clr: Some(button.theme.colors.text1b),
                h_back_clr: Some(button.theme.colors.accent1),
                p_text_clr: Some(button.theme.colors.text1b),
                p_back_clr: Some(button.theme.colors.accent2),
                ..Default::default()
            },
            move |_| {
                let state = cb_button.state.lock();

                for on_press in state.on_press.borrow_mut().iter_mut() {
                    on_press(&cb_button);
                }
            },
        );

        button.style_update();
        button
    }
}

/// Button widget.
pub struct Button {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    state: ReentrantMutex<State>,
}

struct State {
    on_press: RefCell<Vec<Box<dyn FnMut(&Arc<Button>) + Send + 'static>>>,
}

impl Button {
    /// Add a callback to be called when the [`Button`] is pressed.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_press<F>(&self, on_press: F)
    where
        F: FnMut(&Arc<Button>) + Send + 'static,
    {
        self.state
            .lock()
            .on_press
            .borrow_mut()
            .push(Box::new(on_press));
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
            back_color: self.theme.colors.back3,
            text_body: TextBody {
                hori_align: TextHoriAlign::Center,
                vert_align: TextVertAlign::Center,
                text_wrap: TextWrap::None,
                base_attrs: TextAttrs {
                    height: Pixels(self.theme.text_height),
                    color: self.theme.colors.text1a,
                    font_family: self.theme.font_family.clone(),
                    font_weight: self.theme.font_weight,
                    ..Default::default()
                },
                ..TextBody::from(self.props.text.clone())
            },
            ..self.props.placement.clone().into_style()
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

#[derive(Clone, Copy, Default)]
pub(crate) struct BtnHookColors {
    pub text_clr: Option<Color>,
    pub back_clr: Option<Color>,
    pub vert_clr: Option<Color>,
    pub h_text_clr: Option<Color>,
    pub h_back_clr: Option<Color>,
    pub h_vert_clr: Option<Color>,
    pub p_text_clr: Option<Color>,
    pub p_back_clr: Option<Color>,
    pub p_vert_clr: Option<Color>,
}

pub(crate) fn button_hooks<F>(button: &Arc<Bin>, colors: BtnHookColors, mut on_press: F)
where
    F: FnMut(&WindowState) + Send + 'static,
{
    let inside = Arc::new(AtomicBool::new(false));
    let pressed = Arc::new(AtomicBool::new(false));
    let cb_inside = inside.clone();
    let cb_pressed = pressed.clone();

    button.on_enter(move |target, _| {
        let button = target.into_bin().unwrap();
        cb_inside.store(true, atomic::Ordering::SeqCst);

        if !cb_pressed.load(atomic::Ordering::SeqCst)
            && (colors.h_text_clr.is_some()
                || colors.h_back_clr.is_some()
                || colors.h_vert_clr.is_some())
        {
            let mut style = button.style_copy();

            if let Some(h_text_clr) = colors.h_text_clr {
                style.text_body.base_attrs.color = h_text_clr;
            }

            if let Some(h_back_clr) = colors.h_back_clr {
                style.back_color = h_back_clr;
            }

            if let Some(h_vert_clr) = colors.h_vert_clr {
                style.user_vertexes.iter_mut().for_each(|(_, vertexes)| {
                    vertexes
                        .iter_mut()
                        .for_each(|vertex| vertex.color = h_vert_clr)
                });
            }

            button.style_update(style).expect_valid();
        }

        Default::default()
    });

    let cb_inside = inside.clone();
    let cb_pressed = pressed.clone();

    button.on_leave(move |target, _| {
        let button = target.into_bin().unwrap();
        cb_inside.store(false, atomic::Ordering::SeqCst);

        if !cb_pressed.load(atomic::Ordering::SeqCst)
            && (colors.h_text_clr.is_some()
                || colors.h_back_clr.is_some()
                || colors.h_vert_clr.is_some())
        {
            let mut style = button.style_copy();

            if let Some(text_clr) = colors.text_clr {
                style.text_body.base_attrs.color = text_clr;
            }

            if let Some(back_clr) = colors.back_clr {
                style.back_color = back_clr;
            }

            if let Some(vert_clr) = colors.vert_clr {
                style.user_vertexes.iter_mut().for_each(|(_, vertexes)| {
                    vertexes
                        .iter_mut()
                        .for_each(|vertex| vertex.color = vert_clr)
                });
            }

            button.style_update(style).expect_valid();
        }

        Default::default()
    });

    let cb_pressed = pressed.clone();

    button.on_press(MouseButton::Left, move |target, w_state, _| {
        let button = target.into_bin().unwrap();
        cb_pressed.store(true, atomic::Ordering::SeqCst);

        if colors.p_text_clr.is_some() || colors.p_back_clr.is_some() || colors.p_vert_clr.is_some()
        {
            let mut style = button.style_copy();

            if let Some(p_text_clr) = colors.p_text_clr {
                style.text_body.base_attrs.color = p_text_clr;
            }

            if let Some(p_back_clr) = colors.p_back_clr {
                style.back_color = p_back_clr;
            }

            if let Some(p_vert_clr) = colors.p_vert_clr {
                style.user_vertexes.iter_mut().for_each(|(_, vertexes)| {
                    vertexes
                        .iter_mut()
                        .for_each(|vertex| vertex.color = p_vert_clr)
                });
            }

            button.style_update(style).expect_valid();
        }

        on_press(w_state);
        Default::default()
    });

    let cb_inside = inside;
    let cb_pressed = pressed;

    button.on_release(MouseButton::Left, move |target, _, _| {
        let button = target.into_bin().unwrap();
        cb_pressed.store(false, atomic::Ordering::SeqCst);

        if cb_inside.load(atomic::Ordering::SeqCst)
            && (colors.h_text_clr.is_some()
                || colors.h_back_clr.is_some()
                || colors.h_vert_clr.is_some())
        {
            let mut style = button.style_copy();

            if let Some(h_text_clr) = colors.h_text_clr {
                style.text_body.base_attrs.color = h_text_clr;
            }

            if let Some(h_back_clr) = colors.h_back_clr {
                style.back_color = h_back_clr;
            }

            if let Some(h_vert_clr) = colors.h_vert_clr {
                style.user_vertexes.iter_mut().for_each(|(_, vertexes)| {
                    vertexes
                        .iter_mut()
                        .for_each(|vertex| vertex.color = h_vert_clr)
                });
            }

            button.style_update(style).expect_valid();
        } else {
            let mut style = button.style_copy();

            if let Some(text_clr) = colors.text_clr {
                style.text_body.base_attrs.color = text_clr;
            }

            if let Some(back_clr) = colors.back_clr {
                style.back_color = back_clr;
            }

            if let Some(vert_clr) = colors.vert_clr {
                style.user_vertexes.iter_mut().for_each(|(_, vertexes)| {
                    vertexes
                        .iter_mut()
                        .for_each(|vertex| vertex.color = vert_clr)
                });
            }

            button.style_update(style).expect_valid();
        }

        Default::default()
    });
}
