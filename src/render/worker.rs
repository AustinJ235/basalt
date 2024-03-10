use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, Barrier, Weak};
use std::time::Instant;

use cosmic_text::fontdb::Source as FontSource;
use cosmic_text::{FontSystem, SwashCache};
use flume::{Receiver, Sender, TryRecvError};
use guillotiere::{
    Allocation as AtlasAllocation, AllocatorOptions as AtlasAllocatorOptions, AtlasAllocator,
    Size as AtlasSize,
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
    CopyImageInfo, PrimaryAutoCommandBuffer, PrimaryCommandBufferAbstract,
};
use vulkano::format::Format as VkFormat;
use vulkano::image::sys::ImageCreateInfo;
use vulkano::image::{Image, ImageSubresourceLayers, ImageType, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryAllocatePreference, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::memory::MemoryPropertyFlags;
use vulkano::sync::GpuFuture;
use vulkano::DeviceSize;

use crate::interface::bin::{Bin, BinID};
use crate::interface::{DefaultFont, ItfVertInfo};
use crate::render::{ImageCacheKey, ImageSource, RenderEvent, UpdateContext};
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

enum OVDEvent {
    AddBinaryFont(Arc<dyn AsRef<[u8]> + Sync + Send>),
    SetDefaultFont(DefaultFont),
    SetExtent([u32; 2]),
    SetScale(f32),
    PerformOVD,
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
        let max_image_dimension2_d = window
            .basalt_ref()
            .physical_device()
            .properties()
            .max_image_dimension2_d;

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
        let mut zeroing_buffer: Option<Subbuffer<[u8]>> = None;
        let mut image_backings: Vec<ImageBacking> = Vec::new();

        let ovd_num_threads = window.basalt_ref().options_ref().bin_parallel_threads.get();
        let mut ovd_font_systems = vec![FontSystem::new()];

        for binary_font in window.basalt_ref().interface_ref().binary_fonts() {
            ovd_font_systems[0]
                .db_mut()
                .load_font_source(FontSource::Binary(binary_font));
        }

        while ovd_font_systems.len() < ovd_num_threads {
            let locale = ovd_font_systems[0].locale().to_string();
            let db = ovd_font_systems[0].db().clone();
            ovd_font_systems.push(FontSystem::new_with_locale_and_db(locale, db));
        }

        let default_font = window.basalt_ref().interface_ref().default_font();
        let mut ovd_event_sends = Vec::with_capacity(ovd_num_threads);
        let (ovd_data_send, ovd_data_recv) = flume::unbounded();
        let (ovd_bin_send, ovd_bin_recv) = flume::unbounded::<Arc<Bin>>();
        let mut ovd_threads = Vec::with_capacity(ovd_num_threads);

        for font_system in ovd_font_systems {
            let (ovd_event_send, event_recv) = flume::unbounded();
            ovd_event_sends.push(ovd_event_send);

            let mut update_context = UpdateContext {
                extent: [window_size[0] as f32, window_size[1] as f32],
                scale: effective_scale,
                font_system,
                glyph_cache: SwashCache::new(),
                default_font: default_font.clone(),
            };

            let data_send = ovd_data_send.clone();
            let bin_recv = ovd_bin_recv.clone();

            ovd_threads.push(std::thread::spawn(move || {
                while let Ok(ovd_event) = event_recv.recv() {
                    match ovd_event {
                        OVDEvent::AddBinaryFont(binary_font) => {
                            update_context
                                .font_system
                                .db_mut()
                                .load_font_source(FontSource::Binary(binary_font));
                        },
                        OVDEvent::SetDefaultFont(default_font) => {
                            update_context.default_font = default_font;
                        },
                        OVDEvent::SetScale(scale) => {
                            update_context.scale = scale;
                        },
                        OVDEvent::SetExtent(extent) => {
                            update_context.extent = [extent[0] as f32, extent[1] as f32];
                        },
                        OVDEvent::PerformOVD => {
                            while let Ok(bin) = bin_recv.try_recv() {
                                let id = bin.id();

                                if data_send
                                    .send((id, bin.obtain_vertex_data(&mut update_context)))
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        },
                    }
                }
            }));
        }

        let mut next_cmd_builder_op = None;
        let mut active_index = 0;
        let mut inactive_index = 1;

        'main_loop: loop {
            let mut work_to_do = update_all;

            loop {
                let window_event = match work_to_do && window_size != [0; 2] {
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

                            for ovd_event_send in ovd_event_sends.iter() {
                                if ovd_event_send
                                    .send(OVDEvent::SetExtent(window_size))
                                    .is_err()
                                {
                                    panic!("an ovd thread has panicked.");
                                }
                            }

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

                            for ovd_event_send in ovd_event_sends.iter() {
                                if ovd_event_send
                                    .send(OVDEvent::SetScale(effective_scale))
                                    .is_err()
                                {
                                    panic!("an ovd thread has panicked.");
                                }
                            }

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
                    WindowEvent::AddBinaryFont(binary_font) => {
                        for ovd_event_send in ovd_event_sends.iter() {
                            if ovd_event_send
                                .send(OVDEvent::AddBinaryFont(binary_font.clone()))
                                .is_err()
                            {
                                panic!("an ovd thread has panicked.");
                            }
                        }

                        work_to_do = true;
                        update_all = true;
                    },
                    WindowEvent::SetDefaultFont(default_font) => {
                        for ovd_event_send in ovd_event_sends.iter() {
                            if ovd_event_send
                                .send(OVDEvent::SetDefaultFont(default_font.clone()))
                                .is_err()
                            {
                                panic!("an ovd thread has panicked.");
                            }
                        }

                        work_to_do = true;
                        update_all = true;
                    },
                    WindowEvent::SetMSAA(msaa) => {
                        if render_event_send.send(RenderEvent::SetMSAA(msaa)).is_err() {
                            break 'main_loop;
                        }
                    },
                    WindowEvent::SetVSync(vsync) => {
                        if render_event_send
                            .send(RenderEvent::SetVSync(vsync))
                            .is_err()
                        {
                            break 'main_loop;
                        }
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

            let mut add_image_sources: HashMap<ImageSource, usize> = HashMap::new();

            if !update_bins.is_empty() {
                let update_count = update_bins.len();

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

                    if ovd_bin_send.send(bin).is_err() {
                        panic!("all ovd threads have panicked");
                    }
                }

                for ovd_event_send in ovd_event_sends.iter() {
                    if ovd_event_send.send(OVDEvent::PerformOVD).is_err() {
                        panic!("an ovd thread has panicked.");
                    }
                }

                let mut update_recv_count = 0;

                while update_recv_count < update_count {
                    let (bin_id, obtained_data) = match ovd_data_recv.recv().ok() {
                        Some(some) => some,
                        None => panic!("all ovd threads have panicked"),
                    };

                    let state = bin_states.get_mut(&bin_id).unwrap();
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
                    update_recv_count += 1;
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
            let mut active_atlas_clear_regions: HashMap<Arc<Image>, Vec<BufferImageCopy>> =
                HashMap::new();
            let mut next_atlas_clear_regions: HashMap<Arc<Image>, Vec<BufferImageCopy>> =
                HashMap::new();

            for (i, image_backing) in image_backings.iter_mut().enumerate() {
                match image_backing {
                    ImageBacking::Atlas {
                        allocator,
                        contains,
                        images,
                        ..
                    } => {
                        // NOTE: Atlas's that are empty are kept as it assumed that if the user
                        //       execeeded the capacity of the other atlas's they will do so again.

                        contains.retain(|image_source, contains| {
                            if contains.use_count == 0 {
                                if let ImageSource::Cache(image_cache_key) = &image_source {
                                    deref_image_cache_keys.push(image_cache_key.clone());
                                    allocator.deallocate(contains.data.id);

                                    let clear_region_info = BufferImageCopy {
                                        image_offset: [
                                            contains.data.rectangle.min.x as u32,
                                            contains.data.rectangle.min.y as u32,
                                            0,
                                        ],
                                        image_extent: [
                                            contains.data.rectangle.width() as u32,
                                            contains.data.rectangle.height() as u32,
                                            1,
                                        ],
                                        ..BufferImageCopy::default()
                                    };

                                    active_atlas_clear_regions
                                        .entry(images[active_index].clone())
                                        .or_insert_with(Vec::new)
                                        .push(clear_region_info.clone());

                                    next_atlas_clear_regions
                                        .entry(images[inactive_index].clone())
                                        .or_insert_with(Vec::new)
                                        .push(clear_region_info);

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

            // -- Clear Previously Used Atlas Regions -- //

            let modified_images = !remove_image_backings.is_empty()
                || !obtain_image_sources.is_empty()
                || !active_atlas_clear_regions.is_empty()
                || !next_atlas_clear_regions.is_empty();

            if !active_atlas_clear_regions.is_empty() || !next_atlas_clear_regions.is_empty() {
                let mut required_buffer_len = 0;

                for regions in active_atlas_clear_regions
                    .values()
                    .chain(next_atlas_clear_regions.values())
                {
                    required_buffer_len = required_buffer_len.max(
                        regions
                            .iter()
                            .map(|region| {
                                region.image_extent[0] as DeviceSize
                                    * region.image_extent[1] as DeviceSize
                            })
                            .max()
                            .unwrap(),
                    );
                }

                check_resize_zeroing_buffer(
                    &mut active_cmd_builder,
                    &mem_alloc,
                    &mut zeroing_buffer,
                    required_buffer_len,
                );

                let buffer = zeroing_buffer.as_ref().unwrap();

                for (image, regions) in active_atlas_clear_regions {
                    active_cmd_builder
                        .copy_buffer_to_image(CopyBufferToImageInfo {
                            regions: regions.into(),
                            ..CopyBufferToImageInfo::buffer_image(buffer.clone(), image)
                        })
                        .unwrap();
                }

                for (image, regions) in next_atlas_clear_regions {
                    next_cmd_builder
                        .copy_buffer_to_image(CopyBufferToImageInfo {
                            regions: regions.into(),
                            ..CopyBufferToImageInfo::buffer_image(buffer.clone(), image)
                        })
                        .unwrap();
                }
            }

            // -- Update Vertex Data Effected by Image Backing Removal -- //

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
                                                            contained.data.rectangle.min.x as f32
                                                                + 1.0,
                                                            contained.data.rectangle.min.y as f32
                                                                + 1.0,
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

                if !obtain_image_sources.is_empty() {
                    for image_backing in image_backings.iter_mut() {
                        if let ImageBacking::Atlas {
                            staging_buffer_index,
                            ..
                        } = image_backing
                        {
                            *staging_buffer_index = 0;
                        }
                    }
                }

                let mut active_atlas_copy_infos = HashMap::new();
                let mut next_atlas_copy_infos = HashMap::new();

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
                            if obtained_image.width > 512 || obtained_image.height > 512 {
                                let (image, buffer) = create_image_with_buffer(
                                    &mem_alloc,
                                    image_format,
                                    obtained_image.width,
                                    obtained_image.height,
                                    false,
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
                                let mut image_allocated = false;
                                let alloc_size = AtlasSize::new(
                                    obtained_image.width.max(14) as i32 + 2,
                                    obtained_image.height.max(14) as i32 + 2,
                                );

                                for image_backing in image_backings.iter_mut() {
                                    if let ImageBacking::Atlas {
                                        contains,
                                        staging_buffers,
                                        staging_buffer_index,
                                        images,
                                        allocator,
                                    } = image_backing
                                    {
                                        // Try allocation without resizing
                                        if let Some(allocation) = allocator.allocate(alloc_size) {
                                            staging_buffers[active_index].write().unwrap()
                                                [*staging_buffer_index..]
                                                [..obtained_image.data.len()]
                                                .copy_from_slice(&obtained_image.data);

                                            active_atlas_copy_infos
                                                .entry((
                                                    staging_buffers[active_index].clone(),
                                                    images[active_index].clone(),
                                                ))
                                                .or_insert_with(Vec::new)
                                                .push(BufferImageCopy {
                                                    buffer_offset: *staging_buffer_index
                                                        as DeviceSize,
                                                    image_subresource:
                                                        ImageSubresourceLayers::from_parameters(
                                                            image_format,
                                                            1,
                                                        ),
                                                    image_offset: [
                                                        allocation.rectangle.min.x as u32 + 1,
                                                        allocation.rectangle.min.y as u32 + 1,
                                                        0,
                                                    ],
                                                    image_extent: [
                                                        obtained_image.width,
                                                        obtained_image.height,
                                                        1,
                                                    ],
                                                    ..BufferImageCopy::default()
                                                });

                                            next_atlas_copy_infos
                                                .entry((
                                                    staging_buffers[active_index].clone(),
                                                    images[inactive_index].clone(),
                                                ))
                                                .or_insert_with(Vec::new)
                                                .push(BufferImageCopy {
                                                    buffer_offset: *staging_buffer_index
                                                        as DeviceSize,
                                                    image_subresource:
                                                        ImageSubresourceLayers::from_parameters(
                                                            image_format,
                                                            1,
                                                        ),
                                                    image_offset: [
                                                        allocation.rectangle.min.x as u32 + 1,
                                                        allocation.rectangle.min.y as u32 + 1,
                                                        0,
                                                    ],
                                                    image_extent: [
                                                        obtained_image.width,
                                                        obtained_image.height,
                                                        1,
                                                    ],
                                                    ..BufferImageCopy::default()
                                                });

                                            *staging_buffer_index += obtained_image.data.len();
                                            image_allocated = true;

                                            contains.insert(
                                                image_source.clone(),
                                                ContainedImage {
                                                    data: allocation,
                                                    use_count: uses,
                                                    last_used: None,
                                                },
                                            );

                                            break;
                                        }

                                        // Try resizing then allocating
                                        if allocator.size().width as u32 * 2
                                            < max_image_dimension2_d
                                        {
                                            allocator.grow(AtlasSize::new(
                                                allocator.size().width * 2,
                                                allocator.size().height * 2,
                                            ));

                                            let (new_images, new_staging_buffers) =
                                                create_images_with_buffers(
                                                    &mem_alloc,
                                                    image_format,
                                                    allocator.size().width as u32,
                                                    allocator.size().height as u32,
                                                    true,
                                                );

                                            clear_image(
                                                &mut active_cmd_builder,
                                                &mem_alloc,
                                                &mut zeroing_buffer,
                                                new_images[active_index].clone(),
                                            );

                                            active_cmd_builder
                                                .copy_image(CopyImageInfo::images(
                                                    images[active_index].clone(),
                                                    new_images[active_index].clone(),
                                                ))
                                                .unwrap();

                                            clear_image(
                                                &mut next_cmd_builder,
                                                &mem_alloc,
                                                &mut zeroing_buffer,
                                                new_images[inactive_index].clone(),
                                            );

                                            next_cmd_builder
                                                .copy_image(CopyImageInfo::images(
                                                    images[inactive_index].clone(),
                                                    new_images[inactive_index].clone(),
                                                ))
                                                .unwrap();

                                            *staging_buffer_index = 0;
                                            *images = new_images;
                                            *staging_buffers = new_staging_buffers;

                                            let allocation =
                                                allocator.allocate(alloc_size).unwrap();

                                            staging_buffers[active_index].write().unwrap()
                                                [*staging_buffer_index..]
                                                [..obtained_image.data.len()]
                                                .copy_from_slice(&obtained_image.data);

                                            active_atlas_copy_infos
                                                .entry((
                                                    staging_buffers[active_index].clone(),
                                                    images[active_index].clone(),
                                                ))
                                                .or_insert_with(Vec::new)
                                                .push(BufferImageCopy {
                                                    buffer_offset: *staging_buffer_index
                                                        as DeviceSize,
                                                    image_subresource:
                                                        ImageSubresourceLayers::from_parameters(
                                                            image_format,
                                                            1,
                                                        ),
                                                    image_offset: [
                                                        allocation.rectangle.min.x as u32 + 1,
                                                        allocation.rectangle.min.y as u32 + 1,
                                                        0,
                                                    ],
                                                    image_extent: [
                                                        obtained_image.width,
                                                        obtained_image.height,
                                                        1,
                                                    ],
                                                    ..BufferImageCopy::default()
                                                });

                                            next_atlas_copy_infos
                                                .entry((
                                                    staging_buffers[active_index].clone(),
                                                    images[inactive_index].clone(),
                                                ))
                                                .or_insert_with(Vec::new)
                                                .push(BufferImageCopy {
                                                    buffer_offset: *staging_buffer_index
                                                        as DeviceSize,
                                                    image_subresource:
                                                        ImageSubresourceLayers::from_parameters(
                                                            image_format,
                                                            1,
                                                        ),
                                                    image_offset: [
                                                        allocation.rectangle.min.x as u32 + 1,
                                                        allocation.rectangle.min.y as u32 + 1,
                                                        0,
                                                    ],
                                                    image_extent: [
                                                        obtained_image.width,
                                                        obtained_image.height,
                                                        1,
                                                    ],
                                                    ..BufferImageCopy::default()
                                                });

                                            *staging_buffer_index += obtained_image.data.len();
                                            image_allocated = true;

                                            contains.insert(
                                                image_source.clone(),
                                                ContainedImage {
                                                    data: allocation,
                                                    use_count: uses,
                                                    last_used: None,
                                                },
                                            );

                                            break;
                                        }
                                    }
                                }

                                // no suitable atlas found, create a new one
                                if !image_allocated {
                                    let mut allocator = AtlasAllocator::with_options(
                                        AtlasSize::new(4096, 4096),
                                        &AtlasAllocatorOptions {
                                            alignment: AtlasSize::new(16, 16),
                                            small_size_threshold: 16,
                                            large_size_threshold: 512,
                                        },
                                    );

                                    let (images, staging_buffers) = create_images_with_buffers(
                                        &mem_alloc,
                                        image_format,
                                        allocator.size().width as u32,
                                        allocator.size().height as u32,
                                        true,
                                    );

                                    clear_image(
                                        &mut active_cmd_builder,
                                        &mem_alloc,
                                        &mut zeroing_buffer,
                                        images[active_index].clone(),
                                    );

                                    clear_image(
                                        &mut next_cmd_builder,
                                        &mem_alloc,
                                        &mut zeroing_buffer,
                                        images[inactive_index].clone(),
                                    );

                                    let mut contains = HashMap::new();
                                    let mut staging_buffer_index = 0;

                                    let allocation = allocator.allocate(alloc_size).unwrap();
                                    staging_buffers[active_index].write().unwrap()
                                        [staging_buffer_index..][..obtained_image.data.len()]
                                        .copy_from_slice(&obtained_image.data);

                                    active_atlas_copy_infos
                                        .entry((
                                            staging_buffers[active_index].clone(),
                                            images[active_index].clone(),
                                        ))
                                        .or_insert_with(Vec::new)
                                        .push(BufferImageCopy {
                                            buffer_offset: staging_buffer_index as DeviceSize,
                                            image_subresource:
                                                ImageSubresourceLayers::from_parameters(
                                                    image_format,
                                                    1,
                                                ),
                                            image_offset: [
                                                allocation.rectangle.min.x as u32 + 1,
                                                allocation.rectangle.min.y as u32 + 1,
                                                0,
                                            ],
                                            image_extent: [
                                                obtained_image.width,
                                                obtained_image.height,
                                                1,
                                            ],
                                            ..BufferImageCopy::default()
                                        });

                                    next_atlas_copy_infos
                                        .entry((
                                            staging_buffers[active_index].clone(),
                                            images[inactive_index].clone(),
                                        ))
                                        .or_insert_with(Vec::new)
                                        .push(BufferImageCopy {
                                            buffer_offset: staging_buffer_index as DeviceSize,
                                            image_subresource:
                                                ImageSubresourceLayers::from_parameters(
                                                    image_format,
                                                    1,
                                                ),
                                            image_offset: [
                                                allocation.rectangle.min.x as u32 + 1,
                                                allocation.rectangle.min.y as u32 + 1,
                                                0,
                                            ],
                                            image_extent: [
                                                obtained_image.width,
                                                obtained_image.height,
                                                1,
                                            ],
                                            ..BufferImageCopy::default()
                                        });

                                    staging_buffer_index += obtained_image.data.len();
                                    contains.insert(
                                        image_source,
                                        ContainedImage {
                                            data: allocation,
                                            use_count: uses,
                                            last_used: None,
                                        },
                                    );

                                    image_backings.push(ImageBacking::Atlas {
                                        contains,
                                        staging_buffers,
                                        staging_buffer_index,
                                        images,
                                        allocator,
                                    });
                                }
                            }
                        },
                    }
                }

                for ((buffer, image), copy_infos) in active_atlas_copy_infos {
                    active_cmd_builder
                        .copy_buffer_to_image(CopyBufferToImageInfo {
                            regions: copy_infos.into(),
                            ..CopyBufferToImageInfo::buffer_image(buffer, image)
                        })
                        .unwrap();
                }

                for ((buffer, image), copy_infos) in next_atlas_copy_infos {
                    next_cmd_builder
                        .copy_buffer_to_image(CopyBufferToImageInfo {
                            regions: copy_infos.into(),
                            ..CopyBufferToImageInfo::buffer_image(buffer, image)
                        })
                        .unwrap();
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
                                                            contained.data.rectangle.min.x as f32
                                                                + 1.0,
                                                            contained.data.rectangle.min.y as f32
                                                                + 1.0,
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
                    move_regions.sort_by_key(|region| region.src_offset);
                    let mut merged_move_regions = Vec::new();

                    for region in move_regions {
                        if merged_move_regions.is_empty() {
                            merged_move_regions.push(region);
                        } else {
                            let last_region = merged_move_regions.last_mut().unwrap();

                            if last_region.src_offset + last_region.size == region.src_offset {
                                last_region.size += region.size;
                            } else {
                                merged_move_regions.push(region);
                            }
                        }
                    }

                    active_cmd_builder
                        .copy_buffer(CopyBufferInfoTyped {
                            regions: merged_move_regions.clone().into(),
                            ..CopyBufferInfoTyped::buffers(
                                vertex_buffers[active_index].clone(),
                                vertex_buffers[active_index].clone(),
                            )
                        })
                        .unwrap();

                    next_cmd_builder
                        .copy_buffer(CopyBufferInfoTyped {
                            regions: merged_move_regions.into(),
                            ..CopyBufferInfoTyped::buffers(
                                vertex_buffers[inactive_index].clone(),
                                vertex_buffers[inactive_index].clone(),
                            )
                        })
                        .unwrap();
                }

                if !upload_regions.is_empty() {
                    upload_regions.sort_by_key(|region| region.src_offset);
                    let mut merged_upload_regions = Vec::new();

                    for region in upload_regions {
                        if merged_upload_regions.is_empty() {
                            merged_upload_regions.push(region);
                        } else {
                            let last_region = merged_upload_regions.last_mut().unwrap();

                            if last_region.src_offset + last_region.size == region.src_offset {
                                last_region.size += region.size;
                            } else {
                                merged_upload_regions.push(region);
                            }
                        }
                    }

                    active_cmd_builder
                        .copy_buffer(CopyBufferInfoTyped {
                            regions: merged_upload_regions.clone().into(),
                            ..CopyBufferInfoTyped::buffers(
                                staging_buffers[active_index].clone(),
                                vertex_buffers[active_index].clone(),
                            )
                        })
                        .unwrap();

                    next_cmd_builder
                        .copy_buffer(CopyBufferInfoTyped {
                            regions: merged_upload_regions.into(),
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

fn check_resize_zeroing_buffer(
    cmd_builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    mem_alloc: &Arc<StandardMemoryAllocator>,
    zeroing_buffer: &mut Option<Subbuffer<[u8]>>,
    required_len: DeviceSize,
) {
    let buffer_len = required_len.next_power_of_two();

    if zeroing_buffer.is_none() || zeroing_buffer.as_ref().unwrap().size() < buffer_len {
        let buffer = Buffer::new_slice::<u8>(
            mem_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
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
            buffer_len,
        )
        .unwrap();

        cmd_builder
            .fill_buffer(buffer.clone().reinterpret(), 0)
            .unwrap();

        *zeroing_buffer = Some(buffer);
    }
}

fn clear_image(
    cmd_builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    mem_alloc: &Arc<StandardMemoryAllocator>,
    zeroing_buffer: &mut Option<Subbuffer<[u8]>>,
    image: Arc<Image>,
) {
    let [width, height, _] = image.extent();
    let required_buffer_len =
        image.format().block_size() * width as DeviceSize * height as DeviceSize;

    check_resize_zeroing_buffer(cmd_builder, mem_alloc, zeroing_buffer, required_buffer_len);

    cmd_builder
        .copy_buffer_to_image(CopyBufferToImageInfo::buffer_image(
            zeroing_buffer.clone().unwrap(),
            image,
        ))
        .unwrap();
}

fn create_buffer_for_image(
    mem_alloc: &Arc<StandardMemoryAllocator>,
    image_format: VkFormat,
    width: u32,
    height: u32,
    buffer_long_lived: bool,
) -> Subbuffer<[u8]> {
    let buffer_alloc_preference = if buffer_long_lived {
        MemoryAllocatePreference::AlwaysAllocate
    } else {
        MemoryAllocatePreference::Unknown
    };

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
            allocate_preference: buffer_alloc_preference,
            ..AllocationCreateInfo::default()
        },
        image_format.block_size() * width as DeviceSize * height as DeviceSize,
    )
    .unwrap()
}

fn create_image_with_buffer(
    mem_alloc: &Arc<StandardMemoryAllocator>,
    image_format: VkFormat,
    width: u32,
    height: u32,
    buffer_long_lived: bool,
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
        create_buffer_for_image(mem_alloc, image_format, width, height, buffer_long_lived),
    )
}

fn create_images_with_buffers(
    mem_alloc: &Arc<StandardMemoryAllocator>,
    image_format: VkFormat,
    width: u32,
    height: u32,
    buffer_long_lived: bool,
) -> (Vec<Arc<Image>>, Vec<Subbuffer<[u8]>>) {
    let (image1, buffer1) =
        create_image_with_buffer(mem_alloc, image_format, width, height, buffer_long_lived);
    let (image2, buffer2) =
        create_image_with_buffer(mem_alloc, image_format, width, height, buffer_long_lived);
    (vec![image1, image2], vec![buffer1, buffer2])
}