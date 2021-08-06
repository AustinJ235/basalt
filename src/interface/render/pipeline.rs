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
use vulkano::buffer::immutable::ImmutableBuffer;
use vulkano::buffer::BufferUsage;
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, DynamicState, PrimaryAutoCommandBuffer, SubpassContents,
};
use vulkano::descriptor::descriptor_set::FixedSizeDescriptorSetsPool;
use vulkano::format::ClearValue;
use vulkano::image::attachment::AttachmentImage;
use vulkano::image::ImageUsage;
use vulkano::pipeline::cache::PipelineCache;
use vulkano::pipeline::vertex::SingleBufferDefinition;
use vulkano::pipeline::viewport::Viewport;
use vulkano::pipeline::{GraphicsPipeline, GraphicsPipelineAbstract};
use vulkano::render_pass::{Framebuffer, FramebufferAbstract, RenderPass, Subpass};
use vulkano::sampler::{Filter, MipmapMode, Sampler, SamplerAddressMode};

pub(super) struct BstRasterPipeline {
	bst: Arc<Basalt>,
	context: Option<Context>,
	layer_vs: layer_vs::Shader,
	layer_fs: layer_fs::Shader,
	square_vs: square_vs::Shader,
	final_fs: final_fs::Shader,
	final_vert_buf: Arc<ImmutableBuffer<[SquareShaderVertex]>>,
	pipeline_cache: Arc<PipelineCache>,
	image_sampler: Arc<Sampler>,
}

