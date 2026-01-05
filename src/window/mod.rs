//! Window related objects

mod backend;
mod builder;
mod error;
mod monitor;
mod window;

use std::sync::Arc;

use foldhash::{HashMap, HashMapExt};
use parking_lot::{FairMutex, FairMutexGuard, Mutex};

use self::backend::BackendHandle;
pub use self::backend::WindowBackend;
#[cfg(feature = "wayland_window")]
pub use self::backend::wayland::WlLayerHandle;
pub use self::builder::WindowBuilder;
pub use self::error::{CreateWindowError, EnableFullScreenError, WindowError};
pub use self::monitor::{FullScreenBehavior, Monitor, MonitorMode};
pub(crate) use self::window::WindowEvent;
pub use self::window::{CursorIcon, Window, WindowID, WindowType};
use crate::Basalt;
use crate::interface::DefaultFont;
use crate::window::builder::WindowAttributes;
#[cfg(feature = "wayland_window")]
pub use crate::window::builder::wl_layer::{
    WlLayerAnchor, WlLayerBuilder, WlLayerDepth, WlLayerKeyboardFocus,
};

/// An ID that is used to identify a hook on `WindowManager`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WMHookID(u64);

pub(crate) struct DrawGuard<'a> {
    #[allow(dead_code)]
    inner: FairMutexGuard<'a, ()>,
}

pub(crate) struct WMConfig {
    pub window_backend: WindowBackend,
    #[allow(dead_code)]
    pub winit_force_x11: bool,
}

/// Manages windows and their associated events.
pub struct WindowManager {
    backend: Box<dyn BackendHandle + Send + Sync>,
    state: Mutex<WindowManagerState>,
    draw_lock: FairMutex<()>,
}

struct WindowManagerState {
    basalt_op: Option<Arc<Basalt>>,
    windows: HashMap<WindowID, Arc<Window>>,
    on_open: HashMap<WMHookID, Box<dyn FnMut(Arc<Window>) + Send + Sync + 'static>>,
    on_close: HashMap<WMHookID, Box<dyn FnMut(WindowID) + Send + Sync + 'static>>,
    next_hook_id: u64,
    next_window_id: u64,
}

impl WindowManager {
    pub(crate) fn run<F>(config: WMConfig, exec: F)
    where
        F: FnOnce(Arc<Self>) + Send + 'static,
    {
        backend::run(config, move |backend| {
            exec(Arc::new(Self {
                backend,
                state: Mutex::new(WindowManagerState {
                    basalt_op: None,
                    windows: HashMap::new(),
                    on_open: HashMap::new(),
                    on_close: HashMap::new(),
                    next_hook_id: 1,
                    next_window_id: 1,
                }),
                draw_lock: FairMutex::new(()),
            }))
        });
    }

    /// Obtain a [`WindowBuilder`] to begin building a window.
    #[allow(unreachable_code)] // Hides warning when no backend is enabled
    pub fn create(&self) -> WindowBuilder {
        let basalt = self
            .state
            .lock()
            .basalt_op
            .as_ref()
            .cloned()
            .expect("unreachable");

        WindowBuilder::new(basalt, self.backend.window_backend())
    }

    /// Obtain a [`WlLayerBuilder`] to begin building a layer.
    ///
    /// This uses the `wlr_layer_shell` extension and not all compositors support it.
    ///
    /// See compositor support see: [wlr-layer-shell-unstable-v1#compositor-support](https://wayland.app/protocols/wlr-layer-shell-unstable-v1#compositor-support).
    #[cfg(feature = "wayland_window")]
    pub fn create_layer(&self) -> Result<WlLayerBuilder, WindowError> {
        if self.backend.window_backend() != WindowBackend::Wayland {
            return Err(WindowError::NotSupported);
        }

        let basalt = self
            .state
            .lock()
            .basalt_op
            .as_ref()
            .cloned()
            .expect("unreachable");

        Ok(WlLayerBuilder::new(basalt))
    }

