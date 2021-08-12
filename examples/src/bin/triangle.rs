extern crate basalt;
#[macro_use]
extern crate vulkano;
#[macro_use]
extern crate vulkano_shaders;

use basalt::image_view::BstImageView;
use basalt::interface::bin::{self, BinPosition, BinStyle};
use basalt::interface::ItfDrawTarget;
use basalt::Basalt;
use std::iter;
use std::sync::Arc;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::BufferUsage;
use vulkano::command_buffer::{
	AutoCommandBufferBuilder, CommandBufferUsage, DynamicState, SubpassContents,
};
use vulkano::image::attachment::AttachmentImage;
use vulkano::image::view::ImageView;
use vulkano::image::ImageUsage;
use vulkano::pipeline::viewport::Viewport;
use vulkano::pipeline::GraphicsPipeline;
use vulkano::render_pass::{Framebuffer, Subpass};
use vulkano::swapchain::{self, CompositeAlpha, Swapchain, SwapchainCreationError};
use vulkano::sync::{FlushError, GpuFuture};

fn main() {
	Basalt::initialize(
		basalt::Options::default().window_size(300, 300).title("Triangle Example"),
		Box::new(move |basalt_res| {
			let basalt = basalt_res.unwrap();
			let bin = basalt.interface_ref().new_bin();

			bin.style_update(BinStyle {
				position: Some(BinPosition::Window),
				pos_from_t: Some(25.0),
				pos_from_l: Some(25.0),
				width: Some(150.0),
				height: Some(30.0),
				back_color: Some(bin::Color::srgb_hex("e0e0e0")),
				border_size_t: Some(1.0),
				border_size_b: Some(1.0),
				border_size_l: Some(1.0),
				border_size_r: Some(1.0),
				border_color_t: Some(bin::Color::srgb_hex("ffffff")),
				border_color_b: Some(bin::Color::srgb_hex("ffffff")),
				border_color_l: Some(bin::Color::srgb_hex("ffffff")),
				border_color_r: Some(bin::Color::srgb_hex("ffffff")),
				text: String::from("Triangle Example"),
				text_height: Some(14.0),
				pad_t: Some(10.0),
				pad_l: Some(10.0),
				text_color: Some(bin::Color::srgb_hex("303030")),
				..BinStyle::default()
			});

			#[derive(Default, Debug, Clone)]
			struct Vertex {
				position: [f32; 2],
			}
			vulkano::impl_vertex!(Vertex, position);

			let vertex_buffer = CpuAccessibleBuffer::from_iter(
				basalt.device(),
				BufferUsage::vertex_buffer(),
				false,
				[
					Vertex {
						position: [-0.5, -0.25],
					},
					Vertex {
						position: [0.0, 0.5],
					},
					Vertex {
						position: [0.25, -0.1],
					},
				]
				.iter()
				.cloned(),
			)
			.unwrap();

			mod triangle_vs {
				shader! {
					ty: "vertex",
					src: "
                        #version 450

                        layout(location = 0) in vec2 position;

                        void main() {
                            gl_Position = vec4(position, 0.0, 1.0);
                        }
                    "
				}
			}

			mod triangle_fs {
				shader! {
					ty: "fragment",
					src: "
                        #version 450

                        layout(location = 0) out vec4 f_color;

                        void main() {
                            f_color = vec4(1.0, 0.0, 0.0, 1.0);
                        }
                    "
				}
			}

			let triangle_vs = triangle_vs::Shader::load(basalt.device()).unwrap();
			let triangle_fs = triangle_fs::Shader::load(basalt.device()).unwrap();
			let mut capabilities = basalt.swap_caps();
			let mut current_extent = basalt.current_extent();
			let mut current_extent_f = [current_extent[0] as f32, current_extent[1] as f32];

			let mut swapchain_and_images = {
				let (swapchain, images) = Swapchain::start(basalt.device(), basalt.surface())
					.num_images(capabilities.min_image_count)
					.format(capabilities.supported_formats[0].0)
					.dimensions(current_extent)
					.usage(ImageUsage::color_attachment())
					.sharing_mode(basalt.graphics_queue_ref())
					.composite_alpha(CompositeAlpha::Opaque)
					.build()
					.unwrap();
				let images: Vec<_> =
					images.into_iter().map(|img| ImageView::new(img).unwrap()).collect();
				(swapchain, images)
			};

			let mut recreate_swapchain = false;

			'recreate: loop {
				if recreate_swapchain {
					capabilities = basalt.swap_caps();
					current_extent = basalt.current_extent();
					current_extent_f = [current_extent[0] as f32, current_extent[1] as f32];

					swapchain_and_images = {
						let (swapchain, images) = match swapchain_and_images
							.0
							.recreate()
							.num_images(capabilities.min_image_count)
							.format(capabilities.supported_formats[0].0)
							.dimensions(current_extent)
							.usage(ImageUsage::color_attachment())
							.sharing_mode(basalt.graphics_queue_ref())
							.composite_alpha(CompositeAlpha::Opaque)
							.build()
						{
							Ok(ok) => ok,
							Err(SwapchainCreationError::UnsupportedDimensions) => continue,
							Err(e) => panic!("Failed to recreate swapchain: {:?}", e),
						};

						let images: Vec<_> = images
							.into_iter()
							.map(|img| ImageView::new(img).unwrap())
							.collect();
						(swapchain, images)
					};
				}

				let swapchain = &swapchain_and_images.0;
				let sc_images = &swapchain_and_images.1;

				let triangle_img = BstImageView::from_attachment(
					AttachmentImage::with_usage(
						basalt.device(),
						current_extent.into(),
						basalt.formats_in_use().interface,
						ImageUsage {
							color_attachment: true,
							transfer_source: true,
							..ImageUsage::none()
						},
					)
					.unwrap(),
				)
				.unwrap();

				let triangle_renderpass = Arc::new(
					single_pass_renderpass!(
						basalt.device(),
						attachments: {
							color: {
								load: Clear,
								store: Store,
								format: basalt.formats_in_use().interface,
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

				let triangle_pipeline = Arc::new(
					GraphicsPipeline::start()
						.vertex_input_single_buffer()
						.vertex_shader(triangle_vs.main_entry_point(), ())
						.triangle_list()
						.viewports(iter::once(Viewport {
							origin: [0.0, 0.0],
							depth_range: 0.0..1.0,
							dimensions: current_extent_f,
						}))
						.fragment_shader(triangle_fs.main_entry_point(), ())
						.render_pass(Subpass::from(triangle_renderpass.clone(), 0).unwrap())
						.build(basalt.device())
						.unwrap(),
				);

				let triangle_framebuffer = Arc::new(
					Framebuffer::start(triangle_renderpass.clone())
						.add(triangle_img.clone())
						.unwrap()
						.build()
						.unwrap(),
				);
				let mut previous_frame =
					Box::new(vulkano::sync::now(basalt.device())) as Box<dyn GpuFuture>;

				loop {
					previous_frame.cleanup_finished();

					if basalt
						.poll_events()
						.into_iter()
						.any(|ev| ev.requires_swapchain_recreate())
					{
						recreate_swapchain = true;
						continue 'recreate;
					}

					let (image_num, sub_optimal, acquire_future) =
						match swapchain::acquire_next_image(swapchain.clone(), None) {
							Ok(ok) => ok,
							Err(_) => {
								recreate_swapchain = true;
								continue 'recreate;
							},
						};

					let mut cmd_buf = AutoCommandBufferBuilder::primary(
						basalt.device(),
						basalt.graphics_queue_ref().family(),
						CommandBufferUsage::OneTimeSubmit,
					)
					.unwrap();

					cmd_buf
						.begin_render_pass(
							triangle_framebuffer.clone(),
							SubpassContents::Inline,
							vec![[0.0, 0.0, 0.0, 1.0].into()].into_iter(),
						)
						.unwrap()
						.draw(
							triangle_pipeline.clone(),
							&DynamicState::none(),
							vertex_buffer.clone(),
							(),
							(),
							iter::empty(),
						)
						.unwrap()
						.end_render_pass()
						.unwrap();

					let (cmd_buf, _) = basalt.interface_ref().draw(
						cmd_buf,
						ItfDrawTarget::SwapchainWithSource {
							source: triangle_img.clone(),
							images: sc_images.clone(),
							image_num,
						},
					);

					let cmd_buf = cmd_buf.build().unwrap();
					let future = match previous_frame
						.join(acquire_future)
						.then_execute(basalt.graphics_queue(), cmd_buf)
						.unwrap()
						.then_swapchain_present(
							basalt.graphics_queue(),
							swapchain.clone(),
							image_num,
						)
						.then_signal_fence_and_flush()
					{
						Ok(ok) => ok,
						Err(e) =>
							match e {
								FlushError::OutOfDate => {
									recreate_swapchain = true;
									continue 'recreate;
								},
								e => panic!("then_signal_fence_and_flush() Err: {}", e),
							},
					};

					if sub_optimal {
						future.wait(None).unwrap();
						recreate_swapchain = true;
						continue 'recreate;
					}

					if basalt.wants_exit() {
						future.wait(None).unwrap();
						break 'recreate;
					}

					previous_frame = Box::new(future) as Box<_>;
				}
			}

			basalt.wait_for_exit().unwrap();
		}),
	);
}