struct Context {
	auxiliary_images: Vec<Arc<BstImageView>>,
	#[allow(dead_code)]
	layer_renderpass: Arc<RenderPass>,
	#[allow(dead_code)]
	final_renderpass: Arc<RenderPass>,
	layer_pipeline: Arc<dyn GraphicsPipelineAbstract + Send + Sync>,
	final_pipeline: Arc<dyn GraphicsPipelineAbstract + Send + Sync>,
	e_layer_fb: Arc<dyn FramebufferAbstract + Send + Sync>,
	o_layer_fb: Arc<dyn FramebufferAbstract + Send + Sync>,
	final_fbs: Vec<Arc<dyn FramebufferAbstract + Send + Sync>>,
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
			image_sampler: Sampler::new(
				bst.device(),
				Filter::Nearest,
				Filter::Nearest,
				MipmapMode::Linear,
				SamplerAddressMode::Repeat,
				SamplerAddressMode::Repeat,
				SamplerAddressMode::Repeat,
				0.0,
				1.0,
				0.0,
				1.0,
			)
			.unwrap(),
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
		if recreate_pipeline || self.context.is_none() {
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
								sampled: true,
								transfer_destination: true,
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
							target.format(&self.bst),
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

			let layer_renderpass = Arc::new(
				single_pass_renderpass!(
					self.bst.device(),
					attachments: {
						color: {
							load: DontCare,
							store: Store,
							format: self.bst.formats_in_use().interface,
							samples: 1,
						},
						alpha: {
							load: DontCare,
							store: Store,
							format: self.bst.formats_in_use().interface,
							samples: 1,
						}
					},
					pass: {
						color: [color, alpha],
						depth_stencil: {}
					}
				)
				.unwrap(),
			);

			let final_renderpass = Arc::new(
				single_pass_renderpass!(
					self.bst.device(),
					attachments: {
						color: {
							load: DontCare,
							store: Store,
							format: target.format(&self.bst),
							samples: 1,
						}
					},
					pass: {
						color: [color],
						depth_stencil: {}
					}
				)
				.unwrap(),
			);

			let layer_vert_input: Arc<SingleBufferDefinition<ItfVertInfo>> =
				Arc::new(SingleBufferDefinition::new());
			let square_vert_input: Arc<SingleBufferDefinition<SquareShaderVertex>> =
				Arc::new(SingleBufferDefinition::new());

			let layer_pipeline = Arc::new(
				GraphicsPipeline::start()
					.vertex_input(layer_vert_input.clone())
					.vertex_shader(self.layer_vs.main_entry_point(), ())
					.triangle_list()
					.viewports_dynamic_scissors_irrelevant(1)
					.fragment_shader(self.layer_fs.main_entry_point(), ())
					.depth_stencil_disabled()
					.render_pass(Subpass::from(layer_renderpass.clone(), 0).unwrap())
					.polygon_mode_fill()
					.sample_shading_enabled(1.0)
					.build_with_cache(self.pipeline_cache.clone())
					.build(self.bst.device())
					.unwrap(),
			) as Arc<dyn GraphicsPipelineAbstract + Send + Sync>;

			let final_pipeline = Arc::new(
				GraphicsPipeline::start()
					.vertex_input(square_vert_input)
					.vertex_shader(self.square_vs.main_entry_point(), ())
					.triangle_list()
					.viewports_dynamic_scissors_irrelevant(1)
					.fragment_shader(self.final_fs.main_entry_point(), ())
					.depth_stencil_disabled()
					.render_pass(Subpass::from(final_renderpass.clone(), 0).unwrap())
					.polygon_mode_fill()
					.sample_shading_enabled(1.0)
					.build_with_cache(self.pipeline_cache.clone())
					.build(self.bst.device())
					.unwrap(),
			) as Arc<dyn GraphicsPipelineAbstract + Send + Sync>;

			let e_layer_fb = Arc::new(
				Framebuffer::start(layer_renderpass.clone())
					.add(auxiliary_images[0].clone())
					.unwrap()
					.add(auxiliary_images[1].clone())
					.unwrap()
					.build()
					.unwrap(),
			) as Arc<dyn FramebufferAbstract + Send + Sync>;

			let o_layer_fb = Arc::new(
				Framebuffer::start(layer_renderpass.clone())
					.add(auxiliary_images[2].clone())
					.unwrap()
					.add(auxiliary_images[3].clone())
					.unwrap()
					.build()
					.unwrap(),
			) as Arc<dyn FramebufferAbstract + Send + Sync>;

			let mut final_fbs = Vec::new();

			for i in 0..target_info.num_images() {
				if target.is_swapchain() {
					final_fbs.push(Arc::new(
						Framebuffer::start(final_renderpass.clone())
							.add(target.swapchain_image(i))
							.unwrap()
							.build()
							.unwrap(),
					) as Arc<dyn FramebufferAbstract + Send + Sync>);
				} else {
					final_fbs.push(Arc::new(
						Framebuffer::start(final_renderpass.clone())
							.add(auxiliary_images[4].clone())
							.unwrap()
							.build()
							.unwrap(),
					) as Arc<dyn FramebufferAbstract + Send + Sync>);
				}
			}

			let layer_set_pool = FixedSizeDescriptorSetsPool::new(
				layer_pipeline.layout().descriptor_set_layout(0).unwrap().clone(),
			);

			let final_set_pool = FixedSizeDescriptorSetsPool::new(
				final_pipeline.layout().descriptor_set_layout(0).unwrap().clone(),
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
				auxiliary_images,
				layer_renderpass,
				final_renderpass,
				layer_pipeline,
				final_pipeline,
				e_layer_fb,
				o_layer_fb,
				final_fbs,
				layer_set_pool,
				final_set_pool,
				dynamic_state,
			});
		}

		let context = self.context.as_mut().unwrap();
		let linear_sampler = self.bst.atlas_ref().linear_sampler();
		let nearest_sampler = self.bst.atlas_ref().nearest_sampler();

		cmd.clear_color_image(context.auxiliary_images[2].clone(), [0.0, 0.0, 0.0, 1.0].into())
			.unwrap();

		cmd.clear_color_image(context.auxiliary_images[3].clone(), [0.0, 0.0, 0.0, 1.0].into())
			.unwrap();

		for i in 0..view.buffers_and_imgs.len() {
			let (prev_c, prev_a) = if i % 2 == 0 {
				cmd.begin_render_pass(
					context.e_layer_fb.clone(),
					SubpassContents::Inline,
					vec![ClearValue::None, ClearValue::None],
				)
				.unwrap();

				(context.auxiliary_images[2].clone(), context.auxiliary_images[3].clone())
			} else {
				cmd.begin_render_pass(
					context.o_layer_fb.clone(),
					SubpassContents::Inline,
					vec![ClearValue::None, ClearValue::None],
				)
				.unwrap();

				(context.auxiliary_images[0].clone(), context.auxiliary_images[1].clone())
			};

			for (buf, img) in view.buffers_and_imgs[i].iter() {
				let layer_set = context
					.layer_set_pool
					.next()
					.add_sampled_image(prev_c.clone(), self.image_sampler.clone())
					.unwrap()
					.add_sampled_image(prev_a.clone(), self.image_sampler.clone())
					.unwrap()
					.add_sampled_image(img.clone(), linear_sampler.clone())
					.unwrap()
					.add_sampled_image(img.clone(), nearest_sampler.clone())
					.unwrap()
					.build()
					.unwrap();

				cmd.draw(
					context.layer_pipeline.clone(),
					&context.dynamic_state,
					vec![buf.clone()],
					layer_set,
					(),
					iter::empty(),
				)
				.unwrap();
			}

			cmd.end_render_pass().unwrap();
		}

		cmd.begin_render_pass(
			context.final_fbs[target.image_num()].clone(),
			SubpassContents::Inline,
			vec![ClearValue::None],
		)
		.unwrap();

		let final_i = view.buffers_and_imgs.len();
		let (prev_c, prev_a) = if final_i % 2 == 0 {
			(context.auxiliary_images[2].clone(), context.auxiliary_images[3].clone())
		} else {
			(context.auxiliary_images[0].clone(), context.auxiliary_images[1].clone())
		};

		let final_set = context
			.final_set_pool
			.next()
			.add_sampled_image(prev_c.clone(), self.image_sampler.clone())
			.unwrap()
			.add_sampled_image(prev_a.clone(), self.image_sampler.clone())
			.unwrap()
			.build()
			.unwrap();

		cmd.draw(
			context.final_pipeline.clone(),
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
