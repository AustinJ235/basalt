use std::sync::Arc;
use vulkano::command_buffer::pool::standard::StandardCommandPoolBuilder;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::image::traits::ImageAccess;
use vulkano::image::traits::ImageViewAccess;
use vulkano::pipeline::GraphicsPipelineAbstract;
use vulkano::image::attachment::AttachmentImage;
use vulkano::framebuffer::RenderPassAbstract;
use vulkano::framebuffer::FramebufferAbstract;
use vulkano::descriptor::descriptor_set::FixedSizeDescriptorSetsPool;
use vulkano::format::Format as VkFormat;
use vulkano::image::ImageUsage;
use vulkano::pipeline::viewport::Viewport;
use vulkano::pipeline::vertex::SingleBufferDefinition;
use vulkano::framebuffer::Subpass;
use vulkano::framebuffer::Framebuffer;
use vulkano::pipeline::GraphicsPipeline;
use vulkano::command_buffer;
use vulkano::image::swapchain::SwapchainImage;
use vulkano::format::ClearValue;
use Engine;
use interface::interface::ItfVertInfo;
use shaders;
use parking_lot::Mutex;
use interface::interface::ItfEvent;

#[allow(dead_code)]
struct RenderContext {
	target_op: Option<(Arc<ImageAccess + Send + Sync>, Arc<ImageViewAccess + Send + Sync>)>,
	target_ms_op: Option<Arc<ImageAccess + Send + Sync>>,
	renderpass: Arc<RenderPassAbstract + Send + Sync>,
	framebuffer: Vec<Arc<FramebufferAbstract + Send + Sync>>,
	pipeline: Arc<GraphicsPipelineAbstract + Send + Sync>,
	set_pool: FixedSizeDescriptorSetsPool<Arc<GraphicsPipelineAbstract + Send + Sync>>,
	clear_values: Vec<ClearValue>,
}

pub struct ItfRenderer {
	engine: Arc<Engine>,
	rc_op: Option<RenderContext>,
	shader_vs: shaders::interface_vs::Shader,
	shader_fs: shaders::interface_fs::Shader,
	msaa: Mutex<u32>,
	scale: Mutex<f32>,
}

impl ItfRenderer {
	pub fn new(engine: Arc<Engine>) -> Self {
		let shader_vs = shaders::interface_vs::Shader::load(engine.device.clone()).unwrap();
		let shader_fs = shaders::interface_fs::Shader::load(engine.device.clone()).unwrap();
	
		ItfRenderer {
			rc_op: None,
			msaa: Mutex::new(4),
			scale: Mutex::new(1.0),
			engine, shader_vs, shader_fs
		}
	}
	
