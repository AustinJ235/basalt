//! Window rendering

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

pub use amwr::AutoMultiWindowRenderer;
use cosmic_text::{FontSystem, SwashCache};
use flume::Receiver;
use vulkano::buffer::Subbuffer;
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, ClearColorImageInfo, CommandBufferUsage, PrimaryAutoCommandBuffer,
    PrimaryCommandBufferAbstract,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::layout::DescriptorSetLayout;
use vulkano::descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet};
use vulkano::device::Queue;
use vulkano::format::{Format, FormatFeatures, NumericFormat};
use vulkano::image::sampler::{Sampler, SamplerAddressMode, SamplerCreateInfo};
use vulkano::image::sys::ImageCreateInfo;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryAllocatePreference, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::memory::MemoryPropertyFlags;
use vulkano::pipeline::graphics::viewport::Viewport;
use vulkano::swapchain::{
    self, ColorSpace, FullScreenExclusive, PresentGravity, PresentGravityFlags, PresentMode,
    PresentScaling, PresentScalingFlags, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo,
    Win32Monitor,
};
use vulkano::sync::future::{FenceSignalFuture, GpuFuture};
use vulkano::VulkanError;
pub use worker::WorkerPerfMetrics;

use self::draw::DrawState;
use crate::image_cache::ImageCacheKey;
use crate::interface::{BinID, BinPlacement, DefaultFont, ItfVertInfo};
use crate::window::Window;

mod amwr;
mod draw;
mod shaders;
mod worker;

/// Used to specify the MSAA sample count of the ui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MSAA {
    X1,
    X2,
    X4,
    X8,
}

/// Used to specify if VSync should be enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VSync {
    Enable,
    Disable,
}

/// Trait used for user provided renderers.
pub trait UserRenderer {
    /// Called everytime a change occurs that results in the target image changing.
    fn target_changed(&mut self, target_image: Arc<ImageView>);
    /// Called everytime a draw is requested on to the provided target image.
    fn draw(&mut self, cmd_builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>);
}

pub(crate) struct UpdateContext {
    pub extent: [f32; 2],
    pub scale: f32,
    pub font_system: FontSystem,
    pub glyph_cache: SwashCache,
    pub default_font: DefaultFont,
    pub metrics_level: RendererMetricsLevel,
    pub placement_cache: BTreeMap<BinID, BinPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) enum ImageSource {
    #[default]
    None,
    Cache(ImageCacheKey),
    Vulkano(Arc<Image>),
}

enum RenderEvent {
    Redraw,
    Update {
        buffer: Subbuffer<[ItfVertInfo]>,
        images: Vec<Arc<Image>>,
        barrier: Arc<Barrier>,
        metrics: Option<WorkerPerfMetrics>,
    },
    Resize,
    SetMSAA(MSAA),
    SetVSync(VSync),
    SetMetrics(RendererMetricsLevel),
    WindowFullscreenEnabled,
    WindowFullscreenDisabled,
}

/// Performance metrics of a `Renderer`.
#[derive(Debug, Clone, Default)]
pub struct RendererPerfMetrics {
    pub total_frames: usize,
    pub total_updates: usize,
    pub avg_cpu_time: f32,
    pub avg_frame_rate: f32,
    pub avg_update_rate: f32,
    pub avg_worker_metrics: Option<WorkerPerfMetrics>,
}

/// Defines the level of metrics tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RendererMetricsLevel {
    /// No Metrics
    None,
    /// Renderer Metrics
    Basic,
    /// Renderer Metrics & Worker Metrics
    Extended,
    /// Renderer Metrics, Worker Metrics, & OVD Metrics
    ///
    /// ***Note:** This level may impact performance.*
    Full,
}

struct MetricsState {
    state_begin: Instant,
    last_acquire: Instant,
    last_update: Instant,
    cpu_times: Vec<f32>,
    gpu_times: Vec<f32>,
    update_times: Vec<f32>,
    worker_metrics: Vec<WorkerPerfMetrics>,
}

impl MetricsState {
    fn new() -> Self {
        let inst = Instant::now();

        Self {
            state_begin: inst,
            last_acquire: inst,
            last_update: inst,
            cpu_times: Vec::new(),
            gpu_times: Vec::new(),
            update_times: Vec::new(),
            worker_metrics: Vec::new(),
        }
    }

