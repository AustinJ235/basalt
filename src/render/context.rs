use std::any::Any;
use std::iter;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use smallvec::SmallVec;
use vulkano::pipeline::Pipeline;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};

mod vko {
    pub use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
    pub use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
    pub use vulkano::descriptor_set::layout::DescriptorSetLayout;
    pub use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
    pub use vulkano::device::Device;
    pub use vulkano::format::{ClearValue, Format, FormatFeatures, NumericFormat};
    pub use vulkano::image::sampler::{Sampler, SamplerAddressMode, SamplerCreateInfo};
    pub use vulkano::image::view::ImageView;
    pub use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage, SampleCount};
    pub use vulkano::memory::MemoryPropertyFlags;
    pub use vulkano::memory::allocator::{
        AllocationCreateInfo, MemoryAllocatePreference, MemoryTypeFilter,
    };
    pub use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
    pub use vulkano::pipeline::graphics::color_blend::{
        AttachmentBlend, ColorBlendAttachmentState, ColorBlendState,
    };
    pub use vulkano::pipeline::graphics::input_assembly::InputAssemblyState;
    pub use vulkano::pipeline::graphics::multisample::MultisampleState;
    pub use vulkano::pipeline::graphics::rasterization::RasterizationState;
    pub use vulkano::pipeline::graphics::vertex_input::VertexInputState;
    pub use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
    pub use vulkano::pipeline::layout::PipelineDescriptorSetLayoutCreateInfo;
    pub use vulkano::pipeline::{
        DynamicState, GraphicsPipeline, PipelineBindPoint, PipelineLayout,
        PipelineShaderStageCreateInfo,
    };
    pub use vulkano::render_pass::Subpass;
    pub use vulkano::swapchain::{
        ColorSpace, FullScreenExclusive, PresentGravity, PresentGravityFlags, PresentMode,
        PresentScaling, PresentScalingFlags, Swapchain, SwapchainCreateInfo,
    };
    pub use vulkano::sync::Sharing;
    pub use vulkano::{Validated, VulkanError};
    pub use vulkano_taskgraph::command_buffer::{ClearColorImageInfo, RecordingCommandBuffer};
    pub use vulkano_taskgraph::graph::{
        AttachmentInfo, CompileInfo, ExecutableTaskGraph, ExecuteError, NodeId, ResourceMap,
        TaskGraph,
    };
    pub use vulkano_taskgraph::resource::{AccessTypes, Flight, ImageLayoutType, Resources};
    pub use vulkano_taskgraph::{
        ClearValues, Id, QueueFamilyType, Task, TaskContext, TaskResult, execute,
    };
}

use crate::interface::ItfVertInfo;
use crate::render::{
    ContextCreateError, ContextError, MSAA, MetricsState, UserRenderer, VSync, VulkanoError,
    clear_color_value_for_format, clear_value_for_format, shaders,
};
use crate::window::Window;

/// The internal rendering context.
///
/// This is only accessible during task execution.
pub struct RendererContext {
    window: Arc<Window>,
    image_format: vko::Format,
    render_flt_id: vko::Id<vko::Flight>,
    resource_sharing: vko::Sharing<SmallVec<[u32; 4]>>,
    swapchain_id: vko::Id<vko::Swapchain>,
    swapchain_ci: vko::SwapchainCreateInfo,
    swapchain_rc: bool,
    viewport: vko::Viewport,
    msaa: MSAA,
    image_capacity: u32,
    specific: Specific,
    buffer_id: Option<vko::Id<vko::Buffer>>,
    draw_count: Option<u32>,
    update_token: Option<Arc<(Mutex<Option<u64>>, Condvar)>>,
    image_ids: Vec<vko::Id<vko::Image>>,
    desc_set: Option<Arc<vko::DescriptorSet>>,
    default_image_id: vko::Id<vko::Image>,
    default_image_view: Arc<vko::ImageView>,
    sampler: Arc<vko::Sampler>,
    desc_alloc: Arc<vko::StandardDescriptorSetAllocator>,
    desc_layout: Option<Arc<vko::DescriptorSetLayout>>,
    user_renderer: Option<Box<dyn UserRenderer>>,
}

enum Specific {
    ItfOnly(ItfOnly),
    User(User),
    None,
}

impl Specific {
    fn remove_images(&mut self, resources: &Arc<vko::Resources>) {
        match self {
            Specific::None => (),
            Specific::ItfOnly(specific) => specific.remove_images(resources),
            Specific::User(specific) => specific.remove_images(resources),
        }
    }
}

struct ItfOnly {
    color_ms_id: Option<vko::Id<vko::Image>>,
    task_graph: Option<vko::ExecutableTaskGraph<RendererContext>>,
    virtual_ids: Option<ItfOnlyVids>,
}

impl ItfOnly {
    fn remove_images(&mut self, resources: &Arc<vko::Resources>) {
        if let Some(color_ms_id) = self.color_ms_id.take() {
            unsafe {
                let _ = resources.remove_image(color_ms_id);
            }
        }
    }
}

struct User {
    itf_color_id: Option<vko::Id<vko::Image>>,
    itf_color_ms_id: Option<vko::Id<vko::Image>>,
    user_color_id: Option<vko::Id<vko::Image>>,
    final_desc_layout: Option<Arc<vko::DescriptorSetLayout>>,
    task_graph: Option<vko::ExecutableTaskGraph<RendererContext>>,
    virtual_ids: Option<UserVids>,
}

impl User {
    fn remove_images(&mut self, resources: &Arc<vko::Resources>) {
        if let Some(itf_color_id) = self.itf_color_id.take() {
            unsafe {
                let _ = resources.remove_image(itf_color_id);
            }
        }

        if let Some(itf_color_ms_id) = self.itf_color_ms_id.take() {
            unsafe {
                let _ = resources.remove_image(itf_color_ms_id);
            }
        }

        if let Some(user_color_id) = self.user_color_id.take() {
            unsafe {
                let _ = resources.remove_image(user_color_id);
            }
        }
    }
}

struct ItfOnlyVids {
    swapchain: vko::Id<vko::Swapchain>,
    buffer: vko::Id<vko::Buffer>,
    color_ms: Option<vko::Id<vko::Image>>,
}

struct UserVids {
    final_node: vko::NodeId,
    swapchain: vko::Id<vko::Swapchain>,
    buffer: vko::Id<vko::Buffer>,
    itf_color: vko::Id<vko::Image>,
    itf_color_ms: Option<vko::Id<vko::Image>>,
    user_color: vko::Id<vko::Image>,
}

