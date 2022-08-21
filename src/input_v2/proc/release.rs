use crate::input_v2::state::{HookState, WindowState};
use crate::input_v2::{Hook, InputHookCtrl, InputHookID, Key};
use crate::window::BstWindowID;
use std::collections::HashMap;

pub(in crate::input_v2) fn release(
	hooks: &mut HashMap<InputHookID, Hook>,
	win_state: &mut HashMap<BstWindowID, WindowState>,
	win: BstWindowID,
	key: Key,
) {
	let window_state = win_state.entry(win).or_insert_with(|| WindowState::new(win));

	if window_state.update_key(key, false) {
		let mut remove_hooks: Vec<InputHookID> = Vec::new();
		let focused_bin_id = window_state.focused_bin_id();

		for (hook_id, hook) in hooks.iter_mut() {
			if hook.is_for_window_id(win)
				|| (focused_bin_id.is_some() && hook.is_for_bin_id(focused_bin_id.unwrap()))
			{
				if let HookState::Release {
					state,
					method,
					pressed,
					..
				} = &mut hook.data
				{
					if !state.update(key, false) && *pressed {
						*pressed = false;

						match hook.target_wk.upgrade() {
							Some(hook_target) => {
								match method(hook_target, &window_state, &state) {
									InputHookCtrl::Retain | InputHookCtrl::RetainNoPass => (),
									InputHookCtrl::Remove | InputHookCtrl::RemoveNoPass => {
										remove_hooks.push(*hook_id);
									},
								}
							},
							None => {
								remove_hooks.push(*hook_id);
							},
						}
					}
				}
			}
		}
	}
}
