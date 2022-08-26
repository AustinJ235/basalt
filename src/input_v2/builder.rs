use crate::input_v2::state::{HookState, LocalKeyState, WindowState};
use crate::input_v2::{
	Hook, InputError, InputHookCtrl, InputHookID, InputHookTarget, InputV2, Key, NO_HOOK_WEIGHT,
};
use crate::interface::bin::Bin;
use crate::window::BasaltWindow;
use std::sync::Arc;

pub struct InputHookBuilder<'a> {
	input: &'a InputV2,
	target: InputHookTarget,
	hook: Option<HookState>,
}

impl<'a> InputHookBuilder<'a> {
	pub(in crate::input_v2) fn start(input: &'a InputV2) -> Self {
		Self {
			input,
			target: InputHookTarget::None,
			hook: None,
		}
	}

	/// Attach input hook to a `Bin`
	///
	/// # Notes
	/// - Overrides previously used `bin`.
	pub fn window(mut self, window: &Arc<dyn BasaltWindow>) -> Self {
		self.target = InputHookTarget::Window(window.clone());
		self
	}

	/// Attach input hook to a `Bin`
	///
	/// # Notes
	/// - Overrides previously used `window`.
	pub fn bin(mut self, bin: &Arc<Bin>) -> Self {
		self.target = InputHookTarget::Bin(bin.clone());
		self
	}

	// TODO: Doc example
	/// Trigger on a press
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_press(self) -> InputPressBuilder<'a> {
		InputPressBuilder::start(self, PressOrRelease::Press)
	}

	// TODO: Doc example
	/// Trigger on a release
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_release(self) -> InputPressBuilder<'a> {
		InputPressBuilder::start(self, PressOrRelease::Release)
	}

	// TODO: Doc example
	/// Trigger on a cursor enter
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_enter(self) -> InputEnterBuilder<'a> {
		InputEnterBuilder::start(self, EnterOrLeave::Enter)
	}

	// TODO: Doc example
	/// Trigger on a cursor leave
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_leave(self) -> InputEnterBuilder<'a> {
		InputEnterBuilder::start(self, EnterOrLeave::Leave)
	}

	// TODO: Doc example
	/// Trigger on a focus
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_focus(self) -> InputFocusBuilder<'a> {
		InputFocusBuilder::start(self, FocusOrFocusLost::Focus)
	}

	// TODO: Doc example
	/// Trigger on a focus lost
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_focus_lost(self) -> InputFocusBuilder<'a> {
		InputFocusBuilder::start(self, FocusOrFocusLost::FocusLost)
	}

	/// Submit the created hook to `Input`.
	///
	/// # Possible Errors
	/// `NoTrigger`: No `on_` method was called.
	/// `NoTarget`: The trigger requires either `bin()` or `window()` to be called.
	pub fn submit(self) -> Result<InputHookID, InputError> {
		let state = self.hook.ok_or(InputError::NoTrigger)?;

		if state.requires_target() && self.target == InputHookTarget::None {
			return Err(InputError::NoTarget);
		}

		Ok(self.input.add_hook(Hook {
			target_id: self.target.id(),
			target_wk: self.target.weak(),
			state,
		}))
	}
}

enum PressOrRelease {
	Press,
	Release,
}

pub struct InputPressBuilder<'a> {
	parent: InputHookBuilder<'a>,
	ty: PressOrRelease,
	keys: Vec<Key>,
	weight: i16,
	method: Option<
		Box<
			dyn FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	>,
}

