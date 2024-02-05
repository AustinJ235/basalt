use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use crate::input::state::{HookState, WindowState};
use crate::input::{proc, Hook, InputHookCtrl, InputHookID, Key, BIN_FOCUS_KEY, NO_HOOK_WEIGHT};
use crate::interface::Interface;
use crate::interval::Interval;
use crate::window::WindowID;

pub(in crate::input) fn press(
    interface: &Arc<Interface>,
    interval: &Arc<Interval>,
    hooks: &mut HashMap<InputHookID, Hook>,
    win_state: &mut HashMap<WindowID, WindowState>,
    win: WindowID,
    key: Key,
) {
    let window_state = win_state
        .entry(win)
        .or_insert_with(|| WindowState::new(win));

    // Returns true if the state changed
    if window_state.update_key(key, true) {
        let mut proc_in_order: Vec<_> = hooks
            .iter_mut()
            .filter_map(|(hook_id, hook)| {
                if hook.is_for_window_id(win) {
                    match &mut hook.state {
                        HookState::Press {
                            state,
                            weight,
                            ..
                        } => {
                            if state.update(key, true) {
                                Some((*weight, (hook_id, hook)))
                            } else {
                                None
                            }
                        },
                        HookState::Release {
                            state,
                            weight,
                            ..
                        } => {
                            if state.is_involved(key) {
                                Some((*weight, (hook_id, hook)))
                            } else {
                                None
                            }
                        },
                        HookState::Hold {
                            state,
                            weight,
                            ..
                        } => {
                            if state.is_involved(key) {
                                Some((*weight, (hook_id, hook)))
                            } else {
                                None
                            }
                        },
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect();

        proc_in_order.sort_by_key(|(weight, _)| Reverse(*weight));
        let mut pass_bin_event = true;
        let mut remove_hooks: Vec<InputHookID> = Vec::new();

        for (weight, (hook_id, hook)) in proc_in_order {
            match &mut hook.state {
                HookState::Press {
                    state,
                    method,
                    ..
                } => {
                    match hook.target_wk.upgrade() {
                        Some(hook_target) => {
                            match method(hook_target, window_state, state) {
                                InputHookCtrl::Retain => (),
                                InputHookCtrl::RetainNoPass => {
                                    if weight != NO_HOOK_WEIGHT {
                                        pass_bin_event = false;
                                        break;
                                    }
                                },
                                InputHookCtrl::Remove => {
                                    remove_hooks.push(*hook_id);
                                },
                                InputHookCtrl::RemoveNoPass => {
                                    remove_hooks.push(*hook_id);

                                    if weight != NO_HOOK_WEIGHT {
                                        pass_bin_event = false;
                                        break;
                                    }
                                },
                            }
                        },
                        None => {
                            remove_hooks.push(*hook_id);
                        },
                    }
                },
                HookState::Release {
                    state,
                    pressed,
                    ..
                } => {
                    if state.update(key, true) {
                        *pressed = true;
                    }
                },
                HookState::Hold {
                    state,
                    pressed,
                    intvl_id,
                    ..
                } => {
                    if state.update(key, true) {
                        *pressed = true;
                        interval.start(*intvl_id);
                    }
                },
                _ => unreachable!(),
            }
        }

        if pass_bin_event && !window_state.is_cursor_captured() {
            // Check Bin Focus
            if key == BIN_FOCUS_KEY {
                if let Some((old_bin_id_op, new_bin_id_op)) =
                    window_state.check_focus_bin(interface)
                {
                    proc::bin_focus(interval, hooks, window_state, old_bin_id_op, new_bin_id_op);
                }
            }

            if let Some(focus_bin_id) = window_state.focused_bin_id() {
                let mut call_in_order: Vec<_> = hooks
                    .iter_mut()
                    .filter_map(|(hook_id, hook)| {
                        if hook.is_for_bin_id(focus_bin_id) {
                            match &mut hook.state {
                                HookState::Press {
                                    state,
                                    weight,
                                    ..
                                } => {
                                    if state.update(key, true) {
                                        Some((*weight, (hook_id, hook)))
                                    } else {
                                        None
                                    }
                                },
                                HookState::Release {
                                    state,
                                    weight,
                                    ..
                                } => {
                                    if state.is_involved(key) {
                                        Some((*weight, (hook_id, hook)))
                                    } else {
                                        None
                                    }
                                },
                                HookState::Hold {
                                    state,
                                    weight,
                                    ..
                                } => {
                                    if state.is_involved(key) {
                                        Some((*weight, (hook_id, hook)))
                                    } else {
                                        None
                                    }
                                },
                                _ => None,
                            }
                        } else {
                            None
                        }
                    })
                    .collect();

                call_in_order.sort_by_key(|(weight, _)| Reverse(*weight));

                for (weight, (hook_id, hook)) in call_in_order {
                    match &mut hook.state {
                        HookState::Press {
                            state,
                            method,
                            ..
                        } => {
                            match hook.target_wk.upgrade() {
                                Some(hook_target) => {
                                    match method(hook_target, window_state, state) {
                                        InputHookCtrl::Retain => (),
                                        InputHookCtrl::RetainNoPass => {
                                            if weight != NO_HOOK_WEIGHT {
                                                break;
                                            }
                                        },
                                        InputHookCtrl::Remove => {
                                            remove_hooks.push(*hook_id);
                                        },
                                        InputHookCtrl::RemoveNoPass => {
                                            remove_hooks.push(*hook_id);

                                            if weight != NO_HOOK_WEIGHT {
                                                break;
                                            }
                                        },
                                    }
                                },
                                None => {
                                    remove_hooks.push(*hook_id);
                                },
                            }
                        },
                        HookState::Release {
                            state,
                            pressed,
                            ..
                        } => {
                            if state.update(key, true) {
                                *pressed = true;
                            }
                        },
                        HookState::Hold {
                            state,
                            pressed,
                            intvl_id,
                            ..
                        } => {
                            if state.update(key, true) {
                                *pressed = true;
                                interval.start(*intvl_id);
                            }
                        },
                        _ => unreachable!(),
                    }
                }
            }
        }

        for hook_id in remove_hooks {
            hooks.remove(&hook_id);
        }
    }
}
