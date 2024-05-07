use std::cmp::Reverse;
use std::collections::HashMap;

use crate::input::state::{HookState, WindowState};
use crate::input::{Hook, InputHookCtrl, InputHookID, NO_HOOK_WEIGHT};
use crate::window::WindowID;

pub(in crate::input) fn character(
    hooks: &mut HashMap<InputHookID, Hook>,
    win_state: &mut HashMap<WindowID, WindowState>,
    win: WindowID,
    c: char,
) {
    let window_state = win_state
        .entry(win)
        .or_insert_with(|| WindowState::new(win));

    let is_valid_target: Box<dyn Fn(&Hook) -> bool> = match window_state.focused_bin_id() {
        Some(bin) => {
            Box::new(move |hook: &Hook| -> bool {
                hook.is_for_window_id(win) || hook.is_for_bin_id(bin)
            })
        },
        None => Box::new(|hook: &Hook| -> bool { hook.is_for_window_id(win) }),
    };

    let mut call_in_order: Vec<_> = hooks
        .iter_mut()
        .filter_map(|(hook_id, hook)| {
            if is_valid_target(hook) {
                if let HookState::Character {
                    weight, ..
                } = &mut hook.state
                {
                    Some((*weight, *hook_id, hook))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    call_in_order.sort_by_key(|(weight, ..)| Reverse(*weight));
    let mut remove_hooks = Vec::new();

    for (weight, hook_id, hook) in call_in_order {
        if let HookState::Character {
            method, ..
        } = &mut hook.state
        {
            let hook_target = match hook.target_wk.upgrade() {
                Some(some) => some,
                None => {
                    remove_hooks.push(hook_id);
                    continue;
                },
            };

            match method(hook_target, window_state, c.into()) {
                InputHookCtrl::Retain => (),
                InputHookCtrl::RetainNoPass => {
                    if weight != NO_HOOK_WEIGHT {
                        break;
                    }
                },
                InputHookCtrl::Remove => {
                    remove_hooks.push(hook_id);
                },
                InputHookCtrl::RemoveNoPass => {
                    remove_hooks.push(hook_id);

                    if weight != NO_HOOK_WEIGHT {
                        break;
                    }
                },
            }
        } else {
            unreachable!()
        }
    }

    for hook_id in remove_hooks {
        hooks.remove(&hook_id);
    }
}
