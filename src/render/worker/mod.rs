mod update;

use std::collections::BTreeMap;
use std::ops::{AddAssign, DivAssign, Range};
use std::sync::{Arc, Barrier, Weak};
use std::thread::JoinHandle;
use std::time::Instant;

use flume::{Receiver, Sender};
use foldhash::{HashMap, HashMapExt};
use guillotiere::{
    Allocation as AtlasAllocation, AllocatorOptions as AtlasAllocatorOptions, AtlasAllocator,
    Size as AtlasSize,
};
use ordered_float::OrderedFloat;
use parking_lot::{Condvar, Mutex};
use smallvec::SmallVec;

mod vko {
    pub use vulkano::DeviceSize;
    pub use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
    pub use vulkano::format::Format;
    pub use vulkano::image::{
        Image, ImageCreateInfo, ImageLayout, ImageSubresourceLayers, ImageSubresourceRange,
        ImageType, ImageUsage,
    };
    pub use vulkano::memory::MemoryPropertyFlags;
    pub use vulkano::memory::allocator::{
        AllocationCreateInfo, DeviceLayout, MemoryAllocatePreference, MemoryTypeFilter,
    };
    pub use vulkano::sync::{AccessFlags, PipelineStages, Sharing};
    pub use vulkano_taskgraph::command_buffer::{
        BufferCopy, BufferImageCopy, CopyBufferInfo, CopyBufferToImageInfo, CopyImageInfo,
        DependencyInfo, FillBufferInfo, ImageMemoryBarrier, RecordingCommandBuffer,
    };
    pub use vulkano_taskgraph::graph::{CompileInfo, ExecutableTaskGraph, TaskGraph};
    pub use vulkano_taskgraph::resource::{AccessTypes, Flight, HostAccessType, ImageLayoutType};
    pub use vulkano_taskgraph::{
        Id, QueueFamilyType, Task, TaskContext, TaskResult, execute, resource_map,
    };
}

use self::update::{UpdateSubmission, UpdateWorker};
use crate::image::{ImageKey, ImageMap, ImageSet};
use crate::interface::{Bin, BinID, DefaultFont, ItfVertInfo, OVDPerfMetrics, UpdateContext};
use crate::render::{
    RenderEvent, RendererMetricsLevel, VulkanoError, WorkerCreateError, WorkerError,
};
use crate::window::{Window, WindowEvent};

const VERTEX_SIZE: vko::DeviceSize = std::mem::size_of::<ItfVertInfo>() as vko::DeviceSize;
const INITIAL_BUFFER_LEN: vko::DeviceSize = 32768;

const ATLAS_SMALL_THRESHOLD: u32 = 16;
const ATLAS_LARGE_THRESHOLD: u32 = 512;
const ATLAS_DEFAULT_SIZE: u32 = ATLAS_LARGE_THRESHOLD * 4;

pub struct SpawnInfo {
    pub window: Arc<Window>,
    pub window_event_recv: Receiver<WindowEvent>,
    pub render_event_send: Sender<RenderEvent>,
    pub image_format: vko::Format,
    pub render_flt_id: vko::Id<vko::Flight>,
    pub resource_sharing: vko::Sharing<SmallVec<[u32; 4]>>,
}

/// Performance metrics of a `Renderer`'s worker.
#[derive(Debug, Clone, Default)]
pub struct WorkerPerfMetrics {
    pub total: f32,
    pub bin_count: usize,
    pub bin_remove: f32,
    pub bin_obtain: f32,
    pub image_count: f32,
    pub image_remove: f32,
    pub image_obtain: f32,
    pub image_update_prep: f32,
    pub vertex_count: f32,
    pub vertex_update_prep: f32,
    pub swap_wait: f32,
    pub execution: f32,
    pub ovd_metrics: Option<OVDPerfMetrics>,
}

impl AddAssign for WorkerPerfMetrics {
    fn add_assign(&mut self, mut rhs: Self) {
        self.total += rhs.total;
        self.bin_count += rhs.bin_count;
        self.bin_remove += rhs.bin_remove;
        self.bin_obtain += rhs.bin_obtain;
        self.image_count += rhs.image_count;
        self.image_remove += rhs.image_remove;
        self.image_obtain += rhs.image_obtain;
        self.image_update_prep += rhs.image_update_prep;
        self.vertex_count += rhs.vertex_count;
        self.vertex_update_prep += rhs.vertex_update_prep;
        self.swap_wait += rhs.swap_wait;
        self.execution += rhs.execution;

        if let Some(rhs_ovd_metrics) = rhs.ovd_metrics.take() {
            match self.ovd_metrics.as_mut() {
                Some(ovd_metrics) => {
                    *ovd_metrics += rhs_ovd_metrics;
                },
                None => {
                    self.ovd_metrics = Some(rhs_ovd_metrics);
                },
            }
        }
    }
}

impl DivAssign<f32> for WorkerPerfMetrics {
    fn div_assign(&mut self, rhs: f32) {
        self.total /= rhs;
        self.bin_count = (self.bin_count as f32 / rhs).trunc() as usize;
        self.bin_remove /= rhs;
        self.bin_obtain /= rhs;
        self.image_count /= rhs;
        self.image_remove /= rhs;
        self.image_obtain /= rhs;
        self.image_update_prep /= rhs;
        self.vertex_count /= rhs;
        self.vertex_update_prep /= rhs;
        self.swap_wait /= rhs;
        self.execution /= rhs;

        if let Some(ovd_metrics) = self.ovd_metrics.as_mut() {
            *ovd_metrics /= rhs;
        }
    }
}

struct MetricsState {
    inner: WorkerPerfMetrics,
    start: Instant,
    last_segment: u128,
}

struct BinState {
    bin_wk: Weak<Bin>,
    pending_removal: bool,
    pending_update: bool,
    images: ImageSet,
    vertexes: BTreeMap<OrderedFloat<f32>, VertexState>,
}

struct VertexState {
    offset: [Option<vko::DeviceSize>; 2],
    staging: [Option<vko::DeviceSize>; 2],
    data: ImageMap<Vec<ItfVertInfo>>,
    total: usize,
}

enum ImageBacking {
    Atlas {
        allocator: Box<AtlasAllocator>,
        allocations: ImageMap<AtlasAllocationState>,
        images: [vko::Id<vko::Image>; 2],
        staging_buffers: [vko::Id<vko::Buffer>; 2],
        pending_clears: [Vec<[u32; 4]>; 2],
        pending_uploads: [Vec<StagedAtlasUpload>; 2],
        staging_write: [Vec<u8>; 2],
        resize_images: [bool; 2],
    },
    Dedicated {
        key: ImageKey,
        uses: usize,
        image_id: vko::Id<vko::Image>,
        write_info: Option<(u32, u32, Vec<u8>)>,
    },
    User {
        key: ImageKey,
        uses: usize,
        image_id: vko::Id<vko::Image>,
    },
}

struct StagedAtlasUpload {
    staging_write_i: usize,
    buffer_offset: vko::DeviceSize,
    image_offset: [u32; 3],
    image_extent: [u32; 3],
}

struct AtlasAllocationState {
    alloc: AtlasAllocation,
    uses: usize,
}

#[derive(Default)]
struct Indexes {
    vertex: usize,
    vertex_sub: [usize; 2],
    image: usize,
}

impl Indexes {
    fn prev_vertex(&self) -> usize {
        self.vertex ^ 1
    }

    fn curr_vertex(&self) -> usize {
        self.vertex
    }

    fn next_vertex(&self) -> usize {
        self.vertex ^ 1
    }

    fn prev_vertex_sub(&self) -> [usize; 2] {
        [self.vertex ^ 1, self.vertex_sub[self.vertex ^ 1] ^ 1]
    }

    fn curr_vertex_sub(&self) -> [usize; 2] {
        [self.vertex, self.vertex_sub[self.vertex]]
    }

    fn curr_vertex_prev_sub(&self) -> [usize; 2] {
        [self.vertex, self.vertex_sub[self.vertex] ^ 1]
    }

    fn adv_vertex(&mut self) {
        self.vertex_sub[self.vertex] ^= 1;
        self.vertex ^= 1;
    }

    fn adv_vertex_no_sub(&mut self) {
        self.vertex ^= 1;
    }

    fn adv_vertex_sub(&mut self) {
        self.vertex_sub[self.vertex] ^= 1;
    }

