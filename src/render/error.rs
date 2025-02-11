mod vk {
    pub use vulkano::buffer::AllocateBufferError;
    pub use vulkano::image::AllocateImageError;
    pub use vulkano::memory::allocator::MemoryAllocatorError;
    pub use vulkano::{Validated, VulkanError};
    pub use vulkano_taskgraph::graph::{CompileError, CompileErrorKind, ExecuteError};
}

use std::fmt::{self, Display, Formatter};

use crate::render::RendererContext;
use crate::render::worker::VertexUploadTaskWorld;

/// An error occurred during the `Renderer` creation.
#[derive(Debug)]
pub enum RendererCreateError {
    /// Window already has a rendererer.
    WindowHasRenderer,
    /// An error occurred during the `RendererContext` creation.
    Context(ContextCreateError),
    /// An error occurred during the `Renderer`s worker creation.
    Worker(WorkerCreateError),
    /// An error occurred with an operation using vulkano.
    Vulkano(VulkanoError),
}

impl From<ContextCreateError> for RendererCreateError {
    fn from(e: ContextCreateError) -> Self {
        RendererCreateError::Context(e)
    }
}

impl From<WorkerCreateError> for RendererCreateError {
    fn from(e: WorkerCreateError) -> Self {
        RendererCreateError::Worker(e)
    }
}

impl From<VulkanoError> for RendererCreateError {
    fn from(e: VulkanoError) -> Self {
        Self::Vulkano(e)
    }
}

impl Display for RendererCreateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::WindowHasRenderer => {
                f.write_str("Failed to create renderer, the window already has a renderer.")
            },
            Self::Context(e) => write!(f, "{}", e),
            Self::Worker(e) => write!(f, "{}", e),
            Self::Vulkano(e) => {
                write!(
                    f,
                    "Failed to create renderer, a vulkano error occurred: {}",
                    e
                )
            },
        }
    }
}

/// An error occurred during the `RendererContext` creation.
#[derive(Debug)]
pub enum ContextCreateError {
    /// Unable to find a suitable swapchain format.
    NoSuitableSwapchainFormat,
    /// Unable to find a suitable image format.
    NoSuitableImageFormat,
    /// An error occurred with an operation using vulkano.
    Vulkano(VulkanoError),
}

impl From<VulkanoError> for ContextCreateError {
    fn from(e: VulkanoError) -> Self {
        Self::Vulkano(e)
    }
}

impl Display for ContextCreateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::NoSuitableSwapchainFormat => {
                f.write_str("Failed to create context, no suitable swapchain format was found.")
            },
            Self::NoSuitableImageFormat => {
                f.write_str("Failed to create context, no suitable image format was found.")
            },
            Self::Vulkano(e) => {
                write!(
                    f,
                    "Failed to create context, a vulkano error occurred: {}",
                    e
                )
            },
        }
    }
}

/// An error occurred during the `Renderer`s worker creation.
#[derive(Debug)]
pub enum WorkerCreateError {
    /// An error occurred with an operation using vulkano.
    Vulkano(VulkanoError),
}

impl From<VulkanoError> for WorkerCreateError {
    fn from(e: VulkanoError) -> Self {
        Self::Vulkano(e)
    }
}

impl Display for WorkerCreateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::Vulkano(e) => {
                write!(
                    f,
                    "Failed to create worker, a vulkano error occurred: {}",
                    e
                )
            },
        }
    }
}

/// An error occurred within the `Renderer`.
#[derive(Debug)]
pub enum RendererError {
    /// The window has been closed.
    Closed,
    /// An error occurred within the `Renderer`'s worker.
    Worker(WorkerError),
    /// An error occurred within the `RendererContext`.
    Context(ContextError),
    /// An error occurred previously that wasn't handled.
    ErrorNotHandled,
}

impl From<ContextError> for RendererError {
    fn from(e: ContextError) -> Self {
        Self::Context(e)
    }
}

impl From<WorkerError> for RendererError {
    fn from(e: WorkerError) -> Self {
        Self::Worker(e)
    }
}

