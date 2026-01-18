use std::sync::Arc;

use crate::input::InputHookCtrl;
use crate::interface::UnitValue::Pixels;
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{
    Container, Frame, ScrollAxis, ScrollBar, Theme, WidgetPlacement, text_hooks,
};
use crate::interface::{
    Bin, BinPostUpdate, BinStyle, Position, TextAttrs, TextBody, TextCursor, TextSpan,
};
use crate::ulps_eq;

/// Builder for [`TextEditor`]
pub struct TextEditorBuilder<'a, C> {
    widget: WidgetBuilder<'a, C>,
    props: Properties,
    text_body: TextBody,
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

impl<'a, C> TextEditorBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| TextEditor::default_placement(&builder.theme)),
            ),
            text_body: TextBody {
                base_attrs: TextAttrs {
                    color: builder.theme.colors.text1a,
                    height: Pixels(builder.theme.text_height),
                    font_family: builder.theme.font_family.clone(),
                    font_weight: builder.theme.font_weight,
                    ..Default::default()
                },
                spans: vec![TextSpan::default()],
                ..Default::default()
            },
            widget: builder,
        }
    }

    /// Set the initial text.
    pub fn with_text<T>(mut self, text: T) -> Self
    where
        T: Into<String>,
    {
        self.text_body.spans[0] = TextSpan::from(text.into());
        self
    }

    /// Set the [`TextAttrs`] used.
    pub fn with_attrs(mut self, attrs: TextAttrs) -> Self {
        self.text_body.base_attrs = attrs;
        self
    }

    /// Finish building the [`TextEditor`].
    pub fn build(self) -> Arc<TextEditor> {
        let container = self.widget.container.create_bin();

        let frame = container
            .create_widget()
            .with_placement(WidgetPlacement {
                pos_from_t: Pixels(0.0),
                pos_from_b: Pixels(0.0),
                pos_from_l: Pixels(0.0),
                pos_from_r: Pixels(0.0),
                ..Default::default()
            })
            .frame()
            .build();

        let text_editor = Arc::new(TextEditor {
            theme: self.widget.theme,
            props: self.props,
            container,
            frame,
        });

        let text_editor_wk1 = Arc::downgrade(&text_editor);
        let text_editor_wk2 = Arc::downgrade(&text_editor);

        text_hooks::create(
            text_hooks::Properties::EDITOR,
            text_editor.frame.view_area_bin().clone(),
            text_editor.theme.clone(),
            Some(Arc::new(move |updated| {
                let text_hooks::Updated {
                    cursor: _,
                    cursor_bounds,
                    body_line_count: _,
                    cursor_line_col: _,
                    editor_bpu,
                } = updated;

                if let Some(cursor_bounds) = cursor_bounds
                    && let Some(text_editor) = text_editor_wk1.upgrade()
                {
                    text_editor.check_cursor_in_view(editor_bpu, cursor_bounds);
                }
            })),
            Some(Arc::new(move |amt| {
                if let Some(text_editor) = text_editor_wk2.upgrade() {
                    text_editor.frame.v_scroll_bar().scroll(amt);
                }
            })),
        );

        let text_editor_wk = Arc::downgrade(&text_editor);

        text_editor.frame.view_area_bin().on_focus(move |_, _| {
            let text_editor = match text_editor_wk.upgrade() {
                Some(some) => some,
                None => return InputHookCtrl::Remove,
            };

            let theme = &text_editor.theme;

            if theme.border.is_some() {
                text_editor.container.style_modify(|style| {
                    style.border_color_t = theme.colors.accent2;
                    style.border_color_b = theme.colors.accent2;
                    style.border_color_l = theme.colors.accent2;
                    style.border_color_r = theme.colors.accent2;
                });
            }

            Default::default()
        });

        let text_editor_wk = Arc::downgrade(&text_editor);

        text_editor
            .frame
            .view_area_bin()
            .on_focus_lost(move |_, _| {
                let text_editor = match text_editor_wk.upgrade() {
                    Some(some) => some,
                    None => return InputHookCtrl::Remove,
                };

                let theme = &text_editor.theme;

                if theme.border.is_some() {
                    text_editor.container.style_modify(|style| {
                        style.border_color_t = theme.colors.border1;
                        style.border_color_b = theme.colors.border1;
                        style.border_color_l = theme.colors.border1;
                        style.border_color_r = theme.colors.border1;
                    });
                }

                Default::default()
            });

        text_editor.style_update(Some(self.text_body));
        text_editor
    }
}

