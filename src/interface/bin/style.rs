use std::sync::Arc;

mod vko {
    pub use vulkano::format::FormatFeatures;
    pub use vulkano::image::ImageType;
}

use crate::NonExhaustive;
use crate::image::ImageKey;
use crate::interface::{
    Bin, Color, Flow, FontFamily, FontStretch, FontStyle, FontWeight, PosTextCursor, Position,
    TextCursor, TextCursorAffinity, TextSelection, UnitValue,
};

/// Z-Index behavior
///
/// **Default**: [`Auto`](`ZIndex::Auto`)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZIndex {
    /// Z-index will be determinted automatically.
    #[default]
    Auto,
    /// Z-index will be set to a specific value.
    Fixed(i16),
    /// Z-index will be offset from the automatic value.
    Offset(i16),
}

/// How visiblity is determined.
///
/// **Default**: [`Inheirt`](`Visibility::Inheirt`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// Inheirt visibility of the parent.
    ///
    /// **Note**: If there is no parent this will be [`Show`][`Visibility::Show`].
    #[default]
    Inheirt,
    /// Set the visibility to hidden.
    ///
    /// **Note**: This ignores the parent's visibility.
    Hide,
    /// Set the visibility to shown.
    ///
    /// **Note**: This ignores the parent's visibility.
    Show,
}

/// How opacity is determinted.
///
/// Opacity is a value between `0.0..=1.0`.
///
/// **Default**: [`Inheirt`](`Opacity::Inheirt`)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Opacity {
    /// Inheirt the opacity of the parent.
    ///
    /// **Note**: If there is no parent this will be [`Fixed(1.0)`][`Opacity::Fixed`].
    #[default]
    Inheirt,
    /// Set the opacity to a fixed value.
    ///
    /// **Note**: This ignores the parent's opacity.
    Fixed(f32),
    /// Multiply the parent's opacity by the provided value.
    Multiply(f32),
}

/// Determintes order of floating targets.
///
/// **Default**: [`Auto`](`FloatWeight::Auto`)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FloatWeight {
    /// Float weight will be determinted by creation order.
    #[default]
    Auto,
    /// Float weight will be fixed.
    Fixed(i16),
}

/// Set the region of the background image to use.
///
/// **Default Behavior**: If the fields are left [`Undefined`](`UnitValue::Undefined`) the whole
/// extent of the provided image will be used.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BackImageRegion {
    pub offset: [UnitValue; 2],
    pub extent: [UnitValue; 2],
}

/// Effect used on the background image of a `Bin`.
///
/// **Default**: [`None`](`ImageEffect::None`)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageEffect {
    #[default]
    None,
    BackColorAdd,
    BackColorBehind,
    BackColorSubtract,
    BackColorMultiply,
    BackColorDivide,
    GlyphWithColor,
    Invert,
}

impl ImageEffect {
    pub(crate) fn vert_type(&self) -> i32 {
        match *self {
            ImageEffect::None => 100,
            ImageEffect::BackColorAdd => 102,
            ImageEffect::BackColorBehind => 103,
            ImageEffect::BackColorSubtract => 104,
            ImageEffect::BackColorMultiply => 105,
            ImageEffect::BackColorDivide => 106,
            ImageEffect::Invert => 107,
            ImageEffect::GlyphWithColor => 108,
        }
    }
}

/// Text wrap method used
///
/// **Default**: [`Normal`](`TextWrap::Normal`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextWrap {
    /// When the line overflows the line will shifted to the left.
    Shift,
    #[default]
    /// When the line overflows text will wrap.
    Normal,
    /// The line is allowed to overflow.
    None,
}

/// Text horizonal alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextHoriAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Text vertical alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextVertAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

/// How lines are spaced.
///
/// **Default**: [`HeightMult(1.2)`](`LineSpacing::HeightMult`)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineSpacing {
    /// Multiply the line height by the provided value.
    ///
    /// **Note**: This should generally be greater than `1.0`.
    HeightMult(f32),
    /// Multiply the line height by the provided and add the provided amount of pixels.
    ///
    /// **Note**: The multiplier (first value) should be greater than `1.0` and the added pixels
    /// (second value) should be greater than or equal to `0.0`.
    HeightMultAdd(f32, f32),
}

impl Default for LineSpacing {
    fn default() -> Self {
        Self::HeightMult(1.2)
    }
}

/// How many lines the [`TextBody`] should be limited to.
///
/// **Default**: [`None`](`LineLimit::None`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineLimit {
    /// No line limit.
    #[default]
    None,
    /// Limit the amount of lines to a fixed value.
    Fixed(usize),
}

/// The text body of a `Bin`.
///
/// Each [`BinStyle`](`BinStyle`) has a single `TextBody`. It can contain multiple
/// [`TextSpan`](`TextSpan`).
///
/// The default values for `base_attrs` will inheirt those set with
/// [`Interface::set_default_font`](`crate::interface::Interface::set_default_font`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextBody {
    pub spans: Vec<TextSpan>,
    pub line_spacing: LineSpacing,
    pub line_limit: LineLimit,
    pub text_wrap: TextWrap,
    pub vert_align: TextVertAlign,
    pub hori_align: TextHoriAlign,
    pub base_attrs: TextAttrs,
    pub cursor: TextCursor,
    pub cursor_color: Color,
    pub selection: Option<TextSelection>,
    pub selection_color: Color,
    pub _ne: NonExhaustive,
}

