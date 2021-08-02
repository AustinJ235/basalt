use crate::image_view::BstImageView;
use crate::interface::render::composer::ComposerView;
use crate::interface::render::final_fs::final_fs;
use crate::interface::render::layer_fs::layer_fs;
use crate::interface::render::layer_vs::layer_vs;
use crate::interface::render::square_vs::square_vs;
use crate::interface::render::{ItfRenderTarget, ItfRenderTargetInfo};
use crate::interface::ItfVertInfo;
use crate::Basalt;
use std::iter;
use std::sync::Arc;
use std::time::Instant;
use vulkano::buffer::immutable::ImmutableBuffer;
use vulkano::buffer::BufferUsage;
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, DynamicState, PrimaryAutoCommandBuffer, SubpassContents,
};
use vulkano::descriptor::descriptor_set::FixedSizeDescriptorSetsPool;
use vulkano::format::ClearValue;
use vulkano::image::attachment::AttachmentImage;
use vulkano::image::{ImageLayout, ImageUsage, SampleCount};
use vulkano::pipeline::vertex::SingleBufferDefinition;
use vulkano::pipeline::viewport::Viewport;
use vulkano::pipeline::{GraphicsPipeline, GraphicsPipelineAbstract};
use vulkano::render_pass::{
	AttachmentDesc, Framebuffer, FramebufferAbstract, LoadOp, RenderPass, RenderPassDesc,
	StoreOp, Subpass, SubpassDependencyDesc, SubpassDesc,
};
use vulkano::sync::{AccessFlags, PipelineStages};
use vulkano::pipeline::cache::PipelineCache;

pub(super) struct BstRasterPipeline {
	bst: Arc<Basalt>,
	context: Option<Context>,
	layer_vs: layer_vs::Shader,
	layer_fs: layer_fs::Shader,
	square_vs: square_vs::Shader,
	final_fs: final_fs::Shader,
	final_vert_buf: Arc<ImmutableBuffer<[SquareShaderVertex]>>,
	pipeline_cache: Arc<PipelineCache>,
}

struct Context {
	inst: Instant,
	auxiliary_images: Vec<Arc<BstImageView>>,
	#[allow(dead_code)]
	renderpass: Arc<RenderPass>,
	framebuffers: Vec<Arc<dyn FramebufferAbstract + Send + Sync>>,
	pipelines: Vec<Arc<dyn GraphicsPipelineAbstract + Send + Sync>>,
	layer_set_pool: FixedSizeDescriptorSetsPool,
	final_set_pool: FixedSizeDescriptorSetsPool,
	dynamic_state: DynamicState,
}

#[derive(Default, Debug, Clone)]
struct SquareShaderVertex {
	pub position: [f32; 2],
}

vulkano::impl_vertex!(SquareShaderVertex, position);