impl RendererContext {
    pub(in crate::render) fn new(
        window: Arc<Window>,
        render_flt_id: vko::Id<vko::Flight>,
        resource_sharing: vko::Sharing<SmallVec<[u32; 4]>>,
    ) -> Result<Self, ContextCreateError> {
        let (fullscreen_mode, win32_monitor) = match window
            .basalt_ref()
            .device_ref()
            .enabled_extensions()
            .ext_full_screen_exclusive
        {
            true => {
                (
                    vko::FullScreenExclusive::ApplicationControlled,
                    window.win32_monitor(),
                )
            },
            false => (vko::FullScreenExclusive::Default, None),
        };

        let present_mode = find_present_mode(&window, fullscreen_mode, window.renderer_vsync());
        let mut surface_formats = window.surface_formats(fullscreen_mode, present_mode);

        /*let ext_swapchain_colorspace = window
        .basalt_ref()
        .instance_ref()
        .enabled_extensions()
        .ext_swapchain_colorspace;*/

        surface_formats.retain(|(format, colorspace)| {
            if !match colorspace {
                vko::ColorSpace::SrgbNonLinear => true,
                // TODO: Support these properly, these are for hdr mainly. Typically the format
                //       is a signed float where values are allowed to be less than zero or greater
                //       one. The main problem currently is that anything that falls in the normal
                //       range don't appear as bright as one would expect on a hdr display.
                // vko::ColorSpace::ExtendedSrgbLinear => ext_swapchain_colorspace,
                // vko::ColorSpace::ExtendedSrgbNonLinear => ext_swapchain_colorspace,
                _ => false,
            } {
                return false;
            }

            // TODO: Support non SRGB formats properly. When writing to a non-SRGB format using the
            //       SrgbNonLinear colorspace, colors written will be assumed to be SRGB. This
            //       causes issues since everything is done with linear color.
            if format.numeric_format_color() != Some(vko::NumericFormat::SRGB) {
                return false;
            }

            true
        });

        surface_formats.sort_by_key(|(format, _colorspace)| format.components()[0]);

        let (surface_format, surface_colorspace) = surface_formats
            .pop()
            .ok_or(ContextCreateError::NoSuitableSwapchainFormat)?;

        let surface_capabilities = window.surface_capabilities(fullscreen_mode, present_mode);

        let (scaling_behavior, present_gravity) = if window
            .basalt_ref()
            .device_ref()
            .enabled_extensions()
            .ext_swapchain_maintenance1
        {
            let scaling = if surface_capabilities
                .supported_present_scaling
                .contains(vko::PresentScalingFlags::ONE_TO_ONE)
            {
                Some(vko::PresentScaling::OneToOne)
            } else {
                None
            };

            let gravity = if surface_capabilities.supported_present_gravity[0]
                .contains(vko::PresentGravityFlags::MIN)
                && surface_capabilities.supported_present_gravity[1]
                    .contains(vko::PresentGravityFlags::MIN)
            {
                Some([vko::PresentGravity::Min, vko::PresentGravity::Min])
            } else {
                None
            };

            (scaling, gravity)
        } else {
            (None, None)
        };

        let swapchain_ci = vko::SwapchainCreateInfo {
            min_image_count: surface_capabilities.min_image_count.max(2),
            image_format: surface_format,
            image_color_space: surface_colorspace,
            image_extent: window.surface_current_extent(fullscreen_mode, present_mode),
            image_usage: vko::ImageUsage::COLOR_ATTACHMENT | vko::ImageUsage::TRANSFER_DST,
            present_mode,
            full_screen_exclusive: fullscreen_mode,
            win32_monitor,
            scaling_behavior,
            present_gravity,
            ..vko::SwapchainCreateInfo::default()
        };

        let swapchain_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_swapchain(render_flt_id, window.surface(), swapchain_ci.clone())
            .map_err(VulkanoError::CreateSwapchain)?;

        let viewport = vko::Viewport {
            offset: [0.0, 0.0],
            extent: [
                swapchain_ci.image_extent[0] as f32,
                swapchain_ci.image_extent[1] as f32,
            ],
            depth_range: 0.0..=1.0,
        };

        let image_format = if surface_format.components()[0] > 8 {
            vec![
                vko::Format::R16G16B16A16_UINT,
                vko::Format::R16G16B16A16_UNORM,
                vko::Format::R8G8B8A8_UINT,
                vko::Format::R8G8B8A8_UNORM,
                vko::Format::B8G8R8A8_UINT,
                vko::Format::B8G8R8A8_UNORM,
                vko::Format::A8B8G8R8_UINT_PACK32,
                vko::Format::A8B8G8R8_UNORM_PACK32,
                vko::Format::R8G8B8A8_SRGB,
                vko::Format::B8G8R8A8_SRGB,
                vko::Format::A8B8G8R8_SRGB_PACK32,
            ]
        } else {
            vec![
                vko::Format::R8G8B8A8_UINT,
                vko::Format::R8G8B8A8_UNORM,
                vko::Format::B8G8R8A8_UINT,
                vko::Format::B8G8R8A8_UNORM,
                vko::Format::A8B8G8R8_UINT_PACK32,
                vko::Format::A8B8G8R8_UNORM_PACK32,
                vko::Format::R8G8B8A8_SRGB,
                vko::Format::B8G8R8A8_SRGB,
                vko::Format::A8B8G8R8_SRGB_PACK32,
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
                vko::FormatFeatures::TRANSFER_DST
                    | vko::FormatFeatures::TRANSFER_SRC
                    | vko::FormatFeatures::SAMPLED_IMAGE
                    | vko::FormatFeatures::SAMPLED_IMAGE_FILTER_LINEAR,
            )
        })
        .ok_or(ContextCreateError::NoSuitableImageFormat)?;

        let sampler = vko::Sampler::new(
            window.basalt_ref().device(),
            vko::SamplerCreateInfo {
                address_mode: [vko::SamplerAddressMode::ClampToBorder; 3],
                unnormalized_coordinates: true,
                ..Default::default()
            },
        )
        .map_err(VulkanoError::CreateSampler)?;

        let desc_alloc = Arc::new(vko::StandardDescriptorSetAllocator::new(
            window.basalt_ref().device(),
            Default::default(),
        ));

        let default_image_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_image(
                vko::ImageCreateInfo {
                    format: image_format,
                    extent: [1; 3],
                    usage: vko::ImageUsage::SAMPLED | vko::ImageUsage::TRANSFER_DST,
                    ..Default::default()
                },
                vko::AllocationCreateInfo {
                    memory_type_filter: vko::MemoryTypeFilter {
                        preferred_flags: vko::MemoryPropertyFlags::DEVICE_LOCAL,
                        not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED,
                        ..vko::MemoryTypeFilter::empty()
                    },
                    allocate_preference: vko::MemoryAllocatePreference::AlwaysAllocate,
                    ..Default::default()
                },
            )
            .map_err(VulkanoError::CreateImage)?;