impl Display for RendererError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::Closed => f.write_str("The window was closed."),
            Self::Worker(e) => write!(f, "{}", e),
            Self::Context(e) => write!(f, "{}", e),
            Self::ErrorNotHandled => {
                f.write_str("The renderer previously returned an error, but it wasn't handled.")
            },
        }
    }
}

/// An error occurred within the `RendererContext`.
#[derive(Debug)]
pub enum ContextError {
    /// No mode was set during the creation of `Renderer`.
    NoModeSet,
    /// An error occurred with an operation using vulkano.
    Vulkano(VulkanoError),
}

impl From<VulkanoError> for ContextError {
    fn from(e: VulkanoError) -> Self {
        Self::Vulkano(e)
    }
}

impl Display for ContextError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::NoModeSet => f.write_str("The context doesn't have a mode."),
            Self::Vulkano(e) => {
                write!(f, "The context had a vulkano error occur: {}", e)
            },
        }
    }
}

/// An error occurred within the `Renderer`'s worker.
#[derive(Debug)]
pub enum WorkerError {
    /// The worker panicked.
    Panicked,
    /// The worker lost connection to the `Renderer`.
    Disconnected,
    /// The worker had an OVD worker panicked.
    OvdWorkerPanicked,
    /// An error occurred with an operation using vulkano.
    Vulkano(VulkanoError),
}

impl From<VulkanoError> for WorkerError {
    fn from(e: VulkanoError) -> Self {
        Self::Vulkano(e)
    }
}

impl Display for WorkerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::Panicked => f.write_str("The worker panicked."),
            Self::Disconnected => f.write_str("The worker lost connection to the `Renderer`."),
            Self::OvdWorkerPanicked => f.write_str("The worker had an OVD worker panicked."),
            Self::Vulkano(e) => {
                write!(f, "The worker had a vulkano error occur: {}", e)
            },
        }
    }
}

/// An error related to an operation with vulkano.
#[derive(Debug)]
pub enum VulkanoError {
    CompileTaskGraph(vk::CompileErrorKind),
    CreateBuffer(vk::Validated<vk::AllocateBufferError>),
    CreateDescSet(vk::Validated<vk::VulkanError>),
    CreateDescSetLayout(vk::Validated<vk::VulkanError>),
    CreateFlight(vk::VulkanError),
    CreateFramebuffer(vk::Validated<vk::VulkanError>),
    CreateGraphicsPipeline(vk::Validated<vk::VulkanError>),
    CreateImage(vk::Validated<vk::AllocateImageError>),
    CreateImageView(vk::Validated<vk::VulkanError>),
    CreatePipelineLayout(vk::Validated<vk::VulkanError>),
    CreateRenderPass(vk::Validated<vk::VulkanError>),
    CreateSampler(vk::Validated<vk::VulkanError>),
    CreateSwapchain(vk::Validated<vk::VulkanError>),
    ExecuteTaskGraph(vk::ExecuteError),
    FlightWait(vk::VulkanError),
}

