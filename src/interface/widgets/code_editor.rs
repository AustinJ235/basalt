use std::sync::Arc;

use crate::input::InputHookCtrl;
use crate::interface::UnitValue::Pixels;
use crate::interface::widgets::builder::WidgetBuilder;
use crate::interface::widgets::{
    ScrollAxis, ScrollBar, Theme, Container, WidgetPlacement, text_hooks,
};
use crate::interface::{
    Bin, BinPostUpdate, BinStyle, FontFamily, Position, TextAttrs, TextBody, TextCursor,
    TextHoriAlign, TextSpan, TextWrap,
};
use crate::ulps_eq;

/// Builder for [`CodeEditor`]
pub struct CodeEditorBuilder<'a, C> {
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

impl<'a, C> CodeEditorBuilder<'a, C>
where
    C: Container,
{
    pub(crate) fn with_builder(mut builder: WidgetBuilder<'a, C>) -> Self {
        Self {
            props: Properties::new(
                builder
                    .placement
                    .take()
                    .unwrap_or_else(|| CodeEditor::default_placement(&builder.theme)),
            ),
            text_body: TextBody {
                base_attrs: TextAttrs {
                    color: builder.theme.colors.text1a,
                    height: Pixels(builder.theme.text_height),
                    font_family: FontFamily::Monospace,
                    ..Default::default()
                },
                text_wrap: TextWrap::None,
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

    /// Finish building the [`CodeEditor`].
    pub fn build(self) -> Arc<CodeEditor> {
        let container = self.widget.container.create_bin();
        let mut bins = container.create_bins(3);
        let editor = bins.next().unwrap();
        let status_bar = bins.next().unwrap();
        let line_numbers = bins.next().unwrap();
        drop(bins);

        let sb_size = match ScrollBar::default_placement(&self.widget.theme, ScrollAxis::Y).width {
            Pixels(px) => px,
            _ => unreachable!(),
        };

        let border_size = self.widget.theme.border.unwrap_or(0.0);
        let status_bar_h = self.widget.theme.base_size + self.widget.theme.spacing;
        let line_numbers_w = self.widget.theme.base_size * 2.0;

        let v_scroll_b = container
            .create_widget()
            .with_theme(self.widget.theme.clone())
            .with_placement(WidgetPlacement {
                pos_from_b: Pixels(sb_size + (border_size * 2.0) + status_bar_h),
                ..ScrollBar::default_placement(&self.widget.theme, ScrollAxis::Y)
            })
            .scroll_bar(editor.clone())
            .build();

        let h_scroll_b = container
            .create_widget()
            .with_theme(self.widget.theme.clone())
            .with_placement(WidgetPlacement {
                pos_from_l: Pixels(line_numbers_w + border_size),
                pos_from_r: Pixels(sb_size + border_size),
                pos_from_b: Pixels(status_bar_h + border_size),
                ..ScrollBar::default_placement(&self.widget.theme, ScrollAxis::X)
            })
            .scroll_bar(editor.clone())
            .axis(ScrollAxis::X)
            .build();

        let code_editor = Arc::new(CodeEditor {
            theme: self.widget.theme,
            props: self.props,
            container,
            editor,
            status_bar,
            line_numbers,
            v_scroll_b,
            h_scroll_b,
        });

        let code_editor_wk1 = Arc::downgrade(&code_editor);
        let code_editor_wk2 = Arc::downgrade(&code_editor);

        text_hooks::create(
            text_hooks::Properties::CODE_EDITOR,
            code_editor.editor.clone(),
            code_editor.theme.clone(),
            Some(Arc::new(move |updated| {
                let text_hooks::Updated {
                    cursor: _,
                    cursor_bounds,
                    body_line_count,
                    cursor_line_col,
                    editor_bpu,
                } = updated;

                let code_editor = match code_editor_wk1.upgrade() {
                    Some(some) => some,
                    None => return,
                };

                if let Some(cursor_bounds) = cursor_bounds {
                    code_editor.check_cursor_in_view(editor_bpu, cursor_bounds);
                }

                let status = match cursor_line_col {
                    Some([line_i, col_i]) => format!("Ln: {}, Col: {}", line_i + 1, col_i + 1),
                    None => String::from("Cursor: None"),
                };

                code_editor.status_bar.style_modify(|style| {
                    style.text_body.spans[0].text = status;
                });

                code_editor.line_numbers.style_modify(|style| {
                    let mut text = String::new();

                    for i in 0..body_line_count {
                        if i == body_line_count - 1 {
                            text.push_str(format!("{}", i + 1).as_str());
                        } else {
                            text.push_str(format!("{}\n", i + 1).as_str());
                        }
                    }

                    style.text_body.spans[0].text = text;
                });
            })),
            Some(Arc::new(move |amt| {
                if let Some(code_editor) = code_editor_wk2.upgrade() {
                    code_editor.v_scroll_b.scroll(amt);
                }
            })),
        );

        let code_editor_wk = Arc::downgrade(&code_editor);

        code_editor.editor.on_update(move |_, editor_bpu| {
            let code_editor = match code_editor_wk.upgrade() {
                Some(some) => some,
                None => return,
            };

            let scroll_y = -editor_bpu.content_offset[1];

            if let Some(mut style) = code_editor.line_numbers.style_inspect(|style| {
                if style.scroll_y != scroll_y {
                    Some(style.clone())
                } else {
                    None
                }
            }) {
                style.scroll_y = scroll_y;
                code_editor.line_numbers.style_update(style).expect_valid();
            }
        });

        let code_editor_wk = Arc::downgrade(&code_editor);

        code_editor.editor.on_focus(move |_, _| {
            let code_editor = match code_editor_wk.upgrade() {
                Some(some) => some,
                None => return InputHookCtrl::Remove,
            };

            let theme = &code_editor.theme;

            if theme.border.is_some() {
                code_editor.container.style_modify(|style| {
                    style.border_color_t = theme.colors.accent2;
                    style.border_color_b = theme.colors.accent2;
                    style.border_color_l = theme.colors.accent2;
                    style.border_color_r = theme.colors.accent2;
                });
            }

            Default::default()
        });

        let code_editor_wk = Arc::downgrade(&code_editor);

        code_editor.editor.on_focus_lost(move |_, _| {
            let code_editor = match code_editor_wk.upgrade() {
                Some(some) => some,
                None => return InputHookCtrl::Remove,
            };

            let theme = &code_editor.theme;

            if theme.border.is_some() {
                code_editor.container.style_modify(|style| {
                    style.border_color_t = theme.colors.border1;
                    style.border_color_b = theme.colors.border1;
                    style.border_color_l = theme.colors.border1;
                    style.border_color_r = theme.colors.border1;
                });

                code_editor.status_bar.style_modify(|style| {
                    style.text_body.spans[0].text = format!("Cursor: None");
                });
            }

            Default::default()
        });

        code_editor.style_update(Some(self.text_body));
        code_editor
    }
}

/// Text editor widget.
pub struct CodeEditor {
    theme: Theme,
    props: Properties,
    container: Arc<Bin>,
    editor: Arc<Bin>,
    status_bar: Arc<Bin>,
    line_numbers: Arc<Bin>,
    v_scroll_b: Arc<ScrollBar>,
    h_scroll_b: Arc<ScrollBar>,
}

impl CodeEditor {
    /// Obtain the value as a [`String`](String).
    pub fn value(&self) -> String {
        let text_body = self.editor.text_body();

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
        self.editor.style_modify(|style| {
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
            self.h_scroll_b.target_scroll(),
            self.v_scroll_b.target_scroll(),
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
            self.h_scroll_b
                .scroll_to(cursor_bounds[0] + target_scroll[0] - view_bounds[0]);
        } else if cursor_bounds[1] > view_bounds[1] {
            self.h_scroll_b
                .scroll_to(cursor_bounds[1] + target_scroll[0] - view_bounds[1]);
        }

        if cursor_bounds[2] < view_bounds[2] {
            self.v_scroll_b
                .scroll_to(cursor_bounds[2] + target_scroll[1] - view_bounds[2]);
        } else if cursor_bounds[3] > view_bounds[3] {
            self.v_scroll_b
                .scroll_to(cursor_bounds[3] + target_scroll[1] - view_bounds[3]);
        }
    }

    fn style_update(&self, text_body_op: Option<TextBody>) {
        let mut container_style = self.props.placement.clone().into_style();
        container_style.back_color = self.theme.colors.back3;
        let mut editor_style = BinStyle::default();

        if let Some(text_body) = text_body_op {
            editor_style.text_body = text_body;
        }

        let status_bar_h = self.theme.base_size + self.theme.spacing;
        let line_numbers_w = self.theme.base_size * 2.0;
        let border_size = self.theme.border.unwrap_or(0.0);

        editor_style.position = Position::Relative;
        editor_style.pos_from_t = Pixels(0.0);
        editor_style.pos_from_b = ScrollBar::default_placement(&self.theme, ScrollAxis::X)
            .height
            .offset_pixels(self.theme.base_size + self.theme.spacing + (border_size * 2.0));
        editor_style.pos_from_l = Pixels(line_numbers_w + border_size);
        editor_style.pos_from_r = ScrollBar::default_placement(&self.theme, ScrollAxis::Y).width;
        editor_style.back_color = self.theme.colors.back2;
        editor_style.padding_t = Pixels(self.theme.spacing);
        editor_style.padding_b = Pixels(self.theme.spacing);
        editor_style.padding_l = Pixels(self.theme.spacing);
        editor_style.padding_r = Pixels(self.theme.spacing);

        let mut status_bar_style = BinStyle {
            pos_from_b: Pixels(0.0),
            pos_from_l: Pixels(0.0),
            pos_from_r: Pixels(0.0),
            height: Pixels(status_bar_h),
            padding_t: Pixels(self.theme.spacing / 2.0),
            padding_b: Pixels(self.theme.spacing / 2.0),
            padding_l: Pixels(self.theme.spacing),
            padding_r: Pixels(self.theme.spacing),
            back_color: self.theme.colors.back2,
            text_body: TextBody {
                base_attrs: TextAttrs {
                    color: self.theme.colors.text1a,
                    height: Pixels(self.theme.text_height),
                    font_family: FontFamily::Monospace,
                    ..Default::default()
                },
                hori_align: TextHoriAlign::Right,
                ..TextBody::from("Cursor: None")
            },
            ..Default::default()
        };

        let num_lines = editor_style
            .text_body
            .spans
            .iter()
            .map(|span| {
                span.text
                    .chars()
                    .map(|c| (c == '\n') as usize)
                    .sum::<usize>()
            })
            .sum::<usize>()
            + 1;

        let mut line_numbers = String::new();

        for i in 0..num_lines {
            line_numbers.push_str(format!("{}\n", i + 1).as_str());
        }

        let mut line_numbers_style = BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_b: ScrollBar::default_placement(&self.theme, ScrollAxis::X)
                .height
                .offset_pixels(self.theme.base_size + self.theme.spacing + (border_size * 2.0)),
            pos_from_l: Pixels(0.0),
            width: Pixels(line_numbers_w),
            back_color: self.theme.colors.back2,
            padding_t: Pixels(self.theme.spacing),
            padding_b: Pixels(self.theme.spacing),
            padding_l: Pixels(self.theme.spacing / 2.0),
            padding_r: Pixels(self.theme.spacing / 2.0),
            text_body: TextBody {
                base_attrs: TextAttrs {
                    color: self.theme.colors.text1a,
                    height: Pixels(self.theme.text_height),
                    font_family: FontFamily::Monospace,
                    ..Default::default()
                },
                hori_align: TextHoriAlign::Right,
                text_wrap: TextWrap::None,
                ..TextBody::from(line_numbers)
            },
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

            editor_style.border_size_l = Pixels(border_size);
            editor_style.border_color_l = self.theme.colors.border2;
            editor_style.border_size_b = Pixels(border_size);
            editor_style.border_color_b = self.theme.colors.border2;

            line_numbers_style.border_size_b = Pixels(border_size);
            line_numbers_style.border_color_b = self.theme.colors.border2;

            status_bar_style.border_size_t = Pixels(border_size);
            status_bar_style.border_color_t = self.theme.colors.border2;
        }

        if let Some(border_radius) = self.theme.roundness {
            container_style.border_radius_tl = Pixels(border_radius);
            container_style.border_radius_tr = Pixels(border_radius);
            container_style.border_radius_bl = Pixels(border_radius);
            container_style.border_radius_br = Pixels(border_radius);

            status_bar_style.border_radius_bl = Pixels(border_radius);
            status_bar_style.border_radius_br = Pixels(border_radius);

            line_numbers_style.border_radius_tl = Pixels(border_radius);
        }

        Bin::style_update_batch([
            (&self.container, container_style),
            (&self.editor, editor_style),
            (&self.status_bar, status_bar_style),
            (&self.line_numbers, line_numbers_style),
        ]);
    }
}
