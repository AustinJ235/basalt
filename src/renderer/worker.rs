use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Barrier, Weak};
use std::time::Instant;

use cosmic_text::{FontSystem, SwashCache};
use flume::{Receiver, Sender, TryRecvError};
use guillotiere::{AllocId as AtlasAllocID, AtlasAllocator};
use vulkano::buffer::sys::BufferCreateInfo;
use vulkano::buffer::{Buffer, BufferUsage, Subbuffer};
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::auto::AutoCommandBufferBuilder;
use vulkano::command_buffer::{
    BufferCopy, CommandBufferUsage, CopyBufferInfoTyped, PrimaryCommandBufferAbstract,
};
use vulkano::format::Format as VkFormat;
use vulkano::image::sys::ImageCreateInfo;
use vulkano::image::{Image, ImageType, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryAllocatePreference, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::memory::MemoryPropertyFlags;
use vulkano::sync::GpuFuture;
use vulkano::DeviceSize;

use crate::interface::bin::{Bin, BinID};
use crate::interface::ItfVertInfo;
use crate::renderer::{ImageSource, RenderEvent, UpdateContext};
use crate::window::{Window, WindowEvent};

struct BinState {
    weak: Weak<Bin>,
    vertex_range: Option<Range<DeviceSize>>,
    image_locations: Vec<(ImageSource, usize)>,
    vertex_data: Option<HashMap<ImageSource, Vec<ItfVertInfo>>>,
}

struct AtlasImage {
    contains: HashMap<ImageSource, ContainedImage<AtlasAllocID>>,
    staging_buffers: Vec<Subbuffer<[u8]>>,
    images: Vec<Arc<Image>>,
    allocator: AtlasAllocator,
}

struct DedicatedImage {
    contains: ContainedImage<()>,
    staging_buffers: Vec<Subbuffer<[u8]>>,
    images: Vec<Arc<Image>>,
}

struct ContainedImage<T> {
    data: T,
    use_count: usize,
    last_used: Option<Instant>,
}

enum ImageBacking {
    Atlas(AtlasImage),
    Dedidicated(DedicatedImage),
    UserProvided(Arc<Image>),
}

pub fn spawn(
    window: Arc<Window>,
    window_event_recv: Receiver<WindowEvent>,
    render_event_send: Sender<RenderEvent>,
    _image_format: VkFormat,
) -> Result<(), String> {
    std::thread::spawn(move || {
        let mem_alloc = Arc::new(StandardMemoryAllocator::new_default(
            window.basalt_ref().device(),
        ));

        let cmd_alloc = StandardCommandBufferAllocator::new(
            window.basalt_ref().device(),
            StandardCommandBufferAllocatorCreateInfo {
                primary_buffer_count: 16,
                secondary_buffer_count: 0,
                ..StandardCommandBufferAllocatorCreateInfo::default()
            },
        );

        let queue = window.basalt_ref().transfer_queue();
        let mut window_size = window.inner_dimensions();
        let mut effective_scale = window.effective_interface_scale();
        let mut bin_states: HashMap<BinID, BinState> = HashMap::new();

        for bin in window.associated_bins() {
            bin_states.insert(
                bin.id(),
                BinState {
                    weak: Arc::downgrade(&bin),
                    vertex_range: None,
                    image_locations: Vec::new(),
                    vertex_data: None,
                },
            );
        }

        let mut update_all = true;
        let mut update_bins: Vec<Arc<Bin>> = Vec::new();
        let mut remove_bins: Vec<BinID> = Vec::new();
        let (mut staging_buffers, mut vertex_buffers) =
            create_buffers(&mem_alloc as &Arc<_>, 32768);

        if render_event_send
            .send(RenderEvent::Update {
                buffer: vertex_buffers[1].clone(),
                images: Vec::new(),
                barrier: Arc::new(Barrier::new(1)),
            })
            .is_err()
        {
            return;
        }

        let mut update_context = UpdateContext {
            extent: [window_size[0] as f32, window_size[1] as f32],
            scale: effective_scale,
            font_system: FontSystem::new(), // TODO: Include user fonts
            glyph_cache: SwashCache::new(),
            default_font: window.basalt_ref().interface_ref().default_font(),
        };

        let mut next_cmd_builder_op = None;
        let mut active_index = 0;
        let mut inactive_index = 1;

        'main_loop: loop {
            let mut work_to_do = update_all;

            loop {
                let window_event = match work_to_do && update_context.extent != [0.0; 2] {
                    true => {
                        match window_event_recv.try_recv() {
                            Ok(ok) => ok,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => break 'main_loop,
                        }
                    },
                    false => {
                        match window_event_recv.recv() {
                            Ok(ok) => ok,
                            Err(_) => break 'main_loop,
                        }
                    },
                };

                match window_event {
                    WindowEvent::Opened => (),
                    WindowEvent::Closed => break 'main_loop,
                    WindowEvent::Resized {
                        width,
                        height,
                    } => {
                        if [width, height] != window_size {
                            window_size = [width, height];
                            update_context.extent = [width as f32, height as f32];
                            update_all = true;
                            work_to_do = true;

                            if render_event_send
                                .send(RenderEvent::Resize {
                                    width,
                                    height,
                                })
                                .is_err()
                            {
                                break 'main_loop;
                            }
                        }
                    },
                    WindowEvent::ScaleChanged(new_scale) => {
                        if new_scale != effective_scale {
                            effective_scale = new_scale;
                            update_context.scale = effective_scale;
                            update_all = true;
                            work_to_do = true;
                        }
                    },
                    WindowEvent::RedrawRequested => {
                        if render_event_send.send(RenderEvent::Redraw).is_err() {
                            break 'main_loop;
                        }
                    },
                    WindowEvent::EnabledFullscreen => {
                        if render_event_send
                            .send(RenderEvent::WindowFullscreenEnabled)
                            .is_err()
                        {
                            break 'main_loop;
                        }
                    },
                    WindowEvent::DisabledFullscreen => {
                        if render_event_send
                            .send(RenderEvent::WindowFullscreenDisabled)
                            .is_err()
                        {
                            break 'main_loop;
                        }
                    },
                    WindowEvent::AssociateBin(bin) => {
                        bin_states.insert(
                            bin.id(),
                            BinState {
                                weak: Arc::downgrade(&bin),
                                vertex_range: None,
                                image_locations: Vec::new(),
                                vertex_data: None,
                            },
                        );

                        update_bins.push(bin);
                        work_to_do = true;
                    },
                    WindowEvent::DissociateBin(bin_id) => {
                        remove_bins.push(bin_id);
                        work_to_do = true;
                    },
                    WindowEvent::UpdateBin(bin_id) => {
                        match bin_states
                            .get(&bin_id)
                            .and_then(|bin_state| bin_state.weak.upgrade())
                        {
                            Some(bin) => update_bins.push(bin),
                            None => remove_bins.push(bin_id),
                        }

                        work_to_do = true;
                    },
                }
            }

            // --- Remove Bin States --- //

            let mut move_vertexes = false;

            if update_all {
                update_all = false;
                remove_bins.sort();
                update_bins.clear();

                bin_states.retain(|bin_id, state| {
                    let retain = if remove_bins.binary_search(&bin_id).is_ok() {
                        false
                    } else {
                        match state.weak.upgrade() {
                            Some(bin) => {
                                update_bins.push(bin);
                                true
                            },
                            None => false,
                        }
                    };

                    if retain {
                        state.vertex_data = None;
                        state.vertex_range = None;
                    } else {
                        if state.vertex_range.is_some() {
                            move_vertexes = true;
                        }

                        // TODO: Remove Images
                    }

                    retain
                });

                remove_bins.clear();
            } else {
                for bin_id in remove_bins.drain(..) {
                    if let Some(state) = bin_states.remove(&bin_id) {
                        if state.vertex_range.is_some() {
                            move_vertexes = true;
                        }

                        // TODO: Remove Images
                    }
                }
            }

            // --- Obtain Vertex Data --- //
            // TODO: Threaded

            let updated_bin_count = update_bins.len();

            if updated_bin_count != 0 {
                for bin in update_bins.drain(..) {
                    let state = bin_states.get_mut(&bin.id()).unwrap();
                    state.vertex_range = None;
                    state.vertex_data = Some(bin.obtain_vertex_data(&mut update_context));
                    move_vertexes = true;

                    // TODO: Remove Old Images
                }
            }

            // -- Count Vertexes -- //

            let mut total_vertexes = 0;

            for state in bin_states.values() {
                match &state.vertex_range {
                    Some(vertex_range) => total_vertexes += vertex_range.end - vertex_range.start,
                    None => {
                        match &state.vertex_data {
                            Some(vertex_data) => {
                                for vertexes in vertex_data.values() {
                                    total_vertexes += vertexes.len() as DeviceSize;
                                }
                            },
                            None => (),
                        }
                    },
                }
            }

            // -- Obtain Command Buffer Builders -- //

            let (exec_prev_cmds, mut active_cmd_builder) = match next_cmd_builder_op.take() {
                Some(some) => (true, some),
                None => {
                    (
                        false,
                        AutoCommandBufferBuilder::primary(
                            &cmd_alloc,
                            queue.queue_family_index(),
                            CommandBufferUsage::OneTimeSubmit,
                        )
                        .unwrap(),
                    )
                },
            };

            let mut next_cmd_builder = AutoCommandBufferBuilder::primary(
                &cmd_alloc,
                queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .unwrap();

            // -- Check Buffer Size -- //

            if vertex_buffers[active_index].len() < total_vertexes {
                let mut new_buffer_size = vertex_buffers[active_index].len();

                while new_buffer_size < total_vertexes {
                    new_buffer_size *= 2;
                }

                let (new_staging_buffers, new_vertex_buffers) =
                    create_buffers(&mem_alloc, new_buffer_size);

                active_cmd_builder
                    .copy_buffer(CopyBufferInfoTyped::buffers(
                        vertex_buffers[active_index].clone(),
                        new_vertex_buffers[active_index].clone(),
                    ))
                    .unwrap();

                next_cmd_builder
                    .copy_buffer(CopyBufferInfoTyped::buffers(
                        vertex_buffers[inactive_index].clone(),
                        new_vertex_buffers[inactive_index].clone(),
                    ))
                    .unwrap();

                staging_buffers = new_staging_buffers;
                vertex_buffers = new_vertex_buffers;
            }

            // -- Move already uploaded vertex -- //

            let mut next_vertex_index: DeviceSize = 0;

            if move_vertexes {
                let mut states = bin_states
                    .values_mut()
                    .filter(|state| state.vertex_range.is_some())
                    .collect::<Vec<_>>();

                states.sort_by_key(|state| state.vertex_range.as_ref().unwrap().start);
                let mut copy_regions = Vec::with_capacity(states.len());

                for state in states {
                    let vertex_range = state.vertex_range.as_mut().unwrap();
                    let range_len = vertex_range.end - vertex_range.start;

                    if vertex_range.start == next_vertex_index {
                        next_vertex_index += range_len;
                    } else {
                        let new_range = (vertex_range.start - next_vertex_index)
                            ..(vertex_range.end - next_vertex_index);

                        copy_regions.push(BufferCopy {
                            src_offset: vertex_range.start,
                            dst_offset: new_range.start,
                            size: range_len,
                            ..BufferCopy::default()
                        });

                        *vertex_range = new_range;
                    }
                }

                active_cmd_builder
                    .copy_buffer(CopyBufferInfoTyped {
                        regions: copy_regions.clone().into(),
                        ..CopyBufferInfoTyped::buffers(
                            vertex_buffers[active_index].clone(),
                            vertex_buffers[active_index].clone(),
                        )
                    })
                    .unwrap();

                next_cmd_builder
                    .copy_buffer(CopyBufferInfoTyped {
                        regions: copy_regions.into(),
                        ..CopyBufferInfoTyped::buffers(
                            vertex_buffers[inactive_index].clone(),
                            vertex_buffers[inactive_index].clone(),
                        )
                    })
                    .unwrap();
            }

            // -- Upload new vertexes -- //

            if updated_bin_count != 0 {
                let mut staging_buffer_write = staging_buffers[active_index].write().unwrap();
                let mut next_staging_index: DeviceSize = 0;
                let mut copy_regions = Vec::new();

                for state in bin_states
                    .values_mut()
                    .filter(|state| state.vertex_data.is_some())
                {
                    let src_range_start = next_staging_index;
                    let dst_range_start = next_vertex_index;

                    for (_image_src, mut vertexes) in state.vertex_data.take().unwrap() {
                        if vertexes.is_empty() {
                            continue;
                        }

                        // TODO: images / set tex_i and adjust coords

                        (*staging_buffer_write)[(src_range_start as usize)..][..vertexes.len()]
                            .swap_with_slice(&mut vertexes);
                        next_staging_index += vertexes.len() as DeviceSize;
                        next_vertex_index += vertexes.len() as DeviceSize;
                    }

                    if dst_range_start == next_vertex_index {
                        continue;
                    }

                    state.vertex_range = Some(dst_range_start..next_vertex_index);

                    copy_regions.push(BufferCopy {
                        src_offset: src_range_start,
                        dst_offset: dst_range_start,
                        size: next_staging_index - src_range_start,
                        ..BufferCopy::default()
                    });
                }

                active_cmd_builder
                    .copy_buffer(CopyBufferInfoTyped {
                        regions: copy_regions.clone().into(),
                        ..CopyBufferInfoTyped::buffers(
                            staging_buffers[active_index].clone(),
                            vertex_buffers[active_index].clone(),
                        )
                    })
                    .unwrap();

                next_cmd_builder
                    .copy_buffer(CopyBufferInfoTyped {
                        regions: copy_regions.into(),
                        ..CopyBufferInfoTyped::buffers(
                            staging_buffers[active_index].clone(),
                            vertex_buffers[inactive_index].clone(),
                        )
                    })
                    .unwrap();
            }

            // active cmd builder has something to execute
            if exec_prev_cmds || move_vertexes || updated_bin_count > 0 {
                active_cmd_builder
                    .build()
                    .unwrap()
                    .execute(queue.clone())
                    .unwrap()
                    .then_signal_fence_and_flush()
                    .unwrap()
                    .wait(None)
                    .unwrap();
            }

            // next cmd builder has commands to execute perform a swap
            if move_vertexes || updated_bin_count > 0 {
                next_cmd_builder_op = Some(next_cmd_builder);
                let barrier = Arc::new(Barrier::new(2));

                if render_event_send
                    .send(RenderEvent::Update {
                        buffer: vertex_buffers[active_index]
                            .clone()
                            .slice(0..total_vertexes),
                        images: Vec::new(),
                        barrier: barrier.clone(),
                    })
                    .is_err()
                {
                    break 'main_loop;
                }

                barrier.wait();
                active_index ^= 1;
                inactive_index ^= 1;
            }
        }
    });

    Ok(())
}

fn create_buffers(
    mem_alloc: &Arc<StandardMemoryAllocator>,
    len: DeviceSize,
) -> (Vec<Subbuffer<[ItfVertInfo]>>, Vec<Subbuffer<[ItfVertInfo]>>) {
    let mut staging_buffers = Vec::with_capacity(2);
    let mut vertex_buffers = Vec::with_capacity(2);

    for _ in 0..2 {
        staging_buffers.push(
            Buffer::new_slice::<ItfVertInfo>(
                mem_alloc.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_SRC,
                    ..BufferCreateInfo::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter {
                        required_flags: MemoryPropertyFlags::HOST_VISIBLE,
                        not_preferred_flags: MemoryPropertyFlags::HOST_CACHED
                            | MemoryPropertyFlags::DEVICE_COHERENT,
                        ..MemoryTypeFilter::empty()
                    },
                    allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
                    ..AllocationCreateInfo::default()
                },
                len,
            )
            .unwrap(),
        );

        vertex_buffers.push(
            Buffer::new_slice::<ItfVertInfo>(
                mem_alloc.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_SRC
                        | BufferUsage::TRANSFER_DST
                        | BufferUsage::VERTEX_BUFFER,
                    ..BufferCreateInfo::default()
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
                len,
            )
            .unwrap(),
        );
    }

    (staging_buffers, vertex_buffers)
}

