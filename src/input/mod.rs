//! System for handling input related events.
//!
//! ### Weights
//! A weight can be assigned to each hook via the respective builder. Weight defines the order
//! in which hooks of the same class are called. A higher weight will be called first. Hooks
//! that have a weight specified can also block the execution of hooks in the same class by
//! a `NoPass` varient of `InputHookCtrl`.
//!
//! ##### Press/Hold/Release Weight Class
//! These hook types all share the same weighing. An important note with this class is that
//! window hooks will get called before bin hooks. A press hook with a higher weight than
//! a release hook that returns a `NoPass` varient of `InputHookCtrl` will prevent the release
//! from being called if the either share a key. Likewise with a hold, a press of a higher
//! weight can prevent it getting it called, but unlike release it will not prevent it from
//! getting released.
//!
//! ##### Enter/Leave
//! Window and Bins are seperate in the class of weights. Only hooks targeted for bins can
//! prevent hooks towards bins. Likewise with windows. A hook can effect multiple bins
//! depending of if `require_on_top` has been set to `false`. In this case hooks on different
//! bins can block the execution of one another.
//!
//! ##### Character
//! Window and Bins are treated the same. They are called in order of their weight. Calling
//! a `NoPass` varient of `InputHookCtrl` prevents the execution of all lesser weighed hooks.
//!
//! ##### Focus/FocusLost
//! Similar to Enter/Leave, but a hook can not effect multiple bins.
//!
//! ##### Scroll
//! Similar to Enter/Leave, but windows and bins are in the same class of weights.
//!
//! ##### Cursor
//! Same behavior as Scroll.
//!
//! ##### Motion
//! Similar to Character, but there are no targets.

pub mod builder;
mod inner;
pub mod key;
mod proc;
pub mod state;

use std::sync::atomic::{self, AtomicU64};
use std::sync::{Arc, Weak};

use flume::Sender;

use self::inner::LoopEvent;
pub use self::key::{Char, Key, MouseButton, Qwerty};
use self::state::HookState;
use crate::input::builder::InputHookBuilder;
use crate::interface::bin::{Bin, BinID};
use crate::interface::Interface;
use crate::interval::Interval;
use crate::window::{Window, WindowID};

const NO_HOOK_WEIGHT: i16 = i16::min_value();
const BIN_FOCUS_KEY: Key = Key::Mouse(MouseButton::Left);

/// An ID of a `Input` hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputHookID(u64);

/// The target of a hook.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum InputHookTarget {
    None,
    Window(Arc<Window>),
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

    /// Try to convert target into a `Bin`.
    pub fn into_bin(self) -> Option<Arc<Bin>> {
        match self {
            InputHookTarget::Bin(bin) => Some(bin),
            _ => None,
        }
    }

    /// Try to convert target into a `Arc<Window>`.
    pub fn into_window(self) -> Option<Arc<Window>> {
        match self {
            InputHookTarget::Window(win) => Some(win),
            _ => None,
        }
    }
}

impl PartialEq for InputHookTarget {
    fn eq(&self, other: &Self) -> bool {
        match self {
            Self::None => matches!(other, Self::None),
            Self::Window(window) => {
                match other {
                    Self::Window(other_window) => window.id() == other_window.id(),
                    _ => false,
                }
            },
            Self::Bin(bin) => {
                match other {
                    Self::Bin(other_bin) => bin == other_bin,
                    _ => false,
                }
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
    Press { win: WindowID, key: Key },
    Release { win: WindowID, key: Key },
    Character { win: WindowID, c: char },
    Cursor { win: WindowID, x: f32, y: f32 },
    Scroll { win: WindowID, v: f32, h: f32 },
    Enter { win: WindowID },
    Leave { win: WindowID },
    Focus { win: WindowID },
    FocusLost { win: WindowID },
    Motion { x: f32, y: f32 },
    CursorCapture { win: WindowID, captured: bool },
}

/// An error that is returned by various `Input` related methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    NoKeys,
    NoMethod,
    NoTarget,
    NoTrigger,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum InputHookTargetID {
    #[default]
    None,
    Window(WindowID),
    Bin(BinID),
}

enum InputHookTargetWeak {
    None,
    Window(Weak<Window>),
    Bin(Weak<Bin>),
}

impl InputHookTargetWeak {
    fn upgrade(&self) -> Option<InputHookTarget> {
        match self {
            Self::None => Some(InputHookTarget::None),
            Self::Window(wk) => wk.upgrade().map(InputHookTarget::Window),
            Self::Bin(wk) => wk.upgrade().map(InputHookTarget::Bin),
        }
    }
}

struct Hook {
    target_id: InputHookTargetID,
    target_wk: InputHookTargetWeak,
    state: HookState,
}

impl Hook {
    fn is_for_window_id(&self, win_id: WindowID) -> bool {
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

    fn bin_id(&self) -> Option<BinID> {
        match &self.target_id {
            InputHookTargetID::Bin(bin_id) => Some(*bin_id),
            _ => None,
        }
    }
}

/// The main struct for the input system.
///
/// Accessed via `basalt.input_ref()`.
pub struct Input {
    event_send: Sender<LoopEvent>,
    current_id: AtomicU64,
    interval: Arc<Interval>,
}

impl Input {
    pub(crate) fn new(interface: Arc<Interface>, interval: Arc<Interval>) -> Self {
        let (event_send, event_recv) = flume::unbounded();
        inner::begin_loop(interface, interval.clone(), event_send.clone(), event_recv);

        Self {
            event_send,
            interval,
            current_id: AtomicU64::new(0),
        }
    }

    pub(in crate::input) fn event_send(&self) -> Sender<LoopEvent> {
        self.event_send.clone()
    }

    pub(in crate::input) fn interval(&self) -> Arc<Interval> {
        self.interval.clone()
    }

    /// Returns a builder to add a hook.
    ///
    /// ```no_run
    /// let hook_id = basalt
    ///     .input_ref()
    ///     .hook()
    ///     .bin(&bin)
    ///     .on_press()
    ///     .keys(Qwerty::W)
    ///     .call(move |_target, _global, local| {
    ///         assert!(local.is_pressed(Qwerty::W));
    ///         println!("Pressed W on Bin");
    ///         Default::default()
    ///     })
    ///     .finish()
    ///     .unwrap();
    /// ```
    pub fn hook(&self) -> InputHookBuilder {
        InputHookBuilder::start(self)
    }

    /// Remove a hook from `Input`.
    ///
    /// # Notes
    /// - Hooks on a `Bin` or `Window` are automatically removed when they are dropped.
    pub fn remove_hook(&self, id: InputHookID) {
        self.event_send.send(LoopEvent::Remove(id)).unwrap();
    }

    /// Manually set the `Bin` that is focused.
    ///
    /// Useful for dialogs/forms that require text input.
    pub fn set_bin_focused(&self, bin: &Arc<Bin>) {
        // TODO: get window from Bin
        let win = WindowID::invalid();

        self.event_send
            .send(LoopEvent::FocusBin {
                win,
                bin: Some(bin.id()),
            })
            .unwrap();
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

    pub(in crate::input) fn add_hook_with_id(&self, id: InputHookID, hook: Hook) {
        self.event_send
            .send(LoopEvent::Add {
                id,
                hook,
            })
            .unwrap();
    }

    pub(in crate::input) fn next_id(&self) -> InputHookID {
        InputHookID(self.current_id.fetch_add(1, atomic::Ordering::SeqCst))
    }
}
