use std::any::Any;
use std::ops::{Deref, DerefMut};
use std::sync::Weak;
use std::sync::atomic::{self, AtomicBool};
use std::time::Duration;

use cosmic_text::fontdb::Source as FontSource;
use flume::{Receiver, Sender};
use foldhash::{HashMap, HashMapExt};
use parking_lot::{Mutex, MutexGuard};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::input::{
    self, Char, InputHookCtrl, InputHookID, InputHookTarget, KeyCombo, LocalCursorState,
    LocalKeyState,
};
use crate::interface::{Bin, BinID, DefaultFont, UpdateContext};
use crate::interval::IntvlHookID;
use crate::render::{RendererMetricsLevel, RendererPerfMetrics};
use crate::window::backend::BackendWindowHandle;
use crate::window::{
    CreateWindowError, FullScreenBehavior, Monitor, WindowBackend, WindowError, WindowManager,
};
use crate::{Basalt, MSAA, VSync};

mod vko {
    pub use vulkano::format::Format;
    pub use vulkano::swapchain::{
        ColorSpace, FullScreenExclusive, PresentMode, Surface, SurfaceCapabilities, SurfaceInfo,
        Win32Monitor,
    };
}

use std::sync::Arc;

/// An ID that is used to identify a `Window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowID(pub(super) u64);

/// An enum that specifies window type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowType {
    Android,
    Macos,
    Ios,
    Wayland,
    Windows,
    X11,
}

impl WindowType {
    pub(super) fn from_window_handle<H>(hwh: H) -> Result<Self, WindowError>
    where
        H: HasWindowHandle,
    {
        Ok(match hwh.window_handle() {
            Ok(window_handle) => {
                match window_handle.as_raw() {
                    RawWindowHandle::AndroidNdk(_) => WindowType::Android,
                    RawWindowHandle::AppKit(_) => WindowType::Macos,
                    RawWindowHandle::UiKit(_) => WindowType::Ios,
                    RawWindowHandle::Wayland(_) => WindowType::Wayland,
                    RawWindowHandle::Win32(_) => WindowType::Windows,
                    RawWindowHandle::Xcb(_) | RawWindowHandle::Xlib(_) => WindowType::X11,
                    _ => return Err(CreateWindowError::HandleNotSupported.into()),
                }
            },
            Err(..) => return Err(CreateWindowError::HandleUnavailable.into()),
        })
    }
}

/// An enum representing system cursor icons.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorIcon {
    #[default]
    Default,
    ContextMenu,
    Help,
    Pointer,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    VerticalText,
    Alias,
    Copy,
    Move,
    NoDrop,
    NotAllowed,
    Grab,
    Grabbing,
    EResize,
    NResize,
    NeResize,
    NwResize,
    SResize,
    SeResize,
    SwResize,
    WResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ColResize,
    RowResize,
    AllScroll,
    ZoomIn,
    ZoomOut,
    DndAsk,
    AllResize,
}

#[allow(dead_code)] // Not all window backends impl all events
pub(crate) enum WindowEvent {
    Closed,
    Resized { width: u32, height: u32 },
    ScaleChanged(f32),
    RedrawRequested,
    EnabledFullscreen,
    DisabledFullscreen,
    AssociateBin(Arc<Bin>),
    DissociateBin(BinID),
    UpdateBin(BinID),
    UpdateBinBatch(Vec<BinID>),
    AddBinaryFont(Arc<dyn AsRef<[u8]> + Sync + Send>),
    SetDefaultFont(DefaultFont),
    SetMSAA(MSAA),
    SetVSync(VSync),
    SetConsvDraw(bool),
    SetMetrics(RendererMetricsLevel),
    OnFrame(Box<dyn FnMut(Option<Duration>) -> bool + Send>),
}

