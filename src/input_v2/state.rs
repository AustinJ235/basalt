use super::{InputHookCtrl, InputHookTarget, Key};
use std::collections::HashMap;

pub struct GlobalKeyState {
	state: HashMap<Key, bool>,
}

impl GlobalKeyState {
	pub(super) fn new() -> Self {
		Self {
			// TODO: Set all Qwerty & MouseButton?
			state: HashMap::new(),
		}
	}

	// Returns true if state changed.
	pub(super) fn update(&mut self, key: Key, key_state: bool) -> bool {
		let mut changed = false;

		let current = self.state.entry(key).or_insert_with(|| {
			changed = true;
			key_state
		});

		if !changed && *current != key_state {
			changed = true;
			*current = key_state;
		}

		changed
	}

	/// Check if a key is pressed globally.
	pub fn is_pressed<K: Into<Key>>(&self, key: K) -> bool {
		let key = key.into();
		self.state.get(&key).copied().unwrap_or(false)
	}
}

pub struct LocalKeyState {
	state: HashMap<Key, bool>,
}

impl LocalKeyState {
	pub(super) fn from_keys<K: IntoIterator<Item = Key>>(keys: K) -> Self {
		Self {
			state: HashMap::from_iter(keys.into_iter().map(|key| (key, false))),
		}
	}

	// Returns true if all keys where not pressed before, but now are.
	pub(super) fn update(&mut self, key: Key, key_state: bool) -> bool {
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

pub(super) enum HookState {
	Press {
		state: LocalKeyState,
		active: bool,
		weight: i16,
		method: Box<
			dyn FnMut(InputHookTarget, &GlobalKeyState, &LocalKeyState) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	},
	Release {
		state: LocalKeyState,
		active: bool,
		weight: i16,
		method: Box<
			dyn FnMut(InputHookTarget, &GlobalKeyState, &LocalKeyState) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	},
}
