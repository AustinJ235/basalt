use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::thread::spawn;

use foldhash::{HashMap, HashMapExt, HashSet, HashSetExt};
use ordered_float::OrderedFloat;
use raw_window_handle::{
    DisplayHandle, HandleError as RwhHandleError, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::reexports::client::Proxy;
use smithay_client_toolkit::shell::WaylandSurface;

use crate::Basalt;
use crate::input::{InputEvent, MouseButton, Qwerty};
use crate::window::backend::PendingRes;
use crate::window::builder::WindowAttributes;
use crate::window::monitor::{MonitorHandle, MonitorModeHandle};
use crate::window::window::BackendWindowHandle;
use crate::window::{
    BackendHandle, EnableFullScreenError, FullScreenBehavior, Monitor, MonitorMode, Window,
    WindowBackend, WindowError, WindowEvent, WindowID,
};

mod vko {
    pub use vulkano::swapchain::Win32Monitor;
}

mod wl {
    pub use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
    pub use smithay_client_toolkit::output::{OutputHandler, OutputState};
    pub use smithay_client_toolkit::reexports::client::globals::{GlobalList, registry_queue_init};
    pub use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_output::{Transform, WlOutput};
    pub use smithay_client_toolkit::reexports::client::protocol::wl_pointer::WlPointer;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
    pub use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
    pub use smithay_client_toolkit::reexports::csd_frame::{
        // WindowManagerCapabilities,
        WindowState,
    };
    pub use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
    pub use smithay_client_toolkit::seat::keyboard::{
        KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
    };
    pub use smithay_client_toolkit::seat::pointer::{
        PointerEvent, PointerEventKind, PointerHandler,
    };
    pub use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
    pub use smithay_client_toolkit::shell::wlr_layer::{
        Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
        LayerSurfaceConfigure,
    };
    pub use smithay_client_toolkit::shell::xdg::XdgShell;
    pub use smithay_client_toolkit::shell::xdg::window::{
        DecorationMode, Window, WindowConfigure, WindowDecorations, WindowHandler,
    };
    pub use smithay_client_toolkit::{
        delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
        delegate_registry, delegate_seat, delegate_xdg_shell, delegate_xdg_window,
        registry_handlers,
    };
}

mod cl {
    pub use smithay_client_toolkit::reexports::calloop::channel::{Event, Sender, channel};
    pub use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopSignal};
    pub use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
}

#[derive(Debug)]
pub struct WlLayerAttributes {
    pub namespace_op: Option<String>,
    pub size_op: Option<[u32; 2]>,
    pub anchor: wl::Anchor,
    pub exclusive_zone: i32,
    pub margin_t: i32,
    pub margin_b: i32,
    pub margin_l: i32,
    pub margin_r: i32,
    pub layer: wl::Layer,
    pub keyboard_interactivity: wl::KeyboardInteractivity,
    pub output_op: Option<wl::WlOutput>,
}

#[derive(Debug)]
pub struct WlWindowAttributes {
    pub title: Option<String>,
    pub size: Option<[u32; 2]>,
    pub min_size: Option<[u32; 2]>,
    pub max_size: Option<[u32; 2]>,
    pub minimized: bool,
    pub maximized: bool,
    pub decorations: bool,
}

impl Default for WlWindowAttributes {
    fn default() -> Self {
        Self {
            title: None,
            size: None,
            min_size: None,
            max_size: None,
            minimized: false,
            maximized: false,
            decorations: true,
        }
    }
}

pub struct WlBackendHandle {
    event_send: cl::Sender<WlBackendEv>,
}

