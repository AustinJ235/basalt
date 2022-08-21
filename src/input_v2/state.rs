use crate::input_v2::{InputHookCtrl, InputHookTarget, Key};
use crate::interface::bin::BinID;
use crate::interface::Interface;
use crate::window::BstWindowID;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
pub struct WindowState {
	window_id: BstWindowID,
	key_state: HashMap<Key, bool>,
	focus_bin: Option<BinID>,
	cursor_pos: [f32; 2],
}

impl WindowState {
	pub(in crate::input_v2) fn new(window_id: BstWindowID) -> Self {
		Self {
			window_id,
			key_state: HashMap::new(),
			focus_bin: None,
			cursor_pos: [0.0; 2],
		}
	}

	// Returns true if state changed.
	pub(in crate::input_v2) fn update_key(&mut self, key: Key, key_state: bool) -> bool {
		let mut changed = false;

		let current = self.key_state.entry(key).or_insert_with(|| {
			changed = true;
			key_state
		});

		if !changed && *current != key_state {
			changed = true;
			*current = key_state;
		}

		changed
	}

	// If changed returns (old, new)
	pub(in crate::input_v2) fn update_focus_bin(
		&mut self,
		interface: &Arc<Interface>,
	) -> Option<(Option<BinID>, Option<BinID>)> {
		let new_bin_id_op =
			interface.get_bin_id_atop(self.window_id, self.cursor_pos[0], self.cursor_pos[1]);

		if new_bin_id_op != self.focus_bin {
			let old_bin_id_op = self.focus_bin.take();
			self.focus_bin = new_bin_id_op;
			Some((old_bin_id_op, new_bin_id_op))
		} else {
			None
		}
	}

	// If changed returns provided cursor position
	pub(in crate::input_v2) fn update_cursor_pos(
		&mut self,
		x: f32,
		y: f32,
	) -> Option<[f32; 2]> {
		if x != self.cursor_pos[0] || y != self.cursor_pos[1] {
			self.cursor_pos[0] = x;
			self.cursor_pos[1] = y;
			Some(self.cursor_pos)
		} else {
			None
		}
	}

	/// Returns the `BinID` of the currently focused `Bin`.
	pub fn focused_bin_id(&self) -> Option<BinID> {
		self.focus_bin
	}

	/// Returns the current cursor position.
	pub fn cursor_pos(&self) -> [f32; 2] {
		self.cursor_pos
	}

	/// Check if a `Key` is pressed.
	///
	/// Supports using `Qwerty` or `MouseButton`.
	pub fn is_key_pressed<K: Into<Key>>(&self, key: K) -> bool {
		let key = key.into();
		self.key_state.get(&key).copied().unwrap_or(false)
	}
}

#[derive(Debug)]
pub struct LocalKeyState {
	state: HashMap<Key, bool>,
}

impl LocalKeyState {
	pub(in crate::input_v2) fn from_keys<K: IntoIterator<Item = Key>>(keys: K) -> Self {
		Self {
			state: HashMap::from_iter(keys.into_iter().map(|key| (key, false))),
		}
	}

	// Returns true if all keys where not pressed before, but now are.
	pub(in crate::input_v2) fn update(&mut self, key: Key, key_state: bool) -> bool {
		let all_before = self.state.values().all(|state| *state);

		let check_again = match self.state.get_mut(&key) {
			Some(current) =>
				if *current != key_state {
					*current = key_state;
					true
				} else {
					false
				},
			None => false,
		};

		if check_again {
			let all_after = self.state.values().all(|state| *state);

			if all_after {
				if all_before {
					false
				} else {
					true
				}
			} else {
				false
			}
		} else {
			false
		}
	}

	/// Check if a key is pressed in the scope of this hook.
	pub fn is_pressed<K: Into<Key>>(&self, key: K) -> bool {
		let key = key.into();
		self.state.get(&key).copied().unwrap_or(false)
	}

	/// Check if a key is involved in this hook.
	///
	/// This may be useful where multiple hooks call the same method.
	pub fn is_involved<K: Into<Key>>(&self, key: K) -> bool {
		self.state.contains_key(&key.into())
	}
}

pub(in crate::input_v2) enum HookState {
	Press {
		state: LocalKeyState,
		weight: i16,
		method: Box<
			dyn FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	},
	Release {
		state: LocalKeyState,
		pressed: bool,
		weight: i16,
		method: Box<
			dyn FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	},
	None,
}
