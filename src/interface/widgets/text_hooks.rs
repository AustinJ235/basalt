use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign};
use std::sync::Arc;
use std::sync::atomic::{self, AtomicU8};
use std::time::{Duration, Instant};

use crate::Basalt;
use crate::clipboard::ClipboardItem;
use crate::input::{InputHookCtrl, MouseButton, Qwerty};
use crate::interface::widgets::Theme;
use crate::interface::{
    Bin, BinPostUpdate, PosTextCursor, TextBodyGuard, TextCursor, TextSelection,
};
use crate::interval::{IntvlHookCtrl, IntvlHookID};

#[derive(Clone, Copy)]
pub struct Properties {
    pub single_line: bool,
    pub display_cursor: bool,
    pub use_display_lines: bool,
    pub allow_modifications: bool,
    pub allow_cursor_to_selection: bool,
}

#[allow(dead_code)]
impl Properties {
    pub const CODE_EDITOR: Self = Self {
        single_line: false,
        display_cursor: true,
        use_display_lines: false,
        allow_modifications: true,
        allow_cursor_to_selection: true,
    };
    pub const EDITOR: Self = Self {
        single_line: false,
        display_cursor: true,
        use_display_lines: true,
        allow_modifications: true,
        allow_cursor_to_selection: true,
    };
    pub const ENTRY: Self = Self {
        single_line: true,
        display_cursor: true,
        use_display_lines: false,
        allow_modifications: true,
        allow_cursor_to_selection: true,
    };
    pub const LABEL: Self = Self {
        single_line: false,
        display_cursor: false,
        use_display_lines: true,
        allow_modifications: false,
        allow_cursor_to_selection: false,
    };
}

#[allow(dead_code)]
pub struct Updated<'a> {
    pub cursor: TextCursor,
    pub cursor_bounds: Option<[f32; 4]>,
    pub body_line_count: usize,
    pub cursor_line_col: Option<[usize; 2]>,
    pub editor_bpu: &'a BinPostUpdate,
}

