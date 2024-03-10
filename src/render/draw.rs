use std::sync::Arc;

use vulkano::buffer::Subbuffer;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, PrimaryAutoCommandBuffer, RenderPassBeginInfo, SubpassBeginInfo,
    SubpassEndInfo,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::layout::DescriptorSetLayout;
use vulkano::descriptor_set::persistent::PersistentDescriptorSet;
use vulkano::descriptor_set::WriteDescriptorSet;
use vulkano::device::Device;
use vulkano::format::{ClearColorValue, ClearValue, Format, NumericFormat};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage, SampleCount};
use vulkano::memory::allocator::{AllocationCreateInfo, StandardMemoryAllocator};
use vulkano::pipeline::graphics::color_blend::{
    AttachmentBlend, ColorBlendAttachmentState, ColorBlendState,
};
use vulkano::pipeline::graphics::input_assembly::InputAssemblyState;
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::RasterizationState;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition, VertexInputState};
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::layout::PipelineDescriptorSetLayoutCreateInfo;
use vulkano::pipeline::{
    DynamicState, GraphicsPipeline, Pipeline, PipelineBindPoint, PipelineLayout,
    PipelineShaderStageCreateInfo,
};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};

use crate::interface::ItfVertInfo;
use crate::render::{shaders, UserRenderer, MSAA};

pub enum DrawState {
    InterfaceOnly(InterfaceOnly),
    User(User),
}

#[derive(Default)]
pub struct InterfaceOnly {
    msaa: Option<MSAA>,
    render_pass: Option<Arc<RenderPass>>,
    pipeline: Option<Arc<GraphicsPipeline>>,
    framebuffers: Option<Vec<Arc<Framebuffer>>>,
}

