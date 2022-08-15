pub mod winit;

use crate::{Basalt, BstOptions};
use std::sync::Arc;
use vulkano::instance::Instance;
use vulkano::swapchain::{Surface, Win32Monitor};

pub trait BasaltWindow: Send + Sync + std::fmt::Debug {
	fn capture_cursor(&self);
	fn release_cursor(&self);
	fn cursor_captured(&self) -> bool;
	fn attach_basalt(&self, basalt: Arc<Basalt>);
	fn enable_fullscreen(&self);
	fn disable_fullscreen(&self);
	fn toggle_fullscreen(&self);
	fn request_resize(&self, width: u32, height: u32);
	fn inner_dimensions(&self) -> [u32; 2];
	fn window_type(&self) -> WindowType;
	fn scale_factor(&self) -> f32;
	fn win32_monitor(&self) -> Option<Win32Monitor>;
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

pub fn open_surface(
	ops: BstOptions,
	instance: Arc<Instance>,
	result_fn: Box<dyn Fn(Result<Arc<Surface<Arc<dyn BasaltWindow>>>, String>) + Send + Sync>,
) {
	winit::open_surface(ops, instance, result_fn)
}
