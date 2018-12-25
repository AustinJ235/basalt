#![allow(warnings)]

use vulkano::pipeline::GraphicsPipelineAbstract;
use parking_lot::Mutex;
use std::sync::Arc;
use vulkano::pipeline::shader::ShaderModule;
use vulkano::image::attachment::AttachmentImage;
use vulkano::framebuffer::RenderPassAbstract;
use vulkano::framebuffer::FramebufferAbstract;
use vulkano::descriptor::descriptor_set::FixedSizeDescriptorSetsPool;
use Engine;
use vulkano::image::traits::ImageAccess;
use vulkano::image::traits::ImageViewAccess;
use vulkano::format::Format as VkFormat;
use vulkano::image::ImageUsage;
use shaders;
use vulkano::pipeline::viewport::Viewport;
use vulkano::pipeline::vertex::SingleBufferDefinition;
use vulkano::framebuffer::Subpass;
use vulkano::framebuffer::Framebuffer;
use vulkano::pipeline::GraphicsPipeline;
use interface::interface::ItfVertInfo;
use std::ops::Deref;
use vulkano::command_buffer;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use interface::odb::OrderedDualBuffer;
use vulkano::command_buffer::CommandBuffer;
use vulkano::sync::GpuFuture;

pub struct ImageLease {
	pub num: u8,
	inner: Arc<Image>,
}

impl ImageLease {
	pub fn image(&self) -> Arc<ImageViewAccess + Send + Sync> {
		self.inner.imageview.clone()
	}
}

impl Drop for ImageLease {
	fn drop(&mut self) {
		*self.inner.leased.lock() = false;
	}
}

struct Image {
	leased: Mutex<bool>,
	image: Arc<ImageAccess + Send + Sync>,
	imageview: Arc<ImageViewAccess + Send + Sync>,
	renderpass: Arc<RenderPassAbstract + Send + Sync>,
	framebuffer: Arc<FramebufferAbstract + Send + Sync>,
	pipeline: Arc<GraphicsPipelineAbstract + Send + Sync>,
	set_pool: Mutex<FixedSizeDescriptorSetsPool<Arc<GraphicsPipelineAbstract + Send + Sync>>>,
}

pub struct Renderer {
	engine: Arc<Engine>,
	shader_vs: shaders::interface_vs::Shader,
	shader_fs: shaders::interface_fs::Shader,
	image1: Arc<Image>,
	image2: Arc<Image>,
	active_img: Mutex<u8>,
}

impl Renderer {
	pub fn new(engine: Arc<Engine>) -> Arc<Self> {
		let shader_vs = shaders::interface_vs::Shader::load(engine.device.clone()).unwrap();
		let shader_fs = shaders::interface_fs::Shader::load(engine.device.clone()).unwrap();
		let mut images = Vec::new();
	
		for _ in 0..2 {
			let image = AttachmentImage::with_usage(
				engine.device(),
				[1920, 1080],
				//4,
				VkFormat::R8G8B8A8Srgb,
				ImageUsage {
					transfer_source: true,
					color_attachment: true,
					sampled: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
			
			let renderpass = Arc::new(
				single_pass_renderpass!(engine.device(),
					attachments: {
						image: {
							load: Clear,
							store: Store,
							format: VkFormat::R8G8B8A8Srgb,
							samples: 1,
						}
					}, pass: {
						color: [image],
						depth_stencil: {},
						resolve: []
					}
				).unwrap()
			) as Arc<RenderPassAbstract + Send + Sync>;
			
			let framebuffer = Arc::new(Framebuffer::start(renderpass.clone())
				.add(image.clone()).unwrap()
				.build().unwrap()
			) as Arc<vulkano::framebuffer::FramebufferAbstract + Send + Sync>;
			
			let vert_input: Arc<SingleBufferDefinition<ItfVertInfo>> = Arc::new(SingleBufferDefinition::new());
			
			let pipeline = Arc::new(
				GraphicsPipeline::start()
					.vertex_input(vert_input)
					.vertex_shader(shader_vs.main_entry_point(), ())
					.triangle_list()
					.viewports(::std::iter::once(Viewport {
						origin: [0.0, 0.0],
						depth_range: 0.0 .. 1.0,
						dimensions: [1920.0, 1080.0],
					}))
					.fragment_shader(shader_fs.main_entry_point(), ())
					.depth_stencil_disabled()
					.blend_collective(vulkano::pipeline::blend::AttachmentBlend::alpha_blending())
					.render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
					.polygon_mode_fill()
					.build(engine.device()).unwrap()
			) as Arc<GraphicsPipelineAbstract + Send + Sync>;
			
			let set_pool = Mutex::new(FixedSizeDescriptorSetsPool::new(pipeline.clone(), 0));
				
			images.push(Arc::new(Image {
				leased: Mutex::new(false),
				image: image.clone(),
				imageview: image,
				renderpass, framebuffer,
				pipeline, set_pool
			}));
		}
			
		Arc::new(Renderer {
			engine, shader_vs, shader_fs,
			image1: images.pop().unwrap(),
			image2: images.pop().unwrap(),
			active_img: Mutex::new(0),
		})
	}
	
	pub(crate) fn draw(&self, odb: &OrderedDualBuffer) {
		let image: &Image = match *self.active_img.lock() {
			1 => &self.image2,
			_ => &self.image1,
		};
		
		loop {
			if *image.leased.lock() {
				::std::thread::sleep(::std::time::Duration::from_millis(1));
			} else {
				break;
			}
		}
		
		let mut cmd_buf = AutoCommandBufferBuilder::primary_one_time_submit(self.engine.device(), self.engine.graphics_queue_ref().family()).unwrap();
		cmd_buf = cmd_buf.begin_render_pass(
			image.framebuffer.clone(), false,
			vec![
				[1.0, 1.0, 1.0, 1.0].into(),
			]
		).unwrap();
	
		for (buffer, buf_image, sampler) in odb.draw_data([1920, 1080], false, 1.0) {
			let set = Arc::new(
				image.set_pool.lock().next()
					.add_sampled_image(buf_image, sampler).unwrap()
					.build().unwrap()
			);
			
			cmd_buf = cmd_buf.draw(
				image.pipeline.clone(),
				&command_buffer::DynamicState::none(),
				vec![Arc::new(buffer)], set, ()
			).unwrap();
		}
		
		let cmd_buf = cmd_buf.end_render_pass().unwrap().build().unwrap();
		let fence = cmd_buf
			.execute(self.engine.transfer_queue()).unwrap()
			.then_signal_fence_and_flush().unwrap();
		fence.wait(None).unwrap();
		
		let mut active_img = self.active_img.lock();
		
		*active_img = match *active_img {
			1 => 2,
			_ => 1,
		};
	}
	
	pub fn lease_image(&self, old: Option<ImageLease>) -> Option<ImageLease> {
		drop(old);
		
		let (active_img, num) = match *self.active_img.lock() {
			0 => return None,
			1 => (self.image1.clone(), 1),
			_ => (self.image2.clone(), 2)
		};
		
		*active_img.leased.lock() = true;
		
		Some(ImageLease {
			num,
			inner: active_img
		})
	}
}
