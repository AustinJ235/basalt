use crate::image_view::BstImageView;
use crate::interface::interface::ItfVertInfo;
use crate::{shaders, Basalt, BstEvent, BstItfEv, BstMSAALevel};
use parking_lot::Mutex;
use std::sync::Arc;
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, DynamicState, PrimaryAutoCommandBuffer, SubpassContents,
};
use vulkano::descriptor::descriptor_set::FixedSizeDescriptorSetsPool;
use vulkano::format::{ClearValue, Format as VkFormat};
use vulkano::image::attachment::AttachmentImage;
use vulkano::image::swapchain::SwapchainImage;
use vulkano::image::view::{ImageView, ImageViewAbstract};
use vulkano::image::ImageUsage;
use vulkano::pipeline::vertex::SingleBufferDefinition;
use vulkano::pipeline::viewport::Viewport;
use vulkano::pipeline::{GraphicsPipeline, GraphicsPipelineAbstract};
use vulkano::render_pass::{Framebuffer, FramebufferAbstract, RenderPass, Subpass};
use vulkano::swapchain::CompositeAlpha;

#[allow(dead_code)]
struct RenderContext {
	target_op: Option<Arc<BstImageView>>,
	target_ms_op: Option<Arc<BstImageView>>,
	renderpass: Arc<RenderPass>,
	framebuffer: Vec<Arc<dyn FramebufferAbstract + Send + Sync>>,
	pipeline: Arc<dyn GraphicsPipelineAbstract + Send + Sync>,
	set_pool: FixedSizeDescriptorSetsPool,
	clear_values: Vec<ClearValue>,
}

pub struct ItfRenderer {
	basalt: Arc<Basalt>,
	rc_op: Option<RenderContext>,
	shader_vs: shaders::interface_vs::Shader,
	shader_fs: shaders::interface_fs::Shader,
	msaa: Mutex<BstMSAALevel>,
	scale: Mutex<f32>,
	dynamic_state: DynamicState,
}

impl ItfRenderer {
	pub fn new(basalt: Arc<Basalt>) -> Self {
		let shader_vs = shaders::interface_vs::Shader::load(basalt.device.clone()).unwrap();
		let shader_fs = shaders::interface_fs::Shader::load(basalt.device.clone()).unwrap();

		ItfRenderer {
			rc_op: None,
			msaa: Mutex::new(basalt.options_ref().msaa),
			scale: Mutex::new(basalt.options_ref().scale),
			dynamic_state: DynamicState::none(),
			basalt,
			shader_vs,
			shader_fs,
		}
	}

