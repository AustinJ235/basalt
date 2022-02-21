use crate::input::{MouseButton, Qwery as Qwerty};
use crate::interface::bin::Bin;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

pub struct Hooks {}

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
			Self::Bin(weak) =>
				match weak.upgrade() {
					Some(strong) => {
						1_u8.hash(state);
						strong.id().hash(state);
					},
					None => 2_u8.hash(state),
				},
		}
	}
}

impl PartialEq for HookTarget {
	fn eq(&self, other: &Self) -> bool {
		match self {
			Self::Window =>
				match other {
					Self::Window => true,
					_ => false,
				},
			Self::Bin(weak) =>
				match other {
					Self::Bin(other_weak) => weak.ptr_eq(other_weak),
					_ => false,
				},
		}
	}
}

impl Eq for HookTarget {}

impl std::fmt::Debug for HookTarget {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Window => f.write_str("HookTarget::Window"),
			Self::Bin(weak) =>
				match weak.upgrade() {
					Some(strong) =>
						f.write_fmt(format_args!("HookTarget::Bin({})", strong.id())),
					None => f.write_str("HookTarget::Bin(dropped)"),
				},
		}
	}
}

impl PartialEq<Arc<Bin>> for HookTarget {
	fn eq(&self, other: &Arc<Bin>) -> bool {
		match self {
			HookTarget::Bin(weak) =>
				match weak.upgrade() {
					Some(strong) => strong.id() == other.id(),
					None => false,
				},
			_ => false,
		}
	}
}

