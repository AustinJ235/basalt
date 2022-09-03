//! Collection of builders used for `Input`.

use std::sync::Arc;
use std::time::Duration;

use crate::input::inner::LoopEvent;
use crate::input::key::KeyCombo;
use crate::input::state::{HookState, LocalCursorState, LocalKeyState, WindowState};
use crate::input::{
    Char, Hook, Input, InputError, InputHookCtrl, InputHookID, InputHookTarget, Key, NO_HOOK_WEIGHT,
};
use crate::interface::bin::Bin;
use crate::interval::IntvlHookCtrl;
use crate::window::BasaltWindow;

/// The main builder for `Input`.
pub struct InputHookBuilder<'a> {
    input: &'a Input,
    target: InputHookTarget,
    hook: Option<HookState>,
}

impl<'a> InputHookBuilder<'a> {
    pub(in crate::input) fn start(input: &'a Input) -> Self {
        Self {
            input,
            target: InputHookTarget::None,
            hook: None,
        }
    }

    /// Attach hook to a `Bin`
    pub fn window(mut self, window: &Arc<dyn BasaltWindow>) -> Self {
        self.target = InputHookTarget::Window(window.clone());
        self
    }

    /// Attach hook to a `Bin`
    pub fn bin(mut self, bin: &Arc<Bin>) -> Self {
        self.target = InputHookTarget::Bin(bin.clone());
        self
    }

    /// Attach hook to a press event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_press(self) -> InputPressBuilder<'a> {
        InputPressBuilder::start(self, PressOrRelease::Press)
    }

    /// Attach hook to a hold event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_hold(self) -> InputHoldBuilder<'a> {
        InputHoldBuilder::start(self)
    }

    /// Attach hook to a release event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_release(self) -> InputPressBuilder<'a> {
        InputPressBuilder::start(self, PressOrRelease::Release)
    }

    /// Attach hook to a character event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_character(self) -> InputCharacterBuilder<'a> {
        InputCharacterBuilder::start(self)
    }

    /// Attach hook to a cursor enter event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_enter(self) -> InputEnterBuilder<'a> {
        InputEnterBuilder::start(self, EnterOrLeave::Enter)
    }

    /// Attach hook to a cursor leave event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_leave(self) -> InputEnterBuilder<'a> {
        InputEnterBuilder::start(self, EnterOrLeave::Leave)
    }

    /// Attach hook to a focus event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_focus(self) -> InputFocusBuilder<'a> {
        InputFocusBuilder::start(self, FocusOrFocusLost::Focus)
    }

    /// Attach hook to a focus lost event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_focus_lost(self) -> InputFocusBuilder<'a> {
        InputFocusBuilder::start(self, FocusOrFocusLost::FocusLost)
    }

    /// Attach hook to a scroll event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_scroll(self) -> InputScrollBuilder<'a> {
        InputScrollBuilder::start(self)
    }

    /// Attach hook to a cursor move event.
    ///
    /// Requires a proceeding call to either `window` or `bin`.
    pub fn on_cursor(self) -> InputCursorBuilder<'a> {
        InputCursorBuilder::start(self)
    }

    /// Attach hook to a mouse motion event.
    pub fn on_motion(self) -> InputMotionBuilder<'a> {
        InputMotionBuilder::start(self)
    }

    fn submit(self) -> Result<InputHookID, InputError> {
        let state = self.hook.ok_or(InputError::NoTrigger)?;

        if state.requires_target() && self.target == InputHookTarget::None {
            return Err(InputError::NoTarget);
        }

        let id = self.input.add_hook(Hook {
            target_id: self.target.id(),
            target_wk: self.target.weak(),
            state,
        });

        match &self.target {
            InputHookTarget::Window(_) => {
                // TODO:
            },
            InputHookTarget::Bin(bin) => {
                bin.attach_input_hook(id);
            },
            _ => (),
        }

        Ok(id)
    }
}

enum PressOrRelease {
    Press,
    Release,
}

/// Builder returned by `on_press` or `on_release`.
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

    /// Add a list of `Key`'s to the combination used.
    ///
    /// # Notes
    /// - This adds to any previous `combo` call.
    ///
    /// ```no_run
    /// // Example Inputs
    /// .keys(Qwerty::Q)
    /// .keys((Qwerty::LCtrl, MouseButton::Left))
    /// .keys(vec![Qwerty::LCtrl, Qwerty::A])
    /// .keys([Qwerty::A, Qwerty::D])
    pub fn keys<C: KeyCombo>(mut self, combo: C) -> Self {
        let mut combo = combo.into_vec();
        self.keys.append(&mut combo);
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
    /// - Calling this multiple times will not add additional methods.
    pub fn call<
        F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl + Send + 'static,
    >(
        mut self,
        method: F,
    ) -> Self {
        self.method = Some(Box::new(method));
        self
    }

    /// Finish building, validate, and submit it to `Input`.
    ///
    /// # Possible Errors
    /// - `NoKeys`: No call to `key` or `keys` was made.
    /// - `NoMethod`: No method was added. See `call`.
    /// - `NoTarget`: No call to `bin()` or `window()` was made.
    pub fn finish(mut self) -> Result<InputHookID, InputError> {
        if self.keys.is_empty() {
            Err(InputError::NoKeys)
        } else if self.method.is_none() {
            Err(InputError::NoMethod)
        } else {
            // NOTE: HashMap guarentees deduplication

            self.parent.hook = match self.ty {
                PressOrRelease::Press => {
                    Some(HookState::Press {
                        state: LocalKeyState::from_keys(self.keys),
                        weight: self.weight,
                        method: self.method.unwrap(),
                    })
                },
                PressOrRelease::Release => {
                    Some(HookState::Release {
                        state: LocalKeyState::from_keys(self.keys),
                        pressed: false,
                        weight: self.weight,
                        method: self.method.unwrap(),
                    })
                },
            };

            self.parent.submit()
        }
    }
}

