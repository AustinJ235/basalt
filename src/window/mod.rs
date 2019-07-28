pub mod winit;
#[cfg(target_os = "linux")]
pub mod x11;
use Basalt;

use std::sync::Arc;
use vulkano::swapchain::Surface;
use vulkano::instance::Instance;
use ::InputSource;
use ::Options as BasaltOptions;

pub trait BasaltWindow {
	fn capture_cursor(&self);
	fn release_cursor(&self);
	fn attach_basalt(&self, basalt: Arc<Basalt>);
	fn enable_fullscreen(&self);
	fn disable_fullscreen(&self);
	fn request_resize(&self, width: u32, height: u32);
}

#[allow(unused_assignments)]
pub fn open_surface(ops: BasaltOptions, instance: Arc<Instance>) -> Result<Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>, String> {
	#[cfg(target_os = "linux")]
	{
		match ops.input_src {
			InputSource::Native => x11::open_surface(ops, instance),
			InputSource::Winit => winit::open_surface(ops, instance)
		}
	}
	#[cfg(target_os = "windows")]
	{
		winit::open_surface(ops, instance)
	}
	#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
	{
		winit::open_surface(ops, instance)
	}
}

