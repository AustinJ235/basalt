use std::ptr::NonNull;
use std::sync::Arc;

use raw_window_handle::{
    DisplayHandle, HandleError as RwhHandleError, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::reexports::client::Proxy;

use crate::Basalt;
use crate::window::backend::PendingRes;
use crate::window::backend::wayland::{WindowRequest, WlBackendEv};
use crate::window::builder::WindowAttributes;
use crate::window::window::BackendWindowHandle;
use crate::window::{
    BackendHandle, FullScreenBehavior, Monitor, Window, WindowBackend, WindowError, WindowID,
};

mod vko {
    pub use vulkano::swapchain::Win32Monitor;
}

mod wl {
    pub use smithay_client_toolkit::reexports::client::protocol::wl_display::WlDisplay as Display;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface as Surface;
}

mod cl {
    pub use smithay_client_toolkit::reexports::calloop::channel::Sender;
}

pub struct WlBackendHandle {
    pub(super) event_send: cl::Sender<WlBackendEv>,
}

impl BackendHandle for WlBackendHandle {
    fn window_backend(&self) -> WindowBackend {
        WindowBackend::Wayland
    }

    fn associate_basalt(&self, basalt: Arc<Basalt>) {
        let _ = self.event_send.send(WlBackendEv::AssociateBasalt {
            basalt,
        });
    }

    fn create_window(
        &self,
        window_id: WindowID,
        window_attributes: WindowAttributes,
    ) -> Result<Arc<Window>, WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::CreateWindow {
                window_id,
                window_attributes,
                pending_res: pending_res.clone(),
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn close_window(&self, window_id: WindowID) -> Result<(), WindowError> {
        self.event_send
            .send(WlBackendEv::CloseWindow {
                window_id,
            })
            .map_err(|_| WindowError::BackendExited)
    }

    fn get_monitors(&self) -> Result<Vec<Monitor>, WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::GetMonitors {
                pending_res: pending_res.clone(),
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn get_primary_monitor(&self) -> Result<Monitor, WindowError> {
        Err(WindowError::NotSupported)
    }

    fn exit(&self) {
        let _ = self.event_send.send(WlBackendEv::Exit);
    }
}

pub struct WlWindowHandle {
    pub(super) window_id: WindowID,
    pub(super) is_layer: bool,
    pub(super) wl_display: wl::Display,
    pub(super) wl_surface: wl::Surface,
    pub(super) event_send: cl::Sender<WlBackendEv>,
}

impl BackendWindowHandle for WlWindowHandle {
    fn resize(&self, window_size: [u32; 2]) -> Result<(), WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::Resize {
                    window_size,
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn inner_size(&self) -> Result<[u32; 2], WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::GetInnerSize {
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn backend(&self) -> WindowBackend {
        WindowBackend::Wayland
    }

    fn win32_monitor(&self) -> Result<vko::Win32Monitor, WindowError> {
        Err(WindowError::NotSupported)
    }

    fn capture_cursor(&self) -> Result<(), WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::CaptureCursor {
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn release_cursor(&self) -> Result<(), WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::ReleaseCursor {
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn cursor_captured(&self) -> Result<bool, WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::IsCursorCaptured {
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn current_monitor(&self) -> Result<Monitor, WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::GetCurrentMonitor {
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn enable_fullscreen(
        &self,
        borderless_fallback: bool,
        fullscreen_behavior: FullScreenBehavior,
    ) -> Result<(), WindowError> {
        if self.is_layer || (!borderless_fallback && fullscreen_behavior.is_exclusive()) {
            return Err(WindowError::NotSupported);
        }

        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::EnableFullscreen {
                    fullscreen_behavior,
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn disable_fullscreen(&self) -> Result<(), WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::DisableFullscreen {
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn toggle_fullscreen(&self) -> Result<(), WindowError> {
        // TODO: This makes two requests to the event loop!

        if self.is_fullscreen()? {
            self.disable_fullscreen()
        } else {
            self.enable_fullscreen(true, FullScreenBehavior::Auto)
        }
    }

    fn is_fullscreen(&self) -> Result<bool, WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        let pending_res = PendingRes::empty();

        self.event_send
            .send(WlBackendEv::WindowRequest {
                window_id: self.window_id,
                window_request: WindowRequest::IsFullscreen {
                    pending_res: pending_res.clone(),
                },
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }
}

impl HasWindowHandle for WlWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, RwhHandleError> {
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(self.wl_surface.id().as_ptr() as *mut _).unwrap(),
        ));

        Ok(unsafe { WindowHandle::borrow_raw(raw_window_handle) })
    }
}

impl HasDisplayHandle for WlWindowHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, RwhHandleError> {
        let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(self.wl_display.id().as_ptr() as *mut _).unwrap(),
        ));

        Ok(unsafe { DisplayHandle::borrow_raw(raw_display_handle) })
    }
}
