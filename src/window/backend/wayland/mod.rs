mod convert;
mod handles;
mod wl_handlers;

use std::sync::Arc;

use foldhash::{HashMap, HashSet, HashSetExt};
use smithay_client_toolkit::shell::WaylandSurface;

use self::convert::{raw_code_to_qwerty, wl_button_to_mouse_button, wl_output_to_monitor};
use self::handles::{BackendEvent, WindowRequest};
pub use self::handles::{WlBackendHandle, WlWindowHandle};
use crate::Basalt;
use crate::input::{InputEvent, Qwerty};
use crate::window::backend::PendingRes;
use crate::window::builder::WindowAttributes;
use crate::window::monitor::MonitorHandle;
use crate::window::{
    EnableFullScreenError, FullScreenBehavior, Monitor, Window, WindowError, WindowEvent, WindowID,
};

mod wl {
    pub use smithay_client_toolkit::compositor::CompositorState;
    pub use smithay_client_toolkit::output::OutputState;
    pub use smithay_client_toolkit::reexports::client::globals::GlobalList;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard as Keyboard;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput as Output;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat as Seat;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface as Surface;
    pub use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
    pub use smithay_client_toolkit::reexports::csd_frame::WindowState;
    pub use smithay_client_toolkit::registry::RegistryState;
    pub use smithay_client_toolkit::seat::keyboard::KeyEvent;
    pub use smithay_client_toolkit::seat::pointer::{
        CursorIcon, PointerData, PointerEvent, PointerEventKind, ThemeSpec, ThemedPointer,
    };
    pub use smithay_client_toolkit::seat::{Capability, SeatState};
    pub use smithay_client_toolkit::shell::wlr_layer::{
        Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface, LayerSurfaceConfigure,
    };
    pub use smithay_client_toolkit::shell::xdg::XdgShell;
    pub use smithay_client_toolkit::shell::xdg::window::{
        DecorationMode, Window, WindowConfigure, WindowDecorations,
    };
    pub use smithay_client_toolkit::shm::Shm;
}

mod cl {
    pub use smithay_client_toolkit::reexports::calloop::LoopSignal;
    pub use smithay_client_toolkit::reexports::calloop::channel::Sender;
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
    pub output_op: Option<wl::Output>,
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

struct WindowState {
    window: Arc<Window>,
    surface: SurfaceBacking,

    inner_size: [u32; 2],
    scale_factor: f32,
    keys_pressed: HashSet<Qwerty>,

    cur_output_op: Option<wl::Output>,
    last_configure: Option<wl::WindowConfigure>,

    create_pending_res: Option<PendingRes<Result<Arc<Window>, WindowError>>>,
}

#[derive(Clone)]
enum SurfaceBacking {
    Layer(wl::LayerSurface),
    Window(wl::Window),
}

impl WaylandSurface for SurfaceBacking {
    fn wl_surface(&self) -> &wl::Surface {
        match self {
            Self::Layer(layer) => layer.wl_surface(),
            Self::Window(window) => window.wl_surface(),
        }
    }
}

struct BackendState {
    basalt_op: Option<Arc<Basalt>>,

    window_state: HashMap<WindowID, WindowState>,
    surface_to_id: HashMap<wl::Surface, WindowID>,
    id_to_surface: HashMap<WindowID, wl::Surface>,

    loop_signal: cl::LoopSignal,
    event_send: cl::Sender<BackendEvent>,

    connection: wl::Connection,
    queue_handle: wl::QueueHandle<Self>,
    global_list: wl::GlobalList,

    registry_state: wl::RegistryState,
    output_state: wl::OutputState,
    seat_state: wl::SeatState,
    compositor_state: wl::CompositorState,
    shm: wl::Shm,

    xdg_shell: Option<wl::XdgShell>,
    layer_shell: Option<wl::LayerShell>,

    keyboard: Option<wl::Keyboard>,
    pointer: Option<wl::ThemedPointer<wl::PointerData>>,

    focus_window_id: Option<WindowID>,
}

impl BackendState {
    #[inline(always)]
    fn get_monitors(&mut self) -> Result<Vec<Monitor>, WindowError> {
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
            if let Some(monitor) = wl_output_to_monitor(
                &self.output_state,
                &wl_output,
                cur_output_op.is_some() && *cur_output_op.as_ref().unwrap() == wl_output,
            ) {
                monitors.push(monitor);
            }
        }

        Ok(monitors)
    }