	/// Command buffer used must not be in the middle of a render pass. Resize is to be set to
	/// true anytime the swapchain is recreated. Render to swapchain option will render the
	/// ui directly onto the swapchain images. If this is not set this function will return
	/// ImageViewAccess to the rendered image of the interface.
	pub fn draw<S: Send + Sync + 'static>(
		&mut self,
		mut cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
		win_size: [u32; 2],
		mut resize: bool,
		swap_imgs: &Vec<Arc<ImageView<Arc<SwapchainImage<S>>>>>,
		render_to_swapchain: bool,
		image_num: usize,
	) -> (AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>, Option<Arc<BstImageView>>) {
		let mut msaa_level = self.msaa.lock();
		let mut scale = self.scale.lock();
		let mut recreate_rc = resize;

		self.basalt.poll_events_internal(|ev| {
			match ev {
				BstEvent::BstItfEv(itf_ev) =>
					match itf_ev {
						BstItfEv::ScaleChanged => {
							*scale = self.basalt.interface_ref().scale();
							resize = true;
							false
						},
						BstItfEv::MSAAChanged => {
							*msaa_level = self.basalt.interface_ref().msaa();
							recreate_rc = true;
							false
						},
						_ => true,
					},
				_ => true,
			}
		});

		if self.rc_op.is_none() || recreate_rc {
			let color_format = if render_to_swapchain {
				swap_imgs[0].image().format()
			} else {
				VkFormat::R8G8B8A8Srgb
			};

			self.dynamic_state.viewports = Some(vec![Viewport {
				origin: [0.0; 2],
				dimensions: [win_size[0] as f32, win_size[1] as f32],
				depth_range: 0.0..1.0,
			}]);

			let target_op = if !render_to_swapchain {
				Some(
					BstImageView::from_attachment(
						AttachmentImage::with_usage(
							self.basalt.device(),
							win_size,
							color_format,
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
				)
			} else {
				None
			};

			let target_ms_op = if *msaa_level != BstMSAALevel::One {
				Some(
					BstImageView::from_attachment(
						AttachmentImage::multisampled_with_usage(
							self.basalt.device(),
							win_size,
							msaa_level.as_vulkano(),
							color_format,
							ImageUsage {
								color_attachment: true,
								transient_attachment: true,
								..vulkano::image::ImageUsage::none()
							},
						)
						.unwrap(),
					)
					.unwrap(),
				)
			} else {
				None
			};

			let renderpass = if *msaa_level == BstMSAALevel::One {
				Arc::new(
					single_pass_renderpass!(self.basalt.device(),
						attachments: {
							image: {
								load: Clear,
								store: Store,
								format: color_format,
								samples: 1,
							}
						}, pass: {
							color: [image],
							depth_stencil: {},
							resolve: []
						}
					)
					.unwrap(),
				)
			} else {
				Arc::new(
					single_pass_renderpass!(self.basalt.device(),
						attachments: {
							image_ms: {
								load: Clear,
								store: DontCare,
								format: color_format,
								samples: msaa_level.as_u32(),
							}, image: {
								load: DontCare,
								store: Store,
								format: color_format,
								samples: 1,
							}
						}, pass: {
							color: [image_ms],
							depth_stencil: {},
							resolve: [image]
						}
					)
					.unwrap(),
				)
			};

			let framebuffer = swap_imgs
				.iter()
				.map(|image| {
					if render_to_swapchain {
						if *msaa_level != BstMSAALevel::One {
							Arc::new(
								Framebuffer::start(renderpass.clone())
									.add(target_ms_op.as_ref().unwrap().clone())
									.unwrap()
									.add(image.clone())
									.unwrap()
									.build()
									.unwrap(),
							) as Arc<dyn FramebufferAbstract + Send + Sync>
						} else {
							Arc::new(
								Framebuffer::start(renderpass.clone())
									.add(image.clone())
									.unwrap()
									.build()
									.unwrap(),
							) as Arc<dyn FramebufferAbstract + Send + Sync>
						}
					} else {
						if *msaa_level != BstMSAALevel::One {
							Arc::new(
								Framebuffer::start(renderpass.clone())
									.add(target_ms_op.as_ref().unwrap().clone())
									.unwrap()
									.add(target_op.as_ref().unwrap().clone())
									.unwrap()
									.build()
									.unwrap(),
							) as Arc<dyn FramebufferAbstract + Send + Sync>
						} else {
							Arc::new(
								Framebuffer::start(renderpass.clone())
									.add(target_op.as_ref().unwrap().clone())
									.unwrap()
									.build()
									.unwrap(),
							) as Arc<dyn FramebufferAbstract + Send + Sync>
						}
					}
				})
				.collect::<Vec<_>>();

			let blend = match self.basalt.options_ref().composite_alpha {
				CompositeAlpha::PreMultiplied =>
					vulkano::pipeline::blend::AttachmentBlend {
						enabled: true,
						color_op: vulkano::pipeline::blend::BlendOp::Add,
						color_source: vulkano::pipeline::blend::BlendFactor::SrcAlpha,
						color_destination:
							vulkano::pipeline::blend::BlendFactor::OneMinusSrc1Alpha,
						alpha_op: vulkano::pipeline::blend::BlendOp::Add,
						alpha_source: vulkano::pipeline::blend::BlendFactor::SrcAlpha,
						alpha_destination:
							vulkano::pipeline::blend::BlendFactor::OneMinusSrc1Alpha,
						mask_red: true,
						mask_green: true,
						mask_blue: true,
						mask_alpha: true,
					},
				_ => vulkano::pipeline::blend::AttachmentBlend::alpha_blending(),
			};

			let vert_input: Arc<SingleBufferDefinition<ItfVertInfo>> =
				Arc::new(SingleBufferDefinition::new());
			let pipeline = Arc::new(
				GraphicsPipeline::start()
					.vertex_input(vert_input)
					.vertex_shader(self.shader_vs.main_entry_point(), ())
					.triangle_list()
					.viewports_dynamic_scissors_irrelevant(1)
					.fragment_shader(self.shader_fs.main_entry_point(), ())
					.depth_stencil_disabled()
					.blend_collective(blend)
					.render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
					.polygon_mode_fill()
					.sample_shading_enabled(1.0)
					.build(self.basalt.device())
					.unwrap(),
			);

			let set_pool = FixedSizeDescriptorSetsPool::new(
				pipeline.layout().descriptor_set_layout(0).unwrap().clone(),
			);

			let clear_values = if *msaa_level != BstMSAALevel::One {
				vec![[0.0, 0.0, 0.0, 0.0].into(), ClearValue::None]
			} else {
				vec![[0.0, 0.0, 0.0, 0.0].into()]
			};

			self.rc_op = Some(RenderContext {
				target_op,
				target_ms_op,
				renderpass,
				framebuffer,
				pipeline,
				set_pool,
				clear_values,
			});
		}

		let rc = self.rc_op.as_mut().unwrap();
		let mut odb_updated = false;

		self.basalt.poll_events_internal(|ev| {
			match ev {
				BstEvent::BstItfEv(BstItfEv::ODBUpdate) => {
					odb_updated = true;
					false
				},
				_ => true,
			}
		});

		if !resize
			&& rc.target_op.is_some()
			&& self.basalt.options_ref().itf_limit_draw
			&& !odb_updated
		{
			return (cmd, rc.target_op.clone());
		}

		cmd.begin_render_pass(
			rc.framebuffer[image_num].clone(),
			SubpassContents::Inline,
			rc.clear_values.clone(),
		)
		.unwrap();

		for (buf, buf_img, buf_sampler) in
			self.basalt.interface_ref().odb.draw_data(win_size, resize, *scale)
		{
			let set = rc
				.set_pool
				.next()
				.add_sampled_image(buf_img, buf_sampler)
				.unwrap()
				.build()
				.unwrap();
			cmd.draw(
				rc.pipeline.clone(),
				&self.dynamic_state,
				vec![Arc::new(buf)],
				set,
				(),
				std::iter::empty(),
			)
			.unwrap();
		}

		cmd.end_render_pass().unwrap();
		(cmd, rc.target_op.clone())
	}
}
