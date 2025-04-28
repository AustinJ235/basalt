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
    pub use vulkano::command_buffer::RenderPassBeginInfo;
    pub use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
    pub use vulkano::descriptor_set::layout::DescriptorSetLayout;
    pub use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
    pub use vulkano::device::Device;
    pub use vulkano::format::{Format, FormatFeatures, NumericFormat};
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
    pub use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
    pub use vulkano::swapchain::{
        ColorSpace, FullScreenExclusive, PresentGravity, PresentGravityFlags, PresentMode,
        PresentScaling, PresentScalingFlags, Swapchain, SwapchainCreateInfo,
    };
    pub use vulkano::sync::Sharing;
    pub use vulkano::{Validated, VulkanError};
    pub use vulkano_taskgraph::command_buffer::{ClearColorImageInfo, RecordingCommandBuffer};
    pub use vulkano_taskgraph::graph::{
        CompileInfo, ExecutableTaskGraph, ExecuteError, ResourceMap, TaskGraph,
    };
    pub use vulkano_taskgraph::resource::{AccessTypes, Flight, ImageLayoutType, Resources};
    pub use vulkano_taskgraph::{Id, QueueFamilyType, Task, TaskContext, TaskResult, execute};
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
    fn remove_images(&mut self) -> Vec<vko::Id<vko::Image>> {
        match self {
            Specific::None => Vec::new(),
            Specific::ItfOnly(specific) => specific.remove_images(),
            Specific::User(specific) => specific.remove_images(),
        }
    }
}

struct ItfOnly {
    render_pass: Option<Arc<vko::RenderPass>>,
    pipeline: Option<Arc<vko::GraphicsPipeline>>,
    color_ms_id: Option<vko::Id<vko::Image>>,
    framebuffers: Option<Vec<Arc<vko::Framebuffer>>>,
    task_graph: Option<vko::ExecutableTaskGraph<RendererContext>>,
    virtual_ids: Option<VirtualIds>,
}

impl ItfOnly {
    fn remove_images(&mut self) -> Vec<vko::Id<vko::Image>> {
        let mut image_ids = Vec::new();

        if let Some(color_ms_id) = self.color_ms_id.take() {
            image_ids.push(color_ms_id);
        }

        image_ids
    }
}

struct User {
    render_pass: Option<Arc<vko::RenderPass>>,
    pipeline_itf: Option<Arc<vko::GraphicsPipeline>>,
    pipeline_final: Option<Arc<vko::GraphicsPipeline>>,
    itf_color_id: Option<vko::Id<vko::Image>>,
    itf_color_ms_id: Option<vko::Id<vko::Image>>,
    user_color_id: Option<vko::Id<vko::Image>>,
    framebuffers: Option<Vec<Arc<vko::Framebuffer>>>,
    final_desc_layout: Option<Arc<vko::DescriptorSetLayout>>,
    final_desc_set: Option<Arc<vko::DescriptorSet>>,
    task_graph: Option<vko::ExecutableTaskGraph<RendererContext>>,
    virtual_ids: Option<VirtualIds>,
}

impl User {
    fn remove_images(&mut self) -> Vec<vko::Id<vko::Image>> {
        let mut image_ids = Vec::new();

        if let Some(itf_color_id) = self.itf_color_id.take() {
            image_ids.push(itf_color_id);
        }

        if let Some(itf_color_ms_id) = self.itf_color_ms_id.take() {
            image_ids.push(itf_color_ms_id);
        }

        if let Some(user_color_id) = self.user_color_id.take() {
            image_ids.push(user_color_id);
        }

        image_ids
    }
}

#[allow(clippy::enum_variant_names)]
enum VirtualIds {
    ItfOnlyNoMsaa(ItfOnlyNoMsaaVIds),
    ItfOnlyMsaa(ItfOnlyMsaaVIds),
    UserNoMsaa(UserNoMsaaVIds),
    UserMsaa(UserMsaaVIds),
}

struct ItfOnlyNoMsaaVIds {
    swapchain: vko::Id<vko::Swapchain>,
    buffer: vko::Id<vko::Buffer>,
}

