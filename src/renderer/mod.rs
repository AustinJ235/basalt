use std::collections::VecDeque;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use cosmic_text::{FontSystem, SwashCache};
use flume::{Receiver, TryRecvError};
use vulkano::buffer::Subbuffer;
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, ClearColorImageInfo, CommandBufferUsage, PrimaryCommandBufferAbstract,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::format::{Format, FormatFeatures};
use vulkano::image::sys::ImageCreateInfo;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryAllocatePreference, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::memory::MemoryPropertyFlags;
use vulkano::pipeline::graphics::viewport::Viewport;
use vulkano::swapchain::{
    self, ColorSpace, FullScreenExclusive, PresentMode, Swapchain, SwapchainCreateInfo,
    SwapchainPresentInfo, Win32Monitor,
};
use vulkano::sync::future::{FenceSignalFuture, GpuFuture};
use vulkano::VulkanError;

use self::draw::DrawState;
use crate::image_cache::ImageCacheKey;
use crate::interface::{DefaultFont, ItfVertInfo};
use crate::window::Window;

mod draw;
mod shaders;
mod worker;

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
    WindowFullscreenEnabled,
    WindowFullscreenDisabled,
}

pub struct Renderer {
    window: Arc<Window>,
    render_event_recv: Receiver<RenderEvent>,
    pending_events: VecDeque<RenderEvent>,
    image_format: Format,
    surface_format: Format,
    surface_colorspace: ColorSpace,
    fullscreen_mode: FullScreenExclusive,
    win32_monitor: Option<Win32Monitor>,
    image_capacity: u32,
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

        surface_formats.retain(|(_format, colorspace)| {
            match colorspace {
                ColorSpace::SrgbNonLinear => true,
                // TODO: These require an extention
                // ColorSpace::ExtendedSrgbLinear => true,
                // ColorSpace::ExtendedSrgbNonLinear => true,
                _ => false,
            }
        });

        surface_formats.sort_by_key(|(format, _colorspace)| {
            format.components()[0] + format.components()[1] + format.components()[2]
        });

        let (surface_format, surface_colorspace) = surface_formats.pop().ok_or(String::from(
            "Unable to find suitable format & colorspace for the swapchain.",
        ))?;

        let image_format = [
            Format::R16G16B16A16_UINT,
            Format::R16G16B16A16_UNORM,
            Format::R12X4G12X4B12X4A12X4_UNORM_4PACK16,
            Format::R10X6G10X6B10X6A10X6_UNORM_4PACK16,
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

        Ok(Self {
            window,
            render_event_recv,
            pending_events: VecDeque::new(),
            image_format,
            surface_format,
            surface_colorspace,
            fullscreen_mode,
            win32_monitor,
            image_capacity: 4,
            draw_state: None,
        })
    }

    fn wait_for_resize(&mut self) -> Result<bool, String> {
        loop {
            match self.render_event_recv.recv() {
                Ok(RenderEvent::Resize {
                    ..
                }) => {
                    return Ok(false);
                },
                Ok(event) => self.pending_events.push_back(event),
                Err(_) => return Ok(true),
            }
        }
    }

    fn check_image_capacity(&mut self, image_len: usize) {
        if image_len as u32 > self.image_capacity {
            while self.image_capacity < image_len as u32 {
                self.image_capacity *= 2;
            }

            self.draw_state
                .as_mut()
                .unwrap()
                .update_image_capacity(self.window.basalt_ref().device(), self.image_capacity);
        }
    }

    pub fn run_interface_only(&mut self) -> Result<(), String> {
        self.draw_state = Some(DrawState::interface_only(
            self.window.basalt_ref().device(),
            self.surface_format,
            self.image_capacity,
        ));

        self.run()
    }

    pub fn run_with_user_renderer<R: UserRenderer>(
        &mut self,
        _user_renderer: R,
    ) -> Result<(), String> {
        todo!()
    }

