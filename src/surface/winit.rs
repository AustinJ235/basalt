use super::{
    BackendRequest,
    BstSurface,
    BstSurfaceBuilder,
    BstSurfaceCaps,
    SurfaceBackend,
    SurfaceRequest,
};

use crate::Basalt;
use crossbeam::queue::SegQueue;
use std::{borrow::Borrow, sync::Arc};
use vulkano::{
    instance::Instance,
    swapchain::{Surface, SurfaceCreationError},
};

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

pub(crate) struct WinitBackend {
    instance: Arc<Instance>,
    event_loop: winit_ty::EventLoop<()>,
    surfaces: Vec<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>>,
    backend_req_queue: Arc<SegQueue<BackendRequest>>,
}

impl WinitBackend {
    pub fn new(
        instance: Arc<Instance>,
        backend_req_queue: Arc<SegQueue<BackendRequest>>,
    ) -> Box<dyn SurfaceBackend> {
        Box::new(WinitBackend {
            instance,
            event_loop: winit_ty::EventLoop::new(),
            surfaces: Vec::new(),
            backend_req_queue,
        })
    }
}

impl SurfaceBackend for WinitBackend {
    fn create_surface(
        &mut self,
        builder: BstSurfaceBuilder,
    ) -> Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, String> {
        let winit_window = winit_ty::WindowBuilder::new()
            .with_inner_size(winit_ty::PhysicalSize::<u32>::from(builder.size))
            .with_title(builder.title)
            .build(&self.event_loop)
            .map_err(|e| format!("Failed to create window: {:?}", e))?;

        let bst_surface = unsafe {
            winit_to_surface(self.instance.clone(), winit_window)
                .map_err(|e| format!("Failed to create surface from window: {}", e))
        }?;

        self.surfaces.push(bst_surface.clone());
        Ok(bst_surface)
    }

    fn run(self: Box<Self>, basalt: Arc<Basalt>) {
        unimplemented!()
    }
}

impl BstSurface for winit_ty::Window {
    fn capture_cursor(&self) {
        unimplemented!()
    }

    fn release_cursor(&self) {
        unimplemented!()
    }

    fn is_cursor_captured(&self) -> bool {
        unimplemented!()
    }

    fn enable_fullscreen(&self) {
        unimplemented!()
    }

    fn enable_exclusive_fullscreen(&self) {
        unimplemented!()
    }

    fn disable_fullscreen(&self) {
        unimplemented!()
    }

    fn toggle_fullscreen(&self) {
        unimplemented!()
    }

    fn toggle_exclusive_fullscreen(&self) {
        unimplemented!()
    }

    fn is_fullscreen_active(&self) -> bool {
        unimplemented!()
    }

    fn is_exclusive_fullscreen_active(&self) -> bool {
        unimplemented!()
    }

    fn enable_fullscreen_prefer_exclusive(&self) {
        unimplemented!()
    }

    fn toggle_fullscreen_prefer_exclusive(&self) {
        unimplemented!()
    }

    fn capabilities(&self) -> BstSurfaceCaps {
        unimplemented!()
    }
}

#[cfg(target_os = "android")]
unsafe fn winit_to_surface(
    instance: Arc<Instance>,
    win: winit_ty::Window,
) -> Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, SurfaceCreationError> {
    use winit::platform::android::WindowExtAndroid;

    Surface::from_anativewindow(instance, win.borrow().native_window(), Arc::new(win))
}

#[cfg(all(unix, not(target_os = "android"), not(target_os = "macos")))]
unsafe fn winit_to_surface(
    instance: Arc<Instance>,
    win: winit_ty::Window,
) -> Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, SurfaceCreationError> {
    use winit::platform::unix::WindowExtUnix;

    match (win.borrow().wayland_display(), win.borrow().wayland_surface()) {
        (Some(display), Some(surface)) => {
            Surface::from_wayland(instance, display, surface, win)
        },
        _ => {
            // No wayland display found, check if we can use xlib.
            // If not, we use xcb.
            if instance.loaded_extensions().khr_xlib_surface {
                Surface::from_xlib(
                    instance,
                    win.borrow().xlib_display().unwrap(),
                    win.borrow().xlib_window().unwrap() as _,
                    Arc::new(win),
                )
            } else {
                Surface::from_xcb(
                    instance,
                    win.borrow().xcb_connection().unwrap(),
                    win.borrow().xlib_window().unwrap() as _,
                    Arc::new(win),
                )
            }
        },
    }
}

#[cfg(target_os = "windows")]
unsafe fn winit_to_surface(
    instance: Arc<Instance>,
    win: winit_ty::Window,
) -> Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, SurfaceCreationError> {
    use winit::platform::windows::WindowExtWindows;

    Surface::from_hwnd(
        instance,
        ::std::ptr::null() as *const (), // FIXME
        win.borrow().hwnd(),
        Arc::new(win),
    )
}

#[cfg(target_os = "macos")]
unsafe fn winit_to_surface(
    instance: Arc<Instance>,
    win: winit_ty::Window,
) -> Result<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>, SurfaceCreationError> {
    use winit::platform::macos::WindowExtMacOS;

    let wnd: cocoa_id = ::std::mem::transmute(win.borrow().ns_window());
    let layer = CoreAnimationLayer::new();

    layer.set_edge_antialiasing_mask(0);
    layer.set_presents_with_transaction(false);
    layer.remove_all_animations();

    let view = wnd.contentView();

    layer.set_contents_scale(view.backingScaleFactor());
    view.setLayer(mem::transmute(layer.as_ref())); // Bombs here with out of memory
    view.setWantsLayer(YES);

    Surface::from_macos_moltenvk(instance, win.borrow().ns_view() as *const (), Arc::new(win))
}
