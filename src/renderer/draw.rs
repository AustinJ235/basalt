use std::sync::Arc;

use vulkano::buffer::Subbuffer;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, PrimaryAutoCommandBuffer, RenderPassBeginInfo, SubpassBeginInfo,
    SubpassEndInfo,
};
use vulkano::descriptor_set::persistent::PersistentDescriptorSet;
use vulkano::device::Device;
use vulkano::format::{ClearColorValue, ClearValue, Format, NumericFormat};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage};
use vulkano::memory::allocator::{AllocationCreateInfo, StandardMemoryAllocator};
use vulkano::pipeline::graphics::color_blend::{
    AttachmentBlend, ColorBlendAttachmentState, ColorBlendState,
};
use vulkano::pipeline::graphics::depth_stencil::{DepthState, DepthStencilState};
use vulkano::pipeline::graphics::input_assembly::InputAssemblyState;
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::RasterizationState;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::{
    DynamicState, GraphicsPipeline, Pipeline, PipelineBindPoint, PipelineLayout,
    PipelineShaderStageCreateInfo,
};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};

use crate::interface::ItfVertInfo;
use crate::renderer::shaders;

pub enum DrawState {
    InterfaceOnly(InterfaceOnly),
}

pub struct InterfaceOnly {
    render_pass: Arc<RenderPass>,
    pipeline: Arc<GraphicsPipeline>,
    framebuffers: Option<Vec<Arc<Framebuffer>>>,
}

impl InterfaceOnly {
    fn create_pipeline(
        device: Arc<Device>,
        render_pass: Arc<RenderPass>,
        image_capacity: u32,
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

        let subpass = Subpass::from(render_pass, 0).unwrap();

        GraphicsPipeline::new(
            device,
            None,
            GraphicsPipelineCreateInfo {
                stages: stages.into_iter().collect(),
                vertex_input_state: Some(vertex_input_state),
                input_assembly_state: Some(InputAssemblyState::default()),
                viewport_state: Some(ViewportState::default()),
                rasterization_state: Some(RasterizationState::default()),
                depth_stencil_state: Some(DepthStencilState {
                    depth: Some(DepthState::simple()),
                    ..Default::default()
                }),
                multisample_state: Some(MultisampleState::default()),
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
}

impl DrawState {
    pub fn interface_only(
        device: Arc<Device>,
        surface_format: Format,
        image_capacity: u32,
    ) -> Self {
        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: surface_format,
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                },
                depth_stencil: {
                    format: Format::D16_UNORM,
                    samples: 1,
                    load_op: Clear,
                    store_op: DontCare,
                },
            },
            pass: {
                color: [color],
                depth_stencil: {depth_stencil},
            }
        )
        .unwrap();

        let pipeline = InterfaceOnly::create_pipeline(device, render_pass.clone(), image_capacity);

        Self::InterfaceOnly(InterfaceOnly {
            render_pass,
            pipeline,
            framebuffers: None,
        })
    }

    pub fn update_framebuffers(
        &mut self,
        mem_alloc: Arc<StandardMemoryAllocator>,
        swapchain_views: Vec<Arc<ImageView>>,
    ) {
        match self {
            Self::InterfaceOnly(state) => {
                let depth_buffer = ImageView::new_default(
                    Image::new(
                        mem_alloc,
                        ImageCreateInfo {
                            image_type: ImageType::Dim2d,
                            format: Format::D16_UNORM,
                            extent: swapchain_views[0].image().extent(),
                            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT
                                | ImageUsage::TRANSIENT_ATTACHMENT,
                            ..Default::default()
                        },
                        AllocationCreateInfo::default(),
                    )
                    .unwrap(),
                )
                .unwrap();

                state.framebuffers = Some(
                    swapchain_views
                        .into_iter()
                        .map(|swapchain_view| {
                            Framebuffer::new(
                                state.render_pass.clone(),
                                FramebufferCreateInfo {
                                    attachments: vec![swapchain_view, depth_buffer.clone()],
                                    ..FramebufferCreateInfo::default()
                                },
                            )
                            .unwrap()
                        })
                        .collect(),
                );
            },
        }
    }

    pub fn update_image_capacity(&mut self, device: Arc<Device>, image_capacity: u32) {
        match self {
            Self::InterfaceOnly(state) => {
                state.pipeline = InterfaceOnly::create_pipeline(
                    device,
                    state.render_pass.clone(),
                    image_capacity,
                );
            },
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
        let buffer_len = buffer.len();

        match self {
            Self::InterfaceOnly(state) => {
                cmd_builder
                    .begin_render_pass(
                        RenderPassBeginInfo {
                            clear_values: vec![
                                Some(clear_value_for_format(
                                    state.framebuffers.as_ref().unwrap()[0].attachments()[0]
                                        .format(),
                                )),
                                Some(ClearValue::Depth(1.0)),
                            ],
                            ..RenderPassBeginInfo::framebuffer(
                                state.framebuffers.as_ref().unwrap()[swapchain_image_index].clone(),
                            )
                        },
                        SubpassBeginInfo::default(),
                    )
                    .unwrap()
                    .set_viewport(0, [viewport].into_iter().collect())
                    .unwrap()
                    .bind_pipeline_graphics(state.pipeline.clone())
                    .unwrap()
                    .bind_descriptor_sets(
                        PipelineBindPoint::Graphics,
                        state.pipeline.layout().clone(),
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
