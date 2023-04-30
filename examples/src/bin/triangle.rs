#[macro_use]
extern crate vulkano;
#[macro_use]
extern crate vulkano_shaders;

use std::iter;

use basalt::image_view::BstImageView;
use basalt::interface::bin::{self, BinPosition, BinStyle};
use basalt::interface::ItfDrawTarget;
use basalt::{Basalt, BstOptions};
use vulkano::buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, RenderPassBeginInfo, SubpassContents,
};
use vulkano::format::ClearValue;
use vulkano::image::attachment::AttachmentImage;
use vulkano::image::view::ImageView;
use vulkano::image::ImageUsage;
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryUsage, StandardMemoryAllocator};
use vulkano::pipeline::graphics::input_assembly::{InputAssemblyState, PrimitiveTopology};
use vulkano::pipeline::graphics::vertex_input::Vertex;
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::GraphicsPipeline;
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, Subpass};
use vulkano::swapchain::{
    self, FullScreenExclusive, Swapchain, SwapchainCreateInfo, SwapchainCreationError,
    SwapchainPresentInfo,
};
use vulkano::sync::{FlushError, GpuFuture};

fn main() {
    Basalt::initialize(
        BstOptions::default()
            .window_size(300, 300)
            .title("Triangle Example"),
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
            })
            .expect_valid();

            let mem_alloc = StandardMemoryAllocator::new_default(basalt.device());

            let cmd_alloc =
                StandardCommandBufferAllocator::new(basalt.device(), Default::default());

            #[derive(BufferContents, Vertex, Clone, Debug)]
            #[repr(C)]
            struct Vertex {
                #[format(R32G32_SFLOAT)]
                position: [f32; 2],
            }

            let vertex_buffer = Buffer::from_iter(
                &mem_alloc,
                BufferCreateInfo {
                    usage: BufferUsage::VERTEX_BUFFER,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    usage: MemoryUsage::Upload,
                    ..Default::default()
                },
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
                ],
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

            let triangle_vs = triangle_vs::load(basalt.device()).unwrap();
            let triangle_fs = triangle_fs::load(basalt.device()).unwrap();
            let mut capabilities = basalt.surface_capabilities(FullScreenExclusive::Default);
            let surface_formats = basalt.surface_formats(FullScreenExclusive::Default);
            let mut current_extent = basalt.current_extent(FullScreenExclusive::Default);
            let mut current_extent_f = [current_extent[0] as f32, current_extent[1] as f32];

            let mut swapchain_and_images = {
                let (swapchain, images) = Swapchain::new(
                    basalt.device(),
                    basalt.surface(),
                    SwapchainCreateInfo {
                        min_image_count: capabilities.min_image_count,
                        image_format: Some(surface_formats[0].0),
                        image_extent: current_extent,
                        image_usage: ImageUsage::COLOR_ATTACHMENT,
                        ..Default::default()
                    },
                )
                .unwrap();

                let images: Vec<_> = images
                    .into_iter()
                    .map(|img| ImageView::new_default(img).unwrap())
                    .collect();
                (swapchain, images)
            };

            let mut recreate_swapchain = false;

            'recreate: loop {
                if recreate_swapchain {
                    capabilities = basalt.surface_capabilities(FullScreenExclusive::Default);
                    current_extent = basalt.current_extent(FullScreenExclusive::Default);
                    current_extent_f = [current_extent[0] as f32, current_extent[1] as f32];

                    swapchain_and_images = {
                        let (swapchain, images) =
                            match swapchain_and_images.0.recreate(SwapchainCreateInfo {
                                min_image_count: capabilities.min_image_count,
                                image_format: Some(surface_formats[0].0),
                                image_extent: current_extent,
                                image_usage: ImageUsage::COLOR_ATTACHMENT,
                                ..Default::default()
                            }) {
                                Ok(ok) => ok,
                                Err(SwapchainCreationError::ImageExtentNotSupported {
                                    ..
                                }) => continue,
                                Err(e) => panic!("Failed to recreate swapchain: {:?}", e),
                            };

                        let images: Vec<_> = images
                            .into_iter()
                            .map(|img| ImageView::new_default(img).unwrap())
                            .collect();
                        (swapchain, images)
                    };
                }

                let swapchain = &swapchain_and_images.0;
                let sc_images = &swapchain_and_images.1;

                let triangle_img = BstImageView::from_attachment(
                    AttachmentImage::with_usage(
                        &mem_alloc,
                        current_extent,
                        basalt.formats_in_use().interface,
                        ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                    )
                    .unwrap(),
                )
                .unwrap();

                let triangle_renderpass = single_pass_renderpass!(
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
                .unwrap();

                let triangle_pipeline = GraphicsPipeline::start()
                    .vertex_input_state(Vertex::per_vertex())
                    .vertex_shader(triangle_vs.entry_point("main").unwrap(), ())
                    .input_assembly_state(
                        InputAssemblyState::new().topology(PrimitiveTopology::TriangleList),
                    )
                    .viewport_state(ViewportState::viewport_fixed_scissor_irrelevant(
                        iter::once(Viewport {
                            origin: [0.0; 2],
                            dimensions: current_extent_f,
                            depth_range: 0.0..1.0,
                        }),
                    ))
                    .fragment_shader(triangle_fs.entry_point("main").unwrap(), ())
                    .render_pass(Subpass::from(triangle_renderpass.clone(), 0).unwrap())
                    .build(basalt.device())
                    .unwrap();

                let triangle_framebuffer = Framebuffer::new(
                    triangle_renderpass.clone(),
                    FramebufferCreateInfo {
                        attachments: vec![triangle_img.clone()],
                        ..Default::default()
                    },
                )
                .unwrap();

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
                        &cmd_alloc,
                        basalt.graphics_queue_ref().queue_family_index(),
                        CommandBufferUsage::OneTimeSubmit,
                    )
                    .unwrap();

                    cmd_buf
                        .begin_render_pass(
                            RenderPassBeginInfo {
                                clear_values: vec![Some(ClearValue::Float([0.0, 0.0, 0.0, 1.0]))],
                                ..RenderPassBeginInfo::framebuffer(triangle_framebuffer.clone())
                            },
                            SubpassContents::Inline,
                        )
                        .unwrap()
                        .bind_pipeline_graphics(triangle_pipeline.clone())
                        .bind_vertex_buffers(0, vertex_buffer.clone())
                        .draw(vertex_buffer.len() as u32, 1, 0, 0)
                        .unwrap()
                        .end_render_pass()
                        .unwrap();

                    let (cmd_buf, _) = basalt.interface_ref().draw(
                        cmd_buf,
                        ItfDrawTarget::SwapchainWithSource {
                            source: triangle_img.clone(),
                            images: sc_images.clone(),
                            image_num: image_num as usize,
                        },
                    );

                    let cmd_buf = cmd_buf.build().unwrap();
                    let future = match previous_frame
                        .join(acquire_future)
                        .then_execute(basalt.graphics_queue(), cmd_buf)
                        .unwrap()
                        .then_swapchain_present(
                            basalt.graphics_queue(),
                            SwapchainPresentInfo::swapchain_image_index(
                                swapchain.clone(),
                                image_num,
                            ),
                        )
                        .then_signal_fence_and_flush()
                    {
                        Ok(ok) => ok,
                        Err(e) => {
                            match e {
                                FlushError::OutOfDate => {
                                    recreate_swapchain = true;
                                    continue 'recreate;
                                },
                                e => panic!("then_signal_fence_and_flush() Err: {}", e),
                            }
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