        unsafe {
            vko::execute(
                window.basalt_ref().graphics_queue_ref(),
                window.basalt_ref().device_resources_ref(),
                render_flt_id,
                |cmd, _| {
                    cmd.clear_color_image(&vko::ClearColorImageInfo {
                        image: default_image_id,
                        clear_value: clear_color_value_for_format(image_format),
                        ..Default::default()
                    })?;

                    Ok(())
                },
                [],
                [],
                [(
                    default_image_id,
                    vko::AccessTypes::CLEAR_TRANSFER_WRITE,
                    vko::ImageLayoutType::Optimal,
                )],
            )
        }
        .map_err(VulkanoError::ExecuteTaskGraph)?;

        let default_image_view = vko::ImageView::new_default(
            window
                .basalt_ref()
                .device_resources_ref()
                .image(default_image_id)
                .unwrap()
                .image()
                .clone(),
        )
        .map_err(VulkanoError::CreateImageView)?;

        let msaa = window.renderer_msaa();

        Ok(Self {
            window,
            image_format,
            render_flt_id,
            resource_sharing,
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
            draw_count: None,
            update_token: None,
            desc_set: None,
            user_renderer: None,
        })
    }

    /// Obtain a copy of `Arc<Window>` of this context.
    pub fn window(&self) -> Arc<Window> {
        self.window.clone()
    }

    /// Obtain a reference of `Arc<Window>` of this context.
    pub fn window_ref(&self) -> &Arc<Window> {
        &self.window
    }

    /// Obtain a reference of user renderer provided at creation of the `Renderer`.
    pub fn user_renderer_ref<T>(&self) -> Option<&T>
    where
        T: Any,
    {
        self.user_renderer
            .as_ref()
            .and_then(|boxxed| (boxxed.as_ref() as &dyn Any).downcast_ref())
    }

    /// Obtain a mutable reference of user renderer provided at creation of the `Renderer`.
    pub fn user_renderer_mut<T>(&mut self) -> Option<&mut T>
    where
        T: Any,
    {
        self.user_renderer
            .as_mut()
            .and_then(|boxxed| (boxxed.as_mut() as &mut dyn Any).downcast_mut())
    }

    pub(crate) fn is_user_renderer(&self) -> bool {
        matches!(self.specific, Specific::User(..))
    }

    pub(in crate::render) fn with_interface_only(&mut self) {
        self.user_renderer = None;
        self.specific
            .remove_images(self.window.basalt_ref().device_resources_ref());

        self.specific = Specific::ItfOnly(ItfOnly {
            color_ms_id: None,
            task_graph: None,
            virtual_ids: None,
        });
    }

    pub(in crate::render) fn with_user_renderer<R: UserRenderer>(&mut self, mut user_renderer: R) {
        user_renderer.initialize(self.render_flt_id);
        self.user_renderer = Some(Box::new(user_renderer));
        self.specific
            .remove_images(self.window.basalt_ref().device_resources_ref());

        self.specific = Specific::User(User {
            itf_color_id: None,
            itf_color_ms_id: None,
            user_color_id: None,
            final_desc_layout: None,
            task_graph: None,
            virtual_ids: None,
        });
    }

    pub(in crate::render) fn image_format(&self) -> vko::Format {
        self.image_format
    }

    pub(in crate::render) fn check_extent(&mut self) {
        let current_extent = self.window.surface_current_extent(
            self.swapchain_ci.full_screen_exclusive,
            self.swapchain_ci.present_mode,
        );

        if current_extent == self.swapchain_ci.image_extent {
            return;
        }

        self.swapchain_rc = true;
    }

    pub(in crate::render) fn set_msaa(&mut self, msaa: MSAA) {
        if msaa == self.msaa {
            return;
        }

        self.msaa = msaa;

        match &mut self.specific {
            Specific::None => (),
            Specific::ItfOnly(specific) => {
                specific.task_graph = None;
            },
            Specific::User(specific) => {
                specific.task_graph = None;
            },
        }
    }

    pub(in crate::render) fn set_vsync(&mut self, vsync: VSync) {
        let present_mode =
            find_present_mode(&self.window, self.swapchain_ci.full_screen_exclusive, vsync);

        if present_mode == self.swapchain_ci.present_mode {
            return;
        }

        self.swapchain_ci.min_image_count = self
            .window
            .surface_capabilities(self.swapchain_ci.full_screen_exclusive, present_mode)
            .min_image_count
            .max(2);

        // TODO: Is it possible that changing present mode also changes other supported swapchain
        //       create info fields? Such as image_extent, image_format & image_color_space.

        self.swapchain_ci.present_mode = present_mode;
        self.swapchain_rc = true;
    }

    pub(in crate::render) fn set_buffer_and_images(
        &mut self,
        buffer_id: vko::Id<vko::Buffer>,
        image_ids: Vec<vko::Id<vko::Image>>,
        draw_count: u32,
        token: Arc<(Mutex<Option<u64>>, Condvar)>,
    ) -> Result<(), ContextError> {
        if image_ids.len() as u32 > self.image_capacity {
            while self.image_capacity < image_ids.len() as u32 {
                self.image_capacity *= 2;
            }

            match &mut self.specific {
                Specific::None => (),
                Specific::ItfOnly(specific) => {
                    specific.task_graph = None;
                },
                Specific::User(specific) => {
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
                vko::DescriptorSetLayout::new(
                    self.window.basalt_ref().device(),
                    shaders::pipeline_descriptor_set_layout_create_info(self.image_capacity)
                        .set_layouts[0]
                        .clone(),
                )
                .map_err(VulkanoError::CreateDescSetLayout)?,
            );
        }

        let num_default_images = self.image_capacity as usize - image_ids.len();
        let mut image_views = Vec::with_capacity(image_ids.len());

        for image_id in image_ids.iter() {
            image_views.push(
                vko::ImageView::new_default(
                    self.window
                        .basalt_ref()
                        .device_resources_ref()
                        .image(*image_id)
                        .unwrap()
                        .image()
                        .clone(),
                )
                .map_err(VulkanoError::CreateImageView)?,
            );
        }

        self.desc_set = Some(
            vko::DescriptorSet::new_variable(
                self.desc_alloc.clone(),
                self.desc_layout.clone().unwrap(),
                self.image_capacity,
                [
                    vko::WriteDescriptorSet::sampler(0, self.sampler.clone()),
                    vko::WriteDescriptorSet::image_view_array(
                        1,
                        0,
                        image_views.into_iter().chain(
                            (0..num_default_images).map(|_| self.default_image_view.clone()),
                        ),
                    ),
                ],
                [],
            )
            .map_err(VulkanoError::CreateDescSet)?,
        );

        self.buffer_id = Some(buffer_id);
        self.image_ids = image_ids;
        self.draw_count = Some(draw_count);
        self.update_token = Some(token);
        Ok(())
    }

    fn update(&mut self) -> Result<(), ContextError> {
        let mut attachments_rc = false;

        if self.swapchain_rc {
            self.swapchain_ci.image_extent = self.window.surface_current_extent(
                self.swapchain_ci.full_screen_exclusive,
                self.swapchain_ci.present_mode,
            );

            self.viewport.extent = [
                self.swapchain_ci.image_extent[0] as f32,
                self.swapchain_ci.image_extent[1] as f32,
            ];

            self.swapchain_id = self
                .window
                .basalt_ref()
                .device_resources_ref()
                .recreate_swapchain(self.swapchain_id, |_| self.swapchain_ci.clone())
                .map_err(VulkanoError::CreateSwapchain)?;

            self.swapchain_rc = false;
            attachments_rc = true;
        }

        match &mut self.specific {
            Specific::None => (),
            Specific::ItfOnly(specific) => {
                if attachments_rc || (self.msaa.is_enabled() && specific.color_ms_id.is_none()) {
                    specific.remove_images(self.window.basalt_ref().device_resources_ref());

                    if self.msaa.is_enabled() {
                        let color_ms_id = self
                            .window
                            .basalt_ref()
                            .device_resources()
                            .create_image(
                                vko::ImageCreateInfo {
                                    image_type: vko::ImageType::Dim2d,
                                    format: self.swapchain_ci.image_format,
                                    extent: [
                                        self.swapchain_ci.image_extent[0],
                                        self.swapchain_ci.image_extent[1],
                                        1,
                                    ],
                                    usage: vko::ImageUsage::COLOR_ATTACHMENT
                                        | vko::ImageUsage::TRANSIENT_ATTACHMENT,
                                    samples: self.msaa.sample_count(),
                                    ..vko::ImageCreateInfo::default()
                                },
                                vko::AllocationCreateInfo {
                                    memory_type_filter: vko::MemoryTypeFilter {
                                        preferred_flags: vko::MemoryPropertyFlags::DEVICE_LOCAL,
                                        not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED,
                                        ..vko::MemoryTypeFilter::empty()
                                    },
                                    allocate_preference:
                                        vko::MemoryAllocatePreference::AlwaysAllocate,
                                    ..vko::AllocationCreateInfo::default()
                                },
                            )
                            .map_err(VulkanoError::CreateImage)?;

                        specific.color_ms_id = Some(color_ms_id);
                    }
                }

                if specific.task_graph.is_none() {
                    let mut task_graph =
                        vko::TaskGraph::new(self.window.basalt_ref().device_resources_ref(), 1, 3);

                    let vid_swapchain = task_graph.add_swapchain(&self.swapchain_ci);
                    let vid_framebuffer = task_graph.add_framebuffer();

                    let vid_buffer = task_graph.add_buffer(&vko::BufferCreateInfo {
                        usage: vko::BufferUsage::TRANSFER_SRC
                            | vko::BufferUsage::TRANSFER_DST
                            | vko::BufferUsage::VERTEX_BUFFER,
                        sharing: self.resource_sharing.clone(),
                        ..Default::default()
                    });

                    let vid_color_ms = (self.msaa != MSAA::X1).then(|| {
                        task_graph.add_image(&vko::ImageCreateInfo {
                            image_type: vko::ImageType::Dim2d,
                            format: self.swapchain_ci.image_format,
                            usage: vko::ImageUsage::COLOR_ATTACHMENT
                                | vko::ImageUsage::TRANSIENT_ATTACHMENT,
                            samples: match self.msaa {
                                MSAA::X1 => unreachable!(),
                                MSAA::X2 => vko::SampleCount::Sample2,
                                MSAA::X4 => vko::SampleCount::Sample4,
                                MSAA::X8 => vko::SampleCount::Sample8,
                            },
                            ..Default::default()
                        })
                    });

                    let mut node = task_graph.create_task_node(
                        format!("Render[{:?}]", self.window.id()),
                        vko::QueueFamilyType::Graphics,
                        ItfTask::default(),
                    );

                    node.framebuffer(vid_framebuffer)
                        .buffer_access(vid_buffer, vko::AccessTypes::VERTEX_ATTRIBUTE_READ);

                    if self.msaa.is_disabled() {
                        node.color_attachment(
                            vid_swapchain.current_image_id(),
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                            &vko::AttachmentInfo {
                                index: 0,
                                clear: true,
                                ..Default::default()
                            },
                        );
                    } else {
                        node.color_attachment(
                            vid_color_ms.unwrap(),
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                            &vko::AttachmentInfo {
                                index: 0,
                                clear: true,
                                ..Default::default()
                            },
                        );

                        node.color_attachment(
                            vid_swapchain.current_image_id(),
                            vko::AccessTypes::RESOLVE_TRANSFER_WRITE,
                            vko::ImageLayoutType::Optimal,
                            &vko::AttachmentInfo {
                                index: 1,
                                clear: true,
                                ..Default::default()
                            },
                        );
                    }

                    let itf_node_id = node.build();

                    let mut task_graph = unsafe {
                        task_graph.compile(&vko::CompileInfo {
                            queues: &[self.window.basalt_ref().graphics_queue_ref()],
                            present_queue: Some(self.window.basalt_ref().graphics_queue_ref()),
                            flight_id: self.render_flt_id,
                            ..Default::default()
                        })
                    }
                    .map_err(VulkanoError::from)?;

                    let itf_node = task_graph.task_node_mut(itf_node_id).unwrap();
                    let itf_subpass = itf_node.subpass().unwrap().clone();

                    let itf_pipeline = create_itf_pipeline(
                        self.window.basalt_ref().device(),
                        self.image_capacity,
                        self.msaa,
                        itf_subpass,
                    )?;

                    let task = itf_node.task_mut().downcast_mut::<ItfTask>().unwrap();
                    let clear_value = clear_value_for_format(self.swapchain_ci.image_format);

                    if self.msaa.is_disabled() {
                        task.clear = Some((ClearTarget::Swapchain(vid_swapchain), clear_value));
                    } else {
                        task.clear = Some((ClearTarget::Image(vid_color_ms.unwrap()), clear_value));
                    }

                    task.pipeline = Some(itf_pipeline);
                    specific.task_graph = Some(task_graph);

                    specific.virtual_ids = Some(ItfOnlyVids {
                        swapchain: vid_swapchain,
                        buffer: vid_buffer,
                        color_ms: vid_color_ms,
                    });
                }
            },
            Specific::User(specific) => {
                let user_renderer = self.user_renderer.as_mut().unwrap();
                let mut set_final_desc_set = false;

                if attachments_rc
                    || specific.itf_color_id.is_none()
                    || (self.msaa.is_enabled() && specific.itf_color_ms_id.is_none())
                    || specific.user_color_id.is_none()
                {
                    specific.remove_images(self.window.basalt_ref().device_resources_ref());

                    let user_color_id = self
                        .window
                        .basalt_ref()
                        .device_resources_ref()
                        .create_image(
                            vko::ImageCreateInfo {
                                image_type: vko::ImageType::Dim2d,
                                format: self.swapchain_ci.image_format,
                                extent: [
                                    self.swapchain_ci.image_extent[0],
                                    self.swapchain_ci.image_extent[1],
                                    1,
                                ],
                                usage: vko::ImageUsage::COLOR_ATTACHMENT
                                    | vko::ImageUsage::INPUT_ATTACHMENT
                                    | vko::ImageUsage::TRANSFER_DST,
                                ..Default::default()
                            },
                            vko::AllocationCreateInfo {
                                memory_type_filter: vko::MemoryTypeFilter {
                                    preferred_flags: vko::MemoryPropertyFlags::DEVICE_LOCAL,
                                    not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED,
                                    ..vko::MemoryTypeFilter::empty()
                                },
                                allocate_preference: vko::MemoryAllocatePreference::AlwaysAllocate,
                                ..Default::default()
                            },
                        )
                        .map_err(VulkanoError::CreateImage)?;

                    specific.user_color_id = Some(user_color_id);

                    let itf_color_id = self
                        .window
                        .basalt_ref()
                        .device_resources_ref()
                        .create_image(
                            vko::ImageCreateInfo {
                                image_type: vko::ImageType::Dim2d,
                                format: self.swapchain_ci.image_format,
                                extent: [
                                    self.swapchain_ci.image_extent[0],
                                    self.swapchain_ci.image_extent[1],
                                    1,
                                ],
                                usage: vko::ImageUsage::COLOR_ATTACHMENT
                                    | vko::ImageUsage::INPUT_ATTACHMENT
                                    | vko::ImageUsage::TRANSFER_DST,
                                ..Default::default()
                            },
                            vko::AllocationCreateInfo {
                                memory_type_filter: vko::MemoryTypeFilter {
                                    preferred_flags: vko::MemoryPropertyFlags::DEVICE_LOCAL,
                                    not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED,
                                    ..vko::MemoryTypeFilter::empty()
                                },
                                allocate_preference: vko::MemoryAllocatePreference::AlwaysAllocate,
                                ..Default::default()
                            },
                        )
                        .map_err(VulkanoError::CreateImage)?;

                    specific.itf_color_id = Some(itf_color_id);

                    if self.msaa.is_enabled() {
                        let itf_color_ms_id = self
                            .window
                            .basalt_ref()
                            .device_resources()
                            .create_image(
                                vko::ImageCreateInfo {
                                    image_type: vko::ImageType::Dim2d,
                                    format: self.swapchain_ci.image_format,
                                    extent: [
                                        self.swapchain_ci.image_extent[0],
                                        self.swapchain_ci.image_extent[1],
                                        1,
                                    ],
                                    usage: vko::ImageUsage::COLOR_ATTACHMENT
                                        | vko::ImageUsage::TRANSIENT_ATTACHMENT,
                                    samples: self.msaa.sample_count(),
                                    ..vko::ImageCreateInfo::default()
                                },
                                vko::AllocationCreateInfo {
                                    memory_type_filter: vko::MemoryTypeFilter {
                                        preferred_flags: vko::MemoryPropertyFlags::DEVICE_LOCAL,
                                        not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED,
                                        ..vko::MemoryTypeFilter::empty()
                                    },
                                    allocate_preference:
                                        vko::MemoryAllocatePreference::AlwaysAllocate,
                                    ..vko::AllocationCreateInfo::default()
                                },
                            )
                            .map_err(VulkanoError::CreateImage)?;

                        specific.itf_color_ms_id = Some(itf_color_ms_id);
                    }

                    set_final_desc_set = true;
                    user_renderer.target_changed(user_color_id);
                }

                if specific.task_graph.is_none() {
                    let user_task_graph_info = user_renderer.task_graph_info();

                    let mut task_graph = vko::TaskGraph::new(
                        self.window.basalt_ref().device_resources_ref(),
                        2 + user_task_graph_info.max_nodes,
                        5 + user_task_graph_info.max_resources,
                    );

                    let vid_swapchain = task_graph.add_swapchain(&self.swapchain_ci);
                    let vid_framebuffer = task_graph.add_framebuffer();

                    let vid_buffer = task_graph.add_buffer(&vko::BufferCreateInfo {
                        usage: vko::BufferUsage::TRANSFER_SRC
                            | vko::BufferUsage::TRANSFER_DST
                            | vko::BufferUsage::VERTEX_BUFFER,
                        sharing: self.resource_sharing.clone(),
                        ..Default::default()
                    });

                    let vid_itf_color = task_graph.add_image(&vko::ImageCreateInfo {
                        image_type: vko::ImageType::Dim2d,
                        format: self.swapchain_ci.image_format,
                        extent: [
                            self.swapchain_ci.image_extent[0],
                            self.swapchain_ci.image_extent[1],
                            1,
                        ],
                        usage: vko::ImageUsage::COLOR_ATTACHMENT
                            | vko::ImageUsage::INPUT_ATTACHMENT
                            | vko::ImageUsage::TRANSFER_DST,
                        ..Default::default()
                    });

                    let vid_itf_color_ms = (self.msaa != MSAA::X1).then(|| {
                        task_graph.add_image(&vko::ImageCreateInfo {
                            image_type: vko::ImageType::Dim2d,
                            format: self.swapchain_ci.image_format,
                            usage: vko::ImageUsage::COLOR_ATTACHMENT
                                | vko::ImageUsage::TRANSIENT_ATTACHMENT,
                            samples: match self.msaa {
                                MSAA::X1 => unreachable!(),
                                MSAA::X2 => vko::SampleCount::Sample2,
                                MSAA::X4 => vko::SampleCount::Sample4,
                                MSAA::X8 => vko::SampleCount::Sample8,
                            },
                            ..Default::default()
                        })
                    });

                    let vid_user_color = task_graph.add_image(&vko::ImageCreateInfo {
                        image_type: vko::ImageType::Dim2d,
                        format: self.swapchain_ci.image_format,
                        extent: [
                            self.swapchain_ci.image_extent[0],
                            self.swapchain_ci.image_extent[1],
                            1,
                        ],
                        usage: vko::ImageUsage::COLOR_ATTACHMENT
                            | vko::ImageUsage::INPUT_ATTACHMENT
                            | vko::ImageUsage::TRANSFER_DST,
                        ..Default::default()
                    });

                    let user_node_id =
                        user_renderer.task_graph_build(&mut task_graph, vid_user_color);

                    let mut itf_node = task_graph.create_task_node(
                        format!("Render-Itf[{:?}]", self.window.id()),
                        vko::QueueFamilyType::Graphics,
                        ItfTask::default(),
                    );

                    itf_node
                        .framebuffer(vid_framebuffer)
                        .buffer_access(vid_buffer, vko::AccessTypes::VERTEX_ATTRIBUTE_READ);

                    if self.msaa.is_disabled() {
                        itf_node.color_attachment(
                            vid_itf_color,
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                            &vko::AttachmentInfo {
                                index: 0,
                                clear: true,
                                ..Default::default()
                            },
                        );
                    } else {
                        itf_node
                            .color_attachment(
                                vid_itf_color,
                                vko::AccessTypes::RESOLVE_TRANSFER_WRITE,
                                vko::ImageLayoutType::Optimal,
                                &vko::AttachmentInfo {
                                    index: 3,
                                    ..Default::default()
                                },
                            )
                            .color_attachment(
                                vid_itf_color_ms.unwrap(),
                                vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                                vko::ImageLayoutType::Optimal,
                                &vko::AttachmentInfo {
                                    index: 0,
                                    clear: true,
                                    ..Default::default()
                                },
                            );
                    }

                    let itf_node_id = itf_node.build();

                    let mut final_node = task_graph.create_task_node(
                        format!("Render-Final[{:?}]", self.window.id()),
                        vko::QueueFamilyType::Graphics,
                        FinalTask::default(),
                    );

                    final_node
                        .framebuffer(vid_framebuffer)
                        .color_attachment(
                            vid_swapchain.current_image_id(),
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                            &vko::AttachmentInfo {
                                index: 2,
                                ..Default::default()
                            },
                        )
                        .input_attachment(
                            vid_user_color,
                            vko::AccessTypes::FRAGMENT_SHADER_COLOR_INPUT_ATTACHMENT_READ,
                            vko::ImageLayoutType::Optimal,
                            &vko::AttachmentInfo {
                                index: 1,
                                ..Default::default()
                            },
                        )
                        .input_attachment(
                            vid_itf_color,
                            vko::AccessTypes::FRAGMENT_SHADER_COLOR_INPUT_ATTACHMENT_READ,
                            vko::ImageLayoutType::Optimal,
                            &vko::AttachmentInfo {
                                index: if self.msaa.is_disabled() { 0 } else { 3 },
                                ..Default::default()
                            },
                        );

                    let final_node_id = final_node.build();
                    task_graph.add_edge(itf_node_id, final_node_id).unwrap();
                    task_graph.add_edge(user_node_id, final_node_id).unwrap();

                    let mut task_graph = unsafe {
                        task_graph.compile(&vko::CompileInfo {
                            queues: &[self.window.basalt_ref().graphics_queue_ref()],
                            present_queue: Some(self.window.basalt_ref().graphics_queue_ref()),
                            flight_id: self.render_flt_id,
                            ..Default::default()
                        })
                    }
                    .map_err(VulkanoError::from)?;

                    user_renderer.task_graph_modify(&mut task_graph);

                    {
                        let itf_node = task_graph.task_node_mut(itf_node_id).unwrap();
                        let itf_subpass = itf_node.subpass().unwrap().clone();

                        let itf_pipeline = create_itf_pipeline(
                            self.window.basalt_ref().device(),
                            self.image_capacity,
                            self.msaa,
                            itf_subpass,
                        )?;

                        let task = itf_node.task_mut().downcast_mut::<ItfTask>().unwrap();
                        let clear_value = clear_value_for_format(self.swapchain_ci.image_format);

                        if self.msaa.is_disabled() {
                            task.clear = Some((ClearTarget::Image(vid_itf_color), clear_value));
                        } else {
                            task.clear =
                                Some((ClearTarget::Image(vid_itf_color_ms.unwrap()), clear_value));
                        }

                        task.pipeline = Some(itf_pipeline);
                    }

                    {
                        let final_node = task_graph.task_node_mut(final_node_id).unwrap();
                        let final_subpass = final_node.subpass().unwrap().clone();

                        let final_vs = shaders::final_vs_sm(self.window.basalt_ref().device())
                            .entry_point("main")
                            .unwrap();

                        let final_fs = shaders::final_fs_sm(self.window.basalt_ref().device())
                            .entry_point("main")
                            .unwrap();

                        let stages = [
                            vko::PipelineShaderStageCreateInfo::new(final_vs),
                            vko::PipelineShaderStageCreateInfo::new(final_fs),
                        ];

                        let layout = vko::PipelineLayout::new(
                            self.window.basalt_ref().device(),
                            vko::PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
                                .into_pipeline_layout_create_info(self.window.basalt_ref().device())
                                .unwrap(),
                        )
                        .map_err(VulkanoError::CreatePipelineLayout)?;

                        let final_pipeline = vko::GraphicsPipeline::new(
                            self.window.basalt_ref().device(),
                            None,
                            vko::GraphicsPipelineCreateInfo {
                                stages: stages.into_iter().collect(),
                                vertex_input_state: Some(vko::VertexInputState::new()),
                                input_assembly_state: Some(vko::InputAssemblyState::default()),
                                viewport_state: Some(vko::ViewportState::default()),
                                rasterization_state: Some(vko::RasterizationState::default()),
                                multisample_state: Some(vko::MultisampleState::default()),
                                color_blend_state: Some(
                                    vko::ColorBlendState::with_attachment_states(
                                        final_subpass.num_color_attachments(),
                                        Default::default(),
                                    ),
                                ),
                                dynamic_state: [vko::DynamicState::Viewport].into_iter().collect(),
                                subpass: Some(final_subpass.into()),
                                ..vko::GraphicsPipelineCreateInfo::layout(layout)
                            },
                        )
                        .map_err(VulkanoError::CreateGraphicsPipeline)?;

                        if specific.final_desc_layout.is_none() {
                            specific.final_desc_layout = Some(
                                final_pipeline
                                    .layout()
                                    .set_layouts()
                                    .first()
                                    .unwrap()
                                    .clone(),
                            );
                        }

                        let task = final_node.task_mut().downcast_mut::<FinalTask>().unwrap();
                        task.pipeline = Some(final_pipeline);
                        set_final_desc_set = true;
                    }

                    specific.task_graph = Some(task_graph);

                    specific.virtual_ids = Some(UserVids {
                        final_node: final_node_id,
                        swapchain: vid_swapchain,
                        buffer: vid_buffer,
                        itf_color: vid_itf_color,
                        itf_color_ms: vid_itf_color_ms,
                        user_color: vid_user_color,
                    });
                }

                if set_final_desc_set {
                    let user_color_view = vko::ImageView::new_default(
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .image(specific.user_color_id.unwrap())
                            .unwrap()
                            .image()
                            .clone(),
                    )
                    .map_err(VulkanoError::CreateImageView)?;

                    let itf_color_view = vko::ImageView::new_default(
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .image(specific.itf_color_id.unwrap())
                            .unwrap()
                            .image()
                            .clone(),
                    )
                    .map_err(VulkanoError::CreateImageView)?;

                    let desc_set = vko::DescriptorSet::new(
                        self.desc_alloc.clone(),
                        specific.final_desc_layout.clone().unwrap(),
                        [
                            vko::WriteDescriptorSet::image_view(0, user_color_view),
                            vko::WriteDescriptorSet::image_view(1, itf_color_view),
                        ],
                        [],
                    )
                    .map_err(VulkanoError::CreateDescSet)?;

                    specific
                        .task_graph
                        .as_mut()
                        .unwrap()
                        .task_node_mut(specific.virtual_ids.as_ref().unwrap().final_node)
                        .unwrap()
                        .task_mut()
                        .downcast_mut::<FinalTask>()
                        .unwrap()
                        .desc_set = Some(desc_set);
                }
            },
        }

        Ok(())
    }

    pub(in crate::render) fn execute(
        &mut self,
        metrics_state_op: &mut Option<MetricsState>,
    ) -> Result<(), ContextError> {
        self.update()?;

        let flight = self
            .window
            .basalt_ref()
            .device_resources_ref()
            .flight(self.render_flt_id)
            .unwrap();

        let buffer_id = match self.buffer_id {
            Some(some) => some,
            None => return Ok(()),
        };

        let exec_result = match &self.specific {
            Specific::ItfOnly(specific) => {
                let mut resource_map =
                    vko::ResourceMap::new(specific.task_graph.as_ref().unwrap()).unwrap();
                let vids = specific.virtual_ids.as_ref().unwrap();

                resource_map
                    .insert_swapchain(vids.swapchain, self.swapchain_id)
                    .unwrap();
                resource_map.insert_buffer(vids.buffer, buffer_id).unwrap();

                if let Some(vid_color_ms) = vids.color_ms {
                    resource_map
                        .insert_image(vid_color_ms, specific.color_ms_id.unwrap())
                        .unwrap();
                }

                flight.wait(None).map_err(VulkanoError::FlightWait)?;
                let _draw_guard = self.window.window_manager_ref().request_draw();

                if let Some(metrics_state) = metrics_state_op.as_mut() {
                    metrics_state.track_acquire();
                }

                if let Some(update_token) = self.update_token.take() {
                    *update_token.0.lock() = Some(flight.current_frame());
                    update_token.1.notify_one();
                }

                unsafe {
                    specific
                        .task_graph
                        .as_ref()
                        .unwrap()
                        .execute(resource_map, self, || {
                            if let Some(metrics_state) = metrics_state_op.as_mut() {
                                metrics_state.track_present();
                            }
                        })
                }
                .map_err(VulkanoError::ExecuteTaskGraph)?;

                Ok(())
            },
            Specific::User(specific) => {
                let user_renderer = self.user_renderer.as_mut().unwrap();
                let mut resource_map =
                    vko::ResourceMap::new(specific.task_graph.as_ref().unwrap()).unwrap();
                user_renderer.task_graph_resources(&mut resource_map);
                let vids = specific.virtual_ids.as_ref().unwrap();

                resource_map
                    .insert_swapchain(vids.swapchain, self.swapchain_id)
                    .unwrap();
                resource_map.insert_buffer(vids.buffer, buffer_id).unwrap();
                resource_map
                    .insert_image(vids.itf_color, specific.itf_color_id.unwrap())
                    .unwrap();
                resource_map
                    .insert_image(vids.user_color, specific.user_color_id.unwrap())
                    .unwrap();

                if let Some(vid_itf_color_ms) = vids.itf_color_ms {
                    resource_map
                        .insert_image(vid_itf_color_ms, specific.itf_color_ms_id.unwrap())
                        .unwrap();
                }

                flight.wait(None).map_err(VulkanoError::FlightWait)?;
                let _draw_guard = self.window.window_manager_ref().request_draw();

                if let Some(metrics_state) = metrics_state_op.as_mut() {
                    metrics_state.track_acquire();
                }

                if let Some(update_token) = self.update_token.take() {
                    *update_token.0.lock() = Some(flight.current_frame());
                    update_token.1.notify_one();
                }

                unsafe {
                    specific
                        .task_graph
                        .as_ref()
                        .unwrap()
                        .execute(resource_map, self, || {
                            if let Some(metrics_state) = metrics_state_op.as_mut() {
                                metrics_state.track_present();
                            }
                        })
                }
                .map_err(VulkanoError::ExecuteTaskGraph)?;

                Ok(())
            },
            Specific::None => return Err(ContextError::NoModeSet),
        };

        match exec_result {
            Ok(()) => (),
            Err(vko::ExecuteError::Swapchain {
                error: vko::Validated::Error(vko::VulkanError::OutOfDate),
                ..
            }) => {
                self.swapchain_rc = true;
            },
            Err(e) => return Err(VulkanoError::ExecuteTaskGraph(e).into()),
        }

        if let Some(metrics_state) = metrics_state_op.as_mut() {
            if metrics_state.tracked_time() >= Duration::from_secs(1) {
                self.window.set_renderer_metrics(metrics_state.complete());
            }
        }

        Ok(())
    }
}

