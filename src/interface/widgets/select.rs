use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::ReentrantMutex;

use crate::image::ImageKey;
use crate::input::{MouseButton, Qwerty};
use crate::interface::UnitValue::{PctOfHeight, PctOffset, Pixels};
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::scroll_bar::down_symbol_verts;
use crate::interface::widgets::{ScrollBar, Theme, WidgetContainer, WidgetPlacement};
use crate::interface::{
    Bin, BinStyle, Position, TextAttrs, TextBody, TextHoriAlign, TextVertAlign, TextWrap,
    Visibility, ZIndex,
};

/// Builder for [`Select`]
pub struct SelectBuilder<'a, C, I> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    select: Option<I>,
    options: BTreeMap<I, String>,
    on_select: Vec<Box<dyn FnMut(&Arc<Select<I>>, Option<I>) + Send + 'static>>,
}

struct Properties {
    no_selection_label: String,
    drop_down_items: usize,
    placement: WidgetPlacement,
}

impl Properties {
    fn new(placement: WidgetPlacement) -> Self {
        Self {
            no_selection_label: String::new(),
            drop_down_items: 3,
            placement,
        }
    }
}

impl<'a, C, I> SelectBuilder<'a, C, I>
where
    C: WidgetContainer,
    I: Ord + Copy + Send + 'static,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| Select::<()>::default_placement(&builder.theme)),
            ),
            widget: builder,
            select: None,
            options: BTreeMap::new(),
            on_select: Vec::new(),
        }
    }

    /// Add an option with the provided id and label.
    ///
    /// **Note**: Ids must be unique. Adding an option of the same id as a previously added id will
    ///           overwrite the existing option.
    pub fn add_option<L>(mut self, option_id: I, label: L) -> Self
    where
        L: Into<String>,
    {
        self.options.insert(option_id, label.into());
        self
    }

    /// Set the option to be selected at creation.
    ///
    /// **Note**: If the id is not present nothing will be selected.
    pub fn select(mut self, option_id: I) -> Self {
        self.select = Some(option_id);
        self
    }

    /// Set the label to be shown when there is no selection.
    pub fn no_selection_label<L>(mut self, label: L) -> Self
    where
        L: Into<String>,
    {
        self.props.no_selection_label = label.into();
        self
    }

    /// Set the number of options to be displayed within the drop down.
    ///
    /// **Note**: If there are more options than what is specified to be displayed they'll be scrollable.
    pub fn drop_down_items(mut self, count: usize) -> Self {
        self.props.drop_down_items = count;
        self
    }

    /// Add a callback to be called when the selection changed.
    ///
    /// **Note**: When changing the state within the callback, no callbacks on this [`Select`]
    /// will be called.
    ///
    /// **Panics**: When adding a callback within the callback to this [`Select`].
    pub fn on_select<F>(mut self, on_select: F) -> Self
    where
        F: FnMut(&Arc<Select<I>>, Option<I>) + Send + 'static,
    {
        self.on_select.push(Box::new(on_select));
        self
    }

    /// Finish building the [`Select`].
    pub fn build(self) -> Arc<Select<I>> {
        let window = self
            .widget
            .container
            .container_bin()
            .window()
            .expect("The widget container must have an associated window.");

        let mut new_bins = window.new_bins(4 + self.options.len()).into_iter();
        let container = new_bins.next().unwrap();
        let popup = new_bins.next().unwrap();
        let arrow_down = new_bins.next().unwrap();
        let option_list = new_bins.next().unwrap();

        self.widget
            .container
            .container_bin()
            .add_child(container.clone());

        container.add_child(arrow_down.clone());
        container.add_child(popup.clone());
        popup.add_child(option_list.clone());

        let scroll_bar = popup
            .create_widget()
            .with_theme(self.widget.theme.clone())
            .scroll_bar(option_list.clone())
            .step(
                self.widget.theme.spacing
                    + self.widget.theme.base_size
                    + self.widget.theme.border.unwrap_or(0.0),
            )
            .build();

        let select_id = match self.select {
            Some(select_id) => {
                if self.options.keys().find(|id| **id == select_id).is_some() {
                    Some(select_id)
                } else {
                    None
                }
            },
            None => None,
        };

        let options_state = RefCell::new(BTreeMap::from_iter(self.options.into_iter().map(
            |(id, label)| {
                let bin = new_bins.next().unwrap();
                option_list.add_child(bin.clone());
                (
                    id,
                    OptionState {
                        label,
                        bin,
                    },
                )
            },
        )));

        let select = Arc::new(Select {
            theme: self.widget.theme,
            props: self.props,
            container,
            popup,
            arrow_down,
            scroll_bar,
            option_list,
            state: ReentrantMutex::new(State {
                select: RefCell::new(select_id),
                options: options_state,
                on_select: RefCell::new(self.on_select),
                popup: RefCell::new(PopupState {
                    visible: false,
                    select_i: None,
                }),
            }),
        });

        for target in [&select.container, &select.arrow_down] {
            let cb_select = select.clone();

            target.on_press(MouseButton::Left, move |_, _, _| {
                cb_select.toggle_popup();
                Default::default()
            });

            let cb_select = select.clone();

            target.on_press(Qwerty::ArrowDown, move |_, _, _| {
                cb_select.popup_select_next();
                Default::default()
            });

            let cb_select = select.clone();

            target.on_press(Qwerty::ArrowUp, move |_, _, _| {
                cb_select.popup_select_prev();
                Default::default()
            });

            let cb_select = select.clone();

            target.on_press(Qwerty::Enter, move |_, _, _| {
                let state = cb_select.state.lock();

                if state.popup.borrow().visible {
                    cb_select.popup_finish(false);
                } else {
                    cb_select.show_popup();
                }

                Default::default()
            });

            let cb_select = select.clone();

            target.on_press(Qwerty::Esc, move |_, _, _| {
                let state = cb_select.state.lock();

                if state.popup.borrow().visible {
                    cb_select.popup_finish(true);
                }

                Default::default()
            });
        }

        let cb_select = select.clone();
        let mut currently_focused = false;

        select
            .container
            .attach_input_hook(window.on_bin_focus_change(move |_, w_state, _| {
                let now_focused = match w_state.focused_bin_id() {
                    Some(bin_id) => {
                        bin_id == cb_select.container.id()
                            || bin_id == cb_select.popup.id()
                            || bin_id == cb_select.arrow_down.id()
                            || bin_id == cb_select.option_list.id()
                            || cb_select.scroll_bar.has_bin_id(bin_id)
                    },
                    None => false,
                };

                if currently_focused {
                    if !now_focused {
                        currently_focused = false;
                        cb_select.hide_popup();
                    }
                } else {
                    currently_focused = now_focused;
                }

                Default::default()
            }));

        select
            .state
            .lock()
            .options
            .borrow()
            .iter()
            .for_each(|(id, option_state)| {
                select.add_option_select_hook(*id, &option_state.bin);
            });

        select.style_update();
        select.rebuild_list();
        select
    }
}

