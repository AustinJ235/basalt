use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam::channel::{self, Receiver, Sender, TryRecvError, TrySendError};
use ordered_float::OrderedFloat;
use smallvec::smallvec;
use vulkano::buffer::subbuffer::Subbuffer;
use vulkano::buffer::sys::BufferCreateInfo;
use vulkano::buffer::{Buffer, BufferUsage};
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, BufferCopy, CommandBufferUsage, CopyBufferInfoTyped,
};
use vulkano::memory::allocator::{
    AllocationCreateInfo, GenericMemoryAllocator, MemoryAllocatePreference, MemoryUsage,
};
use vulkano::DeviceSize;

use super::STAT_WINDOW_SIZE;
use crate::atlas::AtlasView;
use crate::image_view::BstImageView;
use crate::interface::render::updater::Updater;
use crate::interface::render::ImageKey;
use crate::interface::{Bin, BinID, DefaultFont, ItfVertInfo};
use crate::vulkano::command_buffer::PrimaryCommandBufferAbstract;
use crate::vulkano::sync::GpuFuture;
use crate::window::BasaltWindow;

const VIEW_COUNT: usize = 3;
const INITIAL_BUFFER_LEN: DeviceSize = 16384; // 704 KiB

#[allow(dead_code)]
pub(crate) struct ComposerView {
    id: u64,
    index: usize,
    version: u64,
    images: Vec<Arc<BstImageView>>,
    buffer: Subbuffer<[ItfVertInfo]>,
    atlas_view: Arc<AtlasView>,
    event_send: Sender<ComposerEvent>,
    outdated: Arc<AtomicBool>,
    stats: ComposerStats,
}

#[derive(Clone)]
pub(crate) struct ComposerStats {
    pub composer_time: f64,
    pub updater_time: f64,
    pub update_rate: f64,
    pub send_rate: f64,
}

impl ComposerView {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn images(&self) -> Vec<Arc<BstImageView>> {
        self.images.clone()
    }

    pub(crate) fn image_count(&self) -> u32 {
        self.images.len() as u32
    }

    pub(crate) fn buffer(&self) -> Subbuffer<[ItfVertInfo]> {
        self.buffer.clone()
    }

    pub(crate) fn is_outdated(&self) -> bool {
        self.outdated.load(atomic::Ordering::SeqCst)
    }

    pub(crate) fn stats(&self) -> ComposerStats {
        self.stats.clone()
    }
}

impl Drop for ComposerView {
    fn drop(&mut self) {
        self.event_send
            .send(ComposerEvent::ReleaseView(self.index))
            .unwrap();
    }
}

#[derive(Clone)]
pub(crate) enum ComposerEvent {
    Scale(f32),
    Extent([u32; 2]),
    AddBins(Vec<Weak<Bin>>),
    DefaultFont(DefaultFont),
    ReleaseView(usize),
    Refresh,
}

pub(crate) struct Composer {
    view_recv: Receiver<ComposerView>,
    event_send: Sender<ComposerEvent>,
}

struct View {
    in_use: bool,
    version: u64,
    buffer: Subbuffer<[ItfVertInfo]>,
    atlas_view: Arc<AtlasView>,
    complete: Option<ComposerView>,
    outdated: Arc<AtomicBool>,
}

