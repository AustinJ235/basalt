mod vk {
    pub use vulkano::buffer::Subbuffer;
    pub use vulkano::format::{ClearColorValue, Format, FormatFeatures, NumericFormat};
    pub use vulkano::image::{Image, ImageUsage};
    pub use vulkano::pipeline::GraphicsPipeline;
    pub use vulkano::render_pass::{Framebuffer, RenderPass};
    pub use vulkano::swapchain::{
        ColorSpace, FullScreenExclusive, PresentGravity, PresentGravityFlags, PresentMode,
        PresentScaling, PresentScalingFlags, Swapchain, SwapchainCreateInfo,
    };
    pub use vulkano::{Validated, VulkanError};
    pub use vulkano_taskgraph::command_buffer::{ClearColorImageInfo, RecordingCommandBuffer};
    pub use vulkano_taskgraph::graph::{CompileInfo, ExecutableTaskGraph, ExecuteError, TaskGraph};
    pub use vulkano_taskgraph::resource::{AccessType, Flight, ImageLayoutType, Resources};
    pub use vulkano_taskgraph::{resource_map, Id, QueueFamilyType, Task, TaskContext, TaskResult};
}

use std::sync::Arc;

use super::{VSync, MSAA};
use crate::interface::ItfVertInfo;
use crate::window::Window;

pub struct ContextExecuteInfo {
    buffer: vk::Subbuffer<[ItfVertInfo]>,
    images: Vec<Arc<vk::Image>>,
}

pub struct Context {
    window: Arc<Window>,
    render_flt_id: vk::Id<vk::Flight>,
    swapchain_id: vk::Id<vk::Swapchain>,
    swapchain_ci: vk::SwapchainCreateInfo,
    swapchain_rc: bool,
    specific: Specific,
}

enum Specific {
    ItfOnly(ItfOnly),
    Minimal(Minimal),
    None,
}

struct ItfOnly {
    task_graph: Option<vk::ExecutableTaskGraph<Context>>,
    render_pass: Option<Arc<vk::RenderPass>>,
    pipeline: Option<Arc<vk::GraphicsPipeline>>,
    framebuffers: Option<Vec<Arc<vk::Framebuffer>>>,
    virtual_swapchain_id: vk::Id<vk::Swapchain>,
}

struct Minimal {
    task_graph: vk::ExecutableTaskGraph<Context>,
    virtual_swapchain_id: vk::Id<vk::Swapchain>,
}

