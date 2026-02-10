use std::cell::RefCell;
use std::sync::Arc;

use parking_lot::ReentrantMutex;

use crate::input::InputHookCtrl;
use crate::interface::UnitValue::Pixels;
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Container, Theme, WidgetPlacement, text_hooks};
use crate::interface::{
    Bin, BinPostUpdate, BinStyle, Position, TextAttrs, TextBody, TextCursor, TextHoriAlign,
    TextSpan, TextVertAlign, TextWrap,
};

/// Builder for [`TextEntry`]
pub struct TextEntryBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    text: String,
    on_change: Vec<Box<dyn FnMut(&Arc<TextEntry>, &String) + Send + 'static>>,
    filter_op: Option<Box<dyn FnMut(&Arc<TextEntry>, &String, char) -> bool + Send + 'static>>,
}

#[derive(Default)]
struct Properties {
    placement: WidgetPlacement,
}

impl Properties {
    fn new(placement: WidgetPlacement) -> Self {
        Self {
            placement,
        }
    }
}

impl<'a, C> TextEntryBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| TextEntry::default_placement(&builder.theme)),
            ),
            widget: builder,
            text: String::new(),
            on_change: Vec::new(),
            filter_op: None,
        }
    }

    /// Set the inital text.
    pub fn with_text<T>(mut self, text: T) -> Self
    where
        T: Into<String>,
    {
        self.text = text.into();
        self
    }

    /// Add a callback to be called when the [`TextEntry`]'s state changed.
    ///
    /// **Note**: When changing the state within the callback, no callbacks will be called with
    /// the updated state.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: FnMut(&Arc<TextEntry>, &String) + Send + 'static,
    {
        self.on_change.push(Box::new(on_change));
        self
    }

    /// Set the callback that is used to filter `char`'s.
    ///
    /// Returning `true` will allow the `char`, and `false` will filter out the `char`.
    ///
    /// **Note**: Only one callback is allowed. Calling this method again will overwrite the
    ///           previously set filter.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: FnMut(&Arc<TextEntry>, &String, char) -> bool + Send + 'static,
    {
        self.filter_op = Some(Box::new(filter));
        self
    }

    /// Finish building the [`TextEntry`].
    pub fn build(self) -> Arc<TextEntry> {
        let entry = self.widget.container.create_bin();

        let text_entry = Arc::new(TextEntry {
            theme: self.widget.theme,
            props: self.props,
            entry,
            state: ReentrantMutex::new(State {
                on_change: RefCell::new(self.on_change),
                filter_op: RefCell::new(self.filter_op),
            }),
        });

        let text_entry_wk1 = Arc::downgrade(&text_entry);
        let text_entry_wk2 = Arc::downgrade(&text_entry);

        text_hooks::create(
            text_hooks::Properties::ENTRY,
            text_entry.entry.clone(),
            text_entry.theme.clone(),
            Some(Arc::new(move |updated| {
                let text_hooks::Updated {
                    cursor: _,
                    cursor_bounds,
                    body_line_count: _,
                    cursor_line_col: _,
                    editor_bpu,
                } = updated;

                if let Some(text_entry) = text_entry_wk1.upgrade() {
                    if let Some(cursor_bounds) = cursor_bounds {
                        text_entry.check_cursor_in_view(editor_bpu, cursor_bounds);
                    }

                    let state = text_entry.state.lock();

                    if let Ok(mut on_change_cbs) = state.on_change.try_borrow_mut()
                        && !on_change_cbs.is_empty()
                    {
                        let cur_value = text_entry.entry.style_inspect(|style| {
                            style
                                .text_body
                                .spans
                                .get(0)
                                .map(|span| span.text.clone())
                                .unwrap_or(String::new())
                        });

                        for on_change in on_change_cbs.iter_mut() {
                            (*on_change)(&text_entry, &cur_value);
                        }
                    }
                }
            })),
            None,
            Some(Arc::new(move |text_body, c| {
                let text_entry = match text_entry_wk2.upgrade() {
                    Some(some) => some,
                    None => return true,
                };

                let state = text_entry.state.lock();
                let mut filter_op = state.filter_op.borrow_mut();

                let filter = match filter_op.as_mut() {
                    Some(some) => some,
                    None => return true,
                };

                text_body.style_inspect(|style| {
                    if style.text_body.spans.is_empty() {
                        (*filter)(&text_entry, &String::new(), c)
                    } else {
                        (*filter)(&text_entry, &style.text_body.spans[0].text, c)
                    }
                })
            })),
        );

        let text_entry_wk = Arc::downgrade(&text_entry);

        text_entry.entry.on_focus(move |_, _| {
            let text_entry = match text_entry_wk.upgrade() {
                Some(some) => some,
                None => return InputHookCtrl::Remove,
            };

            let theme = &text_entry.theme;

            if theme.border.is_some() {
                text_entry.entry.style_modify(|style| {
                    style.border_color_t = theme.colors.accent2;
                    style.border_color_b = theme.colors.accent2;
                    style.border_color_l = theme.colors.accent2;
                    style.border_color_r = theme.colors.accent2;
                });
            }

            Default::default()
        });

        let text_entry_wk = Arc::downgrade(&text_entry);

        text_entry.entry.on_focus_lost(move |_, _| {
            let text_entry = match text_entry_wk.upgrade() {
                Some(some) => some,
                None => return InputHookCtrl::Remove,
            };

            let theme = &text_entry.theme;

            text_entry.entry.style_modify(|style| {
                if theme.border.is_some() {
                    style.border_color_t = theme.colors.border1;
                    style.border_color_b = theme.colors.border1;
                    style.border_color_l = theme.colors.border1;
                    style.border_color_r = theme.colors.border1;
                }

                style.scroll_x = 0.0;
            });

            Default::default()
        });

        text_entry.style_update(self.text);
        text_entry
    }
}