    fn track_acquire(&mut self) {
        self.gpu_times
            .push(self.last_acquire.elapsed().as_micros() as f32 / 1000.0);
        self.last_acquire = Instant::now();
    }

    fn track_present(&mut self) {
        self.cpu_times
            .push(self.last_acquire.elapsed().as_micros() as f32 / 1000.0);
    }

    fn track_update(&mut self, worker_metrics_op: Option<WorkerPerfMetrics>) {
        self.update_times
            .push(self.last_update.elapsed().as_micros() as f32 / 1000.0);
        self.last_update = Instant::now();

        if let Some(worker_metrics) = worker_metrics_op {
            self.worker_metrics.push(worker_metrics);
        }
    }

    fn tracked_time(&self) -> Duration {
        self.state_begin.elapsed()
    }

    fn complete(&mut self) -> RendererPerfMetrics {
        let (total_updates, avg_update_rate, avg_worker_metrics) = if !self.update_times.is_empty()
        {
            let avg_worker_metrics = if !self.worker_metrics.is_empty() {
                let mut total_worker_metrics = WorkerPerfMetrics::default();
                let count = self.worker_metrics.len();

                for worker_metrics in self.worker_metrics.drain(..) {
                    total_worker_metrics += worker_metrics;
                }

                total_worker_metrics /= count as f32;
                Some(total_worker_metrics)
            } else {
                None
            };

            (
                self.update_times.len(),
                1000.0 / (self.update_times.iter().sum::<f32>() / self.update_times.len() as f32),
                avg_worker_metrics,
            )
        } else {
            (0, 0.0, None)
        };

        let (total_frames, avg_cpu_time, avg_frame_rate) = if !self.gpu_times.is_empty() {
            (
                self.gpu_times.len(),
                self.cpu_times.iter().sum::<f32>() / self.cpu_times.len() as f32,
                1000.0 / (self.gpu_times.iter().sum::<f32>() / self.gpu_times.len() as f32),
            )
        } else {
            (0, 0.0, 0.0)
        };

        *self = Self::new();

        RendererPerfMetrics {
            total_updates,
            avg_update_rate,
            avg_worker_metrics,
            total_frames,
            avg_cpu_time,
            avg_frame_rate,
        }
    }
}

/// Provides rendering for a window.
pub struct Renderer {
    window: Arc<Window>,
    render_event_recv: Receiver<RenderEvent>,
    surface_format: Format,
    surface_colorspace: ColorSpace,
    fullscreen_mode: FullScreenExclusive,
    win32_monitor: Option<Win32Monitor>,
    queue: Arc<Queue>,
    cmd_alloc: StandardCommandBufferAllocator,
    mem_alloc: Arc<StandardMemoryAllocator>,
    desc_alloc: StandardDescriptorSetAllocator,
    desc_image_capacity: u32,
    desc_layout: Option<Arc<DescriptorSetLayout>>,
    sampler: Arc<Sampler>,
    default_image: Arc<ImageView>,
    draw_state: Option<DrawState>,
}

impl Renderer {
    /// Create a new `Renderer` given a window.
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let (fullscreen_mode, win32_monitor) = match window
            .basalt_ref()
            .device_ref()
            .enabled_extensions()
            .ext_full_screen_exclusive
        {
            true => {
                (
                    FullScreenExclusive::ApplicationControlled,
                    window.win32_monitor(),
                )
            },
            false => (FullScreenExclusive::Default, None),
        };

        let mut surface_formats = window.surface_formats(fullscreen_mode);

        /*let ext_swapchain_colorspace = window
        .basalt_ref()
        .instance_ref()
        .enabled_extensions()
        .ext_swapchain_colorspace;*/