impl Drop for RendererContext {
    fn drop(&mut self) {
        let resources = self.window.basalt_ref().device_resources_ref();
        let render_flt = resources.flight(self.render_flt_id).unwrap();
        render_flt
            .wait_for_frame(render_flt.current_frame(), None)
            .unwrap();

        unsafe {
            let _ = resources.remove_image(self.default_image_id);
            let _ = resources.remove_swapchain(self.swapchain_id);
        }

        self.specific.remove_images(resources);
        // TODO: remove render_flt_id
    }
}

fn create_itf_pipeline(
    device: Arc<vko::Device>,
    image_capacity: u32,
    msaa: MSAA,
    subpass: vko::Subpass,
) -> Result<Arc<vko::GraphicsPipeline>, VulkanoError> {
    let ui_vs = shaders::ui_vs_sm(device.clone())
        .entry_point("main")
        .unwrap();

    let ui_fs = shaders::ui_fs_sm(device.clone())
        .entry_point("main")
        .unwrap();

    let vertex_input_state = ItfVertInfo::per_vertex().definition(&ui_vs).unwrap();

    let stages = [
        vko::PipelineShaderStageCreateInfo::new(ui_vs),
        vko::PipelineShaderStageCreateInfo::new(ui_fs),
    ];

    let layout = vko::PipelineLayout::new(
        device.clone(),
        shaders::pipeline_descriptor_set_layout_create_info(image_capacity)
            .into_pipeline_layout_create_info(device.clone())
            .unwrap(),
    )
    .map_err(VulkanoError::CreatePipelineLayout)?;

    let sample_count = match msaa {
        MSAA::X1 => vko::SampleCount::Sample1,
        MSAA::X2 => vko::SampleCount::Sample2,
        MSAA::X4 => vko::SampleCount::Sample4,
        MSAA::X8 => vko::SampleCount::Sample8,
    };

    vko::GraphicsPipeline::new(
        device,
        None,
        vko::GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: Some(vko::InputAssemblyState::default()),
            viewport_state: Some(vko::ViewportState::default()),
            rasterization_state: Some(vko::RasterizationState::default()),
            multisample_state: Some(vko::MultisampleState {
                rasterization_samples: sample_count,
                ..vko::MultisampleState::default()
            }),
            color_blend_state: Some(vko::ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                vko::ColorBlendAttachmentState {
                    blend: Some(vko::AttachmentBlend::alpha()),
                    ..vko::ColorBlendAttachmentState::default()
                },
            )),
            dynamic_state: [vko::DynamicState::Viewport].into_iter().collect(),
            subpass: Some(subpass.into()),
            ..vko::GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .map_err(VulkanoError::CreateGraphicsPipeline)
}