impl std::fmt::Debug for WindowEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => f.debug_struct("WindowEvent::Closed").finish(),
            Self::Resized {
                width,
                height,
            } => {
                f.debug_struct("WindowEvent::Resized")
                    .field("width", width)
                    .field("height", height)
                    .finish()
            },
            Self::ScaleChanged(scale) => {
                f.debug_tuple("WindowEvent::ScaleChanged")
                    .field(scale)
                    .finish()
            },
            Self::RedrawRequested => f.debug_struct("WindowEvent::RedrawRequested").finish(),
            Self::EnabledFullscreen => f.debug_struct("WindowEvent::EnabledFullscreen").finish(),
            Self::DisabledFullscreen => f.debug_struct("WindowEvent::DisabledFullscreen").finish(),
            Self::AssociateBin(bin_id) => {
                f.debug_tuple("WindowEvent::AssociateBin")
                    .field(bin_id)
                    .finish()
            },
            Self::DissociateBin(bin_id) => {
                f.debug_tuple("WindowEvent::DissociateBin")
                    .field(bin_id)
                    .finish()
            },
            Self::UpdateBin(bin_id) => {
                f.debug_tuple("WindowEvent::UpdateBin")
                    .field(bin_id)
                    .finish()
            },
            Self::UpdateBinBatch(bin_ids) => {
                f.debug_tuple("WindowEvent::UpdateBinBatch")
                    .field(bin_ids)
                    .finish()
            },
            Self::AddBinaryFont(_) => {
                f.debug_tuple("WindowEvent::AddBinaryFont")
                    .finish_non_exhaustive()
            },
            Self::SetDefaultFont(default_font) => {
                f.debug_tuple("WindowEvent::SetDefaultFont")
                    .field(default_font)
                    .finish()
            },
            Self::SetMSAA(msaa) => f.debug_tuple("WindowEvent::SetMSAA").field(msaa).finish(),
            Self::SetVSync(vsync) => f.debug_tuple("WindowEvent::SetVSync").field(vsync).finish(),
            Self::SetConsvDraw(consv_draw) => {
                f.debug_tuple("WindowEvent::SetConsvDraw")
                    .field(consv_draw)
                    .finish()
            },
            Self::SetMetrics(metrics_level) => {
                f.debug_tuple("WindowEvent::SetMetrics")
                    .field(metrics_level)
                    .finish()
            },
            Self::OnFrame(_) => {
                f.debug_tuple("WindowEvent::OnFrame")
                    .finish_non_exhaustive()
            },
        }
    }
}

impl std::fmt::Debug for Window {
    #[allow(unreachable_code)] // Hides warning when no backend is enabled
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Window")
            .field("id", &self.id)
            .field("backend", &self.inner.backend())
            .field("type", &self.window_type)
            .finish_non_exhaustive()
    }
}

/// Object that represents a window.
pub struct Window {
    id: WindowID,
    basalt: Arc<Basalt>,
    inner: Box<dyn BackendWindowHandle>,
    surface: Arc<vko::Surface>,
    window_type: WindowType,
    state: Mutex<WindowState>,
    is_closing: AtomicBool,
    event_send: Sender<WindowEvent>,
    event_recv: Receiver<WindowEvent>,
    event_recv_acquired: AtomicBool,
    shared_update_ctx: Mutex<Option<UpdateContext>>,
}

struct WindowState {
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
    on_close_request_op: Option<Box<dyn FnMut(WindowID) -> bool + Send + Sync + 'static>>,
    attached_input_hooks: Vec<InputHookID>,
    attached_intvl_hooks: Vec<IntvlHookID>,
    keep_alive_objects: Vec<Box<dyn Any + Send + Sync + 'static>>,
}

impl Window {
    pub(super) fn new<W>(
        basalt: Arc<Basalt>,
        id: WindowID,
        inner: W,
    ) -> Result<Arc<Self>, WindowError>
    where
        W: BackendWindowHandle,
    {
        // Note: Calls other than display/window_handle on BackendWindowHandle may deadlock!

        let surface = unsafe { vko::Surface::from_window_ref(basalt.instance(), &inner) }
            .map_err(|e| WindowError::CreateWindow(e.into()))?;

        let window_type = WindowType::from_window_handle(&inner)?;
        let (event_send, event_recv) = flume::unbounded();

        let state = WindowState {
            ignore_dpi: basalt.config.window_ignore_dpi,
            dpi_scale: 1.0, // Note: the backend impl should set this when the window is ready.
            renderer_msaa: basalt.config.render_default_msaa,
            renderer_vsync: basalt.config.render_default_vsync,
            renderer_consv_draw: basalt.config.render_default_consv_draw,
            metrics: RendererPerfMetrics::default(),
            metrics_level: RendererMetricsLevel::None,
            on_metrics_update: Vec::new(),
            on_close_request_op: None,
            interface_scale: basalt.config.window_default_scale,
            associated_bins: HashMap::new(),
            attached_input_hooks: Vec::new(),
            attached_intvl_hooks: Vec::new(),
            keep_alive_objects: Vec::new(),
        };

        Ok(Arc::new(Self {
            id,
            inner: Box::new(inner),
            basalt,
            surface,
            window_type,
            state: Mutex::new(state),
            is_closing: AtomicBool::new(false),
            event_send,
            event_recv,
            event_recv_acquired: AtomicBool::new(false),
            shared_update_ctx: Mutex::new(None),
        }))
    }