        surface_formats.retain(|(format, colorspace)| {
            if !match colorspace {
                ColorSpace::SrgbNonLinear => true,
                // TODO: Support these properly, these are for hdr mainly. Typically the format
                //       is a signed float where values are allowed to be less than zero or greater
                //       one. The main problem currently is that anything that falls in the normal
                //       range don't appear as bright as one would expect on a hdr display.
                // ColorSpace::ExtendedSrgbLinear => ext_swapchain_colorspace,
                // ColorSpace::ExtendedSrgbNonLinear => ext_swapchain_colorspace,
                _ => false,
            } {
                return false;
            }

            // TODO: Support non SRGB formats properly. When writing to a non-SRGB format using the
            //       SrgbNonLinear colorspace, colors written will be assumed to be SRGB. This
            //       causes issues since everything is done with linear color.
            if format.numeric_format_color() != Some(NumericFormat::SRGB) {
                return false;
            }

            true
        });

        surface_formats.sort_by_key(|(format, _colorspace)| format.components()[0]);

        let (surface_format, surface_colorspace) = surface_formats.pop().ok_or(String::from(
            "Unable to find suitable format & colorspace for the swapchain.",
        ))?;

        let image_format = if surface_format.components()[0] > 8 {
            vec![
                Format::R16G16B16A16_UINT,
                Format::R16G16B16A16_UNORM,
                Format::R8G8B8A8_UINT,
                Format::R8G8B8A8_UNORM,
                Format::B8G8R8A8_UINT,
                Format::B8G8R8A8_UNORM,
                Format::A8B8G8R8_UINT_PACK32,
                Format::A8B8G8R8_UNORM_PACK32,
                Format::R8G8B8A8_SRGB,
                Format::B8G8R8A8_SRGB,
                Format::A8B8G8R8_SRGB_PACK32,
            ]
        } else {
            vec![
                Format::R8G8B8A8_UINT,
                Format::R8G8B8A8_UNORM,
                Format::B8G8R8A8_UINT,
                Format::B8G8R8A8_UNORM,
                Format::A8B8G8R8_UINT_PACK32,
                Format::A8B8G8R8_UNORM_PACK32,
                Format::R8G8B8A8_SRGB,
                Format::B8G8R8A8_SRGB,
                Format::A8B8G8R8_SRGB_PACK32,
            ]
        }
        .into_iter()
        .find(|format| {
            let properties = match window
                .basalt_ref()
                .physical_device_ref()
                .format_properties(*format)
            {
                Ok(ok) => ok,
                Err(_) => return false,
            };

            properties.optimal_tiling_features.contains(
                FormatFeatures::TRANSFER_DST
                    | FormatFeatures::TRANSFER_SRC
                    | FormatFeatures::SAMPLED_IMAGE
                    | FormatFeatures::SAMPLED_IMAGE_FILTER_LINEAR,
            )
        })
        .ok_or(String::from("Failed to find suitable image format."))?;

        let window_event_recv = window
            .window_manager_ref()
            .window_event_queue(window.id())
            .ok_or_else(|| String::from("There is already a renderer for this window."))?;

        let (render_event_send, render_event_recv) = flume::unbounded();

        worker::spawn(
            window.clone(),
            window_event_recv,
            render_event_send,
            image_format,
        )?;

        let queue = window.basalt_ref().graphics_queue();

        let cmd_alloc = StandardCommandBufferAllocator::new(
            queue.device().clone(),
            StandardCommandBufferAllocatorCreateInfo {
                primary_buffer_count: 16,
                secondary_buffer_count: 0,
                ..StandardCommandBufferAllocatorCreateInfo::default()
            },
        );

        let mem_alloc = Arc::new(StandardMemoryAllocator::new_default(queue.device().clone()));

        let desc_alloc =
            StandardDescriptorSetAllocator::new(queue.device().clone(), Default::default());

