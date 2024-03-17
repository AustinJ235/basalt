use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex;
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use vulkano::format::Format as VkFormat;
use vulkano::swapchain::{
    ColorSpace as VkColorSpace, FullScreenExclusive, PresentMode, Surface, SurfaceCapabilities,
    SurfaceInfo, Win32Monitor,
};
use winit::dpi::PhysicalSize;
use winit::window::{CursorGrabMode, Window as WinitWindow, WindowId as WinitWindowId};

use crate::input::key::KeyCombo;
use crate::input::state::{LocalCursorState, LocalKeyState, WindowState};
use crate::input::{Char, InputEvent, InputHookCtrl, InputHookID, InputHookTarget};
use crate::interface::bin::Bin;
use crate::render::{VSync, MSAA};
use crate::window::monitor::{FullScreenBehavior, FullScreenError, Monitor};
use crate::window::{BinID, WindowEvent, WindowID, WindowManager, WindowType};
use crate::Basalt;

/// Object that represents a window.
///
/// This object is generally past around as it allows accessing mosts things within the crate.
pub struct Window {
    id: WindowID,
    inner: Arc<WinitWindow>,
    basalt: Arc<Basalt>,
    wm: Arc<WindowManager>,
    surface: Arc<Surface>,
    window_type: WindowType,
    state: Mutex<State>,
    close_requested: AtomicBool,
}

#[derive(Debug)]
struct State {
    cursor_captured: bool,
    ignore_dpi: bool,
    dpi_scale: f32,
    interface_scale: f32,
    msaa: MSAA,
    vsync: VSync,
    associated_bins: HashMap<BinID, Weak<Bin>>,
    attached_input_hooks: Vec<InputHookID>,
    keep_alive_objects: Vec<Box<dyn Any + Send + Sync + 'static>>,
}

impl std::fmt::Debug for Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Window")
            .field("id", &self.id)
            .field("inner", &self.inner)
            .field("surface", &self.surface)
            .field("window_type", &self.window_type)
            .field("state", &self.state)
            .finish()
    }
}

impl PartialEq<Window> for Window {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Arc::ptr_eq(&self.basalt, &other.basalt)
    }
}

impl Window {
    pub(crate) fn new(
        basalt: Arc<Basalt>,
        wm: Arc<WindowManager>,
        id: WindowID,
        winit: Arc<WinitWindow>,
    ) -> Result<Arc<Self>, String> {
        // NOTE: Although it may seem the winit window doesn't need to be in an Arc. This allows
        //       vulkano to keep the window alive longer than the surface. It may be possible to
        //       pass the basalt window instead, but that'd likey mean keeping surface in a mutex.

        let surface = Surface::from_window(basalt.instance(), winit.clone())
            .map_err(|e| format!("Failed to create surface: {}", e))?;

        let window_type = match winit.raw_window_handle() {
            RawWindowHandle::AndroidNdk(_) => WindowType::Android,
            RawWindowHandle::AppKit(_) => WindowType::Macos,
            RawWindowHandle::UiKit(_) => WindowType::Ios,
            RawWindowHandle::Wayland(_) => WindowType::Wayland,
            RawWindowHandle::Win32(_) => WindowType::Windows,
            RawWindowHandle::Xcb(_) => WindowType::Xcb,
            RawWindowHandle::Xlib(_) => WindowType::Xlib,
            _ => unimplemented!(),
        };

        let (ignore_dpi, dpi_scale) = match basalt.config.window_ignore_dpi {
            true => (true, 1.0),
            false => (false, winit.scale_factor() as f32),
        };

        let state = State {
            cursor_captured: false,
            ignore_dpi,
            dpi_scale,
            msaa: basalt.config.render_default_msaa,
            vsync: basalt.config.render_default_vsync,
            interface_scale: basalt.config.window_default_scale,
            associated_bins: HashMap::new(),
            attached_input_hooks: Vec::new(),
            keep_alive_objects: Vec::new(),
        };

        Ok(Arc::new(Self {
            id,
            inner: winit,
            basalt,
            wm,
            surface,
            window_type,
            state: Mutex::new(state),
            close_requested: AtomicBool::new(false),
        }))
    }

    pub(crate) fn winit_id(&self) -> WinitWindowId {
        self.inner.id()
    }

