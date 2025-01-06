mod update;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::{Arc, Barrier, Weak};
use std::time::{Duration, Instant};

use flume::{Receiver, Sender};
use foldhash::{HashMap, HashMapExt, HashSet, HashSetExt};
use guillotiere::{
    Allocation as AtlasAllocation, AllocatorOptions as AtlasAllocatorOptions, AtlasAllocator,
    Size as AtlasSize,
};
use ordered_float::OrderedFloat;

use self::update::{UpdateSubmission, UpdateWorker};
use super::RenderEvent;
use crate::image_cache::ImageCacheKey;
use crate::interface::{Bin, BinID, DefaultFont, ItfVertInfo, UpdateContext};
use crate::window::{Window, WindowEvent};

mod vk {
    pub use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
    pub use vulkano::format::Format;
    pub use vulkano::image::{
        Image, ImageCreateInfo, ImageSubresourceLayers, ImageType, ImageUsage,
    };
    pub use vulkano::memory::allocator::{
        AllocationCreateInfo, DeviceLayout, MemoryAllocatePreference, MemoryTypeFilter,
    };
    pub use vulkano::memory::MemoryPropertyFlags;
    pub use vulkano::DeviceSize;
    pub use vulkano_taskgraph::command_buffer::{
        BufferCopy, BufferImageCopy, CopyBufferInfo, CopyBufferToImageInfo, CopyImageInfo,
        FillBufferInfo, RecordingCommandBuffer,
    };
    pub use vulkano_taskgraph::graph::{CompileInfo, ExecutableTaskGraph, TaskGraph};
    pub use vulkano_taskgraph::resource::{
        AccessType, Flight, HostAccessType, ImageLayoutType, Resources,
    };
    pub use vulkano_taskgraph::{
        execute, resource_map, Id, QueueFamilyType, Task, TaskContext, TaskResult,
    };
}

const VERTEX_SIZE: vk::DeviceSize = std::mem::size_of::<ItfVertInfo>() as vk::DeviceSize;
const INITIAL_BUFFER_LEN: vk::DeviceSize = 32768;

const ATLAS_SMALL_THRESHOLD: u32 = 16;
const ATLAS_LARGE_THRESHOLD: u32 = 512;

pub struct SpawnInfo {
    pub window: Arc<Window>,
    pub render_flt_id: vk::Id<vk::Flight>,
    pub worker_flt_id: vk::Id<vk::Flight>,
    pub window_event_recv: Receiver<WindowEvent>,
    pub render_event_send: Sender<RenderEvent>,
    pub image_format: vk::Format,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
enum ImageSource {
    #[default]
    None,
    Cache(ImageCacheKey),
    Vulkano(vk::Id<vk::Image>),
}

struct BinState {
    bin_wk: Weak<Bin>,
    pending_removal: bool,
    pending_update: bool,
    images: Vec<ImageSource>,
    vertexes: BTreeMap<OrderedFloat<f32>, VertexState>,
}

struct VertexState {
    offset: [Option<vk::DeviceSize>; 2],
    staging: [Option<vk::DeviceSize>; 2],
    data: HashMap<ImageSource, Vec<ItfVertInfo>>,
    total: usize,
}

enum ImageBacking {
    Atlas {
        allocator: AtlasAllocator,
        allocations: HashMap<ImageSource, AtlasAllocationState>,
        images: [vk::Id<vk::Image>; 2],
        staging_buffers: [vk::Id<vk::Buffer>; 2],
        pending_clears: [Vec<[u32; 4]>; 2],
        pending_uploads: [Vec<StagedAtlasUpload>; 2],
        staging_write: [Vec<u8>; 2],
        resize_images: [bool; 2],
    },
    Dedicated {
        source: ImageSource,
        uses: usize,
        image_id: vk::Id<vk::Image>,
        write_info: Option<(u32, u32, Vec<u8>)>,
    },
    User {
        source: ImageSource,
        uses: usize,
        image_id: vk::Id<vk::Image>,
    },
}

struct StagedAtlasUpload {
    staging_write_i: usize,
    buffer_offset: vk::DeviceSize,
    image_offset: [u32; 3],
    image_extent: [u32; 3],
}

struct AtlasAllocationState {
    alloc: AtlasAllocation,
    uses: usize,
}

pub struct Worker {
    window: Arc<Window>,
    render_flt_id: vk::Id<vk::Flight>,
    worker_flt_id: vk::Id<vk::Flight>,
    window_event_recv: Receiver<WindowEvent>,
    render_event_send: Sender<RenderEvent>,
    image_format: vk::Format,

    bin_state: BTreeMap<BinID, BinState>,
    pending_work: bool,

    update_workers: Vec<UpdateWorker>,
    update_work_send: Sender<Arc<Bin>>,
    update_submission_recv: Receiver<UpdateSubmission>,

    buffers: [[vk::Id<vk::Buffer>; 2]; 2],
    buffer_update: [bool; 2],
    staging_buffers: [vk::Id<vk::Buffer>; 2],

    image_backings: Vec<ImageBacking>,
    image_update: [bool; 2],
    atlas_clear_buffer: vk::Id<vk::Buffer>,