impl PartialEq<HookTarget> for Arc<Bin> {
	fn eq(&self, other: &HookTarget) -> bool {
		match other {
			HookTarget::Bin(weak) =>
				match weak.upgrade() {
					Some(strong) => strong.id() == self.id(),
					None => false,
				},
			_ => false,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyMouseButton {
	Key(Qwerty),
	MouseButton(MouseButton),
}

impl From<Qwerty> for KeyMouseButton {
	fn from(key: Qwerty) -> Self {
		Self::Key(key)
	}
}

impl From<MouseButton> for KeyMouseButton {
	fn from(button: MouseButton) -> Self {
		Self::MouseButton(button)
	}
}

impl PartialEq<Qwerty> for KeyMouseButton {
	fn eq(&self, other: &Qwerty) -> bool {
		match self {
			KeyMouseButton::Key(key) => *key == *other,
			_ => false,
		}
	}
}

impl PartialEq<KeyMouseButton> for Qwerty {
	fn eq(&self, other: &KeyMouseButton) -> bool {
		match other {
			KeyMouseButton::Key(key) => *key == *self,
			_ => false,
		}
	}
}

impl PartialEq<MouseButton> for KeyMouseButton {
	fn eq(&self, other: &MouseButton) -> bool {
		match self {
			KeyMouseButton::MouseButton(button) => *button == *other,
			_ => false,
		}
	}
}

impl PartialEq<KeyMouseButton> for MouseButton {
	fn eq(&self, other: &KeyMouseButton) -> bool {
		match other {
			KeyMouseButton::MouseButton(button) => *button == *self,
			_ => false,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HookRQMT {
	pub target: HookTarget,
	pub combos: Vec<Vec<KeyMouseButton>>,
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

impl HookState {
	pub fn ty(&self) -> HookType {
		self.local.ty
	}

	pub fn target(&self) -> HookTarget {
		self.local.rqmt.target.clone()
	}

	pub fn active_combo(&self) -> Option<Vec<KeyMouseButton>> {
		todo!()
	}

	pub fn mouse_position(&self) -> [f32; 2] {
		self.global.mouse_p
	}

	pub fn mouse_delta(&self) -> [f32; 2] {
		[
			self.local.last_mouse_p[0] - self.global.mouse_p[0],
			self.local.last_mouse_p[1] - self.global.mouse_p[1],
		]
	}

	pub fn motion_delta(&self) -> [f32; 2] {
		[
			self.local.last_mouse_m[0] - self.global.mouse_m[0],
			self.local.last_mouse_m[1] - self.global.mouse_m[1],
		]
	}

	pub fn scroll_delta(&self) -> f32 {
		todo!()
	}

	pub fn mouse_inside(&self) -> bool {
		todo!()
	}

	pub fn lat_call(&self) -> Option<Duration> {
		todo!()
	}

	pub fn last_call_since_met(&self) -> Option<Duration> {
		todo!()
	}

	pub fn first_call(&self) -> bool {
		todo!()
	}

	pub fn first_call_ever(&self) -> bool {
		todo!()
	}
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

	pub fn mutable<F: FnMut(HookState) -> HookResult + Send + Sync + 'static>(func: F) -> Self {
		Self {
			inner: Arc::new(HookInnerFn::Mutable(Mutex::new(Box::new(func)))),
		}
	}
}

impl Hooks {
	pub(crate) fn new() -> Arc<Self> {
		Arc::new(Self {})
	}
}

pub struct HookBuilder {
	ty: HookType,
	rqmt: HookRQMT,
}

impl HookBuilder {
	pub fn start(ty: HookType) -> Self {
		Self {
			ty,
			rqmt: HookRQMT {
				target: HookTarget::Window,
				combos: Vec::new(),
				delay: None,
				repeat: None,
			},
		}
	}

	pub fn target_bin(mut self, bin: &Arc<Bin>) -> Self {
		self.rqmt.target = HookTarget::Bin(Arc::downgrade(bin));
		self
	}

	pub fn target_window(mut self) -> Self {
		self.rqmt.target = HookTarget::Window;
		self
	}

	pub fn delay(mut self, duration: Duration) -> Self {
		self.rqmt.delay = Some(duration);
		self
	}

	pub fn repeat(mut self, interval: Duration) -> Self {
		self.rqmt.repeat = Some(interval);
		self
	}

	pub fn combo<C: KeyButtonCombo>(mut self, combo: C) -> Self {
		self.rqmt.combos.push(combo.into_combo());
		self
	}

	pub fn validate(&self) -> Result<(), String> {
		todo!()
	}
}

pub trait KeyButtonCombo {
	fn into_combo(self) -> Vec<KeyMouseButton>;
}

impl<T: Into<KeyMouseButton>> KeyButtonCombo for T {
	fn into_combo(self) -> Vec<KeyMouseButton> {
		vec![self.into()]
	}
}

impl<T: Into<KeyMouseButton>, const N: usize> KeyButtonCombo for [T; N] {
	fn into_combo(self) -> Vec<KeyMouseButton> {
		IntoIterator::into_iter(self).map(|k| k.into()).collect()
	}
}

macro_rules! impl_tuple_combo {
    ($first:ident $(, $others:ident)+) => (
        impl<$first$(, $others)+> KeyButtonCombo for ($first, $($others),+)
            where $first: Into<KeyMouseButton>
                  $(, $others: Into<KeyMouseButton>)*
        {
            #[inline]
            fn into_combo(self) -> Vec<KeyMouseButton> {
                #![allow(non_snake_case)]

                let ($first, $($others,)*) = self;
                let mut list = Vec::new();
                list.push($first.into());

                $(
                    list.push($others.into());
                )+

                list
            }
        }

        impl_tuple_combo!($($others),+);
    );

    ($i:ident) => ();
}

impl_tuple_combo!(Z, Y, X, W, V, U, T, S, R, Q, P, O, N, M, L, K, J, I, H, G, F, E, D, C, B, A);

#[test]
fn hook_creation() {
	let _hookfn = HookFn::new(|_| ().into());

	let mut outer_variable = 0_u8;

	let _hookfn = HookFn::mutable(move |_| {
		for _ in 0..10 {
			outer_variable += 1;
		}

		println!("{}", outer_variable);
		().into()
	});
}

#[test]
fn combo_creation() {
	fn takes_combo<C: KeyButtonCombo>(combo: C) {
		let _ = combo.into_combo();
	}

	takes_combo(Qwerty::W);
	takes_combo(MouseButton::Left);
	takes_combo([Qwerty::W]);
	takes_combo([Qwerty::W, Qwerty::Q]);
	takes_combo([MouseButton::Left, MouseButton::Right]);
	takes_combo((Qwerty::W, MouseButton::Left));
	takes_combo((Qwerty::W, Qwerty::Q));
}