    pub(crate) fn associate_bin(&self, bin: Arc<Bin>) {
        self.state
            .lock()
            .associated_bins
            .insert(bin.id(), Arc::downgrade(&bin));

        self.wm
            .send_window_event(self.id, WindowEvent::AssociateBin(bin));
    }

    pub(crate) fn dissociate_bin(&self, bin_id: BinID) {
        self.state.lock().associated_bins.remove(&bin_id);
        self.wm
            .send_window_event(self.id, WindowEvent::DissociateBin(bin_id));
    }

    pub(crate) fn update_bin(&self, bin_id: BinID) {
        self.wm
            .send_window_event(self.id, WindowEvent::UpdateBin(bin_id));
    }

    pub(crate) fn update_bin_batch(&self, bin_ids: Vec<BinID>) {
        self.wm
            .send_window_event(self.id, WindowEvent::UpdateBinBatch(bin_ids));
    }

    /// The window id of this window.
    pub fn id(&self) -> WindowID {
        self.id
    }

    /// Obtain a copy of `Arc<Basalt>`
    pub fn basalt(&self) -> Arc<Basalt> {
        self.basalt.clone()
    }

    /// Obtain a reference of `Arc<Basalt>`
    pub fn basalt_ref(&self) -> &Arc<Basalt> {
        &self.basalt
    }

    /// Obtain a copy of `Arc<Surface>`
    pub fn surface(&self) -> Arc<Surface> {
        self.surface.clone()
    }

    /// Obtain a reference of `Arc<Surface>`
    pub fn surface_ref(&self) -> &Arc<Surface> {
        &self.surface
    }

    /// Obtain a copy of `Arc<WindowManager>`
    pub fn window_manager(&self) -> Arc<WindowManager> {
        self.wm.clone()
    }

    /// Obtain a reference of `Arc<WindowManager>`
    pub fn window_manager_ref(&self) -> &Arc<WindowManager> {
        &self.wm
    }

    /// Create a new `Bin` associated with this window.
    pub fn new_bin(self: &Arc<Self>) -> Arc<Bin> {
        let bin = self.basalt.interface_ref().new_bin();
        bin.associate_window(self);
        bin
    }

    /// Create new `Bin`'s associated with this window.
    pub fn new_bins(self: &Arc<Self>, count: usize) -> Vec<Arc<Bin>> {
        let bins = self.basalt.interface_ref().new_bins(count);

        for bin in &bins {
            bin.associate_window(self);
        }

        bins
    }

    /// Retrieve a list of `Bin`'s associated to this window.
    pub fn associated_bins(&self) -> Vec<Arc<Bin>> {
        self.state
            .lock()
            .associated_bins
            .values()
            .filter_map(|wk| wk.upgrade())
            .collect()
    }

    /// Retrieve a list of `BinID`'s associated to this window.
    pub fn associated_bin_ids(&self) -> Vec<BinID> {
        self.state.lock().associated_bins.keys().copied().collect()
    }

    /// Hides and captures cursor.
    pub fn capture_cursor(&self) {
        let mut state = self.state.lock();
        state.cursor_captured = true;

        self.inner.set_cursor_visible(false);
        self.inner
            .set_cursor_grab(CursorGrabMode::Confined)
            .unwrap();
        self.basalt
            .input_ref()
            .send_event(InputEvent::CursorCapture {
                win: self.id,
                captured: true,
            });
    }

    /// Shows and releases cursor.
    pub fn release_cursor(&self) {
        let mut state = self.state.lock();
        state.cursor_captured = false;

        self.inner.set_cursor_visible(true);
        self.inner.set_cursor_grab(CursorGrabMode::None).unwrap();

        self.basalt
            .input_ref()
            .send_event(InputEvent::CursorCapture {
                win: self.id,
                captured: false,
            });
    }

    /// Checks if cursor is currently captured.
    pub fn cursor_captured(&self) -> bool {
        self.state.lock().cursor_captured
    }

    /// Return a list of active monitors on the system.
    pub fn monitors(&self) -> Vec<Monitor> {
        let current_op = self.inner.current_monitor();
        let primary_op = self.inner.primary_monitor();

        self.inner
            .available_monitors()
            .filter_map(|winit_monitor| {
                let is_current = match current_op.as_ref() {
                    Some(current) => *current == winit_monitor,
                    None => false,
                };

                let is_primary = match primary_op.as_ref() {
                    Some(primary) => *primary == winit_monitor,
                    None => false,
                };

                let mut monitor = Monitor::from_winit(winit_monitor)?;
                monitor.is_current = is_current;
                monitor.is_primary = is_primary;
                Some(monitor)
            })
            .collect()
    }

