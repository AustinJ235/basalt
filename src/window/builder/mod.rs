#[cfg(feature = "wayland_window")]
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

/// Builder for creating [`Window`]'s.
///
/// Created via [`WindowManager::create`](crate::window::WindowManager::create).
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
    pub fn size(mut self, _size: [u32; 2]) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.inner_size = Some(wnt::PhysicalSize::new(_size[0], _size[1]).into());
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.size = Some(_size);
            },
            _ => unreachable!(),
        }

        self
    }

    /// Set the minimum inner size of the window.
    ///
    /// When not specified, the window will not have a minimum size.
    pub fn min_size(mut self, _min_size: [u32; 2]) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.min_inner_size =
                    Some(wnt::PhysicalSize::new(_min_size[0], _min_size[1]).into());
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.min_size = Some(_min_size);
            },
            _ => unreachable!(),
        }

        self
    }

    /// Set the maximum inner size of the window.
    ///
    /// When not specified, the window will not have a maximum size.
    pub fn max_size(mut self, _max_size: [u32; 2]) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.max_inner_size =
                    Some(wnt::PhysicalSize::new(_max_size[0], _max_size[1]).into());
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.max_size = Some(_max_size);
            },
            _ => unreachable!(),
        }

        self
    }

    /// Set the position of the window.
    ///
    /// When not specified, the window will be positioned by the implementation.
    ///
    /// - **winit**: may be ignored depending on support.
    /// - **wayland**: not implemented, ignored.
    pub fn position(mut self, _position: [u32; 2]) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.position =
                    Some(wnt::PhysicalPosition::new(_position[0], _position[1]).into());
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(_) => {
                // TODO:
            },
            _ => unreachable!(),
        }

        self
    }

    /// Create the window on the specified monitor.
    ///
    /// - **winit**: not implemented, ignored.
    /// - **wayland**: not implemented, ignored.
    pub fn monitor(mut self, _monitor: Monitor) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(_) => {
                // TODO:
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(_) => {
                // TODO:
            },
            _ => unreachable!(),
        }

        self
    }

    /// If the window is allowed to be resized.
    ///
    /// - **winit**: not configurable after creation, yet.
    /// - **wayland**: not implemented, ignored.
    pub fn resizeable(mut self, _resizable: bool) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.resizable = _resizable;
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(_) => {
                // TODO:
            },
            _ => unreachable!(),
        }

        self
    }

    /// Open the window maximized.
    ///
    /// When not specified, this will be `false`.
    pub fn maximized(mut self, _maximized: bool) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.maximized = _maximized;
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.maximized = _maximized;
            },
            _ => unreachable!(),
        }

        self
    }

    /// Open the window minimized.
    ///
    /// When not specified, this will be `false`.
    pub fn minimized(mut self, _minimized: bool) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.visible = !_minimized;
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.minimized = _minimized;
            },
            _ => unreachable!(),
        }

        self
    }

    /// If the window should have decorations.
    ///
    /// When not specified, this will be `true`.
    ///
    /// **TODO:** This may not working as intended.
    pub fn decorations(mut self, _decorations: bool) -> Self {
        match &mut self.attributes {
            #[cfg(feature = "winit_window")]
            WindowAttributes::Winit(attrs) => {
                attrs.decorations = _decorations;
            },
            #[cfg(feature = "wayland_window")]
            WindowAttributes::WlWindow(attrs) => {
                attrs.decorations = _decorations;
            },
            _ => unreachable!(),
        }

        self
    }

    /// Open the window full screen with the given behavior.
    ///
    /// When not specified, the window be opened regularly.
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