    vertex_upload_task: vk::ExecutableTaskGraph<VertexUploadTaskWorld>,
    vertex_upload_task_ids: VertexUploadTask,
}

impl Worker {
    pub fn spawn(spawn_info: SpawnInfo) {
        let SpawnInfo {
            window,
            render_flt_id,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format,
        } = spawn_info;

        let update_threads = window
            .basalt_ref()
            .config
            .render_default_worker_threads
            .get();

        let mut update_contexts = Vec::with_capacity(update_threads);
        update_contexts.push(UpdateContext::from(&window));

        while update_contexts.len() < update_threads {
            update_contexts.push(UpdateContext::from(&update_contexts[0]));
        }

        let (update_work_send, update_work_recv) = flume::unbounded();
        let (update_submission_send, update_submission_recv) = flume::unbounded();

        let update_workers = update_contexts
            .into_iter()
            .map(|update_context| {
                UpdateWorker::spawn(
                    update_work_recv.clone(),
                    update_submission_send.clone(),
                    update_context,
                )
            })
            .collect::<Vec<_>>();

        let buffer_ids = (0..4)
            .into_iter()
            .map(|_| create_buffer(&window, INITIAL_BUFFER_LEN))
            .collect::<Vec<_>>();

        let staging_buffers = (0..2)
            .into_iter()
            .map(|_| create_staging_buffer(&window, INITIAL_BUFFER_LEN))
            .collect::<Vec<_>>();

        let (vertex_upload_task, vertex_upload_task_ids) =
            VertexUploadTask::create_task_graph(&window, worker_flt_id);

        let atlas_clear_buffer = window
            .basalt_ref()
            .device_resources_ref()
            .create_buffer(
                vk::BufferCreateInfo {
                    usage: vk::BufferUsage::TRANSFER_SRC | vk::BufferUsage::TRANSFER_DST,
                    ..Default::default()
                },
                vk::AllocationCreateInfo {
                    memory_type_filter: vk::MemoryTypeFilter {
                        preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        not_preferred_flags: vk::MemoryPropertyFlags::HOST_CACHED,
                        ..vk::MemoryTypeFilter::empty()
                    },
                    allocate_preference: vk::MemoryAllocatePreference::AlwaysAllocate,
                    ..Default::default()
                },
                // TODO: This could be very wrong
                vk::DeviceLayout::new_unsized::<[u8]>(
                    image_format.block_size()
                        * ATLAS_LARGE_THRESHOLD as vk::DeviceSize
                        * ATLAS_LARGE_THRESHOLD as vk::DeviceSize,
                )
                .unwrap(),
            )
            .unwrap();

        unsafe {
            vk::execute(
                window.basalt_ref().transfer_queue_ref(),
                window.basalt_ref().device_resources_ref(),
                worker_flt_id,
                |cmd, _| {
                    cmd.fill_buffer(&vk::FillBufferInfo {
                        dst_buffer: atlas_clear_buffer,
                        size: image_format.block_size()
                            * ATLAS_LARGE_THRESHOLD as vk::DeviceSize
                            * ATLAS_LARGE_THRESHOLD as vk::DeviceSize,
                        data: 0,
                        ..Default::default()
                    })
                    .unwrap();

                    Ok(())
                },
                [],
                [(atlas_clear_buffer, vk::AccessType::CopyTransferWrite)],
                [],
            )
            .unwrap();
        }

        let mut worker = Self {
            window,
            render_flt_id,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format,

            bin_state: BTreeMap::new(),
            pending_work: false,

            update_workers,
            update_work_send,
            update_submission_recv,

            buffers: [
                [buffer_ids[0], buffer_ids[1]],
                [buffer_ids[2], buffer_ids[3]],
            ],
            buffer_update: [false; 2],
            staging_buffers: [staging_buffers[0], staging_buffers[1]],

            image_backings: Vec::new(),
            image_update: [false; 2],
            atlas_clear_buffer,

            vertex_upload_task,
            vertex_upload_task_ids,
        };

        for bin in worker.window.associated_bins() {
            worker.associate_bin(bin);
        }

        std::thread::spawn(move || worker.run());
    }

    fn dissociate_bin(&mut self, bin_id: BinID) {
        if let Some(state) = self.bin_state.get_mut(&bin_id) {
            state.pending_removal = true;
            self.pending_work = true;
        }
    }

    fn associate_bin(&mut self, bin: Arc<Bin>) {
        match self.bin_state.get_mut(&bin.id()) {
            Some(state) => {
                state.pending_removal = false;
            },
            None => {
                self.bin_state.insert(
                    bin.id(),
                    BinState {
                        bin_wk: Arc::downgrade(&bin),
                        pending_removal: false,
                        pending_update: true,
                        images: Vec::new(),
                        vertexes: BTreeMap::new(),
                    },
                );

                self.pending_work = true;
            },
        }
    }

    fn update_bin(&mut self, bin_id: BinID) {
        if let Some(state) = self.bin_state.get_mut(&bin_id) {
            state.pending_update = true;
        }

        self.pending_work = true;
    }

    fn update_all(&mut self) {
        for state in self.bin_state.values_mut() {
            state.pending_update = true;
        }

        self.pending_work = true;
    }

    fn set_extent(&mut self, extent: [u32; 2]) {
        self.update_all();

        for worker in self.update_workers.iter() {
            worker.set_extent(extent);
        }
    }

    fn set_scale(&mut self, scale: f32) {
        self.update_all();

        for worker in self.update_workers.iter() {
            worker.set_scale(scale);
        }
    }

    fn add_binary_font(&self, bytes: Arc<dyn AsRef<[u8]> + Sync + Send>) {
        // TODO: Update all bins with glyph image sources?

        for worker in self.update_workers.iter() {
            worker.add_binary_font(bytes.clone());
        }
    }

    fn set_default_font(&mut self, default_font: DefaultFont) {
        // TODO: Update only those with glyph image sources?
        self.update_all();

        for worker in self.update_workers.iter() {
            worker.set_default_font(default_font.clone());
        }
    }

    fn image_subresource_layers(&self) -> vk::ImageSubresourceLayers {
        vk::ImageSubresourceLayers::from_parameters(self.image_format, 1)
    }

    // TODO:
    // fn set_metrics_level(&self, metrics_level: ());