/// Select widget
pub struct Select<I> {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    popup: Arc<Bin>,
    arrow_down: Arc<Bin>,
    scroll_bar: Arc<ScrollBar>,
    option_list: Arc<Bin>,
    state: ReentrantMutex<State<I>>,
}

struct State<I> {
    select: RefCell<Option<I>>,
    options: RefCell<BTreeMap<I, OptionState>>,
    on_select: RefCell<Vec<Box<dyn FnMut(&Arc<Select<I>>, Option<I>) + Send + 'static>>>,
    popup: RefCell<PopupState>,
}

struct OptionState {
    label: String,
    bin: Arc<Bin>,
}

struct PopupState {
    visible: bool,
    select_i: Option<usize>,
}

impl<I> Select<I>
where
    I: Ord + Copy + Send + 'static,
{
    /// Set the currently selected id.
    ///
    /// **Note**: This is a no-op if the id is not present.
    pub fn select(self: &Arc<Self>, option_id: I) {
        self.select_inner(Some(option_id));
    }

    /// Clear the selection.
    pub fn clear_selection(self: &Arc<Self>) {
        self.select_inner(None);
    }

    /// Add an option with the provided id and label.
    ///
    /// **Note**: Ids must be unique. Adding an option of the same id as a previously added id will
    ///           overwrite the existing option.
    pub fn add_option<L>(self: &Arc<Self>, option_id: I, label: L)
    where
        L: Into<String>,
    {
        let bin = self.container.window().unwrap().new_bin();
        let state = self.state.lock();

        {
            let mut options = state.options.borrow_mut();
            self.add_option_select_hook(option_id, &bin);

            options.insert(
                option_id,
                OptionState {
                    label: label.into(),
                    bin,
                },
            );
        }

        self.rebuild_list();
    }

    /// Same as [`add_option`](`Select::add_option`), but selects the newly added option after it has been added.
    pub fn add_option_selected<L>(self: &Arc<Self>, option_id: I, label: L)
    where
        L: Into<String>,
    {
        let _state = self.state.lock();
        self.add_option(option_id, label);
        self.select_inner(Some(option_id));
    }

    /// Remove an option with the provided id.
    ///
    /// **Notes**:
    /// - If the id is not present nothing will happen and `false` will be returned.
    /// - If the id is currently selected, the selection will be cleared.
    pub fn remove_option(self: &Arc<Self>, option_id: I) -> bool {
        let state = self.state.lock();
        let mut options = state.options.borrow_mut();
        let select = state.select.borrow();

        if options.remove(&option_id).is_some() {
            let clear_selection = match *select {
                Some(select_id) => {
                    if select_id == option_id {
                        true
                    } else {
                        false
                    }
                },
                None => false,
            };

            drop(select);
            drop(options);

            if clear_selection {
                self.clear_selection();
            }

            self.rebuild_list();
            true
        } else {
            false
        }
    }

    /// Add a callback to be called when the selection changed.
    ///
    /// **Note**: When changing the state within the callback, no callbacks on this [`Select`]
    /// will be called.
    ///
    /// **Panics**: When adding a callback within the callback to this [`Select`].
    pub fn on_select<F>(&self, on_select: F)
    where
        F: FnMut(&Arc<Select<I>>, Option<I>) + Send + 'static,
    {
        self.state
            .lock()
            .on_select
            .borrow_mut()
            .push(Box::new(on_select));
    }

    fn select_inner(self: &Arc<Self>, option_id_op: Option<I>) {
        let state = self.state.lock();

        let label = {
            let mut select = state.select.borrow_mut();
            let options = state.options.borrow();

            match option_id_op {
                Some(option_id) => {
                    match options.get(&option_id) {
                        Some(option_state) => {
                            if select.is_some() && select.unwrap() == option_id {
                                return;
                            }

                            *select = option_id_op;
                            option_state.label.clone()
                        },
                        None => {
                            if select.is_none() {
                                return;
                            }

                            *select = None;
                            self.props.no_selection_label.clone()
                        },
                    }
                },
                None => {
                    if select.is_none() {
                        return;
                    }

                    *select = None;
                    self.props.no_selection_label.clone()
                },
            }
        };

        self.container.style_modify(move |style| {
            style.text_body.spans[0].text = label.clone(); // TODO: Why Clone???
        });

        if let Ok(mut callbacks) = state.on_select.try_borrow_mut() {
            for callback in callbacks.iter_mut() {
                callback(self, option_id_op);
            }
        }
    }

    fn add_option_select_hook(self: &Arc<Self>, id: I, bin: &Arc<Bin>) {
        let cb_select = self.clone();

        bin.on_press(MouseButton::Left, move |_, _, _| {
            cb_select.select(id);
            Default::default()
        });
    }

    fn toggle_popup(&self) {
        let state = self.state.lock();

        if state.popup.borrow().visible {
            self.hide_popup();
        } else {
            self.show_popup();
        }
    }

    fn show_popup(&self) {
        let state = self.state.lock();
        let select = state.select.borrow();
        let options = state.options.borrow();
        let mut popup_state = state.popup.borrow_mut();

        let mut style_update_batch = Vec::new();
        let mut popup_style = self.popup.style_copy();
        popup_style.visibility = Visibility::Inheirt;
        style_update_batch.push((&self.popup, popup_style));

        if self.theme.roundness.is_some() {
            let mut container_style = self.container.style_copy();
            container_style.border_radius_bl = Default::default();
            container_style.border_radius_br = Default::default();
            style_update_batch.push((&self.container, container_style));
        }

        let index = match *select {
            Some(sel_id) => {
                match options.keys().enumerate().find(|(_, id)| **id == sel_id) {
                    Some(some) => Some(some.0),
                    None => None,
                }
            },
            None => None,
        };

        popup_state.select_i = index;
        popup_state.visible = true;

        for (i, option_state) in options.values().enumerate() {
            let [back_color, text_color] = if index.is_some() && i == index.unwrap() {
                [self.theme.colors.accent1, self.theme.colors.text1b]
            } else {
                [Default::default(), self.theme.colors.text1a]
            };

            if let Some(mut option_style) = option_state.bin.style_inspect(|style| {
                if style.back_color == back_color && style.text_body.base_attrs.color == text_color
                {
                    None
                } else {
                    Some(style.clone())
                }
            }) {
                option_style.back_color = back_color;
                option_style.text_body.base_attrs.color = text_color;
                style_update_batch.push((&option_state.bin, option_style));
            }
        }

        self.popup_jump_to(index.unwrap_or(0));
        Bin::style_update_batch(style_update_batch);
    }

    fn hide_popup(&self) {
        let state = self.state.lock();
        let mut popup_state = state.popup.borrow_mut();

        let mut style_update_batch = Vec::new();
        let mut popup_style = self.popup.style_copy();
        popup_style.visibility = Visibility::Hide;
        style_update_batch.push((&self.popup, popup_style));

        if let Some(border_radius) = self.theme.roundness {
            let mut container_style = self.container.style_copy();
            container_style.border_radius_bl = Pixels(border_radius);
            container_style.border_radius_br = Pixels(border_radius);
            style_update_batch.push((&self.container, container_style));
        }

        popup_state.visible = false;
        Bin::style_update_batch(style_update_batch);
    }

    fn popup_jump_to(&self, index: usize) {
        let jump_index = index
            .checked_sub(self.props.drop_down_items / 3)
            .unwrap_or(0);

        let jump_to = jump_index as f32
            * (self.theme.base_size + self.theme.spacing + self.theme.border.unwrap_or(0.0));

        self.scroll_bar.jump_to(jump_to);
    }

    fn popup_select_prev(&self) {
        let state = self.state.lock();

        let index = match state.popup.borrow().select_i {
            Some(select_i) => select_i.checked_sub(1).unwrap_or(0),
            None => 0,
        };

        self.popup_select(index);
    }

    fn popup_select_next(&self) {
        let state = self.state.lock();

        let index = match state.popup.borrow().select_i {
            Some(select_i) => select_i + 1,
            None => 0,
        };

        self.popup_select(index);
    }

    fn popup_select(&self, mut index: usize) {
        let state = self.state.lock();
        let options = state.options.borrow();
        let mut popup = state.popup.borrow_mut();
        index = index.min(options.len().checked_sub(1).unwrap_or(0));

        if popup.select_i.is_some() && index == popup.select_i.unwrap() {
            return;
        }

        let mut style_update_batch = Vec::new();

        for (i, option_state) in options.values().enumerate() {
            if popup.select_i.is_some() && i == popup.select_i.unwrap() {
                let mut option_style = option_state.bin.style_copy();
                option_style.back_color = Default::default();
                option_style.text_body.base_attrs.color = self.theme.colors.text1a;
                style_update_batch.push((&option_state.bin, option_style));
            } else if i == index {
                let mut option_style = option_state.bin.style_copy();
                option_style.back_color = self.theme.colors.accent1;
                option_style.text_body.base_attrs.color = self.theme.colors.text1b;
                style_update_batch.push((&option_state.bin, option_style));
            }

            if style_update_batch.len() == 2 {
                break;
            }
        }

        popup.select_i = Some(index);
        Bin::style_update_batch(style_update_batch);
        self.popup_jump_to(index);
    }

    fn popup_finish(self: &Arc<Self>, esc: bool) {
        let state = self.state.lock();
        self.hide_popup();

        if !esc {
            let option_id_op = match state.popup.borrow().select_i {
                Some(select_i) => state.options.borrow().keys().skip(select_i).next().copied(),
                None => None,
            };

            self.select_inner(option_id_op);
        }
    }

    fn rebuild_list(&self) {
        let state = self.state.lock();
        let options = state.options.borrow();
        let num_options = options.len();

        if !options.is_empty() {
            let mut styles = Vec::with_capacity(num_options);

            for (i, option_state) in options.values().enumerate() {
                let mut option_style = BinStyle {
                    pos_from_t: Pixels(
                        i as f32
                            * (self.theme.spacing
                                + self.theme.base_size
                                + self.theme.border.unwrap_or(0.0)),
                    ),
                    pos_from_l: Pixels(0.0),
                    pos_from_r: Pixels(0.0),
                    height: Pixels(self.theme.spacing + self.theme.base_size),
                    padding_l: Pixels(self.theme.spacing),
                    padding_r: Pixels(self.theme.spacing),
                    text_body: TextBody {
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
                        ..TextBody::from(option_state.label.clone())
                    },
                    ..Default::default()
                };

                if i != num_options - 1 {
                    if let Some(border_size) = self.theme.border {
                        option_style.border_size_b = Pixels(border_size);
                        option_style.border_color_b = self.theme.colors.border2;
                    }
                }

                styles.push(option_style);
            }

            Bin::style_update_batch(
                options
                    .values()
                    .map(|option_state| &option_state.bin)
                    .zip(styles),
            );
        }
    }

    fn style_update(&self) {
        let border_size = self.theme.border.unwrap_or(0.0);

        let mut container_style = BinStyle {
            padding_l: Pixels(self.theme.spacing),
            padding_r: PctOfHeight(100.0),
            back_color: self.theme.colors.back3,
            text_body: TextBody {
                spans: vec![Default::default()],
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
            ..self.props.placement.clone().into_style()
        };

        let arrow_down_style = BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_b: Pixels(0.0),
            pos_from_r: Pixels(0.0),
            width: PctOfHeight(100.0),
            user_vertexes: vec![(
                ImageKey::INVALID,
                down_symbol_verts(33.0, self.theme.colors.text1a),
            )],
            ..Default::default()
        };

        let mut popup_style = BinStyle {
            position: Position::Anchor,
            z_index: ZIndex::Offset(100),
            visibility: Visibility::Hide,
            pos_from_t: PctOffset(100.0, border_size),
            pos_from_l: Pixels(0.0),
            pos_from_r: Pixels(0.0),
            height: PctOffset(
                100.0 * self.props.drop_down_items as f32,
                border_size * self.props.drop_down_items.checked_sub(1).unwrap_or(0) as f32,
            ),
            back_color: self.theme.colors.back2,
            ..Default::default()
        };

        let option_list_style = BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_l: Pixels(0.0),
            pos_from_r: Pixels(ScrollBar::size(&self.theme)),
            pos_from_b: Pixels(0.0),
            ..Default::default()
        };

        container_style.text_body.spans[0].text = {
            let state = self.state.lock();
            let select = state.select.borrow();
            let options = state.options.borrow();

            match *select {
                Some(select_id) => {
                    match options.get(&select_id) {
                        Some(option_state) => option_state.label.clone(),
                        None => self.props.no_selection_label.clone(),
                    }
                },
                None => self.props.no_selection_label.clone(),
            }
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

            popup_style.border_size_b = Pixels(border_size);
            popup_style.border_size_l = Pixels(border_size);
            popup_style.border_size_r = Pixels(border_size);
            popup_style.border_color_b = self.theme.colors.border1;
            popup_style.border_color_l = self.theme.colors.border1;
            popup_style.border_color_r = self.theme.colors.border1;
        }

        if let Some(border_radius) = self.theme.roundness {
            container_style.border_radius_tl = Pixels(border_radius);
            container_style.border_radius_tr = Pixels(border_radius);
            container_style.border_radius_bl = Pixels(border_radius);
            container_style.border_radius_br = Pixels(border_radius);

            popup_style.border_radius_bl = Pixels(border_radius);
            popup_style.border_radius_br = Pixels(border_radius);
        }

        Bin::style_update_batch([
            (&self.container, container_style),
            (&self.arrow_down, arrow_down_style),
            (&self.popup, popup_style),
            (&self.option_list, option_list_style),
        ]);
    }
}

impl<I> Select<I> {
    /// Obtain the default [`WidgetPlacement`](`WidgetPlacement`) given a [`Theme`](`Theme`).
    pub fn default_placement(theme: &Theme) -> WidgetPlacement {
        let height = theme.spacing + theme.base_size;
        let width = height * 5.0;

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
}
