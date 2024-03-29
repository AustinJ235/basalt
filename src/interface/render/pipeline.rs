use std::iter;
use std::sync::Arc;
use std::time::Instant;

use vulkano::buffer::subbuffer::Subbuffer;
use vulkano::buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, ClearColorImageInfo, CopyBufferInfo, CopyImageInfo,
    PrimaryAutoCommandBuffer, RenderPassBeginInfo, SubpassContents,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet};
use vulkano::device::Device;
use vulkano::format::{ClearColorValue, ClearValue, Format as VkFormat};
use vulkano::image::attachment::AttachmentImage;
use vulkano::image::ImageUsage;
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryUsage, StandardMemoryAllocator};
use vulkano::pipeline::cache::PipelineCache;
use vulkano::pipeline::graphics::depth_stencil::DepthStencilState;
use vulkano::pipeline::graphics::input_assembly::{InputAssemblyState, PrimitiveTopology};
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::{CullMode, PolygonMode, RasterizationState};
use vulkano::pipeline::graphics::vertex_input::Vertex;
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::{GraphicsPipeline, Pipeline, PipelineBindPoint};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
use vulkano::sampler::{Sampler, SamplerCreateInfo};
use vulkano::shader::ShaderModule;
use vulkano::DeviceSize;

use crate::atlas::Atlas;
use crate::image_view::BstImageView;
use crate::interface::render::composer::ComposerView;
use crate::interface::render::layer_desc_pool::LayerDescPool;
use crate::interface::render::{
    final_fs, layer_fs, layer_vs, square_vs, ItfDrawTarget, ItfDrawTargetInfo,
};
use crate::interface::ItfVertInfo;
use crate::{BstMSAALevel, BstOptions};

const ITF_VERTEX_SIZE: DeviceSize = std::mem::size_of::<ItfVertInfo>() as DeviceSize;

pub(super) struct ItfPipeline {
    device: Arc<Device>,
    mem_alloc: StandardMemoryAllocator,
    itf_format: VkFormat,
    atlas: Arc<Atlas>,
    context: Option<Context>,
    layer_vs: Arc<ShaderModule>,
    layer_fs: Arc<ShaderModule>,
    square_vs: Arc<ShaderModule>,
    final_fs: Arc<ShaderModule>,
    final_vert_buf: Option<Subbuffer<[SquareShaderVertex]>>,
    pipeline_cache: Arc<PipelineCache>,
    image_sampler: Arc<Sampler>,
    conservative_draw: bool,
    empty_image: Arc<BstImageView>,
    set_alloc: StandardDescriptorSetAllocator,
}

struct Context {
    auxiliary_images: Vec<Arc<BstImageView>>,
    #[allow(dead_code)]
    layer_renderpass: Arc<RenderPass>,
    #[allow(dead_code)]
    final_renderpass: Arc<RenderPass>,
    layer_pipeline: Arc<GraphicsPipeline>,
    final_pipeline: Arc<GraphicsPipeline>,
    e_layer_fb: Arc<Framebuffer>,
    o_layer_fb: Arc<Framebuffer>,
    final_fbs: Vec<Arc<Framebuffer>>,
    layer_set_pool: LayerDescPool,
    layer_clear_values: Vec<Option<ClearValue>>,
    image_capacity: usize,
    cons_draw_last_view: Option<Instant>,
}

#[derive(BufferContents, Vertex, Clone, Debug)]
#[repr(C)]
struct SquareShaderVertex {
    #[format(R32G32_SFLOAT)]
    pub position: [f32; 2],
}

// vulkano::impl_vertex!(SquareShaderVertex, position);

pub struct ItfPipelineInit {
    pub options: BstOptions,
    pub device: Arc<Device>,
    pub atlas: Arc<Atlas>,
    pub itf_format: VkFormat,
}

