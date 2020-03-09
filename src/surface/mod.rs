pub mod winit;

pub(crate) use self::winit::WinitBackend as DefaultSurfaceBackend;

use crate::Basalt;
use crossbeam::queue::SegQueue;
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use vulkano::swapchain::Surface;

pub(crate) struct SurfaceRequest {
    pub builder: BstSurfaceBuilder,
    pub result:
        Arc<Mutex<Option<Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, String>>>>,
    pub result_cond: Arc<Condvar>,
}

pub(crate) enum BackendRequest {
    SurfaceRequest(SurfaceRequest),
}

pub(crate) trait SurfaceBackend {
    fn create_surface(
        &mut self,
        builder: BstSurfaceBuilder,
    ) -> Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, String>;
    fn run(self: Box<Self>, basalt: Arc<Basalt>);
}

pub struct BstSurfaceCaps {
    pub capture_cursor: bool,
    pub fullscreen: bool,
    pub exclusive_fullscreen: bool,
}

pub struct BstSurfaceBuilder {
    pub(crate) size: [u32; 2],
    pub(crate) title: String,
}

impl BstSurfaceBuilder {
    pub fn new() -> Self {
        BstSurfaceBuilder {
            size: [1024, 576],
            title: String::from("Basalt"),
        }
    }

    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = [width, height];
        self
    }

    pub fn with_title<T: Into<String>>(mut self, title: T) -> Self {
        self.title = title.into();
        self
    }
}

pub trait BstSurface {
    /// Grab and hide cursor.
    fn capture_cursor(&self);
    /// Ungrab and show cursor.
    fn release_cursor(&self);
    /// Check the current capture state.
    fn is_cursor_captured(&self) -> bool;
    /// Enable fullscreen via fullscreen window.
    fn enable_fullscreen(&self);
    /// Enable fullscreen via exclusive window.
    fn enable_exclusive_fullscreen(&self);
    /// Disable fullscreen either in fullscreen or exclusive fullscreen.
    fn disable_fullscreen(&self);
    /// Toggle fullscreen. On enable it will use a fullscreen window.
    fn toggle_fullscreen(&self);
    /// Toggle fullscreen. On enable it will use exclusive fullscreen. Support may vary.
    fn toggle_exclusive_fullscreen(&self);
    /// Returns true if either fullscreen exclusive or fullscreen window are active.
    fn is_fullscreen_active(&self) -> bool;
    /// Returns true if fullscreen exclusive is active.
    fn is_exclusive_fullscreen_active(&self) -> bool;
    /// Enable fullscreen with a preference for an exclusive window.
    fn enable_fullscreen_prefer_exclusive(&self);
    /// Toggle fullscreen. On enable it prefers fullscreen exclusive if supported.
    fn toggle_fullscreen_prefer_exclusive(&self);
    /// Get the surface's capabilities for supported methods.
    fn capabilities(&self) -> BstSurfaceCaps;
}