impl Composer {
    pub(crate) fn new(window: Arc<dyn BasaltWindow>) -> Option<Self> {
        let basalt = window.basalt();
        let (event_send, event_recv) = basalt.interface_ref().attach_composer(window.id())?;
        let (view_send, view_recv) = channel::bounded(1);
        let view_event_send = event_send.clone();

        thread::spawn(move || {
            let options = basalt.options();
            let queue = basalt.transfer_queue();
            let mem_alloc = GenericMemoryAllocator::new_default(queue.device().clone());
            let mut updater = Updater::new(options.clone());
            let mut views = Vec::with_capacity(VIEW_COUNT);
            let atlas_view = basalt.atlas_ref().view();

            let cmd_alloc = StandardCommandBufferAllocator::new(
                queue.device().clone(),
                StandardCommandBufferAllocatorCreateInfo {
                    primary_buffer_count: 8,
                    secondary_buffer_count: 1,
                    ..Default::default()
                },
            );

            for _ in 0..VIEW_COUNT {
                let buffer = Buffer::new_slice(
                    &mem_alloc,
                    BufferCreateInfo {
                        usage: BufferUsage::TRANSFER_DST
                            | BufferUsage::TRANSFER_SRC
                            | BufferUsage::VERTEX_BUFFER,
                        ..Default::default()
                    },
                    AllocationCreateInfo {
                        usage: MemoryUsage::DeviceOnly,
                        allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
                        ..Default::default()
                    },
                    INITIAL_BUFFER_LEN,
                )
                .unwrap();

                views.push(View {
                    in_use: false,
                    version: 0,
                    buffer,
                    atlas_view: atlas_view.clone(),
                    complete: None,
                    outdated: Arc::new(AtomicBool::new(true)),
                });
            }

            drop(atlas_view);

            let mut src_buffer: Subbuffer<[ItfVertInfo]> = Buffer::new_slice(
                &mem_alloc,
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_SRC,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    usage: MemoryUsage::Upload,
                    ..Default::default()
                },
                INITIAL_BUFFER_LEN,
            )
            .unwrap();

            let mut view_i = 0;
            let mut next_view_id = 0;
            let mut wait_for_event = false;
            let mut last_view_sent = Instant::now();
            let mut last_version = 0;
            let mut last_version_change = Instant::now();

            let mut updater_times = VecDeque::from_iter([Duration::from_secs(0)].into_iter());
            let mut composer_times = VecDeque::from_iter([Duration::from_secs(0)].into_iter());
            let mut version_times = VecDeque::from_iter([Duration::from_secs(0)].into_iter());
            let mut view_send_times = VecDeque::from_iter([Duration::from_secs(0)].into_iter());

            loop {
                loop {
                    let event = if wait_for_event {
                        match event_recv.recv() {
                            Ok(ok) => ok,
                            Err(_) => panic!("composer event channel disconnected"),
                        }
                    } else {
                        match event_recv.try_recv() {
                            Ok(ok) => ok,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                panic!("composer event channel disconnected")
                            },
                        }
                    };

                    match event {
                        ComposerEvent::Scale(scale) => updater.set_scale(scale),
                        ComposerEvent::Extent(extent) => updater.set_extent(extent),
                        ComposerEvent::AddBins(bins) => updater.track_bins(bins),
                        ComposerEvent::DefaultFont(font) => updater.set_default_font(font),
                        ComposerEvent::ReleaseView(index) => views[index].in_use = false,
                        ComposerEvent::Refresh => (),
                    }

                    wait_for_event = false;
                }

                if view_i >= VIEW_COUNT {
                    view_i = 0;
                }

                let updater_start = Instant::now();
                updater.perform();

                if updater.version() != last_version {
                    last_version = updater.version();
                    version_times.push_back(last_version_change.elapsed());
                    last_version_change = Instant::now();

                    if version_times.len() > STAT_WINDOW_SIZE {
                        version_times.pop_front();
                    }
                }

                updater_times.push_back(updater_start.elapsed());

                if updater_times.len() > STAT_WINDOW_SIZE {
                    updater_times.pop_front();
                }

                let composer_start = std::time::Instant::now();
                let mut update_indexes = Vec::new();

                for (i, view) in views.iter().enumerate() {
                    if view.version < updater.version()
                        || view.atlas_view.is_stale()
                        || (!view.in_use && view.complete.is_none())
                    {
                        if view.in_use {
                            view.outdated.store(true, atomic::Ordering::SeqCst);
                        } else {
                            update_indexes.push(i);
                        }
                    }
                }

                update_indexes.sort_by_key(|i| std::cmp::Reverse(views[*i].version));

                let update_i = match update_indexes.pop() {
                    Some(some) => some,
                    None => {
                        wait_for_event = true;

                        if view_send.is_empty() {
                            for (_, view) in views.iter_mut().enumerate() {
                                if !view.in_use {
                                    debug_assert!(view.version == updater.version());
                                    debug_assert!(!view.atlas_view.is_stale());
                                    debug_assert!(view.complete.is_some());

                                    view_send.send(view.complete.take().unwrap()).unwrap();
                                    view_send_times.push_back(last_view_sent.elapsed());
                                    last_view_sent = Instant::now();

                                    if view_send_times.len() > STAT_WINDOW_SIZE {
                                        view_send_times.pop_front();
                                    }

                                    view.in_use = true;
                                    break;
                                }
                            }
                        }

                        continue;
                    },
                };

                // TODO: rewrite below for partial updates

                let view = &mut views[update_i];

                if view.atlas_view.is_stale() {
                    view.atlas_view = basalt.atlas_ref().view();
                }

                let mut total_vertexes = 1;
                let mut image_keys = BTreeSet::new();

                for (_, vertex_data) in updater.all_vertex_data() {
                    for (image_key, vertexes) in vertex_data {
                        if *image_key != ImageKey::None {
                            image_keys.insert(image_key);
                        }

                        total_vertexes += vertexes.len() as DeviceSize;
                    }
                }

                let image_keys = BTreeMap::from_iter(
                    image_keys
                        .into_iter()
                        .enumerate()
                        .map(|(i, key)| (key, i as u32)),
                );

                let mut data_mapped: BTreeMap<
                    OrderedFloat<f32>,
                    BTreeMap<BinID, Vec<ItfVertInfo>>,
                > = BTreeMap::new();

                for (id, vertex_data) in updater.all_vertex_data() {
                    for (image_key, vertexes) in vertex_data {
                        let tex_i = image_keys.get(&image_key).copied().unwrap_or(0);

                        for chunk in vertexes.chunks_exact(3) {
                            data_mapped
                                .entry(OrderedFloat::from(chunk[0].position[2]))
                                .or_insert_with(BTreeMap::new)
                                .entry(id)
                                .or_insert_with(Vec::new)
                                .extend(chunk.iter().cloned().map(|mut vert| {
                                    vert.tex_i = tex_i;
                                    vert
                                }));
                        }
                    }
                }

                if src_buffer.len() < total_vertexes {
                    let mut new_len = src_buffer.len() * 2;

                    while new_len < total_vertexes {
                        new_len *= 2;
                    }

                    src_buffer = Buffer::new_slice(
                        &mem_alloc,
                        BufferCreateInfo {
                            usage: BufferUsage::TRANSFER_SRC,
                            ..Default::default()
                        },
                        AllocationCreateInfo {
                            usage: MemoryUsage::Upload,
                            ..Default::default()
                        },
                        new_len,
                    )
                    .unwrap();
                }

                {
                    let mut src_buffer_gu = src_buffer.write().unwrap();
                    let mut i = 0;

                    for bin_map in data_mapped.into_values().rev() {
                        for vertexes in bin_map.into_values() {
                            for vertex in vertexes {
                                src_buffer_gu[i] = vertex;
                                i += 1;
                            }
                        }
                    }
                }

                if view.buffer.len() < total_vertexes {
                    let mut new_len = view.buffer.len() * 2;

                    while new_len < total_vertexes {
                        new_len *= 2;
                    }

                    view.buffer = Buffer::new_slice(
                        &mem_alloc,
                        BufferCreateInfo {
                            usage: BufferUsage::TRANSFER_DST
                                | BufferUsage::TRANSFER_SRC
                                | BufferUsage::VERTEX_BUFFER,
                            ..Default::default()
                        },
                        AllocationCreateInfo {
                            usage: MemoryUsage::DeviceOnly,
                            allocate_preference: MemoryAllocatePreference::AlwaysAllocate,
                            ..Default::default()
                        },
                        new_len,
                    )
                    .unwrap();
                }

                let mut cmd_buf = AutoCommandBufferBuilder::primary(
                    &cmd_alloc,
                    queue.queue_family_index(),
                    CommandBufferUsage::OneTimeSubmit,
                )
                .unwrap();

                cmd_buf
                    .copy_buffer(CopyBufferInfoTyped {
                        regions: smallvec![BufferCopy {
                            size: total_vertexes,
                            ..BufferCopy::default()
                        }],
                        ..CopyBufferInfoTyped::buffers(src_buffer.clone(), view.buffer.clone())
                    })
                    .unwrap();

                let mut future = cmd_buf
                    .build()
                    .unwrap()
                    .execute(queue.clone())
                    .unwrap()
                    .then_signal_fence_and_flush()
                    .unwrap();

                future.wait(None).unwrap();
                future.cleanup_finished();

                let images = image_keys
                    .into_keys()
                    .filter_map(|key| {
                        match key {
                            ImageKey::None => None,
                            ImageKey::Atlas(i) => {
                                Some(
                                    view.atlas_view
                                        .image(*i)
                                        .unwrap_or(basalt.atlas_ref().empty_image()),
                                )
                            },
                            ImageKey::Direct(img) => Some(img.clone()),
                        }
                    })
                    .collect();

                let stats = ComposerStats {
                    composer_time: (composer_times.iter().sum::<Duration>()
                        / composer_times.len() as u32)
                        .as_micros() as f64
                        / 1000.0,
                    updater_time: (updater_times.iter().sum::<Duration>()
                        / updater_times.len() as u32)
                        .as_micros() as f64
                        / 1000.0,
                    update_rate: 1000.0
                        / ((view_send_times.iter().sum::<Duration>() / view_send_times.len() as u32)
                            .as_micros() as f64
                            / 1000.0),
                    send_rate: 1000.0
                        / ((version_times.iter().sum::<Duration>() / version_times.len() as u32)
                            .as_micros() as f64
                            / 1000.0),
                };

                let complete = ComposerView {
                    id: next_view_id,
                    index: update_i,
                    version: updater.version(),
                    images,
                    buffer: view.buffer.clone().slice(0..total_vertexes),
                    atlas_view: view.atlas_view.clone(),
                    event_send: view_event_send.clone(),
                    outdated: view.outdated.clone(),
                    stats,
                };

                next_view_id += 1;
                view.outdated.store(false, atomic::Ordering::SeqCst);
                view.version = complete.version;

                match view_send.try_send(complete) {
                    Ok(_) => {
                        view.in_use = true;

                        view_send_times.push_back(last_view_sent.elapsed());
                        last_view_sent = Instant::now();

                        if view_send_times.len() > STAT_WINDOW_SIZE {
                            view_send_times.pop_front();
                        }
                    },
                    Err(TrySendError::Full(complete)) => {
                        view.complete = Some(complete);
                    },
                    Err(TrySendError::Disconnected(_)) => return,
                }

                composer_times.push_back(composer_start.elapsed());

                if composer_times.len() > STAT_WINDOW_SIZE {
                    composer_times.pop_front();
                }
            }
        });

        Some(Self {
            view_recv,
            event_send,
        })
    }

    pub(crate) fn try_acquire_view(&self) -> Option<Arc<ComposerView>> {
        match self.view_recv.try_recv() {
            Ok(ok) => Some(Arc::new(ok)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!("composer thread has panicked"),
        }
    }

    pub(crate) fn acquire_view(&self) -> Arc<ComposerView> {
        match self.view_recv.recv() {
            Ok(ok) => Arc::new(ok),
            Err(_) => panic!("composer thread has panicked"),
        }
    }

    pub(crate) fn set_extent(&self, extent: [u32; 2]) {
        self.event_send.send(ComposerEvent::Extent(extent)).unwrap();
    }
}
