pub mod winit;

use crate::input::key::KeyCombo;
use crate::input::state::{LocalCursorState, LocalKeyState, WindowState};
use crate::input::{Char, InputHookCtrl, InputHookID, InputHookTarget};
use crate::{Basalt, BstOptions};
use ordered_float::OrderedFloat;
use std::cmp::Reverse;
use std::sync::Arc;
use std::time::Duration;
use vulkano::instance::Instance;
use vulkano::swapchain::{Surface, Win32Monitor};

mod winit_ty {
	pub use winit::monitor::{MonitorHandle, VideoMode};
}

/// A window id used to differentiate between windows within Basalt.
///
/// # Notes
/// - This type doesn't correspond to any implementation's id. It is a unique id for Basalt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BstWindowID(pub(crate) u64);

pub trait BasaltWindow: Send + Sync + std::fmt::Debug {
	/// The window id of this window.
	fn id(&self) -> BstWindowID;
	/// Get `Basalt` used by the window.
	fn basalt(&self) -> Arc<Basalt>;
	/// Hides and captures cursor.
	fn capture_cursor(&self);
	/// Shows and releases cursor.
	fn release_cursor(&self);
	/// Checks if cursor is currently captured.
	fn cursor_captured(&self) -> bool;
	/// Return a list of active monitors on the system.
	fn monitors(&self) -> Vec<Monitor>;
	/// Return the primary monitor if the implementation is able to determine it.
	fn primary_monitor(&self) -> Option<Monitor>;
	/// Return the current monitor if the implementation is able to determine it.
	fn current_monitor(&self) -> Option<Monitor>;
	/// Enable fullscreen with the provided behavior.
	fn enable_fullscreen(&self, behavior: FullScreenBehavior) -> Result<(), FullScreenError>;
	/// Disable fullscreen.
	///
	/// # Notes
	/// - Does nothing if the window is not fullscreen.
	fn disable_fullscreen(&self);
	/// Toggle fullscreen mode. Uses `FullScreenBehavior::Auto`.
	///
	/// # Notes
	/// - Does nothing if there are no monitors available.
	fn toggle_fullscreen(&self);
	/// Check if the window is fullscreen.
	fn is_fullscreen(&self) -> bool;
	/// Request the monitor to resize to the given dimensions.
	fn request_resize(&self, width: u32, height: u32);
	/// Return the dimensions of the client area of this window.
	fn inner_dimensions(&self) -> [u32; 2];
	/// Return the `WindowType` of this window.
	fn window_type(&self) -> WindowType;
	/// DPI scaling used on this window.
	fn scale_factor(&self) -> f32;
	/// Return the `Win32Monitor` used if present.
	fn win32_monitor(&self) -> Option<Win32Monitor>;
	/// Attach an input hook to this window.
	fn attach_input_hook(&self, id: InputHookID);
	/// Used internally
	unsafe fn attach_basalt(&self, basalt: Arc<Basalt>);
}