impl ItfPipeline {
    pub fn new(init: ItfPipelineInit) -> Self {
        let ItfPipelineInit {
            options,
            device,
            atlas,
            itf_format,
        } = init;

        let mem_alloc = StandardMemoryAllocator::new_default(device.clone());
        let set_alloc = StandardDescriptorSetAllocator::new(device.clone());

        Self {
            context: None,
            layer_vs: layer_vs::load(device.clone()).unwrap(),
            layer_fs: layer_fs::load(device.clone()).unwrap(),
            square_vs: square_vs::load(device.clone()).unwrap(),
            final_fs: final_fs::load(device.clone()).unwrap(),
            final_vert_buf: None,
            pipeline_cache: PipelineCache::empty(device.clone()).unwrap(),
            image_sampler: Sampler::new(
                device.clone(),
                SamplerCreateInfo {
                    mag_filter: vulkano::sampler::Filter::Nearest,
                    address_mode: [vulkano::sampler::SamplerAddressMode::Repeat; 3],
                    lod: 0.0..=0.0,
                    ..SamplerCreateInfo::default()
                },
            )
            .unwrap(),
            conservative_draw: options.app_loop && options.conservative_draw,
            empty_image: atlas.empty_image(),
            device,
            atlas,
            itf_format,
            mem_alloc,
            set_alloc,
        }
    }

    pub fn draw(
        &mut self,
        recreate_pipeline: bool,
        view: &ComposerView,
        target: ItfDrawTarget,
        target_info: &ItfDrawTargetInfo,
        mut cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    ) -> (
        AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        Option<Arc<BstImageView>>,
    ) {
        if self.final_vert_buf.is_none() {
            let src = Buffer::from_iter(
                &self.mem_alloc,
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_SRC,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    usage: MemoryUsage::Upload,
                    ..Default::default()
                },
                vec![
                    SquareShaderVertex {
                        position: [-1.0, -1.0],
                    },
                    SquareShaderVertex {
                        position: [1.0, -1.0],
                    },
                    SquareShaderVertex {
                        position: [1.0, 1.0],
                    },
                    SquareShaderVertex {
                        position: [1.0, 1.0],
                    },
                    SquareShaderVertex {
                        position: [-1.0, 1.0],
                    },
                    SquareShaderVertex {
                        position: [-1.0, -1.0],
                    },
                ],
            )
            .unwrap();

            let dst = Buffer::new_slice(
                &self.mem_alloc,
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_DST | BufferUsage::VERTEX_BUFFER,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    usage: MemoryUsage::DeviceOnly,
                    ..Default::default()
                },
                6,
            )
            .unwrap();

