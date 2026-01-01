mod convert;
mod handles;
mod wl_handlers;

use std::sync::{Arc, Weak};

use foldhash::{HashMap, HashMapExt, HashSet, HashSetExt};
use smithay_client_toolkit::reexports::client::Proxy;
use smithay_client_toolkit::shell::WaylandSurface;

use self::convert::{
    cursor_icon_to_wl, raw_code_to_qwerty, wl_button_to_mouse_button, wl_output_to_monitor,
};
use self::handles::{BackendEvent, WindowRequest};
pub use self::handles::{WlBackendHandle, WlLayerHandle, WlWindowHandle};
use crate::Basalt;
use crate::input::{InputEvent, Qwerty};
use crate::window::backend::PendingRes;
use crate::window::builder::WindowAttributes;
use crate::window::monitor::MonitorHandle;
use crate::window::{
    CursorIcon, EnableFullScreenError, FullScreenBehavior, Monitor, Window, WindowError,
    WindowEvent, WindowID, WlLayerAnchor, WlLayerDepth, WlLayerKeyboardFocus,
};

mod wl {
    pub use smithay_client_toolkit::compositor::CompositorState;
    pub use smithay_client_toolkit::output::OutputState;
    pub use smithay_client_toolkit::reexports::client::globals::GlobalList;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard as Keyboard;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput as Output;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_pointer::WlPointer as Pointer;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat as Seat;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface as Surface;
    pub use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
    pub use smithay_client_toolkit::reexports::csd_frame::WindowState;
    pub use smithay_client_toolkit::registry::RegistryState;
    pub use smithay_client_toolkit::seat::keyboard::KeyEvent;
    pub use smithay_client_toolkit::seat::pointer::{
        PointerData, PointerEvent, PointerEventKind, ThemeSpec, ThemedPointer,
    };
    pub use smithay_client_toolkit::seat::{Capability, SeatState};
    pub use smithay_client_toolkit::shell::wlr_layer::{
        Layer, LayerShell, LayerSurface, LayerSurfaceConfigure,
    };
    pub use smithay_client_toolkit::shell::xdg::XdgShell;
    pub use smithay_client_toolkit::shell::xdg::window::{
        Window, WindowConfigure, WindowDecorations,
    };
    pub use smithay_client_toolkit::shm::Shm;
    pub use smithay_client_toolkit::seat::pointer_constraints::PointerConstraintsState;
    pub use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::ZwpLockedPointerV1;
    pub use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::zwp_confined_pointer_v1::ZwpConfinedPointerV1;
    pub use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::Lifetime as PtrConstrLifetime;
    pub use smithay_client_toolkit::reexports::protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1;
    pub use smithay_client_toolkit::seat::relative_pointer::RelativeMotionEvent;
    pub use smithay_client_toolkit::seat::relative_pointer::RelativePointerState;
}

mod cl {
    pub use smithay_client_toolkit::reexports::calloop::channel::Sender;
    pub use smithay_client_toolkit::reexports::calloop::{LoopHandle, LoopSignal};
}

#[derive(Debug)]
pub struct WlLayerAttributes {
    pub namespace_op: Option<String>,
    pub size_op: Option<[u32; 2]>,
    pub anchor: WlLayerAnchor,
    pub exclusive_zone: i32,
    pub margin_t: i32,
    pub margin_b: i32,
    pub margin_l: i32,
    pub margin_r: i32,
    pub depth: WlLayerDepth,
    pub keyboard_focus: WlLayerKeyboardFocus,
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
    window_wk: Weak<Window>,
    surface: SurfaceBacking,
    inner_size: [u32; 2],
    scale_factor: f32,
    cached_attributes: WindowCachedAttributes,
    pointer_state: WindowPointerState,
    keyboard_state: WindowKeyboardState,
    cur_output_op: Option<wl::Output>,
    last_configure: Option<wl::WindowConfigure>,
    create_pending: Option<(Arc<Window>, PendingRes<Result<Arc<Window>, WindowError>>)>,
}

enum WindowCachedAttributes {
    Window {
        title_op: Option<String>,
        min_size_op: Option<[u32; 2]>,
        max_size_op: Option<[u32; 2]>,
    },
    Layer {
        anchor: WlLayerAnchor,
        exclusive_zone: i32,
        margin_tblr: [i32; 4],
        keyboard_focus: WlLayerKeyboardFocus,
        depth: WlLayerDepth,
    },
}

struct WindowPointerState {
    visible: bool,
    locked: bool,
    confined: bool,
    cursor_icon: CursorIcon,
    active_pointers: HashMap<wl::Pointer, WindowActivePointer>,
}

struct WindowActivePointer {
    locked_op: Option<wl::ZwpLockedPointerV1>,
    confined_op: Option<wl::ZwpConfinedPointerV1>,
}

struct WindowKeyboardState {
    pressed: HashSet<Qwerty>,
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
    focus_window_id: Option<WindowID>,
    seat_state: HashMap<wl::Seat, BackendSeatState>,

    loop_signal: cl::LoopSignal,
    loop_handle: cl::LoopHandle<'static, Self>,
    event_send: cl::Sender<BackendEvent>,