    fn prev_image(&self) -> usize {
        self.image ^ 1
    }

    fn curr_image(&self) -> usize {
        self.image
    }

    fn adv_image(&mut self) {
        self.image ^= 1
    }
}

pub struct Worker {
    window: Arc<Window>,
    render_flt_id: vko::Id<vko::Flight>,
    vertex_flt_id: vko::Id<vko::Flight>,
    image_flt_id: vko::Id<vko::Flight>,
    window_event_recv: Receiver<WindowEvent>,
    render_event_send: Sender<RenderEvent>,
    image_format: vko::Format,
    resource_sharing: vko::Sharing<SmallVec<[u32; 4]>>,
    current_extent: [u32; 2],

    metrics_level: RendererMetricsLevel,
    metrics_state: Option<MetricsState>,

    bin_state: BTreeMap<BinID, BinState>,
    pending_work: bool,

    update_workers: Vec<UpdateWorker>,
    update_work_send: Sender<Arc<Bin>>,
    update_submission_recv: Receiver<UpdateSubmission>,

    buffers: [[vko::Id<vko::Buffer>; 2]; 2],
    buffer_update: [bool; 2],
    buffer_total: [u32; 2],
    staging_buffers: [vko::Id<vko::Buffer>; 2],

    image_backings: Vec<ImageBacking>,
    image_update: [bool; 2],
    atlas_clear_buffer: vko::Id<vko::Buffer>,

    vertex_upload_task: vko::ExecutableTaskGraph<VertexUploadTaskWorld>,
    vertex_upload_task_ids: VertexUploadTask,
}

impl Worker {
    pub fn spawn(
        spawn_info: SpawnInfo,
    ) -> Result<JoinHandle<Result<(), WorkerError>>, WorkerCreateError> {
        let SpawnInfo {
            window,
            window_event_recv,
            render_event_send,
            image_format,
            render_flt_id,
            resource_sharing,
        } = spawn_info;

        let vertex_flt_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_flight(1)
            .map_err(VulkanoError::CreateFlight)?;

        let image_flt_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_flight(1)
            .map_err(VulkanoError::CreateFlight)?;

        let update_threads = window
            .basalt_ref()
            .config
            .render_default_worker_threads
            .get();

        let mut update_contexts = Vec::with_capacity(update_threads);
        update_contexts.push(UpdateContext::from(&window));
        let metrics_level = update_contexts[0].metrics_level;

        let current_extent = [
            update_contexts[0].extent[0] as u32,
            update_contexts[0].extent[1] as u32,
        ];

        render_event_send
            .send(RenderEvent::SetMetricsLevel(metrics_level))
            .unwrap();

        while update_contexts.len() < update_threads {
            update_contexts.push(UpdateContext::from(&update_contexts[0]));
        }

        let (update_work_send, update_work_recv) = flume::unbounded();
        let (update_submission_send, update_submission_recv) = flume::unbounded();
        let eoc_barrier = Arc::new(Barrier::new(update_threads));

        let update_workers = update_contexts
            .into_iter()
            .map(|update_context| {
                UpdateWorker::spawn(
                    update_work_recv.clone(),
                    update_submission_send.clone(),
                    eoc_barrier.clone(),
                    update_context,
                )
            })
            .collect::<Vec<_>>();

        let mut buffer_ids = Vec::with_capacity(4);

        for _ in 0..4 {
            buffer_ids.push(create_buffer(
                &window,
                resource_sharing.clone(),
                INITIAL_BUFFER_LEN,
            )?);
        }

        let mut staging_buffers = Vec::with_capacity(2);

        for _ in 0..2 {
            staging_buffers.push(create_staging_buffer(&window, INITIAL_BUFFER_LEN)?);
        }

        let (vertex_upload_task, vertex_upload_task_ids) =
            VertexUploadTask::create_task_graph(&window, resource_sharing.clone(), vertex_flt_id)?;

        let atlas_clear_buffer = window
            .basalt_ref()
            .device_resources_ref()
            .create_buffer(
                vko::BufferCreateInfo {
                    usage: vko::BufferUsage::TRANSFER_SRC | vko::BufferUsage::TRANSFER_DST,
                    ..Default::default()
                },
                vko::AllocationCreateInfo {
                    memory_type_filter: vko::MemoryTypeFilter {
                        preferred_flags: vko::MemoryPropertyFlags::DEVICE_LOCAL,
                        not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED,
                        ..vko::MemoryTypeFilter::empty()
                    },
                    allocate_preference: vko::MemoryAllocatePreference::AlwaysAllocate,
                    ..Default::default()
                },
                vko::DeviceLayout::new_unsized::<[u8]>(
                    image_format.block_size()
                        * ATLAS_LARGE_THRESHOLD as vko::DeviceSize
                        * ATLAS_LARGE_THRESHOLD as vko::DeviceSize,
                )
                .unwrap(),
            )
            .map_err(VulkanoError::CreateBuffer)?;

        unsafe {
            vko::execute(
                window.basalt_ref().transfer_queue_ref(),
                window.basalt_ref().device_resources_ref(),
                image_flt_id,
                |cmd, _| {
                    cmd.fill_buffer(&vko::FillBufferInfo {
                        dst_buffer: atlas_clear_buffer,
                        size: image_format.block_size()
                            * ATLAS_LARGE_THRESHOLD as vko::DeviceSize
                            * ATLAS_LARGE_THRESHOLD as vko::DeviceSize,
                        data: 0,
                        ..Default::default()
                    })?;

                    Ok(())
                },
                [],
                [(atlas_clear_buffer, vko::AccessTypes::COPY_TRANSFER_WRITE)],
                [],
            )
        }
        .map_err(VulkanoError::ExecuteTaskGraph)?;

        window
            .basalt_ref()
            .device_resources_ref()
            .flight(image_flt_id)
            .unwrap()
            .wait(None)
            .map_err(VulkanoError::FlightWait)?;

        render_event_send
            .send(RenderEvent::Update {
                buffer_id: buffer_ids[3],
                image_ids: Vec::new(),
                draw_count: 0,
                metrics_op: None,
                token: Arc::new((Mutex::new(None), Condvar::new())),
            })
            .unwrap();

        let mut worker = Self {
            window,
            render_flt_id,
            vertex_flt_id,
            image_flt_id,
            window_event_recv,
            render_event_send,
            image_format,
            resource_sharing,
            current_extent,

            metrics_state: None,
            metrics_level,

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
            buffer_total: [0; 2],
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

        Ok(std::thread::spawn(move || worker.run()))
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
                        images: ImageSet::new(),
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

    fn update_all_with_glyphs(&mut self) {
        for state in self.bin_state.values_mut() {
            if state.images.iter().any(|image_key| image_key.is_glyph()) {
                state.pending_update = true;
                self.pending_work = true;
            }
        }
    }

    fn set_extent(&mut self, extent: [u32; 2]) -> Result<(), WorkerError> {
        self.current_extent = extent;
        self.update_all();

        for worker in self.update_workers.iter() {
            worker.set_extent(extent)?;
        }

        Ok(())
    }

    fn set_scale(&mut self, scale: f32) -> Result<(), WorkerError> {
        self.update_all();

        for worker in self.update_workers.iter() {
            worker.set_scale(scale)?;
        }

        Ok(())
    }

    fn add_binary_font(
        &mut self,
        bytes: Arc<dyn AsRef<[u8]> + Sync + Send>,
    ) -> Result<(), WorkerError> {
        self.update_all_with_glyphs();

        for worker in self.update_workers.iter() {
            worker.add_binary_font(bytes.clone())?;
        }

        Ok(())
    }

    fn set_default_font(&mut self, default_font: DefaultFont) -> Result<(), WorkerError> {
        self.update_all_with_glyphs();

        for worker in self.update_workers.iter() {
            worker.set_default_font(default_font.clone())?;
        }

        Ok(())
    }

    fn set_metrics_level(
        &mut self,
        metrics_level: RendererMetricsLevel,
    ) -> Result<(), WorkerError> {
        self.metrics_level = metrics_level;

        for worker in self.update_workers.iter() {
            worker.set_metrics_level(metrics_level)?;
        }

        Ok(())
    }

    fn metrics_begin(&mut self) {
        if self.metrics_level >= RendererMetricsLevel::Extended {
            let inst = Instant::now();
            let mut inner = WorkerPerfMetrics::default();

            if self.metrics_level >= RendererMetricsLevel::Full {
                inner.ovd_metrics = Some(Default::default());
            }

            self.metrics_state = Some(MetricsState {
                inner,
                start: inst,
                last_segment: 0,
            });
        }
    }

    fn metrics_segment<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut WorkerPerfMetrics, f32),
    {
        if let Some(metrics_state) = self.metrics_state.as_mut() {
            let segment = metrics_state.start.elapsed().as_micros();
            let elapsed = (segment - metrics_state.last_segment) as f32 / 1000.0;
            metrics_state.last_segment = segment;
            f(&mut metrics_state.inner, elapsed);
        }
    }

    fn metrics_ovd(&mut self, ovd_metrics: Option<OVDPerfMetrics>) {
        if let Some(ovd_metrics) = ovd_metrics {
            if let Some(metrics_state) = self.metrics_state.as_mut() {
                if let Some(total_ovd_metrics) = metrics_state.inner.ovd_metrics.as_mut() {
                    *total_ovd_metrics += ovd_metrics;
                }
            }
        }
    }

    fn metrics_complete(&mut self) -> Option<WorkerPerfMetrics> {
        self.metrics_state.take().map(|mut metrics_state| {
            metrics_state.inner.total = metrics_state.start.elapsed().as_micros() as f32 / 1000.0;
            metrics_state.inner
        })
    }

    fn run(mut self) -> Result<(), WorkerError> {
        let mut idx = Indexes::default();

        let max_image_dimension2_d = self
            .window
            .basalt_ref()
            .physical_device()
            .properties()
            .max_image_dimension2_d;

        let mut previous_token: Option<Arc<(Mutex<Option<u64>>, Condvar)>> = None;
        let mut window_events = Vec::new();

        let mut image_keys_remove: ImageMap<usize> = ImageMap::new();
        let mut image_keys_add: ImageMap<usize> = ImageMap::new();
        let mut image_cache_obtain: ImageMap<usize> = ImageMap::new();
        let mut image_backings_remove = Vec::new();
        let mut image_cache_deref: Vec<ImageKey> = Vec::new();
        let mut image_keys_effected = ImageSet::new();

        loop {
            loop {
                window_events.extend(self.window_event_recv.drain());

                for window_event in window_events.drain(..) {
                    match window_event {
                        WindowEvent::Closed => return Ok(()),
                        WindowEvent::Resized {
                            width,
                            height,
                        } => {
                            if width != self.current_extent[0] || height != self.current_extent[1] {
                                if self
                                    .render_event_send
                                    .send(RenderEvent::CheckExtent)
                                    .is_err()
                                {
                                    return Err(WorkerError::Disconnected);
                                }

                                self.set_extent([width, height])?;
                            }
                        },
                        WindowEvent::ScaleChanged(scale) => {
                            self.set_scale(scale)?;
                        },
                        WindowEvent::RedrawRequested => {
                            if self.render_event_send.send(RenderEvent::Redraw).is_err() {
                                return Err(WorkerError::Disconnected);
                            }
                        },
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
                        WindowEvent::AddBinaryFont(bytes) => self.add_binary_font(bytes)?,
                        WindowEvent::SetDefaultFont(default_font) => {
                            self.set_default_font(default_font)?;
                        },
                        WindowEvent::SetMSAA(msaa) => {
                            if self
                                .render_event_send
                                .send(RenderEvent::SetMSAA(msaa))
                                .is_err()
                            {
                                return Err(WorkerError::Disconnected);
                            }
                        },
                        WindowEvent::SetVSync(vsync) => {
                            if self
                                .render_event_send
                                .send(RenderEvent::SetVSync(vsync))
                                .is_err()
                            {
                                return Err(WorkerError::Disconnected);
                            }
                        },
                        WindowEvent::SetConsvDraw(enabled) => {
                            if self
                                .render_event_send
                                .send(RenderEvent::SetConsvDraw(enabled))
                                .is_err()
                            {
                                return Err(WorkerError::Disconnected);
                            }
                        },
                        WindowEvent::SetMetrics(metrics_level) => {
                            self.set_metrics_level(metrics_level)?;

                            if self
                                .render_event_send
                                .send(RenderEvent::SetMetricsLevel(metrics_level))
                                .is_err()
                            {
                                return Err(WorkerError::Disconnected);
                            }
                        },
                        WindowEvent::OnFrame(method) => {
                            if self
                                .render_event_send
                                .send(RenderEvent::OnFrame(method))
                                .is_err()
                            {
                                return Err(WorkerError::Disconnected);
                            }
                        },
                    }
                }

                if self.pending_work
                    || self.buffer_update[0]
                    || self.buffer_update[1]
                    || self.image_update[0]
                    || self.image_update[1]
                {
                    break;
                }

                match self.window_event_recv.recv() {
                    Ok(window_event) => window_events.push(window_event),
                    Err(_) => return Err(WorkerError::Disconnected),
                }
            }

            self.metrics_begin();

            // --- Remove Bin States --- //

            let mut remove_count = 0;

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
                }

                for image_key in state.images.drain() {
                    image_keys_remove.modify(&image_key, || 0, |count| *count += 1);
                }

                remove_count += 1;
                false
            });