    /// Return the primary monitor if the implementation is able to determine it.
    pub fn primary_monitor(&self) -> Option<Monitor> {
        self.inner.primary_monitor().and_then(|winit_monitor| {
            let is_current = match self.inner.current_monitor() {
                Some(current) => current == winit_monitor,
                None => false,
            };

            let mut monitor = Monitor::from_winit(winit_monitor)?;
            monitor.is_primary = true;
            monitor.is_current = is_current;
            Some(monitor)
        })
    }

    /// Return the current monitor if the implementation is able to determine it.
    pub fn current_monitor(&self) -> Option<Monitor> {
        self.inner.current_monitor().and_then(|winit_monitor| {
            let is_primary = match self.inner.primary_monitor() {
                Some(primary) => primary == winit_monitor,
                None => false,
            };

            let mut monitor = Monitor::from_winit(winit_monitor)?;
            monitor.is_current = true;
            monitor.is_primary = is_primary;
            Some(monitor)
        })
    }

    /// Enable fullscreen with the provided behavior.
    ///
    /// If `fallback_borderless` is set to `true` and am exclusive behavior is used when it isn't
    /// supported am equivalent borderless behavior will be used.
    pub fn enable_fullscreen(
        &self,
        fallback_borderless: bool,
        behavior: FullScreenBehavior,
    ) -> Result<(), FullScreenError> {
        let winit_fullscreen = behavior.determine_winit_fullscreen(
            fallback_borderless,
            self.basalt
                .device_ref()
                .enabled_extensions()
                .ext_full_screen_exclusive,
            self.current_monitor(),
            self.primary_monitor(),
            self.monitors(),
        )?;

        self.inner.set_fullscreen(Some(winit_fullscreen));
        self.wm
            .send_window_event(self.id, WindowEvent::EnabledFullscreen);
        Ok(())
    }

    /// Disable fullscreen.
    ///
    /// ***Note:** This is a no-op if this window isn't fullscreen.*
    pub fn disable_fullscreen(&self) {
        if self.inner.fullscreen().is_some() {
            self.inner.set_fullscreen(None);
            self.wm
                .send_window_event(self.id, WindowEvent::DisabledFullscreen);
        }
    }

    /// Toggle fullscreen mode. Uses `FullScreenBehavior::Auto`.
    pub fn toggle_fullscreen(&self) -> Result<(), FullScreenError> {
        if self.is_fullscreen() {
            self.disable_fullscreen();
            Ok(())
        } else {
            self.enable_fullscreen(true, Default::default())
        }
    }