    /// Get the [`WindowID`] of this window.
    pub fn id(&self) -> WindowID {
        self.id
    }

    /// Get the [`WindowType`] of this window.
    pub fn ty(&self) -> WindowType {
        self.window_type
    }

    /// Get the [`WindowBackend`] of this window.
    ///
    /// **Note:** The window backend can be configured at runtime with
    /// [`BasaltOptions::window_backend`](crate::BasaltOptions::window_backend).
    #[allow(unreachable_code)] // Hides warning when no backend is enabled
    pub fn backend(&self) -> WindowBackend {
        self.inner.backend()
    }

    /// Obtain a copy of [`Arc<Basalt>`](Basalt)
    pub fn basalt(&self) -> Arc<Basalt> {
        self.basalt.clone()
    }

    /// Obtain a reference of [`Arc<Basalt>`](Basalt)
    pub fn basalt_ref(&self) -> &Arc<Basalt> {
        &self.basalt
    }

    /// Obtain a copy of [`Arc<WindowManager>`](WindowManager)
    pub fn window_manager(&self) -> Arc<WindowManager> {
        self.basalt.window_manager()
    }

    /// Obtain a reference of [`Arc<WindowManager>`](WindowManager)
    pub fn window_manager_ref(&self) -> &Arc<WindowManager> {
        self.basalt.window_manager_ref()
    }

    /// Get the current title of the window.
    ///
    /// - **wayland/layer**: not supported.
    pub fn title(&self) -> Result<String, WindowError> {
        self.inner.title()
    }

    /// Get the title of the window.
    ///
    /// - **wayland/layer**: not supported.
    pub fn set_title<T>(&self, title: T) -> Result<(), WindowError>
    where
        T: Into<String>,
    {
        self.inner.set_title(title.into())
    }

    /// Check if the window is maximized.
    ///
    /// - **wayland/layer**: not supported.
    pub fn maximized(&self) -> Result<bool, WindowError> {
        self.inner.maximized()
    }

    /// Set if the window is maximized.
    ///
    /// - **wayland/layer**: not supported.
    pub fn set_maximized(&self, maximized: bool) -> Result<(), WindowError> {
        self.inner.set_maximized(maximized)
    }

    /// Check if the window is minimized.
    ///
    /// - **winit/wayland**: not supported.
    /// - **wayland/layer**: not supported.
    /// - **wayland/window**: checks if window is suspended.
    pub fn minimized(&self) -> Result<bool, WindowError> {
        self.inner.minimized()
    }

    /// Set if the window is minimized.
    ///
    /// - **winit/wayland**: un-minimize not supported.
    /// - **wayland/layer**: not supported.
    /// - **wayland/window**: un-minimize not supported.
    pub fn set_minimized(&self, minimized: bool) -> Result<(), WindowError> {
        self.inner.set_minimized(minimized)
    }

    /// Get the inner size of the window.
    pub fn size(&self) -> Result<[u32; 2], WindowError> {
        self.inner.size()
    }

    /// Set the inner size of the window.
    ///
    /// - **winit**: will return [`NotSupported`](WindowError::NotSupported) if not supported.
    /// - **wayland/window**: only supported if not a tiling window.
    pub fn set_size(&self, window_size: [u32; 2]) -> Result<(), WindowError> {
        self.inner.set_size(window_size)
    }

    /// Get the minimum inner size of the window.
    ///
    /// - **wayland/layer**: not supported.
    pub fn min_size(&self) -> Result<Option<[u32; 2]>, WindowError> {
        self.inner.min_size()
    }

