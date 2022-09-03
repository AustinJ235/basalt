use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use crate::input::state::{HookState, WindowState};
use crate::input::{Hook, InputHookCtrl, InputHookID, InputHookTargetID, NO_HOOK_WEIGHT};
use crate::interface::Interface;
use crate::window::BstWindowID;

pub(in crate::input) fn scroll(
    interface: &Arc<Interface>,
    hooks: &mut HashMap<InputHookID, Hook>,
    win_state: &mut HashMap<BstWindowID, WindowState>,
    win: BstWindowID,
    ss: bool,
    v: f32,
    h: f32,
) {
    let window_state = win_state
        .entry(win)
        .or_insert_with(|| WindowState::new(win));
    let [x, y] = window_state.cursor_pos();
    let inside_bin_ids = interface.get_bin_ids_atop(win, x, y);
    let focused_bin_id = window_state.focused_bin_id();

    let mut call_in_order: Vec<_> = hooks
        .iter_mut()
        .filter_map(|(hook_id, hook)| {
            if let HookState::Scroll {
                weight,
                top,
                focus,
                smooth,
                ..
            } = &mut hook.state
            {
                if ss != *smooth {
                    return None;
                }

                match &hook.target_id {
                    InputHookTargetID::Window(hook_win) => {
                        if *hook_win != win {
                            return None;
                        }

                        if *focus && !window_state.is_focused() {
                            return None;
                        }

                        Some((*weight, 0, *hook_id, hook))
                    },
                    InputHookTargetID::Bin(hook_bin) => {
                        let mut inside_i_op: Option<usize> = None;

                        for (i, inside_id) in inside_bin_ids.iter().enumerate() {
                            if *hook_bin == *inside_id {
                                inside_i_op = Some(i);
                                break;
                            }
                        }

                        let inside_i = match inside_i_op {
                            Some(some) => some,
                            None => return None,
                        };

                        if *top && inside_i != 0 {
                            return None;
                        }

                        if *focus && focused_bin_id == Some(*hook_bin) {
                            return None;
                        }

                        Some((*weight, inside_i + 1, *hook_id, hook))
                    },
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect();

    call_in_order.sort_by_key(|(weight, z, ..)| (Reverse(*weight), *z));
    let mut remove_hooks = Vec::new();
    let mut last_weight_z_order = None;

    for (weight, z_order, hook_id, hook) in call_in_order {
        if let HookState::Scroll {
            method,
            upper_blocks,
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

            // z_order of zero is a window
            if *upper_blocks && !z_order != 0 {
                if let Some((last_weight, last_z_order)) = &last_weight_z_order {
                    if *last_weight == weight && *last_z_order != 0 && *last_z_order < z_order {
                        continue;
                    }
                }
            }

            last_weight_z_order = Some((weight, z_order));

            match method(hook_target, window_state, v, h) {
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
