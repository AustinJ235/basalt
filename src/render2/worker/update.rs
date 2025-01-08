use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread::JoinHandle;

use cosmic_text::fontdb::Source as FontSource;
use flume::{Receiver, Sender};
use foldhash::{HashMap, HashMapExt, HashSet, HashSetExt};
use ordered_float::OrderedFloat;

use super::{ImageSource, VertexState};
use crate::interface::{Bin, BinID, DefaultFont, UpdateContext};

enum Event {
    AddBinaryFont(Arc<dyn AsRef<[u8]> + Sync + Send>),
    SetDefaultFont(DefaultFont),
    SetExtent([u32; 2]),
    SetScale(f32),
    Perform,
}

pub struct UpdateSubmission {
    pub id: BinID,
    pub images: HashSet<ImageSource>,
    pub vertexes: BTreeMap<OrderedFloat<f32>, VertexState>,
}

pub struct UpdateWorker {
    event_send: Sender<Event>,
    handle: Option<JoinHandle<()>>,
}

impl UpdateWorker {
    pub fn spawn(
        work_recv: Receiver<Arc<Bin>>,
        work_submit: Sender<UpdateSubmission>,
        mut context: UpdateContext,
    ) -> Self {
        let (event_send, event_recv) = flume::unbounded();

        let handle = std::thread::spawn(move || {
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
                        Ok(Event::Perform) => {
                            break;
                        },
                        Err(_) => break 'main,
                    }
                }

                for bin in work_recv.try_iter() {
                    let id = bin.id();

                    // TODO: Metrics
                    let (vertex_data, _metrics) = bin.obtain_vertex_data(&mut context);

                    // TODO: Remove this mapping when the origial renderer is gone.
                    let vertex_data = vertex_data
                        .into_iter()
                        .map(|(image_source, vertexes)| {
                            (
                                match image_source {
                                    crate::render::ImageSource::None => ImageSource::None,
                                    crate::render::ImageSource::Cache(cache_key) => {
                                        ImageSource::Cache(cache_key)
                                    },
                                    crate::render::ImageSource::Vulkano(_) => ImageSource::None,
                                },
                                vertexes,
                            )
                        })
                        .collect::<HashMap<_, _>>();

                    let mut image_sources = HashSet::new();

                    for (image_source, _) in vertex_data.iter() {
                        if *image_source != ImageSource::None {
                            image_sources.insert(image_source.clone());
                        }
                    }

                    let mut vertex_states = BTreeMap::new();
                    let mut current_vertexes = Vec::new();
                    let mut current_z = OrderedFloat::<f32>::from(0.0);

                    for (image_source, vertexes) in vertex_data {
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
                                                data: HashMap::new(),
                                                total: 0,
                                            }
                                        });

                                    vertex_state.total += current_vertexes.len();

                                    vertex_state
                                        .data
                                        .entry(image_source.clone())
                                        .or_insert_with(Vec::new)
                                        .append(&mut current_vertexes);
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
                                        data: HashMap::new(),
                                        total: 0,
                                    }
                                });

                            vertex_state.total += current_vertexes.len();

                            vertex_state
                                .data
                                .entry(image_source.clone())
                                .or_insert_with(Vec::new)
                                .append(&mut current_vertexes);
                        }
                    }

                    if work_submit
                        .send(UpdateSubmission {
                            id,
                            images: image_sources,
                            vertexes: vertex_states,
                        })
                        .is_err()
                    {
                        break 'main;
                    }
                }

                context.placement_cache.clear();
            }
        });

        Self {
            event_send,
            handle: Some(handle),
        }
    }

    pub fn add_binary_font(&self, bytes: Arc<dyn AsRef<[u8]> + Sync + Send>) {
        self.event_send.send(Event::AddBinaryFont(bytes)).unwrap();
    }

    pub fn set_default_font(&self, default_font: DefaultFont) {
        self.event_send
            .send(Event::SetDefaultFont(default_font))
            .unwrap();
    }

    pub fn set_extent(&self, extent: [u32; 2]) {
        self.event_send.send(Event::SetExtent(extent)).unwrap();
    }

    pub fn set_scale(&self, scale: f32) {
        self.event_send.send(Event::SetScale(scale)).unwrap();
    }

    pub fn perform(&self) {
        self.event_send.send(Event::Perform).unwrap();
    }

    // TODO:
    #[allow(dead_code)]
    pub fn has_panicked(&self) -> bool {
        self.handle.as_ref().unwrap().is_finished()
    }
}
