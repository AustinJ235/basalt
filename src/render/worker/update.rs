use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use std::time::Instant;

use cosmic_text::fontdb::Source as FontSource;
use flume::{Receiver, Sender};
use ordered_float::OrderedFloat;

use crate::image::{ImageMap, ImageSet};
use crate::interface::{Bin, BinID, BinPostUpdate, DefaultFont, UpdateContext};
use crate::render::worker::{OVDPerfMetrics, VertexState};
use crate::render::{RendererMetricsLevel, WorkerError};

enum Event {
    AddBinaryFont(Arc<dyn AsRef<[u8]> + Sync + Send>),
    SetDefaultFont(DefaultFont),
    SetExtent([u32; 2]),
    SetScale(f32),
    SetMetricsLevel(RendererMetricsLevel),
    Perform,
    ClearCache,
}

pub struct UpdateSubmission {
    pub id: BinID,
    pub images: ImageSet,
    pub vertexes: BTreeMap<OrderedFloat<f32>, VertexState>,
    pub metrics_op: Option<OVDPerfMetrics>,
}

pub struct UpdateWorker {
    event_send: Sender<Event>,
}

impl UpdateWorker {
    pub fn spawn(
        work_recv: Receiver<Arc<Bin>>,
        work_submit: Sender<UpdateSubmission>,
        eoc_barrier: Arc<Barrier>,
        mut context: UpdateContext,
    ) -> Self {
        let (event_send, event_recv) = flume::unbounded();

        std::thread::spawn(move || {
            let mut call_eoc: Vec<(Arc<Bin>, BinPostUpdate)> = Vec::new();

            'main: loop {
                loop {
                    match event_recv.recv() {
                        Ok(Event::AddBinaryFont(bytes)) => {
                            context
                                .font_system
                                .db_mut()
                                .load_font_source(FontSource::Binary(bytes));
                        },
                        Ok(Event::SetDefaultFont(default_font)) => {
                            context.default_font = default_font;
                        },
                        Ok(Event::SetScale(scale)) => {
                            context.scale = scale;
                        },
                        Ok(Event::SetExtent(extent)) => {
                            context.extent = [extent[0] as f32, extent[1] as f32];
                        },
                        Ok(Event::SetMetricsLevel(metrics_level)) => {
                            context.metrics_level = metrics_level;
                        },
                        Ok(Event::Perform) => {
                            break;
                        },
                        Ok(Event::ClearCache) => {
                            context.placement_cache.clear();
                        },
                        Err(_) => break 'main,
                    }
                }

                for bin in work_recv.try_iter() {
                    let id = bin.id();
                    let (vertex_data, bpu, mut metrics_op) = bin.obtain_vertex_data(&mut context);
                    let process_start_op = metrics_op.is_some().then(Instant::now);

                    let image_keys = ImageSet::from_iter(
                        vertex_data
                            .keys()
                            .filter(|image_key| !image_key.is_invalid())
                            .cloned(),
                    );

                    let mut vertex_states = BTreeMap::new();
                    let mut current_vertexes = Vec::new();
                    let mut current_z = OrderedFloat::<f32>::from(0.0);

                    for (image_key, vertexes) in vertex_data {
                        let mut iter = vertexes.into_iter();

                        while let (Some(a), Some(b), Some(c)) =
                            (iter.next(), iter.next(), iter.next())
                        {
                            let z = OrderedFloat::<f32>::from(a.position[2]);

                            if z != current_z {
                                if !current_vertexes.is_empty() {
                                    let vertex_state =
                                        vertex_states.entry(current_z).or_insert_with(|| {
                                            VertexState {
                                                offset: [None, None],
                                                staging: [None, None],
                                                data: ImageMap::new(),
                                                total: 0,
                                            }
                                        });

                                    vertex_state.total += current_vertexes.len();
                                    vertex_state.data.modify(&image_key, Vec::new, |vertexes| {
                                        vertexes.append(&mut current_vertexes)
                                    });
                                }

                                current_z = z;
                            }

                            current_vertexes.push(a);
                            current_vertexes.push(b);
                            current_vertexes.push(c);
                        }

                        if !current_vertexes.is_empty() {
                            let vertex_state =
                                vertex_states.entry(current_z).or_insert_with(|| {
                                    VertexState {
                                        offset: [None, None],
                                        staging: [None, None],
                                        data: ImageMap::new(),
                                        total: 0,
                                    }
                                });

                            vertex_state.total += current_vertexes.len();
                            vertex_state.data.modify(&image_key, Vec::new, |vertexes| {
                                vertexes.append(&mut current_vertexes)
                            });
                        }
                    }

                    if let Some(process_start) = process_start_op {
                        let metrics = metrics_op.as_mut().unwrap();
                        metrics.worker_process =
                            process_start.elapsed().as_micros() as f32 / 1000.0;
                        metrics.total += metrics.worker_process;
                    }

                    if work_submit
                        .send(UpdateSubmission {
                            id,
                            images: image_keys,
                            vertexes: vertex_states,
                            metrics_op,
                        })
                        .is_err()
                    {
                        break 'main;
                    }

                    call_eoc.push((bin, bpu));
                }

                eoc_barrier.wait();

                for (bin, bpu) in call_eoc.drain(..) {
                    bin.call_end_of_cycle_hooks(bpu);
                }
            }
        });

        Self {
            event_send,
        }
    }

    pub fn add_binary_font(
        &self,
        bytes: Arc<dyn AsRef<[u8]> + Sync + Send>,
    ) -> Result<(), WorkerError> {
        self.event_send
            .send(Event::AddBinaryFont(bytes))
            .map_err(|_| WorkerError::OvdWorkerPanicked)
    }

    pub fn set_default_font(&self, default_font: DefaultFont) -> Result<(), WorkerError> {
        self.event_send
            .send(Event::SetDefaultFont(default_font))
            .map_err(|_| WorkerError::OvdWorkerPanicked)
    }

    pub fn set_extent(&self, extent: [u32; 2]) -> Result<(), WorkerError> {
        self.event_send
            .send(Event::SetExtent(extent))
            .map_err(|_| WorkerError::OvdWorkerPanicked)
    }

    pub fn set_scale(&self, scale: f32) -> Result<(), WorkerError> {
        self.event_send
            .send(Event::SetScale(scale))
            .map_err(|_| WorkerError::OvdWorkerPanicked)
    }

    pub fn set_metrics_level(
        &self,
        metrics_level: RendererMetricsLevel,
    ) -> Result<(), WorkerError> {
        self.event_send
            .send(Event::SetMetricsLevel(metrics_level))
            .map_err(|_| WorkerError::OvdWorkerPanicked)
    }

    pub fn perform(&self) -> Result<(), WorkerError> {
        self.event_send
            .send(Event::Perform)
            .map_err(|_| WorkerError::OvdWorkerPanicked)
    }

    pub fn clear_cache(&self) -> Result<(), WorkerError> {
        self.event_send
            .send(Event::ClearCache)
            .map_err(|_| WorkerError::OvdWorkerPanicked)
    }
}
