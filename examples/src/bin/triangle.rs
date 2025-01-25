use std::sync::Arc;
use std::{iter, slice};

use basalt::input::Qwerty;
use basalt::interface::{BinPosition, BinStyle, Color};
use basalt::render::{Renderer, RendererContext, UserRenderer, UserTaskGraphInfo};
use basalt::window::{Window, WindowOptions};
use basalt::{Basalt, BasaltOptions};

mod vk {
    pub use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
    pub use vulkano::command_buffer::RenderPassBeginInfo;
    pub use vulkano::format::ClearValue;
    pub use vulkano::image::view::ImageView;
    pub use vulkano::image::Image;
    pub use vulkano::memory::allocator::{AllocationCreateInfo, DeviceLayout, MemoryTypeFilter};
    pub use vulkano::pipeline::graphics::color_blend::ColorBlendState;
    pub use vulkano::pipeline::graphics::viewport::Viewport;
    pub use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
    pub use vulkano::pipeline::layout::PipelineDescriptorSetLayoutCreateInfo;
    pub use vulkano::pipeline::{
        DynamicState, GraphicsPipeline, PipelineLayout, PipelineShaderStageCreateInfo,
    };
    pub use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
    pub use vulkano::shader::ShaderModule;
    pub use vulkano_taskgraph::command_buffer::RecordingCommandBuffer;
    pub use vulkano_taskgraph::graph::{NodeId, ResourceMap, TaskGraph};
    pub use vulkano_taskgraph::resource::{AccessType, Flight, HostAccessType};
    pub use vulkano_taskgraph::{execute, Id, QueueFamilyType, Task, TaskContext, TaskResult};
}

use vulkano::buffer::BufferContents;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};

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
            .user_renderer(MyRenderer::new(window))
            .run()
            .unwrap();

        basalt.exit();
    });
}

struct MyRenderer {
    window: Arc<Window>,
    tri_vs_sm: Arc<vk::ShaderModule>,
    tri_fs_sm: Arc<vk::ShaderModule>,
    vertex_buffer_id: Option<vk::Id<vk::Buffer>>,
    target_image_id: Option<vk::Id<vk::Image>>,
    render_pass: Option<Arc<vk::RenderPass>>,
    pipeline: Option<Arc<vk::GraphicsPipeline>>,
    viewport: vk::Viewport,
    framebuffer: Option<Arc<vk::Framebuffer>>,
    vertex_buffer_vid: Option<vk::Id<vk::Buffer>>,
}
impl MyRenderer {
    fn new(window: Arc<Window>) -> Self {
        let tri_vs_sm = triangle_vs::load(window.basalt_ref().device()).unwrap();
        let tri_fs_sm = triangle_fs::load(window.basalt_ref().device()).unwrap();

        Self {
            window,
            tri_vs_sm,
            tri_fs_sm,
            vertex_buffer_id: None,
            target_image_id: None,
            render_pass: None,
            pipeline: None,
            viewport: vk::Viewport {
                offset: [0.0, 0.0],
                extent: [0.0, 0.0],
                depth_range: 0.0..=1.0,
            },
            framebuffer: None,
            vertex_buffer_vid: None,
        }
    }
}