struct ItfOnlyMsaaVIds {
    swapchain: vko::Id<vko::Swapchain>,
    color_ms: vko::Id<vko::Image>,
    buffer: vko::Id<vko::Buffer>,
}

struct UserNoMsaaVIds {
    swapchain: vko::Id<vko::Swapchain>,
    itf_color: vko::Id<vko::Image>,
    user_color: vko::Id<vko::Image>,
    buffer: vko::Id<vko::Buffer>,
}

struct UserMsaaVIds {
    swapchain: vko::Id<vko::Swapchain>,
    itf_color: vko::Id<vko::Image>,
    itf_color_ms: vko::Id<vko::Image>,
    user_color: vko::Id<vko::Image>,
    buffer: vko::Id<vko::Buffer>,
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

        enqueue_image_destruction(
            self.window.basalt_ref().device_resources_ref(),
            self.render_flt_id,
            self.specific.remove_images(),
        );

        self.specific = Specific::ItfOnly(ItfOnly {
            render_pass: None,
            pipeline: None,
            color_ms_id: None,
            framebuffers: None,
            task_graph: None,
            virtual_ids: None,
        });
    }

    pub(in crate::render) fn with_user_renderer<R: UserRenderer>(&mut self, mut user_renderer: R) {
        user_renderer.initialize(self.render_flt_id);
        self.user_renderer = Some(Box::new(user_renderer));

        enqueue_image_destruction(
            self.window.basalt_ref().device_resources_ref(),
            self.render_flt_id,
            self.specific.remove_images(),
        );

        self.specific = Specific::User(User {
            render_pass: None,
            pipeline_itf: None,
            pipeline_final: None,
            itf_color_id: None,
            itf_color_ms_id: None,
            user_color_id: None,
            framebuffers: None,
            final_desc_layout: None,
            final_desc_set: None,
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
                specific.render_pass = None;
                specific.pipeline = None;
                specific.framebuffers = None;
                specific.task_graph = None;
            },
            Specific::User(specific) => {
                specific.render_pass = None;
                specific.pipeline_itf = None;
                // TODO: Subpass::from uses render_pass but does really need to be recreated?
                specific.pipeline_final = None;
                specific.framebuffers = None;
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
                    specific.pipeline = None;
                    specific.task_graph = None;
                },
                Specific::User(specific) => {
                    specific.pipeline_itf = None;
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
        let mut framebuffers_rc = false;

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
            framebuffers_rc = true;
        }

        match &mut self.specific {
            Specific::None => (),
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
                            .map_err(VulkanoError::CreateRenderPass)?,
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
                            .map_err(VulkanoError::CreateRenderPass)?,
                        );
                    }
                }

                if specific.pipeline.is_none() {
                    specific.pipeline = Some(create_itf_pipeline(
                        self.window.basalt_ref().device(),
                        self.image_capacity,
                        self.msaa,
                        vko::Subpass::from(specific.render_pass.clone().unwrap(), 0).unwrap(),
                    )?);
                }

                if framebuffers_rc || specific.framebuffers.is_none() {
                    enqueue_image_destruction(
                        self.window.basalt_ref().device_resources_ref(),
                        self.render_flt_id,
                        specific.remove_images(),
                    );

                    let swapchain_state = self
                        .window
                        .basalt_ref()
                        .device_resources_ref()
                        .swapchain(self.swapchain_id)
                        .unwrap();

                    if self.msaa == MSAA::X1 {
                        let mut framebuffers = Vec::with_capacity(swapchain_state.images().len());

                        for swapchain_image in swapchain_state.images().iter() {
                            framebuffers.push(
                                vko::Framebuffer::new(
                                    specific.render_pass.clone().unwrap(),
                                    vko::FramebufferCreateInfo {
                                        attachments: vec![
                                            vko::ImageView::new_default(swapchain_image.clone())
                                                .map_err(VulkanoError::CreateImageView)?,
                                        ],
                                        ..Default::default()
                                    },
                                )
                                .map_err(VulkanoError::CreateFramebuffer)?,
                            );
                        }

                        specific.framebuffers = Some(framebuffers);
                    } else {
                        let sample_count = match self.msaa {
                            MSAA::X1 => unreachable!(),
                            MSAA::X2 => vko::SampleCount::Sample2,
                            MSAA::X4 => vko::SampleCount::Sample4,
                            MSAA::X8 => vko::SampleCount::Sample8,
                        };

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
                                    samples: sample_count,
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

                        let color_ms_state = self
                            .window
                            .basalt_ref()
                            .device_resources_ref()
                            .image(color_ms_id)
                            .unwrap();

                        let mut framebuffers = Vec::with_capacity(swapchain_state.images().len());

                        for swapchain_image in swapchain_state.images().iter() {
                            framebuffers.push(
                                vko::Framebuffer::new(
                                    specific.render_pass.clone().unwrap(),
                                    vko::FramebufferCreateInfo {
                                        attachments: vec![
                                            vko::ImageView::new_default(
                                                color_ms_state.image().clone(),
                                            )
                                            .map_err(VulkanoError::CreateImageView)?,
                                            vko::ImageView::new_default(swapchain_image.clone())
                                                .map_err(VulkanoError::CreateImageView)?,
                                        ],
                                        ..Default::default()
                                    },
                                )
                                .map_err(VulkanoError::CreateFramebuffer)?,
                            );
                        }

                        specific.framebuffers = Some(framebuffers);
                    }
                }

                if specific.task_graph.is_none() {
                    let mut task_graph =
                        vko::TaskGraph::new(self.window.basalt_ref().device_resources_ref());
                    let vid_swapchain = task_graph.add_swapchain(&self.swapchain_ci);

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
                        RenderTask,
                    );

                    node.buffer_access(vid_buffer, vko::AccessTypes::VERTEX_ATTRIBUTE_READ);

                    let virtual_ids = if self.msaa == MSAA::X1 {
                        node.image_access(
                            vid_swapchain.current_image_id(),
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                        );

                        VirtualIds::ItfOnlyNoMsaa(ItfOnlyNoMsaaVIds {
                            swapchain: vid_swapchain,
                            buffer: vid_buffer,
                        })
                    } else {
                        let vid_color_ms = vid_color_ms.unwrap();

                        node.image_access(
                            vid_swapchain.current_image_id(),
                            vko::AccessTypes::RESOLVE_TRANSFER_WRITE,
                            vko::ImageLayoutType::Optimal,
                        )
                        .image_access(
                            vid_color_ms,
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                        );

                        VirtualIds::ItfOnlyMsaa(ItfOnlyMsaaVIds {
                            swapchain: vid_swapchain,
                            color_ms: vid_color_ms,
                            buffer: vid_buffer,
                        })
                    };

                    specific.task_graph = Some(
                        unsafe {
                            task_graph.compile(&vko::CompileInfo {
                                queues: &[self.window.basalt_ref().graphics_queue_ref()],
                                present_queue: Some(self.window.basalt_ref().graphics_queue_ref()),
                                flight_id: self.render_flt_id,
                                ..Default::default()
                            })
                        }
                        .map_err(VulkanoError::from)?,
                    );

                    specific.virtual_ids = Some(virtual_ids);
                }
            },
            Specific::User(specific) => {
                let user_renderer = self.user_renderer.as_mut().unwrap();

                if specific.render_pass.is_none() {
                    if self.msaa == MSAA::X1 {
                        specific.render_pass = Some(
                            vulkano::ordered_passes_renderpass!(
                                self.window.basalt_ref().device(),
                                attachments: {
                                    user: {
                                        format: self.swapchain_ci.image_format,
                                        samples: 1,
                                        load_op: Load,
                                        store_op: Store,
                                    },
                                    ui: {
                                        format: self.swapchain_ci.image_format,
                                        samples: 1,
                                        load_op: Clear,
                                        store_op: DontCare,
                                    },
                                    sc: {
                                        format: self.swapchain_ci.image_format,
                                        samples: 1,
                                        load_op: DontCare,
                                        store_op: Store,
                                    },
                                },
                                passes: [
                                    {
                                        color: [ui],
                                        depth_stencil: {},
                                        input: [],
                                    },
                                    {
                                        color: [sc],
                                        depth_stencil: {},
                                        input: [user, ui],
                                    }
                                ],
                            )
                            .map_err(VulkanoError::CreateRenderPass)?,
                        );
                    } else {
                        let sample_count = match self.msaa {
                            MSAA::X1 => unreachable!(),
                            MSAA::X2 => 2,
                            MSAA::X4 => 4,
                            MSAA::X8 => 8,
                        };

                        specific.render_pass = Some(
                            vulkano::ordered_passes_renderpass!(
                                self.window.basalt_ref().device(),
                                attachments: {
                                    user: {
                                        format: self.swapchain_ci.image_format,
                                        samples: 1,
                                        load_op: Load,
                                        store_op: Store,
                                    },
                                    ui_ms: {
                                        format: self.swapchain_ci.image_format,
                                        samples: sample_count,
                                        load_op: Clear,
                                        store_op: DontCare,
                                    },
                                    ui: {
                                        format: self.swapchain_ci.image_format,
                                        samples: 1,
                                        load_op: DontCare,
                                        store_op: DontCare,
                                    },
                                    sc: {
                                        format: self.swapchain_ci.image_format,
                                        samples: 1,
                                        load_op: DontCare,
                                        store_op: Store,
                                    },
                                },
                                passes: [
                                    {
                                        color: [ui_ms],
                                        color_resolve: [ui],
                                        depth_stencil: {},
                                        input: [],
                                    },
                                    {
                                        color: [sc],
                                        depth_stencil: {},
                                        input: [user, ui],
                                    }
                                ],
                            )
                            .map_err(VulkanoError::CreateRenderPass)?,
                        );
                    }
                }

                if specific.pipeline_itf.is_none() {
                    specific.pipeline_itf = Some(create_itf_pipeline(
                        self.window.basalt_ref().device(),
                        self.image_capacity,
                        self.msaa,
                        vko::Subpass::from(specific.render_pass.clone().unwrap(), 0).unwrap(),
                    )?);
                }

                if specific.pipeline_final.is_none() {
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

                    let subpass =
                        vko::Subpass::from(specific.render_pass.clone().unwrap(), 1).unwrap();

                    specific.pipeline_final = Some(
                        vko::GraphicsPipeline::new(
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
                                        subpass.num_color_attachments(),
                                        Default::default(),
                                    ),
                                ),
                                dynamic_state: [vko::DynamicState::Viewport].into_iter().collect(),
                                subpass: Some(subpass.into()),
                                ..vko::GraphicsPipelineCreateInfo::new(layout)
                            },
                        )
                        .map_err(VulkanoError::CreateGraphicsPipeline)?,
                    );

                    if specific.final_desc_layout.is_none() {
                        specific.final_desc_layout = Some(
                            specific
                                .pipeline_final
                                .as_ref()
                                .unwrap()
                                .layout()
                                .set_layouts()
                                .first()
                                .unwrap()
                                .clone(),
                        );
                    }
                }

                if framebuffers_rc || specific.framebuffers.is_none() {
                    enqueue_image_destruction(
                        self.window.basalt_ref().device_resources_ref(),
                        self.render_flt_id,
                        specific.remove_images(),
                    );

                    let swapchain_state = self
                        .window
                        .basalt_ref()
                        .device_resources_ref()
                        .swapchain(self.swapchain_id)
                        .unwrap();

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

                    let user_color_view = vko::ImageView::new_default(
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .image(user_color_id)
                            .unwrap()
                            .image()
                            .clone(),
                    )
                    .map_err(VulkanoError::CreateImageView)?;

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

                    let itf_color_view = vko::ImageView::new_default(
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .image(itf_color_id)
                            .unwrap()
                            .image()
                            .clone(),
                    )
                    .map_err(VulkanoError::CreateImageView)?;

                    if self.msaa == MSAA::X1 {
                        let mut framebuffers = Vec::with_capacity(swapchain_state.images().len());

                        for swapchain_image in swapchain_state.images().iter() {
                            framebuffers.push(
                                vko::Framebuffer::new(
                                    specific.render_pass.clone().unwrap(),
                                    vko::FramebufferCreateInfo {
                                        attachments: vec![
                                            user_color_view.clone(),
                                            itf_color_view.clone(),
                                            vko::ImageView::new_default(swapchain_image.clone())
                                                .map_err(VulkanoError::CreateImageView)?,
                                        ],
                                        ..Default::default()
                                    },
                                )
                                .map_err(VulkanoError::CreateFramebuffer)?,
                            );
                        }

                        specific.framebuffers = Some(framebuffers);
                    } else {
                        let sample_count = match self.msaa {
                            MSAA::X1 => unreachable!(),
                            MSAA::X2 => vko::SampleCount::Sample2,
                            MSAA::X4 => vko::SampleCount::Sample4,
                            MSAA::X8 => vko::SampleCount::Sample8,
                        };

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
                                    samples: sample_count,
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

                        let itf_color_ms_view = vko::ImageView::new_default(
                            self.window
                                .basalt_ref()
                                .device_resources_ref()
                                .image(itf_color_ms_id)
                                .unwrap()
                                .image()
                                .clone(),
                        )
                        .map_err(VulkanoError::CreateImageView)?;

                        let mut framebuffers = Vec::with_capacity(swapchain_state.images().len());

                        for swapchain_image in swapchain_state.images().iter() {
                            framebuffers.push(
                                vko::Framebuffer::new(
                                    specific.render_pass.clone().unwrap(),
                                    vko::FramebufferCreateInfo {
                                        attachments: vec![
                                            user_color_view.clone(),
                                            itf_color_ms_view.clone(),
                                            itf_color_view.clone(),
                                            vko::ImageView::new_default(swapchain_image.clone())
                                                .map_err(VulkanoError::CreateImageView)?,
                                        ],
                                        ..Default::default()
                                    },
                                )
                                .map_err(VulkanoError::CreateFramebuffer)?,
                            );
                        }

                        specific.framebuffers = Some(framebuffers);
                    }

                    specific.final_desc_set = Some(
                        vko::DescriptorSet::new(
                            self.desc_alloc.clone(),
                            specific.final_desc_layout.clone().unwrap(),
                            [
                                vko::WriteDescriptorSet::image_view(0, user_color_view),
                                vko::WriteDescriptorSet::image_view(1, itf_color_view),
                            ],
                            [],
                        )
                        .map_err(VulkanoError::CreateDescSet)?,
                    );

                    user_renderer.target_changed(user_color_id);
                }

                if specific.task_graph.is_none() {
                    let mut task_graph =
                        vko::TaskGraph::new(self.window.basalt_ref().device_resources_ref());

                    let vid_swapchain = task_graph.add_swapchain(&self.swapchain_ci);

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

                    let mut node = task_graph.create_task_node(
                        format!("Render[{:?}]", self.window.id()),
                        vko::QueueFamilyType::Graphics,
                        RenderTask,
                    );

                    node.buffer_access(vid_buffer, vko::AccessTypes::VERTEX_ATTRIBUTE_READ)
                        .image_access(
                            vid_user_color,
                            vko::AccessTypes::FRAGMENT_SHADER_COLOR_INPUT_ATTACHMENT_READ,
                            vko::ImageLayoutType::Optimal,
                        );

                    let virtual_ids = if self.msaa == MSAA::X1 {
                        node.image_access(
                            vid_itf_color,
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE
                                | vko::AccessTypes::FRAGMENT_SHADER_COLOR_INPUT_ATTACHMENT_READ,
                            vko::ImageLayoutType::Optimal,
                        )
                        .image_access(
                            vid_swapchain.current_image_id(),
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                        );

                        VirtualIds::UserNoMsaa(UserNoMsaaVIds {
                            swapchain: vid_swapchain,
                            itf_color: vid_itf_color,
                            user_color: vid_user_color,
                            buffer: vid_buffer,
                        })
                    } else {
                        let vid_itf_color_ms = vid_itf_color_ms.unwrap();

                        node.image_access(
                            vid_swapchain.current_image_id(),
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                        )
                        .image_access(
                            vid_itf_color,
                            vko::AccessTypes::RESOLVE_TRANSFER_WRITE
                                | vko::AccessTypes::FRAGMENT_SHADER_COLOR_INPUT_ATTACHMENT_READ,
                            vko::ImageLayoutType::Optimal,
                        )
                        .image_access(
                            vid_itf_color_ms,
                            vko::AccessTypes::COLOR_ATTACHMENT_WRITE,
                            vko::ImageLayoutType::Optimal,
                        );

                        VirtualIds::UserMsaa(UserMsaaVIds {
                            swapchain: vid_swapchain,
                            itf_color: vid_itf_color,
                            itf_color_ms: vid_itf_color_ms,
                            user_color: vid_user_color,
                            buffer: vid_buffer,
                        })
                    };

                    let itf_node_id = node.build();
                    task_graph.add_edge(user_node_id, itf_node_id).unwrap();

                    specific.task_graph = Some(
                        unsafe {
                            task_graph.compile(&vko::CompileInfo {
                                queues: &[self.window.basalt_ref().graphics_queue_ref()],
                                present_queue: Some(self.window.basalt_ref().graphics_queue_ref()),
                                flight_id: self.render_flt_id,
                                ..Default::default()
                            })
                        }
                        .map_err(VulkanoError::from)?,
                    );

                    specific.virtual_ids = Some(virtual_ids);
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

                match specific.virtual_ids.as_ref().unwrap() {
                    VirtualIds::ItfOnlyNoMsaa(vids) => {
                        resource_map
                            .insert_swapchain(vids.swapchain, self.swapchain_id)
                            .unwrap();
                        resource_map.insert_buffer(vids.buffer, buffer_id).unwrap();
                    },
                    VirtualIds::ItfOnlyMsaa(vids) => {
                        resource_map
                            .insert_swapchain(vids.swapchain, self.swapchain_id)
                            .unwrap();
                        resource_map.insert_buffer(vids.buffer, buffer_id).unwrap();
                        resource_map
                            .insert_image(vids.color_ms, specific.color_ms_id.unwrap())
                            .unwrap();
                    },
                    _ => unreachable!(),
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

                match specific.virtual_ids.as_ref().unwrap() {
                    VirtualIds::UserNoMsaa(vids) => {
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
                    },
                    VirtualIds::UserMsaa(vids) => {
                        resource_map
                            .insert_swapchain(vids.swapchain, self.swapchain_id)
                            .unwrap();
                        resource_map.insert_buffer(vids.buffer, buffer_id).unwrap();
                        resource_map
                            .insert_image(vids.itf_color, specific.itf_color_id.unwrap())
                            .unwrap();
                        resource_map
                            .insert_image(vids.itf_color_ms, specific.itf_color_ms_id.unwrap())
                            .unwrap();
                        resource_map
                            .insert_image(vids.user_color, specific.user_color_id.unwrap())
                            .unwrap();
                    },
                    _ => unreachable!(),
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
        println!("[RendererContext.drop][1/3]: building destruction");

        let resources = self.window.basalt_ref().device_resources_ref();
        let mut deferred_batch = resources.create_deferred_batch();

        for image_id in self.specific.remove_images() {
            deferred_batch.destroy_image(image_id);
        }

        deferred_batch.destroy_image(self.default_image_id);
        deferred_batch.destroy_swapchain(self.swapchain_id);
        // TODO: remove render_flt_id

        unsafe {
            deferred_batch.enqueue_with_flights([self.render_flt_id]);
        }

        println!("[RendererContext.drop][1/2]: render flight wait");

        let render_flt = resources.flight(self.render_flt_id).unwrap();
        render_flt.wait_for_frame(render_flt.current_frame() - 1, None).unwrap();

        println!("[RendererContext.drop][3/3]: dropped");
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
            ..vko::GraphicsPipelineCreateInfo::new(layout)
        },
    )
    .map_err(VulkanoError::CreateGraphicsPipeline)
}

