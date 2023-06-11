#![allow(dead_code)]

pub(crate) mod composer;
mod shaders;
pub(crate) mod updater;

use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use composer::{Composer, ComposerView};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, PrimaryAutoCommandBuffer, RenderPassBeginInfo,
    SubpassContents,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::layout::{
    DescriptorSetLayout, DescriptorSetLayoutCreateInfo, DescriptorSetLayoutCreationError,
};
use vulkano::descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet};
use vulkano::image::attachment::AttachmentImage;
use vulkano::image::view::ImageView;
use vulkano::image::ImageUsage;
use vulkano::pipeline::graphics::color_blend::ColorBlendState;
use vulkano::pipeline::graphics::input_assembly::{InputAssemblyState, PrimitiveTopology};
use vulkano::pipeline::graphics::vertex_input::Vertex;
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::layout::PipelineLayoutCreateInfo;
use vulkano::pipeline::{GraphicsPipeline, Pipeline, PipelineBindPoint, PipelineLayout};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, Subpass};
use vulkano::swapchain::{
    acquire_next_image, FullScreenExclusive, Swapchain, SwapchainCreateInfo,
    SwapchainCreationError, SwapchainPresentInfo,
};
use vulkano::sync::future::FenceSignalFuture;
use vulkano::sync::{FlushError, GpuFuture};

use self::shaders::*;
use crate::interface::{BstImageView, ItfVertInfo};
use crate::window::BasaltWindow;

const STAT_WINDOW_SIZE: usize = 16;

pub enum RenderEvent<'a> {
    OutputChanged(Arc<ImageView<AttachmentImage>>),
    DrawRequested(&'a mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>),
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub(crate) enum ImageKey {
    None,
    Atlas(u64),
    Direct(Arc<BstImageView>),
}

impl ImageKey {
    fn sort_key(&self) -> u128 {
        match self {
            Self::None => 0,
            Self::Atlas(i) => *i as u128,
            Self::Direct(img) => {
                let mut hasher = DefaultHasher::new();
                img.hash(&mut hasher);
                u64::MAX as u128 + hasher.finish() as u128
            },
        }
    }
}

impl PartialOrd for ImageKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.sort_key().cmp(&other.sort_key()))
    }
}

impl Ord for ImageKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sort_key().cmp(&other.sort_key())
    }
}

pub struct RenderLoop {
    window: Arc<dyn BasaltWindow>,
    composer: Composer,
}

impl RenderLoop {
    pub fn new(window: Arc<dyn BasaltWindow>) -> Option<Self> {
        let composer = Composer::new(window.clone())?;

        Some(Self {
            window,
            composer,
        })
    }

