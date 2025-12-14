pub mod wl_layer;

use std::sync::Arc;

use crate::Basalt;
use crate::window::{FullScreenBehavior, Monitor, Window, WindowBackend, WindowError};

#[cfg(feature = "winit_window")]
mod wnt {
    pub use winit::dpi::{PhysicalPosition, PhysicalSize};
    pub use winit::window::WindowAttributes;
}

#[cfg(feature = "wayland_window")]
use crate::window::backend::wayland::{WlLayerAttributes, WlWindowAttributes};

pub struct WindowBuilder {
    basalt: Arc<Basalt>,
    attributes: WindowAttributes,
}

#[derive(Debug)]
pub(crate) enum WindowAttributes {
    #[cfg(feature = "winit_window")]
    Winit(wnt::WindowAttributes),
    #[cfg(feature = "wayland_window")]
    WlLayer(WlLayerAttributes),
    #[cfg(feature = "wayland_window")]
    WlWindow(WlWindowAttributes),
    #[allow(dead_code)]
    NonExhaustive,
}

#[allow(unreachable_code)]
impl WindowBuilder {
    pub(crate) fn new(basalt: Arc<Basalt>, backend: WindowBackend) -> Self {
        Self {
            basalt,
            attributes: match backend {
                #[cfg(feature = "winit_window")]
                WindowBackend::Winit => WindowAttributes::Winit(Default::default()),
                #[cfg(feature = "wayland_window")]
                WindowBackend::Wayland => WindowAttributes::WlWindow(Default::default()),
            },
        }
    }

    /// Set the title of the window.
    pub fn title<T>(mut self, _title: T) -> Self
    where
        T: Into<String>,
    {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.title = _title.into();
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.title = Some(_title.into());
            },
            _ => unreachable!(),
        }

        self
    }

    /// Set the inner size of the window.
    pub fn size<S>(mut self, _size: S) -> Self
    where
        S: Into<[u32; 2]>,
    {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                let size = _size.into();
                attrs.inner_size = Some(wnt::PhysicalSize::new(size[0], size[1]).into());
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.size = Some(_size.into());
            },
            _ => unreachable!(),
        }

        self
    }

    /// Set the minimum inner size of the window.
    ///
    /// **Note**: When not specified, the window will not have a minimum size.
    pub fn min_size<S>(mut self, _size: S) -> Result<Self, WindowError>
    where
        S: Into<[u32; 2]>,
    {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                let size = _size.into();
                attrs.min_inner_size = Some(wnt::PhysicalSize::new(size[0], size[1]).into());
                Ok(self)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.min_size = Some(_size.into());
                Ok(self)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// Set the maximum inner size of the window.
    ///
    /// **Note**: When not specified, the window will not have a maximum size.
    pub fn max_size<S>(mut self, _size: S) -> Result<Self, WindowError>
    where
        S: Into<[u32; 2]>,
    {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                let size = _size.into();
                attrs.max_inner_size = Some(wnt::PhysicalSize::new(size[0], size[1]).into());
                Ok(self)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.max_size = Some(_size.into());
                Ok(self)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// Set the position of the window.
    ///
    /// **Note**: When not specified, the window will be positioned by the implementation.
    pub fn position<P>(mut self, _position: P) -> Result<Self, WindowError>
    where
        P: Into<[u32; 2]>,
    {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                let position = _position.into();
                attrs.position = Some(wnt::PhysicalPosition::new(position[0], position[1]).into());
                Ok(self)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(_) => {
                // TODO:
                Err(WindowError::NotImplemented)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// Create the window on the specified monitor.
    pub fn monitor(mut self, _monitor: Monitor) -> Result<Self, WindowError> {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(_) => {
                // TODO:
                Err(WindowError::NotImplemented)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(_) => {
                // TODO:
                Err(WindowError::NotImplemented)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// If the window is allowed to be resized.
    ///
    /// **Note**: When not specified, this will be `true`.
    pub fn resizeable(mut self, _resizable: bool) -> Result<Self, WindowError> {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.resizable = _resizable;
                Ok(self)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(_) => {
                // TODO:
                Err(WindowError::NotImplemented)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// Open the window maximized.
    ///
    /// **Note**: When not specified, this will be `false`.
    pub fn maximized(mut self, _maximized: bool) -> Result<Self, WindowError> {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.maximized = _maximized;
                Ok(self)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.maximized = _maximized;
                Ok(self)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// Open the window minimized.
    ///
    /// **Note**: When not specified, this will be `false`.
    pub fn minimized(mut self, _minimized: bool) -> Result<Self, WindowError> {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.visible = !_minimized;
                Ok(self)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.minimized = _minimized;
                Ok(self)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// If the window should have decorations.
    ///
    /// **Note**: When not specified, this will be `true`.
    pub fn decorations(mut self, _decorations: bool) -> Result<Self, WindowError> {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.decorations = _decorations;
                Ok(self)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.decorations = _decorations;
                Ok(self)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// Open the window full screen with the given behavior.
    ///
    /// **Note**: When not specified, the window be opened regularly.
    pub fn fullscreen(
        mut self,
        _full_screen_behavior: FullScreenBehavior,
    ) -> Result<Self, WindowError> {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                let monitors = self.basalt.window_manager_ref().monitors()?;

                let primary_monitor = monitors.iter().find_map(|monitor| {
                    if monitor.is_primary {
                        Some(monitor.clone())
                    } else {
                        None
                    }
                });

                attrs.fullscreen = Some(
                    _full_screen_behavior.determine_winit_fullscreen(
                        true,
                        self.basalt
                            .device_ref()
                            .enabled_extensions()
                            .ext_full_screen_exclusive,
                        None,
                        primary_monitor,
                        monitors,
                    )?,
                );

                Ok(self)
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(_) => {
                // TODO:
                Err(WindowError::NotImplemented)
            },
            _ => Err(WindowError::NotSupported),
        }
    }

    /// Finish building the [`Window`].
    pub fn build(self) -> Result<Arc<Window>, WindowError> {
        self.basalt.window_manager().create_window(self.attributes)
    }
}
