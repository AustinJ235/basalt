use std::sync::{Arc, Weak};
use crate::interface::bin::Bin;
use crate::input::{Qwery as Qwerty, MouseButton};
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub struct Hooks {

}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookType {
    Press,
    Release,
    Character,
    MouseEnter,
    MouseLeave,
    MouseMove,
    MouseMotion,
    MouseScroll,
    Focused,
    LostFocus,
}

#[derive(Clone)]
pub enum HookTarget {
    Window,
    Bin(Weak<Bin>),
}

impl Hash for HookTarget {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Window => 0_u8.hash(state),
            Self::Bin(weak) => match weak.upgrade() {
                Some(strong) => {
                    1_u8.hash(state);
                    strong.id().hash(state);
                },
                None => 2_u8.hash(state),
            }
        }
    }
}

impl PartialEq for HookTarget {
    fn eq(&self, other: &Self) -> bool {
        match self {
            Self::Window => match other {
                Self::Window => true,
                _ => false,
            },
            Self::Bin(weak) => match other {
                Self::Bin(other_weak) => weak.ptr_eq(other_weak),
                _ => false
            }
        }
    }
}

impl Eq for HookTarget { }

impl std::fmt::Debug for HookTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Window => f.write_str("HookTarget::Window"),
            Self::Bin(weak) => match weak.upgrade() {
                Some(strong) => f.write_fmt(format_args!("HookTarget::Bin({})", strong.id())),
                None => f.write_str("HookTarget::Bin(dropped)"),
            }
        }
    }
}

impl PartialEq<Arc<Bin>> for HookTarget {
    fn eq(&self, other: &Arc<Bin>) -> bool {
        match self {
            HookTarget::Bin(weak) => match weak.upgrade() {
                Some(strong) => strong.id() == other.id(),
                None => false,
            },
            _ => false
        }
    }
}

impl PartialEq<HookTarget> for Arc<Bin> {
    fn eq(&self, other: &HookTarget) -> bool {
        match other {
            HookTarget::Bin(weak) => match weak.upgrade() {
                Some(strong) => strong.id() == self.id(),
                None => false,
            },
            _ => false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyMouseButton {
    Key(Qwerty),
    MouseButton(MouseButton),
}

impl PartialEq<Qwerty> for KeyMouseButton {
    fn eq(&self, other: &Qwerty) -> bool {
        match self {
            KeyMouseButton::Key(key) => *key == *other,
            _ => false
        }
    }
}

impl PartialEq<KeyMouseButton> for Qwerty {
    fn eq(&self, other: &KeyMouseButton) -> bool {
        match other {
            KeyMouseButton::Key(key) => *key == *self,
            _ => false
        }
    }
}

impl PartialEq<MouseButton> for KeyMouseButton {
    fn eq(&self, other: &MouseButton) -> bool {
        match self {
            KeyMouseButton::MouseButton(button) => *button == *other,
            _ => false
        }
    }
}

impl PartialEq<KeyMouseButton> for MouseButton {
    fn eq(&self, other: &KeyMouseButton) -> bool {
        match other {
            KeyMouseButton::MouseButton(button) => *button == *self,
            _ => false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HookRQMT {
    pub target: HookTarget,
    pub kmb: Vec<KeyMouseButton>,
    pub delay: Option<Duration>,
    pub repeat: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct GlobalHookState {
    pub press: HashMap<KeyMouseButton, bool>,
    pub mouse_p: [f32; 2],
    pub mouse_m: [f32; 2],
    pub mouse_s: f32,
}

#[derive(Debug, Clone)]
pub struct LocalHookState {
    pub ty: HookType,
    pub rqmt: HookRQMT,
    pub rqmt_met: Option<Instant>,
    pub rqmt_repeat: bool,
    pub last_call: Option<Instant>,
    pub last_mouse_p: [f32; 2],
    pub last_mouse_m: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct HookState {
    pub global: GlobalHookState,
    pub local: LocalHookState,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HookResult {
    Ok,
    Remove,
}

impl From<()> for HookResult {
    fn from(_: ()) -> Self {
        Self::Ok
    }
}

#[derive(Clone)]
pub struct HookFn {
    inner: Arc<HookInnerFn>,
}

enum HookInnerFn {
    Immutable(Box<dyn Fn(HookState) -> HookResult + Send + Sync>),
    Mutable(Mutex<Box<dyn FnMut(HookState) -> HookResult + Send + Sync>>),
}

impl HookFn {
    pub fn call(&self, state: HookState) -> HookResult {
        match &*self.inner {
            HookInnerFn::Immutable(func) => func(state),
            HookInnerFn::Mutable(func_lk) => func_lk.lock()(state),
        }
    }
    
    pub fn new<F: Fn(HookState) -> HookResult + Send + Sync + 'static>(func: F) -> Self {
        Self {
            inner: Arc::new(HookInnerFn::Immutable(Box::new(func))),
        }
    }

    pub fn new_mutable<F: FnMut(HookState) -> HookResult + Send + Sync + 'static>(func: F) -> Self {
        Self {
            inner: Arc::new(HookInnerFn::Mutable(Mutex::new(Box::new(func)))),
        }
    }
}

impl Hooks {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {

        })
    }
}