impl Context {
    pub fn new(window: Arc<Window>, render_flt_id: vk::Id<vk::Flight>) -> Result<Self, String> {
        let (fullscreen_mode, win32_monitor) = match window
            .basalt_ref()
            .device_ref()
            .enabled_extensions()
            .ext_full_screen_exclusive
        {
            true => {
                (
                    vk::FullScreenExclusive::ApplicationControlled,
                    window.win32_monitor(),
                )
            },
            false => (vk::FullScreenExclusive::Default, None),
        };

        let mut surface_formats = window.surface_formats(fullscreen_mode);

        /*let ext_swapchain_colorspace = window
        .basalt_ref()
        .instance_ref()
        .enabled_extensions()
        .ext_swapchain_colorspace;*/

        surface_formats.retain(|(format, colorspace)| {
            if !match colorspace {
                vk::ColorSpace::SrgbNonLinear => true,
                // TODO: Support these properly, these are for hdr mainly. Typically the format
                //       is a signed float where values are allowed to be less than zero or greater
                //       one. The main problem currently is that anything that falls in the normal
                //       range don't appear as bright as one would expect on a hdr display.
                // vk::ColorSpace::ExtendedSrgbLinear => ext_swapchain_colorspace,
                // vk::ColorSpace::ExtendedSrgbNonLinear => ext_swapchain_colorspace,
                _ => false,
            } {
                return false;
            }

            // TODO: Support non SRGB formats properly. When writing to a non-SRGB format using the
            //       SrgbNonLinear colorspace, colors written will be assumed to be SRGB. This
            //       causes issues since everything is done with linear color.
            if format.numeric_format_color() != Some(vk::NumericFormat::SRGB) {
                return false;
            }

            true
        });

        surface_formats.sort_by_key(|(format, _colorspace)| format.components()[0]);

        let (surface_format, surface_colorspace) = surface_formats.pop().ok_or(String::from(
            "Unable to find suitable format & colorspace for the swapchain.",
        ))?;

        let (scaling_behavior, present_gravity) = if window
            .basalt_ref()
            .device_ref()
            .enabled_extensions()
            .ext_swapchain_maintenance1
        {
            let capabilities = window.surface_capabilities(fullscreen_mode);

            let scaling = if capabilities
                .supported_present_scaling
                .contains(vk::PresentScalingFlags::ONE_TO_ONE)
            {
                Some(vk::PresentScaling::OneToOne)
            } else {
                None
            };

            let gravity = if capabilities.supported_present_gravity[0]
                .contains(vk::PresentGravityFlags::MIN)
                && capabilities.supported_present_gravity[1].contains(vk::PresentGravityFlags::MIN)
            {
                Some([vk::PresentGravity::Min, vk::PresentGravity::Min])
            } else {
                None
            };

            (scaling, gravity)
        } else {
            (None, None)
        };

        let swapchain_ci = vk::SwapchainCreateInfo {
            min_image_count: 2,
            image_format: surface_format,
            image_color_space: surface_colorspace,
            image_extent: window.surface_current_extent(fullscreen_mode),
            image_usage: vk::ImageUsage::COLOR_ATTACHMENT | vk::ImageUsage::TRANSFER_DST,
            present_mode: find_present_mode(&window, fullscreen_mode, window.renderer_vsync()),
            full_screen_exclusive: fullscreen_mode,
            win32_monitor,
            scaling_behavior,
            present_gravity,
            ..vk::SwapchainCreateInfo::default()
        };

        let swapchain_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_swapchain(render_flt_id, window.surface(), swapchain_ci.clone())
            .unwrap();

        Ok(Self {
            window,
            render_flt_id,
            swapchain_id,
            swapchain_ci,
            swapchain_rc: false,
            specific: Specific::None,
        })
    }

    pub fn surface_format(&self) -> vk::Format {
        self.swapchain_ci.image_format
    }

    pub fn itf_only(&mut self) {
        todo!()
    }

    pub fn minimal(&mut self) -> Result<(), String> {
        Minimal::create(self)
    }

    pub fn set_extent(&mut self, extent: [f32; 2]) {
        todo!()
    }

    pub fn set_msaa(&mut self, msaa: MSAA) {
        todo!()
    }

    pub fn set_vsync(&mut self, vsync: VSync) {
        todo!()
    }

    pub fn set_image_capacity(&mut self, image_capacity: u32) {
        todo!()
    }

    fn update(&mut self) -> Result<(), String> {
        if self.swapchain_rc {
            self.swapchain_id = self
                .window
                .basalt_ref()
                .device_resources_ref()
                .recreate_swapchain(self.swapchain_id, |_| self.swapchain_ci.clone())
                .map_err(|e| format!("Failed to recreate swapchain: {}", e))?;

            self.swapchain_rc = false;
        }

        Ok(())
    }

    pub fn execute(&mut self) -> Result<(), String> {
        self.update()?;

        let flight = self
            .window
            .basalt_ref()
            .device_resources_ref()
            .flight(self.render_flt_id)
            .unwrap();

        let frame_index = flight.current_frame_index();

        match &self.specific {
            Specific::ItfOnly(_) => todo!(),
            Specific::Minimal(minimal) => {
                let resource_map = vk::resource_map!(
                    &minimal.task_graph,
                    minimal.virtual_swapchain_id => self.swapchain_id,
                )
                .unwrap();

                flight.wait(None).unwrap();

                match unsafe { minimal.task_graph.execute(resource_map, self, || ()) } {
                    Ok(()) => (),
                    Err(vk::ExecuteError::Swapchain {
                        error: vk::Validated::Error(vk::VulkanError::OutOfDate),
                        ..
                    }) => {
                        self.swapchain_rc = true;
                    },
                    Err(e) => {
                        return Err(format!("Failed to execute frame: {}", e));
                    },
                }
            },
            Specific::None => panic!("Renderer mode not set!"),
        }

        Ok(())
    }
}