    /// Check if the window is fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        self.inner.fullscreen().is_some()
    }

    /// Request the monitor to resize to the given dimensions.
    ///
    /// ***Note:** Returns `false` if the platform doesn't support resize.*
    pub fn request_resize(&self, width: u32, height: u32) -> bool {
        // TODO: Should this take into account dpi scaling and interface scaling?

        let request_size = PhysicalSize::new(width, height);
        let pre_request_size = self.inner.inner_size();

        match self.inner.request_inner_size(request_size) {
            Some(physical_size) => {
                if physical_size == pre_request_size {
                    // Platform doesn't support resize.
                    return false;
                }

                if physical_size == request_size {
                    // If the size is the same as the one that was requested, then the platform
                    // resized the window immediately. In this case, the resize event may not get
                    // sent out per winit docs.

                    self.wm.send_window_event(
                        self.id,
                        WindowEvent::Resized {
                            width,
                            height,
                        },
                    );
                }

                true
            },
            None => {
                // The resize request was sent and a subsequent resize event will be sent.
                true
            },
        }
    }

    /// Return the dimensions of the client area of this window.
    pub fn inner_dimensions(&self) -> [u32; 2] {
        self.inner.inner_size().into()
    }

    /// Return the `WindowType` of this window.
    pub fn window_type(&self) -> WindowType {
        self.window_type
    }

    /// DPI scaling used on this window.
    pub fn dpi_scale(&self) -> f32 {
        self.state.lock().dpi_scale
    }

    /// Check if this window is ignoring dpi scaling.
    ///
    /// ***Note:** This is configured upon basalt's creation via its options.*
    pub fn ignoring_dpi(&self) -> bool {
        self.state.lock().ignore_dpi
    }

    pub(crate) fn set_dpi_scale(&self, scale: f32) {
        let mut state = self.state.lock();

        if state.ignore_dpi {
            state.dpi_scale = 1.0;
        } else {
            state.dpi_scale = scale;
        }

        self.wm.send_window_event(
            self.id,
            WindowEvent::ScaleChanged(state.interface_scale * state.dpi_scale),
        );
    }

    /// Current interface scale. This does not include dpi scaling.
    pub fn current_interface_scale(&self) -> f32 {
        self.state.lock().interface_scale
    }

    /// Current effective interface scale. This includes dpi scaling.
    pub fn effective_interface_scale(&self) -> f32 {
        let state = self.state.lock();
        state.interface_scale * state.dpi_scale
    }

    /// Set the scale of the interface. This does not include dpi scaling.
    pub fn set_interface_scale(&self, set_scale: f32) {
        let mut state = self.state.lock();
        state.interface_scale = set_scale;

        self.wm.send_window_event(
            self.id,
            WindowEvent::ScaleChanged(state.interface_scale * state.dpi_scale),
        );
    }

    /// Set the scale of the interface. This includes dpi scaling.
    pub fn set_effective_interface_scale(&self, set_scale: f32) {
        let mut state = self.state.lock();
        state.interface_scale = set_scale / state.dpi_scale;

        self.wm.send_window_event(
            self.id,
            WindowEvent::ScaleChanged(state.interface_scale * state.dpi_scale),
        );
    }

    /// Get the current MSAA used for rendering.
    pub fn renderer_msaa(&self) -> MSAA {
        self.state.lock().msaa
    }

    /// Set the current MSAA used for rendering.
    pub fn set_renderer_msaa(&self, msaa: MSAA) {
        self.state.lock().msaa = msaa;

        self.wm
            .send_window_event(self.id, WindowEvent::SetMSAA(msaa));
    }

    /// Increase the current MSAA used for rendering returning the new value.
    pub fn incr_renderer_msaa(&self) -> MSAA {
        let mut state = self.state.lock();

        let msaa = match state.msaa {
            MSAA::X1 => MSAA::X2,
            MSAA::X2 => MSAA::X4,
            MSAA::X4 => MSAA::X8,
            MSAA::X8 => return MSAA::X8,
        };

        self.wm
            .send_window_event(self.id, WindowEvent::SetMSAA(msaa));

        state.msaa = msaa;
        msaa
    }

    /// Decrease the current MSAA used for rendering returning the new value.
    pub fn decr_renderer_msaa(&self) -> MSAA {
        let mut state = self.state.lock();

        let msaa = match state.msaa {
            MSAA::X1 => return MSAA::X1,
            MSAA::X2 => MSAA::X1,
            MSAA::X4 => MSAA::X2,
            MSAA::X8 => MSAA::X4,
        };

        self.wm
            .send_window_event(self.id, WindowEvent::SetMSAA(msaa));

        state.msaa = msaa;
        msaa
    }

    /// Get the current VSync used for rendering.
    pub fn renderer_vsync(&self) -> VSync {
        self.state.lock().vsync
    }

    /// Set the current VSync used for rendering.
    pub fn set_renderer_vsync(&self, vsync: VSync) {
        self.state.lock().vsync = vsync;

        self.wm
            .send_window_event(self.id, WindowEvent::SetVSync(vsync));
    }

    /// Toggle the current VSync used returning the new value.
    pub fn toggle_renderer_vsync(&self) -> VSync {
        let mut state = self.state.lock();

        let vsync = match state.vsync {
            VSync::Enable => VSync::Disable,
            VSync::Disable => VSync::Enable,
        };

        self.wm
            .send_window_event(self.id, WindowEvent::SetVSync(vsync));

        state.vsync = vsync;
        vsync
    }

    /// Keep objects alive for the lifetime of the window.
    pub fn keep_alive<O, T>(&self, objects: O)
    where
        O: IntoIterator<Item = T>,
        T: Any + Send + Sync + 'static,
    {
        for object in objects {
            self.state.lock().keep_alive_objects.push(Box::new(object));
        }
    }

    /// Request the window to close.
    ///
    /// ***Note:** This will not result in the window closing immeditely. Instead, this will remove any
    /// strong references basalt may have to this window allowing it to be dropped. It is also on
    /// the user to remove their strong references to the window to allow it drop. When the window
    /// drops it will be closed.*
    pub fn close(&self) {
        self.close_requested.store(true, atomic::Ordering::SeqCst);
        self.wm.send_window_event(self.id, WindowEvent::Closed);
    }

    /// Check if a close has been requested.
    pub fn close_requested(&self) -> bool {
        self.close_requested.load(atomic::Ordering::SeqCst)
    }

    /// Return the `Win32Monitor` used if present.
    pub(crate) fn win32_monitor(&self) -> Option<Win32Monitor> {
        #[cfg(target_os = "windows")]
        unsafe {
            use winit::platform::windows::MonitorHandleExtWindows;

            self.inner
                .current_monitor()
                .map(|m| Win32Monitor::new(m.hmonitor() as *const std::ffi::c_void))
        }

        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }

    pub(crate) fn surface_capabilities(&self, fse: FullScreenExclusive) -> SurfaceCapabilities {
        self.basalt
            .physical_device_ref()
            .surface_capabilities(
                &self.surface,
                match fse {
                    FullScreenExclusive::ApplicationControlled => {
                        SurfaceInfo {
                            full_screen_exclusive: FullScreenExclusive::ApplicationControlled,
                            win32_monitor: self.win32_monitor(),
                            ..SurfaceInfo::default()
                        }
                    },
                    fse => {
                        SurfaceInfo {
                            full_screen_exclusive: fse,
                            ..SurfaceInfo::default()
                        }
                    },
                },
            )
            .unwrap()
    }

    pub(crate) fn surface_formats(
        &self,
        fse: FullScreenExclusive,
    ) -> Vec<(VkFormat, VkColorSpace)> {
        self.basalt
            .physical_device_ref()
            .surface_formats(
                &self.surface,
                match fse {
                    FullScreenExclusive::ApplicationControlled => {
                        SurfaceInfo {
                            full_screen_exclusive: FullScreenExclusive::ApplicationControlled,
                            win32_monitor: self.win32_monitor(),
                            ..SurfaceInfo::default()
                        }
                    },
                    fse => {
                        SurfaceInfo {
                            full_screen_exclusive: fse,
                            ..SurfaceInfo::default()
                        }
                    },
                },
            )
            .unwrap()
    }

    pub(crate) fn surface_present_modes(&self, fse: FullScreenExclusive) -> Vec<PresentMode> {
        self.basalt
            .physical_device_ref()
            .surface_present_modes(
                &self.surface,
                match fse {
                    FullScreenExclusive::ApplicationControlled => {
                        SurfaceInfo {
                            full_screen_exclusive: FullScreenExclusive::ApplicationControlled,
                            win32_monitor: self.win32_monitor(),
                            ..SurfaceInfo::default()
                        }
                    },
                    fse => {
                        SurfaceInfo {
                            full_screen_exclusive: fse,
                            ..SurfaceInfo::default()
                        }
                    },
                },
            )
            .unwrap()
            .collect()
    }

    pub(crate) fn surface_current_extent(&self, fse: FullScreenExclusive) -> [u32; 2] {
        self.surface_capabilities(fse)
            .current_extent
            .unwrap_or_else(|| self.inner_dimensions())
    }

    /// Attach an input hook to this window. When the window closes, this hook will be
    /// automatically removed from `Input`.
    ///
    /// ***Note**: If a hook's target is a window this behavior already occurs without needing to
    /// call this method.*
    pub fn attach_input_hook(&self, hook: InputHookID) {
        self.state.lock().attached_input_hooks.push(hook);
    }

    pub fn on_press<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_press()
            .keys(combo)
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_release<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_release()
            .keys(combo)
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_hold<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl
            + Send
            + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_hold()
            .keys(combo)
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_character<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_character()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_enter<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_enter()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_leave<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_leave()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_focus<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_focus()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_focus_lost<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_focus_lost()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_scroll<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_scroll()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_cursor<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
            + Send
            + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_cursor()
            .call(method)
            .finish()
            .unwrap()
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        for hook_id in self.state.lock().attached_input_hooks.drain(..) {
            self.basalt.input_ref().remove_hook(hook_id);
        }
    }
}

unsafe impl HasRawWindowHandle for Window {
    fn raw_window_handle(&self) -> RawWindowHandle {
        self.inner.raw_window_handle()
    }
}

unsafe impl HasRawDisplayHandle for Window {
    fn raw_display_handle(&self) -> RawDisplayHandle {
        self.inner.raw_display_handle()
    }
}