impl VulkanoError {
    /// Return the `VulkanError` if present.
    pub fn as_vulkan(&self) -> Option<vk::VulkanError> {
        match self {
            Self::CreateDescSet(e)
            | Self::CreateDescSetLayout(e)
            | Self::CreateFramebuffer(e)
            | Self::CreateGraphicsPipeline(e)
            | Self::CreateImageView(e)
            | Self::CreatePipelineLayout(e)
            | Self::CreateRenderPass(e)
            | Self::CreateSampler(e)
            | Self::CreateSwapchain(e) => {
                match e {
                    vk::Validated::Error(e) => Some(*e),
                    _ => None,
                }
            },
            Self::CreateFlight(e) | Self::FlightWait(e) => Some(*e),
            Self::CompileTaskGraph(e) => {
                match e {
                    vk::CompileErrorKind::VulkanError(e) => Some(*e),
                    _ => None,
                }
            },
            Self::CreateBuffer(e) => {
                match e {
                    vk::Validated::Error(e) => {
                        match e {
                            vk::AllocateBufferError::CreateBuffer(e) => Some(*e),
                            vk::AllocateBufferError::AllocateMemory(e) => {
                                match e {
                                    vk::MemoryAllocatorError::AllocateDeviceMemory(e) => {
                                        match e {
                                            vk::Validated::Error(e) => Some(*e),
                                            _ => None,
                                        }
                                    },
                                    _ => None,
                                }
                            },
                            vk::AllocateBufferError::BindMemory(e) => Some(*e),
                        }
                    },
                    _ => None,
                }
            },
            Self::CreateImage(e) => {
                match e {
                    vk::Validated::Error(e) => {
                        match e {
                            vk::AllocateImageError::CreateImage(e) => Some(*e),
                            vk::AllocateImageError::AllocateMemory(e) => {
                                match e {
                                    vk::MemoryAllocatorError::AllocateDeviceMemory(e) => {
                                        match e {
                                            vk::Validated::Error(e) => Some(*e),
                                            _ => None,
                                        }
                                    },
                                    _ => None,
                                }
                            },
                            vk::AllocateImageError::BindMemory(e) => Some(*e),
                        }
                    },
                    _ => None,
                }
            },
            Self::ExecuteTaskGraph(e) => {
                match e {
                    vk::ExecuteError::Swapchain {
                        error, ..
                    } => {
                        match error {
                            vk::Validated::Error(e) => Some(*e),
                            _ => None,
                        }
                    },
                    vk::ExecuteError::VulkanError(e) => Some(*e),
                    _ => None,
                }
            },
        }
    }
}

impl Display for VulkanoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::CompileTaskGraph(e) => {
                let desc = match e {
                    vk::CompileErrorKind::Unconnected => {
                        String::from("the graph is not weakly connected")
                    },
                    vk::CompileErrorKind::Cycle => {
                        String::from("the graph contains a directed cycle")
                    },
                    vk::CompileErrorKind::InsufficientQueues => {
                        String::from(
                            "the given queues are not sufficient for the requirements of a task",
                        )
                    },
                    vk::CompileErrorKind::VulkanError(_) => {
                        String::from("a runtime error occurred")
                    },
                };

                write!(f, "Failed to compile task graph: {}", desc)
            },
            Self::CreateBuffer(e) => {
                write!(f, "Failed to create buffer: {}", e)
            },
            Self::CreateDescSet(e) => {
                write!(f, "Failed to create descriptor set: {}", e)
            },
            Self::CreateDescSetLayout(e) => {
                write!(f, "Failed to create descriptor set layout: {}", e)
            },
            Self::CreateFlight(e) => {
                write!(f, "Failed to create flight: {}", e)
            },
            Self::CreateFramebuffer(e) => {
                write!(f, "Failed to create framebuffer: {}", e)
            },
            Self::CreateGraphicsPipeline(e) => {
                write!(f, "Failed to create graphics pipeline: {}", e)
            },
            Self::CreateImage(e) => {
                write!(f, "Failed to create image: {}", e)
            },
            Self::CreateImageView(e) => {
                write!(f, "Failed to create image view: {}", e)
            },
            Self::CreatePipelineLayout(e) => {
                write!(f, "Failed to create pipeline layout: {}", e)
            },
            Self::CreateRenderPass(e) => {
                write!(f, "Failed to create render pass: {}", e)
            },
            Self::CreateSampler(e) => {
                write!(f, "Failed to create sampler: {}", e)
            },
            Self::CreateSwapchain(e) => {
                write!(f, "Failed to create swapchain: {}", e)
            },
            Self::ExecuteTaskGraph(e) => {
                write!(f, "Failed to execute task graph: {}", e)
            },
            Self::FlightWait(e) => {
                write!(f, "Failed to wait on flight: {}", e)
            },
        }
    }
}

impl From<vk::CompileError<RendererContext>> for VulkanoError {
    fn from(e: vk::CompileError<RendererContext>) -> Self {
        Self::CompileTaskGraph(e.kind)
    }
}

impl From<vk::CompileError<VertexUploadTaskWorld>> for VulkanoError {
    fn from(e: vk::CompileError<VertexUploadTaskWorld>) -> Self {
        Self::CompileTaskGraph(e.kind)
    }
}