impl Minimal {
    pub fn create(context: &mut Context) -> Result<(), String> {
        let mut task_graph =
            vk::TaskGraph::new(context.window.basalt_ref().device_resources_ref(), 1, 1);

        let virtual_swapchain_id = task_graph.add_swapchain(&vk::SwapchainCreateInfo::default());
        // let virtual_swapchain_id = task_graph.add_swapchain(&context.swapchain_ci);

        task_graph
            .create_task_node("Render", vk::QueueFamilyType::Graphics, RenderTask)
            .image_access(
                virtual_swapchain_id.current_image_id(),
                vk::AccessType::ClearTransferWrite,
                vk::ImageLayoutType::Optimal,
            );

        let task_graph = unsafe {
            task_graph.compile(&vk::CompileInfo {
                queues: &[context.window.basalt_ref().graphics_queue_ref()],
                present_queue: Some(context.window.basalt_ref().graphics_queue_ref()),
                flight_id: context.render_flt_id,
                ..Default::default()
            })
        }
        .map_err(|e| format!("Failed to compile task graph: {}", e))?;

        context.specific = Specific::Minimal(Self {
            task_graph,
            virtual_swapchain_id,
        });

        Ok(())
    }
}

struct RenderTask;

impl vk::Task for RenderTask {
    type World = Context;

    unsafe fn execute(
        &self,
        cmd: &mut vk::RecordingCommandBuffer<'_>,
        task: &mut vk::TaskContext<'_>,
        context: &Self::World,
    ) -> vk::TaskResult {
        let swapchain_state = task.swapchain(context.swapchain_id)?;
        let image_index = swapchain_state.current_image_index().unwrap();

        match &context.specific {
            Specific::ItfOnly(_) => todo!(),
            Specific::Minimal(minimal) => {
                cmd.clear_color_image(&vk::ClearColorImageInfo {
                    image: context.swapchain_id.current_image_id(),
                    clear_value: vk::ClearColorValue::Float([0.0; 4]),
                    // clear_value: vk::ClearColorValue::Uint([0; 4]),
                    // clear_value: vk::ClearColorValue::Int([0; 4]),
                    ..Default::default()
                })
                .unwrap();
            },
            Specific::None => unreachable!(),
        }

        Ok(())
    }
}

fn find_present_mode(
    window: &Arc<Window>,
    fullscreen_mode: vk::FullScreenExclusive,
    vsync: VSync,
) -> vk::PresentMode {
    let mut present_modes = window.surface_present_modes(fullscreen_mode);

    present_modes.retain(|present_mode| {
        matches!(
            present_mode,
            vk::PresentMode::Fifo
                | vk::PresentMode::FifoRelaxed
                | vk::PresentMode::Mailbox
                | vk::PresentMode::Immediate
        )
    });

    present_modes.sort_by_key(|present_mode| {
        match vsync {
            VSync::Enable => {
                match present_mode {
                    vk::PresentMode::Fifo => 3,
                    vk::PresentMode::FifoRelaxed => 2,
                    vk::PresentMode::Mailbox => 1,
                    vk::PresentMode::Immediate => 0,
                    _ => unreachable!(),
                }
            },
            VSync::Disable => {
                match present_mode {
                    vk::PresentMode::Mailbox => 3,
                    vk::PresentMode::Immediate => 2,
                    vk::PresentMode::Fifo => 1,
                    vk::PresentMode::FifoRelaxed => 0,
                    _ => unreachable!(),
                }
            },
        }
    });

    present_modes.pop().unwrap()
}