fn create_image_with_buffer(
    mem_alloc: &Arc<StandardMemoryAllocator>,
    image_format: VkFormat,
    width: u32,
    height: u32,
) -> (Vec<Subbuffer<[u8]>>, Vec<Arc<Image>>) {
    let mut image_staging_buffers = Vec::with_capacity(2);
    let mut images = Vec::with_capacity(2);

    for _ in 0..2 {
        image_staging_buffers.push(
            Buffer::new_slice::<u8>(
                mem_alloc.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_SRC,
                    ..BufferCreateInfo::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter {
                        required_flags: MemoryPropertyFlags::HOST_VISIBLE,
                        not_preferred_flags: MemoryPropertyFlags::HOST_CACHED
                            | MemoryPropertyFlags::DEVICE_COHERENT,
                        ..MemoryTypeFilter::empty()
                    },
                    allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
                    ..AllocationCreateInfo::default()
                },
                image_format.block_size() * width as DeviceSize * height as DeviceSize,
            )
            .unwrap(),
        );

        images.push(
            Image::new(
                mem_alloc.clone(),
                ImageCreateInfo {
                    image_type: ImageType::Dim2d,
                    format: image_format,
                    extent: [width, height, 1],
                    usage: ImageUsage::TRANSFER_SRC
                        | ImageUsage::TRANSFER_DST
                        | ImageUsage::SAMPLED,
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
            .unwrap(),
        );
    }

    (image_staging_buffers, images)
}
