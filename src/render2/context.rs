mod vk {
    pub use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer};
    pub use vulkano::command_buffer::RenderPassBeginInfo;
    pub use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
    pub use vulkano::descriptor_set::layout::DescriptorSetLayout;
    pub use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
    pub use vulkano::device::Device;
    pub use vulkano::format::{ClearColorValue, Format, FormatFeatures, NumericFormat};
    pub use vulkano::image::sampler::{Sampler, SamplerAddressMode, SamplerCreateInfo};
    pub use vulkano::image::view::ImageView;
    pub use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage, SampleCount};
    pub use vulkano::memory::allocator::{
        AllocationCreateInfo, MemoryAllocatePreference, MemoryTypeFilter,
    };
    pub use vulkano::memory::MemoryPropertyFlags;
    pub use vulkano::pipeline::graphics::color_blend::{
        AttachmentBlend, ColorBlendAttachmentState, ColorBlendState,
    };
    pub use vulkano::pipeline::graphics::input_assembly::InputAssemblyState;
    pub use vulkano::pipeline::graphics::multisample::MultisampleState;
    pub use vulkano::pipeline::graphics::rasterization::RasterizationState;
    pub use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
    pub use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
    pub use vulkano::pipeline::{
        DynamicState, GraphicsPipeline, PipelineBindPoint, PipelineLayout,
        PipelineShaderStageCreateInfo,
    };
    pub use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
    pub use vulkano::swapchain::{
        ColorSpace, FullScreenExclusive, PresentGravity, PresentGravityFlags, PresentMode,
        PresentScaling, PresentScalingFlags, Swapchain, SwapchainCreateInfo,
    };
    pub use vulkano::{Validated, VulkanError};
    pub use vulkano_taskgraph::command_buffer::{ClearColorImageInfo, RecordingCommandBuffer};
    pub use vulkano_taskgraph::graph::{
        CompileInfo, ExecutableTaskGraph, ExecuteError, ResourceMap, TaskGraph,
    };
    pub use vulkano_taskgraph::resource::{AccessType, Flight, ImageLayoutType, Resources};
    pub use vulkano_taskgraph::{
        execute, resource_map, Id, QueueFamilyType, Task, TaskContext, TaskResult,
    };
}

use std::iter;
use std::ops::Range;
use std::sync::Arc;

use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};
use vulkano::pipeline::Pipeline;

use super::{clear_color_value_for_format, clear_value_for_format, shaders, VSync, MSAA};
use crate::interface::ItfVertInfo;
use crate::window::Window;

pub struct Context {
    window: Arc<Window>,
    image_format: vk::Format,
    render_flt_id: vk::Id<vk::Flight>,
    swapchain_id: vk::Id<vk::Swapchain>,
    swapchain_ci: vk::SwapchainCreateInfo,
    swapchain_rc: bool,
    viewport: vk::Viewport,
    msaa: MSAA,
    image_capacity: u32,
    specific: Specific,
    buffer_id: Option<vk::Id<vk::Buffer>>,
    draw_range: Option<Range<u32>>,
    image_ids: Vec<vk::Id<vk::Image>>,
    desc_set: Option<Arc<vk::DescriptorSet>>,
    default_image_id: vk::Id<vk::Image>,
    default_image_view: Arc<vk::ImageView>,
    sampler: Arc<vk::Sampler>,
    desc_alloc: Arc<vk::StandardDescriptorSetAllocator>,
    desc_layout: Option<Arc<vk::DescriptorSetLayout>>,
}

enum Specific {
    ItfOnly(ItfOnly),
    Minimal(Minimal),
    None,
}

struct ItfOnly {
    render_pass: Option<Arc<vk::RenderPass>>,
    pipeline: Option<Arc<vk::GraphicsPipeline>>,
    color_ms_id: Option<vk::Id<vk::Image>>,
    framebuffers: Option<Vec<Arc<vk::Framebuffer>>>,
    task_graph: Option<vk::ExecutableTaskGraph<Context>>,
    virtual_ids: Option<VirtualIds>,
}

struct Minimal {
    task_graph: vk::ExecutableTaskGraph<Context>,
    virtual_swapchain_id: vk::Id<vk::Swapchain>,
}

enum VirtualIds {
    ItfOnlyNoMsaa(ItfOnlyNoMsaaVIds),
    ItfOnlyMsaa(ItfOnlyMsaaVIds),
}