pub fn create(
    properties: Properties,
    editor: Arc<Bin>,
    theme: Theme,
    updated: Option<Arc<dyn Fn(Updated) + Send + Sync + 'static>>,
    scroll_v: Option<Arc<dyn Fn(f32) + Send + Sync + 'static>>,
    char_filter: Option<Arc<dyn Fn(&TextBodyGuard, char) -> bool + Send + Sync + 'static>>,
) {
    let intvl_blink_id = if properties.display_cursor {
        let editor_wk = Arc::downgrade(&editor);
        let mut cursor_visible = false;

        let intvl_id = editor.basalt_ref().interval_ref().do_every(
            Duration::from_millis(500),
            None,
            move |elapsed| {
                let editor = match editor_wk.upgrade() {
                    Some(some) => some,
                    None => return IntvlHookCtrl::Remove,
                };

                if elapsed.is_none() {
                    cursor_visible = true;
                } else {
                    cursor_visible = !cursor_visible;
                }

                editor.style_modify(|style| {
                    if cursor_visible {
                        style.text_body.cursor_color.a = 1.0;
                    } else {
                        style.text_body.cursor_color.a = 0.0;
                    }
                });

                Default::default()
            },
        );

        editor.attach_intvl_hook(intvl_id);
        Some(intvl_id)
    } else {
        None
    };

    let hooks = Arc::new(Hooks {
        basalt: editor.basalt(),
        properties,
        theme,
        modifiers: AtomicU8::new(0),
        intvl_blink_id,
        updated,
        scroll_v,
        char_filter,
    });

    if properties.display_cursor {
        let cb_hooks = hooks.clone();

        editor.on_focus(move |_, _| {
            cb_hooks.start_cursor_blink();
            Default::default()
        });

        let cb_hooks = hooks.clone();

        editor.on_focus_lost(move |target, _| {
            cb_hooks.pause_cursor_blink();

            target.into_bin().unwrap().style_modify(|style| {
                style.text_body.cursor_color.a = 0.0;
                style.text_body.selection = None;
            });

            Default::default()
        });
    }

    for (key, mask) in [
        (Qwerty::LShift, Modifiers::LEFT_SHIFT),
        (Qwerty::RShift, Modifiers::RIGHT_SHIFT),
        (Qwerty::LCtrl, Modifiers::LEFT_CTRL),
        (Qwerty::RCtrl, Modifiers::RIGHT_CTRL),
        (Qwerty::LAlt, Modifiers::LEFT_ALT),
        (Qwerty::RAlt, Modifiers::RIGHT_ALT),
    ] {
        let cb_hooks = hooks.clone();

        editor.on_press(key, move |_, _, _| {
            let mut modifiers = cb_hooks.modifiers();
            modifiers |= mask;
            cb_hooks
                .modifiers
                .store(modifiers.0, atomic::Ordering::SeqCst);
            Default::default()
        });

        let cb_hooks = hooks.clone();

        editor.on_release(key, move |_, _, _| {
            let mut modifiers = cb_hooks.modifiers();
            modifiers &= Modifiers(255) ^ mask;
            cb_hooks
                .modifiers
                .store(modifiers.0, atomic::Ordering::SeqCst);
            Default::default()
        });
    }

    let cb_hooks = hooks.clone();
    let mut consecutive_presses: u8 = 0;
    let mut last_press_op: Option<Instant> = None;

    editor.on_press(MouseButton::Left, move |target, window_state, _| {
        match last_press_op {
            Some(last_press) => {
                if last_press.elapsed() <= Duration::from_millis(300) {
                    consecutive_presses += 1;

                    if consecutive_presses > 3 {
                        consecutive_presses = 1;
                    }
                } else {
                    consecutive_presses = 1;
                }
            },
            None => {
                consecutive_presses = 1;
            },
        }

        last_press_op = Some(Instant::now());
        cb_hooks.proc_left_mb(
            target.into_bin().unwrap(),
            window_state.cursor_pos(),
            consecutive_presses,
        )
    });

    let cb_hooks = hooks.clone();

    hooks
        .basalt
        .input_ref()
        .hook()
        .bin(&editor)
        .on_cursor()
        .require_on_top(true)
        .require_focused(true)
        .call(move |target, window_state, _| {
            if window_state.is_key_pressed(MouseButton::Left) {
                cb_hooks.proc_cursor_move(target.into_bin().unwrap(), window_state.cursor_pos())
            } else {
                Default::default()
            }
        })
        .finish()
        .unwrap();

    for key in [
        Qwerty::ArrowLeft,
        Qwerty::ArrowRight,
        Qwerty::ArrowUp,
        Qwerty::ArrowDown,
        Qwerty::PageUp,
        Qwerty::PageDown,
    ] {
        if properties.single_line
            && matches!(
                key,
                Qwerty::PageUp | Qwerty::PageDown | Qwerty::ArrowUp | Qwerty::ArrowDown
            )
        {
            continue;
        }

        let cb_hooks = hooks.clone();

        editor.on_press(key, move |target, _, _| {
            cb_hooks.proc_movement_key(target.into_bin().unwrap(), key)
        });

        let cb_hooks = hooks.clone();

        editor
            .basalt_ref()
            .input_ref()
            .hook()
            .bin(&editor)
            .on_hold()
            .keys(key)
            .delay(Some(Duration::from_millis(600)))
            .interval(Duration::from_millis(40))
            .call(move |target, _, _| cb_hooks.proc_movement_key(target.into_bin().unwrap(), key))
            .finish()
            .unwrap();
    }

    for key in [Qwerty::Home, Qwerty::End] {
        let cb_hooks = hooks.clone();

        editor.on_press(key, move |target, _, _| {
            cb_hooks.proc_movement_key(target.into_bin().unwrap(), key)
        });
    }

    for key_combo in [[Qwerty::LCtrl, Qwerty::C], [Qwerty::RCtrl, Qwerty::C]] {
        let cb_hooks = hooks.clone();

        editor.on_press(key_combo, move |target, _, _| {
            cb_hooks.proc_copy(target.into_bin().unwrap())
        });
    }

    if properties.allow_modifications {
        for key_combo in [[Qwerty::LCtrl, Qwerty::X], [Qwerty::RCtrl, Qwerty::X]] {
            let cb_hooks = hooks.clone();

            editor.on_press(key_combo, move |target, _, _| {
                cb_hooks.proc_cut(target.into_bin().unwrap())
            });
        }

        for key_combo in [[Qwerty::LCtrl, Qwerty::V], [Qwerty::RCtrl, Qwerty::V]] {
            let cb_hooks = hooks.clone();

            editor.on_press(key_combo, move |target, _, _| {
                cb_hooks.proc_paste(target.into_bin().unwrap())
            });
        }

        let cb_hooks = hooks.clone();

        editor.on_character(move |target, _, c| {
            cb_hooks.proc_character(target.into_bin().unwrap(), c.0)
        });
    }

    if properties.allow_cursor_to_selection {
        for key_combo in [[Qwerty::LCtrl, Qwerty::A], [Qwerty::RCtrl, Qwerty::A]] {
            let cb_hooks = hooks.clone();

            editor.on_press(key_combo, move |target, _, _| {
                cb_hooks.proc_select_all(target.into_bin().unwrap())
            });
        }
    }
}

struct Hooks {
    basalt: Arc<Basalt>,
    properties: Properties,
    theme: Theme,
    modifiers: AtomicU8,
    intvl_blink_id: Option<IntvlHookID>,
    updated: Option<Arc<dyn Fn(Updated) + Send + Sync + 'static>>,
    scroll_v: Option<Arc<dyn Fn(f32) + Send + Sync + 'static>>,
    char_filter: Option<Arc<dyn Fn(&TextBodyGuard, char) -> bool + Send + Sync + 'static>>,
}

