use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::ReentrantMutex;

use crate::image::ImageKey;
use crate::input::MouseButton;
use crate::interface::UnitValue::{Percent, Pixels};
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Theme, WidgetContainer, WidgetPlacement};
use crate::interface::{Bin, BinStyle, BinVertex, Color, Position, Visibility};

/// Builder for [`CheckBox`]
pub struct CheckBoxBuilder<'a, C, T> {
    widget: WidgetBuilder<'a, C>,
    props: Properties<T>,
    selected: bool,
    on_change: Vec<Box<dyn FnMut(&Arc<CheckBox<T>>, bool) + Send + 'static>>,
}

struct Properties<T> {
    value: T,
    placement: WidgetPlacement,
}

impl<T> Properties<T> {
    fn new(value: T, placement: WidgetPlacement) -> Self {
        Self {
            value,
            placement,
        }
    }
}

impl<'a, C, T> CheckBoxBuilder<'a, C, T>
where
    C: WidgetContainer,
    T: Send + Sync + 'static,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>, value: T) -> Self {
        Self {
            props: Properties::new(
                value,
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| CheckBox::<()>::default_placement(&builder.theme)),
            ),
            widget: builder,
            selected: false,
            on_change: Vec::new(),
        }
    }

    /// Specify if the [`CheckBox`] should be selected after being built.
    ///
    /// **Note**: When this isn't used this defaults to `false`.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Add a callback to be called when the [`CheckBox`]'s state changed.
    ///
    /// **Note**: When changing the state within the callback, no callbacks will be called with
    /// the updated state.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: FnMut(&Arc<CheckBox<T>>, bool) + Send + 'static,
    {
        self.on_change.push(Box::new(on_change));
        self
    }

    /// Finish building the [`CheckBox`].
    pub fn build(self) -> Arc<CheckBox<T>> {
        let window = self
            .widget
            .container
            .container_bin()
            .window()
            .expect("The widget container must have an associated window.");

        let mut new_bins = window.new_bins(2).into_iter();
        let container = new_bins.next().unwrap();
        let fill = new_bins.next().unwrap();

        self.widget
            .container
            .container_bin()
            .add_child(container.clone());

        container.add_child(fill.clone());

        let check_box = Arc::new(CheckBox {
            theme: self.widget.theme,
            props: self.props,
            container,
            fill,
            state: ReentrantMutex::new(State {
                selected: RefCell::new(false),
                on_change: RefCell::new(self.on_change),
            }),
        });

        let cb_check_box = check_box.clone();

        check_box
            .container
            .on_press(MouseButton::Left, move |_, _, _| {
                cb_check_box.toggle_select();
                Default::default()
            });

        let cb_check_box = check_box.clone();

        check_box.fill.on_press(MouseButton::Left, move |_, _, _| {
            cb_check_box.toggle_select();
            Default::default()
        });

        check_box.style_update();
        check_box
    }
}

/// Check box widget
pub struct CheckBox<T> {
    theme: Theme,
    props: Properties<T>,
    container: Arc<Bin>,
    fill: Arc<Bin>,
    state: ReentrantMutex<State<T>>,
}

struct State<T> {
    selected: RefCell<bool>,
    on_change: RefCell<Vec<Box<dyn FnMut(&Arc<CheckBox<T>>, bool) + Send + 'static>>>,
}

impl<T> CheckBox<T> {
    /// Select this [`CheckBox`].
    pub fn select(self: &Arc<Self>) {
        self.set_selected(true);
    }

    /// Unselect this [`CheckBox`]
    pub fn unselect(self: &Arc<Self>) {
        self.set_selected(false);
    }

    /// Toggle the selection of this [`CheckBox`].
    ///
    /// Returns the new selection state.
    pub fn toggle_select(self: &Arc<Self>) -> bool {
        let state = self.state.lock();
        let selected = !*state.selected.borrow();
        self.set_selected(selected);
        selected
    }

