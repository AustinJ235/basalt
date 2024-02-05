use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use vulkano::format::Format as VkFormat;
use vulkano::swapchain::{
    ColorSpace as VkColorSpace, FullScreenExclusive, PresentMode, Surface, SurfaceCapabilities,
    Win32Monitor,
};
use winit::window::Window as WinitWindow;

use crate::input::key::KeyCombo;
use crate::input::state::{LocalCursorState, LocalKeyState, WindowState};
use crate::input::{Char, InputHookCtrl, InputHookID, InputHookTarget};
use crate::window::monitor::{FullScreenBehavior, FullScreenError, Monitor};
use crate::window::{WindowID, WindowType};
use crate::Basalt;

pub struct Window {
    id: WindowID,
    inner: Arc<WinitWindow>,
    basalt: Arc<Basalt>,
    surface: Arc<Surface>,
    window_type: WindowType,
    cursor_captured: AtomicBool,
    associated_hooks: Mutex<Vec<InputHookID>>,
}

impl std::fmt::Debug for Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Window")
            .field("id", &self.id)
            .field("inner", &self.inner)
            .field("surface", &self.surface)
            .field("window_type", &self.window_type)
            .field("associated_hooks", &self.associated_hooks)
            .finish()
    }
}

impl Window {
    pub(crate) fn new(
        basalt: Arc<Basalt>,
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

        Ok(Arc::new(Self {
            id,
            inner: winit,
            basalt,
            surface,
            window_type,
            cursor_captured: AtomicBool::new(false),
            associated_hooks: Mutex::new(Vec::new()),
        }))
    }

    /// The window id of this window.
    pub fn id(&self) -> WindowID {
        self.id
    }

    /// Get `Basalt` used by the window.
    pub fn basalt(&self) -> Arc<Basalt> {
        self.basalt.clone()
    }

    pub fn basalt_ref(&self) -> &Arc<Basalt> {
        &self.basalt
    }

    /// Hides and captures cursor.
    pub fn capture_cursor(&self) {
        todo!()
    }

    /// Shows and releases cursor.
    pub fn release_cursor(&self) {
        todo!()
    }

    /// Checks if cursor is currently captured.
    pub fn cursor_captured(&self) -> bool {
        self.cursor_captured.load(atomic::Ordering::SeqCst)
    }

    /// Return a list of active monitors on the system.
    pub fn monitors(&self) -> Vec<Monitor> {
        todo!()
    }

    /// Return the primary monitor if the implementation is able to determine it.
    pub fn primary_monitor(&self) -> Option<Monitor> {
        todo!()
    }

    /// Return the current monitor if the implementation is able to determine it.
    pub fn current_monitor(&self) -> Option<Monitor> {
        todo!()
    }

    /// Enable fullscreen with the provided behavior.
    pub fn enable_fullscreen(&self, behavior: FullScreenBehavior) -> Result<(), FullScreenError> {
        todo!()
    }

    /// Disable fullscreen.
    ///
    /// # Notes
    /// - Does nothing if the window is not fullscreen.
    pub fn disable_fullscreen(&self) {
        todo!()
    }

    /// Toggle fullscreen mode. Uses `FullScreenBehavior::Auto`.
    ///
    /// # Notes
    /// - Does nothing if there are no monitors available.
    pub fn toggle_fullscreen(&self) {
        todo!()
    }

    /// Check if the window is fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        todo!()
    }

    /// Request the monitor to resize to the given dimensions.
    pub fn request_resize(&self, width: u32, height: u32) {
        todo!()
    }

    /// Return the dimensions of the client area of this window.
    pub fn inner_dimensions(&self) -> [u32; 2] {
        todo!()
    }

    /// Return the `WindowType` of this window.
    pub fn window_type(&self) -> WindowType {
        self.window_type
    }

    /// DPI scaling used on this window.
    pub fn dpi_scale_factor(&self) -> f32 {
        todo!()
    }

    /// Current interface scale. This does not include dpi scaling.
    pub fn current_interface_scale(&self) -> f32 {
        todo!()
    }

    /// Current effective interface scale. This includes dpi scaling.
    pub fn effective_interface_scale(&self) -> f32 {
        todo!()
    }

    /// Set the scale of the interface. This does not include dpi scaling.
    pub fn set_interface_scale(&self, set_scale: f32) {
        todo!()
    }

    /// Set the scale of the interface. This includes dpi scaling.
    pub fn set_effective_interface_scale(&self, set_scale: f32) {
        todo!()
    }

    /// Return the `Win32Monitor` used if present.
    pub fn win32_monitor(&self) -> Option<Win32Monitor> {
        todo!()
    }

    pub fn close(&self) {
        todo!()
    }

    pub fn surface(&self) -> Arc<Surface> {
        self.surface.clone()
    }

    pub fn surface_ref(&self) -> &Arc<Surface> {
        &self.surface
    }

    pub fn surface_capabilities(&self, fse: FullScreenExclusive) -> SurfaceCapabilities {
        todo!()

        /*self.physical_device()
        .surface_capabilities(
            &self.surface,
            match fse {
                FullScreenExclusive::ApplicationControlled => {
                    SurfaceInfo {
                        full_screen_exclusive: FullScreenExclusive::ApplicationControlled,
                        win32_monitor: self.window_ref().win32_monitor(),
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
        .unwrap()*/
    }

    pub fn surface_formats(&self, fse: FullScreenExclusive) -> Vec<(VkFormat, VkColorSpace)> {
        todo!()

        /*self.physical_device()
        .surface_formats(
            &self.surface,
            match fse {
                FullScreenExclusive::ApplicationControlled => {
                    SurfaceInfo {
                        full_screen_exclusive: FullScreenExclusive::ApplicationControlled,
                        win32_monitor: self.window_ref().win32_monitor(),
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
        .unwrap()*/
    }

    pub fn surface_present_modes(&self, fse: FullScreenExclusive) -> Vec<PresentMode> {
        todo!()

        /*self.physical_device()
        .surface_present_modes(
            &self.surface,
            match fse {
                FullScreenExclusive::ApplicationControlled => {
                    SurfaceInfo {
                        full_screen_exclusive: FullScreenExclusive::ApplicationControlled,
                        win32_monitor: self.window_ref().win32_monitor(),
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
        .collect()*/
    }

    /// Get the current extent of the surface. In the case current extent is none, the window's
    /// inner dimensions will be used instead.
    pub fn surface_current_extent(&self, fse: FullScreenExclusive) -> [u32; 2] {
        todo!()

        /*self.surface_capabilities(fse)
        .current_extent
        .unwrap_or_else(|| self.window_ref().inner_dimensions())*/
    }

    pub fn associate_hook(&self, hook: InputHookID) {
        todo!()
    }

    pub fn on_press<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl + Send + 'static,
    {
        todo!()
    }

    pub fn on_release<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl + Send + 'static,
    {
        todo!()
    }

    pub fn on_hold<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl
            + Send
            + 'static,
    {
        todo!()
    }

    pub fn on_character<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + 'static,
    {
        todo!()
    }

    pub fn on_enter<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        todo!()
    }

    pub fn on_leave<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        todo!()
    }

    pub fn on_focus<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        todo!()
    }

    pub fn on_focus_lost<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        todo!()
    }

    pub fn on_scroll<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + 'static,
    {
        todo!()
    }

    pub fn on_cursor<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
            + Send
            + 'static,
    {
        todo!()
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