impl WlBackendHandle {
    pub fn run<F>(exec: F)
    where
        F: FnOnce(Self) + Send + 'static,
    {
        let connection = wl::Connection::connect_to_env().unwrap();
        let (global_list, event_queue) =
            wl::registry_queue_init::<WlBackendState>(&connection).unwrap();
        let queue_handle = event_queue.handle();
        let compositor_state = wl::CompositorState::bind(&global_list, &queue_handle).unwrap();

        let mut event_loop: cl::EventLoop<WlBackendState> = cl::EventLoop::try_new().unwrap();
        cl::WaylandSource::new(connection.clone(), event_queue)
            .insert(event_loop.handle())
            .unwrap();
        let (event_send, event_recv) = cl::channel();

        event_loop
            .handle()
            .insert_source(event_recv, move |event, _, wl_backend_state| {
                if let cl::Event::Msg(backend_ev) = event {
                    wl_backend_state.proc_backend_ev(backend_ev);
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
        let loop_signal = event_loop.get_signal();

        event_loop
            .run(
                None,
                &mut WlBackendState {
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
                    xdg_shell: None,
                    layer_shell: None,
                    keyboard: None,
                    pointer: None,
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

        if self
            .event_send
            .send(WlBackendEv::CreateWindow {
                window_id,
                window_attributes,
                pending_res: pending_res.clone(),
            })
            .is_err()
        {
            return Err(WindowError::BackendExited);
        }

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

        if self
            .event_send
            .send(WlBackendEv::GetMonitors {
                pending_res: pending_res.clone(),
            })
            .is_err()
        {
            return Err(WindowError::BackendExited);
        }

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
    window_id: WindowID,
    is_ready: Arc<AtomicBool>,
    connection: wl::Connection,
    backing: WlSurfaceBacking,
    event_send: cl::Sender<WlBackendEv>,
}

impl WlWindowHandle {
    fn is_layer(&self) -> bool {
        matches!(self.backing, WlSurfaceBacking::Layer(_))
    }
}

impl BackendWindowHandle for WlWindowHandle {
    fn resize(&self, _window_size: [u32; 2]) -> Result<(), WindowError> {
        // TODO:
        Err(WindowError::NotImplemented)
    }

    fn inner_size(&self) -> Result<[u32; 2], WindowError> {
        if !self.is_ready.load(atomic::Ordering::SeqCst) {
            return Err(WindowError::NotReady);
        }

        let pending_res = PendingRes::empty();

        if self
            .event_send
            .send(WlBackendEv::GetInnerSize {
                window_id: self.window_id,
                pending_res: pending_res.clone(),
            })
            .is_err()
        {
            return Err(WindowError::BackendExited);
        }

        pending_res.wait()
    }

    fn scale_factor(&self) -> Result<f32, WindowError> {
        if !self.is_ready.load(atomic::Ordering::SeqCst) {
            return Err(WindowError::NotReady);
        }

        let pending_res = PendingRes::empty();

        if self
            .event_send
            .send(WlBackendEv::GetScaleFactor {
                window_id: self.window_id,
                pending_res: pending_res.clone(),
            })
            .is_err()
        {
            return Err(WindowError::BackendExited);
        }

        pending_res.wait()
    }

    fn backend(&self) -> WindowBackend {
        WindowBackend::Wayland
    }

    fn win32_monitor(&self) -> Result<vko::Win32Monitor, WindowError> {
        Err(WindowError::NotSupported)
    }

    fn capture_cursor(&self) -> Result<(), WindowError> {
        // TODO:
        Err(WindowError::NotImplemented)
    }

    fn release_cursor(&self) -> Result<(), WindowError> {
        // TODO:
        Err(WindowError::NotImplemented)
    }

    fn cursor_captured(&self) -> Result<bool, WindowError> {
        // TODO:
        Err(WindowError::NotImplemented)
    }

    fn current_monitor(&self) -> Result<Monitor, WindowError> {
        if !self.is_ready.load(atomic::Ordering::SeqCst) {
            return Err(WindowError::NotReady);
        }

        let pending_res = PendingRes::empty();

        if self
            .event_send
            .send(WlBackendEv::GetCurrentMonitor {
                window_id: self.window_id,
                pending_res: pending_res.clone(),
            })
            .is_err()
        {
            return Err(WindowError::BackendExited);
        }

        pending_res.wait()
    }

    fn enable_fullscreen(
        &self,
        borderless_fallback: bool,
        behavior: FullScreenBehavior,
    ) -> Result<(), WindowError> {
        if self.is_layer() {
            return Err(WindowError::NotSupported);
        }

        let fs_supported_pr = PendingRes::empty();

        if self
            .event_send
            .send(WlBackendEv::IsFullscreenSupported {
                window_id: self.window_id,
                pending_res: fs_supported_pr.clone(),
            })
            .is_err()
        {
            return Err(WindowError::BackendExited);
        }

        if !fs_supported_pr.wait()? {
            return Err(WindowError::NotSupported);
        }

        if !borderless_fallback && behavior.is_exclusive() {
            // TODO: exclusive fullscreen not implemented yet...
            return Err(WindowError::NotImplemented);
        }

        let (monitor_op, check_output) = match behavior {
            FullScreenBehavior::AutoBorderlessPrimary
            | FullScreenBehavior::AutoExclusivePrimary => {
                return Err(EnableFullScreenError::UnableToDeterminePrimary.into());
            },
            FullScreenBehavior::Auto
            | FullScreenBehavior::AutoBorderless
            | FullScreenBehavior::AutoExclusive => {
                match self.current_monitor() {
                    Ok(ok) => (Some(ok), false),
                    Err(_) => {
                        let monitors_pr = PendingRes::empty();

                        if self
                            .event_send
                            .send(WlBackendEv::GetMonitors {
                                pending_res: monitors_pr.clone(),
                            })
                            .is_err()
                        {
                            return Err(WindowError::BackendExited);
                        }

                        (
                            monitors_pr
                                .wait()
                                .ok()
                                .and_then(|monitors| monitors.into_iter().next()),
                            false,
                        )
                    },
                }
            },
            FullScreenBehavior::AutoBorderlessCurrent
            | FullScreenBehavior::AutoExclusiveCurrent => {
                match self.current_monitor() {
                    Ok(ok) => (Some(ok), false),
                    Err(_) => return Err(EnableFullScreenError::UnableToDetermineCurrent.into()),
                }
            },
            FullScreenBehavior::Borderless(monitor)
            | FullScreenBehavior::ExclusiveAutoMode(monitor)
            | FullScreenBehavior::Exclusive(monitor, _) => (Some(monitor), true),
        };

        let output_op = monitor_op.map(|monitor| {
            match monitor.handle {
                MonitorHandle::Wayland(output) => output,
                _ => unreachable!(),
            }
        });

        if check_output && let Some(output) = output_op.as_ref() {
            let output_exists_pr = PendingRes::empty();

            if self
                .event_send
                .send(WlBackendEv::CheckOutputExists {
                    output: output.clone(),
                    pending_res: output_exists_pr.clone(),
                })
                .is_err()
            {
                return Err(WindowError::BackendExited);
            }

            if !output_exists_pr.wait() {
                return Err(EnableFullScreenError::MonitorDoesNotExist.into());
            }
        }

        let window = match &self.backing {
            WlSurfaceBacking::Window(window) => window,
            _ => unreachable!(),
        };

        window.set_fullscreen(output_op.as_ref());
        Ok(())
    }

    fn disable_fullscreen(&self) -> Result<(), WindowError> {
        let window = match &self.backing {
            WlSurfaceBacking::Window(window) => window,
            WlSurfaceBacking::Layer(_) => return Err(WindowError::NotSupported),
        };

        window.unset_fullscreen();
        Ok(())
    }

    fn toggle_fullscreen(&self) -> Result<(), WindowError> {
        if self.is_fullscreen()? {
            self.disable_fullscreen()
        } else {
            self.enable_fullscreen(true, FullScreenBehavior::Auto)
        }
    }

    fn is_fullscreen(&self) -> Result<bool, WindowError> {
        if self.is_layer() {
            return Err(WindowError::NotSupported);
        }

        let pending_res = PendingRes::empty();

        if self
            .event_send
            .send(WlBackendEv::IsFullscreen {
                window_id: self.window_id,
                pending_res: pending_res.clone(),
            })
            .is_err()
        {
            return Err(WindowError::BackendExited);
        }

        pending_res.wait()
    }
}

impl HasWindowHandle for WlWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, RwhHandleError> {
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(self.backing.wl_surface().id().as_ptr() as *mut _).unwrap(),
        ));

        Ok(unsafe { WindowHandle::borrow_raw(raw_window_handle) })
    }
}

impl HasDisplayHandle for WlWindowHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, RwhHandleError> {
        let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(self.connection.backend().display_ptr() as *mut _).unwrap(),
        ));

        Ok(unsafe { DisplayHandle::borrow_raw(raw_display_handle) })
    }
}