impl Hooks {
    fn modifiers(&self) -> Modifiers {
        Modifiers(self.modifiers.load(atomic::Ordering::SeqCst))
    }

    fn start_cursor_blink(&self) {
        if let Some(intvl_blink_id) = self.intvl_blink_id {
            self.basalt.interval_ref().start(intvl_blink_id);
        }
    }

    fn pause_cursor_blink(&self) {
        if let Some(intvl_blink_id) = self.intvl_blink_id {
            self.basalt.interval_ref().pause(intvl_blink_id);
        }
    }

    fn reset_cursor_blink(&self) {
        if let Some(intvl_blink_id) = self.intvl_blink_id {
            self.basalt.interval_ref().pause(intvl_blink_id);
            self.basalt.interval_ref().start(intvl_blink_id);
        }
    }

    fn proc_left_mb(
        self: &Arc<Self>,
        editor: Arc<Bin>,
        position: [f32; 2],
        consecutive_presses: u8,
    ) -> InputHookCtrl {
        let modifiers = self.modifiers();
        let text_body = editor.text_body();
        let cursor = text_body.get_cursor(position);

        if !matches!(cursor, TextCursor::Position(..)) {
            return Default::default();
        }

        match consecutive_presses {
            1 => {
                if modifiers.shift() {
                    match text_body.selection() {
                        Some(existing_selection) => {
                            let sel_s = match text_body.cursor() {
                                TextCursor::None | TextCursor::Empty => existing_selection.start,
                                TextCursor::Position(existing_cursor) => {
                                    if existing_cursor == existing_selection.start {
                                        existing_selection.end
                                    } else {
                                        existing_selection.start
                                    }
                                },
                            };

                            text_body.set_selection(TextSelection::unordered(
                                sel_s,
                                cursor.into_position().unwrap(),
                            ))
                        },
                        None => {
                            match text_body.cursor() {
                                TextCursor::None | TextCursor::Empty => (),
                                TextCursor::Position(sel_s) => {
                                    text_body.set_selection(TextSelection::unordered(
                                        sel_s,
                                        cursor.into_position().unwrap(),
                                    ));
                                },
                            }
                        },
                    }

                    text_body.set_cursor(cursor);
                } else {
                    text_body.set_cursor(cursor);
                    text_body.clear_selection();
                }
            },
            2 | 3 => {
                match match consecutive_presses {
                    2 => text_body.cursor_select_word(cursor),
                    3 => text_body.cursor_select_line(cursor, self.properties.use_display_lines),
                    _ => unreachable!(),
                } {
                    Some(selection) => {
                        if modifiers.shift() {
                            match text_body.selection() {
                                Some(existing_selection) => {
                                    text_body.set_selection(TextSelection {
                                        start: existing_selection.start.min(selection.start),
                                        end: existing_selection.end.max(selection.end),
                                    });

                                    if selection.start > existing_selection.start {
                                        text_body.set_cursor(selection.end.into());
                                    } else {
                                        text_body.set_cursor(selection.start.into());
                                    }
                                },
                                None => {
                                    text_body.set_cursor(selection.end.into());
                                    text_body.set_selection(selection);
                                },
                            }
                        } else {
                            text_body.set_cursor(selection.end.into());
                            text_body.set_selection(selection);
                        }
                    },
                    None => {
                        text_body.set_cursor(cursor);
                        text_body.clear_selection();
                    },
                }
            },
            0 | 4.. => unreachable!(),
        }

        if matches!(text_body.cursor(), TextCursor::Position(..)) {
            self.updated(&text_body)
        } else {
            Default::default()
        }
    }

    fn proc_cursor_move(self: &Arc<Self>, editor: Arc<Bin>, position: [f32; 2]) -> InputHookCtrl {
        let text_body = editor.text_body();

        let cursor = match text_body.cursor() {
            TextCursor::None | TextCursor::Empty => return Default::default(),
            TextCursor::Position(cursor) => cursor,
        };

        let sel_s = match text_body.selection() {
            Some(selection) => {
                if selection.start == cursor {
                    selection.end
                } else {
                    selection.start
                }
            },
            None => cursor,
        };

        let sel_e = match text_body.get_cursor(position) {
            TextCursor::None | TextCursor::Empty => {
                text_body.set_cursor(TextCursor::None);
                text_body.clear_selection();
                return Default::default();
            },
            TextCursor::Position(cursor) => cursor,
        };

        text_body.set_cursor(sel_e.into());

        if sel_s == sel_e {
            text_body.clear_selection();
        } else {
            text_body.set_selection(TextSelection::unordered(sel_s, sel_e));
        }

        self.updated(&text_body)
    }

