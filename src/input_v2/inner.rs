use super::{
	Hook, HookState, InputEvent, InputHookCtrl, InputHookID, Key, MouseButton, WindowKeyState,
};
use crate::interface::bin::BinID;
use crate::interface::Interface;
use crate::window::BstWindowID;
use crossbeam::channel::Receiver;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

const BIN_FOCUS_KEY: Key = Key::Mouse(MouseButton::Left);
const NO_HOOK_WEIGHT: i16 = i16::min_value();

pub(super) enum LoopEvent {
	Normal(InputEvent),
	Add {
		id: InputHookID,
		hook: Hook,
	},
	Remove(InputHookID),
}

#[derive(Default)]
struct WindowState {
	focus_bin: Option<BinID>,
	key_state: WindowKeyState,
	cursor_pos: [f32; 2],
}

pub(super) fn begin_loop(interface: Arc<Interface>, event_recv: Receiver<LoopEvent>) {
	thread::spawn(move || {
		let mut hooks: HashMap<InputHookID, Hook> = HashMap::new();
		let mut win_state: HashMap<BstWindowID, WindowState> = HashMap::new();

		while let Ok(event) = event_recv.recv() {
			match event {
				LoopEvent::Add {
					id,
					hook,
				} => {
					hooks.insert(id, hook);
				},
				LoopEvent::Remove(id) => {
					hooks.remove(&id);
				},
				LoopEvent::Normal(event) =>
					match event {
						InputEvent::Press {
							win,
							key,
						} => {
							let window_state =
								win_state.entry(win).or_insert_with(|| Default::default());

							// Returns true if the state changed
							if window_state.key_state.update(key, true) {
								let mut call_in_order: Vec<_> = hooks
									.iter_mut()
									.filter_map(|(hook_id, hook)| {
										if hook.is_for_window_id(win) {
											if let HookState::Press {
												state,
												weight,
												..
											} = &mut hook.data
											{
												if state.update(key, true) {
													Some((*weight, (hook_id, hook)))
												} else {
													None
												}
											} else {
												None
											}
										} else {
											None
										}
									})
									.collect();

								call_in_order.sort_by_key(|(weight, _)| Reverse(*weight));
								let mut pass_bin_event = true;
								let mut remove_hooks: Vec<InputHookID> = Vec::new();

								for (weight, (hook_id, hook)) in call_in_order {
									if let HookState::Press {
										state,
										method,
										..
									} = &mut hook.data
									{
										match hook.target_wk.upgrade() {
											Some(hook_target) =>
												match method(
													hook_target,
													&window_state.key_state,
													&state,
												) {
													InputHookCtrl::Retain => (),
													InputHookCtrl::RetainNoPass =>
														if weight != NO_HOOK_WEIGHT {
															pass_bin_event = false;
															break;
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
												},
											None => {
												remove_hooks.push(*hook_id);
											},
										}
									}
								}

								if pass_bin_event {
									// Check Bin Focus
									if key == BIN_FOCUS_KEY {
										let new_focus_bin = interface.get_bin_id_atop(
											win,
											window_state.cursor_pos[0],
											window_state.cursor_pos[1],
										);

										// Bin Focus Changed
										if new_focus_bin != window_state.focus_bin {
											if let Some(_old_focus_bin) =
												window_state.focus_bin.take()
											{
												// TODO: Call FocusLost, Release (on active ones), Stop Hold (on active ones)
											}

											match new_focus_bin {
												Some(new_focus_bin) => {
													// TODO: Call Focus on new_focus_bin
													window_state.focus_bin =
														Some(new_focus_bin);
												},
												None => {
													window_state.focus_bin = None;
												},
											}
										}
									}

									if let Some(focus_bin) = &window_state.focus_bin {
										let mut call_in_order: Vec<_> = hooks
											.iter_mut()
											.filter_map(|(hook_id, hook)| {
												if hook.is_for_bin_id(*focus_bin) {
													if let HookState::Press {
														state,
														weight,
														..
													} = &mut hook.data
													{
														if state.update(key, true) {
															Some((*weight, (hook_id, hook)))
														} else {
															None
														}
													} else {
														None
													}
												} else {
													None
												}
											})
											.collect();

										call_in_order
											.sort_by_key(|(weight, _)| Reverse(*weight));

										for (weight, (hook_id, hook)) in call_in_order {
											if let HookState::Press {
												state,
												method,
												..
											} = &mut hook.data
											{
												match hook.target_wk.upgrade() {
													Some(hook_target) =>
														match method(
															hook_target,
															&window_state.key_state,
															&state,
														) {
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
														},
													None => {
														remove_hooks.push(*hook_id);
													},
												}
											}
										}
									}
								}

								for hook_id in remove_hooks {
									hooks.remove(&hook_id);
								}
							}
						},
						InputEvent::Cursor {
							win,
							x,
							y,
						} => {
							let window_state =
								win_state.entry(win).or_insert_with(|| Default::default());
							window_state.cursor_pos[0] = x;
							window_state.cursor_pos[1] = y;
						},
						_ => (), // TODO
					},
			}
		}
	});
}
