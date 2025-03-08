use std::cmp::Reverse;

use foldhash::HashMap;

use crate::input::state::HookState;
use crate::input::{Hook, InputHookCtrl, InputHookID, NO_HOOK_WEIGHT};

pub(in crate::input) fn motion(hooks: &mut HashMap<InputHookID, Hook>, x: f32, y: f32) {
    let mut call_in_order: Vec<_> = hooks
        .iter_mut()
        .filter_map(|(hook_id, hook)| {
            if let HookState::Motion {
                weight, ..
            } = &mut hook.state
            {
                Some((*weight, *hook_id, hook))
            } else {
                None
            }
        })
        .collect();

    call_in_order.sort_by_key(|(weight, ..)| Reverse(*weight));
    let mut remove_hooks = Vec::new();

    for (weight, hook_id, hook) in call_in_order {
        if let HookState::Motion {
            method, ..
        } = &mut hook.state
        {
            match method(x, y) {
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