struct ItfOnlyNoMsaaVIds {
    swapchain: vk::Id<vk::Swapchain>,
    buffer: vk::Id<vk::Buffer>,
    images: Vec<vk::Id<vk::Image>>,
}

struct ItfOnlyMsaaVIds {
    swapchain: vk::Id<vk::Swapchain>,
    color_ms: vk::Id<vk::Image>,
    buffer: vk::Id<vk::Buffer>,
    images: Vec<vk::Id<vk::Image>>,
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

        let mut viewport = vk::Viewport {
            offset: [0.0, 0.0],
            extent: [
                swapchain_ci.image_extent[0] as f32,
                swapchain_ci.image_extent[1] as f32,
            ],
            depth_range: 0.0..=1.0,
        };

        let image_format = if surface_format.components()[0] > 8 {
            vec![
                vk::Format::R16G16B16A16_UINT,
                vk::Format::R16G16B16A16_UNORM,
                vk::Format::R8G8B8A8_UINT,
                vk::Format::R8G8B8A8_UNORM,
                vk::Format::B8G8R8A8_UINT,
                vk::Format::B8G8R8A8_UNORM,
                vk::Format::A8B8G8R8_UINT_PACK32,
                vk::Format::A8B8G8R8_UNORM_PACK32,
                vk::Format::R8G8B8A8_SRGB,
                vk::Format::B8G8R8A8_SRGB,
                vk::Format::A8B8G8R8_SRGB_PACK32,
            ]
        } else {
            vec![
                vk::Format::R8G8B8A8_UINT,
                vk::Format::R8G8B8A8_UNORM,
                vk::Format::B8G8R8A8_UINT,
                vk::Format::B8G8R8A8_UNORM,
                vk::Format::A8B8G8R8_UINT_PACK32,
                vk::Format::A8B8G8R8_UNORM_PACK32,
                vk::Format::R8G8B8A8_SRGB,
                vk::Format::B8G8R8A8_SRGB,
                vk::Format::A8B8G8R8_SRGB_PACK32,
            ]
        }
        .into_iter()
        .find(|format| {
            let properties = match window
                .basalt_ref()
                .physical_device_ref()
                .format_properties(*format)
            {
                Ok(ok) => ok,
                Err(_) => return false,
            };

            properties.optimal_tiling_features.contains(
                vk::FormatFeatures::TRANSFER_DST
                    | vk::FormatFeatures::TRANSFER_SRC
                    | vk::FormatFeatures::SAMPLED_IMAGE
                    | vk::FormatFeatures::SAMPLED_IMAGE_FILTER_LINEAR,
            )
        })
        .ok_or(String::from("Failed to find suitable image format."))?;

        let sampler = vk::Sampler::new(
            window.basalt_ref().device(),
            vk::SamplerCreateInfo {
                address_mode: [vk::SamplerAddressMode::ClampToBorder; 3],
                unnormalized_coordinates: true,
                ..Default::default()
            },
        )
        .unwrap();

        let desc_alloc = Arc::new(vk::StandardDescriptorSetAllocator::new(
            window.basalt_ref().device(),
            Default::default(),
        ));

        let default_image_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_image(
                vk::ImageCreateInfo {
                    format: image_format,
                    extent: [1; 3],
                    usage: vk::ImageUsage::SAMPLED | vk::ImageUsage::TRANSFER_DST,
                    ..Default::default()
                },
                vk::AllocationCreateInfo {
                    memory_type_filter: vk::MemoryTypeFilter {
                        preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        not_preferred_flags: vk::MemoryPropertyFlags::HOST_CACHED,
                        ..vk::MemoryTypeFilter::empty()
                    },
                    allocate_preference: vk::MemoryAllocatePreference::AlwaysAllocate,
                    ..Default::default()
                },
            )
            .unwrap();

        unsafe {
            vk::execute(
                window.basalt_ref().graphics_queue_ref(),
                window.basalt_ref().device_resources_ref(),
                render_flt_id,
                |cmd, _| {
                    cmd.clear_color_image(&vk::ClearColorImageInfo {
                        image: default_image_id,
                        clear_value: clear_color_value_for_format(image_format),
                        ..Default::default()
                    })
                    .unwrap();

                    Ok(())
                },
                [],
                [],
                [(
                    default_image_id,
                    vk::AccessType::ClearTransferWrite,
                    vk::ImageLayoutType::Optimal,
                )],
            )
            .unwrap();
        }

