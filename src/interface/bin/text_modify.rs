use std::cell::{Ref, RefCell, RefMut};
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use parking_lot::{MutexGuard, RwLockUpgradableReadGuard};
use unicode_segmentation::UnicodeSegmentation;

use crate::interface::bin::{InternalHookFn, InternalHookTy, TextState, UpdateState};
use crate::interface::{
    Bin, BinPostUpdate, BinStyle, BinStyleValidation, Color, DefaultFont, PosTextCursor, Position,
    TextAttrs, TextBody, TextCursor, TextCursorAffinity, TextSelection, TextSpan,
};

/// Used to inspect and/or modify the [`TextBody`](TextBody).
///
/// **Warning:** While `TextBodyGuard` is in scope, methods that modify [`Bin`](Bin)'s
/// should not be used. Failure to do so, may result in potential deadlocks. Methods that
/// should be avoided include [`Bin::style_modify`](Bin::style_modify),
/// [`Bin::style_modify_then`](Bin::style_modify_then), [`Bin::style_update`](Bin::style_update),
/// [`Bin::set_visibility`](Bin::set_visibility), [`Bin::toggle_visibility`](Bin::toggle_visibility), etc.
/// `TextBodyGuard` provides methods to do so, see [`TextBodyGuard::style_modify`] and
/// [`TextBodyGuard::bin_on_update`].
pub struct TextBodyGuard<'a> {
    bin: &'a Arc<Bin>,
    text_state: RefCell<Option<TextStateGuard<'a>>>,
    style_state: RefCell<Option<StyleState<'a>>>,
    tlwh: RefCell<Option<[f32; 4]>>,
    default_font: RefCell<Option<DefaultFont>>,
    on_update: RefCell<Vec<Box<dyn FnOnce(&Arc<Bin>, &BinPostUpdate) + Send + 'static>>>,
}

impl<'a> TextBodyGuard<'a> {
    /// Check if [`TextBody`](TextBody) is empty.
    pub fn is_empty(&self) -> bool {
        let body = &self.style().text_body;
        body.spans.is_empty() || body.spans.iter().all(|span| span.is_empty())
    }

    /// Check if the provided [`TextCursor`](TextCursor) is valid.
    pub fn is_cursor_valid<C>(&self, cursor: C) -> bool
    where
        C: Into<TextCursor>,
    {
        let body = &self.style().text_body;

        let cursor = match cursor.into() {
            TextCursor::Position(cursor) => cursor,
            _ => return true,
        };

        if cursor.span >= body.spans.len()
            || cursor.byte_s >= body.spans[cursor.span].text.len()
            || cursor.byte_e > body.spans[cursor.span].text.len()
            || cursor.byte_e <= cursor.byte_s
        {
            return false;
        }

        if !body.spans[cursor.span].text.is_char_boundary(cursor.byte_s) {
            return false;
        }

        for (byte_i, c) in body.spans[cursor.span].text.char_indices() {
            if byte_i == cursor.byte_s {
                if c.len_utf8() != cursor.byte_e - cursor.byte_s {
                    return false;
                }

                break;
            }
        }

        true
    }

    /// Check if the provided [`TextSelection`](TextSelection) is valid.
    pub fn is_selection_valid(&self, selection: TextSelection) -> bool {
        self.is_cursor_valid(selection.start) && self.is_cursor_valid(selection.end)
    }

    /// Obtain the current displayed [`TextCursor`](TextCursor).
    pub fn cursor(&self) -> TextCursor {
        self.style().text_body.cursor
    }

    /// Set the displayed [`TextCursor`](TextCursor).
    pub fn set_cursor(&self, cursor: TextCursor) {
        self.style_mut().text_body.cursor = cursor;
    }

    /// Set the [`Color`](Color) of the displayed [`TextCursor`](TextCursor).
    pub fn set_cursor_color(&self, color: Color) {
        self.style_mut().text_body.cursor_color = color;
    }

    /// Obtain a [`TextCursor`](TextCursor) given a phyiscal position.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - this `Bin` is currently not visible.
    ///
    /// **Returns [`Empty`](TextCursor::Empty) if:**
    /// - this `Bin` has yet to update the text layout.
    /// - this `Bin`'s `TextBody` is empty.
    pub fn get_cursor(&self, mut position: [f32; 2]) -> TextCursor {
        self.update_layout();
        let tlwh = self.tlwh();
        position[0] -= tlwh[1];
        position[1] -= tlwh[0];
        self.state().get_cursor(position)
    }

    /// Obtain the line and column index of the provided [`TextCursor`](TextCursor).
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns `None` if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_line_column(&self, cursor: TextCursor, as_displayed: bool) -> Option<[usize; 2]> {
        if as_displayed {
            self.update_layout();
            self.state().cursor_line_column(cursor)
        } else {
            if !self.is_cursor_valid(cursor) {
                return None;
            }

            let cursor = match cursor {
                TextCursor::None | TextCursor::Empty => return None,
                TextCursor::Position(cursor) => cursor,
            };

            let body = &self.style().text_body;
            let mut line_i = 0;
            let mut col_i = 0;

            for span_i in 0..body.spans.len() {
                if span_i < cursor.span {
                    for c in body.spans[span_i].text.chars() {
                        if c == '\n' {
                            line_i += 1;
                        }
                    }
                } else {
                    for (byte_i, _) in body.spans[span_i].text.char_indices() {
                        if byte_i < cursor.byte_s {
                            col_i += 1;
                        } else {
                            break;
                        }
                    }
                }
            }

            Some([line_i, col_i])
        }
    }

    /// Obtain the bounding box of the provided [`TextCursor`](TextCursor).
    ///
    /// Format: `[MIN_X, MAX_X, MIN_Y, MAX_Y]`.
    ///
    /// **Returns `None` if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](TextCursor::None).
    pub fn cursor_bounds(&self, mut cursor: TextCursor) -> Option<[f32; 4]> {
        if cursor == TextCursor::None {
            return None;
        }

        if cursor == TextCursor::Empty {
            // Note: TextState::get_cursor_bounds doesn't check if the body is empty and assumes if
            // the provided cursor is Empty the body is empty.

            cursor = match self.cursor_next(TextCursor::Empty) {
                TextCursor::Empty => TextCursor::Empty,
                TextCursor::None => return None,
                text_cursor_position @ TextCursor::Position(_) => text_cursor_position,
            };
        }

        self.update_layout();
        let tlwh = self.tlwh();
        let default_font = self.default_font();
        let style = self.style();

        self.state()
            .get_cursor_bounds(cursor, tlwh, &style.text_body, default_font.height)
            .map(|(bounds, _)| bounds)
    }