    /// Set the minimum inner size of the window.
    ///
    /// If `None`, the window will not have a minimum size.
    ///
    /// - **wayland/layer**: not supported.
    pub fn set_min_size(&self, min_size_op: Option<[u32; 2]>) -> Result<(), WindowError> {
        self.inner.set_min_size(min_size_op)
    }

    /// Get the maximum inner size of the window.
    ///
    /// - **wayland/layer**: not supported.
    pub fn max_size(&self) -> Result<Option<[u32; 2]>, WindowError> {
        self.inner.max_size()
    }

    /// Set the maximum inner size of the window.
    ///
    /// If `None`, the window will not have a maximum size.
    ///
    /// - **wayland/layer**: not supported.
    pub fn set_max_size(&self, max_size_op: Option<[u32; 2]>) -> Result<(), WindowError> {
        self.inner.set_max_size(max_size_op)
    }

    /// Get the current [`CursorIcon`] used for the window.
    pub fn cursor_icon(&self) -> Result<CursorIcon, WindowError> {
        self.inner.cursor_icon()
    }

    /// Set the current [`CursorIcon`] use for the window.
    pub fn set_cursor_icon(&self, cursor_icon: CursorIcon) -> Result<(), WindowError> {
        self.inner.set_cursor_icon(cursor_icon)
    }

    /// Check if the cursor is visible.
    pub fn cursor_visible(&self) -> Result<bool, WindowError> {
        self.inner.cursor_visible()
    }

    /// Set if the cursor is visible.
    pub fn set_cursor_visible(&self, visible: bool) -> Result<(), WindowError> {
        self.inner.set_cursor_visible(visible)
    }

    /// Check if the cursor is locked.
    ///
    /// - **winit/x11**: not supported
    pub fn cursor_locked(&self) -> Result<bool, WindowError> {
        self.inner.cursor_locked()
    }

    /// Lock the cursor in-place.
    ///
    /// If cursor is outside the window, cursor will be locked upon entering.
    ///
    /// - **winit/x11**: not implemented
    pub fn set_cursor_locked(&self, locked: bool) -> Result<(), WindowError> {
        self.inner.set_cursor_locked(locked)
    }

    /// Check if the cursor is confined.
    ///
    /// - **winit/macos**: not implemented
    pub fn cursor_confined(&self) -> Result<bool, WindowError> {
        self.inner.cursor_confined()
    }

    /// Confine the cursor to the bounds of the window.
    ///
    /// If cursor is outside the window, cursor will be confined upon entering.
    ///
    /// - **winit/macos**: not implemented
    pub fn set_cursor_confined(&self, confined: bool) -> Result<(), WindowError> {
        self.inner.set_cursor_confined(confined)
    }

    /// Check if the cursor is captured.
    ///
    /// **returns `true` if**: the cursor is not visible and is either locked or confined.
    pub fn cursor_captured(&self) -> Result<bool, WindowError> {
        self.inner.cursor_captured()
    }

    /// Set if the cursor is captured.
    ///
    /// **if `captured` is `true`**:
    /// - makes the cursor hidden.
    /// - locks or confines the cursor depending on backend support.
    ///
    /// **if `captured` is `false`**:
    /// - makes the cursor visible.
    /// - unlocks & unconfines the cursor.
    pub fn set_cursor_captured(&self, captured: bool) -> Result<(), WindowError> {
        self.inner.set_cursor_captured(captured)
    }

    /// Get the [`Monitor`] that the window is on.
    ///
    /// - **winit**: returns [`NotSupported`](WindowError::NotSupported) if unable to determine.
    /// - **wayland**: returns [`Unavailable`](WindowError::Unavailable) if the surface hasn't been shown.
    pub fn monitor(&self) -> Result<Monitor, WindowError> {
        self.inner.monitor()
    }

    /// Check if the window is full screen.
    ///
    /// - **wayland/layer**: not supported.
    pub fn full_screen(&self) -> Result<bool, WindowError> {
        self.inner.full_screen()
    }

    /// Enable full screen for the window.
    ///
    /// Exclusive full screen is currently only supported on windows. Set `fallback_borderless` to
    /// `true` to fallback to borderless when exclusive mode isn't supported or just use borderless.
    /// See [`FullScreenBehavior`] for more info.
    ///
    /// - **wayland/layer**: not supported.
    pub fn enable_full_screen(
        &self,
        fallback_borderless: bool,
        full_screen_behavior: FullScreenBehavior,
    ) -> Result<(), WindowError> {
        self.inner
            .enable_full_screen(fallback_borderless, full_screen_behavior)
    }