    fn run(mut self) {
        let mut loop_i = 0_usize;

        let max_image_dimension2_d = self
            .window
            .basalt_ref()
            .physical_device()
            .properties()
            .max_image_dimension2_d;

        'main: loop {
            while !self.pending_work {
                // TODO: Eww about collecting to a vec
                for window_event in self.window_event_recv.drain().collect::<Vec<_>>() {
                    match window_event {
                        WindowEvent::Opened => (),
                        // TODO: Care about device resources? Does the context have to drop first?
                        WindowEvent::Closed => break 'main,
                        WindowEvent::Resized {
                            width,
                            height,
                        } => {
                            if self
                                .render_event_send
                                .send(RenderEvent::CheckExtent)
                                .is_err()
                            {
                                break 'main;
                            }

                            self.set_extent([width, height]);
                        },
                        WindowEvent::ScaleChanged(scale) => {
                            self.set_scale(scale);
                        },
                        WindowEvent::RedrawRequested => (), // TODO:
                        WindowEvent::EnabledFullscreen => (), // TODO: does task graph support?
                        WindowEvent::DisabledFullscreen => (), // TODO: does task graph support?
                        WindowEvent::AssociateBin(bin) => self.associate_bin(bin),
                        WindowEvent::DissociateBin(bin_id) => self.dissociate_bin(bin_id),
                        WindowEvent::UpdateBin(bin_id) => self.update_bin(bin_id),
                        WindowEvent::UpdateBinBatch(bin_ids) => {
                            for bin_id in bin_ids {
                                self.update_bin(bin_id);
                            }
                        },
                        WindowEvent::AddBinaryFont(bytes) => self.add_binary_font(bytes),
                        WindowEvent::SetDefaultFont(default_font) => {
                            self.set_default_font(default_font)
                        },
                        WindowEvent::SetMSAA(msaa) => {
                            if self
                                .render_event_send
                                .send(RenderEvent::SetMSAA(msaa))
                                .is_err()
                            {
                                break 'main;
                            }
                        },
                        WindowEvent::SetVSync(vsync) => {
                            if self
                                .render_event_send
                                .send(RenderEvent::SetVSync(vsync))
                                .is_err()
                            {
                                break 'main;
                            }
                        },
                        WindowEvent::SetMetrics(_metrics_level) => (), // TODO:
                    }
                }
            }

            // --- Remove Bin States --- //

            let mut image_source_remove: HashMap<ImageSource, usize> = HashMap::new();

            self.bin_state.retain(|_, state| {
                if !state.pending_removal {
                    return true;
                }

                for vertex_state in state.vertexes.values() {
                    for (buffer_i, offset_op) in vertex_state.offset.iter().enumerate() {
                        if offset_op.is_some() {
                            self.buffer_update[buffer_i] = true;
                        }
                    }

                    for image_source in state.images.iter() {
                        *image_source_remove.entry(image_source.clone()).or_default() += 1;
                    }
                }

                false
            });

            // --- Obtain Vertex Data --- //

            let mut update_count = 0;

            for state in self.bin_state.values() {
                if state.pending_update {
                    if let Some(bin) = state.bin_wk.upgrade() {
                        self.update_work_send.send(bin).unwrap();
                        update_count += 1;
                    }
                }
            }

            for worker in self.update_workers.iter() {
                worker.perform();
            }

            let mut update_received = 0;
            let mut image_source_add: HashMap<ImageSource, usize> = HashMap::new();

            while update_received < update_count {
                let UpdateSubmission {
                    id,
                    mut images,
                    mut vertexes,
                } = self.update_submission_recv.recv().unwrap();

                update_received += 1;
                let state = self.bin_state.get_mut(&id).unwrap();
                std::mem::swap(&mut images, &mut state.images);
                std::mem::swap(&mut vertexes, &mut state.vertexes);

                for new_image_source in state.images.iter() {
                    if !images.contains(&new_image_source) {
                        *image_source_add
                            .entry(new_image_source.clone())
                            .or_default() += 1;
                    }
                }

                for old_image_source in images.into_iter() {
                    if !state.images.contains(&old_image_source) {
                        *image_source_remove.entry(old_image_source).or_default() += 1;
                    }
                }

                if !state.vertexes.is_empty() {
                    self.buffer_update = [true; 2];
                } else {
                    for old_vertex_state in vertexes.into_values() {
                        for (buffer_i, offset_op) in old_vertex_state.offset.into_iter().enumerate()
                        {
                            self.buffer_update[buffer_i] = true;
                        }
                    }
                }
            }

            // -- Decrease Image Use Counters -- //

            for (image_source, count) in image_source_remove {
                for image_backing in self.image_backings.iter_mut() {
                    match image_backing {
                        ImageBacking::Atlas {
                            allocations, ..
                        } => {
                            if let Some(alloc_state) = allocations.get_mut(&image_source) {
                                alloc_state.uses -= count;
                                break;
                            }
                        },
                        ImageBacking::Dedicated {
                            mut uses, ..
                        } => {
                            uses -= count;
                            break;
                        },
                        ImageBacking::User {
                            mut uses, ..
                        } => {
                            uses -= count;
                            break;
                        },
                    }
                }
            }

            // -- Increase Image Use Counters -- //

            let mut image_source_obtain: HashMap<ImageSource, usize> = HashMap::new();

            for (image_source, count) in image_source_add {
                let mut obtain_image_source = true;

                for image_backing in self.image_backings.iter_mut() {
                    match image_backing {
                        ImageBacking::Atlas {
                            allocations, ..
                        } => {
                            if let Some(alloc_state) = allocations.get_mut(&image_source) {
                                alloc_state.uses += count;
                                obtain_image_source = false;
                                break;
                            }
                        },
                        ImageBacking::Dedicated {
                            mut uses, ..
                        } => {
                            uses += count;
                            obtain_image_source = false;
                            break;
                        },
                        ImageBacking::User {
                            mut uses, ..
                        } => {
                            uses += count;
                            obtain_image_source = false;
                        },
                    }
                }

                if obtain_image_source {
                    *image_source_obtain.entry(image_source).or_default() += count;
                }
            }

            // -- Deref Image Cache Keys & Remove Image Backings -- //

            let mut image_backings_remove = Vec::new();
            let mut image_cache_keys_deref: Vec<ImageCacheKey> = Vec::new();

            for (i, image_backing) in self.image_backings.iter_mut().enumerate() {
                match image_backing {
                    ImageBacking::Atlas {
                        allocator,
                        allocations,
                        pending_clears,
                        ..
                    } => {
                        allocations.retain(|image_source, alloc_state| {
                            if alloc_state.uses == 0 {
                                if let ImageSource::Cache(image_cache_key) = &image_source {
                                    image_cache_keys_deref.push(image_cache_key.clone());
                                    allocator.deallocate(alloc_state.alloc.id);

                                    for j in 0..2 {
                                        pending_clears[j].push([
                                            alloc_state.alloc.rectangle.min.x as u32,
                                            alloc_state.alloc.rectangle.min.y as u32,
                                            alloc_state.alloc.rectangle.width() as u32,
                                            alloc_state.alloc.rectangle.height() as u32,
                                        ]);
                                    }

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
                        uses,
                        ..
                    } => {
                        if *uses == 0 {
                            if let ImageSource::Cache(image_cache_key) = &source {
                                image_cache_keys_deref.push(image_cache_key.clone());
                                image_backings_remove.push(i);
                            }
                        }
                    },
                    ImageBacking::User {
                        source,
                        uses,
                        ..
                    } => {
                        if *uses == 0 {
                            image_backings_remove.push(i);
                        }
                    },
                }
            }

            if !image_backings_remove.is_empty() {
                let mut image_source_effected = HashSet::new();

                for image_backing in self
                    .image_backings
                    .iter()
                    .skip(image_backings_remove[0] + 1)
                {
                    match image_backing {
                        ImageBacking::Atlas {
                            allocations, ..
                        } => {
                            image_source_effected.extend(allocations.keys().cloned());
                        },
                        ImageBacking::Dedicated {
                            source, ..
                        } => {
                            image_source_effected.insert(source.clone());
                        },
                        ImageBacking::User {
                            source, ..
                        } => {
                            image_source_effected.insert(source.clone());
                        },
                    }
                }

                for i in image_backings_remove.into_iter().rev() {
                    self.image_backings.remove(i);
                }

                for bin_state in self.bin_state.values_mut() {
                    if bin_state
                        .images
                        .iter()
                        .any(|image_source| image_source_effected.contains(image_source))
                    {
                        for vertex_state in bin_state.vertexes.values_mut() {
                            vertex_state.offset = [None; 2];
                            vertex_state.staging = [None; 2];
                            self.buffer_update = [true; 2];
                        }
                    }
                }
            }

            // -- Obtain Image Sources -- //

            if !image_source_obtain.is_empty() || !image_cache_keys_deref.is_empty() {
                let image_cache_keys_obtain = image_source_obtain
                    .iter()
                    .filter_map(|(image_source, _)| {
                        match image_source {
                            ImageSource::Cache(image_cache_key) => Some(image_cache_key.clone()),
                            _ => None,
                        }
                    })
                    .collect::<Vec<_>>();

                let mut obtained_images = self.window.basalt_ref().image_cache_ref().obtain_data(
                    image_cache_keys_deref,
                    image_cache_keys_obtain,
                    self.image_format,
                );

                let active_i = buffer_index(loop_i)[0];
                let inactive_i = buffer_index(loop_i + 2)[0];

                'obtain: for (image_source, count) in image_source_obtain {
                    match image_source.clone() {
                        ImageSource::None => unreachable!(),
                        ImageSource::Vulkano(image_id) => {
                            self.image_backings.push(ImageBacking::User {
                                source: image_source,
                                uses: count,
                                image_id,
                            });
                        },
                        ImageSource::Cache(image_cache_key) => {
                            let mut obtained_image =
                                obtained_images.remove(&image_cache_key).unwrap();

                            if obtained_image.width > ATLAS_LARGE_THRESHOLD - 2
                                || obtained_image.height > ATLAS_LARGE_THRESHOLD - 2
                            {
                                let image_id = create_image(
                                    &self.window,
                                    self.image_format,
                                    obtained_image.width,
                                    obtained_image.height,
                                );

                                self.image_backings.push(ImageBacking::Dedicated {
                                    source: image_source,
                                    uses: count,
                                    image_id,
                                    write_info: Some((
                                        obtained_image.width,
                                        obtained_image.height,
                                        obtained_image.data,
                                    )),
                                });

                                self.image_update[active_i] = true;
                                continue;
                            }

                            let alloc_size = AtlasSize::new(
                                obtained_image.width.max(ATLAS_SMALL_THRESHOLD - 2) as i32 + 2,
                                obtained_image.height.max(ATLAS_SMALL_THRESHOLD - 2) as i32 + 2,
                            );

                            for image_backing in self.image_backings.iter_mut() {
                                if let ImageBacking::Atlas {
                                    allocator,
                                    allocations,
                                    pending_uploads,
                                    staging_write,
                                    resize_images,
                                    pending_clears,
                                    ..
                                } = image_backing
                                {
                                    let alloc_op = match allocator.allocate(alloc_size) {
                                        Some(some) => Some(some),
                                        None => {
                                            if allocator.size().width as u32 * 2
                                                < max_image_dimension2_d
                                            {
                                                *resize_images = [true; 2];

                                                let old_width = allocator.size().width as u32;
                                                let old_height = allocator.size().height as u32;

                                                for i in 0..2 {
                                                    pending_clears[i].append(&mut atlas_clears(
                                                        old_width..(old_width * 2),
                                                        old_height..(old_height * 2),
                                                    ));
                                                }

                                                allocator.grow(AtlasSize::new(
                                                    allocator.size().width * 2,
                                                    allocator.size().height * 2,
                                                ));

                                                Some(allocator.allocate(alloc_size).unwrap())
                                            } else {
                                                None
                                            }
                                        },
                                    };

                                    if let Some(alloc) = alloc_op {
                                        let buffer_offset =
                                            staging_write[active_i].len() as vk::DeviceSize;
                                        staging_write[active_i].append(&mut obtained_image.data);

                                        pending_uploads[active_i].push(StagedAtlasUpload {
                                            staging_write_i: active_i,
                                            buffer_offset,
                                            image_offset: [
                                                alloc.rectangle.min.x as u32 + 1,
                                                alloc.rectangle.min.y as u32 + 1,
                                                0,
                                            ],
                                            image_extent: [
                                                obtained_image.width,
                                                obtained_image.height,
                                                1,
                                            ],
                                        });

                                        allocations.insert(
                                            image_source,
                                            AtlasAllocationState {
                                                alloc,
                                                uses: count,
                                            },
                                        );

                                        self.image_update = [true; 2];
                                        continue 'obtain;
                                    }
                                }
                            }

                            let mut allocator = AtlasAllocator::with_options(
                                AtlasSize::new(4096, 4096),
                                &AtlasAllocatorOptions {
                                    alignment: AtlasSize::new(
                                        ATLAS_SMALL_THRESHOLD as i32,
                                        ATLAS_SMALL_THRESHOLD as i32,
                                    ),
                                    small_size_threshold: ATLAS_SMALL_THRESHOLD as i32,
                                    large_size_threshold: ATLAS_LARGE_THRESHOLD as i32,
                                },
                            );

                            let mut allocations = HashMap::new();
                            let mut staging_write = [Vec::new(), Vec::new()];
                            let mut pending_uploads = [Vec::new(), Vec::new()];
                            let alloc = allocator.allocate(alloc_size).unwrap();
                            staging_write[active_i].append(&mut obtained_image.data);

                            pending_uploads[active_i].push(StagedAtlasUpload {
                                staging_write_i: active_i,
                                buffer_offset: 0,
                                image_offset: [
                                    alloc.rectangle.min.x as u32 + 1,
                                    alloc.rectangle.min.y as u32 + 1,
                                    0,
                                ],
                                image_extent: [obtained_image.width, obtained_image.height, 1],
                            });

                            allocations.insert(
                                image_source,
                                AtlasAllocationState {
                                    alloc,
                                    uses: count,
                                },
                            );

                            self.image_backings.push(ImageBacking::Atlas {
                                allocator,
                                allocations,
                                images: [
                                    create_image(
                                        &self.window,
                                        self.image_format,
                                        ATLAS_LARGE_THRESHOLD * 4,
                                        ATLAS_LARGE_THRESHOLD * 4,
                                    ),
                                    create_image(
                                        &self.window,
                                        self.image_format,
                                        ATLAS_LARGE_THRESHOLD * 4,
                                        ATLAS_LARGE_THRESHOLD * 4,
                                    ),
                                ],
                                staging_buffers: [
                                    create_image_staging_buffer(
                                        &self.window,
                                        self.image_format,
                                        4096,
                                        4096,
                                        true,
                                    ),
                                    create_image_staging_buffer(
                                        &self.window,
                                        self.image_format,
                                        4096,
                                        4096,
                                        true,
                                    ),
                                ],
                                pending_clears: [
                                    atlas_clears(
                                        0..(ATLAS_LARGE_THRESHOLD * 4),
                                        0..(ATLAS_LARGE_THRESHOLD * 4),
                                    ),
                                    atlas_clears(
                                        0..(ATLAS_LARGE_THRESHOLD * 4),
                                        0..(ATLAS_LARGE_THRESHOLD * 4),
                                    ),
                                ],
                                pending_uploads,
                                staging_write,
                                resize_images: [false; 2],
                            });

                            self.image_update = [true; 2];
                        },
                    }
                }
            }

            // --- Image Updates --- //

            let mut remove_image_ids: Vec<vk::Id<vk::Image>> = Vec::new();
            let mut remove_buffer_ids: Vec<vk::Id<vk::Buffer>> = Vec::new();
            let active_i = buffer_index(loop_i)[0];

            if self.image_update[active_i] {
                let mut host_buffer_accesses = HashMap::new();
                let mut buffer_accesses = HashMap::new();
                let mut image_accesses = HashMap::new();

                let mut op_image_copy: Vec<[vk::Id<vk::Image>; 2]> = Vec::new();
                let mut op_image_clear: Vec<(vk::Id<vk::Image>, [u32; 4])> = Vec::new();
                let mut op_staging_write: Vec<(vk::Id<vk::Buffer>, Vec<u8>)> = Vec::new();

                let mut op_image_write: Vec<(
                    vk::Id<vk::Buffer>,
                    vk::Id<vk::Image>,
                    Vec<(vk::DeviceSize, [u32; 3], [u32; 3])>,
                )> = Vec::new();

                let image_subresource_layers = self.image_subresource_layers();

                for image_backing in self.image_backings.iter_mut() {
                    match image_backing {
                        ImageBacking::Atlas {
                            allocator,
                            images,
                            staging_buffers,
                            pending_clears,
                            pending_uploads,
                            staging_write,
                            resize_images,
                            ..
                        } => {
                            if resize_images[active_i] {
                                let old_image = images[active_i];
                                let old_staging_buffer = staging_buffers[active_i];

                                images[active_i] = create_image(
                                    &self.window,
                                    self.image_format,
                                    allocator.size().width as u32,
                                    allocator.size().height as u32,
                                );

                                staging_buffers[active_i] = create_image_staging_buffer(
                                    &self.window,
                                    self.image_format,
                                    allocator.size().width as u32,
                                    allocator.size().height as u32,
                                    true,
                                );

                                op_image_copy.push([old_image, images[active_i]]);

                                image_accesses.insert(old_image, vk::AccessType::CopyTransferRead);
                                image_accesses
                                    .insert(images[active_i], vk::AccessType::CopyTransferWrite);

                                remove_image_ids.push(old_image);
                                remove_buffer_ids.push(old_staging_buffer);

                                resize_images[active_i] = false;
                            }

                            if !pending_clears[active_i].is_empty() {
                                op_image_clear.extend(
                                    pending_clears[active_i]
                                        .drain(..)
                                        .map(|xywh| (images[active_i], xywh)),
                                );

                                image_accesses
                                    .insert(images[active_i], vk::AccessType::CopyTransferWrite);

                                buffer_accesses.insert(
                                    self.atlas_clear_buffer,
                                    vk::AccessType::CopyTransferRead,
                                );
                            }

                            if !staging_write[active_i].is_empty() {
                                op_staging_write.push((
                                    staging_buffers[active_i],
                                    staging_write[active_i].split_off(0),
                                ));

                                host_buffer_accesses
                                    .insert(staging_buffers[active_i], vk::HostAccessType::Write);
                            }

                            if !pending_uploads[active_i].is_empty() {
                                let mut write_info = [Vec::new(), Vec::new()];

                                for StagedAtlasUpload {
                                    staging_write_i,
                                    buffer_offset,
                                    image_offset,
                                    image_extent,
                                } in pending_uploads[active_i].drain(..)
                                {
                                    write_info[staging_write_i].push((
                                        buffer_offset,
                                        image_offset,
                                        image_extent,
                                    ));
                                }

                                for (i, write_info) in write_info.into_iter().enumerate() {
                                    if write_info.is_empty() {
                                        continue;
                                    }

                                    buffer_accesses.insert(
                                        staging_buffers[i],
                                        vk::AccessType::CopyTransferRead,
                                    );

                                    image_accesses.insert(
                                        images[active_i],
                                        vk::AccessType::CopyTransferWrite,
                                    );

                                    op_image_write.push((
                                        staging_buffers[i],
                                        images[active_i],
                                        write_info,
                                    ));
                                }
                            }
                        },
                        ImageBacking::Dedicated {
                            image_id,
                            write_info,
                            ..
                        } => {
                            if let Some((w, h, staging_write)) = write_info.take() {
                                let buffer_id = create_image_staging_buffer(
                                    &self.window,
                                    self.image_format,
                                    w,
                                    h,
                                    false,
                                );

                                op_staging_write.push((buffer_id, staging_write));

                                op_image_write.push((
                                    buffer_id,
                                    *image_id,
                                    vec![(0, [0; 3], [w, h, 1])],
                                ));

                                host_buffer_accesses.insert(buffer_id, vk::HostAccessType::Write);
                                buffer_accesses.insert(buffer_id, vk::AccessType::CopyTransferRead);
                                image_accesses.insert(*image_id, vk::AccessType::CopyTransferWrite);

                                remove_buffer_ids.push(buffer_id);
                            }
                        },
                        ImageBacking::User {
                            ..
                        } => (),
                    }
                }

                unsafe {
                    vk::execute(
                        self.window.basalt_ref().transfer_queue_ref(),
                        self.window.basalt_ref().device_resources_ref(),
                        self.worker_flt_id,
                        |cmd, task| {
                            // TODO: !!!

                            Ok(())
                        },
                        host_buffer_accesses.into_iter(),
                        buffer_accesses.into_iter(),
                        image_accesses
                            .into_iter()
                            .map(|(image, access)| (image, access, vk::ImageLayoutType::Optimal)),
                    )
                    .unwrap()
                }

                for image_id in remove_image_ids {
                    unsafe {
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .remove_image(image_id);
                    }
                }

                for buffer_id in remove_buffer_ids {
                    unsafe {
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .remove_buffer(buffer_id);
                    }
                }
            }

            // --- Vertex Updates --- //

            if self.buffer_update[active_i] {
                let src_buf_i = buffer_index(loop_i + 2);
                let src_buf_id = self.buffers[src_buf_i[0]][src_buf_i[1]];
                let dst_buf_i = buffer_index(loop_i);
                let mut dst_buf_id = self.buffers[dst_buf_i[0]][dst_buf_i[1]];
                let mut stage_buf_id = self.staging_buffers[src_buf_i[0]];
                let prev_stage_buf_i = buffer_index(loop_i + 1)[0];
                let prev_stage_buf_id = self.staging_buffers[prev_stage_buf_i];

                // -- Count Vertexes -- //

                let mut count_by_z: BTreeMap<OrderedFloat<f32>, vk::DeviceSize> = BTreeMap::new();

                for state in self.bin_state.values() {
                    for (z, vertex_state) in state.vertexes.iter() {
                        *count_by_z.entry(*z).or_default() += vertex_state.total as vk::DeviceSize;
                    }
                }

                let mut total_count = count_by_z.values().sum::<vk::DeviceSize>();

                // -- Check Buffer Size -- //

                let new_buffer_size_op = {
                    let dst_buf_state = self
                        .window
                        .basalt_ref()
                        .device_resources_ref()
                        .buffer(dst_buf_id)
                        .unwrap();

                    if dst_buf_state.buffer().size() < total_count * VERTEX_SIZE {
                        let mut new_buffer_size = dst_buf_state.buffer().size();

                        while new_buffer_size < total_count * VERTEX_SIZE {
                            new_buffer_size *= 2;
                        }

                        Some(new_buffer_size)
                    } else {
                        None
                    }
                };

                if let Some(new_buffer_size) = new_buffer_size_op {
                    let new_buf_id = create_buffer(&self.window, new_buffer_size / VERTEX_SIZE);

                    unsafe {
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .remove_buffer(dst_buf_id);
                    }

                    dst_buf_id = new_buf_id;
                    self.buffers[dst_buf_i[0]][dst_buf_i[1]] = dst_buf_id;

                    if self
                        .window
                        .basalt_ref()
                        .device_resources_ref()
                        .buffer(stage_buf_id)
                        .unwrap()
                        .buffer()
                        .size()
                        != new_buffer_size
                    {
                        let new_staging_buf_id =
                            create_staging_buffer(&self.window, new_buffer_size / VERTEX_SIZE);

                        unsafe {
                            self.window
                                .basalt_ref()
                                .device_resources_ref()
                                .remove_buffer(stage_buf_id);
                        }

                        stage_buf_id = new_staging_buf_id;
                        self.staging_buffers[dst_buf_i[0]] = new_staging_buf_id;
                    }
                }

                // -- Prepare Vertex Operations -- //

                let mut z_dst_offset: BTreeMap<OrderedFloat<f32>, vk::DeviceSize> = BTreeMap::new();
                let mut cumulative_offset = 0;

                for (z, count) in count_by_z {
                    z_dst_offset.insert(z, cumulative_offset);
                    cumulative_offset += count * VERTEX_SIZE;
                }

                let mut copy_from_prev: Vec<vk::BufferCopy> = Vec::new();
                let mut copy_from_prev_stage: Vec<vk::BufferCopy> = Vec::new();
                let mut copy_from_curr_stage: Vec<vk::BufferCopy> = Vec::new();
                let mut staging_write = Vec::new(); // TODO: Specify Capacity?

                for bin_state in self.bin_state.values_mut() {
                    for (z, vertex_state) in bin_state.vertexes.iter_mut() {
                        let size = vertex_state.total as vk::DeviceSize * VERTEX_SIZE;
                        let prev_stage_offset = vertex_state.staging[prev_stage_buf_i].take();

                        let dst_offset = {
                            let zdo = z_dst_offset.get_mut(&z).unwrap();
                            let dst_offset = *zdo;
                            *zdo += size;
                            dst_offset
                        };

                        if let Some(src_offset) = vertex_state.offset[src_buf_i[0]] {
                            copy_from_prev.push(vk::BufferCopy {
                                src_offset,
                                dst_offset,
                                size,
                                ..Default::default()
                            });

                            continue;
                        }

                        if let Some(src_offset) = prev_stage_offset {
                            copy_from_prev_stage.push(vk::BufferCopy {
                                src_offset,
                                dst_offset,
                                size,
                                ..Default::default()
                            });

                            continue;
                        }

                        let src_offset = staging_write.len() as vk::DeviceSize * VERTEX_SIZE;

                        for (_image_source, vertexes) in vertex_state.data.iter() {
                            for vertex in vertexes {
                                // TODO: Modify coords & tex_i
                                staging_write.push(vertex.clone());
                            }
                        }

                        copy_from_curr_stage.push(vk::BufferCopy {
                            src_offset,
                            dst_offset,
                            size,
                            ..Default::default()
                        });
                    }
                }

                // -- Execute Vertex Operations -- //

                let resource_map = vk::resource_map!(
                    &self.vertex_upload_task,
                    self.vertex_upload_task_ids.prev_stage_buffer => prev_stage_buf_id,
                    self.vertex_upload_task_ids.curr_stage_buffer => stage_buf_id,
                    self.vertex_upload_task_ids.prev_buffer => src_buf_id,
                    self.vertex_upload_task_ids.curr_buffer => dst_buf_id,
                )
                .unwrap();

                let world = VertexUploadTaskWorld {
                    copy_from_prev,
                    copy_from_prev_stage,
                    copy_from_curr_stage,
                    staging_write,
                };

                unsafe {
                    self.vertex_upload_task
                        .execute(resource_map, &world, || {})
                        .unwrap();
                }

                self.window
                    .basalt_ref()
                    .device_resources_ref()
                    .flight(self.worker_flt_id)
                    .unwrap()
                    .wait(None)
                    .unwrap();

                // TODO: execute image work also, before sending update

                let barrier = Arc::new(Barrier::new(2));

                if let Err(_) = self.render_event_send.send(RenderEvent::Update {
                    buffer_id: dst_buf_id,
                    image_ids: Vec::new(),
                    draw_count: total_count as u32,
                    barrier: barrier.clone(),
                }) {
                    break 'main;
                }

                barrier.wait();
            }

            if self.buffer_update[active_i] || self.image_update[active_i] {
                // TODO: This is very broken!
            }

            self.pending_work = false;
            loop_i = loop_i.overflowing_add(1).0;
        }
    }
}

fn buffer_index(i: usize) -> [usize; 2] {
    [i % 2, (i & 0x2) >> 1]
}

fn create_buffer(window: &Arc<Window>, len: vk::DeviceSize) -> vk::Id<vk::Buffer> {
    window
        .basalt_ref()
        .device_resources_ref()
        .create_buffer(
            vk::BufferCreateInfo {
                usage: vk::BufferUsage::TRANSFER_SRC
                    | vk::BufferUsage::TRANSFER_DST
                    | vk::BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            vk::AllocationCreateInfo {
                memory_type_filter: vk::MemoryTypeFilter {
                    preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                    not_preferred_flags: vk::MemoryPropertyFlags::HOST_CACHED,
                    ..vk::MemoryTypeFilter::empty()
                },
                allocate_preference: vk::MemoryAllocatePreference::AlwaysAllocate,
                ..Default::default()
            },
            vk::DeviceLayout::new_unsized::<[ItfVertInfo]>(len).unwrap(),
        )
        .unwrap()
}

fn create_staging_buffer(window: &Arc<Window>, len: vk::DeviceSize) -> vk::Id<vk::Buffer> {
    window
        .basalt_ref()
        .device_resources_ref()
        .create_buffer(
            vk::BufferCreateInfo {
                usage: vk::BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            vk::AllocationCreateInfo {
                memory_type_filter: vk::MemoryTypeFilter {
                    required_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                    not_preferred_flags: vk::MemoryPropertyFlags::HOST_CACHED
                        | vk::MemoryPropertyFlags::DEVICE_COHERENT,
                    ..vk::MemoryTypeFilter::empty()
                },
                allocate_preference: vk::MemoryAllocatePreference::AlwaysAllocate,
                ..Default::default()
            },
            vk::DeviceLayout::new_unsized::<[ItfVertInfo]>(len).unwrap(),
        )
        .unwrap()
}

fn create_image(
    window: &Arc<Window>,
    format: vk::Format,
    width: u32,
    height: u32,
) -> vk::Id<vk::Image> {
    window
        .basalt_ref()
        .device_resources_ref()
        .create_image(
            vk::ImageCreateInfo {
                image_type: vk::ImageType::Dim2d,
                format,
                extent: [width, height, 1],
                usage: vk::ImageUsage::TRANSFER_DST
                    | vk::ImageUsage::TRANSFER_SRC
                    | vk::ImageUsage::SAMPLED,
                ..Default::default()
            },
            vk::AllocationCreateInfo {
                memory_type_filter: vk::MemoryTypeFilter {
                    preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                    not_preferred_flags: vk::MemoryPropertyFlags::HOST_CACHED,
                    ..vk::MemoryTypeFilter::empty()
                },
                allocate_preference: vk::MemoryAllocatePreference::AlwaysAllocate,
                ..Default::default()
            },
        )
        .unwrap()
}

fn create_image_staging_buffer(
    window: &Arc<Window>,
    format: vk::Format,
    width: u32,
    height: u32,
    long_lived: bool,
) -> vk::Id<vk::Buffer> {
    window
        .basalt_ref()
        .device_resources_ref()
        .create_buffer(
            vk::BufferCreateInfo {
                usage: vk::BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            vk::AllocationCreateInfo {
                memory_type_filter: vk::MemoryTypeFilter {
                    required_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                    not_preferred_flags: vk::MemoryPropertyFlags::HOST_CACHED
                        | vk::MemoryPropertyFlags::DEVICE_COHERENT,
                    ..vk::MemoryTypeFilter::empty()
                },
                allocate_preference: if long_lived {
                    vk::MemoryAllocatePreference::AlwaysAllocate
                } else {
                    vk::MemoryAllocatePreference::Unknown
                },
                ..Default::default()
            },
            // TODO: This could be very wrong
            vk::DeviceLayout::new_unsized::<[u8]>(
                format.block_size() * width as vk::DeviceSize * height as vk::DeviceSize,
            )
            .unwrap(),
        )
        .unwrap()
}

fn atlas_clears(xr: Range<u32>, yr: Range<u32>) -> Vec<[u32; 4]> {
    let mut regions = Vec::new();

    for x in (xr.start / ATLAS_LARGE_THRESHOLD)..(xr.end / ATLAS_LARGE_THRESHOLD) {
        for y in (yr.start / ATLAS_LARGE_THRESHOLD)..(yr.end / ATLAS_LARGE_THRESHOLD) {
            regions.push([
                x * ATLAS_LARGE_THRESHOLD,
                y * ATLAS_LARGE_THRESHOLD,
                ATLAS_LARGE_THRESHOLD,
                ATLAS_LARGE_THRESHOLD,
            ]);
        }
    }

    regions
}

#[derive(Clone)]
struct VertexUploadTask {
    prev_stage_buffer: vk::Id<vk::Buffer>,
    curr_stage_buffer: vk::Id<vk::Buffer>,
    prev_buffer: vk::Id<vk::Buffer>,
    curr_buffer: vk::Id<vk::Buffer>,
}

struct VertexUploadTaskWorld {
    copy_from_prev: Vec<vk::BufferCopy<'static>>,
    copy_from_prev_stage: Vec<vk::BufferCopy<'static>>,
    copy_from_curr_stage: Vec<vk::BufferCopy<'static>>,
    staging_write: Vec<ItfVertInfo>,
}

impl VertexUploadTask {
    pub fn create_task_graph(
        window: &Arc<Window>,
        worker_flt_id: vk::Id<vk::Flight>,
    ) -> (vk::ExecutableTaskGraph<VertexUploadTaskWorld>, Self) {
        let mut task_graph = vk::TaskGraph::new(window.basalt_ref().device_resources_ref(), 1, 4);

        let prev_stage_buffer = task_graph.add_buffer(&vk::BufferCreateInfo {
            usage: vk::BufferUsage::TRANSFER_SRC,
            ..Default::default()
        });

        let curr_stage_buffer = task_graph.add_buffer(&vk::BufferCreateInfo {
            usage: vk::BufferUsage::TRANSFER_SRC,
            ..Default::default()
        });

        let prev_buffer = task_graph.add_buffer(&vk::BufferCreateInfo {
            usage: vk::BufferUsage::TRANSFER_SRC
                | vk::BufferUsage::TRANSFER_DST
                | vk::BufferUsage::VERTEX_BUFFER,
            ..Default::default()
        });

        let curr_buffer = task_graph.add_buffer(&vk::BufferCreateInfo {
            usage: vk::BufferUsage::TRANSFER_SRC
                | vk::BufferUsage::TRANSFER_DST
                | vk::BufferUsage::VERTEX_BUFFER,
            ..Default::default()
        });

        let this = Self {
            prev_stage_buffer,
            curr_stage_buffer,
            prev_buffer,
            curr_buffer,
        };

        task_graph.add_host_buffer_access(curr_stage_buffer, vk::HostAccessType::Write);

        task_graph
            .create_task_node(
                format!("VertexUpload[{:?}]", window.id()),
                vk::QueueFamilyType::Transfer,
                this.clone(),
            )
            .buffer_access(prev_stage_buffer, vk::AccessType::CopyTransferRead)
            .buffer_access(curr_stage_buffer, vk::AccessType::CopyTransferRead)
            .buffer_access(prev_buffer, vk::AccessType::CopyTransferRead)
            .buffer_access(curr_buffer, vk::AccessType::CopyTransferWrite);

        (
            unsafe {
                task_graph
                    .compile(&vk::CompileInfo {
                        queues: &[window.basalt_ref().transfer_queue_ref()],
                        flight_id: worker_flt_id,
                        ..Default::default()
                    })
                    .unwrap()
            },
            this,
        )
    }
}

impl vk::Task for VertexUploadTask {
    type World = VertexUploadTaskWorld;

    unsafe fn execute(
        &self,
        cmd: &mut vk::RecordingCommandBuffer<'_>,
        task: &mut vk::TaskContext<'_>,
        world: &Self::World,
    ) -> vk::TaskResult {
        if !world.staging_write.is_empty() {
            task.write_buffer::<[ItfVertInfo]>(
                self.curr_stage_buffer,
                ..(world.staging_write.len() as vk::DeviceSize * VERTEX_SIZE),
            )
            .unwrap()
            .clone_from_slice(world.staging_write.as_slice());
        }

        if !world.copy_from_prev.is_empty() {
            cmd.copy_buffer(&vk::CopyBufferInfo {
                src_buffer: self.prev_buffer,
                dst_buffer: self.curr_buffer,
                regions: world.copy_from_prev.as_slice(),
                ..Default::default()
            })
            .unwrap();
        }

        if !world.copy_from_prev_stage.is_empty() {
            cmd.copy_buffer(&vk::CopyBufferInfo {
                src_buffer: self.prev_stage_buffer,
                dst_buffer: self.curr_buffer,
                regions: world.copy_from_prev_stage.as_slice(),
                ..Default::default()
            })
            .unwrap();
        }

        if !world.copy_from_curr_stage.is_empty() {
            cmd.copy_buffer(&vk::CopyBufferInfo {
                src_buffer: self.curr_stage_buffer,
                dst_buffer: self.curr_buffer,
                regions: world.copy_from_curr_stage.as_slice(),
                ..Default::default()
            })
            .unwrap();
        }

        Ok(())
    }
}
