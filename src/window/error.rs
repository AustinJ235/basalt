use std::fmt::{self, Display, Formatter};

use raw_window_handle::HandleError;

mod vko {
    pub use vulkano::swapchain::FromWindowError;
    pub use vulkano::{Validated, VulkanError};
}

/// An error occurred during the `Window` creation.
#[derive(Debug)]
pub enum WindowCreateError {
    /// The `WindowManager`'s event loop has exited.
    EventLoopExited,
    /// The OS failed to create the window.
    Os(String),
    /// The window handle is not supported by basalt.
    NotSupported,
    /// The window handle is not available.
    Unavailable,
    /// Failed to create the surface.
    CreateSurface(vko::Validated<vko::VulkanError>),
}

impl From<HandleError> for WindowCreateError {
    fn from(e: HandleError) -> Self {
        match e {
            HandleError::NotSupported => Self::NotSupported,
            HandleError::Unavailable => Self::Unavailable,
            _ => Self::Unavailable,
        }
    }
}

impl From<vko::FromWindowError> for WindowCreateError {
    fn from(e: vko::FromWindowError) -> Self {
        match e {
            vko::FromWindowError::RetrieveHandle(e) => e.into(),
            vko::FromWindowError::CreateSurface(e) => Self::CreateSurface(e),
        }
    }
}

impl Display for WindowCreateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::EventLoopExited => f.write_str("The `WindowManager`'s event loop has exited."),
            Self::Os(e) => {
                write!(f, "The OS failed to create the window: {}", e)
            },
            Self::NotSupported => f.write_str("The window handle is not supported by basalt"),
            Self::Unavailable => f.write_str("The window handle is not available"),
            Self::CreateSurface(e) => write!(f, "Failed to create surface: {}", e),
        }
    }
}