    /// Disable full screen for the window.
    ///
    /// - **wayland/layer**: not supported.
    pub fn disable_full_screen(&self) -> Result<(), WindowError> {
        self.inner.disable_full_screen()
    }

    /// Toggles full screen being enabled for the window.
    ///
    /// See [`enable_full_screen`](Self::enable_full_screen) for more info.
    pub fn toggle_fullscreen(
        &self,
        fallback_borderless: bool,
        full_screen_behavior: FullScreenBehavior,
    ) -> Result<(), WindowError> {
        if self.full_screen()? {
            self.disable_full_screen()
        } else {
            self.enable_full_screen(fallback_borderless, full_screen_behavior)
        }
    }

    /// Attempt to obtain a `WlLayerHandle` to the underlying wayland layer.
    ///
    /// Used to get/set layer attributes after the creation of the layer.
    ///
    /// **returns `None` if**: the window backend isn't wayland or the window isn't a layer.
    #[cfg(feature = "wayland_window")]
    pub fn layer_handle(&self) -> Option<crate::window::backend::wayland::WlLayerHandle<'_>> {
        crate::window::backend::wayland::WlLayerHandle::from_window(self)
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

    /// Create a [`Bin`] associated with this window.
    pub fn new_bin(self: &Arc<Self>) -> Arc<Bin> {
        let bin = self.basalt.interface_ref().new_bin();
        bin.associate_window(self);
        bin
    }

    /// Create [`Bin`]'s associated with this window.
    pub fn new_bins(self: &Arc<Self>, count: usize) -> Vec<Arc<Bin>> {
        let bins = self.basalt.interface_ref().new_bins(count);

        for bin in &bins {
            bin.associate_window(self);
        }

        bins
    }

    /// Retrieve a list of [`Bin`]'s associated to this window.
    pub fn associated_bins(&self) -> Vec<Arc<Bin>> {
        self.state
            .lock()
            .associated_bins
            .values()
            .filter_map(|wk| wk.upgrade())
            .collect()
    }

    /// Retrieve a list of [`BinID`]'s associated to this window.
    pub fn associated_bin_ids(&self) -> Vec<BinID> {
        self.state.lock().associated_bins.keys().copied().collect()
    }

    /// Get the current [`MSAA`] used for rendering.
    pub fn renderer_msaa(&self) -> MSAA {
        self.state.lock().renderer_msaa
    }

    /// Set the current [`MSAA`] used for rendering.
    pub fn set_renderer_msaa(&self, msaa: MSAA) {
        self.state.lock().renderer_msaa = msaa;
        self.send_event(WindowEvent::SetMSAA(msaa));
    }

    /// Increase the current [`MSAA`] used for rendering returning the new value.
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

    /// Decrease the current [`MSAA`] used for rendering returning the new value.
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

    /// Get the current [`VSync`] used for rendering.
    pub fn renderer_vsync(&self) -> VSync {
        self.state.lock().renderer_vsync
    }

    /// Set the current [`VSync`] used for rendering.
    pub fn set_renderer_vsync(&self, vsync: VSync) {
        self.state.lock().renderer_vsync = vsync;
        self.send_event(WindowEvent::SetVSync(vsync));
    }

    /// Toggle the current [`VSync`] used returning the new value.
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

    /// Add a callback to the [`Renderer`](crate::render::Renderer) to be called every frame.
    ///
    /// When the callback method returns `false` the callback will be removed.
    pub fn renderer_on_frame<M>(&self, method: M)
    where
        M: FnMut(Option<Duration>) -> bool + Send + 'static,
    {
        self.send_event(WindowEvent::OnFrame(Box::new(method)));
    }

    /// Closes the window.
    ///
    /// **Notes**:
    /// - The window will not close until the window is fully dropped.
    /// - Further use of the window may result in [`WindowError::Closed`] errors.
    pub fn close(&self) {
        self.is_closing.store(true, atomic::Ordering::SeqCst);
        self.basalt.window_manager_ref().window_closed(self.id);
        self.send_event(WindowEvent::Closed);
    }

    /// Check if the window is trying to close.
    pub fn is_closing(&self) -> bool {
        self.is_closing.load(atomic::Ordering::SeqCst)
    }