    wl_connection: wl::Connection,
    wl_queue_handle: wl::QueueHandle<Self>,
    wl_global_list: wl::GlobalList,
    wl_registry_state: wl::RegistryState,
    wl_output_state: wl::OutputState,
    wl_seat_state: wl::SeatState,
    wl_compositor_state: wl::CompositorState,
    wl_ptr_constrs_state: wl::PointerConstraintsState,
    wl_relative_ptr_state: wl::RelativePointerState,
    wl_shm: wl::Shm,
    wl_xdg_shell_op: Option<wl::XdgShell>,
    wl_layer_shell_op: Option<wl::LayerShell>,
}

struct BackendSeatState {
    wl_keyboard_op: Option<wl::Keyboard>,
    wl_pointer_op: Option<wl::ThemedPointer<wl::PointerData>>,
    wl_relative_ptr_op: Option<wl::ZwpRelativePointerV1>,
}

impl BackendState {
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

        for wl_output in self.wl_output_state.outputs() {
            if let Some(monitor) = wl_output_to_monitor(
                &self.wl_output_state,
                &wl_output,
                cur_output_op.is_some() && *cur_output_op.as_ref().unwrap() == wl_output,
            ) {
                monitors.push(monitor);
            }
        }

        Ok(monitors)
    }

    fn create_window(
        &mut self,
        window_id: WindowID,
        window_attributes: WindowAttributes,
        pending_res: PendingRes<Result<Arc<Window>, WindowError>>,
    ) {
        let basalt = self
            .basalt_op
            .as_ref()
            .expect("unreachable: windows can only exist after basalt's creation");

        let (wl_surface_backing, inner_size, cached_attributes) = match window_attributes {
            WindowAttributes::WlLayer(attributes) => {
                if self.wl_layer_shell_op.is_none() {
                    match wl::LayerShell::bind(&self.wl_global_list, &self.wl_queue_handle) {
                        Ok(wl_layer_shell) => self.wl_layer_shell_op = Some(wl_layer_shell),
                        Err(_) => {
                            return pending_res.set(Err(WindowError::NotSupported));
                        },
                    }
                }

                let wl_layer_shell = self.wl_layer_shell_op.as_ref().unwrap();

                let wl_surface = self
                    .wl_compositor_state
                    .create_surface(&self.wl_queue_handle);

                let wl_layer_surface = wl_layer_shell.create_layer_surface(
                    &self.wl_queue_handle,
                    wl_surface,
                    wl::Layer::Top,
                    attributes.namespace_op,
                    attributes.output_op.as_ref(),
                );

                if let Some([width, height]) = attributes.size_op {
                    wl_layer_surface.set_size(width, height);
                }

                wl_layer_surface.set_margin(
                    attributes.margin_t,
                    attributes.margin_r,
                    attributes.margin_b,
                    attributes.margin_l,
                );

                wl_layer_surface.set_anchor(attributes.anchor.as_wl());
                wl_layer_surface.set_exclusive_zone(attributes.exclusive_zone);
                wl_layer_surface.set_layer(attributes.depth.as_wl());
                wl_layer_surface.set_keyboard_interactivity(attributes.keyboard_focus.as_wl());
                wl_layer_surface.commit();

                let cached_attributes = WindowCachedAttributes::Layer {
                    anchor: attributes.anchor,
                    exclusive_zone: attributes.exclusive_zone,
                    margin_tblr: [
                        attributes.margin_t,
                        attributes.margin_b,
                        attributes.margin_l,
                        attributes.margin_r,
                    ],
                    keyboard_focus: attributes.keyboard_focus,
                    depth: attributes.depth,
                };

                let inner_size = attributes.size_op.unwrap_or([0; 2]);
                (
                    SurfaceBacking::Layer(wl_layer_surface),
                    inner_size,
                    cached_attributes,
                )
            },
            WindowAttributes::WlWindow(attributes) => {
                if self.wl_xdg_shell_op.is_none() {
                    match wl::XdgShell::bind(&self.wl_global_list, &self.wl_queue_handle) {
                        Ok(wl_xdg_shell) => self.wl_xdg_shell_op = Some(wl_xdg_shell),
                        Err(_) => {
                            return pending_res.set(Err(WindowError::NotSupported));
                        },
                    }
                }

                let wl_xdg_shell = self.wl_xdg_shell_op.as_ref().unwrap();

                let wl_surface = self
                    .wl_compositor_state
                    .create_surface(&self.wl_queue_handle);

                let wl_xdg_window = wl_xdg_shell.create_window(
                    wl_surface,
                    if attributes.decorations {
                        wl::WindowDecorations::RequestServer
                    } else {
                        wl::WindowDecorations::RequestClient
                    },
                    &self.wl_queue_handle,
                );

                if let Some(ref title) = attributes.title {
                    wl_xdg_window.set_title(title.clone());
                }

                if let Some(min_size) = attributes.min_size {
                    wl_xdg_window.set_min_size(Some((min_size[0], min_size[1])));
                }

                if let Some(max_size) = attributes.max_size {
                    wl_xdg_window.set_max_size(Some((max_size[0], max_size[1])));
                }

                if attributes.minimized {
                    wl_xdg_window.set_minimized();
                }

                if attributes.maximized {
                    wl_xdg_window.set_maximized();
                }

                wl_xdg_window.commit();

                let cached_attributes = WindowCachedAttributes::Window {
                    title_op: attributes.title,
                    min_size_op: attributes.min_size,
                    max_size_op: attributes.max_size,
                };

                (
                    SurfaceBacking::Window(wl_xdg_window),
                    attributes.size.unwrap_or([854, 480]),
                    cached_attributes,
                )
            },
            _ => unreachable!(),
        };

        let window_handle = WlWindowHandle {
            window_id,
            is_layer: matches!(&wl_surface_backing, SurfaceBacking::Layer(_)),
            wl_display: self.wl_connection.display(),
            wl_surface: wl_surface_backing.wl_surface().clone(),
            event_send: self.event_send.clone(),
        };

        let wl_surface = wl_surface_backing.wl_surface().clone();

        let window = match Window::new(basalt.clone(), window_id, window_handle) {
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
                window_wk: Arc::downgrade(&window),
                surface: wl_surface_backing,
                create_pending: Some((window, pending_res)),
                inner_size,
                scale_factor: 1.0,
                cached_attributes,
                pointer_state: WindowPointerState {
                    visible: true,
                    locked: false,
                    confined: false,
                    cursor_icon: Default::default(),
                    active_pointers: HashMap::new(),
                },
                keyboard_state: WindowKeyboardState {
                    pressed: HashSet::new(),
                },
                cur_output_op: None,
                last_configure: None,
            },
        );

        // Note: The pending_res will be set and the window manager informed after the first
        //       configure to ensure the window is ready to draw.
    }

    fn close_window(&mut self, window_id: WindowID) {
        if let Some(wl_surface) = self.id_to_surface.remove(&window_id) {
            self.surface_to_id.remove(&wl_surface);
        }

        if let Some(mut window_state) = self.window_state.remove(&window_id) {
            for qwerty in window_state.keyboard_state.pressed.drain() {
                self.basalt_op
                    .as_ref()
                    .unwrap()
                    .input_ref()
                    .send_event(InputEvent::Release {
                        win: window_id,
                        key: qwerty.into(),
                    });
            }

            for mut active_pointer in window_state.pointer_state.active_pointers.into_values() {
                if let Some(wl_locked_pointer) = active_pointer.locked_op.take() {
                    wl_locked_pointer.destroy();
                }

                if let Some(wl_confined_pointer) = active_pointer.confined_op.take() {
                    wl_confined_pointer.destroy();
                }
            }

            if let Some((_, pending_res)) = window_state.create_pending.take() {
                pending_res.set(Err(WindowError::Closed));
            }
        }

        if self.focus_window_id.is_some() && *self.focus_window_id.as_ref().unwrap() == window_id {
            self.focus_window_id = None;
        }
    }

    fn window_title(&self, window_id: WindowID) -> Result<String, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let WindowCachedAttributes::Window {
            title_op, ..
        } = &window_state.cached_attributes
        {
            Ok(title_op.clone().unwrap_or_else(String::new))
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_set_title(&mut self, window_id: WindowID, title: String) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Window(wl_window) = &window_state.surface
            && let WindowCachedAttributes::Window {
                title_op, ..
            } = &mut window_state.cached_attributes
        {
            wl_window.set_title(title.clone());
            *title_op = Some(title);
            Ok(())
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_maximized(&self, window_id: WindowID) -> Result<bool, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let Some(wl_configure) = window_state.last_configure.as_ref() {
            Ok(wl_configure.state.contains(wl::WindowState::MAXIMIZED))
        } else {
            unreachable!() // Window only exists after first configure.
        }
    }

    fn window_set_maximized(
        &mut self,
        window_id: WindowID,
        maximized: bool,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Window(wl_window) = &window_state.surface {
            // TODO: Check if supported

            if maximized {
                wl_window.set_maximized();
            } else {
                wl_window.unset_maximized();
            }

            Ok(())
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_minimized(&self, window_id: WindowID) -> Result<bool, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let Some(wl_configure) = window_state.last_configure.as_ref() {
            Ok(wl_configure.state.contains(wl::WindowState::SUSPENDED))
        } else {
            unreachable!() // Window only exists after first configure.
        }
    }

    fn window_set_minimized(
        &mut self,
        window_id: WindowID,
        minimized: bool,
    ) -> Result<(), WindowError> {
        if !minimized {
            return Err(WindowError::NotSupported);
        }

        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Window(wl_window) = &window_state.surface {
            // TODO: Check if supported
            wl_window.set_minimized();
            Ok(())
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_size(&self, window_id: WindowID) -> Result<[u32; 2], WindowError> {
        Ok(self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?
            .inner_size)
    }

    fn window_set_size(&mut self, window_id: WindowID, size: [u32; 2]) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        match &window_state.surface {
            SurfaceBacking::Layer(wl_layer) => {
                // Note: A configure event should follow, triggering the resize event.

                wl_layer.set_size(size[0], size[1]);
                Ok(())
            },
            SurfaceBacking::Window(_) => {
                let last_configure = window_state
                    .last_configure
                    .as_ref()
                    .expect("unreachable: window doesn't exist until first configure");

                // Note: If window state has tiling assume it can't be resized.

                if last_configure.state.contains(wl::WindowState::TILED) {
                    return Err(WindowError::NotSupported);
                }

                // Note: Resizing a window is just a matter of drawing at the new size.

                window_state.inner_size = size;
                let window = window_state
                    .window_wk
                    .upgrade()
                    .ok_or(WindowError::Closed)?;

                window.send_event(WindowEvent::Resized {
                    width: size[0],
                    height: size[1],
                });

                Ok(())
            },
        }
    }

    fn window_min_size(&self, window_id: WindowID) -> Result<Option<[u32; 2]>, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let WindowCachedAttributes::Window {
            min_size_op, ..
        } = &window_state.cached_attributes
        {
            Ok(*min_size_op)
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_set_min_size(
        &mut self,
        window_id: WindowID,
        new_min_size_op: Option<[u32; 2]>,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Window(wl_window) = &window_state.surface
            && let WindowCachedAttributes::Window {
                min_size_op, ..
            } = &mut window_state.cached_attributes
        {
            wl_window.set_min_size(new_min_size_op.map(|[w, h]| (w, h)));
            *min_size_op = new_min_size_op;
            Ok(())
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_max_size(&self, window_id: WindowID) -> Result<Option<[u32; 2]>, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let WindowCachedAttributes::Window {
            max_size_op, ..
        } = &window_state.cached_attributes
        {
            Ok(*max_size_op)
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_set_max_size(
        &mut self,
        window_id: WindowID,
        new_max_size_op: Option<[u32; 2]>,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Window(wl_window) = &window_state.surface
            && let WindowCachedAttributes::Window {
                max_size_op, ..
            } = &mut window_state.cached_attributes
        {
            // TODO: It is a protocol error if max size is less than min size.
            wl_window.set_max_size(new_max_size_op.map(|[w, h]| (w, h)));
            *max_size_op = new_max_size_op;
            Ok(())
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_cursor_icon(&self, window_id: WindowID) -> Result<CursorIcon, WindowError> {
        Ok(self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?
            .pointer_state
            .cursor_icon)
    }

    fn window_set_cursor_icon(
        &mut self,
        window_id: WindowID,
        cursor_icon: CursorIcon,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        window_state.pointer_state.cursor_icon = cursor_icon;

        if window_state.pointer_state.visible {
            for wl_pointer in window_state.pointer_state.active_pointers.keys() {
                if let Some(wl_pointer_data) = wl_pointer.data::<wl::PointerData>()
                    && let Some(seat_state) = self.seat_state.get(wl_pointer_data.seat())
                    && let Some(themed_pointer) = seat_state.wl_pointer_op.as_ref()
                {
                    let _ = themed_pointer.set_cursor(
                        &self.wl_connection,
                        cursor_icon_to_wl(window_state.pointer_state.cursor_icon),
                    );
                }
            }
        }

        Ok(())
    }

    fn window_cursor_visible(&self, window_id: WindowID) -> Result<bool, WindowError> {
        Ok(self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?
            .pointer_state
            .visible)
    }

    fn window_set_cursor_visible(
        &mut self,
        window_id: WindowID,
        visible: bool,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if visible == window_state.pointer_state.visible {
            return Ok(());
        }

        for wl_pointer in window_state.pointer_state.active_pointers.keys() {
            if visible {
                if let Some(wl_pointer_data) = wl_pointer.data::<wl::PointerData>()
                    && let Some(seat_state) = self.seat_state.get(wl_pointer_data.seat())
                    && let Some(themed_pointer) = seat_state.wl_pointer_op.as_ref()
                {
                    let _ = themed_pointer.set_cursor(
                        &self.wl_connection,
                        cursor_icon_to_wl(window_state.pointer_state.cursor_icon),
                    );
                }
            } else {
                if let Some(wl_pointer_data) = wl_pointer.data::<wl::PointerData>()
                    && let Some(seat_state) = self.seat_state.get(wl_pointer_data.seat())
                    && let Some(themed_pointer) = seat_state.wl_pointer_op.as_ref()
                {
                    let _ = themed_pointer.hide_cursor();
                }
            }
        }

        let was_captured = !window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined);

        window_state.pointer_state.visible = visible;

        let is_captured = !window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined);

        if was_captured != is_captured {
            self.basalt_op
                .as_ref()
                .expect("unreachable: windows can only exist after basalt's creation")
                .input_ref()
                .send_event(InputEvent::CursorCapture {
                    win: window_id,
                    captured: is_captured,
                });
        }

        Ok(())
    }

    fn window_cursor_locked(&self, window_id: WindowID) -> Result<bool, WindowError> {
        Ok(self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?
            .pointer_state
            .locked)
    }

    fn window_set_cursor_locked(
        &mut self,
        window_id: WindowID,
        locked: bool,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if locked == window_state.pointer_state.locked {
            return Ok(());
        }

        for (wl_pointer, active_pointer) in window_state.pointer_state.active_pointers.iter_mut() {
            if locked {
                if let Some(wl_confined_pointer) = active_pointer.confined_op.take() {
                    wl_confined_pointer.destroy();
                }

                if active_pointer.locked_op.is_none() {
                    active_pointer.locked_op = self
                        .wl_ptr_constrs_state
                        .lock_pointer(
                            window_state.surface.wl_surface(),
                            wl_pointer,
                            None,
                            wl::PtrConstrLifetime::Oneshot,
                            &self.wl_queue_handle,
                        )
                        .ok();
                }
            } else if let Some(wl_locked_pointer) = active_pointer.locked_op.take() {
                wl_locked_pointer.destroy();
            }
        }

        let was_captured = !window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined);

        window_state.pointer_state.locked = true;
        window_state.pointer_state.confined = false;

        let is_captured = !window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined);

        if was_captured != is_captured {
            self.basalt_op
                .as_ref()
                .expect("unreachable: windows can only exist after basalt's creation")
                .input_ref()
                .send_event(InputEvent::CursorCapture {
                    win: window_id,
                    captured: is_captured,
                });
        }

        Ok(())
    }

    fn window_cursor_confined(&self, window_id: WindowID) -> Result<bool, WindowError> {
        Ok(self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?
            .pointer_state
            .confined)
    }

    fn window_set_cursor_confined(
        &mut self,
        window_id: WindowID,
        confined: bool,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if confined == window_state.pointer_state.confined {
            return Ok(());
        }

        for (wl_pointer, active_pointer) in window_state.pointer_state.active_pointers.iter_mut() {
            if confined {
                if let Some(wl_locked_pointer) = active_pointer.locked_op.take() {
                    wl_locked_pointer.destroy();
                }

                if active_pointer.confined_op.is_none() {
                    active_pointer.confined_op = self
                        .wl_ptr_constrs_state
                        .confine_pointer(
                            window_state.surface.wl_surface(),
                            wl_pointer,
                            None,
                            wl::PtrConstrLifetime::Oneshot,
                            &self.wl_queue_handle,
                        )
                        .ok();
                }
            } else if let Some(wl_confined_pointer) = active_pointer.confined_op.take() {
                wl_confined_pointer.destroy();
            }
        }

        let was_captured = !window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined);

        window_state.pointer_state.confined = true;
        window_state.pointer_state.locked = false;

        let is_captured = !window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined);

        if was_captured != is_captured {
            self.basalt_op
                .as_ref()
                .expect("unreachable: windows can only exist after basalt's creation")
                .input_ref()
                .send_event(InputEvent::CursorCapture {
                    win: window_id,
                    captured: is_captured,
                });
        }

        Ok(())
    }

    fn window_cursor_captured(&self, window_id: WindowID) -> Result<bool, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        Ok(!window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined))
    }

    fn window_set_cursor_captured(
        &mut self,
        window_id: WindowID,
        captured: bool,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if (captured
            && !window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined))
            || (!captured
                && window_state.pointer_state.visible
                && !window_state.pointer_state.locked
                && !window_state.pointer_state.confined)
        {
            return Ok(());
        }

        for (wl_pointer, active_pointer) in window_state.pointer_state.active_pointers.iter_mut() {
            if captured {
                if window_state.pointer_state.visible
                    && let Some(wl_pointer_data) = wl_pointer.data::<wl::PointerData>()
                    && let Some(seat_state) = self.seat_state.get(wl_pointer_data.seat())
                    && let Some(themed_pointer) = seat_state.wl_pointer_op.as_ref()
                {
                    let _ = themed_pointer.hide_cursor();
                }

                if active_pointer.locked_op.is_none() && active_pointer.confined_op.is_none() {
                    active_pointer.locked_op = self
                        .wl_ptr_constrs_state
                        .lock_pointer(
                            window_state.surface.wl_surface(),
                            wl_pointer,
                            None,
                            wl::PtrConstrLifetime::Oneshot,
                            &self.wl_queue_handle,
                        )
                        .ok();
                }
            } else {
                if !window_state.pointer_state.visible
                    && let Some(wl_pointer_data) = wl_pointer.data::<wl::PointerData>()
                    && let Some(seat_state) = self.seat_state.get(wl_pointer_data.seat())
                    && let Some(themed_pointer) = seat_state.wl_pointer_op.as_ref()
                {
                    let _ = themed_pointer.set_cursor(
                        &self.wl_connection,
                        cursor_icon_to_wl(window_state.pointer_state.cursor_icon),
                    );
                }

                if let Some(wl_locked_pointer) = active_pointer.locked_op.take() {
                    wl_locked_pointer.destroy();
                }

                if let Some(wl_confined_pointer) = active_pointer.confined_op.take() {
                    wl_confined_pointer.destroy();
                }
            }
        }

        let was_captured = !window_state.pointer_state.visible
            && (window_state.pointer_state.locked || window_state.pointer_state.confined);

        if captured {
            window_state.pointer_state.visible = false;

            if !window_state.pointer_state.locked && !window_state.pointer_state.confined {
                window_state.pointer_state.locked = true;
            }
        } else {
            window_state.pointer_state.visible = true;
            window_state.pointer_state.locked = false;
            window_state.pointer_state.confined = false;
        }

        if was_captured != captured {
            self.basalt_op
                .as_ref()
                .expect("unreachable: windows can only exist after basalt's creation")
                .input_ref()
                .send_event(InputEvent::CursorCapture {
                    win: window_id,
                    captured,
                });
        }

        Ok(())
    }

    fn window_monitor(&self, window_id: WindowID) -> Result<Monitor, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if window_state.cur_output_op.is_none() {
            return Err(WindowError::Unavailable);
        }

        match wl_output_to_monitor(
            &self.wl_output_state,
            window_state.cur_output_op.as_ref().unwrap(),
            true,
        ) {
            Some(monitor) => Ok(monitor),
            None => Err(WindowError::Unavailable),
        }
    }

    fn window_full_screen(&self, window_id: WindowID) -> Result<bool, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let Some(wl_configure) = window_state.last_configure.as_ref() {
            Ok(wl_configure.state.contains(wl::WindowState::FULLSCREEN))
        } else {
            unreachable!() // Window only exists after first configure.
        }
    }

    fn window_enable_full_screen(
        &mut self,
        window_id: WindowID,
        full_screen_behavior: FullScreenBehavior,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        // TODO: This doesn't seem to be reported correctly, at least on sway.

        /*if let Some(wl_configure) = window_state.last_configure.as_ref() {
            if !wl_configure
                .capabilities
                .contains(wl::WindowManagerCapabilities::FULLSCREEN)
            {
                return Err(WindowError::NotSupported);
            }
        } else {
            unreachable!() // Window only exists after first configure.
        }*/

        // Note: This maps exclusive behaviors to borderless ones as no compositors actually
        //       support fullscreen_shell. Maybe that changes in the future?

        let wl_output_op = match full_screen_behavior {
            FullScreenBehavior::AutoBorderlessPrimary
            | FullScreenBehavior::AutoExclusivePrimary => {
                return Err(EnableFullScreenError::UnableToDeterminePrimary.into());
            },
            FullScreenBehavior::Auto
            | FullScreenBehavior::AutoBorderless
            | FullScreenBehavior::AutoExclusive => {
                match window_state.cur_output_op.clone() {
                    Some(cur_output) => Some(cur_output),
                    None => self.wl_output_state.outputs().next(),
                }
            },
            FullScreenBehavior::AutoBorderlessCurrent
            | FullScreenBehavior::AutoExclusiveCurrent => {
                match window_state.cur_output_op.clone() {
                    Some(some) => Some(some),
                    None => {
                        return Err(EnableFullScreenError::UnableToDetermineCurrent.into());
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

                if self.wl_output_state.info(&user_output).is_none() {
                    return Err(EnableFullScreenError::MonitorDoesNotExist.into());
                }

                Some(user_output)
            },
        };

        if let SurfaceBacking::Window(wl_window) = &window_state.surface {
            wl_window.set_fullscreen(wl_output_op.as_ref());
            Ok(())
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn window_disable_full_screen(&mut self, window_id: WindowID) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Window(wl_window) = &window_state.surface {
            wl_window.unset_fullscreen();
            Ok(())
        } else {
            unreachable!() // Checked by WlWindowHandle
        }
    }

    fn layer_anchor(&self, window_id: WindowID) -> Result<WlLayerAnchor, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let WindowCachedAttributes::Layer {
            anchor, ..
        } = &window_state.cached_attributes
        {
            Ok(*anchor)
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_set_anchor(
        &mut self,
        window_id: WindowID,
        new_anchor: WlLayerAnchor,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Layer(wl_layer) = &window_state.surface
            && let WindowCachedAttributes::Layer {
                anchor, ..
            } = &mut window_state.cached_attributes
        {
            wl_layer.set_anchor(new_anchor.as_wl());
            *anchor = new_anchor;
            Ok(())
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_exclusive_zone(&self, window_id: WindowID) -> Result<i32, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let WindowCachedAttributes::Layer {
            exclusive_zone, ..
        } = &window_state.cached_attributes
        {
            Ok(*exclusive_zone)
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_set_exclusive_zone(
        &mut self,
        window_id: WindowID,
        new_exclusive_zone: i32,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Layer(wl_layer) = &window_state.surface
            && let WindowCachedAttributes::Layer {
                exclusive_zone, ..
            } = &mut window_state.cached_attributes
        {
            wl_layer.set_exclusive_zone(new_exclusive_zone);
            *exclusive_zone = new_exclusive_zone;
            Ok(())
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_margin(&self, window_id: WindowID) -> Result<[i32; 4], WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let WindowCachedAttributes::Layer {
            margin_tblr, ..
        } = &window_state.cached_attributes
        {
            Ok(*margin_tblr)
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_set_margin(
        &mut self,
        window_id: WindowID,
        new_margin_tblr: [i32; 4],
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Layer(wl_layer) = &window_state.surface
            && let WindowCachedAttributes::Layer {
                margin_tblr, ..
            } = &mut window_state.cached_attributes
        {
            wl_layer.set_margin(
                new_margin_tblr[0],
                new_margin_tblr[3],
                new_margin_tblr[1],
                new_margin_tblr[2],
            );
            *margin_tblr = new_margin_tblr;
            Ok(())
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_keyboard_focus(
        &self,
        window_id: WindowID,
    ) -> Result<WlLayerKeyboardFocus, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let WindowCachedAttributes::Layer {
            keyboard_focus, ..
        } = &window_state.cached_attributes
        {
            Ok(*keyboard_focus)
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_set_keyboard_focus(
        &mut self,
        window_id: WindowID,
        new_keyboard_focus: WlLayerKeyboardFocus,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Layer(wl_layer) = &window_state.surface
            && let WindowCachedAttributes::Layer {
                keyboard_focus, ..
            } = &mut window_state.cached_attributes
        {
            wl_layer.set_keyboard_interactivity(new_keyboard_focus.as_wl());
            *keyboard_focus = new_keyboard_focus;
            Ok(())
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_depth(&self, window_id: WindowID) -> Result<WlLayerDepth, WindowError> {
        let window_state = self
            .window_state
            .get(&window_id)
            .ok_or(WindowError::Closed)?;

        if let WindowCachedAttributes::Layer {
            depth, ..
        } = &window_state.cached_attributes
        {
            Ok(*depth)
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn layer_set_depth(
        &mut self,
        window_id: WindowID,
        new_depth: WlLayerDepth,
    ) -> Result<(), WindowError> {
        let window_state = self
            .window_state
            .get_mut(&window_id)
            .ok_or(WindowError::Closed)?;

        if let SurfaceBacking::Layer(wl_layer) = &window_state.surface
            && let WindowCachedAttributes::Layer {
                depth, ..
            } = &mut window_state.cached_attributes
        {
            wl_layer.set_layer(new_depth.as_wl());
            *depth = new_depth;
            Ok(())
        } else {
            unreachable!() // Checked by WlLayerHandle
        }
    }

    fn surface_scale_change(&mut self, wl_surface: &wl::Surface, scale_factor: i32) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
            && let Some(window) = window_state.window_wk.upgrade()
        {
            window_state.scale_factor = scale_factor as f32;
            window.set_dpi_scale(window_state.scale_factor);
        }
    }

    fn surface_enter(&mut self, wl_surface: &wl::Surface, wl_output: &wl::Output) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            window_state.cur_output_op = Some(wl_output.clone());
        }
    }

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

            match window_state.create_pending.take() {
                Some((window, pending_res)) => {
                    // This is the first configure, finish window creation.

                    window
                        .basalt_ref()
                        .window_manager_ref()
                        .window_created(window.clone());

                    pending_res.set(Ok(window));
                },
                None => {
                    if let Some(window) = window_state.window_wk.upgrade() {
                        if resized {
                            window.send_event(WindowEvent::Resized {
                                width: new_width,
                                height: new_height,
                            });
                        } else {
                            // Note: Probably not a bad idea to force a redraw after a configure.
                            window.send_event(WindowEvent::RedrawRequested);
                        }
                    }
                },
            }

            window_state.last_configure = Some(wl_configure);
        }
    }

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

            match window_state.create_pending.take() {
                Some((window, pending_res)) => {
                    // This is the first configure, finish window creation.

                    window
                        .basalt_ref()
                        .window_manager_ref()
                        .window_created(window.clone());

                    pending_res.set(Ok(window));
                },
                None => {
                    if let Some(window) = window_state.window_wk.upgrade() {
                        if resized {
                            window.send_event(WindowEvent::Resized {
                                width: new_width,
                                height: new_height,
                            });
                        } else {
                            // Note: Probably not a bad idea to force a redraw after a configure.
                            window.send_event(WindowEvent::RedrawRequested);
                        }
                    }
                },
            }
        }
    }

    fn window_close_request(&mut self, wl_surface: &wl::Surface) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get(window_id)
            && let Some(window) = window_state.window_wk.upgrade()
        {
            window.close_requested();
        }
    }

    fn layer_close(&mut self, wl_surface: &wl::Surface) {
        if let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
            && let Some(window) = window_state.window_wk.upgrade()
        {
            window.close();
        }
    }

    fn seat_new_capability(&mut self, wl_seat: wl::Seat, wl_capability: wl::Capability) {
        let seat_state = self.seat_state.entry(wl_seat.clone()).or_insert_with(|| {
            BackendSeatState {
                wl_keyboard_op: None,
                wl_pointer_op: None,
                wl_relative_ptr_op: None,
            }
        });

        if wl_capability == wl::Capability::Keyboard
            && seat_state.wl_keyboard_op.is_none()
            && let Ok(wl_keyboard) = self.wl_seat_state.get_keyboard_with_repeat(
                &self.wl_queue_handle,
                &wl_seat,
                None,
                self.loop_handle.clone(),
                Box::new(move |backend_state, _, wl_key_event| {
                    backend_state.keyboard_repeat(wl_key_event);
                }),
            )
        {
            seat_state.wl_keyboard_op = Some(wl_keyboard);
        }

        if wl_capability == wl::Capability::Pointer
            && seat_state.wl_pointer_op.is_none()
            && let Ok(themed_pointer) = self.wl_seat_state.get_pointer_with_theme(
                &self.wl_queue_handle,
                &wl_seat,
                self.wl_shm.wl_shm(),
                self.wl_compositor_state
                    .create_surface(&self.wl_queue_handle),
                wl::ThemeSpec::System,
            )
        {
            if seat_state.wl_relative_ptr_op.is_none() {
                seat_state.wl_relative_ptr_op = self
                    .wl_relative_ptr_state
                    .get_relative_pointer(themed_pointer.pointer(), &self.wl_queue_handle)
                    .ok();
            }

            seat_state.wl_pointer_op = Some(themed_pointer);
        }
    }

    fn seat_remove_capability(&mut self, wl_seat: wl::Seat, wl_capability: wl::Capability) {
        let seat_state = match self.seat_state.get_mut(&wl_seat) {
            Some(some) => some,
            None => return,
        };

        if wl_capability == wl::Capability::Keyboard
            && let Some(wl_keyboard) = seat_state.wl_keyboard_op.take()
        {
            wl_keyboard.release();
        }

        if wl_capability == wl::Capability::Pointer
            && let Some(themed_pointer) = seat_state.wl_pointer_op.take()
        {
            let wl_pointer = themed_pointer.pointer();

            for window_state in self.window_state.values_mut() {
                if let Some(mut active_pointer) = window_state
                    .pointer_state
                    .active_pointers
                    .remove(wl_pointer)
                {
                    if let Some(wl_locked_pointer) = active_pointer.locked_op.take() {
                        wl_locked_pointer.destroy();
                    }

                    if let Some(wl_confined_pointer) = active_pointer.confined_op.take() {
                        wl_confined_pointer.destroy();
                    }
                }
            }

            if let Some(wl_relative_ptr) = seat_state.wl_relative_ptr_op.take() {
                wl_relative_ptr.destroy();
            }

            wl_pointer.release();
        }

        if seat_state.wl_keyboard_op.is_none()
            && seat_state.wl_pointer_op.is_none()
            && seat_state.wl_relative_ptr_op.is_none()
        {
            self.seat_state.remove(&wl_seat);
        }
    }

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

    fn keyboard_leave(&mut self, wl_surface: &wl::Surface) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.surface_to_id.get(wl_surface)
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            for qwerty in window_state.keyboard_state.pressed.drain() {
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

    fn keyboard_press(&mut self, wl_key_event: wl::KeyEvent) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
            && let Some(window_state) = self.window_state.get_mut(window_id)
        {
            if let Some(qwerty) = raw_code_to_qwerty(wl_key_event.raw_code) {
                window_state.keyboard_state.pressed.insert(qwerty);

                basalt.input_ref().send_event(InputEvent::Press {
                    win: *window_id,
                    key: qwerty.into(),
                });
            }

            if let Some(utf8) = wl_key_event.utf8 {
                for c in utf8.chars() {
                    basalt.input_ref().send_event(InputEvent::Character {
                        win: *window_id,
                        c,
                    });
                }
            }
        }
    }

    fn keyboard_repeat(&mut self, wl_key_event: wl::KeyEvent) {
        if let Some(utf8) = wl_key_event.utf8
            && !utf8.is_empty()
            && let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
        {
            for c in utf8.chars() {
                basalt.input_ref().send_event(InputEvent::Character {
                    win: *window_id,
                    c,
                });
            }
        }
    }

    fn keyboard_release(&mut self, wl_key_event: wl::KeyEvent) {
        if let Some(basalt) = self.basalt_op.as_ref()
            && let Some(window_id) = self.focus_window_id.as_ref()
            && let Some(window_state) = self.window_state.get_mut(window_id)
            && let Some(qwerty) = raw_code_to_qwerty(wl_key_event.raw_code)
            && window_state.keyboard_state.pressed.remove(&qwerty)
        {
            basalt.input_ref().send_event(InputEvent::Release {
                win: *window_id,
                key: qwerty.into(),
            });
        }
    }

    fn pointer_frame(&mut self, wl_pointer: &wl::Pointer, wl_pointer_events: &[wl::PointerEvent]) {
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

                        if let Some(window_state) = self.window_state.get_mut(window_id)
                            && let Some(wl_pointer_data) = wl_pointer.data::<wl::PointerData>()
                            && let Some(seat_state) = self.seat_state.get(wl_pointer_data.seat())
                            && let Some(themed_pointer) = seat_state.wl_pointer_op.as_ref()
                        {
                            if window_state.pointer_state.visible {
                                let _ = themed_pointer.set_cursor(
                                    &self.wl_connection,
                                    cursor_icon_to_wl(window_state.pointer_state.cursor_icon),
                                );
                            } else {
                                let _ = themed_pointer.hide_cursor();
                            }

                            let locked_op = window_state
                                .pointer_state
                                .locked
                                .then_some(())
                                .and_then(|_| {
                                    self.wl_ptr_constrs_state
                                        .lock_pointer(
                                            &wl_pointer_event.surface,
                                            wl_pointer,
                                            None,
                                            wl::PtrConstrLifetime::Oneshot,
                                            &self.wl_queue_handle,
                                        )
                                        .ok()
                                });

                            let confined_op = window_state
                                .pointer_state
                                .confined
                                .then_some(())
                                .and_then(|_| {
                                    self.wl_ptr_constrs_state
                                        .confine_pointer(
                                            &wl_pointer_event.surface,
                                            wl_pointer,
                                            None,
                                            wl::PtrConstrLifetime::Oneshot,
                                            &self.wl_queue_handle,
                                        )
                                        .ok()
                                });

                            window_state.pointer_state.active_pointers.insert(
                                wl_pointer.clone(),
                                WindowActivePointer {
                                    locked_op,
                                    confined_op,
                                },
                            );
                        }
                    },
                    wl::PointerEventKind::Leave {
                        ..
                    } => {
                        basalt.input_ref().send_event(InputEvent::Leave {
                            win: *window_id,
                        });

                        if let Some(window_state) = self.window_state.get_mut(window_id) {
                            if let Some(mut active_pointer) = window_state
                                .pointer_state
                                .active_pointers
                                .remove(wl_pointer)
                            {
                                if let Some(wl_locked_pointer) = active_pointer.locked_op.take() {
                                    wl_locked_pointer.destroy();
                                }

                                if let Some(wl_confined_pointer) = active_pointer.locked_op.take() {
                                    wl_confined_pointer.destroy();
                                }
                            }
                        }
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
                            None => continue,
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
                            None => continue,
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

    fn relative_motion(&mut self, wl_relative_motion_event: wl::RelativeMotionEvent) {
        if let Some(basalt) = self.basalt_op.as_ref() {
            basalt.input_ref().send_event(InputEvent::Motion {
                x: wl_relative_motion_event.delta_unaccel.0 as f32,
                y: wl_relative_motion_event.delta_unaccel.1 as f32,
            });
        }
    }
}
