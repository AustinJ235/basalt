 use crate::input_v2::{InputHookCtrl, InputHookTarget, Key};
use crate::interface::bin::BinID;
use crate::interface::Interface;
use crate::window::BstWindowID;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use crate::interval::IntvlHookID;

#[derive(Debug)]
pub struct WindowState {
	window_id: BstWindowID,
	key_state: HashMap<Key, bool>,
	focus_bin: Option<BinID>,
	cursor_pos: [f32; 2],
	focused: bool,
	cursor_inside: bool,
}

impl WindowState {
	pub(in crate::input_v2) fn new(window_id: BstWindowID) -> Self {
		Self {
			window_id,
			key_state: HashMap::new(),
			focus_bin: None,
			cursor_pos: [0.0; 2],
			focused: true,
			cursor_inside: true,
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

	// If changed returns true
	pub(in crate::input_v2) fn update_cursor_pos(&mut self, x: f32, y: f32) -> bool {
		if x != self.cursor_pos[0] || y != self.cursor_pos[1] {
			self.cursor_pos[0] = x;
			self.cursor_pos[1] = y;
			true
		} else {
			false
		}
	}

	// If changed returns true
	pub(in crate::input_v2) fn update_focus(&mut self, focus: bool) -> bool {
		if self.focused != focus {
			self.focused = focus;
			true
		} else {
			false
		}
	}

	// If changed returns true
	pub(in crate::input_v2) fn update_cursor_inside(&mut self, inside: bool) -> bool {
		if self.cursor_inside != inside {
			self.cursor_inside = inside;
			true
		} else {
			false
		}
	}

	/// Returns the `BstWindowID` this state corresponds to.
	pub fn window_id(&self) -> BstWindowID {
		self.window_id
	}

	/// Returns `true` if the window is focused.
	pub fn is_focused(&self) -> bool {
		self.focused
	}

	/// Returns `true` if the cursor is inside.
	pub fn is_cursor_inside(&self) -> bool {
		self.cursor_inside
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

	pub(in crate::input_v2) fn release_all(&mut self) {
		self.state.values_mut().for_each(|state| *state = false);
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

/// State of cursor in between hook calls.
#[derive(Default)]
pub struct LocalCursorState {
	old: Option<[f32; 2]>,
	delta: Option<[f32; 2]>,
	top_most: bool,
}

impl LocalCursorState {
	pub(in crate::input_v2) fn new() -> Self {
		Self {
			old: None,
			delta: None,
			top_most: false,
		}
	}

	pub(in crate::input_v2) fn reset(&mut self) {
		self.delta = None;
		self.old = None;
		self.top_most = false;
	}

	pub(in crate::input_v2) fn update_delta(&mut self, x: f32, y: f32) {
		if let Some([old_x, old_y]) = self.old.take() {
			self.delta = Some([x - old_x, y - old_y]);
		}

		self.old = Some([x, y]);
	}

	pub(in crate::input_v2) fn update_top_most(&mut self, top: bool) {
		self.top_most = top;
	}

	/// The delta between the last cursor position and the current position.
	pub fn delta(&self) -> Option<[f32; 2]> {
		self.delta
	}

	/// If the target is top-most.
	pub fn target_is_top_most(&self) -> bool {
		self.top_most
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
	Hold {
		state: LocalKeyState,
		pressed: bool,
		weight: i16,
		intvl_id: IntvlHookID,
		method: Box<
			dyn FnMut(InputHookTarget, &WindowState, &LocalKeyState, Option<Duration>) -> InputHookCtrl
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
	Enter {
		weight: i16,
		top: bool,
		inside: bool,
		pass: bool,
		method: Box<dyn FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>,
	},
	Leave {
		weight: i16,
		top: bool,
		inside: bool,
		method: Box<dyn FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>,
	},
	Focus {
		weight: i16,
		method: Box<dyn FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>,
	},
	FocusLost {
		weight: i16,
		method: Box<dyn FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>,
	},
	Cursor {
		state: LocalCursorState,
		weight: i16,
		top: bool,
		focus: bool,
		inside: bool,
		method: Box<
			dyn FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	},
	None,
}

impl HookState {
	pub fn requires_target(&self) -> bool {
		match self {
			Self::None => false,
			_ => true,
		}
	}
}