impl Default for TextBody {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            line_spacing: Default::default(),
            line_limit: Default::default(),
            text_wrap: Default::default(),
            vert_align: Default::default(),
            hori_align: Default::default(),
            base_attrs: TextAttrs::default(),
            cursor: Default::default(),
            cursor_color: Color::black(),
            selection: None,
            selection_color: Color::shex("4040ffc0"),
            _ne: NonExhaustive(()),
        }
    }
}

impl<T> From<T> for TextBody
where
    T: Into<String>,
{
    fn from(from: T) -> Self {
        Self {
            spans: vec![TextSpan::from(from)],
            ..Default::default()
        }
    }
}

impl TextBody {
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty() || self.spans.iter().all(|span| span.is_empty())
    }

    pub fn is_valid_cursor(&self, cursor: TextCursor) -> bool {
        let cursor = match cursor {
            TextCursor::Position(cursor) => cursor,
            _ => return true,
        };

        if cursor.span >= self.spans.len()
            || cursor.byte_s >= self.spans[cursor.span].text.len()
            || cursor.byte_e > self.spans[cursor.span].text.len()
            || cursor.byte_e <= cursor.byte_s
        {
            return false;
        }

        if !self.spans[cursor.span].text.is_char_boundary(cursor.byte_s) {
            return false;
        }

        for (byte_i, c) in self.spans[cursor.span].text.char_indices() {
            if byte_i == cursor.byte_s {
                if c.len_utf8() != cursor.byte_e - cursor.byte_s {
                    return false;
                }

                break;
            }
        }

        true
    }

    pub fn is_selection_valid(&self, selection: TextSelection) -> bool {
        self.is_valid_cursor(selection.start.into()) && self.is_valid_cursor(selection.end.into())
    }

    pub fn cursor_next(&self, cursor: TextCursor) -> TextCursor {
        let cursor = match cursor {
            TextCursor::None => return TextCursor::None,
            TextCursor::Empty => {
                for span_i in 0..self.spans.len() {
                    for (byte_i, c) in self.spans[span_i].text.char_indices() {
                        return TextCursor::Position(PosTextCursor {
                            span: span_i,
                            byte_s: byte_i,
                            byte_e: byte_i + c.len_utf8(),
                            affinity: TextCursorAffinity::After,
                        });
                    }
                }

                return TextCursor::Empty;
            },
            TextCursor::Position(cursor) => cursor,
        };

        if cursor.affinity == TextCursorAffinity::Before {
            if !self.is_valid_cursor(cursor.into()) {
                return TextCursor::None;
            }

            return TextCursor::Position(PosTextCursor {
                affinity: TextCursorAffinity::After,
                ..cursor
            });
        }

        if cursor.span >= self.spans.len()
            || cursor.byte_s >= self.spans[cursor.span].text.len()
            || cursor.byte_e > self.spans[cursor.span].text.len()
            || cursor.byte_e <= cursor.byte_s
        {
            return TextCursor::None;
        }

        let mut is_next = false;

        for span_i in cursor.span..self.spans.len() {
            if !is_next && span_i != cursor.span {
                return TextCursor::None;
            }

            for (byte_i, c) in self.spans[span_i].text.char_indices() {
                if is_next {
                    return TextCursor::Position(PosTextCursor {
                        span: span_i,
                        byte_s: byte_i,
                        byte_e: byte_i + c.len_utf8(),
                        affinity: TextCursorAffinity::After,
                    });
                }

                if byte_i == cursor.byte_s {
                    if c.len_utf8() != cursor.byte_e - cursor.byte_s {
                        return TextCursor::None;
                    }

                    is_next = true;
                    continue;
                }

                if byte_i > cursor.byte_s {
                    return TextCursor::None;
                }
            }
        }

        TextCursor::None
    }

    pub fn cursor_prev(&self, cursor: TextCursor) -> TextCursor {
        let cursor = match cursor {
            TextCursor::None | TextCursor::Empty => return TextCursor::None,
            TextCursor::Position(cursor) => cursor,
        };

        if cursor.affinity == TextCursorAffinity::After {
            if !self.is_valid_cursor(cursor.into()) {
                return TextCursor::None;
            }

            return TextCursor::Position(PosTextCursor {
                affinity: TextCursorAffinity::Before,
                ..cursor
            });
        }

        if cursor.span >= self.spans.len()
            || cursor.byte_s >= self.spans[cursor.span].text.len()
            || cursor.byte_e > self.spans[cursor.span].text.len()
            || cursor.byte_e <= cursor.byte_s
        {
            return TextCursor::None;
        }

        let mut is_next = false;

        for span_i in (0..=cursor.span).rev() {
            if !is_next && span_i != cursor.span {
                return TextCursor::None;
            }

            for (byte_i, c) in self.spans[span_i].text.char_indices().rev() {
                if is_next {
                    return TextCursor::Position(PosTextCursor {
                        span: span_i,
                        byte_s: byte_i,
                        byte_e: byte_i + c.len_utf8(),
                        affinity: TextCursorAffinity::Before,
                    });
                }

                if byte_i == cursor.byte_s {
                    if c.len_utf8() != cursor.byte_e - cursor.byte_s {
                        return TextCursor::None;
                    }

                    is_next = true;
                    continue;
                }

                if byte_i < cursor.byte_s {
                    return TextCursor::None;
                }
            }
        }

        TextCursor::None
    }

    pub fn push(&mut self, c: char) -> TextCursor {
        if self.spans.is_empty() {
            self.spans.push(Default::default());
        }

        let span_i = self.spans.len() - 1;
        let byte_s = self.spans[span_i].text.len();
        let byte_e = byte_s + c.len_utf8();
        self.spans[span_i].text.push(c);

        TextCursor::Position(PosTextCursor {
            span: span_i,
            byte_s,
            byte_e,
            affinity: TextCursorAffinity::After,
        })
    }

    pub fn cursor_insert(&mut self, cursor: TextCursor, c: char) -> TextCursor {
        match cursor {
            TextCursor::None => TextCursor::None,
            TextCursor::Empty => {
                if self.spans.is_empty() {
                    self.spans.push(Default::default());
                }

                self.spans[0].text.insert(0, c);

                TextCursor::Position(PosTextCursor {
                    span: 0,
                    byte_s: 0,
                    byte_e: c.len_utf8(),
                    affinity: TextCursorAffinity::After,
                })
            },
            TextCursor::Position(mut cursor) => {
                if !self.is_valid_cursor(cursor.into()) {
                    return TextCursor::None;
                }

                if cursor.affinity == TextCursorAffinity::Before {
                    self.spans[cursor.span].text.insert(cursor.byte_s, c);
                    cursor.byte_e = cursor.byte_s + c.len_utf8();
                    cursor.affinity = TextCursorAffinity::After;
                    TextCursor::Position(cursor)
                } else {
                    self.spans[cursor.span].text.insert(cursor.byte_e, c);
                    cursor.byte_s = cursor.byte_e;
                    cursor.byte_e = cursor.byte_s + c.len_utf8();
                    TextCursor::Position(cursor)
                }
            },
        }
    }

    pub fn cursor_delete(&mut self, cursor: TextCursor) -> TextCursor {
        let rm_cursor = match self.cursor_prev(cursor) {
            TextCursor::None => {
                return match self.cursor_next(TextCursor::Empty) {
                    TextCursor::None => TextCursor::None,
                    TextCursor::Empty => TextCursor::Empty,
                    TextCursor::Position(mut cursor) => {
                        cursor.affinity = TextCursorAffinity::Before;
                        TextCursor::Position(cursor)
                    },
                };
            },
            TextCursor::Empty => unreachable!(),
            TextCursor::Position(cursor) => cursor,
        };

        let mut ret_cursor = match self.cursor_prev(rm_cursor.into()) {
            TextCursor::None => TextCursor::None,
            TextCursor::Empty => unreachable!(),
            TextCursor::Position(mut cursor) => {
                cursor.affinity = TextCursorAffinity::After;
                TextCursor::Position(cursor)
            },
        };

        self.spans[rm_cursor.span].text.remove(rm_cursor.byte_s);

        if self.spans[rm_cursor.span].text.is_empty() {
            self.spans.remove(rm_cursor.span);
        }

        if ret_cursor == TextCursor::None {
            ret_cursor = match self.cursor_next(TextCursor::Empty) {
                TextCursor::None => TextCursor::None,
                TextCursor::Empty => TextCursor::Empty,
                TextCursor::Position(mut cursor) => {
                    cursor.affinity = TextCursorAffinity::Before;
                    TextCursor::Position(cursor)
                },
            };
        }

        ret_cursor
    }

    pub fn selection_value(&self, selection: TextSelection) -> Option<String> {
        if !self.is_selection_valid(selection) {
            return None;
        }

        let mut output = String::new();

        for span_i in selection.start.span..=selection.end.span {
            if self.spans[span_i].is_empty() {
                continue;
            }

            let byte_i_start = if span_i == selection.start.span {
                if selection.start.affinity == TextCursorAffinity::Before {
                    selection.start.byte_s
                } else {
                    if selection.start.byte_e == self.spans[span_i].text.len() {
                        continue;
                    }

                    selection.start.byte_e
                }
            } else {
                0
            };

            let byte_i_end = if span_i == selection.end.span {
                if selection.end.affinity == TextCursorAffinity::Before {
                    let cursor_prev = match self.cursor_prev(selection.end.into()) {
                        TextCursor::Position(cursor) => cursor,
                        TextCursor::None => continue,
                        TextCursor::Empty => unreachable!(),
                    };

                    if cursor_prev.span != span_i {
                        continue;
                    }

                    cursor_prev.byte_e
                } else {
                    selection.end.byte_e
                }
            } else {
                self.spans[span_i].text.len()
            };

            for (byte_i, c) in self.spans[span_i].text.char_indices() {
                if byte_i >= byte_i_start && byte_i < byte_i_end {
                    output.push(c);
                }
            }
        }

        Some(output)
    }

    pub fn selection_delete(&mut self, selection: TextSelection) -> TextCursor {
        if !self.is_selection_valid(selection) {
            return TextCursor::None;
        }

        let ret_cursor = match selection.start.affinity {
            TextCursorAffinity::Before => {
                match self.cursor_prev(selection.start.into()) {
                    TextCursor::None => TextCursor::None,
                    TextCursor::Empty => unreachable!(),
                    TextCursor::Position(mut cursor) => {
                        cursor.affinity = TextCursorAffinity::After;
                        cursor.into()
                    },
                }
            },
            TextCursorAffinity::After => selection.start.into(),
        };

        let [start_span, start_b] = match selection.start.affinity {
            TextCursorAffinity::Before => [selection.start.span, selection.start.byte_s],
            TextCursorAffinity::After => {
                match self.cursor_next(selection.start.into()) {
                    TextCursor::None => return ret_cursor,
                    TextCursor::Empty => unreachable!(),
                    TextCursor::Position(cursor) => [cursor.span, cursor.byte_s],
                }
            },
        };

        let [end_span, end_b] = match selection.end.affinity {
            TextCursorAffinity::Before => {
                match self.cursor_prev(selection.end.into()) {
                    TextCursor::None => return ret_cursor,
                    TextCursor::Empty => unreachable!(),
                    TextCursor::Position(cursor) => [cursor.span, cursor.byte_e],
                }
            },
            TextCursorAffinity::After => [selection.end.span, selection.end.byte_e],
        };

        let mut remove_span_i = Vec::new();

        if start_span == end_span {
            self.spans[start_span]
                .text
                .replace_range(start_b..end_b, "");

            if self.spans[start_span].text.is_empty() {
                remove_span_i.push(start_span);
            }
        } else {
            for span_i in start_span..=end_span {
                if span_i == start_span {
                    self.spans[span_i].text.replace_range(start_b.., "");
                } else if span_i > start_span && span_i < end_span {
                    self.spans[span_i].text.clear();
                } else {
                    self.spans[span_i].text.replace_range(..end_b, "");
                }

                if self.spans[span_i].text.is_empty() {
                    remove_span_i.push(span_i);
                }
            }
        }

        for span_i in remove_span_i.into_iter().rev() {
            self.spans.remove(span_i);
        }

        match ret_cursor {
            TextCursor::None => {
                match self.cursor_next(TextCursor::Empty) {
                    TextCursor::None | TextCursor::Empty => TextCursor::Empty,
                    TextCursor::Position(mut cursor) => {
                        cursor.affinity = TextCursorAffinity::Before;
                        cursor.into()
                    },
                }
            },
            TextCursor::Empty => unreachable!(),
            TextCursor::Position(cursor) => cursor.into(),
        }
    }
}

