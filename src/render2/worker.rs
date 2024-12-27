use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use flume::{Receiver, Sender};
use foldhash::HashMap;
use guillotiere::{
    Allocation as AtlasAllocation, AllocatorOptions as AtlasAllocatorOptions, AtlasAllocator,
    Size as AtlasSize,
};
use ordered_float::OrderedFloat;

use super::RenderEvent;
use crate::image_cache::ImageCacheKey;
use crate::interface::{Bin, BinID, DefaultFont, ItfVertInfo};
use crate::window::{Window, WindowEvent};

mod vk {
    pub use vulkano::buffer::{BufferCreateInfo, BufferUsage};
    pub use vulkano::format::Format;
    pub use vulkano::image::Image;
    pub use vulkano::memory::allocator::{AllocationCreateInfo, DeviceLayout, MemoryTypeFilter};
    pub use vulkano::DeviceSize;
    pub use vulkano_taskgraph::resource::{Flight, HostAccessType, Resources};
    pub use vulkano_taskgraph::{execute, Id};
}

pub struct SpawnInfo {
    pub window: Arc<Window>,
    pub render_flt_id: vk::Id<vk::Flight>,
    pub worker_flt_id: vk::Id<vk::Flight>,
    pub window_event_recv: Receiver<WindowEvent>,
    pub render_event_send: Sender<RenderEvent>,
    pub image_format: vk::Format,
}

enum ImageSource {
    None,
    Cache(ImageCacheKey),
    Vulkano(vk::Id<vk::Image>),
}

struct BinState {
    images: Vec<ImageSource>,
    vertexes: Option<BTreeMap<OrderedFloat<f32>, VertexState>>,
    pending_removal: bool,
    pending_update: bool,
}

struct VertexState {
    range: Option<Range<vk::DeviceSize>>,
    data: HashMap<ImageSource, Vec<ItfVertInfo>>,
}

enum ImageBacking {
    Atlas {
        allocator: AtlasAllocator,
        allocations: HashMap<ImageSource, ImageAllocation<AtlasAllocation>>,
    },
    Dedicated {
        source: ImageSource,
        allocation: ImageAllocation<vk::Id<vk::Image>>,
    },
    User {
        source: ImageSource,
        allocation: ImageAllocation<vk::Id<vk::Image>>,
    },
}

enum ImageRemoval {
    Immediate,
    Delayed(Duration),
    Never,
}

struct ImageAllocation<T> {
    allocation: T,
    uses: usize,
    last_used: Instant,
    removal: ImageRemoval,
}

pub struct Worker {
    window: Arc<Window>,
    render_flt_id: vk::Id<vk::Flight>,
    worker_flt_id: vk::Id<vk::Flight>,
    window_event_recv: Receiver<WindowEvent>,
    render_event_send: Sender<RenderEvent>,
    image_format: vk::Format,

    bin_state: BTreeMap<BinID, BinState>,
    bin_id_to_wk: BTreeMap<BinID, Weak<Bin>>,
    image_backings: Vec<ImageBacking>,
    pending_work: bool,
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

        // TODO: Spawn Sub-workers

        let mut worker = Self {
            window,
            render_flt_id,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format,
            bin_state: BTreeMap::new(),
            bin_id_to_wk: BTreeMap::new(),
            image_backings: Vec::new(),
            pending_work: false,
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
                        images: Vec::new(),
                        vertexes: None,
                        pending_removal: false,
                        pending_update: true,
                    },
                );

                self.bin_id_to_wk.insert(bin.id(), Arc::downgrade(&bin));
                self.pending_work = true;
            },
        }
    }

    fn update_bin(&mut self, bin_id: BinID) {
        if let Some(state) = self.bin_state.get_mut(&bin_id) {
            state.pending_update = true;
        }
    }

    fn update_all(&mut self) {
        for state in self.bin_state.values_mut() {
            state.pending_update = true;
        }

        self.pending_work = true;
    }

    fn update_extent(&mut self, extent: [u32; 2]) {
        self.update_all();
        // TODO: Update Workers
    }

    fn update_scale(&mut self, scale: f32) {
        self.update_all();
        // TODO: Update Workers
    }

    fn add_binary_font(&self, bytes: Arc<dyn AsRef<[u8]> + Sync + Send>) {
        // TODO: Update all bins with glyph image sources?

        // TODO: Update Workers
    }

    fn set_default_font(&mut self, default_font: DefaultFont) {
        // TODO: Update only those with glyph image sources?
        self.update_all();

        // TODO: Update Workers
    }

    // TODO:
    // fn set_metrics_level(&self, metrics_level: ());

    fn run(mut self) {
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

                            self.update_extent([width, height]);
                        },
                        WindowEvent::ScaleChanged(scale) => {
                            self.update_scale(scale);
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

            // TODO: DO WORK
            self.pending_work = false;
        }
    }
}