    /// Retrieves an [`Arc<Window>`](Window) given a [`WindowID`].
    ///
    /// **returns `None` if**: The window was closed or requested to be closed.
    pub fn window(&self, window_id: WindowID) -> Option<Arc<Window>> {
        self.state.lock().windows.get(&window_id).cloned()
    }

    /// Retrieves all [`Arc<Window>`](Window)'s.
    ///
    /// **Note**: Windows that are closed will be absent.
    pub fn windows(&self) -> Vec<Arc<Window>> {
        self.state.lock().windows.values().cloned().collect()
    }

    /// Return a list of active monitors on the system.
    pub fn monitors(&self) -> Result<Vec<Monitor>, WindowError> {
        self.backend.get_monitors()
    }

    /// Return the primary monitor if the implementation is able to determine it.
    pub fn primary_monitor(&self) -> Result<Monitor, WindowError> {
        self.backend.get_primary_monitor()
    }

    /// Create a hook that is called whenever a window is opened.
    pub fn on_open<F: FnMut(Arc<Window>) + Send + Sync + 'static>(&self, on_open: F) -> WMHookID {
        let mut state = self.state.lock();
        let hook_id = WMHookID(state.next_hook_id);
        state.next_hook_id += 1;
        state.on_open.insert(hook_id, Box::new(on_open));
        hook_id
    }

    /// Create a hook that is called whenever a window is closed.
    pub fn on_close<F: FnMut(WindowID) + Send + Sync + 'static>(&self, on_close: F) -> WMHookID {
        let mut state = self.state.lock();
        let hook_id = WMHookID(state.next_hook_id);
        state.next_hook_id += 1;
        state.on_close.insert(hook_id, Box::new(on_close));
        hook_id
    }

    /// Remove a hook given a [`WMHookID`].
    pub fn remove_hook(&self, hook_id: WMHookID) {
        let mut state = self.state.lock();
        state.on_open.remove(&hook_id);
        state.on_close.remove(&hook_id);
    }

    pub(crate) fn associate_basalt(&self, basalt: Arc<Basalt>) {
        self.state.lock().basalt_op = Some(basalt.clone());
        self.backend.associate_basalt(basalt);
    }

    pub(crate) fn request_draw(&self) -> DrawGuard<'_> {
        DrawGuard {
            inner: self.draw_lock.lock(),
        }
    }

    pub(crate) fn add_binary_font(&self, binary_font: Arc<dyn AsRef<[u8]> + Sync + Send>) {
        let state = self.state.lock();

        for window in state.windows.values() {
            window.send_event(WindowEvent::AddBinaryFont(binary_font.clone()));
        }
    }

    pub(crate) fn set_default_font(&self, default_font: DefaultFont) {
        let state = self.state.lock();

        for window in state.windows.values() {
            window.send_event(WindowEvent::SetDefaultFont(default_font.clone()));
        }
    }

    fn window_closed(&self, window_id: WindowID) {
        let mut state = self.state.lock();
        state.windows.remove(&window_id);

        for callback in state.on_close.values_mut() {
            (*callback)(window_id);
        }
    }

    fn window_created(&self, window: Arc<Window>) {
        let mut state = self.state.lock();
        state.windows.insert(window.id(), window.clone());

        for callback in state.on_open.values_mut() {
            callback(window.clone());
        }
    }

    fn next_window_id(&self) -> WindowID {
        let mut state = self.state.lock();
        let window_id = WindowID(state.next_window_id);
        state.next_window_id += 1;
        window_id
    }

    fn create_window(
        &self,
        window_attributes: WindowAttributes,
    ) -> Result<Arc<Window>, WindowError> {
        self.backend
            .create_window(self.next_window_id(), window_attributes)
    }

    pub(crate) fn exit(&self) {
        self.backend.exit();
    }
}