    fn proc_movement_key(self: &Arc<Self>, editor: Arc<Bin>, key: Qwerty) -> InputHookCtrl {
        let modifiers = self.modifiers();
        let text_body = editor.text_body();

        if modifiers.shift() {
            if modifiers.ctrl()
                && matches!(
                    key,
                    Qwerty::ArrowUp | Qwerty::ArrowDown | Qwerty::PageUp | Qwerty::PageDown
                )
            {
                return Default::default();
            }

            match text_body.selection() {
                Some(sel_exist) => {
                    let cur_exist = match text_body.cursor() {
                        TextCursor::None | TextCursor::Empty => {
                            text_body.clear_selection();
                            return Default::default();
                        },
                        TextCursor::Position(cursor) => cursor,
                    };

                    let (sel_start, sel_end) = if sel_exist.start == cur_exist {
                        (sel_exist.end, sel_exist.start)
                    } else if sel_exist.end == cur_exist {
                        (sel_exist.start, sel_exist.end)
                    } else {
                        text_body.clear_selection();
                        return Default::default();
                    };

                    let cur_move = if modifiers.alt() { sel_start } else { sel_end };

                    let cur_next = if modifiers.ctrl() {
                        let next_op = match key {
                            Qwerty::ArrowLeft => Some(NextWordLineOp::WordStart),
                            Qwerty::ArrowRight => Some(NextWordLineOp::WordEnd),
                            Qwerty::Home | Qwerty::End => None,
                            Qwerty::ArrowUp
                            | Qwerty::ArrowDown
                            | Qwerty::PageUp
                            | Qwerty::PageDown => unreachable!(),
                            _ => return Default::default(),
                        };

                        match next_op {
                            Some(next_op) => {
                                self.cursor_next_word_line(&text_body, cur_move, next_op)
                            },
                            None => {
                                let sel_all = match text_body.select_all() {
                                    Some(selection) => selection,
                                    None => {
                                        text_body.clear_selection();
                                        return Default::default();
                                    },
                                };

                                match key {
                                    Qwerty::Home => sel_all.start,
                                    Qwerty::End => sel_all.end,
                                    _ => unreachable!(),
                                }
                            },
                        }
                    } else {
                        let next_op = match key {
                            Qwerty::Home => Some(NextWordLineOp::LineStart),
                            Qwerty::End => Some(NextWordLineOp::LineEnd),
                            Qwerty::ArrowLeft
                            | Qwerty::ArrowRight
                            | Qwerty::ArrowUp
                            | Qwerty::ArrowDown
                            | Qwerty::PageUp
                            | Qwerty::PageDown => None,
                            _ => return Default::default(),
                        };

                        match next_op {
                            Some(next_op) => {
                                self.cursor_next_word_line(&text_body, cur_move, next_op)
                            },
                            None => {
                                match match key {
                                    Qwerty::ArrowLeft => text_body.cursor_prev(cur_move.into()),
                                    Qwerty::ArrowRight => text_body.cursor_next(cur_move.into()),
                                    Qwerty::ArrowUp => {
                                        text_body.cursor_up(
                                            cur_move.into(),
                                            self.properties.use_display_lines,
                                        )
                                    },
                                    Qwerty::ArrowDown => {
                                        text_body.cursor_down(
                                            cur_move.into(),
                                            self.properties.use_display_lines,
                                        )
                                    },
                                    Qwerty::PageUp => {
                                        text_body.cursor_line_offset(
                                            cur_move.into(),
                                            -self.page_lines(&editor),
                                            self.properties.use_display_lines,
                                        )
                                    },
                                    Qwerty::PageDown => {
                                        text_body.cursor_line_offset(
                                            cur_move.into(),
                                            self.page_lines(&editor),
                                            self.properties.use_display_lines,
                                        )
                                    },
                                    _ => unreachable!(),
                                } {
                                    TextCursor::None | TextCursor::Empty => {
                                        return Default::default();
                                    },
                                    TextCursor::Position(cursor) => cursor,
                                }
                            },
                        }
                    };

                    if text_body.are_cursors_equivalent(cur_move.into(), cur_next.into()) {
                        return Default::default();
                    }

                    if modifiers.alt() {
                        if text_body.are_cursors_equivalent(sel_end.into(), cur_next.into()) {
                            text_body.clear_selection();
                        } else {
                            text_body.set_selection(TextSelection::unordered(sel_end, cur_next));
                        }
                    } else {
                        if text_body.are_cursors_equivalent(sel_start.into(), cur_next.into()) {
                            text_body.clear_selection();
                            text_body.set_cursor(sel_start.into());
                        } else {
                            text_body.set_selection(TextSelection::unordered(sel_start, cur_next));
                            text_body.set_cursor(cur_next.into());
                        }
                    }
                },
                None => {
                    if !self.properties.allow_cursor_to_selection {
                        return Default::default();
                    }

                    let sel_start = match text_body.cursor() {
                        TextCursor::None => return Default::default(),
                        TextCursor::Empty => {
                            match text_body.select_all() {
                                Some(sel_all) => sel_all.start,
                                None => return Default::default(),
                            }
                        },
                        TextCursor::Position(cursor) => cursor,
                    };

                    let sel_end = if modifiers.ctrl() {
                        match key {
                            Qwerty::ArrowLeft => {
                                self.cursor_next_word_line(
                                    &text_body,
                                    sel_start,
                                    NextWordLineOp::WordStart,
                                )
                            },
                            Qwerty::ArrowRight => {
                                self.cursor_next_word_line(
                                    &text_body,
                                    sel_start,
                                    NextWordLineOp::WordEnd,
                                )
                            },
                            Qwerty::Home => {
                                match text_body.select_all() {
                                    Some(sel_all) => sel_all.start,
                                    None => return Default::default(),
                                }
                            },
                            Qwerty::End => {
                                match text_body.select_all() {
                                    Some(sel_all) => sel_all.end,
                                    None => return Default::default(),
                                }
                            },
                            Qwerty::ArrowUp
                            | Qwerty::ArrowDown
                            | Qwerty::PageUp
                            | Qwerty::PageDown => unreachable!(),
                            _ => return Default::default(),
                        }
                    } else {
                        match match key {
                            Qwerty::ArrowLeft => text_body.cursor_prev(sel_start.into()),
                            Qwerty::ArrowRight => text_body.cursor_next(sel_start.into()),
                            Qwerty::ArrowUp => {
                                text_body
                                    .cursor_up(sel_start.into(), self.properties.use_display_lines)
                            },
                            Qwerty::ArrowDown => {
                                text_body.cursor_down(
                                    sel_start.into(),
                                    self.properties.use_display_lines,
                                )
                            },
                            Qwerty::Home => {
                                text_body.cursor_line_start(
                                    sel_start.into(),
                                    self.properties.use_display_lines,
                                )
                            },
                            Qwerty::End => {
                                text_body.cursor_line_end(
                                    sel_start.into(),
                                    self.properties.use_display_lines,
                                )
                            },
                            Qwerty::PageUp => {
                                text_body.cursor_line_offset(
                                    sel_start.into(),
                                    -self.page_lines(&editor),
                                    self.properties.use_display_lines,
                                )
                            },
                            Qwerty::PageDown => {
                                text_body.cursor_line_offset(
                                    sel_start.into(),
                                    self.page_lines(&editor),
                                    self.properties.use_display_lines,
                                )
                            },
                            _ => return Default::default(),
                        } {
                            TextCursor::None | TextCursor::Empty => return Default::default(),
                            TextCursor::Position(cursor) => cursor,
                        }
                    };

                    if text_body.are_cursors_equivalent(sel_start.into(), sel_end.into()) {
                        return Default::default();
                    }

                    text_body.set_cursor(sel_end.into());
                    text_body.set_selection(TextSelection::unordered(sel_start, sel_end));
                },
            }
        } else if modifiers.ctrl() {
            if !self.properties.display_cursor || matches!(key, Qwerty::PageUp | Qwerty::PageDown) {
                return Default::default();
            }

            match text_body.selection() {
                Some(selection) => {
                    let cursor = match key {
                        Qwerty::ArrowLeft => selection.start,
                        Qwerty::ArrowRight => selection.end,
                        Qwerty::ArrowUp => {
                            match text_body.cursor_up(
                                selection.start.into(),
                                self.properties.use_display_lines,
                            ) {
                                TextCursor::None | TextCursor::Empty => selection.start,
                                TextCursor::Position(cursor) => cursor,
                            }
                        },
                        Qwerty::ArrowDown => {
                            match text_body.cursor_down(
                                selection.end.into(),
                                self.properties.use_display_lines,
                            ) {
                                TextCursor::None | TextCursor::Empty => selection.end,
                                TextCursor::Position(cursor) => cursor,
                            }
                        },
                        Qwerty::Home => {
                            match text_body.select_all() {
                                Some(sel_all) => sel_all.start,
                                None => return Default::default(),
                            }
                        },
                        Qwerty::End => {
                            match text_body.select_all() {
                                Some(sel_all) => sel_all.end,
                                None => return Default::default(),
                            }
                        },
                        Qwerty::PageUp | Qwerty::PageDown => unreachable!(),
                        _ => return Default::default(),
                    };

                    text_body.set_cursor(cursor.into());
                    text_body.clear_selection();
                },
                None => {
                    match key {
                        Qwerty::ArrowLeft | Qwerty::ArrowRight => {
                            match text_body.cursor() {
                                TextCursor::None => return Default::default(),
                                TextCursor::Empty => {
                                    match text_body.select_all() {
                                        Some(sel_all) => {
                                            text_body.set_cursor(sel_all.start.into());
                                        },
                                        None => return Default::default(),
                                    }
                                },
                                TextCursor::Position(cursor) => {
                                    let cursor_next = self.cursor_next_word_line(
                                        &text_body,
                                        cursor,
                                        if key == Qwerty::ArrowLeft {
                                            NextWordLineOp::WordStart
                                        } else {
                                            NextWordLineOp::WordEnd
                                        },
                                    );

                                    if text_body
                                        .are_cursors_equivalent(cursor.into(), cursor_next.into())
                                    {
                                        return Default::default();
                                    }

                                    text_body.set_cursor(cursor_next.into());
                                },
                            }
                        },
                        Qwerty::ArrowUp | Qwerty::ArrowDown => {
                            let line_height = (self.theme.text_height * 1.2).round();

                            self.v_scroll(
                                if key == Qwerty::ArrowUp {
                                    -line_height
                                } else {
                                    line_height
                                },
                            );
                        },
                        Qwerty::Home => {
                            match text_body.select_all() {
                                Some(sel_all) => {
                                    text_body.set_cursor(sel_all.start.into());
                                },
                                None => return Default::default(),
                            }
                        },
                        Qwerty::End => {
                            match text_body.select_all() {
                                Some(sel_all) => {
                                    text_body.set_cursor(sel_all.end.into());
                                },
                                None => return Default::default(),
                            }
                        },
                        Qwerty::PageUp | Qwerty::PageDown => unreachable!(),
                        _ => return Default::default(),
                    }
                },
            }
        } else {
            if !self.properties.display_cursor {
                return Default::default();
            }

            match text_body.selection() {
                Some(selection) => {
                    let cursor = match key {
                        Qwerty::ArrowLeft => selection.start,
                        Qwerty::ArrowRight => selection.end,
                        Qwerty::ArrowUp => {
                            match text_body.cursor_up(
                                selection.start.into(),
                                self.properties.use_display_lines,
                            ) {
                                TextCursor::None | TextCursor::Empty => selection.start,
                                TextCursor::Position(cursor) => cursor,
                            }
                        },
                        Qwerty::ArrowDown => {
                            match text_body.cursor_down(
                                selection.end.into(),
                                self.properties.use_display_lines,
                            ) {
                                TextCursor::None | TextCursor::Empty => selection.end,
                                TextCursor::Position(cursor) => cursor,
                            }
                        },
                        Qwerty::Home => {
                            match text_body.cursor_line_start(
                                selection.start.into(),
                                self.properties.use_display_lines,
                            ) {
                                TextCursor::None | TextCursor::Empty => selection.start,
                                TextCursor::Position(cursor) => cursor,
                            }
                        },
                        Qwerty::End => {
                            match text_body.cursor_line_end(
                                selection.end.into(),
                                self.properties.use_display_lines,
                            ) {
                                TextCursor::None | TextCursor::Empty => selection.end,
                                TextCursor::Position(cursor) => cursor,
                            }
                        },
                        Qwerty::PageUp => {
                            match text_body.cursor_line_offset(
                                selection.start.into(),
                                -self.page_lines(&editor),
                                self.properties.use_display_lines,
                            ) {
                                TextCursor::None | TextCursor::Empty => selection.start,
                                TextCursor::Position(cursor) => cursor,
                            }
                        },
                        Qwerty::PageDown => {
                            match text_body.cursor_line_offset(
                                selection.end.into(),
                                self.page_lines(&editor),
                                self.properties.use_display_lines,
                            ) {
                                TextCursor::None | TextCursor::Empty => selection.end,
                                TextCursor::Position(cursor) => cursor,
                            }
                        },
                        _ => return Default::default(),
                    };

                    text_body.set_cursor(cursor.into());
                    text_body.clear_selection();
                },
                None => {
                    let cursor = match match key {
                        Qwerty::ArrowLeft => text_body.cursor_prev(text_body.cursor()),
                        Qwerty::ArrowRight => text_body.cursor_next(text_body.cursor()),
                        Qwerty::ArrowUp => {
                            text_body
                                .cursor_up(text_body.cursor(), self.properties.use_display_lines)
                        },
                        Qwerty::ArrowDown => {
                            text_body
                                .cursor_down(text_body.cursor(), self.properties.use_display_lines)
                        },
                        Qwerty::Home => {
                            text_body.cursor_line_start(
                                text_body.cursor(),
                                self.properties.use_display_lines,
                            )
                        },
                        Qwerty::End => {
                            text_body.cursor_line_end(
                                text_body.cursor(),
                                self.properties.use_display_lines,
                            )
                        },
                        Qwerty::PageUp => {
                            text_body.cursor_line_offset(
                                text_body.cursor(),
                                -self.page_lines(&editor),
                                self.properties.use_display_lines,
                            )
                        },
                        Qwerty::PageDown => {
                            text_body.cursor_line_offset(
                                text_body.cursor(),
                                self.page_lines(&editor),
                                self.properties.use_display_lines,
                            )
                        },
                        _ => return Default::default(),
                    } {
                        TextCursor::None | TextCursor::Empty => return Default::default(),
                        position => position,
                    };

                    text_body.set_cursor(cursor);
                },
            }
        }

        self.updated(&text_body)
    }

