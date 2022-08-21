pub mod builder;
mod inner;
pub mod state;

use self::state::HookState;
use crate::interface::bin::{Bin, BinID};
use crate::interface::Interface;
use crate::window::{BasaltWindow, BstWindowID};
use crossbeam::channel::{self, Sender};
use std::sync::atomic::{self, AtomicU64};
use std::sync::{Arc, Weak};

pub use self::builder::{InputHookBuilder, InputPressBuilder};
use self::inner::LoopEvent;
pub use self::state::{LocalKeyState, WindowKeyState};
// TODO: Define in this module.
pub use crate::input::{MouseButton, Qwerty};

/// An ID of a `Input` hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputHookID(u64);

/// A keyboard/mouse agnostic type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
	Keyboard(Qwerty),
	Mouse(MouseButton),
}

impl From<Qwerty> for Key {
	fn from(key: Qwerty) -> Self {
		Key::Keyboard(key)
	}
}

impl From<MouseButton> for Key {
	fn from(key: MouseButton) -> Self {
		Key::Mouse(key)
	}
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum InputHookTarget {
	None,
	Window(Arc<dyn BasaltWindow>),
	Bin(Arc<Bin>),
}

impl InputHookTarget {
	fn id(&self) -> InputHookTargetID {
		match self {
			Self::None => InputHookTargetID::None,
			Self::Window(win) => InputHookTargetID::Window(win.id()),
			Self::Bin(bin) => InputHookTargetID::Bin(bin.id()),
		}
	}

	fn weak(&self) -> InputHookTargetWeak {
		match self {
			Self::None => InputHookTargetWeak::None,
			Self::Window(win) => InputHookTargetWeak::Window(Arc::downgrade(win)),
			Self::Bin(bin) => InputHookTargetWeak::Bin(Arc::downgrade(bin)),
		}
	}
}

impl PartialEq for InputHookTarget {
	fn eq(&self, other: &Self) -> bool {
		match self {
			Self::None =>
				match other {
					Self::None => true,
					_ => false,
				},
			Self::Window(window) =>
				match other {
					Self::Window(other_window) => Arc::ptr_eq(window, other_window),
					_ => false,
				},
			Self::Bin(bin) =>
				match other {
					Self::Bin(other_bin) => bin == other_bin,
					_ => false,
				},
		}
	}
}

impl Eq for InputHookTarget {}

/// Controls what happens after the hook method is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputHookCtrl {
	/// Retain the hook and pass then events.
	#[default]
	Retain,
	/// Same as `Retain`, but will not pass event onto next hook.
	///
	/// # Notes
	/// - If this hook doesn't have a weight this is the same as `Retain`.
	RetainNoPass,
	/// Remove the hook
	Remove,
	/// Remove the hook and pass the event onto the next hook.
	///
	/// # Notes
	/// - If this hook doesn't have a weight this is the same as `Remove`.
	RemoveNoPass,
}

/// An event that `Input` should process.
///
/// # Notes
/// - This type should only be used externally when using a custom window implementation.
#[derive(Debug, Clone)]
pub enum InputEvent {
	Press {
		win: BstWindowID,
		key: Key,
	},
	Release {
		win: BstWindowID,
		key: Key,
	},
	Character {
		win: BstWindowID,
		c: char,
	},
	Cursor {
		win: BstWindowID,
		x: f32,
		y: f32,
	},
	Scroll {
		win: BstWindowID,
		v: f32,
		h: f32,
	},
	Enter {
		win: BstWindowID,
	},
	Leave {
		win: BstWindowID,
	},
	Focus {
		win: BstWindowID,
	},
	FocusLost {
		win: BstWindowID,
	},
	Motion {
		x: f32,
		y: f32,
	},
}

/// An error that is returned by various `Input` related methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
	NoKeys,
	NoMethod,
	NoTarget,
	NoTrigger,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum InputHookTargetID {
	#[default]
	None,
	Window(BstWindowID),
	Bin(BinID),
}

enum InputHookTargetWeak {
	None,
	Window(Weak<dyn BasaltWindow>),
	Bin(Weak<Bin>),
}

impl InputHookTargetWeak {
	fn upgrade(&self) -> Option<InputHookTarget> {
		match self {
			Self::None => Some(InputHookTarget::None),
			Self::Window(wk) => wk.upgrade().map(|win| InputHookTarget::Window(win)),
			Self::Bin(wk) => wk.upgrade().map(|bin| InputHookTarget::Bin(bin)),
		}
	}
}

struct Hook {
	target_id: InputHookTargetID,
	target_wk: InputHookTargetWeak,
	data: HookState,
}

impl Hook {
	fn is_for_window_id(&self, win_id: BstWindowID) -> bool {
		match &self.target_id {
			InputHookTargetID::Window(self_win_id) => *self_win_id == win_id,
			_ => false,
		}
	}

	fn is_for_bin_id(&self, bin_id: BinID) -> bool {
		match &self.target_id {
			InputHookTargetID::Bin(self_bin_id) => *self_bin_id == bin_id,
			_ => false,
		}
	}
}

pub struct InputV2 {
	event_send: Sender<LoopEvent>,
	current_id: AtomicU64,
}

impl InputV2 {
	pub(crate) fn new(interface: Arc<Interface>) -> Arc<Self> {
		let (event_send, event_recv) = channel::unbounded();
		inner::begin_loop(interface, event_recv);

		Arc::new(Self {
			event_send,
			current_id: AtomicU64::new(0),
		})
	}

	/// Returns a builder to add a hook.
	///
	/// ```no_run
	/// let _hook_id = basalt
	/// 	.input()
	/// 	.hook()
	/// 	.bin()
	/// 	.on_press()
	/// 	.key(Qwerty::W)
	/// 	.call(move |_target, _global, local| {
	/// 		assert!(local.is_pressed(Qwerty::W));
	/// 		println!("Pressed W on Bin");
	/// 		InputHookCtrl::Retain
	/// 	})
	/// 	.finish()
	/// 	.unwrap()
	/// 	.submit()
	/// 	.unwrap();
	/// ```
	pub fn hook(self: &Arc<Self>) -> InputHookBuilder {
		InputHookBuilder::start(self.clone())
	}

	/// Remove a hook from `Input`.
	pub fn remove_hook(&self, id: InputHookID) {
		self.event_send.send(LoopEvent::Remove(id)).unwrap();
	}

	/// Send an `InputEvent` to `Input`.
	///
	/// # Notes
	/// - This method should only be used externally when using a custom window implementation.
	pub fn send_event(&self, event: InputEvent) {
		self.event_send.send(LoopEvent::Normal(event)).unwrap();
	}

	fn add_hook(&self, hook: Hook) -> InputHookID {
		let id = InputHookID(self.current_id.fetch_add(1, atomic::Ordering::SeqCst));
		self.event_send
			.send(LoopEvent::Add {
				id,
				hook,
			})
			.unwrap();
		id
	}
}