    pub fn run<F: FnMut(RenderEvent)>(&mut self, method: F) {
        let basalt = self.window.basalt();
        let device = basalt.device();
        let queue = basalt.graphics_queue();

        let cmd_alloc = StandardCommandBufferAllocator::new(device.clone(), Default::default());
        let set_alloc = StandardDescriptorSetAllocator::new(device.clone());

        let ui_vs = ui_vs::load(device.clone()).unwrap();
        let ui_fs = ui_fs::load(device.clone()).unwrap();

        let mut extent = basalt.current_extent(FullScreenExclusive::Default);
        self.composer.set_extent(extent);

        let mut capabilities = basalt.surface_capabilities(FullScreenExclusive::Default);
        let swapchain_format = basalt.formats_in_use().swapchain;
        let swapchain_colorspace = basalt.formats_in_use().swapchain_colorspace;

        let mut swapchain_and_images = {
            let (swapchain, images) = Swapchain::new(
                basalt.device(),
                basalt.surface(),
                SwapchainCreateInfo {
                    min_image_count: capabilities.min_image_count,
                    image_format: Some(swapchain_format),
                    image_color_space: swapchain_colorspace,
                    image_extent: extent,
                    image_usage: ImageUsage::COLOR_ATTACHMENT,
                    present_mode: vulkano::swapchain::PresentMode::Mailbox,
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
        let mut view_image_capacity = 2_u32;
        let mut current_view = self.composer.acquire_view();
        let sampler = basalt.atlas_ref().nearest_sampler();
        let empty_image = basalt.atlas_ref().empty_image();

        let mut frame_times = VecDeque::with_capacity(STAT_WINDOW_SIZE);
        let mut last_report = Instant::now();
        let mut last_present = Instant::now();

        'recreate: loop {
            if recreate_swapchain {
                capabilities = basalt.surface_capabilities(FullScreenExclusive::Default);
                extent = basalt.current_extent(FullScreenExclusive::Default);
                self.composer.set_extent(extent);

                swapchain_and_images = {
                    let (swapchain, images) =
                        match swapchain_and_images.0.recreate(SwapchainCreateInfo {
                            min_image_count: capabilities.min_image_count,
                            image_format: Some(swapchain_format),
                            image_color_space: swapchain_colorspace,
                            image_extent: extent,
                            image_usage: ImageUsage::COLOR_ATTACHMENT,
                            present_mode: vulkano::swapchain::PresentMode::Mailbox,
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

                    recreate_swapchain = false;
                    (swapchain, images)
                };
            }

            let swapchain = &swapchain_and_images.0;
            let sc_images = &swapchain_and_images.1;

            let renderpass = single_pass_renderpass!(
                device.clone(),
                attachments: {
                    color: {
                        load: Clear,
                        store: Store,
                        format: swapchain.image_format(),
                        samples: 1,
                    }
                },
                pass: {
                    color: [color],
                    depth_stencil: {}
                }
            )
            .unwrap();

            let mut image_capacity_changed = false;

            while view_image_capacity < current_view.image_count() {
                view_image_capacity *= 2;
                image_capacity_changed = true;
            }

            if image_capacity_changed {
                set_alloc.clear_all();
            }

            // TODO: Immutable sampler
            let pipeline_layout = {
                let mut layout_create_infos: Vec<_> =
                    DescriptorSetLayoutCreateInfo::from_requirements(
                        ui_fs
                            .entry_point("main")
                            .unwrap()
                            .descriptor_binding_requirements(),
                    );

                let binding = layout_create_infos[0].bindings.get_mut(&0).unwrap();
                binding.variable_descriptor_count = true;
                binding.descriptor_count = view_image_capacity;

                let set_layouts = layout_create_infos
                    .into_iter()
                    .map(|desc| DescriptorSetLayout::new(device.clone(), desc))
                    .collect::<Result<Vec<_>, DescriptorSetLayoutCreationError>>()
                    .unwrap();

                PipelineLayout::new(
                    device.clone(),
                    PipelineLayoutCreateInfo {
                        set_layouts,
                        push_constant_ranges: ui_fs
                            .entry_point("main")
                            .unwrap()
                            .push_constant_requirements()
                            .cloned()
                            .into_iter()
                            .collect(),
                        ..Default::default()
                    },
                )
                .unwrap()
            };

            let mut current_view_set = PersistentDescriptorSet::new_variable(
                &set_alloc,
                pipeline_layout.set_layouts().get(0).unwrap().clone(),
                view_image_capacity,
                [WriteDescriptorSet::image_view_sampler_array(
                    0,
                    0,
                    current_view
                        .images()
                        .into_iter()
                        .chain(
                            (0..(view_image_capacity - current_view.image_count()))
                                .map(|_| empty_image.clone()),
                        )
                        .map(|image| (image as Arc<_>, sampler.clone())),
                )],
            )
            .unwrap();

            // TODO: use pipeline cache
            let pipeline = GraphicsPipeline::start()
                .vertex_input_state(ItfVertInfo::per_vertex())
                .vertex_shader(ui_vs.entry_point("main").unwrap(), ())
                .input_assembly_state(
                    InputAssemblyState::new().topology(PrimitiveTopology::TriangleList),
                )
                .color_blend_state(
                    ColorBlendState::new(
                        Subpass::from(renderpass.clone(), 0)
                            .unwrap()
                            .num_color_attachments(),
                    )
                    .blend_alpha(),
                )
                .viewport_state(ViewportState::viewport_fixed_scissor_irrelevant([
                    Viewport {
                        origin: [0.0; 2],
                        dimensions: [extent[0] as f32, extent[1] as f32],
                        depth_range: 0.0..1.0,
                    },
                ]))
                .fragment_shader(ui_fs.entry_point("main").unwrap(), ())
                .render_pass(Subpass::from(renderpass.clone(), 0).unwrap())
                .with_pipeline_layout(device.clone(), pipeline_layout)
                .unwrap();

            let framebuffers = sc_images
                .iter()
                .map(|sc_image| {
                    Framebuffer::new(
                        renderpass.clone(),
                        FramebufferCreateInfo {
                            attachments: vec![sc_image.clone()],
                            ..Default::default()
                        },
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();

            let mut present_future_op: Option<FenceSignalFuture<Box<dyn GpuFuture>>> = None;
            let mut assoc_views: Vec<Option<Arc<ComposerView>>> = vec![None; sc_images.len()];
            let mut assoc_view_count: HashMap<u64, usize> = HashMap::new();

            loop {
                // TODO: Replace with window events
                if basalt.poll_events().into_iter().any(|ev| {
                    match ev {
                        crate::BstEvent::BstWinEv(crate::BstWinEv::Resized(..)) => true,
                        _ => false,
                    }
                }) {
                    if let Some(mut present_future) = present_future_op.take() {
                        present_future.wait(None).unwrap();
                        present_future.cleanup_finished();
                    }

                    recreate_swapchain = true;
                    continue 'recreate;
                }

                if current_view.is_outdated() {
                    if let Some(updated_view) = self.composer.try_acquire_view() {
                        current_view = updated_view;
                        let mut image_capacity_changed = false;

                        while view_image_capacity < current_view.image_count() {
                            view_image_capacity *= 2;
                            image_capacity_changed = true;
                        }

                        if image_capacity_changed {
                            if let Some(mut present_future) = present_future_op.take() {
                                present_future.wait(None).unwrap();
                                present_future.cleanup_finished();
                            }

                            continue 'recreate;
                        }

                        current_view_set = PersistentDescriptorSet::new_variable(
                            &set_alloc,
                            pipeline.layout().set_layouts().get(0).unwrap().clone(),
                            view_image_capacity,
                            [WriteDescriptorSet::image_view_sampler_array(
                                0,
                                0,
                                current_view
                                    .images()
                                    .into_iter()
                                    .chain(
                                        (0..(view_image_capacity - current_view.image_count()))
                                            .map(|_| empty_image.clone()),
                                    )
                                    .map(|image| (image as Arc<_>, sampler.clone())),
                            )],
                        )
                        .unwrap();
                    }
                }

                if let Some(present_future) = present_future_op.as_mut() {
                    present_future.cleanup_finished();
                }

                let (image_num, sub_optimal, acquire_future) =
                    match acquire_next_image(swapchain.clone(), None) {
                        Ok(ok) => ok,
                        Err(_) => {
                            recreate_swapchain = true;
                            continue 'recreate;
                        },
                    };

                let image_num_us = image_num as usize;

                if let Some(assoc_view_id) = assoc_views[image_num_us]
                    .as_ref()
                    .map(|assoc_view| assoc_view.id())
                {
                    if assoc_view_id != current_view.id() {
                        let view_count = assoc_view_count.get_mut(&assoc_view_id).unwrap();
                        *view_count -= 1;

                        if *view_count == 0 {
                            assoc_view_count.remove(&assoc_view_id);
                            acquire_future.wait(None).unwrap();
                        }

                        assoc_views[image_num_us] = None;
                    }
                }

                if assoc_views[image_num_us].is_none() {
                    assoc_views[image_num_us] = Some(current_view.clone());
                    *assoc_view_count.entry(current_view.id()).or_insert(0) += 1;
                }

                let mut cmd_buf = AutoCommandBufferBuilder::primary(
                    &cmd_alloc,
                    basalt.graphics_queue_ref().queue_family_index(),
                    CommandBufferUsage::OneTimeSubmit,
                )
                .unwrap();

                let vertex_buffer = current_view.buffer();
                let vertex_count = vertex_buffer.len();

                cmd_buf
                    .begin_render_pass(
                        RenderPassBeginInfo {
                            clear_values: vec![Some([1.0; 4].into())],
                            ..RenderPassBeginInfo::framebuffer(
                                framebuffers[image_num as usize].clone(),
                            )
                        },
                        SubpassContents::Inline,
                    )
                    .unwrap()
                    .bind_pipeline_graphics(pipeline.clone())
                    .bind_descriptor_sets(
                        PipelineBindPoint::Graphics,
                        pipeline.layout().clone(),
                        0,
                        current_view_set.clone(),
                    )
                    .bind_vertex_buffers(0, vertex_buffer)
                    .draw(vertex_count as _, 1, 0, 0)
                    .unwrap()
                    .end_render_pass()
                    .unwrap();

                let acquire_joined_future = match present_future_op.take() {
                    Some(some) => some.join(acquire_future).boxed(),
                    None => acquire_future.boxed(),
                };

                let mut present_future = match acquire_joined_future
                    .then_execute(queue.clone(), cmd_buf.build().unwrap())
                    .unwrap()
                    .then_swapchain_present(
                        queue.clone(),
                        SwapchainPresentInfo::swapchain_image_index(swapchain.clone(), image_num),
                    )
                    .boxed()
                    .then_signal_fence_and_flush()
                {
                    Ok(ok) => ok,
                    Err(FlushError::OutOfDate) => {
                        recreate_swapchain = true;
                        continue 'recreate;
                    },
                    Err(e) => {
                        println!("{:?}", e);
                        recreate_swapchain = true;
                        continue 'recreate;
                    },
                };

                if sub_optimal {
                    present_future.wait(None).unwrap();
                    present_future.cleanup_finished();
                    recreate_swapchain = true;
                    continue 'recreate;
                }

                if basalt.wants_exit() {
                    present_future.wait(None).unwrap();
                    present_future.cleanup_finished();
                    return;
                }

                present_future_op = Some(present_future);
                frame_times.push_back(last_present.elapsed());
                last_present = Instant::now();

                if frame_times.len() > STAT_WINDOW_SIZE {
                    frame_times.pop_front();
                }

                if last_report.elapsed() > Duration::from_secs(1) {
                    let stats = current_view.stats();
                    let frame_rate = 1000.0
                        / ((frame_times.iter().sum::<Duration>() / frame_times.len() as u32)
                            .as_micros() as f64
                            / 1000.0);
                    last_report = Instant::now();

                    println!(
                        "Framerate: {:.1} Hz, Update Rate: {:.1} Hz, View Rate: {:.1} Hz, \
                         Composer Time: {:.1} ms, Updater Time: {:.1} ms",
                        frame_rate,
                        stats.update_rate,
                        stats.send_rate,
                        stats.composer_time,
                        stats.updater_time,
                    );
                }
            }
        }
    }
}