        let default_image = {
            let image = Image::new(
                mem_alloc.clone(),
                ImageCreateInfo {
                    format: image_format,
                    extent: [1; 3],
                    usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
                    ..ImageCreateInfo::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter {
                        preferred_flags: MemoryPropertyFlags::DEVICE_LOCAL,
                        not_preferred_flags: MemoryPropertyFlags::HOST_CACHED,
                        ..MemoryTypeFilter::empty()
                    },
                    allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
                    ..AllocationCreateInfo::default()
                },
            )
            .unwrap();

            let mut cmd_builder = AutoCommandBufferBuilder::primary(
                &cmd_alloc,
                queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .unwrap();

            cmd_builder
                .clear_color_image(ClearColorImageInfo {
                    clear_value: draw::clear_color_value_for_format(image_format),
                    ..ClearColorImageInfo::image(image.clone())
                })
                .unwrap();

            cmd_builder
                .build()
                .unwrap()
                .execute(queue.clone())
                .unwrap()
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();

            ImageView::new_default(image).unwrap()
        };

        let sampler = Sampler::new(
            queue.device().clone(),
            SamplerCreateInfo {
                address_mode: [SamplerAddressMode::ClampToBorder; 3],
                unnormalized_coordinates: true,
                ..SamplerCreateInfo::default()
            },
        )
        .unwrap();

        Ok(Self {
            window,
            render_event_recv,
            surface_format,
            surface_colorspace,
            fullscreen_mode,
            win32_monitor,
            queue,
            cmd_alloc,
            mem_alloc,
            desc_alloc,
            desc_image_capacity: 4,
            desc_layout: None,
            sampler,
            default_image,
            draw_state: None,
        })
    }

    fn create_desc_set(&mut self, images: Vec<Arc<Image>>) -> Arc<PersistentDescriptorSet> {
        if images.len() as u32 > self.desc_image_capacity {
            while self.desc_image_capacity < images.len() as u32 {
                self.desc_image_capacity *= 2;
            }

            self.draw_state
                .as_mut()
                .unwrap()
                .update_image_capacity(self.queue.device().clone(), self.desc_image_capacity);

            if let Some(old_layout) = self.desc_layout.take() {
                self.desc_alloc.clear(&old_layout);
            }
        }

        if self.desc_layout.is_none() {
            self.desc_layout = Some(
                DescriptorSetLayout::new(
                    self.queue.device().clone(),
                    shaders::pipeline_descriptor_set_layout_create_info(self.desc_image_capacity)
                        .set_layouts[0]
                        .clone(),
                )
                .unwrap(),
            );
        }

        let num_default_images = self.desc_image_capacity as usize - images.len();

        PersistentDescriptorSet::new_variable(
            &self.desc_alloc,
            self.desc_layout.as_ref().unwrap().clone(),
            self.desc_image_capacity,
            [
                WriteDescriptorSet::sampler(0, self.sampler.clone()),
                WriteDescriptorSet::image_view_array(
                    1,
                    0,
                    images
                        .into_iter()
                        .map(|image| ImageView::new_default(image).unwrap())
                        .chain((0..num_default_images).map(|_| self.default_image.clone())),
                ),
            ],
            [],
        )
        .unwrap()
    }

    /// This renderer will only render an interface.
    pub fn with_interface_only(mut self) -> Self {
        self.draw_state = Some(DrawState::interface_only(
            self.queue.device().clone(),
            self.surface_format,
            self.desc_image_capacity,
            self.window.renderer_msaa(),
        ));

        self
    }