    /// Add a callback to be called when the window was requested to close.
    ///
    /// This is generally when the user presses the close button the window.
    ///
    /// The provided callback should return `true` if the close request should close the window.
    ///
    /// **Notes**:
    /// - If a callback is not added, the close request will be respected and the window closed.
    /// - Calling `on_close_request` multiple times will remove the previously set callback.
    pub fn on_close_request<E>(&self, exec: E)
    where
        E: FnMut(WindowID) -> bool + Send + Sync + 'static,
    {
        self.state.lock().on_close_request_op = Some(Box::new(exec));
    }

    /// Attach an input hook to this window. When the window closes, this hook will be
    /// automatically removed from `Input`.
    ///
    /// ***Note**: If a hook's target is a window this behavior already occurs without needing to
    /// call this method.*
    pub fn attach_input_hook(&self, hook: InputHookID) {
        self.state.lock().attached_input_hooks.push(hook);
    }

    /// Attach an interval hook to this window. When the window closes, this hook will be
    /// automatically removed from `Interval`.
    pub fn attach_intvl_hook(&self, hook: IntvlHookID) {
        self.state.lock().attached_intvl_hooks.push(hook);
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

    pub(super) fn inner_ref(&self) -> &dyn BackendWindowHandle {
        &*self.inner
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

    #[allow(dead_code)] // TODO: Not all window backends support dpi?
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

    pub(crate) fn set_renderer_consv_draw_nev(&self, enabled: bool) {
        self.state.lock().renderer_consv_draw = enabled;
    }

    pub(crate) fn set_renderer_msaa_nev(&self, msaa: MSAA) {
        self.state.lock().renderer_msaa = msaa;
    }

    pub(crate) fn set_renderer_vsync_nev(&self, vsync: VSync) {
        self.state.lock().renderer_vsync = vsync;
    }

    pub(crate) fn set_renderer_metrics(&self, metrics: RendererPerfMetrics) {
        let mut state = self.state.lock();

        for method in state.on_metrics_update.iter_mut() {
            method(self.id, metrics.clone());
        }

        state.metrics = metrics;
    }

    pub(crate) fn close_requested(&self) {
        if match self.state.lock().on_close_request_op {
            Some(ref mut exec) => exec(self.id),
            None => true,
        } {
            self.close();
        }
    }

    pub(crate) fn surface(&self) -> Arc<vko::Surface> {
        self.surface.clone()
    }

    pub(crate) fn win32_monitor(&self) -> Result<vko::Win32Monitor, WindowError> {
        self.inner.win32_monitor()
    }

    fn surface_info(
        &self,
        fse: vko::FullScreenExclusive,
        mut present_mode: Option<vko::PresentMode>,
    ) -> vko::SurfaceInfo {
        if !self
            .basalt
            .instance_ref()
            .enabled_extensions()
            .ext_surface_maintenance1
        {
            present_mode = None;
        }

        let win32_monitor = if fse == vko::FullScreenExclusive::ApplicationControlled {
            self.inner.win32_monitor().ok()
        } else {
            None
        };

        vko::SurfaceInfo {
            present_mode,
            full_screen_exclusive: fse,
            win32_monitor,
            ..Default::default()
        }
    }

    pub(crate) fn surface_capabilities(
        &self,
        fse: vko::FullScreenExclusive,
        present_mode: vko::PresentMode,
    ) -> vko::SurfaceCapabilities {
        self.basalt
            .physical_device_ref()
            .surface_capabilities(&self.surface, self.surface_info(fse, Some(present_mode)))
            .unwrap()
    }

    pub(crate) fn surface_formats(
        &self,
        fse: vko::FullScreenExclusive,
        present_mode: vko::PresentMode,
    ) -> Vec<(vko::Format, vko::ColorSpace)> {
        self.basalt
            .physical_device_ref()
            .surface_formats(&self.surface, self.surface_info(fse, Some(present_mode)))
            .unwrap()
    }

    pub(crate) fn surface_present_modes(
        &self,
        fse: vko::FullScreenExclusive,
    ) -> Vec<vko::PresentMode> {
        self.basalt
            .physical_device_ref()
            .surface_present_modes(&self.surface, self.surface_info(fse, None))
            .unwrap()
    }

    pub(crate) fn surface_current_extent(
        &self,
        fse: vko::FullScreenExclusive,
        present_mode: vko::PresentMode,
    ) -> Result<[u32; 2], WindowError> {
        match self.surface_capabilities(fse, present_mode).current_extent {
            Some(some) => Ok(some),
            None => self.inner.size(),
        }
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

    pub(crate) fn shared_update_ctx<'a>(self: &'a Arc<Self>) -> SharedUpdateCtx<'a> {
        let mut ctx = SharedUpdateCtx {
            inner: self.shared_update_ctx.lock(),
        };

        ctx.ready(self);
        ctx
    }

    pub(crate) fn send_event(&self, event: WindowEvent) {
        match &event {
            WindowEvent::Resized {
                width,
                height,
            } => {
                if let Some(shared_update_ctx) = self.shared_update_ctx.lock().as_mut() {
                    shared_update_ctx.extent[0] = *width as f32;
                    shared_update_ctx.extent[1] = *height as f32;
                }
            },
            WindowEvent::ScaleChanged(scale) => {
                if let Some(shared_update_ctx) = self.shared_update_ctx.lock().as_mut() {
                    shared_update_ctx.scale = *scale;
                }
            },
            WindowEvent::AddBinaryFont(binary_font) => {
                if let Some(shared_update_ctx) = self.shared_update_ctx.lock().as_mut() {
                    shared_update_ctx
                        .font_system
                        .db_mut()
                        .load_font_source(FontSource::Binary(binary_font.clone()));
                }
            },
            WindowEvent::SetDefaultFont(default_font) => {
                if let Some(shared_update_ctx) = self.shared_update_ctx.lock().as_mut() {
                    shared_update_ctx.default_font = default_font.clone();
                }
            },
            _ => (),
        }

        if self.event_recv_acquired.load(atomic::Ordering::SeqCst) {
            self.event_send.send(event).unwrap();
        }
    }

    pub fn on_press<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &input::WindowState, &LocalKeyState) -> InputHookCtrl
            + Send
            + 'static,
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
        F: FnMut(InputHookTarget, &input::WindowState, &LocalKeyState) -> InputHookCtrl
            + Send
            + 'static,
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
        F: FnMut(InputHookTarget, &input::WindowState, Char) -> InputHookCtrl + Send + 'static,
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
        F: FnMut(InputHookTarget, &input::WindowState) -> InputHookCtrl + Send + 'static,
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
        F: FnMut(InputHookTarget, &input::WindowState) -> InputHookCtrl + Send + 'static,
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
        F: FnMut(InputHookTarget, &input::WindowState) -> InputHookCtrl + Send + 'static,
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
        F: FnMut(InputHookTarget, &input::WindowState) -> InputHookCtrl + Send + 'static,
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