enum WlSurfaceBacking {
    Layer(wl::LayerSurface),
    Window(wl::Window),
}

impl WaylandSurface for WlSurfaceBacking {
    fn wl_surface(&self) -> &wl::WlSurface {
        match self {
            Self::Layer(layer) => layer.wl_surface(),
            Self::Window(window) => window.wl_surface(),
        }
    }
}

struct WlWindowState {
    window: Arc<Window>,

    is_ready: Arc<AtomicBool>,
    create_pending_res: Option<PendingRes<Result<Arc<Window>, WindowError>>>,

    inner_size: [u32; 2],
    scale_factor: f32,
    keys_pressed: HashSet<Qwerty>,
    cur_output_op: Option<wl::WlOutput>,
    last_configure: Option<wl::WindowConfigure>,
}

enum WlBackendEv {
    AssociateBasalt {
        basalt: Arc<Basalt>,
    },
    CreateWindow {
        window_id: WindowID,
        window_attributes: WindowAttributes,
        pending_res: PendingRes<Result<Arc<Window>, WindowError>>,
    },
    CloseWindow {
        window_id: WindowID,
    },
    GetMonitors {
        pending_res: PendingRes<Result<Vec<Monitor>, WindowError>>,
    },
    GetInnerSize {
        window_id: WindowID,
        pending_res: PendingRes<Result<[u32; 2], WindowError>>,
    },
    GetScaleFactor {
        window_id: WindowID,
        pending_res: PendingRes<Result<f32, WindowError>>,
    },
    GetCurrentMonitor {
        window_id: WindowID,
        pending_res: PendingRes<Result<Monitor, WindowError>>,
    },
    IsFullscreen {
        window_id: WindowID,
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    IsFullscreenSupported {
        window_id: WindowID,
        pending_res: PendingRes<Result<bool, WindowError>>,
    },
    CheckOutputExists {
        output: wl::WlOutput,
        pending_res: PendingRes<bool>,
    },
    Exit,
}

struct WlBackendState {
    basalt_op: Option<Arc<Basalt>>,

    window_state: HashMap<WindowID, WlWindowState>,
    surface_to_id: HashMap<wl::WlSurface, WindowID>,
    id_to_surface: HashMap<WindowID, wl::WlSurface>,

    loop_signal: cl::LoopSignal,
    event_send: cl::Sender<WlBackendEv>,

    connection: wl::Connection,
    queue_handle: wl::QueueHandle<Self>,
    global_list: wl::GlobalList,

