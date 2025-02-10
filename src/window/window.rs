use std::any::Any;
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Weak};
use std::time::Duration;

use flume::{Receiver, Sender};
use foldhash::{HashMap, HashMapExt};
use parking_lot::Mutex;
use raw_window_handle::{
    DisplayHandle, HandleError as RwhHandleError, HasDisplayHandle, HasWindowHandle,
    RawWindowHandle, WindowHandle,
};

use crate::input::{
    Char, InputEvent, InputHookCtrl, InputHookID, InputHookTarget, KeyCombo, LocalCursorState,
    LocalKeyState, WindowState,
};
use crate::interface::{Bin, BinID};
use crate::render::{RendererMetricsLevel, RendererPerfMetrics, VSync, MSAA};
use crate::window::monitor::{FullScreenBehavior, FullScreenError, Monitor};
use crate::window::{WMEvent, WindowEvent, WindowID, WindowManager, WindowType};
use crate::Basalt;

mod winit {
    pub use winit::dpi::PhysicalSize;
    #[allow(unused_imports)]
    pub use winit::platform;
    pub use winit::window::{CursorGrabMode, Window, WindowId};
}

mod vk {
    pub use vulkano::format::Format;
    pub use vulkano::swapchain::{
        ColorSpace, FullScreenExclusive, PresentMode, Surface, SurfaceCapabilities, SurfaceInfo,
        Win32Monitor,
    };
}

/// Object that represents a window.
///
/// This object is generally passed around as it allows accessing mosts things within the crate.
pub struct Window {
    id: WindowID,
    inner: Arc<winit::Window>,
    basalt: Arc<Basalt>,
    wm: Arc<WindowManager>,
    surface: Arc<vk::Surface>,
    window_type: WindowType,
    state: Mutex<State>,
    close_requested: AtomicBool,
    event_send: Sender<WindowEvent>,
    event_recv: Receiver<WindowEvent>,
    event_recv_acquired: AtomicBool,
}