            cmd.copy_buffer(CopyBufferInfo::buffers(src, dst.clone()))
                .unwrap();
            self.final_vert_buf = Some(dst);
        }

        if recreate_pipeline
            || self.context.is_none()
            || (self.context.is_some()
                && view.images.len() > self.context.as_ref().unwrap().image_capacity)
        {
            let mut image_capacity = self.context.as_ref().map(|c| c.image_capacity).unwrap_or(1);

            while image_capacity < view.images.len() {
                image_capacity *= 2;
            }

            let mut auxiliary_images: Vec<Arc<BstImageView>> = (0..4)
                .map(|_| {
                    BstImageView::from_attachment(
                        AttachmentImage::with_usage(
                            &self.mem_alloc,
                            target_info.extent(),
                            self.itf_format,
                            ImageUsage::COLOR_ATTACHMENT
                                | ImageUsage::SAMPLED
                                | ImageUsage::TRANSFER_DST,
                        )
                        .unwrap(),
                    )
                    .unwrap()
                })
                .collect();

            if target_info.msaa() > BstMSAALevel::One {
                for _ in 0..2 {
                    auxiliary_images.push(
                        BstImageView::from_attachment(
                            AttachmentImage::multisampled_with_usage(
                                &self.mem_alloc,
                                target_info.extent(),
                                target_info.msaa().as_vulkano(),
                                self.itf_format,
                                ImageUsage::TRANSIENT_ATTACHMENT,
                            )
                            .unwrap(),
                        )
                        .unwrap(),
                    );
                }
            }

            if !target.is_swapchain() {
                auxiliary_images.push(
                    BstImageView::from_attachment(
                        AttachmentImage::with_usage(
                            &self.mem_alloc,
                            target_info.extent(),
                            target.format(self.itf_format),
                            ImageUsage::TRANSFER_SRC
                                | ImageUsage::COLOR_ATTACHMENT
                                | ImageUsage::SAMPLED,
                        )
                        .unwrap(),
                    )
                    .unwrap(),
                );
            }

            let layer_renderpass = if target_info.msaa() > BstMSAALevel::One {
                single_pass_renderpass!(
                    self.device.clone(),
                    attachments: {
                        color: {
                            load: DontCare,
                            store: Store,
                            format: self.itf_format,
                            samples: 1,
                        },
                        alpha: {
                            load: DontCare,
                            store: Store,
                            format: self.itf_format,
                            samples: 1,
                        },
                        color_ms: {
                            load: DontCare,
                            store: DontCare,
                            format: self.itf_format,
                            samples: target_info.msaa().as_vulkano(),
                        },
                        alpha_ms: {
                            load: DontCare,
                            store: DontCare,
                            format: self.itf_format,
                            samples: target_info.msaa().as_vulkano(),
                        }
                    },
                    pass: {
                        color: [color_ms, alpha_ms],
                        depth_stencil: {},
                        resolve: [color, alpha],
                    }
                )
                .unwrap()
            } else {
                single_pass_renderpass!(
                    self.device.clone(),
                    attachments: {
                        color: {
                            load: DontCare,
                            store: Store,
                            format: self.itf_format,
                            samples: 1,
                        },
                        alpha: {
                            load: DontCare,
                            store: Store,
                            format: self.itf_format,
                            samples: 1,
                        }
                    },
                    pass: {
                        color: [color, alpha],
                        depth_stencil: {}
                    }
                )
                .unwrap()
            };

            let final_renderpass = single_pass_renderpass!(
                self.device.clone(),
                attachments: {
                    color: {
                        load: DontCare,
                        store: Store,
                        format: target.format(self.itf_format),
                        samples: 1,
                    }
                },
                pass: {
                    color: [color],
                    depth_stencil: {}
                }
            )
            .unwrap();

            let extent = target_info.extent();

            let layer_pipeline = GraphicsPipeline::start()
                .vertex_input_state(ItfVertInfo::per_vertex())
                .vertex_shader(self.layer_vs.entry_point("main").unwrap(), ())
                .input_assembly_state(
                    InputAssemblyState::new().topology(PrimitiveTopology::TriangleList),
                )
                .viewport_state(ViewportState::viewport_fixed_scissor_irrelevant(
                    iter::once(Viewport {
                        origin: [0.0; 2],
                        dimensions: [extent[0] as f32, extent[1] as f32],
                        depth_range: 0.0..1.0,
                    }),
                ))
                .fragment_shader(self.layer_fs.entry_point("main").unwrap(), ())
                .depth_stencil_state(DepthStencilState::disabled())
                .render_pass(Subpass::from(layer_renderpass.clone(), 0).unwrap())
                .rasterization_state(
                    RasterizationState::new()
                        .polygon_mode(PolygonMode::Fill)
                        .cull_mode(CullMode::None),
                )
                .multisample_state(MultisampleState {
                    rasterization_samples: target_info.msaa().as_vulkano(),
                    ..MultisampleState::new()
                })
                .build_with_cache(self.pipeline_cache.clone())
                .with_auto_layout(self.device.clone(), |set_descs| {
                    set_descs[0]
                        .bindings
                        .get_mut(&0)
                        .unwrap()
                        .immutable_samplers = vec![self.image_sampler.clone()];
                    set_descs[0]
                        .bindings
                        .get_mut(&1)
                        .unwrap()
                        .immutable_samplers = vec![self.image_sampler.clone()];
                    set_descs[0]
                        .bindings
                        .get_mut(&2)
                        .unwrap()
                        .variable_descriptor_count = true;
                    set_descs[0].bindings.get_mut(&2).unwrap().descriptor_count =
                        image_capacity as u32;
                })
                .unwrap();

            let final_pipeline = GraphicsPipeline::start()
                .vertex_input_state(SquareShaderVertex::per_vertex())
                .vertex_shader(self.square_vs.entry_point("main").unwrap(), ())
                .input_assembly_state(
                    InputAssemblyState::new().topology(PrimitiveTopology::TriangleList),
                )
                .viewport_state(ViewportState::viewport_fixed_scissor_irrelevant(
                    iter::once(Viewport {
                        origin: [0.0; 2],
                        dimensions: [extent[0] as f32, extent[1] as f32],
                        depth_range: 0.0..1.0,
                    }),
                ))
                .fragment_shader(self.final_fs.entry_point("main").unwrap(), ())
                .depth_stencil_state(DepthStencilState::disabled())
                .render_pass(Subpass::from(final_renderpass.clone(), 0).unwrap())
                .rasterization_state(
                    RasterizationState::new()
                        .polygon_mode(PolygonMode::Fill)
                        .cull_mode(CullMode::None),
                )
                .build_with_cache(self.pipeline_cache.clone())
                .build(self.device.clone())
                .unwrap();

            let (e_layer_fb, o_layer_fb, layer_clear_values) =
                if target_info.msaa() > BstMSAALevel::One {
                    (
                        Framebuffer::new(
                            layer_renderpass.clone(),
                            FramebufferCreateInfo {
                                attachments: vec![
                                    auxiliary_images[0].clone(),
                                    auxiliary_images[1].clone(),
                                    auxiliary_images[4].clone(),
                                    auxiliary_images[5].clone(),
                                ],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap(),
                        Framebuffer::new(
                            layer_renderpass.clone(),
                            FramebufferCreateInfo {
                                attachments: vec![
                                    auxiliary_images[2].clone(),
                                    auxiliary_images[3].clone(),
                                    auxiliary_images[4].clone(),
                                    auxiliary_images[5].clone(),
                                ],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap(),
                        vec![None, None, None, None],
                    )
                } else {
                    (
                        Framebuffer::new(
                            layer_renderpass.clone(),
                            FramebufferCreateInfo {
                                attachments: vec![
                                    auxiliary_images[0].clone(),
                                    auxiliary_images[1].clone(),
                                ],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap(),
                        Framebuffer::new(
                            layer_renderpass.clone(),
                            FramebufferCreateInfo {
                                attachments: vec![
                                    auxiliary_images[2].clone(),
                                    auxiliary_images[3].clone(),
                                ],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap(),
                        vec![None, None],
                    )
                };

            let mut final_fbs = Vec::new();

            for i in 0..target_info.num_images() {
                if target.is_swapchain() {
                    final_fbs.push(
                        Framebuffer::new(
                            final_renderpass.clone(),
                            FramebufferCreateInfo {
                                attachments: vec![target.swapchain_image(i)],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap(),
                    );
                } else if target_info.msaa() > BstMSAALevel::One {
                    final_fbs.push(
                        Framebuffer::new(
                            final_renderpass.clone(),
                            FramebufferCreateInfo {
                                attachments: vec![auxiliary_images[6].clone()],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap(),
                    );
                } else {
                    final_fbs.push(
                        Framebuffer::new(
                            final_renderpass.clone(),
                            FramebufferCreateInfo {
                                attachments: vec![auxiliary_images[4].clone()],
                                ..FramebufferCreateInfo::default()
                            },
                        )
                        .unwrap(),
                    );
                }
            }

            let layer_set_pool = LayerDescPool::new(
                self.device.clone(),
                layer_pipeline.layout().set_layouts()[0].clone(),
            );

            self.context = Some(Context {
                auxiliary_images,
                layer_renderpass,
                final_renderpass,
                layer_pipeline,
                final_pipeline,
                e_layer_fb,
                o_layer_fb,
                final_fbs,
                layer_set_pool,
                layer_clear_values,
                image_capacity,
                cons_draw_last_view: None,
            });
        }

        let context = self.context.as_mut().unwrap();

        if self.conservative_draw
            && !target.is_swapchain()
            && context.cons_draw_last_view.is_some()
            && *context.cons_draw_last_view.as_ref().unwrap() == view.inst
        {
            return if target_info.msaa() > BstMSAALevel::One {
                (cmd, context.auxiliary_images.get(6).cloned())
            } else {
                (cmd, context.auxiliary_images.get(4).cloned())
            };
        }

        let nearest_sampler = self.atlas.nearest_sampler();

        match target.source_image() {
            Some(source) => {
                cmd.copy_image(CopyImageInfo::images(
                    source,
                    context.auxiliary_images[2].clone(),
                ))
                .unwrap();

                cmd.clear_color_image(ClearColorImageInfo {
                    clear_value: ClearColorValue::Float([1.0; 4]),
                    ..ClearColorImageInfo::image(context.auxiliary_images[3].clone())
                })
                .unwrap();
            },
            None => {
                cmd.clear_color_image(ClearColorImageInfo {
                    clear_value: ClearColorValue::Float([0.0, 0.0, 0.0, 1.0]),
                    ..ClearColorImageInfo::image(context.auxiliary_images[2].clone())
                })
                .unwrap();

                cmd.clear_color_image(ClearColorImageInfo {
                    clear_value: ClearColorValue::Float([1.0; 4]),
                    ..ClearColorImageInfo::image(context.auxiliary_images[3].clone())
                })
                .unwrap();
            },
        }

        for i in 0..view.buffers.len() {
            let (prev_c, prev_a) = if i % 2 == 0 {
                cmd.begin_render_pass(
                    RenderPassBeginInfo {
                        clear_values: context.layer_clear_values.clone(),
                        ..RenderPassBeginInfo::framebuffer(context.e_layer_fb.clone())
                    },
                    SubpassContents::Inline,
                )
                .unwrap();

                (
                    context.auxiliary_images[2].clone(),
                    context.auxiliary_images[3].clone(),
                )
            } else {
                cmd.begin_render_pass(
                    RenderPassBeginInfo {
                        clear_values: context.layer_clear_values.clone(),
                        ..RenderPassBeginInfo::framebuffer(context.o_layer_fb.clone())
                    },
                    SubpassContents::Inline,
                )
                .unwrap();

                (
                    context.auxiliary_images[0].clone(),
                    context.auxiliary_images[1].clone(),
                )
            };

            let layer_set = PersistentDescriptorSet::new_variable(
                &context.layer_set_pool,
                context.layer_pipeline.layout().set_layouts()[0].clone(),
                context.image_capacity as u32,
                vec![
                    WriteDescriptorSet::image_view(0, prev_c.clone()),
                    WriteDescriptorSet::image_view(1, prev_a.clone()),
                    WriteDescriptorSet::image_view_sampler_array(
                        2,
                        0,
                        view.images
							.iter()
							.cloned()
							// VUID-vkCmdDraw-None-02699
							.chain((0..(context.image_capacity - view.images.len())).map(|_| self.empty_image.clone()))
							.map(|image| (image as Arc<_>, nearest_sampler.clone())),
                    ),
                ]
                .into_iter(),
            )
            .unwrap();

            cmd.bind_pipeline_graphics(context.layer_pipeline.clone())
                .bind_descriptor_sets(
                    PipelineBindPoint::Graphics,
                    context.layer_pipeline.layout().clone(),
                    0,
                    layer_set,
                )
                .bind_vertex_buffers(0, view.buffers[i].clone())
                .draw((view.buffers[i].size() / ITF_VERTEX_SIZE) as u32, 1, 0, 0)
                .unwrap()
                .end_render_pass()
                .unwrap();
        }

        cmd.begin_render_pass(
            RenderPassBeginInfo {
                clear_values: vec![None],
                ..RenderPassBeginInfo::framebuffer(context.final_fbs[target.image_num()].clone())
            },
            SubpassContents::Inline,
        )
        .unwrap();

        let final_i = view.buffers.len();
        let (prev_c, prev_a) = if final_i % 2 == 0 {
            (
                context.auxiliary_images[2].clone(),
                context.auxiliary_images[3].clone(),
            )
        } else {
            (
                context.auxiliary_images[0].clone(),
                context.auxiliary_images[1].clone(),
            )
        };

        let final_set = PersistentDescriptorSet::new(
            &self.set_alloc,
            context.final_pipeline.layout().set_layouts()[0].clone(),
            vec![
                WriteDescriptorSet::image_view_sampler(0, prev_c, self.image_sampler.clone()),
                WriteDescriptorSet::image_view_sampler(1, prev_a, self.image_sampler.clone()),
            ]
            .into_iter(),
        )
        .unwrap();

        cmd.bind_pipeline_graphics(context.final_pipeline.clone())
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                context.final_pipeline.layout().clone(),
                0,
                final_set,
            )
            .bind_vertex_buffers(0, self.final_vert_buf.as_ref().unwrap().clone())
            .draw(6, 1, 0, 0)
            .unwrap()
            .end_render_pass()
            .unwrap();

        let output_image = if target.is_swapchain() {
            None
        } else if target_info.msaa() > BstMSAALevel::One {
            context.auxiliary_images.get(6).cloned()
        } else {
            context.auxiliary_images.get(4).cloned()
        };

        (cmd, output_image)
    }
}
