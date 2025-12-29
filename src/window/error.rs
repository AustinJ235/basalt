use std::fmt::{self, Display, Formatter};

use raw_window_handle::HandleError as RwhHandleError;

mod vko {
    pub use vulkano::swapchain::FromWindowError;
    pub use vulkano::{Validated, VulkanError};
}

/// An error related to operations on a [`Window`](crate::window::Window).
#[derive(Debug, Clone)]
pub enum WindowError {
    /// The window is not ready yet.
    ///
    /// This error can only occur internally, and  isn't an error the user will encounter.
    NotReady,
    /// The window backend doesn't support this operation.
    NotSupported,
    /// The window backend hasn't implemented this yet.
    NotImplemented,
    /// The window has been requested to close or is closed.
    Closed,
    /// The window backend has exited.
    ///
    /// This is mainly encountered when the application was requested to exit, but an operation
    /// was attempted on a [`Window`](crate::window::Window). This also may occur if the backend
    /// has panicked. In either case the application is in the process of exiting.
    BackendExited,
    /// An error related to enabling fullscreen.
    EnableFullScreen(EnableFullScreenError),
    /// An error related to creating a window.
    CreateWindow(CreateWindowError),
    /// An error that isn't covered by this enum.
    Other(String),
}

impl Display for WindowError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::NotReady => f.write_str("the window is not ready yet."),
            Self::NotSupported => f.write_str("the window backend doesn't support this operation."),
            Self::NotImplemented => f.write_str("the window backend hasn't implemented this yet."),
            Self::Closed => f.write_str("the window has been requested to close or is closed."),
            Self::BackendExited => f.write_str("the window backend has exited."),
            Self::EnableFullScreen(e) => {
                f.write_fmt(format_args!("failed to enable full screen: {}", e))
            },
            Self::CreateWindow(e) => f.write_fmt(format_args!("failed to create window: {}", e)),
            Self::Other(e) => f.write_str(e.as_str()),
        }
    }
}

impl From<EnableFullScreenError> for WindowError {
    fn from(e: EnableFullScreenError) -> Self {
        Self::EnableFullScreen(e)
    }
}

impl From<CreateWindowError> for WindowError {
    fn from(e: CreateWindowError) -> Self {
        Self::CreateWindow(e)
    }
}

/// An error that can be returned from attempting to go full screen.
#[derive(Debug, Clone)]
pub enum EnableFullScreenError {
    /// The window backend is unable to determine the primary monitor.
    UnableToDeterminePrimary,
    /// The window backend is unable to determine the current monitor.
    UnableToDetermineCurrent,
    /// Attempted to use exclusive fullscreen when it wasn't enabled.
    ///
    /// See: `BstOptions::use_exclusive_fullscreen`
    ExclusiveNotSupported,
    /// The monitor no longer exists.
    MonitorDoesNotExist,
    /// No available monitors
    NoAvailableMonitors,
    /// The provided mode doesn't belong to the monitor.
    IncompatibleMonitorMode,
}

impl Display for EnableFullScreenError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::UnableToDeterminePrimary => {
                f.write_str("unable to determine the primary monitor.")
            },
            Self::UnableToDetermineCurrent => {
                f.write_str("unable to determine the current monitor.")
            },
            Self::ExclusiveNotSupported => f.write_str("exclusive full screen not supported."),
            Self::MonitorDoesNotExist => f.write_str("the provided monitor doesn't exist."),
            Self::NoAvailableMonitors => f.write_str("no monitors currently exist."),
            Self::IncompatibleMonitorMode => {
                f.write_str("the provided monitor mode isn't compatible.")
            },
        }
    }
}

/// An error related to the creation of a window.
#[derive(Debug, Clone)]
pub enum CreateWindowError {
    /// The OS failed to create the window.
    Os(String),
    /// The window handle is not supported.
    HandleNotSupported,
    /// The window handle is not available.
    HandleUnavailable,
    /// Failed to create the surface.
    CreateSurface(vko::Validated<vko::VulkanError>),
}

impl Display for CreateWindowError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::Os(e) => f.write_fmt(format_args!("an os error occured: {}", e)),
            Self::HandleNotSupported => f.write_str("the window handle isn't supported."),
            Self::HandleUnavailable => f.write_str("the window handle is unavailable."),
            Self::CreateSurface(e) => f.write_fmt(format_args!("failed to create surface: {}", e)),
        }
    }
}

impl From<RwhHandleError> for CreateWindowError {
    fn from(e: RwhHandleError) -> Self {
        match e {
            RwhHandleError::NotSupported => Self::HandleNotSupported,
            RwhHandleError::Unavailable | _ => Self::HandleUnavailable,
        }
    }
}

impl From<vko::FromWindowError> for CreateWindowError {
    fn from(e: vko::FromWindowError) -> Self {
        match e {
            vko::FromWindowError::RetrieveHandle(e) => e.into(),
            vko::FromWindowError::CreateSurface(e) => Self::CreateSurface(e),
        }
    }
}