    fn proc_character(self: &Arc<Self>, editor: Arc<Bin>, c: char) -> InputHookCtrl {
        let modifiers = self.modifiers();
        let text_body = editor.text_body();

        if modifiers.alt() || (!matches!(c, '\u{007F}' | '\x08') && modifiers.ctrl()) {
            return Default::default();
        }

        let sel_deleted = match text_body.selection() {
            Some(selection) => {
                text_body.set_cursor(text_body.selection_delete(selection));
                text_body.clear_selection();
                true
            },
            None => false,
        };

        match c {
            '\u{007F}' | '\x08' => {
                if !sel_deleted {
                    let mut cursor = match text_body.cursor() {
                        TextCursor::None | TextCursor::Empty => return Default::default(),
                        TextCursor::Position(cursor) => cursor,
                    };

                    if modifiers.ctrl() {
                        let op = if c == '\u{007F}' {
                            if modifiers.shift() {
                                NextWordLineOp::LineEnd
                            } else {
                                NextWordLineOp::WordEnd
                            }
                        } else {
                            if modifiers.shift() {
                                NextWordLineOp::LineStart
                            } else {
                                NextWordLineOp::WordStart
                            }
                        };

                        let del_to = self.cursor_next_word_line(&text_body, cursor, op);

                        if text_body.are_cursors_equivalent(cursor.into(), del_to.into()) {
                            return Default::default();
                        }

                        text_body.set_cursor(
                            text_body.selection_delete(TextSelection::unordered(cursor, del_to)),
                        );
                    } else {
                        if c == '\u{007F}' {
                            cursor = match text_body.cursor_next(cursor.into()) {
                                TextCursor::None | TextCursor::Empty => return Default::default(),
                                TextCursor::Position(cursor) => cursor,
                            };
                        }

                        text_body.set_cursor(text_body.cursor_delete(cursor.into()));
                    }
                }
            },
            '\u{1b}' => return Default::default(),
            mut c => {
                if c == '\r' {
                    c = '\n';
                }

                if self.properties.single_line && c == '\n' {
                    return Default::default();
                }

                if let Some(char_filter) = self.char_filter.as_ref() {
                    if !(*char_filter)(&text_body, c) {
                        return Default::default();
                    }
                }

                text_body.set_cursor(text_body.cursor_insert(text_body.cursor(), c));
            },
        }

        self.updated(&text_body)
    }