    registry_state: wl::RegistryState,
    output_state: wl::OutputState,
    seat_state: wl::SeatState,
    compositor_state: wl::CompositorState,

    xdg_shell: Option<wl::XdgShell>,
    layer_shell: Option<wl::LayerShell>,

    keyboard: Option<wl::WlKeyboard>,
    pointer: Option<wl::WlPointer>,

    focus_window_id: Option<WindowID>,
}

wl::delegate_registry!(WlBackendState);
wl::delegate_compositor!(WlBackendState);
wl::delegate_output!(WlBackendState);
wl::delegate_seat!(WlBackendState);
wl::delegate_keyboard!(WlBackendState);
wl::delegate_pointer!(WlBackendState);
wl::delegate_layer!(WlBackendState);
wl::delegate_xdg_shell!(WlBackendState);
wl::delegate_xdg_window!(WlBackendState);

impl WlBackendState {
    fn proc_backend_ev(&mut self, event: WlBackendEv) {
        match event {
            WlBackendEv::AssociateBasalt {
                basalt,
            } => {
                self.basalt_op = Some(basalt);
            },
            WlBackendEv::CreateWindow {
                window_id,
                window_attributes,
                pending_res,
            } => {
                let basalt = self.basalt_op.as_ref().expect("unreachable");

                let (wl_surface_backing, inner_size) = match window_attributes {
                    WindowAttributes::WlLayer(attributes) => {
                        if self.layer_shell.is_none() {
                            match wl::LayerShell::bind(&self.global_list, &self.queue_handle) {
                                Ok(layer_shell) => self.layer_shell = Some(layer_shell),
                                Err(_) => {
                                    pending_res.set(Err(WindowError::NotSupported));
                                    return;
                                },
                            }
                        }

                        let layer_shell = self.layer_shell.as_ref().unwrap();
                        let surface = self.compositor_state.create_surface(&self.queue_handle);

                        let layer_surface = layer_shell.create_layer_surface(
                            &self.queue_handle,
                            surface,
                            wl::Layer::Top,
                            attributes.namespace_op,
                            attributes.output_op.as_ref(),
                        );

                        if let Some([width, height]) = attributes.size_op {
                            layer_surface.set_size(width, height);
                        }

                        layer_surface.set_margin(
                            attributes.margin_t,
                            attributes.margin_r,
                            attributes.margin_b,
                            attributes.margin_l,
                        );

                        layer_surface.set_anchor(attributes.anchor);
                        layer_surface.set_exclusive_zone(attributes.exclusive_zone);
                        layer_surface.set_layer(attributes.layer);
                        layer_surface.set_keyboard_interactivity(attributes.keyboard_interactivity);
                        layer_surface.commit();
                        let inner_size = attributes.size_op.unwrap_or([0; 2]);
                        (WlSurfaceBacking::Layer(layer_surface), inner_size)
                    },
                    WindowAttributes::WlWindow(attributes) => {
                        if self.xdg_shell.is_none() {
                            match wl::XdgShell::bind(&self.global_list, &self.queue_handle) {
                                Ok(xdg_shell) => self.xdg_shell = Some(xdg_shell),
                                Err(_) => {
                                    pending_res.set(Err(WindowError::NotSupported));
                                    return;
                                },
                            }
                        }

                        let xdg_shell = self.xdg_shell.as_ref().unwrap();
                        let surface = self.compositor_state.create_surface(&self.queue_handle);

                        let xdg_window = xdg_shell.create_window(
                            surface,
                            wl::WindowDecorations::RequestServer,
                            &self.queue_handle,
                        );

                        if let Some(title) = attributes.title {
                            xdg_window.set_title(title);
                        }

                        if let Some(_size) = attributes.size {
                            // TODO: How to set this?
                        }

                        if let Some(min_size) = attributes.min_size {
                            xdg_window.set_min_size(Some((min_size[0], min_size[1])));
                        }

                        if let Some(max_size) = attributes.max_size {
                            xdg_window.set_max_size(Some((max_size[0], max_size[1])));
                        }

                        if attributes.minimized {
                            xdg_window.set_minimized();
                        }

                        if attributes.maximized {
                            xdg_window.set_maximized();
                        }

                        if attributes.decorations {
                            xdg_window.request_decoration_mode(Some(wl::DecorationMode::Client));
                        }

                        xdg_window.commit();
                        let inner_size = attributes.size.unwrap_or([0; 2]);
                        (WlSurfaceBacking::Window(xdg_window), inner_size)
                    },
                    _ => unreachable!(),
                };

                let is_ready = Arc::new(AtomicBool::new(false));

                let wl_window = WlWindowHandle {
                    window_id,
                    is_ready: is_ready.clone(),
                    connection: self.connection.clone(), // TODO: Is this used?
                    backing: wl_surface_backing,
                    event_send: self.event_send.clone(),
                };

                let wl_surface = wl_window.backing.wl_surface().clone();

                let window = match Window::new(basalt.clone(), window_id, wl_window) {
                    Ok(ok) => ok,
                    Err(e) => {
                        pending_res.set(Err(e));
                        return;
                    },
                };

                self.surface_to_id.insert(wl_surface.clone(), window_id);
                self.id_to_surface.insert(window_id, wl_surface);

                self.window_state.insert(
                    window_id,
                    WlWindowState {
                        window: window.clone(),
                        is_ready,
                        create_pending_res: Some(pending_res),
                        inner_size,
                        scale_factor: 1.0, // TODO: Is there a way to fetch this?
                        keys_pressed: HashSet::new(),
                        cur_output_op: None,
                        last_configure: None,
                    },
                );

                // Note: The pending_res will be set and the window manager informed after the first
                //       configure to ensure the window is ready to draw.
            },
            WlBackendEv::CloseWindow {
                window_id,
            } => {
                if let Some(wl_surface) = self.id_to_surface.remove(&window_id) {
                    self.surface_to_id.remove(&wl_surface);
                }

                self.window_state.remove(&window_id);
            },
            WlBackendEv::GetMonitors {
                pending_res,
            } => {
                let mut monitors = Vec::new();

                let cur_output_op = match self.focus_window_id {
                    Some(window_id) => {
                        match self.window_state.get(&window_id) {
                            Some(window_state) => window_state.cur_output_op.clone(),
                            None => None,
                        }
                    },
                    None => None,
                };

                for wl_output in self.output_state.outputs() {
                    if let Some(monitor) = self.output_to_monitor(
                        &wl_output,
                        cur_output_op.is_some() && *cur_output_op.as_ref().unwrap() == wl_output,
                    ) {
                        monitors.push(monitor);
                    }
                }

                pending_res.set(Ok(monitors));
            },
            WlBackendEv::GetInnerSize {
                window_id,
                pending_res,
            } => {
                pending_res.set(
                    self.window_state
                        .get(&window_id)
                        .map(|window_state| window_state.inner_size)
                        .ok_or(WindowError::Closed),
                );
            },
            WlBackendEv::GetScaleFactor {
                window_id,
                pending_res,
            } => {
                match self.window_state.get(&window_id) {
                    Some(window_state) => pending_res.set(Ok(window_state.scale_factor)),
                    None => pending_res.set(Err(WindowError::Closed)),
                }
            },
            WlBackendEv::GetCurrentMonitor {
                window_id,
                pending_res,
            } => {
                let window_state = match self.window_state.get(&window_id) {
                    Some(some) => some,
                    None => {
                        pending_res.set(Err(WindowError::Closed));
                        return;
                    },
                };

                let cur_output = match window_state.cur_output_op.as_ref() {
                    Some(some) => some,
                    None => {
                        pending_res.set(Err(WindowError::Other(String::from(
                            "surface hasn't been entered.",
                        ))));
                        return;
                    },
                };

                pending_res.set(match self.output_to_monitor(cur_output, true) {
                    Some(monitor) => Ok(monitor),
                    None => Err(WindowError::Other(String::from("output no longer exists."))),
                });
            },
            WlBackendEv::IsFullscreen {
                window_id,
                pending_res,
            } => {
                let window_state = match self.window_state.get(&window_id) {
                    Some(some) => some,
                    None => {
                        pending_res.set(Err(WindowError::Closed));
                        return;
                    },
                };

                match window_state.last_configure.as_ref() {
                    Some(last_configure) => {
                        pending_res.set(Ok(last_configure
                            .state
                            .contains(wl::WindowState::FULLSCREEN)));
                    },
                    None => {
                        pending_res.set(Err(WindowError::NotReady));
                    },
                }
            },
            WlBackendEv::IsFullscreenSupported {
                window_id,
                pending_res,
            } => {
                let window_state = match self.window_state.get(&window_id) {
                    Some(some) => some,
                    None => {
                        pending_res.set(Err(WindowError::Closed));
                        return;
                    },
                };

                match window_state.last_configure.as_ref() {
                    Some(_last_configure) => {
                        // TODO: capabilities seems to be bogus? sway reports maximize support, but
                        //       not fullscreen, but fullscreen works and maximize doesn't?

                        pending_res.set(Ok(true));

                        /*pending_res.set(Ok(last_configure
                        .capabilities
                        .contains(wl::WindowManagerCapabilities::FULLSCREEN)));*/
                    },
                    None => {
                        pending_res.set(Err(WindowError::NotReady));
                    },
                }
            },
            WlBackendEv::CheckOutputExists {
                output,
                pending_res,
            } => {
                pending_res.set(self.output_state.info(&output).is_some());
            },
            WlBackendEv::Exit => {
                self.loop_signal.stop();
            },
        }
    }

