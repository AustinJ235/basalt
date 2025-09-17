#![allow(warnings)]

use std::sync::Arc;

use parking_lot::{MutexGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};

use crate::interface::bin::{TextState, UpdateState};
use crate::interface::{Bin, BinStyle, TextCursor, TextSelection, TextSpan};

pub struct TextBodyGuard<'a> {
    parent: &'a Bin,
    update_state_gu: Option<MutexGuard<'a, UpdateState>>,
    style_gu: Option<RwLockUpgradableReadGuard<'a, Arc<BinStyle>>>,
    style_op: Option<BinStyle>,
    layout_stale: bool,
}

impl<'a> TextBodyGuard<'a> {
    fn new(bin: &'a Bin) -> Self {
        todo!()
    }

    fn state(&mut self) -> &mut TextState {
        if self.update_state_gu.is_none() {
            self.update_state_gu = Some(self.parent.update_state.lock());
        }

        &mut self.update_state_gu.as_mut().unwrap().text
    }

    fn style(&mut self) -> &BinStyle {
        if let Some(style) = self.style_op.as_ref() {
            return style;
        }

        if self.style_gu.is_none() {
            self.style_gu = Some(self.parent.style.upgradable_read());
        }

        &**self.style_gu.as_ref().unwrap()
    }

    fn style_mut(&mut self) -> &mut BinStyle {
        if self.style_op.is_none() {
            if self.style_gu.is_none() {
                self.style_gu = Some(self.parent.style.upgradable_read());
            }

            self.style_op = Some((***self.style_gu.as_ref().unwrap()).clone());
        }

        self.style_op.as_mut().unwrap()
    }

    pub fn cursor(&mut self) -> TextCursor {
        self.style().text_body.cursor
    }

    pub fn set_cursor(&mut self, cursor: TextCursor) {
        self.style_mut().text_body.cursor = cursor;
    }

    pub fn get_cursor(&mut self, position: [f32; 2]) -> TextCursor {
        todo!()
    }

    pub fn cursor_position(&mut self, cursor: TextCursor) -> Option<[f32; 2]> {
        if self.layout_stale {
            // TODO:
        }

        todo!()
    }

    pub fn cursor_prev(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_next(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_up(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_down(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_insert(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_insert_str<S>(&mut self, cursor: TextCursor, string: S) -> TextCursor
    where
        S: AsRef<str>,
    {
        todo!()
    }

    pub fn cursor_insert_spans<S>(&mut self, cursor: TextCursor, spans: S) -> TextCursor
    where
        S: IntoIterator<Item = TextSpan>,
    {
        todo!()
    }

    pub fn cursor_delete(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_delete_word(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_delete_line(&mut self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        todo!()
    }

    pub fn cursor_delete_span(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_word_start(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_word_end(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_select_word(&mut self, cursor: TextCursor) -> Option<TextSelection> {
        todo!()
    }

    pub fn cursor_line_start(&mut self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        todo!()
    }

    pub fn cursor_line_end(&mut self, cursor: TextCursor, as_displayed: bool) -> TextCursor {
        todo!()
    }

    pub fn cursor_select_line(
        &mut self,
        cursor: TextCursor,
        as_displayed: bool,
    ) -> Option<TextSelection> {
        todo!()
    }

    pub fn cursor_span_start(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_span_end(&mut self, cursor: TextCursor) -> TextCursor {
        todo!()
    }

    pub fn cursor_select_span(&mut self, cursor: TextCursor) -> Option<TextSelection> {
        todo!()
    }

    pub fn selection(&mut self) -> Option<TextSelection> {
        todo!()
    }

    pub fn set_selection(&mut self, selection: TextSelection) -> Result<(), ()> {
        todo!()
    }

    pub fn clear_selection(&mut self) {
        todo!()
    }

    pub fn select_line(&mut self, line_i: usize, as_displayed: bool) -> Option<TextSelection> {
        todo!()
    }

    pub fn select_span(&mut self, span_i: usize) -> Option<TextSelection> {
        todo!()
    }

    pub fn select_all(&mut self) -> Option<TextSelection> {
        todo!()
    }

    pub fn selection_string(&mut self, selection: TextSelection) -> String {
        todo!()
    }

    pub fn selection_spans(&mut self, selection: TextSelection) -> Vec<TextSpan> {
        todo!()
    }

    pub fn selection_take(&mut self, selection: TextSelection) -> (TextCursor, Vec<TextSpan>) {
        todo!()
    }

    pub fn selection_delete(&mut self, selection: TextSelection) -> TextCursor {
        todo!()
    }

    pub fn abort_changes(&mut self) {
        todo!()
    }
}

impl<'a> Drop for TextBodyGuard<'a> {
    fn drop(&mut self) {
        // TODO:
    }
}