    fn proc_copy(self: &Arc<Self>, editor: Arc<Bin>) -> InputHookCtrl {
        let text_body = editor.text_body();

        if let Some(selection) = text_body.selection() {
            let selection_str = text_body.selection_string(selection);

            if !selection_str.is_empty() {
                self.basalt.clipboard().set(selection_str);
            }
        }

        Default::default()
    }

    fn proc_cut(self: &Arc<Self>, editor: Arc<Bin>) -> InputHookCtrl {
        let text_body = editor.text_body();

        if let Some(selection) = text_body.selection() {
            let (cursor, selection_value) = text_body.selection_take_string(selection);
            text_body.clear_selection();
            text_body.set_cursor(cursor);

            if !selection_value.is_empty() {
                self.basalt.clipboard().set(selection_value);
            }
        }

        self.updated(&text_body)
    }

    fn proc_paste(self: &Arc<Self>, editor: Arc<Bin>) -> InputHookCtrl {
        let text_body = editor.text_body();
        let mut selection_cleared = false;

        if let Some(selection) = text_body.selection() {
            text_body.clear_selection();
            text_body.set_cursor(text_body.selection_delete(selection));
            selection_cleared = true;
        }

        match self.basalt.clipboard().get() {
            Some(ClipboardItem::PlainText(text)) => {
                text_body.set_cursor(text_body.cursor_insert_str(text_body.cursor(), text));
            },
            None => {
                if !selection_cleared {
                    return Default::default();
                }
            },
        }

        self.updated(&text_body)
    }