    fn output_to_monitor(&self, wl_output: &wl::WlOutput, is_current: bool) -> Option<Monitor> {
        let info = match self.output_state.info(&wl_output) {
            Some(some) => some,
            None => return None,
        };

        let mut monitor = Monitor {
            name: info.name.unwrap_or_else(String::new),
            resolution: [0; 2],
            position: [info.location.0, info.location.1],
            refresh_rate: 0.0.into(),
            bit_depth: 32, // Note: Not Supported
            is_current,
            is_primary: false, // Note: Not Supported
            modes: Vec::with_capacity(info.modes.len()),
            handle: MonitorHandle::Wayland(wl_output.clone()),
        };

        for mode in info.modes.iter() {
            if mode.current {
                monitor.resolution = [
                    mode.dimensions.0.try_into().unwrap_or(0),
                    mode.dimensions.1.try_into().unwrap_or(0),
                ];

                monitor.refresh_rate = OrderedFloat(mode.refresh_rate as f32 / 1000.0);
            }

            monitor.modes.push(MonitorMode {
                resolution: [
                    mode.dimensions.0.try_into().unwrap_or(0),
                    mode.dimensions.1.try_into().unwrap_or(0),
                ],
                bit_depth: 32, // Note: Not Supported
                refresh_rate: OrderedFloat(mode.refresh_rate as f32 / 1000.0),
                handle: MonitorModeHandle::Wayland,
                monitor_handle: monitor.handle.clone(),
            });
        }

        Some(monitor)
    }
}

impl wl::ProvidesRegistryState for WlBackendState {
    wl::registry_handlers![wl::OutputState, wl::SeatState];