            self.metrics_segment(|metrics, elapsed| {
                metrics.bin_remove = elapsed;
            });

            // --- Obtain Vertex Data --- //

            let mut update_count = 0;

            for state in self.bin_state.values() {
                if state.pending_update {
                    if let Some(bin) = state.bin_wk.upgrade() {
                        self.update_work_send
                            .send(bin)
                            .map_err(|_| WorkerError::OvdWorkerPanicked)?;

                        update_count += 1;
                    }
                }
            }

            if update_count > 0 {
                for worker in self.update_workers.iter() {
                    worker.perform()?;
                }

                let mut update_received = 0;

                while update_received < update_count {
                    let UpdateSubmission {
                        id,
                        mut images,
                        mut vertexes,
                        metrics_op,
                    } = self
                        .update_submission_recv
                        .recv()
                        .map_err(|_| WorkerError::OvdWorkerPanicked)?;

                    self.metrics_ovd(metrics_op);
                    update_received += 1;
                    let state = self.bin_state.get_mut(&id).unwrap();
                    std::mem::swap(&mut images, &mut state.images);
                    std::mem::swap(&mut vertexes, &mut state.vertexes);
                    state.pending_update = false;

                    for new_image_key in state.images.iter() {
                        if !images.contains(new_image_key) {
                            image_keys_add.modify(new_image_key, || 0, |count| *count += 1);
                        }
                    }

                    for old_image_key in images.into_iter() {
                        if !state.images.contains(&old_image_key) {
                            image_keys_remove.modify(&old_image_key, || 0, |count| *count += 1);
                        }
                    }

                    if !state.vertexes.is_empty() {
                        self.buffer_update = [true; 2];
                    } else {
                        for old_vertex_state in vertexes.into_values() {
                            for (buffer_i, offset_op) in
                                old_vertex_state.offset.into_iter().enumerate()
                            {
                                if offset_op.is_some() {
                                    self.buffer_update[buffer_i] = true;
                                }
                            }
                        }
                    }
                }
            }

            self.update_workers[0].clear_cache()?;

            self.metrics_segment(|metrics, elapsed| {
                metrics.bin_count = remove_count + update_count;
                metrics.bin_obtain = elapsed;
            });

            // -- Decrease Image Use Counters -- //

            for (image_key, count) in image_keys_remove.drain() {
                for image_backing in self.image_backings.iter_mut() {
                    match image_backing {
                        ImageBacking::Atlas {
                            allocations, ..
                        } => {
                            if let Some(alloc_state) = allocations.get_mut(&image_key) {
                                alloc_state.uses -= count;
                                break;
                            }
                        },
                        ImageBacking::Dedicated {
                            key,
                            uses,
                            ..
                        } => {
                            if *key == image_key {
                                *uses -= count;
                                break;
                            }
                        },
                        ImageBacking::User {
                            key,
                            uses,
                            ..
                        } => {
                            if *key == image_key {
                                *uses -= count;
                                break;
                            }
                        },
                    }
                }
            }

            // -- Increase Image Use Counters -- //

