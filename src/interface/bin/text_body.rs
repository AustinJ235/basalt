use std::collections::BTreeMap;

use unicode_segmentation::UnicodeSegmentation;

use crate::NonExhaustive;
use crate::interface::{
    Color, FontFamily, FontStretch, FontStyle, FontWeight, LineLimit, LineSpacing, PosTextCursor,
    TextCursor, TextCursorAffinity, TextHoriAlign, TextSelection, TextVertAlign, TextWrap,
    UnitValue,
};

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

/// The text body of a `Bin`.
///
/// Each [`BinStyle`](`crate::interface::BinStyle`) has a single `TextBody`. It can contain multiple
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
    /// Checks if all spans are empty.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty() || self.spans.iter().all(|span| span.is_empty())
    }

    /// Checks if the provided cursor is valid for this `TextBody`.
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

    /// Checks if the provided selection is valid for this `TextBody`.
    pub fn is_selection_valid(&self, selection: TextSelection) -> bool {
        self.is_valid_cursor(selection.start.into()) && self.is_valid_cursor(selection.end.into())
    }

    /// Tries to moves the provided cursor to the next position.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is `None`.
    /// - there isn't a valid cursor position after the one provided.
    ///
    /// **Returns [`Empty`]('TextCursor::Empty`) if:**
    /// -  the provided cursor is [`Empty`](`TextCursor::Empty`) and the `TextBody` is empty.
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

    /// Tries to moves the provided cursor to the previous position.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is `None` or `Empty`.
    /// - there isn't a valid cursor position before the one provided.
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

    /// Inserts a character at the end of the last span.
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

    /// Inserts a character at the provided cursor.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
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

    /// Inserts a string at the provided cursor.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](`TextCursor::None`).
    pub fn cursor_insert_string<S>(&mut self, cursor: TextCursor, string: S) -> TextCursor
    where
        S: AsRef<str>,
    {
        let string = string.as_ref();

        if string.is_empty() {
            return cursor;
        }

        let [span_i, byte_i] = match cursor {
            TextCursor::None => return TextCursor::None,
            TextCursor::Empty => {
                if self.spans.is_empty() {
                    self.spans.push(Default::default());
                }

                [0, 0]
            },
            TextCursor::Position(cursor) => {
                if !self.is_valid_cursor(cursor.into()) {
                    return TextCursor::None;
                }

                match cursor.affinity {
                    TextCursorAffinity::Before => [cursor.span, cursor.byte_s],
                    TextCursorAffinity::After => [cursor.span, cursor.byte_e],
                }
            },
        };

        self.spans[span_i].text.insert_str(byte_i, string);
        let (mut byte_s, c) = string.char_indices().rev().next().unwrap();
        byte_s += byte_i;
        let byte_e = byte_s + c.len_utf8();

        TextCursor::Position(PosTextCursor {
            span: span_i,
            byte_s,
            byte_e,
            affinity: TextCursorAffinity::After,
        })
    }

    /// Inserts a `TextSpan`'s at the provided cursor.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](`TextCursor::None`).
    ///
    /// **Note:** This will merge adjacent spans with the same attributes.
    pub fn cursor_insert_spans<S>(&mut self, cursor: TextCursor, spans: S) -> TextCursor
    where
        S: IntoIterator<Item = TextSpan>,
    {
        let [span_i, byte_i] = match cursor {
            TextCursor::None => return TextCursor::None,
            TextCursor::Empty => {
                if self.spans.is_empty() {
                    self.spans.push(Default::default());
                }

                [0, 0]
            },
            TextCursor::Position(cursor) => {
                if !self.is_valid_cursor(cursor.into()) {
                    return TextCursor::None;
                }

                match cursor.affinity {
                    TextCursorAffinity::Before => [cursor.span, cursor.byte_s],
                    TextCursorAffinity::After => [cursor.span, cursor.byte_e],
                }
            },
        };

        let mut pos_c = PosTextCursor {
            span: span_i,
            byte_s: 0,
            byte_e: byte_i,
            affinity: TextCursorAffinity::After,
        };

        let mut is_empty = true;

        for span in spans.into_iter() {
            if span.attrs == self.spans[pos_c.span].attrs {
                // Same attributes as the current span, so merge.

                self.spans[pos_c.span]
                    .text
                    .insert_str(pos_c.byte_e, span.text.as_str());
                pos_c.byte_e += span.text.len();
                is_empty = false;
                continue;
            }

            if pos_c.span + 1 < self.spans.len()
                && pos_c.byte_e == self.spans[pos_c.span].text.len()
                && span.attrs == self.spans[pos_c.span + 1].attrs
            {
                // There is a span following this one, the cursor is at the end of the current span
                // and the attributes are the same as the following span, so merge.

                self.spans[pos_c.span + 1]
                    .text
                    .insert_str(0, span.text.as_str());
                pos_c.span += 1;
                pos_c.byte_e = span.text.len();
                is_empty = false;
                continue;
            }

            // Doesn't match either span, so insert.

            pos_c.span += 1;
            pos_c.byte_e = span.text.len();
            self.spans.insert(pos_c.span + 1, span);
            is_empty = false;
        }

        if is_empty { cursor } else { pos_c.into() }
    }

    /// Deletes the character at the provided cursor.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is `None`.
    ///
    /// **Returns [`Empty`](`TextCursor::Empty`) if:**
    /// - the `TextBody` is empty after the deletion.
    ///
    /// **Note**: If deleletion empties the cursor's span the span will be removed.
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

    /// Return a selection containing the whole word of the provided cursor.
    ///
    /// **Returns `None` if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is `None` or `Empty`.
    pub fn select_word(&self, cursor: TextCursor) -> Option<TextSelection> {
        let cursor = match cursor {
            TextCursor::None | TextCursor::Empty => return None,
            TextCursor::Position(cursor) => {
                if !self.is_valid_cursor(cursor.into()) {
                    return None;
                }

                cursor
            },
        };

        let mut spans_concat = String::new();
        let mut byte_map: BTreeMap<usize, [usize; 3]> = BTreeMap::new();

        for (span_i, span) in self.spans.iter().enumerate() {
            let offset = spans_concat.len();
            spans_concat.push_str(span.text.as_str());

            for (byte_i, c) in span.text.char_indices() {
                byte_map.insert(offset + byte_i, [span_i, byte_i, byte_i + c.len_utf8()]);
            }
        }

        let mut cursor_byte_i = cursor.byte_s;

        for span_i in 0..cursor.span {
            cursor_byte_i += self.spans[span_i].text.len();
        }

        for (byte_i, word_str) in spans_concat.split_word_bound_indices() {
            if !(byte_i..(byte_i + word_str.len())).contains(&cursor_byte_i) {
                continue;
            }

            let char_map = byte_map
                .range(byte_i..(byte_i + word_str.len()))
                .collect::<Vec<_>>();

            if char_map.is_empty() {
                return None;
            }

            let f_char = char_map.first().unwrap();
            let l_char = char_map.last().unwrap();

            return Some(TextSelection {
                start: PosTextCursor {
                    span: f_char.1[0],
                    byte_s: f_char.1[1],
                    byte_e: f_char.1[2],
                    affinity: TextCursorAffinity::Before,
                },
                end: PosTextCursor {
                    span: l_char.1[0],
                    byte_s: l_char.1[1],
                    byte_e: l_char.1[2],
                    affinity: TextCursorAffinity::After,
                },
            });
        }

        None
    }

    /// Obtain the selection's value as `String`.
    ///
    /// **Returns an empty `String` if:**
    /// - The provided `TextSelection` is invalid.
    pub fn selection_string(&self, selection: TextSelection) -> String {
        self.selection_spans(selection)
            .into_iter()
            .map(|span| span.text)
            .collect()
    }

    /// Obtain the selection's value as `Vec<TextSpan>`.
    ///
    /// **Returns an empty `Vec` if:**
    /// - The provided `TextSelection` is invalid.
    pub fn selection_spans(&self, selection: TextSelection) -> Vec<TextSpan> {
        let [s_span, s_byte, e_span, e_byte] = match self.selection_byte_range(selection) {
            Some(some) => some,
            None => return Vec::new(),
        };

        let mut spans = Vec::with_capacity(e_span - s_span + 1);

        for span_i in s_span..=e_span {
            let span_s_byte_op = if span_i == s_span {
                if s_byte == 0 { None } else { Some(s_byte) }
            } else {
                None
            };

            let span_e_byte_op = if span_i == e_span {
                if e_byte == self.spans[span_i].text.len() {
                    None
                } else {
                    Some(e_byte)
                }
            } else {
                None
            };

            let mut sel_str = match span_s_byte_op {
                Some(span_s_byte) => self.spans[span_i].text.split_at(span_s_byte).1,
                None => self.spans[span_i].text.as_str(),
            };

            if let Some(span_e_byte) = span_e_byte_op {
                sel_str = sel_str.split_at(span_e_byte).0;
            }

            spans.push(TextSpan {
                attrs: self.spans[span_i].attrs.clone(),
                text: sel_str.into(),
                ..Default::default()
            });
        }

        spans
    }

    /// Take the selection out of the `TextBody` returning the value as `String`.
    ///
    /// **The returned `String` will be empty if:**
    /// -  the provided selection is invalid.
    ///
    /// **Note**: The returned `TextCursor` behaves the same as
    /// [`selection_delete`](`TextBody::selection_delete`)
    pub fn selection_take_string(&mut self, selection: TextSelection) -> (TextCursor, String) {
        let (cursor, spans) = self.selection_take_spans(selection);
        (cursor, spans.into_iter().map(|span| span.text).collect())
    }

    /// Take the selection out of the `TextBody` returning the value as `Vec<TextSpan>`.
    ///
    /// **The returned `Vec<TextSpan>` will be empty if:**
    /// -  the provided selection is invalid.
    ///
    /// **Note**: The returned `TextCursor` behaves the same as
    /// [`selection_delete`](`TextBody::selection_delete`)
    pub fn selection_take_spans(
        &mut self,
        selection: TextSelection,
    ) -> (TextCursor, Vec<TextSpan>) {
        let mut spans = Vec::with_capacity(selection.end.span - selection.start.span + 1);
        let cursor = self.inner_selection_delete(selection, Some(&mut spans));
        (cursor, spans)
    }

    /// Delete the provided selection.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided selection is invalid.
    ///
    /// **Returns [`Empty`](`TextCursor::Empty`) if:**
    /// - the `TextBody` is empty after the deletion.
    ///
    /// **Note**: If deleletion empties any span within the selection, the span will be removed.
    pub fn selection_delete(&mut self, selection: TextSelection) -> TextCursor {
        self.inner_selection_delete(selection, None)
    }

    fn inner_selection_delete(
        &mut self,
        selection: TextSelection,
        mut spans_op: Option<&mut Vec<TextSpan>>,
    ) -> TextCursor {
        let [s_span, s_byte, e_span, e_byte] = match self.selection_byte_range(selection) {
            Some(some) => some,
            None => return TextCursor::None,
        };

        let mut ret_cursor = match selection.start.affinity {
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

        let mut remove_spans = Vec::new();

        for span_i in s_span..=e_span {
            let span_s_byte = if span_i == s_span { s_byte } else { 0 };

            let span_e_byte = if span_i == e_span {
                e_byte
            } else {
                self.spans[span_i].text.len()
            };

            let text = self.spans[span_i]
                .text
                .drain(span_s_byte..span_e_byte)
                .collect::<String>();

            if let Some(spans) = spans_op.as_mut() {
                spans.push(TextSpan {
                    attrs: self.spans[span_i].attrs.clone(),
                    text,
                    ..Default::default()
                });
            }

            if self.spans[span_i].text.is_empty() {
                remove_spans.push(span_i);
            }
        }

        for span_i in remove_spans.into_iter().rev() {
            self.spans.remove(span_i);
        }

        if ret_cursor == TextCursor::None {
            ret_cursor = match self.cursor_next(TextCursor::Empty) {
                TextCursor::None | TextCursor::Empty => TextCursor::Empty,
                TextCursor::Position(mut cursor) => {
                    cursor.affinity = TextCursorAffinity::Before;
                    cursor.into()
                },
            };
        }

        ret_cursor
    }

    fn selection_byte_range(&self, selection: TextSelection) -> Option<[usize; 4]> {
        if !self.is_selection_valid(selection) {
            return None;
        }

        let [start_span, start_b] = match selection.start.affinity {
            TextCursorAffinity::Before => [selection.start.span, selection.start.byte_s],
            TextCursorAffinity::After => {
                match self.cursor_next(selection.start.into()) {
                    TextCursor::None => return None,
                    TextCursor::Empty => unreachable!(),
                    TextCursor::Position(cursor) => [cursor.span, cursor.byte_s],
                }
            },
        };

        let [end_span, end_b] = match selection.end.affinity {
            TextCursorAffinity::Before => {
                match self.cursor_prev(selection.end.into()) {
                    TextCursor::None => return None,
                    TextCursor::Empty => unreachable!(),
                    TextCursor::Position(cursor) => [cursor.span, cursor.byte_e],
                }
            },
            TextCursorAffinity::After => [selection.end.span, selection.end.byte_e],
        };

        Some([start_span, start_b, end_span, end_b])
    }
}