    fn registry(&mut self) -> &mut wl::RegistryState {
        &mut self.registry_state
    }
}

impl wl::CompositorHandler for WlBackendState {
    fn scale_factor_changed(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        surface: &wl::WlSurface,
        scale_factor: i32,
    ) {
        if let Some(window_id) = self.surface_to_id.get(surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            window_state.scale_factor = scale_factor as f32;
            window_state.window.set_dpi_scale(window_state.scale_factor);
        }
    }

    fn transform_changed(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::WlSurface,
        _: wl::Transform,
    ) {
    }

    fn frame(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: &wl::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        surface: &wl::WlSurface,
        output: &wl::WlOutput,
    ) {
        if let Some(window_id) = self.surface_to_id.get(surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            window_state.cur_output_op = Some(output.clone());
        }
    }

    fn surface_leave(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::WlSurface,
        _: &wl::WlOutput,
    ) {
    }
}

impl wl::OutputHandler for WlBackendState {
    fn output_state(&mut self) -> &mut wl::OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::WlOutput) {
        // Note: output_state tracks outputs
    }

    fn update_output(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::WlOutput) {
        // Note: output_state tracks outputs
    }

    fn output_destroyed(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::WlOutput) {
        // Note: output_state tracks outputs
    }
}

impl wl::WindowHandler for WlBackendState {
    fn request_close(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        xdg_window: &wl::Window,
    ) {
        if let Some(window_id) = self.surface_to_id.get(xdg_window.wl_surface())
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            let _ = window_state.window.close();
        }
    }

    fn configure(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        xdg_window: &wl::Window,
        configure: wl::WindowConfigure,
        _: u32,
    ) {
        if let Some(window_state) = self
            .surface_to_id
            .get(xdg_window.wl_surface())
            .and_then(|window_id| self.window_state.get_mut(window_id))
        {
            let new_width = match configure.new_size.0 {
                Some(width_nz) => width_nz.get(),
                None => window_state.inner_size[0],
            };

            let new_height = match configure.new_size.1 {
                Some(height_nz) => height_nz.get(),
                None => window_state.inner_size[1],
            };

            let resized =
                new_width != window_state.inner_size[0] || new_height != window_state.inner_size[1];

            window_state.inner_size = [new_width, new_height];

            match window_state.create_pending_res.take() {
                Some(pending_res) => {
                    // This is the first configure, finish window creation.
                    window_state
                        .window
                        .basalt_ref()
                        .window_manager_ref()
                        .window_created(window_state.window.clone());
                    pending_res.set(Ok(window_state.window.clone()));
                    window_state.is_ready.store(true, atomic::Ordering::SeqCst);
                },
                None => {
                    if resized {
                        window_state.window.send_event(WindowEvent::Resized {
                            width: new_width,
                            height: new_height,
                        });
                    } else {
                        // Note: Probably not a bad idea to force a redraw after a configure.
                        window_state.window.send_event(WindowEvent::RedrawRequested);
                    }
                },
            }

            window_state.last_configure = Some(configure);
        }
    }
}

