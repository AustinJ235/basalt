use interface::interface::{ItfEvent, ItfVertInfo};
use parking_lot::Mutex;
use shaders;
use std::sync::Arc;
use vulkano::{
	command_buffer::{
		pool::standard::StandardCommandPoolBuilder, AutoCommandBufferBuilder, DynamicState
	}, descriptor::descriptor_set::FixedSizeDescriptorSetsPool, format::{ClearValue, Format as VkFormat}, framebuffer::{Framebuffer, FramebufferAbstract, RenderPassAbstract, Subpass}, image::{
		attachment::AttachmentImage, swapchain::SwapchainImage, traits::{ImageAccess, ImageViewAccess}, ImageUsage
	}, pipeline::{
		vertex::SingleBufferDefinition, viewport::Viewport, GraphicsPipeline, GraphicsPipelineAbstract
	}
};
use Basalt;

#[allow(dead_code)]
struct RenderContext {
	target_op:
		Option<(Arc<dyn ImageAccess + Send + Sync>, Arc<dyn ImageViewAccess + Send + Sync>)>,
	target_ms_op: Option<Arc<dyn ImageAccess + Send + Sync>>,
	renderpass: Arc<dyn RenderPassAbstract + Send + Sync>,
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
	msaa: Mutex<u32>,
	scale: Mutex<f32>,
	dynamic_state: DynamicState,
}

impl ItfRenderer {
	pub fn new(basalt: Arc<Basalt>) -> Self {
		let shader_vs = shaders::interface_vs::Shader::load(basalt.device.clone()).unwrap();
		let shader_fs = shaders::interface_fs::Shader::load(basalt.device.clone()).unwrap();

		ItfRenderer {
			rc_op: None,
			msaa: Mutex::new(4),
			scale: Mutex::new(1.0),
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
		mut cmd: AutoCommandBufferBuilder<StandardCommandPoolBuilder>,
		win_size: [u32; 2],
		mut resize: bool,
		swap_imgs: &Vec<Arc<SwapchainImage<S>>>,
		render_to_swapchain: bool,
		image_num: usize,
	) -> (
		AutoCommandBufferBuilder<StandardCommandPoolBuilder>,
		Option<Arc<dyn ImageViewAccess + Send + Sync>>,
	) {
		let mut samples = self.msaa.lock();
		let mut scale = self.scale.lock();
		let mut recreate_rc = resize;

		self.basalt.interface_ref().itf_events.lock().retain(|e| {
			match e {
				ItfEvent::MSAAChanged => {
					*samples = self.basalt.interface_ref().msaa();
					recreate_rc = true;
					false
				},
				ItfEvent::ScaleChanged => {
					*scale = self.basalt.interface_ref().scale();
					resize = true;
					false
				},
			}
		});

		if self.rc_op.is_none() || recreate_rc {
			let color_format = if render_to_swapchain {
				swap_imgs[0].swapchain().format()
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
			} else {
				None
			};

			let target_ms_op = if *samples > 1 {
				Some(
					AttachmentImage::multisampled_with_usage(
						self.basalt.device(),
						win_size,
						*samples,
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
			} else {
				None
			};

			let renderpass = match *samples {
				1 =>
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
					) as Arc<dyn RenderPassAbstract + Send + Sync>,

				s =>
					if render_to_swapchain {
						Arc::new(
							single_pass_renderpass!(self.basalt.device(),
								attachments: {
									image_ms: {
										load: Clear,
										store: Store,
										format: color_format,
										samples: s,
									}, image: {
										load: Clear,
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
						) as Arc<dyn RenderPassAbstract + Send + Sync>
					} else {
						Arc::new(
							single_pass_renderpass!(self.basalt.device(),
								attachments: {
									image_ms: {
										load: Clear,
										store: Store,
										format: color_format,
										samples: s,
									}, image: {
										load: Clear,
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
						) as Arc<dyn RenderPassAbstract + Send + Sync>
					},
			};

			let framebuffer = swap_imgs
				.iter()
				.map(|image| {
					if render_to_swapchain {
						if *samples > 1 {
							Arc::new(
								Framebuffer::start(renderpass.clone())
									.add(target_ms_op.as_ref().unwrap().clone())
									.unwrap()
									.add(image.clone())
									.unwrap()
									.build()
									.unwrap(),
							)
								as Arc<
									dyn vulkano::framebuffer::FramebufferAbstract + Send + Sync,
								>
						} else {
							Arc::new(
								Framebuffer::start(renderpass.clone())
									.add(image.clone())
									.unwrap()
									.build()
									.unwrap(),
							)
								as Arc<
									dyn vulkano::framebuffer::FramebufferAbstract + Send + Sync,
								>
						}
					} else {
						if *samples > 1 {
							Arc::new(
								Framebuffer::start(renderpass.clone())
									.add(target_ms_op.as_ref().unwrap().clone())
									.unwrap()
									.add(target_op.as_ref().unwrap().clone())
									.unwrap()
									.build()
									.unwrap(),
							)
								as Arc<
									dyn vulkano::framebuffer::FramebufferAbstract + Send + Sync,
								>
						} else {
							Arc::new(
								Framebuffer::start(renderpass.clone())
									.add(target_op.as_ref().unwrap().clone())
									.unwrap()
									.build()
									.unwrap(),
							)
								as Arc<
									dyn vulkano::framebuffer::FramebufferAbstract + Send + Sync,
								>
						}
					}
				})
				.collect::<Vec<_>>();

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
					.blend_collective(
						vulkano::pipeline::blend::AttachmentBlend::alpha_blending(),
					)
					.render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
					.polygon_mode_fill()
					.sample_shading_enabled(1.0)
					.build(self.basalt.device())
					.unwrap(),
			);

			let set_pool = FixedSizeDescriptorSetsPool::new(
				pipeline.layout().descriptor_set_layout(0).unwrap().clone(),
			);

			let clear_values = if *samples > 1 {
				vec![[0.0, 0.0, 0.0, 0.0].into(), [0.0, 0.0, 0.0, 0.0].into()]
			} else {
				vec![[0.0, 0.0, 0.0, 0.0].into()]
			};

			self.rc_op = Some(RenderContext {
				target_op: target_op.map(|v| {
					(
						v.clone() as Arc<dyn ImageAccess + Send + Sync>,
						v as Arc<dyn ImageViewAccess + Send + Sync>,
					)
				}),

				target_ms_op: target_ms_op.map(|v| v as Arc<dyn ImageAccess + Send + Sync>),
				renderpass,
				framebuffer,
				pipeline,
				set_pool,
				clear_values,
			});
		}

		let rc = self.rc_op.as_mut().unwrap();
		cmd = cmd
			.begin_render_pass(
				rc.framebuffer[image_num].clone(),
				false,
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
			cmd = cmd
				.draw(rc.pipeline.clone(), &self.dynamic_state, vec![Arc::new(buf)], set, ())
				.unwrap();
		}

		cmd = cmd.end_render_pass().unwrap();
		(cmd, rc.target_op.as_ref().map(|v| v.1.clone()))
	}
}
