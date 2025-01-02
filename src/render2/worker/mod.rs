mod update;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::{Arc, Barrier, Weak};
use std::time::{Duration, Instant};

use flume::{Receiver, Sender};
use foldhash::{HashMap, HashMapExt};
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
    pub use vulkano::image::Image;
    pub use vulkano::memory::allocator::{
        AllocationCreateInfo, DeviceLayout, MemoryAllocatePreference, MemoryTypeFilter,
    };
    pub use vulkano::memory::MemoryPropertyFlags;
    pub use vulkano::DeviceSize;
    pub use vulkano_taskgraph::command_buffer::{
        BufferCopy, CopyBufferInfo, RecordingCommandBuffer,
    };
    pub use vulkano_taskgraph::graph::{CompileInfo, ExecutableTaskGraph, TaskGraph};
    pub use vulkano_taskgraph::resource::{AccessType, Flight, HostAccessType, Resources};
    pub use vulkano_taskgraph::{
        execute, resource_map, Id, QueueFamilyType, Task, TaskContext, TaskResult,
    };
}

const VERTEX_SIZE: vk::DeviceSize = std::mem::size_of::<ItfVertInfo>() as vk::DeviceSize;
const INITIAL_BUFFER_LEN: vk::DeviceSize = 32768;

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
        pending_clears: [Vec<[u32; 4]>; 2],
        staging_buffers: [vk::Id<vk::Buffer>; 2],
    },
    Dedicated {
        source: ImageSource,
        uses: usize,
        allocation: vk::Id<vk::Image>,
    },
    User {
        source: ImageSource,
        uses: usize,
        uploaded: bool,
        allocation: vk::Id<vk::Image>,
    },
}

struct AtlasAllocationState {
    allocation: AtlasAllocation,
    uses: usize,
    uploaded: [bool; 2],
    staging: [Option<vk::DeviceSize>; 2],
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
    // atlas_clear_buffer: vk::Id<vk::Image>,

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

        let mut worker = Self {
            window,
            render_flt_id,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format,

            bin_state: BTreeMap::new(),
            image_backings: Vec::new(),
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

    // TODO:
    // fn set_metrics_level(&self, metrics_level: ());

    fn run(mut self) {
        let mut loop_i = 0_usize;

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
                    self.buffer_update[0] = true;
                    self.buffer_update[1] = true;
                } else {
                    for old_vertex_state in vertexes.into_values() {
                        for (buffer_i, offset_op) in old_vertex_state.offset.into_iter().enumerate()
                        {
                            self.buffer_update[buffer_i] = true;
                        }
                    }
                }
            }

            // TODO: Image sources

            if self.buffer_update[buffer_index(loop_i)[0]] {
                let src_buf_i = buffer_index(loop_i + 2);
                let src_buf_id = self.buffers[src_buf_i[0]][src_buf_i[1]];
                let dst_buf_i = buffer_index(loop_i);
                let mut dst_buf_id = self.buffers[dst_buf_i[0]][dst_buf_i[1]];
                let mut stage_buf_id = self.staging_buffers[src_buf_i[0]];
                let prev_stage_buf_i = buffer_index(loop_i + 1)[0];
                let prev_stage_buf_id = self.staging_buffers[prev_stage_buf_i];

                let mut count_by_z: BTreeMap<OrderedFloat<f32>, vk::DeviceSize> = BTreeMap::new();

                for state in self.bin_state.values() {
                    for (z, vertex_state) in state.vertexes.iter() {
                        *count_by_z.entry(*z).or_default() += vertex_state.total as vk::DeviceSize;
                    }
                }

                let mut total_count = count_by_z.values().sum::<vk::DeviceSize>();

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

            // TODO: DO WORK
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