impl wl::LayerShellHandler for WlBackendState {
    fn closed(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        layer_surface: &wl::LayerSurface,
    ) {
        if let Some(window_id) = self.surface_to_id.get(layer_surface.wl_surface())
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            let _ = window_state.window.close();
        }
    }

    fn configure(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        layer_surface: &wl::LayerSurface,
        configure: wl::LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(window_state) = self
            .surface_to_id
            .get(layer_surface.wl_surface())
            .and_then(|window_id| self.window_state.get_mut(window_id))
        {
            let new_width = if configure.new_size.0 == 0 {
                window_state.inner_size[0]
            } else {
                configure.new_size.0
            };

            let new_height = if configure.new_size.1 == 0 {
                window_state.inner_size[1]
            } else {
                configure.new_size.1
            };

            let resized =
                new_width != window_state.inner_size[0] || new_height != window_state.inner_size[1];

            window_state.inner_size = [new_width, new_height];

            match window_state.create_pending_res.take() {
                Some(pending_res) => {
                    // This is the first configure, finish window creation.
                    window_state
                        .window
                        .basalt_ref()
                        .window_manager_ref()
                        .window_created(window_state.window.clone());
                    pending_res.set(Ok(window_state.window.clone()));
                    window_state.is_ready.store(true, atomic::Ordering::SeqCst);
                },
                None => {
                    if resized {
                        window_state.window.send_event(WindowEvent::Resized {
                            width: new_width,
                            height: new_height,
                        });
                    } else {
                        // Note: Probably not a bad idea to force a redraw after a configure.
                        window_state.window.send_event(WindowEvent::RedrawRequested);
                    }
                },
            }
        }
    }
}

impl wl::SeatHandler for WlBackendState {
    fn seat_state(&mut self) -> &mut wl::SeatState {
        &mut self.seat_state
    }

    fn new_capability(
        &mut self,
        _: &wl::Connection,
        queue_handle: &wl::QueueHandle<Self>,
        seat: wl::WlSeat,
        capability: wl::Capability,
    ) {
        if capability == wl::Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(queue_handle, &seat, None).ok();
        }

        if capability == wl::Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(queue_handle, &seat).ok();
        }
    }

    fn remove_capability(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: wl::WlSeat,
        capability: wl::Capability,
    ) {
        if capability == wl::Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }

        if capability == wl::Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }
    }

    fn new_seat(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::WlSeat) {}

    fn remove_seat(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::WlSeat) {}
}

impl wl::KeyboardHandler for WlBackendState {
    fn enter(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::WlKeyboard,
        surface: &wl::WlSurface,
        _: u32,
        _: &[u32],
        _: &[wl::Keysym],
    ) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.surface_to_id.get(surface)
        {
            basalt.input_ref().send_event(InputEvent::Focus {
                win: *window_id,
            });

            self.focus_window_id = Some(*window_id);
        }
    }

    fn leave(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::WlKeyboard,
        surface: &wl::WlSurface,
        _: u32,
    ) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.surface_to_id.get(surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            for qwerty in window_state.keys_pressed.drain() {
                basalt.input_ref().send_event(InputEvent::Release {
                    win: *window_id,
                    key: qwerty.into(),
                });
            }

            basalt.input_ref().send_event(InputEvent::FocusLost {
                win: *window_id,
            });

            self.focus_window_id = None;
        }
    }

    fn press_key(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::WlKeyboard,
        _: u32,
        event: wl::KeyEvent,
    ) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            if let Some(qwerty) = raw_code_to_qwerty(event.raw_code) {
                window_state.keys_pressed.insert(qwerty);

                basalt.input_ref().send_event(InputEvent::Press {
                    win: *window_id,
                    key: qwerty.into(),
                });
            }

            if let Some(utf8) = event.utf8 {
                for c in utf8.chars() {
                    basalt.input_ref().send_event(InputEvent::Character {
                        win: *window_id,
                        c,
                    });
                }
            }
        }
    }

    fn repeat_key(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::WlKeyboard,
        _: u32,
        event: wl::KeyEvent,
    ) {
        // TODO: Not being emitted for some reason???

        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
            && let Some(utf8) = event.utf8
        {
            for c in utf8.chars() {
                basalt.input_ref().send_event(InputEvent::Character {
                    win: *window_id,
                    c,
                });
            }
        }
    }

    fn release_key(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::WlKeyboard,
        _: u32,
        event: wl::KeyEvent,
    ) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
            && let Some(window_state) = self.window_state.get_mut(window_id)
            && let Some(qwerty) = raw_code_to_qwerty(event.raw_code)
            && window_state.keys_pressed.remove(&qwerty)
        {
            basalt.input_ref().send_event(InputEvent::Release {
                win: *window_id,
                key: qwerty.into(),
            });
        }
    }

    fn update_modifiers(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::WlKeyboard,
        _: u32,
        _: wl::Modifiers,
        _: wl::RawModifiers,
        _: u32,
    ) {
    }
}