    fn proc_select_all(self: &Arc<Self>, editor: Arc<Bin>) -> InputHookCtrl {
        let text_body = editor.text_body();

        if let Some(selection) = text_body.select_all() {
            text_body.set_selection(selection);
        }

        self.updated(&text_body)
    }

    fn cursor_next_word_line(
        &self,
        text_body: &TextBodyGuard,
        cursor: PosTextCursor,
        op: NextWordLineOp,
    ) -> PosTextCursor {
        let edge = match match op {
            NextWordLineOp::WordStart => text_body.cursor_word_start(cursor.into()),
            NextWordLineOp::WordEnd => text_body.cursor_word_end(cursor.into()),
            NextWordLineOp::LineStart => {
                text_body.cursor_line_start(cursor.into(), self.properties.use_display_lines)
            },
            NextWordLineOp::LineEnd => {
                text_body.cursor_line_end(cursor.into(), self.properties.use_display_lines)
            },
        } {
            TextCursor::None | TextCursor::Empty => return cursor,
            TextCursor::Position(cursor) => cursor,
        };

        if !text_body.are_cursors_equivalent(cursor.into(), edge.into()) {
            return edge;
        }

        let next = match match op {
            NextWordLineOp::WordStart | NextWordLineOp::LineStart => {
                text_body.cursor_prev(cursor.into())
            },
            NextWordLineOp::WordEnd | NextWordLineOp::LineEnd => {
                text_body.cursor_next(cursor.into())
            },
        } {
            TextCursor::None | TextCursor::Empty => return edge,
            TextCursor::Position(cursor) => cursor,
        };

        match match op {
            NextWordLineOp::WordStart => text_body.cursor_word_start(next.into()),
            NextWordLineOp::WordEnd => text_body.cursor_word_end(next.into()),
            NextWordLineOp::LineStart => {
                text_body.cursor_line_start(next.into(), self.properties.use_display_lines)
            },
            NextWordLineOp::LineEnd => {
                text_body.cursor_line_end(next.into(), self.properties.use_display_lines)
            },
        } {
            TextCursor::None | TextCursor::Empty => next,
            TextCursor::Position(cursor) => cursor,
        }
        .into()
    }