    fn run(&mut self) -> Result<(), String> {
        let cmd_alloc = StandardCommandBufferAllocator::new(
            self.window.basalt_ref().device(),
            StandardCommandBufferAllocatorCreateInfo {
                primary_buffer_count: 16,
                secondary_buffer_count: 0,
                ..StandardCommandBufferAllocatorCreateInfo::default()
            },
        );

        let mem_alloc = Arc::new(StandardMemoryAllocator::new_default(
            self.window.basalt_ref().device(),
        ));

        let desc_alloc = StandardDescriptorSetAllocator::new(
            self.window.basalt_ref().device(),
            Default::default(),
        );

        let queue = self.window.basalt_ref().graphics_queue();

        let (mut buffer, images) = loop {
            match self.render_event_recv.recv() {
                Ok(RenderEvent::Update {
                    buffer,
                    images,
                    barrier,
                }) => {
                    barrier.wait();
                    break (buffer, images);
                },
                Ok(event) => self.pending_events.push_back(event),
                Err(_) => return Ok(()),
            }
        };

        let default_image = {
            let image = Image::new(
                mem_alloc.clone(),
                ImageCreateInfo {
                    format: self.image_format,
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

            // TODO: Make sure the numeric type is correct

            cmd_builder
                .clear_color_image(ClearColorImageInfo {
                    clear_value: draw::clear_color_value_for_format(self.image_format),
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

        self.check_image_capacity(images.len());

        let mut desc_set = shaders::create_desc_set(
            self.window.basalt_ref().device(),
            &desc_alloc,
            self.image_capacity,
            images,
            default_image.clone(),
        );

        let mut swapchain_create_info = SwapchainCreateInfo {
            min_image_count: 2,
            image_format: self.surface_format,
            image_color_space: self.surface_colorspace,
            image_extent: self.window.surface_current_extent(self.fullscreen_mode),
            image_usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_DST,
            present_mode: PresentMode::Fifo,
            full_screen_exclusive: self.fullscreen_mode,
            win32_monitor: self.win32_monitor.clone(),
            ..SwapchainCreateInfo::default()
        };

        while swapchain_create_info.image_extent == [0; 2] {
            if self.wait_for_resize()? {
                return Ok(());
            }

            swapchain_create_info.image_extent =
                self.window.surface_current_extent(self.fullscreen_mode);
        }

        let (mut swapchain, swapchain_images) = match Swapchain::new(
            self.window.basalt_ref().device(),
            self.window.surface(),
            swapchain_create_info.clone(),
        )
        .map_err(|e| e.unwrap())
        {
            Ok(ok) => ok,
            Err(VulkanError::InitializationFailed) => {
                if self.fullscreen_mode == FullScreenExclusive::ApplicationControlled {
                    self.fullscreen_mode = FullScreenExclusive::Default;
                    swapchain_create_info.win32_monitor = None;
                    swapchain_create_info.full_screen_exclusive = FullScreenExclusive::Default;

                    match Swapchain::new(
                        self.window.basalt_ref().device(),
                        self.window.surface(),
                        swapchain_create_info.clone(),
                    )
                    .map_err(|e| e.unwrap())
                    {
                        Ok(ok) => ok,
                        Err(e) => panic!("Unhandled error: {:?}", e),
                    }
                } else {
                    panic!("Unhandled error: {:?}", VulkanError::InitializationFailed);
                }
            },
            Err(e) => panic!("Unhandled error: {:?}", e),
        };

        let swapchain_image_extent = swapchain_images[0].extent();

        let mut viewport = Viewport {
            offset: [0.0, 0.0],
            extent: [
                swapchain_image_extent[0] as f32,
                swapchain_image_extent[1] as f32,
            ],
            depth_range: 0.0..=1.0,
        };

        self.draw_state.as_mut().unwrap().update_framebuffers(
            mem_alloc.clone(),
            swapchain_images
                .into_iter()
                .map(|image| ImageView::new_default(image).unwrap())
                .collect::<Vec<_>>(),
        );

        let mut recreate_swapchain = false;
        let mut previous_frame_op: Option<FenceSignalFuture<Box<dyn GpuFuture>>> = None;

        'render_loop: loop {
            if recreate_swapchain {
                if let Some(previous_frame) = previous_frame_op.take() {
                    previous_frame.wait(None).unwrap();
                }

                swapchain_create_info.image_extent =
                    self.window.surface_current_extent(self.fullscreen_mode);

                while swapchain_create_info.image_extent == [0; 2] {
                    if self.wait_for_resize()? {
                        return Ok(());
                    }

                    swapchain_create_info.image_extent =
                        self.window.surface_current_extent(self.fullscreen_mode);
                }

                if self.fullscreen_mode == FullScreenExclusive::ApplicationControlled {
                    self.fullscreen_mode = FullScreenExclusive::Default;
                    swapchain_create_info.win32_monitor = None;
                    swapchain_create_info.full_screen_exclusive = FullScreenExclusive::Default;
                }

                let (new_swapchain, swapchain_images) = match swapchain
                    .recreate(swapchain_create_info.clone())
                    .map_err(|e| e.unwrap())
                {
                    Ok(ok) => ok,
                    Err(VulkanError::InitializationFailed) => {
                        if self.fullscreen_mode == FullScreenExclusive::ApplicationControlled {
                            self.fullscreen_mode = FullScreenExclusive::Default;
                            swapchain_create_info.win32_monitor = None;
                            swapchain_create_info.full_screen_exclusive =
                                FullScreenExclusive::Default;
                            continue;
                        }

                        panic!("Unhandled error: {:?}", VulkanError::InitializationFailed);
                    },
                    Err(e) => panic!("Unhandled error: {:?}", e),
                };

                swapchain = new_swapchain;
                let swapchain_image_extent = swapchain_images[0].extent();

                viewport.extent = [
                    swapchain_image_extent[0] as f32,
                    swapchain_image_extent[1] as f32,
                ];

                self.draw_state.as_mut().unwrap().update_framebuffers(
                    mem_alloc.clone(),
                    swapchain_images
                        .into_iter()
                        .map(|image| ImageView::new_default(image).unwrap())
                        .collect::<Vec<_>>(),
                );

                recreate_swapchain = false;
            } else {
                if let Some(previous_frame) = previous_frame_op.as_mut() {
                    previous_frame.cleanup_finished();
                }
            }

            let mut update = None;
            let mut exclusive_fullscreen_acquired = false;

            loop {
                let render_event = match self.pending_events.pop_front() {
                    Some(some) => some,
                    None => {
                        match self.render_event_recv.try_recv() {
                            Ok(ok) => ok,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return Ok(()),
                        }
                    },
                };

                match render_event {
                    RenderEvent::Redraw => (), // TODO: used for conservative draw
                    RenderEvent::Update {
                        buffer: new_buffer,
                        images: new_images,
                        barrier,
                    } => {
                        update = Some((new_buffer, new_images, barrier));
                    },
                    RenderEvent::Resize {
                        ..
                    } => {
                        recreate_swapchain = true;
                        break;
                    },
                    RenderEvent::WindowFullscreenEnabled => {
                        if self.fullscreen_mode == FullScreenExclusive::ApplicationControlled {
                            if !exclusive_fullscreen_acquired {
                                if swapchain.acquire_full_screen_exclusive_mode().is_ok() {
                                    exclusive_fullscreen_acquired = true;
                                }
                            }
                        }
                    },
                    RenderEvent::WindowFullscreenDisabled => {
                        if self.fullscreen_mode == FullScreenExclusive::ApplicationControlled {
                            if exclusive_fullscreen_acquired {
                                let _ = swapchain.release_full_screen_exclusive_mode();
                                exclusive_fullscreen_acquired = false;
                            }
                        }
                    },
                }
            }

            if recreate_swapchain {
                if let Some(previous_frame) = previous_frame_op.take() {
                    previous_frame.wait(None).unwrap();
                }

                if let Some((new_buffer, images, barrier)) = update.take() {
                    buffer = new_buffer;
                    self.check_image_capacity(images.len());

                    desc_set = shaders::create_desc_set(
                        self.window.basalt_ref().device(),
                        &desc_alloc,
                        self.image_capacity,
                        images,
                        default_image.clone(),
                    );

                    barrier.wait();
                }

                continue 'render_loop;
            }

            let (image_num, suboptimal, acquire_future) = match swapchain::acquire_next_image(
                swapchain.clone(),
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

                    if let Some((new_buffer, images, barrier)) = update.take() {
                        buffer = new_buffer;
                        self.check_image_capacity(images.len());

                        desc_set = shaders::create_desc_set(
                            self.window.basalt_ref().device(),
                            &desc_alloc,
                            self.image_capacity,
                            images,
                            default_image.clone(),
                        );

                        barrier.wait();
                    }

                    continue 'render_loop;
                },
            };

            if suboptimal {
                recreate_swapchain = true;
            }

            acquire_future.wait(None).unwrap();

            if let Some((new_buffer, images, barrier)) = update.take() {
                buffer = new_buffer;
                self.check_image_capacity(images.len());

                desc_set = shaders::create_desc_set(
                    self.window.basalt_ref().device(),
                    &desc_alloc,
                    self.image_capacity,
                    images,
                    default_image.clone(),
                );

                barrier.wait();
            }

            let mut cmd_builder = AutoCommandBufferBuilder::primary(
                &cmd_alloc,
                queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .unwrap();

            self.draw_state.as_mut().unwrap().draw(
                buffer.clone(),
                desc_set.clone(),
                image_num as usize,
                viewport.clone(),
                &mut cmd_builder,
            );

            let cmd_buffer = cmd_builder.build().unwrap();

            match match previous_frame_op.take() {
                Some(previous_frame) => {
                    previous_frame
                        .join(acquire_future)
                        .then_execute(queue.clone(), cmd_buffer)
                        .unwrap()
                        .then_swapchain_present(
                            queue.clone(),
                            SwapchainPresentInfo::swapchain_image_index(
                                swapchain.clone(),
                                image_num,
                            ),
                        )
                        .boxed()
                        .then_signal_fence_and_flush()
                        .map_err(|e| e.unwrap())
                },
                None => {
                    acquire_future
                        .then_execute(queue.clone(), cmd_buffer)
                        .unwrap()
                        .then_swapchain_present(
                            queue.clone(),
                            SwapchainPresentInfo::swapchain_image_index(
                                swapchain.clone(),
                                image_num,
                            ),
                        )
                        .boxed()
                        .then_signal_fence_and_flush()
                        .map_err(|e| e.unwrap())
                },
            } {
                Ok(future) => previous_frame_op = Some(future),
                Err(VulkanError::OutOfDate) => recreate_swapchain = true,
                Err(e) => panic!("Unhandled error: {:?}", e),
            }
        }
    }
}

pub trait UserRenderer {
    fn surface_changed(&mut self, target_image: Arc<Image>);
    fn draw_requested(&mut self, command_buffer: u8);
}
