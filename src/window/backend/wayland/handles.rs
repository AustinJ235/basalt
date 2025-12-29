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
use crate::window::backend::{BackendHandle, BackendWindowHandle, PendingRes};
use crate::window::builder::WindowAttributes;
use crate::window::{
    CursorIcon, FullScreenBehavior, Monitor, Window, WindowBackend, WindowError, WindowID,
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
    pub use smithay_client_toolkit::seat::pointer_constraints::PointerConstraintsState;
    pub use smithay_client_toolkit::seat::relative_pointer::RelativePointerState;
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
        let wl_connection = wl::Connection::connect_to_env().unwrap();
        let (wl_global_list, event_queue) =
            wl::registry_queue_init::<BackendState>(&wl_connection).unwrap();
        let wl_queue_handle = event_queue.handle();
        let wl_compositor_state =
            wl::CompositorState::bind(&wl_global_list, &wl_queue_handle).unwrap();
        let mut event_loop: cl::EventLoop<BackendState> = cl::EventLoop::try_new().unwrap();

        cl::WaylandSource::new(wl_connection.clone(), event_queue)
            .insert(event_loop.handle())
            .unwrap();
        let (event_send, event_recv) = cl::channel();

        event_loop
            .handle()
            .insert_source(event_recv, move |event, _, backend_state| {
                if let cl::Event::Msg(backend_ev) = event {
                    match backend_ev {
                        BackendEvent::AssociateBasalt {
                            basalt,
                        } => {
                            backend_state.basalt_op = Some(basalt);
                        },
                        BackendEvent::GetMonitors {
                            pending_res,
                        } => {
                            pending_res.set(backend_state.get_monitors());
                        },
                        BackendEvent::CreateWindow {
                            window_id,
                            window_attributes,
                            pending_res,
                        } => {
                            backend_state.create_window(window_id, window_attributes, pending_res);
                        },
                        BackendEvent::CloseWindow {
                            window_id,
                        } => {
                            backend_state.close_window(window_id);
                        },
                        BackendEvent::WindowRequest {
                            window_id,
                            window_request,
                        } => {
                            backend_state.window_request(window_id, window_request);
                        },
                        BackendEvent::Exit => {
                            backend_state.loop_signal.stop();
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

        let wl_registry_state = wl::RegistryState::new(&wl_global_list);
        let wl_seat_state = wl::SeatState::new(&wl_global_list, &wl_queue_handle);
        let wl_output_state = wl::OutputState::new(&wl_global_list, &wl_queue_handle);

        let wl_ptr_constrs_state =
            wl::PointerConstraintsState::bind(&wl_global_list, &wl_queue_handle);
        let wl_relative_ptr_state =
            wl::RelativePointerState::bind(&wl_global_list, &wl_queue_handle);

        // TODO: When is wl_shm not available?
        let wl_shm = wl::Shm::bind(&wl_global_list, &wl_queue_handle).unwrap();
        let loop_signal = event_loop.get_signal();
        let loop_handle = event_loop.handle().clone();

        event_loop
            .run(
                None,
                &mut BackendState {
                    basalt_op: None,
                    window_state: HashMap::new(),
                    surface_to_id: HashMap::new(),
                    id_to_surface: HashMap::new(),
                    focus_window_id: None,
                    seat_state: HashMap::new(),
                    loop_signal,
                    loop_handle,
                    event_send,
                    wl_connection,
                    wl_global_list,
                    wl_queue_handle,
                    wl_compositor_state,
                    wl_registry_state,
                    wl_seat_state,
                    wl_output_state,
                    wl_ptr_constrs_state,
                    wl_relative_ptr_state,
                    wl_shm,
                    wl_xdg_shell_op: None,
                    wl_layer_shell_op: None,
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
    Title {
        pending_res: PendingRes<Result<String, WindowError>>,
    },
    SetTitle {
        title: String,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    Maximized {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    SetMaximized {
        maximized: bool,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    Minimized {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    SetMinimized {
        minimized: bool,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    Size {
        pending_res: PendingRes<Result<[u32; 2], WindowError>>,
    },
    SetSize {
        size: [u32; 2],
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    MinSize {
        pending_res: PendingRes<Result<Option<[u32; 2]>, WindowError>>,
    },
    SetMinSize {
        min_size_op: Option<[u32; 2]>,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    MaxSize {
        pending_res: PendingRes<Result<Option<[u32; 2]>, WindowError>>,
    },
    SetMaxSize {
        max_size_op: Option<[u32; 2]>,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    CursorIcon {
        pending_res: PendingRes<Result<CursorIcon, WindowError>>,
    },
    SetCursorIcon {
        cursor_icon: CursorIcon,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    CursorVisible {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    SetCursorVisible {
        visible: bool,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    CursorLocked {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    SetCursorLocked {
        locked: bool,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    CursorConfined {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    SetCursorConfined {
        confined: bool,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    CursorCaptured {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    SetCursorCaptured {
        captured: bool,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    Monitor {
        pending_res: PendingRes<Result<Monitor, WindowError>>,
    },
    FullScreen {
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    EnableFullScreen {
        full_screen_behavior: FullScreenBehavior,
        pending_res: PendingRes<Result<(), WindowError>>,
    },
    DisableFullScreen {
        pending_res: PendingRes<Result<(), WindowError>>,
    },
}

impl WindowRequest {
    pub fn set_err(self, e: WindowError) {
        match self {
            // String
            Self::Title {
                pending_res,
            } => pending_res.set(Err(e)),
            // ()
            Self::SetTitle {
                pending_res, ..
            }
            | Self::SetMaximized {
                pending_res, ..
            }
            | Self::SetMinimized {
                pending_res, ..
            }
            | Self::SetSize {
                pending_res, ..
            }
            | Self::SetMinSize {
                pending_res, ..
            }
            | Self::SetMaxSize {
                pending_res, ..
            }
            | Self::SetCursorIcon {
                pending_res, ..
            }
            | Self::SetCursorVisible {
                pending_res, ..
            }
            | Self::SetCursorLocked {
                pending_res, ..
            }
            | Self::SetCursorConfined {
                pending_res, ..
            }
            | Self::SetCursorCaptured {
                pending_res, ..
            }
            | Self::EnableFullScreen {
                pending_res, ..
            }
            | Self::DisableFullScreen {
                pending_res, ..
            } => pending_res.set(Err(e)),
            // bool
            Self::Maximized {
                pending_res,
            }
            | Self::Minimized {
                pending_res,
            }
            | Self::CursorVisible {
                pending_res,
            }
            | Self::CursorLocked {
                pending_res,
            }
            | Self::CursorConfined {
                pending_res,
            }
            | Self::CursorCaptured {
                pending_res,
            }
            | Self::FullScreen {
                pending_res,
            } => pending_res.set(Err(e)),
            // [u32; 2]
            Self::Size {
                pending_res,
            } => pending_res.set(Err(e)),
            // Option<[u32; 2]>
            Self::MinSize {
                pending_res,
            }
            | Self::MaxSize {
                pending_res,
            } => pending_res.set(Err(e)),
            // CursorIcon,
            Self::CursorIcon {
                pending_res,
            } => pending_res.set(Err(e)),
            // Monitor
            Self::Monitor {
                pending_res,
            } => pending_res.set(Err(e)),
        }
    }
}

macro_rules! window_request {
    ($self:ident, $variant:ident $(, $field:ident)*) => {
        {
            let pending_res = PendingRes::empty();

            $self.event_send
                .send(BackendEvent::WindowRequest {
                    window_id: $self.window_id,
                    window_request: WindowRequest::$variant {
                        $($field,)*
                        pending_res: pending_res.clone(),
                    },
                })
                .map_err(|_| WindowError::BackendExited)?;

            pending_res.wait()
        }
    };
}

pub struct WlWindowHandle {
    pub(super) window_id: WindowID,
    pub(super) is_layer: bool,
    pub(super) wl_display: wl::Display,
    pub(super) wl_surface: wl::Surface,
    pub(super) event_send: cl::Sender<BackendEvent>,
}

impl BackendWindowHandle for WlWindowHandle {
    fn backend(&self) -> WindowBackend {
        WindowBackend::Wayland
    }

    fn win32_monitor(&self) -> Result<vko::Win32Monitor, WindowError> {
        Err(WindowError::NotSupported)
    }

    fn title(&self) -> Result<String, WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, Title)
    }

    fn set_title(&self, title: String) -> Result<(), WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, SetTitle, title)
    }

    fn maximized(&self) -> Result<bool, WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, Maximized)
    }

    fn set_maximized(&self, maximized: bool) -> Result<(), WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, SetMaximized, maximized)
    }

    fn minimized(&self) -> Result<bool, WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, Minimized)
    }

    fn set_minimized(&self, minimized: bool) -> Result<(), WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, SetMinimized, minimized)
    }

    fn size(&self) -> Result<[u32; 2], WindowError> {
        window_request!(self, Size)
    }

    fn set_size(&self, size: [u32; 2]) -> Result<(), WindowError> {
        window_request!(self, SetSize, size)
    }

    fn min_size(&self) -> Result<Option<[u32; 2]>, WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, MinSize)
    }

    fn set_min_size(&self, min_size_op: Option<[u32; 2]>) -> Result<(), WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, SetMinSize, min_size_op)
    }

    fn max_size(&self) -> Result<Option<[u32; 2]>, WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, MaxSize)
    }

    fn set_max_size(&self, max_size_op: Option<[u32; 2]>) -> Result<(), WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, SetMaxSize, max_size_op)
    }

    fn cursor_icon(&self) -> Result<CursorIcon, WindowError> {
        window_request!(self, CursorIcon)
    }

    fn set_cursor_icon(&self, cursor_icon: CursorIcon) -> Result<(), WindowError> {
        window_request!(self, SetCursorIcon, cursor_icon)
    }

    fn cursor_visible(&self) -> Result<bool, WindowError> {
        window_request!(self, CursorVisible)
    }

    fn set_cursor_visible(&self, visible: bool) -> Result<(), WindowError> {
        window_request!(self, SetCursorVisible, visible)
    }

    fn cursor_locked(&self) -> Result<bool, WindowError> {
        window_request!(self, CursorLocked)
    }

    fn set_cursor_locked(&self, locked: bool) -> Result<(), WindowError> {
        window_request!(self, SetCursorLocked, locked)
    }

    fn cursor_confined(&self) -> Result<bool, WindowError> {
        window_request!(self, CursorConfined)
    }

    fn set_cursor_confined(&self, confined: bool) -> Result<(), WindowError> {
        window_request!(self, SetCursorConfined, confined)
    }

    fn cursor_captured(&self) -> Result<bool, WindowError> {
        window_request!(self, CursorCaptured)
    }

    fn set_cursor_captured(&self, captured: bool) -> Result<(), WindowError> {
        window_request!(self, SetCursorCaptured, captured)
    }

    fn monitor(&self) -> Result<Monitor, WindowError> {
        window_request!(self, Monitor)
    }

    fn full_screen(&self) -> Result<bool, WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, FullScreen)
    }

    fn enable_full_screen(
        &self,
        borderless_fallback: bool,
        full_screen_behavior: FullScreenBehavior,
    ) -> Result<(), WindowError> {
        if self.is_layer || (!borderless_fallback && full_screen_behavior.is_exclusive()) {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, EnableFullScreen, full_screen_behavior)
    }

    fn disable_full_screen(&self) -> Result<(), WindowError> {
        if self.is_layer {
            return Err(WindowError::NotSupported);
        }

        window_request!(self, DisableFullScreen)
    }
}

impl Drop for WlWindowHandle {
    fn drop(&mut self) {
        let _ = self.event_send.send(BackendEvent::CloseWindow {
            window_id: self.window_id,
        });
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