    /// Obtain the bounding box of the displayed line with the provided [`TextCursor`](TextCursor).
    ///
    /// Format: `[MIN_X, MAX_X, MIN_Y, MAX_Y]`.
    ///
    /// **Returns `None` if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](TextCursor::None) or [`Empty`](TextCursor::Empty).
    pub fn cursor_line_bounds(&self, cursor: TextCursor) -> Option<[f32; 4]> {
        let line_i = self.cursor_line_column(cursor, true)?[0];
        let tlwh = self.tlwh();
        self.state().line_bounds(tlwh, line_i)
    }

    /// Get the [`TextCursor`](TextCursor) one position to the left of the provided [`TextCursor`](TextCursor).
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](TextCursor::None) or [`Empty`](TextCursor::Empty).
    /// - there isn't a valid cursor position before the one provided.
    pub fn cursor_prev(&self, cursor: TextCursor) -> TextCursor {
        let body = &self.style().text_body;

        let cursor = match cursor {
            TextCursor::None | TextCursor::Empty => return TextCursor::None,
            TextCursor::Position(cursor) => cursor,
        };

        if cursor.affinity == TextCursorAffinity::After {
            if !self.is_cursor_valid(cursor) {
                return TextCursor::None;
            }

            return TextCursor::Position(PosTextCursor {
                affinity: TextCursorAffinity::Before,
                ..cursor
            });
        }

        if cursor.span >= body.spans.len()
            || cursor.byte_s >= body.spans[cursor.span].text.len()
            || cursor.byte_e > body.spans[cursor.span].text.len()
            || cursor.byte_e <= cursor.byte_s
        {
            return TextCursor::None;
        }

        let mut is_next = false;

        for span_i in (0..=cursor.span).rev() {
            if !is_next && span_i != cursor.span {
                return TextCursor::None;
            }

            for (byte_i, c) in body.spans[span_i].text.char_indices().rev() {
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

    /// Get the [`TextCursor`](TextCursor) one position to the right of the provided [`TextCursor`](TextCursor).
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is `None`.
    /// - there isn't a valid cursor position after the one provided.
    ///
    /// **Returns [`Empty`]('TextCursor::Empty`) if:**
    /// -  the provided cursor is [`Empty`](`TextCursor::Empty`) and the [`TextBody`](TextBody) is empty.
    pub fn cursor_next(&self, cursor: TextCursor) -> TextCursor {
        let body = &self.style().text_body;

        let cursor = match cursor {
            TextCursor::None => return TextCursor::None,
            TextCursor::Empty => {
                for span_i in 0..body.spans.len() {
                    for (byte_i, c) in body.spans[span_i].text.char_indices() {
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
            if !self.is_cursor_valid(cursor) {
                return TextCursor::None;
            }

            return TextCursor::Position(PosTextCursor {
                affinity: TextCursorAffinity::After,
                ..cursor
            });
        }

        if cursor.span >= body.spans.len()
            || cursor.byte_s >= body.spans[cursor.span].text.len()
            || cursor.byte_e > body.spans[cursor.span].text.len()
            || cursor.byte_e <= cursor.byte_s
        {
            return TextCursor::None;
        }

        let mut is_next = false;

        for span_i in cursor.span..body.spans.len() {
            if !is_next && span_i != cursor.span {
                return TextCursor::None;
            }

            for (byte_i, c) in body.spans[span_i].text.char_indices() {
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

    /// Get the [`TextCursor`](TextCursor) one line up from the provided [`TextCursor`](TextCursor).
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is `None`.
    /// - there isn't a valid cursor position above the one provided.
    pub fn cursor_up(&self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        if as_displayed {
            self.update_layout();
            self.state().cursor_up(cursor, &self.style().text_body)
        } else {
            let [mut line_i, mut col_i] = match self.cursor_line_column(cursor, false) {
                Some(some) => some,
                None => return TextCursor::None,
            };

            if line_i == 0 {
                return TextCursor::None;
            }

            line_i -= 1;

            let num_cols = match self.line_column_count(line_i, false) {
                Some(some) => some,
                None => unreachable!(),
            };

            col_i = col_i.min(num_cols);
            self.line_column_cursor(line_i, col_i, false)
        }
    }

    /// Get the [`TextCursor`](TextCursor) one line down from the provided [`TextCursor`](TextCursor).
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is `None`.
    /// - there isn't a valid cursor position below the one provided.
    pub fn cursor_down(&self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        if as_displayed {
            self.update_layout();
            self.state().cursor_down(cursor, &self.style().text_body)
        } else {
            let [mut line_i, mut col_i] = match self.cursor_line_column(cursor, false) {
                Some(some) => some,
                None => return TextCursor::None,
            };

            line_i += 1;

            let num_cols = match self.line_column_count(line_i, false) {
                Some(some) => some,
                None => return TextCursor::None,
            };

            col_i = col_i.min(num_cols);
            self.line_column_cursor(line_i, col_i, false)
        }
    }

    /// Insert a `char` after the provided [`TextCursor`](TextCursor).
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    pub fn cursor_insert(&self, cursor: TextCursor, c: char) -> TextCursor {
        match cursor {
            TextCursor::None => TextCursor::None,
            TextCursor::Empty => {
                let body = &mut self.style_mut().text_body;

                if body.spans.is_empty() {
                    body.spans.push(Default::default());
                }

                body.spans[0].text.insert(0, c);

                TextCursor::Position(PosTextCursor {
                    span: 0,
                    byte_s: 0,
                    byte_e: c.len_utf8(),
                    affinity: TextCursorAffinity::After,
                })
            },
            TextCursor::Position(mut cursor) => {
                if !self.is_cursor_valid(cursor) {
                    return TextCursor::None;
                }

                let body = &mut self.style_mut().text_body;

                if cursor.affinity == TextCursorAffinity::Before {
                    body.spans[cursor.span].text.insert(cursor.byte_s, c);
                    cursor.byte_e = cursor.byte_s + c.len_utf8();
                    cursor.affinity = TextCursorAffinity::After;
                    TextCursor::Position(cursor)
                } else {
                    body.spans[cursor.span].text.insert(cursor.byte_e, c);
                    cursor.byte_s = cursor.byte_e;
                    cursor.byte_e = cursor.byte_s + c.len_utf8();
                    TextCursor::Position(cursor)
                }
            },
        }
    }

    /// Insert a string after the provided [`TextCursor`](TextCursor).
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](`TextCursor::None`).
    pub fn cursor_insert_str<S>(&self, cursor: TextCursor, string: S) -> TextCursor
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
                let body = &mut self.style_mut().text_body;

                if body.spans.is_empty() {
                    body.spans.push(Default::default());
                }

                [0, 0]
            },
            TextCursor::Position(cursor) => {
                if !self.is_cursor_valid(cursor) {
                    return TextCursor::None;
                }

                match cursor.affinity {
                    TextCursorAffinity::Before => [cursor.span, cursor.byte_s],
                    TextCursorAffinity::After => [cursor.span, cursor.byte_e],
                }
            },
        };

        let body = &mut self.style_mut().text_body;
        body.spans[span_i].text.insert_str(byte_i, string);
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

    /// Insert a collection of [`TextSpan`](TextSpan)'s after the provided [`TextCursor`](TextCursor).
    ///
    /// **Note:** This will merge adjacent spans with the same attributes.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](`TextCursor::None`).
    pub fn cursor_insert_spans<S>(&self, cursor: TextCursor, spans: S) -> TextCursor
    where
        S: IntoIterator<Item = TextSpan>,
    {
        let [span_i, byte_i] = match cursor {
            TextCursor::None => return TextCursor::None,
            TextCursor::Empty => {
                let body = &mut self.style_mut().text_body;

                if body.spans.is_empty() {
                    body.spans.push(Default::default());
                }

                [0, 0]
            },
            TextCursor::Position(cursor) => {
                if !self.is_cursor_valid(cursor) {
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

        let body = &mut self.style_mut().text_body;
        let mut is_empty = true;

        for span in spans.into_iter() {
            if span.attrs == body.spans[pos_c.span].attrs {
                // Same attributes as the current span, so merge.

                body.spans[pos_c.span]
                    .text
                    .insert_str(pos_c.byte_e, span.text.as_str());
                pos_c.byte_e += span.text.len();
                is_empty = false;
                continue;
            }

            if pos_c.span + 1 < body.spans.len()
                && pos_c.byte_e == body.spans[pos_c.span].text.len()
                && span.attrs == body.spans[pos_c.span + 1].attrs
            {
                // There is a span following this one, the cursor is at the end of the current span
                // and the attributes are the same as the following span, so merge.

                body.spans[pos_c.span + 1]
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
            body.spans.insert(pos_c.span + 1, span);
            is_empty = false;
        }

        if is_empty { cursor } else { pos_c.into() }
    }

    /// Delete the `char` before the provided [`TextCursor`](TextCursor).
    ///
    /// **Note:** If deleletion empties the cursor's span the span will be removed.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is `None`.
    ///
    /// **Returns [`Empty`](`TextCursor::Empty`) if:**
    /// - the `TextBody` is empty after the deletion.
    pub fn cursor_delete(&self, cursor: TextCursor) -> TextCursor {
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

        {
            let body = &mut self.style_mut().text_body;
            body.spans[rm_cursor.span].text.remove(rm_cursor.byte_s);

            if body.spans[rm_cursor.span].text.is_empty() {
                body.spans.remove(rm_cursor.span);
            }
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

    /// Delete the word that the provided cursor is within.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](TextCursor::None) or [`Empty`](TextCursor::Empty).
    pub fn cursor_delete_word(&self, cursor: TextCursor) -> TextCursor {
        let selection = match self.cursor_select_word(cursor) {
            Some(some) => some,
            None => return TextCursor::None,
        };

        self.selection_delete(selection)
    }

    /// Delete the line that the provided [`TextCursor`](TextCursor) is within.
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    ///
    /// **Returns [`Empty`](TextCursor::Empty) if:**
    /// - the `TextBody` is empty after the deletion.
    pub fn cursor_delete_line(&self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        let selection = match self.cursor_select_line(cursor, as_displayed) {
            Some(selection) => selection,
            None => return TextCursor::None,
        };

        self.selection_delete(selection)
    }

    /// Delete the span that the provided [`TextCursor`](TextCursor) is within.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    ///
    /// **Returns [`Empty`](TextCursor::Empty) if:**
    /// - the `TextBody` is empty after the deletion.
    pub fn cursor_delete_span(&self, cursor: TextCursor) -> TextCursor {
        let selection = match self.cursor_select_span(cursor) {
            Some(selection) => selection,
            None => return TextCursor::None,
        };

        self.selection_delete(selection)
    }

    /// Get the [`TextCursor`](TextCursor) at the start of the word that the provided [`TextCursor`](TextCursor) is within.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_word_start(&self, cursor: TextCursor) -> TextCursor {
        match self.cursor_select_word(cursor) {
            Some(selection) => selection.start.into(),
            None => TextCursor::None,
        }
    }

    /// Get the [`TextCursor`](TextCursor) at the end of the word that the provided [`TextCursor`](TextCursor) is within.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_word_end(&self, cursor: TextCursor) -> TextCursor {
        match self.cursor_select_word(cursor) {
            Some(selection) => selection.end.into(),
            None => TextCursor::None,
        }
    }

    /// Get the [`TextCursor`](TextCursor) of the word that the provided [`TextCursor`](TextCursor) is within.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_select_word(&self, cursor: TextCursor) -> Option<TextSelection> {
        let body = &self.style().text_body;

        let cursor = match cursor {
            TextCursor::None | TextCursor::Empty => return None,
            TextCursor::Position(cursor) => {
                if !self.is_cursor_valid(cursor) {
                    return None;
                }

                cursor
            },
        };

        let mut spans_concat = String::new();
        let mut byte_map: BTreeMap<usize, [usize; 3]> = BTreeMap::new();

        for (span_i, span) in body.spans.iter().enumerate() {
            let offset = spans_concat.len();
            spans_concat.push_str(span.text.as_str());

            for (byte_i, c) in span.text.char_indices() {
                byte_map.insert(offset + byte_i, [span_i, byte_i, byte_i + c.len_utf8()]);
            }
        }

        let mut cursor_byte_i = cursor.byte_s;

        for span_i in 0..cursor.span {
            cursor_byte_i += body.spans[span_i].text.len();
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

    /// Get the [`TextCursor`](TextCursor) at the start of line of the provided [`TextCursor`](TextCursor).
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_line_start(&self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        if as_displayed {
            self.update_layout();

            let line_i = match self.state().cursor_line_column(cursor) {
                Some([line_i, _]) => line_i,
                None => return TextCursor::None,
            };

            match self.state().select_line(line_i) {
                Some(selection) => selection.start.into(),
                None => TextCursor::None,
            }
        } else {
            let cursor = match cursor {
                TextCursor::Empty | TextCursor::None => return TextCursor::None,
                TextCursor::Position(cursor) => cursor,
            };

            let body = &self.style().text_body;

            for span_i in (0..=cursor.span).rev() {
                for (byte_i, c) in body.spans[span_i].text.char_indices().rev() {
                    if span_i == cursor.span && byte_i > cursor.byte_s {
                        continue;
                    }

                    if c == '\n' {
                        return PosTextCursor {
                            span: span_i,
                            byte_s: byte_i,
                            byte_e: byte_i + c.len_utf8(),
                            affinity: TextCursorAffinity::After,
                        }
                        .into();
                    }
                }
            }

            for span_i in 0..=cursor.span {
                for (byte_i, c) in body.spans[span_i].text.char_indices() {
                    // It shouldn't be possible to get after the cursor.
                    debug_assert!(cursor.span != span_i || byte_i <= cursor.byte_s);

                    return PosTextCursor {
                        span: span_i,
                        byte_s: 0,
                        byte_e: c.len_utf8(),
                        affinity: TextCursorAffinity::Before,
                    }
                    .into();
                }
            }

            // The cursor is valid, so the text body isn't empty.
            unreachable!()
        }
    }

    /// Get the [`TextCursor`](TextCursor) at the end of line of the provided [`TextCursor`](TextCursor).
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_line_end(&self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        if as_displayed {
            self.update_layout();

            let line_i = match self.state().cursor_line_column(cursor) {
                Some([line_i, _]) => line_i,
                None => return TextCursor::None,
            };

            match self.state().select_line(line_i) {
                Some(selection) => selection.end.into(),
                None => TextCursor::None,
            }
        } else {
            let cursor = match cursor {
                TextCursor::Empty | TextCursor::None => return TextCursor::None,
                TextCursor::Position(cursor) => cursor,
            };

            let body = &self.style().text_body;

            for span_i in 0..=cursor.span {
                for (byte_i, c) in body.spans[span_i].text.char_indices() {
                    if span_i == cursor.span && byte_i < cursor.byte_s {
                        continue;
                    }

                    if c == '\n' {
                        return PosTextCursor {
                            span: span_i,
                            byte_s: byte_i,
                            byte_e: byte_i + c.len_utf8(),
                            affinity: TextCursorAffinity::Before,
                        }
                        .into();
                    }
                }
            }

            for span_i in (0..=cursor.span).rev() {
                for (byte_i, c) in body.spans[span_i].text.char_indices().rev() {
                    // It shouldn't be possible to get before the cursor.
                    debug_assert!(cursor.span != span_i || byte_i >= cursor.byte_s);

                    return PosTextCursor {
                        span: span_i,
                        byte_s: 0,
                        byte_e: c.len_utf8(),
                        affinity: TextCursorAffinity::After,
                    }
                    .into();
                }
            }

            // The cursor is valid, so the text body isn't empty.
            unreachable!()
        }
    }

    /// Get the [`TextSelection`](TextSelection) of the line that provided cursor is on.
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns `None` if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_select_line(
        &self,
        cursor: TextCursor,
        as_displayed: bool,
    ) -> Option<TextSelection> {
        if as_displayed {
            self.state().cursor_select_line(cursor)
        } else {
            let start = match self.cursor_line_start(cursor, false) {
                TextCursor::None => return None,
                TextCursor::Empty => unreachable!(),
                TextCursor::Position(cursor) => cursor,
            };

            let end = match self.cursor_line_start(cursor, false) {
                TextCursor::None => return None,
                TextCursor::Empty => unreachable!(),
                TextCursor::Position(cursor) => cursor,
            };

            Some(TextSelection {
                start,
                end,
            })
        }
    }

    /// Get the [`TextCursor`](`TextCursor`) at the start of the span that the provided [`TextCursor`](`TextCursor`) is in.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](TextCursor::None) or [`Empty`](TextCursor::Empty)
    pub fn cursor_span_start(&self, cursor: TextCursor) -> TextCursor {
        let body = &self.style().text_body;

        if !self.is_cursor_valid(cursor) {
            return TextCursor::None;
        }

        let cursor = match cursor {
            TextCursor::Empty | TextCursor::None => return TextCursor::None,
            TextCursor::Position(cursor) => cursor,
        };

        let byte_e = match body.spans[cursor.span].text.chars().next() {
            Some(c) => c.len_utf8(),
            None => {
                // Note: is_cursor_valid ensures that byte_e <= byte_s, therefore; there should
                //       be at least one character in this span.
                unreachable!()
            },
        };

        PosTextCursor {
            span: cursor.span,
            byte_s: 0,
            byte_e,
            affinity: TextCursorAffinity::Before,
        }
        .into()
    }

    /// Get the [`TextCursor`](`TextCursor`) at the end of the span that the provided [`TextCursor`](`TextCursor`) is in.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](TextCursor::None) or [`Empty`](TextCursor::Empty)
    pub fn cursor_span_end(&self, cursor: TextCursor) -> TextCursor {
        let body = &self.style().text_body;

        if !self.is_cursor_valid(cursor) {
            return TextCursor::None;
        }

        let cursor = match cursor {
            TextCursor::Empty | TextCursor::None => return TextCursor::None,
            TextCursor::Position(cursor) => cursor,
        };

        let [byte_s, byte_e] = match body.spans[cursor.span].text.char_indices().rev().next() {
            Some((byte_s, c)) => [byte_s, byte_s + c.len_utf8()],
            None => {
                // Note: is_cursor_valid ensures that byte_e <= byte_s, therefore; there should
                //       be at least one character in this span.
                unreachable!()
            },
        };

        PosTextCursor {
            span: cursor.span,
            byte_s,
            byte_e,
            affinity: TextCursorAffinity::Before,
        }
        .into()
    }

    /// Get a [`TextSelection`][TextSelection] of the span that the provided [`TextCursor`](TextCursor) is in.
    ///
    /// **Returns `None` if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`None`](TextCursor::None) or [`Empty`](TextCursor::Empty)
    pub fn cursor_select_span(&self, cursor: TextCursor) -> Option<TextSelection> {
        let start = match self.cursor_span_start(cursor) {
            TextCursor::Empty | TextCursor::None => return None,
            TextCursor::Position(cursor) => cursor,
        };

        let end = match self.cursor_span_end(cursor) {
            TextCursor::Empty | TextCursor::None => return None,
            TextCursor::Position(cursor) => cursor,
        };

        Some(TextSelection {
            start,
            end,
        })
    }

    /// Get the [`TextCursor`](TextCursor) at the start of line with the provided index.
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the text body is empty.
    /// - the line index is invalid.
    pub fn line_start(&self, line_i: usize, as_displayed: bool) -> TextCursor {
        if as_displayed {
            self.update_layout();

            match self.state().select_line(line_i) {
                Some(selection) => selection.start.into(),
                None => TextCursor::None,
            }
        } else {
            let body = &self.style().text_body;
            let mut cur_line_i = 0;

            for (span_i, span) in body.spans.iter().enumerate() {
                for (byte_i, c) in span.text.char_indices() {
                    if line_i == 0 {
                        return PosTextCursor {
                            span: span_i,
                            byte_s: byte_i,
                            byte_e: byte_i + c.len_utf8(),
                            affinity: TextCursorAffinity::Before,
                        }
                        .into();
                    }

                    if c == '\n' {
                        cur_line_i += 1;

                        if cur_line_i == line_i {
                            return PosTextCursor {
                                span: span_i,
                                byte_s: byte_i,
                                byte_e: byte_i + c.len_utf8(),
                                affinity: TextCursorAffinity::After,
                            }
                            .into();
                        }
                    }
                }
            }

            TextCursor::None
        }
    }

    /// Get the [`TextCursor`](TextCursor) at the end of line with the provided index.
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the text body is empty.
    /// - the line index is invalid.
    pub fn line_end(&self, line_i: usize, as_displayed: bool) -> TextCursor {
        if as_displayed {
            self.update_layout();

            match self.state().select_line(line_i) {
                Some(selection) => selection.end.into(),
                None => TextCursor::None,
            }
        } else {
            let body = &self.style().text_body;
            let mut cur_line_i = 0;

            for (span_i, span) in body.spans.iter().enumerate() {
                for (byte_i, c) in span.text.char_indices() {
                    if c == '\n' {
                        if cur_line_i == line_i {
                            return PosTextCursor {
                                span: span_i,
                                byte_s: byte_i,
                                byte_e: byte_i + c.len_utf8(),
                                affinity: TextCursorAffinity::Before,
                            }
                            .into();
                        }

                        cur_line_i += 1;
                    }
                }
            }

            TextCursor::None
        }
    }

    /// Obtain the bounding box of the displayed line with the provided index.
    ///
    /// Format: `[MIN_X, MAX_X, MIN_Y, MAX_Y]`.
    ///
    /// **Returns `None` if:**
    /// - the line index is invalid.
    pub fn line_bounds(&self, line_i: usize) -> Option<[f32; 4]> {
        self.update_layout();
        let tlwh = self.tlwh();
        self.state().line_bounds(tlwh, line_i)
    }

    /// Count the number of lines within the [`TextBody`](TextBody).
    ///
    /// **Returns `None` if:**
    /// - the text body is empty.
    pub fn line_count(&self, as_displayed: bool) -> Option<usize> {
        if as_displayed {
            self.update_layout();
            self.state().line_count()
        } else {
            if self.is_empty() {
                return None;
            }

            let body = &self.style().text_body;
            let mut count = 1;

            for span in body.spans.iter() {
                for c in span.text.chars() {
                    if c == '\n' {
                        count += 1;
                    }
                }
            }

            Some(count)
        }
    }

    /// Count the number of columns of the line with the provided index.
    ///
    /// **Returns `None` if:**
    /// - the text body is empty.
    /// - the provided line index is invalid.
    pub fn line_column_count(&self, line_i: usize, as_displayed: bool) -> Option<usize> {
        if as_displayed {
            self.update_layout();
            self.state().line_column_count(line_i)
        } else {
            let body = &self.style().text_body;
            let mut cur_line_i = 0;
            let mut count = 0;

            for span in body.spans.iter() {
                for c in span.text.chars() {
                    if c == '\n' {
                        if cur_line_i == line_i {
                            break;
                        } else {
                            cur_line_i += 1;
                        }
                    } else if cur_line_i == line_i {
                        count += 1;
                    }
                }
            }

            Some(count)
        }
    }

    /// Obtain a [`TextCursor`](TextCursor) given a line and column index.
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the text body is empty.
    /// - the line or column index is invalid.
    pub fn line_column_cursor(
        &self,
        line_i: usize,
        col_i: usize,
        as_displayed: bool,
    ) -> TextCursor {
        if as_displayed {
            self.update_layout();
            self.state().line_column_cursor(line_i, col_i)
        } else {
            if self.is_empty() {
                return TextCursor::None;
            }

            let body = &self.style().text_body;
            let mut cur_line_i = 0;
            let mut cur_col_i = 0;

            for (span_i, span) in body.spans.iter().enumerate() {
                for (byte_i, c) in span.text.char_indices() {
                    if cur_line_i == line_i && cur_col_i == col_i {
                        return PosTextCursor {
                            span: span_i,
                            byte_s: byte_i,
                            byte_e: byte_i + c.len_utf8(),
                            affinity: TextCursorAffinity::After,
                        }
                        .into();
                    }

                    if c == '\n' {
                        cur_line_i += 1;
                        cur_col_i = 0;
                    } else {
                        cur_col_i += 1;
                    }
                }
            }

            TextCursor::None
        }
    }

    /// Get the [`TextCursor`](`TextCursor`) at the start of the span with the provided index.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided span index is invalid.
    pub fn span_start(&self, span_i: usize) -> TextCursor {
        let body = &self.style().text_body;

        if span_i >= body.spans.len() {
            return TextCursor::None;
        }

        match body.spans[span_i].text.char_indices().next() {
            Some((byte_i, c)) => {
                PosTextCursor {
                    span: span_i,
                    byte_s: byte_i,
                    byte_e: byte_i + c.len_utf8(),
                    affinity: TextCursorAffinity::Before,
                }
                .into()
            },
            None => TextCursor::None,
        }
    }

    /// Get the [`TextCursor`](`TextCursor`) at the end of the span with the provided index.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided span index is invalid.
    pub fn span_end(&self, span_i: usize) -> TextCursor {
        let body = &self.style().text_body;

        if span_i >= body.spans.len() {
            return TextCursor::None;
        }

        match body.spans[span_i].text.char_indices().rev().next() {
            Some((byte_i, c)) => {
                PosTextCursor {
                    span: span_i,
                    byte_s: byte_i,
                    byte_e: byte_i + c.len_utf8(),
                    affinity: TextCursorAffinity::After,
                }
                .into()
            },
            None => TextCursor::None,
        }
    }

    /// Count the total number of spans.
    pub fn span_count(&self) -> usize {
        self.style().text_body.spans.len()
    }

    /// TODO: not impl
    pub fn span_set_attrs(&self, _span_i: usize, _attrs: TextAttrs) {
        todo!()
    }

    /// Obtain the current displayed [`TextSelection`](TextSelection).
    pub fn selection(&self) -> Option<TextSelection> {
        self.style().text_body.selection
    }

    /// Set the displayed [`TextSelection`](TextSelection).
    pub fn set_selection(&self, selection: TextSelection) {
        self.style_mut().text_body.selection = Some(selection);
    }

    /// Clear the displayed [`TextSelection`](TextSelection).
    pub fn clear_selection(&self) {
        self.style_mut().text_body.selection = None;
    }

    /// Set the [`Color`](Color) of the displayed [`TextSelection`](TextSelection).
    pub fn set_selection_color(&self, color: Color) {
        self.style_mut().text_body.selection_color = color;
    }

    /// Get the [`TextSelection`](TextSelection) of the line with the provided index.
    ///
    /// **Note:** When `as_displayed` is `true` wrapping is taken into account.
    pub fn select_line(&self, line_i: usize, as_displayed: bool) -> Option<TextSelection> {
        if as_displayed {
            self.update_layout();
            self.state().select_line(line_i)
        } else {
            let start = match self.line_start(line_i, false) {
                TextCursor::None => return None,
                TextCursor::Empty => unreachable!(),
                TextCursor::Position(cursor) => cursor,
            };

            let end = match self.line_end(line_i, false) {
                TextCursor::None => return None,
                TextCursor::Empty => unreachable!(),
                TextCursor::Position(cursor) => cursor,
            };

            Some(TextSelection {
                start,
                end,
            })
        }
    }

    /// Get the [`TextSelection`](TextSelection) of the [`TextSpan`](TextSpan) with the provided index.
    ///
    /// **Returns `None` if:**
    /// - the provided span index is invalid.
    pub fn select_span(&self, span_i: usize) -> Option<TextSelection> {
        let start = match self.span_start(span_i) {
            TextCursor::None => return None,
            TextCursor::Empty => unreachable!(),
            TextCursor::Position(cursor) => cursor,
        };

        let end = match self.span_end(span_i) {
            TextCursor::None => return None,
            TextCursor::Empty => unreachable!(),
            TextCursor::Position(cursor) => cursor,
        };

        Some(TextSelection {
            start,
            end,
        })
    }

    /// Get the [`TextSelection`](TextSelection) of the whole [`TextBody`](TextBody).
    ///
    /// **Returns `None` if:**
    /// - the text body is empty.
    pub fn select_all(&self) -> Option<TextSelection> {
        let span_count = self.span_count();

        if span_count == 0 {
            return None;
        }

        let start = match self.span_start(0) {
            TextCursor::None => return None,
            TextCursor::Empty => unreachable!(),
            TextCursor::Position(cursor) => cursor,
        };

        let end = match self.span_end(span_count - 1) {
            TextCursor::None => return None,
            TextCursor::Empty => unreachable!(),
            TextCursor::Position(cursor) => cursor,
        };

        Some(TextSelection {
            start,
            end,
        })
    }

    /// Obtain the selection's value as `String`.
    ///
    /// **Returns an empty `String` if:**
    /// - The provided [`TextSelection`](TextSelection) is invalid.
    pub fn selection_string(&self, selection: TextSelection) -> String {
        self.selection_spans(selection)
            .into_iter()
            .map(|span| span.text)
            .collect()
    }

    /// TODO: not impl
    pub fn selection_set_attrs(&self, _selection: TextSelection, _attrs: TextAttrs) {
        todo!()
    }

    /// Obtain the selection's value as [`Vec<TextSpan>`](TextSpan).
    ///
    /// **Returns an empty `Vec` if:**
    /// - The provided [`TextSelection`](TextSelection) is invalid.
    pub fn selection_spans(&self, selection: TextSelection) -> Vec<TextSpan> {
        let body = &self.style().text_body;

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

            let mut span_e_byte_op = if span_i == e_span {
                if e_byte == body.spans[span_i].text.len() {
                    None
                } else {
                    Some(e_byte)
                }
            } else {
                None
            };

            let mut sel_str = match span_s_byte_op {
                Some(span_s_byte) => {
                    if let Some(span_e_byte) = span_e_byte_op.as_mut() {
                        *span_e_byte -= span_s_byte;
                    }

                    body.spans[span_i].text.split_at(span_s_byte).1
                },
                None => body.spans[span_i].text.as_str(),
            };

            if let Some(span_e_byte) = span_e_byte_op {
                sel_str = sel_str.split_at(span_e_byte).0;
            }

            spans.push(TextSpan {
                attrs: body.spans[span_i].attrs.clone(),
                text: sel_str.into(),
                ..Default::default()
            });
        }

        spans
    }

    /// Take the selection out of the [`TextBody`](TextBody) returning the value as `String`.
    ///
    /// **Note:** The returned [`TextCursor`](TextCursor) behaves the same as
    /// [`selection_delete`](`TextBodyGuard::selection_delete`)
    ///
    /// **The returned `String` will be empty if:**
    /// -  the provided selection is invalid.
    pub fn selection_take_string(&self, selection: TextSelection) -> (TextCursor, String) {
        let (cursor, spans) = self.selection_take_spans(selection);
        (cursor, spans.into_iter().map(|span| span.text).collect())
    }

    /// Take the selection out of the [`TextBody`](TextBody) returning the value as [`Vec<TextSpan>`](TextSpan).
    ///
    /// **Note:** The returned [`TextCursor`](TextCursor) behaves the same as
    /// [`selection_delete`](`TextBodyGuard::selection_delete`)
    ///
    /// **The returned [`Vec<TextSpan>`](TextSpan) will be empty if:**
    /// -  the provided selection is invalid.
    pub fn selection_take_spans(&self, selection: TextSelection) -> (TextCursor, Vec<TextSpan>) {
        let mut spans = Vec::with_capacity(selection.end.span - selection.start.span + 1);
        let cursor = self.inner_selection_delete(selection, Some(&mut spans));
        (cursor, spans)
    }

    /// Delete the provided ['TextSelection`](TextSelection).
    ///
    /// **Note:** If deleletion empties any span within the selection, the span will be removed.
    ///
    /// **Returns [`None`](`TextCursor::None`) if:**
    /// - the provided selection is invalid.
    ///
    /// **Returns [`Empty`](`TextCursor::Empty`) if:**
    /// - the [`TextBody`](TextBody) is empty after the deletion.
    pub fn selection_delete(&self, selection: TextSelection) -> TextCursor {
        self.inner_selection_delete(selection, None)
    }

    fn inner_selection_delete(
        &self,
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

        {
            let body = &mut self.style_mut().text_body;
            let mut remove_spans = Vec::new();

            for span_i in s_span..=e_span {
                let span_s_byte = if span_i == s_span { s_byte } else { 0 };

                let span_e_byte = if span_i == e_span {
                    e_byte
                } else {
                    body.spans[span_i].text.len()
                };

                let text = body.spans[span_i]
                    .text
                    .drain(span_s_byte..span_e_byte)
                    .collect::<String>();

                if let Some(spans) = spans_op.as_mut() {
                    spans.push(TextSpan {
                        attrs: body.spans[span_i].attrs.clone(),
                        text,
                        ..Default::default()
                    });
                }

                if body.spans[span_i].text.is_empty() {
                    remove_spans.push(span_i);
                }
            }

            for span_i in remove_spans.into_iter().rev() {
                body.spans.remove(span_i);
            }
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

    fn update_layout(&self) {
        if self.style().layout_stale {
            let window = self.bin.window().expect("no associated window");

            {
                let style = self.style();
                let mut update_ctx = window.shared_update_ctx();

                let tlwh = self.bin.calc_placement(&mut update_ctx).tlwh;
                let padding_t = style.padding_t.px_height([tlwh[2], tlwh[3]]).unwrap_or(0.0);
                let padding_b = style.padding_b.px_height([tlwh[2], tlwh[3]]).unwrap_or(0.0);
                let padding_l = style.padding_l.px_width([tlwh[2], tlwh[3]]).unwrap_or(0.0);
                let padding_r = style.padding_r.px_width([tlwh[2], tlwh[3]]).unwrap_or(0.0);

                let content_tlwh = [
                    tlwh[0] + padding_t - style.scroll_y,
                    tlwh[1] + padding_l + style.scroll_x,
                    tlwh[2] + padding_l - padding_r,
                    tlwh[3] + padding_t - padding_b,
                ];

                let image_cache = window.basalt_ref().image_cache_ref();

                self.state()
                    .update(content_tlwh, &style.text_body, &mut update_ctx, image_cache);

                *self.tlwh.borrow_mut() = Some(content_tlwh);
                *self.default_font.borrow_mut() = Some(update_ctx.default_font.clone());
            }

            self.style_mut().layout_stale = false;
        }
    }

    /// Inspect the inner [`TextBody`](TextBody) with the provided method.
    ///
    /// **Warning:** A deadlock may occur if modifications are made to this
    /// [`TextBodyGuard`](TextBodyGuard) or the parent [`Bin`](Bin) with the provided method.
    pub fn inspect<I, T>(&self, inspect: I) -> T
    where
        I: FnOnce(&TextBody) -> T,
    {
        inspect(&self.style().text_body)
    }

    /// Modify the inner [`TextBody`](TextBody) with the provided method.
    ///
    /// **Warning:** A deadlock may occur if modifications are made to this
    /// [`TextBodyGuard`](TextBodyGuard) or the parent [`Bin`](Bin) with the provided method.
    pub fn modify<M, T>(&self, modify: M) -> T
    where
        M: FnOnce(&mut TextBody) -> T,
    {
        modify(&mut self.style_mut().text_body)
    }

    /// Modify the [`TextBody`](TextBody) parent ['BinStyle`](BinStyle).
    ///
    /// **Warning:** A deadlock may occur if modifications are made to this
    /// [`TextBodyGuard`](TextBodyGuard) or the parent [`Bin`](Bin) with the provided method.
    #[track_caller]
    pub fn style_modify<M, T>(&self, modify: M) -> Result<T, BinStyleValidation>
    where
        M: FnOnce(&mut BinStyle) -> T,
    {
        let mut style = self.style().clone();
        let user_ret = modify(&mut style);
        let validation = style.validate(self.bin);

        if validation.errors_present() {
            return Err(validation);
        }

        **self.style_mut() = style;
        Ok(user_ret)
    }

    /// Modify the [`TextBody`](TextBody) parent ['BinStyle`](BinStyle).
    ///
    /// **Warning:** A deadlock may occur if modifications are made to this
    /// [`TextBodyGuard`](TextBodyGuard) or the parent [`Bin`](Bin) with the provided method.
    pub fn style_inspect<I, T>(&self, inspect: I) -> T
    where
        I: FnOnce(&BinStyle) -> T,
    {
        inspect(&self.style())
    }

    /// This is equivlent to [`Bin::on_update_once`](Bin::on_update_once).
    ///
    /// Useful when having an up-to-date [`BinPostUpdate`](BinPostUpdate) is needed.
    ///
    /// Method is called at the end of a ui update cycle when everything is up-to-date.
    ///
    /// **Note:** If no modifications are made, the provided method won't be called.
    pub fn bin_on_update<U>(&self, updated: U)
    where
        U: FnOnce(&Arc<Bin>, &BinPostUpdate) + Send + 'static,
    {
        self.on_update.borrow_mut().push(Box::new(updated));
    }

    /// Finish modifications.
    ///
    /// **Note:** This is automatically called when [`TextBodyGuard`](TextBodyGuard) is dropped.
    #[track_caller]
    pub fn finish(self) {
        self.finish_inner();
    }

    #[track_caller]
    fn finish_inner(&self) {
        let StyleState {
            guard: style_guard,
            modified: modified_style_op,
            ..
        } = match self.style_state.borrow_mut().take() {
            Some(style_state) => style_state,
            None => return,
        };

        let modified_style = match modified_style_op {
            Some(modified_style) => modified_style,
            None => return,
        };

        modified_style.validate(self.bin).expect_valid();
        let mut effects_siblings = modified_style.position == Position::Floating;
        let mut old_style = Arc::new(modified_style);

        {
            let mut style_guard = RwLockUpgradableReadGuard::upgrade(style_guard);
            std::mem::swap(&mut *style_guard, &mut old_style);
            effects_siblings |= old_style.position == Position::Floating;
        }

        {
            let mut internal_hooks = self.bin.internal_hooks.lock();

            let on_update_once = internal_hooks
                .get_mut(&InternalHookTy::UpdatedOnce)
                .unwrap();

            for updated in self.on_update.borrow_mut().drain(..) {
                on_update_once.push(InternalHookFn::UpdatedOnce(updated));
            }
        }

        if effects_siblings && let Some(parent) = self.bin.parent() {
            parent.trigger_children_update();
        } else {
            self.bin.trigger_recursive_update();
        }
    }

    pub(crate) fn new(bin: &'a Arc<Bin>) -> Self {
        Self {
            bin,
            text_state: RefCell::new(None),
            style_state: RefCell::new(None),
            tlwh: RefCell::new(None),
            default_font: RefCell::new(None),
            on_update: RefCell::new(Vec::new()),
        }
    }

    #[track_caller]
    fn state<'b>(&'b self) -> SomeRefMut<'b, TextStateGuard<'a>> {
        if self.text_state.borrow().is_none() {
            *self.text_state.borrow_mut() = Some(TextStateGuard {
                inner: self.bin.update_state.lock(),
            });
        }

        SomeRefMut {
            inner: self.text_state.borrow_mut(),
        }
    }

    #[track_caller]
    fn style(&self) -> SomeRef<'_, StyleState<'_>> {
        if self.style_state.borrow().is_none() {
            *self.style_state.borrow_mut() = Some(StyleState {
                guard: self.bin.style.upgradable_read(),
                modified: None,
                layout_stale: false,
            });
        }

        SomeRef {
            inner: self.style_state.borrow(),
        }
    }

    #[track_caller]
    fn style_mut<'b>(&'b self) -> SomeRefMut<'b, StyleState<'a>> {
        if self.style_state.borrow().is_none() {
            *self.style_state.borrow_mut() = Some(StyleState {
                guard: self.bin.style.upgradable_read(),
                modified: None,
                layout_stale: true,
            });
        }

        let mut style_state = self.style_state.borrow_mut();
        style_state.as_mut().unwrap().layout_stale = true;

        SomeRefMut {
            inner: style_state,
        }
    }

    fn tlwh(&self) -> [f32; 4] {
        if self.tlwh.borrow().is_none() {
            let bpu = self.bin.post_update.read_recursive();

            *self.tlwh.borrow_mut() = Some([
                bpu.optimal_content_bounds[2] + bpu.content_offset[1],
                bpu.optimal_content_bounds[0] + bpu.content_offset[0],
                bpu.optimal_content_bounds[1] - bpu.optimal_content_bounds[0],
                bpu.optimal_content_bounds[3] - bpu.optimal_content_bounds[2],
            ]);
        }

        self.tlwh.borrow().unwrap()
    }

    fn default_font(&self) -> SomeRef<'_, DefaultFont> {
        if self.default_font.borrow().is_none() {
            *self.default_font.borrow_mut() =
                Some(self.bin.basalt_ref().interface_ref().default_font());
        }

        SomeRef {
            inner: self.default_font.borrow(),
        }
    }
}

impl<'a> Drop for TextBodyGuard<'a> {
    #[track_caller]
    fn drop(&mut self) {
        self.finish_inner();
    }
}

struct SomeRef<'a, T: Sized + 'a> {
    inner: Ref<'a, Option<T>>,
}

impl<T> Deref for SomeRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        (*self.inner).as_ref().unwrap()
    }
}

struct SomeRefMut<'a, T: Sized + 'a> {
    inner: RefMut<'a, Option<T>>,
}

impl<T> Deref for SomeRefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        (*self.inner).as_ref().unwrap()
    }
}

impl<T> DerefMut for SomeRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        (*self.inner).as_mut().unwrap()
    }
}

struct StyleState<'a> {
    guard: RwLockUpgradableReadGuard<'a, Arc<BinStyle>>,
    modified: Option<BinStyle>,
    layout_stale: bool,
}

impl Deref for StyleState<'_> {
    type Target = BinStyle;

    fn deref(&self) -> &BinStyle {
        if let Some(modified) = self.modified.as_ref() {
            return modified;
        }

        &**self.guard
    }
}

impl DerefMut for StyleState<'_> {
    fn deref_mut(&mut self) -> &mut BinStyle {
        if self.modified.is_none() {
            self.modified = Some((**self.guard).clone());
        }

        self.modified.as_mut().unwrap()
    }
}

struct TextStateGuard<'a> {
    inner: MutexGuard<'a, UpdateState>,
}

impl Deref for TextStateGuard<'_> {
    type Target = TextState;

    fn deref(&self) -> &TextState {
        &self.inner.text
    }
}

impl DerefMut for TextStateGuard<'_> {
    fn deref_mut(&mut self) -> &mut TextState {
        &mut self.inner.text
    }
}
