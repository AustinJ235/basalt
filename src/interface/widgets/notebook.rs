use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::ReentrantMutex;

use crate::input::{InputHookCtrl, MouseButton};
use crate::interface::UnitValue::{Percent, Pixels};
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Container, Frame, ScrollBarVisibility, Theme, WidgetPlacement};
use crate::interface::{
    Bin, BinStyle, FloatWeight, Position, StyleUpdateBatch, TextAttrs, TextBody, TextSpan,
    TextVertAlign, TextWrap, Visibility, ZIndex,
};

#[derive(Default)]
struct NotebookProperties {
    v_sb_visibility: ScrollBarVisibility,
    h_sb_visibility: ScrollBarVisibility,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NotebookError {
    PageIdInvalid,
    PageIdExists,
}

/// Builder for [`Notebook`]
pub struct NotebookBuilder<'a, C, I> {
    widget: WidgetBuilder<'a, C>,
    properties: NotebookProperties,
    create_pages: BTreeMap<I, String>,
    select_page: Option<I>,
}

impl<'a, C, I> NotebookBuilder<'a, C, I>
where
    C: Container,
    I: Ord + Copy + Send + 'static,
{
    pub(crate) fn with_builder(builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            properties: Default::default(),
            widget: builder,
            create_pages: BTreeMap::new(),
            select_page: None,
        }
    }

    pub fn with_page<L>(mut self, page_id: I, label: L) -> Self
    where
        L: Into<String>,
    {
        self.create_pages.insert(page_id, label.into());
        self
    }

    pub fn select_page(mut self, page_id: I) -> Result<Self, NotebookError> {
        if !self.create_pages.contains_key(&page_id) {
            return Err(NotebookError::PageIdInvalid);
        }

        self.select_page = Some(page_id);
        Ok(self)
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

    /// Finish building the [`Notebook`].
    pub fn build(self) -> Arc<Notebook<I>> {
        let container = self.widget.container.create_bin();
        let nav_bar = container.create_bin();

        let placement = self
            .widget
            .placement
            .unwrap_or_else(|| Notebook::<()>::default_placement(&self.widget.theme));

        let notebook = Arc::new(Notebook {
            theme: self.widget.theme,
            properties: self.properties,
            container,
            nav_bar,
            state: ReentrantMutex::new(State {
                placement: RefCell::new(placement),
                pages: RefCell::new(BTreeMap::new()),
                current_page: RefCell::new(None),
            }),
        });

        for (page_id, page_label) in self.create_pages {
            notebook.page_add(page_id, page_label).unwrap();
        }

        if let Some(page_id) = self.select_page {
            notebook.page_switch(page_id).unwrap();
        }

        notebook.style_update(StyleUpdateKind::Initial, None);
        notebook
    }
}

/// Notebook widget
pub struct Notebook<I> {
    theme: Theme,
    properties: NotebookProperties,
    container: Arc<Bin>,
    nav_bar: Arc<Bin>,
    state: ReentrantMutex<State<I>>,
}

struct State<I> {
    placement: RefCell<WidgetPlacement>,
    pages: RefCell<BTreeMap<I, PageState>>,
    current_page: RefCell<Option<I>>,
}

struct PageState {
    label: String,
    frame: Arc<Frame>,
    nav_item: Arc<Bin>,
}

enum StyleUpdateKind {
    Initial,
    PageAdded,
    PageSwitch,
    Placement,
}