/// Builder returned by `on_hold`.
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

    /// Add a list of `Key`'s to the combination used.
    ///
    /// # Notes
    /// - This adds to any previous `combo` call.
    ///
    /// ```no_run
    /// // Example Inputs
    /// .keys(Qwerty::Q)
    /// .keys((Qwerty::LCtrl, MouseButton::Left))
    /// .keys(vec![Qwerty::LCtrl, Qwerty::A])
    /// .keys([Qwerty::A, Qwerty::D])
    pub fn keys<C: KeyCombo>(mut self, combo: C) -> Self {
        let mut combo = combo.into_vec();
        self.keys.append(&mut combo);
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
    pub fn delay(mut self, delay: Option<Duration>) -> Self {
        self.delay = delay;
        self
    }

    /// Set the interval.
    ///
    /// **Default**: `Duration::from_millis(15)`
    pub fn interval(mut self, intvl: Duration) -> Self {
        self.intvl = intvl;
        self
    }

    /// Assign a function to call.
    ///
    /// # Notes
    /// - Calling this multiple times will not add additional methods.
    /// - `NoPass` varients of `InputHookCtrl` will be treated like their normal varients.
    pub fn call<
        F: FnMut(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl + Send + 'static,
    >(
        mut self,
        method: F,
    ) -> Self {
        self.method = Some(Box::new(method));
        self
    }

    /// Finish building, validate, and submit it to `Input`.
    ///
    /// # Possible Errors
    /// - `NoKeys`: No call to `key` or `keys` was made.
    /// - `NoMethod`: No method was added. See `call`.
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
                    Some(target) => {
                        match method(target, &local, last_call) {
                            InputHookCtrl::Retain | InputHookCtrl::RetainNoPass => {
                                IntvlHookCtrl::Continue
                            },
                            InputHookCtrl::Remove | InputHookCtrl::RemoveNoPass => {
                                event_send.send(LoopEvent::Remove(input_hook_id)).unwrap();
                                IntvlHookCtrl::Remove
                            },
                        }
                    },
                    None => {
                        event_send.send(LoopEvent::Remove(input_hook_id)).unwrap();
                        IntvlHookCtrl::Remove
                    },
                }
            });

            self.parent.input.add_hook_with_id(
                input_hook_id,
                Hook {
                    target_id: self.parent.target.id(),
                    target_wk: self.parent.target.weak(),
                    state: HookState::Hold {
                        state,
                        pressed: false,
                        weight: self.weight,
                        intvl_id,
                    },
                },
            );

            Ok(input_hook_id)
        }
    }
}

enum EnterOrLeave {
    Enter,
    Leave,
}

/// Builder returned by `on_enter` or `on_leave`.
pub struct InputEnterBuilder<'a> {
    parent: InputHookBuilder<'a>,
    ty: EnterOrLeave,
    weight: i16,
    top: bool,
    method: Option<Box<dyn FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>>,
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
    /// **Default**: `false`
    ///
    /// # Notes
    /// - Has no effect on window targets.
    pub fn require_on_top(mut self, top: bool) -> Self {
        self.top = top;
        self
    }

    /// Assign a function to call.
    ///
    /// # Notes
    /// - Calling this multiple times will not add additional methods.
    pub fn call<F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>(
        mut self,
        method: F,
    ) -> Self {
        self.method = Some(Box::new(method));
        self
    }

    /// Finish building, validate, and submit it to `Input`.
    ///
    /// # Possible Errors
    /// - `NoMethod`: No method was added. See `call`.
    /// - `NoTarget`: No call to `bin()` or `window()` was made.
    pub fn finish(mut self) -> Result<InputHookID, InputError> {
        if self.method.is_none() {
            Err(InputError::NoMethod)
        } else {
            self.parent.hook = match self.ty {
                EnterOrLeave::Enter => {
                    Some(HookState::Enter {
                        weight: self.weight,
                        top: self.top,
                        inside: false,
                        pass: true,
                        method: self.method.unwrap(),
                    })
                },
                EnterOrLeave::Leave => {
                    Some(HookState::Leave {
                        weight: self.weight,
                        top: self.top,
                        inside: false,
                        method: self.method.unwrap(),
                    })
                },
            };

            self.parent.submit()
        }
    }
}