        let default_image_view = vk::ImageView::new_default(
            window
                .basalt_ref()
                .device_resources_ref()
                .image(default_image_id)
                .unwrap()
                .image()
                .clone(),
        )
        .unwrap();

        let msaa = window.renderer_msaa();

        Ok(Self {
            window,
            image_format,
            render_flt_id,
            swapchain_id,
            swapchain_ci,
            swapchain_rc: false,
            viewport,
            image_capacity: 4,
            msaa,
            specific: Specific::None,
            sampler,
            desc_alloc,
            desc_layout: None,
            default_image_id,
            default_image_view,
            buffer_id: None,
            image_ids: Vec::new(),
            draw_range: None,
            desc_set: None,
        })
    }

    pub fn itf_only(&mut self) {
        self.specific = Specific::ItfOnly(ItfOnly {
            render_pass: None,
            pipeline: None,
            color_ms_id: None,
            framebuffers: None,
            task_graph: None,
            virtual_ids: None,
        });
    }

    pub fn minimal(&mut self) -> Result<(), String> {
        Minimal::create(self)
    }

    pub fn image_format(&self) -> vk::Format {
        self.image_format
    }

    pub fn check_extent(&mut self) {
        let current_extent = self
            .window
            .surface_current_extent(self.swapchain_ci.full_screen_exclusive);

        if current_extent == self.swapchain_ci.image_extent {
            return;
        }

        self.swapchain_rc = true;
    }

    pub fn set_msaa(&mut self, msaa: MSAA) {
        if msaa == self.msaa {
            return;
        }

        match &mut self.specific {
            Specific::Minimal(_) | Specific::None => (),
            Specific::ItfOnly(specific) => {
                specific.render_pass = None;
                specific.framebuffers = None;
                specific.task_graph = None;
                specific.virtual_ids = None;
            },
        }
    }

    pub fn set_vsync(&mut self, vsync: VSync) {
        let present_mode =
            find_present_mode(&self.window, self.swapchain_ci.full_screen_exclusive, vsync);

        if present_mode == self.swapchain_ci.present_mode {
            return;
        }

        self.swapchain_ci.present_mode = present_mode;
        self.swapchain_rc = true;
    }

    pub fn set_buffer_and_images(
        &mut self,
        buffer_id: vk::Id<vk::Buffer>,
        image_ids: Vec<vk::Id<vk::Image>>,
        draw_range: Range<u32>,
    ) {
        if image_ids.len() as u32 > self.image_capacity {
            while self.image_capacity < image_ids.len() as u32 {
                self.image_capacity *= 2;
            }

            match &mut self.specific {
                Specific::Minimal(_) | Specific::None => (),
                Specific::ItfOnly(specific) => {
                    specific.pipeline = None;
                    specific.task_graph = None;
                },
            }

            if let Some(old_layout) = self.desc_layout.take() {
                self.desc_alloc.clear(&old_layout);
            }

            self.desc_set = None;
        }

        if self.desc_layout.is_none() {
            self.desc_layout = Some(
                vk::DescriptorSetLayout::new(
                    self.window.basalt_ref().device(),
                    shaders::pipeline_descriptor_set_layout_create_info(self.image_capacity)
                        .set_layouts[0]
                        .clone(),
                )
                .unwrap(),
            );
        }

        let num_default_images = self.image_capacity as usize - image_ids.len();

        self.desc_set = Some(
            vk::DescriptorSet::new_variable(
                self.desc_alloc.clone(),
                self.desc_layout.clone().unwrap(),
                self.image_capacity,
                [
                    vk::WriteDescriptorSet::sampler(0, self.sampler.clone()),
                    vk::WriteDescriptorSet::image_view_array(
                        1,
                        0,
                        image_ids
                            .iter()
                            .map(|image_id| {
                                vk::ImageView::new_default(
                                    self.window
                                        .basalt_ref()
                                        .device_resources_ref()
                                        .image(*image_id)
                                        .unwrap()
                                        .image()
                                        .clone(),
                                )
                                .unwrap()
                            })
                            .chain(
                                (0..num_default_images).map(|_| self.default_image_view.clone()),
                            ),
                    ),
                ],
                [],
            )
            .unwrap(),
        );

        self.buffer_id = Some(buffer_id);
        self.image_ids = image_ids;
        self.draw_range = Some(draw_range);
    }

    fn update(&mut self) -> Result<(), String> {
        let mut framebuffers_rc = false;

        if self.swapchain_rc {
            self.swapchain_ci.image_extent = self
                .window
                .surface_current_extent(self.swapchain_ci.full_screen_exclusive);

            self.viewport.extent = [
                self.swapchain_ci.image_extent[0] as f32,
                self.swapchain_ci.image_extent[1] as f32,
            ];

            self.swapchain_id = self
                .window
                .basalt_ref()
                .device_resources_ref()
                .recreate_swapchain(self.swapchain_id, |_| self.swapchain_ci.clone())
                .map_err(|e| format!("Failed to recreate swapchain: {}", e))?;

            self.swapchain_rc = false;
            framebuffers_rc = true;
        }

        match &mut self.specific {
            Specific::Minimal(_) | Specific::None => (),
            Specific::ItfOnly(specific) => {
                if specific.render_pass.is_none() {
                    if self.msaa == MSAA::X1 {
                        specific.render_pass = Some(
                            vulkano::single_pass_renderpass!(
                                self.window.basalt_ref().device(),
                                attachments: {
                                    color: {
                                        format: self.swapchain_ci.image_format,
                                        samples: 1,
                                        load_op: Clear,
                                        store_op: Store,
                                    },
                                },
                                pass: {
                                    color: [color],
                                    depth_stencil: {},
                                }
                            )
                            .unwrap(),
                        );
                    } else {
                        let sample_count = match self.msaa {
                            MSAA::X1 => unreachable!(),
                            MSAA::X2 => 2,
                            MSAA::X4 => 4,
                            MSAA::X8 => 8,
                        };

                        specific.render_pass = Some(
                            vulkano::single_pass_renderpass!(
                                self.window.basalt_ref().device(),
                                attachments: {
                                    color_ms: {
                                        format: self.swapchain_ci.image_format,
                                        samples: sample_count,
                                        load_op: Clear,
                                        store_op: DontCare,
                                    },
                                    color: {
                                        format: self.swapchain_ci.image_format,
                                        samples: 1,
                                        load_op: DontCare,
                                        store_op: Store,
                                    },
                                },
                                pass: {
                                    color: [color_ms],
                                    color_resolve: [color],
                                    depth_stencil: {},
                                }
                            )
                            .unwrap(),
                        );
                    }
                }

                if specific.pipeline.is_none() {
                    specific.pipeline = Some(create_itf_pipeline(
                        self.window.basalt_ref().device(),
                        self.image_capacity,
                        self.msaa,
                        vk::Subpass::from(specific.render_pass.clone().unwrap(), 0).unwrap(),
                    ));
                }

                if framebuffers_rc || specific.framebuffers.is_none() {
                    if let Some(color_ms_id) = specific.color_ms_id.take() {
                        unsafe {
                            self.window
                                .basalt_ref()
                                .device_resources_ref()
                                .remove_image(color_ms_id);
                        }
                    }

                    let swapchain_state = self
                        .window
                        .basalt_ref()
                        .device_resources_ref()
                        .swapchain(self.swapchain_id)
                        .unwrap();

                    let swapchain_images = swapchain_state.images();

                    if self.msaa == MSAA::X1 {
                        specific.framebuffers = Some(
                            swapchain_state
                                .images()
                                .iter()
                                .map(|sc_image| {
                                    vk::Framebuffer::new(
                                        specific.render_pass.clone().unwrap(),
                                        vk::FramebufferCreateInfo {
                                            attachments: vec![vk::ImageView::new_default(
                                                sc_image.clone(),
                                            )
                                            .unwrap()],
                                            ..Default::default()
                                        },
                                    )
                                    .unwrap()
                                })
                                .collect::<Vec<_>>(),
                        );
                    } else {
                        let sample_count = match self.msaa {
                            MSAA::X1 => unreachable!(),
                            MSAA::X2 => vk::SampleCount::Sample2,
                            MSAA::X4 => vk::SampleCount::Sample4,
                            MSAA::X8 => vk::SampleCount::Sample8,
                        };

                        let color_ms_id = self
                            .window
                            .basalt_ref()
                            .device_resources()
                            .create_image(
                                vk::ImageCreateInfo {
                                    image_type: vk::ImageType::Dim2d,
                                    format: self.swapchain_ci.image_format,
                                    extent: [
                                        self.swapchain_ci.image_extent[0],
                                        self.swapchain_ci.image_extent[1],
                                        1,
                                    ],
                                    usage: vk::ImageUsage::COLOR_ATTACHMENT
                                        | vk::ImageUsage::TRANSIENT_ATTACHMENT,
                                    samples: sample_count,
                                    ..vk::ImageCreateInfo::default()
                                },
                                vk::AllocationCreateInfo {
                                    memory_type_filter: vk::MemoryTypeFilter {
                                        preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                        not_preferred_flags: vk::MemoryPropertyFlags::HOST_CACHED,
                                        ..vk::MemoryTypeFilter::empty()
                                    },
                                    allocate_preference:
                                        vk::MemoryAllocatePreference::AlwaysAllocate,
                                    ..vk::AllocationCreateInfo::default()
                                },
                            )
                            .unwrap();

                        specific.color_ms_id = Some(color_ms_id);

                        let color_ms_state = self
                            .window
                            .basalt_ref()
                            .device_resources_ref()
                            .image(color_ms_id)
                            .unwrap();

                        specific.framebuffers = Some(
                            swapchain_state
                                .images()
                                .iter()
                                .map(|sc_image| {
                                    vk::Framebuffer::new(
                                        specific.render_pass.clone().unwrap(),
                                        vk::FramebufferCreateInfo {
                                            attachments: vec![
                                                vk::ImageView::new_default(
                                                    color_ms_state.image().clone(),
                                                )
                                                .unwrap(),
                                                vk::ImageView::new_default(sc_image.clone())
                                                    .unwrap(),
                                            ],
                                            ..Default::default()
                                        },
                                    )
                                    .unwrap()
                                })
                                .collect::<Vec<_>>(),
                        );
                    }
                }

                if specific.task_graph.is_none() {
                    let mut task_graph =
                        vk::TaskGraph::new(self.window.basalt_ref().device_resources_ref(), 1, 1);
                    let vid_swapchain = task_graph.add_swapchain(&self.swapchain_ci);

                    let vid_buffer = task_graph.add_buffer(&vk::BufferCreateInfo {
                        usage: vk::BufferUsage::TRANSFER_SRC
                            | vk::BufferUsage::TRANSFER_DST
                            | vk::BufferUsage::VERTEX_BUFFER,
                        ..Default::default()
                    });

                    let vid_images = (0..self.image_capacity)
                        .map(|_| {
                            task_graph.add_image(&vk::ImageCreateInfo {
                                image_type: vk::ImageType::Dim2d,
                                format: self.image_format,
                                usage: vk::ImageUsage::TRANSFER_SRC
                                    | vk::ImageUsage::TRANSFER_DST
                                    | vk::ImageUsage::SAMPLED,
                                ..Default::default()
                            })
                        })
                        .collect::<Vec<_>>();

                    let vid_color_ms = (self.msaa != MSAA::X1).then(|| {
                        task_graph.add_image(&vk::ImageCreateInfo {
                            image_type: vk::ImageType::Dim2d,
                            format: self.swapchain_ci.image_format,
                            usage: vk::ImageUsage::COLOR_ATTACHMENT
                                | vk::ImageUsage::TRANSIENT_ATTACHMENT,
                            samples: match self.msaa {
                                MSAA::X1 => unreachable!(),
                                MSAA::X2 => vk::SampleCount::Sample2,
                                MSAA::X4 => vk::SampleCount::Sample4,
                                MSAA::X8 => vk::SampleCount::Sample8,
                            },
                            ..Default::default()
                        })
                    });

                    let mut node = task_graph.create_task_node(
                        format!("Render[{:?}]", self.window.id()),
                        vk::QueueFamilyType::Graphics,
                        RenderTask,
                    );

                    node.buffer_access(vid_buffer, vk::AccessType::VertexAttributeRead);

                    for vid_image in vid_images.iter() {
                        node.image_access(
                            *vid_image,
                            vk::AccessType::FragmentShaderSampledRead,
                            vk::ImageLayoutType::Optimal,
                        );
                    }

                    let virtual_ids = if self.msaa == MSAA::X1 {
                        node.image_access(
                            vid_swapchain.current_image_id(),
                            vk::AccessType::ColorAttachmentWrite,
                            vk::ImageLayoutType::Optimal,
                        );

                        VirtualIds::ItfOnlyNoMsaa(ItfOnlyNoMsaaVIds {
                            swapchain: vid_swapchain,
                            buffer: vid_buffer,
                            images: vid_images,
                        })
                    } else {
                        let vid_color_ms = vid_color_ms.unwrap();

                        // TODO: vid_color_ms is accessed as ColorAttachmentWrite & ResolveTransferRead
                        //       how is that suppose to be handled?

                        node.image_access(
                            vid_swapchain.current_image_id(),
                            vk::AccessType::ResolveTransferWrite,
                            vk::ImageLayoutType::Optimal,
                        )
                        .image_access(
                            vid_color_ms,
                            vk::AccessType::ColorAttachmentWrite,
                            vk::ImageLayoutType::Optimal,
                        );

                        VirtualIds::ItfOnlyMsaa(ItfOnlyMsaaVIds {
                            swapchain: vid_swapchain,
                            color_ms: vid_color_ms,
                            buffer: vid_buffer,
                            images: vid_images,
                        })
                    };

                    specific.task_graph = Some(unsafe {
                        task_graph
                            .compile(&vk::CompileInfo {
                                queues: &[self.window.basalt_ref().graphics_queue_ref()],
                                present_queue: Some(self.window.basalt_ref().graphics_queue_ref()),
                                flight_id: self.render_flt_id,
                                ..Default::default()
                            })
                            .unwrap()
                    });

                    specific.virtual_ids = Some(virtual_ids);
                }
            },
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
        
        let exec_result = match &self.specific {
            Specific::ItfOnly(specific) => {
                // TODO: Move this above match specific after minimal is gone
                let buffer_id = match self.buffer_id {
                    Some(some) => some,
                    None => return Ok(()),
                };

                let resource_map = match specific.virtual_ids.as_ref().unwrap() {
                    VirtualIds::ItfOnlyNoMsaa(vids) => {
                        let mut map =
                            vk::ResourceMap::new(specific.task_graph.as_ref().unwrap()).unwrap();

                        map.insert_swapchain(vids.swapchain, self.swapchain_id);
                        map.insert_buffer(vids.buffer, buffer_id);
                        let mut image_ids_iter = self.image_ids.iter();

                        for vid_image in vids.images.iter() {
                            let image_id = match image_ids_iter.next() {
                                Some(image_id) => *image_id,
                                None => self.default_image_id,
                            };

                            // TODO: This probably panics when default_image_id is used multiple times?
                            map.insert_image(*vid_image, image_id);
                        }

                        map
                    },
                    VirtualIds::ItfOnlyMsaa(vids) => {
                        let mut map =
                            vk::ResourceMap::new(specific.task_graph.as_ref().unwrap()).unwrap();

                        map.insert_swapchain(vids.swapchain, self.swapchain_id);
                        map.insert_buffer(vids.buffer, buffer_id);
                        map.insert_image(vids.color_ms, specific.color_ms_id.unwrap());
                        let mut image_ids_iter = self.image_ids.iter();

                        for vid_image in vids.images.iter() {
                            let image_id = match image_ids_iter.next() {
                                Some(image_id) => *image_id,
                                None => self.default_image_id,
                            };

                            // TODO: Same as above TODO: ^
                            map.insert_image(*vid_image, image_id);
                        }

                        map
                    },
                };

                flight.wait(None).unwrap();

                unsafe {
                    specific
                        .task_graph
                        .as_ref()
                        .unwrap()
                        .execute(resource_map, self, || ())
                }
            },
            Specific::Minimal(specific) => {
                let resource_map = vk::resource_map!(
                    &specific.task_graph,
                    specific.virtual_swapchain_id => self.swapchain_id,
                )
                .unwrap();

                flight.wait(None).unwrap();
                unsafe { specific.task_graph.execute(resource_map, self, || ()) }
            },
            Specific::None => panic!("Renderer mode not set!"),
        };

        match exec_result {
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

        Ok(())
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        match &mut self.specific {
            Specific::Minimal(_) | Specific::None => (),
            Specific::ItfOnly(specific) => {
                if let Some(color_ms_id) = specific.color_ms_id.take() {
                    unsafe {
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .remove_image(color_ms_id);
                    }
                }
            },
        }
    }
}

fn create_itf_pipeline(
    device: Arc<vk::Device>,
    image_capacity: u32,
    msaa: MSAA,
    subpass: vk::Subpass,
) -> Arc<vk::GraphicsPipeline> {
    let ui_vs = shaders::ui_vs_sm(device.clone())
        .entry_point("main")
        .unwrap();

    let ui_fs = shaders::ui_fs_sm(device.clone())
        .entry_point("main")
        .unwrap();

    let vertex_input_state = ItfVertInfo::per_vertex().definition(&ui_vs).unwrap();

    let stages = [
        vk::PipelineShaderStageCreateInfo::new(ui_vs),
        vk::PipelineShaderStageCreateInfo::new(ui_fs),
    ];

    let layout = vk::PipelineLayout::new(
        device.clone(),
        shaders::pipeline_descriptor_set_layout_create_info(image_capacity)
            .into_pipeline_layout_create_info(device.clone())
            .unwrap(),
    )
    .unwrap();

    let sample_count = match msaa {
        MSAA::X1 => vk::SampleCount::Sample1,
        MSAA::X2 => vk::SampleCount::Sample2,
        MSAA::X4 => vk::SampleCount::Sample4,
        MSAA::X8 => vk::SampleCount::Sample8,
    };

    vk::GraphicsPipeline::new(
        device,
        None,
        vk::GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: Some(vk::InputAssemblyState::default()),
            viewport_state: Some(vk::ViewportState::default()),
            rasterization_state: Some(vk::RasterizationState::default()),
            multisample_state: Some(vk::MultisampleState {
                rasterization_samples: sample_count,
                ..vk::MultisampleState::default()
            }),
            color_blend_state: Some(vk::ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                vk::ColorBlendAttachmentState {
                    blend: Some(vk::AttachmentBlend::alpha()),
                    ..vk::ColorBlendAttachmentState::default()
                },
            )),
            dynamic_state: [vk::DynamicState::Viewport].into_iter().collect(),
            subpass: Some(subpass.into()),
            ..vk::GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .unwrap()
}

impl Minimal {
    pub fn create(context: &mut Context) -> Result<(), String> {
        let mut task_graph =
            vk::TaskGraph::new(context.window.basalt_ref().device_resources_ref(), 1, 1);
        let virtual_swapchain_id = task_graph.add_swapchain(&context.swapchain_ci);

        task_graph
            .create_task_node("Render", vk::QueueFamilyType::Graphics, RenderTask)
            .image_access(
                virtual_swapchain_id.current_image_id(),
                vk::AccessType::ClearTransferWrite,
                vk::ImageLayoutType::Optimal,
            )
            .build();

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
            Specific::ItfOnly(specific) => {
                let framebuffers = specific.framebuffers.as_ref().unwrap();
                let pipeline = specific.pipeline.as_ref().unwrap();

                let clear_values = if specific.color_ms_id.is_some() {
                    vec![
                        Some(clear_value_for_format(
                            framebuffers[0].attachments()[0].format(),
                        )),
                        None,
                    ]
                } else {
                    vec![Some(clear_value_for_format(
                        framebuffers[0].attachments()[0].format(),
                    ))]
                };

                cmd.as_raw().begin_render_pass(
                    &vk::RenderPassBeginInfo {
                        clear_values,
                        ..vk::RenderPassBeginInfo::framebuffer(
                            framebuffers[image_index as usize].clone(),
                        )
                    },
                    &Default::default(),
                )?;

                cmd.destroy_objects(iter::once(framebuffers[image_index as usize].clone()));
                cmd.set_viewport(0, std::slice::from_ref(&context.viewport))?;
                cmd.bind_pipeline_graphics(pipeline)?;

                if let (Some(desc_set), Some(buffer_id), Some(draw_range)) = (
                    context.desc_set.as_ref(),
                    context.buffer_id.as_ref(),
                    context.draw_range.clone(),
                ) {
                    cmd.as_raw().bind_descriptor_sets(
                        vk::PipelineBindPoint::Graphics,
                        pipeline.layout(),
                        0,
                        &[desc_set.as_raw()],
                        &[],
                    )?;

                    cmd.destroy_objects(iter::once(desc_set.clone()));
                    cmd.bind_vertex_buffers(0, &[*buffer_id], &[0], &[], &[])?;

                    unsafe {
                        cmd.draw(draw_range.end - draw_range.start, 1, draw_range.start, 0)?;
                    }
                }

                cmd.as_raw().end_render_pass(&Default::default())?;
            },
            Specific::Minimal(minimal) => {
                /*cmd.clear_color_image(&vk::ClearColorImageInfo {
                    image: context.swapchain_id.current_image_id(),
                    clear_value: vk::ClearColorValue::Float([0.0; 4]),
                    ..Default::default()
                })
                .unwrap();*/
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