	/// Command buffer used must not be in the middle of a render pass. Resize is to be set to true
	/// anytime the swapchain is recreated. Render to swapchain option will render the ui directly
	/// onto the swapchain images. If this is not set this function will return ImageViewAccess to
	/// the rendered image of the interface.
	pub fn draw<S: Send + Sync + 'static>(
		&mut self,
		mut cmd: AutoCommandBufferBuilder<StandardCommandPoolBuilder>,
		win_size: [u32; 2],
		mut resize: bool,
		swap_imgs: &Vec<Arc<SwapchainImage<S>>>,
		render_to_swapchain: bool,
		image_num: usize
	) -> (AutoCommandBufferBuilder<StandardCommandPoolBuilder>, Option<Arc<ImageViewAccess + Send + Sync>>) {
		const COLOR_FORMAT: VkFormat = VkFormat::R8G8B8A8Srgb;
		let mut samples = self.msaa.lock();
		let mut scale = self.scale.lock();
		let mut recreate_rc = resize;
		
		self.engine.interface_ref().itf_events.lock().retain(|e| match e {
			ItfEvent::MSAAChanged => {
				*samples = self.engine.interface_ref().msaa();
				recreate_rc = true;
				false
			}, ItfEvent::ScaleChanged => {
				*scale = self.engine.interface_ref().scale();
				resize = true;
				false
			}
		});
		
		if self.rc_op.is_none() || recreate_rc {
			let target_op = if !render_to_swapchain {
				Some(AttachmentImage::with_usage(
					self.engine.device(),
					win_size,
					COLOR_FORMAT,
					ImageUsage {
						transfer_source: true,
						color_attachment: true,
						sampled: true,
						.. vulkano::image::ImageUsage::none()
					}
				).unwrap())
			} else {
				None
			};
			
			let target_ms_op = if *samples > 1 {
				Some(AttachmentImage::multisampled_with_usage(
					self.engine.device(),
					win_size,
					*samples,
					COLOR_FORMAT,
					ImageUsage {
						transfer_source: true,
						color_attachment: true,
						sampled: true,
						.. vulkano::image::ImageUsage::none()
					}
				).unwrap())
			} else {
				None
			};	
			
			let color_format = match render_to_swapchain {
				false => COLOR_FORMAT,
				true => swap_imgs[0].swapchain().format()
			};
			
			let renderpass = match *samples {
				1 => Arc::new(
					single_pass_renderpass!(self.engine.device(),
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
					).unwrap()
				) as Arc<RenderPassAbstract + Send + Sync>,
				
				s => if render_to_swapchain {
					Arc::new(
						single_pass_renderpass!(self.engine.device(),
							attachments: {
								image_ms: {
									load: Clear,
									store: Store,
									format: COLOR_FORMAT,
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
						).unwrap()
					) as Arc<RenderPassAbstract + Send + Sync>
				} else {
					Arc::new(
						single_pass_renderpass!(self.engine.device(),
							attachments: {
								image_ms: {
									load: Clear,
									store: Store,
									format: COLOR_FORMAT,
									samples: s,
								}, image: {
									load: Clear,
									store: Store,
									format: COLOR_FORMAT,
									samples: 1,
								}
							}, pass: {
								color: [image_ms],
								depth_stencil: {},
								resolve: [image]
							}
						).unwrap()
					) as Arc<RenderPassAbstract + Send + Sync>
				}
			};
			
			let framebuffer = swap_imgs.iter().map(|image| {
				if render_to_swapchain {
					if *samples > 1 {
						Arc::new(Framebuffer::start(renderpass.clone())
							.add(target_ms_op.as_ref().unwrap().clone()).unwrap()
							.add(image.clone()).unwrap()
							.build().unwrap()
						) as Arc<vulkano::framebuffer::FramebufferAbstract + Send + Sync>
					} else {
						Arc::new(Framebuffer::start(renderpass.clone())
							.add(image.clone()).unwrap()
							.build().unwrap()
						) as Arc<vulkano::framebuffer::FramebufferAbstract + Send + Sync>
					}
				} else {
					if *samples > 1 {
						Arc::new(Framebuffer::start(renderpass.clone())
							.add(target_ms_op.as_ref().unwrap().clone()).unwrap()
							.add(target_op.as_ref().unwrap().clone()).unwrap()
							.build().unwrap()
						) as Arc<vulkano::framebuffer::FramebufferAbstract + Send + Sync>
					} else {
						Arc::new(Framebuffer::start(renderpass.clone())
							.add(target_op.as_ref().unwrap().clone()).unwrap()
							.build().unwrap()
						) as Arc<vulkano::framebuffer::FramebufferAbstract + Send + Sync>
					}
				}
			}).collect::<Vec<_>>();
			
			let vert_input: Arc<SingleBufferDefinition<ItfVertInfo>> = Arc::new(SingleBufferDefinition::new());
			let pipeline = Arc::new(
				GraphicsPipeline::start()
					.vertex_input(vert_input)
					.vertex_shader(self.shader_vs.main_entry_point(), ())
					.triangle_list()
					.viewports(::std::iter::once(Viewport {
						origin: [0.0, 0.0],
						depth_range: 0.0 .. 1.0,
						dimensions: [win_size[0] as f32, win_size[1] as f32],
					}))
					.fragment_shader(self.shader_fs.main_entry_point(), ())
					.depth_stencil_disabled()
					.blend_collective(vulkano::pipeline::blend::AttachmentBlend::alpha_blending())
					.render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
					.polygon_mode_fill()
					.build(self.engine.device()).unwrap()
			) as Arc<GraphicsPipelineAbstract + Send + Sync>;
			
			let set_pool = FixedSizeDescriptorSetsPool::new(pipeline.clone(), 0);
			
			let clear_values = if *samples > 1 {
				vec![[0.0, 0.0, 0.0, 0.0].into(), [0.0, 0.0, 0.0, 0.0].into()]
			} else {
				vec![[0.0, 0.0, 0.0, 0.0].into()]
			};
			
			self.rc_op = Some(RenderContext {
				target_op: target_op.map(|v| (
					v.clone() as Arc<ImageAccess + Send + Sync>, 
					v as Arc<ImageViewAccess + Send + Sync>
				)),
				
				target_ms_op: target_ms_op.map(|v| v as Arc<ImageAccess + Send + Sync>),
				renderpass, framebuffer, pipeline, set_pool, clear_values
			});
		}
		
		let rc = self.rc_op.as_mut().unwrap();
		cmd = cmd.begin_render_pass(rc.framebuffer[image_num].clone(), false, rc.clear_values.clone()).unwrap();
		
		for (buf, buf_img, buf_sampler) in self.engine.interface_ref().odb.draw_data(win_size, resize, *scale) {
			let set = rc.set_pool.next().add_sampled_image(buf_img, buf_sampler).unwrap().build().unwrap();
			cmd = cmd.draw(rc.pipeline.clone(), &command_buffer::DynamicState::none(), vec![Arc::new(buf)], set, ()).unwrap();
		}
		
		cmd = cmd.end_render_pass().unwrap();
		(cmd, rc.target_op.as_ref().map(|v| v.1.clone()))
	}
}