impl InterfaceOnly {
    fn create_render_pass(&mut self, device: Arc<Device>, surface_format: Format, msaa: MSAA) {
        self.msaa = Some(msaa);

        self.render_pass = Some(match msaa {
            MSAA::X1 => {
                vulkano::single_pass_renderpass!(
                    device.clone(),
                    attachments: {
                        color: {
                            format: surface_format,
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
                .unwrap()
            },
            msaa => {
                let sample_count = match msaa {
                    MSAA::X1 => unreachable!(),
                    MSAA::X2 => 2,
                    MSAA::X4 => 4,
                    MSAA::X8 => 8,
                };

                vulkano::single_pass_renderpass!(
                    device.clone(),
                    attachments: {
                        color_ms: {
                            format: surface_format,
                            samples: sample_count,
                            load_op: Clear,
                            store_op: DontCare,
                        },
                        color: {
                            format: surface_format,
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
                .unwrap()
            },
        });

        self.pipeline = None;
        self.framebuffers = None;
    }

    fn create_pipeline(&mut self, device: Arc<Device>, image_capacity: u32) {
        self.pipeline = Some(create_ui_pipeline(
            device,
            image_capacity,
            self.msaa.clone().unwrap(),
            Subpass::from(self.render_pass.clone().unwrap(), 0).unwrap(),
        ));
    }

    fn create_framebuffers(
        &mut self,
        mem_alloc: &Arc<StandardMemoryAllocator>,
        swapchain_views: Vec<Arc<ImageView>>,
    ) {
        self.framebuffers = Some(match self.msaa.clone().unwrap() {
            MSAA::X1 => {
                swapchain_views
                    .into_iter()
                    .map(|swapchain_view| {
                        Framebuffer::new(
                            self.render_pass.clone().unwrap(),
                            FramebufferCreateInfo {
                                attachments: vec![swapchain_view],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap()
                    })
                    .collect()
            },
            msaa => {
                let sample_count = match msaa {
                    MSAA::X1 => unreachable!(),
                    MSAA::X2 => SampleCount::Sample2,
                    MSAA::X4 => SampleCount::Sample4,
                    MSAA::X8 => SampleCount::Sample8,
                };

                let color_ms = ImageView::new_default(
                    Image::new(
                        mem_alloc.clone(),
                        ImageCreateInfo {
                            image_type: ImageType::Dim2d,
                            format: swapchain_views[0].format(),
                            extent: swapchain_views[0].image().extent(),
                            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSIENT_ATTACHMENT,
                            samples: sample_count,
                            ..ImageCreateInfo::default()
                        },
                        AllocationCreateInfo::default(), // TODO: Be specific
                    )
                    .unwrap(),
                )
                .unwrap();

                swapchain_views
                    .into_iter()
                    .map(|swapchain_view| {
                        Framebuffer::new(
                            self.render_pass.clone().unwrap(),
                            FramebufferCreateInfo {
                                attachments: vec![color_ms.clone(), swapchain_view],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap()
                    })
                    .collect()
            },
        });
    }

    fn draw(
        &mut self,
        buffer: Subbuffer<[ItfVertInfo]>,
        desc_set: Arc<PersistentDescriptorSet>,
        swapchain_image_index: usize,
        viewport: Viewport,
        cmd_builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    ) {
        let buffer_len = buffer.len();
        let clear_values = match self.msaa.clone().unwrap() {
            MSAA::X1 => {
                vec![Some(clear_value_for_format(
                    self.framebuffers.as_ref().unwrap()[0].attachments()[0].format(),
                ))]
            },
            _ => {
                vec![
                    Some(clear_value_for_format(
                        self.framebuffers.as_ref().unwrap()[0].attachments()[0].format(),
                    )),
                    None,
                ]
            },
        };

        cmd_builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values,
                    ..RenderPassBeginInfo::framebuffer(
                        self.framebuffers.as_ref().unwrap()[swapchain_image_index].clone(),
                    )
                },
                SubpassBeginInfo::default(),
            )
            .unwrap()
            .set_viewport(0, [viewport].into_iter().collect())
            .unwrap()
            .bind_pipeline_graphics(self.pipeline.clone().unwrap())
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                self.pipeline.as_ref().unwrap().layout().clone(),
                0,
                desc_set,
            )
            .unwrap()
            .bind_vertex_buffers(0, buffer)
            .unwrap()
            .draw(buffer_len as u32, 1, 0, 0)
            .unwrap()
            .end_render_pass(SubpassEndInfo::default())
            .unwrap();
    }
}

pub struct User {
    user_renderer: Box<dyn UserRenderer + Send + 'static>,
    msaa: Option<MSAA>,
    render_pass: Option<Arc<RenderPass>>,
    pipeline_ui: Option<Arc<GraphicsPipeline>>,
    pipeline_final: Option<Arc<GraphicsPipeline>>,
    framebuffers: Option<Vec<Arc<Framebuffer>>>,
    final_desc_layout: Option<Arc<DescriptorSetLayout>>,
    final_set: Option<Arc<PersistentDescriptorSet>>,
}

impl User {
    fn new<T: UserRenderer + Send + 'static>(user_renderer: T) -> Self {
        Self {
            user_renderer: Box::new(user_renderer),
            msaa: None,
            render_pass: None,
            pipeline_ui: None,
            pipeline_final: None,
            framebuffers: None,
            final_desc_layout: None,
            final_set: None,
        }
    }

    fn create_render_pass(&mut self, device: Arc<Device>, surface_format: Format, msaa: MSAA) {
        self.msaa = Some(msaa);

        self.render_pass = Some(match msaa {
            MSAA::X1 => {
                vulkano::ordered_passes_renderpass!(
                    device.clone(),
                    attachments: {
                        user: {
                            format: surface_format,
                            samples: 1,
                            load_op: Load,
                            store_op: Store,
                        },
                        ui: {
                            format: surface_format,
                            samples: 1,
                            load_op: Clear,
                            store_op: DontCare,
                        },
                        sc: {
                            format: surface_format,
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
                .unwrap()
            },
            msaa => {
                let sample_count = match msaa {
                    MSAA::X1 => unreachable!(),
                    MSAA::X2 => 2,
                    MSAA::X4 => 4,
                    MSAA::X8 => 8,
                };

                vulkano::ordered_passes_renderpass!(
                    device.clone(),
                    attachments: {
                        user: {
                            format: surface_format,
                            samples: 1,
                            load_op: Load,
                            store_op: Store,
                        },
                        ui_ms: {
                            format: surface_format,
                            samples: sample_count,
                            load_op: Clear,
                            store_op: DontCare,
                        },
                        ui: {
                            format: surface_format,
                            samples: 1,
                            load_op: DontCare,
                            store_op: DontCare,
                        },
                        sc: {
                            format: surface_format,
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
                .unwrap()
            },
        });

        self.pipeline_ui = None;
        self.pipeline_final = None;
        self.framebuffers = None;
        self.final_set = None;
    }

    fn create_pipeline(&mut self, device: Arc<Device>, image_capacity: u32) {
        self.pipeline_ui = Some(create_ui_pipeline(
            device.clone(),
            image_capacity,
            self.msaa.clone().unwrap(),
            Subpass::from(self.render_pass.clone().unwrap(), 0).unwrap(),
        ));

        if self.pipeline_final.is_none() {
            let final_vs = shaders::final_vs_sm(device.clone())
                .entry_point("main")
                .unwrap();

            let final_fs = shaders::final_fs_sm(device.clone())
                .entry_point("main")
                .unwrap();

            let stages = [
                PipelineShaderStageCreateInfo::new(final_vs),
                PipelineShaderStageCreateInfo::new(final_fs),
            ];

            let layout = PipelineLayout::new(
                device.clone(),
                PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
                    .into_pipeline_layout_create_info(device.clone())
                    .unwrap(),
            )
            .unwrap();

            let subpass = Subpass::from(self.render_pass.clone().unwrap(), 1).unwrap();

            let pipeline_final = GraphicsPipeline::new(
                device,
                None,
                GraphicsPipelineCreateInfo {
                    stages: stages.into_iter().collect(),
                    vertex_input_state: Some(VertexInputState::new()),
                    input_assembly_state: Some(InputAssemblyState::default()),
                    viewport_state: Some(ViewportState::default()),
                    rasterization_state: Some(RasterizationState::default()),
                    multisample_state: Some(MultisampleState::default()),
                    color_blend_state: Some(ColorBlendState::with_attachment_states(
                        subpass.num_color_attachments(),
                        ColorBlendAttachmentState::default(),
                    )),
                    dynamic_state: [DynamicState::Viewport].into_iter().collect(),
                    subpass: Some(subpass.into()),
                    ..GraphicsPipelineCreateInfo::layout(layout)
                },
            )
            .unwrap();

            if self.final_desc_layout.is_none() {
                self.final_desc_layout = Some(
                    pipeline_final
                        .layout()
                        .set_layouts()
                        .get(0)
                        .unwrap()
                        .clone(),
                );
            }

            self.pipeline_final = Some(pipeline_final);
        }
    }

    fn create_framebuffers(
        &mut self,
        mem_alloc: &Arc<StandardMemoryAllocator>,
        desc_alloc: &StandardDescriptorSetAllocator,
        swapchain_views: Vec<Arc<ImageView>>,
    ) {
        let user_color = ImageView::new_default(
            Image::new(
                mem_alloc.clone(),
                ImageCreateInfo {
                    image_type: ImageType::Dim2d,
                    format: swapchain_views[0].format(),
                    extent: swapchain_views[0].image().extent(),
                    usage: ImageUsage::COLOR_ATTACHMENT
                        | ImageUsage::INPUT_ATTACHMENT
                        | ImageUsage::TRANSFER_DST,
                    ..ImageCreateInfo::default()
                },
                AllocationCreateInfo::default(), // TODO: Be specific
            )
            .unwrap(),
        )
        .unwrap();

        let ui_color = ImageView::new_default(
            Image::new(
                mem_alloc.clone(),
                ImageCreateInfo {
                    image_type: ImageType::Dim2d,
                    format: swapchain_views[0].format(),
                    extent: swapchain_views[0].image().extent(),
                    usage: ImageUsage::COLOR_ATTACHMENT
                        | ImageUsage::INPUT_ATTACHMENT
                        | ImageUsage::TRANSFER_DST,
                    ..ImageCreateInfo::default()
                },
                AllocationCreateInfo::default(), // TODO: Be specific
            )
            .unwrap(),
        )
        .unwrap();

        self.user_renderer.target_changed(user_color.clone());

        self.framebuffers = Some(match self.msaa.clone().unwrap() {
            MSAA::X1 => {
                swapchain_views
                    .into_iter()
                    .map(|swapchain_view| {
                        Framebuffer::new(
                            self.render_pass.clone().unwrap(),
                            FramebufferCreateInfo {
                                attachments: vec![
                                    user_color.clone(),
                                    ui_color.clone(),
                                    swapchain_view,
                                ],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap()
                    })
                    .collect()
            },
            msaa => {
                let sample_count = match msaa {
                    MSAA::X1 => unreachable!(),
                    MSAA::X2 => SampleCount::Sample2,
                    MSAA::X4 => SampleCount::Sample4,
                    MSAA::X8 => SampleCount::Sample8,
                };

                let ui_color_ms = ImageView::new_default(
                    Image::new(
                        mem_alloc.clone(),
                        ImageCreateInfo {
                            image_type: ImageType::Dim2d,
                            format: swapchain_views[0].format(),
                            extent: swapchain_views[0].image().extent(),
                            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSIENT_ATTACHMENT,
                            samples: sample_count,
                            ..ImageCreateInfo::default()
                        },
                        AllocationCreateInfo::default(), // TODO: Be specific
                    )
                    .unwrap(),
                )
                .unwrap();

                swapchain_views
                    .into_iter()
                    .map(|swapchain_view| {
                        Framebuffer::new(
                            self.render_pass.clone().unwrap(),
                            FramebufferCreateInfo {
                                attachments: vec![
                                    user_color.clone(),
                                    ui_color_ms.clone(),
                                    ui_color.clone(),
                                    swapchain_view,
                                ],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap()
                    })
                    .collect()
            },
        });

        self.final_set = Some(
            PersistentDescriptorSet::new(
                desc_alloc,
                self.final_desc_layout.clone().unwrap(),
                [
                    WriteDescriptorSet::image_view(0, user_color),
                    WriteDescriptorSet::image_view(1, ui_color),
                ],
                [],
            )
            .unwrap(),
        );
    }

    fn draw(
        &mut self,
        buffer: Subbuffer<[ItfVertInfo]>,
        desc_set: Arc<PersistentDescriptorSet>,
        swapchain_image_index: usize,
        viewport: Viewport,
        cmd_builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    ) {
        self.user_renderer.draw(cmd_builder);
        let buffer_len = buffer.len();

        let clear_values = match self.msaa.clone().unwrap() {
            MSAA::X1 => {
                vec![
                    None,
                    Some(clear_value_for_format(
                        self.framebuffers.as_ref().unwrap()[0].attachments()[1].format(),
                    )),
                    None,
                ]
            },
            _ => {
                vec![
                    None,
                    Some(clear_value_for_format(
                        self.framebuffers.as_ref().unwrap()[0].attachments()[1].format(),
                    )),
                    None,
                    None,
                ]
            },
        };

        cmd_builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values,
                    ..RenderPassBeginInfo::framebuffer(
                        self.framebuffers.as_ref().unwrap()[swapchain_image_index].clone(),
                    )
                },
                SubpassBeginInfo::default(),
            )
            .unwrap()
            .set_viewport(0, [viewport.clone()].into_iter().collect())
            .unwrap()
            .bind_pipeline_graphics(self.pipeline_ui.clone().unwrap())
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                self.pipeline_ui.as_ref().unwrap().layout().clone(),
                0,
                desc_set,
            )
            .unwrap()
            .bind_vertex_buffers(0, buffer)
            .unwrap()
            .draw(buffer_len as u32, 1, 0, 0)
            .unwrap()
            .next_subpass(SubpassEndInfo::default(), SubpassBeginInfo::default())
            .unwrap()
            .set_viewport(0, [viewport].into_iter().collect())
            .unwrap()
            .bind_pipeline_graphics(self.pipeline_final.clone().unwrap())
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                self.pipeline_final.as_ref().unwrap().layout().clone(),
                0,
                self.final_set.clone().unwrap(),
            )
            .unwrap()
            .draw(3, 1, 0, 0)
            .unwrap()
            .end_render_pass(SubpassEndInfo::default())
            .unwrap();
    }
}

fn create_ui_pipeline(
    device: Arc<Device>,
    image_capacity: u32,
    msaa: MSAA,
    subpass: Subpass,
) -> Arc<GraphicsPipeline> {
    let ui_vs = shaders::ui_vs_sm(device.clone())
        .entry_point("main")
        .unwrap();

    let ui_fs = shaders::ui_fs_sm(device.clone())
        .entry_point("main")
        .unwrap();

    let vertex_input_state = ItfVertInfo::per_vertex()
        .definition(&ui_vs.info().input_interface)
        .unwrap();

    let stages = [
        PipelineShaderStageCreateInfo::new(ui_vs),
        PipelineShaderStageCreateInfo::new(ui_fs),
    ];

    let layout = PipelineLayout::new(
        device.clone(),
        shaders::pipeline_descriptor_set_layout_create_info(image_capacity)
            .into_pipeline_layout_create_info(device.clone())
            .unwrap(),
    )
    .unwrap();

    let sample_count = match msaa {
        MSAA::X1 => SampleCount::Sample1,
        MSAA::X2 => SampleCount::Sample2,
        MSAA::X4 => SampleCount::Sample4,
        MSAA::X8 => SampleCount::Sample8,
    };

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: Some(InputAssemblyState::default()),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            multisample_state: Some(MultisampleState {
                rasterization_samples: sample_count,
                ..MultisampleState::default()
            }),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState {
                    blend: Some(AttachmentBlend::alpha()),
                    ..ColorBlendAttachmentState::default()
                },
            )),
            dynamic_state: [DynamicState::Viewport].into_iter().collect(),
            subpass: Some(subpass.into()),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .unwrap()
}

impl DrawState {
    pub fn interface_only(
        device: Arc<Device>,
        surface_format: Format,
        image_capacity: u32,
        msaa: MSAA,
    ) -> Self {
        let mut state = InterfaceOnly::default();
        state.create_render_pass(device.clone(), surface_format, msaa);
        state.create_pipeline(device, image_capacity);
        Self::InterfaceOnly(state)
    }

    pub fn user<T: UserRenderer + Send + 'static>(
        device: Arc<Device>,
        surface_format: Format,
        image_capacity: u32,
        msaa: MSAA,
        user_renderer: T,
    ) -> Self {
        let mut state = User::new(user_renderer);
        state.create_render_pass(device.clone(), surface_format, msaa);
        state.create_pipeline(device, image_capacity);
        Self::User(state)
    }

    pub fn update_framebuffers(
        &mut self,
        mem_alloc: &Arc<StandardMemoryAllocator>,
        desc_alloc: &StandardDescriptorSetAllocator,
        swapchain_views: Vec<Arc<ImageView>>,
    ) {
        match self {
            Self::InterfaceOnly(state) => state.create_framebuffers(mem_alloc, swapchain_views),
            Self::User(state) => state.create_framebuffers(mem_alloc, desc_alloc, swapchain_views),
        }
    }

    pub fn update_msaa(
        &mut self,
        device: Arc<Device>,
        surface_format: Format,
        image_capacity: u32,
        msaa: MSAA,
    ) {
        match self {
            Self::InterfaceOnly(state) => {
                state.create_render_pass(device.clone(), surface_format, msaa);
                state.create_pipeline(device.clone(), image_capacity);
            },
            Self::User(state) => {
                state.create_render_pass(device.clone(), surface_format, msaa);
                state.create_pipeline(device.clone(), image_capacity);
            },
        }
    }

    pub fn update_image_capacity(&mut self, device: Arc<Device>, image_capacity: u32) {
        match self {
            Self::InterfaceOnly(state) => state.create_pipeline(device, image_capacity),
            Self::User(state) => state.create_pipeline(device, image_capacity),
        }
    }

    pub fn draw(
        &mut self,
        buffer: Subbuffer<[ItfVertInfo]>,
        desc_set: Arc<PersistentDescriptorSet>,
        swapchain_image_index: usize,
        viewport: Viewport,
        cmd_builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    ) {
        match self {
            Self::InterfaceOnly(state) => {
                state.draw(
                    buffer,
                    desc_set,
                    swapchain_image_index,
                    viewport,
                    cmd_builder,
                )
            },
            Self::User(state) => {
                state.draw(
                    buffer,
                    desc_set,
                    swapchain_image_index,
                    viewport,
                    cmd_builder,
                )
            },
        }
    }
}

pub fn clear_value_for_format(format: Format) -> ClearValue {
    match format.numeric_format_color().unwrap() {
        NumericFormat::SFLOAT
        | NumericFormat::UFLOAT
        | NumericFormat::SNORM
        | NumericFormat::UNORM
        | NumericFormat::SRGB => ClearValue::Float([0.0; 4]),
        NumericFormat::SINT | NumericFormat::SSCALED => ClearValue::Int([0; 4]),
        NumericFormat::UINT | NumericFormat::USCALED => ClearValue::Uint([0; 4]),
    }
}

pub fn clear_color_value_for_format(format: Format) -> ClearColorValue {
    match format.numeric_format_color().unwrap() {
        NumericFormat::SFLOAT
        | NumericFormat::UFLOAT
        | NumericFormat::SNORM
        | NumericFormat::UNORM
        | NumericFormat::SRGB => ClearColorValue::Float([0.0; 4]),
        NumericFormat::SINT | NumericFormat::SSCALED => ClearColorValue::Int([0; 4]),
        NumericFormat::UINT | NumericFormat::USCALED => ClearColorValue::Uint([0; 4]),
    }
}
