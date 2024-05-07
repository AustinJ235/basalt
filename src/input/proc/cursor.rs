use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use crate::input::state::{HookState, WindowState};
use crate::input::{Hook, InputHookCtrl, InputHookID, InputHookTargetID, NO_HOOK_WEIGHT};
use crate::interface::Interface;
use crate::window::WindowID;

pub(in crate::input) fn cursor(
    interface: &Arc<Interface>,
    hooks: &mut HashMap<InputHookID, Hook>,
    win_state: &mut HashMap<WindowID, WindowState>,
    win: WindowID,
    x: f32,
    y: f32,
    force: bool,
) {
    let window_state = win_state
        .entry(win)
        .or_insert_with(|| WindowState::new(win));

    if window_state.update_cursor_pos(x, y) || force {
        let inside_bin_ids = interface.get_bin_ids_atop(win, x, y);
        let focused_bin_id = window_state.focused_bin_id();
        let mut call_leave_on: Vec<(i16, InputHookID, &mut Hook)> = Vec::new();
        let mut enter: Vec<(i16, InputHookID, &mut Hook)> = Vec::new();
        let mut call_cursor_on: Vec<(i16, InputHookID, &mut Hook)> = Vec::new();

        for (hook_id, hook) in hooks.iter_mut() {
            let bin_id_op = hook.bin_id();

            match &mut hook.state {
                HookState::Enter {
                    weight,
                    top,
                    inside,
                    ..
                } => {
                    let bin_id = match bin_id_op {
                        Some(some) => some,
                        None => continue,
                    };

                    if window_state.is_cursor_captured() {
                        continue;
                    }

                    let mut inside_i_op: Option<usize> = None;

                    for (i, inside_id) in inside_bin_ids.iter().enumerate() {
                        if bin_id == *inside_id {
                            inside_i_op = Some(i);
                            break;
                        }
                    }

                    let inside_i = match inside_i_op {
                        Some(some) => some,
                        None => {
                            *inside = false;
                            continue;
                        },
                    };

                    if *top {
                        if inside_i == 0 {
                            enter.push((*weight, *hook_id, hook));
                        } else {
                            *inside = false;
                        }
                    } else {
                        enter.push((*weight, *hook_id, hook));
                    }
                },
                HookState::Leave {
                    weight,
                    top,
                    inside,
                    ..
                } => {
                    let bin_id = match bin_id_op {
                        Some(some) => some,
                        None => continue,
                    };

                    if window_state.is_cursor_captured() {
                        if *inside {
                            call_leave_on.push((*weight, *hook_id, hook));
                        }

                        continue;
                    }

                    let mut inside_i_op: Option<usize> = None;

                    for (i, inside_id) in inside_bin_ids.iter().enumerate() {
                        if bin_id == *inside_id {
                            inside_i_op = Some(i);
                            break;
                        }
                    }

                    match inside_i_op {
                        Some(inside_i) => {
                            if *top {
                                if inside_i != 0 {
                                    if *inside {
                                        call_leave_on.push((*weight, *hook_id, hook));
                                    }
                                } else {
                                    enter.push((*weight, *hook_id, hook));
                                }
                            } else {
                                enter.push((*weight, *hook_id, hook));
                            }
                        },
                        None => {
                            if *inside {
                                call_leave_on.push((*weight, *hook_id, hook));
                            }
                        },
                    }
                },
                HookState::Cursor {
                    weight,
                    top,
                    focus,
                    inside,
                    state,
                    ..
                } => {
                    match hook.target_id {
                        InputHookTargetID::Window(hook_win) => {
                            if hook_win == win
                                && window_state.is_cursor_inside()
                                && !window_state.is_cursor_captured()
                                && (!*focus || window_state.is_focused())
                            {
                                *inside = true;
                                call_cursor_on.push((*weight, *hook_id, hook));
                            } else if *inside {
                                // TODO: This isn't called when the cursor leaves the window
                                *inside = false;
                                state.reset();
                            }
                        },
                        InputHookTargetID::Bin(hook_bin) => {
                            if window_state.is_cursor_captured() {
                                *inside = false;
                                state.reset();
                                continue;
                            }

                            let mut inside_i_op: Option<usize> = None;

                            for (i, inside_id) in inside_bin_ids.iter().enumerate() {
                                if hook_bin == *inside_id {
                                    inside_i_op = Some(i);
                                    break;
                                }
                            }

                            let inside_i = match inside_i_op {
                                Some(some) => some,
                                None => {
                                    *inside = false;
                                    state.reset();
                                    continue;
                                },
                            };

                            if (!*focus || Some(hook_bin) == focused_bin_id)
                                && (!*top || inside_i == 0)
                            {
                                *inside = true;
                                state.update_top_most(inside_i == 0);
                                call_cursor_on.push((*weight, *hook_id, hook));
                            } else if *inside {
                                *inside = false;
                                state.reset()
                            }
                        },
                        InputHookTargetID::None => (),
                    }
                },
                _ => (),
            }
        }

        enter.sort_by_key(|(weight, ..)| Reverse(*weight));
        let mut call_enter_method = true;
        let mut remove_hooks = Vec::new();

        for (weight, hook_id, hook) in enter {
            match &mut hook.state {
                HookState::Enter {
                    method,
                    inside,
                    pass,
                    ..
                } => {
                    if call_enter_method {
                        if *inside {
                            if weight != NO_HOOK_WEIGHT && !*pass {
                                call_enter_method = false;
                            }
                        } else {
                            let hook_target = match hook.target_wk.upgrade() {
                                Some(some) => some,
                                None => {
                                    remove_hooks.push(hook_id);
                                    continue;
                                },
                            };

                            match method(hook_target, window_state) {
                                InputHookCtrl::Retain => {
                                    *pass = true;
                                    *inside = true;
                                },
                                InputHookCtrl::RetainNoPass => {
                                    if weight != NO_HOOK_WEIGHT {
                                        call_enter_method = false;
                                        *pass = false;
                                        *inside = true;
                                    }
                                },
                                InputHookCtrl::Remove => {
                                    remove_hooks.push(hook_id);
                                },
                                InputHookCtrl::RemoveNoPass => {
                                    remove_hooks.push(hook_id);

                                    if weight != NO_HOOK_WEIGHT {
                                        call_enter_method = false;
                                    }
                                },
                            }
                        }
                    } else {
                        *inside = false;
                    }
                },
                HookState::Leave {
                    inside, ..
                } => {
                    if *inside {
                        if !call_enter_method {
                            call_leave_on.push((weight, hook_id, hook));
                        }
                    } else if call_enter_method {
                        *inside = true;
                    }
                },
                _ => unreachable!(),
            }
        }

        call_leave_on.sort_by_key(|(weight, ..)| Reverse(*weight));
        let mut call_leave_method = true;

        for (weight, hook_id, hook) in call_leave_on {
            if let HookState::Leave {
                inside,
                method,
                ..
            } = &mut hook.state
            {
                let hook_target = match hook.target_wk.upgrade() {
                    Some(some) => some,
                    None => {
                        remove_hooks.push(hook_id);
                        continue;
                    },
                };

                *inside = false;

                if call_leave_method {
                    match method(hook_target, window_state) {
                        InputHookCtrl::Retain => (),
                        InputHookCtrl::RetainNoPass => {
                            if weight != NO_HOOK_WEIGHT {
                                call_leave_method = false;
                            }
                        },
                        InputHookCtrl::Remove => {
                            remove_hooks.push(hook_id);
                        },
                        InputHookCtrl::RemoveNoPass => {
                            remove_hooks.push(hook_id);
                            call_leave_method = false;
                        },
                    }
                }
            } else {
                unreachable!()
            }
        }

        call_cursor_on.sort_by_key(|(weight, ..)| Reverse(*weight));
        let mut call_cursor_method = true;

        for (weight, hook_id, hook) in call_cursor_on {
            if let HookState::Cursor {
                state,
                method,
                ..
            } = &mut hook.state
            {
                let hook_target = match hook.target_wk.upgrade() {
                    Some(some) => some,
                    None => {
                        remove_hooks.push(hook_id);
                        continue;
                    },
                };

                if call_cursor_method {
                    state.update_delta(x, y);

                    match method(hook_target, window_state, state) {
                        InputHookCtrl::Retain => (),
                        InputHookCtrl::RetainNoPass => {
                            if weight != NO_HOOK_WEIGHT {
                                call_cursor_method = false;
                            }
                        },
                        InputHookCtrl::Remove => {
                            remove_hooks.push(hook_id);
                        },
                        InputHookCtrl::RemoveNoPass => {
                            remove_hooks.push(hook_id);
                            call_cursor_method = false;
                        },
                    }
                } else {
                    state.reset();
                }
            } else {
                unreachable!()
            }
        }

        for hook_id in remove_hooks {
            hooks.remove(&hook_id);
        }
    }
}