impl BstRasterPipeline {
	pub fn new(bst: Arc<Basalt>) -> Self {
		Self {
			context: None,
			layer_vs: layer_vs::Shader::load(bst.device()).unwrap(),
			layer_fs: layer_fs::Shader::load(bst.device()).unwrap(),
			square_vs: square_vs::Shader::load(bst.device()).unwrap(),
			final_fs: final_fs::Shader::load(bst.device()).unwrap(),
			final_vert_buf: ImmutableBuffer::from_iter(
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
				]
				.into_iter(),
				BufferUsage {
					vertex_buffer: true,
					..BufferUsage::none()
				},
				bst.transfer_queue(),
			)
			.unwrap()
			.0,
			pipeline_cache: PipelineCache::empty(bst.device()).unwrap(),
			bst,
		}
	}

	pub fn draw<S: Send + Sync + 'static>(
		&mut self,
		recreate_pipeline: bool,
		view: &ComposerView,
		target: ItfRenderTarget<S>,
		target_info: &ItfRenderTargetInfo,
		mut cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, Option<Arc<BstImageView>>) {
		if recreate_pipeline
			|| self.context.is_none()
			|| self.context.as_ref().unwrap().inst < view.inst
		{
			let mut auxiliary_images: Vec<Arc<BstImageView>> = (0..4)
				.into_iter()
				.map(|_| {
					BstImageView::from_attachment(
						AttachmentImage::with_usage(
							self.bst.device(),
							target_info.extent(),
							self.bst.formats_in_use().interface,
							ImageUsage {
								color_attachment: true,
								input_attachment: true,
								..vulkano::image::ImageUsage::none()
							},
						)
						.unwrap(),
					)
					.unwrap()
				})
				.collect();

			if !target.is_swapchain() {
				auxiliary_images.push(
					BstImageView::from_attachment(
						AttachmentImage::with_usage(
							self.bst.device(),
							target_info.extent(),
							target.format(),
							ImageUsage {
								transfer_source: true,
								color_attachment: true,
								sampled: true,
								..vulkano::image::ImageUsage::none()
							},
						)
						.unwrap(),
					)
					.unwrap(),
				);
			}

			let mut attachment_desc: Vec<AttachmentDesc> = (0..4)
				.into_iter()
				.map(|_| {
					AttachmentDesc {
						format: self.bst.formats_in_use().interface,
						samples: SampleCount::Sample1,
						load: LoadOp::Load,
						store: StoreOp::Store,
						stencil_load: LoadOp::DontCare,
						stencil_store: StoreOp::DontCare,
						initial_layout: ImageLayout::ColorAttachmentOptimal,
						final_layout: ImageLayout::ColorAttachmentOptimal,
					}
				})
				.collect();

			attachment_desc.push(AttachmentDesc {
				format: target.format(),
				samples: SampleCount::Sample1,
				load: LoadOp::Load,
				store: StoreOp::Store,
				stencil_load: LoadOp::DontCare,
				stencil_store: StoreOp::DontCare,
				initial_layout: ImageLayout::ColorAttachmentOptimal,
				final_layout: ImageLayout::ColorAttachmentOptimal,
			});

			let mut subpass_desc: Vec<SubpassDesc> =
				Vec::with_capacity(view.buffers_and_imgs.len());
			let mut subpass_dependency_desc: Vec<SubpassDependencyDesc> =
				Vec::with_capacity(view.buffers_and_imgs.len());

			for i in 0..view.buffers_and_imgs.len() {
				let (dst_c, dst_a, prev_c, prev_a) = if i % 2 == 0 {
					(0, 1, 2, 3)
				} else {
					(2, 3, 0, 1)
				};

				subpass_desc.push(SubpassDesc {
					color_attachments: vec![
						(dst_c, ImageLayout::ColorAttachmentOptimal),
						(dst_a, ImageLayout::ColorAttachmentOptimal),
					],
					depth_stencil: None,
					input_attachments: vec![
						(prev_c, ImageLayout::ColorAttachmentOptimal),
						(prev_a, ImageLayout::ColorAttachmentOptimal),
					],
					resolve_attachments: Vec::new(),
					preserve_attachments: Vec::new(), // TODO: What does this do?
				});

				subpass_dependency_desc.push(SubpassDependencyDesc {
					source_subpass: !0,
					destination_subpass: !0,
					source_stages: PipelineStages {
						color_attachment_output: true,
						..PipelineStages::none()
					},
					destination_stages: PipelineStages {
						fragment_shader: true,
						..PipelineStages::none()
					},
					source_access: AccessFlags {
						color_attachment_write: true,
						..AccessFlags::none()
					},
					destination_access: AccessFlags {
						color_attachment_read: true,
						..AccessFlags::none()
					},
					by_region: true,
				});
			}

			let final_i = subpass_desc.len();

			let (prev_c, prev_a) = if final_i % 2 == 0 {
				(2, 3)
			} else {
				(0, 1)
			};

			subpass_desc.push(SubpassDesc {
				color_attachments: vec![(4, ImageLayout::ColorAttachmentOptimal)],
				depth_stencil: None,
				input_attachments: vec![
					(prev_c, ImageLayout::ColorAttachmentOptimal),
					(prev_a, ImageLayout::ColorAttachmentOptimal),
				],
				resolve_attachments: Vec::new(),
				preserve_attachments: Vec::new(),
			});

			subpass_dependency_desc.push(SubpassDependencyDesc {
				source_subpass: !0,
				destination_subpass: !0,
				source_stages: PipelineStages {
					color_attachment_output: true,
					..PipelineStages::none()
				},
				destination_stages: PipelineStages {
					fragment_shader: true,
					..PipelineStages::none()
				},
				source_access: AccessFlags {
					color_attachment_write: true,
					..AccessFlags::none()
				},
				destination_access: AccessFlags {
					color_attachment_read: true,
					..AccessFlags::none()
				},
				by_region: true,
			});

			let renderpass = Arc::new(
				RenderPass::new(
					self.bst.device(),
					RenderPassDesc::new(attachment_desc, subpass_desc, subpass_dependency_desc),
				)
				.unwrap(),
			);

			let mut framebuffers = Vec::with_capacity(target_info.num_images());

			for i in 0..target_info.num_images() {
				let fb_builder = Framebuffer::start(renderpass.clone())
					.add(auxiliary_images[0].clone())
					.unwrap()
					.add(auxiliary_images[1].clone())
					.unwrap()
					.add(auxiliary_images[2].clone())
					.unwrap()
					.add(auxiliary_images[3].clone())
					.unwrap();

				let framebuffer = if target.is_swapchain() {
					Arc::new(
						fb_builder.add(target.swapchain_image(i)).unwrap().build().unwrap(),
					) as Arc<dyn FramebufferAbstract + Send + Sync>
				} else {
					Arc::new(
						fb_builder.add(auxiliary_images[4].clone()).unwrap().build().unwrap(),
					) as Arc<dyn FramebufferAbstract + Send + Sync>
				};

				framebuffers.push(framebuffer);
			}

			let mut pipelines = Vec::with_capacity(view.buffers_and_imgs.len() + 1);
			let layer_vert_input: Arc<SingleBufferDefinition<ItfVertInfo>> =
				Arc::new(SingleBufferDefinition::new());
			let square_vert_input: Arc<SingleBufferDefinition<SquareShaderVertex>> =
				Arc::new(SingleBufferDefinition::new());

			for i in 0..view.buffers_and_imgs.len() {
				pipelines.push(Arc::new(
					GraphicsPipeline::start()
						.vertex_input(layer_vert_input.clone())
						.vertex_shader(self.layer_vs.main_entry_point(), ())
						.triangle_list()
						.viewports_dynamic_scissors_irrelevant(1)
						.fragment_shader(
							self.layer_fs.main_entry_point(),
							layer_fs::SpecializationConstants {
								layer_i: i as u32,
							},
						)
						.depth_stencil_disabled()
						.render_pass(Subpass::from(renderpass.clone(), i as u32).unwrap())
						.polygon_mode_fill()
						.sample_shading_enabled(1.0)
						.build_with_cache(self.pipeline_cache.clone())
						.build(self.bst.device())
						.unwrap(),
				) as Arc<dyn GraphicsPipelineAbstract + Send + Sync>);
			}

			pipelines.push(Arc::new(
				GraphicsPipeline::start()
					.vertex_input(square_vert_input)
					.vertex_shader(self.square_vs.main_entry_point(), ())
					.triangle_list()
					.viewports_dynamic_scissors_irrelevant(1)
					.fragment_shader(self.final_fs.main_entry_point(), ())
					.depth_stencil_disabled()
					.render_pass(
						Subpass::from(renderpass.clone(), view.buffers_and_imgs.len() as u32)
							.unwrap(),
					)
					.polygon_mode_fill()
					.sample_shading_enabled(1.0)
					.build_with_cache(self.pipeline_cache.clone())
					.build(self.bst.device())
					.unwrap(),
			) as Arc<dyn GraphicsPipelineAbstract + Send + Sync>);

			let layer_set_pool = FixedSizeDescriptorSetsPool::new(
				pipelines[0].layout().descriptor_set_layout(0).unwrap().clone(), /* TODO: what happens if there are no layers? */
			);

			let final_set_pool = FixedSizeDescriptorSetsPool::new(
				pipelines[view.buffers_and_imgs.len()]
					.layout()
					.descriptor_set_layout(0)
					.unwrap()
					.clone(),
			);

			let extent = target_info.extent();
			let dynamic_state = DynamicState {
				viewports: Some(vec![Viewport {
					origin: [0.0; 2],
					dimensions: [extent[0] as f32, extent[1] as f32],
					depth_range: 0.0..1.0,
				}]),
				..DynamicState::none()
			};

			self.context = Some(Context {
				inst: view.inst.clone(),
				auxiliary_images,
				renderpass,
				framebuffers,
				pipelines,
				layer_set_pool,
				final_set_pool,
				dynamic_state,
			});
		}

		let context = self.context.as_mut().unwrap();
		let linear_sampler = self.bst.atlas_ref().linear_sampler();
		let nearest_sampler = self.bst.atlas_ref().nearest_sampler();

		cmd.begin_render_pass(
			context.framebuffers[target.image_num()].clone(),
			SubpassContents::Inline,
			vec![
				ClearValue::None,
				ClearValue::None,
				ClearValue::None,
				ClearValue::None,
				ClearValue::None,
			],
		)
		.unwrap();

		for i in 0..view.buffers_and_imgs.len() {
			let (prev_c, prev_a) = if i % 2 == 0 {
				(context.auxiliary_images[2].clone(), context.auxiliary_images[3].clone())
			} else {
				(context.auxiliary_images[0].clone(), context.auxiliary_images[1].clone())
			};

			for (buf, img) in view.buffers_and_imgs[i].iter() {
				let layer_set = context
					.layer_set_pool
					.next()
					.add_image(prev_c.clone())
					.unwrap()
					.add_image(prev_a.clone())
					.unwrap()
					.add_sampled_image(img.clone(), linear_sampler.clone())
					.unwrap()
					.add_sampled_image(img.clone(), nearest_sampler.clone())
					.unwrap()
					.build()
					.unwrap();

				cmd.draw(
					context.pipelines[i].clone(),
					&context.dynamic_state,
					vec![buf.clone()],
					layer_set,
					(),
					iter::empty(),
				)
				.unwrap();
			}

			cmd.next_subpass(SubpassContents::Inline).unwrap();
		}

		let final_i = view.buffers_and_imgs.len();
		let (prev_c, prev_a) = if final_i % 2 == 0 {
			(context.auxiliary_images[2].clone(), context.auxiliary_images[3].clone())
		} else {
			(context.auxiliary_images[0].clone(), context.auxiliary_images[1].clone())
		};

		let final_set = context
			.final_set_pool
			.next()
			.add_image(prev_c.clone())
			.unwrap()
			.add_image(prev_a.clone())
			.unwrap()
			.build()
			.unwrap();

		cmd.draw(
			context.pipelines[final_i].clone(),
			&context.dynamic_state,
			vec![self.final_vert_buf.clone()],
			final_set,
			(),
			iter::empty(),
		)
		.unwrap()
		.end_render_pass()
		.unwrap();

		(cmd, context.auxiliary_images.get(4).cloned())
	}
}
