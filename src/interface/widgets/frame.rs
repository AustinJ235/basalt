use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::ReentrantMutex;

use crate::interface::UnitValue::Pixels;
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{ScrollAxis, ScrollBar, Theme, Container, WidgetPlacement};
use crate::interface::{Bin, BinStyle, Position, StyleUpdateBatch, Visibility};

/// When a [`ScrollBar`] is shown.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollBarVisibility {
    /// Automatically hide when there isn't any overflow.
    ///
    /// **Default**
    #[default]
    Auto,
    /// Always show regardless of overflow.
    Visible,
    /// Always hide regardless of overflow.
    ///
    /// **Note:** Scrolling will still work, but the scrollbar will remain hidden.
    Hidden,
}

#[derive(Default)]
struct FrameProperties {
    v_sb_visibility: ScrollBarVisibility,
    h_sb_visibility: ScrollBarVisibility,
}

/// Builder for [`Frame`]
pub struct FrameBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    properties: FrameProperties,
}

impl<'a, C> FrameBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            properties: Default::default(),
            widget: builder,
        }
    }

    /// Set the vertical [`ScrollBar`]'s visibility.
    pub fn v_sb_visibility(mut self, visibility: ScrollBarVisibility) -> Self {
        self.properties.v_sb_visibility = visibility;
        self
    }

    /// Set the horizontal [`ScrollBar`]'s visibility.
    pub fn h_sb_visibility(mut self, visibility: ScrollBarVisibility) -> Self {
        self.properties.h_sb_visibility = visibility;
        self
    }

    /// Finish building the [`Frame`].
    pub fn build(self) -> Arc<Frame> {
        let container = self.widget.container.create_bin();
        let view_area = container.create_bin();

        let v_scroll_b = container
            .create_widget()
            .with_theme(self.widget.theme.clone())
            .with_placement(WidgetPlacement {
                visibility: Visibility::Hide,
                ..ScrollBar::default_placement(&self.widget.theme, ScrollAxis::Y)
            })
            .scroll_bar(view_area.clone())
            .build();

        let h_scroll_b = container
            .create_widget()
            .with_theme(self.widget.theme.clone())
            .with_placement(WidgetPlacement {
                visibility: Visibility::Hide,
                ..ScrollBar::default_placement(&self.widget.theme, ScrollAxis::X)
            })
            .scroll_bar(view_area.clone())
            .axis(ScrollAxis::X)
            .build();

        let placement = self
            .widget
            .placement
            .unwrap_or_else(|| Frame::default_placement(&self.widget.theme));

        let frame = Arc::new(Frame {
            theme: self.widget.theme,
            properties: self.properties,
            container,
            view_area,
            v_scroll_b,
            h_scroll_b,
            state: ReentrantMutex::new(State {
                placement: RefCell::new(placement),
                visibility: RefCell::new(VisibilityState {
                    v_scroll_b: false,
                    h_scroll_b: false,
                }),
            }),
        });

        let frame_wk = Arc::downgrade(&frame);

        frame.view_area.on_update(move |_, _| {
            if let Some(frame) = frame_wk.upgrade() {
                frame.style_update(false, None);
            }
        });

        frame.style_update(true, None);
        frame
    }
}

/// Frame widget
///
/// This widget is used as a container with automatic scroll bars.
pub struct Frame {
    theme: Theme,
    properties: FrameProperties,
    container: Arc<Bin>,
    view_area: Arc<Bin>,
    v_scroll_b: Arc<ScrollBar>,
    h_scroll_b: Arc<ScrollBar>,
    state: ReentrantMutex<State>,
}

struct State {
    placement: RefCell<WidgetPlacement>,
    visibility: RefCell<VisibilityState>,
}

#[derive(Clone, PartialEq)]
struct VisibilityState {
    v_scroll_b: bool,
    h_scroll_b: bool,
}

impl Frame {
    /// Obtain the default [`WidgetPlacement`](`WidgetPlacement`) given a [`Theme`](`Theme`).
    pub fn default_placement(theme: &Theme) -> WidgetPlacement {
        let height = theme.spacing + (theme.base_size * 16.0);
        let width = theme.spacing + (theme.base_size * 16.0);

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

    pub fn update_placement(&self, placement: WidgetPlacement) {
        let state = self.state.lock();
        *state.placement.borrow_mut() = placement;
        self.style_update(true, None);
    }

    pub fn update_placement_with_batch<'a>(
        &'a self,
        placement: WidgetPlacement,
        batch: &mut StyleUpdateBatch<'a>,
    ) {
        let state = self.state.lock();
        *state.placement.borrow_mut() = placement;
        self.style_update(true, Some(batch));
    }