/// A span of text within `TextBody`.
///
/// A span consist of the text and its text attributes.
///
/// The default values for `attrs` will inheirt those set in
/// [`TextBody.base_attrs`](struct.TextBody.html#structfield.base_attrs).
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    pub text: String,
    pub attrs: TextAttrs,
    pub _ne: NonExhaustive,
}

impl Default for TextSpan {
    fn default() -> Self {
        Self {
            text: String::new(),
            attrs: TextAttrs {
                color: Default::default(),
                ..Default::default()
            },
            _ne: NonExhaustive(()),
        }
    }
}

impl<T> From<T> for TextSpan
where
    T: Into<String>,
{
    fn from(from: T) -> Self {
        Self {
            text: from.into(),
            ..Default::default()
        }
    }
}

impl TextSpan {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// Attributes of text.
#[derive(Debug, Clone, PartialEq)]
pub struct TextAttrs {
    pub color: Color,
    pub height: UnitValue,
    pub secret: bool,
    pub font_family: FontFamily,
    pub font_weight: FontWeight,
    pub font_stretch: FontStretch,
    pub font_style: FontStyle,
    pub _ne: NonExhaustive,
}

impl Default for TextAttrs {
    fn default() -> Self {
        Self {
            color: Color::black(),
            height: Default::default(),
            secret: false,
            font_family: Default::default(),
            font_weight: Default::default(),
            font_stretch: Default::default(),
            font_style: Default::default(),
            _ne: NonExhaustive(()),
        }
    }
}

/// A user defined vertex for [`Bin`](`Bin`)
///
/// - `x` & `y` will be from the top-left on the inside of the `Bin`.
/// - `z` is an offset from the `Bin`'s z.
/// - If the associated `ImageKey` is invalid, then color will be used.
/// - If the associated `ImageKey` isn't invalid, then coords will be used.
/// - `coords` are unnormalized.
///
/// **Note**: The associated `ImageKey` **must be** loaded into the
/// [`ImageCache`](`crate::image::ImageCache`). Failure to do so will result in panics.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct BinVertex {
    pub x: UnitValue,
    pub y: UnitValue,
    pub z: i16,
    pub color: Color,
    pub coords: [f32; 2],
}

