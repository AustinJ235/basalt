use std::sync::Arc;

use basalt::input::Qwerty;
use basalt::interface::{BinPosition, BinStyle, Color};
use basalt::render::{Renderer, UserRenderer};
use basalt::window::{Window, WindowOptions};
use basalt::{Basalt, BasaltOptions};
use vulkano::buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage, Subbuffer};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, PrimaryAutoCommandBuffer, RenderPassBeginInfo, SubpassBeginInfo,
    SubpassEndInfo,
};
use vulkano::image::view::ImageView;
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator};
use vulkano::pipeline::graphics::color_blend::{ColorBlendAttachmentState, ColorBlendState};
use vulkano::pipeline::graphics::input_assembly::InputAssemblyState;
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::RasterizationState;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::layout::PipelineDescriptorSetLayoutCreateInfo;
use vulkano::pipeline::{
    DynamicState, GraphicsPipeline, PipelineLayout, PipelineShaderStageCreateInfo,
};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
use vulkano::shader::ShaderModule;

fn main() {
    Basalt::initialize(BasaltOptions::default(), move |basalt_res| {
        let basalt = basalt_res.unwrap();

        let window = basalt
            .window_manager_ref()
            .create(WindowOptions {
                title: String::from("triangle"),
                inner_size: Some([400; 2]),
                ..WindowOptions::default()
            })
            .unwrap();

        window.on_press(Qwerty::F8, move |target, _, _| {
            let window = target.into_window().unwrap();
            println!("VSync: {:?}", window.toggle_renderer_vsync());
            Default::default()
        });

        window.on_press(Qwerty::F9, move |target, _, _| {
            let window = target.into_window().unwrap();
            println!("MSAA: {:?}", window.decr_renderer_msaa());
            Default::default()
        });

        window.on_press(Qwerty::F10, move |target, _, _| {
            let window = target.into_window().unwrap();
            println!("MSAA: {:?}", window.incr_renderer_msaa());
            Default::default()
        });

        let example_bin = window.new_bin();

        example_bin
            .style_update(BinStyle {
                position: Some(BinPosition::Window),
                pos_from_t: Some(25.0),
                pos_from_l: Some(25.0),
                width: Some(300.0),
                height: Some(50.0),
                back_color: Some(Color::shex("000000f0")),
                text: String::from("Triangle Example"),
                text_height: Some(28.0),
                pad_t: Some(11.0),
                pad_l: Some(11.0),
                text_color: Some(Color::shex("ffffff")),
                ..BinStyle::default()
            })
            .expect_valid();

        Renderer::new(window.clone())
            .unwrap()
            .with_user_renderer(MyRenderer::new(window))
            .run()
            .unwrap();

        basalt.exit();
    });
}

#[derive(BufferContents, Vertex)]
#[repr(C)]
struct TriangleVertex {
    #[format(R32G32_SFLOAT)]
    position: [f32; 2],
}

mod triangle_vs {
    vulkano_shaders::shader! {
        ty: "vertex",
        src: r"
            #version 450

            layout(location = 0) in vec2 position;

            void main() {
                gl_Position = vec4(position, 0.0, 1.0);
            }
        ",
    }
}

mod triangle_fs {
    vulkano_shaders::shader! {
        ty: "fragment",
        src: r"
            #version 450

            layout(location = 0) out vec4 f_color;

            void main() {
                f_color = vec4(1.0, 0.0, 0.0, 1.0);
            }
        ",
    }
}

#[allow(dead_code)]
struct MyRenderer {
    window: Arc<Window>,
    mem_alloc: Arc<StandardMemoryAllocator>,
    vertex_buffer: Subbuffer<[TriangleVertex]>,
    tri_vs_sm: Arc<ShaderModule>,
    tri_fs_sm: Arc<ShaderModule>,
    target: Option<Arc<ImageView>>,
    render_pass: Option<Arc<RenderPass>>,
    pipeline: Option<Arc<GraphicsPipeline>>,
    viewport: Viewport,
    framebuffer: Option<Arc<Framebuffer>>,
}

