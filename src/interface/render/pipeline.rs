use crate::image_view::BstImageView;
use crate::interface::render::composer::ComposerView;
use crate::interface::render::final_fs::final_fs;
use crate::interface::render::layer_desc_pool::LayerDescPool;
use crate::interface::render::layer_fs::layer_fs;
use crate::interface::render::layer_vs::layer_vs;
use crate::interface::render::square_vs::square_vs;
use crate::interface::render::{ItfDrawTarget, ItfDrawTargetInfo};
use crate::interface::ItfVertInfo;
use crate::vulkano::buffer::BufferAccess;
use crate::{Basalt, BstMSAALevel};
use std::iter;
use std::sync::Arc;
use vulkano::buffer::immutable::ImmutableBuffer;
use vulkano::buffer::BufferUsage;
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, PrimaryAutoCommandBuffer, SubpassContents,
};
use vulkano::descriptor_set::persistent::PersistentDescriptorSet;
use vulkano::descriptor_set::SingleLayoutDescSetPool;
use vulkano::format::ClearValue;
use vulkano::image::attachment::AttachmentImage;
use vulkano::image::ImageUsage;
use vulkano::pipeline::cache::PipelineCache;
use vulkano::pipeline::graphics::depth_stencil::DepthStencilState;
use vulkano::pipeline::graphics::input_assembly::{InputAssemblyState, PrimitiveTopology};
use vulkano::pipeline::graphics::rasterization::{CullMode, PolygonMode, RasterizationState};
use vulkano::pipeline::graphics::vertex_input::BuffersDefinition;
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::{GraphicsPipeline, Pipeline, PipelineBindPoint};
use vulkano::render_pass::{Framebuffer, RenderPass, Subpass};
use vulkano::sampler::{Filter, MipmapMode, Sampler, SamplerAddressMode};
use vulkano::shader::ShaderModule;
use vulkano::DeviceSize;

const ITF_VERTEX_SIZE: DeviceSize = std::mem::size_of::<ItfVertInfo>() as DeviceSize;

pub(super) struct ItfPipeline {
	bst: Arc<Basalt>,
	context: Option<Context>,
	layer_vs: Arc<ShaderModule>,
	layer_fs: Arc<ShaderModule>,
	square_vs: Arc<ShaderModule>,
	final_fs: Arc<ShaderModule>,
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
	layer_pipeline: Arc<GraphicsPipeline>,
	final_pipeline: Arc<GraphicsPipeline>,
	e_layer_fb: Arc<Framebuffer>,
	o_layer_fb: Arc<Framebuffer>,
	final_fbs: Vec<Arc<Framebuffer>>,
	layer_set_pool: LayerDescPool,
	final_set_pool: SingleLayoutDescSetPool,
	layer_clear_values: Vec<ClearValue>,
	image_capacity: usize,
}

#[derive(Default, Debug, Clone)]
struct SquareShaderVertex {
	pub position: [f32; 2],
}

vulkano::impl_vertex!(SquareShaderVertex, position);

