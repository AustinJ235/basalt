use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread::JoinHandle;

use cosmic_text::fontdb::Source as FontSource;
use flume::{Receiver, Sender};
use foldhash::{HashMap, HashMapExt, HashSet, HashSetExt};
use ordered_float::OrderedFloat;

use super::ImageSource;
use crate::interface::{Bin, BinID, DefaultFont, ItfVertInfo, UpdateContext};

enum Event {
    AddBinaryFont(Arc<dyn AsRef<[u8]> + Sync + Send>),
    SetDefaultFont(DefaultFont),
    SetExtent([u32; 2]),
    SetScale(f32),
    Perform,
}

pub struct UpdateSubmission {
    pub id: BinID,
    pub submission: BTreeMap<OrderedFloat<f32>, HashMap<ImageSource, Vec<ItfVertInfo>>>,
}

pub struct UpdateWorker {
    event_send: Sender<Event>,
    handle: Option<JoinHandle<Result<(), String>>>,
}

impl UpdateWorker {
    pub fn spawn(
        work_recv: Receiver<Arc<Bin>>,
        work_submit: Sender<UpdateSubmission>,
        mut context: UpdateContext,
    ) -> Self {
        let (event_send, event_recv) = flume::unbounded();

        let handle = std::thread::spawn(move || -> Result<(), String> {
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
                        Ok(Event::SetScale(scale)) => {
                            context.scale = scale;
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

                    let mut submission = BTreeMap::new();
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
                                    submission
                                        .entry(current_z)
                                        .or_insert_with(HashMap::new)
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
                            submission
                                .entry(current_z)
                                .or_insert_with(HashMap::new)
                                .entry(image_source.clone())
                                .or_insert_with(Vec::new)
                                .append(&mut current_vertexes);
                        }
                    }

                    if work_submit
                        .send(UpdateSubmission {
                            id,
                            submission,
                        })
                        .is_err()
                    {
                        break 'main;
                    }
                }

                context.placement_cache.clear();
            }

            Ok(())
        });

        Self {
            event_send,
            handle: Some(handle),
        }
    }

    pub fn add_binary_font(&self, bytes: Arc<dyn AsRef<[u8]> + Sync + Send>) -> bool {
        self.event_send.send(Event::AddBinaryFont(bytes)).is_ok()
    }

    pub fn set_default_font(&self, default_font: DefaultFont) -> bool {
        self.event_send
            .send(Event::SetDefaultFont(default_font))
            .is_ok()
    }

    pub fn set_extent(&self, extent: [u32; 2]) -> bool {
        self.event_send.send(Event::SetExtent(extent)).is_ok()
    }

    pub fn set_scale(&self, scale: f32) -> bool {
        self.event_send.send(Event::SetScale(scale)).is_ok()
    }

    pub fn perform(&self) -> bool {
        self.event_send.send(Event::Perform).is_ok()
    }

    pub fn obtain_error(mut self) -> Result<(), String> {
        match self.handle.take().unwrap().join() {
            Ok(ok) => ok,
            Err(_) => Err(String::from("panicked")),
        }
    }

    pub fn has_panicked(&self) -> bool {
        self.handle.as_ref().unwrap().is_finished()
    }
}
