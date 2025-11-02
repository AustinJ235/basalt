use std::sync::Arc;

use crate::input::InputHookCtrl;
use crate::interface::UnitValue::Pixels;
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{Theme, Container, WidgetPlacement, text_hooks};
use crate::interface::{
    Bin, BinPostUpdate, BinStyle, Position, TextAttrs, TextBody, TextCursor, TextHoriAlign,
    TextSpan, TextVertAlign, TextWrap,
};

/// Builder for [`TextEntry`]
pub struct TextEntryBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    text: String,
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

    /// Finish building the [`TextEntry`].
    pub fn build(self) -> Arc<TextEntry> {
        let entry = self.widget.container.create_bin();

        let text_entry = Arc::new(TextEntry {
            theme: self.widget.theme,
            props: self.props,
            entry,
        });

        let text_entry_wk = Arc::downgrade(&text_entry);

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

                if let Some(cursor_bounds) = cursor_bounds
                    && let Some(text_entry) = text_entry_wk.upgrade()
                {
                    text_entry.check_cursor_in_view(editor_bpu, cursor_bounds);
                }
            })),
            None,
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
