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
    SurfaceInfo, Win32Monitor,
};
use winit::dpi::PhysicalSize;
use winit::window::{
    CursorGrabMode, Fullscreen as WinitFullscreen, Window as WinitWindow, WindowId as WinitWindowId,
};

use crate::input::key::KeyCombo;
use crate::input::state::{LocalCursorState, LocalKeyState, WindowState};
use crate::input::{Char, InputEvent, InputHookCtrl, InputHookID, InputHookTarget};
use crate::interface::bin::Bin;
use crate::window::monitor::{FullScreenBehavior, FullScreenError, Monitor};
use crate::window::{WMEvent, WindowEvent, WindowID, WindowManager, WindowType};
use crate::Basalt;

pub struct Window {
    id: WindowID,
    inner: Arc<WinitWindow>,
    basalt: Arc<Basalt>,
    wm: Arc<WindowManager>,
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

        Ok(Arc::new(Self {
            id,
            inner: winit,
            basalt,
            wm,
            surface,
            window_type,
            cursor_captured: AtomicBool::new(false),
            associated_hooks: Mutex::new(Vec::new()),
        }))
    }

    pub(crate) fn winit_id(&self) -> WinitWindowId {
        self.inner.id()
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

    /// Hides and captures cursor.
    pub fn capture_cursor(&self) {
        self.inner.set_cursor_visible(false);
        self.inner
            .set_cursor_grab(CursorGrabMode::Confined)
            .unwrap();
        self.cursor_captured.store(true, atomic::Ordering::SeqCst);

        self.basalt
            .input_ref()
            .send_event(InputEvent::CursorCapture {
                win: self.id,
                captured: true,
            });
    }

    /// Shows and releases cursor.
    pub fn release_cursor(&self) {
        self.inner.set_cursor_visible(true);
        self.inner.set_cursor_grab(CursorGrabMode::None).unwrap();
        self.cursor_captured.store(false, atomic::Ordering::SeqCst);

        self.basalt
            .input_ref()
            .send_event(InputEvent::CursorCapture {
                win: self.id,
                captured: false,
            });
    }

    /// Checks if cursor is currently captured.
    pub fn cursor_captured(&self) -> bool {
        self.cursor_captured.load(atomic::Ordering::SeqCst)
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
    pub fn enable_fullscreen(
        &self,
        mut behavior: FullScreenBehavior,
    ) -> Result<(), FullScreenError> {
        let exclusive_supported = self.basalt.options_ref().exclusive_fullscreen;

        if behavior == FullScreenBehavior::Auto {
            if exclusive_supported {
                behavior = FullScreenBehavior::AutoExclusive;
            } else {
                behavior = FullScreenBehavior::AutoBorderless;
            }
        }

        if behavior.is_exclusive() && !exclusive_supported {
            return Err(FullScreenError::ExclusiveNotSupported);
        }

        if behavior.is_exclusive() {
            let (monitor, mode) = match behavior {
                FullScreenBehavior::AutoExclusive => {
                    let monitor = match self.current_monitor() {
                        Some(some) => some,
                        None => {
                            match self.primary_monitor() {
                                Some(some) => some,
                                None => {
                                    match self.monitors().drain(0..1).next() {
                                        Some(some) => some,
                                        None => return Err(FullScreenError::NoAvailableMonitors),
                                    }
                                },
                            }
                        },
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::AutoExclusivePrimary => {
                    let monitor = match self.primary_monitor() {
                        Some(some) => some,
                        None => return Err(FullScreenError::UnableToDeterminePrimary),
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::AutoExclusiveCurrent => {
                    let monitor = match self.current_monitor() {
                        Some(some) => some,
                        None => return Err(FullScreenError::UnableToDetermineCurrent),
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::ExclusiveAutoMode(monitor) => {
                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::Exclusive(monitor, mode) => (monitor, mode),
                _ => unreachable!(),
            };

            if mode.monitor_handle != monitor.handle {
                return Err(FullScreenError::IncompatibleMonitorMode);
            }

            self.inner
                .set_fullscreen(Some(WinitFullscreen::Exclusive(mode.handle)));
        } else {
            let monitor_op = match behavior {
                FullScreenBehavior::AutoBorderless => {
                    match self.current_monitor() {
                        Some(some) => Some(some),
                        None => self.primary_monitor(),
                    }
                },
                FullScreenBehavior::AutoBorderlessPrimary => {
                    match self.primary_monitor() {
                        Some(some) => Some(some),
                        None => return Err(FullScreenError::UnableToDeterminePrimary),
                    }
                },
                FullScreenBehavior::AutoBorderlessCurrent => {
                    match self.current_monitor() {
                        Some(some) => Some(some),
                        None => return Err(FullScreenError::UnableToDetermineCurrent),
                    }
                },
                FullScreenBehavior::Borderless(monitor) => Some(monitor),
                _ => unreachable!(),
            };

            self.inner.set_fullscreen(Some(WinitFullscreen::Borderless(
                monitor_op.map(|monitor| monitor.handle),
            )));
        }

        self.wm.send_event(WMEvent::WindowEvent {
            id: self.id,
            event: WindowEvent::EnabledFullscreen,
        });
        Ok(())
    }

    /// Disable fullscreen.
    ///
    /// # Notes
    /// - Does nothing if the window is not fullscreen.
    pub fn disable_fullscreen(&self) {
        self.inner.set_fullscreen(None);
        self.wm.send_event(WMEvent::WindowEvent {
            id: self.id,
            event: WindowEvent::DisabledFullscreen,
        });
    }

    /// Toggle fullscreen mode. Uses `FullScreenBehavior::Auto`.
    ///
    /// # Notes
    /// - Does nothing if there are no monitors available.
    pub fn toggle_fullscreen(&self) {
        if self.is_fullscreen() {
            let _ = self.enable_fullscreen(Default::default());
        } else {
            self.disable_fullscreen();
        }
    }

    /// Check if the window is fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        self.inner.fullscreen().is_some()
    }

    /// Request the monitor to resize to the given dimensions.
    ///
    /// # Notes
    /// - Returns `false` if the platform doesn't support resize.
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

                    self.wm.send_event(WMEvent::WindowEvent {
                        id: self.id,
                        event: WindowEvent::Resized {
                            width,
                            height,
                        },
                    });
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
    pub fn dpi_scale_factor(&self) -> f32 {
        self.inner.scale_factor() as f32
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

    /// Request the window to close.
    pub fn close(&self) {
        todo!()
    }

    /// Helper function to retrieve the surface capabilities for this window's surface.
    pub fn surface_capabilities(&self, fse: FullScreenExclusive) -> SurfaceCapabilities {
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

    /// Helper function to retrieve the supported `Format` / `Colorspace` pairs for this window's surface.
    pub fn surface_formats(&self, fse: FullScreenExclusive) -> Vec<(VkFormat, VkColorSpace)> {
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

    /// Helper function to retrieve the supported `PresentMode`'s for this window's surface.
    pub fn surface_present_modes(&self, fse: FullScreenExclusive) -> Vec<PresentMode> {
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

    /// Get the current extent of the surface. In the case current extent is none, the window's
    /// inner dimensions will be used instead.
    pub fn surface_current_extent(&self, fse: FullScreenExclusive) -> [u32; 2] {
        self.surface_capabilities(fse)
            .current_extent
            .unwrap_or_else(|| self.inner_dimensions())
    }

    pub fn associate_hook(&self, hook: InputHookID) {
        todo!()
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