    fn v_scroll(&self, amt: f32) {
        if let Some(scroll_v) = self.scroll_v.as_ref() {
            scroll_v(amt);
        }
    }

    fn updated(self: &Arc<Self>, text_body: &TextBodyGuard) -> InputHookCtrl {
        self.reset_cursor_blink();

        if let Some(updated) = self.updated.clone() {
            let cursor = text_body.cursor();
            let cursor_bounds = text_body.cursor_bounds(cursor);
            let body_line_count = text_body
                .line_count(self.properties.use_display_lines)
                .unwrap_or(0);
            let cursor_line_col =
                text_body.cursor_line_column(cursor, self.properties.use_display_lines);

            text_body.bin_on_update(move |_, editor_bpu| {
                updated(Updated {
                    cursor,
                    cursor_bounds,
                    body_line_count,
                    cursor_line_col,
                    editor_bpu,
                });
            });
        }

        Default::default()
    }

    fn page_lines(&self, editor: &Arc<Bin>) -> isize {
        let editor_bpu = editor.post_update();
        let body_height = editor_bpu.optimal_inner_bounds[3] - editor_bpu.optimal_inner_bounds[2];
        let line_height = (self.theme.text_height * 1.2).round();
        (body_height / line_height).max(1.0).floor() as isize
    }
}

enum NextWordLineOp {
    WordStart,
    WordEnd,
    LineStart,
    LineEnd,
}

#[derive(Clone, Copy, PartialEq)]
struct Modifiers(u8);

impl Modifiers {
    const LEFT_ALT: Self = Self(0b00001000);
    const LEFT_CTRL: Self = Self(0b00100000);
    const LEFT_SHIFT: Self = Self(0b10000000);
    const RIGHT_ALT: Self = Self(0b00000100);
    const RIGHT_CTRL: Self = Self(0b00010000);
    const RIGHT_SHIFT: Self = Self(0b01000000);

    fn shift(self) -> bool {
        self & Self::LEFT_SHIFT == Self::LEFT_SHIFT || self & Self::RIGHT_SHIFT == Self::RIGHT_SHIFT
    }

    fn ctrl(self) -> bool {
        self & Self::LEFT_CTRL == Self::LEFT_CTRL || self & Self::RIGHT_CTRL == Self::RIGHT_CTRL
    }

    fn alt(self) -> bool {
        self & Self::LEFT_ALT == Self::LEFT_ALT || self & Self::RIGHT_ALT == Self::RIGHT_ALT
    }
}

impl BitAnd for Modifiers {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for Modifiers {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = Self(self.0 & rhs.0);
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = Self(self.0 | rhs.0);
    }
}

impl BitXor for Modifiers {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Self(self.0 ^ rhs.0)
    }
}

impl BitXorAssign for Modifiers {
    fn bitxor_assign(&mut self, rhs: Self) {
        *self = Self(self.0 ^ rhs.0);
    }
}