enum FocusOrFocusLost {
    Focus,
    FocusLost,
}

/// Builder returned by `on_focus` or `on_focus_lost`.
pub struct InputFocusBuilder<'a> {
    parent: InputHookBuilder<'a>,
    ty: FocusOrFocusLost,
    weight: i16,
    method: Option<Box<dyn FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>>,
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
    /// - Calling this multiple times will not add additional methods.
    pub fn call<F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static>(
        mut self,
        method: F,
    ) -> Self {
        self.method = Some(Box::new(method));
        self
    }

    /// Finish building, validate, and submit it to `Input`.
    ///
    /// # Possible Errors
    /// - `NoMethod`: No method was added. See `call`.
    /// - `NoTarget`: No call to `bin()` or `window()` was made.
    pub fn finish(mut self) -> Result<InputHookID, InputError> {
        if self.method.is_none() {
            Err(InputError::NoMethod)
        } else {
            self.parent.hook = match self.ty {
                FocusOrFocusLost::Focus => {
                    Some(HookState::Focus {
                        weight: self.weight,
                        method: self.method.unwrap(),
                    })
                },
                FocusOrFocusLost::FocusLost => {
                    Some(HookState::FocusLost {
                        weight: self.weight,
                        method: self.method.unwrap(),
                    })
                },
            };

            self.parent.submit()
        }
    }
}

/// Builder returned by `on_cursor`.
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
    /// **Default**: `false`
    ///
    /// # Notes
    /// - This has no effect on Window targets.
    pub fn require_on_top(mut self, top: bool) -> Self {
        self.top = top;
        self
    }

    /// Require the target to be focused.
    ///
    /// **Default**: `false`
    pub fn require_focused(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }

    /// Assign a function to call.
    ///
    /// # Notes
    /// - Calling this multiple times will not add additional methods.
    pub fn call<
        F: FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl + Send + 'static,
    >(
        mut self,
        method: F,
    ) -> Self {
        self.method = Some(Box::new(method));
        self
    }

    /// Finish building, validate, and submit it to `Input`.
    ///
    /// # Possible Errors
    /// - `NoMethod`: No method was added. See `call`.
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

/// Builder returned by `on_character`.
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
    /// - Calling this multiple times will not add additional methods.
    pub fn call<F: FnMut(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + 'static>(
        mut self,
        method: F,
    ) -> Self {
        self.method = Some(Box::new(method));
        self
    }

    /// Finish building, validate, and submit it to `Input`.
    ///
    /// # Possible Errors
    /// - `NoMethod`: No method was added. See `call`.
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

/// Builder returned by `on_scroll`.
pub struct InputScrollBuilder<'a> {
    parent: InputHookBuilder<'a>,
    weight: i16,
    top: bool,
    focus: bool,
    smooth: bool,
    upper_blocks: bool,
    method: Option<
        Box<dyn FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + 'static>,
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
            upper_blocks: false,
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
    /// **Default**: `false`
    ///
    /// # Notes
    /// - This has no effect on Window targets.
    pub fn require_on_top(mut self, top: bool) -> Self {
        self.top = top;
        self
    }

    /// Require the target to be focused.
    ///
    /// **Default**: `false`
    pub fn require_focused(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }

    /// Enable smoothing.
    ///
    /// Convert steps into pixels and provide a smoothed output.
    ///
    /// **Default**: `false`
    pub fn enable_smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    /// Don't call this hook if a higher z-index is processed.
    ///
    /// Useful for when scroll areas contain other scroll areas.
    ///
    /// **Default**: `false`
    ///
    /// # Notes
    /// - Weight is still respected, so this is only effective with hooks of the same weight.
    /// - This has no effect on Window targets.
    pub fn upper_blocks(mut self, upper_blocks: bool) -> Self {
        self.upper_blocks = upper_blocks;
        self
    }

    /// Assign a function to call.
    ///
    /// # Notes
    /// - Calling this multiple times will not add additional methods.
    pub fn call<
        F: FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + 'static,
    >(
        mut self,
        method: F,
    ) -> Self {
        self.method = Some(Box::new(method));
        self
    }

    /// Finish building, validate, and submit it to `Input`.
    ///
    /// # Possible Errors
    /// - `NoMethod`: No method was added. See `call`.
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
                upper_blocks: self.upper_blocks,
                method: self.method.unwrap(),
            });

            self.parent.submit()
        }
    }
}

/// Builder returned by `on_motion`.
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
    /// - Calling this multiple times will not add additional methods.
    pub fn call<F: FnMut(f32, f32) -> InputHookCtrl + Send + 'static>(mut self, method: F) -> Self {
        self.method = Some(Box::new(method));
        self
    }

    /// Finish building, validate, and submit it to `Input`.
    ///
    /// # Possible Errors
    /// - `NoMethod`: No method was added. See `call`.
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
