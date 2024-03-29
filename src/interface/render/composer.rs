use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{self, AtomicUsize};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

use cosmic_text::{fontdb, FontSystem, SwashCache};
use crossbeam::channel::unbounded;
use crossbeam::queue::SegQueue;
use crossbeam::sync::{Parker, Unparker};
use ordered_float::OrderedFloat;
use parking_lot::{Condvar, Mutex};
use vulkano::buffer::subbuffer::Subbuffer;
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, CopyBufferInfo, PrimaryCommandBufferAbstract,
};
use vulkano::device::{Device, Queue};
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryUsage, StandardMemoryAllocator};
use vulkano::sync::GpuFuture;
use vulkano::DeviceSize;

use crate::atlas::AtlasImageID;
use crate::image_view::BstImageView;
use crate::interface::bin::{Bin, BinID};
use crate::interface::{DefaultFont, ItfVertInfo};
use crate::{Atlas, BstOptions};

type ZIndex = OrderedFloat<f32>;
type BinVertexData = Vec<(Vec<ItfVertInfo>, Option<Arc<BstImageView>>, AtlasImageID)>;

const SQUARE_POSITIONS: [[f32; 2]; 6] = [
    [1.0, -1.0],
    [-1.0, -1.0],
    [-1.0, 1.0],
    [1.0, -1.0],
    [-1.0, 1.0],
    [1.0, 1.0],
];

pub(crate) struct Composer {
    view: Mutex<Option<Arc<ComposerView>>>,
    view_cond: Condvar,
    ev_queue: SegQueue<ComposerEv>,
    unparker: Unparker,
    bin_time: AtomicUsize,
}

pub(crate) enum ComposerEv {
    Scale(f32),
    Extent([u32; 2]),
    AddBin(Weak<Bin>),
    DefaultFont(DefaultFont),
}

pub(crate) struct ComposerView {
    pub inst: Instant,
    pub buffers: Vec<Subbuffer<[ItfVertInfo]>>,
    pub images: Vec<Arc<BstImageView>>,
}

impl ComposerView {
    fn atlas_views_stale(&self) -> bool {
        for img in self.images.iter() {
            if img.is_stale() {
                return true;
            }
        }

        false
    }
}

struct BinData {
    weak: Weak<Bin>,
    inst: Instant,
    scale: f32,
    extent: [u32; 2],
}

struct Layer {
    vertex: BTreeMap<BinID, Vec<VertexData>>,
    state_changed: bool,
}

#[derive(Clone)]
struct VertexData {
    img: VertexImage,
    data: Vec<ItfVertInfo>,
}

#[derive(Clone, PartialEq, Debug)]
enum VertexImage {
    None,
    Atlas(AtlasImageID),
    Custom(Arc<BstImageView>),
}

enum UpdateCtx {
    Extent([f32; 2]),
    Scale(f32),
    DefaultFont(DefaultFont),
    Ready,
}

struct BinUpdateOut {
    bin: Arc<Bin>,
    inst: Instant,
    data: BinVertexData,
}

pub(crate) struct ComposerInit {
    pub options: BstOptions,
    pub device: Arc<Device>,
    pub transfer_queue: Arc<Queue>,
    pub atlas: Arc<Atlas>,
    pub initial_scale: f32,
}

pub(crate) struct UpdateContext {
    pub extent: [f32; 2],
    pub scale: f32,
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub default_font: DefaultFont,
}

impl Composer {
    pub fn send_event(&self, ev: ComposerEv) {
        self.ev_queue.push(ev);
        self.unparker.unpark();
    }

    pub fn unpark(&self) {
        self.unparker.unpark();
    }

    pub fn bin_time(&self) -> usize {
        self.bin_time.load(atomic::Ordering::Relaxed)
    }