pub trait BstWindowHooks {
	fn on_press<C: KeyCombo, F>(&self, combo: C, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl
			+ Send
			+ 'static;
	fn on_release<C: KeyCombo, F>(&self, combo: C, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl
			+ Send
			+ 'static;
	fn on_hold<C: KeyCombo, F>(&self, combo: C, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl
			+ Send
			+ 'static;
	fn on_character<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + 'static;
	fn on_enter<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static;
	fn on_leave<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static;
	fn on_focus<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static;
	fn on_focus_lost<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static;
	fn on_scroll<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + 'static;
	fn on_cursor<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
			+ Send
			+ 'static;
}

impl BstWindowHooks for Arc<dyn BasaltWindow> {
	fn on_press<C: KeyCombo, F>(&self, combo: C, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl
			+ Send
			+ 'static,
	{
		self.basalt()
			.input_ref()
			.hook()
			.window(self)
			.on_press()
			.keys(combo)
			.call(method)
			.finish()
			.unwrap()
	}

	fn on_release<C: KeyCombo, F>(&self, combo: C, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl
			+ Send
			+ 'static,
	{
		self.basalt()
			.input_ref()
			.hook()
			.window(self)
			.on_release()
			.keys(combo)
			.call(method)
			.finish()
			.unwrap()
	}

	fn on_hold<C: KeyCombo, F>(&self, combo: C, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl
			+ Send
			+ 'static,
	{
		self.basalt()
			.input_ref()
			.hook()
			.window(self)
			.on_hold()
			.keys(combo)
			.call(method)
			.finish()
			.unwrap()
	}

	fn on_character<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + 'static,
	{
		self.basalt()
			.input_ref()
			.hook()
			.window(self)
			.on_character()
			.call(method)
			.finish()
			.unwrap()
	}

	fn on_enter<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
	{
		self.basalt().input_ref().hook().window(self).on_enter().call(method).finish().unwrap()
	}

	fn on_leave<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
	{
		self.basalt().input_ref().hook().window(self).on_leave().call(method).finish().unwrap()
	}

	fn on_focus<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
	{
		self.basalt().input_ref().hook().window(self).on_focus().call(method).finish().unwrap()
	}

	fn on_focus_lost<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
	{
		self.basalt()
			.input_ref()
			.hook()
			.window(self)
			.on_focus_lost()
			.call(method)
			.finish()
			.unwrap()
	}

	fn on_scroll<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + 'static,
	{
		self.basalt().input_ref().hook().window(self).on_scroll().call(method).finish().unwrap()
	}

	fn on_cursor<F>(&self, method: F) -> InputHookID
	where
		F: FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
			+ Send
			+ 'static,
	{
		self.basalt().input_ref().hook().window(self).on_cursor().call(method).finish().unwrap()
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowType {
	UnixXlib,
	UnixXCB,
	UnixWayland,
	Windows,
	Macos,
	NotSupported,
}

/// Determines how the application should go into full screen.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum FullScreenBehavior {
	#[default]
	/// **Default**
	///
	/// If fullscreen exclusive support is enabled this uses, `AutoExclusive` otherwise `AutoBorderless`.
	Auto,
	/// Enable borderless full screen on a monitor determined by this order:
	/// 1. Current Monitor
	/// 2. Primary Monitor
	/// 3. Implementation decides
	AutoBorderless,
	/// Enable borderless full screen on the primary monitor.
	AutoBorderlessPrimary,
	/// Enable borderless full screen on the current monitor.
	AutoBorderlessCurrent,
	/// Enable borderless full screen on the provided monitor.
	Borderless(Monitor),
	/// Enable exclusive full screen on a monitor determined by this order:
	/// 1. Current Monitor
	/// 2. Primary Monitor
	/// 3. First enumerated
	AutoExclusive,
	/// Enable exclusive full screen on the primary monitor
	///
	/// See `Monitor::optimal_mode` for how the mode is determined.
	AutoExclusivePrimary,
	/// Enable exclusive full screen on the current monitor
	///
	/// See `Monitor::optimal_mode` for how the mode is determined.
	AutoExclusiveCurrent,
	/// Enable exclusive full screen on the provided monitor and automatically select the mode.
	///
	/// See `Monitor::optimal_mode` for how the mode is determined.
	ExclusiveAutoMode(Monitor),
	/// Enable exclusive full screen on the provided monitor and mode.
	Exclusive(Monitor, MonitorMode),
}

impl FullScreenBehavior {
	/// Returns false for `Auto`
	pub fn is_exclusive(&self) -> bool {
		match self {
			Self::Auto => false,
			Self::AutoBorderless => false,
			Self::AutoBorderlessPrimary => false,
			Self::AutoBorderlessCurrent => false,
			Self::Borderless(_) => false,
			Self::AutoExclusive => true,
			Self::AutoExclusivePrimary => true,
			Self::AutoExclusiveCurrent => true,
			Self::ExclusiveAutoMode(_) => true,
			Self::Exclusive(..) => true,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullScreenError {
	/// The window implmentation is unable to determine the primary monitor.
	UnableToDeterminePrimary,
	/// The window implmentation is unable to determine the current monitor.
	UnableToDetermineCurrent,
	/// Attempted to use exclusive fullscreen, when it wasn't enabled.
	///
	/// See: `BstOptions::use_exclusive_fullscreen`
	ExclusiveNotSupported,
	/// The monitor no longer exists.
	MonitorDoesNotExist,
	/// No available monitors
	NoAvailableMonitors,
	/// The provided mode doesn't belong to the monitor.
	IncompatibleMonitorMode,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MonitorMode {
	resolution: [u32; 2],
	bit_depth: u16,
	refresh_rate: OrderedFloat<f32>,
	handle: MonitorModeHandle,
	monitor_handle: MonitorHandle,
}

impl MonitorMode {
	/// Returns the resolution of this mode.
	pub fn resolution(&self) -> [u32; 2] {
		self.resolution
	}

	/// Returns the bit depth of this mode.
	pub fn bit_depth(&self) -> u16 {
		self.bit_depth
	}

	/// Returns the refresh rate in Hz of this mode.
	pub fn refresh_rate(&self) -> f32 {
		self.refresh_rate.into_inner()
	}
}

impl std::fmt::Debug for MonitorMode {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MonitorMode")
			.field("resolution", &self.resolution)
			.field("bit_depth", &self.bit_depth)
			.field("refresh_rate", &self.refresh_rate.into_inner())
			.finish()
	}
}

#[derive(Clone, PartialEq, Eq)]
pub struct Monitor {
	name: String,
	resolution: [u32; 2],
	position: [i32; 2],
	refresh_rate: OrderedFloat<f32>,
	bit_depth: u16,
	is_current: bool,
	is_primary: bool,
	modes: Vec<MonitorMode>,
	handle: MonitorHandle,
}

impl std::fmt::Debug for Monitor {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Monitor")
			.field("name", &self.name)
			.field("resolution", &self.resolution)
			.field("bit_depth", &self.bit_depth)
			.field("refresh_rate", &self.refresh_rate.into_inner())
			.field("is_current", &self.is_current)
			.field("is_primary", &self.is_primary)
			.field("modes", &self.modes)
			.finish()
	}
}

impl Monitor {
	/// Returns a human-readable name of the monitor.
	pub fn name(&self) -> String {
		self.name.clone()
	}

	/// Returns the monitor’s resolution.
	pub fn resolution(&self) -> [u32; 2] {
		self.resolution
	}

	/// Returns the top-left corner position of the monitor relative to the larger full screen area.
	pub fn position(&self) -> [i32; 2] {
		self.position
	}

	/// Returns the bit depth of this monitor.
	pub fn bit_depth(&self) -> u16 {
		self.bit_depth
	}

	/// The monitor refresh rate used by the system.
	pub fn refresh_rate(&self) -> f32 {
		self.refresh_rate.into_inner()
	}

	/// Returns a list of `MonitorMode`'s supported by this monitor.
	pub fn modes(&self) -> Vec<MonitorMode> {
		self.modes.clone()
	}

	/// Returns `true` if it is the systems primary monitor.
	pub fn is_primary(&self) -> bool {
		self.is_primary
	}

	/// Returns `true` if it is the current monitor in use.
	pub fn is_current(&self) -> bool {
		self.is_current
	}

	/// Get the most optimal mode for this monitor.
	///
	/// # Priority
	/// 1. Resolution (Higher than the monitor are less favorable)
	/// 2. Aspect Ratio
	/// 2. Refresh Rate
	/// 3. Bit Depth
	pub fn optimal_mode(&self) -> MonitorMode {
		assert!(!self.modes.is_empty());

		let mut modes: Vec<&MonitorMode> = {
			let same_resolution: Vec<_> =
				self.modes.iter().filter(|mode| mode.resolution == self.resolution).collect();

			if !same_resolution.is_empty() {
				same_resolution
			} else {
				// Same resolution isn't available, try lesser ones first.
				let mut modes_a: Vec<_> = self
					.modes
					.iter()
					.filter(|mode| {
						mode.resolution[0] <= self.resolution[0]
							&& mode.resolution[1] <= self.resolution[1]
					})
					.collect();

				// No lesser resolution modes, use them all.
				if modes_a.is_empty() {
					modes_a = self.modes.iter().collect();
				}

				// Try to find one with the same aspect ratio
				let ideal_aspect = self.resolution[0] as f32 / self.resolution[1] as f32;
				let mut modes_b: Vec<_> = modes_a
					.iter()
					.filter(|mode| {
						mode.resolution[0] as f32 / mode.resolution[1] as f32 == ideal_aspect
					})
					.collect();

				// No modes with same aspect ratio use modes_a.
				if modes_b.is_empty() {
					// TODO: sort by closest aspect ratio?
					modes_b = modes_a.iter().collect();
				}

				modes_b.sort_by_key(|mode| Reverse(mode.resolution[0] * mode.resolution[1]));
				modes_b.into_iter().map(|mode| *mode).collect()
			}
		};

		let best_resolution = modes[0].resolution;
		modes.retain(|mode| mode.resolution == best_resolution);
		modes.sort_by_key(|mode| Reverse(mode.refresh_rate));
		let best_refresh_rate = modes[0].refresh_rate;
		modes.retain(|mode| mode.refresh_rate == best_refresh_rate);
		modes.sort_by_key(|mode| Reverse(mode.bit_depth));
		let best_bit_depth = modes[0].bit_depth;
		modes.retain(|mode| mode.bit_depth == best_bit_depth);
		modes[0].clone()
	}
}

#[derive(Clone, PartialEq, Eq)]
enum MonitorHandle {
	Winit(winit_ty::MonitorHandle),
}

impl MonitorHandle {
	fn into_winit(self) -> winit_ty::MonitorHandle {
		match self {
			Self::Winit(monitor) => monitor,
		}
	}
}

#[derive(Clone, PartialEq, Eq)]
enum MonitorModeHandle {
	Winit(winit_ty::VideoMode),
}

impl MonitorModeHandle {
	fn into_winit(self) -> winit_ty::VideoMode {
		match self {
			Self::Winit(mode) => mode,
		}
	}
}

pub fn open_surface(
	ops: BstOptions,
	id: BstWindowID,
	instance: Arc<Instance>,
	result_fn: Box<dyn Fn(Result<Arc<Surface<Arc<dyn BasaltWindow>>>, String>) + Send + Sync>,
) {
	winit::open_surface(ops, id, instance, result_fn)
}
