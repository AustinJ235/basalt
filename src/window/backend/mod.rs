#[cfg(all(not(feature = "winit_window"), not(feature = "wayland_window")))]
compile_error!("At least one window backend feature must be enabled.");

use std::sync::Arc;

use parking_lot::{Condvar, Mutex};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::Basalt;
use crate::window::builder::WindowAttributes;
use crate::window::{FullScreenBehavior, Monitor, WMConfig, Window, WindowError, WindowID};

mod vko {
    pub use vulkano::swapchain::Win32Monitor;
}

#[cfg(feature = "wayland_window")]
pub mod wayland;
#[cfg(feature = "winit_window")]
pub mod winit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowBackend {
    #[cfg(feature = "winit_window")]
    Winit,
    #[cfg(feature = "wayland_window")]
    Wayland,
}

impl WindowBackend {
    pub fn auto() -> Self {
        #[cfg(feature = "winit_window")]
        {
            Self::Winit
        }
        #[cfg(all(feature = "wayland_window", not(feature = "winit_window")))]
        {
            Self::Wayland
        }
        #[cfg(all(not(feature = "winit_window"), not(feature = "wayland_window")))]
        {
            unreachable!()
        }
    }
}

pub trait BackendHandle {
    fn window_backend(&self) -> WindowBackend;
    fn associate_basalt(&self, basalt: Arc<Basalt>);

    fn create_window(
        &self,
        window_id: WindowID,
        builder: WindowAttributes,
    ) -> Result<Arc<Window>, WindowError>;

    fn get_monitors(&self) -> Result<Vec<Monitor>, WindowError>;
    fn get_primary_monitor(&self) -> Result<Monitor, WindowError>;
    fn exit(&self);
}

pub trait BackendWindowHandle: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {
    fn resize(&self, window_size: [u32; 2]) -> Result<(), WindowError>;
    fn inner_size(&self) -> Result<[u32; 2], WindowError>;

    fn backend(&self) -> WindowBackend;
    fn win32_monitor(&self) -> Result<vko::Win32Monitor, WindowError>;

    fn capture_cursor(&self) -> Result<(), WindowError>;
    fn release_cursor(&self) -> Result<(), WindowError>;
    fn cursor_captured(&self) -> Result<bool, WindowError>;

    fn current_monitor(&self) -> Result<Monitor, WindowError>;

    fn enable_fullscreen(
        &self,
        borderless_fallback: bool,
        behavior: FullScreenBehavior,
    ) -> Result<(), WindowError>;

    fn disable_fullscreen(&self) -> Result<(), WindowError>;
    fn toggle_fullscreen(&self) -> Result<(), WindowError>;
    fn is_fullscreen(&self) -> Result<bool, WindowError>;
}

pub fn run<F>(config: WMConfig, _exec: F)
where
    F: FnOnce(Box<dyn BackendHandle + Send + Sync + 'static>) + Send + 'static,
{
    match config.window_backend {
        #[cfg(feature = "winit_window")]
        WindowBackend::Winit => {
            self::winit::WntBackendHandle::run(config.winit_force_x11, move |backend| {
                _exec(Box::new(backend))
            })
        },
        #[cfg(feature = "wayland_window")]
        WindowBackend::Wayland => {
            self::wayland::WlBackendHandle::run(move |backend| _exec(Box::new(backend)))
        },
    }
}

#[derive(Debug)]
struct PendingRes<T>(Arc<(Mutex<Option<T>>, Condvar)>);

impl<T> PendingRes<T> {
    fn empty() -> Self {
        Self(Arc::new((Mutex::new(None), Condvar::new())))
    }

    fn wait(self) -> T {
        let mut gu = self.0.0.lock();
        while gu.is_none() {
            self.0.1.wait(&mut gu)
        }
        gu.take().unwrap()
    }

    fn set(self, val: T) {
        *self.0.0.lock() = Some(val);
        self.0.1.notify_all();
    }
}

impl<T> Clone for PendingRes<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
