use super::{BstSurface, BstSurfaceBuilder};
use std::sync::Arc;
use vulkano::swapchain::Surface;

mod winit_ty {
    pub use winit::{
        dpi::PhysicalSize,
        event::{
            DeviceEvent,
            ElementState,
            Event,
            KeyboardInput,
            MouseButton,
            MouseScrollDelta,
            WindowEvent,
        },
        event_loop::{ControlFlow, EventLoop},
        window::{Fullscreen, Window, WindowBuilder},
    };
}

pub fn build_surface(
    builder: BstSurfaceBuilder,
) -> Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, String> {
    unimplemented!()
}