            for (image_key, count) in image_keys_add.drain() {
                let mut obtain_image_key = true;

                for image_backing in self.image_backings.iter_mut() {
                    match image_backing {
                        ImageBacking::Atlas {
                            allocations, ..
                        } => {
                            if let Some(alloc_state) = allocations.get_mut(&image_key) {
                                alloc_state.uses += count;
                                obtain_image_key = false;
                                break;
                            }
                        },
                        ImageBacking::Dedicated {
                            key,
                            uses,
                            ..
                        } => {
                            if *key == image_key {
                                *uses += count;
                                obtain_image_key = false;
                                break;
                            }
                        },
                        ImageBacking::User {
                            key,
                            uses,
                            ..
                        } => {
                            if *key == image_key {
                                *uses += count;
                                obtain_image_key = false;
                                break;
                            }
                        },
                    }
                }

                if obtain_image_key {
                    image_cache_obtain.modify(&image_key, || 0, |c| *c += count);
                }
            }

            self.metrics_segment(|metrics, elapsed| {
                metrics.image_count = elapsed;
            });

            // -- Deref Image Cache Keys & Remove Image Backings -- //

            for (i, image_backing) in self.image_backings.iter_mut().enumerate() {
                match image_backing {
                    ImageBacking::Atlas {
                        allocator,
                        allocations,
                        pending_clears,
                        ..
                    } => {
                        allocations.retain(|image_key, alloc_state| {
                            if alloc_state.uses == 0 {
                                image_cache_deref.push(image_key.clone());
                                allocator.deallocate(alloc_state.alloc.id);

                                for clears in pending_clears.iter_mut() {
                                    clears.push([
                                        alloc_state.alloc.rectangle.min.x as u32,
                                        alloc_state.alloc.rectangle.min.y as u32,
                                        alloc_state.alloc.rectangle.width() as u32,
                                        alloc_state.alloc.rectangle.height() as u32,
                                    ]);
                                }

                                false
                            } else {
                                true
                            }
                        });
                    },
                    ImageBacking::Dedicated {
                        key,
                        uses,
                        ..
                    } => {
                        if *uses == 0 {
                            image_cache_deref.push(key.clone());
                            image_backings_remove.push(i);
                        }
                    },
                    ImageBacking::User {
                        uses, ..
                    } => {
                        if *uses == 0 {
                            image_backings_remove.push(i);
                        }
                    },
                }
            }

            if !image_backings_remove.is_empty() {
                for image_backing in self
                    .image_backings
                    .iter()
                    .skip(image_backings_remove[0] + 1)
                {
                    match image_backing {
                        ImageBacking::Atlas {
                            allocations, ..
                        } => {
                            image_keys_effected.extend(allocations.keys().cloned());
                        },
                        ImageBacking::Dedicated {
                            key, ..
                        } => {
                            image_keys_effected.insert(key.clone());
                        },
                        ImageBacking::User {
                            key, ..
                        } => {
                            image_keys_effected.insert(key.clone());
                        },
                    }
                }

                for i in image_backings_remove.drain(..).rev() {
                    self.image_backings.remove(i);
                }

                for bin_state in self.bin_state.values_mut() {
                    if bin_state
                        .images
                        .iter()
                        .any(|image_key| image_keys_effected.contains(image_key))
                    {
                        for vertex_state in bin_state.vertexes.values_mut() {
                            vertex_state.offset = [None; 2];
                            vertex_state.staging = [None; 2];
                            self.buffer_update = [true; 2];
                        }
                    }
                }

                image_keys_effected.clear();
            }

            self.metrics_segment(|metrics, elapsed| {
                metrics.image_remove = elapsed;
            });

            // -- Obtain Image Sources -- //

