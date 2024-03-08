use std::sync::{Arc, Barrier};
use std::time::Duration;

pub use amwr::AutoMultiWindowRenderer;
use cosmic_text::{FontSystem, SwashCache};
use flume::{Receiver, TryRecvError};
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
use vulkano::format::{Format, FormatFeatures};
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
    self, ColorSpace, FullScreenExclusive, PresentGravity, PresentMode, PresentScaling, Swapchain,
    SwapchainCreateInfo, SwapchainPresentInfo, Win32Monitor,
};
use vulkano::sync::future::{FenceSignalFuture, GpuFuture};
use vulkano::VulkanError;

use self::draw::DrawState;
use crate::image_cache::ImageCacheKey;
use crate::interface::{DefaultFont, ItfVertInfo};
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
    },
    Resize {
        width: u32,
        height: u32,
    },
    SetMSAA(MSAA),
    SetVSync(VSync),
    WindowFullscreenEnabled,
    WindowFullscreenDisabled,
}

pub struct Renderer {
    window: Arc<Window>,
    render_event_recv: Receiver<RenderEvent>,
    image_format: Format,
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
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let (fullscreen_mode, win32_monitor) =
            match window.basalt_ref().options_ref().exclusive_fullscreen {
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

        surface_formats.retain(|(_format, colorspace)| {
            match colorspace {
                ColorSpace::SrgbNonLinear => true,
                // TODO: Support these properly
                // ColorSpace::ExtendedSrgbLinear => ext_swapchain_colorspace,
                // ColorSpace::ExtendedSrgbNonLinear => ext_swapchain_colorspace,
                _ => false,
            }
        });

        surface_formats.sort_by_key(|(format, _colorspace)| {
            format.components()[0] + format.components()[1] + format.components()[2]
        });

        let (surface_format, surface_colorspace) = surface_formats.pop().ok_or(String::from(
            "Unable to find suitable format & colorspace for the swapchain.",
        ))?;

        let image_format = if surface_format.components()[0] > 8
            || surface_format.components()[1] > 8
            || surface_format.components()[2] > 8
        {
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
        .filter(|format| {
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
        .next()
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
            image_format,
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

    pub fn run_interface_only(mut self) -> Result<(), String> {
        self.draw_state = Some(DrawState::interface_only(
            self.queue.device().clone(),
            self.surface_format,
            self.desc_image_capacity,
            self.window.renderer_msaa(),
        ));

        self.run()
    }

    pub fn run_with_user_renderer<R: UserRenderer + 'static>(
        mut self,
        user_renderer: R,
    ) -> Result<(), String> {
        self.draw_state = Some(DrawState::user(
            self.queue.device().clone(),
            self.surface_format,
            self.desc_image_capacity,
            self.window.renderer_msaa(),
            user_renderer,
        ));

        self.run()
    }

    fn run(mut self) -> Result<(), String> {
        let (scaling_behavior, present_gravity) = if self
            .queue
            .device()
            .enabled_extensions()
            .ext_swapchain_maintenance1
        {
            (
                Some(PresentScaling::OneToOne),
                Some([PresentGravity::Min, PresentGravity::Min]),
            )
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
            win32_monitor: self.win32_monitor.clone(),
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

        let mut swapchain_op: Option<Arc<Swapchain>> = None;
        let mut swapchain_views_op = None;
        let mut buffer_op = None;
        let mut desc_set_op = None;
        let mut recreate_swapchain = true;
        let mut update_after_acquire_wait = None;
        let conservative_draw = self.window.basalt_ref().options_ref().conservative_draw;
        let mut conservative_draw_ready = true;
        let mut exclusive_fullscreen_acquired = false;
        let mut acquire_exclusive_fullscreen = false;
        let mut release_exclusive_fullscreen = false;
        let mut previous_frame_op: Option<FenceSignalFuture<Box<dyn GpuFuture>>> = None;

        'render_loop: loop {
            assert!(update_after_acquire_wait.is_none());

            loop {
                loop {
                    let render_event = if buffer_op.is_none()
                        || swapchain_create_info.image_extent == [0; 2]
                        || (conservative_draw && !conservative_draw_ready)
                    {
                        match self.render_event_recv.recv() {
                            Ok(ok) => ok,
                            Err(_) => return Ok(()),
                        }
                    } else {
                        match self.render_event_recv.try_recv() {
                            Ok(ok) => ok,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return Ok(()),
                        }
                    };

                    match render_event {
                        RenderEvent::Redraw => {
                            conservative_draw_ready = true;
                        },
                        RenderEvent::Update {
                            buffer,
                            images,
                            barrier,
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

                if buffer_op.is_some() && swapchain_create_info.image_extent != [0; 2] {
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
            } else {
                if let Some(previous_frame) = previous_frame_op.as_mut() {
                    previous_frame.cleanup_finished();
                }
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
