use crate::input::state::{HookState, WindowState};
use crate::input::{Hook, InputHookCtrl, InputHookID, InputHookTargetID, NO_HOOK_WEIGHT};
use crate::interface::Interface;
use crate::window::BstWindowID;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

pub(in crate::input) fn scroll(
	interface: &Arc<Interface>,
	hooks: &mut HashMap<InputHookID, Hook>,
	win_state: &mut HashMap<BstWindowID, WindowState>,
	win: BstWindowID,
	ss: bool,
	v: f32,
	h: f32,
) {
	let window_state = win_state.entry(win).or_insert_with(|| WindowState::new(win));
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

						Some((*weight, *hook_id, hook))
					},
					InputHookTargetID::Bin(hook_bin) => {
						if !inside_bin_ids.contains(hook_bin) {
							return None;
						}

						if *top && inside_bin_ids[0] != *hook_bin {
							return None;
						}

						if *focus && focused_bin_id == Some(*hook_bin) {
							return None;
						}

						Some((*weight, *hook_id, hook))
					},
					_ => None,
				}
			} else {
				None
			}
		})
		.collect();

	call_in_order.sort_by_key(|(weight, ..)| Reverse(*weight));
	let mut remove_hooks = Vec::new();

	for (weight, hook_id, hook) in call_in_order {
		if let HookState::Scroll {
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

			match method(hook_target, window_state, v, h) {
				InputHookCtrl::Retain => (),
				InputHookCtrl::RetainNoPass =>
					if weight != NO_HOOK_WEIGHT {
						break;
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
