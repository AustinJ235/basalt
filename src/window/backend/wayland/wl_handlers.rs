use smithay_client_toolkit::shell::WaylandSurface;

use super::BackendState;

mod wl {
    pub use smithay_client_toolkit::compositor::CompositorHandler;
    pub use smithay_client_toolkit::output::{OutputHandler, OutputState};
    pub use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard as Keyboard;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_output::{
        Transform, WlOutput as Output,
    };
    pub use smithay_client_toolkit::reexports::client::protocol::wl_pointer::WlPointer as Pointer;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat as Seat;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface as Surface;
    pub use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
    pub use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
    pub use smithay_client_toolkit::seat::keyboard::{
        KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
    };
    pub use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerHandler};
    pub use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
    pub use smithay_client_toolkit::shell::wlr_layer::{
        LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
    };
    pub use smithay_client_toolkit::shell::xdg::window::{Window, WindowConfigure, WindowHandler};
    pub use smithay_client_toolkit::shm::{Shm, ShmHandler};
    pub use smithay_client_toolkit::{
        delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
        delegate_registry, delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
        registry_handlers, delegate_pointer_constraints, delegate_relative_pointer,
    };
    pub use smithay_client_toolkit::seat::pointer_constraints::PointerConstraintsHandler;
    pub use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::zwp_confined_pointer_v1::ZwpConfinedPointerV1;
    pub use smithay_client_toolkit::reexports::protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::ZwpLockedPointerV1;
    pub use smithay_client_toolkit::seat::relative_pointer::RelativePointerHandler;
    pub use smithay_client_toolkit::reexports::protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1;
    pub use smithay_client_toolkit::seat::relative_pointer::RelativeMotionEvent;
}

wl::delegate_registry!(BackendState);
wl::delegate_compositor!(BackendState);
wl::delegate_output!(BackendState);
wl::delegate_seat!(BackendState);
wl::delegate_keyboard!(BackendState);
wl::delegate_pointer!(BackendState);
wl::delegate_pointer_constraints!(BackendState);
wl::delegate_relative_pointer!(BackendState);
wl::delegate_shm!(BackendState);
wl::delegate_layer!(BackendState);
wl::delegate_xdg_shell!(BackendState);
wl::delegate_xdg_window!(BackendState);

impl wl::ProvidesRegistryState for BackendState {
    wl::registry_handlers![wl::OutputState, wl::SeatState];

    fn registry(&mut self) -> &mut wl::RegistryState {
        &mut self.wl_registry_state
    }
}

impl wl::ShmHandler for BackendState {
    fn shm_state(&mut self) -> &mut wl::Shm {
        &mut self.wl_shm
    }
}

impl wl::CompositorHandler for BackendState {
    fn scale_factor_changed(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_surface: &wl::Surface,
        scale_factor: i32,
    ) {
        self.surface_scale_change(wl_surface, scale_factor);
    }

    fn transform_changed(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::Surface,
        _: wl::Transform,
    ) {
    }

    fn frame(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: &wl::Surface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_surface: &wl::Surface,
        wl_output: &wl::Output,
    ) {
        self.surface_enter(wl_surface, wl_output);
    }

    fn surface_leave(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::Surface,
        _: &wl::Output,
    ) {
    }
}

impl wl::OutputHandler for BackendState {
    fn output_state(&mut self) -> &mut wl::OutputState {
        &mut self.wl_output_state
    }

    fn new_output(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::Output) {}

    fn update_output(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::Output) {}

    fn output_destroyed(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::Output) {}
}

impl wl::WindowHandler for BackendState {
    fn request_close(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_window: &wl::Window,
    ) {
        self.window_close_request(wl_window.wl_surface());
    }

    fn configure(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_window: &wl::Window,
        wl_configure: wl::WindowConfigure,
        _: u32,
    ) {
        self.window_configure(wl_window.wl_surface(), wl_configure);
    }
}

impl wl::LayerShellHandler for BackendState {
    fn closed(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_layer: &wl::LayerSurface,
    ) {
        self.layer_close(wl_layer.wl_surface());
    }

    fn configure(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_layer: &wl::LayerSurface,
        wl_configure: wl::LayerSurfaceConfigure,
        _: u32,
    ) {
        self.layer_configure(wl_layer.wl_surface(), wl_configure);
    }
}

impl wl::SeatHandler for BackendState {
    fn seat_state(&mut self) -> &mut wl::SeatState {
        &mut self.wl_seat_state
    }

    fn new_capability(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_seat: wl::Seat,
        wl_capability: wl::Capability,
    ) {
        self.seat_new_capability(wl_seat, wl_capability);
    }

    fn remove_capability(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_seat: wl::Seat,
        wl_capability: wl::Capability,
    ) {
        self.seat_remove_capability(wl_seat, wl_capability);
    }

    fn new_seat(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::Seat) {}

    fn remove_seat(&mut self, _: &wl::Connection, _: &wl::QueueHandle<Self>, _: wl::Seat) {}
}

impl wl::KeyboardHandler for BackendState {
    fn enter(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::Keyboard,
        wl_surface: &wl::Surface,
        _: u32,
        _: &[u32],
        _: &[wl::Keysym],
    ) {
        self.keyboard_enter(wl_surface);
    }

    fn leave(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::Keyboard,
        wl_surface: &wl::Surface,
        _: u32,
    ) {
        self.keyboard_leave(wl_surface);
    }

    fn press_key(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::Keyboard,
        _: u32,
        wl_key_event: wl::KeyEvent,
    ) {
        self.keyboard_press(wl_key_event);
    }

    fn repeat_key(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::Keyboard,
        _: u32,
        _: wl::KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::Keyboard,
        _: u32,
        wl_key_event: wl::KeyEvent,
    ) {
        self.keyboard_release(wl_key_event);
    }

    fn update_modifiers(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::Keyboard,
        _: u32,
        _: wl::Modifiers,
        _: wl::RawModifiers,
        _: u32,
    ) {
    }
}

impl wl::PointerHandler for BackendState {
    fn pointer_frame(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        wl_pointer: &wl::Pointer,
        wl_pointer_events: &[wl::PointerEvent],
    ) {
        self.pointer_frame(wl_pointer, wl_pointer_events);
    }
}

impl wl::PointerConstraintsHandler for BackendState {
    fn confined(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::ZwpConfinedPointerV1,
        _: &wl::Surface,
        _: &wl::Pointer,
    ) {
    }

    fn unconfined(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::ZwpConfinedPointerV1,
        _: &wl::Surface,
        _: &wl::Pointer,
    ) {
    }

    fn locked(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::ZwpLockedPointerV1,
        _: &wl::Surface,
        _: &wl::Pointer,
    ) {
    }

    fn unlocked(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::ZwpLockedPointerV1,
        _: &wl::Surface,
        _: &wl::Pointer,
    ) {
    }
}

impl wl::RelativePointerHandler for BackendState {
    fn relative_pointer_motion(
        &mut self,
        _: &wl::Connection,
        _: &wl::QueueHandle<Self>,
        _: &wl::ZwpRelativePointerV1,
        _: &wl::Pointer,
        wl_relative_motion_event: wl::RelativeMotionEvent,
    ) {
        self.relative_motion(wl_relative_motion_event);
    }
}