struct RenderTask;

impl vko::Task for RenderTask {
    type World = RendererContext;

    unsafe fn execute(
        &self,
        cmd: &mut vko::RecordingCommandBuffer<'_>,
        task: &mut vko::TaskContext<'_>,
        context: &Self::World,
    ) -> vko::TaskResult {
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

                unsafe {
                    cmd.as_raw().begin_render_pass(
                        &vko::RenderPassBeginInfo {
                            clear_values,
                            ..vko::RenderPassBeginInfo::framebuffer(
                                framebuffers[image_index as usize].clone(),
                            )
                        },
                        &Default::default(),
                    )
                }?;

                cmd.destroy_objects(iter::once(framebuffers[image_index as usize].clone()));
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

                unsafe { cmd.as_raw().end_render_pass(&Default::default()) }?;
            },
            Specific::User(specific) => {
                let framebuffers = specific.framebuffers.as_ref().unwrap();
                let pipeline_itf = specific.pipeline_itf.as_ref().unwrap();
                let pipeline_final = specific.pipeline_final.as_ref().unwrap();

                let clear_values = if specific.itf_color_ms_id.is_none() {
                    vec![
                        None,
                        Some(clear_value_for_format(
                            framebuffers[0].attachments()[0].format(),
                        )),
                        None,
                    ]
                } else {
                    vec![
                        None,
                        Some(clear_value_for_format(
                            framebuffers[0].attachments()[0].format(),
                        )),
                        None,
                        None,
                    ]
                };

                unsafe {
                    cmd.as_raw().begin_render_pass(
                        &vko::RenderPassBeginInfo {
                            clear_values,
                            ..vko::RenderPassBeginInfo::framebuffer(
                                framebuffers[image_index as usize].clone(),
                            )
                        },
                        &Default::default(),
                    )
                }?;

                cmd.destroy_objects(iter::once(framebuffers[image_index as usize].clone()));
                unsafe { cmd.set_viewport(0, std::slice::from_ref(&context.viewport)) }?;
                unsafe { cmd.bind_pipeline_graphics(pipeline_itf) }?;

                if let (Some(desc_set), Some(buffer_id), Some(draw_count)) = (
                    context.desc_set.as_ref(),
                    context.buffer_id.as_ref(),
                    context.draw_count,
                ) {
                    unsafe {
                        cmd.as_raw().bind_descriptor_sets(
                            vko::PipelineBindPoint::Graphics,
                            pipeline_itf.layout(),
                            0,
                            &[desc_set.as_raw()],
                            &[],
                        )
                    }?;

                    cmd.destroy_objects(iter::once(desc_set.clone()));
                    unsafe { cmd.bind_vertex_buffers(0, &[*buffer_id], &[0], &[], &[]) }?;
                    unsafe { cmd.draw(draw_count, 1, 0, 0) }?;
                } else {
                    unreachable!()
                }

                unsafe {
                    cmd.as_raw()
                        .next_subpass(&Default::default(), &Default::default())?;
                }

                unsafe { cmd.set_viewport(0, std::slice::from_ref(&context.viewport)) }?;
                unsafe { cmd.bind_pipeline_graphics(pipeline_final) }?;
                let final_desc_set = specific.final_desc_set.clone().unwrap();

                unsafe {
                    cmd.as_raw().bind_descriptor_sets(
                        vko::PipelineBindPoint::Graphics,
                        pipeline_final.layout(),
                        0,
                        &[final_desc_set.as_raw()],
                        &[],
                    )
                }?;

                cmd.destroy_objects(iter::once(final_desc_set));
                unsafe { cmd.draw(3, 1, 0, 0) }?;
                unsafe { cmd.as_raw().end_render_pass(&Default::default()) }?;
            },
            Specific::None => unreachable!(),
        }

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

fn enqueue_image_destruction(
    resources: &Arc<vko::Resources>,
    flight_id: vko::Id<vko::Flight>,
    image_ids: Vec<vko::Id<vko::Image>>,
) {
    if image_ids.is_empty() {
        return;
    }

    let current_frame = resources.flight(flight_id).unwrap().current_frame();
    let mut deferred_batch = resources.create_deferred_batch();

    for image_id in image_ids {
        deferred_batch.destroy_image(image_id);
    }

    unsafe {
        deferred_batch.enqueue_with_frames([(flight_id, current_frame)]);
    }
}