    pub fn new(mut init: ComposerInit) -> Arc<Self> {
        let parker = Parker::new();

        let composer_ret = Arc::new(Self {
            view: Mutex::new(None),
            view_cond: Condvar::new(),
            ev_queue: SegQueue::new(),
            unparker: parker.unparker().clone(),
            bin_time: AtomicUsize::new(0),
        });

        let (up_in_s, up_in_r) = unbounded::<Arc<Bin>>();
        let (up_out_s, up_out_r) = unbounded();
        let mut up_ctx_ss = Vec::new();
        let additional_fonts = init
            .options
            .additional_fonts
            .drain(..)
            .map(|font| fontdb::Source::Binary(font))
            .collect::<Vec<_>>();

        for _ in 0..init.options.bin_parallel_threads.get() {
            let up_in_r = up_in_r.clone();
            let up_out_s = up_out_s.clone();
            let (up_ctx_s, up_ctx_r) = unbounded::<UpdateCtx>();
            up_ctx_ss.push(up_ctx_s);
            let additional_fonts = additional_fonts.clone();

            thread::spawn(move || {
                let mut update_context = UpdateContext {
                    extent: [0.0; 2],
                    scale: 1.0,
                    font_system: FontSystem::new_with_fonts(additional_fonts.into_iter()),
                    swash_cache: SwashCache::new(),
                    default_font: DefaultFont::default(),
                };

                loop {
                    loop {
                        match up_ctx_r.recv() {
                            Ok(ctx) => {
                                match ctx {
                                    UpdateCtx::Ready => break,
                                    UpdateCtx::Extent(extent) => update_context.extent = extent,
                                    UpdateCtx::Scale(scale) => update_context.scale = scale,
                                    UpdateCtx::DefaultFont(default_font) => {
                                        update_context.default_font = default_font
                                    },
                                }
                            },
                            Err(_) => return,
                        }
                    }

                    for bin in up_in_r.try_iter() {
                        bin.do_update(&mut update_context);
                        let inst = bin.last_update();
                        let data = bin.verts_cp();

                        if up_out_s
                            .send(BinUpdateOut {
                                bin,
                                inst,
                                data,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            });
        }

        let composer = composer_ret.clone();

        thread::spawn(move || {
            let ComposerInit {
                options,
                device,
                transfer_queue,
                atlas,
                initial_scale,
            } = init;

            let mem_alloc = Arc::new(StandardMemoryAllocator::new_default(device.clone()));

            let cmd_alloc = StandardCommandBufferAllocator::new(
                device,
                StandardCommandBufferAllocatorCreateInfo {
                    primary_buffer_count: 8,
                    secondary_buffer_count: 1,
                    ..Default::default()
                },
            );

            let mut bins: BTreeMap<BinID, BinData> = BTreeMap::new();
            let mut layers: BTreeMap<ZIndex, Layer> = BTreeMap::new();
            let mut scale = initial_scale;
            let mut extent = options.window_size;

            for up_ctx_s in up_ctx_ss.iter() {
                up_ctx_s.send(UpdateCtx::Scale(scale)).unwrap();
                up_ctx_s
                    .send(UpdateCtx::Extent([extent[0] as f32, extent[1] as f32]))
                    .unwrap();
            }

            #[derive(PartialEq, Eq)]
            enum BinStatus {
                Exists,
                Create,
                Remove,
            }

            #[derive(PartialEq, Eq)]
            enum UpdateStatus {
                Current,
                Required,
            }

            let process_vertex_data =
                move |layers: &mut BTreeMap<ZIndex, Layer>,
                      bins: &mut BTreeMap<BinID, BinData>,
                      scale: f32,
                      extent: [u32; 2],
                      up_out: BinUpdateOut| {
                    let BinUpdateOut {
                        bin,
                        inst,
                        data,
                    } = up_out;

                    let id = bin.id();

                    for (vertexes, img_op, atlas_id) in data {
                        let vertex_img = match img_op {
                            Some(some) => VertexImage::Custom(some),
                            None => {
                                if atlas_id == 0 {
                                    VertexImage::None
                                } else {
                                    VertexImage::Atlas(atlas_id)
                                }
                            },
                        };

                        for vertex in vertexes {
                            let z_index = OrderedFloat::from(vertex.position[2]);

                            let layer_entry = layers.entry(z_index).or_insert_with(|| {
                                Layer {
                                    vertex: BTreeMap::new(),
                                    state_changed: true,
                                }
                            });

                            layer_entry.state_changed = true;

                            let vertex_entry =
                                layer_entry.vertex.entry(id).or_insert_with(Vec::new);
                            let mut vertex_entry_i_op = None;

                            for (i, entry_vertex_data) in vertex_entry.iter().enumerate() {
                                if entry_vertex_data.img == vertex_img {
                                    vertex_entry_i_op = Some(i);
                                    break;
                                }
                            }

                            let vertex_entry_i = match vertex_entry_i_op {
                                Some(some) => some,
                                None => {
                                    vertex_entry.push(VertexData {
                                        img: vertex_img.clone(),
                                        data: Vec::new(),
                                    });

                                    vertex_entry.len() - 1
                                },
                            };

                            vertex_entry[vertex_entry_i].data.push(vertex);
                        }
                    }

                    bins.insert(
                        id,
                        BinData {
                            weak: Arc::downgrade(&bin),
                            inst,
                            scale,
                            extent,
                        },
                    );
                };

            let mut bin_times: [u128; 10] = [0; 10];
            let mut bin_times_i = 0;

            loop {
                let bin_times_inst = Instant::now();
                let mut new_bins = Vec::new();
                let mut update_all = false;

                while let Some(ev) = composer.ev_queue.pop() {
                    match ev {
                        ComposerEv::Scale(new_scale) => {
                            scale = new_scale;

                            for up_ctx_s in up_ctx_ss.iter() {
                                let _ = up_ctx_s.send(UpdateCtx::Scale(new_scale));
                            }
                        },
                        ComposerEv::Extent(new_extent) => {
                            extent = new_extent;

                            for up_ctx_s in up_ctx_ss.iter() {
                                let _ = up_ctx_s.send(UpdateCtx::Extent([
                                    new_extent[0] as f32,
                                    new_extent[1] as f32,
                                ]));
                            }
                        },
                        ComposerEv::AddBin(bin_wk) => {
                            if let Some(bin) = bin_wk.upgrade() {
                                new_bins.push(bin);
                            }
                        },
                        ComposerEv::DefaultFont(default_font) => {
                            update_all = true;

                            for up_ctx_s in up_ctx_ss.iter() {
                                let _ = up_ctx_s.send(UpdateCtx::DefaultFont(default_font.clone()));
                            }
                        },
                    }
                }

                let mut bin_state: Vec<(BinID, BinStatus, UpdateStatus, Option<Arc<Bin>>)> =
                    Vec::with_capacity(bins.len() + new_bins.len());

                for (bin_id, bin_data) in bins.iter() {
                    match bin_data.weak.upgrade() {
                        Some(bin) => {
                            let (update_status, bin_op) = if update_all
                                || bin.wants_update()
                                || bin_data.extent != extent
                                || bin_data.scale != scale
                                || bin_data.inst < bin.last_update()
                            {
                                (UpdateStatus::Required, Some(bin))
                            } else {
                                (UpdateStatus::Current, None)
                            };

                            bin_state.push((*bin_id, BinStatus::Exists, update_status, bin_op));
                        },
                        None => {
                            bin_state.push((
                                *bin_id,
                                BinStatus::Remove,
                                UpdateStatus::Current,
                                None,
                            ));
                        },
                    }
                }

                for bin in new_bins {
                    let bin_id = bin.id();
                    let post_up = bin.post_update();

                    let update_status = if update_all
                        || bin.wants_update()
                        || post_up.extent != extent
                        || post_up.scale != scale
                    {
                        UpdateStatus::Required
                    } else {
                        UpdateStatus::Current
                    };

                    bin_state.push((bin_id, BinStatus::Create, update_status, Some(bin)));
                }

                let mut state_changed = false;
                let mut updates_in_prog = 0;

                for (id, status, update, bin_op) in bin_state {
                    if update == UpdateStatus::Required {
                        let bin = (*bin_op.as_ref().unwrap()).clone();

                        up_in_s
                            .send(bin)
                            .expect("All bin update threads have panicked!");

                        updates_in_prog += 1;
                    }

                    if (status == BinStatus::Exists && update == UpdateStatus::Required)
                        || status == BinStatus::Remove
                    {
                        bins.remove(&id).unwrap();

                        for layer in layers.values_mut() {
                            if layer.vertex.remove(&id).is_some() {
                                layer.state_changed = true;
                            }
                        }
                    }

                    if status == BinStatus::Create && update == UpdateStatus::Current {
                        let bin = bin_op.unwrap().clone();
                        let inst = bin.last_update();
                        let data = bin.verts_cp();

                        process_vertex_data(
                            &mut layers,
                            &mut bins,
                            scale,
                            extent,
                            BinUpdateOut {
                                bin,
                                inst,
                                data,
                            },
                        );
                    }
                }

                for up_ctx_s in up_ctx_ss.iter() {
                    let _ = up_ctx_s.send(UpdateCtx::Ready);
                }

                let mut update_received = 0;

                while update_received < updates_in_prog {
                    let out = up_out_r
                        .recv()
                        .expect("All bin update threads have panicked!");
                    process_vertex_data(&mut layers, &mut bins, scale, extent, out);
                    update_received += 1;
                }

                layers.retain(|_, layer| {
                    let empty = layer.vertex.is_empty();

                    if empty {
                        state_changed = true;
                    }

                    !empty
                });

                for layer in layers.values_mut() {
                    if layer.state_changed {
                        state_changed = true;
                        layer.state_changed = false;
                    }
                }

                if state_changed
                    || composer
                        .view
                        .lock()
                        .as_ref()
                        .map(|v| v.atlas_views_stale())
                        .unwrap_or(true)
                {
                    let mut cmd_buf = AutoCommandBufferBuilder::primary(
                        &cmd_alloc,
                        transfer_queue.queue_family_index(),
                        CommandBufferUsage::OneTimeSubmit,
                    )
                    .unwrap();

                    let mut vimages: Vec<VertexImage> = Vec::new();
                    let mut buffers = Vec::with_capacity(layers.len());

                    for (zindex, layer) in layers.iter_mut().rev() {
                        let len: usize = layer
                            .vertex
                            .values()
                            .map(|vd| vd.iter().map(|d| d.data.len()).sum::<usize>())
                            .sum::<usize>()
                            + 6;
                        let mut content: Vec<ItfVertInfo> = Vec::with_capacity(len);

                        for [x, y] in SQUARE_POSITIONS.iter() {
                            content.push(ItfVertInfo {
                                position: [*x, *y, **zindex],
                                coords: [0.0; 2],
                                color: [0.0; 4],
                                ty: -1,
                                tex_i: 0,
                            });
                        }

                        for vd in layer.vertex.values() {
                            for VertexData {
                                img,
                                data,
                            } in vd.iter()
                            {
                                let tex_i: u32 = if *img == VertexImage::None {
                                    0
                                } else {
                                    let mut vimages_iop = None;

                                    for (i, vi) in vimages.iter().enumerate() {
                                        if vi == img {
                                            vimages_iop = Some(i);
                                            break;
                                        }
                                    }

                                    let vimage_i = match vimages_iop {
                                        Some(some) => some,
                                        None => {
                                            vimages.push(img.clone());
                                            vimages.len() - 1
                                        },
                                    };

                                    vimage_i as u32
                                };

                                for mut v in data.iter().cloned() {
                                    v.tex_i = tex_i;
                                    content.push(v);
                                }
                            }
                        }

                        let src_buf: Subbuffer<[ItfVertInfo]> = Buffer::from_iter(
                            &*mem_alloc,
                            BufferCreateInfo {
                                usage: BufferUsage::TRANSFER_SRC,
                                ..Default::default()
                            },
                            AllocationCreateInfo {
                                usage: MemoryUsage::Upload,
                                ..Default::default()
                            },
                            content,
                        )
                        .unwrap();

                        let dst_buf: Subbuffer<[ItfVertInfo]> = Buffer::new_slice(
                            &*mem_alloc,
                            BufferCreateInfo {
                                usage: BufferUsage::TRANSFER_DST | BufferUsage::VERTEX_BUFFER,
                                ..Default::default()
                            },
                            AllocationCreateInfo {
                                usage: MemoryUsage::DeviceOnly,
                                ..Default::default()
                            },
                            len as DeviceSize,
                        )
                        .unwrap();

                        cmd_buf
                            .copy_buffer(CopyBufferInfo::buffers(src_buf, dst_buf.clone()))
                            .unwrap();

                        buffers.push(dst_buf);
                    }

                    let upload_future = cmd_buf
                        .build()
                        .unwrap()
                        .execute(transfer_queue.clone())
                        .unwrap()
                        .then_signal_fence_and_flush()
                        .unwrap();

                    let atlas_views = atlas
                        .image_views()
                        .map(|v| v.1)
                        .unwrap_or_else(|| Arc::new(HashMap::new()));

                    let empty_image = atlas.empty_image();
                    let mut images: Vec<Arc<BstImageView>> = vimages
                        .into_iter()
                        .map(|vi| {
                            match vi {
                                VertexImage::None => unreachable!(),
                                VertexImage::Atlas(atlas_i) => {
                                    match atlas_views.get(&atlas_i) {
                                        Some(some) => some.clone(),
                                        None => empty_image.clone(),
                                    }
                                },
                                VertexImage::Custom(img) => img,
                            }
                        })
                        .collect();

                    if images.is_empty() {
                        images.push(empty_image);
                    }

                    let view = ComposerView {
                        inst: Instant::now(),
                        buffers,
                        images,
                    };

                    upload_future.wait(None).unwrap();

                    *composer.view.lock() = Some(Arc::new(view));
                    composer.view_cond.notify_all();

                    bin_times[bin_times_i] = bin_times_inst.elapsed().as_micros();
                    bin_times_i += 1;

                    if bin_times_i >= 10 {
                        bin_times_i = 0;
                    }

                    composer.bin_time.store(
                        (1.0 / bin_times
                            .iter()
                            .map(|t| *t as f64 / 100000000.0)
                            .sum::<f64>()
                            / 10.0)
                            .ceil() as usize,
                        atomic::Ordering::Relaxed,
                    );
                }

                parker.park();
            }
        });

        composer_ret
    }

    pub fn update_view(
        &self,
        update_view_op: Option<Arc<ComposerView>>,
        wait: Option<Duration>,
    ) -> Arc<ComposerView> {
        let mut self_view_op = self.view.lock();

        while self_view_op.is_none() {
            self.view_cond.wait(&mut self_view_op);
        }

        let mut self_view = self_view_op.as_ref().cloned().unwrap();

        if update_view_op.is_none() {
            return self_view;
        }

        let update_view = update_view_op.unwrap();

        if update_view.inst < self_view.inst {
            return self_view;
        }

        if wait.is_none() {
            return update_view;
        }

        self.view_cond.wait_for(&mut self_view_op, wait.unwrap());
        self_view = self_view_op.as_ref().cloned().unwrap();

        if update_view.inst < self_view.inst {
            self_view
        } else {
            update_view
        }
    }
}