    /// This renderer will render an interface on top of the user's output.
    pub fn with_user_renderer<R: UserRenderer + Send + 'static>(
        mut self,
        user_renderer: R,
    ) -> Self {
        self.draw_state = Some(DrawState::user(
            self.queue.device().clone(),
            self.surface_format,
            self.desc_image_capacity,
            self.window.renderer_msaa(),
            user_renderer,
        ));

        self
    }

    /// Start running the the renderer.
    pub fn run(mut self) -> Result<(), String> {
        if self.draw_state.is_none() {
            return Err(String::from(
                "One of the methods `with_interface_only` or `with_user_renderer` must be called \
                 before this method.",
            ));
        }
        let (scaling_behavior, present_gravity) = if self
            .queue
            .device()
            .enabled_extensions()
            .ext_swapchain_maintenance1
        {
            let capabilities = self.window.surface_capabilities(self.fullscreen_mode);

            let scaling = if capabilities
                .supported_present_scaling
                .contains(PresentScalingFlags::ONE_TO_ONE)
            {
                Some(PresentScaling::OneToOne)
            } else {
                None
            };

            let gravity = if capabilities.supported_present_gravity[0]
                .contains(PresentGravityFlags::MIN)
                && capabilities.supported_present_gravity[1].contains(PresentGravityFlags::MIN)
            {
                Some([PresentGravity::Min, PresentGravity::Min])
            } else {
                None
            };

            (scaling, gravity)
        } else {
            (None, None)
        };

        let mut swapchain_create_info = SwapchainCreateInfo {
            min_image_count: 2,
            image_format: self.surface_format,
            image_color_space: self.surface_colorspace,
            image_extent: self.window.surface_current_extent(self.fullscreen_mode),
            image_usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_DST,
            present_mode: find_present_mode(
                &self.window,
                self.fullscreen_mode,
                self.window.renderer_vsync(),
            ),
            full_screen_exclusive: self.fullscreen_mode,
            win32_monitor: self.win32_monitor,
            scaling_behavior,
            present_gravity,
            ..SwapchainCreateInfo::default()
        };

        let mut viewport = Viewport {
            offset: [0.0, 0.0],
            extent: [
                swapchain_create_info.image_extent[0] as f32,
                swapchain_create_info.image_extent[1] as f32,
            ],
            depth_range: 0.0..=1.0,
        };

        let window_manager = self.window.window_manager();
        let mut swapchain_op: Option<Arc<Swapchain>> = None;
        let mut swapchain_views_op = None;
        let mut buffer_op = None;
        let mut desc_set_op = None;
        let mut recreate_swapchain = true;
        let mut update_after_acquire_wait = None;
        let conservative_draw = self.window.basalt_ref().config.render_default_consv_draw;
        let mut conservative_draw_ready = true;
        let mut exclusive_fullscreen_acquired = false;
        let mut acquire_exclusive_fullscreen = false;
        let mut release_exclusive_fullscreen = false;
        let mut previous_frame_op: Option<FenceSignalFuture<Box<dyn GpuFuture>>> = None;
        let mut pending_render_events = Vec::new();

        let mut metrics_state_op =
            if self.window.renderer_metrics_level() >= RendererMetricsLevel::Basic {
                Some(MetricsState::new())
            } else {
                None
            };

        'render_loop: loop {
            assert!(update_after_acquire_wait.is_none());

            loop {
                pending_render_events.append(&mut self.render_event_recv.drain().collect());

                if pending_render_events.is_empty() && self.render_event_recv.is_disconnected() {
                    return Ok(());
                }

                for render_event in pending_render_events.drain(..) {
                    match render_event {
                        RenderEvent::Redraw => {
                            conservative_draw_ready = true;
                        },
                        RenderEvent::Update {
                            buffer,
                            images,
                            barrier,
                            metrics,
                        } => {
                            if swapchain_op.is_none()
                                || swapchain_create_info.image_extent == [0; 2]
                            {
                                if let Some(previous_frame) = previous_frame_op.take() {
                                    previous_frame.wait(None).unwrap();
                                }

                                buffer_op = Some(buffer);
                                desc_set_op = Some(self.create_desc_set(images));
                                barrier.wait();
                            } else {
                                update_after_acquire_wait = Some((buffer, images, barrier));
                            }

                            if let Some(metrics_state) = metrics_state_op.as_mut() {
                                metrics_state.track_update(metrics);
                            }

                            conservative_draw_ready = true;
                        },
                        RenderEvent::Resize {
                            ..
                        } => {
                            recreate_swapchain = true;
                            swapchain_create_info.image_extent =
                                self.window.surface_current_extent(self.fullscreen_mode);
                            viewport.extent = [
                                swapchain_create_info.image_extent[0] as f32,
                                swapchain_create_info.image_extent[1] as f32,
                            ];
                            conservative_draw_ready = true;
                        },
                        RenderEvent::SetVSync(vsync) => {
                            let present_mode =
                                find_present_mode(&self.window, self.fullscreen_mode, vsync);

                            if swapchain_create_info.present_mode != present_mode {
                                swapchain_create_info.present_mode = present_mode;
                                recreate_swapchain = true;
                                conservative_draw_ready = true;
                            }
                        },
                        RenderEvent::SetMSAA(msaa) => {
                            let draw_state = self.draw_state.as_mut().unwrap();

                            draw_state.update_msaa(
                                self.queue.device().clone(),
                                self.surface_format,
                                self.desc_image_capacity,
                                msaa,
                            );

                            if let Some(swapchain_views) = swapchain_views_op.clone() {
                                draw_state.update_framebuffers(
                                    &self.mem_alloc,
                                    &self.desc_alloc,
                                    swapchain_views,
                                );
                            }

                            conservative_draw_ready = true;
                        },
                        RenderEvent::SetMetrics(level) => {
                            if level >= RendererMetricsLevel::Basic {
                                if metrics_state_op.is_none() {
                                    metrics_state_op = Some(MetricsState::new());
                                }
                            } else {
                                metrics_state_op = None;
                            }
                        },
                        RenderEvent::WindowFullscreenEnabled => {
                            if self.fullscreen_mode == FullScreenExclusive::ApplicationControlled {
                                acquire_exclusive_fullscreen = true;
                                release_exclusive_fullscreen = false;
                                conservative_draw_ready = true;
                            }
                        },
                        RenderEvent::WindowFullscreenDisabled => {
                            if self.fullscreen_mode == FullScreenExclusive::ApplicationControlled {
                                acquire_exclusive_fullscreen = false;
                                release_exclusive_fullscreen = true;
                                conservative_draw_ready = true;
                            }
                        },
                    }
                }

                if buffer_op.is_none()
                    || swapchain_create_info.image_extent == [0; 2]
                    || (conservative_draw && !conservative_draw_ready)
                {
                    match self.render_event_recv.recv() {
                        Ok(ok) => pending_render_events.push(ok),
                        Err(_) => return Ok(()),
                    }
                } else {
                    break;
                }
            }

            if recreate_swapchain {
                loop {
                    if let Some(previous_frame) = previous_frame_op.take() {
                        previous_frame.wait(None).unwrap();
                    }

                    let swapchain_create_result = match swapchain_op.as_ref() {
                        Some(old_swapchain) => {
                            old_swapchain.recreate(swapchain_create_info.clone())
                        },
                        None => {
                            Swapchain::new(
                                self.queue.device().clone(),
                                self.window.surface(),
                                swapchain_create_info.clone(),
                            )
                        },
                    };

                    let (swapchain, swapchain_images) = match swapchain_create_result
                        .map_err(|e| e.unwrap())
                    {
                        Ok(ok) => ok,
                        Err(VulkanError::InitializationFailed) => {
                            if self.fullscreen_mode == FullScreenExclusive::ApplicationControlled {
                                self.fullscreen_mode = FullScreenExclusive::Default;
                                swapchain_create_info.win32_monitor = None;
                                swapchain_create_info.full_screen_exclusive =
                                    FullScreenExclusive::Default;
                                exclusive_fullscreen_acquired = false;
                                continue;
                            }

                            panic!("Unhandled error: {:?}", VulkanError::InitializationFailed);
                        },
                        Err(e) => panic!("Unhandled error: {:?}", e),
                    };

                    swapchain_op = Some(swapchain);
                    swapchain_views_op = Some(
                        swapchain_images
                            .into_iter()
                            .map(|image| ImageView::new_default(image).unwrap())
                            .collect::<Vec<_>>(),
                    );

                    self.draw_state.as_mut().unwrap().update_framebuffers(
                        &self.mem_alloc,
                        &self.desc_alloc,
                        swapchain_views_op.clone().unwrap(),
                    );

                    recreate_swapchain = false;
                    break;
                }
            } else if let Some(previous_frame) = previous_frame_op.as_mut() {
                previous_frame.cleanup_finished();
            }

            if acquire_exclusive_fullscreen && !exclusive_fullscreen_acquired {
                if swapchain_op
                    .as_ref()
                    .unwrap()
                    .acquire_full_screen_exclusive_mode()
                    .is_ok()
                {
                    exclusive_fullscreen_acquired = true;
                }

                acquire_exclusive_fullscreen = false;
            }

            if release_exclusive_fullscreen && exclusive_fullscreen_acquired {
                let _ = swapchain_op
                    .as_ref()
                    .unwrap()
                    .release_full_screen_exclusive_mode();

                exclusive_fullscreen_acquired = false;
                release_exclusive_fullscreen = false;
            }

            let _draw_guard = window_manager.request_draw();

            let (image_num, suboptimal, acquire_future) = match swapchain::acquire_next_image(
                swapchain_op.as_ref().unwrap().clone(),
                Some(Duration::from_millis(1000)),
            )
            .map_err(|e| e.unwrap())
            {
                Ok(ok) => ok,
                Err(e) => {
                    match e {
                        VulkanError::OutOfDate => recreate_swapchain = true,
                        VulkanError::Timeout => (),
                        VulkanError::FullScreenExclusiveModeLost => {
                            exclusive_fullscreen_acquired = false
                        },
                        _ => panic!("Unhandled error: {:?}", e),
                    }

                    if let Some(previous_frame) = previous_frame_op.take() {
                        previous_frame.wait(None).unwrap();
                    }

                    if let Some((buffer, images, barrier)) = update_after_acquire_wait.take() {
                        buffer_op = Some(buffer);
                        desc_set_op = Some(self.create_desc_set(images));
                        barrier.wait();
                    }

                    continue 'render_loop;
                },
            };

            if suboptimal {
                recreate_swapchain = true;
            }

            acquire_future.wait(None).unwrap();

            if let Some(metrics_state) = metrics_state_op.as_mut() {
                metrics_state.track_acquire();
            }

            if let Some((buffer, images, barrier)) = update_after_acquire_wait.take() {
                buffer_op = Some(buffer);
                desc_set_op = Some(self.create_desc_set(images));
                barrier.wait();
            }

            let mut cmd_builder = AutoCommandBufferBuilder::primary(
                &self.cmd_alloc,
                self.queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .unwrap();

            self.draw_state.as_mut().unwrap().draw(
                buffer_op.as_ref().unwrap().clone(),
                desc_set_op.as_ref().unwrap().clone(),
                image_num as usize,
                viewport.clone(),
                &mut cmd_builder,
            );

            let cmd_buffer = cmd_builder.build().unwrap();

            if let Some(metrics_state) = metrics_state_op.as_mut() {
                metrics_state.track_present();

                if metrics_state.tracked_time() >= Duration::from_secs(1) {
                    self.window.set_renderer_metrics(metrics_state.complete());
                }
            }

            match match previous_frame_op.take() {
                Some(previous_frame) => {
                    previous_frame
                        .join(acquire_future)
                        .then_execute(self.queue.clone(), cmd_buffer)
                        .unwrap()
                        .then_swapchain_present(
                            self.queue.clone(),
                            SwapchainPresentInfo::swapchain_image_index(
                                swapchain_op.as_ref().unwrap().clone(),
                                image_num,
                            ),
                        )
                        .boxed()
                        .then_signal_fence_and_flush()
                        .map_err(|e| e.unwrap())
                },
                None => {
                    acquire_future
                        .then_execute(self.queue.clone(), cmd_buffer)
                        .unwrap()
                        .then_swapchain_present(
                            self.queue.clone(),
                            SwapchainPresentInfo::swapchain_image_index(
                                swapchain_op.as_ref().unwrap().clone(),
                                image_num,
                            ),
                        )
                        .boxed()
                        .then_signal_fence_and_flush()
                        .map_err(|e| e.unwrap())
                },
            } {
                Ok(future) => {
                    conservative_draw_ready = false;
                    previous_frame_op = Some(future);
                },
                Err(VulkanError::OutOfDate) => recreate_swapchain = true,
                Err(e) => panic!("Unhandled error: {:?}", e),
            }
        }
    }
}

fn find_present_mode(
    window: &Arc<Window>,
    fullscreen_mode: FullScreenExclusive,
    vsync: VSync,
) -> PresentMode {
    let mut present_modes = window.surface_present_modes(fullscreen_mode);

    present_modes.retain(|present_mode| {
        matches!(
            present_mode,
            PresentMode::Fifo
                | PresentMode::FifoRelaxed
                | PresentMode::Mailbox
                | PresentMode::Immediate
        )
    });

    present_modes.sort_by_key(|present_mode| {
        match vsync {
            VSync::Enable => {
                match present_mode {
                    PresentMode::Fifo => 3,
                    PresentMode::FifoRelaxed => 2,
                    PresentMode::Mailbox => 1,
                    PresentMode::Immediate => 0,
                    _ => unreachable!(),
                }
            },
            VSync::Disable => {
                match present_mode {
                    PresentMode::Mailbox => 3,
                    PresentMode::Immediate => 2,
                    PresentMode::Fifo => 1,
                    PresentMode::FifoRelaxed => 0,
                    _ => unreachable!(),
                }
            },
        }
    });

    present_modes.pop().unwrap()
}
