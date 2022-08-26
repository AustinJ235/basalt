use crate::input_v2::state::WindowState;
use crate::input_v2::{Hook, InputHookID};
use crate::interface::Interface;
use crate::window::BstWindowID;
use std::collections::HashMap;
use std::sync::Arc;

pub(in crate::input_v2) fn cursor(
	_interface: &Arc<Interface>,
	_hooks: &mut HashMap<InputHookID, Hook>,
	win_state: &mut HashMap<BstWindowID, WindowState>,
	win: BstWindowID,
	x: f32,
	y: f32,
) {
	let window_state = win_state.entry(win).or_insert_with(|| WindowState::new(win));

	if window_state.update_cursor_pos(x, y) {}
}
