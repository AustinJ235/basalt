// WARNING! This example isn't feature complete

extern crate basalt;
#[macro_use]
extern crate vulkano;
#[macro_use]
extern crate vulkano_shaders;

use basalt::Basalt;
use basalt::interface::render::ItfRenderer;
use vulkano::buffer::BufferUsage;
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::command_buffer::{AutoCommandBufferBuilder, CommandBufferUsage, SubpassContents, DynamicState};
use vulkano::descriptor::descriptor_set::FixedSizeDescriptorSetsPool;
use vulkano::image::ImageUsage;
use vulkano::image::attachment::AttachmentImage;
use vulkano::pipeline::GraphicsPipeline;
use vulkano::pipeline::viewport::Viewport;
use vulkano::render_pass::{Framebuffer, Subpass};
use vulkano::sampler::Sampler;
use vulkano::swapchain::{self,Swapchain,CompositeAlpha};
use vulkano::sync::{GpuFuture, FlushError};
use std::iter;
use std::sync::Arc;
use vulkano::image::view::ImageView;

fn main() {
	Basalt::initialize(
		basalt::Options::default()
			.window_size(300, 300)
			.title("Triangle Example"),
		Box::new(move |basalt_res| {
			let basalt = basalt_res.unwrap();

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
                    Vertex { position: [-0.5, -0.25], },
                    Vertex { position: [0.0, 0.5], },
                    Vertex { position: [0.25, -0.1], }
                ]
                .iter()
                .cloned(),
            ).unwrap();

            let square_buffer = CpuAccessibleBuffer::from_iter(
                basalt.device(),
                BufferUsage::vertex_buffer(),
                false,
                [
                    Vertex { position: [-1.0, -1.0], },
                    Vertex { position: [1.0, -1.0], },
                    Vertex { position: [1.0, 1.0], },
                    Vertex { position: [1.0, 1.0], },
                    Vertex { position: [-1.0, 1.0], },
                    Vertex { position: [-1.0, -1.0], }
                ]
                .iter()
                .cloned()
            ).unwrap();

            mod triangle_vs {
                vulkano_shaders::shader! {
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
                vulkano_shaders::shader! {
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

            mod merge_vs {
                shader!{
                    ty: "vertex",
                    src: "
                        #version 450

                        layout(location = 0) in vec2 position;
                        layout(location = 0) out vec2 out_coords;

                        void main() {
                            out_coords = vec2(position.x/2, position.y/2)+vec2(0.5);
                            gl_Position = vec4(position, 0, 1);
                        }
                "
                }
            }

            mod merge_fs {
                shader!{
                    ty: "fragment",
                    src: "
                        #version 450

                        layout(location = 0) in vec2 in_coords;
                        layout(location = 0) out vec4 out_color;

                        layout(set = 0, binding = 0) uniform sampler2D triangle;
                        layout(set = 0, binding = 1) uniform sampler2D basalt;

                        void main() {
                            vec4 color = texture(triangle, in_coords);
                            vec4 itf = texture(basalt, in_coords);
                            out_color = vec4(mix(color.rgb, itf.rgb, itf.a), 1);
                        }
                "
                }
            }

            let merge_vs = merge_vs::Shader::load(basalt.device()).unwrap();
            let merge_fs = merge_fs::Shader::load(basalt.device()).unwrap();
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
                let images: Vec<_> = images.into_iter().map(|img| ImageView::new(img).unwrap()).collect();
                (swapchain, images)
            };

            let mut recreate_swapchain = false;
            let mut itf_renderer = ItfRenderer::new(basalt.clone());

            'recreate: loop {
                if recreate_swapchain {
                    capabilities = basalt.swap_caps();
                    current_extent = basalt.current_extent();
                    current_extent_f = [current_extent[0] as f32, current_extent[1] as f32];

                    swapchain_and_images = {
                        let (swapchain, images) = swapchain_and_images.0
                            .recreate()
                            .num_images(capabilities.min_image_count)
                            .format(capabilities.supported_formats[0].0)
                            .dimensions(current_extent)
                            .usage(ImageUsage::color_attachment())
                            .sharing_mode(basalt.graphics_queue_ref())
                            .composite_alpha(CompositeAlpha::Opaque)
                            .build()
                            .unwrap();
                        let images: Vec<_> = images.into_iter().map(|img| ImageView::new(img).unwrap()).collect();
                        (swapchain, images)
                    };
                    
                    recreate_swapchain = false;
                }

                let swapchain = &swapchain_and_images.0;
                let sc_images = &swapchain_and_images.1;

                let triangle_img = ImageView::new(AttachmentImage::with_usage(
                    basalt.device(),
                    current_extent.into(),
                    swapchain.format(),
                    ImageUsage {
                        sampled: true,
                        color_attachment: true,
                        .. ImageUsage::none()
                    }
                ).unwrap()).unwrap();

                // TODO: This is a workaround for vulkano 0.23
                unsafe {
                    use vulkano::image::ImageViewAbstract;
                    triangle_img.image().increase_gpu_lock();
                    triangle_img.image().unlock(Some(vulkano::image::ImageLayout::ColorAttachmentOptimal));
                }

                let triangle_renderpass = Arc::new(
                    single_pass_renderpass!(
                        basalt.device(),
                        attachments: {
                            color: {
                                load: Clear,
                                store: Store,
                                format: swapchain.format(),
                                samples: 1,
                            }
                        },
                        pass: {
                            color: [color],
                            depth_stencil: {}
                        }
                    ).unwrap()
                );

                let merge_renderpass = Arc::new(
                    single_pass_renderpass!(
                        basalt.device(),
                        attachments: {
                            color: {
                                load: Clear,
                                store: Store,
                                format: swapchain.format(),
                                samples: 1,
                            }
                        },
                        pass: {
                            color: [color],
                            depth_stencil: {}
                        }
                    ).unwrap()
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
                        .unwrap()
                );

                let merge_pipeline = Arc::new(
                    GraphicsPipeline::start()
                        .vertex_input_single_buffer()
                        .vertex_shader(merge_vs.main_entry_point(), ())
                        .triangle_list()
                        .viewports(iter::once(Viewport {
                            origin: [0.0, 0.0],
                            depth_range: 0.0..1.0,
                            dimensions: current_extent_f,
                        }))
                        .fragment_shader(merge_fs.main_entry_point(), ())
                        .render_pass(Subpass::from(merge_renderpass.clone(), 0).unwrap())
                        .build(basalt.device())
                        .unwrap()
                );

                let triangle_framebuffer = sc_images
                    .iter()
                    .map(|_| {
                        Arc::new(
                            Framebuffer::start(triangle_renderpass.clone())
                                .add(triangle_img.clone())
                                .unwrap()
                                .build()
                                .unwrap()
                        )
                    })
                    .collect::<Vec<_>>();

                let merge_framebuffer = sc_images
                    .iter()
                    .map(|sc_image| {
                        Arc::new(
                            Framebuffer::start(triangle_renderpass.clone())
                                .add(sc_image.clone())
                                .unwrap()
                                .build()
                                .unwrap()
                        )
                    })
                    .collect::<Vec<_>>();

                let mut merge_set_pool = FixedSizeDescriptorSetsPool::new(
				    merge_pipeline.layout().descriptor_set_layout(0).unwrap().clone(),
			    );

                let merge_sampler = Sampler::simple_repeat_linear_no_mipmap(basalt.device());
                let mut previous_frame = Box::new(vulkano::sync::now(basalt.device()))
					as Box<dyn GpuFuture>;
                let mut recreated = true;

                loop {
                    previous_frame.cleanup_finished();

                    if basalt.poll_events().into_iter().any(|ev| ev.requires_swapchain_recreate()) {
                        recreate_swapchain = true;
                        continue 'recreate;
                    }

                    let (image_num, sub_optimal, acquire_future) =
                        match swapchain::acquire_next_image(swapchain.clone(), None) {
                            Ok(ok) => ok,
                            Err(_) => {
                                recreate_swapchain = true;
                                continue 'recreate;
                            }
                        };

                    let mut cmd_buf = AutoCommandBufferBuilder::primary(
                        basalt.device(),
                        basalt.graphics_queue_ref().family(),
                        CommandBufferUsage::OneTimeSubmit
                    ).unwrap();

                    cmd_buf
                        .begin_render_pass(
                            triangle_framebuffer[image_num].clone(),
                            SubpassContents::Inline,
                            vec![[0.0, 0.0, 0.0, 1.0].into()].into_iter()
                        ).unwrap()

                        .draw(
                            triangle_pipeline.clone(),
                            &DynamicState::none(),
                            vertex_buffer.clone(),
                            (),
                            (),
                            iter::empty()
                        ).unwrap()
                        
                        .end_render_pass().unwrap();

                    let (mut cmd_buf, basalt_img) = itf_renderer.draw(
                        cmd_buf,
                        current_extent,
                        recreated,
                        &sc_images,
                        false,
                        image_num,
                    );

                    unsafe {
                        use vulkano::image::ImageViewAbstract;
                        basalt_img.as_ref().unwrap().image().increase_gpu_lock();
                        basalt_img.as_ref().unwrap().image().unlock(Some(vulkano::image::ImageLayout::ColorAttachmentOptimal));
                    }

                    let merge_set = Arc::new(
                        merge_set_pool
                            .next()
                            .add_sampled_image(triangle_img.clone(), merge_sampler.clone())
                            .unwrap()
                            .add_sampled_image(basalt_img.unwrap(), merge_sampler.clone())
                            .unwrap()
                            .build()
                            .unwrap()
                    );

                    cmd_buf
                        .begin_render_pass(
                            merge_framebuffer[image_num].clone(),
                            SubpassContents::Inline,
                            vec![[0.0, 0.0, 0.0, 1.0].into()].into_iter()
                        ).unwrap()

                        .draw(
                            merge_pipeline.clone(),
                            &DynamicState::none(),
                            square_buffer.clone(),
                            merge_set,
                            (),
                            iter::empty()
                        ).unwrap()

                        .end_render_pass()
                        .unwrap();

                    let cmd_buf = cmd_buf.build().unwrap();
                    let future = match previous_frame
                        .join(acquire_future)
                        .then_execute(basalt.graphics_queue(), cmd_buf)
                        .unwrap()
                        .then_swapchain_present(
                            basalt.graphics_queue(),
                            swapchain.clone(),
                            image_num
                        )
                        .then_signal_fence_and_flush()
                    {
                        Ok(ok) => ok,
                        Err(e) => match e {
                            FlushError::OutOfDate => {
                                recreate_swapchain = true;
                                continue 'recreate;
                            },
                            e => panic!("then_signal_fence_and_flush() Err: {}", e)
                        }
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
                    recreated = false;
                }
            }

			basalt.wait_for_exit().unwrap();
		}),
	);
}
