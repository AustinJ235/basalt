use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::ReentrantMutex;

use crate::input::MouseButton;
use crate::interface::UnitValue::{Percent, Pixels};
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Theme, Container, WidgetPlacement};
use crate::interface::{Bin, BinStyle, Position};

/// Builder for [`ProgressBar`].
pub struct ProgressBarBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    on_press: Vec<Box<dyn FnMut(&Arc<ProgressBar>, f32) + Send + 'static>>,
}

struct Properties {
    pct: f32,
    placement: WidgetPlacement,
}

impl Properties {
    fn new(placement: WidgetPlacement) -> Self {
        Self {
            pct: 0.0,
            placement,
        }
    }
}

impl<'a, C> ProgressBarBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| ProgressBar::default_placement(&builder.theme)),
            ),
            widget: builder,
            on_press: Vec::new(),
        }
    }

    /// Set the initial percent.
    ///
    /// **Note**: When this isn't used the percent will be `0.0`.
    pub fn set_pct(mut self, pct: f32) -> Self {
        self.props.pct = pct.clamp(0.0, 100.0);
        self
    }

    /// Add a callback to be called when the [`ProgressBar`] is pressed.
    ///
    /// The callback is called with the cursors percent along the [`ProgressBar`].
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_press<F>(mut self, on_press: F) -> Self
    where
        F: FnMut(&Arc<ProgressBar>, f32) + Send + 'static,
    {
        self.on_press.push(Box::new(on_press));
        self
    }

    /// Finish building the [`ProgressBar`].
    pub fn build(self) -> Arc<ProgressBar> {
        let container = self.widget.container.create_bin();
        let fill = container.create_bin();
        let initial_pct = self.props.pct;

        let progress_bar = Arc::new(ProgressBar {
            theme: self.widget.theme,
            props: self.props,
            container,
            fill,
            state: ReentrantMutex::new(State {
                pct: RefCell::new(initial_pct),
                on_press: RefCell::new(self.on_press),
            }),
        });

        let cb_progress_bar = progress_bar.clone();

        progress_bar
            .container
            .on_press(MouseButton::Left, move |_, w_state, _| {
                cb_progress_bar.proc_press(w_state.cursor_pos());
                Default::default()
            });

        let cb_progress_bar = progress_bar.clone();

        progress_bar
            .fill
            .on_press(MouseButton::Left, move |_, w_state, _| {
                cb_progress_bar.proc_press(w_state.cursor_pos());
                Default::default()
            });

        progress_bar.style_update();
        progress_bar
    }
}

/// Progress bar widget
pub struct ProgressBar {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    fill: Arc<Bin>,
    state: ReentrantMutex<State>,
}

struct State {
    pct: RefCell<f32>,
    on_press: RefCell<Vec<Box<dyn FnMut(&Arc<ProgressBar>, f32) + Send + 'static>>>,
}

impl ProgressBar {
    /// Set the percent
    pub fn set_pct(self: &Arc<Self>, pct: f32) {
        let pct = pct.clamp(0.0, 100.0);

        self.fill
            .style_update(BinStyle {
                width: Percent(pct),
                ..self.fill.style_copy()
            })
            .expect_valid();

        *self.state.lock().pct.borrow_mut() = pct;
    }

    /// Get the current percent
    pub fn pct(&self) -> f32 {
        *self.state.lock().pct.borrow()
    }

    /// Add a callback to be called when the [`ProgressBar`] is pressed.
    ///
    /// The callback is called with the cursors percent along the [`ProgressBar`].
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_press<F>(&self, on_press: F)
    where
        F: FnMut(&Arc<ProgressBar>, f32) + Send + 'static,
    {
        self.state
            .lock()
            .on_press
            .borrow_mut()
            .push(Box::new(on_press));
    }

    fn proc_press(self: &Arc<Self>, cursor: [f32; 2]) {
        let bpu = self.container.post_update();

        let pct =
            (((cursor[0] - bpu.tli[0]) / (bpu.tri[0] - bpu.tli[0])) * 100.0).clamp(0.0, 100.0);

        let state = self.state.lock();

        for on_press in state.on_press.borrow_mut().iter_mut() {
            on_press(self, pct);
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
            width: Pixels(theme.base_size * 4.0),
            height: Pixels(theme.base_size),
            ..Default::default()
        }
    }

    fn style_update(self: &Arc<Self>) {
        let pct = *self.state.lock().pct.borrow();

        let mut container_style = BinStyle {
            back_color: self.theme.colors.back2,
            ..self.props.placement.clone().into_style()
        };

        let mut fill_style = BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_b: Pixels(0.0),
            pos_from_l: Pixels(0.0),
            width: Percent(pct),
            back_color: self.theme.colors.accent1,
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
            fill_style.border_radius_tl = Pixels(radius);
            fill_style.border_radius_tr = Pixels(radius);
            fill_style.border_radius_bl = Pixels(radius);
            fill_style.border_radius_br = Pixels(radius);
        }

        Bin::style_update_batch([(&self.container, container_style), (&self.fill, fill_style)]);
    }
}