    /// Check if the [`CheckBox`] is selected.
    pub fn is_selected(&self) -> bool {
        *self.state.lock().selected.borrow()
    }

    /// Obtain a reference the value.
    pub fn value_ref(&self) -> &T {
        &self.props.value
    }

    /// Add a callback to be called when the [`CheckBox`]'s selection changed.
    ///
    /// **Note**: When changing the state within the callback, no callbacks add to this
    /// [`CheckBox`] will be called with the updated state.
    ///
    /// **Panics**: When adding a callback within the callback to this [`CheckBox`].
    pub fn on_change<F>(&self, on_change: F)
    where
        F: FnMut(&Arc<CheckBox<T>>, bool) + Send + 'static,
    {
        self.state
            .lock()
            .on_change
            .borrow_mut()
            .push(Box::new(on_change));
    }

    fn set_selected(self: &Arc<Self>, selected: bool) {
        let state = self.state.lock();

        if *state.selected.borrow() == selected {
            return;
        }

        *state.selected.borrow_mut() = selected;
        let mut fill_style = self.fill.style_copy();

        if selected {
            fill_style.visibility = Visibility::Inheirt;
        } else {
            fill_style.visibility = Visibility::Hide;
        }

        self.fill.style_update(fill_style).expect_valid();

        if let Ok(mut on_change_cbs) = state.on_change.try_borrow_mut() {
            for on_change in on_change_cbs.iter_mut() {
                on_change(self, selected);
            }
        }
    }

    /// Obtain the default [`WidgetPlacement`](`WidgetPlacement`) given a [`Theme`](`Theme`).
    pub fn default_placement(theme: &Theme) -> WidgetPlacement {
        WidgetPlacement {
            position: Position::Floating,
            margin_t: Pixels(theme.spacing),
            margin_b: Pixels(theme.spacing),
            margin_l: Pixels(theme.spacing),
            margin_r: Pixels(theme.spacing),
            width: Pixels(theme.base_size),
            height: Pixels(theme.base_size),
            ..Default::default()
        }
    }

    fn style_update(&self) {
        let mut container_style = BinStyle {
            back_color: self.theme.colors.back2,
            ..self.props.placement.clone().into_style()
        };

        let mut fill_style = BinStyle {
            visibility: Visibility::Hide,
            pos_from_t: Pixels(0.0),
            pos_from_b: Pixels(0.0),
            pos_from_l: Pixels(0.0),
            pos_from_r: Pixels(0.0),
            user_vertexes: vec![(
                ImageKey::INVALID,
                check_symbol_verts(self.theme.colors.accent1),
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
        }

        if let Some(radius) = self.theme.roundness {
            container_style.border_radius_tl = Pixels(radius);
            container_style.border_radius_tr = Pixels(radius);
            container_style.border_radius_bl = Pixels(radius);
            container_style.border_radius_br = Pixels(radius);
        }

        if self.is_selected() {
            fill_style.visibility = Visibility::Inheirt;
        }

        Bin::style_update_batch([(&self.container, container_style), (&self.fill, fill_style)]);
    }
}

impl<T> CheckBox<T>
where
    T: Clone,
{
    /// Obtain a copy of the value.
    pub fn value(&self) -> T {
        self.props.value.clone()
    }
}

fn check_symbol_verts(color: Color) -> Vec<BinVertex> {
    const UNIT_POS: [[f32; 2]; 6] = [
        [0.912, 0.131],
        [1.000, 0.218],
        [0.087, 0.432],
        [0.000, 0.519],
        [0.349, 0.694],
        [0.349, 0.868],
    ];

    let mut verts = Vec::with_capacity(12);

    for i in [5, 1, 0, 5, 0, 4, 5, 4, 2, 5, 2, 3] {
        verts.push(BinVertex {
            x: Percent((UNIT_POS[i][0] * 90.0) + 5.0),
            y: Percent((UNIT_POS[i][1] * 90.0) + 5.0),
            color,
            ..Default::default()
        });
    }

    verts
}