impl MyRenderer {
    pub fn new(window: Arc<Window>) -> Self {
        let mem_alloc = Arc::new(StandardMemoryAllocator::new_default(
            window.basalt_ref().device(),
        ));

        let vertices = [
            TriangleVertex {
                position: [-0.5, -0.25],
            },
            TriangleVertex {
                position: [0.0, 0.5],
            },
            TriangleVertex {
                position: [0.25, -0.1],
            },
        ];

        let vertex_buffer = Buffer::from_iter(
            mem_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            vertices,
        )
        .unwrap();

        let tri_vs_sm = triangle_vs::load(window.basalt_ref().device()).unwrap();
        let tri_fs_sm = triangle_fs::load(window.basalt_ref().device()).unwrap();

        let viewport = Viewport {
            offset: [0.0, 0.0],
            extent: [0.0, 0.0],
            depth_range: 0.0..=1.0,
        };

        Self {
            window,
            mem_alloc,
            vertex_buffer,
            tri_vs_sm,
            tri_fs_sm,
            target: None,
            render_pass: None,
            pipeline: None,
            viewport,
            framebuffer: None,
        }
    }
}

impl UserRenderer for MyRenderer {
    fn target_changed(&mut self, target: Arc<ImageView>) {
        if self.render_pass.is_none() {
            self.render_pass = Some(
                vulkano::single_pass_renderpass!(
                    self.window.basalt_ref().device(),
                    attachments: {
                        color: {
                            format: target.format(),
                            samples: 1,
                            load_op: Clear,
                            store_op: Store,
                        },
                    },
                    pass: {
                        color: [color],
                        depth_stencil: {},
                    },
                )
                .unwrap(),
            );
        }

        if self.pipeline.is_none() {
            let tri_vs_entry = self.tri_vs_sm.entry_point("main").unwrap();
            let tri_fs_entry = self.tri_fs_sm.entry_point("main").unwrap();

            let vertex_input_state = TriangleVertex::per_vertex()
                .definition(&tri_vs_entry)
                .unwrap();

            let stages = [
                PipelineShaderStageCreateInfo::new(tri_vs_entry),
                PipelineShaderStageCreateInfo::new(tri_fs_entry),
            ];

            let layout = PipelineLayout::new(
                self.window.basalt_ref().device(),
                PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
                    .into_pipeline_layout_create_info(self.window.basalt_ref().device())
                    .unwrap(),
            )
            .unwrap();

            let subpass = Subpass::from(self.render_pass.clone().unwrap(), 0).unwrap();

            self.pipeline = Some(
                GraphicsPipeline::new(
                    self.window.basalt_ref().device(),
                    None,
                    GraphicsPipelineCreateInfo {
                        stages: stages.into_iter().collect(),
                        vertex_input_state: Some(vertex_input_state),
                        input_assembly_state: Some(InputAssemblyState::default()),
                        viewport_state: Some(ViewportState::default()),
                        rasterization_state: Some(RasterizationState::default()),
                        multisample_state: Some(MultisampleState::default()),
                        color_blend_state: Some(ColorBlendState::with_attachment_states(
                            subpass.num_color_attachments(),
                            ColorBlendAttachmentState::default(),
                        )),
                        dynamic_state: [DynamicState::Viewport].into_iter().collect(),
                        subpass: Some(subpass.into()),
                        ..GraphicsPipelineCreateInfo::layout(layout)
                    },
                )
                .unwrap(),
            );
        }

        self.framebuffer = Some(
            Framebuffer::new(
                self.render_pass.clone().unwrap(),
                FramebufferCreateInfo {
                    attachments: vec![target.clone()],
                    ..FramebufferCreateInfo::default()
                },
            )
            .unwrap(),
        );

        let [width, height, _] = target.image().extent();
        self.viewport.extent = [width as f32, height as f32];
        self.target = Some(target);
    }

    fn draw(&mut self, cmd_builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        cmd_builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![Some([0.0, 0.0, 1.0, 1.0].into())],
                    ..RenderPassBeginInfo::framebuffer(self.framebuffer.clone().unwrap())
                },
                SubpassBeginInfo::default(),
            )
            .unwrap()
            .set_viewport(0, [self.viewport.clone()].into_iter().collect())
            .unwrap()
            .bind_pipeline_graphics(self.pipeline.clone().unwrap())
            .unwrap()
            .bind_vertex_buffers(0, self.vertex_buffer.clone())
            .unwrap();
        
        unsafe {
            cmd_builder
                .draw(self.vertex_buffer.len() as u32, 1, 0, 0)
                .unwrap();
        }

        cmd_builder
            .end_render_pass(SubpassEndInfo::default())
            .unwrap();
    }
}