impl<'a> InputPressBuilder<'a> {
	fn start(parent: InputHookBuilder<'a>, ty: PressOrRelease) -> Self {
		Self {
			parent,
			ty,
			keys: Vec::new(),
			weight: NO_HOOK_WEIGHT,
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
		F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl + Send + 'static,
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
			dyn Fn(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl
				+ Send
				+ Sync,
		>,
	) -> Self {
		self.method =
			Some(Box::new(move |target, window, local| method(target, window, local)));
		self
	}

	/// Finish building a press returning the `InputHookBuilder`.
	///
	/// # Possible Errors
	/// - `NoKeys`: No call to `key` or `keys` was made.
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	pub fn finish(mut self) -> Result<InputHookBuilder<'a>, InputError> {
		if self.keys.is_empty() {
			Err(InputError::NoKeys)
		} else if self.method.is_none() {
			Err(InputError::NoMethod)
		} else {
			// NOTE: HashMap guarentees deduplication

			self.parent.hook = match self.ty {
				PressOrRelease::Press =>
					Some(HookState::Press {
						state: LocalKeyState::from_keys(self.keys),
						weight: self.weight,
						method: self.method.unwrap(),
					}),
				PressOrRelease::Release =>
					Some(HookState::Release {
						state: LocalKeyState::from_keys(self.keys),
						pressed: false,
						weight: self.weight,
						method: self.method.unwrap(),
					}),
			};

			Ok(self.parent)
		}
	}
}

enum EnterOrLeave {
	Enter,
	Leave,
}

pub struct InputEnterBuilder<'a> {
	parent: InputHookBuilder<'a>,
	ty: EnterOrLeave,
	weight: i16,
	top: bool,
	method:
		Option<Box<dyn FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>>,
}

impl<'a> InputEnterBuilder<'a> {
	fn start(parent: InputHookBuilder<'a>, ty: EnterOrLeave) -> Self {
		Self {
			parent,
			ty,
			top: false,
			weight: NO_HOOK_WEIGHT,
			method: None,
		}
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

	/// Only call if the target is the top-most.
	///
	/// # Notes
	/// - Has no effect on window targets.
	pub fn on_top_only(mut self) -> Self {
		self.top = true;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - This overrides any previous `call` or `call_arcd`.
	pub fn call<F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>(
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
		method: Arc<dyn Fn(InputHookTarget, &WindowState) -> InputHookCtrl + Send + Sync>,
	) -> Self {
		self.method = Some(Box::new(move |target, window| method(target, window)));
		self
	}

	/// Finish building a press returning the `InputHookBuilder`.
	///
	/// # Possible Errors
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	pub fn finish(mut self) -> Result<InputHookBuilder<'a>, InputError> {
		if self.method.is_none() {
			Err(InputError::NoMethod)
		} else {
			self.parent.hook = match self.ty {
				EnterOrLeave::Enter =>
					Some(HookState::Enter {
						weight: self.weight,
						top: self.top,
						inside: false,
						pass: true,
						method: self.method.unwrap(),
					}),
				EnterOrLeave::Leave =>
					Some(HookState::Leave {
						weight: self.weight,
						top: self.top,
						inside: false,
						method: self.method.unwrap(),
					}),
			};

			Ok(self.parent)
		}
	}
}

enum FocusOrFocusLost {
	Focus,
	FocusLost,
}

pub struct InputFocusBuilder<'a> {
	parent: InputHookBuilder<'a>,
	ty: FocusOrFocusLost,
	weight: i16,
	method:
		Option<Box<dyn FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>>,
}

impl<'a> InputFocusBuilder<'a> {
	fn start(parent: InputHookBuilder<'a>, ty: FocusOrFocusLost) -> Self {
		Self {
			parent,
			ty,
			weight: NO_HOOK_WEIGHT,
			method: None,
		}
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
	pub fn call<F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>(
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
		method: Arc<dyn Fn(InputHookTarget, &WindowState) -> InputHookCtrl + Send + Sync>,
	) -> Self {
		self.method = Some(Box::new(move |target, window| method(target, window)));
		self
	}

	/// Finish building a press returning the `InputHookBuilder`.
	///
	/// # Possible Errors
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	pub fn finish(mut self) -> Result<InputHookBuilder<'a>, InputError> {
		if self.method.is_none() {
			Err(InputError::NoMethod)
		} else {
			self.parent.hook = match self.ty {
				FocusOrFocusLost::Focus =>
					Some(HookState::Focus {
						weight: self.weight,
						method: self.method.unwrap(),
					}),
				FocusOrFocusLost::FocusLost =>
					Some(HookState::FocusLost {
						weight: self.weight,
						method: self.method.unwrap(),
					}),
			};

			Ok(self.parent)
		}
	}
}
