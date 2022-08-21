use super::{
	GlobalKeyState, Hook, InputError, InputHookCtrl, InputHookID, InputHookTarget,
	InputHookTargetID, InputV2, Key, LocalKeyState,
};
use crate::interface::bin::Bin;
use crate::window::BasaltWindow;
use std::collections::HashMap;
use std::sync::Arc;

pub struct InputHookBuilder {
	input: Arc<InputV2>,
	target: InputHookTargetID,
	hook: Option<Hook>,
}

impl InputHookBuilder {
	pub(super) fn start(input: Arc<InputV2>) -> Self {
		Self {
			input,
			target: InputHookTargetID::None,
			hook: None,
		}
	}

	/// Attach input hook to a `Bin`
	///
	/// # Notes
	/// - Overrides previously used `bin`.
	pub fn window(mut self, window: &Arc<dyn BasaltWindow>) -> Self {
		self.target = InputHookTargetID::Window(window.id());
		self
	}

	/// Attach input hook to a `Bin`
	///
	/// # Notes
	/// - Overrides previously used `window`.
	pub fn bin(mut self, bin: &Arc<Bin>) -> Self {
		self.target = InputHookTargetID::Bin(bin.id());
		self
	}

	/// Trigger on a press
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_press(self) -> InputPressBuilder {
		InputPressBuilder::start(self)
	}

	/// Submit the created hook to `Input`.
	///
	/// # Possible Errors
	/// `NoTrigger`: No `on_` method was called.
	/// `NoTarget`: The trigger requires either `bin()` or `window()` to be called.
	pub fn submit(self) -> Result<InputHookID, InputError> {
		let hook = self.hook.ok_or(InputError::NoTrigger)?;

		match hook {
			Hook::Press {
				..
			} =>
				if self.target == InputHookTargetID::None {
					return Err(InputError::NoTarget);
				},
		}

		Ok(self.input.add_hook(hook))
	}
}

pub struct InputPressBuilder {
	parent: InputHookBuilder,
	keys: Vec<Key>,
	weight: i16,
	method: Option<
		Box<
			dyn FnMut(InputHookTarget, &GlobalKeyState, &LocalKeyState) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	>,
}

impl InputPressBuilder {
	fn start(parent: InputHookBuilder) -> Self {
		Self {
			parent,
			keys: Vec::new(),
			weight: i16::min_value(),
			method: None,
		}
	}

	/// Add a `Key` to the combination used.
	///
	/// # Notes
	/// - This adds to any previous `on_key` or `on_keys` calls.
	pub fn key<K: Into<Key>>(mut self, key: K) -> Self {
		self.keys.push(key.into());
		self
	}

	/// Add multiple `Key`'s to the combination used.
	///
	/// # Notes
	/// - This adds to any previous `on_key` or `on_keys` calls.
	pub fn keys<K: Into<Key>, L: IntoIterator<Item = K>>(mut self, keys: L) -> Self {
		keys.into_iter().for_each(|key| self.keys.push(key.into()));
		self
	}

	/// Assigns a weight.
	///
	/// # Notes
	/// - This overrides any previous `weight` call.
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - This overrides any previous `call` or `call_arcd`.
	pub fn call<
		F: FnMut(InputHookTarget, &GlobalKeyState, &LocalKeyState) -> InputHookCtrl
			+ Send
			+ 'static,
	>(
		mut self,
		method: F,
	) -> Self {
		self.method = Some(Box::new(method));
		self
	}

	/// Assign a `Arc`'d function to call.
	///
	/// # Notes
	/// - This overrides any previous `call` or `call_arcd`.
	pub fn call_arcd(
		mut self,
		method: Arc<
			dyn Fn(InputHookTarget, &GlobalKeyState, &LocalKeyState) -> InputHookCtrl
				+ Send
				+ Sync,
		>,
	) -> Self {
		self.method =
			Some(Box::new(move |target, global, local| method(target, global, local)));
		self
	}

	/// Finish building a press returning the `InputHookBuilder`.
	///
	/// # Possible Errors
	/// - `NoKeys`: No call to `key` or `keys` was made.
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	pub fn finish(mut self) -> Result<InputHookBuilder, InputError> {
		if self.keys.is_empty() {
			Err(InputError::NoKeys)
		} else if self.method.is_none() {
			Err(InputError::NoMethod)
		} else {
			// NOTE: HashMap guarentees deduplication

			self.parent.hook = Some(Hook::Press {
				state: HashMap::from_iter(self.keys.into_iter().map(|key| (key, false))),
				active: false,
				weight: self.weight,
				method: self.method.unwrap(),
			});

			Ok(self.parent)
		}
	}
}