/// Style of a `Bin`
///
/// When updating the style of a `Bin` it is required to have a valid position and size.
///
/// ## Position & Size
/// There are three types of positions: [`Relative`](`Position::Relative`),
/// [`Floating`](`Position::Floating`) and [`Anchor`](`Position::Anchor`).
///
/// ### Relative
/// Bin's are positioned inside of their parent. [`pos_from_t`](`BinStyle.pos_from_t`),
/// [`pos_from_b`](`BinStyle.pos_from_b`), [`pos_from_l`](`BinStyle.pos_from_l`),
/// [`pos_from_r`](`BinStyle.pos_from_r`), [`width`](`BinStyle.width`) and
/// [`height`](`BinStyle.height`) are used to determined the position and size. Two fields of each
/// axis must be defined. By default none of these fields are defined.
///
/// **Example of a Valid Position & Size**:
/// ```no_run
/// BinStyle {
///     pos_from_t: Pixels(10.0),
///     pos_from_l: Pixels(10.0),
///     width: Pixels(100.0),
///     height: Pixels(100.0),
///     ..Default::default()
/// }
/// ```
/// Note: The horizontal axis is defined with `pos_from_l` & `width` and the vertical axis
/// is defined by `pos_from_t` & `height`.
///
/// **Example of an Invalid Position & Size**:
/// ```no_run
/// BinStyle {
///     pos_from_t: Pixels(10.0),
///     pos_from_b: Pixels(10.0),
///     pos_from_r: Pixels(10.0),
///     ..Default::default()
/// }
/// ```
/// Note: The vertical axis is properly constrained with `pos_from_l` & `pos_from_b`. On the
/// horizonal axis the right side is known, but the left side is not known! In order
/// for basalt to figure out where the left side is either `pos_from_l` or `width` must be defined.
///
/// **Behavior with Other Siblings**:
///
/// When a `Bin` has multiple children, the child with a relative position will not have its
/// position or sized altered based on other siblings, therefore; it is possible to have children
/// overlap with this position type. It is on the user to ensure this doesn't happen.
///
/// ### Floating
/// Bin's are positioned inside their parent but their position is based on other sibilings. With
/// position type the size of the `Bin` is defined with only `width` & `height`. These must always
/// be defined.
///
/// **Spacing**:
///
/// The spacing to other siblings is defined with `margin_t`, `margin_b`,  `margin_l` and
/// `margin_r`. Margin if not defined will be zero. Spacing from the outside of the parent is set
/// with the `padding_t`, `padding_b`, `padding_l` and `padding_r` on the parent. If not defined
/// padding will be zero.
///
/// **Positioning**:
///
/// How floating `Bin`'s are positioned is dependant on the parents value of `child_flow`.
/// `Flow::RightThenDown` will place `Bin`'s from left to right then down. `Flow::DownThenRight`
/// will position `Bin`'s from top to bottom then right.  `Bin`'s with a position type of floating
/// are not aware of `Bin`'s with other position types. It is on the user the other position type
/// `Bin`'s are positioned correctly to avoid overlap.
///
/// **Ordering**:
///
/// By default siblings will be positioned based on their `BinID`. `BinID`'s are is sequential and
/// `Bin` created after another will have a higher `BinID`. This can making ordering a bit
/// confusing, so it is recommended when using floating positioning that `float_weight` is defined
/// with `FloatWeight::Fixed`.
///
/// ### Anchor
/// This position type is very similar to [`Relative`](#relative).
///
/// **Differences to Relative**:
/// - Allowed to be outside of the parent without having to specify `overflow_x` & `overflow_y` to
/// `true` on the parent.
/// - Not effected by its parents scrolling.
///
/// **Overflow**:
///
/// Overflow is still constrained to the parent's parent inner bounds.
///
/// **Example of Being to the Right of the Parent**:
/// ```no_run
/// BinStyle {
///     pos_from_t: Pixels(0.0),
///     pos_from_b: Pixels(0.0),
///     pos_from_l: Percent(100.0),
///     width: Pixels(100.0)..Default::default(),
/// }
/// ```
/// Note: The `Bin` will be positioned to the right of the parent. It will have the same height as
/// the parent and width a of `100.0` pixels.
///
/// ### Z Index
/// Most of the time `z_index` shouldn't need to be specificed. By default z-index is determined by
/// a `Bin`'s nested depth. The value can be offset with `ZIndex::Offset` or set to a specific value
/// with `ZIndex::Fixed`.
///
/// ## Scrolling & Overflow
/// ...
///
/// ## Background
/// ...
///
/// ## Borders
/// ...
///
/// ## Text
/// See [`TextBody`] documentation.
#[derive(Clone)]
pub struct BinStyle {
    // Placement
    pub position: Position,
    pub z_index: ZIndex,
    pub pos_from_t: UnitValue,
    pub pos_from_b: UnitValue,
    pub pos_from_l: UnitValue,
    pub pos_from_r: UnitValue,
    pub width: UnitValue,
    pub height: UnitValue,
    // Visiblity & Opacity
    pub visibility: Visibility,
    pub opacity: Opacity,
    // Floating Properties
    pub child_flow: Flow,
    pub float_weight: FloatWeight,
    // Margin
    pub margin_t: UnitValue,
    pub margin_b: UnitValue,
    pub margin_l: UnitValue,
    pub margin_r: UnitValue,
    // Padding
    pub padding_t: UnitValue,
    pub padding_b: UnitValue,
    pub padding_l: UnitValue,
    pub padding_r: UnitValue,
    // Scroll & Overflow
    pub scroll_y: f32,
    pub scroll_x: f32,
    pub overflow_y: bool,
    pub overflow_x: bool,
    // Border
    pub border_size_t: UnitValue,
    pub border_size_b: UnitValue,
    pub border_size_l: UnitValue,
    pub border_size_r: UnitValue,
    pub border_color_t: Color,
    pub border_color_b: Color,
    pub border_color_l: Color,
    pub border_color_r: Color,
    pub border_radius_tl: UnitValue,
    pub border_radius_tr: UnitValue,
    pub border_radius_bl: UnitValue,
    pub border_radius_br: UnitValue,
    // Background
    pub back_color: Color,
    pub back_image: ImageKey,
    pub back_image_region: BackImageRegion,
    pub back_image_effect: ImageEffect,
    // Text
    pub text_body: TextBody,
    // Misc
    pub user_vertexes: Vec<(ImageKey, Vec<BinVertex>)>,
    pub _ne: NonExhaustive,
}

