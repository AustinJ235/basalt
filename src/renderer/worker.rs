use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, Barrier, Weak};
use std::time::Instant;

use cosmic_text::{FontSystem, SwashCache};
use flume::{Receiver, Sender, TryRecvError};
use guillotiere::{
    Allocation as AtlasAllocation, AllocatorOptions as AtlasAllocatorOptions, AtlasAllocator,
};
use ordered_float::OrderedFloat;
use vulkano::buffer::sys::BufferCreateInfo;
use vulkano::buffer::{Buffer, BufferUsage, Subbuffer};
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::auto::AutoCommandBufferBuilder;
use vulkano::command_buffer::{
    BufferCopy, BufferImageCopy, CommandBufferUsage, CopyBufferInfoTyped, CopyBufferToImageInfo,
    PrimaryCommandBufferAbstract,
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
use crate::renderer::{ImageCacheKey, ImageSource, RenderEvent, UpdateContext};
use crate::window::{Window, WindowEvent};

struct BinState {
    weak: Weak<Bin>,
    image_sources: Vec<ImageSource>,
    vertex_data: Option<BTreeMap<OrderedFloat<f32>, BinZData>>,
}

struct BinZData {
    range: Option<Range<DeviceSize>>,
    data: HashMap<ImageSource, Vec<ItfVertInfo>>,
}

struct ContainedImage<T> {
    data: T,
    use_count: usize,
    last_used: Option<Instant>,
}

enum ImageBacking {
    Atlas {
        contains: HashMap<ImageSource, ContainedImage<AtlasAllocation>>,
        staging_buffers: Vec<Subbuffer<[u8]>>,
        staging_buffer_index: usize,
        copy_infos: Vec<BufferImageCopy>,
        images: Vec<Arc<Image>>,
        allocator: AtlasAllocator,
    },
    Dedicated {
        source: ImageSource,
        contains: ContainedImage<()>,
        image: Arc<Image>,
    },
    UserProvided {
        source: ImageSource,
        contains: ContainedImage<()>,
        image: Arc<Image>,
    },
}

pub fn spawn(
    window: Arc<Window>,
    window_event_recv: Receiver<WindowEvent>,
    render_event_send: Sender<RenderEvent>,
    image_format: VkFormat,
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
        let mut bin_states: BTreeMap<BinID, BinState> = BTreeMap::new();

        for bin in window.associated_bins() {
            bin_states.insert(
                bin.id(),
                BinState {
                    weak: Arc::downgrade(&bin),
                    image_sources: Vec::new(),
                    vertex_data: None,
                },
            );
        }

        let mut update_all = true;
        let mut update_bins: Vec<Arc<Bin>> = Vec::new();
        let mut remove_bins: Vec<BinID> = Vec::new();
        let (mut staging_buffers, mut vertex_buffers) =
            create_buffers(&mem_alloc as &Arc<_>, 32768);
        let mut image_backings: Vec<ImageBacking> = Vec::new();

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
                                image_sources: Vec::new(),
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

            let mut modified_vertexes = !update_bins.is_empty() || update_all;
            let mut remove_image_sources: HashMap<ImageSource, usize> = HashMap::new();
            remove_bins.sort();
            remove_bins.dedup();

            if update_all {
                update_all = false;
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
                    } else {
                        if state.vertex_data.is_some() {
                            modified_vertexes = true;
                        }
                    }

                    for image_source in state.image_sources.drain(..) {
                        *remove_image_sources
                            .entry(image_source)
                            .or_insert_with(|| 0) += 1;
                    }

                    retain
                });

                remove_bins.clear();
            } else {
                update_bins.sort_by_key(|bin| bin.id());
                update_bins.dedup_by_key(|bin| bin.id());

                for bin_id in remove_bins.drain(..) {
                    if let Some(mut state) = bin_states.remove(&bin_id) {
                        if state.vertex_data.is_some() {
                            modified_vertexes = true;
                        }

                        for image_source in state.image_sources.drain(..) {
                            *remove_image_sources
                                .entry(image_source)
                                .or_insert_with(|| 0) += 1;
                        }
                    }
                }
            }

            // --- Obtain Vertex Data --- //
            // TODO: Threaded

            let mut add_image_sources: HashMap<ImageSource, usize> = HashMap::new();

            if !update_bins.is_empty() {
                for bin in update_bins.drain(..) {
                    let state = bin_states.get_mut(&bin.id()).unwrap();

                    if state.vertex_data.take().is_some() {
                        modified_vertexes = true;
                    }

                    for image_source in state.image_sources.drain(..) {
                        *remove_image_sources
                            .entry(image_source)
                            .or_insert_with(|| 0) += 1;
                    }

                    let obtained_data = bin.obtain_vertex_data(&mut update_context);
                    let mut image_sources = HashSet::new();

                    for (image_source, _) in obtained_data.iter() {
                        if *image_source != ImageSource::None {
                            image_sources.insert(image_source.clone());
                        }
                    }

                    for image_source in image_sources.iter() {
                        *add_image_sources
                            .entry(image_source.clone())
                            .or_insert_with(|| 0) += 1;
                    }

                    let mut vertex_data = BTreeMap::new();

                    for (image_source, vertexes) in obtained_data {
                        for vertex in vertexes {
                            let z = OrderedFloat::<f32>::from(vertex.position[2]);

                            vertex_data
                                .entry(z)
                                .or_insert_with(|| {
                                    BinZData {
                                        range: None,
                                        data: HashMap::new(),
                                    }
                                })
                                .data
                                .entry(image_source.clone())
                                .or_insert_with(Vec::new)
                                .push(vertex);
                        }
                    }

                    state.vertex_data = Some(vertex_data);
                    state.image_sources = image_sources.into_iter().collect();
                }
            }

            // -- Decrease Image Use Counters -- //

            for (image_source, count) in remove_image_sources {
                for image_backing in image_backings.iter_mut() {
                    match image_backing {
                        ImageBacking::Atlas {
                            contains, ..
                        } => {
                            if let Some(contained_image) = contains.get_mut(&image_source) {
                                contained_image.use_count -= count;
                                break;
                            }
                        },
                        ImageBacking::Dedicated {
                            source,
                            contains,
                            ..
                        } => {
                            if *source == image_source {
                                contains.use_count -= count;
                                break;
                            }
                        },
                        ImageBacking::UserProvided {
                            source,
                            contains,
                            ..
                        } => {
                            if *source == image_source {
                                contains.use_count -= count;
                                break;
                            }
                        },
                    }
                }
            }

            // -- Increase Image Use Counters -- //

            let mut obtain_image_sources: HashMap<ImageSource, usize> = HashMap::new();

            for (image_source, count) in add_image_sources {
                let mut obtain_image_source = true;

                for image_backing in image_backings.iter_mut() {
                    match image_backing {
                        ImageBacking::Atlas {
                            contains, ..
                        } => {
                            if let Some(contained_image) = contains.get_mut(&image_source) {
                                contained_image.use_count += count;
                                obtain_image_source = false;
                                break;
                            }
                        },
                        ImageBacking::Dedicated {
                            source,
                            contains,
                            ..
                        } => {
                            if *source == image_source {
                                contains.use_count += count;
                                obtain_image_source = false;
                                break;
                            }
                        },
                        ImageBacking::UserProvided {
                            source,
                            contains,
                            ..
                        } => {
                            if *source == image_source {
                                contains.use_count += count;
                                obtain_image_source = false;
                                break;
                            }
                        },
                    }
                }

                if obtain_image_source {
                    *obtain_image_sources
                        .entry(image_source)
                        .or_insert_with(|| 0) += count;
                }
            }

            // -- Deref Image Cache Keys & Remove Image Backings -- //

            let mut remove_image_backings = Vec::new();
            let mut deref_image_cache_keys: Vec<ImageCacheKey> = Vec::new();

            for (i, image_backing) in image_backings.iter_mut().enumerate() {
                match image_backing {
                    ImageBacking::Atlas {
                        allocator,
                        contains,
                        ..
                    } => {
                        // TODO: Atlas image backings aren't removed should they be?

                        contains.retain(|image_source, contains| {
                            if contains.use_count == 0 {
                                if let ImageSource::Cache(image_cache_key) = &image_source {
                                    deref_image_cache_keys.push(image_cache_key.clone());
                                    allocator.deallocate(contains.data.id);
                                    false
                                } else {
                                    unreachable!()
                                }
                            } else {
                                true
                            }
                        });
                    },
                    ImageBacking::Dedicated {
                        source,
                        contains,
                        ..
                    } => {
                        if contains.use_count == 0 {
                            if let ImageSource::Cache(image_cache_key) = &source {
                                deref_image_cache_keys.push(image_cache_key.clone());
                                remove_image_backings.push(i);
                            } else {
                                unreachable!()
                            }
                        }
                    },
                    ImageBacking::UserProvided {
                        contains, ..
                    } => {
                        if contains.use_count == 0 {
                            remove_image_backings.push(i);
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

            // -- Update Vertex Data Effected by Image Backing Removal -- //

            let modified_images =
                !remove_image_backings.is_empty() || !obtain_image_sources.is_empty();
            let mut next_staging_index: DeviceSize = 0;

            if !remove_image_backings.is_empty() {
                let image_index_effected_after = remove_image_backings[0];
                let mut image_sources_effected = HashSet::new();

                for image_backing in image_backings.iter().skip(image_index_effected_after + 1) {
                    match image_backing {
                        ImageBacking::Atlas {
                            contains, ..
                        } => {
                            for (image_source, contained) in contains.iter() {
                                if contained.use_count > 0 {
                                    image_sources_effected.insert(image_source.clone());
                                }
                            }
                        },
                        ImageBacking::Dedicated {
                            source,
                            contains,
                            ..
                        } => {
                            if contains.use_count > 0 {
                                image_sources_effected.insert(source.clone());
                            }
                        },
                        ImageBacking::UserProvided {
                            source,
                            contains,
                            ..
                        } => {
                            if contains.use_count > 0 {
                                image_sources_effected.insert(source.clone());
                            }
                        },
                    }
                }

                for remove_image_backing_index in remove_image_backings.into_iter().rev() {
                    image_backings.remove(remove_image_backing_index);
                }

                let mut copy_regions = Vec::new();
                let mut staging_buffer_write = staging_buffers[active_index].write().unwrap();

                for state in bin_states
                    .values()
                    .filter(|state| state.vertex_data.is_some())
                {
                    let mut update = false;

                    for image_source in state.image_sources.iter() {
                        if image_sources_effected.contains(image_source) {
                            update = true;
                            break;
                        }
                    }

                    if update {
                        for z_data in state.vertex_data.as_ref().unwrap().values() {
                            let src_range_start = next_staging_index;
                            let dst_range = match z_data.range.clone() {
                                Some(some) => some,
                                None => continue,
                            };

                            if z_data
                                .data
                                .keys()
                                .any(|image_source| image_sources_effected.contains(image_source))
                            {
                                let mut z_vertexes = Vec::new();

                                for (image_source, vertexes) in z_data.data.iter() {
                                    let mut vertexes = vertexes.clone();

                                    if *image_source != ImageSource::None {
                                        let mut tex_i_op = None;
                                        let mut coords_offset = [0.0; 2];

                                        for (image_index, image_backing) in
                                            image_backings.iter().enumerate()
                                        {
                                            match image_backing {
                                                ImageBacking::Atlas {
                                                    contains, ..
                                                } => {
                                                    if let Some(contained) =
                                                        contains.get(image_source)
                                                    {
                                                        coords_offset = [
                                                            contained.data.rectangle.min.x as f32,
                                                            contained.data.rectangle.min.y as f32,
                                                        ];

                                                        tex_i_op = Some(image_index);
                                                        break;
                                                    }
                                                },
                                                ImageBacking::Dedicated {
                                                    source, ..
                                                } => {
                                                    if *source == *image_source {
                                                        tex_i_op = Some(image_index);
                                                        break;
                                                    }
                                                },
                                                ImageBacking::UserProvided {
                                                    source, ..
                                                } => {
                                                    if *source == *image_source {
                                                        tex_i_op = Some(image_index);
                                                        break;
                                                    }
                                                },
                                            }
                                        }

                                        let tex_i = tex_i_op.unwrap() as u32;

                                        for vertex in vertexes.iter_mut() {
                                            vertex.tex_i = tex_i;
                                            vertex.coords[0] += coords_offset[0];
                                            vertex.coords[1] += coords_offset[1];
                                        }
                                    }

                                    z_vertexes.append(&mut vertexes);
                                }

                                (*staging_buffer_write)[(src_range_start as usize)..]
                                    [..z_vertexes.len()]
                                    .swap_with_slice(&mut z_vertexes);
                                next_staging_index += z_vertexes.len() as DeviceSize;

                                copy_regions.push(BufferCopy {
                                    src_offset: src_range_start,
                                    dst_offset: dst_range.start,
                                    size: dst_range.end - dst_range.start,
                                    ..BufferCopy::default()
                                });
                            }
                        }
                    }
                }

                if !copy_regions.is_empty() {
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
            }

            // -- Obtain Image Sources -- //

            if !obtain_image_sources.is_empty() || !deref_image_cache_keys.is_empty() {
                let obtain_image_cache_keys = obtain_image_sources
                    .iter()
                    .filter_map(|(image_source, _)| {
                        match image_source {
                            ImageSource::Cache(image_cache_key) => Some(image_cache_key.clone()),
                            _ => None,
                        }
                    })
                    .collect::<Vec<_>>();

                let obtained_images = window.basalt_ref().image_cache_ref().obtain_data(
                    deref_image_cache_keys,
                    obtain_image_cache_keys,
                    image_format,
                );

                for image_backing in image_backings.iter_mut() {
                    if let ImageBacking::Atlas {
                        staging_buffer_index,
                        ..
                    } = image_backing
                    {
                        *staging_buffer_index = 0;
                    }
                }

                for (image_source, uses) in obtain_image_sources {
                    match image_source.clone() {
                        ImageSource::None => unreachable!(),
                        ImageSource::Vulkano(image) => {
                            image_backings.push(ImageBacking::UserProvided {
                                source: image_source,
                                contains: ContainedImage {
                                    data: (),
                                    use_count: uses,
                                    last_used: None,
                                },
                                image,
                            });
                        },
                        ImageSource::Cache(image_cache_key) => {
                            let obtained_image = obtained_images.get(&image_cache_key).unwrap();

                            // Large images will use a dedicated allocation
                            if true || obtained_image.width > 512 || obtained_image.height > 512 {
                                let (image, buffer) = create_image_with_buffer(
                                    &mem_alloc,
                                    image_format,
                                    obtained_image.width,
                                    obtained_image.height,
                                );

                                {
                                    let mut buffer_write = buffer.write().unwrap();
                                    buffer_write.copy_from_slice(&obtained_image.data);
                                }

                                active_cmd_builder
                                    .copy_buffer_to_image(CopyBufferToImageInfo::buffer_image(
                                        buffer,
                                        image.clone(),
                                    ))
                                    .unwrap();

                                image_backings.push(ImageBacking::Dedicated {
                                    source: image_source,
                                    contains: ContainedImage {
                                        data: (),
                                        use_count: uses,
                                        last_used: None,
                                    },
                                    image,
                                });
                            } else {
                                // TODO: Atlas Allocation of Smaller Images

                                /*let mut image_allocated = false;

                                for image_backing in image_backings.iter_mut() {
                                    if let ImageBacking::Atlas {
                                        contains,
                                        staging_buffers,
                                        staging_buffer_index,
                                        copy_infos,
                                        images,
                                        allocator,
                                    } = image_backing
                                    {
                                        // TODO:
                                    }
                                }*/
                            }
                        },
                    }
                }
            }

            // -- Count Vertexes -- //

            let mut z_count: BTreeMap<OrderedFloat<f32>, DeviceSize> = BTreeMap::new();

            for state in bin_states.values() {
                let vertex_data = match &state.vertex_data {
                    Some(some) => some,
                    None => continue,
                };

                for (z, z_data) in vertex_data.iter() {
                    *z_count.entry(*z).or_insert(0) += match z_data.range.as_ref() {
                        Some(range) => range.end - range.start,
                        None => {
                            z_data
                                .data
                                .values()
                                .map(|vertexes| vertexes.len() as DeviceSize)
                                .sum()
                        },
                    };
                }
            }

            let total_vertexes = z_count.values().sum();

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

            // -- Move & Upload Vertex Data -- //

            if modified_vertexes {
                let mut z_next_index: BTreeMap<OrderedFloat<f32>, DeviceSize> = BTreeMap::new();
                let mut z_range_start = 0;

                for (z, count) in z_count {
                    z_next_index.insert(z, z_range_start);
                    z_range_start += count;
                }

                let mut move_regions = Vec::new();
                let mut upload_regions = Vec::new();
                let mut staging_buffer_write = staging_buffers[active_index].write().unwrap();

                for state in bin_states.values_mut() {
                    let vertex_data = match state.vertex_data.as_mut() {
                        Some(some) => some,
                        None => continue,
                    };

                    for (z, z_data) in vertex_data.iter_mut() {
                        match z_data.range.clone() {
                            Some(src_range) => {
                                let next_index = z_next_index.get_mut(z).unwrap();
                                let range_len = src_range.end - src_range.start;
                                let dst_range = *next_index..(*next_index + range_len);
                                *next_index += range_len;

                                if dst_range == src_range {
                                    continue;
                                }

                                move_regions.push(BufferCopy {
                                    src_offset: src_range.start,
                                    dst_offset: dst_range.start,
                                    size: range_len,
                                    ..BufferCopy::default()
                                });
                            },
                            None => {
                                let mut z_vertexes = Vec::new();

                                for (image_source, vertexes) in z_data.data.iter() {
                                    let mut vertexes = vertexes.clone();

                                    if *image_source != ImageSource::None {
                                        let mut tex_i_op = None;
                                        let mut coords_offset = [0.0; 2];

                                        for (image_index, image_backing) in
                                            image_backings.iter().enumerate()
                                        {
                                            match image_backing {
                                                ImageBacking::Atlas {
                                                    contains, ..
                                                } => {
                                                    if let Some(contained) =
                                                        contains.get(image_source)
                                                    {
                                                        coords_offset = [
                                                            contained.data.rectangle.min.x as f32,
                                                            contained.data.rectangle.min.y as f32,
                                                        ];

                                                        tex_i_op = Some(image_index);
                                                        break;
                                                    }
                                                },
                                                ImageBacking::Dedicated {
                                                    source, ..
                                                } => {
                                                    if *source == *image_source {
                                                        tex_i_op = Some(image_index);
                                                        break;
                                                    }
                                                },
                                                ImageBacking::UserProvided {
                                                    source, ..
                                                } => {
                                                    if *source == *image_source {
                                                        tex_i_op = Some(image_index);
                                                        break;
                                                    }
                                                },
                                            }
                                        }

                                        let tex_i = tex_i_op.unwrap() as u32;

                                        for vertex in vertexes.iter_mut() {
                                            vertex.tex_i = tex_i;
                                            vertex.coords[0] += coords_offset[0];
                                            vertex.coords[1] += coords_offset[1];
                                        }
                                    }

                                    z_vertexes.append(&mut vertexes);
                                }

                                let range_len = z_vertexes.len() as DeviceSize;
                                let next_index = z_next_index.get_mut(z).unwrap();

                                (*staging_buffer_write)[(next_staging_index as usize)..]
                                    [..z_vertexes.len()]
                                    .swap_with_slice(&mut z_vertexes);

                                upload_regions.push(BufferCopy {
                                    src_offset: next_staging_index,
                                    dst_offset: *next_index,
                                    size: range_len,
                                    ..BufferCopy::default()
                                });

                                next_staging_index += range_len;
                                *next_index += range_len;
                            },
                        }
                    }
                }

                if !move_regions.is_empty() {
                    // TODO: merge regions

                    active_cmd_builder
                        .copy_buffer(CopyBufferInfoTyped {
                            regions: move_regions.clone().into(),
                            ..CopyBufferInfoTyped::buffers(
                                vertex_buffers[active_index].clone(),
                                vertex_buffers[active_index].clone(),
                            )
                        })
                        .unwrap();

                    next_cmd_builder
                        .copy_buffer(CopyBufferInfoTyped {
                            regions: move_regions.into(),
                            ..CopyBufferInfoTyped::buffers(
                                vertex_buffers[inactive_index].clone(),
                                vertex_buffers[inactive_index].clone(),
                            )
                        })
                        .unwrap();
                }

                if !upload_regions.is_empty() {
                    active_cmd_builder
                        .copy_buffer(CopyBufferInfoTyped {
                            regions: upload_regions.clone().into(),
                            ..CopyBufferInfoTyped::buffers(
                                staging_buffers[active_index].clone(),
                                vertex_buffers[active_index].clone(),
                            )
                        })
                        .unwrap();

                    next_cmd_builder
                        .copy_buffer(CopyBufferInfoTyped {
                            regions: upload_regions.into(),
                            ..CopyBufferInfoTyped::buffers(
                                staging_buffers[active_index].clone(),
                                vertex_buffers[inactive_index].clone(),
                            )
                        })
                        .unwrap();
                }
            }

            // active cmd builder has something to execute
            if exec_prev_cmds || modified_vertexes || modified_images {
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
            if modified_vertexes || modified_images {
                if total_vertexes == 0 {
                    continue;
                }

                next_cmd_builder_op = Some(next_cmd_builder);
                let barrier = Arc::new(Barrier::new(2));

                let images = image_backings
                    .iter()
                    .map(|image_backing| {
                        match image_backing {
                            ImageBacking::Atlas {
                                images, ..
                            } => images[active_index].clone(),
                            ImageBacking::Dedicated {
                                image, ..
                            } => image.clone(),
                            ImageBacking::UserProvided {
                                image, ..
                            } => image.clone(),
                        }
                    })
                    .collect::<Vec<_>>();

                if render_event_send
                    .send(RenderEvent::Update {
                        buffer: vertex_buffers[active_index]
                            .clone()
                            .slice(0..total_vertexes),
                        images,
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
) -> (Arc<Image>, Subbuffer<[u8]>) {
    (
        Image::new(
            mem_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: image_format,
                extent: [width, height, 1],
                usage: ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
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
    )
}

fn create_images_with_buffers(
    mem_alloc: &Arc<StandardMemoryAllocator>,
    image_format: VkFormat,
    width: u32,
    height: u32,
) -> (Vec<Arc<Image>>, Vec<Subbuffer<[u8]>>) {
    let (image1, buffer1) = create_image_with_buffer(mem_alloc, image_format, width, height);
    let (image2, buffer2) = create_image_with_buffer(mem_alloc, image_format, width, height);
    (vec![image1, image2], vec![buffer1, buffer2])
}
