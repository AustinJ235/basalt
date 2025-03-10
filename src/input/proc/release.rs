use core::cmp::Reverse;
use std::sync::Arc;

use foldhash::HashMap;

use crate::input::state::{HookState, WindowState};
use crate::input::{Hook, InputHookCtrl, InputHookID, Key, NO_HOOK_WEIGHT};
use crate::interval::Interval;
use crate::window::WindowID;

pub(in crate::input) fn release(
    interval: &Arc<Interval>,
    hooks: &mut HashMap<InputHookID, Hook>,
    win_state: &mut HashMap<WindowID, WindowState>,
    win: WindowID,
    key: Key,
) {
    let window_state = win_state
        .entry(win)
        .or_insert_with(|| WindowState::new(win));

    if window_state.update_key(key, false) {
        let focused_bin_id = window_state.focused_bin_id();
        let mut remove_hooks: Vec<InputHookID> = Vec::new();

        let mut call_release_on: Vec<_> = hooks
            .iter_mut()
            .filter_map(|(hook_id, hook)| {
                if hook.is_for_window_id(win)
                    || (focused_bin_id.is_some() && hook.is_for_bin_id(focused_bin_id.unwrap()))
                {
                    match &mut hook.state {
                        HookState::Release {
                            state,
                            pressed,
                            weight,
                            ..
                        } => {
                            if state.is_involved(key) && !state.update(key, false) && *pressed {
                                *pressed = false;
                                Some((*weight, (hook_id, hook)))
                            } else {
                                None
                            }
                        },
                        HookState::Press {
                            state, ..
                        } => {
                            state.update(key, false);
                            None
                        },
                        HookState::Hold {
                            state,
                            pressed,
                            intvl_id,
                            ..
                        } => {
                            if state.is_involved(key) && !state.update(key, false) && *pressed {
                                *pressed = false;
                                interval.pause(*intvl_id);
                            }

                            None
                        },
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect();

        call_release_on.sort_by_key(|(weight, _)| Reverse(*weight));

        for (weight, (hook_id, hook)) in call_release_on {
            let hook_target = match hook.target_wk.upgrade() {
                Some(some) => some,
                None => {
                    remove_hooks.push(*hook_id);
                    continue;
                },
            };

            if let HookState::Release {
                state,
                method,
                ..
            } = &mut hook.state
            {
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
            } else {
                unreachable!()
            }
        }
    }
}