/// Text entry widget.
pub struct TextEntry {
    theme: Theme,
    props: Properties,
    entry: Arc<Bin>,
    state: ReentrantMutex<State>,
}

struct State {
    on_change: RefCell<Vec<Box<dyn FnMut(&Arc<TextEntry>, &String) + Send + 'static>>>,
    filter_op:
        RefCell<Option<Box<dyn FnMut(&Arc<TextEntry>, &String, char) -> bool + Send + 'static>>>,
}

impl TextEntry {
    /// Obtain the value as a [`String`](String).
    pub fn value(&self) -> String {
        let text_body = self.entry.text_body();

        match text_body.select_all() {
            Some(selection) => text_body.selection_string(selection),
            None => String::new(),
        }
    }

    /// Set the value.
    pub fn set_value<V>(&self, value: V)
    where
        V: Into<String>,
    {
        self.entry.style_modify(|style| {
            style.text_body.spans = vec![TextSpan::from(value.into())];
            style.text_body.cursor = TextCursor::None;
            style.text_body.selection = None;
        });
    }

    /// Add a callback to be called when the [`TextEntry`]'s state changed.
    ///
    /// **Note**: When changing the state within the callback, no callbacks will be called with
    /// the updated state.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn on_change<F>(&self, on_change: F)
    where
        F: FnMut(&Arc<TextEntry>, &String) + Send + 'static,
    {
        self.state
            .lock()
            .on_change
            .borrow_mut()
            .push(Box::new(on_change));
    }

    /// Set the callback that is used to filter `char`'s.
    ///
    /// Returning `true` will allow the `char`, and `false` will filter out the `char`.
    ///
    /// **Note**: Only one callback is allowed. Calling this method again will overwrite the
    ///           previously set filter.
    ///
    /// **Panics**: When adding a callback within the callback.
    pub fn set_filter<F>(&self, filter: F)
    where
        F: FnMut(&Arc<TextEntry>, &String, char) -> bool + Send + 'static,
    {
        *self.state.lock().filter_op.borrow_mut() = Some(Box::new(filter));
    }

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

    fn style_update(&self, text: String) {
        let mut container_style = self.props.placement.clone().into_style();
        container_style.back_color = self.theme.colors.back2;

        let mut entry_style = BinStyle {
            back_color: self.theme.colors.back2,
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
                cursor_color: self.theme.colors.cursor,
                selection_color: self.theme.colors.selection,
                ..TextBody::from(text)
            },
            ..self.props.placement.clone().into_style()
        };

        if let Some(border_size) = self.theme.border {
            entry_style.border_size_t = Pixels(border_size);
            entry_style.border_size_b = Pixels(border_size);
            entry_style.border_size_l = Pixels(border_size);
            entry_style.border_size_r = Pixels(border_size);
            entry_style.border_color_t = self.theme.colors.border1;
            entry_style.border_color_b = self.theme.colors.border1;
            entry_style.border_color_l = self.theme.colors.border1;
            entry_style.border_color_r = self.theme.colors.border1;
        }

        if let Some(border_radius) = self.theme.roundness {
            entry_style.border_radius_tl = Pixels(border_radius);
            entry_style.border_radius_tr = Pixels(border_radius);
            entry_style.border_radius_bl = Pixels(border_radius);
            entry_style.border_radius_br = Pixels(border_radius);
        }

        self.entry.style_update(entry_style).expect_valid();
    }
}
