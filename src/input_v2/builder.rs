use crate::input_v2::inner::LoopEvent;
use crate::input_v2::state::{HookState, LocalCursorState, LocalKeyState, WindowState};
use crate::input_v2::{
	Char, Hook, InputError, InputHookCtrl, InputHookID, InputHookTarget, InputV2, Key,
	NO_HOOK_WEIGHT,
};
use crate::interface::bin::Bin;
use crate::interval::IntvlHookCtrl;
use crate::window::BasaltWindow;
use std::sync::Arc;
use std::time::Duration;

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
	/// Trigger on a hold
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_hold(self) -> InputHoldBuilder<'a> {
		InputHoldBuilder::start(self)
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
	/// Trigger on a character press
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_character(self) -> InputCharacterBuilder<'a> {
		InputCharacterBuilder::start(self)
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
	/// Trigger on a scroll
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_scroll(self) -> InputScrollBuilder<'a> {
		InputScrollBuilder::start(self)
	}

	// TODO: Doc example
	/// Trigger on a focus lost
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_focus_lost(self) -> InputFocusBuilder<'a> {
		InputFocusBuilder::start(self, FocusOrFocusLost::FocusLost)
	}

	/// Trigger on cursor movement.
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_cursor_move(self) -> InputCursorBuilder<'a> {
		InputCursorBuilder::start(self)
	}

	/// Trigger on mouse movement.
	///
	/// # Notes
	/// - Overrides any previously called `on_` method.
	pub fn on_motion(self) -> InputMotionBuilder<'a> {
		InputMotionBuilder::start(self)
	}

	fn submit(self) -> Result<InputHookID, InputError> {
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
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
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
	/// - Calling this multiple times will not add additional methods. The last call will be used.
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

	/// Finish building, validate, and submit it to `Input`.
	///
	/// # Possible Errors
	/// - `NoKeys`: No call to `key` or `keys` was made.
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	/// - `NoTarget`: No call to `bin()` or `window()` was made.
	pub fn finish(mut self) -> Result<InputHookID, InputError> {
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

			self.parent.submit()
		}
	}
}

pub struct InputHoldBuilder<'a> {
	parent: InputHookBuilder<'a>,
	delay: Option<Duration>,
	intvl: Duration,
	keys: Vec<Key>,
	weight: i16,
	method: Option<
		Box<
			dyn FnMut(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	>,
}