            if !image_cache_obtain.is_empty() || !image_cache_deref.is_empty() {
                let mut obtained_images = self.window.basalt_ref().image_cache_ref().obtain_data(
                    image_cache_deref.split_off(0),
                    image_cache_obtain
                        .keys()
                        .cloned()
                        .collect::<Vec<ImageKey>>(),
                    self.image_format,
                );

                'obtain: for (image_key, count) in image_cache_obtain.drain() {
                    if let Some(image_id) = image_key.as_vulkano_id() {
                        self.image_backings.push(ImageBacking::User {
                            key: image_key,
                            uses: count,
                            image_id,
                        });
                    } else {
                        let mut obtained_image = obtained_images.remove(&image_key).unwrap();

                        if obtained_image.width > ATLAS_LARGE_THRESHOLD - 2
                            || obtained_image.height > ATLAS_LARGE_THRESHOLD - 2
                        {
                            let image_id = create_image(
                                &self.window,
                                self.resource_sharing.clone(),
                                self.image_format,
                                obtained_image.width,
                                obtained_image.height,
                            )?;

                            self.image_backings.push(ImageBacking::Dedicated {
                                key: image_key,
                                uses: count,
                                image_id,
                                write_info: Some((
                                    obtained_image.width,
                                    obtained_image.height,
                                    obtained_image.data,
                                )),
                            });

                            self.image_update[idx.curr_image()] = true;
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

                                            for clears in pending_clears.iter_mut() {
                                                clears.append(&mut atlas_clears(
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
                                        staging_write[idx.curr_image()].len() as vko::DeviceSize;
                                    staging_write[idx.curr_image()]
                                        .append(&mut obtained_image.data);

                                    for uploads in pending_uploads.iter_mut() {
                                        uploads.push(StagedAtlasUpload {
                                            staging_write_i: idx.curr_image(),
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
                                    }

                                    allocations.insert(
                                        image_key,
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
                            AtlasSize::new(ATLAS_DEFAULT_SIZE as i32, ATLAS_DEFAULT_SIZE as i32),
                            &AtlasAllocatorOptions {
                                alignment: AtlasSize::new(
                                    ATLAS_SMALL_THRESHOLD as i32,
                                    ATLAS_SMALL_THRESHOLD as i32,
                                ),
                                small_size_threshold: ATLAS_SMALL_THRESHOLD as i32,
                                large_size_threshold: ATLAS_LARGE_THRESHOLD as i32,
                            },
                        );

                        let mut allocations = ImageMap::new();
                        let mut staging_write = [Vec::new(), Vec::new()];
                        let mut pending_uploads = [Vec::new(), Vec::new()];
                        let alloc = allocator.allocate(alloc_size).unwrap();
                        staging_write[idx.curr_image()].append(&mut obtained_image.data);

                        for uploads in pending_uploads.iter_mut() {
                            uploads.push(StagedAtlasUpload {
                                staging_write_i: idx.curr_image(),
                                buffer_offset: 0,
                                image_offset: [
                                    alloc.rectangle.min.x as u32 + 1,
                                    alloc.rectangle.min.y as u32 + 1,
                                    0,
                                ],
                                image_extent: [obtained_image.width, obtained_image.height, 1],
                            });
                        }

                        allocations.insert(
                            image_key,
                            AtlasAllocationState {
                                alloc,
                                uses: count,
                            },
                        );

                        self.image_backings.push(ImageBacking::Atlas {
                            allocator: Box::new(allocator),
                            allocations,
                            images: [
                                create_image(
                                    &self.window,
                                    self.resource_sharing.clone(),
                                    self.image_format,
                                    ATLAS_DEFAULT_SIZE,
                                    ATLAS_DEFAULT_SIZE,
                                )?,
                                create_image(
                                    &self.window,
                                    self.resource_sharing.clone(),
                                    self.image_format,
                                    ATLAS_DEFAULT_SIZE,
                                    ATLAS_DEFAULT_SIZE,
                                )?,
                            ],
                            staging_buffers: [
                                create_image_staging_buffer(
                                    &self.window,
                                    self.image_format,
                                    4096,
                                    4096,
                                    true,
                                )?,
                                create_image_staging_buffer(
                                    &self.window,
                                    self.image_format,
                                    4096,
                                    4096,
                                    true,
                                )?,
                            ],
                            pending_clears: [
                                atlas_clears(0..ATLAS_DEFAULT_SIZE, 0..ATLAS_DEFAULT_SIZE),
                                atlas_clears(0..ATLAS_DEFAULT_SIZE, 0..ATLAS_DEFAULT_SIZE),
                            ],
                            pending_uploads,
                            staging_write,
                            resize_images: [false; 2],
                        });

                        self.image_update = [true; 2];
                    }
                }
            }

            self.metrics_segment(|metrics, elapsed| {
                metrics.image_obtain = elapsed;
            });

            // --- Image Updates --- //

            let mut remove_image_ids: Vec<vko::Id<vko::Image>> = Vec::new();
            let mut remove_buffer_ids: Vec<vko::Id<vko::Buffer>> = Vec::new();

            if self.image_update[idx.curr_image()] {
                let mut host_buffer_accesses = HashMap::new();
                let mut buffer_accesses = HashMap::new();
                let mut image_accesses = HashMap::new();

                let mut op_image_copy: Vec<[vko::Id<vko::Image>; 2]> = Vec::new();
                let mut op_image_barrier1: Vec<vko::Id<vko::Image>> = Vec::new();
                let mut op_image_clear: Vec<(vko::Id<vko::Image>, Vec<[u32; 4]>)> = Vec::new();
                let mut op_image_barrier2: Vec<vko::Id<vko::Image>> = Vec::new();
                let mut op_staging_write: Vec<(vko::Id<vko::Buffer>, Vec<u8>)> = Vec::new();

                let mut op_image_write: Vec<(
                    vko::Id<vko::Buffer>,
                    vko::Id<vko::Image>,
                    Vec<(vko::DeviceSize, [u32; 3], [u32; 3])>,
                )> = Vec::new();

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
                            let mut previous_op = false;

                            if resize_images[idx.curr_image()] {
                                let old_image = images[idx.curr_image()];
                                let old_staging_buffer = staging_buffers[idx.curr_image()];

                                images[idx.curr_image()] = create_image(
                                    &self.window,
                                    self.resource_sharing.clone(),
                                    self.image_format,
                                    allocator.size().width as u32,
                                    allocator.size().height as u32,
                                )?;

                                staging_buffers[idx.curr_image()] = create_image_staging_buffer(
                                    &self.window,
                                    self.image_format,
                                    allocator.size().width as u32,
                                    allocator.size().height as u32,
                                    true,
                                )?;

                                op_image_copy.push([old_image, images[idx.curr_image()]]);

                                image_accesses
                                    .insert(old_image, vko::AccessTypes::COPY_TRANSFER_READ);
                                image_accesses.insert(
                                    images[idx.curr_image()],
                                    vko::AccessTypes::COPY_TRANSFER_WRITE,
                                );

                                remove_image_ids.push(old_image);
                                remove_buffer_ids.push(old_staging_buffer);

                                resize_images[idx.curr_image()] = false;
                                previous_op = true;
                            }

                            if !pending_clears[idx.curr_image()].is_empty() {
                                if previous_op {
                                    op_image_barrier1.push(images[idx.curr_image()]);
                                }

                                op_image_clear.push((
                                    images[idx.curr_image()],
                                    pending_clears[idx.curr_image()].split_off(0),
                                ));

                                image_accesses.insert(
                                    images[idx.curr_image()],
                                    vko::AccessTypes::COPY_TRANSFER_WRITE,
                                );

                                buffer_accesses.insert(
                                    self.atlas_clear_buffer,
                                    vko::AccessTypes::COPY_TRANSFER_READ,
                                );

                                previous_op = true;
                            }

                            if !staging_write[idx.curr_image()].is_empty() {
                                op_staging_write.push((
                                    staging_buffers[idx.curr_image()],
                                    staging_write[idx.curr_image()].split_off(0),
                                ));

                                host_buffer_accesses.insert(
                                    staging_buffers[idx.curr_image()],
                                    vko::HostAccessType::Write,
                                );
                            }

                            if !pending_uploads[idx.curr_image()].is_empty() {
                                if previous_op {
                                    op_image_barrier2.push(images[idx.curr_image()]);
                                }

                                let mut write_info = [Vec::new(), Vec::new()];

                                for StagedAtlasUpload {
                                    staging_write_i,
                                    buffer_offset,
                                    image_offset,
                                    image_extent,
                                } in pending_uploads[idx.curr_image()].drain(..)
                                {
                                    write_info[staging_write_i].push((
                                        buffer_offset,
                                        image_offset,
                                        image_extent,
                                    ));
                                }

                                // TODO: Is a barrier needed between these two writes? In theory
                                // it shouldn't be needed, but the validation complains.

                                for (i, write_info) in write_info.into_iter().enumerate() {
                                    if write_info.is_empty() {
                                        continue;
                                    }

                                    buffer_accesses.insert(
                                        staging_buffers[i],
                                        vko::AccessTypes::COPY_TRANSFER_READ,
                                    );

                                    image_accesses.insert(
                                        images[idx.curr_image()],
                                        vko::AccessTypes::COPY_TRANSFER_WRITE,
                                    );

                                    op_image_write.push((
                                        staging_buffers[i],
                                        images[idx.curr_image()],
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
                                )?;

                                op_staging_write.push((buffer_id, staging_write));

                                op_image_write.push((
                                    buffer_id,
                                    *image_id,
                                    vec![(0, [0; 3], [w, h, 1])],
                                ));

                                host_buffer_accesses.insert(buffer_id, vko::HostAccessType::Write);
                                buffer_accesses
                                    .insert(buffer_id, vko::AccessTypes::COPY_TRANSFER_READ);
                                image_accesses
                                    .insert(*image_id, vko::AccessTypes::COPY_TRANSFER_WRITE);

                                remove_buffer_ids.push(buffer_id);
                            }
                        },
                        ImageBacking::User {
                            ..
                        } => (),
                    }
                }

                let image_subresource_range =
                    vko::ImageSubresourceRange::from_parameters(self.image_format, 1, 1);
                let image_subresource_layers =
                    vko::ImageSubresourceLayers::from_parameters(self.image_format, 1);

                self.metrics_segment(|metrics, elapsed| {
                    metrics.image_update_prep = elapsed;
                });

                if let Some(token) = previous_token.take() {
                    let mut guard = token.0.lock();

                    while guard.is_none() {
                        token.1.wait(&mut guard);
                    }

                    self.window
                        .basalt_ref()
                        .device_resources_ref()
                        .flight(self.render_flt_id)
                        .unwrap()
                        .wait_for_frame(guard.take().unwrap(), None)
                        .map_err(VulkanoError::FlightWait)?;
                }

                self.metrics_segment(|metrics, elapsed| {
                    metrics.swap_wait = elapsed;
                });

                unsafe {
                    vko::execute(
                        self.window.basalt_ref().transfer_queue_ref(),
                        self.window.basalt_ref().device_resources_ref(),
                        self.image_flt_id,
                        |cmd, task| {
                            for (buffer_id, write) in op_staging_write {
                                task.write_buffer::<[u8]>(
                                    buffer_id,
                                    0..(write.len() as vko::DeviceSize),
                                )?
                                .copy_from_slice(write.as_slice());
                            }

                            for [src_image, dst_image] in op_image_copy {
                                cmd.copy_image(&vko::CopyImageInfo {
                                    src_image,
                                    dst_image,
                                    ..Default::default()
                                })?;
                            }

                            if !op_image_barrier1.is_empty() {
                                let barriers = op_image_barrier1
                                    .into_iter()
                                    .map(|image| {
                                        vko::ImageMemoryBarrier {
                                            src_stages: vko::PipelineStages::COPY,
                                            src_access: vko::AccessFlags::MEMORY_WRITE,
                                            dst_stages: vko::PipelineStages::COPY,
                                            dst_access: vko::AccessFlags::MEMORY_WRITE,
                                            old_layout: vko::ImageLayout::TransferDstOptimal,
                                            new_layout: vko::ImageLayout::TransferDstOptimal,
                                            image,
                                            subresource_range: image_subresource_range.clone(),
                                            ..Default::default()
                                        }
                                    })
                                    .collect::<Vec<_>>();

                                cmd.pipeline_barrier(&vko::DependencyInfo {
                                    image_memory_barriers: &barriers,
                                    ..Default::default()
                                })?;
                            }

                            for (dst_image, regions) in op_image_clear {
                                let regions = regions
                                    .into_iter()
                                    .map(|[x, y, w, h]| {
                                        vko::BufferImageCopy {
                                            image_subresource: image_subresource_layers.clone(),
                                            buffer_row_length: w,
                                            buffer_image_height: h,
                                            image_offset: [x, y, 0],
                                            image_extent: [w, h, 1],
                                            ..Default::default()
                                        }
                                    })
                                    .collect::<Vec<_>>();

                                cmd.copy_buffer_to_image(&vko::CopyBufferToImageInfo {
                                    src_buffer: self.atlas_clear_buffer,
                                    dst_image,
                                    regions: regions.as_slice(),
                                    ..Default::default()
                                })?;
                            }

                            if !op_image_barrier2.is_empty() {
                                let barriers = op_image_barrier2
                                    .into_iter()
                                    .map(|image| {
                                        vko::ImageMemoryBarrier {
                                            src_stages: vko::PipelineStages::COPY,
                                            src_access: vko::AccessFlags::MEMORY_WRITE,
                                            dst_stages: vko::PipelineStages::COPY,
                                            dst_access: vko::AccessFlags::MEMORY_WRITE,
                                            old_layout: vko::ImageLayout::TransferDstOptimal,
                                            new_layout: vko::ImageLayout::TransferDstOptimal,
                                            image,
                                            subresource_range: image_subresource_range.clone(),
                                            ..Default::default()
                                        }
                                    })
                                    .collect::<Vec<_>>();

                                cmd.pipeline_barrier(&vko::DependencyInfo {
                                    image_memory_barriers: &barriers,
                                    ..Default::default()
                                })?;
                            }

                            for (src_buffer, dst_image, regions) in op_image_write {
                                let regions = regions
                                    .into_iter()
                                    .map(|(buffer_offset, image_offset, image_extent)| {
                                        vko::BufferImageCopy {
                                            buffer_offset,
                                            buffer_row_length: image_extent[0],
                                            buffer_image_height: image_extent[1],
                                            image_subresource: image_subresource_layers.clone(),
                                            image_offset,
                                            image_extent,
                                            ..Default::default()
                                        }
                                    })
                                    .collect::<Vec<_>>();

                                cmd.copy_buffer_to_image(&vko::CopyBufferToImageInfo {
                                    src_buffer,
                                    dst_image,
                                    regions: regions.as_slice(),
                                    ..Default::default()
                                })?;
                            }

                            Ok(())
                        },
                        host_buffer_accesses,
                        buffer_accesses,
                        image_accesses
                            .into_iter()
                            .map(|(image, access)| (image, access, vko::ImageLayoutType::Optimal)),
                    )
                }
                .map_err(VulkanoError::ExecuteTaskGraph)?;
            }

            // --- Vertex Updates --- //

            let mut vertex_sub_swap = false;

            if self.buffer_update[idx.curr_vertex()] {
                let src_buf_i = idx.curr_vertex_prev_sub();
                let src_buf_id = self.buffers[src_buf_i[0]][src_buf_i[1]];
                let dst_buf_i = idx.curr_vertex_sub();
                let mut dst_buf_id = self.buffers[dst_buf_i[0]][dst_buf_i[1]];
                let mut stage_buf_id = self.staging_buffers[src_buf_i[0]];
                let prev_stage_buf_i = idx.prev_vertex();
                let prev_stage_buf_id = self.staging_buffers[prev_stage_buf_i];

                // -- Count Vertexes -- //

                let mut count_by_z: BTreeMap<OrderedFloat<f32>, vko::DeviceSize> = BTreeMap::new();

                for state in self.bin_state.values() {
                    for (z, vertex_state) in state.vertexes.iter() {
                        *count_by_z.entry(*z).or_default() += vertex_state.total as vko::DeviceSize;
                    }
                }

                let total_count = count_by_z.values().sum::<vko::DeviceSize>();

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
                    let new_buf_id = create_buffer(
                        &self.window,
                        self.resource_sharing.clone(),
                        new_buffer_size / VERTEX_SIZE,
                    )?;

                    unsafe {
                        self.window
                            .basalt_ref()
                            .device_resources_ref()
                            .remove_buffer(dst_buf_id)
                            .unwrap();
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
                            create_staging_buffer(&self.window, new_buffer_size / VERTEX_SIZE)?;

                        unsafe {
                            self.window
                                .basalt_ref()
                                .device_resources_ref()
                                .remove_buffer(stage_buf_id)
                                .unwrap();
                        }

                        stage_buf_id = new_staging_buf_id;
                        self.staging_buffers[dst_buf_i[0]] = new_staging_buf_id;
                    }
                }

                self.metrics_segment(|metrics, elapsed| {
                    metrics.vertex_count = elapsed;
                });

                // -- Prepare Vertex Operations -- //

                let mut z_dst_offset: BTreeMap<OrderedFloat<f32>, vko::DeviceSize> =
                    BTreeMap::new();
                let mut cumulative_offset = 0;

                for (z, count) in count_by_z {
                    z_dst_offset.insert(z, cumulative_offset);
                    cumulative_offset += count * VERTEX_SIZE;
                }

                let mut copy_from_prev: Vec<vko::BufferCopy> = Vec::new();
                let mut copy_from_prev_stage: Vec<vko::BufferCopy> = Vec::new();
                let mut copy_from_curr_stage: Vec<vko::BufferCopy> = Vec::new();
                let mut staging_write = Vec::new();

                for bin_state in self.bin_state.values_mut() {
                    for (z, vertex_state) in bin_state.vertexes.iter_mut() {
                        let size = vertex_state.total as vko::DeviceSize * VERTEX_SIZE;
                        let prev_offset = vertex_state.offset[src_buf_i[0]].take();
                        let prev_stage_offset = vertex_state.staging[prev_stage_buf_i].take();

                        let dst_offset = {
                            let zdo = z_dst_offset.get_mut(z).unwrap();
                            let dst_offset = *zdo;
                            *zdo += size;
                            dst_offset
                        };

                        vertex_state.offset[src_buf_i[0]] = Some(dst_offset);

                        if let Some(src_offset) = prev_offset {
                            copy_from_prev.push(vko::BufferCopy {
                                src_offset,
                                dst_offset,
                                size,
                                ..Default::default()
                            });

                            continue;
                        }

                        if let Some(src_offset) = prev_stage_offset {
                            copy_from_prev_stage.push(vko::BufferCopy {
                                src_offset,
                                dst_offset,
                                size,
                                ..Default::default()
                            });

                            continue;
                        }

                        let src_offset = staging_write.len() as vko::DeviceSize * VERTEX_SIZE;
                        vertex_state.staging[src_buf_i[0]] = Some(src_offset);

                        for (image_key, vertexes) in vertex_state.data.iter() {
                            if !image_key.is_none() {
                                let mut image_backing_i = None;
                                let mut offset_coords = [0.0; 2];

                                for (i, image_backing) in self.image_backings.iter().enumerate() {
                                    match image_backing {
                                        ImageBacking::Atlas {
                                            allocations, ..
                                        } => {
                                            if let Some(alloc_state) = allocations.get(image_key) {
                                                image_backing_i = Some(i as u32);
                                                offset_coords = [
                                                    alloc_state.alloc.rectangle.min.x as f32 + 1.0,
                                                    alloc_state.alloc.rectangle.min.y as f32 + 1.0,
                                                ];

                                                break;
                                            }
                                        },
                                        ImageBacking::Dedicated {
                                            key, ..
                                        }
                                        | ImageBacking::User {
                                            key, ..
                                        } => {
                                            if *key == *image_key {
                                                image_backing_i = Some(i as u32);
                                                break;
                                            }
                                        },
                                    }
                                }

                                let image_backing_i = image_backing_i.unwrap();

                                for mut vertex in vertexes.iter().cloned() {
                                    vertex.tex_i = image_backing_i;
                                    vertex.coords[0] += offset_coords[0];
                                    vertex.coords[1] += offset_coords[1];
                                    staging_write.push(vertex);
                                }
                            } else {
                                staging_write.extend(vertexes.iter().cloned());
                            }
                        }

                        copy_from_curr_stage.push(vko::BufferCopy {
                            src_offset,
                            dst_offset,
                            size,
                            ..Default::default()
                        });
                    }
                }

                // -- Execute Vertex Operations -- //

                consolidate_buffer_copies(&mut copy_from_prev);
                consolidate_buffer_copies(&mut copy_from_prev_stage);
                consolidate_buffer_copies(&mut copy_from_curr_stage);

                for copy in copy_from_prev.iter() {
                    if copy.src_offset != copy.dst_offset {
                        vertex_sub_swap = true;
                        break;
                    }
                }

                if !vertex_sub_swap {
                    copy_from_prev.clear();
                }

                let world = VertexUploadTaskWorld {
                    copy_from_prev,
                    copy_from_prev_stage,
                    copy_from_curr_stage,
                    staging_write,
                };

                self.metrics_segment(|metrics, elapsed| {
                    metrics.vertex_update_prep = elapsed;
                });

                if let Some(token) = previous_token.take() {
                    let mut guard = token.0.lock();

                    while guard.is_none() {
                        token.1.wait(&mut guard);
                    }

                    self.window
                        .basalt_ref()
                        .device_resources_ref()
                        .flight(self.render_flt_id)
                        .unwrap()
                        .wait_for_frame(guard.take().unwrap(), None)
                        .map_err(VulkanoError::FlightWait)?;
                }

                self.metrics_segment(|metrics, elapsed| {
                    metrics.swap_wait = elapsed;
                });

                let resource_map = if vertex_sub_swap {
                    vko::resource_map!(
                        &self.vertex_upload_task,
                        self.vertex_upload_task_ids.prev_stage_buffer => prev_stage_buf_id,
                        self.vertex_upload_task_ids.curr_stage_buffer => stage_buf_id,
                        self.vertex_upload_task_ids.prev_buffer => src_buf_id,
                        self.vertex_upload_task_ids.curr_buffer => dst_buf_id,
                    )
                    .unwrap()
                } else {
                    vko::resource_map!(
                        &self.vertex_upload_task,
                        self.vertex_upload_task_ids.prev_stage_buffer => prev_stage_buf_id,
                        self.vertex_upload_task_ids.curr_stage_buffer => stage_buf_id,
                        // If not switching sub these values are reversed and prev_buffer is
                        // never read. It is set to fill the slot, but otherwise unused.
                        self.vertex_upload_task_ids.prev_buffer => dst_buf_id,
                        self.vertex_upload_task_ids.curr_buffer => src_buf_id,
                    )
                    .unwrap()
                };

                unsafe { self.vertex_upload_task.execute(resource_map, &world, || {}) }
                    .map_err(VulkanoError::ExecuteTaskGraph)?;
                self.buffer_total[idx.curr_vertex()] = total_count as u32;
            }

            if self.buffer_update[idx.curr_vertex()] || self.image_update[idx.curr_image()] {
                if self.image_update[idx.curr_image()] {
                    self.window
                        .basalt_ref()
                        .device_resources_ref()
                        .flight(self.image_flt_id)
                        .unwrap()
                        .wait(None)
                        .map_err(VulkanoError::FlightWait)?;
                }

                if self.buffer_update[idx.curr_vertex()] {
                    self.window
                        .basalt_ref()
                        .device_resources_ref()
                        .flight(self.vertex_flt_id)
                        .unwrap()
                        .wait(None)
                        .map_err(VulkanoError::FlightWait)?;
                }

                self.metrics_segment(|metrics, elapsed| {
                    metrics.execution = elapsed;
                });

                let mut send_update = false;

                let buf_i = if self.buffer_update[idx.curr_vertex()] {
                    self.buffer_update[idx.curr_vertex()] = false;

                    if !self.buffer_update[idx.next_vertex()] {
                        if vertex_sub_swap {
                            idx.adv_vertex_sub();
                        }

                        idx.prev_vertex_sub()
                    } else {
                        send_update = true;

                        if vertex_sub_swap {
                            let buf_i = idx.curr_vertex_sub();
                            idx.adv_vertex();
                            buf_i
                        } else {
                            let buf_i = idx.curr_vertex_prev_sub();
                            idx.adv_vertex_no_sub();
                            buf_i
                        }
                    }
                } else {
                    idx.prev_vertex_sub()
                };

                let img_i = if self.image_update[idx.curr_image()] {
                    self.image_update[idx.curr_image()] = false;
                    send_update = true;
                    let img_i = idx.curr_image();
                    idx.adv_image();
                    img_i
                } else {
                    idx.prev_image()
                };

                let metrics_op = self.metrics_complete();

                if send_update {
                    let buffer_id = self.buffers[buf_i[0]][buf_i[1]];
                    let draw_count = self.buffer_total[buf_i[0]];

                    let image_ids = self
                        .image_backings
                        .iter()
                        .map(|image_backing| {
                            match image_backing {
                                ImageBacking::Atlas {
                                    images, ..
                                } => images[img_i],
                                ImageBacking::Dedicated {
                                    image_id, ..
                                } => *image_id,
                                ImageBacking::User {
                                    image_id, ..
                                } => *image_id,
                            }
                        })
                        .collect::<Vec<_>>();

                    let token = Arc::new((Mutex::new(None), Condvar::new()));

                    if self
                        .render_event_send
                        .send(RenderEvent::Update {
                            buffer_id,
                            image_ids,
                            draw_count,
                            metrics_op,
                            token: token.clone(),
                        })
                        .is_err()
                    {
                        return Err(WorkerError::Disconnected);
                    }

                    previous_token = Some(token);
                } else if self
                    .render_event_send
                    .send(RenderEvent::WorkerCycle(metrics_op))
                    .is_err()
                {
                    return Err(WorkerError::Disconnected);
                }
            }

            for image_id in remove_image_ids {
                unsafe {
                    self.window
                        .basalt_ref()
                        .device_resources_ref()
                        .remove_image(image_id)
                        .unwrap();
                }
            }

            for buffer_id in remove_buffer_ids {
                unsafe {
                    self.window
                        .basalt_ref()
                        .device_resources_ref()
                        .remove_buffer(buffer_id)
                        .unwrap();
                }
            }

            self.pending_work = false;
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Wait until Renderer is dropped and subsequently Context which may be using resources.

        if self.render_event_send.send(RenderEvent::Close).is_ok() {
            while !self.render_event_send.is_disconnected() {}
        }

        self.window.release_event_queue();
        let mut remove_buf_ids = Vec::new();
        let mut remove_img_ids = Vec::new();

        for i in 0..2 {
            for j in 0..2 {
                remove_buf_ids.push(self.buffers[i][j]);
            }

            remove_buf_ids.push(self.staging_buffers[i]);
        }

        remove_buf_ids.push(self.atlas_clear_buffer);

        for image_backing in self.image_backings.iter() {
            match image_backing {
                ImageBacking::Atlas {
                    images,
                    staging_buffers,
                    ..
                } => {
                    for i in 0..2 {
                        remove_img_ids.push(images[i]);
                        remove_buf_ids.push(staging_buffers[i]);
                    }
                },
                ImageBacking::Dedicated {
                    image_id, ..
                } => {
                    remove_img_ids.push(*image_id);
                },
                ImageBacking::User {
                    ..
                } => (),
            }
        }

        for buffer_id in remove_buf_ids {
            unsafe {
                let _ = self
                    .window
                    .basalt_ref()
                    .device_resources_ref()
                    .remove_buffer(buffer_id);
            }
        }

        for image_id in remove_img_ids {
            unsafe {
                let _ = self
                    .window
                    .basalt_ref()
                    .device_resources_ref()
                    .remove_image(image_id);
            }
        }

        // TODO: remove vertex_flt_id & image_flt_id
    }
}

fn create_buffer(
    window: &Arc<Window>,
    sharing: vko::Sharing<SmallVec<[u32; 4]>>,
    len: vko::DeviceSize,
) -> Result<vko::Id<vko::Buffer>, VulkanoError> {
    window
        .basalt_ref()
        .device_resources_ref()
        .create_buffer(
            vko::BufferCreateInfo {
                usage: vko::BufferUsage::TRANSFER_SRC
                    | vko::BufferUsage::TRANSFER_DST
                    | vko::BufferUsage::VERTEX_BUFFER,
                sharing,
                ..Default::default()
            },
            vko::AllocationCreateInfo {
                memory_type_filter: vko::MemoryTypeFilter {
                    preferred_flags: vko::MemoryPropertyFlags::DEVICE_LOCAL,
                    not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED,
                    ..vko::MemoryTypeFilter::empty()
                },
                allocate_preference: vko::MemoryAllocatePreference::AlwaysAllocate,
                ..Default::default()
            },
            vko::DeviceLayout::new_unsized::<[ItfVertInfo]>(len).unwrap(),
        )
        .map_err(VulkanoError::CreateBuffer)
}

fn create_staging_buffer(
    window: &Arc<Window>,
    len: vko::DeviceSize,
) -> Result<vko::Id<vko::Buffer>, VulkanoError> {
    window
        .basalt_ref()
        .device_resources_ref()
        .create_buffer(
            vko::BufferCreateInfo {
                usage: vko::BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            vko::AllocationCreateInfo {
                memory_type_filter: vko::MemoryTypeFilter {
                    required_flags: vko::MemoryPropertyFlags::HOST_VISIBLE,
                    not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED
                        | vko::MemoryPropertyFlags::DEVICE_COHERENT,
                    ..vko::MemoryTypeFilter::empty()
                },
                allocate_preference: vko::MemoryAllocatePreference::AlwaysAllocate,
                ..Default::default()
            },
            vko::DeviceLayout::new_unsized::<[ItfVertInfo]>(len).unwrap(),
        )
        .map_err(VulkanoError::CreateBuffer)
}

fn create_image(
    window: &Arc<Window>,
    sharing: vko::Sharing<SmallVec<[u32; 4]>>,
    format: vko::Format,
    width: u32,
    height: u32,
) -> Result<vko::Id<vko::Image>, VulkanoError> {
    window
        .basalt_ref()
        .device_resources_ref()
        .create_image(
            vko::ImageCreateInfo {
                image_type: vko::ImageType::Dim2d,
                format,
                extent: [width, height, 1],
                usage: vko::ImageUsage::TRANSFER_DST
                    | vko::ImageUsage::TRANSFER_SRC
                    | vko::ImageUsage::SAMPLED,
                sharing,
                ..Default::default()
            },
            vko::AllocationCreateInfo {
                memory_type_filter: vko::MemoryTypeFilter {
                    preferred_flags: vko::MemoryPropertyFlags::DEVICE_LOCAL,
                    not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED,
                    ..vko::MemoryTypeFilter::empty()
                },
                allocate_preference: vko::MemoryAllocatePreference::AlwaysAllocate,
                ..Default::default()
            },
        )
        .map_err(VulkanoError::CreateImage)
}

fn create_image_staging_buffer(
    window: &Arc<Window>,
    format: vko::Format,
    width: u32,
    height: u32,
    long_lived: bool,
) -> Result<vko::Id<vko::Buffer>, VulkanoError> {
    window
        .basalt_ref()
        .device_resources_ref()
        .create_buffer(
            vko::BufferCreateInfo {
                usage: vko::BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            vko::AllocationCreateInfo {
                memory_type_filter: vko::MemoryTypeFilter {
                    required_flags: vko::MemoryPropertyFlags::HOST_VISIBLE,
                    not_preferred_flags: vko::MemoryPropertyFlags::HOST_CACHED
                        | vko::MemoryPropertyFlags::DEVICE_COHERENT,
                    ..vko::MemoryTypeFilter::empty()
                },
                allocate_preference: if long_lived {
                    vko::MemoryAllocatePreference::AlwaysAllocate
                } else {
                    vko::MemoryAllocatePreference::Unknown
                },
                ..Default::default()
            },
            vko::DeviceLayout::new_unsized::<[u8]>(
                format.block_size() * width as vko::DeviceSize * height as vko::DeviceSize,
            )
            .unwrap(),
        )
        .map_err(VulkanoError::CreateBuffer)
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

fn consolidate_buffer_copies(copies: &mut Vec<vko::BufferCopy>) {
    copies.sort_by_key(|copy| copy.src_offset);

    for copy in copies.split_off(0) {
        match copies.last_mut() {
            Some(last_copy) => {
                if last_copy.src_offset + last_copy.size == copy.src_offset
                    && last_copy.dst_offset + last_copy.size == copy.dst_offset
                {
                    last_copy.size += copy.size;
                } else {
                    copies.push(copy);
                }
            },
            None => {
                copies.push(copy);
            },
        }
    }
}

#[derive(Clone)]
struct VertexUploadTask {
    prev_stage_buffer: vko::Id<vko::Buffer>,
    curr_stage_buffer: vko::Id<vko::Buffer>,
    prev_buffer: vko::Id<vko::Buffer>,
    curr_buffer: vko::Id<vko::Buffer>,
}

pub(crate) struct VertexUploadTaskWorld {
    copy_from_prev: Vec<vko::BufferCopy<'static>>,
    copy_from_prev_stage: Vec<vko::BufferCopy<'static>>,
    copy_from_curr_stage: Vec<vko::BufferCopy<'static>>,
    staging_write: Vec<ItfVertInfo>,
}

impl VertexUploadTask {
    pub fn create_task_graph(
        window: &Arc<Window>,
        sharing: vko::Sharing<SmallVec<[u32; 4]>>,
        vertex_flt_id: vko::Id<vko::Flight>,
    ) -> Result<(vko::ExecutableTaskGraph<VertexUploadTaskWorld>, Self), VulkanoError> {
        let mut task_graph = vko::TaskGraph::new(window.basalt_ref().device_resources_ref(), 1, 4);

        let prev_stage_buffer = task_graph.add_buffer(&vko::BufferCreateInfo {
            usage: vko::BufferUsage::TRANSFER_SRC,
            ..Default::default()
        });

        let curr_stage_buffer = task_graph.add_buffer(&vko::BufferCreateInfo {
            usage: vko::BufferUsage::TRANSFER_SRC,
            ..Default::default()
        });

        let prev_buffer = task_graph.add_buffer(&vko::BufferCreateInfo {
            usage: vko::BufferUsage::TRANSFER_SRC
                | vko::BufferUsage::TRANSFER_DST
                | vko::BufferUsage::VERTEX_BUFFER,
            sharing: sharing.clone(),
            ..Default::default()
        });

        let curr_buffer = task_graph.add_buffer(&vko::BufferCreateInfo {
            usage: vko::BufferUsage::TRANSFER_SRC
                | vko::BufferUsage::TRANSFER_DST
                | vko::BufferUsage::VERTEX_BUFFER,
            sharing,
            ..Default::default()
        });

        let this = Self {
            prev_stage_buffer,
            curr_stage_buffer,
            prev_buffer,
            curr_buffer,
        };

        task_graph.add_host_buffer_access(curr_stage_buffer, vko::HostAccessType::Write);

        task_graph
            .create_task_node(
                format!("VertexUpload[{:?}]", window.id()),
                vko::QueueFamilyType::Transfer,
                this.clone(),
            )
            .buffer_access(prev_stage_buffer, vko::AccessTypes::COPY_TRANSFER_READ)
            .buffer_access(curr_stage_buffer, vko::AccessTypes::COPY_TRANSFER_READ)
            .buffer_access(prev_buffer, vko::AccessTypes::COPY_TRANSFER_READ)
            .buffer_access(curr_buffer, vko::AccessTypes::COPY_TRANSFER_WRITE);

        Ok((
            unsafe {
                task_graph.compile(&vko::CompileInfo {
                    queues: &[window.basalt_ref().transfer_queue_ref()],
                    flight_id: vertex_flt_id,
                    ..Default::default()
                })
            }
            .map_err(VulkanoError::from)?,
            this,
        ))
    }
}

impl vko::Task for VertexUploadTask {
    type World = VertexUploadTaskWorld;

    unsafe fn execute(
        &self,
        cmd: &mut vko::RecordingCommandBuffer<'_>,
        task: &mut vko::TaskContext<'_>,
        world: &Self::World,
    ) -> vko::TaskResult {
        if !world.staging_write.is_empty() {
            task.write_buffer::<[ItfVertInfo]>(
                self.curr_stage_buffer,
                ..(world.staging_write.len() as vko::DeviceSize * VERTEX_SIZE),
            )?
            .clone_from_slice(world.staging_write.as_slice());
        }

        if !world.copy_from_prev.is_empty() {
            unsafe {
                cmd.copy_buffer(&vko::CopyBufferInfo {
                    src_buffer: self.prev_buffer,
                    dst_buffer: self.curr_buffer,
                    regions: world.copy_from_prev.as_slice(),
                    ..Default::default()
                })
            }?;
        }

        if !world.copy_from_prev_stage.is_empty() {
            unsafe {
                cmd.copy_buffer(&vko::CopyBufferInfo {
                    src_buffer: self.prev_stage_buffer,
                    dst_buffer: self.curr_buffer,
                    regions: world.copy_from_prev_stage.as_slice(),
                    ..Default::default()
                })
            }?;
        }

        if !world.copy_from_curr_stage.is_empty() {
            unsafe {
                cmd.copy_buffer(&vko::CopyBufferInfo {
                    src_buffer: self.curr_stage_buffer,
                    dst_buffer: self.curr_buffer,
                    regions: world.copy_from_curr_stage.as_slice(),
                    ..Default::default()
                })
            }?;
        }

        Ok(())
    }
}