pub fn spawn(spawn_info: SpawnInfo) -> Result<(), String> {
    std::thread::spawn(move || {
        let SpawnInfo {
            window,
            render_flt_id,
            worker_flt_id,
            window_event_recv,
            render_event_send,
            image_format,
        } = spawn_info;

        let buffer_id = window
            .basalt_ref()
            .device_resources_ref()
            .create_buffer(
                vk::BufferCreateInfo {
                    usage: vk::BufferUsage::VERTEX_BUFFER,
                    ..Default::default()
                },
                vk::AllocationCreateInfo {
                    memory_type_filter: vk::MemoryTypeFilter::PREFER_DEVICE
                        | vk::MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                vk::DeviceLayout::new_unsized::<[ItfVertInfo]>(9).unwrap(),
            )
            .unwrap();

        let vertexes = [
            ([1.0, -1.0, 0.2], [1.0; 4]),
            ([-1.0, -1.0, 0.2], [1.0; 4]),
            ([-1.0, 1.0, 0.2], [1.0; 4]),
            ([1.0, -1.0, 0.2], [1.0; 4]),
            ([-1.0, 1.0, 0.2], [1.0; 4]),
            ([1.0, 1.0, 0.2], [1.0; 4]),
            ([0.0, -0.8, 0.1], [0.0, 0.0, 1.0, 1.0]),
            ([-0.8, 0.8, 0.1], [0.0, 0.0, 1.0, 1.0]),
            ([0.8, 0.8, 0.1], [0.0, 0.0, 1.0, 1.0]),
        ]
        .into_iter()
        .map(|(position, color)| {
            ItfVertInfo {
                position,
                coords: [0.0; 2],
                color,
                ty: 0,
                tex_i: 0,
            }
        })
        .collect::<Vec<_>>();

        unsafe {
            vk::execute(
                window.basalt_ref().transfer_queue_ref(),
                window.basalt_ref().device_resources_ref(),
                worker_flt_id,
                |_, task| {
                    task.write_buffer::<[ItfVertInfo]>(buffer_id, ..)?
                        .clone_from_slice(&vertexes);
                    Ok(())
                },
                [(buffer_id, vk::HostAccessType::Write)],
                [],
                [],
            )
            .unwrap();
        }

        if render_event_send
            .send(RenderEvent::Update {
                buffer_id,
                image_ids: Vec::new(),
                draw_range: 0..9,
            })
            .is_err()
        {
            return;
        }

        'main: loop {
            let mut update_all = false;
            let mut update_bins: BTreeSet<Arc<Bin>> = BTreeSet::new();
            let mut remove_bins: BTreeSet<BinID> = BTreeSet::new();

            for window_event in window_event_recv.drain() {
                match window_event {
                    WindowEvent::Opened => (),
                    WindowEvent::Closed => break 'main,
                    WindowEvent::Resized {
                        width: _,
                        height: _,
                    } => {
                        if render_event_send.send(RenderEvent::CheckExtent).is_err() {
                            break 'main;
                        }
                    },
                    WindowEvent::ScaleChanged(_scale) => (),
                    WindowEvent::RedrawRequested => (),
                    WindowEvent::EnabledFullscreen => (),
                    WindowEvent::DisabledFullscreen => (),
                    WindowEvent::AssociateBin(_bin) => (),
                    WindowEvent::DissociateBin(_bin_id) => (),
                    WindowEvent::UpdateBin(_bin_id) => (),
                    WindowEvent::UpdateBinBatch(_bin_ids) => (),
                    WindowEvent::AddBinaryFont(_bytes) => (),
                    WindowEvent::SetDefaultFont(_default_font) => (),
                    WindowEvent::SetMSAA(msaa) => {
                        if render_event_send.send(RenderEvent::SetMSAA(msaa)).is_err() {
                            break 'main;
                        }
                    },
                    WindowEvent::SetVSync(vsync) => {
                        if render_event_send
                            .send(RenderEvent::SetVSync(vsync))
                            .is_err()
                        {
                            break 'main;
                        }
                    },
                    WindowEvent::SetMetrics(_metrics_level) => (),
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    Ok(())
}