impl<I> Notebook<I>
where
    I: Ord + Copy + Send + 'static,
{
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

    pub fn page_add<L>(self: &Arc<Self>, page_id: I, page_label: L) -> Result<(), NotebookError>
    where
        L: Into<String>,
    {
        let state = self.state.lock();
        let mut pages = state.pages.borrow_mut();

        if pages.contains_key(&page_id) {
            return Err(NotebookError::PageIdExists);
        }

        let frame_pft =
            self.theme.spacing + self.theme.base_size + self.theme.border.unwrap_or(0.0);

        let frame = self
            .container
            .create_widget()
            .with_placement(WidgetPlacement {
                visibility: Visibility::Hide,
                pos_from_t: Pixels(frame_pft),
                pos_from_l: Pixels(0.0),
                pos_from_r: Pixels(0.0),
                pos_from_b: Pixels(0.0),
                ..Default::default()
            })
            .frame()
            .v_sb_visibility(self.properties.v_sb_visibility)
            .h_sb_visibility(self.properties.h_sb_visibility)
            .build();

        let nav_item = self.nav_bar.create_bin();
        let notebook_wk = Arc::downgrade(self);

        // TODO: use button hooks instead?
        nav_item.on_press(MouseButton::Left, move |_, _, _| {
            let notebook = match notebook_wk.upgrade() {
                Some(some) => some,
                None => return InputHookCtrl::Remove,
            };

            notebook.page_switch(page_id).unwrap();
            Default::default()
        });

        pages.insert(
            page_id,
            PageState {
                label: page_label.into(),
                frame,
                nav_item,
            },
        );

        drop(pages);
        self.style_update(StyleUpdateKind::PageAdded, None);
        Ok(())
    }

    pub fn page_remove(&self, page_id: I) -> Result<(), NotebookError> {
        let state = self.state.lock();

        state
            .pages
            .borrow_mut()
            .remove(&page_id)
            .ok_or(NotebookError::PageIdInvalid)?;

        if *state.current_page.borrow() == Some(page_id) {
            *state.current_page.borrow_mut() = None;
        }

        self.style_update(StyleUpdateKind::PageSwitch, None);
        Ok(())
    }

    pub fn page_frame(&self, page_id: I) -> Option<Arc<Frame>> {
        self.state
            .lock()
            .pages
            .borrow()
            .get(&page_id)
            .map(|page| page.frame.clone())
    }

    pub fn page_current(&self) -> Option<I> {
        *self.state.lock().current_page.borrow()
    }

    pub fn page_switch(&self, page_id: I) -> Result<(), NotebookError> {
        let state = self.state.lock();

        if !state.pages.borrow().contains_key(&page_id) {
            return Err(NotebookError::PageIdInvalid);
        }

        *state.current_page.borrow_mut() = Some(page_id);
        self.style_update(StyleUpdateKind::PageSwitch, None);
        Ok(())
    }

    pub fn page_set_label<L>(&self, page_id: I, label: L) -> Result<(), NotebookError>
    where
        L: Into<String>,
    {
        let state = self.state.lock();
        let pages = state.pages.borrow();
        let page = pages.get(&page_id).ok_or(NotebookError::PageIdInvalid)?;

        page.nav_item.style_modify(|style| {
            style.text_body.spans[0].text = label.into();
        });

        Ok(())
    }

    pub fn update_placement(&self, placement: WidgetPlacement) {
        let state = self.state.lock();
        *state.placement.borrow_mut() = placement;
        self.style_update(StyleUpdateKind::Placement, None);
    }

    pub fn update_placement_with_batch(
        &self,
        placement: WidgetPlacement,
        batch: &mut StyleUpdateBatch,
    ) {
        let state = self.state.lock();
        *state.placement.borrow_mut() = placement;
        self.style_update(StyleUpdateKind::Placement, Some(batch));
    }

    fn style_update(&self, kind: StyleUpdateKind, batch_op: Option<&mut StyleUpdateBatch>) {
        let state = self.state.lock();
        let mut owned_batch_op = batch_op.is_none().then(StyleUpdateBatch::default);
        let batch = batch_op.or(owned_batch_op.as_mut()).unwrap();

        if matches!(kind, StyleUpdateKind::Initial | StyleUpdateKind::Placement) {
            let mut container_style = BinStyle {
                overflow_y: true,
                overflow_x: true,
                ..state.placement.borrow().clone().into_style()
            };

            let mut nav_bar_style = BinStyle {
                pos_from_t: Pixels(0.0),
                pos_from_l: Pixels(0.0),
                pos_from_r: Pixels(0.0),
                height: Pixels(self.theme.base_size + self.theme.spacing),
                overflow_y: true,
                overflow_x: true,
                ..Default::default()
            };

            if let Some(border_radius) = self.theme.roundness {
                container_style.border_radius_tl = Pixels(border_radius);
                container_style.border_radius_tr = Pixels(border_radius);
                nav_bar_style.border_radius_tl = Pixels(border_radius);
                nav_bar_style.border_radius_tr = Pixels(border_radius);
            }

            batch.update(&self.container, container_style);
            batch.update(&self.nav_bar, nav_bar_style);
        }

        if matches!(
            kind,
            StyleUpdateKind::Initial | StyleUpdateKind::PageAdded | StyleUpdateKind::PageSwitch
        ) {
            let border_size = self.theme.border.unwrap_or(0.0);
            let pages = state.pages.borrow();

            let cur_page_i = match *state.current_page.borrow() {
                Some(cur_page_id) => {
                    pages
                        .keys()
                        .enumerate()
                        .find(|(_, page_id)| **page_id == cur_page_id)
                        .map(|(i, _)| i)
                },
                None => None,
            };

            for (i, page) in state.pages.borrow().values().enumerate() {
                let is_current = Some(i) == cur_page_i;

                let mut nav_item_style = BinStyle {
                    position: Position::Floating,
                    float_weight: FloatWeight::Fixed(i as i16),
                    width: Pixels(self.theme.base_size * 5.0), // TODO: Auto width
                    padding_l: Pixels(self.theme.spacing),
                    padding_r: Pixels(self.theme.spacing),
                    margin_r: Pixels(border_size),
                    height: Percent(100.0),
                    back_color: if is_current {
                        self.theme.colors.back2
                    } else {
                        self.theme.colors.back3
                    },
                    text_body: TextBody {
                        base_attrs: TextAttrs {
                            color: self.theme.colors.text1a,
                            height: Pixels(self.theme.text_height),
                            font_family: self.theme.font_family.clone(),
                            font_weight: self.theme.font_weight,
                            ..Default::default()
                        },
                        spans: vec![TextSpan::from(&page.label)],
                        text_wrap: TextWrap::None,
                        vert_align: TextVertAlign::Center,
                        ..Default::default()
                    },
                    ..Default::default()
                };

                if let Some(border_size) = self.theme.border {
                    if is_current {
                        nav_item_style.z_index = ZIndex::Offset(1);
                        nav_item_style.border_size_t = Pixels(border_size);
                        nav_item_style.border_size_l = Pixels(border_size);
                        nav_item_style.border_size_r = Pixels(border_size);
                        nav_item_style.border_color_t = self.theme.colors.accent2;
                        nav_item_style.border_color_l = self.theme.colors.accent2;
                        nav_item_style.border_color_r = self.theme.colors.accent2;
                    } else {
                        nav_item_style.border_size_t = Pixels(border_size);
                        nav_item_style.border_color_t = self.theme.colors.border1;

                        if i == 0 {
                            nav_item_style.border_size_l = Pixels(border_size);
                            nav_item_style.border_color_l = self.theme.colors.border1;
                        }

                        match cur_page_i {
                            Some(cur_page_i) => {
                                if i < cur_page_i && cur_page_i - 1 == i {
                                    if let Some(border_radius) = self.theme.roundness {
                                        nav_item_style.border_size_r =
                                            Pixels(border_radius + border_size);
                                        nav_item_style.border_color_r = self.theme.colors.back3;
                                    }
                                } else {
                                    if cur_page_i + 1 == i
                                        && let Some(border_radius) = self.theme.roundness
                                    {
                                        nav_item_style.border_size_l =
                                            Pixels(border_radius + border_size);
                                        nav_item_style.border_color_l = self.theme.colors.back3;
                                    }

                                    nav_item_style.border_size_r = Pixels(border_size);
                                    nav_item_style.border_color_r = self.theme.colors.border1;
                                }
                            },
                            None => {
                                nav_item_style.border_size_r = Pixels(border_size);
                                nav_item_style.border_color_r = self.theme.colors.border1;
                            },
                        }
                    }
                }

                if let Some(border_radius) = self.theme.roundness {
                    if i == 0 && cur_page_i.is_some() {
                        // TODO: This is a workaround for not being able to disable the border
                        // radius on the top-left of the frame. Widget placement for the frame also
                        // requires an offset for this to work correctly to ensure it renders over
                        // this border.
                        nav_item_style.border_size_b = Pixels(border_radius + border_size);
                        nav_item_style.border_color_b = self.theme.colors.border1;
                    }

                    if is_current {
                        nav_item_style.border_radius_tl = Pixels(border_radius);
                        nav_item_style.border_radius_tr = Pixels(border_radius);
                    } else {
                        if i == 0 {
                            nav_item_style.border_radius_tl = Pixels(border_radius);
                        }

                        if i == pages.len() - 1 {
                            nav_item_style.border_radius_tr = Pixels(border_radius);
                        }
                    }
                }

                let text_body = page.nav_item.text_body();

                if let Err(_) = text_body.style_modify(|style| *style = nav_item_style) {
                    unreachable!()
                }

                let overflow = text_body.overflow();

                if overflow[0] > 0.0 {
                    if let Err(_) = text_body.style_modify(|style| {
                        style.width = style.width.offset_pixels(overflow[0]);
                    }) {
                        unreachable!()
                    }
                }

                text_body.finish();

                page.frame.update_placement_with_batch(
                    WidgetPlacement {
                        visibility: if is_current {
                            Default::default()
                        } else {
                            Visibility::Hide
                        },
                        z_index: ZIndex::Offset(2),
                        pos_from_t: Pixels(self.theme.base_size + self.theme.spacing + border_size),
                        pos_from_b: Pixels(0.0),
                        pos_from_l: Pixels(0.0),
                        pos_from_r: Pixels(0.0),
                        ..Default::default()
                    },
                    batch,
                );
            }
        }

        if let Some(owned_batch) = owned_batch_op {
            owned_batch.commit();
        }
    }
}