impl UserRenderer for MyRenderer {
    fn initialize(&mut self, flight_id: vk::Id<vk::Flight>) {
        let vertexes = [
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

        let vertex_buffer_id = self
            .window
            .basalt_ref()
            .device_resources_ref()
            .create_buffer(
                vk::BufferCreateInfo {
                    usage: vk::BufferUsage::VERTEX_BUFFER,
                    ..Default::default()
                },
                vk::AllocationCreateInfo {
                    memory_type_filter: vk::MemoryTypeFilter::PREFER_DEVICE
                        | vk::MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                vk::DeviceLayout::new_unsized::<[TriangleVertex]>(3).unwrap(),
            )
            .unwrap();

        unsafe {
            vk::execute(
                self.window.basalt_ref().graphics_queue_ref(),
                self.window.basalt_ref().device_resources_ref(),
                flight_id,
                |_, task_context| {
                    task_context
                        .write_buffer::<[TriangleVertex]>(vertex_buffer_id, ..)?
                        .copy_from_slice(&vertexes);
                    Ok(())
                },
                [(vertex_buffer_id, vk::HostAccessType::Write)],
                [],
                [],
            )
            .unwrap()
        }

        self.vertex_buffer_id = Some(vertex_buffer_id);
    }

    fn target_changed(&mut self, target_image_id: vk::Id<vk::Image>) {
        let target_image_state = self
            .window
            .basalt_ref()
            .device_resources_ref()
            .image(target_image_id)
            .unwrap();

        if self.render_pass.is_none() {
            self.render_pass = Some(
                vulkano::single_pass_renderpass!(
                    self.window.basalt_ref().device(),
                    attachments: {
                        color: {
                            format: target_image_state.image().format(),
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
                vk::PipelineShaderStageCreateInfo::new(tri_vs_entry),
                vk::PipelineShaderStageCreateInfo::new(tri_fs_entry),
            ];

            let layout = vk::PipelineLayout::new(
                self.window.basalt_ref().device(),
                vk::PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
                    .into_pipeline_layout_create_info(self.window.basalt_ref().device())
                    .unwrap(),
            )
            .unwrap();

            let subpass = vk::Subpass::from(self.render_pass.clone().unwrap(), 0).unwrap();

            self.pipeline = Some(
                vk::GraphicsPipeline::new(
                    self.window.basalt_ref().device(),
                    None,
                    vk::GraphicsPipelineCreateInfo {
                        stages: stages.into_iter().collect(),
                        vertex_input_state: Some(vertex_input_state),
                        input_assembly_state: Some(Default::default()),
                        viewport_state: Some(Default::default()),
                        rasterization_state: Some(Default::default()),
                        multisample_state: Some(Default::default()),
                        color_blend_state: Some(vk::ColorBlendState::with_attachment_states(
                            subpass.num_color_attachments(),
                            Default::default(),
                        )),
                        dynamic_state: [vk::DynamicState::Viewport].into_iter().collect(),
                        subpass: Some(subpass.into()),
                        ..vk::GraphicsPipelineCreateInfo::layout(layout)
                    },
                )
                .unwrap(),
            );
        }

        self.framebuffer = Some(
            vk::Framebuffer::new(
                self.render_pass.clone().unwrap(),
                vk::FramebufferCreateInfo {
                    attachments: vec![vk::ImageView::new_default(
                        target_image_state.image().clone(),
                    )
                    .unwrap()],
                    ..Default::default()
                },
            )
            .unwrap(),
        );

        let [width, height, _] = target_image_state.image().extent();
        self.viewport.extent = [width as f32, height as f32];
        self.target_image_id = Some(target_image_id);
    }

    fn task_graph_info(&mut self) -> UserTaskGraphInfo {
        UserTaskGraphInfo {
            // The target image provided by target_changed() does not count toward user resources,
            // so the only resource that is added in this example is the vertex buffer.
            max_resources: 1,
            max_nodes: 1,
            ..Default::default()
        }
    }

    fn task_graph_build(
        &mut self,
        task_graph: &mut vk::TaskGraph<RendererContext>,
        _target_image_vid: vk::Id<vk::Image>,
    ) -> vk::NodeId {
        let vertex_buffer_vid = task_graph.add_buffer(&vk::BufferCreateInfo {
            usage: vk::BufferUsage::VERTEX_BUFFER,
            ..Default::default()
        });

        let mut node =
            task_graph.create_task_node("triangle", vk::QueueFamilyType::Graphics, TriangleTask);

        self.vertex_buffer_vid = Some(vertex_buffer_vid);
        node.buffer_access(vertex_buffer_vid, vk::AccessType::VertexAttributeRead);
        node.build()
    }

    fn task_graph_resources(&mut self, resource_map: &mut vk::ResourceMap) {
        resource_map
            .insert_buffer(
                self.vertex_buffer_vid.unwrap(),
                self.vertex_buffer_id.unwrap(),
            )
            .unwrap();
    }
}

struct TriangleTask;

impl vk::Task for TriangleTask {
    type World = RendererContext;

    unsafe fn execute(
        &self,
        cmd: &mut vk::RecordingCommandBuffer<'_>,
        _task: &mut vk::TaskContext<'_>,
        context: &Self::World,
    ) -> vk::TaskResult {
        let renderer = context.user_renderer::<MyRenderer>().unwrap();
        let framebuffer = renderer.framebuffer.clone().unwrap();
        let pipeline = renderer.pipeline.as_ref().unwrap();

        cmd.as_raw().begin_render_pass(
            &vk::RenderPassBeginInfo {
                clear_values: vec![Some(vk::ClearValue::Float([0.0, 0.0, 1.0, 1.0]))],
                ..vk::RenderPassBeginInfo::framebuffer(framebuffer.clone())
            },
            &Default::default(),
        )?;

        cmd.destroy_objects(iter::once(framebuffer));
        cmd.set_viewport(0, slice::from_ref(&renderer.viewport))?;
        cmd.bind_pipeline_graphics(pipeline)?;
        cmd.bind_vertex_buffers(0, &[renderer.vertex_buffer_vid.unwrap()], &[0], &[], &[])?;

        unsafe {
            cmd.draw(3, 1, 0, 0)?;
        }

        cmd.as_raw().end_render_pass(&Default::default())?;
        Ok(())
    }
}

#[derive(BufferContents, Vertex, Clone, Copy)]
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