    #[inline(always)]
    fn create_window(
        &mut self,
        window_id: WindowID,
        window_attributes: WindowAttributes,
        pending_res: PendingRes<Result<Arc<Window>, WindowError>>,
    ) {
        let basalt = self
            .basalt_op
            .as_ref()
            .expect("unreachable: windows can only be created after basalt is initialized");

        let (wl_surface_backing, inner_size) = match window_attributes {
            WindowAttributes::WlLayer(attributes) => {
                if self.layer_shell.is_none() {
                    match wl::LayerShell::bind(&self.global_list, &self.queue_handle) {
                        Ok(layer_shell) => self.layer_shell = Some(layer_shell),
                        Err(_) => {
                            return pending_res.set(Err(WindowError::NotSupported));
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
                (SurfaceBacking::Layer(layer_surface), inner_size)
            },
            WindowAttributes::WlWindow(attributes) => {
                if self.xdg_shell.is_none() {
                    match wl::XdgShell::bind(&self.global_list, &self.queue_handle) {
                        Ok(xdg_shell) => self.xdg_shell = Some(xdg_shell),
                        Err(_) => {
                            return pending_res.set(Err(WindowError::NotSupported));
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

                (
                    SurfaceBacking::Window(xdg_window),
                    attributes.size.unwrap_or([854, 480]),
                )
            },
            _ => unreachable!(),
        };

        let wl_window = WlWindowHandle {
            window_id,
            is_layer: matches!(&wl_surface_backing, SurfaceBacking::Layer(_)),
            wl_display: self.connection.display(),
            wl_surface: wl_surface_backing.wl_surface().clone(),
            event_send: self.event_send.clone(),
        };

        let wl_surface = wl_surface_backing.wl_surface().clone();

        let window = match Window::new(basalt.clone(), window_id, wl_window) {
            Ok(ok) => ok,
            Err(e) => {
                return pending_res.set(Err(e));
            },
        };

        self.surface_to_id.insert(wl_surface.clone(), window_id);
        self.id_to_surface.insert(window_id, wl_surface);

        self.window_state.insert(
            window_id,
            WindowState {
                window: window.clone(),
                surface: wl_surface_backing,
                create_pending_res: Some(pending_res),
                inner_size,
                scale_factor: 1.0,
                keys_pressed: HashSet::new(),
                cur_output_op: None,
                last_configure: None,
            },
        );

        // Note: The pending_res will be set and the window manager informed after the first
        //       configure to ensure the window is ready to draw.
    }

    #[inline(always)]
    fn close_window(&mut self, window_id: WindowID) {
        if let Some(wl_surface) = self.id_to_surface.remove(&window_id) {
            self.surface_to_id.remove(&wl_surface);
        }

        self.window_state.remove(&window_id);
    }

    #[inline(always)]
    fn window_request(&mut self, window_id: WindowID, window_request: WindowRequest) {
        let window_state = match self.window_state.get_mut(&window_id) {
            Some(some) => some,
            None => {
                return window_request.set_err(WindowError::Closed);
            },
        };

        match window_request {
            WindowRequest::GetInnerSize {
                pending_res,
            } => {
                if window_state.create_pending_res.is_some() {
                    return pending_res.set(Err(WindowError::NotReady));
                }

                pending_res.set(Ok(window_state.inner_size));
            },
            WindowRequest::Resize {
                window_size,
                pending_res,
            } => {
                match &window_state.surface {
                    SurfaceBacking::Layer(wl_layer) => {
                        // Note: A configure event should follow, triggering the resize event.

                        wl_layer.set_size(window_size[0], window_size[1]);
                        pending_res.set(Ok(()));
                    },
                    SurfaceBacking::Window(_) => {
                        let last_configure = window_state
                            .last_configure
                            .as_ref()
                            .expect("unreachable: window doesn't exist until first configure");

                        // Note: If window state has tiling assume it can't be resized.

                        if last_configure.state.contains(wl::WindowState::TILED) {
                            return pending_res.set(Err(WindowError::NotSupported));
                        }

                        // Note: Resizing a window is just a matter of drawing at the new size.

                        window_state.inner_size = window_size;

                        window_state.window.send_event(WindowEvent::Resized {
                            width: window_size[0],
                            height: window_size[1],
                        });

                        pending_res.set(Ok(()));
                    },
                }
            },
            WindowRequest::GetCurrentMonitor {
                pending_res,
            } => {
                if window_state.create_pending_res.is_some() || window_state.cur_output_op.is_none()
                {
                    return pending_res.set(Err(WindowError::NotReady));
                }

                match wl_output_to_monitor(
                    &self.output_state,
                    window_state.cur_output_op.as_ref().unwrap(),
                    true,
                ) {
                    Some(monitor) => {
                        pending_res.set(Ok(monitor));
                    },
                    None => {
                        pending_res.set(Err(WindowError::Other(String::from(
                            "output doesn't exist.",
                        ))));
                    },
                }
            },
            WindowRequest::IsFullscreen {
                pending_res,
            } => {
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
            WindowRequest::EnableFullscreen {
                fullscreen_behavior,
                pending_res,
            } => {
                // TODO: This doesn't seem to be reported correctly, at least on sway.

                /*match window_state.last_configure.as_ref() {
                    Some(last_configure) => {
                        if !last_configure
                            .capabilities
                            .contains(wl::WindowManagerCapabilities::FULLSCREEN)
                        {
                            return pending_res.set(Err(WindowError::NotSupported));
                        }
                    },
                    None => {
                        return pending_res.set(Err(WindowError::NotReady));
                    },
                }*/

                // Note: This maps exclusive behaviors to borderless ones as no compositors actually
                //       support fullscreen_shell. Maybe that changes in the future?

                let output_op = match fullscreen_behavior {
                    FullScreenBehavior::AutoBorderlessPrimary
                    | FullScreenBehavior::AutoExclusivePrimary => {
                        return pending_res
                            .set(Err(EnableFullScreenError::UnableToDeterminePrimary.into()));
                    },
                    FullScreenBehavior::Auto
                    | FullScreenBehavior::AutoBorderless
                    | FullScreenBehavior::AutoExclusive => {
                        match window_state.cur_output_op.clone() {
                            Some(cur_output) => Some(cur_output),
                            None => self.output_state.outputs().next(),
                        }
                    },
                    FullScreenBehavior::AutoBorderlessCurrent
                    | FullScreenBehavior::AutoExclusiveCurrent => {
                        match window_state.cur_output_op.clone() {
                            Some(some) => Some(some),
                            None => {
                                return pending_res.set(Err(
                                    EnableFullScreenError::UnableToDetermineCurrent.into(),
                                ));
                            },
                        }
                    },
                    FullScreenBehavior::Borderless(monitor)
                    | FullScreenBehavior::ExclusiveAutoMode(monitor)
                    | FullScreenBehavior::Exclusive(monitor, _) => {
                        let user_output = match monitor.handle {
                            MonitorHandle::Wayland(output) => output,
                            _ => unreachable!(),
                        };

                        // Note: Since this is a user provided make sure it still exists.

                        if self.output_state.info(&user_output).is_none() {
                            return pending_res
                                .set(Err(EnableFullScreenError::MonitorDoesNotExist.into()));
                        }

                        Some(user_output)
                    },
                };

                match &window_state.surface {
                    SurfaceBacking::Window(xdg_window) => {
                        xdg_window.set_fullscreen(output_op.as_ref());
                        pending_res.set(Ok(()));
                    },
                    SurfaceBacking::Layer(_) => unreachable!(),
                }
            },
            WindowRequest::DisableFullscreen {
                pending_res,
            } => {
                match &window_state.surface {
                    SurfaceBacking::Window(xdg_window) => {
                        xdg_window.unset_fullscreen();
                        pending_res.set(Ok(()));
                    },
                    SurfaceBacking::Layer(_) => unreachable!(),
                }
            },
            WindowRequest::CaptureCursor {
                pending_res,
            } => {
                // TODO:
                pending_res.set(Err(WindowError::NotImplemented));
            },
            WindowRequest::ReleaseCursor {
                pending_res,
            } => {
                // TODO:
                pending_res.set(Err(WindowError::NotImplemented));
            },
            WindowRequest::IsCursorCaptured {
                pending_res,
            } => {
                // TODO:
                pending_res.set(Err(WindowError::NotImplemented));
            },
        }
    }

    #[inline(always)]
    fn surface_scale_change(&mut self, wl_surface: &wl::Surface, scale_factor: i32) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            window_state.scale_factor = scale_factor as f32;
            window_state.window.set_dpi_scale(window_state.scale_factor);
        }
    }

    #[inline(always)]
    fn surface_enter(&mut self, wl_surface: &wl::Surface, wl_output: &wl::Output) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            window_state.cur_output_op = Some(wl_output.clone());
        }
    }

    #[inline(always)]
    fn window_configure(&mut self, wl_surface: &wl::Surface, wl_configure: wl::WindowConfigure) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            let new_width = match wl_configure.new_size.0 {
                Some(width_nz) => width_nz.get(),
                None => window_state.inner_size[0],
            };

            let new_height = match wl_configure.new_size.1 {
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

            window_state.last_configure = Some(wl_configure);
        }
    }

    #[inline(always)]
    fn layer_configure(
        &mut self,
        wl_surface: &wl::Surface,
        wl_configure: wl::LayerSurfaceConfigure,
    ) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            let new_width = if wl_configure.new_size.0 == 0 {
                window_state.inner_size[0]
            } else {
                wl_configure.new_size.0
            };

            let new_height = if wl_configure.new_size.1 == 0 {
                window_state.inner_size[1]
            } else {
                wl_configure.new_size.1
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

    #[inline(always)]
    fn window_close_request(&mut self, wl_surface: &wl::Surface) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            let _ = window_state.window.close();
        }
    }

    #[inline(always)]
    fn layer_close(&mut self, wl_surface: &wl::Surface) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            let _ = window_state.window.close();
        }
    }

    #[inline(always)]
    fn seat_new_capability(&mut self, wl_seat: wl::Seat, wl_capability: wl::Capability) {
        if wl_capability == wl::Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self
                .seat_state
                .get_keyboard(&self.queue_handle, &wl_seat, None)
                .ok();
        }

        if wl_capability == wl::Capability::Pointer
            && self.pointer.is_none()
            && let Ok(themed_pointer) = self.seat_state.get_pointer_with_theme(
                &self.queue_handle,
                &wl_seat,
                self.shm.wl_shm(),
                self.compositor_state.create_surface(&self.queue_handle),
                wl::ThemeSpec::System,
            )
        {
            self.pointer = Some(themed_pointer);
        }
    }

    #[inline(always)]
    fn seat_remove_capability(&mut self, _wl_seat: wl::Seat, wl_capability: wl::Capability) {
        if wl_capability == wl::Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }

        if wl_capability == wl::Capability::Pointer
            && let Some(themed_pointer) = self.pointer.take()
        {
            themed_pointer.pointer().release();
        }
    }

    #[inline(always)]
    fn keyboard_enter(&mut self, wl_surface: &wl::Surface) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.surface_to_id.get(wl_surface)
        {
            basalt.input_ref().send_event(InputEvent::Focus {
                win: *window_id,
            });

            self.focus_window_id = Some(*window_id);
        }
    }

    #[inline(always)]
    fn keyboard_leave(&mut self, wl_surface: &wl::Surface) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.surface_to_id.get(wl_surface)
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

    #[inline(always)]
    fn keyboard_press(&mut self, wl_event: wl::KeyEvent) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            if let Some(qwerty) = raw_code_to_qwerty(wl_event.raw_code) {
                window_state.keys_pressed.insert(qwerty);

                basalt.input_ref().send_event(InputEvent::Press {
                    win: *window_id,
                    key: qwerty.into(),
                });
            }

            if let Some(utf8) = wl_event.utf8 {
                for c in utf8.chars() {
                    basalt.input_ref().send_event(InputEvent::Character {
                        win: *window_id,
                        c,
                    });
                }
            }
        }
    }

    #[inline(always)]
    fn keyboard_repeat(&mut self, wl_event: wl::KeyEvent) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
            && let Some(utf8) = wl_event.utf8
        {
            for c in utf8.chars() {
                basalt.input_ref().send_event(InputEvent::Character {
                    win: *window_id,
                    c,
                });
            }
        }
    }

    #[inline(always)]
    fn keyboard_release(&mut self, wl_event: wl::KeyEvent) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
            && let Some(window_state) = self.window_state.get_mut(window_id)
            && let Some(qwerty) = raw_code_to_qwerty(wl_event.raw_code)
            && window_state.keys_pressed.remove(&qwerty)
        {
            basalt.input_ref().send_event(InputEvent::Release {
                win: *window_id,
                key: qwerty.into(),
            });
        }
    }

    #[inline(always)]
    fn pointer_frame(&mut self, wl_pointer_events: &[wl::PointerEvent]) {
        let basalt = match self.basalt_op.as_ref() {
            Some(some) => some,
            None => return,
        };

        for wl_pointer_event in wl_pointer_events {
            if let Some(window_id) = self.surface_to_id.get(&wl_pointer_event.surface) {
                match wl_pointer_event.kind {
                    wl::PointerEventKind::Enter {
                        ..
                    } => {
                        basalt.input_ref().send_event(InputEvent::Enter {
                            win: *window_id,
                        });

                        // TODO: This assumes this pointer frame is associated with the pointer
                        // that is stored in BackendState.

                        if let Some(themed_pointer) = self.pointer.as_ref() {
                            // TODO: Hide cursor

                            let _ = themed_pointer
                                .set_cursor(&self.connection, wl::CursorIcon::Default);
                        }
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
                            x: wl_pointer_event.position.0 as f32,
                            y: wl_pointer_event.position.1 as f32,
                        });
                    },
                    wl::PointerEventKind::Press {
                        button: wl_button, ..
                    } => {
                        let button = match wl_button_to_mouse_button(wl_button) {
                            Some(some) => some,
                            None => return,
                        };

                        basalt.input_ref().send_event(InputEvent::Press {
                            win: *window_id,
                            key: button.into(),
                        });
                    },
                    wl::PointerEventKind::Release {
                        button: wl_button, ..
                    } => {
                        let button = match wl_button_to_mouse_button(wl_button) {
                            Some(some) => some,
                            None => return,
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