impl Default for BinStyle {
    fn default() -> Self {
        Self {
            position: Default::default(),
            z_index: Default::default(),
            child_flow: Default::default(),
            float_weight: Default::default(),
            visibility: Default::default(),
            opacity: Default::default(),
            pos_from_t: Default::default(),
            pos_from_b: Default::default(),
            pos_from_l: Default::default(),
            pos_from_r: Default::default(),
            width: Default::default(),
            height: Default::default(),
            margin_t: Default::default(),
            margin_b: Default::default(),
            margin_l: Default::default(),
            margin_r: Default::default(),
            padding_t: Default::default(),
            padding_b: Default::default(),
            padding_l: Default::default(),
            padding_r: Default::default(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            overflow_y: false,
            overflow_x: false,
            border_size_t: Default::default(),
            border_size_b: Default::default(),
            border_size_l: Default::default(),
            border_size_r: Default::default(),
            border_color_t: Default::default(),
            border_color_b: Default::default(),
            border_color_l: Default::default(),
            border_color_r: Default::default(),
            border_radius_tl: Default::default(),
            border_radius_tr: Default::default(),
            border_radius_bl: Default::default(),
            border_radius_br: Default::default(),
            back_color: Default::default(),
            back_image: Default::default(),
            back_image_region: Default::default(),
            back_image_effect: Default::default(),
            text_body: Default::default(),
            user_vertexes: Vec::new(),
            _ne: NonExhaustive(()),
        }
    }
}

/// Error produced from an invalid style
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinStyleError {
    pub ty: BinStyleErrorType,
    pub desc: String,
}

impl std::fmt::Display for BinStyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: {}", self.ty, self.desc)
    }
}

