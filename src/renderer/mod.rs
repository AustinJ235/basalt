use std::collections::VecDeque;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use cosmic_text::{FontSystem, SwashCache};
use flume::{Receiver, TryRecvError};
use vulkano::buffer::Subbuffer;
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::{AutoCommandBufferBuilder, ClearColorImageInfo, CommandBufferUsage};
use vulkano::format::{ClearColorValue, Format};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageUsage};
use vulkano::swapchain::{
    self, ColorSpace, FullScreenExclusive, PresentMode, Swapchain, SwapchainCreateInfo,
    SwapchainPresentInfo, Win32Monitor,
};
use vulkano::sync::future::{FenceSignalFuture, GpuFuture};
use vulkano::VulkanError;

use crate::image_cache::ImageCacheKey;
use crate::interface::{DefaultFont, ItfVertInfo};
use crate::window::Window;

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
    surface_format: Format,
    surface_colorspace: ColorSpace,
    fullscreen_mode: FullScreenExclusive,
    win32_monitor: Option<Win32Monitor>,
    pending_events: VecDeque<RenderEvent>,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let window_event_recv = window
            .window_manager_ref()
            .window_event_queue(window.id())
            .ok_or_else(|| String::from("There is already a renderer for this window."))?;

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

        let (render_event_send, render_event_recv) = flume::unbounded();

        // TODO: Worker should only use 10b+ formats when swapchain format is higher than 8b.
        worker::spawn(window.clone(), window_event_recv, render_event_send)?;

        Ok(Self {
            window,
            render_event_recv,
            surface_format,
            surface_colorspace,
            fullscreen_mode,
            win32_monitor,
            pending_events: VecDeque::new(),
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

    pub fn run_interface_only(&mut self) -> Result<(), String> {
        let (mut buffer, mut images) = loop {
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

        let cmd_alloc = StandardCommandBufferAllocator::new(
            self.window.basalt_ref().device(),
            StandardCommandBufferAllocatorCreateInfo {
                primary_buffer_count: 16,
                secondary_buffer_count: 0,
                ..StandardCommandBufferAllocatorCreateInfo::default()
            },
        );

        let queue = self.window.basalt_ref().graphics_queue();

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

        let (mut swapchain, mut swapchain_images) = match Swapchain::new(
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

        let mut swapchain_views = swapchain_images
            .iter()
            .map(|image| ImageView::new_default(image.clone()).unwrap())
            .collect::<Vec<_>>();

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

                let (new_swapchain, new_swapchain_images) = match swapchain
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
                swapchain_images = new_swapchain_images.clone();

                swapchain_views = new_swapchain_images
                    .into_iter()
                    .map(|image| ImageView::new_default(image).unwrap())
                    .collect::<Vec<_>>();

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
                            }
                        }
                    },
                }
            }

            if recreate_swapchain {
                if let Some(previous_frame) = previous_frame_op.take() {
                    previous_frame.wait(None).unwrap();
                }

                if let Some((new_buffer, new_images, barrier)) = update.take() {
                    buffer = new_buffer;
                    images = new_images;
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

                    if let Some((new_buffer, new_images, barrier)) = update.take() {
                        buffer = new_buffer;
                        images = new_images;
                        barrier.wait();
                    }

                    continue 'render_loop;
                },
            };

            if suboptimal {
                recreate_swapchain = true;
            }

            acquire_future.wait(None).unwrap();

            if let Some((new_buffer, new_images, barrier)) = update.take() {
                buffer = new_buffer;
                images = new_images;
                barrier.wait();
            }

            let mut cmd_builder = AutoCommandBufferBuilder::primary(
                &cmd_alloc,
                queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .unwrap();

            // TODO: Actually draw stuff

            cmd_builder
                .clear_color_image(ClearColorImageInfo {
                    clear_value: ClearColorValue::Uint([0; 4]),
                    ..ClearColorImageInfo::image(swapchain_images[image_num as usize].clone())
                })
                .unwrap();

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

        Ok(())
    }

    pub fn run_with_user_renderer<R: UserRenderer>(
        &mut self,
        user_renderer: R,
    ) -> Result<(), String> {
        todo!()
    }
}

pub trait UserRenderer {
    fn surface_changed(&mut self, target_image: Arc<Image>);
    fn draw_requested(&mut self, command_buffer: u8);
}