enum ClearTarget {
    Swapchain(vko::Id<vko::Swapchain>),
    Image(vko::Id<vko::Image>),
}

#[derive(Default)]
struct ItfTask {
    clear: Option<(ClearTarget, vko::ClearValue)>,
    pipeline: Option<Arc<vko::GraphicsPipeline>>,
}

impl vko::Task for ItfTask {
    type World = RendererContext;

    fn clear_values(&self, clear_values: &mut vko::ClearValues<'_>) {
        if let Some((target, value)) = self.clear.as_ref() {
            let id = match target {
                ClearTarget::Swapchain(id) => id.current_image_id(),
                ClearTarget::Image(id) => *id,
            };

            clear_values.set(id, *value);
        } else {
            unreachable!()
        }
    }

    unsafe fn execute(
        &self,
        cmd: &mut vko::RecordingCommandBuffer<'_>,
        _task: &mut vko::TaskContext<'_>,
        context: &Self::World,
    ) -> vko::TaskResult {
        let pipeline = self.pipeline.as_ref().unwrap();
        unsafe { cmd.set_viewport(0, std::slice::from_ref(&context.viewport)) }?;
        unsafe { cmd.bind_pipeline_graphics(pipeline) }?;

        if let (Some(desc_set), Some(buffer_id), Some(draw_count)) = (
            context.desc_set.as_ref(),
            context.buffer_id.as_ref(),
            context.draw_count,
        ) {
            if draw_count > 0 {
                unsafe {
                    cmd.as_raw().bind_descriptor_sets(
                        vko::PipelineBindPoint::Graphics,
                        pipeline.layout(),
                        0,
                        &[desc_set.as_raw()],
                        &[],
                    )
                }?;

                cmd.destroy_objects(iter::once(desc_set.clone()));
                unsafe { cmd.bind_vertex_buffers(0, &[*buffer_id], &[0], &[], &[]) }?;
                unsafe { cmd.draw(draw_count, 1, 0, 0) }?;
            }
        } else {
            unreachable!()
        }

        Ok(())
    }
}