/// Text editor widget.
pub struct TextEditor {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    frame: Arc<Frame>,
}

impl TextEditor {
    /// Obtain the value as a [`String`](String).
    pub fn value(&self) -> String {
        let text_body = self.frame.view_area_bin().text_body();

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
        self.frame.view_area_bin().style_modify(|style| {
            style.text_body.spans = vec![TextSpan::from(value.into())];
            style.text_body.cursor = TextCursor::None;
            style.text_body.selection = None;
        });
    }

    /// Obtain the default [`WidgetPlacement`](`WidgetPlacement`) given a [`Theme`](`Theme`).
    pub fn default_placement(theme: &Theme) -> WidgetPlacement {
        let height = theme.spacing + (theme.base_size * 9.0);
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

    fn check_cursor_in_view(&self, editor_bpu: &BinPostUpdate, mut cursor_bounds: [f32; 4]) {
        let view_bounds = editor_bpu.optimal_content_bounds;

        let target_scroll = [
            self.frame.h_scroll_bar().target_scroll(),
            self.frame.v_scroll_bar().target_scroll(),
        ];

        if !ulps_eq(-editor_bpu.content_offset[0], target_scroll[0], 4) {
            cursor_bounds[0] -= editor_bpu.content_offset[0];
            cursor_bounds[1] -= editor_bpu.content_offset[0];
            cursor_bounds[0] -= target_scroll[0];
            cursor_bounds[1] -= target_scroll[0];
        }

        if !ulps_eq(-editor_bpu.content_offset[1], target_scroll[1], 4) {
            cursor_bounds[2] -= editor_bpu.content_offset[1];
            cursor_bounds[3] -= editor_bpu.content_offset[1];
            cursor_bounds[2] -= target_scroll[1];
            cursor_bounds[3] -= target_scroll[1];
        }

        if cursor_bounds[0] < view_bounds[0] {
            self.frame
                .h_scroll_bar()
                .scroll_to(cursor_bounds[0] + target_scroll[0] - view_bounds[0]);
        } else if cursor_bounds[1] > view_bounds[1] {
            self.frame
                .h_scroll_bar()
                .scroll_to(cursor_bounds[1] + target_scroll[0] - view_bounds[1]);
        }

        if cursor_bounds[2] < view_bounds[2] {
            self.frame
                .v_scroll_bar()
                .scroll_to(cursor_bounds[2] + target_scroll[1] - view_bounds[2]);
        } else if cursor_bounds[3] > view_bounds[3] {
            self.frame
                .v_scroll_bar()
                .scroll_to(cursor_bounds[3] + target_scroll[1] - view_bounds[3]);
        }
    }

    fn style_update(&self, text_body_op: Option<TextBody>) {
        let mut container_style = self.props.placement.clone().into_style();
        container_style.back_color = self.theme.colors.back2;
        let mut editor_style = BinStyle::default();

        if let Some(text_body) = text_body_op {
            editor_style.text_body = text_body;
        }

        editor_style.position = Position::Relative;
        editor_style.pos_from_t = Pixels(0.0);
        editor_style.pos_from_b = ScrollBar::default_placement(&self.theme, ScrollAxis::X).height;
        editor_style.pos_from_l = Pixels(0.0);
        editor_style.pos_from_r = ScrollBar::default_placement(&self.theme, ScrollAxis::Y).width;
        editor_style.back_color = self.theme.colors.back2;
        editor_style.padding_t = Pixels(self.theme.spacing);
        editor_style.padding_b = Pixels(self.theme.spacing);
        editor_style.padding_l = Pixels(self.theme.spacing);
        editor_style.padding_r = Pixels(self.theme.spacing);

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
            editor_style.border_radius_tl = Pixels(border_radius);
        }

        Bin::style_update_batch([
            (&self.container, container_style),
            (&self.frame.view_area_bin(), editor_style),
        ]);
    }
}