struct State {
    cursor_captured: bool,
    ignore_dpi: bool,
    dpi_scale: f32,
    interface_scale: f32,
    renderer_msaa: MSAA,
    renderer_vsync: VSync,
    renderer_consv_draw: bool,
    metrics: RendererPerfMetrics,
    metrics_level: RendererMetricsLevel,
    on_metrics_update: Vec<Box<dyn FnMut(WindowID, RendererPerfMetrics) + Send + Sync + 'static>>,
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
        winit: Arc<winit::Window>,
    ) -> Result<Arc<Self>, String> {
        // NOTE: Although it may seem the winit window doesn't need to be in an Arc. This allows
        //       vulkano to keep the window alive longer than the surface. It may be possible to
        //       pass the basalt window instead, but that'd likey mean keeping surface in a mutex.

        let surface = vk::Surface::from_window(basalt.instance(), winit.clone())
            .map_err(|e| format!("Failed to create surface: {}", e))?;

        let window_type = match winit.window_handle() {
            Ok(window_handle) => {
                match window_handle.as_raw() {
                    RawWindowHandle::AndroidNdk(_) => WindowType::Android,
                    RawWindowHandle::AppKit(_) => WindowType::Macos,
                    RawWindowHandle::UiKit(_) => WindowType::Ios,
                    RawWindowHandle::Wayland(_) => WindowType::Wayland,
                    RawWindowHandle::Win32(_) => WindowType::Windows,
                    RawWindowHandle::Xcb(_) => WindowType::Xcb,
                    RawWindowHandle::Xlib(_) => WindowType::Xlib,
                    raw_window_handle => {
                        return Err(format!(
                            "Unsupported window handle type: {:?}",
                            raw_window_handle
                        ));
                    },
                }
            },
            Err(handle_err) => {
                return Err(format!("Window handle error: {}", handle_err));
            },
        };

        let (ignore_dpi, dpi_scale) = match basalt.config.window_ignore_dpi {
            true => (true, 1.0),
            false => (false, winit.scale_factor() as f32),
        };

        let (event_send, event_recv) = flume::unbounded();

        let state = State {
            cursor_captured: false,
            ignore_dpi,
            dpi_scale,
            renderer_msaa: basalt.config.render_default_msaa,
            renderer_vsync: basalt.config.render_default_vsync,
            renderer_consv_draw: basalt.config.render_default_consv_draw,
            metrics: RendererPerfMetrics::default(),
            metrics_level: RendererMetricsLevel::None,
            on_metrics_update: Vec::new(),
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
            event_send,
            event_recv,
            event_recv_acquired: AtomicBool::new(false),
        }))
    }

    pub(crate) fn winit_id(&self) -> winit::WindowId {
        self.inner.id()
    }

    pub(crate) fn associate_bin(&self, bin: Arc<Bin>) {
        self.state
            .lock()
            .associated_bins
            .insert(bin.id(), Arc::downgrade(&bin));

        self.send_event(WindowEvent::AssociateBin(bin));
    }

    pub(crate) fn dissociate_bin(&self, bin_id: BinID) {
        self.state.lock().associated_bins.remove(&bin_id);
        self.send_event(WindowEvent::DissociateBin(bin_id));
    }

    pub(crate) fn update_bin(&self, bin_id: BinID) {
        self.send_event(WindowEvent::UpdateBin(bin_id));
    }

    pub(crate) fn update_bin_batch(&self, bin_ids: Vec<BinID>) {
        self.send_event(WindowEvent::UpdateBinBatch(bin_ids));
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
    pub fn surface(&self) -> Arc<vk::Surface> {
        self.surface.clone()
    }

    /// Obtain a reference of `Arc<Surface>`
    pub fn surface_ref(&self) -> &Arc<vk::Surface> {
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
            .set_cursor_grab(winit::CursorGrabMode::Confined)
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
        self.inner
            .set_cursor_grab(winit::CursorGrabMode::None)
            .unwrap();

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
        self.send_event(WindowEvent::EnabledFullscreen);
        Ok(())
    }

    /// Disable fullscreen.
    ///
    /// ***Note:** This is a no-op if this window isn't fullscreen.*
    pub fn disable_fullscreen(&self) {
        if self.inner.fullscreen().is_some() {
            self.inner.set_fullscreen(None);
            self.send_event(WindowEvent::DisabledFullscreen);
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

        let request_size = winit::PhysicalSize::new(width, height);
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

                    self.send_event(WindowEvent::Resized {
                        width,
                        height,
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

        self.send_event(WindowEvent::ScaleChanged(
            state.interface_scale * state.dpi_scale,
        ));
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

        self.send_event(WindowEvent::ScaleChanged(
            state.interface_scale * state.dpi_scale,
        ));
    }

    /// Set the scale of the interface. This includes dpi scaling.
    pub fn set_effective_interface_scale(&self, set_scale: f32) {
        let mut state = self.state.lock();
        state.interface_scale = set_scale / state.dpi_scale;

        self.send_event(WindowEvent::ScaleChanged(
            state.interface_scale * state.dpi_scale,
        ));
    }

    /// Get the current MSAA used for rendering.
    pub fn renderer_msaa(&self) -> MSAA {
        self.state.lock().renderer_msaa
    }

    /// Set the current MSAA used for rendering.
    pub fn set_renderer_msaa(&self, msaa: MSAA) {
        self.state.lock().renderer_msaa = msaa;
        self.send_event(WindowEvent::SetMSAA(msaa));
    }

    pub(crate) fn set_renderer_msaa_nev(&self, msaa: MSAA) {
        self.state.lock().renderer_msaa = msaa;
    }

    /// Increase the current MSAA used for rendering returning the new value.
    pub fn incr_renderer_msaa(&self) -> MSAA {
        let mut state = self.state.lock();

        let msaa = match state.renderer_msaa {
            MSAA::X1 => MSAA::X2,
            MSAA::X2 => MSAA::X4,
            MSAA::X4 => MSAA::X8,
            MSAA::X8 => return MSAA::X8,
        };

        self.send_event(WindowEvent::SetMSAA(msaa));
        state.renderer_msaa = msaa;
        msaa
    }

    /// Decrease the current MSAA used for rendering returning the new value.
    pub fn decr_renderer_msaa(&self) -> MSAA {
        let mut state = self.state.lock();

        let msaa = match state.renderer_msaa {
            MSAA::X1 => return MSAA::X1,
            MSAA::X2 => MSAA::X1,
            MSAA::X4 => MSAA::X2,
            MSAA::X8 => MSAA::X4,
        };

        self.send_event(WindowEvent::SetMSAA(msaa));
        state.renderer_msaa = msaa;
        msaa
    }

    /// Get the current VSync used for rendering.
    pub fn renderer_vsync(&self) -> VSync {
        self.state.lock().renderer_vsync
    }

    /// Set the current VSync used for rendering.
    pub fn set_renderer_vsync(&self, vsync: VSync) {
        self.state.lock().renderer_vsync = vsync;
        self.send_event(WindowEvent::SetVSync(vsync));
    }

    pub(crate) fn set_renderer_vsync_nev(&self, vsync: VSync) {
        self.state.lock().renderer_vsync = vsync;
    }

    /// Toggle the current VSync used returning the new value.
    pub fn toggle_renderer_vsync(&self) -> VSync {
        let mut state = self.state.lock();

        let vsync = match state.renderer_vsync {
            VSync::Enable => VSync::Disable,
            VSync::Disable => VSync::Enable,
        };

        self.send_event(WindowEvent::SetVSync(vsync));
        state.renderer_vsync = vsync;
        vsync
    }

    /// If conservative draw is currently enabled.
    pub fn renderer_consv_draw(&self) -> bool {
        self.state.lock().renderer_consv_draw
    }

    /// Set if conservative draw is enabled.
    pub fn set_renderer_consv_draw(&self, enabled: bool) {
        self.state.lock().renderer_consv_draw = enabled;
        self.send_event(WindowEvent::SetConsvDraw(enabled));
    }

    pub(crate) fn set_renderer_consv_draw_nev(&self, enabled: bool) {
        self.state.lock().renderer_consv_draw = enabled;
    }

    /// Toggle if conservative draw is enabled returning if it is enabled.
    pub fn toggle_renderer_consv_draw(&self) -> bool {
        let mut state = self.state.lock();
        state.renderer_consv_draw = !state.renderer_consv_draw;
        self.send_event(WindowEvent::SetConsvDraw(state.renderer_consv_draw));
        state.renderer_consv_draw
    }

    /// Request the renderer to redraw.
    ///
    /// This is primary intended for user renderers that use conservative draw.
    ///
    /// ***Note:** If not using conservative draw, this is effectively a no-op.*
    pub fn renderer_request_redraw(&self) {
        self.send_event(WindowEvent::RedrawRequested);
    }

    /// Get the current renderer metrics level used.
    pub fn renderer_metrics_level(&self) -> RendererMetricsLevel {
        self.state.lock().metrics_level
    }

    /// Set the current renderer metrics level used.
    pub fn set_renderer_metrics_level(&self, level: RendererMetricsLevel) {
        self.state.lock().metrics_level = level;
        self.send_event(WindowEvent::SetMetrics(level));
    }

    /// Cycle between renderer metrics level returning the new current level.
    pub fn cycle_renderer_metrics_level(&self) -> RendererMetricsLevel {
        let mut state = self.state.lock();

        state.metrics_level = match state.metrics_level {
            RendererMetricsLevel::None => RendererMetricsLevel::Basic,
            RendererMetricsLevel::Basic => RendererMetricsLevel::Extended,
            RendererMetricsLevel::Extended => RendererMetricsLevel::Full,
            RendererMetricsLevel::Full => RendererMetricsLevel::None,
        };

        self.send_event(WindowEvent::SetMetrics(state.metrics_level));
        state.metrics_level
    }

    /// Retrieve the current renderer metrics.
    ///
    /// ***Note:** If renderer metrics are disabled, this value will not be updated.*
    pub fn renderer_metrics(&self) -> RendererPerfMetrics {
        self.state.lock().metrics.clone()
    }

    /// When the renderer metrics are updated call the provided method.
    ///
    /// ***Note:** This method will be kept for the lifetime of the window.*
    pub fn on_renderer_metrics<F: FnMut(WindowID, RendererPerfMetrics) + Send + Sync + 'static>(
        &self,
        method: F,
    ) {
        self.state.lock().on_metrics_update.push(Box::new(method));
    }

    pub(crate) fn set_renderer_metrics(&self, metrics: RendererPerfMetrics) {
        let mut state = self.state.lock();

        for method in state.on_metrics_update.iter_mut() {
            method(self.id, metrics.clone());
        }

        state.metrics = metrics;
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
        self.send_event(WindowEvent::Closed);
        self.wm.send_event(WMEvent::CloseWindow(self.id));
    }

    /// Check if a close has been requested.
    pub fn close_requested(&self) -> bool {
        self.close_requested.load(atomic::Ordering::SeqCst)
    }

    /// Return the `Win32Monitor` used if present.
    pub(crate) fn win32_monitor(&self) -> Option<vk::Win32Monitor> {
        #[cfg(target_os = "windows")]
        unsafe {
            use winit::platform::windows::MonitorHandleExtWindows;

            self.inner
                .current_monitor()
                .map(|m| vk::Win32Monitor::new(m.hmonitor()))
        }

        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }

    fn surface_info(
        &self,
        fse: vk::FullScreenExclusive,
        mut present_mode: Option<vk::PresentMode>,
    ) -> vk::SurfaceInfo {
        if !self
            .basalt
            .instance_ref()
            .enabled_extensions()
            .ext_surface_maintenance1
        {
            present_mode = None;
        }

        let win32_monitor = if fse == vk::FullScreenExclusive::ApplicationControlled {
            self.win32_monitor()
        } else {
            None
        };

        vk::SurfaceInfo {
            present_mode,
            full_screen_exclusive: fse,
            win32_monitor,
            ..Default::default()
        }
    }

    pub(crate) fn surface_capabilities(
        &self,
        fse: vk::FullScreenExclusive,
        present_mode: vk::PresentMode,
    ) -> vk::SurfaceCapabilities {
        self.basalt
            .physical_device_ref()
            .surface_capabilities(&self.surface, self.surface_info(fse, Some(present_mode)))
            .unwrap()
    }

    pub(crate) fn surface_formats(
        &self,
        fse: vk::FullScreenExclusive,
        present_mode: vk::PresentMode,
    ) -> Vec<(vk::Format, vk::ColorSpace)> {
        self.basalt
            .physical_device_ref()
            .surface_formats(&self.surface, self.surface_info(fse, Some(present_mode)))
            .unwrap()
    }

    pub(crate) fn surface_present_modes(
        &self,
        fse: vk::FullScreenExclusive,
    ) -> Vec<vk::PresentMode> {
        self.basalt
            .physical_device_ref()
            .surface_present_modes(&self.surface, self.surface_info(fse, None))
            .unwrap()
    }

    pub(crate) fn surface_current_extent(
        &self,
        fse: vk::FullScreenExclusive,
        present_mode: vk::PresentMode,
    ) -> [u32; 2] {
        self.surface_capabilities(fse, present_mode)
            .current_extent
            .unwrap_or_else(|| self.inner_dimensions())
    }

    pub(crate) fn event_queue(&self) -> Option<Receiver<WindowEvent>> {
        if self
            .event_recv_acquired
            .swap(true, atomic::Ordering::SeqCst)
        {
            None
        } else {
            Some(self.event_recv.clone())
        }
    }

    pub(crate) fn release_event_queue(&self) {
        self.event_recv_acquired
            .store(false, atomic::Ordering::SeqCst);
    }

    pub(crate) fn send_event(&self, event: WindowEvent) {
        if self.event_recv_acquired.load(atomic::Ordering::SeqCst) {
            self.event_send.send(event).unwrap();
        }
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

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle, RwhHandleError> {
        self.inner.window_handle()
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle, RwhHandleError> {
        self.inner.display_handle()
    }
}