/// Type of error produced from an invalid style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BinStyleErrorType {
    /// Two fields are conflicted only one must be set.
    ConflictingFields,
    /// Too many fields are defining an attribute.
    TooManyConstraints,
    /// Not enough fields are defining an attribute.
    NotEnoughConstraints,
    /// Provided Image isn't valid.
    InvalidImage,
    /// Provided Value isn't valid.
    InvalidValue,
}

impl std::fmt::Display for BinStyleErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ConflictingFields => write!(f, "Conflicting Fields"),
            Self::TooManyConstraints => write!(f, "Too Many Constraints"),
            Self::NotEnoughConstraints => write!(f, "Not Enough Constraints"),
            _ => write!(f, "Unknown"),
        }
    }
}

/// Warning produced for a suboptimal style
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinStyleWarn {
    pub ty: BinStyleWarnType,
    pub desc: String,
}

impl std::fmt::Display for BinStyleWarn {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: {}", self.ty, self.desc)
    }
}

/// Type of warning produced for a suboptimal style
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BinStyleWarnType {
    /// Field is set, but isn't used or incompatible with other styles.
    UselessField,
}

impl std::fmt::Display for BinStyleWarnType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::UselessField => write!(f, "Useless Field"),
        }
    }
}

/// Validation result produced from updating a `BinStyle`
///
/// To remove the `#[must_use]` attribute, enable the `style_validation_debug_on_drop` feature.
/// This feature will call the `debug` method automatically if no other method was used.
#[cfg_attr(not(feature = "style_validation_debug_on_drop"), must_use)]
pub struct BinStyleValidation {
    errors: Vec<BinStyleError>,
    warnings: Vec<BinStyleWarn>,
    location: Option<String>,
    used: bool,
}