impl<'a> InputHoldBuilder<'a> {
	fn start(parent: InputHookBuilder<'a>) -> Self {
		Self {
			parent,
			delay: None,
			intvl: Duration::from_millis(15),
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
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Set the delay (How long to be held before activation).
	///
	/// **Default**: None
	pub fn delay(mut self, delay: Duration) -> Self {
		self.delay = Some(delay);
		self
	}

	/// Set the interval.
	///
	/// **Default**: 15 ms
	pub fn interval(mut self, intvl: Duration) -> Self {
		self.intvl = intvl;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	/// - `NoPass` varients of `InputHookCtrl` have no effect and will be treated like their normal varients.
	pub fn call<
		F: FnMut(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl
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
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	/// - `NoPass` varients of `InputHookCtrl` have no effect and will be treated like their normal varients.
	pub fn call_arcd(
		mut self,
		method: Arc<
			dyn Fn(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl
				+ Send
				+ Sync,
		>,
	) -> Self {
		self.method = Some(Box::new(move |target, local, last| method(target, local, last)));
		self
	}

	/// Finish building, validate, and submit it to `Input`.
	///
	/// # Possible Errors
	/// - `NoKeys`: No call to `key` or `keys` was made.
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	/// - `NoTarget`: No call to `bin()` or `window()` was made.
	pub fn finish(mut self) -> Result<InputHookID, InputError> {
		if self.keys.is_empty() {
			Err(InputError::NoKeys)
		} else if self.method.is_none() {
			Err(InputError::NoMethod)
		} else if self.parent.target == InputHookTarget::None {
			Err(InputError::NoTarget)
		} else {
			let state = LocalKeyState::from_keys(self.keys);
			let mut local = state.clone();
			local.press_all();
			let event_send = self.parent.input.event_send();
			let interval = self.parent.input.interval();
			let input_hook_id = self.parent.input.next_id();
			let mut method = self.method.take().unwrap();
			let target_wk = self.parent.target.weak();

			let intvl_id = interval.do_every(self.intvl, self.delay, move |last_call| {
				match target_wk.upgrade() {
					Some(target) =>
						match method(target, &local, last_call) {
							InputHookCtrl::Retain | InputHookCtrl::RetainNoPass =>
								IntvlHookCtrl::Continue,
							InputHookCtrl::Remove | InputHookCtrl::RemoveNoPass => {
								event_send.send(LoopEvent::Remove(input_hook_id)).unwrap();
								IntvlHookCtrl::Remove
							},
						},
					None => {
						event_send.send(LoopEvent::Remove(input_hook_id)).unwrap();
						IntvlHookCtrl::Remove
					},
				}
			});

			self.parent.input.add_hook_with_id(input_hook_id, Hook {
				target_id: self.parent.target.id(),
				target_wk: self.parent.target.weak(),
				state: HookState::Hold {
					state,
					pressed: false,
					weight: self.weight,
					intvl_id,
				},
			});

			Ok(input_hook_id)
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
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Require the target to be the top-most.
	///
	/// # Notes
	/// - Has no effect on window targets.
	pub fn require_on_top(mut self) -> Self {
		self.top = true;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
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
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call_arcd(
		mut self,
		method: Arc<dyn Fn(InputHookTarget, &WindowState) -> InputHookCtrl + Send + Sync>,
	) -> Self {
		self.method = Some(Box::new(move |target, window| method(target, window)));
		self
	}

	/// Finish building, validate, and submit it to `Input`.
	///
	/// # Possible Errors
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	/// - `NoTarget`: No call to `bin()` or `window()` was made.
	pub fn finish(mut self) -> Result<InputHookID, InputError> {
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

			self.parent.submit()
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
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
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
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call_arcd(
		mut self,
		method: Arc<dyn Fn(InputHookTarget, &WindowState) -> InputHookCtrl + Send + Sync>,
	) -> Self {
		self.method = Some(Box::new(move |target, window| method(target, window)));
		self
	}

	/// Finish building, validate, and submit it to `Input`.
	///
	/// # Possible Errors
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	/// - `NoTarget`: No call to `bin()` or `window()` was made.
	pub fn finish(mut self) -> Result<InputHookID, InputError> {
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

			self.parent.submit()
		}
	}
}

pub struct InputCursorBuilder<'a> {
	parent: InputHookBuilder<'a>,
	weight: i16,
	top: bool,
	focus: bool,
	method: Option<
		Box<
			dyn FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	>,
}

impl<'a> InputCursorBuilder<'a> {
	fn start(parent: InputHookBuilder<'a>) -> Self {
		Self {
			parent,
			weight: NO_HOOK_WEIGHT,
			method: None,
			top: false,
			focus: false,
		}
	}

	/// Assigns a weight.
	///
	/// # Notes
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Require the target to be the top-most.
	///
	/// # Notes
	/// - This has no effect on Window targets.
	pub fn require_on_top(mut self) -> Self {
		self.top = true;
		self
	}

	/// Require the target to be focused.
	pub fn require_focused(mut self) -> Self {
		self.focus = true;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call<
		F: FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
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
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call_arcd(
		mut self,
		method: Arc<
			dyn Fn(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
				+ Send
				+ Sync,
		>,
	) -> Self {
		self.method =
			Some(Box::new(move |target, window, local| method(target, window, local)));
		self
	}

	/// Finish building, validate, and submit it to `Input`.
	///
	/// # Possible Errors
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	/// - `NoTarget`: No call to `bin()` or `window()` was made.
	pub fn finish(mut self) -> Result<InputHookID, InputError> {
		if self.method.is_none() {
			Err(InputError::NoMethod)
		} else {
			self.parent.hook = Some(HookState::Cursor {
				state: LocalCursorState::new(),
				weight: self.weight,
				top: self.top,
				focus: self.focus,
				inside: false,
				method: self.method.unwrap(),
			});

			self.parent.submit()
		}
	}
}

pub struct InputCharacterBuilder<'a> {
	parent: InputHookBuilder<'a>,
	weight: i16,
	method: Option<
		Box<dyn FnMut(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + 'static>,
	>,
}

impl<'a> InputCharacterBuilder<'a> {
	fn start(parent: InputHookBuilder<'a>) -> Self {
		Self {
			parent,
			weight: NO_HOOK_WEIGHT,
			method: None,
		}
	}

	/// Assigns a weight.
	///
	/// # Notes
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call<
		F: FnMut(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + 'static,
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
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call_arcd(
		mut self,
		method: Arc<dyn Fn(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + Sync>,
	) -> Self {
		self.method = Some(Box::new(move |target, window, c| method(target, window, c)));
		self
	}

	/// Finish building, validate, and submit it to `Input`.
	///
	/// # Possible Errors
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	/// - `NoTarget`: No call to `bin()` or `window()` was made.
	pub fn finish(mut self) -> Result<InputHookID, InputError> {
		if self.method.is_none() {
			Err(InputError::NoMethod)
		} else {
			self.parent.hook = Some(HookState::Character {
				weight: self.weight,
				method: self.method.unwrap(),
			});

			self.parent.submit()
		}
	}
}

pub struct InputScrollBuilder<'a> {
	parent: InputHookBuilder<'a>,
	weight: i16,
	top: bool,
	focus: bool,
	smooth: bool,
	method: Option<
		Box<
			dyn FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl
				+ Send
				+ 'static,
		>,
	>,
}

impl<'a> InputScrollBuilder<'a> {
	fn start(parent: InputHookBuilder<'a>) -> Self {
		Self {
			parent,
			weight: NO_HOOK_WEIGHT,
			method: None,
			top: false,
			focus: false,
			smooth: false,
		}
	}

	/// Assigns a weight.
	///
	/// # Notes
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Require the target to be the top-most.
	///
	/// # Notes
	/// - This has no effect on Window targets.
	pub fn require_on_top(mut self) -> Self {
		self.top = true;
		self
	}

	/// Require the target to be focused.
	pub fn require_focused(mut self) -> Self {
		self.focus = true;
		self
	}

	/// Enable smoothing.
	///
	/// Convert steps into pixels and provide a smoothed output.
	pub fn smooth(mut self) -> Self {
		self.smooth = true;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call<
		F: FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + 'static,
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
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call_arcd(
		mut self,
		method: Arc<
			dyn Fn(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + Sync,
		>,
	) -> Self {
		self.method = Some(Box::new(move |target, window, v, h| method(target, window, v, h)));
		self
	}

	/// Finish building, validate, and submit it to `Input`.
	///
	/// # Possible Errors
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	/// - `NoTarget`: No call to `bin()` or `window()` was made.
	pub fn finish(mut self) -> Result<InputHookID, InputError> {
		if self.method.is_none() {
			Err(InputError::NoMethod)
		} else {
			self.parent.hook = Some(HookState::Scroll {
				weight: self.weight,
				top: self.top,
				focus: self.focus,
				smooth: self.smooth,
				method: self.method.unwrap(),
			});

			self.parent.submit()
		}
	}
}

pub struct InputMotionBuilder<'a> {
	parent: InputHookBuilder<'a>,
	weight: i16,
	method: Option<Box<dyn FnMut(f32, f32) -> InputHookCtrl + Send + 'static>>,
}

impl<'a> InputMotionBuilder<'a> {
	fn start(parent: InputHookBuilder<'a>) -> Self {
		Self {
			parent,
			weight: NO_HOOK_WEIGHT,
			method: None,
		}
	}

	/// Assigns a weight.
	///
	/// # Notes
	/// - Higher weights get called first and may not pass events.
	pub fn weight(mut self, weight: i16) -> Self {
		self.weight = weight;
		self
	}

	/// Assign a function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call<F: FnMut(f32, f32) -> InputHookCtrl + Send + 'static>(
		mut self,
		method: F,
	) -> Self {
		self.method = Some(Box::new(method));
		self
	}

	/// Assign a `Arc`'d function to call.
	///
	/// # Notes
	/// - Calling this multiple times will not add additional methods. The last call will be used.
	pub fn call_arcd(
		mut self,
		method: Arc<dyn Fn(f32, f32) -> InputHookCtrl + Send + Sync>,
	) -> Self {
		self.method = Some(Box::new(move |x, y| method(x, y)));
		self
	}

	/// Finish building, validate, and submit it to `Input`.
	///
	/// # Possible Errors
	/// - `NoMethod`: No call to `call` or `call_arcd` was made.
	pub fn finish(mut self) -> Result<InputHookID, InputError> {
		if self.method.is_none() {
			Err(InputError::NoMethod)
		} else {
			self.parent.hook = Some(HookState::Motion {
				weight: self.weight,
				method: self.method.unwrap(),
			});

			self.parent.submit()
		}
	}
}
