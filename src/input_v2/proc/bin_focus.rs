use crate::input_v2::state::{HookState, WindowState};
use crate::input_v2::{Hook, InputHookCtrl, InputHookID, NO_HOOK_WEIGHT};
use crate::interface::bin::BinID;
use std::cmp::Reverse;
use std::collections::HashMap;

pub(in crate::input_v2) fn bin_focus(
	hooks: &mut HashMap<InputHookID, Hook>,
	window_state: &mut WindowState,
	old_bin_id_op: Option<BinID>,
	new_bin_id_op: Option<BinID>,
) {
	let mut remove_hooks = Vec::new();

	if let Some(old_bin_id) = old_bin_id_op {
		let mut call_release_on = Vec::new();
		let mut call_focus_lost_on = Vec::new();

		for (hook_id, hook) in hooks.iter_mut() {
			if hook.is_for_bin_id(old_bin_id) {
				match &hook.data {
					HookState::Release {
						pressed,
						weight,
						..
					} if *pressed => {
						call_release_on.push((*weight, (hook_id, hook)));
					},
					// TODO: HookState::Hold
					HookState::FocusLost {
						weight,
						..
					} => {
						call_focus_lost_on.push((*weight, (hook_id, hook)));
					},
					_ => (),
				}
			}
		}

		call_release_on.sort_by_key(|(weight, _)| Reverse(*weight));
		call_focus_lost_on.sort_by_key(|(weight, _)| Reverse(*weight));
		let mut call_release_method = true;

		for (weight, (hook_id, hook)) in call_release_on {
			let hook_target = match hook.target_wk.upgrade() {
				Some(some) => some,
				None => {
					remove_hooks.push(*hook_id);
					continue;
				},
			};

			match &mut hook.data {
				HookState::Release {
					state,
					pressed,
					method,
					..
				} => {
					state.release_all();
					*pressed = false;

					if call_release_method {
						match method(hook_target, &window_state, &state) {
							InputHookCtrl::Retain => (),
							InputHookCtrl::RetainNoPass =>
								if weight != NO_HOOK_WEIGHT {
									call_release_method = false;
								},
							InputHookCtrl::Remove => {
								remove_hooks.push(*hook_id);
							},
							InputHookCtrl::RemoveNoPass => {
								remove_hooks.push(*hook_id);

								if weight != NO_HOOK_WEIGHT {
									call_release_method = false;
								}
							},
						}
					}
				},
				// TODO: HookState::Hold
				_ => (),
			}
		}

		for (weight, (hook_id, hook)) in call_focus_lost_on {
			let hook_target = match hook.target_wk.upgrade() {
				Some(some) => some,
				None => {
					remove_hooks.push(*hook_id);
					continue;
				},
			};

			if let HookState::FocusLost {
				method,
				..
			} = &mut hook.data
			{
				match method(hook_target, &window_state) {
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
	}

	if let Some(new_bin_id) = new_bin_id_op {
		let mut call_focus_on: Vec<_> = hooks
			.iter_mut()
			.filter_map(|(hook_id, hook)| {
				if hook.is_for_bin_id(new_bin_id) {
					if let HookState::Focus {
						weight,
						..
					} = &hook.data
					{
						Some((*weight, (hook_id, hook)))
					} else {
						None
					}
				} else {
					None
				}
			})
			.collect();

		call_focus_on.sort_by_key(|(weight, _)| Reverse(*weight));

		for (weight, (hook_id, hook)) in call_focus_on {
			let hook_target = match hook.target_wk.upgrade() {
				Some(some) => some,
				None => {
					remove_hooks.push(*hook_id);
					continue;
				},
			};

			if let HookState::Focus {
				method,
				..
			} = &mut hook.data
			{
				match method(hook_target, &window_state) {
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
			}
		}
	}

	for hook_id in remove_hooks {
		hooks.remove(&hook_id);
	}
}