impl BinStyleValidation {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            location: None,
            used: false,
        }
    }

    #[track_caller]
    fn error<D: Into<String>>(&mut self, ty: BinStyleErrorType, desc: D) {
        self.errors.push(BinStyleError {
            ty,
            desc: desc.into(),
        });

        if self.location.is_none() {
            self.location = Some(format!("{}", std::panic::Location::caller()));
        }
    }

    #[track_caller]
    fn warning<D: Into<String>>(&mut self, ty: BinStyleWarnType, desc: D) {
        self.warnings.push(BinStyleWarn {
            ty,
            desc: desc.into(),
        });

        if self.location.is_none() {
            self.location = Some(format!("{}", std::panic::Location::caller()));
        }
    }

    /// Expect `BinStyle` provided to `style_update()` is valid panicking if that is not the case.
    pub fn expect_valid(mut self) {
        self.used = true;

        if !self.errors.is_empty() {
            let mut panic_msg = format!(
                "BinStyleValidation-Error {{ caller: {},",
                self.location.take().unwrap()
            );
            let error_count = self.errors.len();

            if error_count == 1 {
                panic_msg = format!(
                    "{} error: Error {{ ty: {}, desc: {} }} }}",
                    panic_msg, self.errors[0].ty, self.errors[0].desc
                );
            } else {
                for (i, error) in self.errors.iter().enumerate() {
                    if i == 0 {
                        panic_msg = format!(
                            "{} errors: [ Error {{ ty: {}, desc: {} }},",
                            panic_msg, error.ty, error.desc
                        );
                    } else if i == error_count - 1 {
                        panic_msg = format!(
                            "{} Error {{ ty: {}, desc: {} }} ] }}",
                            panic_msg, error.ty, error.desc
                        );
                    } else {
                        panic_msg = format!(
                            "{} Error {{ ty: {}, desc: {} }},",
                            panic_msg, error.ty, error.desc
                        );
                    }
                }
            }

            panic!("{}", panic_msg);
        }
    }

    /// Does the same thing as `expect_valid`, but in the case of no errors, it'll print pretty warnings to the terminal.
    pub fn expect_valid_debug_warn(mut self) {
        self.used = true;

        if self.errors.is_empty() {
            if !self.warnings.is_empty() {
                let mut msg = format!(
                    "BinStyleValidation-Warn: {}:\n",
                    self.location.take().unwrap()
                );

                for warning in &self.warnings {
                    msg = format!("{}  {}: {}\n", msg, warning.ty, warning.desc);
                }

                msg.pop();
                println!("{}", msg);
            }
        } else {
            self.expect_valid();
        }
    }

    /// Returns `true` if errors are present.
    pub fn errors_present(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Return an `Iterator` of `BinStyleError`
    ///
    /// ***Note:** This method should only be called once. As it move the errors out.*
    pub fn errors(&mut self) -> impl Iterator<Item = BinStyleError> {
        self.used = true;
        self.errors.split_off(0).into_iter()
    }

    /// Returns `true` if warnings are present.
    pub fn warnings_present(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Return an `Iterator` of `BinStyleWarn`
    ///
    /// ***Note:** This method should only be called once. As it move the warnings out.*
    pub fn warnings(&mut self) -> impl Iterator<Item = BinStyleWarn> {
        self.used = true;
        self.warnings.split_off(0).into_iter()
    }

    /// Acknowlege the style update may have not be successful and just print pretty errors/warnings to the terminal.
    pub fn debug(mut self) {
        self.debug_impl();
    }

    fn debug_impl(&mut self) {
        if self.used {
            return;
        }

        self.used = true;

        if !self.errors.is_empty() || !self.warnings.is_empty() {
            let mut msg = format!("BinStyleValidation: {}:\n", self.location.take().unwrap());

            for error in &self.errors {
                msg = format!("{}  Error {}: {}\n", msg, error.ty, error.desc);
            }

            for warning in &self.warnings {
                msg = format!("{}  Warning {}: {}\n", msg, warning.ty, warning.desc);
            }

            msg.pop();
            println!("{}", msg);
        }
    }
}

#[cfg(feature = "style_validation_debug_on_drop")]
impl Drop for BinStyleValidation {
    fn drop(&mut self) {
        self.debug_impl();
    }
}

macro_rules! useless_field {
    ($style:ident, $field:ident, $name:literal, $validation:ident) => {
        if $style.$field.is_defined() {
            $validation.warning(
                BinStyleWarnType::UselessField,
                format!("'{}' is defined, but is ignored.", $name),
            );
        }
    };
}

impl BinStyle {
    #[track_caller]
    pub(crate) fn validate(&self, bin: &Arc<Bin>) -> BinStyleValidation {
        let mut validation = BinStyleValidation::new();
        let has_parent = bin.hrchy.read().parent.is_some();

        match self.position {
            Position::Relative | Position::Anchor => {
                if self.float_weight != FloatWeight::Auto {
                    validation.warning(
                        BinStyleWarnType::UselessField,
                        "'float_weight' is `Fixed`, but is ignored.",
                    );
                }

                if validation.errors.is_empty() {
                    let pft = self.pos_from_t.is_defined();
                    let pfb = self.pos_from_b.is_defined();
                    let pfl = self.pos_from_l.is_defined();
                    let pfr = self.pos_from_r.is_defined();
                    let width = self.width.is_defined();
                    let height = self.height.is_defined();

                    match (pft, pfb, height) {
                        (true, true, true) => {
                            validation.error(
                                BinStyleErrorType::TooManyConstraints,
                                "'pos_from_t', 'pos_from_b' & 'height' are defined, but only two \
                                 can be defined.",
                            );
                        },
                        (true, false, false) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                "'pos_from_t' is defined, but `pos_from_b` or `height` must also \
                                 be defined.",
                            );
                        },
                        (false, true, false) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                "'pos_from_b' is defined, but `pos_from_t` or `height` must also \
                                 be defined.",
                            );
                        },
                        (false, false, true) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                "'height' is defined, but `pos_from_t` or `pos_from_b` must also \
                                 be defined.",
                            );
                        },
                        _ => (),
                    }

                    match (pfl, pfr, width) {
                        (true, true, true) => {
                            validation.error(
                                BinStyleErrorType::TooManyConstraints,
                                "'pos_from_t', 'pos_from_r' & 'width' are defined, but only two \
                                 can be defined.",
                            );
                        },
                        (true, false, false) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                "'pos_from_l' is defined, but `pos_from_r` or `width` must also \
                                 be defined.",
                            );
                        },
                        (false, true, false) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                "'pos_from_r' is defined, but `pos_from_l` or `width` must also \
                                 be defined.",
                            );
                        },
                        (false, false, true) => {
                            validation.error(
                                BinStyleErrorType::NotEnoughConstraints,
                                "'width' is defined, but `pos_from_l` or `pos_from_r` must also \
                                 be defined.",
                            );
                        },
                        _ => (),
                    }
                }
            },
            Position::Floating => {
                useless_field!(self, pos_from_t, "pos_from_t", validation);
                useless_field!(self, pos_from_b, "pos_from_b", validation);
                useless_field!(self, pos_from_l, "pos_from_l", validation);
                useless_field!(self, pos_from_r, "pos_from_r", validation);

                if !has_parent {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "Floating Bin's must have a parent.",
                    );
                }

                if !self.width.is_defined() {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "'width' must be defined.",
                    );
                }

                if !self.height.is_defined() {
                    validation.error(
                        BinStyleErrorType::NotEnoughConstraints,
                        "'height' must be defined.",
                    );
                }
            },
        }

        if matches!(self.border_radius_tl, UnitValue::Percent(..)) {
            validation.error(
                BinStyleErrorType::InvalidValue,
                "'border_radius_tl' can not be 'Percent`. Use `PctOfWidth` or `PctOfHeight` \
                 instead.",
            );
        }

        if matches!(self.border_radius_tr, UnitValue::Percent(..)) {
            validation.error(
                BinStyleErrorType::InvalidValue,
                "'border_radius_tr' can not be 'Percent`. Use `PctOfWidth` or `PctOfHeight` \
                 instead.",
            );
        }

        if matches!(self.border_radius_bl, UnitValue::Percent(..)) {
            validation.error(
                BinStyleErrorType::InvalidValue,
                "'border_radius_bl' can not be 'Percent`. Use `PctOfWidth` or `PctOfHeight` \
                 instead.",
            );
        }

        if matches!(self.border_radius_br, UnitValue::Percent(..)) {
            validation.error(
                BinStyleErrorType::InvalidValue,
                "'border_radius_br' can not be 'Percent`. Use `PctOfWidth` or `PctOfHeight` \
                 instead.",
            );
        }

        if !self.back_image.is_invalid() {
            if let Some(image_id) = self.back_image.as_vulkano_id() {
                match bin.basalt.device_resources_ref().image(image_id) {
                    Ok(image_state) => {
                        let image = image_state.image();

                        if image.image_type() != vko::ImageType::Dim2d {
                            validation.error(
                                BinStyleErrorType::InvalidImage,
                                "'ImageKey::vulkano_id' provided with 'back_image' must be 2d.",
                            );
                        }

                        if image.array_layers() != 1 {
                            validation.error(
                                BinStyleErrorType::InvalidImage,
                                "'ImageKey::vulkano_id' provided with 'back_image' must not have \
                                 array layers.",
                            );
                        }

                        if image.mip_levels() != 1 {
                            validation.error(
                                BinStyleErrorType::InvalidImage,
                                "'ImageKey::vulkano_id' provided with 'back_image' must not have \
                                 mip levels.",
                            );
                        }

                        if !image.format_features().contains(
                            vko::FormatFeatures::TRANSFER_DST
                                | vko::FormatFeatures::TRANSFER_SRC
                                | vko::FormatFeatures::SAMPLED_IMAGE
                                | vko::FormatFeatures::SAMPLED_IMAGE_FILTER_LINEAR,
                        ) {
                            validation.error(
                                BinStyleErrorType::InvalidImage,
                                "'ImageKey::vulkano_id' provided with 'back_image' must have a \
                                 format that supports, 'TRANSFER_DST`, `TRANSFER_SRC`, \
                                 `SAMPLED_IMAGE`, & `SAMPLED_IMAGE_FILTER_LINEAR`.",
                            );
                        }
                    },
                    Err(_) => {
                        validation.error(
                            BinStyleErrorType::InvalidImage,
                            "'ImageKey::vulkano_id' provided with 'back_image' must be created \
                             from 'Basalt::device_resources_ref()'.",
                        );
                    },
                };
            } else if self.back_image.is_image_cache() {
                if self.back_image.is_glyph() {
                    validation.error(
                        BinStyleErrorType::InvalidImage,
                        "'ImageKey::glyph' provided with 'back_image' can not be used.",
                    );
                } else if self.back_image.is_any_user()
                    && bin
                        .basalt
                        .image_cache_ref()
                        .obtain_image_info(&self.back_image)
                        .is_none()
                {
                    validation.error(
                        BinStyleErrorType::InvalidImage,
                        "'ImageKey::user' provided with 'back_image' must be preloaded into the \
                         `ImageCache`.",
                    );
                }
            } else {
                validation.error(
                    BinStyleErrorType::InvalidImage,
                    "'ImageKey' provided with 'back_image' must be valid.",
                );
            }
        }

        validation
    }
}