impl ItfPipeline {
	pub fn new(bst: Arc<Basalt>) -> Self {
		Self {
			context: None,
			layer_vs: layer_vs::load(bst.device()).unwrap(),
			layer_fs: layer_fs::load(bst.device()).unwrap(),
			square_vs: square_vs::load(bst.device()).unwrap(),
			final_fs: final_fs::load(bst.device()).unwrap(),
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
		target: ItfDrawTarget<S>,
		target_info: &ItfDrawTargetInfo,
		mut cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, Option<Arc<BstImageView>>) {
		if recreate_pipeline
			|| self.context.is_none()
			|| (self.context.is_some()
				&& view.images.len() > self.context.as_ref().unwrap().image_capacity)
		{
			let mut image_capacity =
				self.context.as_ref().map(|c| c.image_capacity).unwrap_or(2);

			while image_capacity < view.images.len() {
				image_capacity *= 2;
			}

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

			if target_info.msaa() > BstMSAALevel::One {
				for _ in 0..2 {
					auxiliary_images.push(
						BstImageView::from_attachment(
							AttachmentImage::multisampled_with_usage(
								self.bst.device(),
								target_info.extent(),
								target_info.msaa().as_vulkano(),
								self.bst.formats_in_use().interface,
								ImageUsage {
									transient_attachment: true,
									..vulkano::image::ImageUsage::none()
								},
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

			let layer_renderpass = if target_info.msaa() > BstMSAALevel::One {
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
						},
						color_ms: {
							load: DontCare,
							store: DontCare,
							format: self.bst.formats_in_use().interface,
							samples: target_info.msaa().as_vulkano(),
						},
						alpha_ms: {
							load: DontCare,
							store: DontCare,
							format: self.bst.formats_in_use().interface,
							samples: target_info.msaa().as_vulkano(),
						}
					},
					pass: {
						color: [color_ms, alpha_ms],
						depth_stencil: {}
						resolve: [color, alpha],
					}
				)
				.unwrap()
			} else {
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
				.unwrap()
			};

			let final_renderpass = single_pass_renderpass!(
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
			.unwrap();

			let extent = target_info.extent();

			let layer_pipeline = GraphicsPipeline::start()
				.vertex_input_state(BuffersDefinition::new().vertex::<ItfVertInfo>())
				.vertex_shader(self.layer_vs.entry_point("main").unwrap(), ())
				.input_assembly_state(
					InputAssemblyState::new().topology(PrimitiveTopology::TriangleList),
				)
				.viewport_state(ViewportState::viewport_fixed_scissor_irrelevant(iter::once(
					Viewport {
						origin: [0.0; 2],
						dimensions: [extent[0] as f32, extent[1] as f32],
						depth_range: 0.0..1.0,
					},
				)))
				.fragment_shader(self.layer_fs.entry_point("main").unwrap(), ())
				.depth_stencil_state(DepthStencilState::disabled())
				.render_pass(Subpass::from(layer_renderpass.clone(), 0).unwrap())
				.rasterization_state(
					RasterizationState::new()
						.polygon_mode(PolygonMode::Fill)
						.cull_mode(CullMode::None),
				)
				.build_with_cache(self.pipeline_cache.clone())
				.with_auto_layout(self.bst.device(), |set_descs| {
					set_descs[0].set_immutable_samplers(0, [self.image_sampler.clone()]);
					set_descs[0].set_immutable_samplers(1, [self.image_sampler.clone()]);
					set_descs[0].set_variable_descriptor_count(2, image_capacity as u32);
				})
				.unwrap();

			let final_pipeline = GraphicsPipeline::start()
				.vertex_input_state(BuffersDefinition::new().vertex::<SquareShaderVertex>())
				.vertex_shader(self.square_vs.entry_point("main").unwrap(), ())
				.input_assembly_state(
					InputAssemblyState::new().topology(PrimitiveTopology::TriangleList),
				)
				.viewport_state(ViewportState::viewport_fixed_scissor_irrelevant(iter::once(
					Viewport {
						origin: [0.0; 2],
						dimensions: [extent[0] as f32, extent[1] as f32],
						depth_range: 0.0..1.0,
					},
				)))
				.fragment_shader(self.final_fs.entry_point("main").unwrap(), ())
				.depth_stencil_state(DepthStencilState::disabled())
				.render_pass(Subpass::from(final_renderpass.clone(), 0).unwrap())
				.rasterization_state(
					RasterizationState::new()
						.polygon_mode(PolygonMode::Fill)
						.cull_mode(CullMode::None),
				)
				.build_with_cache(self.pipeline_cache.clone())
				.build(self.bst.device())
				.unwrap();

			let (e_layer_fb, o_layer_fb, layer_clear_values) =
				if target_info.msaa() > BstMSAALevel::One {
					(
						Framebuffer::start(layer_renderpass.clone())
							.add(auxiliary_images[0].clone())
							.unwrap()
							.add(auxiliary_images[1].clone())
							.unwrap()
							.add(auxiliary_images[4].clone())
							.unwrap()
							.add(auxiliary_images[5].clone())
							.unwrap()
							.build()
							.unwrap(),
						Framebuffer::start(layer_renderpass.clone())
							.add(auxiliary_images[2].clone())
							.unwrap()
							.add(auxiliary_images[3].clone())
							.unwrap()
							.add(auxiliary_images[4].clone())
							.unwrap()
							.add(auxiliary_images[5].clone())
							.unwrap()
							.build()
							.unwrap(),
						vec![ClearValue::None; 4],
					)
				} else {
					(
						Framebuffer::start(layer_renderpass.clone())
							.add(auxiliary_images[0].clone())
							.unwrap()
							.add(auxiliary_images[1].clone())
							.unwrap()
							.build()
							.unwrap(),
						Framebuffer::start(layer_renderpass.clone())
							.add(auxiliary_images[2].clone())
							.unwrap()
							.add(auxiliary_images[3].clone())
							.unwrap()
							.build()
							.unwrap(),
						vec![ClearValue::None; 2],
					)
				};

			let mut final_fbs = Vec::new();

			for i in 0..target_info.num_images() {
				if target.is_swapchain() {
					final_fbs.push(
						Framebuffer::start(final_renderpass.clone())
							.add(target.swapchain_image(i))
							.unwrap()
							.build()
							.unwrap(),
					);
				} else {
					final_fbs.push(
						Framebuffer::start(final_renderpass.clone())
							.add(auxiliary_images[4].clone())
							.unwrap()
							.build()
							.unwrap(),
					);
				}
			}

			let final_set_pool = SingleLayoutDescSetPool::new(
				final_pipeline.layout().descriptor_set_layouts()[0].clone(),
			);

			let layer_set_pool = LayerDescPool::new(
				self.bst.device(),
				layer_pipeline.layout().descriptor_set_layouts()[0].clone(),
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
				final_set_pool,
				layer_set_pool,
				layer_clear_values,
				image_capacity,
			});
		}

		let context = self.context.as_mut().unwrap();
		let nearest_sampler = self.bst.atlas_ref().nearest_sampler();

		match target.source_image() {
			Some(source) => {
				let extent = target_info.extent();

				cmd.copy_image(
					source,
					[0, 0, 0],
					0,
					0,
					context.auxiliary_images[2].clone(),
					[0, 0, 0],
					0,
					0,
					[extent[0], extent[1], 1],
					1,
				)
				.unwrap();

				cmd.clear_color_image(
					context.auxiliary_images[3].clone(),
					[1.0, 1.0, 1.0, 1.0].into(),
				)
				.unwrap();
			},
			None => {
				cmd.clear_color_image(
					context.auxiliary_images[2].clone(),
					[0.0, 0.0, 0.0, 1.0].into(),
				)
				.unwrap();

				cmd.clear_color_image(
					context.auxiliary_images[3].clone(),
					[1.0, 1.0, 1.0, 1.0].into(),
				)
				.unwrap();
			},
		}

		for i in 0..view.buffers.len() {
			let (prev_c, prev_a) = if i % 2 == 0 {
				cmd.begin_render_pass(
					context.e_layer_fb.clone(),
					SubpassContents::Inline,
					context.layer_clear_values.clone(),
				)
				.unwrap();

				(context.auxiliary_images[2].clone(), context.auxiliary_images[3].clone())
			} else {
				cmd.begin_render_pass(
					context.o_layer_fb.clone(),
					SubpassContents::Inline,
					context.layer_clear_values.clone(),
				)
				.unwrap();

				(context.auxiliary_images[0].clone(), context.auxiliary_images[1].clone())
			};

			let mut layer_set_builder = PersistentDescriptorSet::start(
				context.layer_pipeline.layout().descriptor_set_layouts()[0].clone(),
			);

			layer_set_builder
				.add_image(prev_c.clone())
				.unwrap()
				.add_image(prev_a.clone())
				.unwrap()
				.enter_array()
				.unwrap();

			assert!(!view.images.is_empty());

			for image in &view.images {
				layer_set_builder
					.add_sampled_image(image.clone(), nearest_sampler.clone())
					.unwrap();
			}

			layer_set_builder.leave_array().unwrap();
			let layer_set =
				layer_set_builder.build_with_pool(&mut context.layer_set_pool).unwrap();

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
			context.final_fbs[target.image_num()].clone(),
			SubpassContents::Inline,
			vec![ClearValue::None],
		)
		.unwrap();

		let final_i = view.buffers.len();
		let (prev_c, prev_a) = if final_i % 2 == 0 {
			(context.auxiliary_images[2].clone(), context.auxiliary_images[3].clone())
		} else {
			(context.auxiliary_images[0].clone(), context.auxiliary_images[1].clone())
		};

		let mut final_set_builder = context.final_set_pool.next();

		final_set_builder
			.add_sampled_image(prev_c.clone(), self.image_sampler.clone())
			.unwrap()
			.add_sampled_image(prev_a.clone(), self.image_sampler.clone())
			.unwrap();

		let final_set = final_set_builder.build().unwrap();

		cmd.bind_pipeline_graphics(context.final_pipeline.clone())
			.bind_descriptor_sets(
				PipelineBindPoint::Graphics,
				context.final_pipeline.layout().clone(),
				0,
				final_set,
			)
			.bind_vertex_buffers(0, self.final_vert_buf.clone())
			.draw(6, 1, 0, 0)
			.unwrap()
			.end_render_pass()
			.unwrap();

		let output_image = if target.is_swapchain() {
			None
		} else {
			if target_info.msaa() > BstMSAALevel::One {
				context.auxiliary_images.get(6).cloned()
			} else {
				context.auxiliary_images.get(4).cloned()
			}
		};

		(cmd, output_image)
	}
}
