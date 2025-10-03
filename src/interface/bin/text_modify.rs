use std::cell::{Ref, RefCell, RefMut};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use parking_lot::{MutexGuard, RwLockUpgradableReadGuard};
use unicode_segmentation::UnicodeSegmentation;

use crate::interface::bin::{InternalHookFn, InternalHookTy, TextState, UpdateState};
use crate::interface::{
    Bin, BinPostUpdate, BinStyle, BinStyleValidation, Color, DefaultFont, PosTextCursor, Position,
    TextAttrs, TextAttrsMask, TextBody, TextCursor, TextCursorAffinity, TextSelection, TextSpan,
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

    /// Check if the provided [`TextCursor`](TextCursor)'s are equivalent.
    ///
    /// **Note:** will return `false` if either of the provided cursors are invalid.
    pub fn are_cursors_equivalent(&self, a: TextCursor, b: TextCursor) -> bool {
        if !self.is_cursor_valid(a) || !self.is_cursor_valid(b) {
            return false;
        }

        if a == b {
            return true;
        }

        let a = match a {
            TextCursor::Empty | TextCursor::None => return false,
            TextCursor::Position(cursor) => cursor,
        };

        let b = match b {
            TextCursor::Empty | TextCursor::None => return false,
            TextCursor::Position(cursor) => cursor,
        };

        if a.affinity == b.affinity {
            // The affinities must be different
            return false;
        }

        let body = &self.style().text_body;

        match a.affinity {
            TextCursorAffinity::Before => {
                // B is after the character before A
                if a.byte_s == 0 {
                    // A is at the start of the span
                    if a.span == 0 || b.span != a.span - 1 {
                        // B must be in the previous span
                        false
                    } else {
                        // B must be at the end of the previous span.
                        b.byte_e == body.spans[b.span].text.len()
                    }
                } else {
                    // A isn't at the start of the span
                    if a.span != b.span {
                        // B must be within the same span
                        false
                    } else {
                        // A's byte_s should equal B's byte_e
                        a.byte_s == b.byte_e
                    }
                }
            },
            TextCursorAffinity::After => {
                // Same as above, but A/B are reversed.
                if b.byte_s == 0 {
                    if b.span == 0 || a.span != b.span - 1 {
                        false
                    } else {
                        a.byte_e == body.spans[a.span].text.len()
                    }
                } else {
                    if b.span != a.span {
                        false
                    } else {
                        b.byte_s == a.byte_e
                    }
                }
            },
        }
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
        if !self.is_cursor_valid(cursor) {
            return TextCursor::None;
        }

        let cursor = match cursor {
            TextCursor::None | TextCursor::Empty => return TextCursor::None,
            TextCursor::Position(cursor) => cursor,
        };

        let body = &self.style().text_body;
        let word_ranges = word_ranges(body, true);

        for range_i in (0..word_ranges.len()).rev() {
            if cursor > word_ranges[range_i].start
                || self.are_cursors_equivalent(cursor.into(), word_ranges[range_i].start.into())
            {
                return word_ranges[range_i].start.into();
            }
        }

        self.span_start(0)
    }

    /// Get the [`TextCursor`](TextCursor) at the end of the word that the provided [`TextCursor`](TextCursor) is within.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_word_end(&self, cursor: TextCursor) -> TextCursor {
        if !self.is_cursor_valid(cursor) {
            return TextCursor::None;
        }

        let cursor = match cursor {
            TextCursor::None | TextCursor::Empty => return TextCursor::None,
            TextCursor::Position(cursor) => cursor,
        };

        let body = &self.style().text_body;
        let word_ranges = word_ranges(body, true);

        for range_i in 0..word_ranges.len() {
            if cursor < word_ranges[range_i].end
                || self.are_cursors_equivalent(cursor.into(), word_ranges[range_i].end.into())
            {
                return word_ranges[range_i].end.into();
            }
        }

        if body.spans.is_empty() {
            TextCursor::None
        } else {
            self.span_end(body.spans.len() - 1)
        }
    }

    /// Get the [`TextCursor`](TextCursor) of the word that the provided [`TextCursor`](TextCursor) is within.
    ///
    /// **Returns [`None`](TextCursor::None) if:**
    /// - the provided cursor is invalid.
    /// - the provided cursor is [`Empty`](TextCursor::Empty) or [`None`](TextCursor::None).
    pub fn cursor_select_word(&self, cursor: TextCursor) -> Option<TextSelection> {
        if !self.is_cursor_valid(cursor) {
            return None;
        }

        let cursor = match cursor {
            TextCursor::None | TextCursor::Empty => return None,
            TextCursor::Position(cursor) => cursor,
        };

        let body = &self.style().text_body;
        let word_ranges = word_ranges(body, false);

        for range_i in 0..word_ranges.len() {
            if word_ranges[range_i].is_whitespace {
                if range_i == 0 {
                    if word_ranges.len() == 1 || cursor < word_ranges[range_i].end {
                        return Some(TextSelection {
                            start: word_ranges[range_i].start,
                            end: word_ranges[range_i].end,
                        });
                    }
                } else if range_i == word_ranges.len() - 1 {
                    if word_ranges.len() == 1 || cursor > word_ranges[range_i].start {
                        return Some(TextSelection {
                            start: word_ranges[range_i].start,
                            end: word_ranges[range_i].end,
                        });
                    }
                } else {
                    if cursor > word_ranges[range_i].start && cursor < word_ranges[range_i].end {
                        return Some(TextSelection {
                            start: word_ranges[range_i].start,
                            end: word_ranges[range_i].end,
                        });
                    }
                }
            } else {
                if (cursor > word_ranges[range_i].start
                    || self
                        .are_cursors_equivalent(cursor.into(), word_ranges[range_i].start.into()))
                    && (cursor < word_ranges[range_i].end
                        || self
                            .are_cursors_equivalent(cursor.into(), word_ranges[range_i].end.into()))
                {
                    return Some(TextSelection {
                        start: word_ranges[range_i].start,
                        end: word_ranges[range_i].end,
                    });
                }
            }
        }

        // NOTE: Since whitespace isn't ignored, word_ranges *should* include everything, but, for
        //       the sake of robustness, return None instead.
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

    /// Set the [`TextAttrs`] of the span with the provided index.
    ///
    /// `mask` controls which attributes are set from the provided attrs.
    ///
    /// If `consolidate` is `true`, spans with the same attributes will be merged.
    ///
    /// If `preserve_cursors` is `true`, the current cursor & selection will be kept valid.
    ///
    /// **Note:** This is a no-op if that span's index is invalid.
    pub fn span_apply_attrs(
        &self,
        span_i: usize,
        attrs: &TextAttrs,
        mask: TextAttrsMask,
        consolidate: bool,
        preserve_cursors: bool,
    ) {
        if span_i >= self.style().text_body.spans.len() {
            return;
        }

        let preserve_cursors = PreserveCursors::new(self, preserve_cursors);
        self.cursors_invalidated();

        {
            let body = &mut self.style_mut().text_body;
            mask.apply(attrs, &mut body.spans[span_i].attrs);
        }

        if consolidate {
            self.spans_consolidate();
        }

        preserve_cursors.restore(self);
    }

    /// Consolidate spans that share [`TextAttrs`].
    pub fn spans_consolidate(&self) {
        if self.style().text_body.spans.len() < 2 {
            return;
        }

        let body = &mut self.style_mut().text_body;

        for span_i in (1..body.spans.len()).rev() {
            if body.spans[span_i - 1].attrs == body.spans[span_i].attrs {
                let span = body.spans.remove(span_i);
                body.spans[span_i - 1].text.push_str(&span.text);
            }
        }
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

    /// Set the [`TextAttrs`] of the provided [`TextSelection`].
    ///
    /// `mask` controls which attributes are set from the provided attrs.
    ///
    /// If `consolidate` is `true`, spans with the same attributes will be merged.
    ///
    /// If `preserve_cursors` is `true`, the current cursor & selection will be kept valid.
    ///
    /// **Note:** This is a no-op if the provided [`TextSelection`] is invalid.
    pub fn selection_apply_attrs(
        &self,
        selection: TextSelection,
        attrs: &TextAttrs,
        mask: TextAttrsMask,
        consolidate: bool,
        preserve_cursors: bool,
    ) {
        if !self.is_selection_valid(selection) {
            return;
        }

        let preserve_cursors = PreserveCursors::new(self, preserve_cursors);
        self.cursors_invalidated();

        {
            let body = &mut self.style_mut().text_body;

            for span_i in (selection.start.span..=selection.end.span).rev() {
                let mut new_attrs = body.spans[span_i].attrs.clone();
                mask.apply(attrs, &mut new_attrs);

                if new_attrs == body.spans[span_i].attrs {
                    continue;
                }

                if selection.start.span == selection.end.span {
                    if selection.start.byte_s == 0
                        && selection.end.byte_e == body.spans[span_i].text.len()
                    {
                        mask.apply(attrs, &mut body.spans[span_i].attrs);
                    } else {
                        if selection.start.byte_s == 0 {
                            let text = body.spans[span_i].text.split_off(selection.end.byte_s);

                            body.spans.insert(
                                span_i + 1,
                                TextSpan {
                                    text,
                                    attrs: new_attrs,
                                    ..Default::default()
                                },
                            );
                        } else if selection.end.byte_e == body.spans[span_i].text.len() {
                            let text = body.spans[span_i].text.split_off(selection.end.byte_s);
                            let attrs = body.spans[span_i].attrs.clone();

                            body.spans.insert(
                                span_i + 1,
                                TextSpan {
                                    text,
                                    attrs,
                                    ..Default::default()
                                },
                            );

                            body.spans[span_i].attrs = new_attrs;
                        } else {
                            let t_before =
                                body.spans[span_i].text.split_off(selection.start.byte_s);

                            let t_after = body.spans[span_i]
                                .text
                                .split_off(selection.end.byte_s - selection.start.byte_s);

                            let attrs = body.spans[span_i].attrs.clone();

                            body.spans.insert(
                                span_i,
                                TextSpan {
                                    text: t_before,
                                    attrs: attrs.clone(),
                                    ..Default::default()
                                },
                            );

                            body.spans.insert(
                                span_i + 2,
                                TextSpan {
                                    text: t_after,
                                    attrs,
                                    ..Default::default()
                                },
                            );

                            body.spans[span_i + 1].attrs = new_attrs;
                        }
                    }
                } else if span_i == selection.start.span {
                    if selection.start.byte_s == 0 {
                        mask.apply(attrs, &mut body.spans[span_i].attrs);
                    } else {
                        let text = body.spans[span_i].text.split_off(selection.start.byte_s);

                        body.spans.insert(
                            span_i + 1,
                            TextSpan {
                                text,
                                attrs: new_attrs,
                                ..Default::default()
                            },
                        );
                    }
                } else if span_i == selection.end.span {
                    if selection.end.byte_e == body.spans[span_i].text.len() {
                        mask.apply(attrs, &mut body.spans[span_i].attrs);
                    } else {
                        let text = body.spans[span_i].text.split_off(selection.end.byte_s);
                        let attrs = body.spans[span_i].attrs.clone();

                        body.spans.insert(
                            span_i + 1,
                            TextSpan {
                                text,
                                attrs,
                                ..Default::default()
                            },
                        );

                        body.spans[span_i].attrs = new_attrs;
                    }
                }
            }
        }

        if consolidate {
            self.spans_consolidate();
        }

        preserve_cursors.restore(self);
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
                    tlwh[2] - padding_r - padding_l,
                    tlwh[3] - padding_b - padding_t,
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

    fn cursors_invalidated(&self) {
        if matches!(self.cursor(), TextCursor::Position(..)) {
            self.set_cursor(TextCursor::None);
        }

        self.clear_selection();
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

struct WordRange {
    start: PosTextCursor,
    end: PosTextCursor,
    is_whitespace: bool,
}

// TODO: In the future this should be removed and replaced with some means of lazily checking words.
//       Checking the entire body's words is unnecessary and slow.
fn word_ranges(body: &TextBody, ignore_whitespace: bool) -> Vec<WordRange> {
    let mut text = String::new();
    let mut span_ranges = Vec::new();

    for span in body.spans.iter() {
        let start = text.len();
        text.push_str(span.text.as_str());
        span_ranges.push(start..text.len());
    }

    let mut word_ranges = Vec::new();

    for (word_offset, word) in text.split_word_bound_indices() {
        let is_whitespace = word.chars().all(|c| c.is_whitespace());

        if ignore_whitespace && is_whitespace {
            continue;
        }

        let (s_span_i, s_span_o) = span_ranges
            .iter()
            .enumerate()
            .find_map(|(span_i, range)| {
                if range.contains(&word_offset) {
                    Some((span_i, range.start))
                } else {
                    None
                }
            })
            .unwrap();

        let (e_span_i, e_span_o) = span_ranges
            .iter()
            .enumerate()
            .find_map(|(span_i, range)| {
                if range.contains(&word_offset) {
                    Some((span_i, range.start))
                } else {
                    None
                }
            })
            .unwrap();

        let s_byte_s = word_offset - s_span_o;
        let s_byte_e = s_byte_s + word.chars().next().unwrap().len_utf8();
        let e_byte_e = (word_offset + word.len()) - e_span_o;
        let e_byte_s = e_byte_e - word.chars().rev().next().unwrap().len_utf8();

        word_ranges.push(WordRange {
            start: PosTextCursor {
                span: s_span_i,
                byte_s: s_byte_s,
                byte_e: s_byte_e,
                affinity: TextCursorAffinity::Before,
            },
            end: PosTextCursor {
                span: e_span_i,
                byte_s: e_byte_s,
                byte_e: e_byte_e,
                affinity: TextCursorAffinity::After,
            },
            is_whitespace,
        });
    }

    word_ranges
}

struct PreserveCursors {
    cursor_lc: Option<[usize; 2]>,
    selection_lc: Option<[usize; 4]>,
}

impl PreserveCursors {
    fn new(tbg: &TextBodyGuard, preserve: bool) -> Self {
        if preserve {
            let cursor_lc = match tbg.cursor() {
                TextCursor::Empty | TextCursor::None => None,
                TextCursor::Position(cursor) => {
                    if tbg.is_cursor_valid(cursor) {
                        Some(tbg.cursor_line_column(cursor.into(), false).unwrap())
                    } else {
                        tbg.set_cursor(TextCursor::None);
                        None
                    }
                },
            };

            let selection_lc = match tbg.selection() {
                Some(selection) => {
                    if tbg.is_selection_valid(selection) {
                        let [s_line_i, s_col_i] = tbg
                            .cursor_line_column(selection.start.into(), false)
                            .unwrap();
                        let [e_line_i, e_col_i] =
                            tbg.cursor_line_column(selection.end.into(), false).unwrap();
                        Some([s_line_i, s_col_i, e_line_i, e_col_i])
                    } else {
                        tbg.clear_selection();
                        None
                    }
                },
                None => None,
            };

            Self {
                cursor_lc,
                selection_lc,
            }
        } else {
            Self {
                cursor_lc: None,
                selection_lc: None,
            }
        }
    }

    fn restore(self, tbg: &TextBodyGuard) {
        if let Some([line_i, col_i]) = self.cursor_lc {
            tbg.set_cursor(tbg.line_column_cursor(line_i, col_i, false));
        }

        if let Some([s_line_i, s_col_i, e_line_i, e_col_i]) = self.selection_lc {
            tbg.set_selection(TextSelection {
                start: tbg
                    .line_column_cursor(s_line_i, s_col_i, false)
                    .into_position()
                    .unwrap(),
                end: tbg
                    .line_column_cursor(e_line_i, e_col_i, false)
                    .into_position()
                    .unwrap(),
            });
        }
    }
}