#[derive(Default)]
struct FinalTask {
    pipeline: Option<Arc<vko::GraphicsPipeline>>,
    desc_set: Option<Arc<vko::DescriptorSet>>,
}

impl vko::Task for FinalTask {
    type World = RendererContext;

    unsafe fn execute(
        &self,
        cmd: &mut vko::RecordingCommandBuffer<'_>,
        _task: &mut vko::TaskContext<'_>,
        context: &Self::World,
    ) -> vko::TaskResult {
        let pipeline = self.pipeline.as_ref().unwrap();
        let desc_set = self.desc_set.as_ref().unwrap().clone();
        unsafe { cmd.set_viewport(0, std::slice::from_ref(&context.viewport)) }?;
        unsafe { cmd.bind_pipeline_graphics(pipeline) }?;

        unsafe {
            cmd.as_raw().bind_descriptor_sets(
                vko::PipelineBindPoint::Graphics,
                pipeline.layout(),
                0,
                &[desc_set.as_raw()],
                &[],
            )
        }?;

        cmd.destroy_objects(iter::once(desc_set));
        unsafe { cmd.draw(3, 1, 0, 0) }?;
        unsafe { cmd.as_raw().end_render_pass(&Default::default()) }?;
        Ok(())
    }
}

fn find_present_mode(
    window: &Arc<Window>,
    fullscreen_mode: vko::FullScreenExclusive,
    vsync: VSync,
) -> vko::PresentMode {
    let mut present_modes = window.surface_present_modes(fullscreen_mode);

    present_modes.retain(|present_mode| {
        matches!(
            present_mode,
            vko::PresentMode::Fifo
                | vko::PresentMode::FifoRelaxed
                | vko::PresentMode::Mailbox
                | vko::PresentMode::Immediate
        )
    });

    present_modes.sort_by_key(|present_mode| {
        match vsync {
            VSync::Enable => {
                match present_mode {
                    vko::PresentMode::Fifo => 3,
                    vko::PresentMode::FifoRelaxed => 2,
                    vko::PresentMode::Mailbox => 1,
                    vko::PresentMode::Immediate => 0,
                    _ => unreachable!(),
                }
            },
            VSync::Disable => {
                match present_mode {
                    vko::PresentMode::Mailbox => 3,
                    vko::PresentMode::Immediate => 2,
                    vko::PresentMode::Fifo => 1,
                    vko::PresentMode::FifoRelaxed => 0,
                    _ => unreachable!(),
                }
            },
        }
    });

    present_modes.pop().unwrap()
}
