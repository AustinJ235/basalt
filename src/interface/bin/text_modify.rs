#![allow(warnings)]

use std::cell::{Ref, RefCell, RefMut};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use parking_lot::{MutexGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};

use crate::interface::bin::{TextState, UpdateState};
use crate::interface::{Bin, BinStyle, DefaultFont, TextCursor, TextSelection, TextSpan};

pub struct TextBodyGuard<'a> {
    bin: &'a Arc<Bin>,
    text_state: RefCell<Option<TextStateGuard<'a>>>,
    style_state: RefCell<Option<StyleState<'a>>>,
    tlwh: RefCell<Option<[f32; 4]>>,
    default_font: RefCell<Option<DefaultFont>>,
}

impl<'a> TextBodyGuard<'a> {
    pub(crate) fn new(bin: &'a Arc<Bin>) -> Self {
        Self {
            bin,
            text_state: RefCell::new(None),
            style_state: RefCell::new(None),
            tlwh: RefCell::new(None),
            default_font: RefCell::new(None),
        }
    }

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

    fn style(&self) -> SomeRef<StyleState> {
        if self.style_state.borrow().is_none() {
            *self.style_state.borrow_mut() = Some(StyleState {
                guard: self.bin.style.upgradable_read(),
                modified: None,
            });
        }

        SomeRef {
            inner: self.style_state.borrow(),
        }
    }

    fn style_mut<'b>(&'b self) -> SomeRefMut<'b, StyleState<'a>> {
        if self.style_state.borrow().is_none() {
            *self.style_state.borrow_mut() = Some(StyleState {
                guard: self.bin.style.upgradable_read(),
                modified: None,
            });
        }

        SomeRefMut {
            inner: self.style_state.borrow_mut(),
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

    fn default_font(&self) -> SomeRef<DefaultFont> {
        if self.default_font.borrow().is_none() {
            *self.default_font.borrow_mut() =
                Some(self.bin.basalt_ref().interface_ref().default_font());
        }

        SomeRef {
            inner: self.default_font.borrow(),
        }
    }

    pub fn cursor(&self) -> TextCursor {
        self.style().text_body.cursor
    }

    pub fn set_cursor(&self, cursor: TextCursor) {
        self.style_mut().text_body.cursor = cursor;
    }

    pub fn get_cursor(&self, mut position: [f32; 2]) -> TextCursor {
        let tlwh = self.tlwh();
        position[0] -= tlwh[1];
        position[1] -= tlwh[0];
        self.state().get_cursor(position)
    }

    pub fn cursor_bounds(&self, cursor: TextCursor) -> Option<[f32; 4]> {
        let tlwh = self.tlwh();
        let default_font = self.default_font();
        let style = self.style();
        self.state()
            .get_cursor_bounds(cursor, tlwh, &style.text_body, default_font.height)
            .map(|(bounds, _)| bounds)
    }

    pub fn cursor_prev(&self, cursor: TextCursor) -> TextCursor {
        self.style().text_body.cursor_prev(cursor)
    }

    pub fn cursor_next(&self, cursor: TextCursor) -> TextCursor {
        self.style().text_body.cursor_next(cursor)
    }

    pub fn cursor_up(&self, cursor: TextCursor) -> TextCursor {
        self.state().cursor_up(cursor, &self.style().text_body)
    }

    pub fn cursor_down(&self, cursor: TextCursor) -> TextCursor {
        self.state().cursor_down(cursor, &self.style().text_body)
    }

    pub fn cursor_insert(&self, cursor: TextCursor, c: char) -> TextCursor {
        self.style_mut().text_body.cursor_insert(cursor, c)
    }

    pub fn cursor_insert_str<S>(&self, cursor: TextCursor, string: S) -> TextCursor
    where
        S: AsRef<str>,
    {
        self.style_mut()
            .text_body
            .cursor_insert_string(cursor, string)
    }

    pub fn cursor_insert_spans<S>(&self, cursor: TextCursor, spans: S) -> TextCursor
    where
        S: IntoIterator<Item = TextSpan>,
    {
        self.style_mut()
            .text_body
            .cursor_insert_spans(cursor, spans)
    }

    pub fn cursor_delete(&self, cursor: TextCursor) -> TextCursor {
        self.style_mut().text_body.cursor_delete(cursor)
    }

    pub fn cursor_delete_word(&self, cursor: TextCursor) -> TextCursor {
        let selection = match self.cursor_select_word(cursor) {
            Some(some) => some,
            None => return TextCursor::None,
        };

        self.selection_delete(selection)
    }

    pub fn cursor_delete_line(&self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        todo!()
    }

    pub fn cursor_delete_span(&self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_word_start(&self, cursor: TextCursor) -> TextCursor {
        match self.cursor_select_word(cursor) {
            Some(selection) => selection.start.into(),
            None => TextCursor::None,
        }
    }

    pub fn cursor_word_end(&self, cursor: TextCursor) -> TextCursor {
        match self.cursor_select_word(cursor) {
            Some(selection) => selection.end.into(),
            None => TextCursor::None,
        }
    }

    pub fn cursor_select_word(&self, cursor: TextCursor) -> Option<TextSelection> {
        self.style().text_body.select_word(cursor)
    }

    pub fn cursor_line_start(&self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        todo!()
    }

    pub fn cursor_line_end(&self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        todo!()
    }

    pub fn cursor_select_line(
        &self,
        cursor: TextCursor,
        as_displayed: bool,
    ) -> Option<TextSelection> {
        todo!()
    }

    pub fn cursor_span_start(&self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_span_end(&self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_select_span(&self, cursor: TextCursor) -> Option<TextSelection> {
        todo!()
    }

    pub fn selection(&self) -> Option<TextSelection> {
        self.style().text_body.selection
    }

    pub fn set_selection(&self, selection: TextSelection) {
        self.style_mut().text_body.selection = Some(selection);
    }

    pub fn clear_selection(&self) {
        self.style_mut().text_body.selection = None;
    }

    pub fn select_line(&self, line_i: usize, as_displayed: bool) -> Option<TextSelection> {
        todo!()
    }

    pub fn select_span(&self, span_i: usize) -> Option<TextSelection> {
        todo!()
    }

    pub fn select_all(&self) -> Option<TextSelection> {
        todo!()
    }

    pub fn selection_string(&self, selection: TextSelection) -> String {
        self.style().text_body.selection_string(selection)
    }

    pub fn selection_spans(&self, selection: TextSelection) -> Vec<TextSpan> {
        self.style().text_body.selection_spans(selection)
    }

    pub fn selection_take(&self, selection: TextSelection) -> (TextCursor, Vec<TextSpan>) {
        self.style_mut().text_body.selection_take_spans(selection)
    }

    pub fn selection_delete(&self, selection: TextSelection) -> TextCursor {
        self.style_mut().text_body.selection_delete(selection)
    }

    #[track_caller]
    pub fn finish(self) {
        self.finish_inner();
    }

    #[track_caller]
    fn finish_inner(&self) {
        if let Some(style_state) = self.style_state.borrow_mut().take() {
            if let Some(modified_style) = style_state.modified {
                self.bin.style_update(modified_style).expect_valid();
            }
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