    pub fn on_bin_focus_change<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(&Arc<Self>, &input::WindowState, Option<BinID>) -> InputHookCtrl + Send + 'static,
    {
        self.basalt()
            .input_ref()
            .hook()
            .window(self)
            .on_bin_focus_change()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_scroll<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &input::WindowState, f32, f32) -> InputHookCtrl + Send + 'static,
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
        F: FnMut(InputHookTarget, &input::WindowState, &LocalCursorState) -> InputHookCtrl
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
        let mut state = self.state.lock();

        for hook_id in state.attached_input_hooks.drain(..) {
            self.basalt.input_ref().remove_hook(hook_id);
        }

        for hook_id in state.attached_intvl_hooks.drain(..) {
            self.basalt.interval_ref().remove(hook_id);
        }
    }
}

pub(crate) struct SharedUpdateCtx<'a> {
    inner: MutexGuard<'a, Option<UpdateContext>>,
}

impl SharedUpdateCtx<'_> {
    fn ready(&mut self, window: &Arc<Window>) {
        if self.inner.is_none() {
            *self.inner = Some(UpdateContext::from(window));
        }

        self.inner.as_mut().unwrap().placement_cache.clear();
    }
}

impl Deref for SharedUpdateCtx<'_> {
    type Target = UpdateContext;

    fn deref(&self) -> &Self::Target {
        (*self.inner).as_ref().unwrap()
    }
}

impl DerefMut for SharedUpdateCtx<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        (*self.inner).as_mut().unwrap()
    }
}