    fn style_update<'a>(&'a self, force_update: bool, batch_op: Option<&mut StyleUpdateBatch<'a>>) {
        let state = self.state.lock();

        let [vsb_show, hsb_show, sb_plcmt_update] = {
            let vsb_show = match self.properties.v_sb_visibility {
                ScrollBarVisibility::Visible => true,
                ScrollBarVisibility::Hidden => false,
                ScrollBarVisibility::Auto => self.v_scroll_b.target_overflow() > 0.0,
            };

            let hsb_show = match self.properties.h_sb_visibility {
                ScrollBarVisibility::Visible => true,
                ScrollBarVisibility::Hidden => false,
                ScrollBarVisibility::Auto => self.h_scroll_b.target_overflow() > 0.0,
            };

            let mut visibility = state.visibility.borrow_mut();
            let sb_plcmt_update =
                visibility.v_scroll_b != vsb_show || visibility.h_scroll_b != hsb_show;

            if !force_update && !sb_plcmt_update {
                return;
            }

            visibility.v_scroll_b = vsb_show;
            visibility.h_scroll_b = hsb_show;
            [vsb_show, hsb_show, sb_plcmt_update]
        };

        let placement = state.placement.borrow().clone();

        let sb_offset = match ScrollBar::default_placement(&self.theme, ScrollAxis::Y).width {
            Pixels(px) => px,
            _ => unreachable!(),
        } + self.theme.border.unwrap_or(0.0);

        let mut container_style = BinStyle {
            back_color: self.theme.colors.back1,
            ..placement.clone().into_style()
        };

        let mut view_area_style = BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_b: hsb_show.then_some(Pixels(sb_offset)).unwrap_or(Pixels(0.0)),
            pos_from_l: Pixels(0.0),
            pos_from_r: vsb_show.then_some(Pixels(sb_offset)).unwrap_or(Pixels(0.0)),
            ..Default::default()
        };

        self.view_area.style_inspect(|style| {
            view_area_style.scroll_x = style.scroll_x;
            view_area_style.scroll_y = style.scroll_y;
        });

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
            view_area_style.border_radius_tl = Pixels(border_radius);

            if !hsb_show {
                view_area_style.border_radius_bl = Pixels(border_radius);
            }

            if !vsb_show {
                view_area_style.border_radius_tr = Pixels(border_radius);
            }

            if !hsb_show && !vsb_show {
                view_area_style.border_radius_br = Pixels(border_radius);
            }
        }

        let mut owned_batch_op = batch_op.is_none().then(StyleUpdateBatch::default);
        let mut batch = batch_op.or(owned_batch_op.as_mut()).unwrap();

        if force_update || sb_plcmt_update {
            self.v_scroll_b.update_placement_with_batch(
                WidgetPlacement {
                    visibility: vsb_show
                        .then_some(Default::default())
                        .unwrap_or(Visibility::Hide),
                    pos_from_b: hsb_show.then_some(Pixels(sb_offset)).unwrap_or(Pixels(0.0)),
                    ..ScrollBar::default_placement(&self.theme, ScrollAxis::Y)
                },
                &mut batch,
            );

            self.h_scroll_b.update_placement_with_batch(
                WidgetPlacement {
                    visibility: hsb_show
                        .then_some(Default::default())
                        .unwrap_or(Visibility::Hide),
                    pos_from_r: vsb_show.then_some(Pixels(sb_offset)).unwrap_or(Pixels(0.0)),
                    ..ScrollBar::default_placement(&self.theme, ScrollAxis::X)
                },
                &mut batch,
            );
        }

        batch.update(&self.container, container_style);
        batch.update(&self.view_area, view_area_style);

        if let Some(owned_batch) = owned_batch_op {
            owned_batch.commit();
        }
    }
}

impl Container for Arc<Frame> {
    fn create_bins(&self, count: usize) -> impl Iterator<Item = Arc<Bin>> {
        self.view_area.create_bins(count)
    }
}
