use std::ptr::NonNull;
use std::sync::Arc;
use std::thread::spawn;

use foldhash::{HashMap, HashMapExt};
use raw_window_handle::{
    DisplayHandle, HandleError as RwhHandleError, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::reexports::client::Proxy;

use super::BackendState;
use crate::Basalt;
use crate::window::backend::PendingRes;
use crate::window::builder::WindowAttributes;
use crate::window::window::BackendWindowHandle;
use crate::window::{
    BackendHandle, FullScreenBehavior, Monitor, Window, WindowBackend, WindowError, WindowID,
};

mod vko {
    pub use vulkano::swapchain::Win32Monitor;
}

mod wl {
    pub use smithay_client_toolkit::compositor::CompositorState;
    pub use smithay_client_toolkit::output::OutputState;
    pub use smithay_client_toolkit::reexports::client::Connection;
    pub use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_display::WlDisplay as Display;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface as Surface;
    pub use smithay_client_toolkit::registry::RegistryState;
    pub use smithay_client_toolkit::seat::SeatState;
    pub use smithay_client_toolkit::shm::Shm;
}

mod cl {
    pub use smithay_client_toolkit::reexports::calloop::EventLoop;
    pub use smithay_client_toolkit::reexports::calloop::channel::{Event, Sender, channel};
    pub use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
}

pub enum BackendEvent {
    AssociateBasalt {
        basalt: Arc<Basalt>,
    },
    GetMonitors {
        pending_res: PendingRes<Result<Vec<Monitor>, WindowError>>,
    },
    CreateWindow {
        window_id: WindowID,
        window_attributes: WindowAttributes,
        pending_res: PendingRes<Result<Arc<Window>, WindowError>>,
    },
    CloseWindow {
        window_id: WindowID,
    },
    Exit,
    WindowRequest {
        window_id: WindowID,
        window_request: WindowRequest,
    },
}

pub struct WlBackendHandle {
    pub(super) event_send: cl::Sender<BackendEvent>,
}

impl WlBackendHandle {
    pub fn run<F>(exec: F)
    where
        F: FnOnce(Self) + Send + 'static,
    {
        let connection = wl::Connection::connect_to_env().unwrap();
        let (global_list, event_queue) =
            wl::registry_queue_init::<BackendState>(&connection).unwrap();
        let queue_handle = event_queue.handle();
        let compositor_state = wl::CompositorState::bind(&global_list, &queue_handle).unwrap();

        let mut event_loop: cl::EventLoop<BackendState> = cl::EventLoop::try_new().unwrap();
        cl::WaylandSource::new(connection.clone(), event_queue)
            .insert(event_loop.handle())
            .unwrap();
        let (event_send, event_recv) = cl::channel();

        event_loop
            .handle()
            .insert_source(event_recv, move |event, _, wl_backend_state| {
                if let cl::Event::Msg(backend_ev) = event {
                    match backend_ev {
                        BackendEvent::AssociateBasalt {
                            basalt,
                        } => {
                            wl_backend_state.basalt_op = Some(basalt);
                        },
                        BackendEvent::GetMonitors {
                            pending_res,
                        } => {
                            pending_res.set(wl_backend_state.get_monitors());
                        },
                        BackendEvent::CreateWindow {
                            window_id,
                            window_attributes,
                            pending_res,
                        } => {
                            wl_backend_state.create_window(
                                window_id,
                                window_attributes,
                                pending_res,
                            );
                        },
                        BackendEvent::CloseWindow {
                            window_id,
                        } => {
                            wl_backend_state.close_window(window_id);
                        },
                        BackendEvent::WindowRequest {
                            window_id,
                            window_request,
                        } => {
                            wl_backend_state.window_request(window_id, window_request);
                        },
                        BackendEvent::Exit => {
                            wl_backend_state.loop_signal.stop();
                        },
                    }
                }
            })
            .unwrap();

        let thrd_event_send = event_send.clone();

        spawn(move || {
            exec(Self {
                event_send: thrd_event_send,
            });
        });

        let registry_state = wl::RegistryState::new(&global_list);
        let seat_state = wl::SeatState::new(&global_list, &queue_handle);
        let output_state = wl::OutputState::new(&global_list, &queue_handle);

        // TODO: When is wl_shm not available?
        let shm = wl::Shm::bind(&global_list, &queue_handle).unwrap();

        let loop_signal = event_loop.get_signal();

        event_loop
            .run(
                None,
                &mut BackendState {
                    basalt_op: None,
                    window_state: HashMap::new(),
                    surface_to_id: HashMap::new(),
                    id_to_surface: HashMap::new(),
                    loop_signal,
                    event_send,
                    connection,
                    global_list,
                    queue_handle,
                    compositor_state,
                    registry_state,
                    seat_state,
                    output_state,
                    shm,
                    xdg_shell: None,
                    layer_shell: None,
                    keyboards: HashMap::new(),
                    pointers: HashMap::new(),
                    focus_window_id: None,
                },
                |_| (),
            )
            .unwrap();
    }
}

impl BackendHandle for WlBackendHandle {
    fn window_backend(&self) -> WindowBackend {
        WindowBackend::Wayland
    }

    fn associate_basalt(&self, basalt: Arc<Basalt>) {
        let _ = self.event_send.send(BackendEvent::AssociateBasalt {
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
            .send(BackendEvent::CreateWindow {
                window_id,
                window_attributes,
                pending_res: pending_res.clone(),
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn close_window(&self, window_id: WindowID) -> Result<(), WindowError> {
        self.event_send
            .send(BackendEvent::CloseWindow {
                window_id,
            })
            .map_err(|_| WindowError::BackendExited)
    }

    fn get_monitors(&self) -> Result<Vec<Monitor>, WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(BackendEvent::GetMonitors {
                pending_res: pending_res.clone(),
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn get_primary_monitor(&self) -> Result<Monitor, WindowError> {
        Err(WindowError::NotSupported)
    }

    fn exit(&self) {
        let _ = self.event_send.send(BackendEvent::Exit);
    }
}

pub enum WindowRequest {
    GetInnerSize {
        pending_res: PendingRes<Result<[u32; 2], WindowError>>,
    },
    Resize {
        window_size: [u32; 2],
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    GetCurrentMonitor {
        pending_res: PendingRes<Result<Monitor, WindowError>>,
    },
    IsFullscreen {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    EnableFullscreen {
        fullscreen_behavior: FullScreenBehavior,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    DisableFullscreen {
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    CaptureCursor {
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    ReleaseCursor {
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    IsCursorCaptured {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
}

impl WindowRequest {
    pub fn set_err(self, e: WindowError) {
        match self {
            Self::GetInnerSize {
                pending_res,
            } => {
                pending_res.set(Err(e));
            },
            Self::GetCurrentMonitor {
                pending_res,
            } => {
                pending_res.set(Err(e));
            },
            Self::IsFullscreen {
                pending_res,
            }
            | Self::IsCursorCaptured {
                pending_res,
            } => {
                pending_res.set(Err(e));
            },
            Self::Resize {
                pending_res, ..
            }
            | Self::EnableFullscreen {
                pending_res, ..
            }
            | Self::DisableFullscreen {
                pending_res, ..
            }
            | Self::CaptureCursor {
                pending_res, ..
            }
            | Self::ReleaseCursor {
                pending_res, ..
            } => {
                pending_res.set(Err(e));
            },
        }
    }
}

pub struct WlWindowHandle {
    pub(super) window_id: WindowID,
    pub(super) is_layer: bool,
    pub(super) wl_display: wl::Display,
    pub(super) wl_surface: wl::Surface,
    pub(super) event_send: cl::Sender<BackendEvent>,
}

impl BackendWindowHandle for WlWindowHandle {
    fn resize(&self, window_size: [u32; 2]) -> Result<(), WindowError> {
        let pending_res = PendingRes::empty();

        self.event_send
            .send(BackendEvent::WindowRequest {
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
            .send(BackendEvent::WindowRequest {
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
            .send(BackendEvent::WindowRequest {
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
            .send(BackendEvent::WindowRequest {
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
            .send(BackendEvent::WindowRequest {
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
            .send(BackendEvent::WindowRequest {
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
            .send(BackendEvent::WindowRequest {
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
            .send(BackendEvent::WindowRequest {
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
            .send(BackendEvent::WindowRequest {
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
