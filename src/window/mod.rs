pub mod winit;
use Basalt;

use std::sync::Arc;
use vulkano::{
    instance::Instance,
    swapchain::Surface,
};
use Options as BasaltOptions;

pub trait BasaltWindow {
    fn capture_cursor(&self);
    fn release_cursor(&self);
    fn cursor_captured(&self) -> bool;
    fn attach_basalt(&self, basalt: Arc<Basalt>);
    fn enable_fullscreen(&self);
    fn disable_fullscreen(&self);
    fn toggle_fullscreen(&self);
    fn request_resize(&self, width: u32, height: u32);
    fn inner_dimensions(&self) -> [u32; 2];
}

pub fn open_surface(
    ops: BasaltOptions,
    instance: Arc<Instance>,
) -> Result<Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>, String> {
    winit::open_surface(ops, instance)
}