impl wl::PointerHandler for WlBackendState {
    fn pointer_frame(
        &mut self,
        _conn: &wl::Connection,
        _qh: &wl::QueueHandle<Self>,
        _pointer: &wl::WlPointer,
        events: &[wl::PointerEvent],
    ) {
        if let Some(basalt) = self.basalt_op.as_ref() {
            for event in events {
                if let Some(window_id) = self.surface_to_id.get(&event.surface) {
                    match event.kind {
                        wl::PointerEventKind::Enter {
                            ..
                        } => {
                            basalt.input_ref().send_event(InputEvent::Enter {
                                win: *window_id,
                            });
                        },
                        wl::PointerEventKind::Leave {
                            ..
                        } => {
                            basalt.input_ref().send_event(InputEvent::Leave {
                                win: *window_id,
                            });
                        },
                        wl::PointerEventKind::Motion {
                            ..
                        } => {
                            basalt.input_ref().send_event(InputEvent::Cursor {
                                win: *window_id,
                                x: event.position.0 as f32,
                                y: event.position.1 as f32,
                            });
                        },
                        wl::PointerEventKind::Press {
                            button, ..
                        } => {
                            let button = match button {
                                272 => MouseButton::Left,
                                273 => MouseButton::Right,
                                274 => MouseButton::Middle,
                                _ => return,
                            };

                            basalt.input_ref().send_event(InputEvent::Press {
                                win: *window_id,
                                key: button.into(),
                            });
                        },
                        wl::PointerEventKind::Release {
                            button, ..
                        } => {
                            let button = match button {
                                272 => MouseButton::Left,
                                273 => MouseButton::Right,
                                274 => MouseButton::Middle,
                                _ => return,
                            };

                            basalt.input_ref().send_event(InputEvent::Release {
                                win: *window_id,
                                key: button.into(),
                            });
                        },
                        wl::PointerEventKind::Axis {
                            horizontal,
                            vertical,
                            ..
                        } => {
                            basalt.input_ref().send_event(InputEvent::Scroll {
                                win: *window_id,
                                v: vertical.value120 as f32 / 120.0,
                                h: horizontal.value120 as f32 / 120.0,
                            });
                        },
                    }
                }
            }
        }
    }
}

fn raw_code_to_qwerty(raw_code: u32) -> Option<Qwerty> {
    Some(match raw_code {
        1 => Qwerty::Esc,
        59 => Qwerty::F1,
        60 => Qwerty::F2,
        61 => Qwerty::F3,
        62 => Qwerty::F4,
        63 => Qwerty::F5,
        64 => Qwerty::F6,
        65 => Qwerty::F7,
        66 => Qwerty::F8,
        67 => Qwerty::F9,
        68 => Qwerty::F10,
        87 => Qwerty::F11,
        88 => Qwerty::F12,
        41 => Qwerty::Tilda,
        2 => Qwerty::One,
        3 => Qwerty::Two,
        4 => Qwerty::Three,
        5 => Qwerty::Four,
        6 => Qwerty::Five,
        7 => Qwerty::Six,
        8 => Qwerty::Seven,
        9 => Qwerty::Eight,
        10 => Qwerty::Nine,
        11 => Qwerty::Zero,
        12 => Qwerty::Dash,
        13 => Qwerty::Equal,
        14 => Qwerty::Backspace,
        15 => Qwerty::Tab,
        16 => Qwerty::Q,
        17 => Qwerty::W,
        18 => Qwerty::E,
        19 => Qwerty::R,
        20 => Qwerty::T,
        21 => Qwerty::Y,
        22 => Qwerty::U,
        23 => Qwerty::I,
        24 => Qwerty::O,
        25 => Qwerty::P,
        26 => Qwerty::LSqBracket,
        27 => Qwerty::RSqBracket,
        43 => Qwerty::Backslash,
        58 => Qwerty::Caps,
        30 => Qwerty::A,
        31 => Qwerty::S,
        32 => Qwerty::D,
        33 => Qwerty::F,
        34 => Qwerty::G,
        35 => Qwerty::H,
        36 => Qwerty::J,
        37 => Qwerty::K,
        38 => Qwerty::L,
        39 => Qwerty::SemiColon,
        40 => Qwerty::Parenthesis,
        28 => Qwerty::Enter,
        42 => Qwerty::LShift,
        44 => Qwerty::Z,
        45 => Qwerty::X,
        46 => Qwerty::C,
        47 => Qwerty::V,
        48 => Qwerty::B,
        49 => Qwerty::N,
        50 => Qwerty::M,
        51 => Qwerty::Comma,
        52 => Qwerty::Period,
        53 => Qwerty::Slash,
        // ??? => Qwerty::RShift,
        29 => Qwerty::LCtrl,
        // ??? => Qwerty::LSuper,
        56 => Qwerty::LAlt,
        57 => Qwerty::Space,
        100 => Qwerty::RAlt,
        // ??? => Qwerty::RSuper,
        // ??? => Qwerty::RCtrl,
        99 => Qwerty::PrintScreen,
        70 => Qwerty::ScrollLock,
        // ??? => Qwerty::Pause,
        110 => Qwerty::Insert,
        102 => Qwerty::Home,
        // ??? => Qwerty::PageUp,
        111 => Qwerty::Delete,
        107 => Qwerty::End,
        // ??? => Qwerty::PageDown,
        103 => Qwerty::ArrowUp,
        108 => Qwerty::ArrowDown,
        105 => Qwerty::ArrowLeft,
        106 => Qwerty::ArrowRight,
        113 => Qwerty::TrackMute,
        114 => Qwerty::TrackVolDown,
        115 => Qwerty::TrackVolUp,
        // ??? => Qwerty::TrackPlayPause,
        // ??? => Qwerty::TrackBack,
        // ??? => Qwerty::TrackNext,
        _ => return None,
    })
}
