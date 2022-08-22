use crate::input_v2::state::{HookState, WindowState};
use crate::input_v2::{Hook, InputHookCtrl, InputHookID, NO_HOOK_WEIGHT};
use crate::window::BstWindowID;
use std::cmp::Reverse;
use std::collections::HashMap;

macro_rules! call_hook_varient {
	($hooks:ident, $window_state:ident, $varient:ident) => {
		let mut remove_hooks = Vec::new();

		let mut call_on: Vec<_> = $hooks
			.iter_mut()
			.filter_map(|(hook_id, hook)| {
				if hook.is_for_window_id($window_state.window_id()) {
					if let HookState::$varient {
						weight,
						..
					} = &hook.state
					{
						Some((*weight, hook_id, hook))
					} else {
						None
					}
				} else {
					None
				}
			})
			.collect();

		call_on.sort_by_key(|(weight, ..)| Reverse(*weight));

		for (weight, hook_id, hook) in call_on {
			let hook_target = match hook.target_wk.upgrade() {
				Some(some) => some,
				None => {
					remove_hooks.push(*hook_id);
					continue;
				},
			};

			if let HookState::$varient {
				method,
				..
			} = &mut hook.state
			{
				match method(hook_target, $window_state) {
					InputHookCtrl::Retain => (),
					InputHookCtrl::RetainNoPass =>
						if weight != NO_HOOK_WEIGHT {
							break;
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

		for hook_id in remove_hooks {
			$hooks.remove(&hook_id);
		}
	};
}

pub(in crate::input_v2) fn win_focus(
	hooks: &mut HashMap<InputHookID, Hook>,
	win_state: &mut HashMap<BstWindowID, WindowState>,
	win: BstWindowID,
	focused: bool,
) {
	let window_state = win_state.entry(win).or_insert_with(|| WindowState::new(win));

	if window_state.update_focus(focused) {
		if focused {
			call_hook_varient!(hooks, window_state, Focus);
		} else {
			call_hook_varient!(hooks, window_state, FocusLost);
		}
	}
}
